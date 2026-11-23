//! `PF_WorldTransformSuite1` and the legacy callbacks of the same names, at 8,
//! 16 and 32 bits per channel.
//!
//! The arithmetic is written once, in each depth's own units. A call's worlds
//! must have one depth (a mask has its own). A depth that cannot be told, or
//! two in one call, is refused and reported rather than read as 8-bit.
//!
//! The results have not been compared with After Effects. In 32 bpc, values
//! outside `0..=1` pass through copying, filtering, normal compositing and
//! additive modes; the other blend modes keep their formulas for `0..=1`.

use crate::core::diagnostics::diag;
use crate::sys::*;
use rayon::prelude::*;
use std::os::raw::c_void;

// ============================================================================
// Shared pixel-access / compositing helpers
//
// Channels are `[alpha, red, green, blue]` in the depth's own units, so that
// 8-bit rounding does not depend on a trip through 0..1.
// ============================================================================

/// A pixel of one depth.
trait Channels: Copy + 'static {
	/// A full channel.
	const MAX: f64;
	/// Whether values outside `0..=MAX` are kept.
	const HDR: bool;

	fn channels(self) -> [f64; 4];

	/// Round and clamp `channels` into a pixel, as far as the depth needs it.
	fn store(channels: [f64; 4]) -> Self;
}

macro_rules! integer_channels {
	($pixel:ty, $channel:ty, $max:expr) => {
		impl Channels for $pixel {
			const MAX: f64 = $max;
			const HDR: bool = false;

			#[inline]
			fn channels(self) -> [f64; 4] {
				[self.alpha, self.red, self.green, self.blue].map(|v| v as f64)
			}

			#[inline]
			fn store(channels: [f64; 4]) -> Self {
				let [alpha, red, green, blue] = channels.map(|v| v.round().clamp(0.0, $max) as $channel);
				Self {
					alpha,
					red,
					green,
					blue,
				}
			}
		}
	};
}

integer_channels!(PF_Pixel8, u8, PF_MAX_CHAN8 as f64);
integer_channels!(PF_Pixel16, u16, PF_MAX_CHAN16 as f64);

impl Channels for PF_PixelFloat {
	const MAX: f64 = 1.0;
	const HDR: bool = true;

	#[inline]
	fn channels(self) -> [f64; 4] {
		[self.alpha, self.red, self.green, self.blue].map(f64::from)
	}

	#[inline]
	fn store(channels: [f64; 4]) -> Self {
		let [alpha, red, green, blue] = channels.map(|v| v as f32);
		Self {
			alpha,
			red,
			green,
			blue,
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Depth {
	Eight,
	Sixteen,
	Float,
}

impl Depth {
	fn bytes(self) -> usize {
		match self {
			Self::Eight => std::mem::size_of::<PF_Pixel8>(),
			Self::Sixteen => std::mem::size_of::<PF_Pixel16>(),
			Self::Float => std::mem::size_of::<PF_PixelFloat>(),
		}
	}
}

/// `world`'s depth: what it was allocated as or, failing that, what its DEEP
/// flag says. Worlds of the plugin's own that say neither are 8-bit.
pub(super) fn depth_of(world: &PF_EffectWorld) -> Result<Depth, String> {
	// SAFETY: a reference is a valid world.
	let format = unsafe { super::world::world_pixel_format(world) };
	let depth = if format == PF_PixelFormat_ARGB128 as u32 {
		Depth::Float
	} else if format == PF_PixelFormat_ARGB64 as u32 {
		Depth::Sixteen
	} else if format != PF_PixelFormat_ARGB32 as u32 {
		return Err(format!("pixel format {format:#x}"));
	} else if world.world_flags & PF_WorldFlag_DEEP != 0 {
		Depth::Sixteen
	} else {
		Depth::Eight
	};
	// Reading deeper than a row holds would leave the buffer.
	let row = world.width.max(0) as usize * depth.bytes();
	if depth != Depth::Eight && !world.data.is_null() && (world.rowbytes.unsigned_abs() as usize) < row {
		return Err(format!("{depth:?} pixels in rows of {} bytes", world.rowbytes));
	}
	Ok(depth)
}

/// The depth all of `worlds` have, or the refusal reported.
fn common_depth(name: &str, worlds: &[&PF_EffectWorld]) -> Result<Depth, PF_Err> {
	let refuse = |detail: String| {
		crate::observe::unsupported(
			crate::observe::UnsupportedKind::Declined,
			format!("PF_WorldTransformSuite1/{name}"),
			detail,
		);
		PF_Err_BAD_CALLBACK_PARAM
	};
	let mut common = None;
	for world in worlds {
		let depth = depth_of(world).map_err(refuse)?;
		if let Some(first) = common
			&& first != depth
		{
			let all: Vec<String> = worlds
				.iter()
				.map(|w| {
					let known = super::world::allocated_format(w).is_some();
					let (width, height, row) = (w.width, w.height, w.rowbytes);
					format!(
						"{width}x{height} rows of {row} flags {:#x} known {known}",
						w.world_flags
					)
				})
				.collect();
			let all = all.join("; ");
			return Err(refuse(format!("worlds of two depths, {first:?} and {depth:?} ({all})")));
		}
		common = Some(depth);
	}
	common.ok_or(PF_Err_BAD_CALLBACK_PARAM)
}

/// Call the `P`-generic `$f` at `$depth`.
macro_rules! by_depth {
	($depth:expr, $f:ident($($arg:expr),* $(,)?)) => {
		match $depth {
			Depth::Eight => $f::<PF_Pixel8>($($arg),*),
			Depth::Sixteen => $f::<PF_Pixel16>($($arg),*),
			Depth::Float => $f::<PF_PixelFloat>($($arg),*),
		}
	};
}

/// A `Copy`/`Send`/`Sync` view of a `PF_EffectWorld`'s pixel buffer, addressed
/// by raw `usize` rather than a borrowed reference. Mirrors the pattern
/// `iterate_8_sys` already uses to move buffer access across rayon's
/// worker threads without fighting Rust's aliasing rules: each pixel touched
/// by the parallel iteration is disjoint, so concurrent raw-pointer access is
/// sound even though a `&mut PF_EffectWorld` could not be shared this way.
struct RawWorld<P> {
	addr: usize,
	rowbytes: isize,
	width: i32,
	height: i32,
	// A `fn` type, so that the view is `Send` and `Sync` whatever `P` is.
	pixel: std::marker::PhantomData<fn() -> P>,
}

impl<P> Clone for RawWorld<P> {
	fn clone(&self) -> Self {
		*self
	}
}

impl<P> Copy for RawWorld<P> {}

impl<P: Channels> RawWorld<P> {
	fn from(world: &PF_EffectWorld) -> Self {
		Self {
			addr: world.data as usize,
			rowbytes: world.rowbytes as isize,
			width: world.width,
			height: world.height,
			pixel: std::marker::PhantomData,
		}
	}

	#[inline]
	fn at(&self, x: i32, y: i32) -> usize {
		let offset = y as isize * self.rowbytes + x as isize * std::mem::size_of::<P>() as isize;
		self.addr.wrapping_add_signed(offset)
	}

	/// Reads the pixel at `(x, y)`, or transparent black if it falls outside
	/// the buffer (this is what gives `convolve`'s "transparent borders" mode
	/// and off-screen mask/transform samples their fall-through behavior).
	#[inline]
	unsafe fn read(&self, x: i32, y: i32) -> P {
		if self.addr == 0 || x < 0 || y < 0 || x >= self.width || y >= self.height {
			return P::store([0.0; 4]);
		}
		// SAFETY: inside the world; rows need not be aligned for the pixel.
		unsafe { (self.at(x, y) as *const P).read_unaligned() }
	}

	/// Like [`read`](Self::read), but clamps out-of-range coordinates to the
	/// nearest edge pixel instead of returning transparent black, for
	/// `PF_KernelFlag_REPLICATE_BORDERS`.
	#[inline]
	unsafe fn read_edge(&self, x: i32, y: i32, replicate: bool) -> P {
		if replicate {
			let cx = x.clamp(0, (self.width - 1).max(0));
			let cy = y.clamp(0, (self.height - 1).max(0));
			unsafe { self.read(cx, cy) }
		} else {
			unsafe { self.read(x, y) }
		}
	}

	#[inline]
	unsafe fn write(&self, x: i32, y: i32, pixel: P) {
		if self.addr == 0 || x < 0 || y < 0 || x >= self.width || y >= self.height {
			return;
		}
		// SAFETY: as in `read`.
		unsafe { (self.at(x, y) as *mut P).write_unaligned(pixel) };
	}

	/// The straight pixel at `(x, y)` on a 0..1 scale, transparent outside.
	#[inline]
	unsafe fn straight(&self, x: i32, y: i32) -> (f64, [f64; 3]) {
		let [a, r, g, b] = unsafe { self.read(x, y) }.channels().map(|v| v / P::MAX);
		(a.clamp(0.0, 1.0), [r, g, b])
	}

	/// Bilinear sample at `(x, y)` (integers on pixel centres) of the
	/// premultiplied pixels, given back straight on a 0..1 scale, so that a
	/// transparent neighbour fades coverage and not colour.
	unsafe fn sample_premultiplied(&self, x: f64, y: f64) -> (f64, [f64; 3]) {
		let (x0, y0) = (x.floor(), y.floor());
		let (fx, fy) = (x - x0, y - y0);
		let (x0, y0) = (x0 as i32, y0 as i32);
		let mut acc = [0.0; 4];
		for (dx, dy, w) in [
			(0, 0, (1.0 - fx) * (1.0 - fy)),
			(1, 0, fx * (1.0 - fy)),
			(0, 1, (1.0 - fx) * fy),
			(1, 1, fx * fy),
		] {
			if w == 0.0 {
				continue;
			}
			let (a, c) = unsafe { self.straight(x0.saturating_add(dx), y0.saturating_add(dy)) };
			acc[0] += a * w;
			for i in 0..3 {
				acc[i + 1] += c[i] * a * w;
			}
		}
		let a = acc[0];
		if a <= 0.0 {
			return (0.0, [0.0; 3]);
		}
		(a, [acc[1] / a, acc[2] / a, acc[3] / a])
	}
}

/// Whether `field` restricts processing to alternating destination scanlines.
/// `UPPER` is treated as the even rows (0, 2, 4, ...) and `LOWER` the odd
/// rows; `FRAME` (and any other value) processes every row.
#[inline]
fn skip_row_for_field(y: i32, field: PF_Field) -> bool {
	if field == PF_Field_UPPER {
		y.rem_euclid(2) != 0
	} else if field == PF_Field_LOWER {
		y.rem_euclid(2) == 0
	} else {
		false
	}
}

#[inline]
fn from_straight<P: Channels>(a: f64, c: [f64; 3]) -> P {
	P::store([a, c[0], c[1], c[2]].map(|v| v * P::MAX))
}

/// The opacity of `mode` at `P`'s depth: `opacitySu` is "for deep color only"
/// (`AE_EffectCB.h`), taken to run to 32768 as 16-bit channels do. A caller
/// that filled in only `opacity` gets that.
fn opacity_of<P: Channels>(mode: &PF_CompositeMode) -> f64 {
	if P::MAX == f64::from(PF_MAX_CHAN8 as u8) || mode.opacitySu == 0 {
		mode.opacity as f64 / 255.0
	} else {
		(mode.opacitySu as f64 / PF_MAX_CHAN16 as f64).min(1.0)
	}
}

use super::transform_modes::{Px, composite, dissolve_roll};

/// Composite one pixel, as After Effects does (see [`super::transform_modes`]).
#[inline]
fn composite_pixel<P: Channels>(
	(src_a, src_c): (f64, [f64; 3]),
	opacity: f64,
	(dst_a, dst_c): (f64, [f64; 3]),
	mode: A_long,
	roll: f64,
) -> (f64, [f64; 3]) {
	let out = composite(
		mode,
		Px { a: src_a, c: src_c },
		opacity,
		Px { a: dst_a, c: dst_c },
		!P::HDR,
		roll,
	);
	(out.a, out.c)
}

/// A mask world, at a depth of its own.
#[derive(Clone, Copy)]
struct Mask {
	world: MaskPixels,
	offset: PF_Point,
	what: PF_MaskFlags,
}

#[derive(Clone, Copy)]
enum MaskPixels {
	Eight(RawWorld<PF_Pixel8>),
	Sixteen(RawWorld<PF_Pixel16>),
	Float(RawWorld<PF_PixelFloat>),
}

impl Mask {
	/// The mask behind a `mask_world0`, `None` for a null one.
	fn of(name: &str, mask: Option<&PF_MaskWorld>) -> Result<Option<Self>, PF_Err> {
		let Some(mask) = mask else {
			return Ok(None);
		};
		let world = match common_depth(name, &[&mask.mask])? {
			Depth::Eight => MaskPixels::Eight(RawWorld::from(&mask.mask)),
			Depth::Sixteen => MaskPixels::Sixteen(RawWorld::from(&mask.mask)),
			Depth::Float => MaskPixels::Float(RawWorld::from(&mask.mask)),
		};
		Ok(Some(Self {
			world,
			offset: mask.offset,
			what: mask.what_is_mask,
		}))
	}

	/// The mask's value at `(x, y)` (in the masked layer's coordinates;
	/// `offset` shifts into the mask's own coordinate space), honoring
	/// `PF_MaskFlag_LUMINANCE` (use luma instead of alpha) and
	/// `PF_MaskFlag_INVERTED`.
	#[inline]
	fn value(&self, x: i32, y: i32) -> f64 {
		fn sample<P: Channels>(world: RawWorld<P>, x: i32, y: i32, luminance: bool) -> f64 {
			// SAFETY: `read` checks its bounds.
			let [a, r, g, b] = unsafe { world.read(x, y) }.channels();
			let v = if luminance { 0.3 * r + 0.59 * g + 0.11 * b } else { a };
			(v / P::MAX).clamp(0.0, 1.0)
		}
		let (x, y) = (x - self.offset.h, y - self.offset.v);
		let luminance = self.what & PF_MaskFlag_LUMINANCE != 0;
		let v = match self.world {
			MaskPixels::Eight(world) => sample(world, x, y, luminance),
			MaskPixels::Sixteen(world) => sample(world, x, y, luminance),
			MaskPixels::Float(world) => sample(world, x, y, luminance),
		};
		if self.what & PF_MaskFlag_INVERTED != 0 {
			1.0 - v
		} else {
			v
		}
	}
}

// ---- 3x3 affine matrix helpers (row-vector convention: p' = p * M) --------

type Mat3 = [[f64; 3]; 3];
const IDENTITY3: Mat3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

fn apply_matrix(m: &Mat3, x: f64, y: f64) -> (f64, f64) {
	let xh = x * m[0][0] + y * m[1][0] + m[2][0];
	let yh = x * m[0][1] + y * m[1][1] + m[2][1];
	let wh = x * m[0][2] + y * m[1][2] + m[2][2];
	if (wh - 1.0).abs() > 1e-9 && wh.abs() > 1e-9 {
		(xh / wh, yh / wh)
	} else {
		(xh, yh)
	}
}

fn invert3(m: Mat3) -> Option<Mat3> {
	let [[a, b, c], [d, e, f], [g, h, i]] = m;
	let a1 = e * i - f * h;
	let b1 = f * g - d * i;
	let c1 = d * h - e * g;
	let det = a * a1 + b * b1 + c * c1;
	if det.abs() < 1e-12 {
		return None;
	}
	let inv_det = 1.0 / det;
	let d1 = c * h - b * i;
	let e1 = a * i - c * g;
	let f1 = b * g - a * h;
	let g1 = b * f - c * e;
	let h1 = c * d - a * f;
	let i1 = a * e - b * d;
	Some([
		[a1 * inv_det, d1 * inv_det, g1 * inv_det],
		[b1 * inv_det, e1 * inv_det, h1 * inv_det],
		[c1 * inv_det, f1 * inv_det, i1 * inv_det],
	])
}

// ============================================================================
// Suite entry points
// ============================================================================

/// `PF_WorldTransformSuite1::copy` and the legacy `copy`: an unfiltered blit
/// that keeps alpha, at the worlds' depth.
///
/// Without a source rectangle all of the source is copied. Without a
/// destination rectangle it lands where it came from; one of another size
/// stretches it ("non-antialiased stretch blit", `AE_EffectCB.h`).
///
/// # Safety
/// The world and rectangle pointers must be null or valid for the call.
pub(crate) unsafe extern "C" fn copy_sys(
	_effect_ref: PF_ProgPtr,
	src: *mut PF_EffectWorld,
	dst: *mut PF_EffectWorld,
	src_r: *mut PF_Rect,
	dst_r: *mut PF_Rect,
) -> PF_Err {
	crate::core::ffi::guard("copy_sys", || {
		diag!("PF_WorldTransformSuite1/copy", "src" => format!("{src:?}"), "dst" => format!("{dst:?}"));
		// SAFETY: null, or the plugin's worlds for this call.
		let (Some(s), Some(d)) = (unsafe { src.as_ref() }, unsafe { dst.as_ref() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		// An unlinked layer has no pixels: nothing to copy.
		if std::ptr::eq(s, d) || s.data.is_null() || d.data.is_null() {
			return PF_Err_NONE;
		}
		let depth = match common_depth("copy", &[s, d]) {
			Ok(depth) => depth,
			Err(err) => return err,
		};
		// SAFETY: null, or the plugin's rectangles.
		let from = unsafe { src_r.as_ref() }.copied().unwrap_or(PF_Rect {
			left: 0,
			top: 0,
			right: s.width,
			bottom: s.height,
		});
		// SAFETY: as above.
		let to = unsafe { dst_r.as_ref() }.copied().unwrap_or(from);
		by_depth!(depth, copy(s, d, from, to));
		PF_Err_NONE
	})
}

fn copy<P: Channels>(s: &PF_EffectWorld, d: &PF_EffectWorld, from: PF_Rect, to: PF_Rect) {
	let src_raw = RawWorld::<P>::from(s);
	let dst_raw = RawWorld::<P>::from(d);
	let (from_w, from_h) = (from.right - from.left, from.bottom - from.top);
	let (to_w, to_h) = (to.right - to.left, to.bottom - to.top);
	if from_w <= 0 || from_h <= 0 {
		return;
	}
	// An empty destination rectangle is only a point.
	let stretch = to_w > 0 && to_h > 0 && (to_w, to_h) != (from_w, from_h);
	let (w, h) = if stretch { (to_w, to_h) } else { (from_w, from_h) };
	// After Effects takes, for destination pixel `at`, the source pixel at
	// ceil((at + 1) * from / to) - 1: the last one the destination pixel's
	// far edge reaches into.
	let source = |at: i32, to: i32, from: i32| {
		if stretch {
			((i64::from(at) + 1) * i64::from(from) - 1)
				.div_euclid(i64::from(to))
				.min(i64::from(from) - 1) as i32
		} else {
			at
		}
	};

	// The part of the blit whose pixels are in both worlds.
	let columns: Vec<(i32, i32)> = (0..w)
		.map(|col| (from.left + source(col, w, from_w), to.left + col))
		.filter(|&(sx, dx)| (0..s.width).contains(&sx) && (0..d.width).contains(&dx))
		.collect();
	let rows: Vec<(i32, i32)> = (0..h)
		.map(|row| (from.top + source(row, h, from_h), to.top + row))
		.filter(|&(sy, dy)| (0..s.height).contains(&sy) && (0..d.height).contains(&dy))
		.collect();

	let extent = |world: &PF_EffectWorld| {
		let start = world.data as usize;
		start..start + world.height.max(0) as usize * world.rowbytes.unsigned_abs() as usize
	};
	let (a, b) = (extent(s), extent(d));
	if a.start < b.end && b.start < a.end {
		// One buffer: take all of the source before writing any of it.
		let taken: Vec<P> = rows
			.iter()
			// SAFETY: `read` checks its bounds.
			.flat_map(|&(sy, _)| columns.iter().map(move |&(sx, _)| unsafe { src_raw.read(sx, sy) }))
			.collect();
		let targets = rows
			.iter()
			.flat_map(|&(_, dy)| columns.iter().map(move |&(_, dx)| (dx, dy)));
		for ((dx, dy), pixel) in targets.zip(taken) {
			// SAFETY: `write` checks its bounds.
			unsafe { dst_raw.write(dx, dy, pixel) };
		}
	} else {
		rows.par_iter().for_each(|&(sy, dy)| {
			for &(sx, dx) in &columns {
				// SAFETY: `read` and `write` check their bounds.
				unsafe { dst_raw.write(dx, dy, src_raw.read(sx, sy)) };
			}
		});
	}
}

/// Composites `source_wld`'s `src_rect` onto `dest_wld` at `(dest_x,
/// dest_y)`, scaled by `src_opacity` (0..255) and blended per `xfer_mode`.
pub(crate) unsafe extern "C" fn composite_rect_sys(
	_effect_ref: PF_ProgPtr,
	src_rect: *mut PF_Rect,
	src_opacity: A_long,
	source_wld: *mut PF_EffectWorld,
	dest_x: A_long,
	dest_y: A_long,
	field_rdr: PF_Field,
	xfer_mode: PF_XferMode,
	dest_wld: *mut PF_EffectWorld,
) -> PF_Err {
	crate::core::ffi::guard("composite_rect_sys", || {
		diag!("PF_WorldTransformSuite1/composite_rect", "opacity" => src_opacity, "xfer" => xfer_mode, "src" => format!("{source_wld:?}"), "dst" => format!("{dest_wld:?}"));
		if source_wld.is_null() || dest_wld.is_null() {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		let src = unsafe { &*source_wld };
		let dst = unsafe { &*dest_wld };
		if src.data.is_null() || dst.data.is_null() {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		let depth = match common_depth("composite_rect", &[src, dst]) {
			Ok(depth) => depth,
			Err(err) => return err,
		};

		let rect = if !src_rect.is_null() {
			unsafe { *src_rect }
		} else {
			PF_Rect {
				left: 0,
				top: 0,
				right: src.width,
				bottom: src.height,
			}
		};
		let left = rect.left.max(0);
		let top = rect.top.max(0);
		let width = (rect.right.min(src.width) - left).max(0);
		let height = (rect.bottom.min(src.height) - top).max(0);
		if width == 0 || height == 0 {
			return PF_Err_NONE;
		}

		let opacity = src_opacity.clamp(0, 255) as f64 / 255.0;
		let (from, to) = ((left, top, width, height), (dest_x, dest_y));
		by_depth!(depth, composite_rect(src, dst, from, to, opacity, field_rdr, xfer_mode));

		PF_Err_NONE
	})
}

fn composite_rect<P: Channels>(
	src: &PF_EffectWorld,
	dst: &PF_EffectWorld,
	(left, top, width, height): (i32, i32, i32, i32),
	(dest_x, dest_y): (i32, i32),
	opacity: f64,
	field_rdr: PF_Field,
	xfer_mode: PF_XferMode,
) {
	let src_raw = RawWorld::<P>::from(src);
	let dst_raw = RawWorld::<P>::from(dst);

	(0..height).into_par_iter().for_each(|row| {
		let dy = dest_y + row;
		if skip_row_for_field(dy, field_rdr) {
			return;
		}
		let sy = top + row;
		for col in 0..width {
			let dx = dest_x + col;
			let sx = left + col;
			let source = unsafe { src_raw.straight(sx, sy) };
			let backdrop = unsafe { dst_raw.straight(dx, dy) };
			let (out_a, out_c) = composite_pixel::<P>(source, opacity, backdrop, xfer_mode, dissolve_roll(0, dx, dy));
			unsafe { dst_raw.write(dx, dy, from_straight::<P>(out_a, out_c)) };
		}
	});
}

/// Cross-fades `src1`/`src2` into `dst` at a constant `ratio` (16.16 fixed,
/// 0.0..1.0), including the alpha channel. Operates over the overlap of all
/// three worlds' dimensions.
pub(crate) unsafe extern "C" fn blend_sys(
	_effect_ref: PF_ProgPtr,
	src1: *const PF_EffectWorld,
	src2: *const PF_EffectWorld,
	ratio: PF_Fixed,
	dst: *mut PF_EffectWorld,
) -> PF_Err {
	crate::core::ffi::guard("blend_sys", || {
		diag!("PF_WorldTransformSuite1/blend", "ratio" => ratio, "dst" => format!("{dst:?}"));
		if src1.is_null() || src2.is_null() || dst.is_null() {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		let s1 = unsafe { &*src1 };
		let s2 = unsafe { &*src2 };
		let d = unsafe { &*dst };
		if s1.data.is_null() || s2.data.is_null() || d.data.is_null() {
			return PF_Err_BAD_CALLBACK_PARAM;
		}

		let t = (ratio as f64 / 65536.0).clamp(0.0, 1.0);
		let width = s1.width.min(s2.width).min(d.width).max(0);
		let height = s1.height.min(s2.height).min(d.height).max(0);
		if width == 0 || height == 0 {
			return PF_Err_NONE;
		}
		let depth = match common_depth("blend", &[s1, s2, d]) {
			Ok(depth) => depth,
			Err(err) => return err,
		};

		by_depth!(depth, blend(s1, s2, d, (width, height), t));

		PF_Err_NONE
	})
}

fn blend<P: Channels>(
	s1: &PF_EffectWorld,
	s2: &PF_EffectWorld,
	d: &PF_EffectWorld,
	(width, height): (i32, i32),
	t: f64,
) {
	let r1 = RawWorld::<P>::from(s1);
	let r2 = RawWorld::<P>::from(s2);
	let rd = RawWorld::<P>::from(d);

	(0..height).into_par_iter().for_each(|y| {
		for x in 0..width {
			let p1 = unsafe { r1.read(x, y) };
			let p2 = unsafe { r2.read(x, y) };
			let (from, to) = (p1.channels(), p2.channels());
			let out = P::store(std::array::from_fn(|i| from[i] + (to[i] - from[i]) * t));
			unsafe { rd.write(x, y, out) };
		}
	});
}

/// A convolved channel: wrapped with `NO_CLAMP` in the integer depths (the
/// clamping is `store`'s), as it is in 32 bpc.
fn quantize<P: Channels>(v: f64, no_clamp: bool) -> f64 {
	if no_clamp && !P::HDR {
		(v.round() as i64).rem_euclid(P::MAX as i64 + 1) as f64
	} else {
		v
	}
}

/// Bundles the parameters shared by every tap of a convolution at a given
/// output pixel, so the per-channel helpers below don't need a long, easily
/// mis-ordered argument list.
struct ConvolveCtx<'a, P> {
	world: RawWorld<P>,
	taps: &'a [(i32, i32)],
	replicate: bool,
	normalized: bool,
}

impl<P> Clone for ConvolveCtx<'_, P> {
	fn clone(&self) -> Self {
		*self
	}
}

impl<P> Copy for ConvolveCtx<'_, P> {}

fn convolve_value<P: Channels>(ctx: ConvolveCtx<P>, x: i32, y: i32, weights: &[f64], sum: f64, channel: usize) -> f64 {
	let mut acc = 0.0;
	for (i, &(ox, oy)) in ctx.taps.iter().enumerate() {
		let w = weights[i];
		if w == 0.0 {
			continue;
		}
		let px = unsafe { ctx.world.read_edge(x + ox, y + oy, ctx.replicate) };
		acc += w * px.channels()[channel];
	}
	if ctx.normalized || sum.abs() < 1e-9 {
		acc
	} else {
		acc / sum
	}
}

/// Convolves premultiplied by each tap's own alpha, then unpremultiplies the
/// result -- avoids the dark/bright fringing a straight convolution produces
/// at partially-transparent edges. Self-normalizing regardless of the
/// `NORMALIZED` flag, since scaling every weight by a constant cancels out of
/// the ratio.
fn convolve_value_alpha_weighted<P: Channels>(
	ctx: ConvolveCtx<P>,
	x: i32,
	y: i32,
	weights: &[f64],
	channel: usize,
) -> f64 {
	let mut num = 0.0;
	let mut den = 0.0;
	for (i, &(ox, oy)) in ctx.taps.iter().enumerate() {
		let w = weights[i];
		if w == 0.0 {
			continue;
		}
		let px = unsafe { ctx.world.read_edge(x + ox, y + oy, ctx.replicate) };
		let channels = px.channels();
		let a = channels[0] / P::MAX;
		num += w * channels[channel] * a;
		den += w * a;
	}
	if den.abs() > 1e-9 { num / den } else { 0.0 }
}

/// Convolves `src`'s `area` with the given per-channel kernels, writing the
/// result to `dst`. A `null` channel kernel leaves that channel unmodified.
/// `src` and `dst` must be different buffers (a sliding-window filter can't
/// safely run in place, and parallelising by row assumes disjoint reads).
#[allow(non_upper_case_globals)]
pub(crate) unsafe extern "C" fn convolve_sys(
	_effect_ref: PF_ProgPtr,
	src: *mut PF_EffectWorld,
	area: *const PF_Rect,
	flags: PF_KernelFlags,
	kernel_size: A_long,
	a_kernel: *mut c_void,
	r_kernel: *mut c_void,
	g_kernel: *mut c_void,
	b_kernel: *mut c_void,
	dst: *mut PF_EffectWorld,
) -> PF_Err {
	crate::core::ffi::guard("convolve_sys", || {
		diag!("PF_WorldTransformSuite1/convolve", "flags" => format!("{flags:#x}"), "kernel_size" => kernel_size, "dst" => format!("{dst:?}"));
		if src.is_null() || dst.is_null() {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		if std::ptr::eq(src, dst) {
			log::error!("PF_WorldTransformSuite1::convolve: src and dst must be different buffers");
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		let s = unsafe { &*src };
		let d = unsafe { &*dst };
		if s.data.is_null() || d.data.is_null() || kernel_size <= 0 {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		let depth = match common_depth("convolve", &[s, d]) {
			Ok(depth) => depth,
			Err(err) => return err,
		};

		let rect = if !area.is_null() {
			unsafe { *area }
		} else {
			PF_Rect {
				left: 0,
				top: 0,
				right: s.width,
				bottom: s.height,
			}
		};
		let left = rect.left.max(0);
		let top = rect.top.max(0);
		let width = (rect.right.min(s.width).min(d.width) - left).max(0);
		let height = (rect.bottom.min(s.height).min(d.height) - top).max(0);
		if width == 0 || height == 0 {
			return PF_Err_NONE;
		}

		let is_1d = flags & PF_KernelFlag_1D != 0;
		let vertical = flags & PF_KernelFlag_VERTICAL != 0;
		let normalized = flags & PF_KernelFlag_NORMALIZED != 0;
		let no_clamp = flags & PF_KernelFlag_NO_CLAMP != 0;
		let replicate = flags & PF_KernelFlag_REPLICATE_BORDERS != 0;
		let alpha_weighted = flags & PF_KernelFlag_ALPHA_WEIGHT_CONVOLVE != 0;
		let kernel_type = flags & (PF_KernelFlag_USE_CHAR | PF_KernelFlag_USE_FIXED);

		let taps: Vec<(i32, i32)> = if is_1d {
			(0..kernel_size)
				.map(|i| {
					let offset = i - kernel_size / 2;
					if vertical { (0, offset) } else { (offset, 0) }
				})
				.collect()
		} else {
			let mut v = Vec::with_capacity((kernel_size * kernel_size).max(0) as usize);
			for ky in 0..kernel_size {
				for kx in 0..kernel_size {
					v.push((kx - kernel_size / 2, ky - kernel_size / 2));
				}
			}
			v
		};

		let build_channel = |ptr: *mut c_void| -> Option<Channel> {
			if ptr.is_null() {
				return None;
			}
			let weights: Vec<f64> = (0..taps.len())
				.map(|i| match kernel_type {
					PF_KernelFlag_USE_CHAR => (unsafe { *(ptr as *const u8).add(i) }) as f64,
					PF_KernelFlag_USE_FIXED => (unsafe { *(ptr as *const PF_Fixed).add(i) }) as f64 / 65536.0,
					_ => (unsafe { *(ptr as *const A_long).add(i) }) as f64,
				})
				.collect();
			let sum = weights.iter().sum();
			Some(Channel { weights, sum })
		};

		let kernels = [a_kernel, r_kernel, g_kernel, b_kernel].map(build_channel);
		let how = Convolution {
			area: (left, top, width, height),
			taps,
			kernels,
			replicate,
			normalized,
			no_clamp,
			alpha_weighted,
		};

		by_depth!(depth, convolve(s, d, &how));

		PF_Err_NONE
	})
}

struct Channel {
	weights: Vec<f64>,
	sum: f64,
}

/// A convolution, read from the suite's arguments.
struct Convolution {
	area: (i32, i32, i32, i32),
	taps: Vec<(i32, i32)>,
	/// Alpha, red, green and blue; a channel without one is left as it is.
	kernels: [Option<Channel>; 4],
	replicate: bool,
	normalized: bool,
	no_clamp: bool,
	alpha_weighted: bool,
}

fn convolve<P: Channels>(s: &PF_EffectWorld, d: &PF_EffectWorld, how: &Convolution) {
	let (left, top, width, height) = how.area;
	let src_raw = RawWorld::<P>::from(s);
	let dst_raw = RawWorld::<P>::from(d);
	let ctx = ConvolveCtx {
		world: src_raw,
		taps: &how.taps[..],
		replicate: how.replicate,
		normalized: how.normalized,
	};

	(0..height).into_par_iter().for_each(|row| {
		let y = top + row;
		for col in 0..width {
			let x = left + col;
			// SAFETY: `read` and `write` check their bounds.
			let here = unsafe { src_raw.read(x, y) }.channels();
			let out = P::store(std::array::from_fn(|channel| match &how.kernels[channel] {
				Some(kernel) => {
					let v = if how.alpha_weighted {
						convolve_value_alpha_weighted(ctx, x, y, &kernel.weights, channel)
					} else {
						convolve_value(ctx, x, y, &kernel.weights, kernel.sum, channel)
					};
					quantize::<P>(v, how.no_clamp)
				}
				None => here[channel],
			}));
			// SAFETY: as above.
			unsafe { dst_raw.write(x, y, out) };
		}
	});
}

/// Like [`copy_sys`], but resamples with bilinear filtering, so `src_r` and
/// `dst_r` may have different sizes (a scaled blit) rather than requiring an
/// exact 1:1 pixel match.
unsafe extern "C" fn copy_hq_sys(
	_effect_ref: PF_ProgPtr,
	src: *mut PF_EffectWorld,
	dst: *mut PF_EffectWorld,
	src_r: *mut PF_Rect,
	dst_r: *mut PF_Rect,
) -> PF_Err {
	crate::core::ffi::guard("copy_hq_sys", || {
		diag!("PF_WorldTransformSuite1/copy_hq", "src" => format!("{src:?}"), "dst" => format!("{dst:?}"));
		if src.is_null() || dst.is_null() {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		let s = unsafe { &*src };
		let d = unsafe { &*dst };
		if s.data.is_null() || d.data.is_null() {
			return PF_Err_BAD_CALLBACK_PARAM;
		}

		let depth = match common_depth("copy_hq", &[s, d]) {
			Ok(depth) => depth,
			Err(err) => return err,
		};
		let src_rect = if !src_r.is_null() {
			unsafe { *src_r }
		} else {
			PF_Rect {
				left: 0,
				top: 0,
				right: s.width,
				bottom: s.height,
			}
		};
		let dst_rect = if !dst_r.is_null() {
			unsafe { *dst_r }
		} else {
			PF_Rect {
				left: 0,
				top: 0,
				right: d.width,
				bottom: d.height,
			}
		};

		let src_w = (src_rect.right - src_rect.left).max(0);
		let src_h = (src_rect.bottom - src_rect.top).max(0);
		let dst_left = dst_rect.left.max(0);
		let dst_top = dst_rect.top.max(0);
		let dst_w = (dst_rect.right.min(d.width) - dst_left).max(0);
		let dst_h = (dst_rect.bottom.min(d.height) - dst_top).max(0);
		if src_w == 0 || src_h == 0 || dst_w == 0 || dst_h == 0 {
			return PF_Err_NONE;
		}

		let from = (src_rect.left, src_rect.top, src_w, src_h);
		let to = (dst_left, dst_top, dst_w, dst_h);
		by_depth!(depth, copy_hq(s, d, from, to));

		PF_Err_NONE
	})
}

fn copy_hq<P: Channels>(
	s: &PF_EffectWorld,
	d: &PF_EffectWorld,
	(src_left, src_top, src_w, src_h): (i32, i32, i32, i32),
	(dst_left, dst_top, dst_w, dst_h): (i32, i32, i32, i32),
) {
	let scale_x = src_w as f64 / dst_w as f64;
	let scale_y = src_h as f64 / dst_h as f64;
	let src_raw = RawWorld::<P>::from(s);
	let dst_raw = RawWorld::<P>::from(d);

	// Each destination pixel averages the source area it covers, weighted by
	// how much of each source pixel falls in it and by coverage.
	let spans = |at: i32, scale: f64, start: i32| -> Vec<(i32, f64)> {
		let (from, to) = (f64::from(at) * scale, f64::from(at + 1) * scale);
		let mut out = Vec::new();
		let mut p = from.floor();
		while p < to {
			let w = (p + 1.0).min(to) - p.max(from);
			if w > 0.0 {
				out.push((start + p as i32, w));
			}
			p += 1.0;
		}
		out
	};
	(0..dst_h).into_par_iter().for_each(|row| {
		let dy = dst_top + row;
		let rows = spans(row, scale_y, src_top);
		for col in 0..dst_w {
			let dx = dst_left + col;
			let mut acc = [0.0; 4];
			let mut area = 0.0;
			for &(sx, wx) in &spans(col, scale_x, src_left) {
				for &(sy, wy) in &rows {
					let w = wx * wy;
					// SAFETY: `straight` checks its bounds.
					let (a, c) = unsafe { src_raw.straight(sx, sy) };
					acc[0] += a * w;
					for i in 0..3 {
						acc[i + 1] += c[i] * a * w;
					}
					area += w;
				}
			}
			let a = if area > 0.0 { acc[0] / area } else { 0.0 };
			let c = if acc[0] > 0.0 {
				[acc[1] / acc[0], acc[2] / acc[0], acc[3] / acc[0]]
			} else {
				[0.0; 3]
			};
			// SAFETY: `write` checks its bounds.
			unsafe { dst_raw.write(dx, dy, from_straight::<P>(a, c)) };
		}
	});
}

/// `composite_rect`'s more capable sibling: takes a structured
/// `PF_CompositeMode` (opacity, `rgb_only`, Dissolve's `rand_seed`) and an
/// optional mask. As in After Effects, `m_flags` and `quality` change
/// nothing here: colours are straight either way.
pub(crate) unsafe extern "C" fn transfer_rect_sys(
	_effect_ref: PF_ProgPtr,
	_quality: PF_Quality,
	m_flags: PF_ModeFlags,
	field: PF_Field,
	src_rec: *const PF_Rect,
	src_world: *const PF_EffectWorld,
	comp_mode: *const PF_CompositeMode,
	mask_world0: *const PF_MaskWorld,
	dest_x: A_long,
	dest_y: A_long,
	dst_world: *mut PF_EffectWorld,
) -> PF_Err {
	crate::core::ffi::guard("transfer_rect_sys", || {
		diag!("PF_WorldTransformSuite1/transfer_rect", "flags" => format!("{m_flags:#x}"), "mode" => format!("{:?}", unsafe { comp_mode.as_ref() }), "dst" => format!("{dst_world:?}"));
		if src_world.is_null() || dst_world.is_null() || comp_mode.is_null() {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		let src = unsafe { &*src_world };
		let dst = unsafe { &*dst_world };
		if src.data.is_null() || dst.data.is_null() {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		let depth = match common_depth("transfer_rect", &[src, dst]) {
			Ok(depth) => depth,
			Err(err) => return err,
		};
		// SAFETY: null, or the plugin's mask for this call.
		let mask = match Mask::of("transfer_rect", unsafe { mask_world0.as_ref() }) {
			Ok(mask) => mask,
			Err(err) => return err,
		};
		let mode = unsafe { &*comp_mode };

		let rect = if !src_rec.is_null() {
			unsafe { *src_rec }
		} else {
			PF_Rect {
				left: 0,
				top: 0,
				right: src.width,
				bottom: src.height,
			}
		};
		let left = rect.left.max(0);
		let top = rect.top.max(0);
		let width = (rect.right.min(src.width) - left).max(0);
		let height = (rect.bottom.min(src.height) - top).max(0);
		if width == 0 || height == 0 {
			return PF_Err_NONE;
		}

		let (from, to) = ((left, top, width, height), (dest_x, dest_y));
		let how = Transfer { mode, mask, field };
		by_depth!(depth, transfer_rect(src, dst, from, to, &how));

		PF_Err_NONE
	})
}

/// What `transfer_rect` and `transform_world` composite with.
struct Transfer<'a> {
	mode: &'a PF_CompositeMode,
	mask: Option<Mask>,
	field: PF_Field,
}

fn transfer_rect<P: Channels>(
	src: &PF_EffectWorld,
	dst: &PF_EffectWorld,
	(left, top, width, height): (i32, i32, i32, i32),
	(dest_x, dest_y): (i32, i32),
	how: &Transfer,
) {
	let Transfer { mode, mask, field } = *how;
	let opacity = opacity_of::<P>(mode);
	let rgb_only = mode.rgb_only != 0;
	let xfer = mode.xfer;

	let src_raw = RawWorld::<P>::from(src);
	let dst_raw = RawWorld::<P>::from(dst);

	(0..height).into_par_iter().for_each(|row| {
		let dy = dest_y + row;
		if skip_row_for_field(dy, field) {
			return;
		}
		let sy = top + row;
		for col in 0..width {
			let dx = dest_x + col;
			let sx = left + col;
			let (mut src_a, src_c) = unsafe { src_raw.straight(sx, sy) };
			if let Some(mask) = mask {
				src_a *= mask.value(dx, dy);
			}
			let backdrop = unsafe { dst_raw.straight(dx, dy) };
			let roll = dissolve_roll(mode.rand_seed, dx, dy);
			let (out_a, out_c) = composite_pixel::<P>((src_a, src_c), opacity, backdrop, xfer, roll);
			let final_a = if rgb_only { backdrop.0 } else { out_a };
			unsafe { dst_raw.write(dx, dy, from_straight::<P>(final_a, out_c)) };
		}
	});
}

/// Applies one (or, for motion blur, several averaged) affine transform(s)
/// to `src_world`, compositing the result into `dst_world`'s `dest_rect`.
///
/// `matrices` map source space to destination space when `src2dst_matrix` is
/// set (so each destination pixel is resampled through the inverse) and
/// destination-to-source otherwise. `PF_Quality_LO` takes the source pixel
/// under each destination pixel's centre; `PF_Quality_HI` filters the
/// premultiplied pixels bilinearly. Multiple matrices are averaged the same
/// way, for motion blur.
pub(crate) unsafe extern "C" fn transform_world_sys(
	_effect_ref: PF_ProgPtr,
	quality: PF_Quality,
	m_flags: PF_ModeFlags,
	field: PF_Field,
	src_world: *const PF_EffectWorld,
	comp_mode: *const PF_CompositeMode,
	mask_world0: *const PF_MaskWorld,
	matrices: *const PF_FloatMatrix,
	num_matrices: A_long,
	src2dst_matrix: PF_Boolean,
	dest_rect: *const PF_Rect,
	dst_world: *mut PF_EffectWorld,
) -> PF_Err {
	crate::core::ffi::guard("transform_world_sys", || {
		diag!("PF_WorldTransformSuite1/transform_world", "flags" => format!("{m_flags:#x}"), "matrices" => num_matrices, "mode" => format!("{:?}", unsafe { comp_mode.as_ref() }), "dst" => format!("{dst_world:?}"));
		if src_world.is_null() || dst_world.is_null() || comp_mode.is_null() || matrices.is_null() || num_matrices <= 0
		{
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		let src = unsafe { &*src_world };
		let dst = unsafe { &*dst_world };
		if src.data.is_null() || dst.data.is_null() {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		let depth = match common_depth("transform_world", &[src, dst]) {
			Ok(depth) => depth,
			Err(err) => return err,
		};
		// SAFETY: null, or the plugin's mask for this call.
		let mask = match Mask::of("transform_world", unsafe { mask_world0.as_ref() }) {
			Ok(mask) => mask,
			Err(err) => return err,
		};
		let mode = unsafe { &*comp_mode };

		let rect = if !dest_rect.is_null() {
			unsafe { *dest_rect }
		} else {
			PF_Rect {
				left: 0,
				top: 0,
				right: dst.width,
				bottom: dst.height,
			}
		};
		let left = rect.left.max(0);
		let top = rect.top.max(0);
		let width = (rect.right.min(dst.width) - left).max(0);
		let height = (rect.bottom.min(dst.height) - top).max(0);
		if width == 0 || height == 0 {
			return PF_Err_NONE;
		}

		// Pre-invert into dst->src matrices up front so the per-pixel loop only
		// ever has to forward-map.
		let mats: Vec<Mat3> = unsafe { std::slice::from_raw_parts(matrices, num_matrices as usize) }
			.iter()
			.map(|m| {
				if src2dst_matrix != 0 {
					invert3(m.mat).unwrap_or(IDENTITY3)
				} else {
					m.mat
				}
			})
			.collect();

		let to = (left, top, width, height);
		let how = Transfer { mode, mask, field };
		let nearest = quality == PF_Quality_LO as PF_Quality;
		by_depth!(depth, transform_world(src, dst, to, &mats, &how, nearest));

		PF_Err_NONE
	})
}

fn transform_world<P: Channels>(
	src: &PF_EffectWorld,
	dst: &PF_EffectWorld,
	(left, top, width, height): (i32, i32, i32, i32),
	mats: &[Mat3],
	how: &Transfer,
	nearest: bool,
) {
	let Transfer { mode, mask, field } = *how;
	let opacity = opacity_of::<P>(mode);
	let rgb_only = mode.rgb_only != 0;
	let xfer = mode.xfer;

	let src_raw = RawWorld::<P>::from(src);
	let dst_raw = RawWorld::<P>::from(dst);

	(0..height).into_par_iter().for_each(|row| {
		let dy = top + row;
		if skip_row_for_field(dy, field) {
			return;
		}
		for col in 0..width {
			let dx = left + col;

			let mut acc_a = 0.0;
			let mut acc_c = [0.0; 3];
			for mat in mats {
				// Each destination pixel's centre, mapped back into the source.
				let (sx, sy) = apply_matrix(mat, dx as f64 + 0.5, dy as f64 + 0.5);
				// Low quality takes the pixel under it, high quality filters.
				let (a, c) = if nearest {
					unsafe { src_raw.straight(sx.floor() as i32, sy.floor() as i32) }
				} else {
					unsafe { src_raw.sample_premultiplied(sx - 0.5, sy - 0.5) }
				};
				acc_a += a;
				for i in 0..3 {
					acc_c[i] += c[i] * a;
				}
			}
			let mut src_a = acc_a / mats.len() as f64;
			let src_c = if acc_a > 1e-9 {
				[acc_c[0] / acc_a, acc_c[1] / acc_a, acc_c[2] / acc_a]
			} else {
				[0.0; 3]
			};
			if let Some(mask) = mask {
				src_a *= mask.value(dx, dy);
			}

			let backdrop = unsafe { dst_raw.straight(dx, dy) };
			let roll = dissolve_roll(mode.rand_seed, dx, dy);
			let (out_a, out_c) = composite_pixel::<P>((src_a, src_c), opacity, backdrop, xfer, roll);
			let final_a = if rgb_only { backdrop.0 } else { out_a };
			unsafe { dst_raw.write(dx, dy, from_straight::<P>(final_a, out_c)) };
		}
	});
}

// ============================================================================
// Factory Function
// ============================================================================

/// Builds the `PF_WorldTransformSuite1` vtable.
///
/// `const` so it can initialize the shared [`SUITE_CONTAINER`](crate::suites::SUITE_CONTAINER)
/// static; the suite is a stateless table of function pointers.
pub const fn create_world_transform_suite_1() -> PF_WorldTransformSuite1 {
	PF_WorldTransformSuite1 {
		composite_rect: Some(composite_rect_sys),
		blend: Some(blend_sys),
		convolve: Some(convolve_sys),
		copy: Some(copy_sys),
		copy_hq: Some(copy_hq_sys),
		transfer_rect: Some(transfer_rect_sys),
		transform_world: Some(transform_world_sys),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	const PIXEL_SIZE: isize = std::mem::size_of::<PF_Pixel8>() as isize;

	#[test]
	fn composite_rect_rejects_two_depths_in_one_call() {
		let mut src = super::super::world::alloc_world(2, 2, 8, PF_PixelFormat_ARGB64 as u32);
		let mut dst = super::super::world::alloc_world(2, 2, 4, PF_PixelFormat_ARGB32 as u32);
		let err = unsafe {
			composite_rect_sys(
				std::ptr::null_mut(),
				std::ptr::null_mut(),
				255,
				&mut src,
				0,
				0,
				PF_Field_FRAME,
				PF_Xfer_IN_FRONT as PF_XferMode,
				&mut dst,
			)
		};
		assert_eq!(err, PF_Err_BAD_CALLBACK_PARAM);
		unsafe {
			super::super::world::free_world(&mut src);
			super::super::world::free_world(&mut dst);
		}
	}

	/// An owned, heap-backed `PF_EffectWorld` for exercising suite entry
	/// points without a real host. `world.data` points into `buf`, which is
	/// why `buf` must outlive every raw-pointer read/write into `world`.
	struct TestWorld {
		buf: Vec<u8>,
		world: PF_EffectWorld,
	}

	impl TestWorld {
		fn new(width: i32, height: i32, fill: PF_Pixel8) -> Self {
			let rowbytes = width * PIXEL_SIZE as i32;
			let mut buf = vec![0u8; (rowbytes * height).max(0) as usize];
			for chunk in buf.as_chunks_mut::<{ PIXEL_SIZE as usize }>().0 {
				chunk[0] = fill.alpha;
				chunk[1] = fill.red;
				chunk[2] = fill.green;
				chunk[3] = fill.blue;
			}
			let world = PF_EffectWorld {
				reserved0: std::ptr::null_mut(),
				reserved1: std::ptr::null_mut(),
				world_flags: 0,
				data: buf.as_mut_ptr() as PF_PixelPtr,
				rowbytes,
				width,
				height,
				extent_hint: PF_UnionableRect {
					left: 0,
					top: 0,
					right: width,
					bottom: height,
				},
				platform_ref: std::ptr::null_mut(),
				reserved_long1: 0,
				reserved_long4: std::ptr::null_mut(),
				pix_aspect_ratio: PF_RationalScale { num: 1, den: 1 },
				reserved_long2: std::ptr::null_mut(),
				origin_x: 0,
				origin_y: 0,
				reserved_long3: 0,
				dephault: 0,
			};
			Self { buf, world }
		}

		fn pixel(&self, x: i32, y: i32) -> PF_Pixel8 {
			let idx = (y * self.world.rowbytes + x * PIXEL_SIZE as i32) as usize;
			PF_Pixel8 {
				alpha: self.buf[idx],
				red: self.buf[idx + 1],
				green: self.buf[idx + 2],
				blue: self.buf[idx + 3],
			}
		}

		fn as_mut_ptr(&mut self) -> *mut PF_EffectWorld {
			&mut self.world
		}
	}

	fn px(a: u8, r: u8, g: u8, b: u8) -> PF_Pixel8 {
		PF_Pixel8 {
			alpha: a,
			red: r,
			green: g,
			blue: b,
		}
	}

	/// `PF_Pixel` (from `after-effects-sys`) doesn't derive `PartialEq`, so
	/// tests compare fields through this helper instead.
	#[track_caller]
	fn assert_px_eq(actual: PF_Pixel8, expected: PF_Pixel8, msg: &str) {
		assert_eq!(
			(actual.alpha, actual.red, actual.green, actual.blue),
			(expected.alpha, expected.red, expected.green, expected.blue),
			"{msg}"
		);
	}

	#[test]
	fn blend_at_endpoints_and_midpoint() {
		let mut s1 = TestWorld::new(2, 2, px(255, 100, 0, 0));
		let mut s2 = TestWorld::new(2, 2, px(255, 0, 200, 0));
		let mut d = TestWorld::new(2, 2, px(0, 0, 0, 0));

		unsafe {
			blend_sys(
				std::ptr::null_mut(),
				s1.as_mut_ptr(),
				s2.as_mut_ptr(),
				0,
				d.as_mut_ptr(),
			)
		};
		assert_px_eq(d.pixel(0, 0), px(255, 100, 0, 0), "ratio=0 should equal src1");

		unsafe {
			blend_sys(
				std::ptr::null_mut(),
				s1.as_mut_ptr(),
				s2.as_mut_ptr(),
				1 << 16,
				d.as_mut_ptr(),
			)
		};
		assert_px_eq(d.pixel(0, 0), px(255, 0, 200, 0), "ratio=1 should equal src2");

		unsafe {
			blend_sys(
				std::ptr::null_mut(),
				s1.as_mut_ptr(),
				s2.as_mut_ptr(),
				1 << 15,
				d.as_mut_ptr(),
			)
		};
		assert_px_eq(d.pixel(0, 0), px(255, 50, 100, 0), "ratio=0.5 should be the midpoint");
	}

	#[test]
	fn composite_rect_copy_opaque_src_replaces_dst() {
		let mut src = TestWorld::new(2, 2, px(255, 10, 20, 30));
		let mut dst = TestWorld::new(2, 2, px(255, 200, 200, 200));

		let err = unsafe {
			composite_rect_sys(
				std::ptr::null_mut(),
				std::ptr::null_mut(),
				255,
				src.as_mut_ptr(),
				0,
				0,
				PF_Field_FRAME,
				PF_Xfer_COPY,
				dst.as_mut_ptr(),
			)
		};
		assert_eq!(err, PF_Err_NONE);
		assert_px_eq(
			dst.pixel(0, 0),
			px(255, 10, 20, 30),
			"opaque COPY should fully replace dst",
		);
	}

	#[test]
	fn composite_rect_zero_opacity_leaves_dst_unchanged() {
		let mut src = TestWorld::new(2, 2, px(255, 10, 20, 30));
		let mut dst = TestWorld::new(2, 2, px(255, 200, 200, 200));

		unsafe {
			composite_rect_sys(
				std::ptr::null_mut(),
				std::ptr::null_mut(),
				0,
				src.as_mut_ptr(),
				0,
				0,
				PF_Field_FRAME,
				PF_Xfer_IN_FRONT,
				dst.as_mut_ptr(),
			)
		};
		assert_px_eq(
			dst.pixel(0, 0),
			px(255, 200, 200, 200),
			"zero opacity in front should leave dst untouched (COPY replaces)",
		);
	}

	#[test]
	fn copy_hq_same_size_matches_source() {
		let mut src = TestWorld::new(3, 3, px(255, 42, 84, 126));
		let mut dst = TestWorld::new(3, 3, px(0, 0, 0, 0));

		unsafe {
			copy_hq_sys(
				std::ptr::null_mut(),
				src.as_mut_ptr(),
				dst.as_mut_ptr(),
				std::ptr::null_mut(),
				std::ptr::null_mut(),
			)
		};

		for y in 0..3 {
			for x in 0..3 {
				assert_px_eq(dst.pixel(x, y), px(255, 42, 84, 126), &format!("mismatch at ({x},{y})"));
			}
		}
	}

	#[test]
	fn convolve_identity_kernel_is_a_no_op() {
		let mut src = TestWorld::new(3, 3, px(255, 10, 20, 30));
		let mut dst = TestWorld::new(3, 3, px(0, 0, 0, 0));

		// A 3x3 kernel with a single 1 at the center and 0 everywhere else
		// convolves to exactly the source value at every pixel.
		let mut kernel = [0i32; 9];
		kernel[4] = 1;

		let err = unsafe {
			convolve_sys(
				std::ptr::null_mut(),
				src.as_mut_ptr(),
				std::ptr::null(),
				PF_KernelFlag_NORMALIZED,
				3,
				kernel.as_mut_ptr() as *mut c_void,
				kernel.as_mut_ptr() as *mut c_void,
				kernel.as_mut_ptr() as *mut c_void,
				kernel.as_mut_ptr() as *mut c_void,
				dst.as_mut_ptr(),
			)
		};
		assert_eq!(err, PF_Err_NONE);
		for y in 0..3 {
			for x in 0..3 {
				assert_px_eq(dst.pixel(x, y), px(255, 10, 20, 30), &format!("mismatch at ({x},{y})"));
			}
		}
	}

	#[test]
	fn convolve_rejects_aliased_buffers() {
		let mut w = TestWorld::new(2, 2, px(255, 1, 2, 3));
		let ptr = w.as_mut_ptr();
		let kernel = [1i32];
		let err = unsafe {
			convolve_sys(
				std::ptr::null_mut(),
				ptr,
				std::ptr::null(),
				PF_KernelFlag_NORMALIZED,
				1,
				std::ptr::null_mut(),
				std::ptr::null_mut(),
				std::ptr::null_mut(),
				kernel.as_ptr() as *mut c_void,
				ptr,
			)
		};
		assert_eq!(err, PF_Err_BAD_CALLBACK_PARAM);
	}

	#[test]
	fn transform_world_identity_matrix_is_a_no_op() {
		let mut src = TestWorld::new(4, 4, px(255, 5, 6, 7));
		let mut dst = TestWorld::new(4, 4, px(0, 0, 0, 0));
		let comp_mode = PF_CompositeMode {
			xfer: PF_Xfer_COPY,
			rand_seed: 0,
			opacity: 255,
			rgb_only: 0,
			opacitySu: 0,
		};
		let matrix = PF_FloatMatrix { mat: IDENTITY3 };

		let err = unsafe {
			transform_world_sys(
				std::ptr::null_mut(),
				PF_Quality_HI,
				PF_MF_Alpha_STRAIGHT,
				PF_Field_FRAME,
				src.as_mut_ptr(),
				&comp_mode as *const _,
				std::ptr::null(),
				&matrix as *const _,
				1,
				0,
				std::ptr::null(),
				dst.as_mut_ptr(),
			)
		};
		assert_eq!(err, PF_Err_NONE);
		for y in 1..3 {
			for x in 1..3 {
				assert_px_eq(dst.pixel(x, y), px(255, 5, 6, 7), &format!("mismatch at ({x},{y})"));
			}
		}
	}
}

#[cfg(test)]
#[path = "transform_tests.rs"]
mod depth_tests;
