//! The project being edited: one composition of layers, each with masks, a
//! stack of plugin effects, and keyframable properties.
//!
//! Pure data: the UI edits it, the render engine reads snapshots of it, and it
//! round-trips through JSON as the project file.

use std::collections::BTreeMap;

use aexlo::ParamValue;
use serde::{Deserialize, Serialize};

/// Identifies layers, masks, shape items and effects; unique per project.
pub type Id = u64;

/// Straight (unpremultiplied) RGBA, each channel in `0..=1`.
pub type Rgba = [f32; 4];

// ---- Keyframes ---------------------------------------------------------------

/// How a keyframe moves on to the next one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Interp {
	#[default]
	Linear,
	/// Ease in and out (smoothstep).
	Ease,
	/// Keep this value until the next keyframe.
	Hold,
}

impl Interp {
	pub const ALL: [Interp; 3] = [Interp::Linear, Interp::Ease, Interp::Hold];

	pub fn label(self) -> &'static str {
		match self {
			Interp::Linear => "Linear",
			Interp::Ease => "Easy Ease",
			Interp::Hold => "Hold",
		}
	}
}

/// Values that can be interpolated between keyframes. Discrete values hold
/// until the next keyframe.
pub trait Lerp: Clone + PartialEq {
	fn lerp(&self, other: &Self, t: f64) -> Self;
}

impl Lerp for f64 {
	fn lerp(&self, other: &Self, t: f64) -> Self {
		self + (other - self) * t
	}
}

impl Lerp for f32 {
	fn lerp(&self, other: &Self, t: f64) -> Self {
		self + (other - self) * t as f32
	}
}

impl<const N: usize> Lerp for [f32; N] {
	fn lerp(&self, other: &Self, t: f64) -> Self {
		std::array::from_fn(|i| self[i].lerp(&other[i], t))
	}
}

impl<const N: usize> Lerp for [f64; N] {
	fn lerp(&self, other: &Self, t: f64) -> Self {
		std::array::from_fn(|i| self[i].lerp(&other[i], t))
	}
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Key<T> {
	pub frame: i32,
	pub value: T,
	#[serde(default)]
	pub interp: Interp,
}

/// A property that is either constant (`value`) or keyframed (`keys`, sorted
/// by frame, non-empty). Like After Effects' stopwatch: once animated, every
/// edit sets a keyframe at the current frame.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Anim<T> {
	pub value: T,
	#[serde(default = "Vec::new", skip_serializing_if = "Vec::is_empty")]
	pub keys: Vec<Key<T>>,
}

impl<T: Lerp> Anim<T> {
	pub fn new(value: T) -> Self {
		Self {
			value,
			keys: Vec::new(),
		}
	}

	pub fn is_animated(&self) -> bool {
		!self.keys.is_empty()
	}

	/// The value at `frame`.
	pub fn at(&self, frame: i32) -> T {
		let (Some(first), Some(last)) = (self.keys.first(), self.keys.last()) else {
			return self.value.clone();
		};
		if frame <= first.frame {
			return first.value.clone();
		}
		if frame >= last.frame {
			return last.value.clone();
		}
		let i = self.keys.partition_point(|k| k.frame <= frame) - 1;
		let (a, b) = (&self.keys[i], &self.keys[i + 1]);
		let t = (frame - a.frame) as f64 / (b.frame - a.frame) as f64;
		match a.interp {
			Interp::Hold => a.value.clone(),
			Interp::Linear => a.value.lerp(&b.value, t),
			Interp::Ease => a.value.lerp(&b.value, t * t * (3.0 - 2.0 * t)),
		}
	}

	/// Edit the value at `frame`: a keyframe there when animated, the constant
	/// value otherwise.
	pub fn set(&mut self, frame: i32, value: T) {
		if !self.is_animated() {
			self.value = value;
			return;
		}
		match self.keys.binary_search_by_key(&frame, |k| k.frame) {
			Ok(i) => self.keys[i].value = value,
			Err(i) => self.keys.insert(
				i,
				Key {
					frame,
					value,
					interp: self.keys.get(i.saturating_sub(1)).map(|k| k.interp).unwrap_or_default(),
				},
			),
		}
	}

	/// The stopwatch: start animating with a keyframe at `frame`, or stop and
	/// keep the value at `frame`.
	pub fn toggle_animation(&mut self, frame: i32) {
		if self.is_animated() {
			self.value = self.at(frame);
			self.keys.clear();
		} else {
			self.keys.push(Key {
				frame,
				value: self.value.clone(),
				interp: Interp::Linear,
			});
		}
	}

	pub fn has_key_at(&self, frame: i32) -> bool {
		self.keys.binary_search_by_key(&frame, |k| k.frame).is_ok()
	}

	/// Add a keyframe holding the current value at `frame`, or remove the one
	/// there.
	pub fn toggle_key(&mut self, frame: i32) {
		match self.keys.binary_search_by_key(&frame, |k| k.frame) {
			Ok(i) => {
				let value = self.keys.remove(i).value;
				if self.keys.is_empty() {
					self.value = value;
				}
			}
			Err(_) => {
				let value = self.at(frame);
				if !self.is_animated() {
					self.keys.push(Key {
						frame,
						value,
						interp: Interp::Linear,
					});
				} else {
					self.set(frame, value);
				}
			}
		}
	}
}

/// Type-erased keyframe access, so the timeline can list and edit the keys of
/// properties of any value type.
pub trait Track {
	fn key_frames(&self) -> Vec<(i32, Interp)>;
	/// Move the keyframe at `from` to `to`, replacing any keyframe there.
	fn move_key(&mut self, from: i32, to: i32);
	fn remove_key(&mut self, frame: i32);
	fn set_interp(&mut self, frame: i32, interp: Interp);
}

impl<T: Lerp> Track for Anim<T> {
	fn key_frames(&self) -> Vec<(i32, Interp)> {
		self.keys.iter().map(|k| (k.frame, k.interp)).collect()
	}

	fn move_key(&mut self, from: i32, to: i32) {
		if from == to {
			return;
		}
		let Ok(i) = self.keys.binary_search_by_key(&from, |k| k.frame) else {
			return;
		};
		let mut key = self.keys.remove(i);
		key.frame = to;
		match self.keys.binary_search_by_key(&to, |k| k.frame) {
			Ok(j) => self.keys[j] = key,
			Err(j) => self.keys.insert(j, key),
		}
	}

	fn remove_key(&mut self, frame: i32) {
		if self.has_key_at(frame) {
			self.toggle_key(frame);
		}
	}

	fn set_interp(&mut self, frame: i32, interp: Interp) {
		if let Ok(i) = self.keys.binary_search_by_key(&frame, |k| k.frame) {
			self.keys[i].interp = interp;
		}
	}
}

// ---- Plugin parameter values ---------------------------------------------------

/// A serializable mirror of [`aexlo::ParamValue`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum PValue {
	Float(f64),
	Fixed(f32),
	Slider(i32),
	Checkbox(bool),
	Popup(i32),
	Angle(f32),
	Point([f32; 2]),
	Point3D([f64; 3]),
	Color([u8; 4]),
	Path(u32),
}

impl From<ParamValue> for PValue {
	fn from(value: ParamValue) -> Self {
		match value {
			ParamValue::Float(v) => PValue::Float(v),
			ParamValue::Fixed(v) => PValue::Fixed(v),
			ParamValue::Slider(v) => PValue::Slider(v),
			ParamValue::Checkbox(v) => PValue::Checkbox(v),
			ParamValue::Popup(v) => PValue::Popup(v),
			ParamValue::Angle(v) => PValue::Angle(v),
			ParamValue::Point { x, y } => PValue::Point([x, y]),
			ParamValue::Point3D { x, y, z } => PValue::Point3D([x, y, z]),
			ParamValue::Color {
				red,
				green,
				blue,
				alpha,
			} => PValue::Color([red, green, blue, alpha]),
			ParamValue::Path(id) => PValue::Path(id),
		}
	}
}

impl From<&PValue> for ParamValue {
	fn from(value: &PValue) -> Self {
		match *value {
			PValue::Float(v) => ParamValue::Float(v),
			PValue::Fixed(v) => ParamValue::Fixed(v),
			PValue::Slider(v) => ParamValue::Slider(v),
			PValue::Checkbox(v) => ParamValue::Checkbox(v),
			PValue::Popup(v) => ParamValue::Popup(v),
			PValue::Angle(v) => ParamValue::Angle(v),
			PValue::Point([x, y]) => ParamValue::Point { x, y },
			PValue::Point3D([x, y, z]) => ParamValue::Point3D { x, y, z },
			PValue::Color([red, green, blue, alpha]) => ParamValue::Color {
				red,
				green,
				blue,
				alpha,
			},
			PValue::Path(id) => ParamValue::Path(id),
		}
	}
}

impl Lerp for PValue {
	fn lerp(&self, other: &Self, t: f64) -> Self {
		use PValue::*;
		match (self, other) {
			(Float(a), Float(b)) => Float(a.lerp(b, t)),
			(Fixed(a), Fixed(b)) => Fixed(a.lerp(b, t)),
			(Slider(a), Slider(b)) => Slider((*a as f64).lerp(&(*b as f64), t).round() as i32),
			(Angle(a), Angle(b)) => Angle(a.lerp(b, t)),
			(Point(a), Point(b)) => Point(a.lerp(b, t)),
			(Point3D(a), Point3D(b)) => Point3D(a.lerp(b, t)),
			(Color(a), Color(b)) => Color(std::array::from_fn(|i| {
				(a[i] as f64).lerp(&(b[i] as f64), t).round() as u8
			})),
			// Checkboxes, popups and paths (and mismatched kinds) step.
			_ => self.clone(),
		}
	}
}

// ---- Geometry -----------------------------------------------------------------

/// A Bezier vertex; tangents are relative to `p`, as in After Effects.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Vertex {
	pub p: [f32; 2],
	#[serde(default)]
	pub tin: [f32; 2],
	#[serde(default)]
	pub tout: [f32; 2],
}

impl Vertex {
	pub fn corner(x: f32, y: f32) -> Self {
		Self {
			p: [x, y],
			..Default::default()
		}
	}
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BezPath {
	pub vertices: Vec<Vertex>,
	pub closed: bool,
}

/// Cubic-Bezier handle length for a quarter circle of radius 1.
pub const KAPPA: f32 = 0.552_284_8;

impl BezPath {
	pub fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> Self {
		Self {
			vertices: vec![
				Vertex::corner(x0, y0),
				Vertex::corner(x1, y0),
				Vertex::corner(x1, y1),
				Vertex::corner(x0, y1),
			],
			closed: true,
		}
	}

	pub fn ellipse(x0: f32, y0: f32, x1: f32, y1: f32) -> Self {
		let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
		let (rx, ry) = ((x1 - x0).abs() / 2.0, (y1 - y0).abs() / 2.0);
		let (kx, ky) = (rx * KAPPA, ry * KAPPA);
		Self {
			vertices: vec![
				Vertex {
					p: [cx, cy - ry],
					tin: [-kx, 0.0],
					tout: [kx, 0.0],
				},
				Vertex {
					p: [cx + rx, cy],
					tin: [0.0, -ky],
					tout: [0.0, ky],
				},
				Vertex {
					p: [cx, cy + ry],
					tin: [kx, 0.0],
					tout: [-kx, 0.0],
				},
				Vertex {
					p: [cx - rx, cy],
					tin: [0.0, ky],
					tout: [0.0, -ky],
				},
			],
			closed: true,
		}
	}

	/// The segments as cubic control points `[p0, c0, c1, p1]`.
	pub fn segments(&self) -> Vec<[[f32; 2]; 4]> {
		let n = self.vertices.len();
		let count = match n {
			0 | 1 => 0,
			_ if self.closed => n,
			_ => n - 1,
		};
		(0..count)
			.map(|i| {
				let a = self.vertices[i];
				let b = self.vertices[(i + 1) % n];
				[
					a.p,
					[a.p[0] + a.tout[0], a.p[1] + a.tout[1]],
					[b.p[0] + b.tin[0], b.p[1] + b.tin[1]],
					b.p,
				]
			})
			.collect()
	}
}

// ---- Layers ---------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlendMode {
	#[default]
	Normal,
	Add,
	Multiply,
	Screen,
	Overlay,
	Darken,
	Lighten,
	ColorDodge,
	ColorBurn,
	HardLight,
	SoftLight,
	Difference,
	Exclusion,
	Hue,
	Saturation,
	Color,
	Luminosity,
}

impl BlendMode {
	pub const ALL: [BlendMode; 17] = [
		BlendMode::Normal,
		BlendMode::Add,
		BlendMode::Multiply,
		BlendMode::Screen,
		BlendMode::Overlay,
		BlendMode::Darken,
		BlendMode::Lighten,
		BlendMode::ColorDodge,
		BlendMode::ColorBurn,
		BlendMode::HardLight,
		BlendMode::SoftLight,
		BlendMode::Difference,
		BlendMode::Exclusion,
		BlendMode::Hue,
		BlendMode::Saturation,
		BlendMode::Color,
		BlendMode::Luminosity,
	];
}

/// How a mask combines with the masks above it (`PF_MaskMode_*`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaskMode {
	/// Doesn't cut the layer; still visible to plugins as a path.
	None,
	#[default]
	Add,
	Subtract,
	Intersect,
	Lighten,
	Darken,
	Difference,
}

impl MaskMode {
	pub const ALL: [MaskMode; 7] = [
		MaskMode::None,
		MaskMode::Add,
		MaskMode::Subtract,
		MaskMode::Intersect,
		MaskMode::Lighten,
		MaskMode::Darken,
		MaskMode::Difference,
	];

	pub fn to_aexlo(self) -> aexlo::MaskMode {
		match self {
			MaskMode::None => aexlo::MaskMode::None,
			MaskMode::Add => aexlo::MaskMode::Add,
			MaskMode::Subtract => aexlo::MaskMode::Subtract,
			MaskMode::Intersect => aexlo::MaskMode::Intersect,
			MaskMode::Lighten => aexlo::MaskMode::Lighten,
			MaskMode::Darken => aexlo::MaskMode::Darken,
			MaskMode::Difference => aexlo::MaskMode::Difference,
		}
	}
}

/// A mask on a layer, in layer pixel coordinates. Cuts the layer's alpha
/// before its effects run, and is served to plugins through the Path suites
/// (for `PF_Param_PATH` parameters).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Mask {
	pub id: Id,
	pub name: String,
	pub path: BezPath,
	pub mode: MaskMode,
	pub inverted: bool,
	/// Percent.
	pub opacity: Anim<f32>,
	/// Blur radius in pixels.
	pub feather: Anim<f32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Transform {
	/// Layer pixels.
	pub anchor: Anim<[f32; 2]>,
	/// Comp pixels.
	pub position: Anim<[f32; 2]>,
	/// Percent.
	pub scale: Anim<[f32; 2]>,
	/// Degrees, clockwise.
	pub rotation: Anim<f32>,
	/// Percent.
	pub opacity: Anim<f32>,
}

impl Transform {
	/// Identity placement of a `w` x `h` layer centred in the comp.
	pub fn centered(w: u32, h: u32, comp: &Comp) -> Self {
		Self {
			anchor: Anim::new([w as f32 / 2.0, h as f32 / 2.0]),
			position: Anim::new([comp.width as f32 / 2.0, comp.height as f32 / 2.0]),
			scale: Anim::new([100.0, 100.0]),
			rotation: Anim::new(0.0),
			opacity: Anim::new(100.0),
		}
	}

	/// Layer pixels to comp pixels at `frame`.
	pub fn matrix(&self, frame: i32) -> tiny_skia::Transform {
		let [px, py] = self.position.at(frame);
		let [ax, ay] = self.anchor.at(frame);
		let [sx, sy] = self.scale.at(frame);
		tiny_skia::Transform::from_translate(px, py)
			.pre_rotate(self.rotation.at(frame))
			.pre_scale(sx / 100.0, sy / 100.0)
			.pre_translate(-ax, -ay)
	}
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Geometry {
	Rect {
		size: Anim<[f32; 2]>,
		roundness: Anim<f32>,
	},
	Ellipse {
		size: Anim<[f32; 2]>,
	},
	/// A star; `inner == outer` makes a regular polygon.
	Star {
		points: u32,
		outer: Anim<f32>,
		inner: Anim<f32>,
	},
	/// Vertices relative to the item's position.
	Path(BezPath),
}

impl Geometry {
	pub fn label(&self) -> &'static str {
		match self {
			Geometry::Rect { .. } => "Rectangle",
			Geometry::Ellipse { .. } => "Ellipse",
			Geometry::Star { .. } => "Star",
			Geometry::Path(_) => "Path",
		}
	}
}

/// One filled and/or stroked shape on a shape layer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ShapeItem {
	pub id: Id,
	pub name: String,
	pub geometry: Geometry,
	/// Layer pixels: the centre (or a path's origin).
	pub position: Anim<[f32; 2]>,
	pub rotation: Anim<f32>,
	pub fill_on: bool,
	pub fill: Anim<Rgba>,
	pub stroke_on: bool,
	pub stroke: Anim<Rgba>,
	pub stroke_width: Anim<f32>,
}

impl ShapeItem {
	pub fn new(id: Id, geometry: Geometry, position: [f32; 2], fill: Rgba) -> Self {
		Self {
			id,
			name: format!("{} {id}", geometry.label()),
			geometry,
			position: Anim::new(position),
			rotation: Anim::new(0.0),
			fill_on: true,
			fill: Anim::new(fill),
			stroke_on: false,
			stroke: Anim::new([1.0, 1.0, 1.0, 1.0]),
			stroke_width: Anim::new(4.0),
		}
	}
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextAlign {
	#[default]
	Left,
	Center,
	Right,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Text {
	pub text: String,
	/// A TrueType/OpenType font file; `None` for the built-in font.
	pub font: Option<String>,
	pub size: Anim<f32>,
	pub color: Anim<Rgba>,
	/// Layer pixels: the first line's baseline at the alignment point.
	pub position: Anim<[f32; 2]>,
	pub align: TextAlign,
	/// Extra space between characters, in pixels.
	pub tracking: Anim<f32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum LayerKind {
	Solid {
		color: Anim<Rgba>,
		width: u32,
		height: u32,
	},
	/// A still image; `width`/`height` are cached from the file at import.
	Image {
		path: String,
		width: u32,
		height: u32,
	},
	/// Comp-sized canvas of vector shapes.
	Shape {
		items: Vec<ShapeItem>,
	},
	/// Comp-sized canvas of text.
	Text(Text),
	/// Runs its effects over the composite of the layers below it.
	Adjustment,
}

impl LayerKind {
	pub fn label(&self) -> &'static str {
		match self {
			LayerKind::Solid { .. } => "Solid",
			LayerKind::Image { .. } => "Image",
			LayerKind::Shape { .. } => "Shape",
			LayerKind::Text(_) => "Text",
			LayerKind::Adjustment => "Adjustment",
		}
	}
}

/// A plugin effect applied to a layer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Effect {
	pub id: Id,
	/// A fixture name (see [`crate::plugins`]) or a path to a plugin.
	pub plugin: String,
	pub enabled: bool,
	/// Values the user set, by parameter index; the rest stay at the plugin's
	/// defaults.
	#[serde(default)]
	pub params: BTreeMap<usize, Anim<PValue>>,
	/// Layers linked to the plugin's own layer parameters, by index.
	#[serde(default)]
	pub layers: BTreeMap<usize, Id>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Layer {
	pub id: Id,
	pub name: String,
	pub kind: LayerKind,
	pub visible: bool,
	/// Comp frame of the layer's time 0: effects see `frame - start`.
	#[serde(default)]
	pub start: i32,
	/// First frame shown.
	pub in_frame: i32,
	/// First frame no longer shown.
	pub out_frame: i32,
	pub blend: BlendMode,
	pub transform: Transform,
	#[serde(default)]
	pub masks: Vec<Mask>,
	#[serde(default)]
	pub effects: Vec<Effect>,
}

impl Layer {
	/// Slide the layer in time: its span, time origin and keyframes.
	pub fn shift(&mut self, by: i32) {
		self.start += by;
		self.in_frame += by;
		self.out_frame += by;
		for (_, _, track) in self.tracks_mut(&|_, _| String::new()) {
			let mut frames: Vec<i32> = track.key_frames().into_iter().map(|k| k.0).collect();
			if by > 0 {
				frames.reverse();
			}
			for f in frames {
				track.move_key(f, f + by);
			}
		}
	}

	/// Layer pixels to comp pixels at `frame`. Adjustment layers apply in
	/// place over the comp, so their transform doesn't move them.
	pub fn matrix(&self, frame: i32) -> tiny_skia::Transform {
		match self.kind {
			LayerKind::Adjustment => tiny_skia::Transform::identity(),
			_ => self.transform.matrix(frame),
		}
	}

	pub fn is_active(&self, frame: i32) -> bool {
		self.visible && (self.in_frame..self.out_frame).contains(&frame)
	}

	/// The layer's own pixel size.
	pub fn size(&self, comp: &Comp) -> (u32, u32) {
		match &self.kind {
			LayerKind::Solid { width, height, .. } | LayerKind::Image { width, height, .. } => {
				((*width).max(1), (*height).max(1))
			}
			_ => (comp.width, comp.height),
		}
	}

	/// Every keyframable property, as `(stable key, label, track)`. Effect
	/// parameters are labelled through `param_name(plugin, index)`.
	pub fn tracks_mut<'a>(
		&'a mut self,
		param_name: &dyn Fn(&str, usize) -> String,
	) -> Vec<(String, String, &'a mut dyn Track)> {
		let mut out: Vec<(String, String, &'a mut dyn Track)> = Vec::new();
		let t = &mut self.transform;
		out.push(("anchor".into(), "Anchor Point".into(), &mut t.anchor));
		out.push(("position".into(), "Position".into(), &mut t.position));
		out.push(("scale".into(), "Scale".into(), &mut t.scale));
		out.push(("rotation".into(), "Rotation".into(), &mut t.rotation));
		out.push(("opacity".into(), "Opacity".into(), &mut t.opacity));
		match &mut self.kind {
			LayerKind::Solid { color, .. } => out.push(("solid.color".into(), "Color".into(), color)),
			LayerKind::Text(text) => {
				out.push(("text.size".into(), "Font Size".into(), &mut text.size));
				out.push(("text.color".into(), "Fill Color".into(), &mut text.color));
				out.push(("text.position".into(), "Text Position".into(), &mut text.position));
				out.push(("text.tracking".into(), "Tracking".into(), &mut text.tracking));
			}
			LayerKind::Shape { items } => {
				for item in items {
					let (id, name) = (item.id, item.name.clone());
					let mut push = |key: &str, label: &str, track: &'a mut dyn Track| {
						out.push((format!("shape.{id}.{key}"), format!("{name} › {label}"), track))
					};
					push("position", "Position", &mut item.position);
					push("rotation", "Rotation", &mut item.rotation);
					push("fill", "Fill", &mut item.fill);
					push("stroke", "Stroke", &mut item.stroke);
					push("stroke_width", "Stroke Width", &mut item.stroke_width);
					match &mut item.geometry {
						Geometry::Rect { size, roundness } => {
							push("size", "Size", size);
							push("roundness", "Roundness", roundness);
						}
						Geometry::Ellipse { size } => push("size", "Size", size),
						Geometry::Star { outer, inner, .. } => {
							push("outer", "Outer Radius", outer);
							push("inner", "Inner Radius", inner);
						}
						Geometry::Path(_) => {}
					}
				}
			}
			LayerKind::Image { .. } | LayerKind::Adjustment => {}
		}
		for mask in &mut self.masks {
			let (id, name) = (mask.id, mask.name.clone());
			out.push((
				format!("mask.{id}.opacity"),
				format!("{name} › Opacity"),
				&mut mask.opacity,
			));
			out.push((
				format!("mask.{id}.feather"),
				format!("{name} › Feather"),
				&mut mask.feather,
			));
		}
		for effect in &mut self.effects {
			let (id, plugin) = (effect.id, effect.plugin.clone());
			for (index, anim) in &mut effect.params {
				out.push((
					format!("fx.{id}.{index}"),
					format!(
						"{} › {}",
						crate::plugins::display_name(&plugin),
						param_name(&plugin, *index)
					),
					anim,
				));
			}
		}
		out
	}
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Comp {
	pub width: u32,
	pub height: u32,
	pub fps: u32,
	/// In frames.
	pub duration: i32,
	pub background: Rgba,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Project {
	pub comp: Comp,
	/// Top layer first, as in the After Effects timeline.
	pub layers: Vec<Layer>,
	pub next_id: Id,
}

impl Default for Project {
	fn default() -> Self {
		Self {
			comp: Comp {
				width: 1280,
				height: 720,
				fps: 30,
				duration: 150,
				background: [0.07, 0.07, 0.09, 1.0],
			},
			layers: Vec::new(),
			next_id: 1,
		}
	}
}

impl Project {
	pub fn alloc_id(&mut self) -> Id {
		let id = self.next_id;
		self.next_id += 1;
		id
	}

	pub fn layer(&self, id: Id) -> Option<&Layer> {
		self.layers.iter().find(|l| l.id == id)
	}

	pub fn layer_mut(&mut self, id: Id) -> Option<&mut Layer> {
		self.layers.iter_mut().find(|l| l.id == id)
	}

	pub fn layer_index(&self, id: Id) -> Option<usize> {
		self.layers.iter().position(|l| l.id == id)
	}

	/// A new full-length layer, placed centred.
	pub fn new_layer(&mut self, name: impl Into<String>, kind: LayerKind) -> Layer {
		let id = self.alloc_id();
		let mut layer = Layer {
			id,
			name: name.into(),
			kind,
			visible: true,
			start: 0,
			in_frame: 0,
			out_frame: self.comp.duration,
			blend: BlendMode::Normal,
			transform: Transform::centered(1, 1, &self.comp),
			masks: Vec::new(),
			effects: Vec::new(),
		};
		let (w, h) = layer.size(&self.comp);
		layer.transform = Transform::centered(w, h, &self.comp);
		layer
	}

	pub fn new_effect(&mut self, plugin: impl Into<String>) -> Effect {
		Effect {
			id: self.alloc_id(),
			plugin: plugin.into(),
			enabled: true,
			params: BTreeMap::new(),
			layers: BTreeMap::new(),
		}
	}

	pub fn new_mask(&mut self, layer: Id, path: BezPath) -> Option<Id> {
		let id = self.alloc_id();
		let layer = self.layer_mut(layer)?;
		let mask = Mask {
			id,
			name: format!("Mask {}", layer.masks.len() + 1),
			path,
			mode: MaskMode::Add,
			inverted: false,
			opacity: Anim::new(100.0),
			feather: Anim::new(0.0),
		};
		layer.masks.push(mask);
		Some(id)
	}

	/// Copy of a layer with fresh ids for it and everything it owns.
	pub fn duplicate_layer(&mut self, id: Id) -> Option<Id> {
		let index = self.layer_index(id)?;
		let mut copy = self.layers[index].clone();
		copy.id = self.alloc_id();
		copy.name = format!("{} copy", copy.name);
		for mask in &mut copy.masks {
			mask.id = self.alloc_id();
		}
		for effect in &mut copy.effects {
			effect.id = self.alloc_id();
		}
		if let LayerKind::Shape { items } = &mut copy.kind {
			for item in items {
				item.id = self.alloc_id();
			}
		}
		let new_id = copy.id;
		self.layers.insert(index, copy);
		Some(new_id)
	}

	/// Remove a layer and every effect link that points at it.
	pub fn remove_layer(&mut self, id: Id) {
		self.layers.retain(|l| l.id != id);
		for layer in &mut self.layers {
			for effect in &mut layer.effects {
				effect.layers.retain(|_, target| *target != id);
			}
		}
	}

	/// The effect with `id` and the layer it is on.
	pub fn effect(&self, id: Id) -> Option<(&Layer, &Effect)> {
		self.layers
			.iter()
			.find_map(|l| l.effects.iter().find(|e| e.id == id).map(|e| (l, e)))
	}

	pub fn effect_mut(&mut self, id: Id) -> Option<&mut Effect> {
		self.layers
			.iter_mut()
			.find_map(|l| l.effects.iter_mut().find(|e| e.id == id))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn constant_until_animated() {
		let mut a = Anim::new(1.0f32);
		a.set(10, 2.0);
		assert_eq!(a.at(0), 2.0);
		assert!(!a.is_animated());
	}

	#[test]
	fn keys_interpolate_and_clamp() {
		let mut a = Anim::new(0.0f32);
		a.toggle_animation(0);
		a.set(10, 10.0);
		assert_eq!(a.at(-5), 0.0);
		assert_eq!(a.at(5), 5.0);
		assert_eq!(a.at(50), 10.0);
		a.set_interp(0, Interp::Hold);
		assert_eq!(a.at(9), 0.0);
		a.set_interp(0, Interp::Ease);
		assert!(a.at(2) < 2.0 && a.at(8) > 8.0);
	}

	#[test]
	fn removing_the_last_key_keeps_its_value() {
		let mut a = Anim::new(0.0f32);
		a.toggle_animation(3);
		a.set(3, 7.0);
		a.remove_key(3);
		assert!(!a.is_animated());
		assert_eq!(a.value, 7.0);
	}

	#[test]
	fn moving_a_key_replaces_the_target() {
		let mut a = Anim::new(0.0f32);
		a.toggle_animation(0);
		a.set(10, 1.0);
		a.set(20, 2.0);
		a.move_key(10, 20);
		assert_eq!(a.key_frames().iter().map(|k| k.0).collect::<Vec<_>>(), vec![0, 20]);
		assert_eq!(a.at(20), 1.0);
	}

	#[test]
	fn discrete_params_step() {
		let mut a = Anim::new(PValue::Popup(1));
		a.toggle_animation(0);
		a.set(10, PValue::Popup(3));
		assert_eq!(a.at(9), PValue::Popup(1));
		assert_eq!(a.at(10), PValue::Popup(3));
	}

	#[test]
	fn project_round_trips_through_json() {
		let mut p = Project::default();
		let mut layer = p.new_layer(
			"Solid",
			LayerKind::Solid {
				color: Anim::new([1.0, 0.0, 0.0, 1.0]),
				width: 100,
				height: 50,
			},
		);
		let mut fx = p.new_effect("SDK_Noise");
		fx.params.insert(1, Anim::new(PValue::Float(3.0)));
		fx.layers.insert(2, layer.id);
		layer.effects.push(fx);
		p.layers.push(layer);
		let json = serde_json::to_string(&p).unwrap();
		assert_eq!(serde_json::from_str::<Project>(&json).unwrap(), p);
	}

	#[test]
	fn layer_matrix_maps_anchor_to_position() {
		let comp = Project::default().comp;
		let t = Transform::centered(100, 50, &comp);
		let mut pt = tiny_skia::Point::from_xy(50.0, 25.0);
		t.matrix(0).map_point(&mut pt);
		assert_eq!((pt.x, pt.y), (640.0, 360.0));
	}
}
