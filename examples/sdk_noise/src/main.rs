#![feature(stmt_expr_attributes)]

extern crate env_logger as logger;
extern crate log;

use std::error::Error;

use colored::Colorize;

// pub use aex::plugin_instance::PluginInstance;
use aexlo::PluginInstance;

//* Configuration constants */
const MODULE_NAME: &str = "YY_Ramp+";

fn write_png(data: &[u8], width: u32, height: u32) -> Result<(), Box<dyn Error>> {
	log::info!("Writing output image...");

	let mut writer = Vec::<u8>::new();
	let options = mtpng::encoder::Options::default();

	let mut header = mtpng::Header::new();
	header.set_size(width, height)?;
	header.set_color(mtpng::ColorType::TruecolorAlpha, 8)?;

	let mut encoder = mtpng::encoder::Encoder::new(&mut writer, &options);
	encoder.write_header(&header)?;
	encoder.write_image_rows(data)?;
	encoder.finish()?;

	std::fs::write("output.png", writer)?;
	log::info!(
		"Wrote output image to '{}' {}.",
		"output.png".white(),
		"successfully".green()
	);

	Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
	#[rustfmt::skip]
	{
		println!("\n========  {} --- After Effects Plugin Loader  ========", "aexlo-rs".bold());
		println!("________  _______      ___    ___ ___       ________                 ________  ________");
		println!("|\\   __  \\|\\  ___ \\    |\\  \\  /  /|\\  \\     |\\   __  \\               |\\   __  \\|\\   ____\\");
		println!("\\ \\  \\|\\  \\ \\   __/|   \\ \\  \\/  / | \\  \\    \\ \\  \\|\\  \\  ____________\\ \\  \\|\\  \\ \\  \\___|_");
		println!(" \\ \\   __  \\ \\  \\_|/__  \\ \\    / / \\ \\  \\    \\ \\  \\\\\\  \\|\\____________\\ \\   _  _\\ \\_____  \\");
		println!("  \\ \\  \\ \\  \\ \\  \\_|\\ \\  /     \\/   \\ \\  \\____\\ \\  \\\\\\  \\|____________|\\ \\  \\\\  \\\\|____|\\  \\");
		println!("   \\ \\__\\ \\__\\ \\_______\\/  /\\   \\    \\ \\_______\\ \\_______\\              \\ \\__\\\\ _\\ ____\\_\\  \\");
		println!("    \\|__|\\|__|\\|_______/__/ /\\ __\\    \\|_______|\\|_______|               \\|__|\\|__|\\_________\\");
		println!("                       |__|/ \\|__|                                                \\|_________|\n");
	}

	env_logger::init();

	//* ---- Determine plugin path ---------------------- */
	let plugin_path = std::path::PathBuf::from(
		"D:/Projects/Develop/Rust/aexlo-rs/examples/sdk_noise/tests/mocks/windows/",
	)
	.join(MODULE_NAME);

	//* ---- Execute the plugin ------------------------- */
	let mut instance = PluginInstance::try_load(&plugin_path)?;

	//* ------------------------------------------------- */
	//* Call `about()`                                    */
	//* This function means to call the plugin with       */
	//* `PF_Cmd_ABOUT` command, which is used to retrieve */
	//* the plugin's information.                         */
	//* ------------------------------------------------- */
	let message = instance.about()?;
	println!("plugin information: {:?}", message);

	// instance.setup_global()?;
	// instance.setup_params()?;
	instance.render()?;

	//* ---- Extract the output layer ------------------- */
	log::info!("Extracting output layer...");
	let (width, height) = instance.output_size();
	let mut buffer = vec![0u8; (width * height * 4) as usize];

	instance.write_output_rgba(&mut buffer)?;
	log::info!("Extracted output layer {}.", "successfully".green());

	log::debug!("First 10 pixels (out of {}):", buffer.len() / 4);
	for (i, pixel) in buffer.iter().enumerate().take(10) {
		log::debug!("    Pixel {}: {:?}", i, pixel);
	}

	write_png(&buffer, width, height)?;

	println!("======== Execution completed ========\n");
	Ok(())
}
