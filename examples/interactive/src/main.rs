//! Interactive playground for driving real After Effects plugin fixtures through
//! aexlo and previewing their output live.
//!
//! Pick any bundled fixture from the sidebar, tweak its numeric/boolean
//! parameters, and watch the rendered frame update in real time. Effects are
//! driven through [`PluginInstance::render_frame`], so both smart-render and
//! legacy effects work.

use std::path::PathBuf;

use aexlo::{Depth8, Layer, ParamValue, PluginInstance};
use eframe::egui;

const DEFAULT_PLUGIN_NAME: &str = "SDK_Noise";

/// Directory holding the prebuilt plugin fixtures for the current platform.
///
/// These are real, compiled plugin binaries shared across the workspace's
/// examples and tests -- not mock objects.
fn fixtures_dir() -> PathBuf {
	let platform_dir = if cfg!(target_os = "windows") {
		"windows"
	} else {
		"macos"
	};
	PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("../../fixtures/plugins")
		.join(platform_dir)
}

/// Resolve the fixture bundle path for a plugin by name.
fn fixture_path(plugin_name: &str) -> PathBuf {
	let extension = if cfg!(target_os = "windows") { "aex" } else { "plugin" };
	fixtures_dir().join(format!("{plugin_name}.{extension}"))
}

/// List the available fixture names (file stems), sorted, so the sidebar can
/// offer them in a picker.
fn list_fixtures() -> Vec<String> {
	let extension = if cfg!(target_os = "windows") { "aex" } else { "plugin" };
	let mut names: Vec<String> = std::fs::read_dir(fixtures_dir())
		.into_iter()
		.flatten()
		.flatten()
		.map(|entry| entry.path())
		.filter(|path| path.extension().and_then(|e| e.to_str()) == Some(extension))
		.filter_map(|path| path.file_stem().map(|s| s.to_string_lossy().into_owned()))
		.collect();
	names.sort();
	names
}

/// Load the shared `input.png` as an RGBA8 buffer, if present, so effects that
/// read their input layer have something meaningful to work on.
fn load_input_image() -> Option<(Vec<u8>, u32, u32)> {
	let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../input.png");
	match image::open(&path) {
		Ok(img) => {
			let img = img.to_rgba8();
			let (w, h) = img.dimensions();
			Some((img.into_raw(), w, h))
		}
		Err(e) => {
			log::warn!(
				"No input image at {:?} ({e}); effects will use the default layer.",
				path
			);
			None
		}
	}
}

/// Read a parameter's plugin-declared display name, falling back to a
/// positional label when the plugin left it blank.
fn param_name(instance: &PluginInstance, index: usize) -> String {
	instance
		.param_name(index)
		.filter(|name| !name.is_empty())
		.unwrap_or_else(|| format!("Param {index}"))
}

/// One editable parameter surfaced in the sidebar.
struct ParamControl {
	index: usize,
	name: String,
	value: ParamValue,
}

fn main() -> eframe::Result<()> {
	env_logger::init();

	let requested = std::env::args()
		.nth(1)
		.unwrap_or_else(|| DEFAULT_PLUGIN_NAME.to_string());

	let options = eframe::NativeOptions {
		viewport: egui::ViewportBuilder::default()
			.with_inner_size([1280.0, 720.0])
			.with_title("aexlo Interactive Playground"),
		..Default::default()
	};

	log::info!("Starting aexlo interactive playground");

	eframe::run_native(
		"aexlo-playground",
		options,
		Box::new(move |_cc| Ok(Box::new(AexloApp::new(&requested)))),
	)
}

struct AexloApp {
	/// Available fixture names and the currently selected index.
	plugins: Vec<String>,
	selected: usize,

	/// Loaded plugin and its state.
	instance: Option<PluginInstance>,
	error: Option<String>,
	render_error: Option<String>,
	smart_render: bool,
	total_params: usize,
	params: Vec<ParamControl>,

	/// Shared input image (RGBA8, width, height), applied to each loaded plugin.
	input: Option<(Vec<u8>, u32, u32)>,

	/// Rendered output preview.
	pixels: Vec<u8>,
	width: usize,
	height: usize,
	texture: Option<egui::TextureHandle>,

	/// Render pacing: re-render continuously (for animated effects) or only when
	/// something changes.
	auto_render: bool,
	needs_render: bool,

	/// FPS tracking.
	fps: f32,
	frame_count: u32,
	fps_update_time: std::time::Instant,
}

impl AexloApp {
	fn new(requested: &str) -> Self {
		let plugins = list_fixtures();
		let selected = plugins.iter().position(|name| name == requested).unwrap_or(0);

		let mut app = Self {
			plugins,
			selected,
			instance: None,
			error: None,
			render_error: None,
			smart_render: false,
			total_params: 0,
			params: Vec::new(),
			input: load_input_image(),
			pixels: vec![0u8; 1920 * 1080 * 4],
			width: 1920,
			height: 1080,
			texture: None,
			auto_render: true,
			needs_render: true,
			fps: 0.0,
			frame_count: 0,
			fps_update_time: std::time::Instant::now(),
		};

		if app.plugins.is_empty() {
			app.error = Some(format!("No plugin fixtures found in {:?}", fixtures_dir()));
		} else {
			app.load_selected();
		}

		app
	}

	/// (Re)load the plugin at `self.selected`, apply the input image, and refresh
	/// the parameter list.
	fn load_selected(&mut self) {
		let name = self.plugins[self.selected].clone();
		let path = fixture_path(&name);
		log::info!("Loading plugin '{name}' from {:?}", path);

		match PluginInstance::try_load(&path) {
			Ok(mut instance) => {
				if let Err(e) = instance.about() {
					log::warn!("about() failedor '{name}': {e}");
				}
				self.apply_input(&mut instance);
				self.smart_render = instance.supports_smart_render();
				self.total_params = instance.param_count();
				self.instance = Some(instance);
				self.error = None;
				self.render_error = None;
				self.refresh_params();
				self.needs_render = true;
			}
			Err(e) => {
				log::error!("Failed to load '{name}': {e}");
				self.instance = None;
				self.error = Some(e.to_string());
				self.params.clear();
				self.total_params = 0;
			}
		}
	}

	/// Feed the shared input image (if any) into the instance's input layer.
	fn apply_input(&self, instance: &mut PluginInstance) {
		if let Some((bytes, w, h)) = &self.input {
			match Layer::<Depth8>::from_raw(bytes.clone(), *w, *h) {
				Ok(layer) => instance.set_input(layer),
				Err(e) => log::warn!("Failed to build input layer: {e}"),
			}
		}
	}

	/// Rebuild the editable parameter list from the plugin's current state.
	fn refresh_params(&mut self) {
		self.params.clear();
		if let Some(instance) = &self.instance {
			for (index, value) in instance.param_values() {
				let name = param_name(instance, index);
				self.params.push(ParamControl { index, name, value });
			}
		}
	}

	/// Push every edited parameter value back into the plugin, then let it refresh
	/// any dependent UI state (show/hide, collapse, …) via `PF_Cmd_UPDATE_PARAMS_UI`.
	fn apply_params(&mut self) {
		if let Some(instance) = self.instance.as_mut() {
			for control in &self.params {
				if let Err(e) = instance.set_param(control.index, control.value.clone()) {
					log::warn!("set_param({}) failed: {e}", control.index);
				}
			}
			if let Err(e) = instance.update_params_ui() {
				log::debug!("update_params_ui failed: {e}");
			}
		}
	}

	/// Render one frame and copy it into the preview buffer.
	fn render_frame(&mut self) {
		let Some(instance) = self.instance.as_mut() else {
			return;
		};

		if let Err(e) = instance.render_frame() {
			self.render_error = Some(e.to_string());
			return;
		}
		self.render_error = None;

		let (out_w, out_h) = instance.output_size();
		let required = out_w as usize * out_h as usize * 4;
		if self.pixels.len() != required {
			self.pixels.resize(required, 0);
			self.width = out_w as usize;
			self.height = out_h as usize;
		}

		if let Err(e) = instance.write_output_rgba(&mut self.pixels) {
			self.render_error = Some(e.to_string());
		}
	}

	fn update_texture(&mut self, ctx: &egui::Context) {
		let image = egui::ColorImage::from_rgba_unmultiplied([self.width, self.height], &self.pixels);
		match &mut self.texture {
			Some(texture) => texture.set(image, egui::TextureOptions::NEAREST),
			None => self.texture = Some(ctx.load_texture("rendered-output", image, egui::TextureOptions::NEAREST)),
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

	/// Draw the left control panel; returns pending user actions to apply after
	/// the panel closes (keeping borrows simple).
	fn controls_panel(&mut self, ui: &mut egui::Ui) -> PanelActions {
		let mut actions = PanelActions::default();

		egui::Panel::left("controls")
			.resizable(true)
			.default_size(280.0)
			.show(ui, |ui| {
				ui.heading("🎬 aexlo Playground");
				ui.separator();

				// Plugin picker.
				ui.label("Plugin");
				let current = self.plugins.get(self.selected).cloned().unwrap_or_default();
				egui::ComboBox::from_id_salt("plugin-picker")
					.selected_text(current)
					.width(240.0)
					.show_ui(ui, |ui| {
						for (i, name) in self.plugins.iter().enumerate() {
							if ui.selectable_label(i == self.selected, name).clicked() && i != self.selected {
								actions.select = Some(i);
							}
						}
					});

				ui.separator();

				// Status.
				match (&self.error, &self.render_error) {
					(Some(err), _) => {
						ui.colored_label(egui::Color32::RED, "❌ Load error");
						ui.label(err);
					}
					(None, Some(err)) => {
						ui.colored_label(egui::Color32::from_rgb(230, 160, 30), "⚠ Render error");
						ui.label(err);
					}
					(None, None) => {
						ui.colored_label(egui::Color32::GREEN, "✅ Loaded");
					}
				}
				ui.label(format!(
					"Path: {}",
					if self.smart_render {
						"smart render"
					} else {
						"legacy render"
					}
				));
				ui.label(format!("FPS: {:.1}   ·   {}x{}", self.fps, self.width, self.height));

				ui.separator();

				// Render pacing.
				ui.checkbox(&mut self.auto_render, "Auto-render (animate)");
				if ui
					.add_enabled(!self.auto_render, egui::Button::new("Render once"))
					.clicked()
				{
					actions.render_once = true;
				}

				ui.separator();

				// Parameters.
				ui.heading("🎛 Parameters");
				let editable = self.params.len();
				ui.label(format!("{editable} editable of {} total", self.total_params));
				ui.add_space(4.0);

				if self.params.is_empty() {
					ui.weak("(no numeric/boolean parameters)");
				}

				egui::ScrollArea::vertical().show(ui, |ui| {
					for control in &mut self.params {
						let changed = param_widget(ui, control);
						if changed {
							actions.params_changed = true;
						}
					}
				});
			});

		actions
	}
}

/// Deferred results from the controls panel, applied after the panel's borrow
/// of `self` ends.
#[derive(Default)]
struct PanelActions {
	select: Option<usize>,
	params_changed: bool,
	render_once: bool,
}

/// Render the right widget for a parameter and report whether it changed.
fn param_widget(ui: &mut egui::Ui, control: &mut ParamControl) -> bool {
	ui.horizontal(|ui| {
		ui.label(&control.name);
		match &mut control.value {
			ParamValue::Float(v) => ui.add(egui::DragValue::new(v).speed(0.1)).changed(),
			ParamValue::Fixed(v) => ui.add(egui::DragValue::new(v).speed(0.01)).changed(),
			ParamValue::Slider(v) => ui.add(egui::DragValue::new(v)).changed(),
			ParamValue::Checkbox(b) => ui.checkbox(b, "").changed(),
			ParamValue::Popup(v) => ui.add(egui::DragValue::new(v).range(1..=i32::MAX)).changed(),
			ParamValue::Angle(deg) => ui.add(egui::DragValue::new(deg).speed(1.0).suffix("°")).changed(),
			ParamValue::Point { x, y } => {
				let cx = ui.add(egui::DragValue::new(x).speed(1.0).prefix("x ")).changed();
				let cy = ui.add(egui::DragValue::new(y).speed(1.0).prefix("y ")).changed();
				cx || cy
			}
			ParamValue::Color {
				red,
				green,
				blue,
				alpha,
			} => {
				let mut rgba = [*red, *green, *blue, *alpha];
				let changed = ui.color_edit_button_srgba_unmultiplied(&mut rgba).changed();
				if changed {
					[*red, *green, *blue, *alpha] = rgba;
				}
				changed
			}
		}
	})
	.inner
}

impl eframe::App for AexloApp {
	fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
		self.update_fps();

		let actions = self.controls_panel(ui);

		if let Some(index) = actions.select {
			self.selected = index;
			self.load_selected();
		}
		if actions.params_changed {
			self.apply_params();
			self.needs_render = true;
		}
		if actions.render_once {
			self.needs_render = true;
		}

		let ctx = ui.ctx().clone();

		egui::CentralPanel::default().show(ui, |ui| {
			if self.auto_render || self.needs_render {
				self.render_frame();
				self.update_texture(&ctx);
				self.needs_render = false;
			}

			if let Some(texture) = &self.texture {
				let available = ui.available_size();
				let tex_size = texture.size_vec2();
				let scale = (available.x / tex_size.x).min(available.y / tex_size.y).min(1.0);
				let scaled = tex_size * scale;

				ui.centered_and_justified(|ui| {
					ui.image((texture.id(), scaled));
				});
			} else {
				ui.centered_and_justified(|ui| {
					ui.heading("No output to display");
				});
			}
		});

		// Keep animating only when auto-render is on; otherwise repaint on demand.
		if self.auto_render {
			ctx.request_repaint();
		}
	}
}
