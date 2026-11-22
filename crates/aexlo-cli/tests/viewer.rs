//! M6: the browser viewer's preset picker, "Save as preset" and button
//! parameters, driven over its HTTP endpoints.

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

/// Start `aexlo preview` on the misbehaving effect with `manifest` and
/// `args`, returning the running viewer and its URL.
fn start(dir: &std::path::Path, manifest: &std::path::Path, args: &[&str]) -> (Viewer, String) {
	let artifact = misbehave_artifact();
	let mut child = Command::new(env!("CARGO_BIN_EXE_aexlo"))
		.args([
			"preview",
			artifact.to_str().unwrap(),
			"--manifest",
			manifest.to_str().unwrap(),
		])
		.args(args)
		.args(["--port", "0", "--no-open"])
		.current_dir(dir)
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
	// Keep reading, so the viewer never writes into a closed pipe.
	std::thread::spawn(move || lines.for_each(drop));
	(viewer, url)
}

/// The viewer's parameter list.
fn params(url: &str) -> Vec<serde_json::Value> {
	serde_json::from_str(&http_get(&format!("{url}params"))).unwrap_or_default()
}

/// Poll `/params` until `done` holds for it.
fn wait_for_params(url: &str, done: impl Fn(&[serde_json::Value]) -> bool) -> Vec<serde_json::Value> {
	let deadline = Instant::now() + Duration::from_secs(20);
	loop {
		let params = params(url);
		if done(&params) {
			return params;
		}
		assert!(Instant::now() < deadline, "the parameters never got there: {params:#?}");
		std::thread::sleep(Duration::from_millis(100));
	}
}

fn gain(params: &[serde_json::Value]) -> Option<f64> {
	params.iter().find(|p| p["name"] == "Gain")?["value"].as_f64()
}

#[test]
fn a_button_in_the_viewer_presses_and_its_change_is_kept() {
	let dir = scratch("viewer_button");
	let text = format!(
		"[plugin]\ncrate = {}\n\n[defaults]\nsize = [24, 16]\ngolden = false\n\n[[preset]]\nname = \"loud\"\nparams = {{ Gain = 3.0 }}\n",
		toml_path(&misbehave_crate())
	);
	let path = manifest(&dir, &text);
	let (viewer, url) = start(&dir, &path, &["--preset", "loud"]);

	// The button comes through with its name, the label on it, and its place.
	let before = wait_for_params(&url, |p| gain(p) == Some(3.0));
	let button = before
		.iter()
		.find(|p| p["kind"] == "button")
		.expect("the viewer lists the button");
	assert_eq!(button["index"], 5);
	assert_eq!(button["name"], "Defaults");
	assert_eq!(button["label"], "Reset Gain");
	assert_eq!(button["disabled"], false);

	// Pressing it sets Gain back to 1, and the controls show it.
	http_get(&format!("{url}press?i=5"));
	wait_for_params(&url, |p| gain(p) == Some(1.0));

	// A parameter that is not a button can't be pressed.
	http_get(&format!("{url}press?i=2"));
	let info = wait_for(&url, |info| info["toast"].is_string());
	assert!(info["toast"].as_str().unwrap().contains("not a button"), "{info:#}");

	// The value the plugin changed is an edit, so a saved preset keeps it.
	http_get(&format!("{url}save?name=reset"));
	wait_for(&url, |info| info["variant"] == "reset");
	drop(viewer);
	let saved = std::fs::read_to_string(&path).unwrap();
	assert!(
		saved.ends_with("\n[[preset]]\nname = \"reset\"\ninherits = \"loud\"\nparams = { Gain = 1.0 }\n"),
		"{saved}"
	);
}

#[test]
fn a_preset_saved_in_the_viewer_round_trips_into_aexlo_test() {
	let dir = scratch("viewer");
	let text = format!(
		"# Kept as written.\n[plugin]\ncrate = {}\n\n[defaults]\nsize = [24, 16]\ngolden = false\n\n[[preset]]\nname = \"gains\"\nparams = {{ Gain = {{ sweep = [0.5, 2.0] }} }}\n",
		toml_path(&misbehave_crate())
	);
	let path = manifest(&dir, &text);
	let (viewer, url) = start(&dir, &path, &["--preset", "gains[Gain=2]"]);

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
