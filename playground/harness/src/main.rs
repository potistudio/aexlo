//! playground — drives the aexlo-probe plugin and compares host behavior.
//!
//! The probe plugin (playground/probe) records everything a host does to a
//! JSONL trace. This binary runs it under aexlo, packages it for real After
//! Effects, and diffs the two traces:
//!
//! ```text
//! cargo run -p playground -- run                 # trace the probe under aexlo
//! cargo run -p playground -- package             # build dist/AexloProbe.aex for real AE
//! cargo run -p playground -- report <trace>      # summarize one trace
//! cargo run -p playground -- diff <ae> <aexlo>   # compare real AE vs aexlo
//! cargo run -p playground -- pipl                # verify the embedded PiPL resource
//! ```

mod summary;

#[cfg(windows)]
mod pipl;

#[cfg(test)]
mod runtime_facts_test;

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Context;
use colored::Colorize;

use aexlo::{Depth8, Layer, ParamValue, PluginInstance};

const USAGE: &str = "\
playground — aexlo probe-plugin harness

USAGE:
  playground run [--release] [--in-process] [--trace <file>] [--input <png>]
      Build the probe, load it under aexlo, render a frame, and report the trace.
  playground report <trace.jsonl>
      Summarize a single trace (from aexlo or from real After Effects).
  playground diff <a.jsonl> <b.jsonl> [--all]
      Compare two traces key-by-key. Exits 1 if they differ.
  playground package [--debug] [--to <dir>]
      Build the probe and drop AexloProbe.aex (default: playground/dist/).
  playground pipl [--release]
      Parse the PiPL resource back out of the built probe DLL (Windows).
";

fn main() -> anyhow::Result<()> {
	let args: Vec<String> = std::env::args().skip(1).collect();

	match args.first().map(String::as_str) {
		Some("run") => run(&args[1..]),
		Some("report") => report(&args[1..]),
		Some("diff") => diff(&args[1..]),
		Some("package") => package(&args[1..]),
		Some("pipl") => pipl_cmd(&args[1..]),
		_ => {
			eprint!("{USAGE}");
			std::process::exit(2);
		}
	}
}

fn workspace_root() -> PathBuf {
	Path::new(env!("CARGO_MANIFEST_DIR"))
		.join("../..")
		.canonicalize()
		.expect("workspace root")
}

fn flag(args: &[String], name: &str) -> bool {
	args.iter().any(|a| a == name)
}

fn option<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
	args.iter()
		.position(|a| a == name)
		.and_then(|i| args.get(i + 1))
		.map(String::as_str)
}

/// `cargo build -p aexlo-probe` and return the built cdylib path.
fn build_probe(release: bool) -> anyhow::Result<PathBuf> {
	let root = workspace_root();

	let mut cmd = Command::new("cargo");
	cmd.current_dir(&root).args(["build", "-p", "aexlo-probe"]);
	if release {
		cmd.arg("--release");
	}
	anyhow::ensure!(cmd.status()?.success(), "cargo build -p aexlo-probe failed");

	let profile = if release { "release" } else { "debug" };
	let file = format!(
		"{}aexlo_probe{}",
		std::env::consts::DLL_PREFIX,
		std::env::consts::DLL_SUFFIX
	);
	let path = root.join("target").join(profile).join(file);
	anyhow::ensure!(path.exists(), "built probe not found at {}", path.display());
	Ok(path)
}

fn run(args: &[String]) -> anyhow::Result<()> {
	let root = workspace_root();
	let release = flag(args, "--release");
	let in_process = flag(args, "--in-process");

	let trace_path = option(args, "--trace")
		.map(PathBuf::from)
		.unwrap_or_else(|| root.join("target/probe/trace-aexlo.jsonl"));
	let input_path = option(args, "--input")
		.map(PathBuf::from)
		.unwrap_or_else(|| root.join("input.png"));

	if let Some(parent) = trace_path.parent() {
		std::fs::create_dir_all(parent)?;
	}
	let _ = std::fs::remove_file(&trace_path);

	// SAFETY: single-threaded at this point; the probe reads this at first use.
	unsafe { std::env::set_var("AEXLO_PROBE_TRACE", &trace_path) };

	let mut instance = if in_process {
		println!("{}", "loading probe in-process (breakpoint-friendly)".dimmed());
		unsafe { PluginInstance::from_entry_raw(aexlo_probe::EffectMain as usize) }?
	} else {
		let dll = build_probe(release)?;
		println!("{} {}", "loading".dimmed(), dll.display());
		PluginInstance::try_load(&dll)?
	};

	println!("{} {}", "about:".bold(), instance.about()?);

	let img = image::open(&input_path)
		.with_context(|| format!("failed to open input image {}", input_path.display()))?
		.to_rgba8();
	let (width, height) = img.dimensions();
	instance.set_input(Layer::<Depth8>::from_raw(img.into_raw(), width, height)?);

	// Nudge every control off its default so parameter plumbing shows up in
	// the trace and in the picture.
	for index in 0..instance.param_count() {
		let value = match instance.param_name(index).as_deref() {
			Some("Gain") => ParamValue::Float(1.5),
			Some("Invert") => ParamValue::Checkbox(true),
			Some("Mode") => ParamValue::Popup(2),
			Some("Tint") => ParamValue::Color {
				red: 255,
				green: 128,
				blue: 0,
				alpha: 255,
			},
			Some("Angle") => ParamValue::Angle(45.0),
			Some("Center") => ParamValue::Point {
				x: width as f32 / 2.0,
				y: height as f32 / 2.0,
			},
			_ => continue,
		};
		if let Err(error) = instance.set_param(index, value) {
			eprintln!("{} set_param({index}): {error}", "warn:".yellow());
		}
	}

	instance.render_frame()?;

	let preview_path = root.join("target/probe/preview-aexlo.png");
	instance.save_preview(&preview_path)?;

	// Unload triggers SEQUENCE_SETDOWN/GLOBAL_SETDOWN so the trace is complete.
	drop(instance);

	println!();
	println!("{} {}", "trace:".bold(), trace_path.display());
	println!("{} {}", "preview:".bold(), preview_path.display());
	println!();

	print_report(&trace_path)
}

fn report(args: &[String]) -> anyhow::Result<()> {
	let path = args
		.first()
		.map(PathBuf::from)
		.context("usage: playground report <trace.jsonl>")?;
	print_report(&path)
}

fn print_report(path: &Path) -> anyhow::Result<()> {
	let events = summary::load_events(path)?;
	let summary = summary::summarize(&events);

	let section = |title: &str| println!("\n{}", title.bold().underline());

	section("Host");
	for (key, value) in summary.range("host/".to_string().."host/~".to_string()) {
		println!("  {key}: {value}");
	}
	for (key, value) in summary.range("global/".to_string().."global/~".to_string()) {
		println!("  {key}: {value}");
	}

	section("Facts (fixed input → exact output; compared by default)");
	for (key, value) in summary.range("fact/".to_string().."fact/~".to_string()) {
		println!("  {} = {value}", key.strip_prefix("fact/").unwrap_or(key));
	}

	section("Commands (context — how this host drove the plugin)");
	for (key, value) in summary.range("cmd/".to_string().."cmd/~".to_string()) {
		println!("  {key} = {value}");
	}

	section("Suites");
	let (mut ok, mut missing) = (0, 0);
	for (key, value) in summary.range("suite/".to_string().."suite/~".to_string()) {
		let name = key.strip_prefix("suite/").unwrap_or(key);
		if value == "ok" {
			ok += 1;
			println!("  {} {name}", "✓".green());
		} else {
			missing += 1;
			println!("  {} {name} ({value})", "✗".red());
		}
	}
	println!(
		"  {} acquired, {} unavailable",
		ok.to_string().green(),
		missing.to_string().red()
	);

	section("Callbacks");
	for (key, value) in summary.range("callback/".to_string().."callback/~".to_string()) {
		println!("  {}: {value}", key.strip_prefix("callback/").unwrap_or(key));
	}
	for (key, value) in summary.range("sequence/".to_string().."sequence/~".to_string()) {
		println!("  {key}: {value}");
	}

	section("Render");
	for (key, value) in summary.range("render/".to_string().."render/~".to_string()) {
		println!("  {key} = {value}");
	}
	for (key, value) in summary.range("param/".to_string().."param/~".to_string()) {
		println!("  {key} = {value}");
	}

	let panics: Vec<_> = summary.range("panic/".to_string().."panic/~".to_string()).collect();
	if !panics.is_empty() {
		section("Panics");
		for (key, value) in panics {
			println!("  {} {key}: {value}", "!".red().bold());
		}
	}

	println!();
	Ok(())
}

fn diff(args: &[String]) -> anyhow::Result<()> {
	let (Some(left_path), Some(right_path)) = (args.first(), args.get(1)) else {
		anyhow::bail!("usage: playground diff <a.jsonl> <b.jsonl> [--all]");
	};
	let include_all = flag(args, "--all");

	let left = summary::summarize(&summary::load_events(Path::new(left_path))?);
	let right = summary::summarize(&summary::load_events(Path::new(right_path))?);

	let (lines, matches) = summary::diff(&left, &right, include_all);

	println!(
		"\n{}  A = {left_path}\n{}  B = {right_path}\n",
		"diffing traces".bold(),
		"              ".bold()
	);

	for line in &lines {
		match line {
			summary::DiffLine::Changed(key, l, r) => {
				println!("{} {key}", "≠".yellow().bold());
				println!("    A: {}", l.red());
				println!("    B: {}", r.green());
			}
			summary::DiffLine::OnlyLeft(key, l) => {
				println!("{} {key} (only in A)", "<".red().bold());
				println!("    A: {}", l.red());
			}
			summary::DiffLine::OnlyRight(key, r) => {
				println!("{} {key} (only in B)", ">".green().bold());
				println!("    B: {}", r.green());
			}
		}
	}

	println!(
		"\n{} matching, {} differing{}",
		matches.to_string().green().bold(),
		lines.len().to_string().red().bold(),
		if include_all {
			" (including context keys)"
		} else {
			" (facts, suites, presence; --all for context)"
		}
	);

	if !lines.is_empty() {
		std::process::exit(1);
	}
	Ok(())
}

fn package(args: &[String]) -> anyhow::Result<()> {
	let release = !flag(args, "--debug");
	let dll = build_probe(release)?;

	let dist = option(args, "--to")
		.map(PathBuf::from)
		.unwrap_or_else(|| workspace_root().join("playground/dist"));
	std::fs::create_dir_all(&dist)?;

	let target = dist.join("AexloProbe.aex");
	std::fs::copy(&dll, &target)?;

	println!("{} {}", "packaged:".bold(), target.display());
	println!();
	println!("To test against real After Effects, copy it into a plugin folder, e.g.:");
	println!("  C:\\Program Files\\Adobe\\Common\\Plug-ins\\7.0\\MediaCore\\");
	println!("then apply Effect > aexlo > Aexlo Probe to a layer and render a frame.");
	println!("The trace lands in %TEMP%\\aexlo-probe\\ (the effect's About dialog shows the path).");
	Ok(())
}

fn pipl_cmd(_args: &[String]) -> anyhow::Result<()> {
	#[cfg(windows)]
	{
		let dll = build_probe(flag(_args, "--release"))?;
		pipl::dump(&dll)
	}

	#[cfg(not(windows))]
	anyhow::bail!("the pipl command reads Windows resources and is only available on Windows");
}
