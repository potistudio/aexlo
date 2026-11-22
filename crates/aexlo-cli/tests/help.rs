//! `aexlo help`, `--help` and command lines that name no command.

mod common;

#[test]
fn help_lists_commands_and_shows_each_ones_options() {
	let dir = common::scratch("help");

	let out = common::aexlo(&["help"], &dir);
	assert!(out.status.success());
	let text = String::from_utf8_lossy(&out.stdout);
	assert!(text.contains("render") && text.contains("check"), "{text}");
	assert!(!text.contains("\n  worker "), "the worker stays hidden: {text}");

	for args in [
		&["help", "render"][..],
		&["render", "--help"],
		&["render", "x.plugin", "-h"],
	] {
		let out = common::aexlo(args, &dir);
		assert!(out.status.success(), "{args:?}");
		let text = String::from_utf8_lossy(&out.stdout);
		assert!(
			text.contains("Usage: aexlo render") && text.contains("--serial-iterate"),
			"{args:?}: {text}"
		);
	}

	let out = common::aexlo(&["test", "--help"], &dir);
	let text = String::from_utf8_lossy(&out.stdout);
	assert!(
		text.contains("--bless") && text.contains("Preset options") && text.contains("Exit codes"),
		"{text}"
	);
}

#[test]
fn usage_errors_exit_2() {
	let dir = common::scratch("help_usage");

	let out = common::aexlo(&[], &dir);
	assert_eq!(out.status.code(), Some(2));
	assert!(String::from_utf8_lossy(&out.stderr).contains("Usage: aexlo <COMMAND>"));

	for args in [
		&["nope"][..],
		&["help", "nope"],
		&["render", "--no-such-flag"],
		&["render", "x.plugin", "--smart", "--legacy"],
		&["dev", "--web"],
	] {
		let out = common::aexlo(args, &dir);
		assert_eq!(
			out.status.code(),
			Some(2),
			"{args:?}: {}",
			String::from_utf8_lossy(&out.stderr)
		);
	}
}

#[test]
fn bench_tells_plugins_from_presets() {
	let dir = common::scratch("help_bench");
	// No plugin by that name: a filter, so the plugin options do not apply.
	let out = common::aexlo(&["bench", "no-such-plugin", "--samples", "3"], &dir);
	assert_eq!(out.status.code(), Some(2));
	assert!(String::from_utf8_lossy(&out.stderr).contains("only apply to timing plugins"));
}
