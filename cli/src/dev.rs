//! `aexlo dev <crate> [filter]` — rerun a `#[aexlo::preview]` test on save.
//!
//! Built-in replacement for pairing `bacon` with `#[aexlo::preview]`: watches
//! the crate's sources and reruns `cargo test` on every change, with
//! `AEXLO_PREVIEW=live` set so the test's own `ensure_live_viewer` call pops (or
//! keeps updating) an `aexlo view` window. Each run is a fresh process driving
//! the in-process `from_entry` render path — not a `dlopen`'d cdylib like
//! `aexlo dev --bin` (see `watch.rs`) — so `println!`, `dbg!`, and debugger
//! attachment all work normally against a single run.

use std::path::Path;
use std::process::{Child, Command};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use notify::{RecursiveMode, Watcher};

/// Debounce window so a burst of editor save events triggers one rerun.
const DEBOUNCE: Duration = Duration::from_millis(150);

pub fn run(crate_dir: &Path, filter: Option<&str>) -> Result<()> {
	let manifest = crate_dir.join("Cargo.toml");
	if !manifest.exists() {
		bail!("no Cargo.toml at {} — pass a crate directory", crate_dir.display());
	}
	let src_dir = crate_dir.join("src");

	let (tx, rx) = mpsc::channel();
	let mut watcher = notify::recommended_watcher(move |res| {
		let _ = tx.send(res);
	})
	.context("creating file watcher")?;
	watcher
		.watch(&src_dir, RecursiveMode::Recursive)
		.with_context(|| format!("watching {}", src_dir.display()))?;
	let _ = watcher.watch(&manifest, RecursiveMode::NonRecursive);

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
			current = Some(spawn_test(&manifest, filter)?);
		}

		std::thread::sleep(Duration::from_millis(50));
	}
}

/// Run `cargo test` for `manifest`, filtered to `filter` if given, with
/// `AEXLO_PREVIEW=live` so any `#[aexlo::preview]` it runs pops a live viewer.
///
/// `#[aexlo::preview]` generates an `#[ignore]`d test (so a plain `cargo test`
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
