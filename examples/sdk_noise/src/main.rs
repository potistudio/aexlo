#![feature(stmt_expr_attributes)]

extern crate env_logger as logger;
extern crate log;

use std::error::Error;
use std::path::PathBuf;

use colored::{ColoredString, Colorize};

use aexlo::{Depth8, PluginInstance};

// Configuration constants
const DEFAULT_PLUGIN_NAME: &str = "AnimatedNoise";
const INPUT_IMAGE_PATH: &str = "input.png";
const OUTPUT_FILE_PATH: &str = "output.png";

fn successfully() -> ColoredString {
	"successfully".green()
}

fn failed() -> ColoredString {
	"failed".red()
}

fn print_banner() {
	#[rustfmt::skip]
	{
		println!("\n========  {} --- After Effects Plugin Loader  ========", "aexlo".bold());
		println!("________  _______      ___    ___ ___       ________");
		println!("|\\   __  \\|\\  ___ \\    |\\  \\  /  /|\\  \\     |\\   __  \\");
		println!("\\ \\  \\|\\  \\ \\   __/|   \\ \\  \\/  / | \\  \\    \\ \\  \\|\\  \\");
		println!(" \\ \\   __  \\ \\  \\_|/__  \\ \\    / / \\ \\  \\    \\ \\  \\\\\\  \\");
		println!("  \\ \\  \\ \\  \\ \\  \\_|\\ \\  /     \\/   \\ \\  \\____\\ \\  \\\\\\  \\");
		println!("   \\ \\__\\ \\__\\ \\_______\\/  /\\   \\    \\ \\_______\\ \\_______\\");
		println!("    \\|__|\\|__|\\|_______/__/ /\\ __\\    \\|_______|\\|_______|");
		println!("                       |__|/ \\|__|");
		println!("=========================================================\n");
	}
}

/// Resolves the path to a prebuilt After Effects plugin bundle checked into
/// the workspace's shared `fixtures/plugins/` directory. These are real,
/// compiled plugin binaries used across examples — not mock objects in the
/// unit-test sense.
fn resolve_plugin_fixture_path(plugin_name: &str) -> PathBuf {
	let (platform_dir, extension) = if cfg!(target_os = "windows") { ("windows", "aex") } else { ("macos", "plugin") };

	PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("../../fixtures/plugins")
		.join(platform_dir)
		.join(format!("{plugin_name}.{extension}"))
}

fn extract_output_rgba(instance: &mut PluginInstance) -> Result<(Vec<u8>, u32, u32), Box<dyn Error>> {
	log::info!("Extracting output layer...");

	let (width, height) = instance.output_size();
	let mut buffer = vec![0u8; (width * height * 4) as usize];

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

	Ok((buffer, width, height))
}

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

	std::fs::write(OUTPUT_FILE_PATH, writer)?;
	log::info!(
		"Wrote output image to '{}' {}.",
		OUTPUT_FILE_PATH.white(),
		"successfully".green()
	);

	Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
	print_banner();

	env_logger::init();

	let plugin_name = std::env::args().nth(1).unwrap_or_else(|| DEFAULT_PLUGIN_NAME.to_string());
	let plugin_path = resolve_plugin_fixture_path(&plugin_name);

	// 1. Load plugin with `PluginInstance::try_load()`
	// `try_load()` will return an error if the plugin fails to load for any reason (e.g. file not found, invalid format, missing dependencies).
	log::info!("Loading plugin from '{}'...", plugin_path.display());
	let mut instance = PluginInstance::try_load(&plugin_path)?;
	log::info!("Plugin loaded {}.", successfully());

	// Call `about()` if you want plugin information from `PF_Cmd_ABOUT`.
	let message = instance.about()?;
	println!("plugin information: {:?}", message);

	let img = image::open(INPUT_IMAGE_PATH).unwrap();
	let input_buffer = img.to_rgba8().into_raw();
	let input_layer = aexlo::Layer::<Depth8>::from_raw(input_buffer, 1920, 1080)?;

	instance.set_input(input_layer);

	log::info!("Rendering...");
	instance.render()?;
	// instance.render_pre()?;
	// instance.render_smart()?;
	log::info!("Rendering completed {}.", successfully());

	let (buffer, width, height) = extract_output_rgba(&mut instance)?;
	write_png(&buffer, width, height)?;

	println!("======== Execution completed ========\n");
	Ok(())
}
