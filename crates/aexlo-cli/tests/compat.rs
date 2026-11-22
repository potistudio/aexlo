//! `aexlo compat` over the misbehaving test effect: each of its `Mode`s is a
//! scenario, so one run shows every status a scenario can end in, isolated
//! from the others, and a baseline catches a scenario that got worse.

mod common;

use common::*;

/// `aexlo compat --json` over `plugin`: the exit code and the report.
fn compat_json(plugin: &std::path::Path, extra: &[&str]) -> (i32, serde_json::Value) {
	let mut args = vec!["compat", plugin.to_str().unwrap(), "--json", "--timeout", "2"];
	args.extend_from_slice(extra);
	let out = aexlo(&args, &workspace());
	let stdout = String::from_utf8_lossy(&out.stdout);
	let report = serde_json::from_str(&stdout).unwrap_or_else(|e| {
		panic!(
			"not a JSON report ({e}), {}:\nstdout:\n{stdout}\nstderr:\n{}",
			out.status,
			String::from_utf8_lossy(&out.stderr)
		)
	});
	(out.status.code().unwrap_or(-1), report)
}

/// The status of scenario `id` of the report's only plugin.
fn status(report: &serde_json::Value, id: &str) -> String {
	let plugin = &report["plugins"][0];
	if id == "load" {
		return plugin["load"]["status"].as_str().unwrap().to_string();
	}
	plugin["scenarios"]
		.as_array()
		.unwrap()
		.iter()
		.find(|s| s["id"] == id)
		.unwrap_or_else(|| panic!("no scenario {id} in {report:#}"))["status"]
		.as_str()
		.unwrap()
		.to_string()
}

#[test]
fn every_way_a_scenario_ends_is_its_own_status() {
	let plugin = misbehave_artifact();
	let (code, report) = compat_json(&plugin, &["--only", "Mode="]);
	assert_eq!(code, 0, "{report:#}");
	assert_eq!(status(&report, "load"), "ok");
	assert_eq!(status(&report, "param:#1 Mode=OK"), "ok");
	assert_eq!(status(&report, "param:#1 Mode=Crash"), "crash");
	assert_eq!(status(&report, "param:#1 Mode=Hang"), "timeout");
	assert_eq!(status(&report, "param:#1 Mode=Error"), "error");
	let crash = report["plugins"][0]["scenarios"]
		.as_array()
		.unwrap()
		.iter()
		.find(|s| s["id"] == "param:#1 Mode=Crash")
		.unwrap();
	assert!(crash["last_command"].as_str().is_some(), "{crash:#}");
}

#[test]
fn the_matrix_covers_paths_depths_inputs_sizes_and_times() {
	let plugin = misbehave_artifact();
	let (_, report) = compat_json(&plugin, &["--only", ":"]);
	let ids: Vec<&str> = report["plugins"][0]["scenarios"]
		.as_array()
		.unwrap()
		.iter()
		.map(|s| s["id"].as_str().unwrap())
		.collect();
	for id in [
		"render:auto@8",
		"render:legacy@16",
		"render:smart@32",
		"render:gpu@8",
		"input:transparent",
		"size:1x1",
		"time:90@30",
		"iterate:serial",
		"param:#2 Gain=0",
	] {
		assert!(ids.contains(&id), "{id} missing from {ids:?}");
	}
	// The effect declares no GPU render: that is not applicable, not a failure.
	assert_eq!(status(&report, "render:gpu@8"), "n/a");
	assert_eq!(status(&report, "render:legacy@16"), "ok");
}

#[test]
fn a_scenario_worse_than_the_baseline_exits_1() {
	let plugin = misbehave_artifact();
	let dir = scratch("compat_baseline");
	let baseline = dir.join("baseline.json");
	let baseline_arg = baseline.to_str().unwrap();
	let only = ["--only", "Mode=Crash"];

	let (code, _) = compat_json(&plugin, &[&only[..], &["--save-baseline", baseline_arg]].concat());
	assert_eq!(code, 0);
	let (code, report) = compat_json(&plugin, &[&only[..], &["--baseline", baseline_arg]].concat());
	assert_eq!(code, 0, "unchanged: {report:#}");
	assert_eq!(report["worse"].as_array().unwrap().len(), 0);

	// Pretend the crash used to render.
	let text = std::fs::read_to_string(&baseline).unwrap();
	std::fs::write(
		&baseline,
		text.replace(r#""param:#1 Mode=Crash": "crash""#, r#""param:#1 Mode=Crash": "ok""#),
	)
	.unwrap();
	let (code, report) = compat_json(&plugin, &[&only[..], &["--baseline", baseline_arg]].concat());
	assert_eq!(code, 1, "{report:#}");
	let worse = &report["worse"][0];
	assert_eq!(worse["id"], "param:#1 Mode=Crash");
	assert_eq!(worse["was"], "ok");
	assert_eq!(worse["now"], "crash");
}
