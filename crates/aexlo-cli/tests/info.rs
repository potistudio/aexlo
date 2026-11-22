//! `aexlo info`: the PiPL, ABOUT text, flags and parameters of a plugin.

mod common;

use std::path::Path;

fn json(args: &[&str], dir: &Path) -> (i32, serde_json::Value) {
	let out = common::aexlo(args, dir);
	let stdout = String::from_utf8_lossy(&out.stdout);
	let value = serde_json::from_str(&stdout).unwrap_or_else(|e| {
		panic!(
			"not JSON ({e}):\nstdout:\n{stdout}\nstderr:\n{}",
			String::from_utf8_lossy(&out.stderr)
		)
	});
	(out.status.code().unwrap_or(-1), value)
}

#[test]
fn info_reports_every_section_and_compares_flags_with_the_pipl() {
	let Some(noise) = common::fixture("SDK_Noise") else {
		eprintln!("skipping: no SDK_Noise fixture");
		return;
	};
	let dir = common::scratch("info");
	let (code, report) = json(&["info", noise.to_str().unwrap(), "--json"], &dir);
	assert_eq!(code, 0, "{report}");
	let names: Vec<&str> = report["pipl"]["properties"]
		.as_array()
		.unwrap()
		.iter()
		.map(|p| p["key"].as_str().unwrap())
		.collect();
	assert!(names.contains(&"eGLO") && names.contains(&"eMNA"), "{names:?}");
	assert!(report["about"].as_str().unwrap().contains("SDK_Noise"), "{report}");
	let flags = &report["flags"];
	assert_eq!(flags["version"]["differs"], false, "{flags}");
	assert_eq!(
		flags["out_flags"]["pipl"], flags["out_flags"]["global_setup"],
		"{flags}"
	);
	assert_eq!(flags["out_flags2"]["differ"], serde_json::json!([]), "{flags}");
	assert_eq!(report["params"][0]["name"], "Noise variation", "{report}");

	// One section prints bare, the way `aexlo about` did.
	let out = common::aexlo(&["info", noise.to_str().unwrap(), "--about"], &dir);
	let text = String::from_utf8_lossy(&out.stdout);
	assert!(out.status.success() && text.starts_with("SDK_Noise"), "{text}");

	// Several print under headings.
	let out = common::aexlo(&["info", noise.to_str().unwrap(), "--pipl", "--flags"], &dir);
	let text = String::from_utf8_lossy(&out.stdout);
	assert!(
		text.starts_with("PiPL\n") && text.contains("\nFlags\n") && !text.contains("About"),
		"{text}"
	);
	assert!(text.contains("GLOBAL_SETUP matches the PiPL"), "{text}");
}

#[test]
fn info_prints_the_pipl_of_a_plugin_that_does_not_load() {
	let Some(noise) = common::fixture("SDK_Noise") else {
		eprintln!("skipping: no SDK_Noise fixture");
		return;
	};
	if !cfg!(target_os = "macos") {
		return;
	}
	// A bundle with SDK_Noise's resources and no binary.
	let dir = common::scratch("info_broken");
	let bundle = dir.join("Broken.plugin");
	let resources = bundle.join("Contents/Resources");
	std::fs::create_dir_all(&resources).unwrap();
	std::fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();
	std::fs::copy(
		noise.join("Contents/Resources/SDK_Noise.rsrc"),
		resources.join("SDK_Noise.rsrc"),
	)
	.unwrap();

	let out = common::aexlo(&["info", "Broken.plugin"], &dir);
	assert_eq!(out.status.code(), Some(1));
	let text = String::from_utf8_lossy(&out.stdout);
	assert!(text.contains("eMNA  \"ADBE SDK_Noise\""), "{text}");
	assert!(String::from_utf8_lossy(&out.stderr).contains("loading plugin"));

	// --pipl alone does not load it.
	let (code, report) = json(&["info", "Broken.plugin", "--pipl", "--json"], &dir);
	assert_eq!(code, 0, "{report}");
	assert!(report["pipl"]["source"].as_str().unwrap().ends_with("SDK_Noise.rsrc"));

	let (code, report) = json(&["info", "Broken.plugin", "--json"], &dir);
	assert_eq!(code, 1);
	assert!(report["pipl"].is_object() && report["error"].is_string(), "{report}");
}

#[test]
fn info_reports_a_missing_plugin_as_json() {
	let dir = common::scratch("info_missing");
	let (code, report) = json(&["info", "no-such.plugin", "--json"], &dir);
	assert_eq!(code, 1);
	assert!(report["error"].as_str().unwrap().contains("not found"), "{report}");
}
