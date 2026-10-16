//! M6: the browser viewer's preset picker and "Save as preset", driven over
//! its HTTP endpoints.

mod common;

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use common::*;

/// Kills the viewer when the test ends, pass or fail.
struct Viewer(Child);

impl Drop for Viewer {
	fn drop(&mut self) {
		let _ = self.0.kill();
		let _ = self.0.wait();
	}
}

/// Poll `/info` until `done` holds for it.
fn wait_for(url: &str, done: impl Fn(&serde_json::Value) -> bool) -> serde_json::Value {
	let deadline = Instant::now() + Duration::from_secs(20);
	loop {
		let info: serde_json::Value = serde_json::from_str(&http_get(&format!("{url}info"))).unwrap_or_default();
		if done(&info) {
			return info;
		}
		assert!(Instant::now() < deadline, "viewer never got there: {info:#}");
		std::thread::sleep(Duration::from_millis(100));
	}
}

#[test]
fn a_preset_saved_in_the_viewer_round_trips_into_aexlo_test() {
	let dir = scratch("viewer");
	let text = format!(
		"# Kept as written.\n[plugin]\ncrate = {}\n\n[defaults]\nsize = [24, 16]\ngolden = false\n\n[[preset]]\nname = \"gains\"\nparams = {{ Gain = {{ sweep = [0.5, 2.0] }} }}\n",
		toml_path(&misbehave_crate())
	);
	let path = manifest(&dir, &text);
	let artifact = misbehave_artifact();

	let mut child = Command::new(env!("CARGO_BIN_EXE_aexlo"))
		.args([
			"preview",
			artifact.to_str().unwrap(),
			"--manifest",
			path.to_str().unwrap(),
		])
		.args(["--preset", "gains[Gain=2]", "--port", "0", "--no-open"])
		.current_dir(&dir)
		.stdout(Stdio::piped())
		.stderr(Stdio::null())
		.spawn()
		.expect("starting aexlo preview");
	let stdout = child.stdout.take().unwrap();
	let viewer = Viewer(child);
	let mut lines = BufReader::new(stdout).lines();
	let url = lines
		.by_ref()
		.map_while(Result::ok)
		.find_map(|line| {
			line.split_whitespace()
				.find(|w| w.starts_with("http://"))
				.map(str::to_string)
		})
		.expect("the viewer prints its URL");

	// The picker lists the manifest's variants, on the one asked for.
	wait_for(&url, |info| {
		info["variant"] == "gains[Gain=2]" && info["timing"].is_object()
	});
	let presets: serde_json::Value = serde_json::from_str(&http_get(&format!("{url}presets"))).unwrap();
	assert_eq!(
		presets["variants"],
		serde_json::json!(["gains[Gain=0.5]", "gains[Gain=2]"])
	);
	assert_eq!(presets["current"], "gains[Gain=2]");

	// Edit a parameter and the time, then save.
	http_get(&format!("{url}set?i=3&v=4"));
	http_get(&format!("{url}time?frame=5"));
	wait_for(&url, |info| info["frame"] == 5);
	http_get(&format!("{url}save?name=tuned"));
	let info = wait_for(&url, |info| info["variant"] == "tuned");
	assert!(
		info["toast"].as_str().unwrap_or("").starts_with("saved preset 'tuned'"),
		"{info:#}"
	);

	// The viewer shows it...
	let presets: serde_json::Value = serde_json::from_str(&http_get(&format!("{url}presets"))).unwrap();
	assert_eq!(presets["current"], "tuned");
	drop(viewer);

	// ...the manifest keeps its formatting...
	let saved = std::fs::read_to_string(&path).unwrap();
	assert!(saved.starts_with(&text), "the original text is untouched:\n{saved}");
	assert!(
		saved.ends_with(
			"\n[[preset]]\nname = \"tuned\"\ninherits = \"gains\"\nparams = { Gain = 2.0, Delay = 4.0 }\ntime = { frame = 5 }\n"
		),
		"{saved}"
	);

	// ...and `aexlo test` runs it.
	let (code, report) = test_json(&path, &["tuned"]);
	assert_eq!(code, 0, "{report:#}");
	assert_eq!(outcomes(&report), [("tuned".to_string(), "pass".to_string())]);
}
