#![feature(stmt_expr_attributes)]

extern crate env_logger as logger;
extern crate log;

use std::error::Error;
use std::io::Write;

use colored::Colorize;

// pub use aex::plugin_instance::PluginInstance;
use aexlo::PluginInstance;

//* Configuration constants */
const MODULE_NAME: &str = "SDK_Noise";

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

	//* ---- Initialize logger -------------------------- */
	unsafe {
		std::env::set_var("RUST_LOG", "warn");
	}

	logger::Builder::from_default_env()
		.format(|buffer, record| {
			let timestamp = chrono::Utc::now().format("%H:%M:%S%.6f").to_string();

			let padded_level = match record.level() {
				log::Level::Error => "<ERROR>".red().bold(),
				log::Level::Warn => "<WARN> ".yellow().bold(),
				log::Level::Info => "<INFO> ".blue().bold(),
				log::Level::Debug => "<DEBUG>".green().bold(),
				log::Level::Trace => "<TRACE>".white().bold(),
			};

			writeln!(
				buffer,
				"[{timestamp}] {padded_level} {args} - {file}:{line}",
				args = record.args(),
				file = record.file().unwrap_or("unknown"),
				line = record.line().unwrap_or(0)
			)
		})
		.init();
	//* ------------------------------------------------- */
	//* ---- Determine plugin path ---------------------- */
	let exe_dir = std::env::current_exe().expect("Failed to get current executable path");
	let plugin_path = exe_dir
		.parent()
		.expect("Failed to get parent directory of executable")
		.join(MODULE_NAME);
	//* ------------------------------------------------- */
	//* ---- Execute the plugin ------------------------- */
	let mut instance = PluginInstance::new(plugin_path.as_path());
	instance.load()?;

	// Warmup run to stabilize system
	log::info!("Performing warmup run...");
	instance.render()?;

	// Benchmark runs
	const BENCHMARK_ITERATIONS: usize = 10;
	let mut times = Vec::with_capacity(BENCHMARK_ITERATIONS);

	log::info!("Running {} benchmark iterations...", BENCHMARK_ITERATIONS);
	for i in 1..=BENCHMARK_ITERATIONS {
		let start = std::time::Instant::now();
		instance.render()?;
		let duration = start.elapsed();
		times.push(duration);
		log::error!("Run {}/{}: {:.2?}", i, BENCHMARK_ITERATIONS, duration);
	}

	// Calculate statistics
	let total: std::time::Duration = times.iter().sum();
	let avg = total / times.len() as u32;
	let min = *times.iter().min().unwrap();
	let max = *times.iter().max().unwrap();
	let variance = times
		.iter()
		.map(|&t| {
			let diff = (t.as_secs_f64() - avg.as_secs_f64()).powi(2);
			diff
		})
		.sum::<f64>()
		/ times.len() as f64;
	let stddev = variance.sqrt();

	log::error!("\n{}", "=== Benchmark Results ===".bold());
	log::error!("  Iterations: {}", BENCHMARK_ITERATIONS);
	log::error!("  Average:    {:.2?}", avg);
	log::error!("  Min:        {:.2?}", min);
	log::error!("  Max:        {:.2?}", max);
	log::error!("  Std Dev:    {:.6}s", stddev);
	log::error!("  Total:      {:.2?}", total);
	//* ------------------------------------------------- */
	//* ---- Extract the output layer ------------------- */
	log::info!("Extracting output layer...");
	let layer = instance.output_layer();
	log::info!("Extracted output layer {}.", "successfully".green());

	log::debug!("First 10 pixels (out of {}):", layer.len());
	for (i, pixel) in layer.iter().enumerate().take(10) {
		log::debug!("    Pixel {}: {:?}", i, pixel);
	}
	//* ------------------------------------------------- */
	//* ---- Write output image as PNG ------------------ */
	log::info!("Writing output image to 'output.png'...");
	let output_buffer: Vec<u8> = layer
		.iter()
		.flat_map(|p| vec![p.red, p.green, p.blue, p.alpha])
		.collect();

	let mut writer = Vec::<u8>::new();
	let options = mtpng::encoder::Options::default();

	let mut header = mtpng::Header::new();
	header.set_size(layer.width(), layer.height())?;
	header.set_color(mtpng::ColorType::TruecolorAlpha, 8)?;

	let mut encoder = mtpng::encoder::Encoder::new(&mut writer, &options);
	encoder.write_header(&header)?;
	encoder.write_image_rows(&output_buffer)?;
	encoder.finish()?;

	std::fs::write("output.png", writer)?;
	log::info!("Wrote output image {}.", "successfully".green());
	//* ------------------------------------------------- */
	println!("======== Execution completed ========\n");
	Ok(())
}
