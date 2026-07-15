//! Trace loading, normalization, and diffing.
//!
//! A raw trace is host-shaped noise (AE sends UPDATE_PARAMS_UI dozens of
//! times; command order varies). `summarize` boils a trace down to a flat
//! `key -> value` map of *behavioral facts* — which suites exist, what the
//! callbacks returned, what the worlds looked like — so two hosts can be
//! compared key-by-key regardless of scenario differences.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Context;
use serde_json::Value;

pub type Summary = BTreeMap<String, String>;

pub fn load_events(path: &Path) -> anyhow::Result<Vec<Value>> {
	let text = std::fs::read_to_string(path).with_context(|| format!("failed to read trace {}", path.display()))?;

	let mut events = Vec::new();
	for (number, line) in text.lines().enumerate() {
		if line.trim().is_empty() {
			continue;
		}
		let value: Value =
			serde_json::from_str(line).with_context(|| format!("{}:{}: invalid JSON", path.display(), number + 1))?;
		events.push(value);
	}

	anyhow::ensure!(!events.is_empty(), "trace {} contains no events", path.display());
	Ok(events)
}

fn compact(value: &Value) -> String {
	match value {
		Value::String(s) => s.clone(),
		other => other.to_string(),
	}
}

/// Flatten a JSON object into `prefix/key = value` entries.
fn flatten_into(summary: &mut Summary, prefix: &str, value: &Value) {
	match value {
		Value::Object(map) => {
			for (key, value) in map {
				flatten_into(summary, &format!("{prefix}/{key}"), value);
			}
		}
		other => {
			summary.insert(prefix.to_string(), compact(other));
		}
	}
}

pub fn summarize(events: &[Value]) -> Summary {
	let mut summary = Summary::new();
	let mut command_counts: BTreeMap<String, u64> = BTreeMap::new();

	for event in events {
		let kind = event["event"].as_str().unwrap_or("");
		match kind {
			"trace_open" => {
				summary.insert("host/exe".into(), compact(&event["exe"]));
				summary.insert("host/os".into(), compact(&event["os"]));
				summary.insert("host/probe_version".into(), compact(&event["probe_version"]));
			}

			"plugin_data_entry" => {
				summary.insert("host/plugin_data/host_name".into(), compact(&event["host_name"]));
				summary.insert("host/plugin_data/host_version".into(), compact(&event["host_version"]));
			}

			"cmd" => {
				let name = event["cmd"].as_str().unwrap_or("?").to_string();
				match event["phase"].as_str() {
					Some("begin") => {
						*command_counts.entry(name.clone()).or_default() += 1;

						let in_data = &event["in"];
						if !in_data.is_null() {
							// Host identity: last write wins, which lands on the
							// most fully-populated in_data the host ever sent.
							summary.insert("host/appl_id".into(), compact(&in_data["appl_id"]));
							summary.insert("host/spec_version".into(), compact(&in_data["spec_version"]));
							summary.insert("host/serial_num".into(), compact(&in_data["serial_num"]));
						}

						// The RENDER in_data is the interesting one: capture it
						// wholesale (first render only, to dodge MFR interleaving).
						if name == "RENDER" && !summary.contains_key("render/in/width") && !in_data.is_null() {
							flatten_into(&mut summary, "render/in", in_data);
						}
					}
					Some("end") => {
						summary.insert(format!("cmd/{name}/err"), compact(&event["err"]));

						if name == "GLOBAL_SETUP" {
							let out = &event["out"];
							summary.insert("global/out_flags".into(), compact(&out["out_flags"]));
							summary.insert("global/out_flags2".into(), compact(&out["out_flags2"]));
							summary.insert("global/my_version".into(), compact(&out["my_version"]));
						}
						if name == "PARAMS_SETUP" {
							summary.insert("global/num_params".into(), compact(&event["out"]["num_params"]));
						}
					}
					_ => {}
				}
			}

			"fact" => {
				summary.insert(
					format!("fact/{}", event["name"].as_str().unwrap_or("?")),
					compact(&event["value"]),
				);
			}

			"suite" => {
				let key = format!(
					"suite/{}/v{}",
					event["name"].as_str().unwrap_or("?"),
					event["version"].as_u64().unwrap_or(0)
				);
				let value = if event["ok"].as_bool().unwrap_or(false) {
					"ok".to_string()
				} else {
					format!("err={}", compact(&event["err"]))
				};
				summary.insert(key, value);
			}

			"callback" => {
				let name = event["name"].as_str().unwrap_or("?").to_string();
				let mut fields: Vec<String> = Vec::new();
				if let Some(map) = event.as_object() {
					for (key, value) in map {
						if !matches!(key.as_str(), "seq" | "t_ms" | "tid" | "event" | "name") {
							fields.push(format!("{key}={}", compact(value)));
						}
					}
				}
				summary.insert(format!("callback/{name}"), fields.join(" "));
			}

			"utils_presence" => {
				if let Some(map) = event.as_object() {
					for (key, value) in map {
						if !matches!(key.as_str(), "seq" | "t_ms" | "tid" | "event") {
							summary.insert(format!("utils/{key}"), compact(value));
						}
					}
				}
			}

			"add_param" => {
				let name = event["name"].as_str().unwrap_or("?");
				summary.insert(format!("setup/param/{name}/err"), compact(&event["err"]));
			}

			"param" => {
				let name = event["name"].as_str().unwrap_or("?");
				let key = format!("param/{name}");
				// First render wins; later renders may carry harness-modified values.
				summary
					.entry(key)
					.or_insert_with(|| format!("{} = {}", compact(&event["type"]), compact(&event["value"])));
			}

			"world" => {
				let which = event["which"].as_str().unwrap_or("?");
				let world = &event["world"];
				if !world.is_null() && !summary.contains_key(&format!("render/{which}/width")) {
					flatten_into(&mut summary, &format!("render/{which}"), world);
				}
			}

			"sequence" => {
				let what = event["what"].as_str().unwrap_or("?").to_lowercase();
				let mut fields: Vec<String> = Vec::new();
				if let Some(map) = event.as_object() {
					for (key, value) in map {
						if !matches!(key.as_str(), "seq" | "t_ms" | "tid" | "event" | "what") {
							fields.push(format!("{key}={}", compact(value)));
						}
					}
				}
				summary.insert(format!("sequence/{what}"), fields.join(" "));
			}

			"panic" => {
				summary.insert(
					format!("panic/{}", event["cmd"].as_str().unwrap_or("?")),
					compact(&event["msg"]),
				);
			}

			"note" => {
				summary.insert(
					format!("note/{}", event["msg"].as_str().unwrap_or("?")),
					"seen".to_string(),
				);
			}

			_ => {}
		}
	}

	for (name, count) in command_counts {
		summary.insert(format!("cmd/{name}/seen"), count.to_string());
	}

	summary
}

/// Keys that are deterministic *facts* about host behavior — fixed input,
/// exact output — and therefore comparable across a headless aexlo run and a
/// GUI After Effects session. Everything else (command order/counts, timing,
/// render context, parameter scenarios) depends on how the host was driven,
/// so the default diff treats it as context; `--all` compares it anyway.
pub fn is_comparable(key: &str) -> bool {
	key.starts_with("fact/")            // unit checks: one function, one suite, one variable
		|| key.starts_with("suite/")    // suite availability map
		|| key.starts_with("utils/")    // callback presence map
		|| key.starts_with("panic/")    // a probe panic on either side is always a finding
		|| key == "host/appl_id"
		|| key == "host/spec_version"
		|| key == "host/probe_version"  // comparing traces from different probe builds is a mistake
		|| key.starts_with("host/plugin_data/")
}

pub enum DiffLine {
	OnlyLeft(String, String),
	OnlyRight(String, String),
	Changed(String, String, String),
}

pub fn diff(left: &Summary, right: &Summary, include_all: bool) -> (Vec<DiffLine>, usize) {
	let mut lines = Vec::new();
	let mut matches = 0;

	let keys: std::collections::BTreeSet<&String> = left.keys().chain(right.keys()).collect();
	for key in keys {
		if !include_all && !is_comparable(key) {
			continue;
		}

		match (left.get(key), right.get(key)) {
			(Some(l), Some(r)) if l == r => matches += 1,
			(Some(l), Some(r)) => lines.push(DiffLine::Changed(key.clone(), l.clone(), r.clone())),
			(Some(l), None) => lines.push(DiffLine::OnlyLeft(key.clone(), l.clone())),
			(None, Some(r)) => lines.push(DiffLine::OnlyRight(key.clone(), r.clone())),
			(None, None) => unreachable!(),
		}
	}

	(lines, matches)
}
