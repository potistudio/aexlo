#![feature(stmt_expr_attributes)]

extern crate env_logger as logger;
extern crate log;

use std::error::Error;

use colored::Colorize;

// pub use aex::plugin_instance::PluginInstance;
use aexlo::PluginInstance;

//* Configuration constants */
const MODULE_NAME: &str = "Chromabba";

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
	// let message = instance.about()?;
	// println!("plugin information: {:?}", message);

	println!("{}", instance.param_count());
	// instance.set_param_float(1, 100.0)?;
	instance.render()?;
	// instance.render_pre()?;
	// instance.render_smart()?;

	//==== Extract the output layer ========================
	log::info!("Extracting output layer...");

	// 1. Get the output layer's dimensions
	let (width, height) = instance.output_size();

	// 2. Allocate a buffer to hold the pixel data
	let mut buffer = vec![0u8; (width * height * 4) as usize];

	// 3. Write the output layer's pixel data into your buffer
	instance.write_output_rgba(&mut buffer)?;
	log::info!("Extracted output layer {}.", "successfully".green());

	log::debug!("First 10 pixels (out of {}):", buffer.len() / 4);
	for (i, pixel) in buffer.chunks_exact(4).enumerate().take(10) {
		let r = pixel[0];
		let g = pixel[1];
		let b = pixel[2];
		let a = pixel[3];
		log::debug!("    {}: {{{}, {}, {}, {}}}", i, r, g, b, a);
	}

	// 4. Output your buffer to any destination you want (e.g. write to a PNG file)
	write_png(&buffer, width, height)?;

	println!("======== Execution completed ========\n");
	Ok(())
}
