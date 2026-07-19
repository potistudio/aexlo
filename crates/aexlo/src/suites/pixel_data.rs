//! `PF_PixelDataSuite2`: hands back the base pixel pointer of an effect world
//! at the caller's chosen depth.
//!
//! Per the SDK these are convenience accessors: `pixelsP0` is an optional
//! pre-fetched pixel pointer (from `checkout_layer_pixels`) that takes
//! precedence; otherwise the world's own `data` pointer is returned. The
//! caller owns knowing the world's actual depth — the worlds this host
//! allocates do not tag their depth in `world_flags`, so no cross-checking is
//! possible here.

use crate::core::diagnostics::diag;
use after_effects_sys::{
	PF_EffectWorld, PF_Err, PF_Err_BAD_CALLBACK_PARAM, PF_Err_NONE, PF_Pixel8, PF_Pixel16, PF_PixelDataSuite2,
	PF_PixelFloat, PF_PixelPtr,
};
use std::os::raw::c_void;

/// Generates one `get_pixel_data*` entry point: prefer `pixelsP0`, fall back
/// to the world's `data` pointer.
macro_rules! define_get_pixel_data {
	($fn_name:ident, $out:ty, $diag_name:literal) => {
		pub(crate) unsafe extern "C" fn $fn_name(
			worldP: *mut PF_EffectWorld,
			pixelsP0: PF_PixelPtr,
			pixPP: *mut *mut $out,
		) -> PF_Err {
			if pixPP.is_null() {
				log::error!(concat!($diag_name, ": pixPP is null"));
				return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
			}

			let data = if !pixelsP0.is_null() {
				pixelsP0 as *mut $out
			} else {
				if worldP.is_null() {
					log::error!(concat!($diag_name, ": both worldP and pixelsP0 are null"));
					return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
				}
				// SAFETY: worldP was just null-checked.
				unsafe { (*worldP).data as *mut $out }
			};

			// SAFETY: pixPP was null-checked above.
			unsafe { *pixPP = data };

			diag!($diag_name,
				"worldP" => format!("{:#x}", worldP as usize),
				"pixelsP0" => format!("{:#x}", pixelsP0 as usize);
				result: format!("{:#x}", data as usize),
			);
			PF_Err_NONE as PF_Err
		}
	};
}

define_get_pixel_data!(get_pixel_data_8_sys, PF_Pixel8, "PixelDataSuite/get_pixel_data8");
define_get_pixel_data!(get_pixel_data_16_sys, PF_Pixel16, "PixelDataSuite/get_pixel_data16");
define_get_pixel_data!(get_pixel_data_float_sys, PF_PixelFloat, "PixelDataSuite/get_pixel_data_float");

/// GPU variant: no `pixelsP0` staging pointer, just the world's device pointer.
pub(crate) unsafe extern "C" fn get_pixel_data_float_gpu_sys(
	worldP: *mut PF_EffectWorld,
	pixPP: *mut *mut c_void,
) -> PF_Err {
	if pixPP.is_null() || worldP.is_null() {
		log::error!("PixelDataSuite/get_pixel_data_float_gpu: null argument");
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	// SAFETY: both pointers were null-checked above.
	unsafe { *pixPP = (*worldP).data as *mut c_void };

	diag!("PixelDataSuite/get_pixel_data_float_gpu",
		"worldP" => format!("{:#x}", worldP as usize),
	);
	PF_Err_NONE as PF_Err
}

/// Builds the `PF_PixelDataSuite2` vtable.
///
/// `const` so it can initialize the shared [`SUITE_CONTAINER`](crate::suites::SUITE_CONTAINER)
/// static; the suite is a stateless table of function pointers.
pub const fn create_pixel_data_suite_2() -> PF_PixelDataSuite2 {
	PF_PixelDataSuite2 {
		get_pixel_data8: Some(get_pixel_data_8_sys),
		get_pixel_data16: Some(get_pixel_data_16_sys),
		get_pixel_data_float: Some(get_pixel_data_float_sys),
		get_pixel_data_float_gpu: Some(get_pixel_data_float_gpu_sys),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn returns_world_data_when_no_staging_pointer() {
		let mut buf = [0u8; 16];
		let mut world: PF_EffectWorld = unsafe { std::mem::zeroed() };
		world.data = buf.as_mut_ptr() as *mut _;

		let mut out: *mut PF_Pixel8 = std::ptr::null_mut();
		let err = unsafe { get_pixel_data_8_sys(&mut world, std::ptr::null_mut(), &mut out) };
		assert_eq!(err, PF_Err_NONE as PF_Err);
		assert_eq!(out as usize, buf.as_ptr() as usize);
	}

	#[test]
	fn staging_pointer_takes_precedence_over_world() {
		let mut staged = [0u16; 8];
		let staged_ptr = staged.as_mut_ptr() as PF_PixelPtr;

		let mut out: *mut PF_Pixel16 = std::ptr::null_mut();
		// worldP null is fine when pixelsP0 is provided.
		let err = unsafe { get_pixel_data_16_sys(std::ptr::null_mut(), staged_ptr, &mut out) };
		assert_eq!(err, PF_Err_NONE as PF_Err);
		assert_eq!(out as usize, staged.as_ptr() as usize);
	}

	#[test]
	fn rejects_null_out_param_and_double_null_source() {
		let err = unsafe { get_pixel_data_float_sys(std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) };
		assert_eq!(err, PF_Err_BAD_CALLBACK_PARAM as PF_Err);

		let mut out: *mut PF_PixelFloat = std::ptr::null_mut();
		let err = unsafe { get_pixel_data_float_sys(std::ptr::null_mut(), std::ptr::null_mut(), &mut out) };
		assert_eq!(err, PF_Err_BAD_CALLBACK_PARAM as PF_Err);
	}

	#[test]
	fn gpu_accessor_returns_world_data() {
		let mut buf = [0u8; 16];
		let mut world: PF_EffectWorld = unsafe { std::mem::zeroed() };
		world.data = buf.as_mut_ptr() as *mut _;

		let mut out: *mut c_void = std::ptr::null_mut();
		let err = unsafe { get_pixel_data_float_gpu_sys(&mut world, &mut out) };
		assert_eq!(err, PF_Err_NONE as PF_Err);
		assert_eq!(out as usize, buf.as_ptr() as usize);
	}
}
