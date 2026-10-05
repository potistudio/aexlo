//! Helpers for driving the `aexlo` binary over generated manifests.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Output;

pub fn workspace() -> PathBuf {
	PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("../..")
		.canonicalize()
		.unwrap()
}

/// The misbehaving test effect's crate (`tests/misbehave`).
pub fn misbehave_crate() -> PathBuf {
	workspace().join("tests/misbehave")
}

/// A bundled fixture by file stem, if present on this machine.
pub fn fixture(stem: &str) -> Option<PathBuf> {
	let (dir, ext) = if cfg!(target_os = "windows") {
		("windows", "aex")
	} else {
		("macos", "plugin")
	};
	let path = workspace()
		.join("fixtures/plugins")
		.join(dir)
		.join(format!("{stem}.{ext}"));
	path.exists().then_some(path)
}

/// A fresh scratch directory under `target/`.
pub fn scratch(name: &str) -> PathBuf {
	let dir = workspace().join("target/aexlo-cli-tests").join(name);
	let _ = std::fs::remove_dir_all(&dir);
	std::fs::create_dir_all(&dir).unwrap();
	dir
}

/// Write `text` as `<dir>/aexlo.toml` and return its path.
pub fn manifest(dir: &Path, text: &str) -> PathBuf {
	let path = dir.join("aexlo.toml");
	std::fs::write(&path, text).unwrap();
	path
}

/// TOML string literal for a path.
pub fn toml_path(path: &Path) -> String {
	format!("'{}'", path.display())
}

/// Run `aexlo <args>` and return its output.
pub fn aexlo(args: &[&str], cwd: &Path) -> Output {
	std::process::Command::new(env!("CARGO_BIN_EXE_aexlo"))
		.args(args)
		.current_dir(cwd)
		.output()
		.expect("spawning aexlo")
}

/// `aexlo test --format json` over `manifest`: the exit code and the report.
pub fn test_json(manifest: &Path, extra: &[&str]) -> (i32, serde_json::Value) {
	let mut args = vec!["test", "--format", "json", "--manifest", manifest.to_str().unwrap()];
	args.extend_from_slice(extra);
	let out = aexlo(&args, manifest.parent().unwrap());
	let stdout = String::from_utf8_lossy(&out.stdout);
	let report = serde_json::from_str(&stdout).unwrap_or_else(|e| {
		panic!(
			"not a JSON report ({e}):\nstdout:\n{stdout}\nstderr:\n{}",
			String::from_utf8_lossy(&out.stderr)
		)
	});
	(out.status.code().unwrap_or(-1), report)
}

/// `(id, outcome)` for every run in a JSON report.
pub fn outcomes(report: &serde_json::Value) -> Vec<(String, String)> {
	report["runs"]
		.as_array()
		.unwrap()
		.iter()
		.map(|r| {
			(
				r["id"].as_str().unwrap().to_string(),
				r["outcome"].as_str().unwrap().to_string(),
			)
		})
		.collect()
}
