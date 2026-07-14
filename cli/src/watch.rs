//! `aexlo dev --bin [-p <package>]` — a live preview window.
//!
//! Watches a plugin crate's sources; on every save it rebuilds the cdylib,
//! `try_load`s the fresh artifact, renders a frame, and blits it into a single
//! persistent [`minifb`] window. Because the window host must outlive each
//! recompile, this uses the build+`dlopen` path (not the in-process
//! `from_entry`, whose host *is* the thing being recompiled).

use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use aexlo::PluginInstance;
use anyhow::{Context, Result, anyhow, bail};
use minifb::{Key, ScaleMode, Window, WindowOptions};
use notify::{RecursiveMode, Watcher};

/// Debounce window so a burst of editor save events triggers one rebuild.
pub(crate) const DEBOUNCE: Duration = Duration::from_millis(150);

pub fn run(manifest: &Path) -> Result<()> {
	let crate_dir = manifest.parent().context("manifest path has no parent directory")?;
	let src_dir = crate_dir.join("src");

	let (init_w, init_h) = (1280usize, 720usize);
	let mut window = Window::new(
		"aexlo dev --bin — building…",
		init_w,
		init_h,
		// Float above the editor so the live preview stays visible while you work.
		WindowOptions {
			resize: true,
			topmost: true,
			scale_mode: ScaleMode::AspectRatioStretch,
			..Default::default()
		},
	)
	.map_err(|e| anyhow!("opening preview window: {e}"))?;

	// File watcher: forward raw events over a channel; the main loop debounces them.
	let (tx, rx) = mpsc::channel();
	let mut watcher = notify::recommended_watcher(move |res| {
		let _ = tx.send(res);
	})
	.context("creating file watcher")?;
	watcher
		.watch(&src_dir, RecursiveMode::Recursive)
		.with_context(|| format!("watching {}", src_dir.display()))?;
	let _ = watcher.watch(manifest, RecursiveMode::NonRecursive);

	// Last good frame, kept on screen across failed builds.
	let mut framebuf: Vec<u32> = vec![0; init_w * init_h];
	let mut frame_dims = (init_w, init_h);
	let mut generation: u64 = 0;
	let mut pending: Option<Instant> = Some(Instant::now()); // build once on startup

	println!(
		"aexlo dev --bin: watching {} (Esc or close window to quit)",
		src_dir.display()
	);

	while window.is_open() && !window.is_key_down(Key::Escape) {
		// Collapse any file-change events into a single pending rebuild.
		while let Ok(res) = rx.try_recv() {
			if let Ok(event) = res
				&& event.paths.iter().any(|p| is_relevant(p))
			{
				pending = Some(Instant::now());
			}
		}

		if let Some(since) = pending
			&& since.elapsed() >= DEBOUNCE
		{
			pending = None;
			generation += 1;
			window.set_title("aexlo dev --bin — building…");

			match build_and_render(manifest, generation) {
				Ok((rgba, w, h)) => {
					framebuf = rgba_to_argb(&rgba);
					frame_dims = (w as usize, h as usize);
					window.set_title(&format!("aexlo dev --bin — {w}×{h} — build #{generation}"));
					println!("aexlo dev --bin: build #{generation} → rendered {w}×{h}");
				}
				Err(err) => {
					window.set_title("aexlo dev --bin — build failed (see terminal)");
					eprintln!("\n─── build/render failed ───\n{err:#}\n");
				}
			}
		}

		window
			.update_with_buffer(&framebuf, frame_dims.0, frame_dims.1)
			.map_err(|e| anyhow!("updating preview window: {e}"))?;
		std::thread::sleep(Duration::from_millis(16));
	}

	Ok(())
}

/// Build the crate's cdylib, load it, render one frame, and hand back RGBA8.
///
/// One-shot: the instance is dropped and its staged copy removed before
/// returning. Callers that need to keep the instance alive (to re-render on a
/// parameter change without rebuilding) use [`build_and_load`] +
/// [`render_instance`] instead.
pub(crate) fn build_and_render(manifest: &Path, generation: u64) -> Result<(Vec<u8>, u32, u32)> {
	let (mut fx, staged) = build_and_load(manifest, generation)?;
	let result = render_instance(&mut fx);
	drop(fx);
	let _ = std::fs::remove_file(&staged);
	result
}

/// Build the crate's cdylib and load it, returning the live instance plus the
/// path of the staged copy it was loaded from (so the caller can remove that
/// copy once it drops the instance).
pub(crate) fn build_and_load(manifest: &Path, generation: u64) -> Result<(PluginInstance, PathBuf)> {
	let artifact = build_cdylib(manifest)?;

	// Load a uniquely named copy each time: reopening the same path can hand back
	// a stale, still-mapped image instead of the freshly built one.
	let ext = artifact.extension().and_then(|s| s.to_str()).unwrap_or("dylib");
	let staged = std::env::temp_dir().join(format!("aexlo-watch-{generation}.{ext}"));
	std::fs::copy(&artifact, &staged).with_context(|| format!("staging {}", artifact.display()))?;

	let fx = PluginInstance::try_load(&staged).context("loading freshly built plugin")?;
	Ok((fx, staged))
}

/// Render the loaded instance's current parameters to an RGBA8 frame.
pub(crate) fn render_instance(fx: &mut PluginInstance) -> Result<(Vec<u8>, u32, u32)> {
	fx.render_frame().context("render failed")?;
	let (w, h) = fx.output_size();
	let mut rgba = vec![0u8; w as usize * h as usize * 4];
	fx.write_output_rgba(&mut rgba).context("reading rendered output")?;
	Ok((rgba, w, h))
}

/// Run `cargo build` for `manifest` and return the path to its cdylib artifact.
pub(crate) fn build_cdylib(manifest: &Path) -> Result<PathBuf> {
	use std::process::{Command, Stdio};

	let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
	let mut child = Command::new(cargo)
		.args(["build", "--message-format=json-render-diagnostics", "--manifest-path"])
		.arg(manifest)
		.stdout(Stdio::piped())
		.stderr(Stdio::inherit())
		.spawn()
		.context("spawning cargo build")?;

	let reader = BufReader::new(child.stdout.take().expect("piped stdout"));
	let mut cdylib: Option<PathBuf> = None;
	for message in cargo_metadata::Message::parse_stream(reader) {
		if let cargo_metadata::Message::CompilerArtifact(artifact) = message.context("parsing cargo output")?
			&& artifact
				.target
				.kind
				.iter()
				.any(|k| *k == cargo_metadata::TargetKind::CDyLib)
		{
			for file in artifact.filenames {
				let path = file.into_std_path_buf();
				if is_dynamic_library(&path) {
					cdylib = Some(path);
				}
			}
		}
	}

	if !child.wait().context("waiting for cargo build")?.success() {
		bail!("cargo build failed");
	}
	cdylib.ok_or_else(|| anyhow!("build produced no cdylib (is the crate `crate-type = [\"cdylib\"]`?)"))
}

fn is_dynamic_library(path: &Path) -> bool {
	matches!(path.extension().and_then(|s| s.to_str()), Some("dylib" | "so" | "dll"))
}

/// Only rebuild for source-ish changes, ignoring editor swap/lock files.
pub(crate) fn is_relevant(path: &Path) -> bool {
	matches!(path.extension().and_then(|s| s.to_str()), Some("rs"))
		|| path.file_name().is_some_and(|n| n == "Cargo.toml")
}

/// Pack RGBA8 into minifb's `0x00RRGGBB` buffer (alpha ignored).
fn rgba_to_argb(rgba: &[u8]) -> Vec<u32> {
	rgba.chunks_exact(4)
		.map(|px| (px[0] as u32) << 16 | (px[1] as u32) << 8 | px[2] as u32)
		.collect()
}
