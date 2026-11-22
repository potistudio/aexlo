//! `aexlo help`, `--help` and command lines that name no command.

mod common;

#[test]
fn help_lists_commands_and_shows_each_ones_options() {
	let dir = common::scratch("help");

	let out = common::aexlo(&["help"], &dir);
	assert!(out.status.success());
	let text = String::from_utf8_lossy(&out.stdout);
	assert!(
		text.contains("render <plugin>") && text.contains("check [filter]"),
		"{text}"
	);

	for args in [
		&["help", "render"][..],
		&["render", "--help"],
		&["render", "x.plugin", "-h"],
	] {
		let out = common::aexlo(args, &dir);
		assert!(out.status.success(), "{args:?}");
		let text = String::from_utf8_lossy(&out.stdout);
		assert!(
			text.starts_with("Usage: aexlo render") && text.contains("--serial-iterate"),
			"{args:?}: {text}"
		);
	}

	let out = common::aexlo(&["test", "--help"], &dir);
	let text = String::from_utf8_lossy(&out.stdout);
	assert!(text.contains("--bless") && text.contains("Exit codes"), "{text}");
}

#[test]
fn no_command_or_an_unknown_one_exits_2() {
	let dir = common::scratch("help_unknown");

	let out = common::aexlo(&[], &dir);
	assert_eq!(out.status.code(), Some(2));
	assert!(String::from_utf8_lossy(&out.stderr).contains("Usage: aexlo <command>"));

	for args in [&["nope"][..], &["help", "nope"]] {
		let out = common::aexlo(args, &dir);
		assert_eq!(out.status.code(), Some(2), "{args:?}");
		assert!(String::from_utf8_lossy(&out.stderr).contains("unknown command 'nope'"));
	}
}
