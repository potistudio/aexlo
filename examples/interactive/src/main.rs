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

/// One plugin the sidebar can load: a bundled fixture or a file the user picked.
#[derive(Clone)]
struct PluginChoice {
	label: String,
	path: PathBuf,
}

fn bundled_plugins() -> Vec<PluginChoice> {
	list_fixtures()
		.into_iter()
		.map(|name| PluginChoice {
			path: fixture_path(&name),
			label: name,
		})
		.collect()
}

fn choice_for_path(path: &std::path::Path) -> PluginChoice {
	let label = path
		.file_stem()
		.map(|stem| stem.to_string_lossy().into_owned())
		.unwrap_or_else(|| path.display().to_string());
	PluginChoice {
		label,
		path: path.to_path_buf(),
	}
}

fn pick_plugin_path() -> Option<PathBuf> {
	let extensions: &[&str] = if cfg!(target_os = "windows") {
		&["aex"]
	} else {
		&["plugin"]
	};
	rfd::FileDialog::new()
		.add_filter("After Effects plugin", extensions)
		.pick_file()
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

/// Load the shared `fixtures/input.png` as an RGBA8 buffer, if present, so effects that
/// read their input layer have something meaningful to work on.
fn load_input_image() -> Option<(Vec<u8>, u32, u32)> {
	let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/input.png");
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

/// A `PF_Param_BUTTON` that is not drawn by the plugin itself.
struct ButtonControl {
	index: usize,
	name: String,
	enabled: bool,
}

/// Pixels of one custom-UI control, ready to show in the sidebar.
struct CustomView {
	index: usize,
	name: String,
	width: u32,
	height: u32,
	rgba: Vec<u8>,
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
		Box::new(move |cc| {
			// Plugins parent editors on this window. It has to be set before
			// load, because some effects cache `PF_PlatData_MAIN_WND`.
			publish_main_window(cc);
			Ok(Box::new(AexloApp::new(&requested)))
		}),
	)
}

/// Hand the playground's HWND to the host as `PF_PlatData_MAIN_WND`.
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
		// SAFETY: this is the app's own live window, re-published every frame
		// while plugins run in it.
		unsafe { aexlo::set_main_window(win.hwnd.get() as *mut std::ffi::c_void) };
	}
	#[cfg(not(windows))]
	{
		let _ = handle;
	}
}

struct AexloApp {
	/// Available plugins and the currently selected index.
	plugins: Vec<PluginChoice>,
	selected: usize,

	/// Loaded plugin and its state.
	instance: Option<PluginInstance>,
	error: Option<String>,
	render_error: Option<String>,
	smart_render: bool,
	has_custom_ui: bool,
	custom_views: Vec<CustomView>,
	custom_animate: bool,
	buttons: Vec<ButtonControl>,
	options_label: Option<String>,
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

	/// True while the pointer is down on a custom control, so mouse-down
	/// sends one `PF_Event_DO_CLICK`.
	custom_pointer_down: bool,
	/// The control asked for `PF_Event_DRAG` through mouse-up. A click that
	/// does not move still has to deliver that mouse-up, with `last_time` set.
	custom_send_drag: bool,
	/// Last point over the custom control, used for the mouse-up sample.
	custom_last: Option<CustomPointer>,

	/// FPS tracking.
	fps: f32,
	frame_count: u32,
	fps_update_time: std::time::Instant,
}

impl AexloApp {
	fn new(requested: &str) -> Self {
		let mut plugins = bundled_plugins();
		let requested_path = PathBuf::from(requested);
		let selected = if requested_path.exists() {
			match plugins.iter().position(|plugin| plugin.path == requested_path) {
				Some(index) => index,
				None => {
					plugins.insert(0, choice_for_path(&requested_path));
					0
				}
			}
		} else {
			plugins.iter().position(|plugin| plugin.label == requested).unwrap_or(0)
		};

		let mut app = Self {
			plugins,
			selected,
			instance: None,
			error: None,
			render_error: None,
			smart_render: false,
			has_custom_ui: false,
			custom_views: Vec::new(),
			custom_animate: false,
			buttons: Vec::new(),
			options_label: None,
			total_params: 0,
			params: Vec::new(),
			input: load_input_image(),
			pixels: vec![0u8; 1920 * 1080 * 4],
			width: 1920,
			height: 1080,
			texture: None,
			auto_render: true,
			needs_render: true,
			custom_pointer_down: false,
			custom_send_drag: false,
			custom_last: None,
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
		let choice = self.plugins[self.selected].clone();
		log::info!("Loading plugin '{}' from {:?}", choice.label, choice.path);

		match aexlo::Host::get().load(&choice.path) {
			Ok(mut instance) => {
				// `PF_Cmd_ABOUT` can open a modal dialog (Looks does). Leave that
				// to an explicit action so the playground can finish starting.
				self.apply_input(&mut instance);
				self.smart_render = instance.supports_smart_render();
				self.has_custom_ui = instance.has_custom_ui();
				self.total_params = instance.param_count();
				self.options_label = instance.has_options_button().then(|| {
					instance
						.options_button_name()
						.filter(|name| !name.is_empty())
						.unwrap_or("Options")
						.to_string()
				});
				self.instance = Some(instance);
				self.error = None;
				self.render_error = None;
				self.refresh_params();
				self.refresh_buttons();
				self.refresh_custom_views();
				// Drawing the control is enough to show a custom UI. Rendering a
				// frame immediately can stall before that control is usable.
				self.auto_render = !self.has_custom_ui;
				self.needs_render = !self.has_custom_ui;
			}
			Err(e) => {
				log::error!("Failed to load '{}': {e}", choice.label);
				self.instance = None;
				self.has_custom_ui = false;
				self.custom_views.clear();
				self.custom_animate = false;
				self.buttons.clear();
				self.options_label = None;
				self.error = Some(e.to_string());
				self.params.clear();
				self.total_params = 0;
			}
		}
	}

	fn load_path(&mut self, path: PathBuf) {
		if let Some(index) = self.plugins.iter().position(|plugin| plugin.path == path) {
			self.selected = index;
		} else {
			self.plugins.insert(0, choice_for_path(&path));
			self.selected = 0;
		}
		self.load_selected();
	}

	fn refresh_custom_views(&mut self) {
		self.custom_views.clear();
		self.custom_animate = false;
		let Some(instance) = self.instance.as_mut() else {
			return;
		};
		let controls: Vec<(usize, String)> = (0..instance.param_count())
			.filter(|index| instance.param_has_custom_ui(*index))
			.map(|index| (index, param_name(instance, index)))
			.collect();
		for (index, name) in controls {
			// Draw at the size the plugin declared; 320x80 stands in for an
			// edge it leaves to the panel.
			let (width, height) = instance.custom_control_size(index).unwrap_or_default();
			match instance.draw_custom_control(index, width.unwrap_or(320), height.unwrap_or(80)) {
				Ok(again) => self.custom_animate |= again,
				Err(err) => log::warn!("draw custom control {index}: {err}"),
			}
			if let Some(image) = instance.custom_control_image(index) {
				self.custom_views.push(CustomView {
					index,
					name,
					width: image.width,
					height: image.height,
					rgba: image.rgba.clone(),
				});
			}
		}
	}

	fn refresh_buttons(&mut self) {
		self.buttons.clear();
		let Some(instance) = &self.instance else {
			return;
		};
		for index in 1..instance.param_count() {
			if instance.param_kind(index) != Some(aexlo::ParamKind::Button) {
				continue;
			}
			if instance.param_hidden(index) || instance.param_has_custom_ui(index) {
				continue;
			}
			self.buttons.push(ButtonControl {
				index,
				name: param_name(instance, index),
				enabled: !instance.param_disabled(index),
			});
		}
	}

	fn press_button(&mut self, index: usize) {
		let Some(instance) = self.instance.as_mut() else {
			return;
		};
		if let Err(err) = instance.user_changed_param(index) {
			self.render_error = Some(err.to_string());
			return;
		}
		if let Err(err) = instance.update_params_ui() {
			log::debug!("update_params_ui failed: {err}");
		}
		self.needs_render = true;
	}

	fn press_options(&mut self) {
		let Some(instance) = self.instance.as_mut() else {
			return;
		};
		if let Err(err) = instance.do_dialog() {
			self.render_error = Some(err.to_string());
		}
	}

	/// Feed the shared input image (if any) into the instance's input layer.
	fn apply_input(&self, instance: &mut PluginInstance) {
		if let Some((bytes, w, h)) = &self.input {
			match Layer::<Depth8>::from_rgba8(bytes.clone(), *w, *h) {
				Ok(layer) => instance.set_input_layer(layer),
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

		if let Err(e) = instance.write_output_rgba8(&mut self.pixels) {
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
				let current = self
					.plugins
					.get(self.selected)
					.map(|plugin| plugin.label.clone())
					.unwrap_or_default();
				egui::ComboBox::from_id_salt("plugin-picker")
					.selected_text(current)
					.width(240.0)
					.show_ui(ui, |ui| {
						for (i, plugin) in self.plugins.iter().enumerate() {
							if ui.selectable_label(i == self.selected, &plugin.label).clicked() && i != self.selected {
								actions.select = Some(i);
							}
						}
					});
				if ui.button("Load plugin…").clicked()
					&& let Some(path) = pick_plugin_path()
				{
					actions.load_path = Some(path);
				}

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
				if let Some(label) = &self.options_label
					&& ui.button(label).clicked()
				{
					actions.options = true;
				}
				for button in &self.buttons {
					if ui
						.add_enabled(button.enabled, egui::Button::new(&button.name))
						.clicked()
					{
						actions.press_button = Some(button.index);
					}
				}
				if self.options_label.is_some() || !self.buttons.is_empty() {
					ui.separator();
				}

				if !self.custom_views.is_empty() {
					ui.heading("Custom controls");
					for view in &self.custom_views {
						ui.label(&view.name);
						let image = egui::ColorImage::from_rgba_unmultiplied(
							[view.width as usize, view.height as usize],
							&view.rgba,
						);
						let texture = ui.ctx().load_texture(
							format!("custom-ui-{}", view.index),
							image,
							egui::TextureOptions::LINEAR,
						);
						let response = ui.add(
							egui::Image::new(&texture)
								.fit_to_exact_size(egui::vec2(view.width as f32, view.height as f32))
								.sense(egui::Sense::click_and_drag()),
						);
						let pos = response.interact_pointer_pos().or_else(|| response.hover_pos());
						if let Some(pos) = pos {
							let rect = response.rect;
							let x = ((pos.x - rect.left()) / rect.width().max(1.0) * view.width as f32) as i32;
							let y = ((pos.y - rect.top()) / rect.height().max(1.0) * view.height as f32) as i32;
							let pointer = CustomPointer {
								index: view.index,
								x,
								y,
								width: view.width,
								height: view.height,
								drag: false,
								last: false,
							};
							self.custom_last = Some(pointer.clone());
							// After Effects sends DO_CLICK when the mouse goes down.
							// If the plugin sets send_drag, DRAG follows until mouse-up,
							// and that last sample is what commits a click.
							if self.custom_pointer_down && self.custom_send_drag && response.dragged() {
								actions.custom_pointer = Some(CustomPointer { drag: true, ..pointer });
							} else if self.custom_pointer_down && self.custom_send_drag && response.drag_stopped() {
								actions.custom_pointer = Some(CustomPointer {
									drag: true,
									last: true,
									..pointer
								});
							} else if !self.custom_pointer_down
								&& (response.is_pointer_button_down_on() || response.clicked())
							{
								actions.custom_pointer = Some(pointer);
							}
							if response.is_pointer_button_down_on() {
								self.custom_pointer_down = true;
							}
						}
					}
					ui.separator();
				}

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

		// A click that never moves does not produce a drag-stop. The plugin
		// still needs the mouse-up sample when it set send_drag.
		if self.custom_pointer_down && !ui.input(|input| input.pointer.any_down()) {
			if self.custom_send_drag
				&& actions.custom_pointer.is_none()
				&& let Some(pointer) = self.custom_last.clone()
			{
				actions.custom_pointer = Some(CustomPointer {
					drag: true,
					last: true,
					..pointer
				});
			}
			self.custom_pointer_down = false;
		}

		actions
	}
}

/// Deferred results from the controls panel, applied after the panel's borrow
/// of `self` ends.
#[derive(Clone, Default)]
struct CustomPointer {
	index: usize,
	x: i32,
	y: i32,
	width: u32,
	height: u32,
	drag: bool,
	last: bool,
}

#[derive(Default)]
struct PanelActions {
	select: Option<usize>,
	load_path: Option<PathBuf>,
	params_changed: bool,
	render_once: bool,
	options: bool,
	press_button: Option<usize>,
	custom_pointer: Option<CustomPointer>,
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
			ParamValue::Path(id) => ui.add(egui::DragValue::new(id).prefix("mask ")).changed(),
			ParamValue::Point3D { x, y, z } => {
				let cx = ui.add(egui::DragValue::new(x).speed(1.0).prefix("x ")).changed();
				let cy = ui.add(egui::DragValue::new(y).speed(1.0).prefix("y ")).changed();
				let cz = ui.add(egui::DragValue::new(z).speed(1.0).prefix("z ")).changed();
				cx || cy || cz
			}
			_ => {
				ui.label("(unsupported)");
				false
			}
		}
	})
	.inner
}

impl eframe::App for AexloApp {
	fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
		publish_main_window(frame);
		self.update_fps();

		let actions = self.controls_panel(ui);

		if let Some(index) = actions.select {
			self.selected = index;
			self.load_selected();
		}
		if let Some(path) = actions.load_path {
			self.load_path(path);
		}
		if actions.options {
			self.press_options();
			self.refresh_params();
			self.refresh_buttons();
			self.refresh_custom_views();
		}
		if let Some(index) = actions.press_button {
			self.press_button(index);
			self.refresh_params();
			self.refresh_buttons();
			self.refresh_custom_views();
		}
		if let Some(pointer) = actions.custom_pointer {
			let again = if let Some(instance) = self.instance.as_mut() {
				let result = if pointer.drag {
					instance.drag_custom_control(
						pointer.index,
						pointer.x,
						pointer.y,
						pointer.width,
						pointer.height,
						pointer.last,
					)
				} else {
					instance.click_custom_control(pointer.index, pointer.x, pointer.y, pointer.width, pointer.height)
				};
				match result {
					Ok(event) => {
						let mut redraw = event.redraw;
						// Press and release can land in one frame. The plugin
						// still needs the mouse-up drag before we move on.
						let released = !ui.input(|input| input.pointer.any_down());
						if !pointer.drag && event.send_drag && released {
							match instance.drag_custom_control(
								pointer.index,
								pointer.x,
								pointer.y,
								pointer.width,
								pointer.height,
								true,
							) {
								Ok(up) => redraw |= up.redraw,
								Err(err) => self.render_error = Some(err.to_string()),
							}
							self.custom_send_drag = false;
							self.custom_pointer_down = false;
						} else {
							self.custom_send_drag = event.send_drag && !pointer.last;
						}
						redraw
					}
					Err(err) => {
						self.render_error = Some(err.to_string());
						self.custom_send_drag = false;
						false
					}
				}
			} else {
				false
			};
			self.refresh_params();
			self.refresh_buttons();
			self.refresh_custom_views();
			self.custom_animate |= again;
			self.needs_render = true;
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
		if self.auto_render || self.custom_animate {
			if self.custom_animate {
				self.refresh_custom_views();
			}
			ctx.request_repaint();
		}
	}
}
