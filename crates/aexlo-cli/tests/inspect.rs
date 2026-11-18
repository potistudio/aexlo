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
