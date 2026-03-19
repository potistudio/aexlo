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
