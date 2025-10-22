/*
use eframe::egui;

fn main() -> eframe::Result {
	env_logger::init();

	let options = eframe::NativeOptions::default();

	eframe::run_native(
		"Demo",
		options,
		Box::new(|_cc| Ok(Box::new(App::default()))),
	)?;

	Ok(())
}

#[derive(Debug, Default)]
struct App {
	name: String,
	age: u8,
}

impl eframe::App for App {
	fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
		egui::CentralPanel::default().show(ctx, |ui| {
			ui.heading("Hello World!");
			ui.horizontal(|ui| {
				let name_label = ui.label("Your name: ");
				ui.text_edit_singleline(&mut self.name)
					.labelled_by(name_label.id);
			});
			ui.add(egui::Slider::new(&mut self.age, 0..=120).text("age"));
			if ui.button("Increment").clicked() {
				self.age += 1;
			}
			ui.label(format!("Hello '{}', age {}", self.name, self.age));
		});
	}
}
*/

use eframe::egui;

fn main() -> eframe::Result<()> {
	// Initialize logger
	unsafe {
		std::env::set_var("RUST_LOG", "debug");
	}
	env_logger::init();

	let options = eframe::NativeOptions {
		viewport: egui::ViewportBuilder::default()
			.with_inner_size([1280.0, 720.0])
			.with_title("aexlo Demo (SDK_Noise.aex)"),
		..Default::default()
	};

	log::info!("Starting aexlo interactive demo application");

	eframe::run_native(
		"aexlo-demo",
		options,
		Box::new(|_cc| Ok(Box::new(PixelApp::default()))),
	)
}

struct PixelApp {
	// Pixel buffer: RGBA format (4 bytes per pixel)
	pixels: Vec<u8>,
	width: usize,
	height: usize,

	// Texture handle for displaying the pixel buffer
	texture: Option<egui::TextureHandle>,

	// Drawing state
	brush_color: egui::Color32,
	brush_size: f32,
	is_drawing: bool,

	// UI state
	show_grid: bool,

	instance: aexlo::PluginInstance,
}

impl Default for PixelApp {
	fn default() -> Self {
		let width = 800;
		let height = 600;

		// Initialize pixel buffer with white background (RGBA)
		let pixels = vec![255u8; width * height * 4];

		let exe_dir = std::env::current_exe().expect("Failed to get current executable path");
		let plugin_path = exe_dir
			.parent()
			.expect("Failed to get parent directory of executable")
			.join("SDK_Noise");

		let mut instance = aexlo::PluginInstance::new(plugin_path.as_path());
		instance.load().unwrap();

		Self {
			pixels,
			width,
			height,
			texture: None,
			brush_color: egui::Color32::BLACK,
			brush_size: 5.0,
			is_drawing: false,
			show_grid: false,
			instance,
		}
	}
}

impl PixelApp {
	fn set_pixel_random(&mut self) {
		// use rand::Rng;
		// let mut rng = rand::rng();
		// self.pixels
		// 	.iter_mut()
		// 	.for_each(|p| *p = rng.random_range(0..255));
		self.instance.render();
		let rendered_layer = self.instance.output_layer();
		// self.pixels.copy_from_slice(rendered_layer.pixels);
	}

	/// Clear the canvas to white
	fn clear_canvas(&mut self) {
		self.pixels.fill(255);
	}

	/// Convert the pixel buffer to an egui ColorImage
	fn pixels_to_image(&self) -> egui::ColorImage {
		egui::ColorImage::from_rgba_unmultiplied([self.width, self.height], &self.pixels)
	}

	/// Update or create the texture from the pixel buffer
	fn update_texture(&mut self, ctx: &egui::Context) {
		let image = self.pixels_to_image();

		if let Some(texture) = &mut self.texture {
			texture.set(image, egui::TextureOptions::NEAREST);
		} else {
			self.texture =
				Some(ctx.load_texture("pixel-canvas", image, egui::TextureOptions::NEAREST));
		}
	}
}

impl eframe::App for PixelApp {
	fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
		// Side panel for controls
		egui::SidePanel::left("controls").show(ctx, |ui| {
			ui.heading("🎨 Controls");
			ui.separator();

			// Color picker
			ui.label("Brush Color:");
			egui::color_picker::color_edit_button_srgba(
				ui,
				&mut self.brush_color,
				egui::color_picker::Alpha::Opaque,
			);

			ui.add_space(10.0);

			// Brush size slider
			ui.label("Brush Size:");
			ui.add(egui::Slider::new(&mut self.brush_size, 1.0..=50.0));

			ui.add_space(10.0);

			// Brush preview
			ui.label("Brush Preview:");
			let (rect, _response) =
				ui.allocate_exact_size(egui::vec2(60.0, 60.0), egui::Sense::hover());
			ui.painter().rect_filled(rect, 0.0, egui::Color32::WHITE);
			ui.painter()
				.circle_filled(rect.center(), self.brush_size / 2.0, self.brush_color);

			ui.add_space(10.0);

			// Clear button
			if ui.button("🗑 Clear Canvas").clicked() {
				self.clear_canvas();
			}

			ui.add_space(20.0);
			ui.separator();

			// Canvas info
			ui.label(format!("Canvas: {}x{}", self.width, self.height));
			ui.label(format!("Total Pixels: {}", self.width * self.height));
			ui.label(format!("Buffer Size: {} bytes", self.pixels.len()));

			ui.add_space(10.0);

			// Grid toggle
			ui.checkbox(&mut self.show_grid, "Show Grid (slow)");
		});

		// Central panel for canvas
		egui::CentralPanel::default().show(ctx, |ui| {
			ui.heading("Canvas");

			// Update texture from pixel buffer
			self.set_pixel_random();
			self.update_texture(ctx);

			if let Some(texture) = &self.texture {
				let available_size = ui.available_size();
				let texture_size = texture.size_vec2();

				// Calculate scaling to fit in available space while maintaining aspect ratio
				let scale = (available_size.x / texture_size.x)
					.min(available_size.y / texture_size.y)
					.min(1.0); // Don't scale up

				let scaled_size = texture_size * scale;

				// Center the image
				ui.scope_builder(
					egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
						ui.available_rect_before_wrap().left_top(),
						scaled_size,
					)),
					|ui| ui.image((texture.id(), scaled_size)),
				);
			}

			// Request continuous repaints while drawing
			log::info!("Repainting while drawing");
			ctx.request_repaint();
		});
	}
}
