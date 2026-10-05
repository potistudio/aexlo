//! The bottom panel: transport, time ruler, layer bars and keyframes.

use eframe::egui::{self, Color32, Pos2, Rect, RichText, Sense, Stroke, Vec2, pos2, vec2};

use crate::app::Studio;
use crate::model::{Id, Interp, LayerKind};
use crate::widgets::{self, KEY_COLOR, timecode};

const LEFT: f32 = 250.0;
const ROW: f32 = 22.0;
const PLAYHEAD: Color32 = Color32::from_rgb(235, 80, 80);

pub enum Drag {
	Scrub,
	Bar {
		layer: Id,
		mode: BarMode,
		grab: i32,
		/// How far a move has shifted the layer so far.
		applied: i32,
	},
	Key {
		layer: Id,
		track: String,
		current: i32,
	},
}

#[derive(Clone, Copy, PartialEq)]
pub enum BarMode {
	Move,
	In,
	Out,
}

/// Frames to pixels across the time area.
#[derive(Clone, Copy)]
struct Map {
	left: f32,
	right: f32,
	duration: i32,
}

impl Map {
	fn per_frame(&self) -> f32 {
		(self.right - self.left) / self.duration.max(1) as f32
	}

	fn x(&self, frame: i32) -> f32 {
		self.left + frame as f32 * self.per_frame()
	}

	/// The frame whose cell contains `x`.
	fn frame_at(&self, x: f32) -> i32 {
		((x - self.left) / self.per_frame()).floor() as i32
	}

	/// The frame boundary nearest `x`.
	fn boundary_at(&self, x: f32) -> i32 {
		((x - self.left) / self.per_frame()).round() as i32
	}
}

pub fn show(ui: &mut egui::Ui, s: &mut Studio) {
	transport(ui, s);
	ui.separator();

	let map = ruler(ui, s);
	let duration = s.project.comp.duration;

	egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
		ui.spacing_mut().item_spacing.y = 1.0;
		let layer_ids: Vec<Id> = s.project.layers.iter().map(|l| l.id).collect();
		for (n, id) in layer_ids.into_iter().enumerate() {
			layer_row(ui, s, map, n, id);
			if s.expanded.contains(&id) {
				key_rows(ui, s, map, id);
			}
		}
		if s.project.layers.is_empty() {
			ui.add_space(8.0);
			ui.weak("No layers yet: add one from the left panel or the Layer menu.");
		}
	});

	if s.timeline_drag.is_some() && !ui.input(|i| i.pointer.any_down()) {
		s.timeline_drag = None;
	}
	let _ = duration;
}

fn transport(ui: &mut egui::Ui, s: &mut Studio) {
	ui.horizontal(|ui| {
		let big = |t: &str| RichText::new(t).size(16.0);
		if ui.button(big("⏮")).on_hover_text("First frame (Home)").clicked() {
			s.set_frame(0);
		}
		if ui.button(big("⏴")).on_hover_text("Previous frame (←)").clicked() {
			s.set_frame(s.frame - 1);
		}
		let play = if s.is_playing() { "⏸" } else { "▶" };
		if ui.button(big(play)).on_hover_text("Play / pause (Space)").clicked() {
			s.toggle_play();
		}
		if ui.button(big("⏵")).on_hover_text("Next frame (→)").clicked() {
			s.set_frame(s.frame + 1);
		}
		if ui.button(big("⏭")).on_hover_text("Last frame (End)").clicked() {
			s.set_frame(i32::MAX);
		}
		ui.toggle_value(&mut s.loop_playback, "🔁")
			.on_hover_text("Loop playback");
		ui.separator();
		ui.label(
			RichText::new(timecode(s.frame, s.project.comp.fps))
				.monospace()
				.size(16.0)
				.color(PLAYHEAD),
		);
		let mut frame = s.frame;
		if ui
			.add(
				egui::DragValue::new(&mut frame)
					.range(0..=s.project.comp.duration - 1)
					.suffix(" f"),
			)
			.changed()
		{
			s.set_frame(frame);
		}
		ui.separator();
		if ui.button("◆⏴").on_hover_text("Previous keyframe (J)").clicked() {
			s.jump_to_key(-1);
		}
		if ui.button("⏵◆").on_hover_text("Next keyframe (K)").clicked() {
			s.jump_to_key(1);
		}
		if let Some(shown) = s.shown_frame
			&& shown != s.frame
		{
			ui.weak(format!("showing {shown}"));
		}
	});
}

fn ruler(ui: &mut egui::Ui, s: &mut Studio) -> Map {
	let (rect, response) = ui
		.horizontal(|ui| {
			ui.allocate_exact_size(vec2(LEFT, 22.0), Sense::hover());
			ui.allocate_exact_size(vec2(ui.available_width() - 12.0, 22.0), Sense::click_and_drag())
		})
		.inner;
	let map = Map {
		left: rect.left(),
		right: rect.right(),
		duration: s.project.comp.duration.max(1),
	};
	let painter = ui.painter_at(rect.expand(4.0));
	painter.rect_filled(rect, 2.0, ui.visuals().extreme_bg_color);

	// Label every `step` frames, at least ~70 px apart.
	let fps = s.project.comp.fps.max(1) as i32;
	let steps = [
		1,
		2,
		5,
		10,
		fps / 2,
		fps,
		fps * 2,
		fps * 5,
		fps * 10,
		fps * 30,
		fps * 60,
	];
	let step = steps
		.into_iter()
		.filter(|&st| st > 0)
		.find(|&st| st as f32 * map.per_frame() >= 70.0)
		.unwrap_or(fps * 120);
	let text_color = ui.visuals().weak_text_color();
	let mut f = 0;
	while f <= map.duration {
		let x = map.x(f);
		painter.vline(x, rect.bottom() - 8.0..=rect.bottom(), Stroke::new(1.0, text_color));
		let label = if f % fps == 0 {
			format!("{}s", f / fps)
		} else {
			format!("{f}f")
		};
		painter.text(
			pos2(x + 3.0, rect.top() + 2.0),
			egui::Align2::LEFT_TOP,
			label,
			egui::FontId::monospace(10.0),
			text_color,
		);
		f += step;
	}
	if map.per_frame() >= 5.0 {
		for f in 0..map.duration {
			painter.vline(
				map.x(f),
				rect.bottom() - 3.0..=rect.bottom(),
				Stroke::new(1.0, text_color),
			);
		}
	}

	if (response.drag_started() || response.clicked())
		&& let Some(p) = response.interact_pointer_pos()
	{
		s.timeline_drag = Some(Drag::Scrub);
		s.set_frame(map.frame_at(p.x));
	}
	if response.dragged()
		&& matches!(s.timeline_drag, Some(Drag::Scrub))
		&& let Some(p) = response.interact_pointer_pos()
	{
		s.set_frame(map.frame_at(p.x));
	}

	// Playhead marker.
	let x = map.x(s.frame) + map.per_frame() / 2.0;
	painter.add(egui::Shape::convex_polygon(
		vec![
			pos2(x - 5.0, rect.top()),
			pos2(x + 5.0, rect.top()),
			pos2(x, rect.top() + 8.0),
		],
		PLAYHEAD,
		Stroke::NONE,
	));
	painter.vline(x, rect.y_range(), Stroke::new(1.5, PLAYHEAD));
	map
}

fn kind_color(kind: &LayerKind) -> Color32 {
	match kind {
		LayerKind::Solid { .. } => Color32::from_rgb(200, 90, 90),
		LayerKind::Image { .. } => Color32::from_rgb(90, 170, 110),
		LayerKind::Shape { .. } => Color32::from_rgb(80, 130, 210),
		LayerKind::Text(_) => Color32::from_rgb(210, 170, 70),
		LayerKind::Adjustment => Color32::from_rgb(150, 150, 160),
	}
}

fn layer_row(ui: &mut egui::Ui, s: &mut Studio, map: Map, n: usize, id: Id) {
	let selected = s.sel.layer == Some(id);
	let frame = s.frame;
	let Some(layer) = s.project.layer_mut(id) else { return };
	let color = kind_color(&layer.kind);
	let mut select = false;
	let mut toggle_expand = false;
	let expanded = s.expanded.contains(&id);

	let row = ui.horizontal(|ui| {
		ui.set_height(ROW);
		ui.allocate_ui_with_layout(
			vec2(LEFT, ROW),
			egui::Layout::left_to_right(egui::Align::Center),
			|ui| {
				ui.set_min_width(LEFT);
				ui.checkbox(&mut layer.visible, "").on_hover_text("Visible");
				if ui
					.add(egui::Button::new(if expanded { "▾" } else { "▸" }).frame(false))
					.on_hover_text("Show keyframes")
					.clicked()
				{
					toggle_expand = true;
				}
				let (chip, _) = ui.allocate_exact_size(vec2(8.0, 14.0), Sense::hover());
				ui.painter().rect_filled(chip, 2.0, color);
				ui.weak(format!("{}", n + 1));
				let fx = if layer.effects.is_empty() {
					String::new()
				} else {
					format!("  fx{}", layer.effects.len())
				};
				let name = egui::Label::new(RichText::new(format!("{}{fx}", layer.name)).color(if selected {
					widgets::ACCENT
				} else {
					ui.visuals().text_color()
				}))
				.truncate()
				.sense(Sense::click());
				if ui.add(name).clicked() {
					select = true;
				}
			},
		);

		let top = ui.cursor().top();
		let rect = Rect::from_min_max(pos2(map.left, top), pos2(map.right, top + ROW));
		let response = ui.allocate_rect(rect, Sense::click_and_drag());
		(rect, response)
	});
	let (rect, response) = row.inner;

	// Bar.
	let painter = ui.painter_at(rect);
	painter.rect_filled(rect, 0.0, ui.visuals().faint_bg_color);
	let bar = Rect::from_min_max(
		pos2(map.x(layer.in_frame), rect.top() + 3.0),
		pos2(map.x(layer.out_frame), rect.bottom() - 3.0),
	);
	let fill = if selected { color } else { color.gamma_multiply(0.6) };
	painter.rect_filled(bar, 3.0, fill);
	if selected {
		painter.rect_stroke(bar, 3.0, Stroke::new(1.5, Color32::WHITE), egui::StrokeKind::Inside);
	}
	// Every keyframe of the layer, as ticks.
	let mut keys: Vec<i32> = layer
		.tracks_mut(&|_, _| String::new())
		.into_iter()
		.flat_map(|(_, _, t)| t.key_frames().into_iter().map(|k| k.0))
		.collect();
	keys.sort_unstable();
	keys.dedup();
	for f in keys {
		let x = map.x(f) + map.per_frame() / 2.0;
		painter.vline(
			x,
			rect.bottom() - 7.0..=rect.bottom() - 3.0,
			Stroke::new(2.0, KEY_COLOR),
		);
	}
	painter.vline(
		map.x(frame) + map.per_frame() / 2.0,
		rect.y_range(),
		Stroke::new(1.5, PLAYHEAD),
	);

	// Dragging: edges trim, the body slides the layer.
	let edge = |x: f32, p: Pos2| (x - p.x).abs() <= 6.0;
	if let Some(p) = response.hover_pos()
		&& (edge(bar.left(), p) || edge(bar.right(), p))
	{
		ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
	}
	if response.drag_started()
		&& let Some(p) = response.interact_pointer_pos()
	{
		let mode = if edge(bar.left(), p) {
			BarMode::In
		} else if edge(bar.right(), p) {
			BarMode::Out
		} else {
			BarMode::Move
		};
		s.timeline_drag = Some(Drag::Bar {
			layer: id,
			mode,
			grab: map.boundary_at(p.x),
			applied: 0,
		});
		select = true;
	}
	if response.dragged()
		&& let Some(p) = response.interact_pointer_pos()
		&& let Some(Drag::Bar {
			layer: drag_layer,
			mode,
			grab,
			applied,
		}) = &mut s.timeline_drag
		&& *drag_layer == id
	{
		let at = map.boundary_at(p.x);
		match mode {
			BarMode::Move => {
				let delta = at - *grab;
				layer.shift(delta - *applied);
				*applied = delta;
			}
			BarMode::In => layer.in_frame = at.min(layer.out_frame - 1),
			BarMode::Out => layer.out_frame = at.max(layer.in_frame + 1),
		}
	}
	if response.clicked() {
		select = true;
	}

	if toggle_expand {
		if expanded {
			s.expanded.remove(&id);
		} else {
			s.expanded.insert(id);
		}
	}
	if select {
		s.select_layer(Some(id));
	}
	response.context_menu(|ui| {
		if ui.button("Duplicate").clicked()
			&& let Some(copy) = s.project.duplicate_layer(id)
		{
			s.select_layer(Some(copy));
		}
		if ui.button("Trim to Playhead (in)").clicked()
			&& let Some(l) = s.project.layer_mut(id)
		{
			l.in_frame = s.frame.min(l.out_frame - 1);
		}
		if ui.button("Trim to Playhead (out)").clicked()
			&& let Some(l) = s.project.layer_mut(id)
		{
			l.out_frame = (s.frame + 1).max(l.in_frame + 1);
		}
		if ui.button("Delete").clicked() {
			s.project.remove_layer(id);
			s.select_layer(None);
		}
	});
}

fn key_rows(ui: &mut egui::Ui, s: &mut Studio, map: Map, id: Id) {
	let Studio {
		project,
		plugin_infos,
		sel,
		timeline_drag,
		frame,
		..
	} = &mut *s;
	let duration = project.comp.duration;
	let name = |plugin: &str, index: usize| {
		plugin_infos
			.get(plugin)
			.and_then(|i| i.params.get(index))
			.map(|p| p.name.clone())
			.filter(|n| !n.is_empty())
			.unwrap_or_else(|| format!("Param {index}"))
	};
	let Some(layer) = project.layer_mut(id) else { return };
	let mut jump = None;
	let mut any = false;
	for (key, label, track) in layer.tracks_mut(&name) {
		let frames = track.key_frames();
		if frames.is_empty() {
			continue;
		}
		any = true;
		let row = ui.horizontal(|ui| {
			ui.set_height(ROW - 4.0);
			ui.allocate_ui_with_layout(
				vec2(LEFT, ROW - 4.0),
				egui::Layout::left_to_right(egui::Align::Center),
				|ui| {
					ui.set_min_width(LEFT);
					ui.add_space(36.0);
					ui.add(egui::Label::new(RichText::new(&label).small()).truncate());
				},
			);
			let top = ui.cursor().top();
			let rect = Rect::from_min_max(pos2(map.left, top), pos2(map.right, top + ROW - 4.0));
			(rect, ui.allocate_rect(rect, Sense::click_and_drag()))
		});
		let (rect, response) = row.inner;
		let painter = ui.painter_at(rect.expand2(Vec2::new(6.0, 0.0)));
		painter.rect_filled(rect, 0.0, ui.visuals().extreme_bg_color);
		let centre = |f: i32| map.x(f) + map.per_frame() / 2.0;
		for &(f, interp) in &frames {
			let c = pos2(centre(f), rect.center().y);
			let is_selected = sel.key.as_ref().is_some_and(|(k, sf)| *k == key && *sf == f);
			let color = if is_selected { Color32::WHITE } else { KEY_COLOR };
			let r = 5.0;
			let shape = match interp {
				Interp::Linear => vec![
					pos2(c.x, c.y - r),
					pos2(c.x + r, c.y),
					pos2(c.x, c.y + r),
					pos2(c.x - r, c.y),
				],
				Interp::Ease => (0..12)
					.map(|i| {
						let a = i as f32 / 12.0 * std::f32::consts::TAU;
						pos2(c.x + r * a.cos(), c.y + r * a.sin())
					})
					.collect(),
				Interp::Hold => vec![
					pos2(c.x - r, c.y - r),
					pos2(c.x + r, c.y - r),
					pos2(c.x + r, c.y + r),
					pos2(c.x - r, c.y + r),
				],
			};
			painter.add(egui::Shape::convex_polygon(
				shape,
				color,
				Stroke::new(1.0, Color32::BLACK),
			));
		}
		painter.vline(centre(*frame), rect.y_range(), Stroke::new(1.5, PLAYHEAD));

		let nearest = |p: Pos2| {
			frames
				.iter()
				.map(|&(f, _)| f)
				.filter(|&f| (centre(f) - p.x).abs() <= 7.0)
				.min_by(|a, b| (centre(*a) - p.x).abs().total_cmp(&(centre(*b) - p.x).abs()))
		};
		if response.drag_started()
			&& let Some(p) = response.interact_pointer_pos()
			&& let Some(f) = nearest(p)
		{
			*timeline_drag = Some(Drag::Key {
				layer: id,
				track: key.clone(),
				current: f,
			});
			sel.key = Some((key.clone(), f));
		}
		if response.dragged()
			&& let Some(p) = response.interact_pointer_pos()
			&& let Some(Drag::Key {
				layer: drag_layer,
				track: drag_track,
				current,
			}) = timeline_drag
			&& *drag_layer == id
			&& *drag_track == key
		{
			let to = map.frame_at(p.x).clamp(0, duration - 1);
			if to != *current && !frames.iter().any(|&(f, _)| f == to) {
				track.move_key(*current, to);
				*current = to;
				sel.key = Some((key.clone(), to));
			}
		}
		if (response.clicked() || response.secondary_clicked())
			&& let Some(p) = response.interact_pointer_pos()
		{
			match nearest(p) {
				Some(f) => {
					sel.key = Some((key.clone(), f));
					if response.clicked() {
						jump = Some(f);
					}
				}
				None => {
					sel.key = None;
					jump = Some(map.frame_at(p.x));
				}
			}
		}
		let selected_key = sel.key.as_ref().filter(|(k, _)| *k == key).map(|(_, f)| *f);
		if let Some(f) = selected_key {
			response.context_menu(|ui| {
				ui.label(RichText::new(format!("Keyframe at {f}")).weak());
				for interp in Interp::ALL {
					if ui.button(interp.label()).clicked() {
						track.set_interp(f, interp);
					}
				}
				ui.separator();
				if ui.button("Delete Keyframe").clicked() {
					track.remove_key(f);
					sel.key = None;
				}
			});
		}
	}
	if !any {
		ui.horizontal(|ui| {
			ui.add_space(36.0);
			ui.weak("No keyframes — click a ⏱ stopwatch in Effect Controls to animate a property.");
		});
	}
	if let Some(f) = jump {
		s.set_frame(f);
	}
}
