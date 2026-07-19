//! Parallel per-pixel iteration for the `PF_Iterate8Suite2`, `PF_iterate16Suite2`
//! and `PF_iterateFloatSuite2` vtables (plus the matching legacy
//! `PF_UtilCallbacks` entries).
//!
//! All entry points funnel into one generic engine, [`iterate_pixels`], that is
//! monomorphized per pixel depth. The engine walks rows in parallel with rayon
//! and propagates the first callback error, mirroring how After Effects splits
//! iteration across cores.
//!
//! # `iterate_origin` semantics
//!
//! `origin` is the position of the *source* world inside the *destination*
//! world (the same convention as `PF_InData::output_origin_x/y`, which is what
//! SDK samples pass here): for a destination pixel `(x, y)` the matching source
//! pixel is `(x - origin.h, y - origin.v)`. The callback receives destination
//! coordinates, exactly like plain `iterate`.
//!
//! The clipped variant only visits destination pixels whose source coordinate
//! falls inside the source world. The `non_clip_src` variant visits the whole
//! requested area; real AE reads whatever memory surrounds the source
//! sub-world there, but our worlds own exactly `rowbytes * height` bytes, so we
//! clamp the source coordinate to the nearest edge pixel instead of reading out
//! of bounds.

#[cfg(feature = "diagnostics")]
use crate::core::diagnostics::DiagnosticBuilder;
use after_effects_sys::*;
use rayon::prelude::*;
use std::os::raw::c_void;
use std::sync::atomic::{AtomicI64, Ordering};

/// Per-pixel callback shape shared by every iterate entry point, generic over
/// the pixel depth `P` (`PF_Pixel`, `PF_Pixel16` or `PF_PixelFloat`).
type IterPixFn<P> =
	unsafe extern "C" fn(refcon: *mut c_void, x: A_long, y: A_long, in_: *mut P, out: *mut P) -> PF_Err;

/// Generic engine behind `iterate`, `iterate_origin` and
/// `iterate_origin_non_clip_src` at every pixel depth.
///
/// Passing a null `origin` (or `(0, 0)`) with `clip_src = true` reproduces the
/// plain `iterate` behavior: the area is intersected with both worlds' bounds.
/// See the module docs for the origin/clipping conventions.
///
/// # Safety
/// `src`/`dst` must describe valid pixel buffers of depth `P` (or be null where
/// documented), and `pix_fn` must be re-entrant, per the AE iterate contract.
unsafe fn iterate_pixels<P>(
	src: *mut PF_EffectWorld,
	area: *const PF_Rect,
	origin: *const PF_Point,
	clip_src: bool,
	refcon: *mut c_void,
	pix_fn: Option<IterPixFn<P>>,
	dst: *mut PF_EffectWorld,
) -> PF_Err {
	if dst.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	// A null `src` is legal: the SDK's iterate then walks `dst` only, which is how
	// generator effects (no input layer) invoke it. Fall back to `dst` as the source
	// so the bounds and per-pixel `in`/`out` pointers coincide, rather than rejecting
	// it as a bad parameter.
	let src = if src.is_null() { dst } else { src };

	// SAFETY: We create shared references here.
	// We specifically avoid creating `&mut *dst` to prevent aliasing UB when using Rayon.
	// Mutation of the destination buffer will occur via raw pointers derived from `dst_world.data`.
	let src_world = &unsafe { *src };
	let dst_world = &unsafe { *dst };

	if src_world.data.is_null() || dst_world.data.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	let (origin_h, origin_v) = if origin.is_null() {
		(0, 0)
	} else {
		let o = unsafe { *origin };
		(o.h, o.v)
	};

	// 1. Determine iteration bounds (dst space) from `area` or default to `dst` extent
	let mut rect = if !area.is_null() {
		unsafe { *area }
	} else {
		PF_Rect {
			left: 0,
			top: 0,
			right: dst_world.width,
			bottom: dst_world.height,
		}
	};

	// 2. Intersect with destination bounds
	rect.left = rect.left.max(0);
	rect.top = rect.top.max(0);
	rect.right = rect.right.min(dst_world.width);
	rect.bottom = rect.bottom.min(dst_world.height);

	// 3. Intersect with the source bounds shifted by `origin` (skipped for the
	// non-clip variant, which clamps per pixel instead).
	if clip_src {
		rect.left = rect.left.max(origin_h);
		rect.top = rect.top.max(origin_v);
		rect.right = rect.right.min(origin_h + src_world.width);
		rect.bottom = rect.bottom.min(origin_v + src_world.height);
	} else if src_world.width <= 0 || src_world.height <= 0 {
		// Non-clip clamping needs at least one source pixel to point at.
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	let start_x = rect.left;
	let start_y = rect.top;

	let width = (rect.right - rect.left).max(0);
	let height = (rect.bottom - rect.top).max(0);

	if width == 0 || height == 0 {
		return PF_Err_NONE as PF_Err;
	}

	// Prepare pointers and strides for thread-safe access
	let src_base_addr = src_world.data as usize;
	let src_rowbytes = src_world.rowbytes as isize;
	let dst_base_addr = dst_world.data as usize;
	let dst_rowbytes = dst_world.rowbytes as isize;

	let pixel_size = std::mem::size_of::<P>();

	// Validate pixel size assumption: catches a stride that can't hold a full row
	// at this depth (indicating a wrong-depth world or corrupt rowbytes).
	debug_assert!(
		src_world.rowbytes >= (src_world.width * pixel_size as i32),
		"Source rowbytes smaller than width * pixel_size"
	);

	// Cast refcon to usize to allow passing it to threads safely.
	// SAFETY: The caller (After Effects or plugin) implicitly guarantees that `refcon` is thread-safe
	// for concurrent reading/writing if they invoke a parallel suite function or if the plugin design allows it.
	// As a generic suite implementation, we must rely on this contract.
	let refcon_addr = refcon as usize;

	// First callback error, if any. `PF_Err` is `u32` on macOS and `i32` elsewhere;
	// staging it through an `i64` sidesteps the platform-dependent atomic type.
	let error_capsule = AtomicI64::new(PF_Err_NONE as i64);

	// Parallel iteration using rayon: rows in parallel, pixels within a row serially.
	if let Some(func) = pix_fn {
		let src_max_x = src_world.width - 1;
		let src_max_y = src_world.height - 1;

		(0..height).into_par_iter().for_each(|y_offset| {
			// Check for early exit on error (relaxed ordering is sufficient for "eventual" stop)
			if error_capsule.load(Ordering::Relaxed) != PF_Err_NONE as i64 {
				return;
			}

			let current_y = start_y + y_offset;
			let src_y = if clip_src {
				current_y - origin_v
			} else {
				(current_y - origin_v).clamp(0, src_max_y)
			};

			// Calculate row start (byte offset)
			let src_row_ptr = (src_base_addr as *const u8).wrapping_offset((src_y as isize) * src_rowbytes);
			let dst_row_ptr = (dst_base_addr as *mut u8).wrapping_offset((current_y as isize) * dst_rowbytes);

			let refcon_ptr = refcon_addr as *mut c_void;

			// Inner loop: iterate pixels in this row
			for x_offset in 0..width {
				let current_x = start_x + x_offset;
				let src_x = if clip_src {
					current_x - origin_h
				} else {
					(current_x - origin_h).clamp(0, src_max_x)
				};

				// Calculate pixel pointers
				let src_pixel = src_row_ptr.wrapping_add((src_x as usize) * pixel_size) as *mut P;
				let dst_pixel = dst_row_ptr.wrapping_add((current_x as usize) * pixel_size) as *mut P;

				// SAFETY:
				// 1. `dst_pixel` points to a unique memory location for this (x, y) coordinate.
				//    The iteration ranges (0..height, 0..width) partition the buffer into disjoint sets.
				//    No two threads will write to the same pixel address.
				// 2. We are writing to `dst`, which is allowed via raw pointer even if `dst_world` is shared,
				//    as long as we respect exclusive access rules (guaranteed by partitioning).
				// 3. `src_pixel` is only read (clip/clamp keeps it inside the source buffer).
				// 4. `func` is an external C function. We trust it adheres to the `Iterate` contract.
				let err = unsafe { func(refcon_ptr, current_x, current_y, src_pixel, dst_pixel) };
				if err != PF_Err_NONE as PF_Err {
					// Attempt to store the first error. We don't care if we overwrite another error or lose one race.
					error_capsule.store(err as i64, Ordering::Relaxed);
					return; // Stop processing this row
				}
			}
		});
	}

	error_capsule.load(Ordering::Relaxed) as PF_Err
}

/// Generates the `extern "C"` iterate entry points for one pixel depth: the
/// plain `iterate`, the clipped `iterate_origin` and the clamping
/// `iterate_origin_non_clip_src`, all delegating to [`iterate_pixels`].
macro_rules! define_iterate_entries {
	($pix:ty, $iterate:ident, $origin:ident, $non_clip:ident, $diag_prefix:literal) => {
		pub(crate) unsafe extern "C" fn $iterate(
			_in_data: *mut PF_InData,
			_progress_base: A_long,
			_progress_final: A_long,
			src: *mut PF_EffectWorld,
			area: *const PF_Rect,
			refcon: *mut c_void,
			pix_fn: Option<IterPixFn<$pix>>,
			dst: *mut PF_EffectWorld,
		) -> PF_Err {
			#[cfg(feature = "diagnostics")]
			emit_iterate_diag(concat!($diag_prefix, "/iterate"), src, area, std::ptr::null(), refcon, pix_fn.is_some(), dst);

			unsafe { iterate_pixels::<$pix>(src, area, std::ptr::null(), true, refcon, pix_fn, dst) }
		}

		pub(crate) unsafe extern "C" fn $origin(
			_in_data: *mut PF_InData,
			_progress_base: A_long,
			_progress_final: A_long,
			src: *mut PF_EffectWorld,
			area: *const PF_Rect,
			origin: *const PF_Point,
			refcon: *mut c_void,
			pix_fn: Option<IterPixFn<$pix>>,
			dst: *mut PF_EffectWorld,
		) -> PF_Err {
			#[cfg(feature = "diagnostics")]
			emit_iterate_diag(concat!($diag_prefix, "/iterate_origin"), src, area, origin, refcon, pix_fn.is_some(), dst);

			unsafe { iterate_pixels::<$pix>(src, area, origin, true, refcon, pix_fn, dst) }
		}

		pub(crate) unsafe extern "C" fn $non_clip(
			_in_data: *mut PF_InData,
			_progress_base: A_long,
			_progress_final: A_long,
			src: *mut PF_EffectWorld,
			area: *const PF_Rect,
			origin: *const PF_Point,
			refcon: *mut c_void,
			pix_fn: Option<IterPixFn<$pix>>,
			dst: *mut PF_EffectWorld,
		) -> PF_Err {
			#[cfg(feature = "diagnostics")]
			emit_iterate_diag(
				concat!($diag_prefix, "/iterate_origin_non_clip_src"),
				src, area, origin, refcon, pix_fn.is_some(), dst,
			);

			unsafe { iterate_pixels::<$pix>(src, area, origin, false, refcon, pix_fn, dst) }
		}
	};
}

define_iterate_entries!(PF_Pixel, iterate_8_sys, iterate_origin_8_sys, iterate_origin_non_clip_src_8_sys, "Iterate8Suite");
define_iterate_entries!(
	PF_Pixel16,
	iterate_16_sys,
	iterate_origin_16_sys,
	iterate_origin_non_clip_src_16_sys,
	"Iterate16Suite"
);
define_iterate_entries!(
	PF_PixelFloat,
	iterate_float_sys,
	iterate_origin_float_sys,
	iterate_origin_non_clip_src_float_sys,
	"IterateFloatSuite"
);

/// Shared diagnostics emitter for the macro-generated entry points.
#[cfg(feature = "diagnostics")]
fn emit_iterate_diag(
	name: &'static str,
	src: *mut PF_EffectWorld,
	area: *const PF_Rect,
	origin: *const PF_Point,
	refcon: *mut c_void,
	has_pix_fn: bool,
	dst: *mut PF_EffectWorld,
) {
	let mut builder = DiagnosticBuilder::new();
	builder.set_name(name);
	builder.add_arg("src", format!("{:?}", src));
	builder.add_arg(
		"area",
		if !area.is_null() {
			format!("{:?}", unsafe { *area })
		} else {
			"(null)".to_string()
		},
	);
	if !origin.is_null() {
		// PF_Point doesn't derive Debug in the bindings; format the fields directly.
		let o = unsafe { *origin };
		builder.add_arg("origin", format!("({}, {})", o.h, o.v));
	}
	builder.add_arg("refcon", format!("{:?}", refcon));
	builder.add_arg("pix_fn", if has_pix_fn { "Some" } else { "None" });
	builder.add_arg("dst", format!("{:?}", dst));
	builder.emit();
}

/// Real `iterate_lut`: map `src` through per-channel 256-entry lookup tables
/// into `dst`. Any null table (`_lut0` suffix = optional) is the identity.
pub(crate) unsafe extern "C" fn iterate_lut_sys(
	_in_data: *mut PF_InData,
	_progress_base: A_long,
	_progress_final: A_long,
	src: *mut PF_EffectWorld,
	area: *const PF_Rect,
	a_lut0: *mut A_u_char,
	r_lut0: *mut A_u_char,
	g_lut0: *mut A_u_char,
	b_lut0: *mut A_u_char,
	dst: *mut PF_EffectWorld,
) -> PF_Err {
	// Copy each table up front: a local array is cheap, can be shared across the
	// rayon rows below, and a null table falls back to the identity mapping.
	let load_lut = |lut: *mut A_u_char| -> [u8; 256] {
		if lut.is_null() {
			std::array::from_fn(|i| i as u8)
		} else {
			// SAFETY: a non-null LUT is a 256-entry table per the AE contract.
			std::array::from_fn(|i| unsafe { *lut.add(i) })
		}
	};
	let a_lut = load_lut(a_lut0);
	let r_lut = load_lut(r_lut0);
	let g_lut = load_lut(g_lut0);
	let b_lut = load_lut(b_lut0);

	// The LUT is applied by our own per-pixel closure, so route through the same
	// engine with a private trampoline: refcon carries the tables.
	struct LutTables {
		a: [u8; 256],
		r: [u8; 256],
		g: [u8; 256],
		b: [u8; 256],
	}
	unsafe extern "C" fn apply_lut(
		refcon: *mut c_void,
		_x: A_long,
		_y: A_long,
		in_: *mut PF_Pixel,
		out: *mut PF_Pixel,
	) -> PF_Err {
		// SAFETY: refcon is the LutTables local below; in_/out come from iterate_pixels.
		let luts = unsafe { &*(refcon as *const LutTables) };
		let px = unsafe { *in_ };
		unsafe {
			*out = PF_Pixel {
				alpha: luts.a[px.alpha as usize],
				red: luts.r[px.red as usize],
				green: luts.g[px.green as usize],
				blue: luts.b[px.blue as usize],
			};
		}
		PF_Err_NONE as PF_Err
	}

	let tables = LutTables {
		a: a_lut,
		r: r_lut,
		g: g_lut,
		b: b_lut,
	};

	unsafe {
		iterate_pixels::<PF_Pixel>(
			src,
			area,
			std::ptr::null(),
			true,
			&tables as *const LutTables as *mut c_void,
			Some(apply_lut),
			dst,
		)
	}
}

/// Real `iterate_generic`: run `fn_func` for every `i` in `0..iterationsL`,
/// in parallel, passing the rayon worker index as `thread_indexL` (plugins use
/// it to index per-thread scratch buffers, so it must be dense and small).
pub(crate) unsafe extern "C" fn iterate_generic_sys(
	iterationsL: A_long,
	refconPV: *mut c_void,
	fn_func: ::std::option::Option<
		unsafe extern "C" fn(refconPV: *mut c_void, thread_indexL: A_long, i: A_long, iterationsL: A_long) -> PF_Err,
	>,
) -> PF_Err {
	let Some(func) = fn_func else {
		log::error!("iterate_generic: fn_func is null");
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	};

	if iterationsL <= 0 {
		return PF_Err_NONE as PF_Err;
	}

	// Cast refcon to usize so it can cross the rayon closure; the plugin owns
	// its thread-safety by contract (same as `iterate`).
	let refcon_addr = refconPV as usize;

	let error_capsule = AtomicI64::new(PF_Err_NONE as i64);

	(0..iterationsL).into_par_iter().for_each(|i| {
		if error_capsule.load(Ordering::Relaxed) != PF_Err_NONE as i64 {
			return;
		}

		let thread_index = rayon::current_thread_index().unwrap_or(0) as A_long;

		// SAFETY: `func` is the plugin's iteration callback; each `i` is visited
		// exactly once and the plugin guarantees refcon thread-safety per the
		// iterate_generic contract.
		let err = unsafe { func(refcon_addr as *mut c_void, thread_index, i, iterationsL) };
		if err != PF_Err_NONE as PF_Err {
			error_capsule.store(err as i64, Ordering::Relaxed);
		}
	});

	error_capsule.load(Ordering::Relaxed) as PF_Err
}

// ============================================================================
// Factory Functions
// ============================================================================

/// Builds the `PF_Iterate8Suite2` vtable.
///
/// `const` so it can initialize the shared [`SUITE_CONTAINER`](crate::suites::SUITE_CONTAINER)
/// static; the suite is a stateless table of function pointers.
pub const fn create_iterate_8_suite_2() -> PF_Iterate8Suite2 {
	PF_Iterate8Suite2 {
		iterate: Some(iterate_8_sys),
		iterate_origin: Some(iterate_origin_8_sys),
		iterate_lut: Some(iterate_lut_sys),
		iterate_origin_non_clip_src: Some(iterate_origin_non_clip_src_8_sys),
		iterate_generic: Some(iterate_generic_sys),
	}
}

/// Builds the `PF_iterate16Suite2` vtable.
///
/// `const` so it can initialize the shared [`SUITE_CONTAINER`](crate::suites::SUITE_CONTAINER)
/// static; the suite is a stateless table of function pointers.
pub const fn create_iterate_16_suite_2() -> PF_iterate16Suite2 {
	PF_iterate16Suite2 {
		iterate: Some(iterate_16_sys),
		iterate_origin: Some(iterate_origin_16_sys),
		iterate_origin_non_clip_src: Some(iterate_origin_non_clip_src_16_sys),
	}
}

/// Builds the `PF_iterateFloatSuite2` vtable.
///
/// `const` so it can initialize the shared [`SUITE_CONTAINER`](crate::suites::SUITE_CONTAINER)
/// static; the suite is a stateless table of function pointers.
pub const fn create_iterate_float_suite_2() -> PF_iterateFloatSuite2 {
	PF_iterateFloatSuite2 {
		iterate: Some(iterate_float_sys),
		iterate_origin: Some(iterate_origin_float_sys),
		iterate_origin_non_clip_src: Some(iterate_origin_non_clip_src_float_sys),
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::sync::atomic::AtomicI64;

	/// Builds a world over `buf` (len must be `width * height`).
	fn world_over<P>(width: i32, height: i32, buf: &mut [P]) -> PF_EffectWorld {
		assert_eq!(buf.len(), (width * height) as usize);
		let mut world: PF_EffectWorld = unsafe { std::mem::zeroed() };
		world.width = width;
		world.height = height;
		world.rowbytes = width * std::mem::size_of::<P>() as i32;
		world.data = buf.as_mut_ptr() as *mut _;
		world
	}

	unsafe extern "C" fn accumulate(refcon: *mut c_void, _thread: A_long, i: A_long, _n: A_long) -> PF_Err {
		let sum = unsafe { &*(refcon as *const AtomicI64) };
		sum.fetch_add(i as i64, Ordering::Relaxed);
		PF_Err_NONE as PF_Err
	}

	unsafe extern "C" fn fail_at_zero(_refcon: *mut c_void, _thread: A_long, i: A_long, _n: A_long) -> PF_Err {
		if i == 0 {
			PF_Err_BAD_CALLBACK_PARAM as PF_Err
		} else {
			PF_Err_NONE as PF_Err
		}
	}

	#[test]
	fn iterate_generic_visits_every_index_once() {
		let sum = AtomicI64::new(0);
		let err = unsafe { iterate_generic_sys(100, &sum as *const _ as *mut c_void, Some(accumulate)) };
		assert_eq!(err, PF_Err_NONE as PF_Err);
		// 0 + 1 + ... + 99
		assert_eq!(sum.load(Ordering::Relaxed), 4950);
	}

	#[test]
	fn iterate_generic_propagates_callback_errors() {
		let err = unsafe { iterate_generic_sys(8, std::ptr::null_mut(), Some(fail_at_zero)) };
		assert_eq!(err, PF_Err_BAD_CALLBACK_PARAM as PF_Err);
	}

	#[test]
	fn iterate_generic_rejects_null_fn_and_accepts_zero_iterations() {
		assert_eq!(
			unsafe { iterate_generic_sys(10, std::ptr::null_mut(), None) },
			PF_Err_BAD_CALLBACK_PARAM as PF_Err
		);
		assert_eq!(
			unsafe { iterate_generic_sys(0, std::ptr::null_mut(), Some(accumulate)) },
			PF_Err_NONE as PF_Err
		);
	}

	unsafe extern "C" fn invert_16(
		_refcon: *mut c_void,
		_x: A_long,
		_y: A_long,
		in_: *mut PF_Pixel16,
		out: *mut PF_Pixel16,
	) -> PF_Err {
		let px = unsafe { *in_ };
		unsafe {
			*out = PF_Pixel16 {
				alpha: px.alpha,
				red: 32768 - px.red,
				green: 32768 - px.green,
				blue: 32768 - px.blue,
			};
		}
		PF_Err_NONE as PF_Err
	}

	#[test]
	fn iterate_16_transforms_every_pixel() {
		let w = 4;
		let h = 3;
		let px = PF_Pixel16 {
			alpha: 32768,
			red: 32768,
			green: 8192,
			blue: 0,
		};
		let mut src_buf = vec![px; (w * h) as usize];
		let mut dst_buf = vec![PF_Pixel16 { alpha: 0, red: 0, green: 0, blue: 0 }; (w * h) as usize];
		let mut src = world_over(w, h, &mut src_buf);
		let mut dst = world_over(w, h, &mut dst_buf);

		let err = unsafe {
			iterate_16_sys(
				std::ptr::null_mut(),
				0,
				0,
				&mut src,
				std::ptr::null(),
				std::ptr::null_mut(),
				Some(invert_16),
				&mut dst,
			)
		};
		assert_eq!(err, PF_Err_NONE as PF_Err);
		for p in &dst_buf {
			assert_eq!((p.alpha, p.red, p.green, p.blue), (32768, 0, 24576, 32768));
		}
	}

	unsafe extern "C" fn copy_float(
		_refcon: *mut c_void,
		_x: A_long,
		_y: A_long,
		in_: *mut PF_PixelFloat,
		out: *mut PF_PixelFloat,
	) -> PF_Err {
		unsafe { *out = *in_ };
		PF_Err_NONE as PF_Err
	}

	#[test]
	fn iterate_float_respects_area_and_leaves_rest_untouched() {
		let w = 4;
		let h = 4;
		let src_px = PF_PixelFloat {
			alpha: 1.0,
			red: 0.25,
			green: 0.5,
			blue: 0.75,
		};
		let sentinel = PF_PixelFloat {
			alpha: -1.0,
			red: -1.0,
			green: -1.0,
			blue: -1.0,
		};
		let mut src_buf = vec![src_px; (w * h) as usize];
		let mut dst_buf = vec![sentinel; (w * h) as usize];
		let mut src = world_over(w, h, &mut src_buf);
		let mut dst = world_over(w, h, &mut dst_buf);

		let area = PF_Rect {
			left: 1,
			top: 1,
			right: 3,
			bottom: 3,
		};
		let err = unsafe {
			iterate_float_sys(
				std::ptr::null_mut(),
				0,
				0,
				&mut src,
				&area,
				std::ptr::null_mut(),
				Some(copy_float),
				&mut dst,
			)
		};
		assert_eq!(err, PF_Err_NONE as PF_Err);
		for y in 0..h {
			for x in 0..w {
				let p = dst_buf[(y * w + x) as usize];
				let inside = (1..3).contains(&x) && (1..3).contains(&y);
				if inside {
					assert_eq!(p.red, 0.25, "({x},{y}) should be copied");
				} else {
					assert_eq!(p.red, -1.0, "({x},{y}) should be untouched");
				}
			}
		}
	}

	/// Records the source red channel into dst; used to verify origin mapping.
	unsafe extern "C" fn copy_8(
		_refcon: *mut c_void,
		_x: A_long,
		_y: A_long,
		in_: *mut PF_Pixel,
		out: *mut PF_Pixel,
	) -> PF_Err {
		unsafe { *out = *in_ };
		PF_Err_NONE as PF_Err
	}

	/// 2x2 source pasted into a 4x4 destination at origin (1, 1): only the
	/// covered quad is visited and each dst pixel gets src[(x-1, y-1)].
	#[test]
	fn iterate_origin_offsets_and_clips_to_source() {
		// Source pixel encodes its own coordinates: red = x, green = y.
		let sw = 2;
		let sh = 2;
		let mut src_buf: Vec<PF_Pixel> = (0..sh)
			.flat_map(|y| {
				(0..sw).map(move |x| PF_Pixel {
					alpha: 255,
					red: x as u8,
					green: y as u8,
					blue: 0,
				})
			})
			.collect();
		let sentinel = PF_Pixel {
			alpha: 9,
			red: 9,
			green: 9,
			blue: 9,
		};
		let dw = 4;
		let dh = 4;
		let mut dst_buf = vec![sentinel; (dw * dh) as usize];
		let mut src = world_over(sw, sh, &mut src_buf);
		let mut dst = world_over(dw, dh, &mut dst_buf);

		let origin = PF_Point { h: 1, v: 1 };
		let err = unsafe {
			iterate_origin_8_sys(
				std::ptr::null_mut(),
				0,
				0,
				&mut src,
				std::ptr::null(),
				&origin,
				std::ptr::null_mut(),
				Some(copy_8),
				&mut dst,
			)
		};
		assert_eq!(err, PF_Err_NONE as PF_Err);
		for y in 0..dh {
			for x in 0..dw {
				let p = dst_buf[(y * dw + x) as usize];
				let covered = (1..3).contains(&x) && (1..3).contains(&y);
				if covered {
					assert_eq!((p.red, p.green), ((x - 1) as u8, (y - 1) as u8), "dst ({x},{y})");
				} else {
					assert_eq!(p.red, 9, "dst ({x},{y}) outside the shifted source must stay untouched");
				}
			}
		}
	}

	/// The non-clip variant visits the whole destination; out-of-source pixels
	/// read the clamped edge pixel instead of memory outside the buffer.
	#[test]
	fn iterate_origin_non_clip_clamps_source_to_edges() {
		let sw = 2;
		let sh = 2;
		let mut src_buf: Vec<PF_Pixel> = (0..sh)
			.flat_map(|y| {
				(0..sw).map(move |x| PF_Pixel {
					alpha: 255,
					red: (10 + x) as u8,
					green: (10 + y) as u8,
					blue: 0,
				})
			})
			.collect();
		let dw = 4;
		let dh = 4;
		let mut dst_buf = vec![
			PF_Pixel {
				alpha: 0,
				red: 0,
				green: 0,
				blue: 0
			};
			(dw * dh) as usize
		];
		let mut src = world_over(sw, sh, &mut src_buf);
		let mut dst = world_over(dw, dh, &mut dst_buf);

		let origin = PF_Point { h: 1, v: 1 };
		let err = unsafe {
			iterate_origin_non_clip_src_8_sys(
				std::ptr::null_mut(),
				0,
				0,
				&mut src,
				std::ptr::null(),
				&origin,
				std::ptr::null_mut(),
				Some(copy_8),
				&mut dst,
			)
		};
		assert_eq!(err, PF_Err_NONE as PF_Err);
		for y in 0..dh {
			for x in 0..dw {
				let p = dst_buf[(y * dw + x) as usize];
				let expect_x = (x - 1).clamp(0, sw - 1);
				let expect_y = (y - 1).clamp(0, sh - 1);
				assert_eq!(
					(p.red, p.green),
					((10 + expect_x) as u8, (10 + expect_y) as u8),
					"dst ({x},{y})"
				);
			}
		}
	}

	#[test]
	fn iterate_lut_maps_channels_and_defaults_null_luts_to_identity() {
		let w = 2;
		let h = 2;
		let px = PF_Pixel {
			alpha: 200,
			red: 3,
			green: 7,
			blue: 11,
		};
		let mut src_buf = vec![px; (w * h) as usize];
		let mut dst_buf = vec![
			PF_Pixel {
				alpha: 0,
				red: 0,
				green: 0,
				blue: 0
			};
			(w * h) as usize
		];
		let mut src = world_over(w, h, &mut src_buf);
		let mut dst = world_over(w, h, &mut dst_buf);

		// Red LUT doubles the value; alpha/green/blue tables stay null (identity).
		let mut r_lut = [0u8; 256];
		for (i, v) in r_lut.iter_mut().enumerate() {
			*v = (i * 2).min(255) as u8;
		}

		let err = unsafe {
			iterate_lut_sys(
				std::ptr::null_mut(),
				0,
				0,
				&mut src,
				std::ptr::null(),
				std::ptr::null_mut(),
				r_lut.as_mut_ptr(),
				std::ptr::null_mut(),
				std::ptr::null_mut(),
				&mut dst,
			)
		};
		assert_eq!(err, PF_Err_NONE as PF_Err);
		for p in &dst_buf {
			assert_eq!((p.alpha, p.red, p.green, p.blue), (200, 6, 7, 11));
		}
	}

	unsafe extern "C" fn fail_pixel_16(
		_refcon: *mut c_void,
		_x: A_long,
		_y: A_long,
		_in: *mut PF_Pixel16,
		_out: *mut PF_Pixel16,
	) -> PF_Err {
		PF_Err_OUT_OF_MEMORY as PF_Err
	}

	#[test]
	fn iterate_16_propagates_pixel_errors_and_rejects_null_dst() {
		let w = 2;
		let h = 2;
		let mut buf = vec![PF_Pixel16 { alpha: 0, red: 0, green: 0, blue: 0 }; (w * h) as usize];
		let mut world = world_over(w, h, &mut buf);

		let err = unsafe {
			iterate_16_sys(
				std::ptr::null_mut(),
				0,
				0,
				&mut world,
				std::ptr::null(),
				std::ptr::null_mut(),
				Some(fail_pixel_16),
				&mut world,
			)
		};
		assert_eq!(err, PF_Err_OUT_OF_MEMORY as PF_Err);

		let err = unsafe {
			iterate_16_sys(
				std::ptr::null_mut(),
				0,
				0,
				&mut world,
				std::ptr::null(),
				std::ptr::null_mut(),
				Some(fail_pixel_16),
				std::ptr::null_mut(),
			)
		};
		assert_eq!(err, PF_Err_BAD_CALLBACK_PARAM as PF_Err);
	}
}
