//! `aexlo bench` - time one or more plugins and rank them by throughput.
//!
//! The measurement machinery lives in `aexlo-bench` and is shared with
//! `cargo bench -p aexlo-bench`; this module is the argument-driven front-end.
//! The `cargo bench` targets are configured through `AEXLO_BENCH_*` environment
//! variables, which is fine for CI but awkward interactively - here every knob
//! is a flag, and a plugin path can be a crate directory that gets built first
//! (the same resolution `render` does).
//!
//! `aexlo bench [filter]`, which times a manifest's presets instead, lives in
//! [`crate::toolkit`]; [`cmd`] picks between the two.

use std::path::PathBuf;
use std::process::ExitCode;

use aexlo_bench::report::{
	Measurement, print_leaderboard, print_speedups, sort_by_throughput, to_csv, to_json, write_file,
};
use aexlo_bench::{MeasureOptions, ParamConfig, RenderMode, Resolution, bench_modes, measure, param_config_label};
use anyhow::{Context, Result, bail};

use crate::args::{next_value, resolve_artifact};
use crate::toolkit;

pub const HELP: &str = "\
Usage: aexlo bench <plugin>... [OPTIONS]
       aexlo bench [filter] [PRESET OPTIONS]

With plugins: time each one and rank them by throughput (megapixels per
second), then optionally export the numbers. Same measurements as
`cargo bench -p aexlo-bench`, driven by flags instead of AEXLO_BENCH_*
variables.

Options:
  -r, --resolution <spec> Frame size: a name (512, 720p, 1080p, 4k) or <W>x<H>.
                          Repeatable, or comma-separated  [default: 1080p]
  -n, --samples <n>       Timed renders per measurement  [default: 30]
      --warmup <n>        Untimed renders first, to pay setup costs
                          [default: 5]
      --mode <m>          Force a render path: auto, cpu or gpu
                          [default: cpu and gpu for GPU-capable plugins]
  -s, --set <p>=<v>       Set parameter <p> (name or index) to a number
                          before timing (repeatable)
  -i, --input <png>       Input frame, resized to each resolution
                          [default: a synthetic gradient]
      --csv <path>        Write the results as CSV
      --json <path>       Write the results as JSON

Without plugins: time the manifest's presets (each preset's `bench` samples
and warmup), one at a time, with a phase breakdown (pre-render, render, GPU,
host overhead).

Options:
      --save-baseline <name>  Store the timings in .aexlo/baselines/
      --baseline <name>       Compare with them; a variant whose median and
                              min are both slower than --threshold regresses
                              (exit 1)
      --threshold <p>         [default: 5%]
";

/// `aexlo bench`: the plugins named, else the manifest's presets.
pub fn cmd(args: impl Iterator<Item = String>) -> ExitCode {
	let args: Vec<String> = args.collect();
	let is_plugin = |arg: &str| crate::args::resolve_plugin(arg).exists() || resolve_fixture(arg).is_some();
	if !toolkit::bench_names_plugins(&args, is_plugin) {
		return toolkit::cmd_bench(args.into_iter());
	}
	crate::report(bench_plugins(args.into_iter()))
}

fn bench_plugins(args: impl Iterator<Item = String>) -> Result<()> {
	let options = parse_args(args, |spec| {
		let path = resolve_artifact(spec)?;
		if path.exists() {
			return Ok(path);
		}
		// Last resort: a bare name may be one of the bundled bench fixtures, so
		// `aexlo bench SDK_Noise` works like the `cargo bench` harness does.
		resolve_fixture(spec).with_context(|| format!("no plugin found for '{spec}'"))
	})?;
	run(options)
}

/// Everything `bench` needs, resolved from the command line.
pub struct Options {
	/// Plugin artifacts to time, as `(label, path)`.
	pub plugins: Vec<(String, PathBuf)>,
	pub resolutions: Vec<Resolution>,
	/// Parameters to apply before timing, `(name-or-index, value)`.
	pub config: ParamConfig,
	pub input: Option<PathBuf>,
	pub samples: usize,
	pub warmup: usize,
	/// Explicit render path; `None` measures cpu and gpu for GPU-capable plugins.
	pub mode: Option<RenderMode>,
	pub csv: Option<PathBuf>,
	pub json: Option<PathBuf>,
}

fn run(options: Options) -> Result<()> {
	let config_label = param_config_label(&options.config);
	println!(
		"aexlo bench: {} plugin(s) x {} resolution(s), params={config_label}, {} samples ({} warmup)\n",
		options.plugins.len(),
		options.resolutions.len(),
		options.samples,
		options.warmup,
	);

	let mut measurements = Vec::new();
	for (label, path) in &options.plugins {
		// Probe once to learn which render paths this plugin actually has, so a
		// GPU-less effect isn't reported as a failed `gpu` row.
		let modes = match options.mode {
			Some(mode) => vec![mode],
			None => match aexlo::Host::get().load(path) {
				Ok(probe) => bench_modes(&probe),
				Err(err) => {
					eprintln!("aexlo bench: {label}: load failed, skipping: {err:?}");
					continue;
				}
			},
		};

		for &resolution in &options.resolutions {
			for &mode in &modes {
				let measure_options = MeasureOptions {
					resolution,
					config: &options.config,
					input: options.input.as_deref(),
					samples: options.samples,
					warmup: options.warmup,
				};
				match measure(path, mode, measure_options) {
					Ok((timing, caps)) => {
						let row = Measurement {
							plugin: label.clone(),
							mode: mode.label(),
							caps,
							config: config_label.clone(),
							resolution,
							timing,
						};
						eprintln!(
							"  measured {label} [{}] @ {} -> {:.1} Mpx/s",
							mode.label(),
							resolution.name,
							row.mpx_per_s(),
						);
						measurements.push(row);
					}
					// A skipped point is normal (a plugin may refuse 4k, or have no
					// GPU path); keep going and report what we did get.
					Err(err) => eprintln!(
						"aexlo bench: {label} [{}] @ {}: skipped ({err})",
						mode.label(),
						resolution.name
					),
				}
			}
		}
	}

	if measurements.is_empty() {
		bail!("no measurements collected");
	}

	sort_by_throughput(&mut measurements);
	print_leaderboard(&measurements);
	print_speedups(&measurements);

	if options.csv.is_some() || options.json.is_some() {
		println!();
	}
	if let Some(path) = &options.csv {
		write_file(path, &to_csv(&measurements));
	}
	if let Some(path) = &options.json {
		write_file(path, &to_json(&measurements));
	}

	Ok(())
}

/// Parse `bench`'s arguments. `resolve` turns a plugin argument into an artifact
/// path (building a crate directory when needed), matching `render`'s behavior.
fn parse_args(args: impl Iterator<Item = String>, resolve: impl Fn(&str) -> Result<PathBuf>) -> Result<Options> {
	let mut specs: Vec<String> = Vec::new();
	let mut resolutions: Vec<Resolution> = Vec::new();
	let mut config: ParamConfig = Vec::new();
	let mut input: Option<PathBuf> = None;
	let mut samples: usize = 30;
	let mut warmup: usize = 5;
	let mut mode: Option<RenderMode> = None;
	let mut csv: Option<PathBuf> = None;
	let mut json: Option<PathBuf> = None;

	let mut args = args.peekable();
	while let Some(arg) = args.next() {
		match arg.as_str() {
			"-r" | "--resolution" => {
				let raw = next_value(&mut args, &arg)?;
				for spec in raw.split(',').filter(|s| !s.trim().is_empty()) {
					let resolution = aexlo_bench::parse_resolution(spec).map_err(anyhow::Error::msg)?;
					resolutions.push(resolution);
				}
			}
			"-n" | "--samples" => samples = parse_count(&next_value(&mut args, &arg)?, &arg)?,
			"--warmup" => {
				warmup = next_value(&mut args, &arg)?
					.parse()
					.with_context(|| format!("option '{arg}' expects a number"))?;
			}
			"-s" | "--set" => config.push(parse_set(&next_value(&mut args, &arg)?)?),
			"-i" | "--input" => input = Some(PathBuf::from(next_value(&mut args, &arg)?)),
			"--mode" => mode = Some(parse_mode(&next_value(&mut args, &arg)?)?),
			"--csv" => csv = Some(PathBuf::from(next_value(&mut args, &arg)?)),
			"--json" => json = Some(PathBuf::from(next_value(&mut args, &arg)?)),
			other if other.starts_with('-') => bail!("unknown option '{other}'"),
			_ => specs.push(arg),
		}
	}

	if specs.is_empty() {
		bail!("bench: missing <plugin> (pass one or more plugin paths or crate directories)");
	}
	if samples == 0 {
		bail!("--samples must be at least 1");
	}
	if resolutions.is_empty() {
		// One mid-size frame by default: enough work to be meaningful, quick
		// enough that a bare `aexlo bench <plugin>` returns promptly.
		resolutions.push(aexlo_bench::parse_resolution("1080p").map_err(anyhow::Error::msg)?);
	}
	if let Some(path) = &input
		&& !path.exists()
	{
		bail!("input {} does not exist", path.display());
	}

	let mut plugins = Vec::with_capacity(specs.len());
	for spec in &specs {
		let path = resolve(spec)?;
		let label = path
			.file_stem()
			.map(|s| s.to_string_lossy().into_owned())
			.unwrap_or_else(|| spec.clone());
		plugins.push((label, path));
	}

	Ok(Options {
		plugins,
		resolutions,
		config,
		input,
		samples,
		warmup,
		mode,
		csv,
		json,
	})
}

fn parse_mode(raw: &str) -> Result<RenderMode> {
	match raw.trim().to_ascii_lowercase().as_str() {
		"auto" => Ok(RenderMode::Auto),
		"cpu" => Ok(RenderMode::Cpu),
		"gpu" => Ok(RenderMode::Gpu),
		other => bail!("unknown --mode '{other}' (expected auto, cpu, or gpu)"),
	}
}

fn parse_count(raw: &str, flag: &str) -> Result<usize> {
	raw.parse()
		.with_context(|| format!("option '{flag}' expects a number, got '{raw}'"))
}

/// Split a `--set` argument of the form `<name|index>=<number>`.
///
/// Unlike `render`'s `--set`, values are always numeric: the sweep coerces a
/// scalar to whatever type the parameter declares, so points and colors aren't
/// benchmarkable this way.
fn parse_set(raw: &str) -> Result<(String, f64)> {
	let (name, value) = raw
		.split_once('=')
		.with_context(|| format!("--set expects <name|index>=<value>, got '{raw}'"))?;
	let name = name.trim();
	if name.is_empty() {
		bail!("--set: empty parameter name in '{raw}'");
	}
	let value: f64 = value
		.trim()
		.parse()
		.with_context(|| format!("--set expects a numeric value, got '{value}'"))?;
	Ok((name.to_string(), value))
}

/// Fixture-name fallback: `aexlo bench SDK_Noise` should find the bundled
/// fixture the same way the `cargo bench` harness does, so the two front-ends
/// accept the same plugin names.
fn resolve_fixture(spec: &str) -> Option<PathBuf> {
	let path = aexlo_bench::resolve_plugin(spec);
	path.exists().then_some(path)
}
