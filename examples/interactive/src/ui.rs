use imgui::*;
use imgui_winit_support::{HiDpiMode, WinitPlatform};

pub struct EffectParams {
	pub frequency: f32,
	pub speed: f32,
	pub color_shift: f32,
}

impl Default for EffectParams {
	fn default() -> Self {
		Self {
			frequency: 5.0,
			speed: 1.0,
			color_shift: 3.0,
		}
	}
}

pub struct UiState {
	pub imgui: Context,
	pub platform: WinitPlatform,
	pub params: EffectParams,
	show_demo: bool,
}

impl UiState {
	pub fn new(window: &winit::window::Window, renderer: &crate::renderer::Renderer) -> Self {
		let mut imgui = Context::create();
		imgui.set_ini_filename(None);

		let mut platform = WinitPlatform::init(&mut imgui);
		platform.attach_window(imgui.io_mut(), window, HiDpiMode::Default);

		imgui.io_mut().display_size = [renderer.width as f32, renderer.height as f32];

		imgui
			.fonts()
			.add_font(&[FontSource::DefaultFontData { config: None }]);

		Self {
			imgui,
			platform,
			params: EffectParams::default(),
			show_demo: false,
		}
	}

	pub fn handle_event<T>(
		&mut self,
		window: &winit::window::Window,
		event: &winit::event::Event<T>,
	) {
		self.platform
			.handle_event(self.imgui.io_mut(), window, event);
	}

	pub fn prepare_frame(&mut self, delta_time: f32) {
		self.imgui.io_mut().delta_time = delta_time;
	}

	pub fn render(&mut self) -> &DrawData {
		let ui = self.imgui.frame();

		// Control panel
		ui.window("Effect Controls")
			.size([300.0, 200.0], Condition::FirstUseEver)
			.position([10.0, 10.0], Condition::FirstUseEver)
			.build(|| {
				ui.text("Plasma Effect Parameters");
				ui.separator();

				ui.slider("Frequency", 0.1, 20.0, &mut self.params.frequency);
				ui.slider("Speed", 0.0, 5.0, &mut self.params.speed);
				ui.slider("Color Shift", 0.0, 10.0, &mut self.params.color_shift);

				ui.separator();

				if ui.button("Reset") {
					self.params = EffectParams::default();
				}

				ui.same_line();
				ui.checkbox("Show Demo", &mut self.show_demo);
			});

		// Stats window
		ui.window("Stats")
			.size([250.0, 100.0], Condition::FirstUseEver)
			.position([10.0, 220.0], Condition::FirstUseEver)
			.build(|| {
				ui.text(format!("FPS: {:.1}", ui.io().framerate));
				ui.text(format!("Frame time: {:.3} ms", 1000.0 / ui.io().framerate));
			});

		if self.show_demo {
			ui.show_demo_window(&mut self.show_demo);
		}

		self.imgui.render()
	}
}
