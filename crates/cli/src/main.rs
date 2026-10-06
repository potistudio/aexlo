//! `aexlo` - a command-line front-end for the aexlo plugin loader.
//!
//! Load a real After Effects plugin (`.plugin` bundle on macOS, `.aex`/`.dll`
//! on Windows) outside of After Effects, inspect it, and render frames to PNG.
//!
//! ```text
//! aexlo render <plugin> [--input in.png] [--output out.png] [--set 3=0.5 ...]
//! aexlo about  <plugin>
//! aexlo params <plugin>
//! ```

mod bench;
mod dev;
mod preview;
mod session;
mod toolkit;
mod view;
mod viewer;
mod watch;
mod web;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use aexlo::{ParamValue, PluginInstance};
use anyhow::{Context, Result, bail};

const USAGE: &str = "\
aexlo - run After Effects plugins without After Effects

USAGE:
    aexlo <COMMAND> <plugin> [OPTIONS]

COMMANDS:
    presets [filter]   List the variants the manifest's presets expand to
    test    [filter]   Run each variant in a worker, check it and compare it
                       with its golden; report pass, fail, error, crash,
                       timeout or skipped
        --bless              Write missing or changed goldens (never ones
                             rendered by After Effects)
        --save-frames <dir>  Also write each variant's frame to <dir>
        --fuzz <n>           Instead, render n cases per preset with parameters
                             drawn from their declared ranges; failing cases
                             print as [[preset]] blocks
        --seed <s>           The fuzzing seed  [default: time-based, printed]
    render <plugin>		Render a frame and write it to a PNG
    about  <plugin>		Print the plugin's ABOUT text
    params <plugin>    List the plugin's parameters (index, name, value)
    dev    [filter]     Rerun a #[aexlo::preview] test on save + live viewer
                              (built-in `bacon` replacement; add a name to filter tests)
                              Like `cargo build`: defaults to the crate in the
                              current directory; use -p to pick another.
        -p, --package <name>   Preview this workspace member instead of the
                              crate in the current directory
        --bin                 Skip the test harness: rebuild the crate's cdylib and
                              dlopen + render it directly on save (faster, but no
                              println!/dbg!/debugger support - no filter, either)
        --web                 Preview in the browser instead of a native window:
                              serve the frame over a local HTTP server and stream
                              it into a <canvas>. Requires --bin. Good for
                              headless/remote hosts.
        --port <n>            Port for --web  [default: OS-assigned]
        --preset <name>       (--web) Start the viewer on a manifest preset;
                              its picker lists them all
        --manifest <path>     (--web) The manifest  [default: nearest]
        --strict              (--web) Poison and guard the output, showing
                              unwritten pixels and overwritten guard bands
        --no-open             (--web) Don't open a browser tab
    preview <plugin>   Interactively preview a *built* plugin in the browser:
                       serve it and expose its parameters as live controls.
                       No compiler in the loop - point it at a finished
                       .plugin/.aex/.dll (the `preview` to `dev`'s watch loop).
        -i, --input <png>    Feed a PNG as the effect's input layer
                              [default: the plugin's built-in test frame]
        --watch              Reload the artifact when the file changes on disk
                              (e.g. rebuilt by another toolchain)
        --port <n>           Port for the preview server  [default: OS-assigned]
        --preset <name>      Start on a manifest preset (or a variant id);
                              the viewer's picker lists them all, compares
                              against goldens, depths and render paths, and
                              `Save as preset` appends to the manifest
        --manifest <path>    The manifest  [default: the nearest aexlo.toml]
        --strict             Poison and guard the output while previewing
        --no-open            Don't open a browser tab
    bench   [filter]   Time the manifest's presets (each preset's `bench`
                       samples/warmup), one at a time, with a phase breakdown
                       (pre-render, render, GPU, host overhead)
        --save-baseline <name>  Store the timings in .aexlo/baselines/
        --baseline <name>       Compare with them; a variant whose median
                                and min are both slower than --threshold
                                regresses (exit 1)
        --threshold <p>         [default: 5%]
    bench  <plugin>... Time one or more plugins and rank them by throughput
                       (megapixels/second), then optionally export the numbers.
                       Same measurements as `cargo bench -p aexlo-bench`, driven
                       by flags instead of AEXLO_BENCH_* variables.
        -r, --resolution <spec>  Frame size to measure: a name (512, 720p, 1080p,
                              4k) or <W>x<H>. Repeatable, or comma-separated
                              [default: 1080p]
        -n, --samples <n>    Timed renders per measurement  [default: 30]
        --warmup <n>         Untimed renders first, to pay setup costs [default: 5]
        --mode <m>           Force a render path: auto, cpu, or gpu
                              [default: cpu and gpu for GPU-capable plugins]
        -s, --set <p>=<v>    Set parameter <p> (name or index) to a number
                              before timing (repeatable)
        -i, --input <png>    Input frame, resized to each resolution
                              [default: a synthetic gradient]
        --csv  <path>        Write the results as CSV
        --json <path>        Write the results as JSON
    view   <png>       Live image window: reload a PNG whenever it changes
                       (spawned automatically by a `dev`-driven #[aexlo::preview];
                       pair manually with your own re-runner otherwise)

PRESET OPTIONS (presets, test):
        --manifest <path>  The manifest  [default: the nearest aexlo.toml]
        --depth 8,16       Narrow the matrix to these depths
        --render smart     Narrow the matrix to these render paths
        --isolate <how>    preset (one worker per preset, default), variant,
                           or none (in this process; debugging only)
    -j, --jobs <n>         Workers at once  [default: 1]
        --strict           Every strict check (bounds, allocations, checkouts,
                           flags) on every variant
        --format <f>       human or json  [default: human]
        --junit <path>     Also write a JUnit XML report
    [filter] matches variant ids by substring, or as a glob when it has `*`.
    Exit codes: 0 all passed or skipped, 1 the plugin was judged bad,
    2 invalid manifest or flags, 3 harness error (no verdict reached).

RENDER OPTIONS:
        --preset <name>    Start from a preset (or one variant's id) of the
                           manifest; <plugin> is then optional and the flags
                           below override the preset
    -i, --input  <png>     Feed a PNG as the effect's input layer
    -o, --output <png>     Where to write the rendered frame  [default: out.png]
                           (.exr keeps 32 bpc; 16 bpc writes a 16-bit PNG)
    -s, --set    <p>=<v>   Set parameter <p> (index, #index or name) to <v>
                           before rendering (repeatable)
        --smart            Force the smart-render path
        --legacy           Force the legacy render path
        --serial-iterate   Run the plugin's iterate callbacks on one thread
                           (for plugins whose pixel callbacks call suites,
                           e.g. ones built on the Rust after-effects crate)

<plugin> is a path to the plugin artifact, or a crate directory (with a
Cargo.toml) to build its cdylib and render that - a one-shot alternative to
`dev --bin`. A bare artifact name (no separator) is also tried with the
platform's extension, e.g. `SDK_Noise` -> `SDK_Noise.plugin`.
";

fn main() -> ExitCode {
	let mut args = std::env::args().skip(1);
	let command = args.next();
	// The preset-driven commands report their own exit codes (§12 of
	// docs/toolkit.md): 1 judged bad, 2 invalid, 3 harness error.
	match command.as_deref() {
		Some("presets") => return toolkit::cmd_presets(args),
		Some("test") => return toolkit::cmd_test(args),
		Some("worker") => return toolkit::cmd_worker(),
		Some("bench") => {
			let args: Vec<String> = args.collect();
			let is_plugin = |arg: &str| resolve_plugin(arg).exists() || bench::resolve_fixture(arg).is_some();
			if !toolkit::bench_names_plugins(&args, is_plugin) {
				return toolkit::cmd_bench(args.into_iter());
			}
			return match cmd_bench(args.into_iter()) {
				Ok(()) => ExitCode::SUCCESS,
				Err(err) => {
					eprintln!("error: {err:#}");
					ExitCode::FAILURE
				}
			};
		}
		_ => {}
	}
	match run(command, args) {
		Ok(()) => ExitCode::SUCCESS,
		Err(err) => {
			eprintln!("error: {err:#}");
			ExitCode::FAILURE
		}
	}
}

fn run(command: Option<String>, args: impl Iterator<Item = String>) -> Result<()> {
	let Some(command) = command else {
		print!("{USAGE}");
		bail!("no command given");
	};

	match command.as_str() {
		"-h" | "--help" | "help" => {
			print!("{USAGE}");
			Ok(())
		}
		"about" => cmd_about(args),
		"params" => cmd_params(args),
		"render" => cmd_render(args),
		"bench" => cmd_bench(args),
		"view" => cmd_view(args),
		"dev" => cmd_dev(args),
		"preview" => cmd_preview(args),
		other => {
			print!("{USAGE}");
			bail!("unknown command '{other}'");
		}
	}
}

/// Resolve a user-supplied plugin argument to a loadable path.
///
/// The argument is used as-is if it exists on disk; otherwise, when it looks
/// like a bare name (no path separator), the platform's plugin extension is
/// appended so `aexlo render SDK_Noise` finds `SDK_Noise.plugin`.
fn resolve_plugin(arg: &str) -> PathBuf {
	let direct = PathBuf::from(arg);
	if direct.exists() {
		return direct;
	}
	if !arg.contains(std::path::MAIN_SEPARATOR) {
		let ext = if cfg!(target_os = "windows") { "aex" } else { "plugin" };
		let candidate = PathBuf::from(format!("{arg}.{ext}"));
		if candidate.exists() {
			return candidate;
		}
	}
	direct
}

/// Load a plugin artifact, or - if `plugin_arg` is a crate directory - build
/// its cdylib first. This is what lets `render`/`about`/`params` take either
/// a prebuilt `.plugin`/`.aex`/`.dll` or a crate's source directory directly.
fn load(plugin_arg: &str) -> Result<PluginInstance> {
	let path = resolve_artifact(plugin_arg)?;
	aexlo::Host::get().try_load(&path).with_context(|| format!("loading plugin {}", path.display()))
}

/// Resolve a plugin argument to an artifact on disk, building it first when the
/// argument is a crate directory. This is the path half of [`load`], split out
/// for `bench`, which loads each plugin itself (once per measurement).
fn resolve_artifact(plugin_arg: &str) -> Result<PathBuf> {
	let path = resolve_plugin(plugin_arg);
	if path.is_dir() {
		let manifest = path.join("Cargo.toml");
		if manifest.exists() {
			return watch::build_cdylib(&manifest);
		}
	}
	Ok(path)
}

fn cmd_about(mut args: impl Iterator<Item = String>) -> Result<()> {
	let plugin = args.next().context("about: missing <plugin>")?;

	let mut instance = load(&plugin)?;

	let text = instance.about().context("plugin rejected PF_Cmd_ABOUT")?;
	println!("{}", text.trim());

	Ok(())
}

fn cmd_params(mut args: impl Iterator<Item = String>) -> Result<()> {
	let plugin = args.next().context("params: missing <plugin>")?;
	let instance = load(&plugin)?;
	let values = instance.param_values();
	if values.is_empty() {
		println!("(plugin declares no parameters)");
		return Ok(());
	}
	for (index, value) in values {
		println!("{index:>3}  {:<24}  {}", param_name(&instance, index), describe(&value));
	}
	Ok(())
}

fn cmd_render(args: impl Iterator<Item = String>) -> Result<()> {
	let mut plugin: Option<String> = None;
	let mut input: Option<PathBuf> = None;
	let mut output = PathBuf::from("out.png");
	let mut sets: Vec<(String, String)> = Vec::new();
	let mut force_smart = false;
	let mut force_legacy = false;
	let mut serial_iterate = false;
	let mut preset: Option<String> = None;
	let mut manifest: Option<PathBuf> = None;

	let mut args = args.peekable();
	while let Some(arg) = args.next() {
		match arg.as_str() {
			"-i" | "--input" => input = Some(PathBuf::from(next_value(&mut args, &arg)?)),
			"-o" | "--output" => output = PathBuf::from(next_value(&mut args, &arg)?),
			"-s" | "--set" => sets.push(parse_set(&next_value(&mut args, &arg)?)?),
			"--smart" => force_smart = true,
			"--legacy" => force_legacy = true,
			"--serial-iterate" => serial_iterate = true,
			"--preset" => preset = Some(next_value(&mut args, &arg)?),
			"--manifest" => manifest = Some(PathBuf::from(next_value(&mut args, &arg)?)),
			other if other.starts_with('-') => bail!("unknown option '{other}'"),
			_ => {
				if plugin.replace(arg).is_some() {
					bail!("render: expected a single <plugin>");
				}
			}
		}
	}

	if force_smart && force_legacy {
		bail!("--smart and --legacy are mutually exclusive");
	}

	// A preset sets the stage (plugin, input, size, time, params, depth,
	// render path); the flags below then override it.
	let variant = match &preset {
		Some(name) => Some(preset_variant(manifest.as_deref(), name)?),
		None => None,
	};
	let mut instance = match (&plugin, &variant) {
		(Some(plugin), _) => load(plugin)?,
		(None, Some(variant)) => {
			let artifacts = toolkit::artifacts(std::slice::from_ref(variant)).map_err(anyhow::Error::msg)?;
			let path = artifacts(&variant.plugin).context("no artifact for the preset's plugin")?;
			aexlo::Host::get()
				.try_load(&path)
				.with_context(|| format!("loading plugin {}", path.display()))?
		}
		(None, None) => bail!("render: missing <plugin> (or --preset)"),
	};
	if let Some(variant) = &variant {
		if let Some(reason) = aexlo_harness::apply::unsupported(&instance, variant) {
			bail!("preset {} does not apply: {reason}", variant.id);
		}
		aexlo_harness::apply::configure(&mut instance, variant).map_err(anyhow::Error::msg)?;
	}
	if serial_iterate {
		instance.set_parallel_iterate(false);
	}

	if let Some(path) = &input {
		let layer = aexlo_harness::frame::load_image(path).map_err(anyhow::Error::msg)?;
		let depth = variant.as_ref().map_or(aexlo::PixelDepthKind::U8, |v| v.depth_kind());
		match layer.converted(depth) {
			aexlo::AnyLayer::U8(l) => instance.set_input_layer(l),
			aexlo::AnyLayer::U16(l) => instance.set_input_layer(l),
			aexlo::AnyLayer::F32(l) => instance.set_input_layer(l),
		}
		if variant.as_ref().is_some_and(|v| v.size.is_none()) {
			instance.set_render_size(instance.input_size().0, instance.input_size().1);
		}
	}

	for (key, raw) in &sets {
		let index = resolve_param_key(&instance, key)?;
		let value = parse_param_value(&instance, index, raw)?;
		instance
			.set_param(index, value)
			.with_context(|| format!("setting parameter {key}"))?;
	}
	if !sets.is_empty() {
		let _ = instance.update_params_ui();
	}

	if force_smart {
		instance
			.render_pre()
			.and_then(|()| instance.render_smart())
			.context("smart render failed")?;
	} else if force_legacy {
		instance.render().context("legacy render failed")?;
	} else if let Some(variant) = &variant {
		aexlo_harness::apply::render(&mut instance, variant.render)
			.with_context(|| format!("{} render failed", variant.render.name()))?;
	} else {
		instance.render_frame().context("render failed")?;
	}

	// Deep output keeps its depth when the file can hold it (16-bit PNG,
	// EXR); otherwise it is quantized to an 8-bit PNG.
	let is_exr = output.extension().is_some_and(|e| e.eq_ignore_ascii_case("exr"));
	let frame = aexlo_harness::Frame::new(instance.output().clone());
	if is_exr {
		frame.converted(aexlo::PixelDepthKind::F32).save(&output).map_err(anyhow::Error::msg)?;
	} else if instance.output_depth() == aexlo::PixelDepthKind::U16 {
		frame.save(&output).map_err(anyhow::Error::msg)?;
	} else {
		// save_preview encodes with mtpng (multithreaded), the toolkit's fast
		// 8-bit PNG path.
		aexlo_harness::preview::save_preview(&instance, &output).map_err(anyhow::Error::msg)?;
	}

	let (w, h) = instance.output_size();
	println!("rendered {}x{} -> {}", w, h, output.display());
	Ok(())
}

/// The variant `name` names: a preset with a single variant, or one
/// variant's display id exactly (`dot_glow[depth=16]`).
fn preset_variant(manifest: Option<&Path>, name: &str) -> Result<aexlo_harness::Variant> {
	let manifest = toolkit::load_manifest(manifest).map_err(anyhow::Error::msg)?;
	let preset = name.split('[').next().unwrap_or(name);
	let variants = manifest.preset_variants(preset).map_err(anyhow::Error::msg)?;
	if let Some(exact) = variants.iter().find(|v| v.id.display == name) {
		return Ok(exact.clone());
	}
	match &variants[..] {
		[only] => Ok(only.clone()),
		[first, ..] if name == preset => {
			eprintln!(
				"aexlo render: preset '{preset}' has {} variants; rendering {} (name one exactly to pick another)",
				variants.len(),
				first.id
			);
			Ok(first.clone())
		}
		_ => bail!(
			"no variant '{name}' (variants: {})",
			variants.iter().map(|v| v.id.display.as_str()).collect::<Vec<_>>().join(", ")
		),
	}
}

/// A `--set` key: a plain index (`3`), `#3`, or a declared name.
fn resolve_param_key(instance: &PluginInstance, key: &str) -> Result<usize> {
	if let Ok(index) = key.trim().parse::<usize>() {
		return Ok(index);
	}
	aexlo_harness::apply::resolve_param(instance, key.trim()).map_err(anyhow::Error::msg)
}

fn cmd_bench(args: impl Iterator<Item = String>) -> Result<()> {
	let options = bench::parse_args(args, |spec| {
		let path = resolve_artifact(spec)?;
		if path.exists() {
			return Ok(path);
		}
		// Last resort: a bare name may be one of the bundled bench fixtures, so
		// `aexlo bench SDK_Noise` works like the `cargo bench` harness does.
		bench::resolve_fixture(spec).with_context(|| format!("no plugin found for '{spec}'"))
	})?;
	bench::run(options)
}

fn cmd_view(mut args: impl Iterator<Item = String>) -> Result<()> {
	let path = args.next().context("view: missing <png> path")?;
	view::run(Path::new(&path))
}

fn cmd_dev(args: impl Iterator<Item = String>) -> Result<()> {
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
			"--preset" => preset = Some(next_value(&mut args, &arg)?),
			"--manifest" => manifest_path = Some(PathBuf::from(next_value(&mut args, &arg)?)),
			"--strict" => strict = true,
			"--no-open" => no_open = true,
			"--port" => {
				port = next_value(&mut args, &arg)?
					.parse()
					.with_context(|| "--port expects a number 0-65535".to_string())?;
			}
			"-p" | "--package" => package = Some(next_value(&mut args, &arg)?),
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

	let manifest = resolve_manifest(package.as_deref())?;

	if bin_mode {
		if filter.is_some() {
			bail!("dev --bin: no test filter - it doesn't run the test harness");
		}

		if web_mode {
			web::run(
				&manifest,
				web::Options {
					port,
					manifest: manifest_path.as_deref(),
					preset: preset.as_deref(),
					strict,
					no_open,
				},
			)
		} else {
			watch::run(&manifest)
		}
	} else {
		dev::run(&manifest, filter.as_deref())
	}
}

fn cmd_preview(args: impl Iterator<Item = String>) -> Result<()> {
	let mut plugin: Option<String> = None;
	let mut input: Option<PathBuf> = None;
	let mut port: u16 = 0;
	let mut watch = false;
	let mut preset: Option<String> = None;
	let mut manifest: Option<PathBuf> = None;
	let mut strict = false;
	let mut no_open = false;

	let mut args = args.peekable();
	while let Some(arg) = args.next() {
		match arg.as_str() {
			"-i" | "--input" => input = Some(PathBuf::from(next_value(&mut args, &arg)?)),
			"--watch" => watch = true,
			"--preset" => preset = Some(next_value(&mut args, &arg)?),
			"--manifest" => manifest = Some(PathBuf::from(next_value(&mut args, &arg)?)),
			"--strict" => strict = true,
			"--no-open" => no_open = true,
			"--port" => {
				port = next_value(&mut args, &arg)?
					.parse()
					.with_context(|| "--port expects a number 0-65535".to_string())?;
			}
			other if other.starts_with('-') => bail!("unknown option '{other}'"),
			_ => {
				if plugin.replace(arg).is_some() {
					bail!("preview: expected a single <plugin>");
				}
			}
		}
	}

	let plugin = plugin.context("preview: missing <plugin>")?;
	let path = resolve_plugin(&plugin);
	preview::run(
		&path,
		preview::Options {
			input: input.as_deref(),
			port,
			watch,
			manifest: manifest.as_deref(),
			preset: preset.as_deref(),
			strict,
			no_open,
		},
	)
}

/// Resolve `-p <package>` (or, if absent, the crate in the current directory)
/// to its `Cargo.toml`, the same way `cargo build [-p <package>]` would.
fn resolve_manifest(package: Option<&str>) -> Result<PathBuf> {
	// No `.no_deps()`: `root_package()` needs the resolve graph cargo omits
	// without it, and dev-only usage doesn't need this to be instant.
	let metadata = cargo_metadata::MetadataCommand::new()
		.exec()
		.context("running cargo metadata")?;

	let pkg = if let Some(name) = package {
		metadata
			.packages
			.iter()
			.find(|p| p.name.as_str() == name)
			.with_context(|| format!("no package named '{name}' in this workspace"))?
	} else {
		metadata
			.root_package()
			.context("no crate in the current directory - pass -p <package>")?
	};
	Ok(pkg.manifest_path.clone().into_std_path_buf())
}

/// Pull the value that follows a flag like `--output`, erroring if it's missing.
fn next_value(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String> {
	args.next().with_context(|| format!("option '{flag}' needs a value"))
}

/// Split a `--set` argument of the form `<name|index>=<value>`.
fn parse_set(raw: &str) -> Result<(String, String)> {
	let (key, value) = raw
		.split_once('=')
		.with_context(|| format!("--set expects <name|index>=<value>, got '{raw}'"))?;
	if key.trim().is_empty() {
		bail!("--set: empty parameter name in '{raw}'");
	}
	Ok((key.trim().to_string(), value.to_string()))
}

/// Parse a textual value into the `ParamValue` variant the plugin already uses
/// at `index`, so callers don't have to spell out the parameter type.
pub(crate) fn parse_param_value(instance: &PluginInstance, index: usize, raw: &str) -> Result<ParamValue> {
	let current = instance
		.get_param(index)
		.with_context(|| format!("no parameter at index {index}"))?;
	let value = match current {
		ParamValue::Float(_) => ParamValue::Float(raw.parse().with_context(|| bad(raw, "a number"))?),
		ParamValue::Fixed(_) => ParamValue::Fixed(raw.parse().with_context(|| bad(raw, "a number"))?),
		ParamValue::Slider(_) => ParamValue::Slider(raw.parse().with_context(|| bad(raw, "an integer"))?),
		ParamValue::Popup(_) => ParamValue::Popup(raw.parse().with_context(|| bad(raw, "an integer"))?),
		ParamValue::Angle(_) => ParamValue::Angle(raw.parse().with_context(|| bad(raw, "a number"))?),
		ParamValue::Checkbox(_) => ParamValue::Checkbox(parse_bool(raw)?),
		ParamValue::Point { .. } => {
			let (x, y) = raw.split_once(',').with_context(|| bad(raw, "'x,y'"))?;
			ParamValue::Point {
				x: x.trim().parse().with_context(|| bad(raw, "'x,y'"))?,
				y: y.trim().parse().with_context(|| bad(raw, "'x,y'"))?,
			}
		}
		ParamValue::Color { .. } => parse_color(raw)?,
		ParamValue::Path(_) => ParamValue::Path(raw.parse().with_context(|| bad(raw, "a mask id"))?),
		ParamValue::Point3D { .. } => {
			let parts: Vec<f64> = raw
				.split(',')
				.map(|v| v.trim().parse())
				.collect::<Result<_, _>>()
				.with_context(|| bad(raw, "'x,y,z'"))?;
			let [x, y, z] = parts[..] else {
				bail!("{}", bad(raw, "'x,y,z'"));
			};
			ParamValue::Point3D { x, y, z }
		}
	};
	Ok(value)
}

fn parse_bool(raw: &str) -> Result<bool> {
	match raw.trim().to_ascii_lowercase().as_str() {
		"true" => Ok(true),
		"false" => Ok(false),
		_ => bail!("expected a boolean (true/false), got '{raw}'"),
	}
}

/// Parse an `r,g,b` or `r,g,b,a` color (0-255 per channel).
fn parse_color(raw: &str) -> Result<ParamValue> {
	let parts: Vec<&str> = raw.split(',').map(str::trim).collect();
	if parts.len() != 3 && parts.len() != 4 {
		bail!("color expects 'r,g,b' or 'r,g,b,a', got '{raw}'");
	}
	let channel = |s: &str| -> Result<u8> { s.parse().with_context(|| bad(raw, "0-255 channels")) };
	Ok(ParamValue::Color {
		red: channel(parts[0])?,
		green: channel(parts[1])?,
		blue: channel(parts[2])?,
		alpha: if parts.len() == 4 { channel(parts[3])? } else { 255 },
	})
}

fn bad(raw: &str, expected: &str) -> String {
	format!("'{raw}' is not {expected}")
}

/// Load a PNG as an RGBA8 buffer plus its dimensions.
pub(crate) fn load_input(path: &Path) -> Result<(Vec<u8>, u32, u32)> {
	let img = image::open(path)
		.with_context(|| format!("opening input {}", path.display()))?
		.to_rgba8();
	let (w, h) = img.dimensions();
	Ok((img.into_raw(), w, h))
}

/// The plugin-declared name for a parameter, falling back to a positional label.
fn param_name(instance: &PluginInstance, index: usize) -> String {
	instance
		.param_name(index)
		.filter(|name| !name.is_empty())
		.unwrap_or_else(|| format!("Param {index}"))
}

/// Render a `ParamValue` for the `params` listing.
fn describe(value: &ParamValue) -> String {
	match value {
		ParamValue::Float(v) => format!("float    {v}"),
		ParamValue::Fixed(v) => format!("fixed    {v}"),
		ParamValue::Slider(v) => format!("slider   {v}"),
		ParamValue::Popup(v) => format!("popup    {v}"),
		ParamValue::Angle(v) => format!("angle    {v}"),
		ParamValue::Checkbox(v) => format!("checkbox {v}"),
		ParamValue::Point { x, y } => format!("point    {x},{y}"),
		ParamValue::Color {
			red,
			green,
			blue,
			alpha,
		} => format!("color    {red},{green},{blue},{alpha}"),
		ParamValue::Path(id) => format!("path     {id}"),
		ParamValue::Point3D { x, y, z } => format!("point3d  {x},{y},{z}"),
	}
}
