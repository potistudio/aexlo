//! `aexlo dev [filter] [-p <package>]` - rerun a `#[aexlo_test::preview]` test on save.
//!
//! [`cmd`] hands `--bin` to [`crate::watch`] and
//! `--bin --web` to [`crate::web`].
//!
//! Built-in replacement for pairing `bacon` with `#[aexlo_test::preview]`: watches
//! the crate's sources and reruns `cargo test` on every change, with
//! `AEXLO_PREVIEW=live` set so the test's own `ensure_live_viewer` call pops (or
//! keeps updating) an `aexlo view` window. Each run is a fresh process driving
//! the in-process `from_entry` render path - not a `dlopen`'d cdylib like
//! `aexlo dev --bin` (see `watch.rs`) - so `println!`, `dbg!`, and debugger
//! attachment all work normally against a single run.

use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use notify::{RecursiveMode, Watcher};

use crate::args;

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
	/// Preview in the browser instead of a native window, streaming the frame
	/// into a <canvas>. Good for headless or remote hosts
	#[arg(long, requires = "bin")]
	web: bool,
	/// Port to serve on [default: OS-assigned]
	#[arg(long, value_name = "N", requires = "web")]
	port: Option<u16>,
	/// Start the viewer on a manifest preset; its picker lists them all
	#[arg(long, value_name = "NAME", requires = "web")]
	preset: Option<String>,
	/// The manifest [default: the nearest aexlo.toml]
	#[arg(long, value_name = "PATH", requires = "web")]
	manifest: Option<PathBuf>,
	/// Poison and guard the output, showing unwritten pixels and overwritten
	/// guard bands
	#[arg(long, requires = "web")]
	strict: bool,
	/// Don't open a browser tab
	#[arg(long, requires = "web")]
	no_open: bool,
}

pub fn cmd(args: DevArgs) -> Result<()> {
	let DevArgs {
		filter,
		package,
		bin: bin_mode,
		web: web_mode,
		port,
		preset,
		manifest: manifest_path,
		strict,
		no_open,
	} = args;
	let manifest = args::cargo_package(package.as_deref())?;

	if bin_mode {
		if web_mode {
			crate::web::run(
				&manifest,
				crate::web::Options {
					port: port.unwrap_or(0),
					manifest: manifest_path.as_deref(),
					preset: preset.as_deref(),
					strict,
					no_open,
				},
			)
		} else {
			crate::watch::run(&manifest)
		}
	} else {
		run(&manifest, filter.as_deref())
	}
}

/// Debounce window so a burst of editor save events triggers one rerun.
const DEBOUNCE: Duration = Duration::from_millis(150);

pub fn run(manifest: &Path, filter: Option<&str>) -> Result<()> {
	let crate_dir = manifest.parent().context("manifest path has no parent directory")?;
	let src_dir = crate_dir.join("src");

	let (tx, rx) = mpsc::channel();
	let mut watcher = notify::recommended_watcher(move |res| {
		let _ = tx.send(res);
	})
	.context("creating file watcher")?;
	watcher
		.watch(&src_dir, RecursiveMode::Recursive)
		.with_context(|| format!("watching {}", src_dir.display()))?;
	let _ = watcher.watch(manifest, RecursiveMode::NonRecursive);

	let mut current: Option<Child> = None;
	let mut pending: Option<Instant> = Some(Instant::now()); // run once on startup

	println!("aexlo dev: watching {} (Ctrl+C to quit)", src_dir.display());

	loop {
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
			// Let an in-flight run finish rather than killing it: cargo already
			// serializes on the target-dir lock, so racing a second `cargo test`
			// in would just block on that lock anyway.
			if let Some(child) = &mut current {
				let _ = child.wait();
			}
			current = Some(spawn_test(manifest, filter)?);
		}

		std::thread::sleep(Duration::from_millis(50));
	}
}

/// Run `cargo test` for `manifest`, filtered to `filter` if given, with
/// `AEXLO_PREVIEW=live` so any `#[aexlo_test::preview]` it runs pops a live viewer.
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
	cmd.env("AEXLO_PREVIEW", "live");
	cmd.spawn().context("spawning cargo test")
}

/// Only rerun for source-ish changes, ignoring editor swap/lock files.
fn is_relevant(path: &Path) -> bool {
	matches!(path.extension().and_then(|s| s.to_str()), Some("rs"))
		|| path.file_name().is_some_and(|n| n == "Cargo.toml")
}
