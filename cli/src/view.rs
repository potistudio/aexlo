//! `aexlo view <png>` — a persistent live image window.
//!
//! Opens a [`minifb`] window showing a PNG and reloads it whenever the file
//! changes. It is decoupled from any plugin build, so it survives recompiles:
//! pair it with a re-runner (e.g. `bacon`) driving an in-process
//! `#[aexlo::preview]`, which overwrites the PNG, to get a live preview that
//! keeps the in-process (debuggable) render path.

use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use minifb::{Key, ScaleMode, Window, WindowOptions};
use notify::{RecursiveMode, Watcher};

/// Debounce so a non-atomic PNG write isn't read back half-finished.
const DEBOUNCE: Duration = Duration::from_millis(80);

pub fn run(path: &Path) -> Result<()> {
	// Single window per PNG: if a live viewer already owns this file, another
	// spawn (e.g. from `#[aexlo::preview]` with AEXLO_PREVIEW=live) just exits and
	// lets the running window pick up the change. `_lock` releases on return.
	let Some(_lock) = aexlo::acquire_viewer_lock(path) else {
		println!("aexlo view: already watching {} — leaving it to the running window", path.display());
		return Ok(());
	};

	let (init_w, init_h) = (1280usize, 720usize);
	let mut window = Window::new(
		&format!("aexlo view — {}", path.display()),
		init_w,
		init_h,
		// Float above the editor so the live preview stays visible while you work.
		WindowOptions { resize: true, topmost: true, scale_mode: ScaleMode::AspectRatioStretch, ..Default::default() },
	)
	.map_err(|e| anyhow!("opening view window: {e}"))?;

	// Watch the parent directory, not the file inode: editors (and `image`'s own
	// write) often replace the file, after which inode-level watches go silent.
	let (tx, rx) = mpsc::channel();
	let mut watcher =
		notify::recommended_watcher(move |res| {
			let _ = tx.send(res);
		})
		.context("creating file watcher")?;
	let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
	watcher.watch(dir, RecursiveMode::NonRecursive).with_context(|| format!("watching {}", dir.display()))?;

	let mut framebuf = vec![0u32; init_w * init_h];
	let mut frame_dims = (init_w, init_h);
	let mut pending: Option<Instant> = Some(Instant::now()); // load once on startup

	println!("aexlo view: watching {} (Esc or close window to quit)", path.display());

	while window.is_open() && !window.is_key_down(Key::Escape) {
		// Reload only when *our* file changes (match by name: event paths may be
		// absolute or a rename's temp sibling).
		while let Ok(res) = rx.try_recv() {
			if let Ok(event) = res
				&& event.paths.iter().any(|p| p.file_name() == path.file_name())
			{
				pending = Some(Instant::now());
			}
		}

		if let Some(since) = pending
			&& since.elapsed() >= DEBOUNCE
		{
			pending = None;
			match load_png(path) {
				Ok((buf, w, h)) => {
					framebuf = buf;
					frame_dims = (w, h);
					window.set_title(&format!("aexlo view — {w}×{h} — {}", path.display()));
				}
				// Keep the last good frame; a partial/rewritten file will fire again.
				Err(err) => eprintln!("aexlo view: {err:#}"),
			}
		}

		window
			.update_with_buffer(&framebuf, frame_dims.0, frame_dims.1)
			.map_err(|e| anyhow!("updating view window: {e}"))?;
		std::thread::sleep(Duration::from_millis(16));
	}

	Ok(())
}

/// Decode a PNG into minifb's `0x00RRGGBB` buffer plus its dimensions.
fn load_png(path: &Path) -> Result<(Vec<u32>, usize, usize)> {
	let img = image::open(path).with_context(|| format!("opening {}", path.display()))?.to_rgba8();
	let (w, h) = img.dimensions();
	let buf = img
		.chunks_exact(4)
		.map(|px| (px[0] as u32) << 16 | (px[1] as u32) << 8 | px[2] as u32)
		.collect();
	Ok((buf, w as usize, h as usize))
}
