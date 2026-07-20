use std::path::PathBuf;

/// Locate a bundled plugin fixture by file stem (e.g. `"nothing"`, `"FillColor"`).
///
/// Most of `fixtures/plugins/windows` are personal, gitignored symlinks into the
/// developer's local After Effects plugin installs (see `fixtures/plugins/.gitignore`) --
/// only a handful of small real binaries are checked in directly, and even those vary
/// by machine. Tests that need a specific named fixture call this and skip (rather than
/// panic) when it resolves to `None`, so the suite stays green on a machine that doesn't
/// have that particular plugin.
pub fn fixture(stem: &str) -> Option<PathBuf> {
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

	path.exists().then_some(path)
}

/// Absolute path to the workspace's shared `input.png`, used as a realistic
/// non-uniform input frame across the pixel/param/GPU e2e tests.
pub fn sample_input_path() -> PathBuf {
	PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../input.png")
}

#[cfg(test)]
mod tests {
	use std::path::PathBuf;

	use aexlo::PluginInstance;

	fn plugin_path(var_name: &str) -> PathBuf {
		std::env::var(var_name)
			.map(PathBuf::from)
			.expect("missing plugin path env var")
	}

	#[test]
	fn plugin_load_test() {
		let dll_path = plugin_path("E2E_ABOUT_MESSAGE_TEST_CLIENT");
		assert!(dll_path.exists(), "native library not found: {}", dll_path.display());

		PluginInstance::try_load(&dll_path).expect("failed to load plugin");
	}

	#[test]
	fn about_message_test() {
		let dll_path = plugin_path("E2E_ABOUT_MESSAGE_TEST_CLIENT");
		assert!(dll_path.exists(), "native library not found: {}", dll_path.display());

		let mut instance = PluginInstance::try_load(&dll_path).expect("failed to load plugin");

		let message = instance.about().unwrap();
		assert_eq!(message, "Hello World!");
	}

	#[test]
	fn pointer_validation_test() {
		let dll_path = plugin_path("E2E_POINTER_VALIDATION_TEST_CLIENT");
		assert!(dll_path.exists(), "native library not found: {}", dll_path.display());

		let mut instance = PluginInstance::try_load(&dll_path).expect("failed to load plugin");
		instance.about().unwrap();
	}
}
