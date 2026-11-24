//! `playground ae`: records a probe trace in real After Effects without
//! touching the GUI.
//!
//! The steps are the ones the README describes for a manual run:
//!
//! 1. Build the probe and install it into the After Effects `Plug-ins` folder.
//! 2. Hand `ae/trace.jsx` to `AfterFX.exe -r`. The script imports
//!    `fixtures/input.png`, applies the probe, sets the same parameter values
//!    `playground run` sets under aexlo, renders the frames, and removes
//!    everything it added.
//! 3. Collect the trace and print the report.
//!
//! `--variant smart` measures the SmartRender effect: the script renders one
//! frame sequence at each of the depths in `variant.depths()` (8, 16 and 32
//! bpc), so a single trace carries all three `smart.<bpc>.*` fact sets.
//!
//! `AfterFX.exe -r` behaves in two ways. If no After Effects is running, the
//! spawned process becomes the instance: it starts, runs the script, and the
//! script quits it. The harness sets `AEXLO_PROBE_TRACE` for that process,
//! so the trace covers one clean session from plugin load to
//! `GLOBAL_SETDOWN`. If an After Effects is already running, the spawned
//! process forwards the script to it and exits. The trace is then the file
//! the running instance's probe already writes (`%TEMP%\aexlo-probe\
//! trace-*-pid<pid>.jsonl`), copied after the script finishes.
//!
//! Windows only: on macOS the scripting entry point is AppleScript, which this
//! module does not drive.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::Context;
use colored::Colorize;
use serde_json::{Value, json};

use crate::variant::Variant;
use crate::{flag, option, workspace_root};

/// The script template. `run_script` prepends the `var AEXLO = {...};` line.
const TRACE_JSX: &str = include_str!("../ae/trace.jsx");

/// Frame rate of the comp the script builds. Render times are frame starts.
const FPS: f64 = 30.0;

/// The measured variant's match name, as the JSX passes it to `addProperty`.
/// The harness links the legacy rlib, so the smart name comes from its own
/// constant rather than from a build with the `smart` feature.
fn probe_match_name(variant: Variant) -> &'static str {
	let name = match variant {
		Variant::Legacy => aexlo_probe::MATCH_NAME,
		Variant::Smart => aexlo_probe::SMART_MATCH_NAME,
	};
	name.to_str().expect("the probe's match name is ASCII")
}

pub fn ae(args: &[String]) -> anyhow::Result<()> {
	let root = workspace_root();
	let variant = Variant::from_args(args)?;
	let timeout = Duration::from_secs(
		option(args, "--timeout")
			.map(str::parse)
			.transpose()
			.context("--timeout takes whole seconds")?
			.unwrap_or(600),
	);
	let frames: usize = option(args, "--frames")
		.map(str::parse)
		.transpose()
		.context("--frames takes a whole number")?
		.unwrap_or(1);
	anyhow::ensure!(frames >= 1, "--frames must be at least 1");

	let trace_path = option(args, "--trace")
		.map(PathBuf::from)
		.unwrap_or_else(|| root.join("target/probe").join(variant.ae_trace_file()));
	let input_path = option(args, "--input")
		.map(PathBuf::from)
		.unwrap_or_else(|| root.join("fixtures/input.png"));
	let input_path = input_path
		.canonicalize()
		.with_context(|| format!("input image {} not found", input_path.display()))?;

	let running = running_after_effects()?;
	anyhow::ensure!(
		running.len() <= 1,
		"{} After Effects processes are running ({}); leave one open, or close them all",
		running.len(),
		running.iter().map(|p| p.pid.to_string()).collect::<Vec<_>>().join(", ")
	);
	let running = running.into_iter().next();

	let exe = choose_after_effects(option(args, "--ae"), running.as_ref())?;
	println!("{} {}", "after effects:".bold(), exe.display());
	match &running {
		Some(process) => println!("{} running instance (pid {})", "mode:".bold(), process.pid),
		None => println!("{} new instance, quit when the script is done", "mode:".bold()),
	}

	let packaged = if flag(args, "--no-build") {
		let path = root.join("playground/dist").join(variant.aex_name());
		anyhow::ensure!(path.exists(), "{} not found; drop --no-build", path.display());
		path
	} else {
		crate::package_probe(true, variant, &root.join("playground/dist"))?
	};

	let plugin_dir = option(args, "--plugin-dir").map(PathBuf::from).unwrap_or_else(|| {
		exe.parent()
			.map(|support| support.join("Plug-ins").join("aexlo"))
			.unwrap_or_default()
	});
	install(&packaged, &plugin_dir.join(variant.aex_name()), running.is_some())?;

	// A fresh instance reads `AEXLO_PROBE_TRACE`, and every installed probe
	// writes to that one file; remove the probe that is not measured so its
	// events stay out of the trace. A running instance never sets the
	// variable - each probe writes its own auto-named file - so there is
	// nothing to correct in that case.
	if running.is_none() {
		remove_other_probe(&plugin_dir, variant);
	}

	let run_dir = root.join("target/probe/ae-run");
	std::fs::create_dir_all(&run_dir)?;
	if let Some(parent) = trace_path.parent() {
		std::fs::create_dir_all(parent)?;
	}

	let (width, height) = image::image_dimensions(&input_path)
		.with_context(|| format!("failed to read the size of {}", input_path.display()))?;
	let result_path = run_dir.join("result.json");
	let config = json!({
		"input": jsx_path(&input_path),
		"match_name": probe_match_name(variant),
		"depths": variant.depths(),
		"fps": FPS,
		"times": (0..frames).map(|i| i as f64 / FPS).collect::<Vec<_>>(),
		"params": params(width, height),
		"render_stem": jsx_path(&run_dir.join("render")),
		"result": jsx_path(&result_path),
		"quit": running.is_none(),
	});

	let result = run_script(
		&exe,
		&run_dir,
		&config,
		&result_path,
		&trace_path,
		running.as_ref(),
		timeout,
	)?;

	if let Some(process) = &running {
		let source = running_trace(process.pid, variant == Variant::Smart)?;
		std::fs::copy(&source, &trace_path)
			.with_context(|| format!("failed to copy {} to {}", source.display(), trace_path.display()))?;
		println!("{} {}", "copied:".dimmed(), source.display());
	}

	print_result(&result);
	println!("{} {}", "trace:".bold(), trace_path.display());

	if result["ok"] != json!(true) {
		anyhow::bail!(
			"the script failed in After Effects: {}",
			result["error"].as_str().unwrap_or("no error message")
		);
	}

	println!();
	crate::print_report(&trace_path)?;
	println!(
		"compare with aexlo: cargo run -p playground -- diff {} target/probe/{}",
		trace_path.display(),
		variant.trace_file()
	);
	Ok(())
}

/// The values `playground run` gives every probe control, in the form
/// `Property.setValue` takes. Keep the two in step, or render context diffs
/// will differ for a reason that is not the host.
fn params(width: u32, height: u32) -> Value {
	json!([
		{ "name": "Gain", "value": 1.5 },
		{ "name": "Invert", "value": 1 },
		{ "name": "Mode", "value": 2 },
		{ "name": "Tint", "value": [1.0, 128.0 / 255.0, 0.0, 1.0] },
		{ "name": "Angle", "value": 45.0 },
		{ "name": "Center", "value": [width as f64 / 2.0, height as f64 / 2.0] },
	])
}

/// ExtendScript takes forward slashes on Windows, and they avoid escaping.
fn jsx_path(path: &Path) -> String {
	let text = path.display().to_string();
	// `canonicalize` on Windows yields the `\\?\` verbatim prefix, which
	// ExtendScript's `File` does not understand.
	text.strip_prefix(r"\\?\").unwrap_or(&text).replace('\\', "/")
}

struct AeProcess {
	pid: u32,
	exe: PathBuf,
}

/// Every running `AfterFX.exe`, with its executable path, so the script and
/// the probe go to the version that is actually open.
fn running_after_effects() -> anyhow::Result<Vec<AeProcess>> {
	let output = Command::new("powershell")
		.args([
			"-NoProfile",
			"-NonInteractive",
			"-Command",
			"Get-Process -Name AfterFX -ErrorAction SilentlyContinue | ForEach-Object { \"$($_.Id)`t$($_.Path)\" }",
		])
		.output()
		.context("failed to run powershell to list After Effects processes")?;

	let text = String::from_utf8_lossy(&output.stdout);
	Ok(text
		.lines()
		.filter_map(|line| {
			let (pid, exe) = line.trim().split_once('\t')?;
			Some(AeProcess {
				pid: pid.parse().ok()?,
				exe: PathBuf::from(exe),
			})
		})
		.collect())
}

/// `--ae`, then `AEXLO_AE_EXE`, then the running instance, then the newest
/// `Adobe After Effects <year>` under Program Files. Beta builds are taken
/// only when named explicitly.
fn choose_after_effects(explicit: Option<&str>, running: Option<&AeProcess>) -> anyhow::Result<PathBuf> {
	let explicit = explicit
		.map(PathBuf::from)
		.or_else(|| std::env::var_os("AEXLO_AE_EXE").map(PathBuf::from));

	if let Some(exe) = explicit {
		anyhow::ensure!(exe.exists(), "{} does not exist", exe.display());
		if let Some(process) = running {
			anyhow::ensure!(
				same_file(&exe, &process.exe),
				"{} is running (pid {}), not {}; close it or pass that path",
				process.exe.display(),
				process.pid,
				exe.display()
			);
		}
		return Ok(exe);
	}

	if let Some(process) = running {
		return Ok(process.exe.clone());
	}

	let program_files = std::env::var_os("ProgramFiles")
		.map(PathBuf::from)
		.unwrap_or_else(|| PathBuf::from(r"C:\Program Files"));
	let adobe = program_files.join("Adobe");

	let newest = std::fs::read_dir(&adobe)
		.with_context(|| format!("failed to list {}", adobe.display()))?
		.filter_map(|entry| {
			let entry = entry.ok()?;
			let name = entry.file_name().into_string().ok()?;
			let year: u32 = name.strip_prefix("Adobe After Effects ")?.parse().ok()?;
			let exe = entry.path().join("Support Files").join("AfterFX.exe");
			exe.exists().then_some((year, exe))
		})
		.max_by_key(|(year, _)| *year)
		.map(|(_, exe)| exe);

	newest.with_context(|| {
		format!(
			"no Adobe After Effects <year> found under {}; pass --ae <AfterFX.exe>",
			adobe.display()
		)
	})
}

fn same_file(a: &Path, b: &Path) -> bool {
	match (a.canonicalize(), b.canonicalize()) {
		(Ok(a), Ok(b)) => a == b,
		_ => a == b,
	}
}

/// Copy the probe into the plugin folder unless the same bytes are already
/// there. After Effects scans plugins only at startup, so a running instance
/// cannot pick up a new probe: that case stops with instructions instead.
fn install(packaged: &Path, installed: &Path, running: bool) -> anyhow::Result<()> {
	let fresh = std::fs::read(packaged).with_context(|| format!("failed to read {}", packaged.display()))?;
	if std::fs::read(installed).ok().as_deref() == Some(fresh.as_slice()) {
		println!("{} {} (up to date)", "probe:".bold(), installed.display());
		return Ok(());
	}

	anyhow::ensure!(
		!running,
		"{} is missing or older than the build, and After Effects only loads plugins at startup. \
		 Close After Effects and run this again; it will start a new instance.",
		installed.display()
	);

	if let Some(parent) = installed.parent() {
		std::fs::create_dir_all(parent).with_context(|| {
			format!(
				"failed to create {}; run as a user that can write there, or pass --plugin-dir",
				parent.display()
			)
		})?;
	}
	std::fs::copy(packaged, installed).with_context(|| {
		format!(
			"failed to install the probe to {}; run as a user that can write there, or pass --plugin-dir",
			installed.display()
		)
	})?;
	println!("{} {} (installed)", "probe:".bold(), installed.display());
	Ok(())
}

/// Delete the probe build not being measured from the harness-managed plugin
/// folder. Only files with the exact `AEXLO Probe` install names are removed.
fn remove_other_probe(plugin_dir: &Path, keep: Variant) {
	for variant in Variant::ALL {
		if variant == keep {
			continue;
		}
		let path = plugin_dir.join(variant.aex_name());
		match std::fs::remove_file(&path) {
			Ok(()) => println!("{} {} (one probe per trace)", "removed:".dimmed(), path.display()),
			Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
			Err(error) => eprintln!("{} removing {}: {error}", "warn:".yellow(), path.display()),
		}
	}
}

/// Write the configured script, hand it to After Effects, and wait for the
/// result file it writes last.
fn run_script(
	exe: &Path,
	run_dir: &Path,
	config: &Value,
	result_path: &Path,
	trace_path: &Path,
	running: Option<&AeProcess>,
	timeout: Duration,
) -> anyhow::Result<Value> {
	let script_path = run_dir.join("trace.jsx");
	std::fs::write(&script_path, format!("var AEXLO = {config};\n{TRACE_JSX}"))?;
	let _ = std::fs::remove_file(result_path);

	let mut command = Command::new(exe);
	command.arg("-r").arg(&script_path);
	if running.is_none() {
		// Read by the probe when it loads: plugin scanning happens in this
		// very process, so the whole session goes to one fresh file.
		let _ = std::fs::remove_file(trace_path);
		command.env("AEXLO_PROBE_TRACE", trace_path);
	}

	let mut child = command
		.spawn()
		.with_context(|| format!("failed to start {}", exe.display()))?;
	println!("{} {}", "script:".dimmed(), script_path.display());
	println!("{}", "waiting for After Effects…".dimmed());

	let started = Instant::now();
	let result = loop {
		if let Some(result) = read_result(result_path) {
			break result;
		}

		// A forwarder exits at once; only an instance we started ending
		// early means the script never ran.
		if running.is_none()
			&& let Some(status) = child.try_wait()?
		{
			// The script writes the result just before it quits.
			if let Some(result) = read_result(result_path) {
				break result;
			}
			anyhow::bail!(
				"After Effects exited ({status}) without writing {}",
				result_path.display()
			);
		}

		if started.elapsed() > timeout {
			if running.is_none() {
				let _ = child.kill();
			}
			anyhow::bail!(
				"no result from After Effects after {}s; a dialog may be waiting in the After Effects window",
				timeout.as_secs()
			);
		}
		std::thread::sleep(Duration::from_millis(500));
	};

	if running.is_none() {
		// Let After Effects finish quitting so the trace ends with
		// GLOBAL_SETDOWN.
		while child.try_wait()?.is_none() {
			if started.elapsed() > timeout {
				let _ = child.kill();
				eprintln!(
					"{} After Effects did not quit in time; the trace may lack GLOBAL_SETDOWN",
					"warn:".yellow()
				);
				break;
			}
			std::thread::sleep(Duration::from_millis(500));
		}
	}

	println!(
		"{} {:.1}s",
		"after effects done in".dimmed(),
		started.elapsed().as_secs_f64()
	);
	Ok(result)
}

/// The result file, once it is complete JSON. A half-written file reads as
/// `None` and is tried again.
fn read_result(path: &Path) -> Option<Value> {
	let text = std::fs::read_to_string(path).ok()?;
	serde_json::from_str(&text).ok()
}

/// The trace the probe in a running instance writes: the newest
/// `trace-*-pid<pid>.jsonl` in the default directory, filtered to the
/// variant's own name (smart traces carry a `smart-` tag, legacy ones none).
fn running_trace(pid: u32, smart: bool) -> anyhow::Result<PathBuf> {
	let dir = std::env::var_os("AEXLO_PROBE_DIR")
		.map(PathBuf::from)
		.unwrap_or_else(|| std::env::temp_dir().join("aexlo-probe"));
	let suffix = format!("-pid{pid}.jsonl");

	std::fs::read_dir(&dir)
		.with_context(|| format!("failed to list {}", dir.display()))?
		.filter_map(|entry| {
			let entry = entry.ok()?;
			let name = entry.file_name().into_string().ok()?;
			let wanted = if smart {
				name.starts_with("trace-smart-")
			} else {
				name.starts_with("trace-") && !name.starts_with("trace-smart-")
			};
			(wanted && name.ends_with(&suffix))
				.then(|| Some((entry.metadata().ok()?.modified().ok()?, entry.path())))?
		})
		.max_by_key(|(modified, _)| *modified)
		.map(|(_, path)| path)
		.with_context(|| {
			format!(
				"no probe trace for After Effects pid {pid} in {}; was After Effects started with \
				 AEXLO_PROBE_TRACE or AEXLO_PROBE_DIR set?",
				dir.display()
			)
		})
}

fn print_result(result: &Value) {
	if let Some(version) = result["ae_version"].as_str() {
		println!("{} {version}", "ae version:".bold());
	}
	for param in result["params"].as_array().into_iter().flatten() {
		let name = param["name"].as_str().unwrap_or("?");
		if param["ok"] == json!(true) {
			println!("  {} {name} = {}", "✓".green(), param["value"]);
		} else {
			println!(
				"  {} {name}: {}",
				"✗".red(),
				param["error"].as_str().unwrap_or("failed")
			);
		}
	}
	for render in result["renders"].as_array().into_iter().flatten() {
		println!("{} {}", "render:".bold(), render.as_str().unwrap_or("?"));
	}
	if let Some(error) = result["cleanup_error"].as_str() {
		eprintln!("{} cleanup in After Effects failed: {error}", "warn:".yellow());
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn jsx_path_drops_the_verbatim_prefix_and_backslashes() {
		assert_eq!(
			jsx_path(Path::new(r"\\?\F:\work\fixtures\input.png")),
			"F:/work/fixtures/input.png"
		);
		assert_eq!(jsx_path(Path::new(r"C:\a b\c.png")), "C:/a b/c.png");
	}

	#[test]
	fn params_cover_every_probe_control() {
		let names: Vec<_> = params(1920, 1080)
			.as_array()
			.unwrap()
			.iter()
			.map(|p| p["name"].as_str().unwrap().to_string())
			.collect();
		assert_eq!(names, ["Gain", "Invert", "Mode", "Tint", "Angle", "Center"]);
		assert_eq!(params(1920, 1080)[5]["value"], json!([960.0, 540.0]));
	}

	/// ExtendScript is ES3. These would be syntax errors inside After Effects,
	/// where nothing but the result file reports them.
	#[test]
	fn trace_jsx_stays_es3() {
		let code = TRACE_JSX.lines().filter(|line| !line.trim_start().starts_with("//"));
		for (number, line) in code.enumerate() {
			for token in ["let ", "const ", "=>", "`", "JSON."] {
				assert!(
					!line.contains(token),
					"trace.jsx code line {number} uses `{token}`, which ExtendScript lacks: {line}"
				);
			}
		}
	}

	#[test]
	fn read_result_waits_for_complete_json() {
		let dir = std::env::temp_dir().join(format!("aexlo-ae-result-{}", std::process::id()));
		std::fs::create_dir_all(&dir).unwrap();
		let path = dir.join("result.json");

		assert!(read_result(&path).is_none());
		std::fs::write(&path, r#"{"ok": tr"#).unwrap();
		assert!(read_result(&path).is_none());
		std::fs::write(&path, r#"{"ok": true}"#).unwrap();
		assert_eq!(read_result(&path), Some(json!({ "ok": true })));

		let _ = std::fs::remove_dir_all(&dir);
	}
}
