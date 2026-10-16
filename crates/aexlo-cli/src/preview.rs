//! `aexlo preview <plugin>` - interactively preview a *built* plugin.
//!
//! Loads a finished `.plugin`/`.aex`/`.dll` (no compiler in the loop) and serves
//! the same interactive surface as `aexlo dev --bin --web` via [`crate::viewer`]:
//! drag the plugin's parameters and watch the frame re-render. With `--watch` it
//! also reloads the artifact whenever the file changes on disk - e.g. rebuilt by
//! another toolchain or by After Effects' own build. This is the `vite preview`
//! to `dev`'s `vite dev`: same viewer, output instead of source.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use aexlo::{Depth8, Layer};
use anyhow::{Context, Result, bail};
use notify::{RecursiveMode, Watcher};

use crate::session::Session;
use crate::viewer::{self, Viewer};

/// Debounce so an external rebuild's non-atomic write isn't loaded half-finished.
const DEBOUNCE: Duration = Duration::from_millis(120);

/// A decoded `--input` PNG, kept as raw pixels so a fresh [`Layer`] can be built
/// for each instance the watch loop loads (the file is read exactly once).
struct Input {
	rgba: Vec<u8>,
	w: u32,
	h: u32,
}

impl Input {
	fn layer(&self) -> Result<Layer<Depth8>> {
		Layer::<Depth8>::from_raw(self.rgba.clone(), self.w, self.h)
			.map_err(|e| anyhow::anyhow!("building input layer: {e}"))
	}
}

/// `aexlo preview`'s options.
pub struct Options<'a> {
	pub input: Option<&'a Path>,
	pub port: u16,
	pub watch: bool,
	pub manifest: Option<&'a Path>,
	pub preset: Option<&'a str>,
	pub strict: bool,
	/// Don't open a browser tab (the URL is printed either way).
	pub no_open: bool,
}

pub fn run(artifact: &Path, options: Options<'_>) -> Result<()> {
	if artifact.is_dir() {
		bail!("preview needs a built plugin artifact - use `aexlo dev` to watch a crate's source");
	}

	// Decode up front: a bad path should fail before we open a browser tab.
	let input = match options.input {
		Some(path) => {
			let (rgba, w, h) = crate::load_input(path)?;
			println!("aexlo preview: input {} ({w}×{h})", path.display());
			Some(Input { rgba, w, h })
		}
		None => None,
	};
	let manifest = crate::session::viewer_manifest(options.manifest, options.preset.is_some())?;
	let mut session = Session::new(manifest, options.preset, options.strict)?;

	let viewer = viewer::start(options.port)?;
	println!("aexlo preview: serving {} (Ctrl+C to quit)", viewer.url);
	if options.watch {
		println!("aexlo preview: watching {}", artifact.display());
	}
	if !options.no_open {
		viewer::open_browser(&viewer.url);
	}

	// The staged copy the session loads instances from; replaced on each
	// disk reload so a rebuilt file is never served from a stale mapping.
	let input = std::sync::Arc::new(input);
	let mut staged: Option<PathBuf> = None;
	let mut attempt: u64 = 0;

	// Load once on startup.
	attempt += 1;
	reload(artifact, &input, attempt, &viewer, &mut session, &mut staged);

	// Only wire up the file watcher when asked; without `--watch`, `preview`
	// serves the artifact exactly as loaded (parameter edits still re-render it).
	let _watcher; // keep the watcher alive for the loop's lifetime
	let rx = if options.watch {
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
			reload(artifact, &input, attempt, &viewer, &mut session, &mut staged);
		}

		// Parameter edits, preset picks, time, comparisons, saves.
		session.step(&viewer);

		std::thread::sleep(Duration::from_millis(50));
	}
}

/// Stage the artifact afresh and have the session load from it. On failure
/// the last good frame stays on screen and the browser dot goes red.
fn reload(
	artifact: &Path,
	input: &std::sync::Arc<Option<Input>>,
	attempt: u64,
	viewer: &Viewer,
	session: &mut Session,
	staged: &mut Option<PathBuf>,
) {
	viewer.begin_attempt(attempt);
	let ext = artifact.extension().and_then(|s| s.to_str()).unwrap_or("dylib");
	let copy = std::env::temp_dir().join(format!("aexlo-preview-{}-{attempt}.{ext}", std::process::id()));
	if let Err(err) = copy_artifact(artifact, &copy) {
		viewer.fail_attempt(attempt);
		eprintln!("\n─── load failed ───\n{err:#}\n");
		return;
	}
	let (path, input) = (copy.clone(), input.clone());
	session.set_loader(Box::new(move || {
		let mut fx = aexlo::Host::get().try_load(&path).context("loading plugin")?;
		// Install the input layer before the first render, so a fresh
		// instance never shows a frame built from the default test image (a
		// preset's own input replaces it).
		if let Some(input) = input.as_ref() {
			fx.set_input_layer(input.layer()?);
		}
		Ok(fx)
	}));
	match session.reload(viewer) {
		Ok(()) => {
			println!("aexlo preview: loaded {}", artifact.display());
			if let Some(old) = staged.replace(copy) {
				remove_staged(&old);
			}
		}
		Err(err) => {
			viewer.fail_attempt(attempt);
			eprintln!("\n─── render failed ───\n{err:#}\n");
		}
	}
}

/// Copy a plugin artifact (a file, or a `.plugin` bundle directory).
fn copy_artifact(from: &Path, to: &Path) -> Result<()> {
	if from.is_dir() {
		bail!("{} is a directory", from.display());
	}
	std::fs::copy(from, to).with_context(|| format!("staging {}", from.display()))?;
	Ok(())
}

fn remove_staged(path: &Path) {
	let _ = std::fs::remove_file(path);
}
