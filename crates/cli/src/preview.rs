//! `aexlo preview <plugin>` — interactively preview a *built* plugin.
//!
//! Loads a finished `.plugin`/`.aex`/`.dll` (no compiler in the loop) and serves
//! the same interactive surface as `aexlo dev --bin --web` via [`crate::viewer`]:
//! drag the plugin's parameters and watch the frame re-render. With `--watch` it
//! also reloads the artifact whenever the file changes on disk — e.g. rebuilt by
//! another toolchain or by After Effects' own build. This is the `vite preview`
//! to `dev`'s `vite dev`: same viewer, output instead of source.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use aexlo::PluginInstance;
use anyhow::{Context, Result, bail};
use notify::{RecursiveMode, Watcher};

use crate::viewer::{self, Viewer};
use crate::watch::{render_instance, stage_and_load};

/// Debounce so an external rebuild's non-atomic write isn't loaded half-finished.
const DEBOUNCE: Duration = Duration::from_millis(120);

pub fn run(artifact: &Path, port: u16, watch: bool) -> Result<()> {
	if artifact.is_dir() {
		bail!("preview needs a built plugin artifact — use `aexlo dev` to watch a crate's source");
	}

	let viewer = viewer::start(port)?;
	println!("aexlo preview: serving {} (Ctrl+C to quit)", viewer.url);
	if watch {
		println!("aexlo preview: watching {}", artifact.display());
	}
	viewer::open_browser(&viewer.url);

	// The live instance and the staged copy it was loaded from. Kept alive across
	// param edits so re-rendering doesn't reload; replaced on each disk reload.
	let mut instance: Option<PluginInstance> = None;
	let mut staged: Option<PathBuf> = None;
	let mut attempt: u64 = 0;

	// Load once on startup.
	attempt += 1;
	reload(artifact, attempt, &viewer, &mut instance, &mut staged);

	// Only wire up the file watcher when asked; without `--watch`, `preview`
	// serves the artifact exactly as loaded (parameter edits still re-render it).
	let _watcher; // keep the watcher alive for the loop's lifetime
	let rx = if watch {
		let (tx, rx) = mpsc::channel();
		let mut watcher = notify::recommended_watcher(move |res| {
			let _ = tx.send(res);
		})
		.context("creating file watcher")?;
		// Watch the parent directory, not the file inode: a rebuild often replaces
		// the file, after which inode-level watches go silent.
		let dir = artifact
			.parent()
			.filter(|p| !p.as_os_str().is_empty())
			.unwrap_or_else(|| Path::new("."));
		watcher
			.watch(dir, RecursiveMode::NonRecursive)
			.with_context(|| format!("watching {}", dir.display()))?;
		_watcher = watcher;
		Some(rx)
	} else {
		None
	};

	let mut pending: Option<Instant> = None;

	loop {
		// Collapse any file-change events into a single pending reload.
		if let Some(rx) = &rx {
			while let Ok(res) = rx.try_recv() {
				if let Ok(event) = res
					&& event.paths.iter().any(|p| p.file_name() == artifact.file_name())
				{
					pending = Some(Instant::now());
				}
			}
		}

		if let Some(since) = pending
			&& since.elapsed() >= DEBOUNCE
		{
			pending = None;
			attempt += 1;
			reload(artifact, attempt, &viewer, &mut instance, &mut staged);
		}

		// Apply any parameter edits, then re-render once for the whole batch.
		if let Some(fx) = &mut instance
			&& viewer.apply_edits(fx)
		{
			let _ = fx.update_params_ui();
			match render_instance(fx) {
				Ok((rgba, w, h)) => viewer.publish_frame(fx, rgba, w, h),
				Err(err) => eprintln!("aexlo preview: re-render failed: {err:#}"),
			}
		}

		std::thread::sleep(Duration::from_millis(50));
	}
}

/// (Re)load the artifact from disk, render a frame, and publish it — swapping in
/// the new instance and dropping the previous staged copy. On failure the last
/// good frame stays on screen and the browser dot goes red.
fn reload(
	artifact: &Path,
	attempt: u64,
	viewer: &Viewer,
	instance: &mut Option<PluginInstance>,
	staged: &mut Option<PathBuf>,
) {
	viewer.begin_attempt(attempt);
	match stage_and_load(artifact, attempt) {
		Ok((mut fx, new_staged)) => match render_instance(&mut fx) {
			Ok((rgba, w, h)) => {
				viewer.publish_reload(&fx, rgba, w, h);
				println!("aexlo preview: loaded {} → {w}×{h}", artifact.display());
				*instance = Some(fx);
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
			eprintln!("\n─── load failed ───\n{err:#}\n");
		}
	}
}
