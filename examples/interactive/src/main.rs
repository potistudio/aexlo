use eframe::egui;

fn main() -> eframe::Result<()> {
	// Initialize logger
	unsafe {
		std::env::set_var("RUST_LOG", "info");
	}
	env_logger::init();

	let options = eframe::NativeOptions {
		viewport: egui::ViewportBuilder::default()
			.with_inner_size([1280.0, 720.0])
			.with_title("aexlo Interactive Demo"),
		..Default::default()
	};

	log::info!("Starting aexlo interactive demo application");

	eframe::run_native(
		"aexlo-demo",
		options,
		Box::new(|_cc| Ok(Box::new(AexloApp::new()))),
	)
}

struct AexloApp {
	// Rendered pixel buffer (RGBA format)
	pixels: Vec<u8>,
	width: usize,
	height: usize,

	// egui texture handle
	texture: Option<egui::TextureHandle>,

	// Plugin instance
	instance: Option<aexlo::PluginInstance>,

	// Error message if plugin failed to load
	error: Option<String>,

	// FPS tracking
	last_frame_time: std::time::Instant,
	fps: f32,
	frame_count: u32,
	fps_update_time: std::time::Instant,
}

impl AexloApp {
	fn new() -> Self {
		let width = 1920;
		let height = 1080;

		// Try to load the plugin
		let exe_dir = std::env::current_exe().expect("Failed to get current executable path");
		let plugin_path = exe_dir
			.parent()
			.expect("Failed to get parent directory")
			.join("SDK_Noise");

		log::info!("Loading plugin from: {:?}", plugin_path);

		let (instance, error) = match load_plugin(&plugin_path) {
			Ok(inst) => (Some(inst), None),
			Err(e) => {
				log::error!("Failed to load plugin: {}", e);
				(None, Some(e.to_string()))
			}
		};

		Self {
			pixels: vec![0u8; width * height * 4],
			width,
			height,
			texture: None,
			instance,
			error,
			last_frame_time: std::time::Instant::now(),
			fps: 0.0,
			frame_count: 0,
			fps_update_time: std::time::Instant::now(),
		}
	}

	fn render_frame(&mut self) {
		if let Some(instance) = &mut self.instance {
			if let Err(e) = instance.render() {
				log::error!("Render failed: {}", e);
				return;
			}

			// Get rendered layer and convert to RGBA bytes
			let layer = instance.output_layer();
			self.pixels = layer.to_rgba_bytes();
			self.width = layer.width() as usize;
			self.height = layer.height() as usize;
		}
	}

	fn update_texture(&mut self, ctx: &egui::Context) {
		let image = egui::ColorImage::from_rgba_unmultiplied(
			[self.width, self.height],
			&self.pixels,
		);

		if let Some(texture) = &mut self.texture {
			texture.set(image, egui::TextureOptions::NEAREST);
		} else {
			self.texture = Some(ctx.load_texture(
				"rendered-output",
				image,
				egui::TextureOptions::NEAREST,
			));
		}
	}

	fn update_fps(&mut self) {
		self.frame_count += 1;
		let now = std::time::Instant::now();
		let elapsed = now.duration_since(self.fps_update_time).as_secs_f32();

		if elapsed >= 1.0 {
			self.fps = self.frame_count as f32 / elapsed;
			self.frame_count = 0;
			self.fps_update_time = now;
		}
	}
}

fn load_plugin(path: &std::path::Path) -> anyhow::Result<aexlo::PluginInstance> {
	let mut instance = aexlo::PluginInstance::new(path);
	instance.load()?;
	instance.about()?;
	instance.setup_global()?;
	instance.setup_params()?;
	Ok(instance)
}

impl eframe::App for AexloApp {
	fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
		// Update FPS
		self.update_fps();

		// Sidebar with controls
		egui::SidePanel::left("controls")
			.resizable(true)
			.default_width(250.0)
			.show(ctx, |ui| {
				ui.heading("� aexlo Interactive Demo");
				ui.separator();

				// FPS display
				ui.label(format!("FPS: {:.1}", self.fps));
				ui.label(format!("Resolution: {}x{}", self.width, self.height));

				ui.separator();

				// Plugin status
				if let Some(error) = &self.error {
					ui.colored_label(egui::Color32::RED, "❌ Plugin Error:");
					ui.label(error);
				} else {
					ui.colored_label(egui::Color32::GREEN, "✅ Plugin Loaded");
				}

				ui.separator();

				// Instructions
				ui.label("This demo renders an After Effects");
				ui.label("plugin in real-time.");
				ui.label("");
				ui.label("The SDK_Noise.aex plugin generates");
				ui.label("random noise patterns.");
			});

		// Main canvas area
		egui::CentralPanel::default().show(ctx, |ui| {
			// Render a new frame
			self.render_frame();
			self.update_texture(ctx);

			if let Some(texture) = &self.texture {
				let available_size = ui.available_size();
				let texture_size = texture.size_vec2();

				// Calculate scale to fit while maintaining aspect ratio
				let scale = (available_size.x / texture_size.x)
					.min(available_size.y / texture_size.y)
					.min(1.0);

				let scaled_size = texture_size * scale;

				// Center the image
				let offset_x = (available_size.x - scaled_size.x) / 2.0;
				let offset_y = (available_size.y - scaled_size.y) / 2.0;

				ui.allocate_space(egui::vec2(offset_x, 0.0));
				ui.vertical_centered(|ui| {
					ui.add_space(offset_y);
					ui.image((texture.id(), scaled_size));
				});
			} else {
				ui.centered_and_justified(|ui| {
					ui.heading("No texture available");
				});
			}
		});

		// Request continuous repaints for real-time rendering
		ctx.request_repaint();
	}
}
