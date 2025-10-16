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
		std::env::set_var("RUST_LOG", "debug");
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

	// log::error!("This is an error message");
	// log::warn!("This is a warning message");
	// log::info!("This is an info message");
	// log::debug!("This is a debug message");
	//* ------------------------------------------------- */
	let exe_dir = std::env::current_exe().expect("Failed to get current executable path");
	let plugin_path = exe_dir
		.parent()
		.expect("Failed to get parent directory of executable")
		.join(MODULE_NAME);

	let mut instance = PluginInstance::new(plugin_path.as_path());
	instance.render()?;

	println!("======== Execution completed ========\n");
	Ok(())
}
