//! What effects are told of the project around them: Studio's layers and
//! keyframes as an [`aexlo::Project`] and tracks in each layer's own time.
//!
//! aexlo interpolates linearly or holds, so eased and other baked properties
//! get a linear key on every frame of their animation. Source text is the
//! current frame's: laying out a font for every frame would be too slow to do
//! on each render.

use std::collections::BTreeMap;

use aexlo::{
	Animate, Comp, Keyframe, LayerContent, ParamValue, ProjectLayer, ShapeFill, ShapeGroup, ShapePath, TimePoint, Track,
};
use tiny_skia::PathSegment;

use crate::model::{Anim, Geometry, Id, Interp, Layer, LayerKind, Lerp, PValue, Project, ShapeItem};
use crate::raster::{Fonts, shape_path, text_outlines};

/// Where a layer's time is in the comp.
#[derive(Clone, Copy)]
pub struct Clock {
	start: i32,
	fps: u32,
}

impl Clock {
	pub fn of(layer: &Layer, project: &Project) -> Self {
		Self {
			start: layer.start,
			fps: project.comp.fps.max(1),
		}
	}

	fn time(self, comp_frame: i32) -> TimePoint {
		TimePoint::frame(comp_frame.saturating_sub(self.start), self.fps)
	}

	fn track<T: Lerp, U: Animate>(self, anim: &Anim<T>, map: impl Fn(&T) -> U) -> Track<U> {
		let (Some(first), Some(last)) = (anim.keys.first(), anim.keys.last()) else {
			return Track::new(map(&anim.value));
		};
		let keys = if anim.keys.iter().any(|key| key.interp == Interp::Ease) {
			(first.frame..=last.frame)
				.map(|frame| Keyframe::linear(self.time(frame), map(&anim.at(frame))))
				.collect()
		} else {
			anim.keys
				.iter()
				.map(|key| match key.interp {
					Interp::Hold => Keyframe::hold(self.time(key.frame), map(&key.value)),
					_ => Keyframe::linear(self.time(key.frame), map(&key.value)),
				})
				.collect()
		};
		Track::keyed(keys).unwrap_or_else(|| Track::new(map(&anim.value)))
	}

	/// A parameter's keyframes as the effect sees them.
	pub fn param(self, anim: &Anim<PValue>) -> Option<Track<ParamValue>> {
		anim.is_animated().then(|| self.track(anim, |value| value.into()))
	}
}

/// The project around an effect on `layer` with `links` as its layer
/// parameters, at comp frame `frame`.
pub fn project(
	project: &Project,
	layer: &Layer,
	links: &BTreeMap<usize, Id>,
	frame: i32,
	fonts: &mut Fonts,
) -> Result<aexlo::Project, String> {
	let fps = project.comp.fps.max(1);
	let (width, height) = (project.comp.width, project.comp.height);
	let mut comp = Comp::new(
		"Comp",
		width,
		height,
		TimePoint::frame(project.comp.duration.max(1), fps),
	);
	comp.background = project.comp.background.map(f64::from);

	let layers = project
		.layers
		.iter()
		.map(|layer| project_layer(layer, project, &comp, frame, fonts))
		.collect::<Result<Vec<_>, String>>()?;
	let mut out = aexlo::Project::new(comp, layers, layer_id(layer.id)?).map_err(|e| e.to_string())?;
	for (&index, &target) in links {
		// A link left pointing at a deleted layer names no layer.
		if project.layers.iter().any(|layer| layer.id == target) {
			out.link_layer_param(index, layer_id(target)?)
				.map_err(|e| e.to_string())?;
		}
	}
	Ok(out)
}

fn layer_id(id: Id) -> Result<u32, String> {
	u32::try_from(id).map_err(|_| format!("layer id {id} is too large"))
}

fn project_layer(
	layer: &Layer,
	project: &Project,
	comp: &Comp,
	frame: i32,
	fonts: &mut Fonts,
) -> Result<ProjectLayer, String> {
	let clock = Clock::of(layer, project);
	let content = match &layer.kind {
		LayerKind::Text(text) => {
			let outlines = text_outlines(text, frame, fonts);
			LayerContent::Text(Track::new(aexlo::TextLayer::from_outlines(text.text.clone(), outlines)))
		}
		LayerKind::Shape { items } => {
			LayerContent::Shape(items.iter().filter_map(|item| group(item, clock, frame)).collect())
		}
		LayerKind::Solid { .. } | LayerKind::Image { .. } | LayerKind::Adjustment => LayerContent::Footage,
	};
	let mut out = ProjectLayer::new(layer_id(layer.id)?, layer.name.clone(), content, comp);
	(out.width, out.height) = layer.size(&project.comp);
	out.start = TimePoint::frame(layer.start, clock.fps);
	out.in_point = TimePoint::frame(layer.in_frame, clock.fps);
	out.out_point = TimePoint::frame(layer.out_frame.max(layer.in_frame), clock.fps);
	out.visible = layer.visible;
	// An adjustment layer applies in place (`Layer::matrix`).
	if !matches!(layer.kind, LayerKind::Adjustment) {
		let transform = &layer.transform;
		out.transform.anchor = clock.track(&transform.anchor, |v| v.map(f64::from));
		out.transform.position = clock.track(&transform.position, |v| v.map(f64::from));
		out.transform.scale = clock.track(&transform.scale, |v| v.map(f64::from));
		out.transform.rotation = clock.track(&transform.rotation, |v| f64::from(*v));
		out.transform.opacity = clock.track(&transform.opacity, |v| f64::from(*v));
	}
	Ok(out)
}

/// An item as a group of its path, as drawn, and its fill.
fn group(item: &ShapeItem, clock: Clock, frame: i32) -> Option<aexlo::ShapeItem> {
	let mut span: Option<(i32, i32)> = None;
	let mut animated = |keys: Option<(i32, i32)>| {
		if let Some((from, to)) = keys {
			span = Some(span.map_or((from, to), |(a, b)| (a.min(from), b.max(to))));
		}
	};
	animated(key_span(&item.position));
	animated(key_span(&item.rotation));
	match &item.geometry {
		Geometry::Rect { size, roundness } => {
			animated(key_span(size));
			animated(key_span(roundness));
		}
		Geometry::Ellipse { size } => animated(key_span(size)),
		Geometry::Star { outer, inner, .. } => {
			animated(key_span(outer));
			animated(key_span(inner));
		}
		Geometry::Path(_) => {}
	}
	let outline = |frame| mask_path(&shape_path(item, frame)?, &item.name);
	let path = span
		.and_then(|(from, to)| {
			let keys = (from..=to)
				.filter_map(|frame| Some(Keyframe::linear(clock.time(frame), outline(frame)?)))
				.collect();
			Track::keyed(keys)
		})
		.or_else(|| outline(frame).map(Track::new))?;

	let mut items = vec![aexlo::ShapeItem::Path(ShapePath::new(item.name.clone(), path))];
	if item.fill_on {
		let mut fill = ShapeFill::new([1.0; 4]);
		fill.colour = clock.track(&item.fill, |v| v.map(f64::from));
		items.push(aexlo::ShapeItem::Fill(fill));
	}
	Some(aexlo::ShapeItem::Group(ShapeGroup::new(item.name.clone(), items)))
}

fn key_span<T>(anim: &Anim<T>) -> Option<(i32, i32)> {
	Some((anim.keys.first()?.frame, anim.keys.last()?.frame))
}

/// A one-contour path with its control points as tangents.
fn mask_path(path: &tiny_skia::Path, name: &str) -> Option<aexlo::MaskPath> {
	let point = |p: tiny_skia::Point| (f64::from(p.x), f64::from(p.y));
	let mut vertices: Vec<aexlo::MaskVertex> = Vec::new();
	let mut closed = false;
	for segment in path.segments() {
		let from = vertices.last().map(|v| (v.x, v.y));
		let (c1, c2, to) = match (segment, from) {
			(PathSegment::MoveTo(p), None) => {
				let (x, y) = point(p);
				vertices.push(aexlo::MaskVertex::corner(x, y));
				continue;
			}
			(PathSegment::LineTo(p), Some(from)) => (from, point(p), point(p)),
			(PathSegment::QuadTo(q, p), Some(from)) => {
				let (q, to) = (point(q), point(p));
				let raise = |p: (f64, f64)| (p.0 + (q.0 - p.0) * 2.0 / 3.0, p.1 + (q.1 - p.1) * 2.0 / 3.0);
				(raise(from), raise(to), to)
			}
			(PathSegment::CubicTo(a, b, p), Some(_)) => (point(a), point(b), point(p)),
			(PathSegment::Close, Some(_)) => {
				closed = true;
				break;
			}
			_ => break,
		};
		let last = vertices.last_mut()?;
		(last.tan_out_x, last.tan_out_y) = (c1.0 - last.x, c1.1 - last.y);
		let mut vertex = aexlo::MaskVertex::corner(to.0, to.1);
		(vertex.tan_in_x, vertex.tan_in_y) = (c2.0 - to.0, c2.1 - to.1);
		vertices.push(vertex);
	}
	// A closed path's last vertex is its first again.
	if closed
		&& let [first, .., last] = vertices[..]
		&& (first.x, first.y) == (last.x, last.y)
	{
		(vertices[0].tan_in_x, vertices[0].tan_in_y) = (last.tan_in_x, last.tan_in_y);
		vertices.pop();
	}
	(!vertices.is_empty()).then(|| aexlo::MaskPath {
		id: 1,
		name: name.to_owned(),
		vertices,
		closed,
		..Default::default()
	})
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::model::Key;

	fn rect(size: [f32; 2]) -> ShapeItem {
		ShapeItem::new(
			9,
			Geometry::Rect {
				size: Anim::new(size),
				roundness: Anim::new(0.0),
			},
			[40.0, 30.0],
			[1.0; 4],
		)
	}

	#[test]
	fn a_shape_layer_is_given_as_the_paths_it_draws() {
		let mut p = Project::default();
		let mut item = rect([20.0, 10.0]);
		item.position.keys = vec![
			Key {
				frame: 10,
				value: [40.0, 30.0],
				interp: Interp::Linear,
			},
			Key {
				frame: 20,
				value: [60.0, 30.0],
				interp: Interp::Linear,
			},
		];
		let mut layer = p.new_layer("Shapes", LayerKind::Shape { items: vec![item] });
		layer.start = 5;
		p.layers = vec![layer.clone()];
		let told = super::project(&p, &layer, &BTreeMap::new(), 0, &mut Fonts::new()).unwrap();
		let fps = p.comp.fps;
		// Layer frame 10 is comp frame 15, half way along.
		let outline = &told.effect_layer().outlines(TimePoint::frame(10, fps))[0];
		assert!(outline.closed);
		assert_eq!(outline.vertices.len(), 4);
		let xs = outline.vertices.iter().map(|v| v.x);
		assert_eq!(
			(xs.clone().fold(f64::MAX, f64::min), xs.fold(f64::MIN, f64::max)),
			(40.0, 60.0)
		);
	}

	#[test]
	fn eased_keys_are_the_values_studio_shows() {
		let mut p = Project::default();
		let mut layer = p.new_layer("Shapes", LayerKind::Shape { items: Vec::new() });
		layer.start = -4;
		let anim = Anim {
			value: PValue::Float(0.0),
			keys: vec![
				Key {
					frame: 0,
					value: PValue::Float(0.0),
					interp: Interp::Ease,
				},
				Key {
					frame: 8,
					value: PValue::Float(80.0),
					interp: Interp::Linear,
				},
			],
		};
		let track = Clock::of(&layer, &p).param(&anim).unwrap();
		for frame in [-2, 0, 1, 4, 7, 8, 12] {
			let seen = track.at(TimePoint::frame(frame + 4, p.comp.fps.max(1)));
			assert_eq!(seen, ParamValue::from(&anim.at(frame)), "frame {frame}");
		}
	}
}
