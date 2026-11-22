//! `aexlo dev [filter] [-p <package>]` - rerun a `#[aexlo_test::preview]` test on save.
//!
//! [`cmd`] hands `--bin` to [`crate::watch`].
//!
//! Built-in replacement for pairing `bacon` with `#[aexlo_test::preview]`: watches
//! the crate's sources and reruns `cargo test` on every change, and shows the
//! PNG a preview test wrote last in a window of its own (they land in
//! `target/aexlo-preview/`). Each run is a fresh process driving
//! the in-process `from_entry` render path - not a `dlopen`'d cdylib like
//! `aexlo dev --bin` (see `watch.rs`) - so `println!`, `dbg!`, and debugger
//! attachment all work normally against a single run.

use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use minifb::Key;
use notify::{RecursiveMode, Watcher};

use crate::args;
use crate::watch::is_relevant;

#[derive(clap::Args)]
pub struct DevArgs {
	/// Only the tests whose name contains this
	#[arg(conflicts_with = "bin")]
	filter: Option<String>,
	/// Preview this workspace member instead of the crate in the current
	/// directory
	#[arg(short, long, value_name = "NAME")]
	package: Option<String>,
	/// Skip the test harness: rebuild the crate's cdylib and load and render
	/// it directly on save (faster, but no println!/dbg!/debugger, and no
	/// filter)
	#[arg(long)]
	bin: bool,
}

pub fn cmd(args: DevArgs) -> Result<()> {
	let manifest = args::cargo_package(args.package.as_deref())?;
	if args.bin {
		crate::watch::run(&manifest)
	} else {
		run(&manifest, args.filter.as_deref())
	}
}

/// Debounce window so a burst of editor save events triggers one rerun.
const DEBOUNCE: Duration = Duration::from_millis(150);
/// Debounce so a preview PNG isn't read back half-written.
const FRAME_DEBOUNCE: Duration = Duration::from_millis(80);

/// Rerun the crate's tests on every save, and show the PNG a
/// `#[aexlo_test::preview]` wrote last in a window.
pub fn run(manifest: &Path, filter: Option<&str>) -> Result<()> {
	let crate_dir = manifest.parent().context("manifest path has no parent directory")?;
	let src_dir = crate_dir.join("src");
	// Where `#[aexlo_test::preview]` writes (`aexlo_harness::preview::preview_path`).
	let frames = crate_dir.join("target").join("aexlo-preview");
	std::fs::create_dir_all(&frames).with_context(|| format!("creating {}", frames.display()))?;

	let mut window = crate::watch::open_window("aexlo dev - waiting for a preview…")?;
	let (init_w, init_h) = crate::watch::WINDOW_SIZE;

	let (tx, rx) = mpsc::channel();
	let mut watcher = notify::recommended_watcher(move |res| {
		let _ = tx.send(res);
	})
	.context("creating file watcher")?;
	watcher
		.watch(&src_dir, RecursiveMode::Recursive)
		.with_context(|| format!("watching {}", src_dir.display()))?;
	let _ = watcher.watch(manifest, RecursiveMode::NonRecursive);
	watcher
		.watch(&frames, RecursiveMode::NonRecursive)
		.with_context(|| format!("watching {}", frames.display()))?;

	let mut current: Option<Child> = None;
	let mut pending: Option<Instant> = Some(Instant::now()); // run once on startup
	// The PNG written last, and when, so it is read once the write settles.
	let mut frame: Option<(PathBuf, Instant)> = None;
	let mut framebuf = vec![0u32; init_w * init_h];
	let mut frame_dims = (init_w, init_h);

	println!(
		"aexlo dev: watching {} (Esc or close the window to quit)",
		src_dir.display()
	);

	while window.is_open() && !window.is_key_down(Key::Escape) {
		while let Ok(res) = rx.try_recv() {
			let Ok(event) = res else { continue };
			for path in &event.paths {
				if is_preview(path) {
					frame = Some((path.clone(), Instant::now()));
				} else if is_relevant(path) {
					pending = Some(Instant::now());
				}
			}
		}

		// Let an in-flight run finish rather than killing it: cargo already
		// serializes on the target-dir lock, so racing a second `cargo test`
		// in would just block on that lock anyway.
		let running = current
			.as_mut()
			.is_some_and(|child| matches!(child.try_wait(), Ok(None)));
		if let Some(since) = pending
			&& since.elapsed() >= DEBOUNCE
			&& !running
		{
			pending = None;
			current = Some(spawn_test(manifest, filter)?);
		}

		if let Some((path, since)) = &frame
			&& since.elapsed() >= FRAME_DEBOUNCE
		{
			match load_png(path) {
				Ok((buf, w, h)) => {
					framebuf = buf;
					frame_dims = (w, h);
					let name = path.file_name().unwrap_or_default().to_string_lossy();
					window.set_title(&format!("aexlo dev - {name} - {w}×{h}"));
				}
				// Keep the last good frame; a rewritten file fires again.
				Err(err) => eprintln!("aexlo dev: {err:#}"),
			}
			frame = None;
		}

		window
			.update_with_buffer(&framebuf, frame_dims.0, frame_dims.1)
			.map_err(|e| anyhow::anyhow!("updating preview window: {e}"))?;
		std::thread::sleep(Duration::from_millis(16));
	}

	if let Some(mut child) = current {
		let _ = child.kill();
		let _ = child.wait();
	}
	Ok(())
}

/// Run `cargo test` for `manifest`, filtered to `filter` if given. Each
/// `#[aexlo_test::preview]` it runs writes a PNG the window then shows.
///
/// `#[aexlo_test::preview]` generates an `#[ignore]`d test (so a plain `cargo test`
/// never pays the render cost), so this passes `--ignored` to actually run it.
fn spawn_test(manifest: &Path, filter: Option<&str>) -> Result<Child> {
	let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
	let mut cmd = Command::new(cargo);
	cmd.args(["test", "--manifest-path"]).arg(manifest);
	if let Some(filter) = filter {
		cmd.arg(filter);
	}
	cmd.args(["--", "--ignored", "--nocapture"]);
	// The window shows the frames; don't also open each in the OS viewer.
	cmd.env_remove("AEXLO_PREVIEW");
	cmd.spawn().context("spawning cargo test")
}

/// A PNG `#[aexlo_test::preview]` wrote.
fn is_preview(path: &Path) -> bool {
	path.extension().is_some_and(|e| e == "png")
		&& path
			.parent()
			.is_some_and(|dir| dir.ends_with(Path::new("target").join("aexlo-preview")))
}

/// Decode a PNG into minifb's `0x00RRGGBB` buffer plus its dimensions.
fn load_png(path: &Path) -> Result<(Vec<u32>, usize, usize)> {
	let img = image::open(path)
		.with_context(|| format!("opening {}", path.display()))?
		.to_rgba8();
	let (w, h) = img.dimensions();
	Ok((crate::watch::rgba_to_argb(img.as_raw()), w as usize, h as usize))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn only_finished_preview_pngs_are_shown() {
		let dir = Path::new("crate").join("target").join("aexlo-preview");
		assert!(is_preview(&dir.join("noise_preview.png")));
		assert!(!is_preview(&dir.join(".noise_preview.png.partial")));
		assert!(!is_preview(&Path::new("crate").join("out.png")));
	}
}
