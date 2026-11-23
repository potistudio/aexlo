//! `aexlo compat`: how far aexlo can drive external plugins. Not whether a
//! plugin's pixels are right (that is `aexlo test`), but whether aexlo can
//! load it, render it down every path and depth, feed it different inputs,
//! sizes and times, and set each of its parameters, and what the plugin asked
//! for along the way that aexlo does not do. Effects that work on text layers
//! go through all of it a second time on a text layer.
//!
//! Every scenario runs in a worker, on a fresh instance, so a plugin that
//! crashes or hangs costs one scenario. A baseline file records each
//! scenario's status, so a later run can show what aexlo stopped (or started)
//! being able to do.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use aexlo_harness::runner::{self, Isolate, Outcome, Run, RunnerConfig};
use aexlo_harness::worker::{ParamInfo, PluginInfo, WorkerClient, WorkerCommand};
use aexlo_harness::{
	Error, ErrorKind, Fps, Gap, Generator, InputSpec, Iterate, PluginRef, PluginSource, RenderMode, RunFailure, Time,
	Variant, VariantId,
};
use serde::{Deserialize, Serialize};
use toml::Value;

use crate::toolkit::{self, Format};

mod progress;
mod report;

use progress::Progress;
use report::Look;

/// `--color`: when the text report is rich.
#[derive(clap::ValueEnum, Clone, Copy, PartialEq, Eq, Default, Debug)]
enum ColorWhen {
	#[default]
	Auto,
	Always,
	Never,
}

#[derive(clap::Args)]
pub struct CompatArgs {
	/// Plugins to drive; a directory means every plugin in it
	#[arg(value_name = "PLUGIN|DIR", required = true)]
	paths: Vec<String>,
	/// Only scenarios whose id contains <TEXT>
	#[arg(long, value_name = "TEXT")]
	only: Option<String>,
	/// Workers at once [default: half the CPUs]
	#[arg(long, value_name = "N")]
	jobs: Option<usize>,
	/// Seconds per scenario
	#[arg(long, value_name = "S", default_value_t = 20.0)]
	timeout: f64,
	/// List every plugin and every scenario, not only the ones with problems
	#[arg(long)]
	all: bool,
	/// Print the report as one JSON document
	#[arg(long)]
	json: bool,
	/// Color the text report, with symbols and bars: auto (on a terminal,
	/// unless NO_COLOR is set), always or never
	#[arg(long, value_enum, value_name = "WHEN", default_value_t)]
	color: ColorWhen,
	/// The old spelling of --json: human or json
	#[arg(long, value_enum, value_name = "F", hide = true)]
	format: Option<Format>,
	/// Write each scenario's status to <FILE>
	#[arg(long, value_name = "FILE")]
	save_baseline: Option<PathBuf>,
	/// Compare with <FILE>; exit 1 when a scenario got worse
	#[arg(long, value_name = "FILE")]
	baseline: Option<PathBuf>,
}

/// The render size of every scenario that is not about size.
const SIZE: [u32; 2] = [128, 96];
/// Scenarios that share one worker (each still on a fresh instance).
const PER_WORKER: usize = 8;
/// At most this many popup choices are tried.
const MAX_CHOICES: usize = 32;
/// What a text layer shows: two lines, several words, letters and digits.
const TEXT: &str = "Text layer\n42";
/// The prefix of scenarios run on a text layer.
const ON_TEXT: &str = "text:";

//==== Statuses ============================================================

/// How a scenario went, from aexlo's side.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Status {
	/// Ran, and asked for nothing aexlo does not do.
	Ok,
	/// Ran, but asked for something aexlo does not do (see its gaps).
	Partial,
	/// The plugin does not declare it (a depth, a render path).
	#[serde(rename = "n/a")]
	NotApplicable,
	/// The scenario does not fit the plugin (a value of the wrong kind): a
	/// gap in `compat`, not in the plugin or aexlo.
	Invalid,
	/// The plugin returned an error.
	Error,
	Timeout,
	Crash,
}

impl Status {
	fn name(self) -> &'static str {
		match self {
			Self::Ok => "ok",
			Self::Partial => "partial",
			Self::NotApplicable => "n/a",
			Self::Invalid => "invalid",
			Self::Error => "error",
			Self::Timeout => "timeout",
			Self::Crash => "crash",
		}
	}

	/// How bad, for comparing with a baseline; `None` when there is
	/// nothing to compare (the plugin does not declare it, or compat erred).
	fn rank(self) -> Option<u8> {
		match self {
			Self::Ok => Some(0),
			Self::Partial => Some(1),
			Self::Error => Some(2),
			Self::Timeout => Some(3),
			Self::Crash => Some(4),
			Self::NotApplicable | Self::Invalid => None,
		}
	}

	const ALL: [Self; 7] = [
		Self::Ok,
		Self::Partial,
		Self::NotApplicable,
		Self::Invalid,
		Self::Error,
		Self::Timeout,
		Self::Crash,
	];
}

/// One scenario's result.
#[derive(Serialize, Clone, Debug)]
pub struct Scenario {
	pub id: String,
	pub status: Status,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub message: Option<String>,
	/// The command it ended in, for errors, crashes and timeouts.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub last_command: Option<String>,
	/// What it asked for that aexlo does not do, beyond what loading did.
	#[serde(skip_serializing_if = "Vec::is_empty")]
	pub unsupported: Vec<Gap>,
}

/// One plugin's results.
#[derive(Serialize, Clone, Debug)]
pub struct PluginReport {
	pub plugin: String,
	pub path: PathBuf,
	/// Loading it (global, params and sequence setup).
	pub load: Scenario,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub info: Option<PluginInfo>,
	pub scenarios: Vec<Scenario>,
}

impl PluginReport {
	fn all(&self) -> impl Iterator<Item = &Scenario> {
		std::iter::once(&self.load).chain(&self.scenarios)
	}
}

//==== Scenarios ===========================================================

/// Which layer the effect sits on in a set of scenarios.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum On {
	/// An AV layer showing a gradient, unless the scenario is about the input.
	AvLayer,
	/// A text layer showing [`TEXT`]. Ids start with [`ON_TEXT`] and there
	/// are no `input:*` scenarios.
	TextLayer,
}

/// The scenario ids and variants for a plugin that declared `info`, on `on`.
pub fn scenarios(plugin: &PluginRef, info: &PluginInfo, timeout: f64, on: On) -> Vec<(String, Variant)> {
	let label = plugin.label();
	let prefix = match on {
		On::AvLayer => "",
		On::TextLayer => ON_TEXT,
	};
	let base = |scenario: &str| {
		let mut variant = Variant::standalone(&label, plugin.clone());
		let file_safe = aexlo_harness::preset::file_safe(&format!("{label}.{scenario}"));
		variant.id = VariantId {
			display: format!("{label} {scenario}"),
			golden: file_safe.clone(),
			file_safe,
		};
		variant.size = Some(SIZE);
		variant.input = match on {
			On::AvLayer => InputSpec::generated(Generator::Gradient),
			On::TextLayer => InputSpec::text(TEXT),
		};
		variant.checks = Vec::new();
		variant.strict = Vec::new();
		variant.golden = None;
		variant.bench = None;
		variant.timeout = timeout;
		variant
	};
	let mut out: Vec<(String, Variant)> = Vec::new();
	let mut add = |id: String, edit: &dyn Fn(&mut Variant)| {
		let id = format!("{prefix}{id}");
		let mut variant = base(&id);
		edit(&mut variant);
		out.push((id, variant));
	};

	add("render:auto@8".into(), &|_| {});
	for (mode, name) in [
		(RenderMode::Legacy, "legacy"),
		(RenderMode::Smart, "smart"),
		(RenderMode::Gpu, "gpu"),
	] {
		for depth in [8, 16, 32] {
			add(format!("render:{name}@{depth}"), &|v| {
				v.render = mode;
				v.depth = depth;
			});
		}
	}

	let transparent = InputSpec {
		color: Some(vec![0.5, 0.5, 0.5, 0.0]),
		..InputSpec::generated(Generator::Solid)
	};
	let inputs = [
		("solid", InputSpec::generated(Generator::Solid)),
		("transparent", transparent),
		("checker", InputSpec::generated(Generator::Checker)),
		("noise", InputSpec::generated(Generator::Noise)),
		("text", InputSpec::text(TEXT)),
	];
	for (name, input) in inputs.into_iter().filter(|_| on == On::AvLayer) {
		add(format!("input:{name}"), &|v| v.input = input.clone());
	}

	for [w, h] in [[1, 1], [37, 23], [1920, 1080]] {
		add(format!("size:{w}x{h}"), &|v| v.size = Some([w, h]));
	}

	for (frame, fps, name) in [
		(90, Fps { num: 30, den: 1 }, "90@30"),
		(1, Fps { num: 24000, den: 1001 }, "1@23.976"),
	] {
		add(format!("time:{name}"), &|v| v.time = Time { frame, fps });
	}

	add("iterate:serial".into(), &|v| v.iterate = Iterate::Serial);

	for param in &info.params {
		let key = format!("#{}", param.index);
		if param.kind == "Layer" {
			add(format!("layer:{key} {}=linked", param.name), &|v| {
				v.layers = vec![(key.clone(), InputSpec::generated(Generator::Checker))];
			});
			continue;
		}
		for (label, value) in param_values(param) {
			add(format!("param:{key} {}={label}", param.name), &|v| {
				v.params = vec![(key.clone(), value.clone())];
			});
		}
	}
	out
}

/// The values a parameter is tried at, with how each is named in its id.
fn param_values(param: &ParamInfo) -> Vec<(String, Value)> {
	let numbers = |values: [f64; 2]| -> Vec<(String, Value)> {
		values.iter().map(|v| (format!("{v}"), Value::Float(*v))).collect()
	};
	let arrays = |values: &[&[f64]]| -> Vec<(String, Value)> {
		values
			.iter()
			.map(|vs| {
				let value = Value::Array(vs.iter().map(|v| Value::Float(*v)).collect());
				(format!("{vs:?}"), value)
			})
			.collect()
	};
	match param.kind.as_str() {
		"FloatSlider" | "FixedSlider" => param.range.map(|(lo, hi)| numbers([lo, hi])).unwrap_or_default(),
		"Slider" => param
			.range
			.map(|(lo, hi)| {
				[lo, hi]
					.iter()
					.map(|v| (format!("{v}"), Value::Integer(*v as i64)))
					.collect()
			})
			.unwrap_or_default(),
		"Angle" => numbers([45.0, 270.0]),
		"Checkbox" => [true, false]
			.iter()
			.map(|b| (b.to_string(), Value::Boolean(*b)))
			.collect(),
		"Popup" => param
			.choices
			.iter()
			.enumerate()
			.take(MAX_CHOICES)
			// A popup's "(-" separators are not choices.
			.filter(|(_, label)| !label.starts_with("(-"))
			.map(|(i, label)| (label.trim().to_string(), Value::Integer(i as i64 + 1)))
			.collect(),
		"Point" => arrays(&[&[0.0, 0.0], &[f64::from(SIZE[0] - 1), f64::from(SIZE[1] - 1)]]),
		"Point3D" => arrays(&[&[0.0, 0.0, 0.0], &[64.0, 48.0, 100.0]]),
		"Color" => arrays(&[&[1.0, 0.0, 0.0], &[0.0, 0.0, 1.0, 0.5]]),
		_ => Vec::new(),
	}
}

/// What `run` says about a scenario, without the gaps loading already had.
fn judge(id: &str, run: &Run, load_gaps: &[Gap]) -> Scenario {
	let unsupported: Vec<Gap> = run
		.trace
		.unsupported
		.iter()
		.filter(|gap| {
			!load_gaps
				.iter()
				.any(|l| l.kind == gap.kind && l.name == gap.name && l.detail == gap.detail && l.command == gap.command)
		})
		.cloned()
		.collect();
	let status = match run.outcome {
		Outcome::Pass if unsupported.is_empty() => Status::Ok,
		Outcome::Pass => Status::Partial,
		Outcome::Skipped => Status::NotApplicable,
		Outcome::Crash => Status::Crash,
		Outcome::Timeout => Status::Timeout,
		Outcome::Error | Outcome::Fail => match run.fault {
			Some(ErrorKind::Invalid) => Status::Invalid,
			_ => Status::Error,
		},
	};
	let failed = matches!(status, Status::Error | Status::Crash | Status::Timeout);
	Scenario {
		id: id.to_string(),
		status,
		message: run.message.clone(),
		last_command: failed.then(|| run.trace.last_command().map(str::to_string)).flatten(),
		unsupported,
	}
}

/// Loading a plugin: what it declares and the trace of its setup.
type Loaded = Result<(PluginInfo, aexlo_harness::Trace), Box<RunFailure>>;

fn load_scenario(result: &Loaded) -> Scenario {
	match result {
		Ok((_, trace)) => Scenario {
			id: "load".into(),
			status: if trace.unsupported.is_empty() {
				Status::Ok
			} else {
				Status::Partial
			},
			message: None,
			last_command: None,
			unsupported: trace.unsupported.clone(),
		},
		Err(failure) => Scenario {
			id: "load".into(),
			status: match failure.as_ref() {
				RunFailure::Crash { .. } => Status::Crash,
				RunFailure::Timeout { .. } => Status::Timeout,
				RunFailure::Invalid { .. } => Status::Invalid,
				_ => Status::Error,
			},
			message: Some(failure.message().to_string()),
			last_command: failure.trace().and_then(|t| t.last_command()).map(str::to_string),
			unsupported: failure.trace().map(|t| t.unsupported.clone()).unwrap_or_default(),
		},
	}
}

//==== Running =============================================================

/// Run `f` over `items` on `jobs` threads, keeping their order.
fn parallel<T: Sync, R: Send>(items: &[T], jobs: usize, f: impl Fn(&T) -> R + Sync) -> Vec<R> {
	let next = Mutex::new(0usize);
	let results: Mutex<Vec<Option<R>>> = Mutex::new((0..items.len()).map(|_| None).collect());
	std::thread::scope(|scope| {
		for _ in 0..jobs.max(1).min(items.len().max(1)) {
			scope.spawn(|| {
				loop {
					let index = {
						let mut next = next.lock().expect("queue");
						let index = *next;
						*next += 1;
						index
					};
					let Some(item) = items.get(index) else { break };
					let result = f(item);
					results.lock().expect("results")[index] = Some(result);
				}
			});
		}
	});
	results
		.into_inner()
		.expect("results")
		.into_iter()
		.map(|r| r.expect("every item ran"))
		.collect()
}

/// The scenario that puts the effect on a text layer.
const TEXT_INPUT: &str = "input:text";
/// The run that puts [`TEXT_INPUT`]'s pixels on an AV layer. Not reported.
const PROBE: &str = "probe:text-on-av";

/// Scenarios to run together, with the plugin each belongs to.
#[derive(Default)]
struct Batch {
	ids: Vec<String>,
	variants: Vec<Variant>,
	owner: Vec<usize>,
}

impl Batch {
	/// Add `list`, the scenarios of plugin `index`, a few per worker.
	fn add(&mut self, plugin: &PluginRef, index: usize, phase: &str, list: impl Iterator<Item = (String, Variant)>) {
		for (n, (id, mut variant)) in list.enumerate() {
			// A few scenarios per worker: isolated enough, without a worker
			// start for each.
			variant.preset = format!("{}#{index}.{phase}.{}", plugin.label(), n / PER_WORKER);
			self.ids.push(id);
			self.variants.push(variant);
			self.owner.push(index);
		}
	}
}

/// An input with the pixels of [`TEXT_INPUT`]'s text layer, on an AV layer:
/// the text written as a PNG in `dir`. `None` when it cannot be (no system
/// font); then no plugin is run on a text layer.
fn text_on_av_layer(dir: &Path) -> Option<InputSpec> {
	let layer = aexlo_harness::input::build(
		&InputSpec::text(TEXT),
		aexlo::PixelDepthKind::U8,
		Some((SIZE[0], SIZE[1])),
	)
	.ok()?;
	let path = dir.join("text.png");
	aexlo_harness::Frame::new(layer).save(&path).ok()?;
	Some(InputSpec {
		path: Some(path),
		..InputSpec::default()
	})
}

/// Run every scenario of every plugin in `paths`.
pub fn run(paths: &[PathBuf], only: Option<&str>, jobs: usize, timeout: f64) -> Result<Vec<PluginReport>, Error> {
	let worker = WorkerCommand::current_exe()?;
	let artifacts: runner::ArtifactMap = Arc::new(|plugin: &PluginRef| match &plugin.source {
		PluginSource::Artifact(path) => Some(path.clone()),
		PluginSource::Crate(_) => None,
	});
	let plugins: Vec<PluginRef> = paths
		.iter()
		.map(|path| PluginRef {
			source: PluginSource::Artifact(path.clone()),
			entry: "EffectMain".into(),
		})
		.collect();

	let progress = Progress::new();

	// Load each plugin once, for what it declares.
	progress.phase("loading", plugins.len());
	let loads: Vec<Loaded> = parallel(&plugins, jobs, |plugin| {
		let loaded = WorkerClient::new(worker.clone(), artifacts.clone())
			.map_err(|e| Box::new(RunFailure::Harness { message: e.message }))
			.and_then(|mut client| client.load(plugin).map_err(Box::new));
		progress.tick();
		loaded
	});
	// A worker that cannot start is aexlo's problem, not the plugin's.
	if let Some(message) = loads
		.iter()
		.find_map(|load| match load.as_ref().err().map(Box::as_ref) {
			Some(RunFailure::Harness { message }) => Some(message),
			_ => None,
		}) {
		return Err(Error::harness(message.clone()));
	}

	let config = RunnerConfig {
		isolate: Isolate::Preset,
		jobs,
		worker,
		artifacts,
	};
	let run_batch = |batch: Batch, phase: &str| -> Result<Vec<(usize, String, Run)>, Error> {
		progress.phase(phase, batch.variants.len());
		let runs = runner::run_all(batch.variants, &config, |executor, variant| {
			let run = runner::render_once(executor, variant);
			progress.tick();
			run
		});
		if let Some(run) = runs.iter().find(|r| r.fault == Some(ErrorKind::Harness)) {
			return Err(Error::harness(format!(
				"{}: {}",
				run.variant.id,
				run.message.as_deref().unwrap_or("harness error")
			)));
		}
		Ok(batch
			.owner
			.into_iter()
			.zip(batch.ids)
			.zip(runs)
			.map(|((owner, id), run)| (owner, id, run))
			.collect())
	};

	// The text a text layer shows, on an AV layer: what tells an effect that
	// works on text layers from one that only sees pixels.
	let probe_dir = std::env::temp_dir().join(format!("aexlo-compat-{}", std::process::id()));
	let probe_input = text_on_av_layer(&probe_dir);

	// First every plugin on an AV layer, and the text probe.
	let mut batch = Batch::default();
	for (index, (plugin, load)) in plugins.iter().zip(&loads).enumerate() {
		let Ok((info, _)) = load else { continue };
		let list = scenarios(plugin, info, timeout, On::AvLayer);
		let probe = probe_input.as_ref().and_then(|input| {
			let (_, text) = list.iter().find(|(id, _)| id == TEXT_INPUT)?;
			let mut variant = text.clone();
			variant.input = input.clone();
			Some((PROBE.to_string(), variant))
		});
		// The text scenario always runs: the probe is compared with it.
		let wanted = |id: &str| id == TEXT_INPUT || only.is_none_or(|text| id.contains(text));
		let list = list.into_iter().filter(|(id, _)| wanted(id)).chain(probe);
		batch.add(plugin, index, "av", list);
	}
	let runs = run_batch(batch, "AV layer");
	let _ = std::fs::remove_dir_all(&probe_dir);
	let mut runs = runs?;

	// Then the plugins that render a text layer differently from the same
	// pixels on an AV layer, on a text layer.
	let mut batch = Batch::default();
	for (index, (plugin, load)) in plugins.iter().zip(&loads).enumerate() {
		let Ok((info, _)) = load else { continue };
		let frame = |id: &str| {
			runs.iter()
				.find(|(owner, i, run)| *owner == index && i == id && run.outcome == Outcome::Pass)
				.and_then(|(_, _, run)| run.frame.as_ref())
		};
		let (Some(text), Some(av)) = (frame(TEXT_INPUT), frame(PROBE)) else {
			continue;
		};
		if text.unit_rgba() == av.unit_rgba() {
			continue;
		}
		let list = scenarios(plugin, info, timeout, On::TextLayer)
			.into_iter()
			.filter(|(id, _)| only.is_none_or(|text| id.contains(text)));
		batch.add(plugin, index, "text", list);
	}
	if !batch.variants.is_empty() {
		runs.extend(run_batch(batch, "text layer")?);
	}
	progress.finish();

	let mut reports: Vec<PluginReport> = plugins
		.iter()
		.zip(&loads)
		.map(|(plugin, load)| PluginReport {
			plugin: plugin.label(),
			path: match &plugin.source {
				PluginSource::Artifact(path) | PluginSource::Crate(path) => path.clone(),
			},
			load: load_scenario(load),
			info: load.as_ref().ok().map(|(info, _)| info.clone()),
			scenarios: Vec::new(),
		})
		.collect();
	for (index, id, run) in &runs {
		if id == PROBE || only.is_some_and(|text| !id.contains(text)) {
			continue;
		}
		let report = &mut reports[*index];
		let scenario = judge(id, run, &report.load.unsupported);
		report.scenarios.push(scenario);
	}
	Ok(reports)
}

//==== Baselines ===========================================================

/// Each scenario's status per plugin, as `--save-baseline` writes it.
#[derive(Serialize, Deserialize, Default)]
pub struct Baseline {
	pub plugins: BTreeMap<String, BTreeMap<String, Status>>,
}

impl Baseline {
	pub fn of(reports: &[PluginReport]) -> Self {
		Self {
			plugins: reports
				.iter()
				.map(|r| (r.plugin.clone(), r.all().map(|s| (s.id.clone(), s.status)).collect()))
				.collect(),
		}
	}

	fn read(path: &Path) -> Result<Self, Error> {
		let text = std::fs::read_to_string(path)
			.map_err(|e| Error::invalid(format!("reading baseline {}: {e}", path.display())))?;
		serde_json::from_str(&text).map_err(|e| Error::invalid(format!("baseline {}: {e}", path.display())))
	}

	fn write(&self, path: &Path) -> Result<(), Error> {
		let text = serde_json::to_string_pretty(self).unwrap_or_default();
		std::fs::write(path, text + "\n").map_err(|e| Error::harness(format!("writing {}: {e}", path.display())))
	}
}

/// A scenario whose status changed since the baseline.
#[derive(Serialize, Clone, Debug)]
pub struct Change {
	pub plugin: String,
	pub id: String,
	pub was: Status,
	pub now: Status,
}

/// The scenarios that got worse and better since `baseline`; scenarios
/// either side lacks, or that are n/a or invalid on either side, are not
/// compared.
pub fn compare(baseline: &Baseline, reports: &[PluginReport]) -> (Vec<Change>, Vec<Change>) {
	let (mut worse, mut better) = (Vec::new(), Vec::new());
	for report in reports {
		let Some(was) = baseline.plugins.get(&report.plugin) else {
			continue;
		};
		for scenario in report.all() {
			let Some(&old) = was.get(&scenario.id) else { continue };
			let (Some(a), Some(b)) = (old.rank(), scenario.status.rank()) else {
				continue;
			};
			let change = Change {
				plugin: report.plugin.clone(),
				id: scenario.id.clone(),
				was: old,
				now: scenario.status,
			};
			if b > a {
				worse.push(change);
			} else if b < a {
				better.push(change);
			}
		}
	}
	(worse, better)
}

/// Coverage in `baseline` and now, over the scenarios both have, so that a
/// run with `--only` or new plugins compares like with like.
pub fn coverage_change(baseline: &Baseline, reports: &[PluginReport]) -> Option<(f64, f64)> {
	let (mut was, mut now) = (Vec::new(), Vec::new());
	for report in reports {
		let Some(old) = baseline.plugins.get(&report.plugin) else {
			continue;
		};
		for scenario in report.all() {
			if let Some(status) = old.get(&scenario.id) {
				was.push(*status);
				now.push(scenario.status);
			}
		}
	}
	Some((report::coverage(&was)?, report::coverage(&now)?))
}

//==== Command =============================================================

/// The plugin artifacts `arg` names: itself, or every plugin in a directory
/// (skipping symlinks to plugins that are not there).
fn expand(arg: &str) -> Result<Vec<PathBuf>, Error> {
	let path = PathBuf::from(arg);
	let ext = if cfg!(target_os = "windows") { "aex" } else { "plugin" };
	let is_plugin = |p: &Path| {
		p.extension()
			.and_then(|e| e.to_str())
			.is_some_and(|e| e.eq_ignore_ascii_case(ext) || e.eq_ignore_ascii_case("dll") || e == "dylib")
	};
	if path.is_dir() && !is_plugin(&path) {
		let mut found: Vec<PathBuf> = std::fs::read_dir(&path)
			.map_err(|e| Error::invalid(format!("reading {}: {e}", path.display())))?
			.filter_map(|entry| entry.ok().map(|e| e.path()))
			.filter(|p| is_plugin(p) && p.exists())
			.collect();
		found.sort();
		if found.is_empty() {
			return Err(Error::invalid(format!("no plugins in {}", path.display())));
		}
		return Ok(found);
	}
	if path.exists() {
		return Ok(vec![path]);
	}
	let with_ext = PathBuf::from(format!("{arg}.{ext}"));
	if with_ext.exists() {
		return Ok(vec![with_ext]);
	}
	Err(Error::invalid(format!("no plugin at {arg}")))
}

pub fn cmd(args: CompatArgs) -> u8 {
	match compat(args) {
		Ok(code) => code,
		Err(err) => toolkit::fail(&err),
	}
}

fn compat(args: CompatArgs) -> Result<u8, Error> {
	let mut paths = Vec::new();
	for plugin in &args.paths {
		paths.extend(expand(plugin)?);
	}
	let only = args.only;
	let jobs = args
		.jobs
		.unwrap_or_else(|| std::thread::available_parallelism().map_or(2, |n| (n.get() / 2).max(1)));
	let (timeout, all) = (args.timeout, args.all);
	let format = Format::from_flags(args.json, args.format);
	let (save, baseline) = (args.save_baseline, args.baseline);
	if paths.is_empty() {
		return Err(Error::invalid("name at least one plugin or directory of plugins"));
	}
	let previous = baseline.as_deref().map(Baseline::read).transpose()?;
	// For the report and the progress line alike.
	match args.color {
		ColorWhen::Auto => {}
		ColorWhen::Always => anstream::ColorChoice::Always.write_global(),
		ColorWhen::Never => anstream::ColorChoice::Never.write_global(),
	}

	let reports = run(&paths, only.as_deref(), jobs, timeout)?;
	let changes = previous.as_ref().map(|b| {
		let (worse, better) = compare(b, &reports);
		report::Changes {
			worse,
			better,
			coverage: coverage_change(b, &reports),
		}
	});
	match format {
		Format::Human => {
			let rich = anstream::AutoStream::choice(&std::io::stdout()) != anstream::ColorChoice::Never;
			let look = if rich { Look::RICH } else { Look::PLAIN };
			anstream::print!("{}", report::human(&reports, all, changes.as_ref(), look));
		}
		Format::Json => println!("{}", report::json(&reports, changes.as_ref())),
	}
	if let Some(path) = save {
		Baseline::of(&reports).write(&path)?;
		eprintln!("aexlo compat: wrote {}", path.display());
	}
	Ok(match changes {
		Some(changes) if !changes.worse.is_empty() => 1,
		_ => 0,
	})
}

#[cfg(test)]
mod tests {
	use super::*;

	fn param(kind: &str) -> ParamInfo {
		ParamInfo {
			index: 2,
			name: "P".into(),
			kind: kind.into(),
			range: Some((0.0, 10.0)),
			choices: vec!["One".into(), "(-".into(), "Two".into()],
			hidden: false,
		}
	}

	#[test]
	fn every_settable_kind_gets_two_or_more_values() {
		for kind in [
			"FloatSlider",
			"FixedSlider",
			"Slider",
			"Angle",
			"Checkbox",
			"Point",
			"Point3D",
			"Color",
		] {
			assert_eq!(param_values(&param(kind)).len(), 2, "{kind}");
		}
		let popup: Vec<_> = param_values(&param("Popup"))
			.into_iter()
			.map(|(l, v)| (l, v.as_integer()))
			.collect();
		assert_eq!(popup, [("One".to_string(), Some(1)), ("Two".to_string(), Some(3))]);
		assert!(param_values(&param("Button")).is_empty());
	}

	#[test]
	fn on_a_text_layer_every_scenario_but_the_inputs_runs_on_text() {
		let plugin = PluginRef {
			source: PluginSource::Artifact("Fx.plugin".into()),
			entry: "EffectMain".into(),
		};
		let info = PluginInfo {
			params: vec![param("Checkbox")],
			..PluginInfo::default()
		};
		let av = scenarios(&plugin, &info, 1.0, On::AvLayer);
		let text = scenarios(&plugin, &info, 1.0, On::TextLayer);
		let ids = |list: &[(String, Variant)]| list.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>();
		assert!(ids(&av).contains(&TEXT_INPUT.to_string()));
		assert!(
			text.iter()
				.all(|(id, v)| id.starts_with(ON_TEXT) && v.input == InputSpec::text(TEXT))
		);
		let expected: Vec<String> = ids(&av)
			.into_iter()
			.filter(|id| !id.starts_with("input:"))
			.map(|id| format!("{ON_TEXT}{id}"))
			.collect();
		assert_eq!(ids(&text), expected);
	}

	pub(super) fn scenario(id: &str, status: Status) -> Scenario {
		Scenario {
			id: id.into(),
			status,
			message: None,
			last_command: None,
			unsupported: Vec::new(),
		}
	}

	pub(super) fn report(statuses: &[(&str, Status)]) -> PluginReport {
		PluginReport {
			plugin: "Fx".into(),
			path: PathBuf::from("Fx.plugin"),
			load: scenario("load", Status::Ok),
			info: None,
			scenarios: statuses.iter().map(|(id, s)| scenario(id, *s)).collect(),
		}
	}

	#[test]
	fn only_comparable_scenarios_count_as_worse_or_better() {
		let baseline = Baseline::of(&[report(&[
			("a", Status::Ok),
			("b", Status::Crash),
			("c", Status::Ok),
			("d", Status::NotApplicable),
			("e", Status::Partial),
		])]);
		let now = [report(&[
			("a", Status::Partial),
			("b", Status::Ok),
			("c", Status::NotApplicable),
			("d", Status::Crash),
			("e", Status::Partial),
			("new", Status::Crash),
		])];
		let (worse, better) = compare(&baseline, &now);
		let ids = |list: &[Change]| list.iter().map(|c| c.id.clone()).collect::<Vec<_>>();
		assert_eq!(ids(&worse), ["a"]);
		assert_eq!(ids(&better), ["b"]);
	}

	#[test]
	fn coverage_change_compares_the_scenarios_both_runs_have() {
		let baseline = Baseline::of(&[report(&[("a", Status::Ok), ("b", Status::Crash), ("c", Status::Ok)])]);
		// `c` is not in this run and `d` not in the baseline: neither counts.
		let now = report(&[("a", Status::Ok), ("b", Status::Ok), ("d", Status::Crash)]);
		assert_eq!(coverage_change(&baseline, &[now]), Some((2.0 / 3.0, 1.0)));
		let mut other = report(&[]);
		other.plugin = "Other".into();
		assert_eq!(coverage_change(&baseline, &[other]), None);
	}

	#[test]
	fn a_baseline_round_trips_through_json() {
		let baseline = Baseline::of(&[report(&[("x", Status::NotApplicable), ("y", Status::Timeout)])]);
		let text = serde_json::to_string(&baseline).unwrap();
		assert!(text.contains(r#""x":"n/a""#), "{text}");
		let back: Baseline = serde_json::from_str(&text).unwrap();
		assert_eq!(back.plugins["Fx"]["y"], Status::Timeout);
		assert_eq!(back.plugins["Fx"]["load"], Status::Ok);
	}
}
