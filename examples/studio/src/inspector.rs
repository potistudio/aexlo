//! The right panel: the selected layer's contents, transform, masks and
//! effect controls.

use aexlo::ParamKind;
use eframe::egui::{self, Color32, RichText, SliderClamping};

use crate::app::{PathTarget, Pen, Studio, Tool, palette};
use crate::engine::{ParamDesc, PluginInfo};
use crate::model::{
	Anim, BezPath, BlendMode, Effect, Geometry, Id, Layer, LayerKind, MaskMode, PValue, ShapeItem, TextAlign,
};
use crate::widgets::{self, anim_row, drag_f32, drag_xy};

/// Effect-panel actions applied after the panel releases its borrows.
#[derive(Default)]
struct Actions {
	press: Option<(Id, usize)>,
	move_effect: Option<(usize, isize)>,
	remove_effect: Option<usize>,
	select_effect: Option<Id>,
	select_mask: Option<Option<Id>>,
	select_shape: Option<Option<Id>>,
	add_shape: Option<Geometry>,
}

pub fn show(ui: &mut egui::Ui, s: &mut Studio) {
	let Some(id) = s.sel.layer.filter(|&id| s.project.layer(id).is_some()) else {
		ui.heading("Effect Controls");
		ui.weak("Select a layer in the viewer or the timeline.");
		return;
	};
	let frame = s.frame;
	let comp = s.project.comp.clone();
	let layer_names: Vec<(Id, String)> = s.project.layers.iter().map(|l| (l.id, l.name.clone())).collect();
	let mut actions = Actions::default();

	egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
		let Studio {
			project,
			infos,
			param_flags,
			sel,
			last,
			..
		} = &mut *s;
		let Some(layer) = project.layer_mut(id) else { return };

		header(ui, layer);
		ui.separator();

		egui::CollapsingHeader::new(RichText::new(layer.kind.label()).strong())
			.id_salt("contents")
			.default_open(true)
			.show(ui, |ui| contents(ui, layer, frame, sel.shape, &mut actions));

		egui::CollapsingHeader::new(RichText::new("Transform").strong())
			.id_salt("transform")
			.default_open(true)
			.show(ui, |ui| transform(ui, layer, frame, &comp));

		egui::CollapsingHeader::new(RichText::new(format!("Masks ({})", layer.masks.len())).strong())
			.id_salt("masks")
			.default_open(!layer.masks.is_empty())
			.show(ui, |ui| masks(ui, layer, frame, sel.mask, &mut actions));

		ui.separator();
		ui.heading(format!("Effects ({})", layer.effects.len()));
		if layer.effects.is_empty() {
			ui.weak("Double-click an effect in the left panel to apply it.");
		}
		let size = layer.size(&comp);
		let masks: Vec<(Id, String)> = layer.masks.iter().map(|m| (m.id, m.name.clone())).collect();
		let count = layer.effects.len();
		for (i, effect) in layer.effects.iter_mut().enumerate() {
			let time = last.as_ref().and_then(|l| l.effect_times.get(&effect.id));
			let title = format!(
				"fx  {}{}",
				crate::plugins::display_name(&effect.plugin),
				time.map_or(String::new(), |t| format!("   {:.1} ms", t.as_secs_f64() * 1000.0))
			);
			let selected = sel.effect == Some(effect.id);
			egui::CollapsingHeader::new(RichText::new(title).strong().color(if selected {
				widgets::ACCENT
			} else {
				Color32::PLACEHOLDER
			}))
			.id_salt(("effect", effect.id))
			.default_open(true)
			.show(ui, |ui| {
				ui.horizontal(|ui| {
					ui.checkbox(&mut effect.enabled, "On");
					if ui
						.selectable_label(selected, "◎ Handles")
						.on_hover_text("Show this effect's points in the viewer")
						.clicked()
					{
						actions.select_effect = Some(effect.id);
					}
					if ui.add_enabled(i > 0, egui::Button::new("⏶").small()).clicked() {
						actions.move_effect = Some((i, -1));
					}
					if ui.add_enabled(i + 1 < count, egui::Button::new("⏷").small()).clicked() {
						actions.move_effect = Some((i, 1));
					}
					if ui.small_button("Reset").clicked() {
						effect.params.clear();
						effect.layers.clear();
					}
					if ui.small_button("🗙").on_hover_text("Remove effect").clicked() {
						actions.remove_effect = Some(i);
					}
				});
				match infos.get(&effect.id) {
					None => {
						ui.horizontal(|ui| {
							ui.spinner();
							ui.label("Loading plugin…");
						});
					}
					Some(Err(e)) => {
						ui.colored_label(Color32::from_rgb(240, 90, 90), format!("Failed to load: {e}"));
					}
					Some(Ok(info)) => {
						let info = info.clone();
						let empty = (Vec::new(), Vec::new());
						let (hidden, disabled) = param_flags.get(&effect.id).unwrap_or(&empty);
						let flags = format!(
							"{} render{}",
							if info.smart_render { "Smart" } else { "Legacy" },
							if info.gpu { " · GPU" } else { "" }
						);
						ui.weak(flags).on_hover_text(&info.about);
						let mut cx = ParamCx {
							info: &info,
							hidden,
							disabled,
							frame,
							size,
							layers: &layer_names,
							self_id: id,
							masks: &masks,
							pressed: None,
						};
						let mut index = 1;
						block(ui, &mut cx, effect, &mut index);
						if let Some(param) = cx.pressed {
							actions.press = Some((effect.id, param));
						}
					}
				}
			});
		}
	});

	apply(s, id, actions);
}

fn apply(s: &mut Studio, id: Id, actions: Actions) {
	if let Some((effect, index)) = actions.press {
		s.press_button(effect, index);
	}
	if let Some(effect) = actions.select_effect {
		s.sel.effect = if s.sel.effect == Some(effect) {
			None
		} else {
			Some(effect)
		};
	}
	if let Some(mask) = actions.select_mask {
		s.sel.mask = mask;
		s.sel.shape = None;
	}
	if let Some(shape) = actions.select_shape {
		s.sel.shape = shape;
		s.sel.mask = None;
	}
	if let Some(geometry) = actions.add_shape {
		let item_id = s.project.alloc_id();
		let is_path = matches!(geometry, Geometry::Path(_));
		let comp = &s.project.comp;
		let centre = if is_path {
			[0.0, 0.0]
		} else {
			[comp.width as f32 / 2.0, comp.height as f32 / 2.0]
		};
		if let Some(LayerKind::Shape { items }) = s.project.layer_mut(id).map(|l| &mut l.kind) {
			let n = items.len();
			items.insert(0, ShapeItem::new(item_id, geometry, centre, palette(n + 1)));
			s.sel.shape = Some(item_id);
			s.sel.mask = None;
			if is_path {
				s.pen = Some(Pen {
					layer: id,
					target: PathTarget::ShapeItem(item_id),
				});
				s.tool = Tool::Pen;
			}
		}
	}
	let Some(layer) = s.project.layer_mut(id) else { return };
	if let Some((i, by)) = actions.move_effect {
		let j = (i as isize + by) as usize;
		layer.effects.swap(i, j);
	}
	if let Some(i) = actions.remove_effect {
		let removed = layer.effects.remove(i);
		if s.sel.effect == Some(removed.id) {
			s.sel.effect = None;
		}
	}
}

fn header(ui: &mut egui::Ui, layer: &mut Layer) {
	ui.horizontal(|ui| {
		ui.checkbox(&mut layer.visible, "");
		ui.add(egui::TextEdit::singleline(&mut layer.name).desired_width(f32::INFINITY));
	});
	egui::Grid::new("layer").num_columns(2).show(ui, |ui| {
		ui.label("Blend");
		widgets::combo(ui, "blend", &mut layer.blend, &BlendMode::ALL, |m| format!("{m:?}"));
		ui.end_row();
		ui.label("In / Out");
		ui.horizontal(|ui| {
			let out = layer.out_frame;
			ui.add(
				egui::DragValue::new(&mut layer.in_frame)
					.range(i32::MIN..=out - 1)
					.suffix(" f"),
			);
			let start = layer.in_frame;
			ui.add(
				egui::DragValue::new(&mut layer.out_frame)
					.range(start + 1..=i32::MAX)
					.suffix(" f"),
			);
		});
		ui.end_row();
		ui.label("Start")
			.on_hover_text("Comp frame of the layer's time 0 — the time effects see");
		ui.add(egui::DragValue::new(&mut layer.start).suffix(" f"));
		ui.end_row();
	});
}

fn contents(ui: &mut egui::Ui, layer: &mut Layer, frame: i32, selected_shape: Option<Id>, actions: &mut Actions) {
	match &mut layer.kind {
		LayerKind::Solid { color, width, height } => {
			egui::Grid::new("solid").num_columns(3).show(ui, |ui| {
				anim_row(ui, "Color", color, frame, widgets::color);
				ui.label("");
				ui.label("Size");
				ui.horizontal(|ui| {
					ui.add(egui::DragValue::new(width).range(1..=8192));
					ui.add(egui::DragValue::new(height).range(1..=8192));
				});
				ui.end_row();
			});
		}
		LayerKind::Image { path, width, height } => {
			ui.label(RichText::new(&*path).small()).on_hover_text(&*path);
			ui.weak(format!("{width} × {height}"));
			if ui.button("Replace…").clicked()
				&& let Some(new) = rfd::FileDialog::new()
					.add_filter("Images", &["png", "jpg", "jpeg"])
					.pick_file()
				&& let Ok((w, h)) = image::image_dimensions(&new)
			{
				*path = new.to_string_lossy().into_owned();
				(*width, *height) = (w, h);
			}
		}
		LayerKind::Text(text) => {
			ui.add(
				egui::TextEdit::multiline(&mut text.text)
					.desired_rows(2)
					.desired_width(f32::INFINITY),
			);
			ui.horizontal(|ui| {
				let font = text
					.font
					.as_deref()
					.and_then(|f| std::path::Path::new(f).file_name())
					.map_or("Built-in".into(), |n| n.to_string_lossy().into_owned());
				ui.label(format!("Font: {font}"));
				if ui.small_button("Choose…").clicked()
					&& let Some(file) = rfd::FileDialog::new()
						.add_filter("Fonts", &["ttf", "otf", "ttc"])
						.pick_file()
				{
					text.font = Some(file.to_string_lossy().into_owned());
				}
				if text.font.is_some() && ui.small_button("Built-in").clicked() {
					text.font = None;
				}
			});
			egui::Grid::new("text").num_columns(3).show(ui, |ui| {
				anim_row(ui, "Size", &mut text.size, frame, |ui, v| drag_f32(ui, v, 0.5, " px"));
				anim_row(ui, "Color", &mut text.color, frame, widgets::color);
				anim_row(ui, "Position", &mut text.position, frame, |ui, v| {
					drag_xy(ui, v, 1.0, "")
				});
				anim_row(ui, "Tracking", &mut text.tracking, frame, |ui, v| {
					drag_f32(ui, v, 0.2, " px")
				});
				ui.label("");
				ui.label("Align");
				widgets::combo(
					ui,
					"align",
					&mut text.align,
					&[TextAlign::Left, TextAlign::Center, TextAlign::Right],
					|a| format!("{a:?}"),
				);
				ui.end_row();
			});
		}
		LayerKind::Shape { items } => shapes(ui, items, frame, selected_shape, actions),
		LayerKind::Adjustment => {
			ui.weak(
				"Effects on this layer process the composite of every layer below it, limited by its masks and opacity.",
			);
		}
	}
}

fn shapes(ui: &mut egui::Ui, items: &mut Vec<ShapeItem>, frame: i32, selected: Option<Id>, actions: &mut Actions) {
	let mut swap = None;
	let mut remove = None;
	for (i, item) in items.iter().enumerate() {
		ui.horizontal(|ui| {
			if ui
				.selectable_label(
					selected == Some(item.id),
					format!("{}  {}", item.geometry.label(), item.name),
				)
				.clicked()
			{
				actions.select_shape = Some(if selected == Some(item.id) { None } else { Some(item.id) });
			}
			ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
				if ui.small_button("🗙").clicked() {
					remove = Some(i);
				}
				if ui
					.add_enabled(i + 1 < items.len(), egui::Button::new("⏷").small())
					.clicked()
				{
					swap = Some((i, i + 1));
				}
				if ui.add_enabled(i > 0, egui::Button::new("⏶").small()).clicked() {
					swap = Some((i, i - 1));
				}
			});
		});
	}
	if let Some((a, b)) = swap {
		items.swap(a, b);
	}
	if let Some(i) = remove {
		items.remove(i);
	}

	ui.horizontal_wrapped(|ui| {
		ui.label("Add:");
		if ui.small_button("Rectangle").clicked() {
			actions.add_shape = Some(Geometry::Rect {
				size: Anim::new([300.0, 200.0]),
				roundness: Anim::new(0.0),
			});
		}
		if ui.small_button("Ellipse").clicked() {
			actions.add_shape = Some(Geometry::Ellipse {
				size: Anim::new([240.0, 240.0]),
			});
		}
		if ui.small_button("Star").clicked() {
			actions.add_shape = Some(Geometry::Star {
				points: 5,
				outer: Anim::new(140.0),
				inner: Anim::new(60.0),
			});
		}
		if ui.small_button("Polygon").clicked() {
			actions.add_shape = Some(Geometry::Star {
				points: 6,
				outer: Anim::new(140.0),
				inner: Anim::new(140.0),
			});
		}
		if ui.small_button("Path (pen)").clicked() {
			actions.add_shape = Some(Geometry::Path(BezPath::default()));
		}
	});

	let Some(item) = items.iter_mut().find(|i| Some(i.id) == selected) else {
		return;
	};
	ui.separator();
	ui.add(egui::TextEdit::singleline(&mut item.name));
	egui::Grid::new(("shape", item.id)).num_columns(3).show(ui, |ui| {
		match &mut item.geometry {
			Geometry::Rect { size, roundness } => {
				anim_row(ui, "Size", size, frame, |ui, v| drag_xy(ui, v, 1.0, ""));
				anim_row(ui, "Roundness", roundness, frame, |ui, v| drag_f32(ui, v, 0.5, " px"));
			}
			Geometry::Ellipse { size } => {
				anim_row(ui, "Size", size, frame, |ui, v| drag_xy(ui, v, 1.0, ""));
			}
			Geometry::Star { points, outer, inner } => {
				ui.label("");
				ui.label("Points");
				ui.add(egui::DragValue::new(points).range(3..=64));
				ui.end_row();
				anim_row(ui, "Outer Radius", outer, frame, |ui, v| drag_f32(ui, v, 1.0, " px"));
				anim_row(ui, "Inner Radius", inner, frame, |ui, v| drag_f32(ui, v, 1.0, " px"));
			}
			Geometry::Path(path) => {
				ui.label("");
				ui.label("Path");
				ui.horizontal(|ui| {
					ui.checkbox(&mut path.closed, "Closed");
					ui.weak(format!("{} points", path.vertices.len()));
				});
				ui.end_row();
			}
		}
		anim_row(ui, "Position", &mut item.position, frame, |ui, v| {
			drag_xy(ui, v, 1.0, "")
		});
		anim_row(ui, "Rotation", &mut item.rotation, frame, |ui, v| {
			drag_f32(ui, v, 1.0, "°")
		});
		ui.label("");
		ui.checkbox(&mut item.fill_on, "Fill");
		ui.label("");
		ui.end_row();
		if item.fill_on {
			anim_row(ui, "Fill Color", &mut item.fill, frame, widgets::color);
		}
		ui.label("");
		ui.checkbox(&mut item.stroke_on, "Stroke");
		ui.label("");
		ui.end_row();
		if item.stroke_on {
			anim_row(ui, "Stroke Color", &mut item.stroke, frame, widgets::color);
			anim_row(ui, "Stroke Width", &mut item.stroke_width, frame, |ui, v| {
				drag_f32(ui, v, 0.2, " px")
			});
		}
	});
}

fn transform(ui: &mut egui::Ui, layer: &mut Layer, frame: i32, comp: &crate::model::Comp) {
	let (w, h) = layer.size(comp);
	let t = &mut layer.transform;
	egui::Grid::new("transform").num_columns(3).show(ui, |ui| {
		anim_row(ui, "Anchor Point", &mut t.anchor, frame, |ui, v| {
			drag_xy(ui, v, 1.0, "")
		});
		anim_row(ui, "Position", &mut t.position, frame, |ui, v| drag_xy(ui, v, 1.0, ""));
		anim_row(ui, "Scale", &mut t.scale, frame, |ui, v| drag_xy(ui, v, 0.5, "%"));
		anim_row(ui, "Rotation", &mut t.rotation, frame, |ui, v| {
			drag_f32(ui, v, 1.0, "°")
		});
		anim_row(ui, "Opacity", &mut t.opacity, frame, |ui, v| {
			ui.add(egui::Slider::new(v, 0.0..=100.0).suffix("%")).changed()
		});
	});
	if ui.small_button("Reset Transform").clicked() {
		*t = crate::model::Transform::centered(w, h, comp);
	}
}

fn masks(ui: &mut egui::Ui, layer: &mut Layer, frame: i32, selected: Option<Id>, actions: &mut Actions) {
	let mut remove = None;
	for (i, mask) in layer.masks.iter_mut().enumerate() {
		ui.horizontal(|ui| {
			let is_selected = selected == Some(mask.id);
			if ui.selectable_label(is_selected, &mask.name).clicked() {
				actions.select_mask = Some(if is_selected { None } else { Some(mask.id) });
			}
			widgets::combo(ui, ("mode", mask.id), &mut mask.mode, &MaskMode::ALL, |m| {
				format!("{m:?}")
			});
			ui.checkbox(&mut mask.inverted, "Inv");
			ui.checkbox(&mut mask.path.closed, "Closed");
			if ui.small_button("🗙").clicked() {
				remove = Some(i);
			}
		});
		if selected == Some(mask.id) {
			ui.add(egui::TextEdit::singleline(&mut mask.name));
		}
		egui::Grid::new(("mask", mask.id)).num_columns(3).show(ui, |ui| {
			anim_row(ui, "Opacity", &mut mask.opacity, frame, |ui, v| {
				ui.add(egui::Slider::new(v, 0.0..=100.0).suffix("%")).changed()
			});
			anim_row(ui, "Feather", &mut mask.feather, frame, |ui, v| {
				ui.add(egui::DragValue::new(v).range(0.0..=500.0).speed(0.5).suffix(" px"))
					.changed()
			});
		});
		ui.add_space(4.0);
	}
	if let Some(i) = remove {
		layer.masks.remove(i);
	}
	ui.weak("Draw masks with the pen or shape tools on this layer.");
}

/// What a parameter row needs besides the effect itself.
struct ParamCx<'a> {
	info: &'a PluginInfo,
	hidden: &'a [bool],
	disabled: &'a [bool],
	frame: i32,
	/// The input layer's size, for point defaults.
	size: (u32, u32),
	layers: &'a [(Id, String)],
	self_id: Id,
	masks: &'a [(Id, String)],
	pressed: Option<usize>,
}

impl ParamCx<'_> {
	fn kind(&self, i: usize) -> ParamKind {
		self.info.params[i].kind
	}

	fn hidden(&self, i: usize) -> bool {
		self.hidden.get(i).copied().unwrap_or(false)
	}

	fn disabled(&self, i: usize) -> bool {
		self.disabled.get(i).copied().unwrap_or(false)
	}
}

/// Parameters from `*i` up to the end of the enclosing group, nesting groups
/// as collapsing headers.
fn block(ui: &mut egui::Ui, cx: &mut ParamCx, effect: &mut Effect, i: &mut usize) {
	let n = cx.info.params.len();
	loop {
		egui::Grid::new(("params", effect.id, *i))
			.num_columns(3)
			.spacing([6.0, 3.0])
			.show(ui, |ui| {
				while *i < n && !matches!(cx.kind(*i), ParamKind::GroupStart | ParamKind::GroupEnd) {
					if !cx.hidden(*i) {
						row(ui, cx, effect, *i);
					}
					*i += 1;
				}
			});
		if *i >= n {
			return;
		}
		let start = *i;
		*i += 1;
		if cx.kind(start) == ParamKind::GroupEnd {
			return;
		}
		let name = &cx.info.params[start].name;
		if cx.hidden(start) {
			skip_group(cx, i);
			continue;
		}
		let shown = egui::CollapsingHeader::new(name.clone())
			.id_salt(("group", effect.id, start))
			.show(ui, |ui| block(ui, cx, effect, i));
		if shown.body_returned.is_none() {
			skip_group(cx, i);
		}
	}
}

fn skip_group(cx: &ParamCx, i: &mut usize) {
	let mut depth = 1;
	while *i < cx.info.params.len() && depth > 0 {
		match cx.kind(*i) {
			ParamKind::GroupStart => depth += 1,
			ParamKind::GroupEnd => depth -= 1,
			_ => {}
		}
		*i += 1;
	}
}

fn row(ui: &mut egui::Ui, cx: &mut ParamCx, effect: &mut Effect, index: usize) {
	let desc = &cx.info.params[index];
	let name = if desc.name.is_empty() {
		format!("Param {index}")
	} else {
		desc.name.clone()
	};
	let enabled = !cx.disabled(index);

	let Some(default) = cx.info.default_value(index, cx.size) else {
		ui.label("");
		match desc.kind {
			ParamKind::Layer => {
				ui.label(&name);
				ui.add_enabled_ui(enabled, |ui| layer_picker(ui, cx, effect, index));
			}
			ParamKind::Button => {
				ui.label("");
				if ui.add_enabled(enabled, egui::Button::new(&name)).clicked() {
					cx.pressed = Some(index);
				}
			}
			ParamKind::NoData => {
				ui.weak(&name);
				ui.label("");
			}
			kind => {
				ui.label(&name);
				ui.weak(format!("{kind:?} (not editable)"));
			}
		}
		ui.end_row();
		return;
	};

	let overridden = effect.params.contains_key(&index);
	let anim = effect.params.get(&index);
	let animated = anim.is_some_and(|a| a.is_animated());
	let key_here = anim.is_some_and(|a| a.has_key_at(cx.frame));
	ui.horizontal(|ui| {
		ui.spacing_mut().item_spacing.x = 2.0;
		if let Some(action) = widgets::stopwatch(ui, animated, key_here) {
			let anim = effect.params.entry(index).or_insert_with(|| Anim::new(default.clone()));
			match action {
				widgets::KeyAction::Stopwatch => anim.toggle_animation(cx.frame),
				widgets::KeyAction::Key => anim.toggle_key(cx.frame),
			}
		}
	});
	let label = ui.label(if overridden {
		RichText::new(&name)
	} else {
		RichText::new(&name).weak()
	});
	label.context_menu(|ui| {
		if ui.button("Reset to default").clicked() {
			effect.params.remove(&index);
		}
	});
	let mut value = effect.params.get(&index).map_or(default.clone(), |a| a.at(cx.frame));
	let changed = ui
		.add_enabled_ui(enabled, |ui| value_editor(ui, &mut value, desc, cx.masks))
		.inner;
	if changed {
		effect
			.params
			.entry(index)
			.or_insert_with(|| Anim::new(default))
			.set(cx.frame, value);
	}
	ui.end_row();
}

fn layer_picker(ui: &mut egui::Ui, cx: &ParamCx, effect: &mut Effect, index: usize) {
	let current = effect.layers.get(&index).copied();
	let label = |id: Option<Id>| match id {
		None => "None".to_string(),
		Some(id) if id == cx.self_id => "This layer (source)".to_string(),
		Some(id) => cx
			.layers
			.iter()
			.find(|(l, _)| *l == id)
			.map_or("(missing)".into(), |(_, n)| n.clone()),
	};
	egui::ComboBox::from_id_salt(("layer-param", effect.id, index))
		.selected_text(label(current))
		.width(170.0)
		.show_ui(ui, |ui| {
			if ui.selectable_label(current.is_none(), "None").clicked() {
				effect.layers.remove(&index);
			}
			for (id, _) in cx.layers {
				if ui.selectable_label(current == Some(*id), label(Some(*id))).clicked() {
					effect.layers.insert(index, *id);
				}
			}
		});
}

fn value_editor(ui: &mut egui::Ui, value: &mut PValue, desc: &ParamDesc, masks: &[(Id, String)]) -> bool {
	ui.spacing_mut().slider_width = 130.0;
	match value {
		PValue::Float(v) => match desc.range {
			Some((lo, hi)) => ui
				.add(egui::Slider::new(v, lo..=hi).clamping(SliderClamping::Never))
				.changed(),
			None => ui.add(egui::DragValue::new(v).speed(0.1)).changed(),
		},
		PValue::Fixed(v) => match desc.range {
			Some((lo, hi)) => ui
				.add(egui::Slider::new(v, lo as f32..=hi as f32).clamping(SliderClamping::Never))
				.changed(),
			None => ui.add(egui::DragValue::new(v).speed(0.01)).changed(),
		},
		PValue::Slider(v) => match desc.range {
			Some((lo, hi)) => ui
				.add(egui::Slider::new(v, lo as i32..=hi as i32).clamping(SliderClamping::Never))
				.changed(),
			None => ui.add(egui::DragValue::new(v)).changed(),
		},
		PValue::Checkbox(b) => ui.checkbox(b, "").changed(),
		PValue::Popup(v) => match &desc.choices {
			Some(choices) => {
				let mut changed = false;
				let current = choices
					.get((*v - 1).max(0) as usize)
					.cloned()
					.unwrap_or_else(|| v.to_string());
				egui::ComboBox::from_id_salt(("popup", desc.index, ui.id()))
					.selected_text(current)
					.width(170.0)
					.show_ui(ui, |ui| {
						for (i, choice) in choices.iter().enumerate() {
							let choice_value = i as i32 + 1;
							// "(-" marks a separator in AE popup strings.
							if choice.starts_with("(-") {
								ui.separator();
							} else if ui.selectable_label(*v == choice_value, choice).clicked() {
								*v = choice_value;
								changed = true;
							}
						}
					});
				changed
			}
			None => ui.add(egui::DragValue::new(v).range(1..=i32::MAX)).changed(),
		},
		PValue::Angle(v) => ui.add(egui::DragValue::new(v).speed(1.0).suffix("°")).changed(),
		PValue::Point(p) => ui.horizontal(|ui| drag_xy(ui, p, 1.0, "")).inner,
		PValue::Point3D(p) => {
			ui.horizontal(|ui| {
				let mut changed = false;
				for c in p.iter_mut() {
					changed |= ui.add(egui::DragValue::new(c).speed(1.0).max_decimals(1)).changed();
				}
				changed
			})
			.inner
		}
		PValue::Color(c) => ui.color_edit_button_srgba_unmultiplied(c).changed(),
		PValue::Path(id) => {
			let mut changed = false;
			let name = |id: u32| {
				if id == 0 {
					"None".to_string()
				} else {
					masks
						.iter()
						.find(|(m, _)| *m as u32 == id)
						.map_or(format!("(mask {id})"), |(_, n)| n.clone())
				}
			};
			egui::ComboBox::from_id_salt(("path", desc.index, ui.id()))
				.selected_text(name(*id))
				.show_ui(ui, |ui| {
					if ui.selectable_label(*id == 0, "None").clicked() {
						*id = 0;
						changed = true;
					}
					for (m, n) in masks {
						if ui.selectable_label(*id == *m as u32, n).clicked() {
							*id = *m as u32;
							changed = true;
						}
					}
				});
			changed
		}
	}
}
