//! aexlo studio: a small compositor for exercising aexlo end to end.
//!
//! Layers (solids, images, shapes, text, adjustment layers) with masks,
//! stacks of real After Effects plugin effects, keyframes and a timeline.
//! Effects can read other layers through their layer parameters and the
//! layer's masks through their path parameters.
//!
//! Usage: `cargo run -p studio --release [-- project.json]`

mod app;
mod demo;
mod engine;
mod inspector;
mod model;
mod plugins;
mod process;
mod raster;
mod sidebar;
mod timeline;
mod viewer;
mod widgets;

use std::path::PathBuf;

use eframe::egui;

fn main() {
	// A panic has been printed by its hook; it must still not leave through
	// the plugins' detach (see `process`).
	let code = match std::panic::catch_unwind(run) {
		Ok(Ok(())) => 0,
		Ok(Err(e)) => {
			eprintln!("studio: {e}");
			1
		}
		Err(_) => PANICKED,
	};
	process::exit(code);
}

/// The status Rust itself exits with after a panic in `main`.
const PANICKED: u8 = 101;

fn run() -> eframe::Result<()> {
	env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

	let file = std::env::args().nth(1).map(PathBuf::from);
	let project = match &file {
		Some(path) => app::load_project(path).unwrap_or_else(|e| {
			eprintln!("{e}");
			process::exit(1);
		}),
		None => demo::project(),
	};

	let options = eframe::NativeOptions {
		viewport: egui::ViewportBuilder::default()
			.with_inner_size([1600.0, 1000.0])
			.with_title("aexlo studio"),
		..Default::default()
	};
	eframe::run_native(
		"aexlo-studio",
		options,
		Box::new(move |cc| {
			// Plugins parent editors on this window. Set it before any effect
			// loads: an editor owned by a hidden window stays invisible.
			publish_main_window(cc);
			Ok(Box::new(app::Studio::new(&cc.egui_ctx, project, file)))
		}),
	)
}

/// Hand the studio's HWND to the host as `PF_PlatData_MAIN_WND` and
/// `AEGP_GetMainHWND`.
fn publish_main_window(handle: &impl raw_window_handle::HasWindowHandle) {
	#[cfg(windows)]
	{
		use raw_window_handle::{HasWindowHandle as _, RawWindowHandle};
		let Ok(window) = handle.window_handle() else {
			return;
		};
		let RawWindowHandle::Win32(win) = window.as_raw() else {
			return;
		};
		// SAFETY: eframe owns this window for the lifetime of Studio, which clears
		// the handle before the window is destroyed.
		unsafe { aexlo::set_main_window(win.hwnd.get() as *mut std::ffi::c_void) };
	}
	#[cfg(not(windows))]
	{
		let _ = handle;
	}
}
