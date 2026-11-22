use std::io::Read;
use std::path::{Path, PathBuf};

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

/// A bundled plugin fixture by file stem (e.g. `"nothing"`, `"FillColor"`).
///
/// Most of `fixtures/plugins/windows` are personal, gitignored symlinks into the
/// developer's local After Effects plugin installs (see `fixtures/plugins/.gitignore`) --
/// only a handful of small real binaries are checked in directly, and even those vary
/// by machine. A test that needs one says so with `#[needs_fixture("...")]`, which
/// reports it as ignored on a machine without that plugin, so it never passes unrun.
///
/// # Panics
/// When the fixture is absent, as when such a test is run with `--ignored`.
pub fn fixture(stem: &str) -> PathBuf {
	find_fixture(stem).unwrap_or_else(|| panic!("fixture '{stem}' is not in fixtures/plugins on this machine"))
}

/// [`fixture`], for a test that takes any of several.
pub fn find_fixture(stem: &str) -> Option<PathBuf> {
	let (platform_dir, extension) = if cfg!(target_os = "windows") {
		("windows", "aex")
	} else {
		("macos", "plugin")
	};

	let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("../..")
		.join("fixtures/plugins")
		.join(platform_dir)
		.join(format!("{stem}.{extension}"));

	is_plugin(&path).then_some(path)
}

/// Absolute path to the workspace's shared `fixtures/input.png`, used as a realistic
/// non-uniform input frame across the pixel/param/GPU e2e tests.
pub fn sample_input_path() -> PathBuf {
	PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/input.png")
}

#[cfg(test)]
mod tests {
	use std::path::PathBuf;

	/// The native test client named by `var_name`, set by the CMake build in
	/// `build.rs`. That build is disabled, so these tests are ignored; run with
	/// `--ignored` and the variable set, they use the client it names.
	fn plugin_path(var_name: &str) -> PathBuf {
		std::env::var_os(var_name)
			.map(PathBuf::from)
			.unwrap_or_else(|| panic!("{var_name} is not set (the CMake test clients are not built)"))
	}

	#[test]
	#[ignore = "the CMake test clients are not built (tests/e2e/build.rs)"]
	fn plugin_load_test() {
		let dll_path = plugin_path("E2E_ABOUT_MESSAGE_TEST_CLIENT");
		assert!(dll_path.exists(), "native library not found: {}", dll_path.display());

		aexlo::Host::get().load(&dll_path).expect("failed to load plugin");
	}

	#[test]
	#[ignore = "the CMake test clients are not built (tests/e2e/build.rs)"]
	fn about_message_test() {
		let dll_path = plugin_path("E2E_ABOUT_MESSAGE_TEST_CLIENT");
		assert!(dll_path.exists(), "native library not found: {}", dll_path.display());

		let mut instance = aexlo::Host::get().load(&dll_path).expect("failed to load plugin");

		let message = instance.about().unwrap();
		assert_eq!(message, "Hello World!");
	}

	#[test]
	#[ignore = "the CMake test clients are not built (tests/e2e/build.rs)"]
	fn pointer_validation_test() {
		let dll_path = plugin_path("E2E_POINTER_VALIDATION_TEST_CLIENT");
		assert!(dll_path.exists(), "native library not found: {}", dll_path.display());

		let mut instance = aexlo::Host::get().load(&dll_path).expect("failed to load plugin");
		instance.about().unwrap();
	}
}
