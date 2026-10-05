//! The toolkit's outcomes and exit codes (`docs/toolkit.md` §6.3, §12),
//! driven through the real `aexlo` binary.

mod common;

use common::*;

/// M2: a manifest over two plugins where one crashes. The crash is one
/// `crash` outcome; every other variant still runs.
#[test]
fn a_crash_fails_one_variant_and_the_rest_still_run() {
	let dir = scratch("crash_isolation");
	let other = fixture("FillColor").map_or_else(
		|| format!("plugin = {{ crate = {} }}", toml_path(&misbehave_crate())),
		|path| format!("plugin = {{ artifact = {} }}", toml_path(&path)),
	);
	let manifest = manifest(
		&dir,
		&format!(
			r#"
[plugin]
crate = {crate_dir}

[defaults]
size = [32, 24]
checks = []
golden = false

[[preset]]
name = "before"
{other}

[[preset]]
name = "crashes"
params = {{ Mode = "Crash" }}

[[preset]]
name = "after"
params = {{ Gain = {{ sweep = [0.5, 2.0] }} }}
"#,
			crate_dir = toml_path(&misbehave_crate()),
		),
	);

	let (code, report) = test_json(&manifest, &[]);
	let outcomes = outcomes(&report);
	assert_eq!(
		outcomes,
		[
			("before".to_string(), "pass".to_string()),
			("crashes".to_string(), "crash".to_string()),
			("after[Gain=0.5]".to_string(), "pass".to_string()),
			("after[Gain=2]".to_string(), "pass".to_string()),
		],
		"{report:#}"
	);
	let crash = &report["runs"][1];
	assert!(crash["message"].as_str().unwrap().contains("signal"), "{crash:#}");
	assert_eq!(crash["last_command"], "RENDER");
	assert_eq!(code, 1, "a crash is a judged failure");

	// Every variant in its own worker gives the same verdicts.
	let (code, report) = test_json(&manifest, &["--isolate", "variant", "--jobs", "3"]);
	assert_eq!(common::outcomes(&report), outcomes);
	assert_eq!(code, 1);
}

#[test]
fn errors_timeouts_and_skips_are_their_own_outcomes() {
	let dir = scratch("outcomes");
	let manifest = manifest(
		&dir,
		&format!(
			r#"
[plugin]
crate = {}

[defaults]
size = [16, 16]
checks = []
golden = false
timeout = 1

[[preset]]
name = "errors"
params = {{ Mode = "Error" }}

[[preset]]
name = "hangs"
params = {{ Mode = "Hang" }}

[[preset]]
name = "float"
depth = 32
"#,
			toml_path(&misbehave_crate())
		),
	);
	let (code, report) = test_json(&manifest, &[]);
	let outcomes: Vec<String> = common::outcomes(&report).into_iter().map(|(_, o)| o).collect();
	assert_eq!(outcomes, ["error", "timeout", "skipped"], "{report:#}");
	assert_eq!(code, 1);

	// Only the skipped variant: nothing judged bad.
	let (code, _) = test_json(&manifest, &["float"]);
	assert_eq!(code, 0);
}

#[test]
fn invalid_manifests_exit_2() {
	let dir = scratch("invalid");
	let crate_dir = toml_path(&misbehave_crate());
	for text in [
		format!("[plugin]\ncrate = {crate_dir}\n[[preset]]\nname = \"a\"\nsizee = [1, 1]\n"),
		format!("[plugin]\ncrate = {crate_dir}\n[[preset]]\nname = \"a\"\ninherits = \"a\"\n"),
		format!("[plugin]\ncrate = {crate_dir}\n[[preset]]\nname = \"a\"\nparams = {{ Nope = 1 }}\n"),
		format!("[plugin]\ncrate = {crate_dir}\n[[preset]]\nname = \"a\"\nparams = {{ Mode = \"Sideways\" }}\n"),
		"[plugin]\nartifact = 'x'\n[[preset]\n".to_string(),
	] {
		let path = manifest(&dir, &text);
		let out = aexlo(&["test", "--manifest", path.to_str().unwrap()], &dir);
		assert_eq!(
			out.status.code(),
			Some(2),
			"{text}\n{}",
			String::from_utf8_lossy(&out.stderr)
		);
	}
	let out = aexlo(&["test", "--jobs", "zero"], &dir);
	assert_eq!(out.status.code(), Some(2));
}

#[test]
fn harness_errors_exit_3() {
	let dir = scratch("harness_error");
	// A `crate` that does not build: no verdict was reached.
	let path = manifest(&dir, "[plugin]\ncrate = 'no-such-crate'\n[[preset]]\nname = \"a\"\n");
	let out = aexlo(&["test", "--manifest", path.to_str().unwrap()], &dir);
	assert_eq!(out.status.code(), Some(3), "{}", String::from_utf8_lossy(&out.stderr));
}

/// M3: a missing golden fails until blessed; a changed render then fails
/// with actual/expected/diff artifacts; AE goldens are never blessed over.
#[test]
fn goldens_bless_compare_and_explain() {
	let dir = scratch("goldens");
	let write = |gain: f64| {
		manifest(
			&dir,
			&format!(
				r#"
[plugin]
crate = {}

[defaults]
size = [24, 16]
checks = ["finite", "coverage", "deterministic"]

[[preset]]
name = "shaded"
depth = [8, 16]
params = {{ Gain = {gain} }}

[[preset]]
name = "parity"
params = {{ Gain = {gain} }}
golden = {{ source = "ae", path = "golden/ae/parity.png" }}
"#,
				toml_path(&misbehave_crate())
			),
		)
	};
	let path = write(1.0);

	let (code, report) = test_json(&path, &["shaded"]);
	assert_eq!(code, 1);
	assert!(
		report["runs"][0]["message"]
			.as_str()
			.unwrap()
			.contains("missing golden/shaded.d8.png")
	);
	assert!(dir.join("target/aexlo/shaded.d8/actual.png").exists());

	let (code, report) = test_json(&path, &["shaded", "--bless"]);
	assert_eq!(code, 0, "{report:#}");
	assert!(dir.join("golden/shaded.d8.png").exists());
	assert!(dir.join("golden/shaded.d16.png").exists());
	assert_eq!(report["runs"][0]["notes"][0], "blessed new golden/shaded.d8.png");

	let (code, _) = test_json(&path, &["shaded"]);
	assert_eq!(code, 0);

	// A different render now fails, leaving images to look at.
	let path = write(0.5);
	let (code, report) = test_json(&path, &["shaded"]);
	assert_eq!(code, 1);
	let message = report["runs"][1]["message"].as_str().unwrap();
	assert!(
		message.starts_with("golden: differs from golden/shaded.d16.png"),
		"{message}"
	);
	for file in ["actual.png", "expected.png", "diff.png"] {
		assert!(dir.join("target/aexlo/shaded.d16").join(file).exists(), "{file}");
	}

	// After Effects references are parity checks and never blessed.
	let (code, report) = test_json(&path, &["parity", "--bless"]);
	assert_eq!(code, 1);
	assert_eq!(report["runs"][0]["checks"][3]["id"], "parity");
	assert!(!dir.join("golden/ae/parity.png").exists());

	// A golden that cannot be decoded is a harness error.
	std::fs::write(dir.join("golden/shaded.d8.png"), b"not a png").unwrap();
	let (code, _) = test_json(&path, &["shaded"]);
	assert_eq!(code, 3);
}

/// `coverage` and `deterministic` catch what they are for; JUnit records it.
#[test]
fn checks_catch_unwritten_and_nondeterministic_output() {
	let dir = scratch("checks");
	let path = manifest(
		&dir,
		&format!(
			r#"
[plugin]
crate = {}

[defaults]
size = [16, 16]
golden = false

[[preset]]
name = "unwritten"
params = {{ Mode = "Unwritten" }}

[[preset]]
name = "random"
params = {{ Mode = "Random" }}

[[preset]]
name = "fine"
"#,
			toml_path(&misbehave_crate())
		),
	);
	let junit = dir.join("junit.xml");
	let (code, report) = test_json(&path, &["--junit", junit.to_str().unwrap()]);
	assert_eq!(code, 1);
	let messages: Vec<&str> = report["runs"]
		.as_array()
		.unwrap()
		.iter()
		.map(|r| r["message"].as_str().unwrap_or(""))
		.collect();
	assert!(
		messages[0].starts_with("coverage: 128 of 256 output pixel(s) never written"),
		"{messages:?}"
	);
	assert!(
		messages[1].starts_with("deterministic: the second render differs"),
		"{messages:?}"
	);
	assert_eq!(messages[2], "");

	let xml = std::fs::read_to_string(&junit).unwrap();
	assert!(
		xml.contains(r#"<testsuite name="unwritten" tests="1" failures="1""#),
		"{xml}"
	);
	assert!(xml.contains(r#"<testcase classname="fine" name="fine""#), "{xml}");
}
