//! Pixels: images moving between layers and plugins, and the rasterizers for
//! everything the project draws itself (solids, shapes, text, masks).

use std::collections::HashMap;
use std::path::PathBuf;

use ab_glyph::{Font, FontArc, PxScale, ScaleFont, point};
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Stroke};

use crate::model::{BezPath, Geometry, KAPPA, Mask, MaskMode, Rgba, ShapeItem, Text, TextAlign};

/// Straight-alpha RGBA8 pixels: the format layers hand to plugins.
#[derive(Clone)]
pub struct Image {
	pub width: u32,
	pub height: u32,
	pub pixels: Vec<u8>,
}

impl Image {
	pub fn transparent(width: u32, height: u32) -> Self {
		Self {
			width,
			height,
			pixels: vec![0; width as usize * height as usize * 4],
		}
	}

	pub fn filled(width: u32, height: u32, color: Rgba) -> Self {
		let px = color.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8);
		Self {
			width,
			height,
			pixels: px.repeat(width as usize * height as usize),
		}
	}

	/// Unpremultiply a pixmap.
	pub fn from_pixmap(pixmap: &Pixmap) -> Self {
		let mut pixels = pixmap.data().to_vec();
		for px in pixels.as_chunks_mut::<4>().0 {
			let a = px[3] as u32;
			if a != 0 && a != 255 {
				for c in &mut px[..3] {
					*c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
				}
			}
		}
		Self {
			width: pixmap.width(),
			height: pixmap.height(),
			pixels,
		}
	}

	/// Premultiply into a pixmap.
	pub fn to_pixmap(&self) -> Pixmap {
		let mut pixels = self.pixels.clone();
		for px in pixels.as_chunks_mut::<4>().0 {
			let a = px[3] as u32;
			if a != 255 {
				for c in &mut px[..3] {
					*c = ((*c as u32 * a + 127) / 255) as u8;
				}
			}
		}
		Pixmap::from_vec(
			pixels,
			tiny_skia::IntSize::from_wh(self.width, self.height).expect("non-empty image"),
		)
		.expect("pixel buffer matches its size")
	}

	/// Scale every pixel's alpha by `coverage` (`0..=1`, one per pixel).
	pub fn multiply_alpha(&mut self, coverage: &[f32]) {
		for (px, &c) in self.pixels.as_chunks_mut::<4>().0.iter_mut().zip(coverage) {
			px[3] = (px[3] as f32 * c).round() as u8;
		}
	}
}

fn color(c: Rgba) -> tiny_skia::Color {
	tiny_skia::Color::from_rgba(
		c[0].clamp(0.0, 1.0),
		c[1].clamp(0.0, 1.0),
		c[2].clamp(0.0, 1.0),
		c[3].clamp(0.0, 1.0),
	)
	.unwrap_or(tiny_skia::Color::TRANSPARENT)
}

fn paint(c: Rgba) -> Paint<'static> {
	let mut paint = Paint {
		anti_alias: true,
		..Paint::default()
	};
	paint.set_color(color(c));
	paint
}

/// A Bezier path, offset by `origin`.
pub fn bez_path(path: &BezPath, origin: [f32; 2]) -> Option<tiny_skia::Path> {
	let first = path.vertices.first()?;
	let mut pb = PathBuilder::new();
	pb.move_to(first.p[0] + origin[0], first.p[1] + origin[1]);
	for [_, c0, c1, p1] in path.segments() {
		pb.cubic_to(
			c0[0] + origin[0],
			c0[1] + origin[1],
			c1[0] + origin[0],
			c1[1] + origin[1],
			p1[0] + origin[0],
			p1[1] + origin[1],
		);
	}
	if path.closed {
		pb.close();
	}
	pb.finish()
}

/// A shape item's outline in layer coordinates at `frame`.
pub fn shape_path(item: &ShapeItem, frame: i32) -> Option<tiny_skia::Path> {
	let local = match &item.geometry {
		Geometry::Rect { size, roundness } => {
			let [w, h] = size.at(frame).map(f32::abs);
			let r = roundness.at(frame).clamp(0.0, w.min(h) / 2.0);
			let (x0, y0, x1, y1) = (-w / 2.0, -h / 2.0, w / 2.0, h / 2.0);
			if r <= 0.0 {
				PathBuilder::from_rect(tiny_skia::Rect::from_ltrb(x0, y0, x1, y1)?)
			} else {
				let k = r * (1.0 - KAPPA);
				let mut pb = PathBuilder::new();
				pb.move_to(x0 + r, y0);
				pb.line_to(x1 - r, y0);
				pb.cubic_to(x1 - k, y0, x1, y0 + k, x1, y0 + r);
				pb.line_to(x1, y1 - r);
				pb.cubic_to(x1, y1 - k, x1 - k, y1, x1 - r, y1);
				pb.line_to(x0 + r, y1);
				pb.cubic_to(x0 + k, y1, x0, y1 - k, x0, y1 - r);
				pb.line_to(x0, y0 + r);
				pb.cubic_to(x0, y0 + k, x0 + k, y0, x0 + r, y0);
				pb.close();
				pb.finish()?
			}
		}
		Geometry::Ellipse { size } => {
			let [w, h] = size.at(frame).map(f32::abs);
			PathBuilder::from_oval(tiny_skia::Rect::from_xywh(-w / 2.0, -h / 2.0, w, h)?)?
		}
		Geometry::Star { points, outer, inner } => {
			let n = (*points).max(3) as usize;
			let (ro, ri) = (outer.at(frame), inner.at(frame));
			let mut pb = PathBuilder::new();
			for i in 0..n * 2 {
				let r = if i % 2 == 0 { ro } else { ri };
				let a = std::f32::consts::PI * i as f32 / n as f32 - std::f32::consts::FRAC_PI_2;
				let (x, y) = (r * a.cos(), r * a.sin());
				if i == 0 {
					pb.move_to(x, y);
				} else {
					pb.line_to(x, y);
				}
			}
			pb.close();
			pb.finish()?
		}
		Geometry::Path(path) => bez_path(path, [0.0, 0.0])?,
	};
	let [x, y] = item.position.at(frame);
	local.transform(tiny_skia::Transform::from_translate(x, y).pre_rotate(item.rotation.at(frame)))
}

/// Draw a shape layer's items (the first item on top).
pub fn render_shapes(items: &[ShapeItem], width: u32, height: u32, frame: i32) -> Image {
	let mut pixmap = Pixmap::new(width, height).expect("non-empty layer");
	for item in items.iter().rev() {
		let Some(path) = shape_path(item, frame) else {
			continue;
		};
		if item.fill_on {
			pixmap.fill_path(
				&path,
				&paint(item.fill.at(frame)),
				FillRule::Winding,
				tiny_skia::Transform::identity(),
				None,
			);
		}
		let stroke_width = item.stroke_width.at(frame);
		if item.stroke_on && stroke_width > 0.0 {
			let stroke = Stroke {
				width: stroke_width,
				line_join: tiny_skia::LineJoin::Round,
				line_cap: tiny_skia::LineCap::Round,
				..Stroke::default()
			};
			pixmap.stroke_path(
				&path,
				&paint(item.stroke.at(frame)),
				&stroke,
				tiny_skia::Transform::identity(),
				None,
			);
		}
	}
	Image::from_pixmap(&pixmap)
}

/// The combined coverage of a layer's masks, or `None` when no mask cuts it.
pub fn mask_coverage(masks: &[Mask], width: u32, height: u32, frame: i32) -> Option<Vec<f32>> {
	let cutting: Vec<&Mask> = masks.iter().filter(|m| m.mode != MaskMode::None).collect();
	let first = cutting.first()?;
	let n = width as usize * height as usize;
	// Like After Effects, start from nothing if the first mask adds, and from
	// the whole layer if it takes away.
	let mut acc = match first.mode {
		MaskMode::Add | MaskMode::Lighten | MaskMode::Difference => vec![0.0f32; n],
		_ => vec![1.0f32; n],
	};
	for mask in cutting {
		let mut cov = vec![0.0f32; n];
		if let (Some(path), Some(mut m)) = (bez_path(&mask.path, [0.0, 0.0]), tiny_skia::Mask::new(width, height)) {
			m.fill_path(&path, FillRule::Winding, true, tiny_skia::Transform::identity());
			for (c, &v) in cov.iter_mut().zip(m.data()) {
				*c = v as f32 / 255.0;
			}
		}
		let feather = mask.feather.at(frame);
		if feather >= 1.0 {
			blur(&mut cov, width as usize, height as usize, feather / 2.0);
		}
		let opacity = (mask.opacity.at(frame) / 100.0).clamp(0.0, 1.0);
		for (a, &c) in acc.iter_mut().zip(&cov) {
			let c = if mask.inverted { 1.0 - c } else { c } * opacity;
			*a = match mask.mode {
				MaskMode::Add => *a + c - *a * c,
				MaskMode::Subtract => *a * (1.0 - c),
				MaskMode::Intersect => *a * c,
				MaskMode::Lighten => a.max(c),
				MaskMode::Darken => a.min(c),
				MaskMode::Difference => (*a - c).abs(),
				MaskMode::None => *a,
			};
		}
	}
	Some(acc)
}

/// Approximate Gaussian blur: three box blurs per axis.
fn blur(buf: &mut [f32], w: usize, h: usize, radius: f32) {
	let r = (radius.round() as usize).max(1);
	let mut tmp = vec![0.0f32; buf.len()];
	for _ in 0..3 {
		box_pass(buf, &mut tmp, w, h, r, 1, w);
		box_pass(&tmp, buf, h, w, r, w, 1);
	}
}

/// One box blur along lines of `len` samples `step` apart, `lines` lines
/// `line_step` apart; edges clamp.
fn box_pass(src: &[f32], dst: &mut [f32], len: usize, lines: usize, r: usize, step: usize, line_step: usize) {
	let norm = 1.0 / (2 * r + 1) as f32;
	for line in 0..lines {
		let at = |i: isize| src[line * line_step + (i.clamp(0, len as isize - 1) as usize) * step];
		let mut sum: f32 = (-(r as isize)..=r as isize).map(at).sum();
		for i in 0..len {
			dst[line * line_step + i * step] = sum * norm;
			sum += at(i as isize + r as isize + 1) - at(i as isize - r as isize);
		}
	}
}

/// Fonts by file, with fallbacks for characters the chosen font lacks.
pub struct Fonts {
	builtin: FontArc,
	fallbacks: Vec<FontArc>,
	files: HashMap<String, Option<FontArc>>,
}

impl Fonts {
	pub fn new() -> Self {
		let builtin = FontArc::try_from_slice(epaint_default_fonts::UBUNTU_LIGHT).expect("bundled font");
		// Japanese and other scripts the built-in font doesn't cover.
		let fallbacks = [
			"/System/Library/Fonts/ヒラギノ角ゴシック W6.ttc",
			"/System/Library/Fonts/Hiragino Sans GB.ttc",
			"C:\\Windows\\Fonts\\YuGothB.ttc",
			"C:\\Windows\\Fonts\\msgothic.ttc",
		]
		.iter()
		.filter_map(|p| load_font(p))
		.collect();
		Self {
			builtin,
			fallbacks,
			files: HashMap::new(),
		}
	}

	fn chain(&mut self, font: Option<&str>) -> Vec<FontArc> {
		let mut chain = Vec::new();
		if let Some(path) = font {
			let loaded = self.files.entry(path.to_string()).or_insert_with(|| load_font(path));
			chain.extend(loaded.clone());
		}
		chain.push(self.builtin.clone());
		chain.extend(self.fallbacks.iter().cloned());
		chain
	}
}

fn load_font(path: &str) -> Option<FontArc> {
	let data = std::fs::read(PathBuf::from(path)).ok()?;
	ab_glyph::FontVec::try_from_vec_and_index(data, 0)
		.ok()
		.map(FontArc::new)
}

/// Lay a text layer out: each glyph, positioned in layer pixels, with the
/// first font in the fallback chain that has it.
fn layout_text(text: &Text, frame: i32, fonts: &mut Fonts) -> Vec<(FontArc, ab_glyph::Glyph)> {
	let chain = fonts.chain(text.font.as_deref());
	let scale = PxScale::from(text.size.at(frame).max(1.0));
	let tracking = text.tracking.at(frame);
	let [ox, oy] = text.position.at(frame);
	let primary = chain[0].as_scaled(scale);
	let line_height = primary.height() + primary.line_gap();

	let mut out = Vec::new();
	for (line_no, line) in text.text.lines().enumerate() {
		let glyphs: Vec<(&FontArc, ab_glyph::GlyphId)> = line
			.chars()
			.map(|ch| {
				chain
					.iter()
					.find_map(|f| Some((f, f.glyph_id(ch))).filter(|(_, id)| id.0 != 0))
					.unwrap_or((&chain[0], chain[0].glyph_id(ch)))
			})
			.collect();
		let advance = |(f, id): &(&FontArc, ab_glyph::GlyphId)| f.as_scaled(scale).h_advance(*id) + tracking;
		let width: f32 = glyphs.iter().map(advance).sum::<f32>() - if glyphs.is_empty() { 0.0 } else { tracking };
		let mut x = ox
			- match text.align {
				TextAlign::Left => 0.0,
				TextAlign::Center => width / 2.0,
				TextAlign::Right => width,
			};
		let y = oy + line_no as f32 * line_height;
		for g in &glyphs {
			out.push((g.0.clone(), g.1.with_scale_and_position(scale, point(x, y))));
			x += advance(g);
		}
	}
	out
}

/// Draw a text layer.
pub fn render_text(text: &Text, width: u32, height: u32, frame: i32, fonts: &mut Fonts) -> Image {
	let mut coverage = vec![0.0f32; width as usize * height as usize];
	for (font, glyph) in layout_text(text, frame, fonts) {
		let Some(outlined) = font.outline_glyph(glyph) else {
			continue;
		};
		let b = outlined.px_bounds();
		outlined.draw(|gx, gy, c| {
			let (px, py) = (b.min.x as i32 + gx as i32, b.min.y as i32 + gy as i32);
			if px >= 0 && py >= 0 && (px as u32) < width && (py as u32) < height {
				let i = py as usize * width as usize + px as usize;
				coverage[i] = (coverage[i] + c).min(1.0);
			}
		});
	}
	let mut image = Image::filled(width, height, text.color.at(frame));
	image.multiply_alpha(&coverage);
	image
}

/// The inked bounds of a text layer, `[x0, y0, x1, y1]` in layer pixels.
pub fn text_bounds(text: &Text, frame: i32, fonts: &mut Fonts) -> Option<[f32; 4]> {
	layout_text(text, frame, fonts)
		.into_iter()
		.filter_map(|(font, glyph)| font.outline_glyph(glyph).map(|o| o.px_bounds()))
		.map(|r| [r.min.x, r.min.y, r.max.x, r.max.y])
		.reduce(|a, b| [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])])
}

/// Decoded image files, kept across frames.
#[derive(Default)]
pub struct ImageCache {
	images: HashMap<String, Result<Image, String>>,
}

impl ImageCache {
	pub fn get(&mut self, path: &str) -> Result<&Image, String> {
		self.images
			.entry(path.to_string())
			.or_insert_with(|| {
				image::open(path)
					.map(|img| {
						let img = img.to_rgba8();
						let (width, height) = img.dimensions();
						Image {
							width,
							height,
							pixels: img.into_raw(),
						}
					})
					.map_err(|e| format!("{path}: {e}"))
			})
			.as_ref()
			.map_err(Clone::clone)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::model::Anim;

	fn mask(mode: MaskMode, path: BezPath) -> Mask {
		Mask {
			id: 1,
			name: String::new(),
			path,
			mode,
			inverted: false,
			opacity: Anim::new(100.0),
			feather: Anim::new(0.0),
		}
	}

	#[test]
	fn premultiply_round_trips_opaque_and_clear() {
		let img = Image {
			width: 2,
			height: 1,
			pixels: vec![10, 20, 30, 255, 0, 0, 0, 0],
		};
		assert_eq!(Image::from_pixmap(&img.to_pixmap()).pixels, img.pixels);
	}

	#[test]
	fn add_then_subtract_masks() {
		let add = mask(MaskMode::Add, BezPath::rect(0.0, 0.0, 10.0, 10.0));
		let sub = mask(MaskMode::Subtract, BezPath::rect(0.0, 0.0, 5.0, 10.0));
		let cov = mask_coverage(&[add, sub], 20, 10, 0).unwrap();
		assert_eq!(cov[5 * 20 + 2], 0.0, "subtracted");
		assert_eq!(cov[5 * 20 + 7], 1.0, "added");
		assert_eq!(cov[5 * 20 + 15], 0.0, "outside");
	}

	#[test]
	fn none_masks_dont_cut() {
		assert!(mask_coverage(&[mask(MaskMode::None, BezPath::rect(0.0, 0.0, 1.0, 1.0))], 4, 4, 0).is_none());
	}

	#[test]
	fn feather_softens_the_edge() {
		let mut m = mask(MaskMode::Add, BezPath::rect(10.0, 0.0, 30.0, 10.0));
		m.feather = Anim::new(6.0);
		let cov = mask_coverage(&[m], 40, 10, 0).unwrap();
		let edge = cov[5 * 40 + 10];
		assert!(edge > 0.2 && edge < 0.8, "{edge}");
	}

	#[test]
	fn text_draws_ink() {
		let text = Text {
			text: "Hi".into(),
			font: None,
			size: Anim::new(40.0),
			color: Anim::new([1.0; 4]),
			position: Anim::new([10.0, 50.0]),
			align: TextAlign::Left,
			tracking: Anim::new(0.0),
		};
		let img = render_text(&text, 100, 60, 0, &mut Fonts::new());
		assert!(img.pixels.chunks(4).filter(|p| p[3] > 128).count() > 50);
	}
}
