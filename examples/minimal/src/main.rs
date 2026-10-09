//! Load an existing After Effects plugin and print its ABOUT message.
use aexlo::Host;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
	let mut args = std::env::args_os().skip(1);
	let argument = args.next();
	if argument.as_deref().is_some_and(|arg| arg == "--help" || arg == "-h") {
		println!("Usage: cargo run -p minimal -- [plugin path]\nDefaults to the bundled SDK_Noise fixture.");
		return Ok(());
	}
	if args.next().is_some() {
		return Err("Expected at most one plugin path; use --help".into());
	}
	let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
	let fixture = if cfg!(target_os = "windows") {
		"fixtures/plugins/windows/SDK_Noise.aex"
	} else {
		"fixtures/plugins/macos/SDK_Noise.plugin"
	};
	let plugin_path = argument.map(PathBuf::from).unwrap_or_else(|| root.join(fixture));
	println!("Loading {}", plugin_path.display());
	let mut instance = Host::get().try_load(&plugin_path)?;
	println!("{}", instance.about()?);
	Ok(())
}
