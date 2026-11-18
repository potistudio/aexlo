//! `--format json`: what `--exec` and `--script` print, as one JSON object
//! per line (NDJSON), for agents and scripts.
//!
//! Every object has an `event`:
//!
//! - `loaded`: the plugin is loaded and takes commands.
//! - `command`: one command ran (`ok`, `elapsed_us`, `message` or `error`,
//!   and what it caused: `step` is the `#seq` `focus` takes, `commands` the
//!   `PF_Cmd_*` it issued with their errors, plus its suite calls and
//!   allocations).
//! - `focus`: the diagnosis of a trace entry, in `sections` of `items`;
//!   `tone` is `good`, `warn` or `bad` where the analysis judges a value.
//! - `report`: the analysis (`report`, and once more at the end).
//! - `error`: the plugin could not be loaded.

use aexlo::Strict;
use serde_json::{Value, json};

use super::diagnose::{Diagnosis, Tone};
use super::engine::Snapshot;
use super::recorder::{CallDetail, Entry, Kind, Recording, err_name};
use super::{MemoryDump, role};

/// `0x7f...`, the way the text output and `focus`/`mem` write addresses.
fn hex(address: usize) -> String {
	format!("{address:#x}")
}

/// `PF_Err_OUT_OF_MEMORY` (without the number `err_name` appends).
fn err_json(code: i32) -> Value {
	let name = err_name(code);
	let name = name.split_once(" (").map_or(name.as_str(), |(name, _)| name);
	json!({ "code": code, "name": name })
}

fn micros(d: std::time::Duration) -> u64 {
	d.as_micros().try_into().unwrap_or(u64::MAX)
}

/// `{"bits": 0x..., "names": ["DEEP_COLOR_AWARE", ...]}` from a flag set's
/// `Name(A | B | 0x40)` debug text.
fn flags(bits: u32, debug: String) -> Value {
	let inner = debug
		.split_once('(')
		.and_then(|(_, rest)| rest.strip_suffix(')'))
		.unwrap_or_default();
	let names: Vec<&str> = inner.split(" | ").filter(|n| !n.is_empty() && *n != "0x0").collect();
	json!({ "bits": bits, "names": names })
}

fn detail(detail: &CallDetail) -> Value {
	let args: Vec<Value> = detail
		.args
		.iter()
		.map(|(name, value)| json!({ "name": name, "value": value }))
		.collect();
	json!({ "args": args, "result": detail.result })
}

/// One trace entry.
pub fn entry(e: &Entry) -> Value {
	let mut out = json!({
		"seq": e.seq,
		"at_us": micros(e.at),
		"depth": e.depth,
		"repeat": e.repeat,
		"text": e.describe(),
	});
	let fields = match &e.kind {
		Kind::Step(text) => json!({ "type": "step", "line": text }),
		Kind::Begin(name) => json!({ "type": "begin", "command": format!("PF_Cmd_{name}") }),
		Kind::End { name, duration, error } => json!({
			"type": "end",
			"command": format!("PF_Cmd_{name}"),
			"elapsed_us": micros(*duration),
			"error": error.map(err_json),
		}),
		Kind::Call {
			suite,
			function,
			detail: first,
			last,
		} => {
			let mut call = json!({ "type": "call", "suite": suite, "function": function });
			merge(&mut call, detail(first));
			call["last"] = last.as_deref().map_or(Value::Null, detail);
			call
		}
		Kind::Alloc {
			kind,
			allocated,
			address,
			bytes,
		} => json!({
			"type": if *allocated { "alloc" } else { "free" },
			"kind": format!("{kind:?}"),
			"address": hex(*address),
			"bytes": bytes,
		}),
		Kind::Checkout { kind, id } => json!({ "type": "checkout", "kind": format!("{kind:?}"), "id": id }),
	};
	merge(&mut out, fields);
	out
}

fn merge(into: &mut Value, from: Value) {
	if let (Value::Object(into), Value::Object(from)) = (into, from) {
		into.extend(from);
	}
}

/// What the user command whose step entry has `seq` caused, up to the next
/// step: the `PF_Cmd_*` it issued (outermost only), its suite calls and
/// allocations.
pub fn caused(rec: &Recording, seq: Option<u64>) -> Value {
	let Some(start) = seq.and_then(|seq| rec.trace.iter().position(|e| e.seq == seq)) else {
		return json!({ "step": null, "commands": [], "suite_calls": 0, "allocations": 0, "frees": 0 });
	};
	let (mut commands, mut calls, mut allocs, mut frees) = (Vec::new(), 0u64, 0u64, 0u64);
	for e in rec.trace.range(start + 1..) {
		match &e.kind {
			Kind::Step(_) => break,
			Kind::End { name, duration, error } if e.depth == 0 => commands.push(json!({
				"command": format!("PF_Cmd_{name}"),
				"elapsed_us": micros(*duration),
				"error": error.map(err_json),
			})),
			Kind::Call { .. } => calls += u64::from(e.repeat),
			Kind::Alloc { allocated: true, .. } => allocs += 1,
			Kind::Alloc { .. } => frees += 1,
			_ => {}
		}
	}
	json!({
		"step": format!("#{}", rec.trace[start].seq),
		"commands": commands,
		"suite_calls": calls,
		"allocations": allocs,
		"frees": frees,
	})
}

fn tone(tone: Tone) -> &'static str {
	match tone {
		Tone::Heading | Tone::Key | Tone::Plain => "plain",
		Tone::Good => "good",
		Tone::Warn => "warn",
		Tone::Bad => "bad",
		Tone::Dim => "dim",
	}
}

/// A diagnosis as `{title, sections: [{heading, items: [...]}]}`. An item is
/// `{key, value, tone, note?}` for a labelled line, `{text, tone}` otherwise.
pub fn diagnosis(d: &Diagnosis) -> Value {
	let mut sections = vec![json!({ "heading": null, "items": [] })];
	for line in &d.lines {
		match line.as_slice() {
			[] => {}
			[(Tone::Heading, heading)] => sections.push(json!({ "heading": heading, "items": [] })),
			[(Tone::Key, key), (t, value), rest @ ..] => {
				let mut item = json!({ "key": key.trim(), "value": value, "tone": tone(*t) });
				// `  ← note`, in the note's tone.
				if let [_, (t, note)] = rest {
					item["note"] = json!(note);
					item["tone"] = json!(tone(*t));
				}
				push_item(&mut sections, item);
			}
			spans => {
				let text: String = spans.iter().map(|(_, s)| s.as_str()).collect();
				let t = spans
					.iter()
					.map(|(t, _)| *t)
					.find(|t| *t != Tone::Key)
					.unwrap_or(Tone::Plain);
				push_item(&mut sections, json!({ "text": text.trim(), "tone": tone(t) }));
			}
		}
	}
	if sections[0]["items"].as_array().is_some_and(Vec::is_empty) {
		sections.remove(0);
	}
	json!({ "title": d.title, "sections": sections })
}

fn push_item(sections: &mut [Value], item: Value) {
	if let Some(Value::Array(items)) = sections.last_mut().map(|s| &mut s["items"]) {
		items.push(item);
	}
}

fn strict(s: Strict) -> Value {
	json!({
		"poison": s.poison_output,
		"guard": s.guard_bands,
		"alloc": s.track_allocations,
		"checkout": s.track_checkouts,
	})
}

/// The analysis `report` prints, as one object.
pub fn report(
	rec: &Recording,
	snapshot: Option<&Snapshot>,
	memory: Option<&[MemoryDump]>,
	print_trace: bool,
	failed: usize,
) -> Value {
	let mut commands: Vec<_> = rec.commands.iter().collect();
	commands.sort_by_key(|(_, s)| std::cmp::Reverse(s.total));
	let commands: Vec<Value> = commands
		.into_iter()
		.map(|(name, s)| {
			json!({
				"command": format!("PF_Cmd_{name}"),
				"count": s.count,
				"errors": s.errors,
				"max_us": micros(s.max),
				"total_us": micros(s.total),
			})
		})
		.collect();

	let mut calls: Vec<_> = rec.calls.iter().collect();
	calls.sort_by(|a, b| b.1.count.cmp(&a.1.count).then(a.0.cmp(b.0)));
	let calls: Vec<Value> = calls
		.into_iter()
		.map(|((suite, function), s)| {
			json!({
				"suite": suite,
				"function": function,
				"count": s.count,
				"commands": s.commands.iter().map(|c| format!("PF_Cmd_{c}")).collect::<Vec<_>>(),
			})
		})
		.collect();

	let errors: Vec<Value> = rec
		.trace
		.iter()
		.filter_map(|e| match &e.kind {
			Kind::End {
				name,
				error: Some(code),
				..
			} => Some(json!({
				"seq": e.seq,
				"command": format!("PF_Cmd_{name}"),
				"error": err_json(*code),
			})),
			_ => None,
		})
		.collect();

	let mut out = json!({
		"event": "report",
		"failed": failed,
		"trace": {
			"entries": rec.trace.len(),
			"dropped": rec.dropped,
		},
		"commands": commands,
		"suite_calls": calls,
		"errors": errors,
	});
	if print_trace {
		out["trace"]["lines"] = rec.trace.iter().map(entry).collect();
	}
	let Some(s) = snapshot else {
		out["loaded"] = json!(false);
		return out;
	};
	out["loaded"] = json!(true);
	let st = &s.state;
	out["plugin"] = json!({
		"path": s.path.display().to_string(),
		"smart_render": s.smart,
		"gpu": s.gpu,
		"custom_ui": s.custom_ui,
		"options_button": s.options_button,
		"pixel_formats": s.pixel_formats,
		"out_flags": flags(s.out_flags.bits(), format!("{:?}", s.out_flags)),
		"out_flags2": flags(s.out_flags2.bits(), format!("{:?}", s.out_flags2)),
	});
	out["state"] = json!({
		"global_data": st.global_data.map(hex),
		"sequence_data": st.sequence_data.map(hex),
		"time": { "value": st.current_time, "step": st.time_step, "scale": st.time_scale },
		"input": { "width": s.input.0, "height": s.input.1, "bpc": s.input.2.bits() },
		"output": { "width": s.output.0, "height": s.output.1, "bpc": s.output.2.bits() },
		"last_out_flags": flags(st.last_out_flags.bits(), format!("{:?}", st.last_out_flags)),
		"last_out_flags2": flags(st.last_out_flags2.bits(), format!("{:?}", st.last_out_flags2)),
		"last_message": st.last_message,
		"output_mean": s.mean,
		"unwritten_pixels": s.unwritten,
		"parallel_iterate": s.parallel_iterate,
		"strict": strict(s.strict),
	});
	out["params"] = s
		.params
		.iter()
		.map(|p| {
			json!({
				"index": p.index,
				"name": p.name,
				"kind": p.kind,
				"value": p.value,
				"detail": p.detail,
				"hidden": p.hidden,
				"disabled": p.disabled,
			})
		})
		.collect();
	out["memory"] = s
		.allocations
		.iter()
		.map(|a| {
			let head = memory
				.and_then(|m| m.iter().find(|(addr, _)| *addr == a.address))
				.and_then(|(_, bytes)| bytes.as_ref())
				.map(|b| b.iter().map(|x| format!("{x:02x}")).collect::<String>());
			json!({
				"kind": format!("{:?}", a.kind),
				"address": hex(a.address),
				"bytes": a.bytes,
				"allocated_in": format!("PF_Cmd_{}", a.allocated_in),
				"role": role(a.address, st),
				"head": head,
			})
		})
		.collect();
	out["findings"] = json!(s.findings);
	out
}
