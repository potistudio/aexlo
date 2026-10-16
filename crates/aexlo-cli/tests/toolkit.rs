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
	assert_eq!(crash["last_command"], "SMART_RENDER");
	assert_eq!(code, 1, "a crash is a judged failure");

	// Every variant in its own worker gives the same verdicts.
	let (code, report) = test_json(&manifest, &["--isolate", "variant", "--jobs", "3"]);
	assert_eq!(common::outcomes(&report), outcomes, "{report:#}");
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

/// M4: strict mode catches a write one row past the world (`bounds`) and a
/// handle never disposed (`allocations`), plus checkout misuse; a well
/// behaved render passes all of it.
#[test]
fn strict_mode_catches_overruns_leaks_and_checkout_misuse() {
	let dir = scratch("strict");
	let path = manifest(
		&dir,
		&format!(
			r#"
[plugin]
crate = {}

[defaults]
size = [32, 16]
golden = false
checks = ["finite", "coverage", "iterate-parallel"]

[[preset]]
name = "overruns"
params = {{ Mode = "Overrun" }}

[[preset]]
name = "overruns_legacy"
params = {{ Mode = "Overrun" }}
render = "legacy"

[[preset]]
name = "leaks"
params = {{ Mode = "Leak" }}

[[preset]]
name = "undeclared"
params = {{ Mode = "Undeclared" }}

[[preset]]
name = "param_leak"
params = {{ Mode = "ParamLeak" }}

[[preset]]
name = "deep_error"
params = {{ Mode = "Error" }}
depth = 16

[[preset]]
name = "fine"
depth = [8, 16]
checks = ["+depth-consistency"]
"#,
			toml_path(&misbehave_crate())
		),
	);
	let (code, report) = test_json(&path, &["--strict"]);
	assert_eq!(code, 1);
	let runs = report["runs"].as_array().unwrap();
	let message = |i: usize| runs[i]["message"].as_str().unwrap_or("").to_string();
	assert!(message(0).starts_with("bounds: output 32x16: "), "{}", message(0));
	assert!(message(0).contains("below the world"), "{}", message(0));
	assert!(message(1).starts_with("bounds: "), "{}", message(1));
	assert!(message(2).starts_with("allocations: Handle "), "{}", message(2));
	assert!(
		message(2).contains("allocated in PF_Cmd_SMART_RENDER"),
		"{}",
		message(2)
	);
	assert!(
		message(3).contains("which SMART_PRE_RENDER never declared"),
		"{}",
		message(3)
	);
	assert!(
		message(4).contains("layer param #4 and never checked it in"),
		"{}",
		message(4)
	);
	assert_eq!(runs[5]["outcome"], "error");
	let flags = runs[5]["checks"]
		.as_array()
		.unwrap()
		.iter()
		.find(|c| c["id"] == "flags")
		.unwrap();
	assert!(
		flags["message"]
			.as_str()
			.unwrap()
			.contains("declares 16 bpc but errors at 16 bpc")
	);
	for run in &runs[6..] {
		assert_eq!(run["outcome"], "pass", "{run:#}");
		let ids: Vec<&str> = run["checks"]
			.as_array()
			.unwrap()
			.iter()
			.map(|c| c["id"].as_str().unwrap())
			.collect();
		for id in [
			"bounds",
			"allocations",
			"checkouts",
			"iterate-parallel",
			"depth-consistency",
		] {
			assert!(ids.contains(&id), "{id} missing from {ids:?}");
		}
	}

	// Without --strict, only the presets' own checks run: the overrun renders.
	let (_, report) = test_json(&path, &["fine"]);
	let ids: Vec<&str> = report["runs"][0]["checks"]
		.as_array()
		.unwrap()
		.iter()
		.map(|c| c["id"].as_str().unwrap())
		.collect();
	assert!(!ids.contains(&"bounds"));
}

/// M4: fuzzing draws reproducible cases and prints failures as presets.
#[test]
fn fuzzing_is_reproducible_and_prints_failing_presets() {
	let dir = scratch("fuzz");
	let path = manifest(
		&dir,
		&format!(
			"[plugin]\ncrate = {}\n[defaults]\nsize = [16, 16]\ntimeout = 1\n[[preset]]\nname = \"base\"\n",
			toml_path(&misbehave_crate())
		),
	);
	let run = |seed: &str| {
		let out = aexlo(
			&[
				"test",
				"--manifest",
				path.to_str().unwrap(),
				"--fuzz",
				"4",
				"--seed",
				seed,
				"--jobs",
				"2",
			],
			&dir,
		);
		(out.status.code(), String::from_utf8_lossy(&out.stdout).into_owned())
	};
	let (code, first) = run("3");
	let (_, again) = run("3");
	let verdicts = |text: &str| -> Vec<String> {
		text.lines()
			.filter(|l| l.contains("base[fuzz="))
			.map(|l| l.split_whitespace().take(2).collect::<Vec<_>>().join(" "))
			.collect()
	};
	assert_eq!(verdicts(&first).len(), 4, "{first}");
	assert_eq!(verdicts(&first), verdicts(&again), "same seed, same cases");
	if code == Some(1) {
		assert!(first.contains("failing cases, ready to paste"), "{first}");
		assert!(first.contains("inherits = \"base\"\nparams = { Mode = "), "{first}");
	}
	let out = aexlo(&["test", "--manifest", path.to_str().unwrap(), "--seed", "1"], &dir);
	assert_eq!(out.status.code(), Some(2), "--seed without --fuzz");
}

/// M5: a render slowed beyond the threshold regresses against the saved
/// baseline (exit 1); the phase breakdown puts the time in the plugin.
#[test]
fn bench_baselines_catch_regressions() {
	let dir = scratch("bench");
	let write = |delay: u32| {
		manifest(
			&dir,
			&format!(
				r#"
[plugin]
crate = {}

[defaults]
size = [32, 32]
bench = {{ samples = 5, warmup = 1 }}

[[preset]]
name = "timed"
params = {{ Delay = {delay} }}

[[preset]]
name = "untimed"
bench = false
"#,
				toml_path(&misbehave_crate())
			),
		)
	};
	let bench = |path: &std::path::Path, extra: &[&str]| {
		let mut args = vec!["bench", "--format", "json", "--manifest", path.to_str().unwrap()];
		args.extend_from_slice(extra);
		let out = aexlo(&args, &dir);
		// Errors before any run (exit 2, 3) print no report.
		let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or(serde_json::Value::Null);
		(out.status.code(), report)
	};

	let path = write(2);
	let (code, report) = bench(&path, &["--save-baseline", "main"]);
	assert_eq!(code, Some(0), "{report:#}");
	assert_eq!(
		report["runs"].as_array().unwrap().len(),
		1,
		"bench = false is not timed"
	);
	let stats = &report["runs"][0]["stats"];
	assert_eq!(stats["samples"], 5);
	assert!(stats["phases"]["render"].as_f64().unwrap() >= 0.002, "{stats:#}");
	assert!(dir.join(".aexlo/baselines/main.json").exists());

	let (code, _) = bench(&path, &["--baseline", "main", "--threshold", "50%"]);
	assert_eq!(code, Some(0), "unchanged");

	let path = write(30);
	let (code, report) = bench(&path, &["--baseline", "main", "--threshold", "5%"]);
	assert_eq!(code, Some(1), "{report:#}");
	assert!(
		report["runs"][0]["message"]
			.as_str()
			.unwrap()
			.starts_with("regressed against baseline 'main'")
	);

	let (code, _) = bench(&path, &["--baseline", "nope"]);
	assert_eq!(code, Some(2), "a missing baseline is a bad flag value");
	std::fs::write(dir.join(".aexlo/baselines/main.json"), "{").unwrap();
	let (code, _) = bench(&path, &["--baseline", "main"]);
	assert_eq!(code, Some(3), "an unreadable baseline is a harness error");
}

/// M7: `aexlo check` is `test --strict`, plus `bench --baseline main` once
/// that baseline exists; parity failures against After Effects references
/// are listed apart from regressions.
#[test]
fn check_runs_strict_tests_parity_and_the_main_baseline() {
	let dir = scratch("check");
	let path = manifest(
		&dir,
		&format!(
			r#"
[plugin]
crate = {}

[defaults]
size = [16, 16]
bench = {{ samples = 3, warmup = 1 }}

[[preset]]
name = "clean"
golden = false

[[preset]]
name = "against_ae"
golden = {{ source = "ae", path = "golden/ae/against_ae.png" }}

[[preset]]
name = "leaky"
params = {{ Mode = "Leak" }}
golden = false
"#,
			toml_path(&misbehave_crate())
		),
	);
	// An "After Effects" reference that disagrees with what aexlo renders.
	std::fs::create_dir_all(dir.join("golden/ae")).unwrap();
	image::RgbaImage::from_pixel(16, 16, image::Rgba([255, 0, 0, 255]))
		.save(dir.join("golden/ae/against_ae.png"))
		.unwrap();

	let check = |extra: &[&str]| {
		let mut args = vec!["check", "--manifest", path.to_str().unwrap()];
		args.extend_from_slice(extra);
		let out = aexlo(&args, &dir);
		(out.status.code(), String::from_utf8_lossy(&out.stdout).into_owned())
	};
	let (code, out) = check(&[]);
	assert_eq!(code, Some(1), "{out}");
	assert!(out.contains("no baseline"), "{out}");
	let failed = out.split("failed:\n").nth(1).unwrap_or_default();
	let (failed, parity) = failed
		.split_once("parity with After Effects:\n")
		.unwrap_or((failed, ""));
	assert!(failed.contains("leaky allocations: "), "strict is on in check:\n{out}");
	assert!(!failed.contains("against_ae"), "{out}");
	assert!(
		parity.contains("against_ae parity: differs from golden/ae/against_ae.png"),
		"{out}"
	);

	// Once `main` exists, check also benches against it.
	let out = aexlo(
		&[
			"bench",
			"--manifest",
			path.to_str().unwrap(),
			"clean",
			"--save-baseline",
			"main",
		],
		&dir,
	);
	assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
	// Timing noise on a sub-millisecond render is not what this tests.
	let (_, out) = check(&["clean", "--format", "json", "--threshold", "1000%"]);
	let report: serde_json::Value = serde_json::from_str(&out).unwrap();
	let ids: Vec<&str> = report["runs"]
		.as_array()
		.unwrap()
		.iter()
		.map(|r| r["id"].as_str().unwrap())
		.collect();
	assert_eq!(ids, ["clean", "clean"], "the test run, then the bench run");
	assert_eq!(report["exit_code"], 0);
}
