//! Small UI building blocks shared by the panels.

use eframe::egui::{self, Color32, RichText, Ui};

use crate::model::{Anim, Lerp, Rgba};

pub const KEY_COLOR: Color32 = Color32::from_rgb(240, 190, 60);
pub const ACCENT: Color32 = Color32::from_rgb(90, 160, 255);

/// One keyframable property as a three-cell grid row: stopwatch and keyframe
/// toggle, label, editor. Edits go to [`Anim::set`] at `frame`. Returns
/// whether anything changed.
pub fn anim_row<T: Lerp>(
	ui: &mut Ui,
	label: &str,
	anim: &mut Anim<T>,
	frame: i32,
	edit: impl FnOnce(&mut Ui, &mut T) -> bool,
) -> bool {
	let mut changed = false;
	ui.horizontal(|ui| {
		ui.spacing_mut().item_spacing.x = 2.0;
		changed |= stopwatch(ui, anim.is_animated(), anim.has_key_at(frame)).is_some_and(|action| {
			match action {
				KeyAction::Stopwatch => anim.toggle_animation(frame),
				KeyAction::Key => anim.toggle_key(frame),
			}
			true
		});
	});
	ui.label(label);
	let mut value = anim.at(frame);
	if edit(ui, &mut value) {
		anim.set(frame, value);
		changed = true;
	}
	ui.end_row();
	changed
}

pub enum KeyAction {
	Stopwatch,
	Key,
}

/// The stopwatch (start/stop animating) and, while animated, the keyframe
/// diamond for the current frame.
pub fn stopwatch(ui: &mut Ui, animated: bool, key_here: bool) -> Option<KeyAction> {
	let mut action = None;
	let watch = RichText::new("⏱").color(if animated { KEY_COLOR } else { Color32::GRAY });
	if ui
		.add(egui::Button::new(watch).frame(false))
		.on_hover_text(if animated {
			"Stop animating (keeps the current value)"
		} else {
			"Animate: add a keyframe here"
		})
		.clicked()
	{
		action = Some(KeyAction::Stopwatch);
	}
	if animated {
		let diamond = RichText::new(if key_here { "◆" } else { "◇" }).color(KEY_COLOR);
		if ui
			.add(egui::Button::new(diamond).frame(false))
			.on_hover_text(if key_here {
				"Remove the keyframe at this frame"
			} else {
				"Add a keyframe at this frame"
			})
			.clicked()
		{
			action = Some(KeyAction::Key);
		}
	} else {
		ui.add_space(14.0);
	}
	action
}

pub fn drag_f32(ui: &mut Ui, v: &mut f32, speed: f64, suffix: &str) -> bool {
	ui.add(egui::DragValue::new(v).speed(speed).max_decimals(2).suffix(suffix))
		.changed()
}

pub fn drag_xy(ui: &mut Ui, v: &mut [f32; 2], speed: f64, suffix: &str) -> bool {
	let x = ui
		.add(
			egui::DragValue::new(&mut v[0])
				.speed(speed)
				.max_decimals(1)
				.suffix(suffix),
		)
		.changed();
	let y = ui
		.add(
			egui::DragValue::new(&mut v[1])
				.speed(speed)
				.max_decimals(1)
				.suffix(suffix),
		)
		.changed();
	x || y
}

/// A color button. Writes back only on an edit: the picker's round trip
/// through HSVA perturbs the low bits of an untouched color.
pub fn color(ui: &mut Ui, v: &mut Rgba) -> bool {
	let mut edited = *v;
	let changed = ui.color_edit_button_rgba_unmultiplied(&mut edited).changed();
	if changed {
		*v = edited;
	}
	changed
}

/// `0:00:01:15`-style timecode.
pub fn timecode(frame: i32, fps: u32) -> String {
	let fps = fps.max(1) as i32;
	let sign = if frame < 0 { "-" } else { "" };
	let f = frame.abs();
	let secs = f / fps;
	format!(
		"{sign}{}:{:02}:{:02}:{:02}",
		secs / 3600,
		secs / 60 % 60,
		secs % 60,
		f % fps
	)
}

/// A combo box over `options`, labelled by `label`.
pub fn combo<T: PartialEq + Copy>(
	ui: &mut Ui,
	id: impl std::hash::Hash + std::fmt::Debug,
	value: &mut T,
	options: &[T],
	label: impl Fn(T) -> String,
) -> bool {
	let mut changed = false;
	egui::ComboBox::from_id_salt(id)
		.selected_text(label(*value))
		.show_ui(ui, |ui| {
			for &option in options {
				if ui.selectable_label(*value == option, label(option)).clicked() && *value != option {
					*value = option;
					changed = true;
				}
			}
		});
	changed
}
