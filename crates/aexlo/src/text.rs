//! Text layers: the effect's layer as After Effects shows it when the effect
//! sits on a text layer.
//!
//! Plugins that work on text (per-character gradients, animators, ...) ask
//! `AEGP_GetLayerObjectType` whether their layer is a text layer and then read
//! its glyph outlines through the "AEGP Text Layer Suite"
//! (`AEGP_GetNewTextOutlines`) and the "PF Path Data Suite". A [`TextLayer`]
//! is those outlines; [`PluginInstance::set_text_layer`] puts it on the
//! effect's layer.
//!
//! The pixels the plugin receives are still the input layer's. In After
//! Effects they are the rendered text, so a host normally sets the input
//! layer to [`TextLayer::rasterize`] as well.
//!
//! [`PluginInstance::set_text_layer`]: crate::PluginInstance::set_text_layer

use std::path::{Path, PathBuf};

use ab_glyph::{Font, FontVec, OutlineCurve};

use crate::mask::{MaskPath, MaskVertex};
use crate::pixel::{AnyLayer, PixelDepthKind};
use crate::{AexloError, Result};

/// How [`TextLayer::layout`] sets a string.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct TextStyle {
	/// A TrueType or OpenType font file (the first face of a collection).
	/// `None` picks a sans-serif face installed with the operating system.
	pub font: Option<PathBuf>,
	/// Font size in pixels: the em square, as After Effects' Character panel
	/// gives it.
	pub size: f32,
	/// Extra space after every character, in thousandths of an em (After
	/// Effects' tracking).
	pub tracking: f32,
	/// Distance between baselines, in pixels. `None` uses the font's own
	/// line spacing.
	pub leading: Option<f32>,
	/// Where the text block starts, in layer pixels: the left edge of its
	/// widest line, on the first line's baseline. Lines are centred on the
	/// widest, so a narrower first line starts to the right of it. `None`
	/// centres the block on the layer.
	pub position: Option<(f32, f32)>,
}

impl Default for TextStyle {
	fn default() -> Self {
		Self {
			font: None,
			size: 72.0,
			tracking: 0.0,
			leading: None,
			position: None,
		}
	}
}

impl TextStyle {
	/// The default style at `size` pixels.
	pub fn sized(size: f32) -> Self {
		Self {
			size,
			..Self::default()
		}
	}
}

/// The text on the effect's layer, as closed glyph outlines in layer pixels.
///
/// One [`MaskPath`] per glyph contour, in reading order, the way
/// `AEGP_GetIndexedTextOutline` hands them out: an `o` is two outlines, a
/// space none.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TextLayer {
	text: String,
	outlines: Vec<MaskPath>,
}

impl TextLayer {
	/// A text layer whose outlines the caller built. `text` is only kept for
	/// reference ([`Self::text`]).
	pub fn from_outlines(text: impl Into<String>, outlines: Vec<MaskPath>) -> Self {
		Self {
			text: text.into(),
			outlines,
		}
	}

	/// Set `text` in `style` on a `width` x `height` layer. Lines break at
	/// `\n` and are centred on each other, as a centred paragraph.
	///
	/// # Errors
	/// [`AexloError::Font`] when the font file cannot be read or parsed, or
	/// when `style.font` is `None` and the system has none of the usual
	/// sans-serif faces.
	pub fn layout(text: &str, style: &TextStyle, width: u32, height: u32) -> Result<Self> {
		let loaded;
		let font = match &style.font {
			Some(path) => {
				loaded = load_font(path)?;
				&loaded
			}
			None => crate::suites::system_font()
				.ok_or_else(|| AexloError::Font("no system sans-serif font found; pass a font file".into()))?,
		};
		Ok(Self {
			text: text.to_string(),
			outlines: set_text(font, text, style, width, height),
		})
	}

	/// The string this layer shows.
	pub fn text(&self) -> &str {
		&self.text
	}

	/// The glyph outlines, in layer pixels.
	pub fn outlines(&self) -> &[MaskPath] {
		&self.outlines
	}

	/// The smallest rectangle `(left, top, right, bottom)` holding every
	/// outline's vertices and tangent handles, or `None` without outlines.
	pub fn bounds(&self) -> Option<(f64, f64, f64, f64)> {
		let mut points = self.outlines.iter().flat_map(|path| {
			path.vertices.iter().flat_map(|v| {
				[
					(v.x, v.y),
					(v.x + v.tan_in_x, v.y + v.tan_in_y),
					(v.x + v.tan_out_x, v.y + v.tan_out_y),
				]
			})
		});
		let first = points.next()?;
		Some(
			points.fold((first.0, first.1, first.0, first.1), |(l, t, r, b), (x, y)| {
				(l.min(x), t.min(y), r.max(x), b.max(y))
			}),
		)
	}

	/// The text filled with `rgba` (straight, 0..1) on a transparent
	/// `width` x `height` layer of depth `kind`: what After Effects hands an
	/// effect on a text layer as its input.
	pub fn rasterize(&self, kind: PixelDepthKind, width: u32, height: u32, rgba: [f32; 4]) -> AnyLayer {
		let mut layer = AnyLayer::filled(kind, width, height, [0.0; 4]);
		let coverage = self.coverage(width, height);
		layer.set_from_unit_rgba(coverage.into_iter().map(|c| [rgba[0], rgba[1], rgba[2], rgba[3] * c]));
		layer
	}

	/// Anti-aliased coverage (0..1) of the filled outlines, row-major.
	fn coverage(&self, width: u32, height: u32) -> Vec<f32> {
		let pixels = width as usize * height as usize;
		let Some(mut mask) = tiny_skia::Mask::new(width, height) else {
			return vec![0.0; pixels];
		};
		let mut builder = tiny_skia::PathBuilder::new();
		for path in &self.outlines {
			let Some(first) = path.vertices.first() else {
				continue;
			};
			builder.move_to(first.x as f32, first.y as f32);
			for seg in 0..path.segment_count() {
				let [_, c1, c2, end] = path.segment(seg).expect("segment index in range");
				builder.cubic_to(
					c1.0 as f32,
					c1.1 as f32,
					c2.0 as f32,
					c2.1 as f32,
					end.0 as f32,
					end.1 as f32,
				);
			}
			builder.close();
		}
		if let Some(path) = builder.finish() {
			mask.fill_path(
				&path,
				tiny_skia::FillRule::Winding,
				true,
				tiny_skia::Transform::identity(),
			);
		}
		mask.data().iter().map(|&a| f32::from(a) / 255.0).collect()
	}
}

fn load_font(path: &Path) -> Result<FontVec> {
	let bytes =
		std::fs::read(path).map_err(|err| AexloError::Font(format!("reading font {}: {err}", path.display())))?;
	FontVec::try_from_vec(bytes).map_err(|err| AexloError::Font(format!("parsing font {}: {err}", path.display())))
}

/// Lay out `text` and return its contours in layer pixels.
fn set_text(font: &FontVec, text: &str, style: &TextStyle, width: u32, height: u32) -> Vec<MaskPath> {
	let units_per_em = font.units_per_em().unwrap_or(1000.0);
	let scale = style.size.max(0.0) / units_per_em;
	let natural_leading = (font.ascent_unscaled() - font.descent_unscaled() + font.line_gap_unscaled()) * scale;
	let leading = style.leading.unwrap_or(natural_leading);
	let tracking = style.tracking / 1000.0 * style.size;

	// Pen positions of every glyph, relative to the first baseline's start.
	let lines: Vec<&str> = text.split('\n').collect();
	let mut glyphs = Vec::new();
	let mut widths = Vec::with_capacity(lines.len());
	for (row, line) in lines.iter().enumerate() {
		let mut caret = 0.0f32;
		let mut previous = None;
		let start = glyphs.len();
		for ch in line.chars() {
			let id = font.glyph_id(ch);
			if let Some(prev) = previous {
				caret += font.kern_unscaled(prev, id) * scale;
			}
			glyphs.push((id, caret, row as f32 * leading));
			caret += font.h_advance_unscaled(id) * scale + tracking;
			previous = Some(id);
		}
		// Tracking only goes between characters.
		let line_width = if glyphs.len() > start { caret - tracking } else { 0.0 };
		widths.push((start..glyphs.len(), line_width));
	}

	let block_width = widths.iter().map(|(_, w)| *w).fold(0.0, f32::max);
	let (origin_x, origin_y) = style.position.unwrap_or_else(|| {
		// Centre the block between the first line's ascender and the last
		// line's descender.
		let ascent = font.ascent_unscaled() * scale;
		let descent = -font.descent_unscaled() * scale;
		let block_height = ascent + leading * (lines.len() - 1) as f32 + descent;
		(
			(width as f32 - block_width) / 2.0,
			(height as f32 - block_height) / 2.0 + ascent,
		)
	});
	for (range, line_width) in widths {
		// Centred paragraph: each line centred on the widest.
		let shift = (block_width - line_width) / 2.0;
		for glyph in &mut glyphs[range] {
			glyph.1 += shift;
		}
	}

	let mut outlines = Vec::new();
	for (id, x, y) in glyphs {
		let Some(outline) = font.outline(id) else {
			continue;
		};
		let to_layer = |p: ab_glyph::Point| {
			(
				f64::from(origin_x + x + p.x * scale),
				f64::from(origin_y + y - p.y * scale),
			)
		};
		for contour in contours(&outline.curves) {
			let path = contour_path(contour, to_layer, outlines.len() as u32 + 1);
			if path.vertices.len() >= 2 {
				outlines.push(path);
			}
		}
	}
	outlines
}

/// Split a glyph's curves into contours: a contour ends where the next curve
/// does not start at the previous one's end.
fn contours(curves: &[OutlineCurve]) -> Vec<&[OutlineCurve]> {
	let mut out = Vec::new();
	let mut start = 0;
	for i in 1..=curves.len() {
		let breaks = i == curves.len() || end_of(&curves[i - 1]) != start_of(&curves[i]);
		if breaks {
			out.push(&curves[start..i]);
			start = i;
		}
	}
	out
}

fn start_of(curve: &OutlineCurve) -> ab_glyph::Point {
	match *curve {
		OutlineCurve::Line(p, _) | OutlineCurve::Quad(p, _, _) | OutlineCurve::Cubic(p, _, _, _) => p,
	}
}

fn end_of(curve: &OutlineCurve) -> ab_glyph::Point {
	match *curve {
		OutlineCurve::Line(_, p) | OutlineCurve::Quad(_, _, p) | OutlineCurve::Cubic(_, _, _, p) => p,
	}
}

/// One closed contour as a mask path: a vertex where each curve starts, with
/// the curve's control points as the tangents on either side. Quadratic
/// curves are raised to cubics.
fn contour_path(curves: &[OutlineCurve], to_layer: impl Fn(ab_glyph::Point) -> (f64, f64), id: u32) -> MaskPath {
	let mut vertices: Vec<MaskVertex> = Vec::with_capacity(curves.len() + 1);
	for curve in curves {
		let (p0, c1, c2, p3) = match *curve {
			OutlineCurve::Line(a, b) => {
				let (a, b) = (to_layer(a), to_layer(b));
				(a, a, b, b)
			}
			OutlineCurve::Quad(a, q, b) => {
				let (a, q, b) = (to_layer(a), to_layer(q), to_layer(b));
				let lerp =
					|from: (f64, f64)| (from.0 + (q.0 - from.0) * 2.0 / 3.0, from.1 + (q.1 - from.1) * 2.0 / 3.0);
				(a, lerp(a), lerp(b), b)
			}
			OutlineCurve::Cubic(a, c, d, b) => (to_layer(a), to_layer(c), to_layer(d), to_layer(b)),
		};
		match vertices.last_mut() {
			Some(last) if (last.x, last.y) == p0 => {}
			_ => vertices.push(MaskVertex::corner(p0.0, p0.1)),
		}
		let from = vertices.last_mut().expect("pushed above");
		from.tan_out_x = c1.0 - p0.0;
		from.tan_out_y = c1.1 - p0.1;
		vertices.push(MaskVertex {
			x: p3.0,
			y: p3.1,
			tan_in_x: c2.0 - p3.0,
			tan_in_y: c2.1 - p3.1,
			..Default::default()
		});
	}
	// The contour closes on its first vertex: fold the closing vertex's
	// incoming tangent into it instead of keeping a duplicate.
	if vertices.len() >= 2 {
		let (first, last) = (vertices[0], vertices[vertices.len() - 1]);
		if (first.x, first.y) == (last.x, last.y) {
			vertices[0].tan_in_x = last.tan_in_x;
			vertices[0].tan_in_y = last.tan_in_y;
			vertices.pop();
		}
	}
	MaskPath {
		id,
		name: format!("Text Outline {id}"),
		vertices,
		closed: true,
		..Default::default()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn pt(x: f32, y: f32) -> ab_glyph::Point {
		ab_glyph::point(x, y)
	}

	#[test]
	fn a_square_contour_closes_on_its_first_vertex() {
		let curves = [
			OutlineCurve::Line(pt(0.0, 0.0), pt(10.0, 0.0)),
			OutlineCurve::Line(pt(10.0, 0.0), pt(10.0, 10.0)),
			OutlineCurve::Line(pt(10.0, 10.0), pt(0.0, 10.0)),
			OutlineCurve::Line(pt(0.0, 10.0), pt(0.0, 0.0)),
			// A second contour.
			OutlineCurve::Line(pt(2.0, 2.0), pt(4.0, 2.0)),
			OutlineCurve::Line(pt(4.0, 2.0), pt(2.0, 4.0)),
			OutlineCurve::Line(pt(2.0, 4.0), pt(2.0, 2.0)),
		];
		let parts = contours(&curves);
		assert_eq!(parts.iter().map(|c| c.len()).collect::<Vec<_>>(), [4, 3]);

		let path = contour_path(parts[0], |p| (f64::from(p.x), f64::from(-p.y)), 1);
		assert!(path.closed);
		assert_eq!(path.vertices.len(), 4);
		assert_eq!(path.segment_count(), 4);
		assert_eq!(path.segment(3).unwrap()[3], (0.0, 0.0));
		assert!(path.vertices.iter().all(|v| v.tan_in_x == 0.0 && v.tan_out_y == 0.0));
	}

	#[test]
	fn quadratics_become_cubics_through_the_same_control_point() {
		let curves = [
			OutlineCurve::Quad(pt(0.0, 0.0), pt(3.0, 3.0), pt(6.0, 0.0)),
			OutlineCurve::Line(pt(6.0, 0.0), pt(0.0, 0.0)),
		];
		let path = contour_path(&curves, |p| (f64::from(p.x), f64::from(p.y)), 1);
		assert_eq!(path.vertices.len(), 2);
		let [p0, c1, c2, p3] = path.segment(0).unwrap();
		assert_eq!((p0, p3), ((0.0, 0.0), (6.0, 0.0)));
		assert!((c1.0 - 2.0).abs() < 1e-9 && (c1.1 - 2.0).abs() < 1e-9, "{c1:?}");
		assert!((c2.0 - 4.0).abs() < 1e-9 && (c2.1 - 2.0).abs() < 1e-9, "{c2:?}");
	}

	#[test]
	fn rasterize_fills_inside_the_outlines() {
		let square = MaskPath {
			vertices: vec![
				MaskVertex::corner(2.0, 2.0),
				MaskVertex::corner(6.0, 2.0),
				MaskVertex::corner(6.0, 6.0),
				MaskVertex::corner(2.0, 6.0),
			],
			closed: true,
			..Default::default()
		};
		let text = TextLayer::from_outlines("■", vec![square]);
		assert_eq!(text.bounds(), Some((2.0, 2.0, 6.0, 6.0)));
		let layer = text.rasterize(PixelDepthKind::U8, 8, 8, [1.0, 0.0, 0.0, 1.0]);
		let inside = layer.unit_rgba(4 * 8 + 4).unwrap();
		let outside = layer.unit_rgba(0).unwrap();
		assert_eq!(inside, [1.0, 0.0, 0.0, 1.0]);
		assert_eq!(outside[3], 0.0);
	}

	#[test]
	fn layout_centres_the_text_with_a_system_font() {
		let Some(_) = crate::suites::system_font() else {
			return; // No system font on this machine: nothing to lay out with.
		};
		let text = TextLayer::layout("Hi o", &TextStyle::sized(40.0), 200, 100).unwrap();
		// H (1 contour), i (2), space (0), o (2).
		assert_eq!(text.outlines().len(), 5);
		let (l, t, r, b) = text.bounds().unwrap();
		assert!(l > 0.0 && r < 200.0 && t > 0.0 && b < 100.0, "{l} {t} {r} {b}");
		assert!(((l + r) / 2.0 - 100.0).abs() < 10.0, "not centred: {l}..{r}");
	}
}
