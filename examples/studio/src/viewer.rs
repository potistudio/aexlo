//! The composition viewer: the rendered frame, overlays for the selected
//! layer (bounds, masks, shape paths, effect points), and the tools.

use aexlo::ParamKind;
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2, pos2, vec2};
use tiny_skia::Transform;

use crate::app::{PathTarget, Pen, Studio, Tool, palette};
use crate::model::{Anim, BezPath, Geometry, Id, Layer, LayerKind, PValue, Project, ShapeItem, Vertex};
use crate::raster::{shape_path, text_bounds};
use crate::widgets::ACCENT;

const MASK_COLOR: Color32 = Color32::from_rgb(255, 210, 80);
const POINT_COLOR: Color32 = Color32::from_rgb(120, 220, 255);
const HANDLE: f32 = 8.0;

pub enum Drag {
	Pan {
		start: Vec2,
		from: Pos2,
	},
	Layer {
		id: Id,
		start: [f32; 2],
		from: [f32; 2],
	},
	Scale {
		id: Id,
		start: [f32; 2],
		anchor: Pos2,
		dist: f32,
	},
	Rotate {
		id: Id,
		start: f32,
		centre: Pos2,
		angle: f32,
	},
	EffectPoint {
		layer: Id,
		effect: Id,
		index: usize,
	},
	Vertex {
		layer: Id,
		target: PathTarget,
		vertex: usize,
		part: Part,
	},
	ShapeItem {
		layer: Id,
		item: Id,
		start: [f32; 2],
		from: [f32; 2],
	},
	Box {
		tool: Tool,
		from: [f32; 2],
	},
	PenTangent {
		layer: Id,
		target: PathTarget,
		vertex: usize,
	},
}

#[derive(Clone, Copy, PartialEq)]
pub enum Part {
	Point,
	In,
	Out,
}

/// Comp pixels to screen points.
#[derive(Clone, Copy)]
struct Geo {
	origin: Pos2,
	zoom: f32,
}

impl Geo {
	fn screen(&self, p: [f32; 2]) -> Pos2 {
		self.origin + vec2(p[0], p[1]) * self.zoom
	}

	fn comp(&self, p: Pos2) -> [f32; 2] {
		let v = (p - self.origin) / self.zoom;
		[v.x, v.y]
	}
}

fn map(t: &Transform, p: [f32; 2]) -> [f32; 2] {
	let mut pt = tiny_skia::Point::from_xy(p[0], p[1]);
	t.map_point(&mut pt);
	[pt.x, pt.y]
}

fn unmap(t: &Transform, p: [f32; 2]) -> [f32; 2] {
	t.invert().map_or(p, |inv| map(&inv, p))
}

fn add(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
	[a[0] + b[0], a[1] + b[1]]
}

fn sub(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
	[a[0] - b[0], a[1] - b[1]]
}

/// Layer pixels of a path's own coordinates: masks are in layer pixels, path
/// shape items relative to the item's position and rotation.
fn path_matrix(layer: &Layer, target: PathTarget, frame: i32) -> Transform {
	let m = layer.matrix(frame);
	match target {
		PathTarget::Mask(_) => m,
		PathTarget::ShapeItem(id) => match &layer.kind {
			LayerKind::Shape { items } => items.iter().find(|i| i.id == id).map_or(m, |item| {
				let [x, y] = item.position.at(frame);
				m.pre_translate(x, y).pre_rotate(item.rotation.at(frame))
			}),
			_ => m,
		},
	}
}

fn path(layer: &Layer, target: PathTarget) -> Option<&BezPath> {
	match target {
		PathTarget::Mask(id) => layer.masks.iter().find(|m| m.id == id).map(|m| &m.path),
		PathTarget::ShapeItem(id) => match &layer.kind {
			LayerKind::Shape { items } => items.iter().find(|i| i.id == id).and_then(|i| match &i.geometry {
				Geometry::Path(p) => Some(p),
				_ => None,
			}),
			_ => None,
		},
	}
}

fn path_mut(project: &mut Project, layer: Id, target: PathTarget) -> Option<&mut BezPath> {
	let layer = project.layer_mut(layer)?;
	match target {
		PathTarget::Mask(id) => layer.masks.iter_mut().find(|m| m.id == id).map(|m| &mut m.path),
		PathTarget::ShapeItem(id) => match &mut layer.kind {
			LayerKind::Shape { items } => items
				.iter_mut()
				.find(|i| i.id == id)
				.and_then(|i| match &mut i.geometry {
					Geometry::Path(p) => Some(p),
					_ => None,
				}),
			_ => None,
		},
	}
}

pub fn show(ui: &mut egui::Ui, s: &mut Studio) {
	let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
	let painter = ui.painter_at(rect);
	painter.rect_filled(rect, 0.0, Color32::from_gray(24));

	let (cw, ch) = (s.project.comp.width as f32, s.project.comp.height as f32);
	if s.view.fit {
		s.view.zoom = ((rect.width() - 40.0) / cw).min((rect.height() - 40.0) / ch).max(0.02);
		s.view.pan = Vec2::ZERO;
	}
	let mut geo = Geo {
		origin: rect.center() + s.view.pan - vec2(cw, ch) * s.view.zoom / 2.0,
		zoom: s.view.zoom,
	};

	// Scroll or pinch to zoom around the pointer.
	if let Some(hover) = response.hover_pos() {
		let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
		let factor = pinch * (scroll * 0.0015).exp();
		if (factor - 1.0).abs() > 1e-4 {
			let anchor = geo.comp(hover);
			let zoom = (geo.zoom * factor).clamp(0.02, 64.0);
			let origin = hover - vec2(anchor[0], anchor[1]) * zoom;
			s.view.zoom = zoom;
			s.view.pan = origin - rect.center() + vec2(cw, ch) * zoom / 2.0;
			s.view.fit = false;
			geo = Geo { origin, zoom };
		}
	}

	// The frame.
	let comp_rect = Rect::from_min_size(geo.origin, vec2(cw, ch) * geo.zoom);
	if s.view.checker {
		checkerboard(&painter, comp_rect.intersect(rect));
	}
	if let Some(texture) = s.texture() {
		painter.image(
			texture.id(),
			comp_rect,
			Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
			Color32::WHITE,
		);
	}
	painter.rect_stroke(
		comp_rect,
		0.0,
		Stroke::new(1.0, Color32::from_gray(90)),
		egui::StrokeKind::Outside,
	);

	overlays(&painter, s, geo, response.hover_pos());
	if let Some(Drag::Box { tool, from }) = &s.viewer_drag
		&& let Some(to) = ui.input(|i| i.pointer.interact_pos())
	{
		let r = Rect::from_two_pos(geo.screen(*from), to);
		let stroke = Stroke::new(1.0, Color32::WHITE);
		match tool {
			Tool::Ellipse => {
				let shape = egui::epaint::EllipseShape::stroke(r.center(), r.size() / 2.0, stroke);
				painter.add(shape);
			}
			_ => {
				painter.rect_stroke(r, 0.0, stroke, egui::StrokeKind::Middle);
			}
		}
	}
	interact(ui, s, geo, &response);

	// Corner controls.
	let fit = ui.put(
		Rect::from_min_size(rect.left_top() + vec2(8.0, 8.0), vec2(40.0, 20.0)),
		egui::Button::new("Fit").small(),
	);
	if fit.clicked() {
		s.view.fit = true;
	}
	let actual = ui.put(
		Rect::from_min_size(rect.left_top() + vec2(52.0, 8.0), vec2(44.0, 20.0)),
		egui::Button::new("100%").small(),
	);
	if actual.clicked() {
		s.view.fit = false;
		s.view.zoom = 1.0;
		s.view.pan = Vec2::ZERO;
	}
	painter.text(
		rect.left_top() + vec2(104.0, 11.0),
		egui::Align2::LEFT_TOP,
		format!("{:.0}%", s.view.zoom * 100.0),
		egui::FontId::proportional(12.0),
		Color32::from_gray(160),
	);
}

fn checkerboard(painter: &egui::Painter, rect: Rect) {
	const CELL: f32 = 12.0;
	painter.rect_filled(rect, 0.0, Color32::from_gray(200));
	let dark = Color32::from_gray(160);
	let mut y = rect.top();
	let mut row = 0;
	while y < rect.bottom() {
		let mut x = rect.left() + if row % 2 == 0 { 0.0 } else { CELL };
		while x < rect.right() {
			let cell = Rect::from_min_size(pos2(x, y), vec2(CELL, CELL)).intersect(rect);
			painter.rect_filled(cell, 0.0, dark);
			x += CELL * 2.0;
		}
		y += CELL;
		row += 1;
	}
}

/// The selected layer's corners on screen, clockwise from the top left.
fn corners(layer: &Layer, s: &Studio, geo: Geo) -> [Pos2; 4] {
	let m = layer.matrix(s.frame);
	let (w, h) = layer.size(&s.project.comp);
	let (w, h) = (w as f32, h as f32);
	[[0.0, 0.0], [w, 0.0], [w, h], [0.0, h]].map(|p| geo.screen(map(&m, p)))
}

/// Where the rotation knob sits: above the top edge's midpoint.
fn rotation_knob(c: &[Pos2; 4]) -> Pos2 {
	let mid = c[0] + (c[1] - c[0]) / 2.0;
	let up = (c[0] - c[3]).normalized();
	mid + up * 28.0
}

/// Effect point parameters with handles: `(effect, index, name, layer point)`.
fn effect_points(s: &Studio, layer: &Layer) -> Vec<(Id, usize, String, [f32; 2])> {
	let size = layer.size(&s.project.comp);
	let mut out = Vec::new();
	for effect in &layer.effects {
		if !effect.enabled || s.sel.effect.is_some_and(|e| e != effect.id) {
			continue;
		}
		let Some(Ok(info)) = s.infos.get(&effect.id) else {
			continue;
		};
		let hidden = s.param_flags.get(&effect.id).map(|f| &f.0);
		for desc in &info.params {
			if !matches!(desc.kind, ParamKind::Point | ParamKind::Point3D)
				|| hidden.is_some_and(|h| h.get(desc.index).copied().unwrap_or(false))
			{
				continue;
			}
			let value = effect
				.params
				.get(&desc.index)
				.map(|a| a.at(s.frame))
				.or_else(|| info.default_value(desc.index, size));
			let p = match value {
				Some(PValue::Point(p)) => p,
				Some(PValue::Point3D([x, y, _])) => [x as f32, y as f32],
				_ => continue,
			};
			out.push((effect.id, desc.index, desc.name.clone(), p));
		}
	}
	out
}

/// Every path drawn on the selected layer with its matrix, selected first.
fn layer_paths(layer: &Layer, s: &Studio) -> Vec<(PathTarget, Transform)> {
	let mut out: Vec<(PathTarget, Transform)> = Vec::new();
	let mut push = |t: PathTarget| out.push((t, path_matrix(layer, t, s.frame)));
	for mask in &layer.masks {
		push(PathTarget::Mask(mask.id));
	}
	if let LayerKind::Shape { items } = &layer.kind {
		for item in items {
			if matches!(item.geometry, Geometry::Path(_)) {
				push(PathTarget::ShapeItem(item.id));
			}
		}
	}
	let selected = |t: &PathTarget| match t {
		PathTarget::Mask(id) => s.sel.mask == Some(*id),
		PathTarget::ShapeItem(id) => s.sel.shape == Some(*id),
	};
	out.sort_by_key(|(t, _)| !selected(t));
	out
}

fn is_selected_target(s: &Studio, target: PathTarget) -> bool {
	let selected = match target {
		PathTarget::Mask(id) => s.sel.mask == Some(id),
		PathTarget::ShapeItem(id) => s.sel.shape == Some(id),
	};
	selected || s.pen.is_some_and(|p| p.target == target)
}

fn draw_bez(painter: &egui::Painter, path: &BezPath, m: &Transform, geo: Geo, stroke: Stroke) {
	for seg in path.segments() {
		let points = seg.map(|p| geo.screen(map(m, p)));
		painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
			points,
			false,
			Color32::TRANSPARENT,
			stroke,
		));
	}
}

fn draw_skia_path(painter: &egui::Painter, path: &tiny_skia::Path, m: &Transform, geo: Geo, stroke: Stroke) {
	let pt = |p: tiny_skia::Point| geo.screen(map(m, [p.x, p.y]));
	let (mut start, mut cur) = (Pos2::ZERO, Pos2::ZERO);
	for seg in path.segments() {
		match seg {
			tiny_skia::PathSegment::MoveTo(p) => {
				start = pt(p);
				cur = start;
			}
			tiny_skia::PathSegment::LineTo(p) => {
				let p = pt(p);
				painter.line_segment([cur, p], stroke);
				cur = p;
			}
			tiny_skia::PathSegment::QuadTo(c, p) => {
				let p = pt(p);
				painter.add(egui::epaint::QuadraticBezierShape::from_points_stroke(
					[cur, pt(c), p],
					false,
					Color32::TRANSPARENT,
					stroke,
				));
				cur = p;
			}
			tiny_skia::PathSegment::CubicTo(c0, c1, p) => {
				let p = pt(p);
				painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
					[cur, pt(c0), pt(c1), p],
					false,
					Color32::TRANSPARENT,
					stroke,
				));
				cur = p;
			}
			tiny_skia::PathSegment::Close => {
				painter.line_segment([cur, start], stroke);
				cur = start;
			}
		}
	}
}

fn overlays(painter: &egui::Painter, s: &Studio, geo: Geo, hover: Option<Pos2>) {
	let Some(layer) = s.sel.layer.and_then(|id| s.project.layer(id)) else {
		return;
	};
	let frame = s.frame;
	let m = layer.matrix(frame);

	if !matches!(layer.kind, LayerKind::Adjustment) {
		let c = corners(layer, s, geo);
		let outline = Stroke::new(1.0, ACCENT);
		painter.add(egui::Shape::closed_line(c.to_vec(), outline));
		for corner in c {
			painter.rect_filled(Rect::from_center_size(corner, Vec2::splat(7.0)), 1.0, ACCENT);
		}
		let knob = rotation_knob(&c);
		painter.line_segment([c[0] + (c[1] - c[0]) / 2.0, knob], outline);
		painter.circle(knob, 5.0, Color32::from_gray(30), outline);
		let anchor = geo.screen(map(&m, layer.transform.anchor.at(frame)));
		painter.circle_stroke(anchor, 5.0, Stroke::new(1.5, ACCENT));
		painter.line_segment([anchor - vec2(9.0, 0.0), anchor + vec2(9.0, 0.0)], outline);
		painter.line_segment([anchor - vec2(0.0, 9.0), anchor + vec2(0.0, 9.0)], outline);
	}

	// Shape outlines; the selected item's centre gets a handle.
	if let LayerKind::Shape { items } = &layer.kind {
		for item in items {
			let selected = s.sel.shape == Some(item.id);
			if let Some(path) = shape_path(item, frame)
				&& !matches!(item.geometry, Geometry::Path(_))
			{
				let stroke = Stroke::new(if selected { 1.5 } else { 0.7 }, ACCENT.gamma_multiply(0.8));
				draw_skia_path(painter, &path, &m, geo, stroke);
			}
			if selected && !matches!(item.geometry, Geometry::Path(_)) {
				let c = geo.screen(map(&m, item.position.at(frame)));
				painter.rect_stroke(
					Rect::from_center_size(c, Vec2::splat(9.0)),
					0.0,
					Stroke::new(1.5, ACCENT),
					egui::StrokeKind::Middle,
				);
			}
		}
	}

	// Masks and path items, with points for the selected one.
	for (target, pm) in layer_paths(layer, s) {
		let Some(path) = path(layer, target) else { continue };
		let selected = is_selected_target(s, target);
		let color = match target {
			PathTarget::Mask(_) => MASK_COLOR,
			PathTarget::ShapeItem(_) => ACCENT,
		};
		draw_bez(
			painter,
			path,
			&pm,
			geo,
			Stroke::new(if selected { 1.5 } else { 1.0 }, color),
		);
		for (i, v) in path.vertices.iter().enumerate() {
			let p = geo.screen(map(&pm, v.p));
			if selected {
				for t in [v.tin, v.tout] {
					if t != [0.0, 0.0] {
						let h = geo.screen(map(&pm, add(v.p, t)));
						painter.line_segment([p, h], Stroke::new(1.0, color));
						painter.circle_filled(h, 3.5, color);
					}
				}
			}
			let size = if i == 0 { 8.0 } else { 6.0 };
			let r = Rect::from_center_size(p, Vec2::splat(size));
			if selected {
				painter.rect_filled(r, 0.0, color);
			} else {
				painter.rect_stroke(r, 0.0, Stroke::new(1.0, color), egui::StrokeKind::Middle);
			}
		}
	}

	// Effect points.
	for (_, _, name, p) in effect_points(s, layer) {
		let c = geo.screen(map(&m, p));
		painter.circle_stroke(c, 7.0, Stroke::new(2.0, Color32::BLACK));
		painter.circle_stroke(c, 7.0, Stroke::new(1.2, POINT_COLOR));
		painter.line_segment(
			[c - vec2(11.0, 0.0), c + vec2(11.0, 0.0)],
			Stroke::new(1.0, POINT_COLOR),
		);
		painter.line_segment(
			[c - vec2(0.0, 11.0), c + vec2(0.0, 11.0)],
			Stroke::new(1.0, POINT_COLOR),
		);
		painter.text(
			c + vec2(10.0, -10.0),
			egui::Align2::LEFT_BOTTOM,
			name,
			egui::FontId::proportional(11.0),
			POINT_COLOR,
		);
	}

	// The pen's rubber band.
	if s.tool == Tool::Pen
		&& let (Some(pen), Some(hover)) = (s.pen, hover)
		&& let Some(path) = s.project.layer(pen.layer).and_then(|l| path(l, pen.target))
		&& let Some(last) = path.vertices.last()
		&& s.viewer_drag.is_none()
	{
		let pm = path_matrix(layer, pen.target, frame);
		let a = geo.screen(map(&pm, last.p));
		painter.line_segment([a, hover], Stroke::new(1.0, Color32::from_white_alpha(140)));
		if path.vertices.len() >= 2 {
			let first = geo.screen(map(&pm, path.vertices[0].p));
			if first.distance(hover) < HANDLE {
				painter.circle_stroke(first, 7.0, Stroke::new(1.5, Color32::WHITE));
			}
		}
	}
}

fn interact(ui: &egui::Ui, s: &mut Studio, geo: Geo, response: &egui::Response) {
	let (pressed, middle_pressed, down, pos, mods) = ui.input(|i| {
		(
			i.pointer.primary_pressed(),
			i.pointer.button_pressed(egui::PointerButton::Middle),
			i.pointer.any_down(),
			i.pointer.interact_pos(),
			i.modifiers,
		)
	});
	let Some(pos) = pos else { return };

	match s.tool {
		Tool::Pen | Tool::Rect | Tool::Ellipse | Tool::Star if response.hovered() => {
			ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair)
		}
		_ => {}
	}

	if response.hovered() && (middle_pressed || pressed && mods.alt) {
		s.viewer_drag = Some(Drag::Pan {
			start: s.view.pan,
			from: pos,
		});
	} else if response.hovered() && pressed {
		begin(s, geo, pos, mods);
	}

	if s.viewer_drag.is_some() {
		if down {
			update(s, geo, pos, mods);
		} else {
			finish(s, geo, pos);
			s.viewer_drag = None;
		}
	}
	if matches!(s.viewer_drag, Some(Drag::Pan { .. })) {
		ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
	}
}

fn begin(s: &mut Studio, geo: Geo, pos: Pos2, mods: egui::Modifiers) {
	match s.tool {
		Tool::Select => select_begin(s, geo, pos),
		Tool::Pen => pen_begin(s, geo, pos),
		tool => {
			s.viewer_drag = Some(Drag::Box {
				tool,
				from: geo.comp(pos),
			})
		}
	}
	let _ = mods;
}

fn select_begin(s: &mut Studio, geo: Geo, pos: Pos2) {
	let frame = s.frame;
	if let Some(layer) = s.sel.layer.and_then(|id| s.project.layer(id)).cloned() {
		let m = layer.matrix(frame);

		for (effect, index, _, p) in effect_points(s, &layer) {
			if geo.screen(map(&m, p)).distance(pos) <= HANDLE + 2.0 {
				s.sel.effect = Some(effect);
				s.viewer_drag = Some(Drag::EffectPoint {
					layer: layer.id,
					effect,
					index,
				});
				return;
			}
		}

		for (target, pm) in layer_paths(&layer, s) {
			let Some(path) = path(&layer, target) else { continue };
			let selected = is_selected_target(s, target);
			for (i, v) in path.vertices.iter().enumerate() {
				let mut parts = vec![(Part::Point, v.p)];
				if selected {
					if v.tin != [0.0, 0.0] {
						parts.push((Part::In, add(v.p, v.tin)));
					}
					if v.tout != [0.0, 0.0] {
						parts.push((Part::Out, add(v.p, v.tout)));
					}
				}
				for (part, p) in parts {
					if geo.screen(map(&pm, p)).distance(pos) <= HANDLE {
						match target {
							PathTarget::Mask(id) => {
								s.sel.mask = Some(id);
								s.sel.shape = None;
							}
							PathTarget::ShapeItem(id) => {
								s.sel.shape = Some(id);
								s.sel.mask = None;
							}
						}
						s.viewer_drag = Some(Drag::Vertex {
							layer: layer.id,
							target,
							vertex: i,
							part,
						});
						return;
					}
				}
			}
		}

		if let LayerKind::Shape { items } = &layer.kind
			&& let Some(item) = items.iter().find(|i| Some(i.id) == s.sel.shape)
			&& geo.screen(map(&m, item.position.at(frame))).distance(pos) <= HANDLE
		{
			s.viewer_drag = Some(Drag::ShapeItem {
				layer: layer.id,
				item: item.id,
				start: item.position.at(frame),
				from: unmap(&m, geo.comp(pos)),
			});
			return;
		}

		if !matches!(layer.kind, LayerKind::Adjustment) {
			let c = corners(&layer, s, geo);
			let anchor = geo.screen(map(&m, layer.transform.anchor.at(frame)));
			let knob = rotation_knob(&c);
			if knob.distance(pos) <= HANDLE {
				s.viewer_drag = Some(Drag::Rotate {
					id: layer.id,
					start: layer.transform.rotation.at(frame),
					centre: anchor,
					angle: (pos - anchor).angle(),
				});
				return;
			}
			if c.iter().any(|c| c.distance(pos) <= HANDLE) {
				s.viewer_drag = Some(Drag::Scale {
					id: layer.id,
					start: layer.transform.scale.at(frame),
					anchor,
					dist: anchor.distance(pos).max(1.0),
				});
				return;
			}
		}
	}

	// Pick the topmost layer under the pointer.
	let p = geo.comp(pos);
	let hit = s
		.project
		.layers
		.clone()
		.into_iter()
		.filter(|l| l.is_active(frame))
		.find_map(|l| hit_test(s, &l, p).map(|item| (l, item)));
	match hit {
		Some((layer, item)) => {
			s.select_layer(Some(layer.id));
			if item.is_some() {
				s.sel.shape = item;
			}
			s.viewer_drag = Some(Drag::Layer {
				id: layer.id,
				start: layer.transform.position.at(frame),
				from: p,
			});
		}
		None => s.select_layer(None),
	}
}

/// Whether comp point `p` hits the layer's content; `Some(Some(item))` for a
/// shape item.
fn hit_test(s: &mut Studio, layer: &Layer, p: [f32; 2]) -> Option<Option<Id>> {
	let m = layer.matrix(s.frame);
	let [x, y] = unmap(&m, p);
	let (w, h) = layer.size(&s.project.comp);
	let inside = |b: [f32; 4]| x >= b[0] && x <= b[2] && y >= b[1] && y <= b[3];
	match &layer.kind {
		LayerKind::Adjustment => None,
		LayerKind::Shape { items } => items
			.iter()
			.find(|item| {
				shape_path(item, s.frame).is_some_and(|path| {
					let b = path.bounds();
					inside([b.left() - 4.0, b.top() - 4.0, b.right() + 4.0, b.bottom() + 4.0])
				})
			})
			.map(|item| Some(item.id)),
		LayerKind::Text(text) => text_bounds(text, s.frame, &mut s.fonts)
			.filter(|b| inside(*b))
			.map(|_| None),
		_ => inside([0.0, 0.0, w as f32, h as f32]).then_some(None),
	}
}

fn pen_begin(s: &mut Studio, geo: Geo, pos: Pos2) {
	let frame = s.frame;
	// Continue the current path if it still exists.
	let pen = s
		.pen
		.filter(|p| s.project.layer(p.layer).and_then(|l| path(l, p.target)).is_some());
	let pen = match pen {
		Some(pen) => pen,
		None => match new_pen_target(s) {
			Some(pen) => pen,
			None => return,
		},
	};
	s.pen = Some(pen);
	let Some(layer) = s.project.layer(pen.layer) else {
		return;
	};
	let pm = path_matrix(layer, pen.target, frame);
	let local = unmap(&pm, geo.comp(pos));
	let Some(path) = path_mut(&mut s.project, pen.layer, pen.target) else {
		return;
	};
	if path.vertices.len() >= 2 && geo.screen(map(&pm, path.vertices[0].p)).distance(pos) <= HANDLE {
		path.closed = true;
		s.pen = None;
		return;
	}
	path.vertices.push(Vertex::corner(local[0], local[1]));
	s.viewer_drag = Some(Drag::PenTangent {
		layer: pen.layer,
		target: pen.target,
		vertex: path.vertices.len() - 1,
	});
}

/// Start a new path: a path item on a shape layer, else a mask on the
/// selected layer, else a path on a new shape layer.
fn new_pen_target(s: &mut Studio) -> Option<Pen> {
	let selected = s.sel.layer.and_then(|id| s.project.layer(id));
	let on_shape_layer = selected.is_some_and(|l| matches!(l.kind, LayerKind::Shape { .. }));
	if let Some(layer) = selected.map(|l| l.id)
		&& (!on_shape_layer || s.view.draw_masks)
	{
		let id = s.project.new_mask(layer, BezPath::default())?;
		s.sel.mask = Some(id);
		return Some(Pen {
			layer,
			target: PathTarget::Mask(id),
		});
	}
	let layer = match s.sel.layer.filter(|_| on_shape_layer) {
		Some(layer) => layer,
		None => {
			let n = s.project.layers.len() + 1;
			s.add_layer(&format!("Shape Layer {n}"), LayerKind::Shape { items: Vec::new() })
		}
	};
	let id = s.project.alloc_id();
	let n = shape_count(s, layer);
	let mut item = ShapeItem::new(id, Geometry::Path(BezPath::default()), [0.0, 0.0], palette(n + 1));
	item.stroke_on = true;
	if let Some(LayerKind::Shape { items }) = s.project.layer_mut(layer).map(|l| &mut l.kind) {
		items.insert(0, item);
	}
	s.sel.shape = Some(id);
	Some(Pen {
		layer,
		target: PathTarget::ShapeItem(id),
	})
}

fn shape_count(s: &Studio, layer: Id) -> usize {
	match s.project.layer(layer).map(|l| &l.kind) {
		Some(LayerKind::Shape { items }) => items.len(),
		_ => 0,
	}
}

fn update(s: &mut Studio, geo: Geo, pos: Pos2, mods: egui::Modifiers) {
	let frame = s.frame;
	let comp_size = (s.project.comp.width, s.project.comp.height);
	let Some(drag) = &s.viewer_drag else { return };
	match *drag {
		Drag::Pan { start, from } => {
			s.view.pan = start + (pos - from);
			s.view.fit = false;
		}
		Drag::Layer { id, start, from } => {
			let mut d = sub(geo.comp(pos), from);
			if mods.shift {
				if d[0].abs() > d[1].abs() {
					d[1] = 0.0;
				} else {
					d[0] = 0.0;
				}
			}
			if let Some(layer) = s.project.layer_mut(id) {
				layer.transform.position.set(frame, add(start, d));
			}
		}
		Drag::Scale {
			id,
			start,
			anchor,
			dist,
		} => {
			let f = anchor.distance(pos) / dist;
			if let Some(layer) = s.project.layer_mut(id) {
				layer.transform.scale.set(frame, [start[0] * f, start[1] * f]);
			}
		}
		Drag::Rotate {
			id,
			start,
			centre,
			angle,
		} => {
			let mut deg = start + ((pos - centre).angle() - angle).to_degrees();
			if mods.shift {
				deg = (deg / 15.0).round() * 15.0;
			}
			if let Some(layer) = s.project.layer_mut(id) {
				layer.transform.rotation.set(frame, deg);
			}
		}
		Drag::EffectPoint { layer, effect, index } => {
			let Some(l) = s.project.layer(layer) else { return };
			let [x, y] = unmap(&l.matrix(frame), geo.comp(pos));
			let size = l.size(&s.project.comp);
			let info = match s.infos.get(&effect) {
				Some(Ok(info)) => info.clone(),
				_ => return,
			};
			let Some(fx) = s.project.effect_mut(effect) else { return };
			let Some(default) = info.default_value(index, size) else {
				return;
			};
			let current = fx.params.get(&index).map_or(default.clone(), |a| a.at(frame));
			let value = match current {
				PValue::Point3D([_, _, z]) => PValue::Point3D([x as f64, y as f64, z]),
				_ => PValue::Point([x, y]),
			};
			fx.params
				.entry(index)
				.or_insert_with(|| Anim::new(default))
				.set(frame, value);
		}
		Drag::Vertex {
			layer,
			target,
			vertex,
			part,
		} => {
			let Some(l) = s.project.layer(layer) else { return };
			let local = unmap(&path_matrix(l, target, frame), geo.comp(pos));
			let Some(v) = path_mut(&mut s.project, layer, target).and_then(|p| p.vertices.get_mut(vertex)) else {
				return;
			};
			match part {
				Part::Point => v.p = local,
				Part::In => {
					v.tin = sub(local, v.p);
					if !mods.alt {
						v.tout = [-v.tin[0], -v.tin[1]];
					}
				}
				Part::Out => {
					v.tout = sub(local, v.p);
					if !mods.alt {
						v.tin = [-v.tout[0], -v.tout[1]];
					}
				}
			}
		}
		Drag::ShapeItem {
			layer,
			item,
			start,
			from,
		} => {
			let Some(l) = s.project.layer_mut(layer) else { return };
			let local = unmap(&l.matrix(frame), geo.comp(pos));
			if let LayerKind::Shape { items } = &mut l.kind
				&& let Some(item) = items.iter_mut().find(|i| i.id == item)
			{
				item.position.set(frame, add(start, sub(local, from)));
			}
		}
		Drag::PenTangent { layer, target, vertex } => {
			let Some(l) = s.project.layer(layer) else { return };
			let local = unmap(&path_matrix(l, target, frame), geo.comp(pos));
			if let Some(v) = path_mut(&mut s.project, layer, target).and_then(|p| p.vertices.get_mut(vertex)) {
				let d = sub(local, v.p);
				if d[0].hypot(d[1]) * geo.zoom > 2.0 {
					v.tout = d;
					v.tin = [-d[0], -d[1]];
				}
			}
		}
		Drag::Box { .. } => {}
	}
	let _ = comp_size;
}

fn finish(s: &mut Studio, geo: Geo, pos: Pos2) {
	let Some(Drag::Box { tool, from }) = s.viewer_drag else {
		return;
	};
	let to = geo.comp(pos);
	if (to[0] - from[0]).abs() * geo.zoom < 3.0 && (to[1] - from[1]).abs() * geo.zoom < 3.0 {
		return;
	}
	let frame = s.frame;
	let selected = s.sel.layer.and_then(|id| s.project.layer(id));
	let as_mask = match selected.map(|l| &l.kind) {
		Some(LayerKind::Shape { .. }) => s.view.draw_masks,
		Some(_) => true,
		None => false,
	};

	if as_mask {
		let layer = selected.expect("a selected layer");
		let m = layer.matrix(frame);
		let (a, b) = (unmap(&m, from), unmap(&m, to));
		let (x0, y0, x1, y1) = (a[0].min(b[0]), a[1].min(b[1]), a[0].max(b[0]), a[1].max(b[1]));
		let path = match tool {
			Tool::Ellipse => BezPath::ellipse(x0, y0, x1, y1),
			Tool::Star => star_path((x0 + x1) / 2.0, (y0 + y1) / 2.0, (x1 - x0).max(y1 - y0) / 2.0),
			_ => BezPath::rect(x0, y0, x1, y1),
		};
		let id = layer.id;
		if let Some(mask) = s.project.new_mask(id, path) {
			s.sel.mask = Some(mask);
			s.sel.shape = None;
		}
		return;
	}

	let layer = match selected.filter(|l| matches!(l.kind, LayerKind::Shape { .. })) {
		Some(l) => l.id,
		None => {
			let n = s.project.layers.len() + 1;
			s.add_layer(&format!("Shape Layer {n}"), LayerKind::Shape { items: Vec::new() })
		}
	};
	let Some(m) = s.project.layer(layer).map(|l| l.matrix(frame)) else {
		return;
	};
	let (a, b) = (unmap(&m, from), unmap(&m, to));
	let centre = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
	let size = [(b[0] - a[0]).abs(), (b[1] - a[1]).abs()];
	let geometry = match tool {
		Tool::Ellipse => Geometry::Ellipse { size: Anim::new(size) },
		Tool::Star => {
			let outer = size[0].max(size[1]) / 2.0;
			Geometry::Star {
				points: 5,
				outer: Anim::new(outer),
				inner: Anim::new(outer * 0.45),
			}
		}
		_ => Geometry::Rect {
			size: Anim::new(size),
			roundness: Anim::new(0.0),
		},
	};
	let id = s.project.alloc_id();
	let n = shape_count(s, layer);
	let item = ShapeItem::new(id, geometry, centre, palette(n + 1));
	if let Some(LayerKind::Shape { items }) = s.project.layer_mut(layer).map(|l| &mut l.kind) {
		items.insert(0, item);
	}
	s.sel.shape = Some(id);
	s.sel.mask = None;
}

fn star_path(cx: f32, cy: f32, outer: f32) -> BezPath {
	let inner = outer * 0.45;
	BezPath {
		vertices: (0..10)
			.map(|i| {
				let r = if i % 2 == 0 { outer } else { inner };
				let a = std::f32::consts::PI * i as f32 / 5.0 - std::f32::consts::FRAC_PI_2;
				Vertex::corner(cx + r * a.cos(), cy + r * a.sin())
			})
			.collect(),
		closed: true,
	}
}
