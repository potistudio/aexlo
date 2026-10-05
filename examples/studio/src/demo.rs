//! The project the studio opens with: a little of everything.

use crate::model::{Anim, BezPath, Geometry, Interp, Key, LayerKind, MaskMode, Project, ShapeItem, Text, TextAlign};
use crate::plugins;

fn keys<T: Clone>(frames: &[(i32, T)], interp: Interp) -> Anim<T> {
	Anim {
		value: frames[0].1.clone(),
		keys: frames
			.iter()
			.map(|(frame, value)| Key {
				frame: *frame,
				value: value.clone(),
				interp,
			})
			.collect(),
	}
}

pub fn project() -> Project {
	let mut p = Project::default();
	let (w, h) = (p.comp.width as f32, p.comp.height as f32);
	let end = p.comp.duration - 1;
	let mut layers = Vec::new();

	// Title, fading in.
	let mut title = p.new_layer(
		"Title",
		LayerKind::Text(Text {
			text: "aexlo studio".into(),
			font: None,
			size: Anim::new(110.0),
			color: Anim::new([1.0, 1.0, 1.0, 1.0]),
			position: Anim::new([w / 2.0, 190.0]),
			align: TextAlign::Center,
			tracking: keys(&[(0, 30.0), (45, 0.0)], Interp::Ease),
		}),
	);
	title.transform.opacity = keys(&[(0, 0.0), (20, 100.0)], Interp::Linear);
	layers.push(title);

	// Particles from Furikake on a transparent null.
	if plugins::list_fixtures().iter().any(|f| f == "Furikake") {
		let mut null = p.new_layer(
			"Particles",
			LayerKind::Solid {
				color: Anim::new([0.0; 4]),
				width: p.comp.width,
				height: p.comp.height,
			},
		);
		null.effects.push(p.new_effect("Furikake"));
		layers.push(null);
	}

	// A spinning star and a ring.
	let star_id = p.alloc_id();
	let mut star = ShapeItem::new(
		star_id,
		Geometry::Star {
			points: 5,
			outer: Anim::new(120.0),
			inner: Anim::new(52.0),
		},
		[w / 2.0, h / 2.0 + 90.0],
		[0.98, 0.78, 0.25, 1.0],
	);
	star.rotation = keys(&[(0, 0.0), (end, 360.0)], Interp::Linear);
	star.name = "Star".into();
	let ring_id = p.alloc_id();
	let mut ring = ShapeItem::new(
		ring_id,
		Geometry::Ellipse {
			size: keys(&[(0, [200.0, 200.0]), (40, [330.0, 330.0])], Interp::Ease),
		},
		[w / 2.0, h / 2.0 + 90.0],
		[1.0; 4],
	);
	ring.name = "Ring".into();
	ring.fill_on = false;
	ring.stroke_on = true;
	ring.stroke_width = Anim::new(6.0);
	layers.push(p.new_layer(
		"Shapes",
		LayerKind::Shape {
			items: vec![star, ring],
		},
	));

	// The workspace's sample image, with a feathered elliptical mask.
	let image = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../input.png");
	if let Ok((iw, ih)) = image::image_dimensions(&image) {
		let mut bg = p.new_layer(
			"Background",
			LayerKind::Image {
				path: image.to_string_lossy().into_owned(),
				width: iw,
				height: ih,
			},
		);
		let cover = (w / iw as f32).max(h / ih as f32) * 100.0;
		bg.transform.scale = Anim::new([cover, cover]);
		let (iw, ih) = (iw as f32, ih as f32);
		let mask_id = p.alloc_id();
		bg.masks.push(crate::model::Mask {
			id: mask_id,
			name: "Vignette".into(),
			path: BezPath::ellipse(iw * 0.08, ih * 0.05, iw * 0.92, ih * 0.95),
			mode: MaskMode::Add,
			inverted: false,
			opacity: Anim::new(100.0),
			feather: Anim::new(iw * 0.06),
		});
		bg.transform.opacity = Anim::new(70.0);
		layers.push(bg);
	}

	p.layers = layers;
	p
}
