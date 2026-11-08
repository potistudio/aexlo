//! Load an existing After Effects plugin and print its ABOUT message.

use aexlo::Host;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
	let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
	let fixture = if cfg!(target_os = "windows") {
		"fixtures/plugins/windows/SDK_Noise.aex"
	} else {
		"fixtures/plugins/macos/SDK_Noise.plugin"
	};
	let plugin_path = root.join(fixture);
	println!("Loading {}", plugin_path.display());
	let mut instance = Host::get().try_load(&plugin_path)?;
	println!("{}", instance.about()?);
	Ok(())
}
