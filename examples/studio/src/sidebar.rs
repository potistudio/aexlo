//! The left panel: tools, composition settings, new layers and the effect
//! browser.

use eframe::egui::{self, RichText};

use crate::app::{Studio, Tool};
use crate::model::{Anim, LayerKind};
use crate::{plugins, widgets};

pub fn show(ui: &mut egui::Ui, s: &mut Studio) {
	egui::ScrollArea::vertical().show(ui, |ui| {
		ui.heading("Tools");
		ui.horizontal_wrapped(|ui| {
			for (tool, icon, hint) in Tool::ALL {
				let label = RichText::new(icon).size(18.0);
				if ui.selectable_label(s.tool == tool, label).on_hover_text(hint).clicked() {
					s.tool = tool;
					s.pen = None;
				}
			}
		});
		if s.tool != Tool::Select {
			ui.checkbox(&mut s.view.draw_masks, "Draw masks on shape layers")
				.on_hover_text("Shape tools and the pen draw masks on any selected layer other than a shape layer; this extends that to shape layers.");
		}
		match s.tool {
			Tool::Pen => ui.weak("Click to add corners, drag for curves, click the first point to close. Enter ends an open path."),
			Tool::Rect | Tool::Ellipse | Tool::Star => {
				ui.weak("Drag in the viewer. Draws a mask on the selected layer, or a shape.")
			}
			Tool::Select => ui.weak("Drag layers, handles, mask points and effect points. Alt-drag or middle-drag pans; scroll zooms."),
		};

		ui.separator();
		egui::CollapsingHeader::new(RichText::new("Composition").heading())
			.default_open(true)
			.show(ui, |ui| comp_settings(ui, s));

		ui.separator();
		ui.heading("New Layer");
		ui.horizontal_wrapped(|ui| new_layer_buttons(ui, s));

		ui.separator();
		ui.heading("Effects");
		ui.add(egui::TextEdit::singleline(&mut s.plugin_filter).hint_text("Search…"));
		let target = s.sel.layer.and_then(|l| s.project.layer(l)).map(|l| l.name.clone());
		ui.weak(match &target {
			Some(name) => format!("Double-click to apply to “{name}”"),
			None => "Select a layer to apply effects".into(),
		});
		let filter = s.plugin_filter.to_lowercase();
		for name in s.fixtures.clone() {
			if !filter.is_empty() && !name.to_lowercase().contains(&filter) {
				continue;
			}
			let response = ui.add_enabled(
				target.is_some(),
				egui::Button::new(format!("fx  {name}")).frame(false),
			);
			if response.double_clicked() || response.clicked() && ui.input(|i| i.modifiers.command) {
				s.add_effect(&name);
			}
		}
		if ui.add_enabled(target.is_some(), egui::Button::new("Other plugin…")).clicked() {
			pick_plugin(s);
		}
	});
}

fn comp_settings(ui: &mut egui::Ui, s: &mut Studio) {
	let comp = &mut s.project.comp;
	egui::Grid::new("comp").num_columns(2).show(ui, |ui| {
		ui.label("Size");
		ui.horizontal(|ui| {
			ui.add(egui::DragValue::new(&mut comp.width).range(16..=8192));
			ui.label("×");
			ui.add(egui::DragValue::new(&mut comp.height).range(16..=8192));
		});
		ui.end_row();
		ui.label("Frame rate");
		ui.add(egui::DragValue::new(&mut comp.fps).range(1..=120).suffix(" fps"));
		ui.end_row();
		ui.label("Duration");
		ui.horizontal(|ui| {
			ui.add(egui::DragValue::new(&mut comp.duration).range(1..=100_000).suffix(" f"));
			ui.weak(format!("{:.2}s", comp.duration as f32 / comp.fps.max(1) as f32));
		});
		ui.end_row();
		ui.label("Background");
		widgets::color(ui, &mut comp.background);
		ui.end_row();
	});
}

pub fn new_layer_buttons(ui: &mut egui::Ui, s: &mut Studio) {
	if ui.button("Solid").clicked() {
		s.new_solid();
	}
	if ui.button("Text").clicked() {
		s.new_text_layer();
	}
	if ui.button("Shape").clicked() {
		let n = s.project.layers.len() + 1;
		s.add_layer(&format!("Shape Layer {n}"), LayerKind::Shape { items: Vec::new() });
		s.tool = Tool::Rect;
	}
	if ui.button("Image…").clicked() {
		s.import_image();
	}
	if ui
		.button("Adjustment")
		.on_hover_text("Its effects process everything below it")
		.clicked()
	{
		s.add_layer("Adjustment Layer", LayerKind::Adjustment);
	}
	if ui
		.button("Null (transparent solid)")
		.on_hover_text("For generators like particle systems")
		.clicked()
	{
		let (width, height) = (s.project.comp.width, s.project.comp.height);
		s.add_layer(
			"Null",
			LayerKind::Solid {
				color: Anim::new([0.0; 4]),
				width,
				height,
			},
		);
	}
}

pub fn pick_plugin(s: &mut Studio) {
	let extensions: &[&str] = if cfg!(target_os = "windows") {
		&["aex", "dll"]
	} else {
		&["plugin"]
	};
	if let Some(path) = rfd::FileDialog::new()
		.add_filter("After Effects plugin", extensions)
		.pick_file()
	{
		s.add_effect(&plugins::reference_for(&path));
	}
}
