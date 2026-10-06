//! The preset-driven commands (`docs/toolkit.md` §12): `presets`, `test`,
//! `worker`, and the manifest plumbing they share.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use aexlo_harness::manifest::{self, Manifest};
use aexlo_harness::runner::{self, Isolate, Outcome, Report, Run, RunnerConfig};
use aexlo_harness::worker::WorkerCommand;
use aexlo_harness::{Error, PluginRef, PluginSource, RenderMode, Variant};

/// How results are printed.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Format {
	#[default]
	Human,
	Json,
}

/// Flags every preset-driven command takes.
#[derive(Default)]
pub struct Common {
	pub manifest: Option<PathBuf>,
	pub depth: Option<Vec<u32>>,
	pub render: Option<Vec<RenderMode>>,
	pub isolate: Isolate,
	pub jobs: usize,
	pub strict: bool,
	pub format: Format,
	pub junit: Option<PathBuf>,
	pub filter: Option<String>,
}

/// Arguments left after [`Common`] took its own.
pub struct Parsed {
	pub common: Common,
	/// Command-specific flags with their values, in order.
	pub rest: Vec<(String, Option<String>)>,
}

/// Parse `args` into common flags, the filter, and the flags listed in
/// `own` (with whether each takes a value).
pub fn parse(args: impl Iterator<Item = String>, own: &[(&str, bool)]) -> Result<Parsed, Error> {
	let mut common = Common {
		jobs: 1,
		..Common::default()
	};
	let mut rest = Vec::new();
	let mut args = args.peekable();
	let value = |args: &mut std::iter::Peekable<_>, flag: &str| -> Result<String, Error> {
		Iterator::next(args).ok_or_else(|| Error::invalid(format!("option '{flag}' needs a value")))
	};
	while let Some(arg) = args.next() {
		match arg.as_str() {
			"--manifest" => common.manifest = Some(PathBuf::from(value(&mut args, &arg)?)),
			"--depth" => {
				let list = value(&mut args, &arg)?
					.split(',')
					.map(|d| {
						d.trim()
							.parse::<u32>()
							.ok()
							.filter(|d| [8, 16, 32].contains(d))
							.ok_or_else(|| Error::invalid(format!("--depth: '{d}' is not 8, 16 or 32")))
					})
					.collect::<Result<_, _>>()?;
				common.depth = Some(list);
			}
			"--render" => {
				let list = value(&mut args, &arg)?
					.split(',')
					.map(RenderMode::parse)
					.collect::<Result<_, _>>()?;
				common.render = Some(list);
			}
			"--isolate" => common.isolate = Isolate::parse(&value(&mut args, &arg)?)?,
			"--jobs" | "-j" => {
				let raw = value(&mut args, &arg)?;
				common.jobs = raw
					.parse()
					.ok()
					.filter(|j| *j > 0)
					.ok_or_else(|| Error::invalid(format!("--jobs expects a positive number, got '{raw}'")))?;
			}
			"--strict" => common.strict = true,
			"--format" => {
				common.format = match value(&mut args, &arg)?.as_str() {
					"human" => Format::Human,
					"json" => Format::Json,
					other => return Err(Error::invalid(format!("--format: '{other}' is not human or json"))),
				}
			}
			"--junit" => common.junit = Some(PathBuf::from(value(&mut args, &arg)?)),
			flag if flag.starts_with('-') => match own.iter().find(|(name, _)| *name == flag) {
				Some((_, true)) => {
					let v = value(&mut args, flag)?;
					rest.push((arg, Some(v)));
				}
				Some((_, false)) => rest.push((arg, None)),
				None => return Err(Error::invalid(format!("unknown option '{flag}'"))),
			},
			_ if common.filter.is_none() => common.filter = Some(arg),
			_ => return Err(Error::invalid(format!("unexpected argument '{arg}'"))),
		}
	}
	Ok(Parsed { common, rest })
}

/// The manifest named by `--manifest`, else the nearest `aexlo.toml`.
pub fn load_manifest(path: Option<&Path>) -> Result<Manifest, Error> {
	let path = match path {
		Some(path) => path.to_path_buf(),
		None => {
			let cwd =
				std::env::current_dir().map_err(|e| Error::harness(format!("reading the current directory: {e}")))?;
			manifest::discover(&cwd).ok_or_else(|| {
				Error::invalid(format!(
					"no {} in {} or its parents (pass --manifest <path>)",
					manifest::FILE_NAME,
					cwd.display()
				))
			})?
		}
	};
	Manifest::load(&path)
}

/// The manifest's variants matching the filter, narrowed by `--depth` and
/// `--render`.
pub fn select(manifest: &Manifest, common: &Common) -> Result<Vec<Variant>, Error> {
	Ok(manifest
		.filtered(common.filter.as_deref())?
		.into_iter()
		.filter(|v| common.depth.as_ref().is_none_or(|d| d.contains(&v.depth)))
		.filter(|v| common.render.as_ref().is_none_or(|r| r.contains(&v.render)))
		.collect())
}

/// Build every crate the variants name and map each plugin to its artifact.
///
/// # Errors
/// A failing `cargo build` is a harness error (§12: exit 3).
pub fn artifacts(variants: &[Variant]) -> Result<runner::ArtifactMap, Error> {
	let mut map: HashMap<PluginRef, PathBuf> = HashMap::new();
	for variant in variants {
		if map.contains_key(&variant.plugin) {
			continue;
		}
		let path = match &variant.plugin.source {
			PluginSource::Artifact(path) => path.clone(),
			PluginSource::Crate(dir) => {
				let cargo_toml = dir.join("Cargo.toml");
				eprintln!("aexlo: building {}", dir.display());
				crate::watch::build_cdylib(&cargo_toml)
					.map_err(|e| Error::harness(format!("building {}: {e:#}", dir.display())))?
			}
		};
		map.insert(variant.plugin.clone(), path);
	}
	Ok(Arc::new(move |plugin: &PluginRef| map.get(plugin).cloned()))
}

/// The runner configuration for `common`.
pub fn runner_config(common: &Common, artifacts: runner::ArtifactMap) -> Result<RunnerConfig, Error> {
	Ok(RunnerConfig {
		isolate: common.isolate,
		jobs: common.jobs,
		worker: WorkerCommand::current_exe()?,
		artifacts,
	})
}

/// Exit with the code `err` maps to, after printing it.
pub fn fail(err: &Error) -> ExitCode {
	eprintln!("error: {err}");
	ExitCode::from(err.exit_code())
}

//==== aexlo presets ====================================================

pub fn cmd_presets(args: impl Iterator<Item = String>) -> ExitCode {
	let run = || -> Result<(), Error> {
		let parsed = parse(args, &[])?;
		let manifest = load_manifest(parsed.common.manifest.as_deref())?;
		let variants = select(&manifest, &parsed.common)?;
		if parsed.common.format == Format::Json {
			let ids: Vec<_> = variants.iter().map(|v| &v.id).collect();
			println!("{}", serde_json::to_string_pretty(&ids).unwrap_or_default());
			return Ok(());
		}
		for v in &variants {
			let size = v.size.map_or("input size".to_string(), |[w, h]| format!("{w}x{h}"));
			println!(
				"{:<48} {:>4} bpc  {:<6}  {size}  frame {} @ {} fps",
				v.id.display,
				v.depth,
				v.render.name(),
				v.time.frame,
				v.time.fps
			);
		}
		let hidden: Vec<&str> = manifest.preset_names().filter(|n| manifest.is_hidden(n)).collect();
		eprintln!(
			"{} variant(s){}",
			variants.len(),
			if hidden.is_empty() {
				String::new()
			} else {
				format!(" (hidden bases: {})", hidden.join(", "))
			}
		);
		Ok(())
	};
	match run() {
		Ok(()) => ExitCode::SUCCESS,
		Err(err) => fail(&err),
	}
}

//==== aexlo worker =====================================================

pub fn cmd_worker() -> ExitCode {
	// Plugins' own log lines and aexlo's warnings land in the run's logs.
	let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn"))
		.target(env_logger::Target::Stderr)
		.try_init();
	match aexlo_harness::worker::serve() {
		Ok(()) => ExitCode::SUCCESS,
		Err(err) => fail(&err),
	}
}

//==== Reporting ========================================================

/// One line per run, as it finishes.
pub fn print_run(run: &Run) {
	let label = match run.outcome {
		Outcome::Pass => "PASS",
		Outcome::Fail => "FAIL",
		Outcome::Error => "ERROR",
		Outcome::Crash => "CRASH",
		Outcome::Timeout => "TIMEOUT",
		Outcome::Skipped => "SKIP",
	};
	let detail = match (&run.message, run.timing.first()) {
		(Some(message), _) => message.clone(),
		(None, Some(seconds)) => format!("{:.1} ms", seconds * 1e3),
		(None, None) => String::new(),
	};
	println!("  {label:<8} {:<48} {detail}", run.variant.id.display);
	for note in &run.notes {
		println!("           {note}");
	}
	if run.outcome.is_bad() || run.fault.is_some() {
		for check in run.checks.iter().filter(|c| c.status == runner::CheckStatus::Fail) {
			let line = format!("{}: {}", check.id, check.message.as_deref().unwrap_or("failed"));
			if run.message.as_deref() != Some(line.as_str()) {
				println!("           {line}");
			}
		}
		let logs = run.trace.logs.trim();
		if !logs.is_empty() {
			for line in logs.lines().rev().take(8).collect::<Vec<_>>().into_iter().rev() {
				println!("           | {line}");
			}
		}
	}
}

/// The report as JSON (`--format json`).
pub fn report_json(report: &Report) -> String {
	let runs: Vec<serde_json::Value> = report
		.runs
		.iter()
		.map(|run| {
			serde_json::json!({
				"id": run.variant.id.display,
				"file_id": run.variant.id.file_safe,
				"preset": run.variant.preset,
				"outcome": run.outcome.name(),
				"message": run.message,
				"fault": run.fault.map(|k| format!("{k:?}").to_lowercase()),
				"timing": run.timing,
				"checks": run.checks,
				"notes": run.notes,
				"strict": run.strict,
				"last_command": run.trace.last_command(),
				"logs": run.trace.logs,
			})
		})
		.collect();
	serde_json::to_string_pretty(&serde_json::json!({
		"summary": report.summary(),
		"exit_code": report.exit_code(),
		"runs": runs,
	}))
	.unwrap_or_default()
}

fn xml_escape(text: &str) -> String {
	text.chars()
		.filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
		.map(|c| match c {
			'&' => "&amp;".to_string(),
			'<' => "&lt;".to_string(),
			'>' => "&gt;".to_string(),
			'"' => "&quot;".to_string(),
			'\'' => "&apos;".to_string(),
			c => c.to_string(),
		})
		.collect()
}

/// The report as JUnit XML (`--junit <path>`): one suite per preset.
pub fn report_junit(report: &Report) -> String {
	let mut suites: Vec<(&str, Vec<&Run>)> = Vec::new();
	for run in &report.runs {
		match suites.iter_mut().find(|(name, _)| *name == run.variant.preset) {
			Some((_, runs)) => runs.push(run),
			None => suites.push((&run.variant.preset, vec![run])),
		}
	}
	let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<testsuites name=\"aexlo\">\n");
	for (name, runs) in suites {
		let count = |f: fn(&Run) -> bool| runs.iter().filter(|r| f(r)).count();
		xml.push_str(&format!(
			"  <testsuite name=\"{}\" tests=\"{}\" failures=\"{}\" errors=\"{}\" skipped=\"{}\">\n",
			xml_escape(name),
			runs.len(),
			count(|r| r.outcome == Outcome::Fail),
			count(|r| matches!(r.outcome, Outcome::Error | Outcome::Crash | Outcome::Timeout)),
			count(|r| r.outcome == Outcome::Skipped),
		));
		for run in runs {
			let time = run.timing.renders.iter().sum::<f64>();
			xml.push_str(&format!(
				"    <testcase classname=\"{}\" name=\"{}\" time=\"{time:.6}\"",
				xml_escape(name),
				xml_escape(&run.variant.id.display)
			));
			let message = xml_escape(run.message.as_deref().unwrap_or(""));
			let body = match run.outcome {
				Outcome::Pass => None,
				Outcome::Skipped => Some(format!("      <skipped message=\"{message}\"/>\n")),
				Outcome::Fail => Some(format!("      <failure message=\"{message}\"/>\n")),
				outcome => Some(format!(
					"      <error type=\"{}\" message=\"{message}\">{}</error>\n",
					outcome.name(),
					xml_escape(&run.trace.logs)
				)),
			};
			match body {
				None => xml.push_str("/>\n"),
				Some(body) => {
					xml.push_str(">\n");
					xml.push_str(&body);
					xml.push_str("    </testcase>\n");
				}
			}
		}
		xml.push_str("  </testsuite>\n");
	}
	xml.push_str("</testsuites>\n");
	xml
}

/// Print the summary (or JSON) and write JUnit, returning the exit code.
pub fn finish(report: &Report, common: &Common) -> ExitCode {
	if common.format == Format::Json {
		println!("{}", report_json(report));
	} else {
		println!("result: {}", report.summary());
	}
	let mut code = report.exit_code();
	if let Some(path) = &common.junit
		&& let Err(e) = std::fs::write(path, report_junit(report))
	{
		eprintln!("error: writing {}: {e}", path.display());
		code = 3;
	}
	ExitCode::from(code)
}

//==== aexlo test =======================================================

/// `aexlo test`'s own flags.
struct TestFlags {
	bless: bool,
	save_frames: Option<PathBuf>,
	/// `--fuzz <n>`: fuzzed cases per preset instead of the presets.
	fuzz: Option<usize>,
	seed: Option<u64>,
}

/// The plugin's parameters and capabilities, from a worker (or in-process).
fn plugin_info(config: &RunnerConfig, plugin: &PluginRef) -> Result<aexlo_harness::worker::PluginInfo, Error> {
	if config.isolate == Isolate::None {
		let path = (config.artifacts)(plugin).ok_or_else(|| Error::harness("no artifact for the plugin"))?;
		let fx = aexlo::Host::get().try_load(&path).map_err(Error::from)?;
		return Ok(aexlo_harness::worker::PluginInfo::of(&fx));
	}
	let mut client = aexlo_harness::worker::WorkerClient::new(config.worker.clone(), config.artifacts.clone())?;
	client.info(plugin).map_err(|failure| match failure {
		aexlo_harness::RunFailure::Invalid { message } => Error::invalid(message),
		aexlo_harness::RunFailure::Harness { message } => Error::harness(message),
		other => Error::plugin(other.message().to_string()),
	})
}

pub fn cmd_test(args: impl Iterator<Item = String>) -> ExitCode {
	let setup = || -> Result<(Common, TestFlags, Manifest, Vec<Variant>, RunnerConfig), Error> {
		let parsed = parse(
			args,
			&[
				("--save-frames", true),
				("--bless", false),
				("--fuzz", true),
				("--seed", true),
			],
		)?;
		let mut flags = TestFlags {
			bless: false,
			save_frames: None,
			fuzz: None,
			seed: None,
		};
		for (flag, value) in parsed.rest {
			let value = value.unwrap_or_default();
			let number = |what: &str| {
				value
					.parse::<u64>()
					.map_err(|_| Error::invalid(format!("{what} expects a number, got '{value}'")))
			};
			match flag.as_str() {
				"--save-frames" => flags.save_frames = Some(PathBuf::from(&value)),
				"--bless" => flags.bless = true,
				"--fuzz" => flags.fuzz = Some(number("--fuzz")?.max(1) as usize),
				"--seed" => flags.seed = Some(number("--seed")?),
				_ => {}
			}
		}
		if flags.fuzz.is_some() && flags.bless {
			return Err(Error::invalid("--fuzz has no goldens to bless"));
		}
		if flags.seed.is_some() && flags.fuzz.is_none() {
			return Err(Error::invalid("--seed only applies to --fuzz"));
		}
		let manifest = load_manifest(parsed.common.manifest.as_deref())?;
		let variants = select(&manifest, &parsed.common)?;
		let artifacts = artifacts(&variants)?;
		let config = runner_config(&parsed.common, artifacts)?;
		Ok((parsed.common, flags, manifest, variants, config))
	};
	let (common, flags, manifest, mut variants, config) = match setup() {
		Ok(setup) => setup,
		Err(err) => return fail(&err),
	};
	let human = common.format == Format::Human;

	// Fuzzing: replace the presets with cases drawn from their first variant.
	let mut cases: Vec<aexlo_harness::fuzz::Case> = Vec::new();
	if let Some(count) = flags.fuzz {
		let seed = flags.seed.unwrap_or_else(|| {
			std::time::SystemTime::now()
				.duration_since(std::time::UNIX_EPOCH)
				.map_or(0, |d| d.as_nanos() as u64)
		});
		let mut bases: Vec<Variant> = Vec::new();
		for variant in variants {
			if !bases.iter().any(|b| b.preset == variant.preset) {
				bases.push(variant);
			}
		}
		let mut infos: HashMap<PluginRef, aexlo_harness::worker::PluginInfo> = HashMap::new();
		for base in &bases {
			if !infos.contains_key(&base.plugin) {
				match plugin_info(&config, &base.plugin) {
					Ok(info) => {
						infos.insert(base.plugin.clone(), info);
					}
					Err(err) => return fail(&err.context(format!("reading {}'s parameters", base.plugin.label()))),
				}
			}
			cases.extend(aexlo_harness::fuzz::cases(base, &infos[&base.plugin], count, seed));
		}
		variants = cases.iter().map(|c| c.variant.clone()).collect();
		if human {
			println!("aexlo test --fuzz: seed {seed} (repeat with --seed {seed})");
		}
	}

	if human {
		println!(
			"aexlo test: {} variant(s){}",
			variants.len(),
			if flags.bless { ", blessing goldens" } else { "" }
		);
	}
	let judge = aexlo_harness::judge::Judge::new(&manifest.dir, flags.bless, common.strict);
	let mut runs = runner::run_all(variants, &config, |executor, variant| {
		let mut run = judge.test(executor, variant);
		if let (Some(dir), Some(frame)) = (&flags.save_frames, &run.frame) {
			let path = dir.join(format!(
				"{}.{}",
				run.variant.id.file_safe,
				aexlo_harness::Frame::extension(frame.depth())
			));
			if let Err(e) = frame.save(&path) {
				run.harness_error(e.message);
			}
		}
		if human {
			print_run(&run);
		}
		run
	});

	// Checks across variants, once all of them ran.
	for i in aexlo_harness::judge::depth_consistency(&mut runs) {
		if human {
			print_run(&runs[i]);
		}
	}

	if human {
		let failing: Vec<String> = cases
			.iter()
			.zip(&runs)
			.filter(|(_, run)| run.outcome.is_bad())
			.map(|(case, run)| {
				let why = format!("{}: {}", run.outcome.name(), run.message.as_deref().unwrap_or(""));
				aexlo_harness::fuzz::preset_block(case, &why)
			})
			.collect();
		if !failing.is_empty() {
			println!("\nfailing cases, ready to paste into {}:\n", manifest::FILE_NAME);
			for block in failing {
				println!("{block}");
			}
		}
	}
	finish(&Report { runs }, &common)
}

//==== aexlo bench (presets) ============================================

/// Whether `aexlo bench`'s arguments name plugins (the flag-driven front-end)
/// rather than a manifest's presets: the first positional argument exists
/// on disk or as a fixture, and no preset-only flag is given.
pub fn bench_names_plugins(args: &[String], is_plugin: impl Fn(&str) -> bool) -> bool {
	const PRESET_FLAGS: &[&str] = &[
		"--manifest",
		"--baseline",
		"--save-baseline",
		"--threshold",
		"--depth",
		"--render",
	];
	const VALUED: &[&str] = &[
		"-r",
		"--resolution",
		"-n",
		"--samples",
		"--warmup",
		"--mode",
		"-s",
		"--set",
		"-i",
		"--input",
		"--csv",
		"--json",
		"--manifest",
		"--depth",
		"--render",
		"--isolate",
		"--jobs",
		"-j",
		"--format",
		"--junit",
		"--baseline",
		"--save-baseline",
		"--threshold",
	];
	if args.iter().any(|a| PRESET_FLAGS.contains(&a.as_str())) {
		return false;
	}
	let mut iter = args.iter();
	while let Some(arg) = iter.next() {
		if VALUED.contains(&arg.as_str()) {
			iter.next();
		} else if !arg.starts_with('-') {
			return is_plugin(arg);
		}
	}
	false
}

fn ms(seconds: f64) -> String {
	format!("{:.3}", seconds * 1e3)
}

pub fn cmd_bench(args: impl Iterator<Item = String>) -> ExitCode {
	struct Flags {
		save: Option<String>,
		baseline: Option<String>,
		threshold: f64,
	}
	let setup = || -> Result<(Common, Flags, Manifest, Vec<Variant>, RunnerConfig), Error> {
		let parsed = parse(
			args,
			&[("--save-baseline", true), ("--baseline", true), ("--threshold", true)],
		)?;
		let mut flags = Flags {
			save: None,
			baseline: None,
			threshold: 0.05,
		};
		let mut threshold_given = false;
		for (flag, value) in parsed.rest {
			let value = value.unwrap_or_default();
			match flag.as_str() {
				"--save-baseline" => {
					aexlo_harness::bench::validate_name(&value)?;
					flags.save = Some(value);
				}
				"--baseline" => {
					aexlo_harness::bench::validate_name(&value)?;
					flags.baseline = Some(value);
				}
				"--threshold" => {
					flags.threshold = aexlo_harness::bench::parse_threshold(&value)?;
					threshold_given = true;
				}
				_ => {}
			}
		}
		if threshold_given && flags.baseline.is_none() {
			return Err(Error::invalid("--threshold only applies with --baseline"));
		}
		if parsed.common.jobs > 1 {
			eprintln!("aexlo bench: benches always run one at a time; ignoring --jobs");
		}
		if parsed.common.strict {
			return Err(Error::invalid("benches run with strict mode off; drop --strict"));
		}
		let manifest = load_manifest(parsed.common.manifest.as_deref())?;
		let variants: Vec<Variant> = select(&manifest, &parsed.common)?
			.into_iter()
			.filter(|v| v.bench.is_some())
			.collect();
		let artifacts = artifacts(&variants)?;
		let mut config = runner_config(&parsed.common, artifacts)?;
		config.jobs = 1;
		Ok((parsed.common, flags, manifest, variants, config))
	};
	let (common, flags, manifest, variants, config) = match setup() {
		Ok(setup) => setup,
		Err(err) => return fail(&err),
	};
	let human = common.format == Format::Human;

	let baseline = match &flags.baseline {
		Some(name) => {
			match aexlo_harness::bench::Baseline::load(&aexlo_harness::bench::baseline_path(&manifest.dir, name)) {
				Ok(baseline) => Some(baseline),
				Err(err) => return fail(&err),
			}
		}
		None => None,
	};
	let fingerprint = aexlo_harness::bench::Fingerprint::current();
	if let Some(baseline) = &baseline {
		let differences = fingerprint.differences(&baseline.fingerprint);
		if !differences.is_empty() {
			eprintln!(
				"aexlo bench: warning: baseline '{}' was measured on another machine ({}); timings may not compare",
				baseline.name,
				differences.join("; ")
			);
		}
	}

	if human {
		println!("aexlo bench: {} variant(s)", variants.len());
		println!(
			"  {:<8} {:<40} {:>10} {:>9} {:>9} {:>8}  {:>7} {:>7} {:>7} {:>7}{}",
			"",
			"variant",
			"median ms",
			"min ms",
			"p95 ms",
			"Mpx/s",
			"pre",
			"render",
			"gpu",
			"host",
			if baseline.is_some() { "   Δmedian    Δmin" } else { "" }
		);
	}

	let records = std::sync::Mutex::new(Vec::new());
	let runs = runner::run_all(variants, &config, |executor, variant| {
		let options = aexlo_harness::RunOptions {
			bench: variant.bench,
			..aexlo_harness::RunOptions::default()
		};
		let mut run = match executor.run(variant, &options) {
			Ok(output) => Run::rendered(variant.clone(), output),
			Err(failure) => Run::from_failure(variant.clone(), failure),
		};
		let stats = match (&run.bench, &run.frame) {
			(Some(samples), Some(frame)) if run.outcome == Outcome::Pass => {
				Some(samples.stats(frame.width() as u64 * frame.height() as u64))
			}
			_ => None,
		};
		let mut delta_text = String::new();
		if let (Some(stats), Some(baseline)) = (&stats, &baseline) {
			match baseline.record(&run.variant.id.display) {
				Some(record) => {
					let delta = aexlo_harness::bench::compare(stats, &record.stats, flags.threshold);
					delta_text = format!("  {:>+8.1}% {:>+6.1}%", delta.median * 100.0, delta.min * 100.0);
					if delta.regressed {
						run.fail(format!(
							"regressed against baseline '{}': median {:+.1}%, min {:+.1}% (threshold {:.1}%)",
							baseline.name,
							delta.median * 100.0,
							delta.min * 100.0,
							flags.threshold * 100.0
						));
					}
				}
				None => run.notes.push(format!("not in baseline '{}'", baseline.name)),
			}
		}
		if let Some(stats) = stats {
			if human {
				let label = if run.outcome == Outcome::Fail { "REGRESS" } else { "" };
				println!(
					"  {label:<8} {:<40} {:>10} {:>9} {:>9} {:>8.1}  {:>7} {:>7} {:>7} {:>7}{delta_text}",
					run.variant.id.display,
					ms(stats.median),
					ms(stats.min),
					ms(stats.p95),
					stats.mpx_per_s,
					ms(stats.phases.pre_render),
					ms(stats.phases.render),
					ms(stats.phases.gpu),
					ms(stats.phases.host),
				);
				if let Some(message) = run.message.as_deref().filter(|_| run.outcome == Outcome::Fail) {
					println!("           {message}");
				}
				for note in &run.notes {
					println!("           {note}");
				}
			}
			if let Ok(mut records) = records.lock() {
				records.push(aexlo_harness::bench::Record {
					id: run.variant.id.display.clone(),
					stats,
				});
			}
		} else if human {
			print_run(&run);
		}
		run
	});

	let report = Report { runs };
	if let Some(name) = &flags.save {
		let path = aexlo_harness::bench::baseline_path(&manifest.dir, name);
		let baseline = aexlo_harness::bench::Baseline {
			name: name.clone(),
			fingerprint,
			records: records.into_inner().unwrap_or_default(),
		};
		if let Err(err) = baseline.save(&path) {
			return fail(&err);
		}
		if human {
			println!(
				"saved baseline '{name}' ({} variant(s)) to {}",
				baseline.records.len(),
				path.display()
			);
		}
	}
	if common.format == Format::Json {
		let runs: Vec<serde_json::Value> = report
			.runs
			.iter()
			.map(|run| {
				let stats = match (&run.bench, &run.frame) {
					(Some(samples), Some(frame)) => Some(samples.stats(frame.width() as u64 * frame.height() as u64)),
					_ => None,
				};
				serde_json::json!({
					"id": run.variant.id.display,
					"outcome": run.outcome.name(),
					"message": run.message,
					"stats": stats,
				})
			})
			.collect();
		println!(
			"{}",
			serde_json::to_string_pretty(&serde_json::json!({
				"summary": report.summary(),
				"exit_code": report.exit_code(),
				"runs": runs,
			}))
			.unwrap_or_default()
		);
		return ExitCode::from(report.exit_code());
	}
	finish(&report, &common)
}
