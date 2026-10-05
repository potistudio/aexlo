//! Mask paths an embedder attaches to a [`PluginInstance`](crate::PluginInstance),
//! served to the plugin through the "PF Path Query Suite" and "PF Path Data
//! Suite" (see [`PluginInstance::set_mask_paths`](crate::PluginInstance::set_mask_paths)).

/// How a mask combines with the others (`PF_MaskMode_*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MaskMode {
	None,
	#[default]
	Add,
	Subtract,
	Intersect,
	Lighten,
	Darken,
	Difference,
	/// Real addition rather than screen (not exposed in AE's UI).
	Accum,
}

impl MaskMode {
	pub(crate) fn to_sdk(self) -> after_effects_sys::PF_MaskMode {
		use after_effects_sys::*;
		(match self {
			MaskMode::None => PF_MaskMode_NONE,
			MaskMode::Add => PF_MaskMode_ADD,
			MaskMode::Subtract => PF_MaskMode_SUBTRACT,
			MaskMode::Intersect => PF_MaskMode_INTERSECT,
			MaskMode::Lighten => PF_MaskMode_LIGHTEN,
			MaskMode::Darken => PF_MaskMode_DARKEN,
			MaskMode::Difference => PF_MaskMode_DIFFERENCE,
			MaskMode::Accum => PF_MaskMode_ACCUM,
		}) as PF_MaskMode
	}
}

/// One Bezier vertex of a mask, in layer pixel coordinates. The tangents are
/// relative to the vertex, as in `AEGP_MaskVertex`: the curve leaves the vertex
/// toward `(x + tan_out_x, y + tan_out_y)` and arrives from
/// `(x + tan_in_x, y + tan_in_y)`. Zero tangents give straight segments.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MaskVertex {
	pub x: f64,
	pub y: f64,
	pub tan_in_x: f64,
	pub tan_in_y: f64,
	pub tan_out_x: f64,
	pub tan_out_y: f64,
}

impl MaskVertex {
	/// A corner vertex (no tangents) at `(x, y)`.
	pub fn corner(x: f64, y: f64) -> Self {
		Self {
			x,
			y,
			..Default::default()
		}
	}
}

/// A mask path on the effect's layer.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MaskPath {
	/// The `PF_PathID` plugins reference this mask by (e.g. from a
	/// `PF_Param_PATH` parameter). Should be unique per instance.
	pub id: u32,
	/// Shown to plugins via `PF_PathGetName`, truncated to 31 bytes.
	pub name: String,
	pub vertices: Vec<MaskVertex>,
	/// Whether the last vertex connects back to the first.
	pub closed: bool,
	pub inverted: bool,
	pub mode: MaskMode,
}

/// A point `(x, y)`.
pub(crate) type Point = (f64, f64);

impl MaskPath {
	/// Number of segments: one per vertex when closed, one fewer when open.
	pub(crate) fn segment_count(&self) -> usize {
		match self.vertices.len() {
			0 => 0,
			n if self.closed => n,
			n => n - 1,
		}
	}

	/// Vertex `i` in `0..=segment_count()`; for closed paths the last index
	/// wraps to vertex 0, per `PF_PathVertexInfo`.
	pub(crate) fn vertex(&self, i: usize) -> Option<&MaskVertex> {
		if self.vertices.is_empty() || i > self.segment_count() {
			return None;
		}
		self.vertices.get(i % self.vertices.len())
	}

	/// The cubic Bezier control points of segment `seg`.
	pub(crate) fn segment(&self, seg: usize) -> Option<[Point; 4]> {
		if seg >= self.segment_count() {
			return None;
		}
		let a = self.vertex(seg)?;
		let b = self.vertex(seg + 1)?;
		Some([
			(a.x, a.y),
			(a.x + a.tan_out_x, a.y + a.tan_out_y),
			(b.x + b.tan_in_x, b.y + b.tan_in_y),
			(b.x, b.y),
		])
	}
}

/// Evaluate a cubic Bezier at `t`.
pub(crate) fn bezier_point(p: &[Point; 4], t: f64) -> Point {
	let u = 1.0 - t;
	let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
	(
		a * p[0].0 + b * p[1].0 + c * p[2].0 + d * p[3].0,
		a * p[0].1 + b * p[1].1 + c * p[2].1 + d * p[3].1,
	)
}

/// First derivative of a cubic Bezier with respect to `t`.
pub(crate) fn bezier_deriv(p: &[Point; 4], t: f64) -> Point {
	let u = 1.0 - t;
	let (a, b, c) = (3.0 * u * u, 6.0 * u * t, 3.0 * t * t);
	(
		a * (p[1].0 - p[0].0) + b * (p[2].0 - p[1].0) + c * (p[3].0 - p[2].0),
		a * (p[1].1 - p[0].1) + b * (p[2].1 - p[1].1) + c * (p[3].1 - p[2].1),
	)
}

fn speed(p: &[Point; 4], t: f64) -> f64 {
	let (dx, dy) = bezier_deriv(p, t);
	dx.hypot(dy)
}

/// Arc length of the curve between `a` and `b` (5-point Gauss-Legendre).
fn arc_length(p: &[Point; 4], a: f64, b: f64) -> f64 {
	const NODES: [(f64, f64); 5] = [
		(0.0, 0.568_888_888_888_888_9),
		(-0.538_469_310_105_683_1, 0.478_628_670_499_366_5),
		(0.538_469_310_105_683_1, 0.478_628_670_499_366_5),
		(-0.906_179_845_938_664, 0.236_926_885_056_189_1),
		(0.906_179_845_938_664, 0.236_926_885_056_189_1),
	];
	let (mid, half) = ((a + b) / 2.0, (b - a) / 2.0);
	half * NODES.iter().map(|&(x, w)| w * speed(p, mid + half * x)).sum::<f64>()
}

/// Arc-length table of one segment: cumulative lengths at `n + 1` uniformly
/// spaced `t` values, used to map a distance along the segment back to its
/// curve parameter.
#[derive(Debug, Clone)]
pub(crate) struct ArcTable {
	points: [Point; 4],
	cumulative: Vec<f64>,
}

impl ArcTable {
	pub(crate) fn new(p: &[Point; 4], samples: usize) -> Self {
		let n = samples.max(1);
		let mut cumulative = Vec::with_capacity(n + 1);
		cumulative.push(0.0);
		for i in 1..=n {
			let last = *cumulative.last().unwrap();
			cumulative.push(last + arc_length(p, (i - 1) as f64 / n as f64, i as f64 / n as f64));
		}
		Self { points: *p, cumulative }
	}

	pub(crate) fn length(&self) -> f64 {
		*self.cumulative.last().unwrap()
	}

	/// The curve parameter at distance `len` along the segment (clamped):
	/// locate the table interval, then solve `arc(t0, t) = len - lo` within it
	/// by Newton steps, bisecting whenever a step leaves the interval or the
	/// curve stalls (zero speed at a cusp or a tangent-less corner).
	pub(crate) fn t_at(&self, len: f64) -> f64 {
		let n = self.cumulative.len() - 1;
		let len = len.clamp(0.0, self.length());
		let i = self.cumulative.partition_point(|&c| c < len).clamp(1, n);
		let (lo, hi) = (self.cumulative[i - 1], self.cumulative[i]);
		let (mut a, mut b) = ((i - 1) as f64 / n as f64, i as f64 / n as f64);
		let t0 = a;
		let mut t = if hi > lo {
			a + (b - a) * (len - lo) / (hi - lo)
		} else {
			a
		};
		for _ in 0..32 {
			let err = lo + arc_length(&self.points, t0, t) - len;
			if err.abs() < 1e-10 {
				break;
			}
			if err > 0.0 {
				b = t;
			} else {
				a = t;
			}
			let v = speed(&self.points, t);
			let next = t - err / v;
			t = if v > 1e-12 && next > a && next < b {
				next
			} else {
				(a + b) / 2.0
			};
		}
		t
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn square(closed: bool) -> MaskPath {
		MaskPath {
			vertices: vec![
				MaskVertex::corner(0.0, 0.0),
				MaskVertex::corner(10.0, 0.0),
				MaskVertex::corner(10.0, 10.0),
				MaskVertex::corner(0.0, 10.0),
			],
			closed,
			..Default::default()
		}
	}

	#[test]
	fn segments_wrap_only_when_closed() {
		assert_eq!(square(true).segment_count(), 4);
		assert_eq!(square(false).segment_count(), 3);
		assert_eq!(square(true).vertex(4).map(|v| v.x), Some(0.0));
		assert!(square(false).vertex(4).is_none());
		assert_eq!(square(true).segment(3).unwrap()[3], (0.0, 0.0));
	}

	#[test]
	fn arc_length_maps_distance_to_parameter() {
		let seg = square(false).segment(0).unwrap();
		let table = ArcTable::new(&seg, 32);
		assert!((table.length() - 10.0).abs() < 1e-9);
		let p = bezier_point(&seg, table.t_at(2.5));
		assert!((p.0 - 2.5).abs() < 1e-6, "{p:?}");
	}

	#[test]
	fn quarter_circle_length() {
		// Standard cubic approximation of a unit quarter circle.
		let k = 0.552_284_749_8;
		let path = MaskPath {
			vertices: vec![
				MaskVertex {
					x: 1.0,
					y: 0.0,
					tan_out_y: k,
					..Default::default()
				},
				MaskVertex {
					x: 0.0,
					y: 1.0,
					tan_in_x: k,
					..Default::default()
				},
			],
			..Default::default()
		};
		let table = ArcTable::new(&path.segment(0).unwrap(), 256);
		assert!((table.length() - std::f64::consts::FRAC_PI_2).abs() < 1e-3);
	}
}
