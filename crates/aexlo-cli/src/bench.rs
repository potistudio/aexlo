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

use aexlo_bench::report::{
	Measurement, print_leaderboard, print_speedups, sort_by_throughput, to_csv, to_json, write_file,
};
use aexlo_bench::{MeasureOptions, ParamConfig, RenderMode, Resolution, bench_modes, measure, param_config_label};
use aexlo_harness::Error;
use anyhow::{Context, Result, bail};

use crate::args::resolve_artifact;
use crate::toolkit;

#[derive(clap::Args)]
pub struct BenchArgs {
	/// Plugins (or crate directories) to time, or else one filter over the
	/// manifest's variants
	#[arg(value_name = "PLUGIN|FILTER")]
	targets: Vec<String>,
	#[command(flatten)]
	plugin: PluginArgs,
	#[command(flatten)]
	own: toolkit::BenchPresetArgs,
	#[command(flatten)]
	preset: toolkit::PresetArgs,
}

/// The flags for timing plugins.
#[derive(clap::Args)]
#[command(next_help_heading = "Plugin options")]
struct PluginArgs {
	/// Frame size: a name (512, 720p, 1080p, 4k) or <W>x<H>. Repeatable, or
	/// comma-separated [default: 1080p]
	#[arg(short, long, value_name = "SPEC", value_delimiter = ',', value_parser = parse_resolution)]
	resolution: Vec<Resolution>,
	/// Timed renders per measurement [default: 30]
	#[arg(short = 'n', long, value_name = "N", value_parser = parse_samples)]
	samples: Option<usize>,
	/// Untimed renders first, to pay setup costs [default: 5]
	#[arg(long, value_name = "N")]
	warmup: Option<usize>,
	/// Force a render path: auto, cpu or gpu [default: cpu and gpu for
	/// GPU-capable plugins]
	#[arg(long, value_name = "M", value_parser = parse_mode)]
	mode: Option<RenderMode>,
	/// Set parameter <P> (name or index) to a number before timing
	/// (repeatable)
	#[arg(short, long = "set", value_name = "P=V", value_parser = parse_set, allow_hyphen_values = true)]
	sets: Vec<(String, f64)>,
	/// Input frame, resized to each resolution [default: a synthetic
	/// gradient]
	#[arg(short, long, value_name = "PNG")]
	input: Option<PathBuf>,
	/// Also write the results as CSV
	#[arg(long, value_name = "PATH")]
	csv: Option<PathBuf>,
}

impl PluginArgs {
	fn is_given(&self) -> bool {
		!self.resolution.is_empty()
			|| self.samples.is_some()
			|| self.warmup.is_some()
			|| self.mode.is_some()
			|| !self.sets.is_empty()
			|| self.input.is_some()
			|| self.csv.is_some()
	}
}

/// `aexlo bench`: the plugins named, else the manifest's presets. The first
/// argument names a plugin when it exists on disk or as a fixture, and no
/// flag only presets take is given.
pub fn cmd(args: BenchArgs) -> u8 {
	let BenchArgs {
		mut targets,
		plugin,
		own,
		preset,
	} = args;
	let is_plugin = |arg: &str| crate::args::resolve_plugin(arg).exists() || resolve_fixture(arg).is_some();
	let names_plugins =
		!own.is_given() && !preset.names_presets() && targets.first().is_some_and(|first| is_plugin(first));
	if names_plugins {
		if preset.is_given() {
			return toolkit::fail(&Error::invalid(
				"--isolate, --jobs, --strict and --junit only apply to timing presets",
			));
		}
		let json = preset.format() == toolkit::Format::Json;
		return crate::report(bench_plugins(&targets, plugin, json));
	}
	if plugin.is_given() {
		let why = match targets.first() {
			Some(first) => format!("no plugin at '{first}'"),
			None => "no plugin named".to_string(),
		};
		return toolkit::fail(&Error::invalid(format!(
			"the plugin options only apply to timing plugins ({why})"
		)));
	}
	if targets.len() > 1 {
		return toolkit::fail(&Error::invalid(format!(
			"no plugin at '{}', and presets take one filter",
			targets[0]
		)));
	}
	toolkit::cmd_bench(targets.pop(), own, preset)
}

fn bench_plugins(specs: &[String], args: PluginArgs, json: bool) -> Result<()> {
	let mut resolutions = args.resolution;
	if resolutions.is_empty() {
		// One mid-size frame by default: enough work to be meaningful, quick
		// enough that a bare `aexlo bench <plugin>` returns promptly.
		resolutions.push(aexlo_bench::parse_resolution("1080p").map_err(anyhow::Error::msg)?);
	}
	if let Some(path) = &args.input
		&& !path.exists()
	{
		bail!("input {} does not exist", path.display());
	}

	let mut plugins = Vec::with_capacity(specs.len());
	for spec in specs {
		let path = resolve_artifact(spec)?;
		let path = if path.exists() {
			path
		} else {
			// Last resort: a bare name may be one of the bundled bench fixtures, so
			// `aexlo bench SDK_Noise` works like the `cargo bench` harness does.
			resolve_fixture(spec).with_context(|| format!("no plugin found for '{spec}'"))?
		};
		let label = path
			.file_stem()
			.map(|s| s.to_string_lossy().into_owned())
			.unwrap_or_else(|| spec.clone());
		plugins.push((label, path));
	}

	run(Options {
		plugins,
		resolutions,
		config: args.sets,
		input: args.input,
		samples: args.samples.unwrap_or(30),
		warmup: args.warmup.unwrap_or(5),
		mode: args.mode,
		csv: args.csv,
		json,
	})
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
	/// Print the results as JSON on stdout instead of the leaderboard.
	pub json: bool,
}

fn run(options: Options) -> Result<()> {
	let config_label = param_config_label(&options.config);
	eprintln!(
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
	if options.json {
		println!("{}", to_json(&measurements).trim_end());
	} else {
		print_leaderboard(&measurements);
		print_speedups(&measurements);
	}

	if let Some(path) = &options.csv {
		if options.json {
			// stdout holds only the JSON.
			std::fs::write(path, to_csv(&measurements)).with_context(|| format!("writing {}", path.display()))?;
			eprintln!("Wrote {}", path.display());
		} else {
			println!();
			write_file(path, &to_csv(&measurements));
		}
	}

	Ok(())
}

/// Parse `bench`'s arguments. `resolve` turns a plugin argument into an artifact
/// path (building a crate directory when needed), matching `render`'s behavior.
fn parse_resolution(raw: &str) -> Result<Resolution, String> {
	aexlo_bench::parse_resolution(raw)
}

fn parse_samples(raw: &str) -> Result<usize, String> {
	match raw.parse() {
		Ok(0) => Err("must be at least 1".to_string()),
		Ok(n) => Ok(n),
		Err(_) => Err(format!("expects a number, got '{raw}'")),
	}
}

fn parse_mode(raw: &str) -> Result<RenderMode, String> {
	match raw.trim().to_ascii_lowercase().as_str() {
		"auto" => Ok(RenderMode::Auto),
		"cpu" => Ok(RenderMode::Cpu),
		"gpu" => Ok(RenderMode::Gpu),
		other => Err(format!("'{other}' is not auto, cpu or gpu")),
	}
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
