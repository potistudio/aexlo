use after_effects_sys::*;
use std::os::raw::c_void;

use super::super::iterate_8::iterate_8_sys;

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
