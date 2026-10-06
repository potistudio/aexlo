//! Running one variant (§6): configuring an instance, rendering it, and
//! recording what happened. The same code runs in-process and inside an
//! `aexlo worker`; [`Executor`] hides which.

use std::panic::AssertUnwindSafe;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use aexlo::{CommandEvent, CommandPhase, ObserveLevel, Observer, PluginInstance};
use serde::{Deserialize, Serialize};

use crate::apply;
use crate::error::ErrorKind;
use crate::frame::Frame;
use crate::preset::{StrictFeature, Variant};

/// One command in a [`Trace`].
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TracedCommand {
	/// The SDK name without `PF_Cmd_`, e.g. `SMART_RENDER`.
	pub name: String,
	/// Time inside the plugin's entry point.
	pub seconds: f64,
	/// The `PF_Err` returned, when not `PF_Err_NONE`.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub error: Option<i32>,
}

/// What the plugin was asked to do, and what it logged (§6.4).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Trace {
	pub commands: Vec<TracedCommand>,
	/// The command that began but never ended, when a run died inside it.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub unfinished: Option<String>,
	/// Captured stderr (worker runs only).
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub logs: String,
}

impl Trace {
	/// The last command sent to the plugin: the unfinished one if any.
	pub fn last_command(&self) -> Option<&str> {
		self.unfinished
			.as_deref()
			.or_else(|| self.commands.last().map(|c| c.name.as_str()))
	}
}

/// Wall-clock time of each render, plus time per command inside them (§6.4).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Timing {
	/// Seconds per render, in order.
	pub renders: Vec<f64>,
	/// Seconds inside the plugin per command name, summed over the renders.
	pub commands: Vec<(String, f64)>,
}

impl Timing {
	pub fn first(&self) -> Option<f64> {
		self.renders.first().copied()
	}
}

/// What strict mode found during a run (§5.3), as text for the report.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct StrictFindings {
	/// Guard bands found overwritten (`bounds`).
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub guard_violations: Vec<String>,
	/// Handles or worlds that outlived their scope (`allocations`).
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub leaks: Vec<String>,
	/// Undeclared or unbalanced checkouts (`checkouts`).
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub checkout_violations: Vec<String>,
}

impl From<aexlo::StrictReport> for StrictFindings {
	fn from(report: aexlo::StrictReport) -> Self {
		let mut findings = Self::default();
		findings.absorb(report);
		findings
	}
}

impl StrictFindings {
	/// Add what `report` found.
	pub fn absorb(&mut self, report: aexlo::StrictReport) {
		self.guard_violations.extend(
			report
				.guard_violations
				.into_iter()
				.map(|v| format!("{}: {} byte(s) {} the world", v.world, v.bytes, v.band)),
		);
		self.leaks.extend(report.leaks.into_iter().map(|l| {
			format!(
				"{:?} {:#x} ({} bytes) allocated in PF_Cmd_{} outlived PF_Cmd_{}",
				l.kind, l.address, l.bytes, l.allocated_in, l.scope
			)
		}));
		self.checkout_violations
			.extend(report.checkout_violations.into_iter().map(|v| v.message));
	}
}

/// The `aexlo::Strict` for a list of features.
pub fn strict_of(features: &[StrictFeature]) -> aexlo::Strict {
	aexlo::Strict {
		poison_output: features.contains(&StrictFeature::PoisonOutput),
		guard_bands: features.contains(&StrictFeature::GuardBands),
		track_allocations: features.contains(&StrictFeature::TrackAllocations),
		track_checkouts: features.contains(&StrictFeature::TrackCheckouts),
	}
}

/// How to run a variant.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct RunOptions {
	/// Renders of the configured instance; each yields a frame (2 for the
	/// `deterministic` check).
	pub renders: usize,
	/// Strict-mode features to turn on (§5.3).
	#[serde(default)]
	pub strict: Vec<StrictFeature>,
	/// Also report suite calls (costly).
	#[serde(default)]
	pub calls: bool,
}

impl Default for RunOptions {
	fn default() -> Self {
		Self {
			renders: 1,
			strict: Vec::new(),
			calls: false,
		}
	}
}

/// A run that rendered, or decided not to.
#[derive(Clone, Debug, Default)]
pub struct RunOutput {
	/// Why the variant does not apply to this plugin (§4.8); no frames then.
	pub skipped: Option<String>,
	pub frames: Vec<Frame>,
	pub trace: Trace,
	pub timing: Timing,
	pub strict: StrictFindings,
}

/// A run that never produced a frame.
#[derive(Clone, Debug, PartialEq)]
pub enum RunFailure {
	/// The plugin failed to load or returned a `PF_Err`: an `error` outcome.
	Plugin { message: String, trace: Trace },
	/// The variant does not fit the plugin (unknown parameter, ...): exit 2.
	Invalid { message: String },
	/// The harness could not run it: exit 3.
	Harness { message: String },
	/// The worker died.
	Crash { message: String, trace: Trace },
	/// The run exceeded its timeout.
	Timeout { message: String, trace: Trace },
}

impl RunFailure {
	pub fn message(&self) -> &str {
		match self {
			Self::Plugin { message, .. }
			| Self::Invalid { message }
			| Self::Harness { message }
			| Self::Crash { message, .. }
			| Self::Timeout { message, .. } => message,
		}
	}

	pub fn trace(&self) -> Option<&Trace> {
		match self {
			Self::Plugin { trace, .. } | Self::Crash { trace, .. } | Self::Timeout { trace, .. } => Some(trace),
			_ => None,
		}
	}

	pub(crate) fn from_error(err: crate::Error, trace: Trace) -> Self {
		match err.kind {
			ErrorKind::Plugin => Self::Plugin {
				message: err.message,
				trace,
			},
			ErrorKind::Invalid => Self::Invalid { message: err.message },
			ErrorKind::Harness => Self::Harness { message: err.message },
		}
	}
}

/// Something that runs variants: in this process, or in a worker.
pub trait Executor {
	fn run(&mut self, variant: &Variant, options: &RunOptions) -> Result<RunOutput, RunFailure>;
}

/// A callback told as each command begins and ends (the worker streams them
/// so a crash can name the command it died in).
pub type CommandSink = Arc<dyn Fn(&CommandEvent) + Send + Sync>;

/// Records commands into a [`Trace`] and forwards them to a sink.
struct Recorder {
	trace: Mutex<Trace>,
	sink: Option<CommandSink>,
}

impl Observer for Recorder {
	fn command(&self, event: &CommandEvent) {
		if let Some(sink) = &self.sink {
			sink(event);
		}
		let Ok(mut trace) = self.trace.lock() else { return };
		match event.phase {
			CommandPhase::Begin => trace.unfinished = Some(event.name.to_string()),
			CommandPhase::End => {
				trace.unfinished = None;
				trace.commands.push(TracedCommand {
					name: event.name.to_string(),
					seconds: event.duration.map_or(0.0, |d| d.as_secs_f64()),
					error: event.error,
				});
			}
		}
	}
}

/// Configure `fx` for `variant` and render it `options.renders` times.
///
/// This is the whole of a run, shared by [`InProcess`] and the worker.
pub fn run_on(
	fx: &mut PluginInstance,
	variant: &Variant,
	options: &RunOptions,
	sink: Option<CommandSink>,
) -> Result<RunOutput, RunFailure> {
	if let Some(reason) = apply::unsupported(fx, variant) {
		return Ok(RunOutput {
			skipped: Some(reason),
			..RunOutput::default()
		});
	}

	let recorder = Arc::new(Recorder {
		trace: Mutex::new(Trace::default()),
		sink,
	});
	let level = if options.calls {
		ObserveLevel::Calls
	} else {
		ObserveLevel::Commands
	};
	fx.set_observer(Some(recorder.clone()), level);
	fx.set_strict(strict_of(&options.strict));
	let take_trace = |recorder: &Recorder| recorder.trace.lock().map(|t| t.clone()).unwrap_or_default();

	if let Err(err) = apply::configure(fx, variant) {
		fx.set_observer(None, level);
		return Err(RunFailure::from_error(err, take_trace(&recorder)));
	}

	let before_render = take_trace(&recorder).commands.len();
	let mut output = RunOutput::default();
	for _ in 0..options.renders.max(1) {
		let started = Instant::now();
		let result = apply::render(fx, variant.render);
		let elapsed = started.elapsed().as_secs_f64();
		match result {
			Ok(()) => {
				output.timing.renders.push(elapsed);
				output.frames.push(Frame::new(fx.output().clone()));
			}
			Err(err) if apply::is_skip(&err) => {
				fx.set_observer(None, level);
				return Ok(RunOutput {
					skipped: Some(err.to_string()),
					trace: take_trace(&recorder),
					..RunOutput::default()
				});
			}
			Err(err) => {
				fx.set_observer(None, level);
				return Err(RunFailure::Plugin {
					message: err.to_string(),
					trace: take_trace(&recorder),
				});
			}
		}
	}
	fx.set_observer(None, level);

	output.trace = take_trace(&recorder);
	output.strict.absorb(fx.strict_report());
	for command in &output.trace.commands[before_render..] {
		match output
			.timing
			.commands
			.iter_mut()
			.find(|(name, _)| *name == command.name)
		{
			Some((_, total)) => *total += command.seconds,
			None => output.timing.commands.push((command.name.clone(), command.seconds)),
		}
	}
	Ok(output)
}

/// [`run_on`], then tear `fx` down and add what it leaked to the findings
/// (allocations are only judged once the plugin is set down).
pub fn run_owned(
	mut fx: PluginInstance,
	variant: &Variant,
	options: &RunOptions,
	sink: Option<CommandSink>,
) -> Result<RunOutput, RunFailure> {
	let mut result = run_on(&mut fx, variant, options, sink);
	let report = fx.finish();
	if let Ok(output) = &mut result {
		output.strict.absorb(report);
	}
	result
}

/// Runs variants in this process: debuggers and `dbg!` work, but a crash
/// takes the process down (§6.1).
pub struct InProcess<F> {
	load: F,
}

impl<F> InProcess<F>
where
	F: FnMut(&Variant, aexlo::Strict) -> aexlo::Result<PluginInstance>,
{
	/// `load` makes a fresh instance for each run, with strict mode on from
	/// the start, e.g. `|_, strict| aexlo::Host::get().try_load_with(path, strict)`.
	pub fn new(load: F) -> Self {
		Self { load }
	}
}

impl<F> Executor for InProcess<F>
where
	F: FnMut(&Variant, aexlo::Strict) -> aexlo::Result<PluginInstance>,
{
	fn run(&mut self, variant: &Variant, options: &RunOptions) -> Result<RunOutput, RunFailure> {
		// A panicking Rust plugin (or harness bug) fails this run, not the process.
		let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
			let fx = (self.load)(variant, strict_of(&options.strict)).map_err(|e| RunFailure::Plugin {
				message: format!("loading the plugin: {e}"),
				trace: Trace::default(),
			})?;
			run_owned(fx, variant, options, None)
		}));
		match result {
			Ok(result) => result,
			Err(panic) => {
				let message = panic
					.downcast_ref::<String>()
					.cloned()
					.or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
					.unwrap_or_else(|| "panicked".to_string());
				Err(RunFailure::Crash {
					message: format!("panic: {message}"),
					trace: Trace::default(),
				})
			}
		}
	}
}
