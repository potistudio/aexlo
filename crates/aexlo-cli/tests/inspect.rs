//! `aexlo inspect --exec`: commands run against the plugin, then the
//! analysis of what it was asked, called, allocated and returned.
mod common;

fn inspect(scratch: &str, args: &[&str]) -> (bool, String) {
	let plugin = common::misbehave_artifact();
	let dir = common::scratch(scratch);
	let out = std::process::Command::new(env!("CARGO_BIN_EXE_aexlo"))
		.current_dir(&dir)
		.arg("inspect")
		.arg(&plugin)
		.args(args)
		.output()
		.expect("spawning aexlo");
	let text = format!(
		"{}{}",
		String::from_utf8_lossy(&out.stdout),
		String::from_utf8_lossy(&out.stderr)
	);
	(out.status.success(), text)
}

#[test]
fn exec_reports_commands_calls_memory_and_errors() {
	let (ok, out) = inspect(
		"inspect_exec",
		&[
			"--size",
			"16x8",
			"--trace",
			"--exec",
			"set Mode Leak; render legacy; render smart; set Mode Error; render legacy",
		],
	);
	assert!(!ok, "a failed command fails the run:\n{out}");
	for expected in [
		"» set Mode Leak  ✓",
		"» render legacy  ✗",
		// The trace nests what a command calls under it.
		"▶ PF_Cmd_GLOBAL_SETUP",
		"  + alloc Handle",
		"  PF_SmartRenderCallbacks::checkout_layer_pixels",
		// Totals.
		"PF_Cmd_SMART_RENDER",
		"PF_SmartRenderCallbacks::checkout_output  [SMART_RENDER]",
		"PF_Cmd_RENDER returned PF_Err_INTERNAL_STRUCT_DAMAGED (512)",
		// The handles the Leak mode left behind, one per render, with their bytes.
		"== live memory (2)",
		"64 B from RENDER",
		"64 B from SMART_RENDER",
		"    00 00 00 00",
		"  1  Mode                     Popup        5",
	] {
		assert!(out.contains(expected), "missing {expected:?} in:\n{out}");
	}
}

#[test]
fn strict_findings_and_bad_commands_are_reported() {
	let (ok, out) = inspect(
		"inspect_strict",
		&["--strict", "--exec", "set Mode Leak; render legacy; reload"],
	);
	assert!(ok, "{out}");
	assert!(out.contains("== strict findings"), "{out}");
	assert!(out.contains("leak: Handle"), "{out}");

	let (ok, out) = inspect("inspect_bad", &["--exec", "render; frobnicate"]);
	assert!(!ok);
	assert!(out.contains("unknown command 'frobnicate'"), "{out}");
}

#[test]
fn focus_diagnoses_a_call_and_a_failed_command() {
	let (ok, out) = inspect(
		"inspect_focus",
		&[
			"--size",
			"16x8",
			"--exec",
			"set Mode Leak; render smart; focus host_new_handle; set Mode Error; render legacy; focus ◀ PF_Cmd_RENDER",
		],
	);
	assert!(!ok, "the failed render fails the run:\n{out}");
	for expected in [
		"── PF_HandleSuite1::host_new_handle ──",
		// Its arguments, and the handle it left behind.
		"size             64",
		"+ alloc Handle",
		"never freed: still held",
		// The failed command: its error, explained.
		"PF_Err_INTERNAL_STRUCT_DAMAGED (512)",
		"around it",
	] {
		assert!(out.contains(expected), "missing {expected:?} in:\n{out}");
	}

	let (ok, out) = inspect("inspect_focus_none", &["--exec", "focus no_such_call"]);
	assert!(!ok, "{out}");
}

/// `aexlo inspect <plugin> --format json <args>`, with `stdin` piped in:
/// the exit status and one JSON value per stdout line.
fn inspect_json(scratch: &str, args: &[&str], stdin: &str) -> (bool, Vec<serde_json::Value>) {
	use std::io::Write;
	let plugin = common::misbehave_artifact();
	let dir = common::scratch(scratch);
	let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_aexlo"))
		.current_dir(&dir)
		.arg("inspect")
		.arg(&plugin)
		.args(["--format", "json"])
		.args(args)
		.stdin(std::process::Stdio::piped())
		.stdout(std::process::Stdio::piped())
		.stderr(std::process::Stdio::piped())
		.spawn()
		.expect("spawning aexlo");
	child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
	let out = child.wait_with_output().unwrap();
	let stdout = String::from_utf8_lossy(&out.stdout);
	let events = stdout
		.lines()
		.map(|line| {
			serde_json::from_str(line).unwrap_or_else(|e| {
				panic!(
					"not a JSON line ({e}): {line:?}\nstdout:\n{stdout}\nstderr:\n{}",
					String::from_utf8_lossy(&out.stderr)
				)
			})
		})
		.collect();
	(out.status.success(), events)
}

fn events<'a>(all: &'a [serde_json::Value], name: &str) -> Vec<&'a serde_json::Value> {
	all.iter().filter(|e| e["event"] == name).collect()
}

#[test]
fn json_format_prints_one_event_per_line() {
	let (ok, all) = inspect_json(
		"inspect_json",
		&[
			"--size",
			"16x8",
			"--exec",
			"set Mode Leak; render smart; focus host_new_handle; set Mode Error; render legacy",
		],
		"",
	);
	assert!(!ok, "the failed render fails the run: {all:#?}");
	assert_eq!(all[0]["event"], "loaded", "{all:#?}");

	let commands = events(&all, "command");
	assert_eq!(commands.len(), 4, "{all:#?}");
	let smart = commands[1];
	assert_eq!(smart["ok"], true);
	assert_eq!(smart["allocations"], 1, "the Leak mode's handle: {smart:#}");
	assert!(
		smart["commands"]
			.as_array()
			.unwrap()
			.iter()
			.any(|c| c["command"] == "PF_Cmd_SMART_RENDER"),
		"{smart:#}"
	);
	assert!(smart["step"].as_str().unwrap().starts_with('#'), "{smart:#}");
	let failed = commands[3];
	assert_eq!(failed["ok"], false);
	assert_eq!(failed["commands"][0]["error"]["name"], "PF_Err_INTERNAL_STRUCT_DAMAGED");

	let focus = events(&all, "focus");
	let focus = focus.first().expect("a focus event");
	assert_eq!(focus["entry"]["type"], "call");
	assert_eq!(focus["entry"]["function"], "host_new_handle");
	assert_eq!(focus["entry"]["args"][0]["name"], "size");
	let sections = focus["diagnosis"]["sections"].as_array().unwrap();
	let moved = sections
		.iter()
		.find(|s| s["heading"] == "memory it moved")
		.expect("what the call allocated");
	assert!(
		moved["items"].as_array().unwrap().iter().any(|i| i["tone"] == "warn"),
		"the handle is never freed: {moved:#}"
	);

	let report = all.last().unwrap();
	assert_eq!(report["event"], "report");
	assert_eq!(report["failed"], 1);
	assert_eq!(report["errors"][0]["command"], "PF_Cmd_RENDER");
	assert_eq!(report["memory"].as_array().unwrap().len(), 1, "{report:#}");
	assert_eq!(report["memory"][0]["allocated_in"], "PF_Cmd_SMART_RENDER");
	assert_eq!(report["params"][0]["name"], "Mode");
}

#[test]
fn script_from_stdin_streams_and_keeps_going_past_a_bad_line() {
	let (ok, all) = inspect_json(
		"inspect_stdin",
		&["--script", "-"],
		"render\nfrobnicate\nclear; render legacy\nreport\nquit\nrender\n",
	);
	assert!(!ok, "the unknown command counts as failed: {all:#?}");
	let kinds: Vec<&str> = all.iter().map(|e| e["event"].as_str().unwrap()).collect();
	assert_eq!(
		kinds,
		["loaded", "command", "command", "command", "command", "report", "report"],
		"`quit` stops before the last render: {all:#?}"
	);
	assert_eq!(all[2]["ok"], false);
	assert!(all[2]["error"].as_str().unwrap().contains("frobnicate"));
	// `clear` forgot the first render: only the legacy one is counted.
	let render = all[5]["commands"]
		.as_array()
		.unwrap()
		.iter()
		.find(|c| c["command"] == "PF_Cmd_RENDER")
		.expect("the legacy render");
	assert_eq!(render["count"], 1, "{:#}", all[5]);
}

#[test]
fn json_format_reports_a_missing_plugin_as_an_event() {
	let dir = common::scratch("inspect_json_missing");
	let out = common::aexlo(
		&[
			"inspect",
			"no_such_plugin.dylib",
			"--format",
			"json",
			"--exec",
			"render",
		],
		&dir,
	);
	assert!(!out.status.success());
	let line: serde_json::Value = serde_json::from_slice(&out.stdout).expect("one JSON line");
	assert_eq!(line["event"], "error");
	assert!(line["error"].as_str().unwrap().contains("no plugin"), "{line}");
}

#[test]
fn the_final_report_covers_global_setdown_and_its_leaks() {
	let (ok, all) = inspect_json(
		"inspect_final",
		&["--strict", "--exec", "set Mode Leak; report; render"],
		"",
	);
	assert!(ok, "{all:#?}");
	let reports = events(&all, "report");
	assert_eq!(reports.len(), 2, "{all:#?}");
	assert_eq!(
		reports[0]["findings"],
		serde_json::json!([]),
		"nothing is judged before SETDOWN"
	);
	let last = reports[1];
	let findings = last["findings"].as_array().unwrap();
	assert!(
		findings
			.iter()
			.any(|f| f.as_str().unwrap().contains("still held after GLOBAL_SETDOWN")),
		"{last:#}"
	);
	assert!(
		last["commands"]
			.as_array()
			.unwrap()
			.iter()
			.any(|c| c["command"] == "PF_Cmd_GLOBAL_SETDOWN"),
		"{last:#}"
	);
	// The state is the one before the unload: the leaked handle is listed.
	assert_eq!(last["memory"].as_array().unwrap().len(), 1, "{last:#}");
}

#[test]
fn set_warns_out_of_range_and_names_what_it_cannot_set() {
	let (ok, all) = inspect_json(
		"inspect_set",
		&[
			"--exec",
			"set mode 99; set Gain=9; set 0 1; save out.foo; strict poison, guard",
		],
		"",
	);
	assert!(!ok, "{all:#?}");
	let commands = events(&all, "command");
	let text = |i: usize, field: &str| commands[i][field].as_str().unwrap_or_default().to_string();
	assert!(text(0, "message").contains("outside the popup's choices"), "{all:#?}");
	assert!(text(1, "message").contains("outside the slider range"), "{all:#?}");
	assert!(text(2, "error").contains("#0 is the input layer"), "{all:#?}");
	assert!(text(3, "error").contains(".png or .exr"), "{all:#?}");
	assert_eq!(commands[4]["ok"], true, "{all:#?}");
}

#[test]
fn help_and_conflicting_scripts() {
	let dir = common::scratch("inspect_help");
	let out = common::aexlo(&["inspect", "--help"], &dir);
	assert!(out.status.success());
	let text = String::from_utf8_lossy(&out.stdout);
	assert!(text.contains("--format") && text.contains("PLUGIN COMMANDS"), "{text}");

	let out = common::aexlo(&["inspect", "x", "--exec", "about", "--script", "-"], &dir);
	assert!(!out.status.success());
	assert!(String::from_utf8_lossy(&out.stderr).contains("either --exec or --script"));
}
