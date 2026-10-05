//! Application state and the frame loop: menus, shortcuts, undo, playback,
//! and the conversation with the render engine.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use eframe::egui::{self, Key, KeyboardShortcut, Modifiers};

use crate::engine::{Engine, Event, ExportJob, ExportKind, FrameResult, PluginInfo, Request};
use crate::model::{Anim, Id, LayerKind, Project, Text, TextAlign};
use crate::raster::Fonts;
use crate::{inspector, plugins, sidebar, timeline, viewer};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
	Select,
	Pen,
	Rect,
	Ellipse,
	Star,
}

impl Tool {
	pub const ALL: [(Tool, &'static str, &'static str); 5] = [
		(Tool::Select, "⬉", "Selection (V)"),
		(Tool::Pen, "✒", "Pen: masks, or paths on a shape layer (G)"),
		(Tool::Rect, "▭", "Rectangle (Q)"),
		(Tool::Ellipse, "◯", "Ellipse (Q)"),
		(Tool::Star, "☆", "Star (Q)"),
	];
}

/// What the user has selected; ids into the project.
#[derive(Clone, Debug, Default)]
pub struct Selection {
	pub layer: Option<Id>,
	/// The effect whose point parameters get handles in the viewer.
	pub effect: Option<Id>,
	pub mask: Option<Id>,
	pub shape: Option<Id>,
	/// A keyframe: `(track key, frame)`.
	pub key: Option<(String, i32)>,
}

pub struct View {
	pub zoom: f32,
	pub pan: egui::Vec2,
	pub fit: bool,
	pub checker: bool,
	/// Shape tools and the pen draw masks even on shape layers.
	pub draw_masks: bool,
}

/// The path the pen tool is adding vertices to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pen {
	pub layer: Id,
	pub target: PathTarget,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PathTarget {
	Mask(Id),
	ShapeItem(Id),
}

pub struct Studio {
	pub project: Project,
	/// The project as of the last undo step.
	committed: Project,
	undo: Vec<Project>,
	redo: Vec<Project>,
	pub file: Option<PathBuf>,

	engine: Engine,
	/// Parameters of each effect's plugin, once loaded.
	pub infos: HashMap<Id, Result<Arc<PluginInfo>, String>>,
	/// The same, by plugin reference, for labelling keyframe tracks.
	pub plugin_infos: HashMap<String, Arc<PluginInfo>>,
	/// Hidden and disabled flags per effect parameter.
	pub param_flags: HashMap<Id, (Vec<bool>, Vec<bool>)>,
	pub fixtures: Vec<String>,
	pub plugin_filter: String,
	/// Text measurement for hit-testing text layers.
	pub fonts: Fonts,

	pub frame: i32,
	/// Playback start: when, and from which frame.
	playing: Option<(Instant, i32)>,
	pub loop_playback: bool,

	pub sel: Selection,
	pub tool: Tool,
	pub view: View,
	pub pen: Option<Pen>,
	/// Layers expanded in the timeline to show their keyframes.
	pub expanded: HashSet<Id>,
	pub viewer_drag: Option<viewer::Drag>,
	pub timeline_drag: Option<timeline::Drag>,

	texture: Option<egui::TextureHandle>,
	pub shown_frame: Option<i32>,
	sent: Option<(Arc<Project>, i32)>,
	generation: u64,
	pub last: Option<Stats>,
	frame_times: Vec<Instant>,
	pub export: Option<(i32, i32)>,
	status: Option<(String, Instant)>,
	ffmpeg: bool,
	/// `AEXLO_STUDIO_SCREENSHOT`: save the window there once the first frame
	/// shows, then quit. For checking the UI from scripts.
	screenshot: Option<PathBuf>,
}

/// About the last frame shown.
pub struct Stats {
	pub elapsed: Duration,
	pub problems: Vec<String>,
	pub effect_times: HashMap<Id, Duration>,
}

impl Studio {
	pub fn new(ctx: &egui::Context, project: Project, file: Option<PathBuf>) -> Self {
		install_fonts(ctx);
		let repaint = ctx.clone();
		Self {
			committed: project.clone(),
			project,
			undo: Vec::new(),
			redo: Vec::new(),
			file,
			engine: Engine::spawn(move || repaint.request_repaint()),
			infos: HashMap::new(),
			plugin_infos: HashMap::new(),
			param_flags: HashMap::new(),
			fixtures: plugins::list_fixtures(),
			plugin_filter: String::new(),
			fonts: Fonts::new(),
			frame: 0,
			playing: None,
			loop_playback: true,
			sel: Selection::default(),
			tool: Tool::Select,
			view: View {
				zoom: 1.0,
				pan: egui::Vec2::ZERO,
				fit: true,
				checker: true,
				draw_masks: false,
			},
			pen: None,
			expanded: HashSet::new(),
			viewer_drag: None,
			timeline_drag: None,
			texture: None,
			shown_frame: None,
			sent: None,
			generation: 0,
			last: None,
			frame_times: Vec::new(),
			export: None,
			status: None,
			ffmpeg: std::process::Command::new("ffmpeg")
				.arg("-version")
				.output()
				.is_ok_and(|o| o.status.success()),
			screenshot: std::env::var_os("AEXLO_STUDIO_SCREENSHOT").map(PathBuf::from),
		}
	}

	pub fn texture(&self) -> Option<&egui::TextureHandle> {
		self.texture.as_ref()
	}

	pub fn is_playing(&self) -> bool {
		self.playing.is_some()
	}

	pub fn toggle_play(&mut self) {
		self.playing = match self.playing {
			Some(_) => None,
			None => Some((Instant::now(), self.frame)),
		};
	}

	pub fn set_frame(&mut self, frame: i32) {
		self.frame = frame.clamp(0, self.project.comp.duration.max(1) - 1);
		if self.playing.is_some() {
			self.playing = Some((Instant::now(), self.frame));
		}
	}

	pub fn notify(&mut self, message: impl Into<String>) {
		self.status = Some((message.into(), Instant::now()));
	}

	pub fn press_button(&mut self, effect: Id, index: usize) {
		self.generation += 1;
		let project = Arc::new(self.project.clone());
		self.sent = Some((project.clone(), self.frame));
		self.engine.send(Request::Button {
			project,
			frame: self.frame,
			generation: self.generation,
			effect,
			index,
		});
	}

	// ---- Editing helpers -----------------------------------------------------

	pub fn add_layer(&mut self, name: &str, kind: LayerKind) -> Id {
		let layer = self.project.new_layer(name, kind);
		let id = layer.id;
		let index = self.sel.layer.and_then(|l| self.project.layer_index(l)).unwrap_or(0);
		self.project.layers.insert(index, layer);
		self.select_layer(Some(id));
		id
	}

	pub fn select_layer(&mut self, layer: Option<Id>) {
		if self.sel.layer != layer {
			self.sel = Selection {
				layer,
				..Default::default()
			};
			self.pen = None;
		}
	}

	pub fn add_effect(&mut self, plugin: &str) {
		let Some(layer) = self.sel.layer else {
			self.notify("Select a layer to apply an effect to");
			return;
		};
		let effect = self.project.new_effect(plugin);
		let id = effect.id;
		if let Some(layer) = self.project.layer_mut(layer) {
			layer.effects.push(effect);
			self.sel.effect = Some(id);
		}
	}

	pub fn new_text_layer(&mut self) {
		let (w, h) = (self.project.comp.width as f32, self.project.comp.height as f32);
		self.add_layer(
			"Text",
			LayerKind::Text(Text {
				text: "Text".into(),
				font: None,
				size: Anim::new(96.0),
				color: Anim::new([1.0; 4]),
				position: Anim::new([w / 2.0, h / 2.0 + 32.0]),
				align: TextAlign::Center,
				tracking: Anim::new(0.0),
			}),
		);
	}

	pub fn new_solid(&mut self) {
		let (w, h) = (self.project.comp.width, self.project.comp.height);
		let n = self.project.layers.len() + 1;
		self.add_layer(
			&format!("Solid {n}"),
			LayerKind::Solid {
				color: Anim::new(palette(n)),
				width: w,
				height: h,
			},
		);
	}

	pub fn import_image(&mut self) {
		let Some(path) = rfd::FileDialog::new()
			.add_filter("Images", &["png", "jpg", "jpeg"])
			.pick_file()
		else {
			return;
		};
		match image::image_dimensions(&path) {
			Ok((width, height)) => {
				let name = path
					.file_name()
					.map(|n| n.to_string_lossy().into_owned())
					.unwrap_or_default();
				self.add_layer(
					&name,
					LayerKind::Image {
						path: path.to_string_lossy().into_owned(),
						width,
						height,
					},
				);
			}
			Err(e) => self.notify(format!("{}: {e}", path.display())),
		}
	}

	pub fn delete_selected(&mut self) {
		let sel = self.sel.clone();
		let Some(layer_id) = sel.layer else { return };
		if let Some((track, frame)) = sel.key {
			let names = self.plugin_infos.clone();
			if let Some(layer) = self.project.layer_mut(layer_id) {
				let label = |p: &str, i: usize| {
					names
						.get(p)
						.and_then(|n| n.params.get(i))
						.map(|d| d.name.clone())
						.unwrap_or_default()
				};
				for (key, _, t) in layer.tracks_mut(&label) {
					if key == track {
						t.remove_key(frame);
					}
				}
			}
			self.sel.key = None;
			return;
		}
		let Some(layer) = self.project.layer_mut(layer_id) else {
			return;
		};
		if let Some(mask) = sel.mask {
			layer.masks.retain(|m| m.id != mask);
			self.sel.mask = None;
		} else if let Some(shape) = sel.shape {
			if let LayerKind::Shape { items } = &mut layer.kind {
				items.retain(|i| i.id != shape);
			}
			self.sel.shape = None;
		} else if let Some(effect) = sel.effect {
			layer.effects.retain(|e| e.id != effect);
			self.sel.effect = None;
		} else {
			self.project.remove_layer(layer_id);
			self.select_layer(None);
		}
		self.pen = None;
	}

	pub fn move_layer(&mut self, by: isize) {
		let Some(i) = self.sel.layer.and_then(|l| self.project.layer_index(l)) else {
			return;
		};
		let j = (i as isize + by).clamp(0, self.project.layers.len() as isize - 1) as usize;
		let layer = self.project.layers.remove(i);
		self.project.layers.insert(j, layer);
	}

	/// Jump to the previous (`dir < 0`) or next keyframe of the selected layer.
	pub fn jump_to_key(&mut self, dir: i32) {
		let Some(id) = self.sel.layer else { return };
		let mut frames: Vec<i32> = Vec::new();
		if let Some(layer) = self.project.layer_mut(id) {
			for (_, _, t) in layer.tracks_mut(&|_, _| String::new()) {
				frames.extend(t.key_frames().into_iter().map(|k| k.0));
			}
		}
		let target = if dir < 0 {
			frames.into_iter().filter(|&f| f < self.frame).max()
		} else {
			frames.into_iter().filter(|&f| f > self.frame).min()
		};
		if let Some(f) = target {
			self.set_frame(f);
		}
	}

	// ---- Undo ------------------------------------------------------------------

	pub fn undo(&mut self) {
		if let Some(previous) = self.undo.pop() {
			self.redo.push(std::mem::replace(&mut self.project, previous));
			self.committed = self.project.clone();
			self.after_history_jump();
		}
	}

	pub fn redo(&mut self) {
		if let Some(next) = self.redo.pop() {
			self.undo.push(std::mem::replace(&mut self.project, next));
			self.committed = self.project.clone();
			self.after_history_jump();
		}
	}

	fn after_history_jump(&mut self) {
		self.pen = None;
		self.viewer_drag = None;
		self.timeline_drag = None;
		if self.sel.layer.is_some_and(|l| self.project.layer(l).is_none()) {
			self.select_layer(None);
		}
	}

	/// Record an undo step once an edit settles (no button held), so a whole
	/// drag is one step.
	fn commit(&mut self, ctx: &egui::Context) {
		if self.project != self.committed && !ctx.input(|i| i.pointer.any_down()) {
			self.undo
				.push(std::mem::replace(&mut self.committed, self.project.clone()));
			self.redo.clear();
			if self.undo.len() > 200 {
				self.undo.remove(0);
			}
		}
	}

	// ---- Files -----------------------------------------------------------------

	fn reset(&mut self, project: Project, file: Option<PathBuf>) {
		self.committed = project.clone();
		self.project = project;
		self.file = file;
		self.undo.clear();
		self.redo.clear();
		self.select_layer(None);
		self.frame = 0;
		self.playing = None;
		self.view.fit = true;
	}

	pub fn open(&mut self) {
		let Some(path) = rfd::FileDialog::new()
			.add_filter("aexlo studio project", &["json"])
			.pick_file()
		else {
			return;
		};
		match load_project(&path) {
			Ok(project) => {
				self.reset(project, Some(path.clone()));
				self.notify(format!("Opened {}", path.display()));
			}
			Err(e) => self.notify(e),
		}
	}

	pub fn save(&mut self, save_as: bool) {
		let path = match (&self.file, save_as) {
			(Some(path), false) => path.clone(),
			_ => {
				let Some(path) = rfd::FileDialog::new()
					.add_filter("aexlo studio project", &["json"])
					.set_file_name("project.json")
					.save_file()
				else {
					return;
				};
				path
			}
		};
		match serde_json::to_string_pretty(&self.project)
			.map_err(|e| e.to_string())
			.and_then(|json| std::fs::write(&path, json).map_err(|e| e.to_string()))
		{
			Ok(()) => {
				self.notify(format!("Saved {}", path.display()));
				self.file = Some(path);
			}
			Err(e) => self.notify(format!("Save failed: {e}")),
		}
	}

	fn export(&mut self, kind: &str) {
		let (start, end, kind) = match kind {
			"png" => {
				let Some(path) = rfd::FileDialog::new()
					.add_filter("PNG", &["png"])
					.set_file_name(format!("frame_{:05}.png", self.frame))
					.save_file()
				else {
					return;
				};
				(self.frame, self.frame, ExportKind::Png(path))
			}
			"sequence" => {
				let Some(dir) = rfd::FileDialog::new().pick_folder() else {
					return;
				};
				(0, self.project.comp.duration - 1, ExportKind::Sequence(dir))
			}
			_ => {
				let Some(path) = rfd::FileDialog::new()
					.add_filter("MP4", &["mp4"])
					.set_file_name("render.mp4")
					.save_file()
				else {
					return;
				};
				(0, self.project.comp.duration - 1, ExportKind::Video(path))
			}
		};
		self.export = Some((0, end - start + 1));
		self.engine.send(Request::Export(ExportJob {
			project: Arc::new(self.project.clone()),
			start,
			end,
			kind,
		}));
	}

	// ---- Frame loop --------------------------------------------------------------

	fn handle_events(&mut self, ctx: &egui::Context) {
		while let Ok(event) = self.engine.events.try_recv() {
			match event {
				Event::Frame(result) => self.show_frame(ctx, *result),
				Event::Info { effect, info } => {
					if let Ok(info) = &info
						&& let Some((_, fx)) = self.project.effect(effect)
					{
						self.plugin_infos.insert(fx.plugin.clone(), info.clone());
					}
					self.infos.insert(effect, info);
				}
				Event::ParamUi {
					effect,
					hidden,
					disabled,
				} => {
					self.param_flags.insert(effect, (hidden, disabled));
				}
				Event::ParamsChanged { effect, frame, values } => {
					if let Some(fx) = self.project.effect_mut(effect) {
						for (index, value) in values {
							fx.params
								.entry(index)
								.or_insert_with(|| Anim::new(value.clone()))
								.set(frame, value);
						}
					}
				}
				Event::ExportProgress { done, total } => self.export = Some((done, total)),
				Event::ExportFinished(result) => {
					self.export = None;
					match result {
						Ok(message) | Err(message) => self.notify(message),
					}
				}
			}
		}
	}

	fn show_frame(&mut self, ctx: &egui::Context, result: FrameResult) {
		if result.generation == 0 {
			return; // an export frame
		}
		let image = egui::ColorImage::from_rgba_unmultiplied(
			[result.image.width as usize, result.image.height as usize],
			&result.image.pixels,
		);
		let options = if self.view.zoom >= 1.0 {
			egui::TextureOptions::NEAREST
		} else {
			egui::TextureOptions::LINEAR
		};
		match &mut self.texture {
			Some(texture) => texture.set(image, options),
			None => self.texture = Some(ctx.load_texture("comp", image, options)),
		}
		self.shown_frame = Some(result.frame);
		self.last = Some(Stats {
			elapsed: result.elapsed,
			problems: result.problems,
			effect_times: result.effect_times,
		});
		let now = Instant::now();
		self.frame_times.push(now);
		self.frame_times
			.retain(|t| now.duration_since(*t) < Duration::from_secs(1));
	}

	pub fn display_fps(&self) -> usize {
		self.frame_times.len()
	}

	fn advance_playback(&mut self, ctx: &egui::Context) {
		let Some((started, from)) = self.playing else { return };
		let duration = self.project.comp.duration.max(1);
		let elapsed = (started.elapsed().as_secs_f64() * self.project.comp.fps as f64) as i32;
		let frame = from + elapsed;
		if frame >= duration {
			if self.loop_playback {
				self.playing = Some((Instant::now(), 0));
				self.frame = 0;
			} else {
				self.playing = None;
				self.frame = duration - 1;
			}
		} else {
			self.frame = frame;
		}
		ctx.request_repaint();
	}

	fn request_render(&mut self) {
		let stale = match &self.sent {
			Some((project, frame)) => **project != self.project || *frame != self.frame,
			None => true,
		};
		if stale {
			self.generation += 1;
			let project = Arc::new(self.project.clone());
			self.sent = Some((project.clone(), self.frame));
			self.engine.send(Request::Render {
				project,
				frame: self.frame,
				generation: self.generation,
			});
		}
	}

	fn shortcuts(&mut self, ctx: &egui::Context) {
		let command = |key| KeyboardShortcut::new(Modifiers::COMMAND, key);
		let command_shift = |key| KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, key);
		let pressed = |s: KeyboardShortcut| ctx.input_mut(|i| i.consume_shortcut(&s));

		if pressed(command_shift(Key::Z)) {
			self.redo();
		}
		if pressed(command(Key::Z)) {
			self.undo();
		}
		if pressed(command_shift(Key::S)) {
			self.save(true);
		}
		if pressed(command(Key::S)) {
			self.save(false);
		}
		if pressed(command(Key::O)) {
			self.open();
		}
		if pressed(command(Key::D))
			&& let Some(id) = self.sel.layer
			&& let Some(copy) = self.project.duplicate_layer(id)
		{
			self.select_layer(Some(copy));
		}
		if pressed(command(Key::OpenBracket)) {
			self.move_layer(1);
		}
		if pressed(command(Key::CloseBracket)) {
			self.move_layer(-1);
		}

		// Single keys only when no text field has focus.
		if ctx.memory(|m| m.focused().is_some()) {
			return;
		}
		let key = |k| ctx.input_mut(|i| i.consume_key(Modifiers::NONE, k));
		let shift_key = |k| ctx.input_mut(|i| i.consume_key(Modifiers::SHIFT, k));
		if key(Key::Space) {
			self.toggle_play();
		}
		if key(Key::ArrowLeft) || key(Key::PageUp) {
			self.set_frame(self.frame - 1);
		}
		if key(Key::ArrowRight) || key(Key::PageDown) {
			self.set_frame(self.frame + 1);
		}
		if shift_key(Key::ArrowLeft) {
			self.set_frame(self.frame - 10);
		}
		if shift_key(Key::ArrowRight) {
			self.set_frame(self.frame + 10);
		}
		if key(Key::Home) {
			self.set_frame(0);
		}
		if key(Key::End) {
			self.set_frame(i32::MAX);
		}
		if key(Key::J) {
			self.jump_to_key(-1);
		}
		if key(Key::K) {
			self.jump_to_key(1);
		}
		if key(Key::V) {
			self.tool = Tool::Select;
			self.pen = None;
		}
		if key(Key::G) {
			self.tool = Tool::Pen;
		}
		if key(Key::Q) {
			self.tool = match self.tool {
				Tool::Rect => Tool::Ellipse,
				Tool::Ellipse => Tool::Star,
				_ => Tool::Rect,
			};
			self.pen = None;
		}
		if key(Key::Enter) || key(Key::Escape) {
			self.pen = None;
		}
		if key(Key::Delete) || key(Key::Backspace) {
			self.delete_selected();
		}
	}

	fn menu_bar(&mut self, ui: &mut egui::Ui) {
		egui::MenuBar::new().ui(ui, |ui| {
			ui.menu_button("File", |ui| {
				if ui.button("New Project").clicked() {
					self.reset(Project::default(), None);
				}
				if ui.button("Demo Project").clicked() {
					self.reset(crate::demo::project(), None);
				}
				if ui.button("Open…  ⌘O").clicked() {
					self.open();
				}
				if ui.button("Save  ⌘S").clicked() {
					self.save(false);
				}
				if ui.button("Save As…  ⇧⌘S").clicked() {
					self.save(true);
				}
				ui.separator();
				if ui.button("Export Frame as PNG…").clicked() {
					self.export("png");
				}
				if ui.button("Export PNG Sequence…").clicked() {
					self.export("sequence");
				}
				if ui
					.add_enabled(self.ffmpeg, egui::Button::new("Export MP4…"))
					.on_disabled_hover_text("Needs ffmpeg on PATH")
					.clicked()
				{
					self.export("mp4");
				}
			});
			ui.menu_button("Edit", |ui| {
				if ui
					.add_enabled(!self.undo.is_empty(), egui::Button::new("Undo  ⌘Z"))
					.clicked()
				{
					self.undo();
				}
				if ui
					.add_enabled(!self.redo.is_empty(), egui::Button::new("Redo  ⇧⌘Z"))
					.clicked()
				{
					self.redo();
				}
				ui.separator();
				if ui.button("Delete  ⌫").clicked() {
					self.delete_selected();
				}
			});
			ui.menu_button("Layer", |ui| {
				ui.menu_button("New", |ui| sidebar::new_layer_buttons(ui, self));
				ui.separator();
				let has_layer = self.sel.layer.is_some();
				if ui.add_enabled(has_layer, egui::Button::new("Duplicate  ⌘D")).clicked()
					&& let Some(copy) = self.sel.layer.and_then(|id| self.project.duplicate_layer(id))
				{
					self.select_layer(Some(copy));
				}
				if ui
					.add_enabled(has_layer, egui::Button::new("Bring Forward  ⌘]"))
					.clicked()
				{
					self.move_layer(-1);
				}
				if ui
					.add_enabled(has_layer, egui::Button::new("Send Backward  ⌘["))
					.clicked()
				{
					self.move_layer(1);
				}
				if ui.add_enabled(has_layer, egui::Button::new("Delete Layer")).clicked() {
					self.sel = Selection {
						layer: self.sel.layer,
						..Default::default()
					};
					self.delete_selected();
				}
			});
			ui.menu_button("Effect", |ui| {
				let enabled = self.sel.layer.is_some();
				for name in self.fixtures.clone() {
					if ui.add_enabled(enabled, egui::Button::new(&name)).clicked() {
						self.add_effect(&name);
					}
				}
				ui.separator();
				if ui.add_enabled(enabled, egui::Button::new("Other Plugin…")).clicked() {
					sidebar::pick_plugin(self);
				}
			});
			ui.menu_button("View", |ui| {
				if ui.button("Fit Comp in Viewer").clicked() {
					self.view.fit = true;
				}
				if ui.button("Zoom 100%").clicked() {
					self.view.fit = false;
					self.view.zoom = 1.0;
					self.view.pan = egui::Vec2::ZERO;
				}
				ui.checkbox(&mut self.view.checker, "Transparency Grid");
			});
		});
	}

	fn status_bar(&mut self, ui: &mut egui::Ui) {
		ui.horizontal(|ui| {
			if let Some(last) = &self.last {
				ui.label(format!(
					"Render {:.1} ms · {} fps shown",
					last.elapsed.as_secs_f64() * 1000.0,
					self.display_fps()
				));
				if !last.problems.is_empty() {
					ui.colored_label(
						egui::Color32::from_rgb(240, 160, 60),
						format!("⚠ {}", last.problems.len()),
					)
					.on_hover_text(last.problems.join("\n"));
				}
			}
			if let Some((done, total)) = self.export {
				ui.separator();
				ui.add(
					egui::ProgressBar::new(done as f32 / total.max(1) as f32)
						.desired_width(160.0)
						.text(format!("Exporting {done}/{total}")),
				);
				if ui.button("Cancel").clicked() {
					self.engine.cancel_export.store(true, Ordering::Relaxed);
				}
			}
			if let Some((message, at)) = &self.status {
				if at.elapsed() < Duration::from_secs(8) {
					ui.separator();
					ui.label(message);
				} else {
					self.status = None;
				}
			}
			ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
				let name = self
					.file
					.as_ref()
					.and_then(|p| p.file_name())
					.map_or("Untitled".into(), |n| n.to_string_lossy().into_owned());
				let dirty = if self.undo.is_empty() { "" } else { " •" };
				ui.weak(format!("{name}{dirty}"));
			});
		});
	}
}

impl eframe::App for Studio {
	fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
		let ctx = ui.ctx().clone();
		self.handle_events(&ctx);
		self.advance_playback(&ctx);
		self.shortcuts(&ctx);

		egui::Panel::top("menu").show(ui, |ui| self.menu_bar(ui));
		egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
		egui::Panel::bottom("timeline")
			.resizable(true)
			.default_size(260.0)
			.min_size(120.0)
			.show(ui, |ui| timeline::show(ui, self));
		egui::Panel::left("sidebar")
			.resizable(true)
			.default_size(230.0)
			.show(ui, |ui| sidebar::show(ui, self));
		egui::Panel::right("inspector")
			.resizable(true)
			.default_size(380.0)
			.show(ui, |ui| inspector::show(ui, self));
		egui::CentralPanel::default().show(ui, |ui| viewer::show(ui, self));

		self.commit(&ctx);
		self.request_render();
		self.scripted_screenshot(&ctx);
	}
}

impl Studio {
	fn scripted_screenshot(&mut self, ctx: &egui::Context) {
		let Some(path) = self.screenshot.clone() else { return };
		let shot = ctx.input(|i| {
			i.events.iter().find_map(|e| match e {
				egui::Event::Screenshot { image, .. } => Some(image.clone()),
				_ => None,
			})
		});
		if let Some(image) = shot {
			let pixels: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect();
			let [w, h] = image.size;
			if let Err(e) = image::save_buffer(&path, &pixels, w as u32, h as u32, image::ExtendedColorType::Rgba8) {
				log::error!("{}: {e}", path.display());
			}
			ctx.send_viewport_cmd(egui::ViewportCommand::Close);
		} else if self.shown_frame.is_some()
			&& self.infos.len() >= self.project.layers.iter().map(|l| l.effects.len()).sum()
		{
			if let Ok(index) = std::env::var("AEXLO_STUDIO_SELECT")
				&& let Some(layer) = index.parse::<usize>().ok().and_then(|i| self.project.layers.get(i))
				&& self.sel.layer.is_none()
			{
				let id = layer.id;
				self.select_layer(Some(id));
				self.expanded.insert(id);
				ctx.request_repaint();
				return;
			}
			ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
		}
		ctx.request_repaint();
	}
}

pub fn load_project(path: &std::path::Path) -> Result<Project, String> {
	let json = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
	serde_json::from_str(&json).map_err(|e| format!("{}: {e}", path.display()))
}

/// Distinct default colors for new solids and shapes.
pub fn palette(n: usize) -> [f32; 4] {
	const COLORS: [[f32; 4]; 6] = [
		[0.95, 0.35, 0.35, 1.0],
		[0.98, 0.72, 0.25, 1.0],
		[0.40, 0.80, 0.45, 1.0],
		[0.30, 0.65, 0.95, 1.0],
		[0.65, 0.45, 0.95, 1.0],
		[0.95, 0.45, 0.75, 1.0],
	];
	COLORS[n % COLORS.len()]
}

/// Add a system CJK font as a fallback so Japanese layer names render.
fn install_fonts(ctx: &egui::Context) {
	let candidates = [
		"/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc",
		"/System/Library/Fonts/Hiragino Sans GB.ttc",
		"C:\\Windows\\Fonts\\YuGothR.ttc",
		"C:\\Windows\\Fonts\\msgothic.ttc",
	];
	let Some(bytes) = candidates.iter().find_map(|p| std::fs::read(p).ok()) else {
		return;
	};
	let mut fonts = egui::FontDefinitions::default();
	fonts
		.font_data
		.insert("cjk".into(), Arc::new(egui::FontData::from_owned(bytes)));
	for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
		fonts.families.entry(family).or_default().push("cjk".into());
	}
	ctx.set_fonts(fonts);
}
