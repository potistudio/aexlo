//! Every bundled plugin fixture renders without crashing or erroring, each
//! in its own worker so one bad binary cannot take the matrix down. Frames
//! land in `target/render_test_output/` for inspection.

mod common;

use common::*;

/// Fixtures whose pixel callbacks must run on the thread that called
/// iterate: they acquire suites in the callback through the Rust
/// after-effects crate's thread-local.
const SERIAL_ITERATE: &[&str] = &["AOD_MobiusTransform"];

#[test]
fn all_fixtures_render() {
	let (platform, ext) = if cfg!(target_os = "windows") {
		("windows", "aex")
	} else {
		("macos", "plugin")
	};
	let fixtures = workspace().join("fixtures/plugins").join(platform);
	let mut plugins: Vec<_> = std::fs::read_dir(&fixtures)
		.unwrap_or_else(|e| panic!("fixtures dir not found at {}: {e}", fixtures.display()))
		.map(|entry| entry.unwrap().path())
		.filter(|path| path.extension().and_then(|e| e.to_str()) == Some(ext))
		.collect();
	plugins.sort();
	assert!(!plugins.is_empty(), "no plugin fixtures in {}", fixtures.display());

	let mut text = format!(
		"[plugin]\nartifact = {}\n\n[defaults]\ninput = {{ path = {} }}\nchecks = []\ngolden = false\ntimeout = 60\n",
		toml_path(&plugins[0]),
		toml_path(&workspace().join("input.png")),
	);
	for plugin in &plugins {
		let stem = plugin.file_stem().unwrap().to_string_lossy();
		let name: String = stem
			.chars()
			.map(|c| {
				if c.is_ascii_alphanumeric() {
					c.to_ascii_lowercase()
				} else {
					'_'
				}
			})
			.collect();
		text.push_str(&format!(
			"\n[[preset]]\nname = \"{name}\"\nplugin = {{ artifact = {} }}\n",
			toml_path(plugin)
		));
		if SERIAL_ITERATE.contains(&stem.as_ref()) {
			text.push_str("iterate = \"serial\"\n");
		}
	}

	let dir = scratch("render_matrix");
	let manifest = manifest(&dir, &text);
	let output = workspace().join("target/render_test_output");
	let (code, report) = test_json(
		&manifest,
		&[
			"--isolate",
			"variant",
			"--jobs",
			"4",
			"--save-frames",
			output.to_str().unwrap(),
		],
	);
	let failures: Vec<String> = report["runs"]
		.as_array()
		.unwrap()
		.iter()
		.filter(|r| r["outcome"] != "pass")
		.map(|r| format!("{}: {} ({})", r["id"], r["outcome"], r["message"]))
		.collect();
	assert!(
		failures.is_empty() && code == 0,
		"{}/{} plugins failed to render:\n{}",
		failures.len(),
		plugins.len(),
		failures.join("\n")
	);
}
