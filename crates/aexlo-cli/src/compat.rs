//! `aexlo compat`: how far aexlo can drive external plugins. Not whether a
//! plugin's pixels are right (that is `aexlo test`), but whether aexlo can
//! load it, render it down every path and depth, feed it different inputs,
//! sizes and times, and set each of its parameters, and what the plugin asked
//! for along the way that aexlo does not do.
//!
//! Every scenario runs in a worker, on a fresh instance, so a plugin that
//! crashes or hangs costs one scenario. A baseline file records each
//! scenario's status, so a later run can show what aexlo stopped (or started)
//! being able to do.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
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

pub const USAGE: &str = "\
    compat <plugin|dir>...   How far aexlo can drive each plugin: load it, render
                       every path and depth, other inputs, sizes and times,
                       and each parameter set to other values, each in a
                       worker. Reports ok, partial (ran, but asked for
                       something aexlo does not do), n/a, error, crash or
                       timeout per scenario, and the missing suites, stubs
                       and declined calls behind them. A directory means every
                       plugin in it.
        --only <text>          Only scenarios whose id contains <text>
        --jobs <n>             Workers at once  [default: half the CPUs]
        --timeout <s>          Seconds per scenario  [default: 20]
        --all                  List every scenario on its own, not only
                               the ones that were not ok, grouped by how
                               they went
        --format json          Print the report as JSON
        --save-baseline <file> Write each scenario's status to <file>
        --baseline <file>      Compare with <file>; exit 1 when a scenario
                               got worse
";

/// The render size of every scenario that is not about size.
const SIZE: [u32; 2] = [128, 96];
/// Scenarios that share one worker (each still on a fresh instance).
const PER_WORKER: usize = 8;
/// At most this many popup choices are tried.
const MAX_CHOICES: usize = 32;

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

/// The scenario ids and variants for a plugin that declared `info`.
pub fn scenarios(plugin: &PluginRef, info: &PluginInfo, timeout: f64) -> Vec<(String, Variant)> {
	let label = plugin.label();
	let base = |scenario: &str| {
		let mut variant = Variant::standalone(&label, plugin.clone());
		let file_safe = aexlo_harness::preset::file_safe(&format!("{label}.{scenario}"));
		variant.id = VariantId {
			display: format!("{label} {scenario}"),
			golden: file_safe.clone(),
			file_safe,
		};
		variant.size = Some(SIZE);
		variant.input = InputSpec::generated(Generator::Gradient);
		variant.checks = Vec::new();
		variant.strict = Vec::new();
		variant.golden = None;
		variant.bench = None;
		variant.timeout = timeout;
		variant
	};
	let mut out: Vec<(String, Variant)> = Vec::new();
	let mut add = |id: String, edit: &dyn Fn(&mut Variant)| {
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
	for (name, input) in [
		("solid", InputSpec::generated(Generator::Solid)),
		("transparent", transparent),
		("checker", InputSpec::generated(Generator::Checker)),
		("noise", InputSpec::generated(Generator::Noise)),
	] {
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

	// Load each plugin once, for what it declares.
	let loads: Vec<Loaded> = parallel(&plugins, jobs, |plugin| {
		let mut client = WorkerClient::new(worker.clone(), artifacts.clone())
			.map_err(|e| Box::new(RunFailure::Harness { message: e.message }))?;
		client.load(plugin).map_err(Box::new)
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

	let mut ids: Vec<String> = Vec::new();
	let mut variants: Vec<Variant> = Vec::new();
	let mut owner: Vec<usize> = Vec::new();
	for (index, (plugin, load)) in plugins.iter().zip(&loads).enumerate() {
		let Ok((info, _)) = load else { continue };
		let mut group = 0;
		for (n, (id, mut variant)) in scenarios(plugin, info, timeout).into_iter().enumerate() {
			if only.is_some_and(|text| !id.contains(text)) {
				continue;
			}
			// A few scenarios per worker: isolated enough, without a
			// worker start for each.
			if n % PER_WORKER == 0 {
				group += 1;
			}
			variant.preset = format!("{}#{index}.{group}", plugin.label());
			ids.push(id);
			variants.push(variant);
			owner.push(index);
		}
	}

	let config = RunnerConfig {
		isolate: Isolate::Preset,
		jobs,
		worker,
		artifacts,
	};
	let runs = runner::run_all(variants, &config, |executor, variant| {
		runner::render_once(executor, variant)
	});
	if let Some(run) = runs.iter().find(|r| r.fault == Some(ErrorKind::Harness)) {
		return Err(Error::harness(format!(
			"{}: {}",
			run.variant.id,
			run.message.as_deref().unwrap_or("harness error")
		)));
	}

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
	for ((id, run), index) in ids.iter().zip(&runs).zip(owner) {
		let report = &mut reports[index];
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

//==== Output ==============================================================

fn counts<'a>(scenarios: impl Iterator<Item = &'a Scenario>) -> BTreeMap<Status, usize> {
	let mut counts = BTreeMap::new();
	for scenario in scenarios {
		*counts.entry(scenario.status).or_insert(0) += 1;
	}
	counts
}

fn counts_text(counts: &BTreeMap<Status, usize>) -> String {
	Status::ALL
		.iter()
		.filter_map(|s| counts.get(s).map(|n| format!("{n} {}", s.name())))
		.collect::<Vec<_>>()
		.join(" · ")
}

/// Every gap, with the plugins that hit it, most widespread first.
fn gaps(reports: &[PluginReport]) -> Vec<(String, Vec<String>)> {
	let mut by_label: BTreeMap<String, Vec<String>> = BTreeMap::new();
	for report in reports {
		for scenario in report.all() {
			for gap in &scenario.unsupported {
				// Where it was asked is detail; what is missing is the gap.
				let key = match gap.detail.is_empty() {
					true => format!("{} {}", gap.kind, gap.name),
					false => format!("{} {} {}", gap.kind, gap.name, gap.detail),
				};
				let plugins = by_label.entry(key).or_default();
				if !plugins.contains(&report.plugin) {
					plugins.push(report.plugin.clone());
				}
			}
		}
	}
	let mut gaps: Vec<_> = by_label.into_iter().collect();
	gaps.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(&b.0)));
	gaps
}

/// What makes two scenarios fail the same way: their status, the first line
/// of their message, and what they asked for.
fn signature(scenario: &Scenario) -> (Status, String, Vec<String>) {
	let message = match scenario.status {
		Status::Ok => String::new(),
		_ => scenario
			.message
			.as_deref()
			.and_then(|m| m.lines().next())
			.unwrap_or_default()
			.to_string(),
	};
	let gaps = scenario
		.unsupported
		.iter()
		.map(|g| format!("{} (in {})", g.label(), g.command))
		.collect();
	(scenario.status, message, gaps)
}

/// The scenarios worth listing, grouped by [`signature`] in first-seen
/// order; every scenario alone with `all`.
fn groups(report: &PluginReport, all: bool) -> Vec<Vec<&Scenario>> {
	let mut groups: Vec<Vec<&Scenario>> = Vec::new();
	for scenario in report.all() {
		if !all && matches!(scenario.status, Status::Ok | Status::NotApplicable) {
			continue;
		}
		let key = signature(scenario);
		match groups.iter_mut().find(|g| !all && signature(g[0]) == key) {
			Some(group) => group.push(scenario),
			None => groups.push(vec![scenario]),
		}
	}
	groups
}

/// At most this many ids are named for a group of scenarios.
const NAMED: usize = 4;

fn print_group(group: &[&Scenario]) {
	let first = group[0];
	let (status, message, gaps) = signature(first);
	if let [only] = group {
		let mut line = format!("    {:<8} {}", status.name(), only.id);
		if !message.is_empty() {
			line.push_str(&format!(" - {message}"));
		}
		println!("{line}");
		for gap in &only.unsupported {
			let times = if gap.count > 1 {
				format!(" ×{}", gap.count)
			} else {
				String::new()
			};
			println!("               {} (in {}{times})", gap.label(), gap.command);
		}
		return;
	}
	println!(
		"    {:<8} ×{} scenarios{}",
		status.name(),
		group.len(),
		match message.is_empty() {
			true => String::new(),
			false => format!(" - {message}"),
		}
	);
	for gap in gaps {
		println!("               {gap}");
	}
	let mut ids: Vec<&str> = group.iter().take(NAMED).map(|s| s.id.as_str()).collect();
	let more = group.len().saturating_sub(NAMED);
	let rest = if more > 0 {
		format!(", … (+{more})")
	} else {
		String::new()
	};
	ids.dedup();
	println!("               e.g. {}{rest}", ids.join(", "));
}

fn print_human(reports: &[PluginReport], all: bool, changes: Option<&(Vec<Change>, Vec<Change>)>) {
	let total = counts(reports.iter().flat_map(PluginReport::all));
	println!(
		"aexlo compat: {} plugin(s), {} scenario(s): {}",
		reports.len(),
		total.values().sum::<usize>(),
		counts_text(&total)
	);
	for report in reports {
		println!();
		println!(
			"{}  load {} · {}",
			report.plugin,
			report.load.status.name(),
			counts_text(&counts(report.scenarios.iter()))
		);
		for group in groups(report, all) {
			print_group(&group);
		}
	}
	let gaps = gaps(reports);
	if !gaps.is_empty() {
		println!();
		println!("Asked for, but not done by aexlo (plugins that asked):");
		for (label, plugins) in gaps {
			println!("  {:>3}  {label}  [{}]", plugins.len(), plugins.join(", "));
		}
	}
	if let Some((worse, better)) = changes {
		println!();
		if worse.is_empty() && better.is_empty() {
			println!("Same as the baseline.");
		}
		for (title, list) in [
			("Worse than the baseline:", worse),
			("Better than the baseline:", better),
		] {
			if list.is_empty() {
				continue;
			}
			println!("{title}");
			for c in list {
				println!("  {}  {}: {} -> {}", c.plugin, c.id, c.was.name(), c.now.name());
			}
		}
	}
}

fn print_json(reports: &[PluginReport], changes: Option<&(Vec<Change>, Vec<Change>)>) {
	let total = counts(reports.iter().flat_map(PluginReport::all));
	let summary: BTreeMap<&str, usize> = total.iter().map(|(s, n)| (s.name(), *n)).collect();
	let gaps: Vec<serde_json::Value> = gaps(reports)
		.into_iter()
		.map(|(gap, plugins)| serde_json::json!({ "gap": gap, "plugins": plugins }))
		.collect();
	let mut value = serde_json::json!({
		"summary": summary,
		"plugins": reports,
		"gaps": gaps,
	});
	if let Some((worse, better)) = changes {
		value["worse"] = serde_json::json!(worse);
		value["better"] = serde_json::json!(better);
	}
	println!("{}", serde_json::to_string_pretty(&value).unwrap_or_default());
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

pub fn cmd_compat(args: impl Iterator<Item = String>) -> ExitCode {
	match compat(args) {
		Ok(code) => code,
		Err(err) => toolkit::fail(&err),
	}
}

fn compat(args: impl Iterator<Item = String>) -> Result<ExitCode, Error> {
	let mut paths = Vec::new();
	let mut only = None;
	let mut jobs = std::thread::available_parallelism().map_or(2, |n| (n.get() / 2).max(1));
	let mut timeout = 20.0;
	let mut all = false;
	let mut format = Format::Human;
	let mut save = None;
	let mut baseline = None;
	let mut args = args.peekable();
	let value = |args: &mut std::iter::Peekable<_>, flag: &str| -> Result<String, Error> {
		Iterator::next(args).ok_or_else(|| Error::invalid(format!("option '{flag}' needs a value")))
	};
	while let Some(arg) = args.next() {
		match arg.as_str() {
			"--only" => only = Some(value(&mut args, &arg)?),
			"--jobs" => {
				jobs = value(&mut args, &arg)?
					.parse()
					.map_err(|_| Error::invalid("--jobs takes a number"))?
			}
			"--timeout" => {
				timeout = value(&mut args, &arg)?
					.parse()
					.map_err(|_| Error::invalid("--timeout takes seconds"))?
			}
			"--all" => all = true,
			"--format" => {
				format = match value(&mut args, &arg)?.as_str() {
					"json" => Format::Json,
					"human" | "text" => Format::Human,
					other => return Err(Error::invalid(format!("unknown --format '{other}'"))),
				}
			}
			"--save-baseline" => save = Some(PathBuf::from(value(&mut args, &arg)?)),
			"--baseline" => baseline = Some(PathBuf::from(value(&mut args, &arg)?)),
			flag if flag.starts_with('-') => return Err(Error::invalid(format!("unknown option '{flag}'"))),
			plugin => paths.extend(expand(plugin)?),
		}
	}
	if paths.is_empty() {
		return Err(Error::invalid("name at least one plugin or directory of plugins"));
	}
	let previous = baseline.as_deref().map(Baseline::read).transpose()?;

	let reports = run(&paths, only.as_deref(), jobs, timeout)?;
	let changes = previous.as_ref().map(|b| compare(b, &reports));
	match format {
		Format::Human => print_human(&reports, all, changes.as_ref()),
		Format::Json => print_json(&reports, changes.as_ref()),
	}
	if let Some(path) = save {
		Baseline::of(&reports).write(&path)?;
		eprintln!("aexlo compat: wrote {}", path.display());
	}
	Ok(match changes {
		Some((worse, _)) if !worse.is_empty() => ExitCode::from(1),
		_ => ExitCode::SUCCESS,
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

	fn scenario(id: &str, status: Status) -> Scenario {
		Scenario {
			id: id.into(),
			status,
			message: None,
			last_command: None,
			unsupported: Vec::new(),
		}
	}

	fn report(statuses: &[(&str, Status)]) -> PluginReport {
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
	fn a_baseline_round_trips_through_json() {
		let baseline = Baseline::of(&[report(&[("x", Status::NotApplicable), ("y", Status::Timeout)])]);
		let text = serde_json::to_string(&baseline).unwrap();
		assert!(text.contains(r#""x":"n/a""#), "{text}");
		let back: Baseline = serde_json::from_str(&text).unwrap();
		assert_eq!(back.plugins["Fx"]["y"], Status::Timeout);
		assert_eq!(back.plugins["Fx"]["load"], Status::Ok);
	}
}
