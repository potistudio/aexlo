//! Diagnosis of one trace entry: what a focused call was given, where it ran,
//! what the memory it names was doing, and what happened around it.
//!
//! Produces styled text independent of the terminal, so the TUI's focus panel
//! and `--exec`'s `focus` print the same thing.

use std::collections::HashMap;

use super::engine::Snapshot;
use super::recorder::{CallDetail, Entry, Kind, Recording, err_name, fmt_duration};

/// How a piece of the diagnosis is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
	Heading,
	Key,
	Plain,
	Good,
	Warn,
	Bad,
	Dim,
}

pub type Span = (Tone, String);

pub struct Diagnosis {
	pub title: String,
	pub lines: Vec<Vec<Span>>,
}

impl Diagnosis {
	fn new(title: String) -> Self {
		Self {
			title,
			lines: Vec::new(),
		}
	}

	fn heading(&mut self, text: &str) {
		if !self.lines.is_empty() {
			self.lines.push(Vec::new());
		}
		self.lines.push(vec![(Tone::Heading, text.to_string())]);
	}

	fn kv(&mut self, key: &str, value: impl Into<String>, tone: Tone) {
		self.lines
			.push(vec![(Tone::Key, format!("  {key:<16} ")), (tone, value.into())]);
	}

	fn note(&mut self, tone: Tone, text: impl Into<String>) {
		self.lines.push(vec![(tone, format!("  {}", text.into()))]);
	}

	/// The diagnosis as plain text.
	#[cfg(test)]
	pub fn to_text(&self) -> String {
		let mut out = format!("── {} ──\n", self.title);
		for line in &self.lines {
			for (_, text) in line {
				out += text;
			}
			out.push('\n');
		}
		out
	}
}

/// The trace index `query` names: `#<seq>`, or the latest entry whose text
/// contains it (ignoring case).
pub fn find(rec: &Recording, query: &str) -> Option<usize> {
	let query = query.trim();
	if let Some(seq) = query.strip_prefix('#').and_then(|s| s.parse::<u64>().ok()) {
		return rec.trace.iter().position(|e| e.seq == seq);
	}
	let needle = query.to_ascii_lowercase();
	rec.trace
		.iter()
		.rposition(|e| e.describe().to_ascii_lowercase().contains(&needle))
}

pub fn diagnose(rec: &Recording, index: usize, snapshot: Option<&Snapshot>) -> Diagnosis {
	let Some(entry) = rec.trace.get(index) else {
		return Diagnosis::new("nothing focused".into());
	};
	let mut d = match &entry.kind {
		Kind::Call {
			suite,
			function,
			detail,
			last,
		} => call(rec, index, suite, function, detail, last.as_deref(), snapshot),
		Kind::Begin(_) | Kind::End { .. } => command(rec, index, snapshot),
		Kind::Alloc { .. } => allocation(rec, index, snapshot),
		Kind::Checkout { kind, id } => {
			let mut d = Diagnosis::new(format!("{kind:?} #{id}"));
			where_it_ran(&mut d, rec, index);
			d
		}
		Kind::Step(text) => step(rec, index, text),
	};
	neighbourhood(&mut d, rec, index);
	d
}

//==== Calls ============================================================

fn call(
	rec: &Recording,
	index: usize,
	suite: &'static str,
	function: &'static str,
	detail: &CallDetail,
	last: Option<&CallDetail>,
	snapshot: Option<&Snapshot>,
) -> Diagnosis {
	let entry = &rec.trace[index];
	let mut d = Diagnosis::new(format!("{suite}::{function}"));
	where_it_ran(&mut d, rec, index);
	if entry.repeat > 1 {
		d.kv(
			"repeated",
			format!(
				"×{} in a row (first shown{})",
				entry.repeat,
				if last.is_some() { ", last below" } else { ", all alike" }
			),
			Tone::Plain,
		);
	}

	if detail.args.is_empty() && detail.result.is_none() {
		d.heading("arguments");
		let level = snapshot.map(|s| s.level);
		if level == Some(aexlo::ObserveLevel::Arguments) {
			d.note(Tone::Dim, "this callback reports no arguments");
		} else {
			d.note(Tone::Warn, "not recorded: arguments are kept at `level args`");
		}
	} else {
		arguments(&mut d, rec, index, "arguments", detail, snapshot);
		if let Some(last) = last {
			arguments(&mut d, rec, index, "last repeat's arguments", last, snapshot);
		}
	}

	memory_it_moved(&mut d, rec, index, snapshot);

	d.heading("this function");
	if let Some(stat) = rec.calls.get(&(suite, function)) {
		d.kv(
			"calls",
			format!("{} in all, {} in the latest step", stat.count, stat.in_step),
			Tone::Plain,
		);
		d.kv(
			"called from",
			stat.commands
				.iter()
				.map(|c| format!("PF_Cmd_{c}"))
				.collect::<Vec<_>>()
				.join(", "),
			Tone::Plain,
		);
	}
	let same: Vec<&Entry> = rec
		.trace
		.iter()
		.filter(|e| matches!(&e.kind, Kind::Call { suite: s, function: f, .. } if *s == suite && *f == function))
		.collect();
	if let Some(position) = same.iter().position(|e| e.seq == entry.seq) {
		d.kv(
			"occurrence",
			format!("{} of {} in the trace", position + 1, same.len()),
			Tone::Plain,
		);
		if let Some(previous) = position.checked_sub(1).and_then(|p| same.get(p)) {
			d.kv(
				"previous",
				format!("#{} {}", previous.seq, previous.describe()),
				Tone::Dim,
			);
		}
	}
	d
}

fn arguments(
	d: &mut Diagnosis,
	rec: &Recording,
	index: usize,
	title: &str,
	detail: &CallDetail,
	snapshot: Option<&Snapshot>,
) {
	d.heading(title);
	for (name, value) in &detail.args {
		let (tone, note) = judge_value(rec, index, name, value, snapshot);
		let mut line = vec![(Tone::Key, format!("  {name:<16} ")), (tone, value.clone())];
		if let Some(note) = note {
			line.push((Tone::Dim, "  ← ".into()));
			line.push((tone, note));
		}
		d.lines.push(line);
	}
	if let Some(result) = &detail.result {
		let (tone, note) = judge_value(rec, index, "result", result, snapshot);
		let tone = if tone == Tone::Plain { Tone::Good } else { tone };
		let mut line = vec![(Tone::Key, format!("  {:<16} ", "→ result")), (tone, result.clone())];
		if let Some(note) = note {
			line.push((Tone::Dim, "  ← ".into()));
			line.push((tone, note));
		}
		d.lines.push(line);
	}
}

/// The allocations and frees the host made right after a call, on its
/// behalf (`host_new_handle` → `+ alloc`), and what became of them.
fn memory_it_moved(d: &mut Diagnosis, rec: &Recording, index: usize, snapshot: Option<&Snapshot>) {
	let depth = rec.trace[index].depth;
	let moved: Vec<&Entry> = rec
		.trace
		.range(index + 1..)
		.take_while(|e| e.depth == depth && matches!(e.kind, Kind::Alloc { .. }))
		.collect();
	if moved.is_empty() {
		return;
	}
	d.heading("memory it moved");
	for e in moved {
		let Kind::Alloc { allocated, address, .. } = e.kind else {
			continue;
		};
		d.note(Tone::Plain, format!("#{} {}", e.seq, e.describe()));
		if !allocated {
			continue;
		}
		let freed = rec.trace.iter().find(|x| {
			x.seq > e.seq && matches!(x.kind, Kind::Alloc { allocated: false, address: a, .. } if a == address)
		});
		let live = snapshot.is_some_and(|s| s.allocations.iter().any(|a| a.address == address));
		match freed {
			Some(free) => d.note(
				Tone::Good,
				format!(
					"  freed at #{} ({} later)",
					free.seq,
					fmt_duration(free.at.saturating_sub(e.at))
				),
			),
			None if live => d.note(Tone::Warn, "  never freed: still held"),
			None => d.note(Tone::Dim, "  no traced free, and no longer live"),
		}
	}
}

/// What an argument value says: a null pointer, or memory the trace knows.
fn judge_value(
	rec: &Recording,
	index: usize,
	name: &str,
	value: &str,
	snapshot: Option<&Snapshot>,
) -> (Tone, Option<String>) {
	let trimmed = value.trim();
	if trimmed == "0x0" || trimmed == "(null)" {
		let optional = name.contains("(out)") || name == "refcon" || name == "src";
		return if optional {
			(Tone::Dim, Some("null".into()))
		} else {
			(Tone::Warn, Some("null pointer".into()))
		};
	}
	let Some(address) = parse_address(trimmed) else {
		return (Tone::Plain, None);
	};
	let mut labels = Vec::new();
	if let Some(s) = snapshot {
		if Some(address) == s.state.global_data {
			labels.push("global_data".to_string());
		}
		if Some(address) == s.state.sequence_data {
			labels.push("sequence_data".to_string());
		}
	}
	let label = if labels.is_empty() {
		String::new()
	} else {
		format!(" ({})", labels.join(", "))
	};
	match lifecycle_at(rec, address, index) {
		Lifecycle::Live {
			kind,
			bytes,
			seq,
			command,
		} => (
			Tone::Good,
			Some(format!(
				"live {kind:?}, {bytes} B, allocated at #{seq} in PF_Cmd_{command}{label}"
			)),
		),
		Lifecycle::Freed { kind, seq, command } => (
			Tone::Bad,
			Some(format!(
				"{kind:?} already freed at #{seq} in PF_Cmd_{command}: use after free{label}"
			)),
		),
		Lifecycle::Later { seq } => (Tone::Warn, Some(format!("not allocated until #{seq}{label}"))),
		Lifecycle::Unknown => {
			let live = snapshot.and_then(|s| s.allocations.iter().find(|a| a.address == address));
			match live {
				Some(a) => (Tone::Good, Some(format!("live {:?}, {} B{label}", a.kind, a.bytes))),
				None if !label.is_empty() => (Tone::Plain, Some(label.trim().to_string())),
				None => (Tone::Plain, None),
			}
		}
	}
}

/// The first non-null `0x…` address in `value` (`0x1f`, `Some(0x1f)`, ...).
fn parse_address(value: &str) -> Option<usize> {
	let start = value.find("0x")?;
	let hex: String = value[start + 2..].chars().take_while(char::is_ascii_hexdigit).collect();
	usize::from_str_radix(&hex, 16).ok().filter(|&a| a != 0)
}

enum Lifecycle {
	Live {
		kind: aexlo::AllocationKind,
		bytes: usize,
		seq: u64,
		command: &'static str,
	},
	Freed {
		kind: aexlo::AllocationKind,
		seq: u64,
		command: &'static str,
	},
	Later {
		seq: u64,
	},
	Unknown,
}

/// What the trace says `address` was at trace position `index`.
fn lifecycle_at(rec: &Recording, address: usize, index: usize) -> Lifecycle {
	let mut state = Lifecycle::Unknown;
	for (i, e) in rec.trace.iter().enumerate() {
		let Kind::Alloc {
			kind,
			allocated,
			address: a,
			bytes,
		} = &e.kind
		else {
			continue;
		};
		if *a != address {
			continue;
		}
		if i > index {
			if matches!(state, Lifecycle::Unknown) && *allocated {
				return Lifecycle::Later { seq: e.seq };
			}
			break;
		}
		let command = enclosing_command(rec, i).map_or("?", |b| command_name(&rec.trace[b]));
		state = if *allocated {
			Lifecycle::Live {
				kind: *kind,
				bytes: *bytes,
				seq: e.seq,
				command,
			}
		} else {
			Lifecycle::Freed {
				kind: *kind,
				seq: e.seq,
				command,
			}
		};
	}
	state
}

//==== Commands =========================================================

fn command(rec: &Recording, index: usize, snapshot: Option<&Snapshot>) -> Diagnosis {
	let (begin, end) = match &rec.trace[index].kind {
		Kind::Begin(_) => (Some(index), matching_end(rec, index)),
		_ => (matching_begin(rec, index), Some(index)),
	};
	let name = command_name(&rec.trace[index]);
	let mut d = Diagnosis::new(format!("PF_Cmd_{name}"));
	where_it_ran(&mut d, rec, begin.unwrap_or(index));

	match end.map(|e| &rec.trace[e].kind) {
		Some(Kind::End { duration, error, .. }) => {
			d.kv("took", fmt_duration(*duration), Tone::Plain);
			match error {
				Some(code) => {
					d.kv("returned", err_name(*code), Tone::Bad);
					d.note(Tone::Warn, err_hint(*code));
				}
				None => d.kv("returned", "PF_Err_NONE", Tone::Good),
			}
		}
		_ => d.kv("returned", "still running (or dropped from the trace)", Tone::Warn),
	}

	let (Some(begin), last) = (begin, end.unwrap_or(rec.trace.len() - 1)) else {
		d.note(Tone::Dim, "its start was dropped from the trace");
		return d;
	};
	let depth = rec.trace[begin].depth;
	let inside = || rec.trace.range(begin + 1..last).filter(move |e| e.depth > depth);

	let mut functions: HashMap<String, u64> = HashMap::new();
	let (mut calls, mut allocs, mut frees, mut bytes, mut checkouts, mut nested) = (0u64, 0, 0, 0usize, 0, 0);
	let mut allocated: Vec<(usize, u64, aexlo::AllocationKind, usize)> = Vec::new();
	for e in inside() {
		match &e.kind {
			Kind::Call { suite, function, .. } => {
				calls += u64::from(e.repeat);
				*functions.entry(format!("{suite}::{function}")).or_default() += u64::from(e.repeat);
			}
			Kind::Alloc {
				allocated: true,
				address,
				bytes: b,
				kind,
			} => {
				allocs += 1;
				bytes += b;
				allocated.push((*address, e.seq, *kind, *b));
			}
			Kind::Alloc { .. } => frees += 1,
			Kind::Checkout { .. } => checkouts += 1,
			Kind::Begin(_) if e.depth == depth + 1 => nested += 1,
			_ => {}
		}
	}

	d.heading("inside it");
	d.kv(
		"suite calls",
		format!("{calls} ({} functions)", functions.len()),
		Tone::Plain,
	);
	let mut top: Vec<_> = functions.into_iter().collect();
	top.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
	for (function, count) in top.iter().take(6) {
		d.note(Tone::Dim, format!("  {count:>6}  {function}"));
	}
	d.kv(
		"memory",
		format!("{allocs} allocations ({bytes} B), {frees} frees"),
		Tone::Plain,
	);
	if checkouts > 0 {
		d.kv("checkouts", checkouts.to_string(), Tone::Plain);
	}
	if nested > 0 {
		d.kv("nested commands", nested.to_string(), Tone::Plain);
	}

	// What it allocated and never gave back, as far as the trace goes.
	let render = name.contains("RENDER") && name != "SMART_PRE_RENDER";
	let kept: Vec<_> = allocated
		.iter()
		.filter(|(address, seq, _, _)| !freed_after(rec, *address, *seq))
		.collect();
	if !kept.is_empty() {
		d.heading("still held");
		for (address, seq, kind, b) in kept {
			let live = snapshot.is_none_or(|s| s.allocations.iter().any(|a| a.address == *address));
			let world_leak = render && *kind == aexlo::AllocationKind::World;
			let tone = if world_leak { Tone::Bad } else { Tone::Warn };
			let why = if world_leak {
				"a render's worlds must be disposed before it returns"
			} else if live {
				"never freed so far (fine for global/sequence data)"
			} else {
				"gone without a traced free (freed off the plugin's thread?)"
			};
			d.note(tone, format!("{kind:?} {address:#x} ({b} B) from #{seq}: {why}"));
		}
	}

	if let Some(Kind::End { error: Some(_), .. }) = end.map(|e| &rec.trace[e].kind)
		&& let Some(call) = rec
			.trace
			.range(begin + 1..last)
			.rev()
			.find(|e| matches!(e.kind, Kind::Call { .. }))
	{
		d.heading("last call before the error");
		d.note(Tone::Warn, format!("#{} {}", call.seq, call.describe()));
	}
	d
}

// The constants' types differ (`_bindgen_ty_*`), so some casts are no-ops.
#[allow(clippy::unnecessary_cast)]
fn err_hint(code: i32) -> &'static str {
	use aexlo::sys::*;
	match code as u32 {
		c if c == PF_Err_OUT_OF_MEMORY as u32 => "the plugin ran out of memory, or a host allocation failed",
		c if c == PF_Err_INTERNAL_STRUCT_DAMAGED as u32 => {
			"a generic failure; aexlo also reports a C++ exception or a failing host callback this way"
		}
		c if c == PF_Err_INVALID_INDEX as u32 => "a parameter or layer index was out of range",
		c if c == PF_Err_UNRECOGNIZED_PARAM_TYPE as u32 => "a parameter type the plugin or host does not know",
		c if c == PF_Err_INVALID_CALLBACK as u32 => "a callback the host does not provide (check the suite versions)",
		c if c == PF_Err_BAD_CALLBACK_PARAM as u32 => "a bad argument reached a host callback (see the calls above)",
		c if c == PF_Interrupt_CANCEL as u32 => "the command was cancelled",
		_ => "a plugin-defined error code",
	}
}

fn freed_after(rec: &Recording, address: usize, seq: u64) -> bool {
	rec.trace
		.iter()
		.any(|e| e.seq > seq && matches!(e.kind, Kind::Alloc { allocated: false, address: a, .. } if a == address))
}

//==== Allocations ======================================================

fn allocation(rec: &Recording, index: usize, snapshot: Option<&Snapshot>) -> Diagnosis {
	let entry = &rec.trace[index];
	let Kind::Alloc {
		kind,
		allocated,
		address,
		bytes,
	} = entry.kind
	else {
		unreachable!()
	};
	let verb = if allocated { "allocated" } else { "freed" };
	let mut d = Diagnosis::new(format!("{kind:?} {address:#x} {verb}"));
	where_it_ran(&mut d, rec, index);
	d.kv("size", format!("{bytes} B"), Tone::Plain);

	d.heading("lifetime");
	let events: Vec<&Entry> = rec
		.trace
		.iter()
		.filter(|e| matches!(e.kind, Kind::Alloc { address: a, .. } if a == address))
		.collect();
	for e in &events {
		let command = rec
			.trace
			.iter()
			.position(|x| x.seq == e.seq)
			.and_then(|i| enclosing_command(rec, i))
			.map_or("?", |b| command_name(&rec.trace[b]));
		let marker = if e.seq == entry.seq { "▶" } else { " " };
		let tone = if matches!(e.kind, Kind::Alloc { allocated: true, .. }) {
			Tone::Good
		} else {
			Tone::Warn
		};
		d.note(
			tone,
			format!("{marker} #{} {} in PF_Cmd_{command}", e.seq, e.describe()),
		);
	}
	let live = snapshot.and_then(|s| s.allocations.iter().find(|a| a.address == address));
	match live {
		Some(a) => d.kv("now", format!("live ({} B)", a.bytes), Tone::Plain),
		None => d.kv("now", "not live", Tone::Dim),
	}
	if let Some(s) = snapshot {
		if Some(address) == s.state.global_data {
			d.kv("role", "global_data", Tone::Plain);
		}
		if Some(address) == s.state.sequence_data {
			d.kv("role", "sequence_data", Tone::Plain);
		}
	}

	// Calls whose arguments name it.
	let text = format!("{address:#x}");
	let users: Vec<&Entry> = rec
		.trace
		.iter()
		.filter(|e| match &e.kind {
			Kind::Call { detail, .. } => detail.args.iter().any(|(_, v)| v.contains(&text)),
			_ => false,
		})
		.collect();
	d.heading(&format!("calls naming it ({})", users.len()));
	if users.is_empty() {
		d.note(Tone::Dim, "none recorded (arguments are kept at `level args`)");
	}
	for e in users.iter().take(8) {
		d.note(Tone::Plain, format!("#{} {}", e.seq, e.describe()));
	}
	d
}

//==== Steps ============================================================

fn step(rec: &Recording, index: usize, text: &str) -> Diagnosis {
	let mut d = Diagnosis::new(format!("» {text}"));
	let end = rec
		.trace
		.iter()
		.skip(index + 1)
		.position(|e| matches!(e.kind, Kind::Step(_)))
		.map_or(rec.trace.len(), |p| index + 1 + p);
	let (mut commands, mut calls, mut allocs, mut frees) = (Vec::new(), 0u64, 0, 0);
	let mut errors = Vec::new();
	for e in rec.trace.range(index + 1..end) {
		match &e.kind {
			Kind::Begin(name) if e.depth == 0 => commands.push(*name),
			Kind::End {
				name,
				error: Some(code),
				..
			} => errors.push(format!("PF_Cmd_{name}: {}", err_name(*code))),
			Kind::Call { .. } => calls += u64::from(e.repeat),
			Kind::Alloc { allocated: true, .. } => allocs += 1,
			Kind::Alloc { .. } => frees += 1,
			_ => {}
		}
	}
	d.heading("what it caused");
	d.kv(
		"commands",
		if commands.is_empty() {
			"none".into()
		} else {
			commands.join(" → ")
		},
		Tone::Plain,
	);
	d.kv("suite calls", calls.to_string(), Tone::Plain);
	d.kv("memory", format!("{allocs} allocations, {frees} frees"), Tone::Plain);
	if errors.is_empty() {
		d.kv("errors", "none", Tone::Good);
	}
	for error in errors {
		d.note(Tone::Bad, error);
	}
	d
}

//==== Context ==========================================================

/// The command and user step an entry ran under, and when.
fn where_it_ran(d: &mut Diagnosis, rec: &Recording, index: usize) {
	let entry = &rec.trace[index];
	d.kv(
		"entry",
		format!(
			"#{} at {:.3} ms, depth {}",
			entry.seq,
			entry.at.as_secs_f64() * 1e3,
			entry.depth
		),
		Tone::Dim,
	);
	if let Some(begin) = enclosing_command(rec, index) {
		let b = &rec.trace[begin];
		let into = entry.at.saturating_sub(b.at);
		let mut chain = vec![command_name(b)];
		let mut at = begin;
		while let Some(outer) = enclosing_command(rec, at) {
			chain.push(command_name(&rec.trace[outer]));
			at = outer;
		}
		chain.reverse();
		let chain: Vec<String> = chain.iter().map(|c| format!("PF_Cmd_{c}")).collect();
		d.kv(
			"inside",
			format!("{}  (+{})", chain.join(" › "), fmt_duration(into)),
			Tone::Plain,
		);
	}
	if let Some(step) = rec.trace.range(..=index).rev().find_map(|e| match &e.kind {
		Kind::Step(text) => Some(text),
		_ => None,
	}) {
		d.kv("after", format!("» {step}"), Tone::Plain);
	}
}

/// How much of a neighbouring line is shown.
const NEIGHBOUR_WIDTH: usize = 96;

/// A few entries either side of `index`.
fn neighbourhood(d: &mut Diagnosis, rec: &Recording, index: usize) {
	d.heading("around it");
	let start = index.saturating_sub(3);
	let end = (index + 4).min(rec.trace.len());
	for (i, e) in rec.trace.iter().enumerate().take(end).skip(start) {
		let (marker, tone) = if i == index {
			("▶", Tone::Plain)
		} else {
			(" ", Tone::Dim)
		};
		let mut text = e.describe();
		if let Some((cut, _)) = text.char_indices().nth(NEIGHBOUR_WIDTH) {
			text.truncate(cut);
			text.push('…');
		}
		d.note(tone, format!("{marker} #{:<6} {}{text}", e.seq, "│ ".repeat(e.depth)));
	}
}

/// The `Begin` of the command `index` ran inside.
pub fn enclosing_command(rec: &Recording, index: usize) -> Option<usize> {
	// A command's own Begin and End sit at its depth, and what runs inside
	// it one deeper: the enclosing Begin is one level up from any entry.
	let target = rec.trace.get(index)?.depth.checked_sub(1)?;
	let mut skip = 0;
	for i in (0..index).rev() {
		let e = &rec.trace[i];
		if e.depth != target {
			continue;
		}
		match e.kind {
			Kind::End { .. } => skip += 1,
			Kind::Begin(_) if skip == 0 => return Some(i),
			Kind::Begin(_) => skip -= 1,
			_ => {}
		}
	}
	None
}

fn matching_end(rec: &Recording, begin: usize) -> Option<usize> {
	let depth = rec.trace[begin].depth;
	(begin + 1..rec.trace.len()).find(|&i| rec.trace[i].depth == depth && matches!(rec.trace[i].kind, Kind::End { .. }))
}

fn matching_begin(rec: &Recording, end: usize) -> Option<usize> {
	let depth = rec.trace[end].depth;
	(0..end)
		.rev()
		.find(|&i| rec.trace[i].depth == depth && matches!(rec.trace[i].kind, Kind::Begin(_)))
}

fn command_name(entry: &Entry) -> &'static str {
	match entry.kind {
		Kind::Begin(name) | Kind::End { name, .. } => name,
		_ => "?",
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::inspect::recorder::Recorder;
	use aexlo::CommandPhase;

	fn detail(args: &[(&'static str, &str)], result: Option<&str>) -> CallDetail {
		CallDetail {
			args: args.iter().map(|(n, v)| (*n, v.to_string())).collect(),
			result: result.map(str::to_string),
		}
	}

	/// » render ▶ RENDER { a ▶ FRAME_SETUP { } ◀ new_handle } ◀ RENDER(err)
	fn sample() -> Recorder {
		let rec = Recorder::default();
		rec.step("render");
		rec.on_command("RENDER", CommandPhase::Begin, None, None);
		rec.on_call("Suite", "a", detail(&[("index", "1")], None));
		rec.on_command("FRAME_SETUP", CommandPhase::Begin, None, None);
		rec.on_command("FRAME_SETUP", CommandPhase::End, None, None);
		rec.on_call("Handles", "new_handle", detail(&[("size", "64")], Some("0x1000")));
		rec.on_command("RENDER", CommandPhase::End, None, Some(512));
		rec
	}

	#[test]
	fn finds_by_text_or_seq_and_places_a_call_in_its_command() {
		let rec = sample();
		let r = rec.lock();
		let call = find(&r, "NEW_HANDLE").expect("case-insensitive match");
		assert!(matches!(
			r.trace[call].kind,
			Kind::Call {
				function: "new_handle",
				..
			}
		));
		assert_eq!(find(&r, &format!("#{}", r.trace[call].seq)), Some(call));
		assert_eq!(find(&r, "nothing like this"), None);

		// The call after a nested command's End still belongs to RENDER.
		let render = enclosing_command(&r, call).expect("inside a command");
		assert!(matches!(r.trace[render].kind, Kind::Begin("RENDER")));
		let setup = find(&r, "▶ PF_Cmd_FRAME_SETUP").unwrap();
		assert_eq!(enclosing_command(&r, setup), Some(render));
		assert_eq!(enclosing_command(&r, render), None);
	}

	#[test]
	fn diagnoses_arguments_results_and_errors() {
		let rec = sample();
		let r = rec.lock();
		let text = diagnose(&r, find(&r, "new_handle").unwrap(), None).to_text();
		for expected in [
			"Handles::new_handle",
			"PF_Cmd_RENDER",
			"size",
			"64",
			"0x1000",
			"around it",
		] {
			assert!(text.contains(expected), "missing {expected:?} in:\n{text}");
		}
		let text = diagnose(&r, find(&r, "◀ PF_Cmd_RENDER").unwrap(), None).to_text();
		assert!(text.contains("INTERNAL_STRUCT_DAMAGED"), "{text}");
		assert!(
			text.contains("Handles::new_handle"),
			"the last call before the error:\n{text}"
		);
	}
}
