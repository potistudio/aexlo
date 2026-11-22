//! `aexlo dev [filter] [-p <package>]` - rerun a `#[aexlo_test::preview]` test on save.
//!
//! [`cmd`] reads the flags and hands `--bin` to [`crate::watch`] and
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

use anyhow::{Context, Result, bail};
use notify::{RecursiveMode, Watcher};

use crate::args;

pub const HELP: &str = "\
Usage: aexlo dev [filter] [OPTIONS]

Rerun a #[aexlo_test::preview] test on every save and show its frame in a
live viewer (a built-in `bacon` replacement; [filter] picks the tests). Like
`cargo build`, it works on the crate in the current directory unless -p
names another.

Options:
  -p, --package <name>    Preview this workspace member instead
      --bin               Skip the test harness: rebuild the crate's cdylib
                          and load and render it directly on save (faster,
                          but no println!/dbg!/debugger, and no filter)
      --web               Preview in the browser instead of a native window,
                          streaming the frame into a <canvas>; requires
                          --bin. Good for headless or remote hosts
      --port <n>          (--web) Port to serve on  [default: OS-assigned]
      --preset <name>     (--web) Start the viewer on a manifest preset; its
                          picker lists them all
      --manifest <path>   (--web) The manifest
                          [default: the nearest aexlo.toml]
      --strict            (--web) Poison and guard the output, showing
                          unwritten pixels and overwritten guard bands
      --no-open           (--web) Don't open a browser tab
";

pub fn cmd(args: impl Iterator<Item = String>) -> Result<()> {
	let mut package: Option<String> = None;
	let mut filter: Option<String> = None;
	let mut bin_mode = false;
	let mut web_mode = false;
	let mut port: u16 = 0;
	let mut preset: Option<String> = None;
	let mut manifest_path: Option<PathBuf> = None;
	let mut strict = false;
	let mut no_open = false;

	let mut args = args.peekable();

	while let Some(arg) = args.next() {
		match arg.as_str() {
			"--bin" => bin_mode = true,
			"--web" => web_mode = true,
			"--preset" => preset = Some(args::next_value(&mut args, &arg)?),
			"--manifest" => manifest_path = Some(PathBuf::from(args::next_value(&mut args, &arg)?)),
			"--strict" => strict = true,
			"--no-open" => no_open = true,
			"--port" => port = args::port(&mut args, &arg)?,
			"-p" | "--package" => package = Some(args::next_value(&mut args, &arg)?),
			other if other.starts_with('-') => bail!("unknown option '{other}'"),
			_ if filter.is_none() => filter = Some(arg),
			_ => bail!("dev: expected [filter] [-p <package>] [--bin] [--web] [--port <n>]"),
		}
	}

	if web_mode && !bin_mode {
		bail!("dev --web requires --bin (the browser preview uses the cdylib render path)");
	}
	if port != 0 && !web_mode {
		bail!("--port only applies to --web");
	}
	if (preset.is_some() || manifest_path.is_some() || strict || no_open) && !web_mode {
		bail!("--preset, --manifest, --strict and --no-open drive the browser viewer: add --bin --web");
	}

	let manifest = args::cargo_package(package.as_deref())?;

	if bin_mode {
		if filter.is_some() {
			bail!("dev --bin: no test filter - it doesn't run the test harness");
		}

		if web_mode {
			crate::web::run(
				&manifest,
				crate::web::Options {
					port,
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
