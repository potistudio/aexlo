//! Record which fixtures and GPU this machine has, for `needs_fixture`.
//!
//! This is the only place that looks: cargo reruns it when `fixtures/plugins`
//! changes, which rebuilds this crate and with it the tests that use it.

use std::path::Path;

fn main() {
	println!("cargo::rerun-if-changed=build.rs");

	let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/plugins");
	// Watching a path that does not exist would rerun this on every build.
	if root.is_dir() {
		println!("cargo::rerun-if-changed={}", root.display());
	}

	// `cfg!` is the host here, which is where the macro runs and the tests do.
	let (platform, extension) = if cfg!(windows) {
		("windows", "aex")
	} else {
		("macos", "plugin")
	};
	let mut present: Vec<String> = std::fs::read_dir(root.join(platform))
		.into_iter()
		.flatten()
		.flatten()
		.map(|entry| entry.path())
		// `exists` follows links: a link to an absent plugin is no fixture.
		.filter(|path| path.extension().is_some_and(|e| e == extension) && path.exists())
		.filter_map(|path| path.file_stem().map(|stem| stem.to_string_lossy().into_owned()))
		.collect();
	present.sort();
	println!("cargo::rustc-env=FIXTURE_GATE_PRESENT={}", present.join("|"));
	println!("cargo::rustc-env=FIXTURE_GATE_GPU={}", has_gpu());
}

/// Whether aexlo's GPU path can have a device here: Metal on macOS, the
/// NVIDIA driver's CUDA on Windows.
fn has_gpu() -> bool {
	if cfg!(target_os = "macos") {
		return true;
	}
	std::env::var_os("SystemRoot").is_some_and(|root| Path::new(&root).join("System32/nvcuda.dll").exists())
}
