//! Load an existing After Effects plugin and print its ABOUT message.

fn main() -> Result<(), Box<dyn std::error::Error>> {
	//* ---- Resolve the path to the example plugin. ------------------------ */
	// The plugin is located in the `fixtures/plugins/` directory.
	let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");

	let fixture = if cfg!(target_os = "windows") {
		// On Windows the plugin is a DLL with the .aex extension.
		"fixtures/plugins/windows/SDK_Noise.aex"
	} else {
		// On macOS the plugin is a .plugin bundle directory.
		"fixtures/plugins/macos/SDK_Noise.plugin"
	};

	let plugin_path = root.join(fixture);
	println!("Loading {}", plugin_path.display());
	//* --------------------------------------------------------------------- */
	//*
	//* ---- Execute the plugin. -------------------------------------------- */
	// Create a host.
	let host = aexlo::Host::get();

	// Load the plugin.
	let mut instance = host.try_load(&plugin_path)?;

	// Print the plugin's ABOUT message.
	println!("{}", instance.about()?);

	Ok(())
	//* --------------------------------------------------------------------- */
}
