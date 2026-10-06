//! Running a set of variants to an outcome each (§6.3), isolated per preset,
//! per variant or not at all (§6.1), on `jobs` workers at once.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::error::ErrorKind;
use crate::exec::{Executor, InProcess, RunFailure, RunOptions, RunOutput, StrictFindings, Timing, Trace};
use crate::frame::Frame;
use crate::preset::Variant;
use crate::worker::{WorkerClient, WorkerCommand};

/// How a run ended (§6.3).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
	Pass,
	Fail,
	Error,
	Crash,
	Timeout,
	Skipped,
}

impl Outcome {
	pub fn name(self) -> &'static str {
		match self {
			Self::Pass => "pass",
			Self::Fail => "fail",
			Self::Error => "error",
			Self::Crash => "crash",
			Self::Timeout => "timeout",
			Self::Skipped => "skipped",
		}
	}

	/// Whether the plugin was judged bad (exit code 1).
	pub fn is_bad(self) -> bool {
		matches!(self, Self::Fail | Self::Error | Self::Crash | Self::Timeout)
	}
}

/// The verdict of one check (§7).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CheckResult {
	pub id: String,
	pub status: CheckStatus,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub message: Option<String>,
}

impl CheckResult {
	pub fn passed(id: &str) -> Self {
		Self {
			id: id.to_string(),
			status: CheckStatus::Pass,
			message: None,
		}
	}

	pub fn failed(id: &str, message: impl Into<String>) -> Self {
		Self {
			id: id.to_string(),
			status: CheckStatus::Fail,
			message: Some(message.into()),
		}
	}

	pub fn skipped(id: &str, why: impl Into<String>) -> Self {
		Self {
			id: id.to_string(),
			status: CheckStatus::Skipped,
			message: Some(why.into()),
		}
	}
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CheckStatus {
	Pass,
	Fail,
	/// The check does not apply to this run (e.g. `finite` below 32 bpc).
	Skipped,
}

/// One variant's run (§6.4).
#[derive(Clone, Debug)]
pub struct Run {
	pub variant: Variant,
	pub outcome: Outcome,
	/// Why it did not pass, or why it was skipped.
	pub message: Option<String>,
	/// The (first) rendered frame.
	pub frame: Option<Frame>,
	/// Every rendered frame, when a run rendered more than once.
	pub frames: Vec<Frame>,
	pub timing: Timing,
	pub trace: Trace,
	pub strict: StrictFindings,
	pub checks: Vec<CheckResult>,
	/// Things worth telling that are not failures (a golden blessed, ...).
	pub notes: Vec<String>,
	/// When the run never reached a verdict for a reason that is not the
	/// plugin's: an invalid variant (exit 2) or a harness error (exit 3).
	pub fault: Option<ErrorKind>,
}

impl Run {
	pub fn from_failure(variant: Variant, failure: RunFailure) -> Self {
		let (outcome, fault) = match &failure {
			RunFailure::Plugin { .. } => (Outcome::Error, None),
			RunFailure::Crash { .. } => (Outcome::Crash, None),
			RunFailure::Timeout { .. } => (Outcome::Timeout, None),
			RunFailure::Invalid { .. } => (Outcome::Error, Some(ErrorKind::Invalid)),
			RunFailure::Harness { .. } => (Outcome::Error, Some(ErrorKind::Harness)),
		};
		Self {
			variant,
			outcome,
			message: Some(failure.message().to_string()),
			frame: None,
			frames: Vec::new(),
			timing: Timing::default(),
			trace: failure.trace().cloned().unwrap_or_default(),
			strict: StrictFindings::default(),
			checks: Vec::new(),
			notes: Vec::new(),
			fault,
		}
	}

	/// A run that rendered, before any check or golden judged it.
	pub fn rendered(variant: Variant, output: RunOutput) -> Self {
		let outcome = if output.skipped.is_some() {
			Outcome::Skipped
		} else {
			Outcome::Pass
		};
		Self {
			variant,
			outcome,
			message: output.skipped,
			frame: output.frames.first().cloned(),
			frames: output.frames,
			timing: output.timing,
			trace: output.trace,
			strict: output.strict,
			checks: Vec::new(),
			notes: Vec::new(),
			fault: None,
		}
	}

	/// Mark the run failed, keeping the first reason given.
	pub fn fail(&mut self, reason: impl Into<String>) {
		if self.outcome == Outcome::Pass {
			self.outcome = Outcome::Fail;
		}
		if self.message.is_none() {
			self.message = Some(reason.into());
		}
	}

	/// Mark the run as unjudgeable because of the harness (exit 3).
	pub fn harness_error(&mut self, reason: impl Into<String>) {
		self.fault = Some(ErrorKind::Harness);
		if self.outcome == Outcome::Pass {
			self.outcome = Outcome::Error;
		}
		self.message.get_or_insert_with(|| reason.into());
	}
}

/// Where variants run (§6.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Isolate {
	/// One worker for all variants of a preset.
	#[default]
	Preset,
	/// One worker per variant.
	Variant,
	/// In this process (debugging only: a crash ends the run).
	None,
}

impl Isolate {
	pub fn parse(name: &str) -> crate::Result<Self> {
		match name {
			"preset" => Ok(Self::Preset),
			"variant" => Ok(Self::Variant),
			"none" => Ok(Self::None),
			other => Err(crate::Error::invalid(format!(
				"unknown --isolate '{other}' (expected preset, variant or none)"
			))),
		}
	}
}

pub use crate::worker::ArtifactMap;

/// What every runner needs.
#[derive(Clone)]
pub struct RunnerConfig {
	pub isolate: Isolate,
	/// Workers running at once (ignored in-process).
	pub jobs: usize,
	pub worker: WorkerCommand,
	pub artifacts: ArtifactMap,
}

/// Run every variant through `run_one`, which turns a variant into a [`Run`]
/// using the executor it is handed (so callers decide how many renders a
/// variant needs and how to judge it). Results come back in input order.
pub fn run_all<J>(variants: Vec<Variant>, config: &RunnerConfig, run_one: J) -> Vec<Run>
where
	J: Fn(&mut dyn Executor, &Variant) -> Run + Send + Sync,
{
	if config.isolate == Isolate::None {
		let artifacts = config.artifacts.clone();
		let mut executor = InProcess::new(move |variant: &Variant, strict| {
			let path = artifacts(&variant.plugin)
				.ok_or_else(|| aexlo::AexloError::Unexpected(format!("no artifact for {}", variant.plugin.label())))?;
			aexlo::Host::get().try_load_with(path, strict)
		});
		return variants.iter().map(|v| run_one(&mut executor, v)).collect();
	}

	// Groups that share a worker: a preset's variants, or each variant alone.
	let mut groups: Vec<Vec<(usize, Variant)>> = Vec::new();
	for (index, variant) in variants.into_iter().enumerate() {
		let joins = config.isolate == Isolate::Preset
			&& groups
				.last()
				.and_then(|g| g.last())
				.is_some_and(|(_, last)| last.preset == variant.preset);
		if joins {
			groups.last_mut().expect("checked").push((index, variant));
		} else {
			groups.push(vec![(index, variant)]);
		}
	}
	let total: usize = groups.iter().map(Vec::len).sum();

	let queue = Mutex::new(groups.into_iter());
	let results: Mutex<Vec<Option<Run>>> = Mutex::new(vec![None; total]);
	let jobs = config.jobs.max(1);
	std::thread::scope(|scope| {
		for _ in 0..jobs {
			scope.spawn(|| {
				loop {
					let Some(group) = queue.lock().ok().and_then(|mut q| q.next()) else {
						break;
					};
					let client = WorkerClient::new(config.worker.clone(), config.artifacts.clone());
					let mut client = match client {
						Ok(client) => client,
						Err(e) => {
							let mut results = results.lock().expect("results");
							for (index, variant) in group {
								results[index] = Some(Run::from_failure(
									variant,
									RunFailure::Harness {
										message: e.message.clone(),
									},
								));
							}
							continue;
						}
					};
					for (index, variant) in group {
						let run = run_one(&mut client, &variant);
						results.lock().expect("results")[index] = Some(run);
					}
				}
			});
		}
	});
	results
		.into_inner()
		.expect("results")
		.into_iter()
		.map(|r| r.expect("every variant ran"))
		.collect()
}

/// The runs of a command and what they add up to.
#[derive(Clone, Debug, Default)]
pub struct Report {
	pub runs: Vec<Run>,
}

impl Report {
	/// The process exit code (§12): 3 when any run hit a harness error, else
	/// 2 when a variant was invalid, else 1 when the plugin was judged bad,
	/// else 0.
	pub fn exit_code(&self) -> u8 {
		if self.runs.iter().any(|r| r.fault == Some(ErrorKind::Harness)) {
			3
		} else if self.runs.iter().any(|r| r.fault == Some(ErrorKind::Invalid)) {
			2
		} else if self.runs.iter().any(|r| r.outcome.is_bad()) {
			1
		} else {
			0
		}
	}

	pub fn count(&self, outcome: Outcome) -> usize {
		self.runs.iter().filter(|r| r.outcome == outcome).count()
	}

	/// `3 passed, 1 crash, 2 skipped`.
	pub fn summary(&self) -> String {
		let parts: Vec<String> = [
			(Outcome::Pass, "passed"),
			(Outcome::Fail, "failed"),
			(Outcome::Error, "error"),
			(Outcome::Crash, "crash"),
			(Outcome::Timeout, "timeout"),
			(Outcome::Skipped, "skipped"),
		]
		.into_iter()
		.filter_map(|(outcome, label)| {
			let n = self.count(outcome);
			(n > 0).then(|| format!("{n} {label}"))
		})
		.collect();
		if parts.is_empty() {
			"no variants".to_string()
		} else {
			parts.join(", ")
		}
	}
}

/// The plain runner: one render per variant, passing whatever renders.
pub fn render_once(executor: &mut dyn Executor, variant: &Variant) -> Run {
	match executor.run(variant, &RunOptions::default()) {
		Ok(output) => Run::rendered(variant.clone(), output),
		Err(failure) => Run::from_failure(variant.clone(), failure),
	}
}
