//! The [`Observer`] behind `aexlo inspect`: a bounded trace of everything the
//! plugin is asked to do and calls back, plus running totals per command and
//! per suite function.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use aexlo::{AllocationEvent, AllocationKind, CheckoutEvent, CommandEvent, CommandPhase, Observer, SuiteCallEvent};

/// Trace entries kept; older ones are dropped (and counted).
pub const TRACE_CAPACITY: usize = 200_000;

/// One line of the trace.
#[derive(Debug, Clone)]
pub struct Entry {
	/// Running number, unique per recording (repeats share their first one).
	pub seq: u64,
	/// When it happened, from the start of the recording.
	pub at: Duration,
	/// Command nesting depth: 0 for a command, 1 for what it calls, ...
	pub depth: usize,
	/// Consecutive identical calls folded into this entry.
	pub repeat: u32,
	pub kind: Kind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
	/// A user command was issued.
	Step(String),
	Begin(&'static str),
	End {
		name: &'static str,
		duration: Duration,
		error: Option<i32>,
	},
	Call {
		suite: &'static str,
		function: &'static str,
	},
	Alloc {
		kind: AllocationKind,
		allocated: bool,
		address: usize,
		bytes: usize,
	},
	Checkout {
		kind: aexlo::CheckoutKind,
		id: i32,
	},
}

impl Entry {
	/// The entry as one line of text (without time or indentation).
	pub fn describe(&self) -> String {
		let text = match &self.kind {
			Kind::Step(text) => format!("» {text}"),
			Kind::Begin(name) => format!("▶ PF_Cmd_{name}"),
			Kind::End { name, duration, error } => match error {
				Some(code) => format!("◀ PF_Cmd_{name}  {}  ({})", err_name(*code), fmt_duration(*duration)),
				None => format!("◀ PF_Cmd_{name}  ({})", fmt_duration(*duration)),
			},
			Kind::Call { suite, function } => format!("{suite}::{function}"),
			Kind::Alloc {
				kind,
				allocated,
				address,
				bytes,
			} => {
				let verb = if *allocated { "+ alloc" } else { "- free " };
				format!("{verb} {kind:?} {address:#x} ({bytes} B)")
			}
			Kind::Checkout { kind, id } => format!("⇄ {kind:?} #{id}"),
		};
		if self.repeat > 1 {
			format!("{text}  ×{}", self.repeat)
		} else {
			text
		}
	}
}

/// Totals for one `PF_Cmd_*`.
#[derive(Debug, Clone, Default)]
pub struct CommandStat {
	pub count: u64,
	pub errors: u64,
	pub total: Duration,
	pub max: Duration,
	pub last: Duration,
}

/// Totals for one suite function.
#[derive(Debug, Clone, Default)]
pub struct CallStat {
	pub count: u64,
	/// Calls during the latest user command.
	pub in_step: u64,
	/// The step that `in_step` counts.
	pub step: u32,
	/// The `PF_Cmd_*` names it was called from, in first-seen order.
	pub commands: Vec<&'static str>,
}

/// Everything recorded so far.
#[derive(Default)]
pub struct Recording {
	pub trace: VecDeque<Entry>,
	/// Entries dropped off the front of `trace`.
	pub dropped: u64,
	pub commands: HashMap<&'static str, CommandStat>,
	pub calls: HashMap<(&'static str, &'static str), CallStat>,
	/// Allocations and disposals seen, by kind: `(allocated, freed, bytes allocated)`.
	pub memory: HashMap<AllocationKind, (u64, u64, u64)>,
	/// Bumped on every change, so the UI knows when to re-read.
	pub generation: u64,
	pub step: u32,
	seq: u64,
	stack: Vec<&'static str>,
	started: Option<Instant>,
}

impl Recording {
	fn push(&mut self, kind: Kind) {
		self.generation += 1;
		let started = *self.started.get_or_insert_with(Instant::now);
		let depth = match &kind {
			Kind::Step(_) => 0,
			// Begin is pushed before the stack grows, End after it shrinks.
			_ => self.stack.len(),
		};
		if let Some(last) = self.trace.back_mut()
			&& matches!(kind, Kind::Call { .. })
			&& last.kind == kind
			&& last.depth == depth
		{
			last.repeat = last.repeat.saturating_add(1);
			return;
		}
		self.seq += 1;
		if self.trace.len() >= TRACE_CAPACITY {
			self.trace.pop_front();
			self.dropped += 1;
		}
		self.trace.push_back(Entry {
			seq: self.seq,
			at: started.elapsed(),
			depth,
			repeat: 1,
			kind,
		});
	}

	/// The trace and every total start over.
	pub fn clear(&mut self) {
		let step = self.step;
		let stack = std::mem::take(&mut self.stack);
		let generation = self.generation + 1;
		*self = Self::default();
		self.step = step;
		self.stack = stack;
		self.generation = generation;
	}

	/// The `PF_Cmd_*` currently running, innermost last.
	pub fn running(&self) -> &[&'static str] {
		&self.stack
	}
}

/// The latest command and suite call, kept where a crash handler can read
/// them without locking: names are `&'static str`, stored as pointer and
/// length.
pub mod last {
	use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

	pub struct Slot {
		ptr: AtomicPtr<u8>,
		len: AtomicUsize,
	}

	impl Slot {
		const fn new() -> Self {
			Self {
				ptr: AtomicPtr::new(std::ptr::null_mut()),
				len: AtomicUsize::new(0),
			}
		}

		pub fn set(&self, text: &'static str) {
			self.len.store(0, Ordering::SeqCst);
			self.ptr.store(text.as_ptr().cast_mut(), Ordering::SeqCst);
			self.len.store(text.len(), Ordering::SeqCst);
		}

		pub fn clear(&self) {
			self.len.store(0, Ordering::SeqCst);
		}

		/// The bytes last set, if any. Read by the Unix crash handler.
		#[cfg_attr(not(unix), allow(dead_code))]
		pub fn get(&self) -> Option<&'static [u8]> {
			let len = self.len.load(Ordering::SeqCst);
			let ptr = self.ptr.load(Ordering::SeqCst);
			// SAFETY: `ptr` and `len` come from a `&'static str`; a torn
			// read leaves `len` 0 (stored first and last).
			(len > 0 && !ptr.is_null()).then(|| unsafe { std::slice::from_raw_parts(ptr, len) })
		}
	}

	/// The `PF_Cmd_*` running (empty when none).
	pub static COMMAND: Slot = Slot::new();
	/// The suite and function it called last.
	pub static SUITE: Slot = Slot::new();
	pub static FUNCTION: Slot = Slot::new();
}

/// Shared between the plugin thread (as its [`Observer`]) and the UI.
#[derive(Clone, Default)]
pub struct Recorder(Arc<Mutex<Recording>>);

impl Recorder {
	pub fn lock(&self) -> MutexGuard<'_, Recording> {
		// A panic while recording leaves plain data behind; keep going.
		self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
	}

	/// Mark the start of a user command in the trace.
	pub fn step(&self, text: &str) {
		let mut rec = self.lock();
		rec.step += 1;
		rec.push(Kind::Step(text.to_string()));
	}

	pub fn observer(&self) -> Arc<dyn Observer> {
		Arc::new(self.clone())
	}
}

impl Recorder {
	fn on_command(&self, name: &'static str, phase: CommandPhase, duration: Option<Duration>, error: Option<i32>) {
		let mut rec = self.lock();
		match phase {
			CommandPhase::Begin => {
				rec.push(Kind::Begin(name));
				rec.stack.push(name);
				last::COMMAND.set(name);
				last::FUNCTION.clear();
			}
			CommandPhase::End => {
				rec.stack.pop();
				match rec.stack.last() {
					Some(outer) => last::COMMAND.set(outer),
					None => last::COMMAND.clear(),
				}
				let duration = duration.unwrap_or_default();
				rec.push(Kind::End { name, duration, error });
				let stat = rec.commands.entry(name).or_default();
				stat.count += 1;
				stat.errors += u64::from(error.is_some());
				stat.total += duration;
				stat.max = stat.max.max(duration);
				stat.last = duration;
			}
		}
	}

	fn on_call(&self, suite: &'static str, function: &'static str) {
		last::SUITE.set(suite);
		last::FUNCTION.set(function);
		let mut rec = self.lock();
		let step = rec.step;
		let command = rec.stack.last().copied();
		let stat = rec.calls.entry((suite, function)).or_default();
		stat.count += 1;
		if stat.step != step {
			stat.step = step;
			stat.in_step = 0;
		}
		stat.in_step += 1;
		if let Some(command) = command
			&& !stat.commands.contains(&command)
		{
			stat.commands.push(command);
		}
		rec.push(Kind::Call { suite, function });
	}
}

impl Observer for Recorder {
	fn command(&self, event: &CommandEvent) {
		self.on_command(event.name, event.phase, event.duration, event.error);
	}

	fn suite_call(&self, event: &SuiteCallEvent) {
		self.on_call(event.suite, event.function);
	}

	fn allocation(&self, event: &AllocationEvent) {
		let mut rec = self.lock();
		let totals = rec.memory.entry(event.kind).or_default();
		if event.allocated {
			totals.0 += 1;
			totals.2 += event.bytes as u64;
		} else {
			totals.1 += 1;
		}
		rec.push(Kind::Alloc {
			kind: event.kind,
			allocated: event.allocated,
			address: event.address,
			bytes: event.bytes,
		});
	}

	fn checkout(&self, event: &CheckoutEvent) {
		self.lock().push(Kind::Checkout {
			kind: event.kind,
			id: event.id,
		});
	}
}

/// `PF_Err_*` by name, or the number.
pub fn err_name(code: i32) -> String {
	use aexlo::sys::*;
	// The constants' types differ (`_bindgen_ty_*`), so some casts are no-ops.
	#[allow(clippy::unnecessary_cast)]
	const NAMES: &[(u32, &str)] = &[
		(PF_Err_OUT_OF_MEMORY as u32, "OUT_OF_MEMORY"),
		(PF_Err_INTERNAL_STRUCT_DAMAGED as u32, "INTERNAL_STRUCT_DAMAGED"),
		(PF_Err_INVALID_INDEX as u32, "INVALID_INDEX"),
		(PF_Err_UNRECOGNIZED_PARAM_TYPE as u32, "UNRECOGNIZED_PARAM_TYPE"),
		(PF_Err_INVALID_CALLBACK as u32, "INVALID_CALLBACK"),
		(PF_Err_BAD_CALLBACK_PARAM as u32, "BAD_CALLBACK_PARAM"),
		(PF_Interrupt_CANCEL as u32, "Interrupt_CANCEL"),
		(PF_Err_CANNOT_PARSE_KEYFRAME_TEXT as u32, "CANNOT_PARSE_KEYFRAME_TEXT"),
	];
	match NAMES.iter().find(|(value, _)| *value == code as u32) {
		Some((_, name)) => format!("PF_Err_{name} ({code})"),
		None => format!("PF_Err {code}"),
	}
}

/// `1.23 ms`, `456 µs`, `2.10 s`.
pub fn fmt_duration(d: Duration) -> String {
	let micros = d.as_secs_f64() * 1e6;
	if micros < 1000.0 {
		format!("{micros:.0} µs")
	} else if micros < 1e6 {
		format!("{:.2} ms", micros / 1e3)
	} else {
		format!("{:.2} s", micros / 1e6)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn call(rec: &Recorder, function: &'static str) {
		rec.on_call("Suite", function);
	}

	#[test]
	fn folds_repeated_calls_and_counts_them_per_step() {
		let rec = Recorder::default();
		rec.step("render");
		rec.on_command("RENDER", CommandPhase::Begin, None, None);
		for _ in 0..3 {
			call(&rec, "a");
		}
		call(&rec, "b");
		call(&rec, "a");
		rec.on_command("RENDER", CommandPhase::End, Some(Duration::from_millis(2)), Some(-1));
		let r = rec.lock();
		let lines: Vec<(usize, String)> = r.trace.iter().map(|e| (e.depth, e.describe())).collect();
		assert_eq!(
			lines,
			[
				(0, "» render".to_string()),
				(0, "▶ PF_Cmd_RENDER".to_string()),
				(1, "Suite::a  ×3".to_string()),
				(1, "Suite::b".to_string()),
				(1, "Suite::a".to_string()),
				(0, "◀ PF_Cmd_RENDER  PF_Err -1  (2.00 ms)".to_string()),
			]
		);
		let a = &r.calls[&("Suite", "a")];
		assert_eq!((a.count, a.in_step, a.commands.as_slice()), (4, 4, &["RENDER"][..]));
		let render = &r.commands["RENDER"];
		assert_eq!((render.count, render.errors), (1, 1));
	}
}
