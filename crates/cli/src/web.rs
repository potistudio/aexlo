//! `aexlo dev --bin --web` - live, interactive preview in the browser.
//!
//! Same build-on-save render loop as [`crate::watch`], but instead of blitting
//! into a minifb window it drives the shared [`crate::viewer`]: the latest frame
//! streams into a `<canvas>` and the plugin's parameters become live HTML
//! controls. This module owns the "compiler in the loop" half; the viewer owns
//! the serving/interactivity half that `aexlo preview` reuses without a build.
//!
//! Threading: the main thread owns the single [`aexlo::PluginInstance`] and its
//! watch/build/render loop; the viewer's background thread runs the HTTP server.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Instant;

use anyhow::{Context, Result};
use notify::{RecursiveMode, Watcher};

use crate::session::Session;
use crate::viewer;
use crate::watch::{DEBOUNCE, build_cdylib, is_relevant};

/// `dev --bin --web`'s options beyond the crate.
pub struct Options<'a> {
	pub port: u16,
	pub manifest: Option<&'a Path>,
	pub preset: Option<&'a str>,
	pub strict: bool,
	pub no_open: bool,
}

pub fn run(manifest: &Path, options: Options<'_>) -> Result<()> {
	let crate_dir = manifest.parent().context("manifest path has no parent directory")?;
	let src_dir = crate_dir.join("src");

	let presets = crate::session::viewer_manifest(options.manifest, options.preset.is_some())?;
	let mut session = Session::new(presets, options.preset, options.strict)?;
	let viewer = viewer::start(options.port)?;

	// File watcher: forward raw events; the main loop debounces them.
	let (tx, rx) = mpsc::channel();
	let mut watcher = notify::recommended_watcher(move |res| {
		let _ = tx.send(res);
	})
	.context("creating file watcher")?;
	watcher
		.watch(&src_dir, RecursiveMode::Recursive)
		.with_context(|| format!("watching {}", src_dir.display()))?;
	let _ = watcher.watch(manifest, RecursiveMode::NonRecursive);

	println!("aexlo dev --bin --web: serving {} (Ctrl+C to quit)", viewer.url);
	println!("aexlo dev --bin --web: watching {}", src_dir.display());
	if !options.no_open {
		viewer::open_browser(&viewer.url);
	}

	// The staged copy of the last good build the session loads instances
	// from; replaced on each build.
	let mut staged: Option<PathBuf> = None;

	let mut attempt: u64 = 0;
	let mut pending: Option<Instant> = Some(Instant::now()); // build once on startup

	loop {
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
			attempt += 1;
			viewer.begin_attempt(attempt);

			match build_and_stage(manifest, attempt) {
				Ok(copy) => {
					let path = copy.clone();
					session.set_loader(Box::new(move || {
						aexlo::Host::get().try_load(&path).context("loading plugin")
					}));
					match session.reload(&viewer) {
						Ok(()) => {
							println!("aexlo dev --bin --web: build #{attempt} rendered");
							// Drop the previous build's staged copy now that the
							// new one is live.
							if let Some(old) = staged.replace(copy) {
								let _ = std::fs::remove_file(old);
							}
						}
						Err(err) => {
							viewer.fail_attempt(attempt);
							eprintln!("\n─── render failed ───\n{err:#}\n");
						}
					}
				}
				Err(err) => {
					viewer.fail_attempt(attempt);
					eprintln!("\n─── build failed ───\n{err:#}\n");
				}
			}
		}

		// Parameter edits, preset picks, time, comparisons, saves.
		session.step(&viewer);

		std::thread::sleep(std::time::Duration::from_millis(50));
	}
}

/// Build the cdylib and copy it to a unique path (reopening the same path
/// can hand back a stale, still-mapped image).
fn build_and_stage(manifest: &Path, attempt: u64) -> Result<PathBuf> {
	let artifact = build_cdylib(manifest)?;
	let ext = artifact.extension().and_then(|s| s.to_str()).unwrap_or("dylib");
	let copy = std::env::temp_dir().join(format!("aexlo-web-{}-{attempt}.{ext}", std::process::id()));
	std::fs::copy(&artifact, &copy).with_context(|| format!("staging {}", artifact.display()))?;
	Ok(copy)
}
