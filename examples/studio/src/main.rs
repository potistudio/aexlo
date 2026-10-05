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
mod raster;
mod sidebar;
mod timeline;
mod viewer;
mod widgets;

use std::path::PathBuf;

use eframe::egui;

fn main() -> eframe::Result<()> {
	env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

	let file = std::env::args().nth(1).map(PathBuf::from);
	let project = match &file {
		Some(path) => app::load_project(path).unwrap_or_else(|e| {
			eprintln!("{e}");
			std::process::exit(1);
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
		Box::new(move |cc| Ok(Box::new(app::Studio::new(&cc.egui_ctx, project, file)))),
	)
}
