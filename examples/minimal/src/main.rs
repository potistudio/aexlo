use aexlo::Host;

fn main() -> Result<(), Box<dyn std::error::Error>> {
	// Replace with the actual path to your plugin
	let plugin_path = "path/to/your/plugin.aex";

	let host = Host::get();
	let mut instance = host.try_load(plugin_path)?;

	println!("{}", instance.about()?);

	Ok(())
}
