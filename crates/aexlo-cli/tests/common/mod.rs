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
			"not a JSON report ({e}), {}:\nstdout:\n{stdout}\nstderr:\n{}",
			out.status,
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

/// Build the misbehaving effect and return its dynamic library.
pub fn misbehave_artifact() -> PathBuf {
	let status = std::process::Command::new(env!("CARGO"))
		.args(["build", "-q", "-p", "aexlo_misbehave"])
		.current_dir(workspace())
		.status()
		.expect("running cargo build");
	assert!(status.success(), "building aexlo_misbehave");
	let name = if cfg!(target_os = "windows") {
		"aexlo_misbehave.dll"
	} else if cfg!(target_os = "macos") {
		"libaexlo_misbehave.dylib"
	} else {
		"libaexlo_misbehave.so"
	};
	// Copy it away: cargo re-links target/debug outputs on every build, and
	// other tests build the same crate concurrently.
	let built = workspace().join("target/debug").join(name);
	let copy = scratch("misbehave_artifact").join(name);
	for _ in 0..50 {
		if std::fs::copy(&built, &copy).is_ok() {
			return copy;
		}
		std::thread::sleep(std::time::Duration::from_millis(20));
	}
	panic!("copying {}", built.display());
}

/// `GET <url>` over plain HTTP/1.0, returning the body.
pub fn http_get(url: &str) -> String {
	use std::io::{Read, Write};
	let rest = url.strip_prefix("http://").expect("http url");
	let (host, path) = rest
		.split_once('/')
		.map_or((rest, "/".to_string()), |(h, p)| (h, format!("/{p}")));
	let mut stream = std::net::TcpStream::connect(host).expect("connecting to the viewer");
	write!(stream, "GET {path} HTTP/1.0\r\nHost: {host}\r\n\r\n").unwrap();
	let mut response = String::new();
	stream.read_to_string(&mut response).unwrap();
	response
		.split_once("\r\n\r\n")
		.map(|(_, body)| body.to_string())
		.unwrap_or_default()
}
