//! `aexlo dev --bin --web` — live, interactive preview in the browser.
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

use aexlo::PluginInstance;
use anyhow::{Context, Result};
use notify::{RecursiveMode, Watcher};

use crate::viewer;
use crate::watch::{DEBOUNCE, build_and_load, is_relevant, render_instance};

pub fn run(manifest: &Path, port: u16) -> Result<()> {
	let crate_dir = manifest.parent().context("manifest path has no parent directory")?;
	let src_dir = crate_dir.join("src");

	let viewer = viewer::start(port)?;

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
	viewer::open_browser(&viewer.url);

	// The live instance and the staged cdylib it was loaded from. Kept alive
	// across param edits so re-rendering doesn't rebuild; replaced on each build.
	let mut instance: Option<PluginInstance> = None;
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

			match build_and_load(manifest, attempt) {
				Ok((mut fx, new_staged)) => match render_instance(&mut fx) {
					Ok((rgba, w, h)) => {
						viewer.publish_reload(&fx, rgba, w, h);
						println!("aexlo dev --bin --web: build #{attempt} → rendered {w}×{h}");
						instance = Some(fx);
						// Drop the previous instance's staged copy now that the new
						// one is live.
						if let Some(old) = staged.replace(new_staged) {
							let _ = std::fs::remove_file(old);
						}
					}
					Err(err) => {
						viewer.fail_attempt(attempt);
						eprintln!("\n─── render failed ───\n{err:#}\n");
						let _ = std::fs::remove_file(new_staged);
					}
				},
				Err(err) => {
					viewer.fail_attempt(attempt);
					eprintln!("\n─── build failed ───\n{err:#}\n");
				}
			}
		}

		// Apply any parameter edits, then re-render once for the whole batch.
		if let Some(fx) = &mut instance
			&& viewer.apply_edits(fx)
		{
			let _ = fx.update_params_ui();
			match render_instance(fx) {
				Ok((rgba, w, h)) => viewer.publish_frame(fx, rgba, w, h),
				Err(err) => eprintln!("aexlo dev --bin --web: re-render failed: {err:#}"),
			}
		}

		std::thread::sleep(std::time::Duration::from_millis(50));
	}
}
