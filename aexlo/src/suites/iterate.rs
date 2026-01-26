use crate::core::diagnostics::*;
use after_effects_sys::*;
use std::os::raw::c_void;
use std::sync::atomic::{AtomicI32, Ordering};

use rayon::prelude::*;

pub(super) unsafe extern "C" fn iterate_8_sys(
	in_data: *mut PF_InData,
	progress_base: A_long,
	progress_final: A_long,
	src: *mut PF_EffectWorld,
	area: *const PF_Rect,
	refcon: *mut ::std::os::raw::c_void,
	pix_fn: ::std::option::Option<
		unsafe extern "C" fn(
			refcon: *mut ::std::os::raw::c_void,
			x: A_long,
			y: A_long,
			in_: *mut PF_Pixel,
			out: *mut PF_Pixel,
		) -> PF_Err,
	>,
	dst: *mut PF_EffectWorld,
) -> PF_Err {
	#[cfg(feature = "diagnostics")]
	DiagnosticBuilder::new()
		.set_name("Iterate8Suite/Iterate8")
		.add_arg("in_data", format!("{:?}", in_data))
		.add_arg("progress_base", progress_base)
		.add_arg("progress_final", progress_final)
		.add_arg("src", format!("{:?}", src))
		.add_arg(
			"area",
			if !area.is_null() {
				format!("{:?}", area)
			} else {
				"(null)".to_string()
			},
		)
		.add_arg("refcon", format!("{:?}", refcon))
		.add_arg("pix_fn", if pix_fn.is_some() { "Some" } else { "None" })
		.add_arg("dst", format!("{:?}", dst))
		.set_result(0)
		.emit();

	// Check for NULL pointers to required worlds
	if src.is_null() || dst.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	// SAFETY: We create shared references here.
	// We specifically avoid creating `&mut *dst` to prevent aliasing UB when using Rayon.
	// Mutation of the destination buffer will occur via raw pointers derived from `dst_world.data`.
	let src_world = unsafe { &*src };
	let dst_world = unsafe { &*dst };

	if src_world.data.is_null() || dst_world.data.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	// 1. Determine iteration bounds from `area` or default to `dst` extent
	let mut rect = if !area.is_null() {
		unsafe { *area }
	} else {
		PF_Rect {
			left: 0,
			top: 0,
			right: dst_world.width as i32,
			bottom: dst_world.height as i32,
		}
	};

	// 2. Intersect with Source Bounds
	rect.left = rect.left.max(0);
	rect.top = rect.top.max(0);
	rect.right = rect.right.min(src_world.width as i32);
	rect.bottom = rect.bottom.min(src_world.height as i32);

	// 3. Intersect with Destination Bounds
	rect.right = rect.right.min(dst_world.width as i32);
	rect.bottom = rect.bottom.min(dst_world.height as i32);

	let start_x = rect.left as i32;
	let start_y = rect.top as i32;
	let end_x = rect.right as i32;
	let end_y = rect.bottom as i32;

	let width = (end_x - start_x).max(0);
	let height = (end_y - start_y).max(0);

	if width == 0 || height == 0 {
		return PF_Err_NONE as PF_Err;
	}

	// Prepare pointers and strides for thread-safe access
	let src_base_addr = src_world.data as usize;
	let src_rowbytes = src_world.rowbytes as isize;
	let dst_base_addr = dst_world.data as usize;
	let dst_rowbytes = dst_world.rowbytes as isize;

	let pixel_size = std::mem::size_of::<PF_Pixel8>();

	// Validate pixel size assumption
	// We act as Iterate8, so we assume PF_Pixel8.
	// This debug assertion helps catch if rowbytes doesn't match the width*size expectation (indicating stride or wrong depth).
	debug_assert!(
		src_world.rowbytes >= (src_world.width as i32 * pixel_size as i32),
		"Source rowbytes smaller than width * pixel_size"
	);

	// Cast refcon to usize to allow passing it to threads safely.
	// SAFETY: The caller (After Effects or plugin) implicitly guarantees that `refcon` is thread-safe
	// for concurrent reading/writing if they invoke a parallel suite function or if the plugin design allows it.
	// As a generic suite implementation, we must rely on this contract.
	let refcon_addr = refcon as usize;

	// Atomic for error propagation from threads
	let error_capsule = AtomicI32::new(PF_Err_NONE as i32);

	// Parallel iteration using rayon
	// Iterate over Y rows in parallel
	if let Some(func) = pix_fn {
		(0..height).into_par_iter().for_each(|y_offset| {
			// Check for early exit on error (relaxed ordering is sufficient for "eventual" stop)
			if error_capsule.load(Ordering::Relaxed) != PF_Err_NONE as i32 {
				return;
			}

			let current_y = start_y + y_offset;
			let current_x_start = start_x;

			// Calculate row start (byte offset)
			let src_row_ptr =
				(src_base_addr as *mut u8).wrapping_offset((current_y as isize) * src_rowbytes);
			let dst_row_ptr =
				(dst_base_addr as *mut u8).wrapping_offset((current_y as isize) * dst_rowbytes);

			let refcon_ptr = refcon_addr as *mut c_void;

			// Inner loop: iterate pixels in this row
			for x_offset in 0..width {
				let current_x = current_x_start + x_offset;

				// Calculate pixel pointers
				let src_pixel =
					src_row_ptr.wrapping_add((current_x as usize) * pixel_size) as *mut PF_Pixel8;
				let dst_pixel =
					dst_row_ptr.wrapping_add((current_x as usize) * pixel_size) as *mut PF_Pixel8;

				// SAFETY:
				// 1. `dst_pixel` points to a unique memory location for this (x, y) coordinate.
				//    The iteration ranges (0..height, 0..width) partition the buffer into disjoint sets.
				//    No two threads will write to the same pixel address.
				// 2. We are writing to `dst`, which is allowed via raw pointer even if `dst_world` is shared,
				//    as long as we respect exclusive access rules (guaranteed by partitioning).
				// 3. `src_pixel` is only read.
				// 4. `func` is an external C function. We trust it adheres to the `Iterate` contract.
				unsafe {
					let err = func(refcon_ptr, current_x, current_y, src_pixel, dst_pixel);
					if err != PF_Err_NONE as i32 {
						// Attempt to store the first error. We don't care if we overwrite another error or lose one race.
						error_capsule.store(err, Ordering::Relaxed);
						return; // Stop processing this row
					}
				}
			}
		});
	}

	error_capsule.load(Ordering::Relaxed) as PF_Err
}

// ============================================================================
// Stub Implementations (Logging Only)
// ============================================================================

unsafe extern "C" fn iterate_origin_stub(
	_in_data: *mut PF_InData,
	_progress_base: A_long,
	_progress_final: A_long,
	_src: *mut PF_EffectWorld,
	_area: *const PF_Rect,
	_origin: *const PF_Point,
	_refcon: *mut c_void,
	_pix_fn: ::std::option::Option<
		unsafe extern "C" fn(
			refcon: *mut c_void,
			x: A_long,
			y: A_long,
			in_: *mut PF_Pixel,
			out: *mut PF_Pixel,
		) -> PF_Err,
	>,
	_dst: *mut PF_EffectWorld,
) -> PF_Err {
	log::warn!("STUB: iterate_origin called");
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn iterate_lut_stub(
	_in_data: *mut PF_InData,
	_progress_base: A_long,
	_progress_final: A_long,
	_src: *mut PF_EffectWorld,
	_area: *const PF_Rect,
	_a_lut0: *mut A_u_char,
	_r_lut0: *mut A_u_char,
	_g_lut0: *mut A_u_char,
	_b_lut0: *mut A_u_char,
	_dst: *mut PF_EffectWorld,
) -> PF_Err {
	log::warn!("STUB: iterate_lut called");
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn iterate_origin_non_clip_src_stub(
	_in_data: *mut PF_InData,
	_progress_base: A_long,
	_progress_final: A_long,
	_src: *mut PF_EffectWorld,
	_area: *const PF_Rect,
	_origin: *const PF_Point,
	_refcon: *mut c_void,
	_pix_fn: ::std::option::Option<
		unsafe extern "C" fn(
			refcon: *mut c_void,
			x: A_long,
			y: A_long,
			in_: *mut PF_Pixel,
			out: *mut PF_Pixel,
		) -> PF_Err,
	>,
	_dst: *mut PF_EffectWorld,
) -> PF_Err {
	log::warn!("STUB: iterate_origin_non_clip_src called");
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn iterate_generic_stub(
	_iterationsL: A_long,
	_refconPV: *mut c_void,
	_fn_func: ::std::option::Option<
		unsafe extern "C" fn(
			refconPV: *mut c_void,
			thread_indexL: A_long,
			i: A_long,
			iterationsL: A_long,
		) -> PF_Err,
	>,
) -> PF_Err {
	log::warn!("STUB: iterate_generic called");
	PF_Err_NONE as PF_Err
}

// ============================================================================
// Factory Function
// ============================================================================

/// Creates a dynamically allocated `PF_Iterate8Suite2` instance.
/// All function pointers are populated with either real implementations or logging stubs.
pub fn create_iterate_8_suite_2() -> Box<PF_Iterate8Suite2> {
	Box::new(PF_Iterate8Suite2 {
		iterate: Some(iterate_8_sys),
		iterate_origin: Some(iterate_origin_stub),
		iterate_lut: Some(iterate_lut_stub),
		iterate_origin_non_clip_src: Some(iterate_origin_non_clip_src_stub),
		iterate_generic: Some(iterate_generic_stub),
	})
}
