//! Record which fixtures and GPU this machine has, for `needs_fixture`.
//!
//! This is the only place that looks: cargo reruns it when `fixtures/plugins`
//! changes, which rebuilds this crate and with it the tests that use it.
//!
//! One change goes unseen: a link's absent target appearing with an old
//! timestamp, as a copy that keeps times makes. The tests stay ignored until
//! `cargo clean -p aexlo_fixture_gate`; they never pass unrun.

use std::io::Read;
use std::path::Path;

fn main() {
	println!("cargo::rerun-if-changed=build.rs");

	let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/plugins");
	// `cfg!` is the host here, which is where the macro runs and the tests do.
	let (platform, extension) = if cfg!(windows) {
		("windows", "aex")
	} else {
		("macos", "plugin")
	};
	let dir = root.join(platform);
	// The platform's directory itself, as well as the root: cargo takes a
	// subdirectory's time from its parent's listing, which NTFS updates late,
	// so a removal would go unseen. Watching a path that does not exist would
	// rerun this on every build.
	for watched in [&root, &dir] {
		if watched.is_dir() {
			println!("cargo::rerun-if-changed={}", watched.display());
		}
	}
	let mut present: Vec<String> = std::fs::read_dir(&dir)
		.into_iter()
		.flatten()
		.flatten()
		.map(|entry| entry.path())
		.filter(|path| path.extension().is_some_and(|e| e == extension) && is_plugin(path))
		.filter_map(|path| path.file_stem().map(|stem| stem.to_string_lossy().into_owned()))
		.collect();
	present.sort();
	println!("cargo::rustc-env=FIXTURE_GATE_PRESENT={}", present.join("|"));
	println!("cargo::rustc-env=FIXTURE_GATE_GPU={}", has_gpu());
}

/// Whether `path` is a plugin, and not only a name for one: a link to an
/// absent plugin, or the text file git writes for a link in a checkout
/// without symlinks, is none.
fn is_plugin(path: &Path) -> bool {
	if cfg!(windows) {
		let mut magic = [0u8; 2];
		std::fs::File::open(path).is_ok_and(|mut file| file.read_exact(&mut magic).is_ok() && magic == *b"MZ")
	} else {
		path.is_dir()
	}
}

/// Whether aexlo's GPU path can have a device here: Metal on macOS, the
/// NVIDIA driver's CUDA on Windows.
fn has_gpu() -> bool {
	if cfg!(target_os = "macos") {
		return true;
	}
	std::env::var_os("SystemRoot").is_some_and(|root| Path::new(&root).join("System32/nvcuda.dll").exists())
}
