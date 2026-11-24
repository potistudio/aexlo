//! The project around an effect, read-only to plugins: one comp, its layers
//! and what is on them.
//!
//! An embedder describes it with a [`Project`] and hands it to
//! [`PluginInstance::set_project`](crate::PluginInstance::set_project). The
//! Layer, Comp, Item, PF Interface, Stream, Dynamic Stream, Keyframe and Text
//! Layer suites then all answer from that one description. Without one, the
//! effect is alone on a layer that fills its comp.
//!
//! Pixels are a separate matter: the effect renders from
//! [`set_input_layer`](crate::PluginInstance::set_input_layer), and layer
//! parameters from [`set_layer_param`](crate::PluginInstance::set_layer_param).
//! [`Project::link_layer_param`] says which project layer such a parameter is
//! showing.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::animation::{TimePoint, Track};
use crate::pixel::{AnyLayer, PixelDepthKind};
use crate::suites::layer::LayerHandle;
use crate::{AexloError, MaskPath, MaskVertex, Result, TextLayer};

/// The composition the layers are in. Its frame rate is that of the time
/// given to [`set_time`](crate::PluginInstance::set_time).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Comp {
	/// The comp's name.
	pub name: String,
	/// Width, in pixels.
	pub width: u32,
	/// Height, in pixels.
	pub height: u32,
	/// How long it runs.
	pub duration: TimePoint,
	/// Background colour: red, green, blue, alpha, in `0..=1`.
	pub background: [f64; 4],
}

impl Comp {
	/// A comp with a black background.
	pub fn new(name: impl Into<String>, width: u32, height: u32, duration: impl Into<TimePoint>) -> Self {
		Self {
			name: name.into(),
			width,
			height,
			duration: duration.into(),
			background: [0.0, 0.0, 0.0, 1.0],
		}
	}
}

/// A layer's 2D transform, over layer time.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct LayerTransform {
	/// Anchor point, in layer pixels.
	pub anchor: Track<[f64; 2]>,
	/// Where the anchor point is, in comp (or parent) pixels.
	pub position: Track<[f64; 2]>,
	/// Scale, in percent.
	pub scale: Track<[f64; 2]>,
	/// Rotation, in degrees clockwise.
	pub rotation: Track<f64>,
	/// Opacity, in percent.
	pub opacity: Track<f64>,
}

impl LayerTransform {
	/// A `size` layer centred in a `comp` sized frame, untransformed: After
	/// Effects' transform for a new layer.
	pub fn centred(size: (u32, u32), comp: (u32, u32)) -> Self {
		let middle = |(w, h): (u32, u32)| Track::new([f64::from(w) / 2.0, f64::from(h) / 2.0]);
		Self {
			anchor: middle(size),
			position: middle(comp),
			scale: Track::new([100.0, 100.0]),
			rotation: Track::new(0.0),
			opacity: Track::new(100.0),
		}
	}
}

/// What a layer is.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum LayerContent {
	/// Footage or a solid: an AV layer.
	Footage,
	/// A null object.
	Null,
	/// A text layer. Its source text only holds between keyframes.
	Text(Track<TextLayer>),
	/// A shape layer's contents (`ADBE Root Vectors Group`), top first.
	Shape(Vec<ShapeItem>),
}

/// One item of a shape layer's contents.
///
/// Parametric rectangles, ellipses and stars are given as their paths.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ShapeItem {
	/// `ADBE Vector Group`.
	Group(ShapeGroup),
	/// `ADBE Vector Shape - Group`.
	Path(ShapePath),
	/// `ADBE Vector Graphic - Fill`.
	Fill(ShapeFill),
	/// `ADBE Vector Filter - Merge`.
	Merge(MergeMode),
}

/// A group of shape items with a transform of its own.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ShapeGroup {
	/// The group's name.
	pub name: String,
	/// What is in it, top first.
	pub items: Vec<ShapeItem>,
	/// The group's transform.
	pub transform: ShapeTransform,
}

impl ShapeGroup {
	/// An untransformed group.
	pub fn new(name: impl Into<String>, items: Vec<ShapeItem>) -> Self {
		Self {
			name: name.into(),
			items,
			transform: ShapeTransform::default(),
		}
	}
}

/// A shape group's transform, over layer time.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ShapeTransform {
	/// Anchor point.
	pub anchor: Track<[f64; 2]>,
	/// Position of the anchor point.
	pub position: Track<[f64; 2]>,
	/// Scale, in percent.
	pub scale: Track<[f64; 2]>,
	/// Skew, in degrees.
	pub skew: Track<f64>,
	/// The axis skewed along, in degrees.
	pub skew_axis: Track<f64>,
	/// Rotation, in degrees clockwise.
	pub rotation: Track<f64>,
	/// Opacity, in percent.
	pub opacity: Track<f64>,
}

impl Default for ShapeTransform {
	fn default() -> Self {
		Self {
			anchor: Track::new([0.0; 2]),
			position: Track::new([0.0; 2]),
			scale: Track::new([100.0, 100.0]),
			skew: Track::new(0.0),
			skew_axis: Track::new(0.0),
			rotation: Track::new(0.0),
			opacity: Track::new(100.0),
		}
	}
}

/// A shape path, in the pixels of the layer (within its groups' transforms).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ShapePath {
	/// The path's name.
	pub name: String,
	/// The path, over layer time.
	pub path: Track<MaskPath>,
	/// Whether it runs backwards (`ADBE Vector Shape Direction` 3).
	pub reversed: bool,
}

impl ShapePath {
	/// A path running the way its vertices do.
	pub fn new(name: impl Into<String>, path: impl Into<Track<MaskPath>>) -> Self {
		Self {
			name: name.into(),
			path: path.into(),
			reversed: false,
		}
	}
}

/// A shape fill.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ShapeFill {
	/// Red, green, blue, alpha, in `0..=1`.
	pub colour: Track<[f64; 4]>,
	/// Opacity, in percent.
	pub opacity: Track<f64>,
	/// Fill by the even-odd rule instead of non-zero winding.
	pub even_odd: bool,
}

impl ShapeFill {
	/// An opaque non-zero fill of `colour`.
	pub fn new(colour: [f64; 4]) -> Self {
		Self {
			colour: Track::new(colour),
			opacity: Track::new(100.0),
			even_odd: false,
		}
	}
}

/// How Merge Paths combines the paths above it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MergeMode {
	/// Merge.
	Merge,
	/// Add.
	Add,
	/// Subtract.
	Subtract,
	/// Intersect.
	Intersect,
	/// Exclude intersections.
	Exclude,
}

impl MergeMode {
	/// The popup value of `ADBE Vector Merge Type`.
	fn choice(self) -> f64 {
		match self {
			Self::Merge => 1.0,
			Self::Add => 2.0,
			Self::Subtract => 3.0,
			Self::Intersect => 4.0,
			Self::Exclude => 5.0,
		}
	}
}

/// One layer of the comp.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ProjectLayer {
	/// The layer's id (`AEGP_GetLayerID`): not zero, unique in the project,
	/// and the same for as long as it is the same layer.
	pub id: u32,
	/// The layer's name.
	pub name: String,
	/// Width, in pixels.
	pub width: u32,
	/// Height, in pixels.
	pub height: u32,
	/// The comp time of layer time 0.
	pub start: TimePoint,
	/// The comp time the layer appears at.
	pub in_point: TimePoint,
	/// The comp time it is gone from.
	pub out_point: TimePoint,
	/// Comp seconds per layer second, as a ratio: `(2, 1)` plays at half
	/// speed and a negative one backwards. Neither part is zero.
	pub stretch: (i32, u32),
	/// The video switch.
	pub visible: bool,
	/// The id of the layer this one is parented to.
	pub parent: Option<u32>,
	/// The transform.
	pub transform: LayerTransform,
	/// What the layer is.
	pub content: LayerContent,
}

impl ProjectLayer {
	/// A layer as big and as long as `comp`, untransformed.
	pub fn new(id: u32, name: impl Into<String>, content: LayerContent, comp: &Comp) -> Self {
		let size = (comp.width, comp.height);
		Self {
			id,
			name: name.into(),
			width: comp.width,
			height: comp.height,
			start: TimePoint::ZERO,
			in_point: TimePoint::ZERO,
			out_point: comp.duration,
			stretch: (1, 1),
			visible: true,
			parent: None,
			transform: LayerTransform::centred(size, size),
			content,
		}
	}

	/// The layer time at comp time `comp`.
	pub fn to_layer_time(&self, comp: impl Into<TimePoint>) -> TimePoint {
		to_layer_time(self.start, self.stretch, comp.into())
	}

	/// The comp time at layer time `layer`.
	pub fn to_comp_time(&self, layer: impl Into<TimePoint>) -> TimePoint {
		to_comp_time(self.start, self.stretch, layer.into())
	}

	/// The outlines a text or shape layer shows at layer time `time`, in layer
	/// pixels with the shape groups' transforms applied. Other layers have
	/// none. Merge Paths are not applied.
	pub fn outlines(&self, time: impl Into<TimePoint>) -> Vec<MaskPath> {
		let time = time.into();
		match &self.content {
			LayerContent::Text(text) => text.at(time).outlines().to_vec(),
			LayerContent::Shape(items) => {
				let mut out = Vec::new();
				flatten(items, time, &IDENTITY, &mut out);
				out
			}
			LayerContent::Footage | LayerContent::Null => Vec::new(),
		}
	}

	/// The layer's outlines at layer time `time` filled with `rgba` on
	/// transparent: an input for an effect on a text or shape layer. `None`
	/// for other layers.
	///
	/// All outlines are filled as one, by the first fill's rule: colours,
	/// strokes and Merge Paths are for the embedder's renderer.
	pub fn rasterize(&self, kind: PixelDepthKind, time: impl Into<TimePoint>, rgba: [f32; 4]) -> Option<AnyLayer> {
		let even_odd = match &self.content {
			LayerContent::Text(_) => false,
			LayerContent::Shape(items) => first_fill(items).is_some_and(|fill| fill.even_odd),
			LayerContent::Footage | LayerContent::Null => return None,
		};
		let outlines = self.outlines(time);
		Some(crate::text::fill(
			&outlines,
			kind,
			(self.width, self.height),
			rgba,
			even_odd,
		))
	}
}

pub(crate) fn to_layer_time(start: TimePoint, stretch: (i32, u32), comp: TimePoint) -> TimePoint {
	let (c, s) = (i128::from(comp.scale()), i128::from(start.scale()));
	let since = i128::from(comp.value()) * s - i128::from(start.value()) * c;
	let num = if stretch.0 == 0 { 1 } else { i128::from(stretch.0) };
	TimePoint::from_ratio(since * i128::from(stretch.1), c * s * num, comp.scale())
}

pub(crate) fn to_comp_time(start: TimePoint, stretch: (i32, u32), layer: TimePoint) -> TimePoint {
	let (l, s) = (i128::from(layer.scale()), i128::from(start.scale()));
	let den = i128::from(stretch.1.max(1));
	let num = i128::from(layer.value()) * i128::from(stretch.0) * s + i128::from(start.value()) * l * den;
	TimePoint::from_ratio(num, l * den * s, layer.scale())
}

/// A comp and its layers, as an effect's host knows them.
///
/// ```
/// use aexlo::{Comp, LayerContent, Project, ProjectLayer, TimePoint};
///
/// let comp = Comp::new("Main", 1920, 1080, TimePoint::whole_seconds(10));
/// let top = ProjectLayer::new(7, "Title", LayerContent::Footage, &comp);
/// let below = ProjectLayer::new(3, "Background", LayerContent::Footage, &comp);
/// let project = Project::new(comp, vec![top, below], 7)?;
/// assert_eq!(project.layer(3).unwrap().name, "Background");
/// # Ok::<(), aexlo::AexloError>(())
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Project {
	comp: Comp,
	layers: Vec<ProjectLayer>,
	effect_layer: u32,
	param_layers: BTreeMap<usize, u32>,
}

impl Project {
	/// A project of `layers`, top first, with the effect on the layer whose
	/// id is `effect_layer`.
	///
	/// # Errors
	/// [`AexloError::InvalidProject`] when ids are zero or repeated, the
	/// effect's layer or a parent is missing, parents form a cycle, a size,
	/// duration or stretch is unusable, or a layer ends before it starts.
	pub fn new(comp: Comp, layers: Vec<ProjectLayer>, effect_layer: u32) -> Result<Self> {
		let invalid = |what: String| Err(AexloError::InvalidProject(what));
		let usable = |size: u32| size > 0 && i32::try_from(size).is_ok();
		if !usable(comp.width) || !usable(comp.height) {
			return invalid(format!("the comp is {}x{}", comp.width, comp.height));
		}
		if comp.duration <= TimePoint::ZERO {
			return invalid("the comp has no duration".into());
		}
		let mut parents = BTreeMap::new();
		for layer in &layers {
			let id = layer.id;
			if id == 0 || i32::try_from(id).is_err() {
				return invalid(format!("{id} cannot be a layer id"));
			}
			if parents.insert(id, layer.parent).is_some() {
				return invalid(format!("two layers have id {id}"));
			}
			if !usable(layer.width) || !usable(layer.height) {
				return invalid(format!("layer {id} is {}x{}", layer.width, layer.height));
			}
			if layer.stretch.0 == 0 || layer.stretch.1 == 0 {
				return invalid(format!("layer {id} has a zero stretch"));
			}
			if layer.out_point < layer.in_point {
				return invalid(format!("layer {id} ends before it starts"));
			}
		}
		if !parents.contains_key(&effect_layer) {
			return invalid(format!("the effect's layer {effect_layer} is not among the layers"));
		}
		for layer in &layers {
			let mut at = layer.parent;
			let mut steps = 0;
			while let Some(id) = at {
				let Some(parent) = parents.get(&id) else {
					return invalid(format!("parent {id} is not among the layers"));
				};
				steps += 1;
				if steps > layers.len() {
					return invalid(format!("the parents of layer {} form a cycle", layer.id));
				}
				at = *parent;
			}
		}
		Ok(Self {
			comp,
			layers,
			effect_layer,
			param_layers: BTreeMap::new(),
		})
	}

	/// Say that the effect's layer parameter `index` is showing layer `id`:
	/// what its stream reports as the chosen layer. The pixels still come
	/// from [`set_layer_param`](crate::PluginInstance::set_layer_param).
	///
	/// # Errors
	/// [`AexloError::InvalidProject`] when `id` is not one of the layers.
	pub fn link_layer_param(&mut self, index: usize, id: u32) -> Result<()> {
		if self.layer(id).is_none() {
			return Err(AexloError::InvalidProject(format!(
				"layer {id} is not among the layers"
			)));
		}
		self.param_layers.insert(index, id);
		Ok(())
	}

	/// The comp.
	pub fn comp(&self) -> &Comp {
		&self.comp
	}

	/// The layers, top first.
	pub fn layers(&self) -> &[ProjectLayer] {
		&self.layers
	}

	/// The layer with this id.
	pub fn layer(&self, id: u32) -> Option<&ProjectLayer> {
		self.layers.iter().find(|layer| layer.id == id)
	}

	/// The layer the effect is on.
	pub fn effect_layer(&self) -> &ProjectLayer {
		self.layer(self.effect_layer).expect("checked by `new`")
	}

	pub(crate) fn param_layer(&self, index: usize) -> Option<u32> {
		self.param_layers.get(&index).copied()
	}

	/// Layer `id`'s pixels to comp pixels at comp time `comp`, through its
	/// parents.
	pub(crate) fn layer_to_comp(&self, id: u32, comp: TimePoint) -> Option<Affine> {
		let mut matrix = IDENTITY;
		let mut at = Some(id);
		while let Some(id) = at {
			let layer = self.layer(id)?;
			let time = layer.to_layer_time(comp);
			let t = &layer.transform;
			let own = transform_matrix(
				t.anchor.at(time),
				t.position.at(time),
				t.scale.at(time),
				t.rotation.at(time),
			);
			matrix = then(&matrix, &own);
			at = layer.parent;
		}
		Some(matrix)
	}
}

// ---- 2D transforms ---------------------------------------------------------

/// `[a, b, c, d, tx, ty]`: maps `(x, y)` to `(ax + cy + tx, bx + dy + ty)`.
pub(crate) type Affine = [f64; 6];

const IDENTITY: Affine = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// `first`, then `second`.
fn then(first: &Affine, second: &Affine) -> Affine {
	let ([a, b, c, d, x, y], [p, q, r, s, u, v]) = (*first, *second);
	[
		a * p + b * r,
		a * q + b * s,
		c * p + d * r,
		c * q + d * s,
		x * p + y * r + u,
		x * q + y * s + v,
	]
}

fn turn(degrees: f64) -> Affine {
	let (sin, cos) = degrees.to_radians().sin_cos();
	[cos, sin, -sin, cos, 0.0, 0.0]
}

fn transform_matrix(anchor: [f64; 2], position: [f64; 2], scale: [f64; 2], rotation: f64) -> Affine {
	skewed_matrix(anchor, position, scale, (0.0, 0.0), rotation)
}

fn skewed_matrix(anchor: [f64; 2], position: [f64; 2], scale: [f64; 2], skew: (f64, f64), rotation: f64) -> Affine {
	let (amount, axis) = skew;
	let mut m = [1.0, 0.0, 0.0, 1.0, -anchor[0], -anchor[1]];
	m = then(&m, &[scale[0] / 100.0, 0.0, 0.0, scale[1] / 100.0, 0.0, 0.0]);
	if amount != 0.0 {
		m = then(&m, &turn(-axis));
		m = then(&m, &[1.0, 0.0, (-amount).to_radians().tan(), 1.0, 0.0, 0.0]);
		m = then(&m, &turn(axis));
	}
	m = then(&m, &turn(rotation));
	then(&m, &[1.0, 0.0, 0.0, 1.0, position[0], position[1]])
}

fn flatten(items: &[ShapeItem], time: TimePoint, outer: &Affine, out: &mut Vec<MaskPath>) {
	for item in items {
		match item {
			ShapeItem::Group(group) => {
				let t = &group.transform;
				let own = skewed_matrix(
					t.anchor.at(time),
					t.position.at(time),
					t.scale.at(time),
					(t.skew.at(time), t.skew_axis.at(time)),
					t.rotation.at(time),
				);
				flatten(&group.items, time, &then(&own, outer), out);
			}
			ShapeItem::Path(shape) => {
				let mut path = shape.path.at(time);
				if shape.reversed {
					reverse(&mut path);
				}
				for vertex in &mut path.vertices {
					transform_vertex(vertex, outer);
				}
				out.push(path);
			}
			ShapeItem::Fill(_) | ShapeItem::Merge(_) => {}
		}
	}
}

fn transform_vertex(v: &mut MaskVertex, m: &Affine) {
	let [a, b, c, d, tx, ty] = *m;
	let vector = |x: f64, y: f64| (a * x + c * y, b * x + d * y);
	let (x, y) = vector(v.x, v.y);
	(v.x, v.y) = (x + tx, y + ty);
	(v.tan_in_x, v.tan_in_y) = vector(v.tan_in_x, v.tan_in_y);
	(v.tan_out_x, v.tan_out_y) = vector(v.tan_out_x, v.tan_out_y);
}

/// The same curve run backwards, from the same vertex if it is closed.
fn reverse(path: &mut MaskPath) {
	if path.vertices.is_empty() {
		return;
	}
	if path.closed {
		path.vertices[1..].reverse();
	} else {
		path.vertices.reverse();
	}
	for v in &mut path.vertices {
		(v.tan_in_x, v.tan_out_x) = (v.tan_out_x, v.tan_in_x);
		(v.tan_in_y, v.tan_out_y) = (v.tan_out_y, v.tan_in_y);
	}
}

fn first_fill(items: &[ShapeItem]) -> Option<&ShapeFill> {
	items.iter().find_map(|item| match item {
		ShapeItem::Fill(fill) => Some(fill),
		ShapeItem::Group(group) => first_fill(&group.items),
		ShapeItem::Path(_) | ShapeItem::Merge(_) => None,
	})
}

// ---- The property tree under a layer's contents ----------------------------

/// How a node groups, as `AEGP_StreamGroupingType` tells it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Grouping {
	Leaf,
	Named,
	Indexed,
}

/// A node's value over time.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Property {
	Group,
	OneD(Track<f64>),
	TwoD { value: Track<[f64; 2]>, spatial: bool },
	Colour(Track<[f64; 4]>),
	Path(Track<MaskPath>),
	Text(Track<TextLayer>),
}

/// A node under `ADBE Root Vectors Group` or `ADBE Text Properties`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PropertyNode {
	pub parent: Option<usize>,
	/// Where it is among its parent's children.
	pub index: usize,
	pub children: Vec<usize>,
	pub match_name: &'static str,
	pub name: String,
	pub grouping: Grouping,
	pub value: Property,
}

/// The nodes of one layer's contents; node 0 is the root. Footage has none.
#[derive(Debug, Clone, Default)]
pub(crate) struct PropertyTree {
	pub nodes: Vec<PropertyNode>,
}

impl PropertyTree {
	pub(crate) fn of(content: &LayerContent) -> Self {
		let mut tree = Self::default();
		match content {
			LayerContent::Footage | LayerContent::Null => {}
			LayerContent::Text(text) => {
				let root = tree.group(None, "ADBE Text Properties", "Text", Grouping::Named);
				tree.leaf(root, "ADBE Text Document", "Source Text", Property::Text(text.clone()));
			}
			LayerContent::Shape(items) => {
				let root = tree.group(None, "ADBE Root Vectors Group", "Contents", Grouping::Indexed);
				tree.items(root, items);
			}
		}
		tree
	}

	fn push(&mut self, mut node: PropertyNode) -> usize {
		let id = self.nodes.len();
		if let Some(parent) = node.parent {
			node.index = self.nodes[parent].children.len();
			self.nodes[parent].children.push(id);
		}
		self.nodes.push(node);
		id
	}

	fn group(&mut self, parent: Option<usize>, match_name: &'static str, name: &str, grouping: Grouping) -> usize {
		self.push(PropertyNode {
			parent,
			index: 0,
			children: Vec::new(),
			match_name,
			name: name.to_string(),
			grouping,
			value: Property::Group,
		})
	}

	fn leaf(&mut self, parent: usize, match_name: &'static str, name: &str, value: Property) {
		self.push(PropertyNode {
			parent: Some(parent),
			index: 0,
			children: Vec::new(),
			match_name,
			name: name.to_string(),
			grouping: Grouping::Leaf,
			value,
		});
	}

	fn items(&mut self, parent: usize, items: &[ShapeItem]) {
		use Grouping::{Indexed, Named};
		use Property as P;
		let one = |value: f64| P::OneD(Track::new(value));
		for item in items {
			match item {
				ShapeItem::Group(group) => {
					let node = self.group(Some(parent), "ADBE Vector Group", &group.name, Named);
					self.leaf(node, "ADBE Vector Blend Mode", "Blend Mode", one(1.0));
					let contents = self.group(Some(node), "ADBE Vectors Group", "Contents", Indexed);
					self.items(contents, &group.items);
					let place = self.group(Some(node), "ADBE Vector Transform Group", "Transform", Named);
					let t = &group.transform;
					let two = |value: &Track<[f64; 2]>, spatial| P::TwoD {
						value: value.clone(),
						spatial,
					};
					for (match_name, name, value) in [
						("ADBE Vector Anchor", "Anchor Point", two(&t.anchor, true)),
						("ADBE Vector Position", "Position", two(&t.position, true)),
						("ADBE Vector Scale", "Scale", two(&t.scale, false)),
						("ADBE Vector Skew", "Skew", P::OneD(t.skew.clone())),
						("ADBE Vector Skew Axis", "Skew Axis", P::OneD(t.skew_axis.clone())),
						("ADBE Vector Rotation", "Rotation", P::OneD(t.rotation.clone())),
						("ADBE Vector Group Opacity", "Opacity", P::OneD(t.opacity.clone())),
					] {
						self.leaf(place, match_name, name, value);
					}
				}
				ShapeItem::Path(shape) => {
					let node = self.group(Some(parent), "ADBE Vector Shape - Group", &shape.name, Named);
					let direction = if shape.reversed { 3.0 } else { 1.0 };
					self.leaf(node, "ADBE Vector Shape Direction", "Shape Direction", one(direction));
					self.leaf(node, "ADBE Vector Shape", "Path", P::Path(shape.path.clone()));
				}
				ShapeItem::Fill(fill) => {
					let node = self.group(Some(parent), "ADBE Vector Graphic - Fill", "Fill", Named);
					let rule = if fill.even_odd { 2.0 } else { 1.0 };
					for (match_name, name, value) in [
						("ADBE Vector Blend Mode", "Blend Mode", one(1.0)),
						("ADBE Vector Composite Order", "Composite", one(1.0)),
						("ADBE Vector Fill Rule", "Fill Rule", one(rule)),
						("ADBE Vector Fill Color", "Color", P::Colour(fill.colour.clone())),
						("ADBE Vector Fill Opacity", "Opacity", P::OneD(fill.opacity.clone())),
					] {
						self.leaf(node, match_name, name, value);
					}
				}
				ShapeItem::Merge(mode) => {
					let node = self.group(Some(parent), "ADBE Vector Filter - Merge", "Merge Paths", Named);
					self.leaf(node, "ADBE Vector Merge Type", "Mode", one(mode.choice()));
				}
			}
		}
	}

	/// Whether `other` has the same nodes, whatever their values and names: a
	/// handle to a node of one then names the same node of the other.
	fn same_nodes(&self, other: &Self) -> bool {
		self.nodes.len() == other.nodes.len()
			&& self
				.nodes
				.iter()
				.zip(&other.nodes)
				.all(|(a, b)| a.parent == b.parent && a.match_name == b.match_name)
	}
}

// ---- What an instance keeps ------------------------------------------------

/// One layer's side of an instance's [`ProjectState`].
#[derive(Debug)]
pub(crate) struct LayerState {
	pub id: u32,
	/// `None` for the effect's own layer, whose handle is the effect's.
	pub handle: Option<LayerHandle>,
	pub tree: PropertyTree,
	/// Changes when the tree's nodes do, so that a stream handle from before
	/// cannot name a different property.
	pub revision: u64,
}

/// A project as an instance serves it.
#[derive(Debug)]
pub(crate) struct ProjectState {
	pub project: Project,
	pub layers: Vec<LayerState>,
}

impl ProjectState {
	/// Serve `project` for the instance whose stable token is `owner`. Layers
	/// that were in `previous` keep their handles; those that are gone take
	/// theirs with them, and a handle is never given out again.
	pub(crate) fn new(project: Project, owner: usize, previous: Option<Self>) -> Self {
		static REVISIONS: AtomicU64 = AtomicU64::new(1);
		let mut before = previous.map_or_else(Vec::new, |state| state.layers);
		let layers = project
			.layers
			.iter()
			.map(|layer| {
				let old = before
					.iter()
					.position(|old| old.id == layer.id)
					.map(|at| before.swap_remove(at));
				let tree = PropertyTree::of(&layer.content);
				let revision = match &old {
					Some(old) if old.tree.same_nodes(&tree) => old.revision,
					_ => REVISIONS.fetch_add(1, Ordering::Relaxed),
				};
				let handle = (layer.id != project.effect_layer).then(|| {
					old.and_then(|old| old.handle)
						.unwrap_or_else(|| LayerHandle::new(owner, layer.id))
				});
				LayerState {
					id: layer.id,
					handle,
					tree,
					revision,
				}
			})
			.collect();
		Self { project, layers }
	}

	pub(crate) fn layer(&self, id: u32) -> Option<&LayerState> {
		self.layers.iter().find(|layer| layer.id == id)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::animation::Keyframe;

	fn comp() -> Comp {
		Comp::new("c", 200, 100, TimePoint::whole_seconds(10))
	}

	fn footage(id: u32) -> ProjectLayer {
		ProjectLayer::new(id, format!("layer {id}"), LayerContent::Footage, &comp())
	}

	fn at(value: i32, scale: u32) -> TimePoint {
		TimePoint::new(value, scale).unwrap()
	}

	#[test]
	fn layer_time_is_offset_and_stretched() {
		let mut layer = footage(1);
		layer.start = at(2, 1);
		layer.stretch = (2, 1);
		assert_eq!(layer.to_layer_time(at(3, 1)), at(1, 2));
		assert_eq!(layer.to_layer_time(at(0, 24)), at(-1, 1));
		assert_eq!(layer.to_comp_time(at(1, 2)), at(3, 1));

		layer.stretch = (-100, 100);
		assert_eq!(layer.to_layer_time(at(3, 1)), at(-1, 1));
		assert_eq!(layer.to_comp_time(at(-1, 1)), at(3, 1));

		// 29.97 fps with an offset of a frame at 25 fps round-trips exactly.
		layer.start = at(1, 25);
		layer.stretch = (3, 7);
		for value in [-1001, 0, 1001, 90_090] {
			let there = layer.to_layer_time(at(value, 30_000));
			assert_eq!(layer.to_comp_time(there), at(value, 30_000), "{value}");
		}
	}

	#[test]
	fn a_project_checks_what_it_is_given() {
		let refused =
			|layers, effect| matches!(Project::new(comp(), layers, effect), Err(AexloError::InvalidProject(_)));
		assert!(refused(vec![], 1));
		assert!(refused(vec![footage(0)], 0));
		assert!(refused(vec![footage(1), footage(1)], 1));
		assert!(refused(vec![footage(1)], 2));

		let mut stretched = footage(1);
		stretched.stretch = (0, 1);
		assert!(refused(vec![stretched], 1));

		let (mut a, mut b) = (footage(1), footage(2));
		a.parent = Some(2);
		assert!(Project::new(comp(), vec![a.clone(), b.clone()], 1).is_ok());
		b.parent = Some(1);
		assert!(refused(vec![a.clone(), b], 1));
		a.parent = Some(9);
		assert!(refused(vec![a], 1));
	}

	#[test]
	fn children_follow_their_parents_at_the_same_comp_time() {
		let mut parent = footage(1);
		parent.start = at(1, 1);
		parent.transform.position = Track::keyed(vec![
			Keyframe::linear(at(0, 1), [100.0, 50.0]),
			Keyframe::linear(at(2, 1), [140.0, 50.0]),
		])
		.unwrap();
		let mut child = footage(2);
		child.parent = Some(1);
		let project = Project::new(comp(), vec![child, parent], 2).unwrap();
		let m = project.layer_to_comp(2, at(2, 1)).unwrap();
		assert_eq!(m, [1.0, 0.0, 0.0, 1.0, 20.0, 0.0]);
	}

	#[test]
	fn shape_outlines_take_their_groups_transforms_and_time() {
		let line = |x: f64| MaskPath {
			vertices: vec![MaskVertex::corner(x, 0.0), MaskVertex::corner(x, 10.0)],
			..Default::default()
		};
		let moving = Track::keyed(vec![
			Keyframe::linear(at(0, 1), line(0.0)),
			Keyframe::linear(at(1, 1), line(8.0)),
		]);
		let mut group = ShapeGroup::new("g", vec![ShapeItem::Path(ShapePath::new("p", moving.unwrap()))]);
		group.transform.position = Track::new([30.0, 40.0]);
		group.transform.scale = Track::new([200.0, 100.0]);
		let layer = ProjectLayer::new(1, "s", LayerContent::Shape(vec![ShapeItem::Group(group)]), &comp());
		assert_eq!(
			layer.outlines(at(1, 2))[0].vertices,
			[MaskVertex::corner(38.0, 40.0), MaskVertex::corner(38.0, 50.0)]
		);
	}

	#[test]
	fn a_shape_tree_has_the_match_names_plugins_walk() {
		let path = MaskPath::default();
		let items = vec![ShapeItem::Group(ShapeGroup::new(
			"g",
			vec![
				ShapeItem::Path(ShapePath::new("p", path)),
				ShapeItem::Merge(MergeMode::Subtract),
			],
		))];
		let tree = PropertyTree::of(&LayerContent::Shape(items));
		let child = |of: usize, name: &str| {
			*tree.nodes[of]
				.children
				.iter()
				.find(|&&id| tree.nodes[id].match_name == name)
				.unwrap()
		};
		let group = child(0, "ADBE Vector Group");
		let contents = child(group, "ADBE Vectors Group");
		let shape = child(contents, "ADBE Vector Shape - Group");
		assert!(matches!(
			tree.nodes[child(shape, "ADBE Vector Shape")].value,
			Property::Path(_)
		));
		let merge = child(contents, "ADBE Vector Filter - Merge");
		assert_eq!(tree.nodes[merge].index, 1);
		child(child(group, "ADBE Vector Transform Group"), "ADBE Vector Skew Axis");
	}
}
