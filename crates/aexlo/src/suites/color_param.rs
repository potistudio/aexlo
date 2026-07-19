//! `PF_ColorParamSuite1`: reads a COLOR param's value as floating-point
//! channels. A COLOR param stores its value as an 8-bpc `PF_Pixel`; the suite
//! reports each channel normalized to `0.0..=1.0`.

use after_effects_sys::{
	PF_ColorParamSuite1, PF_Err, PF_Err_BAD_CALLBACK_PARAM, PF_Err_NONE, PF_MAX_CHAN8, PF_ParamDef, PF_PixelFloat,
	PF_ProgPtr,
};

use crate::core::diagnostics::diag;

unsafe extern "C" fn get_floating_point_color_from_color_def_sys(
	effect_ref: PF_ProgPtr,
	color_defP: *const PF_ParamDef,
	fp_colorP: *mut PF_PixelFloat,
) -> PF_Err {
	//== Validation ==//
	if effect_ref.is_null() {
		log::error!("the arg `effect_ref` is null.");
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	if color_defP.is_null() {
		log::error!("the arg `color_defP` is null.");
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	if fp_colorP.is_null() {
		log::error!("the arg `fp_colorP` is null.");
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	//== Implementation ==//
	let value = unsafe { (*color_defP).u.cd.value };
	let max = PF_MAX_CHAN8 as f32;
	let color = PF_PixelFloat {
		alpha: value.alpha as f32 / max,
		red: value.red as f32 / max,
		green: value.green as f32 / max,
		blue: value.blue as f32 / max,
	};
	unsafe { *fp_colorP = color };

	diag!("PF_ColorParamSuite1/PF_GetFloatingPointColorFromColorDef",
		"effect_ref" => format!("{:?}", effect_ref),
		"color_defP" => format!("{:?}", color_defP);
		result: format!("({}, {}, {}, {})", color.alpha, color.red, color.green, color.blue),
	);
	PF_Err_NONE as PF_Err
}

/// Builds the `PF_ColorParamSuite1` vtable.
///
/// `const` so it can initialize the shared [`SUITE_CONTAINER`](crate::suites::SUITE_CONTAINER)
/// static; the suite is a stateless table of function pointers.
pub const fn create_color_param_suite_1() -> PF_ColorParamSuite1 {
	PF_ColorParamSuite1 {
		PF_GetFloatingPointColorFromColorDef: Some(get_floating_point_color_from_color_def_sys),
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use after_effects_sys::PF_Pixel;

	#[test]
	fn converts_8bit_color_value_to_normalized_float() {
		let mut param: PF_ParamDef = unsafe { std::mem::zeroed() };
		param.u.cd.value = PF_Pixel {
			alpha: 255,
			red: 255,
			green: 51,
			blue: 0,
		};

		let mut out = PF_PixelFloat {
			alpha: -1.0,
			red: -1.0,
			green: -1.0,
			blue: -1.0,
		};
		let effect_ref = 1usize as PF_ProgPtr;
		let err = unsafe { get_floating_point_color_from_color_def_sys(effect_ref, &param, &mut out) };
		assert_eq!(err, PF_Err_NONE as PF_Err);
		assert_eq!(out.alpha, 1.0);
		assert_eq!(out.red, 1.0);
		assert!((out.green - 0.2).abs() < 1e-6);
		assert_eq!(out.blue, 0.0);
	}

	#[test]
	fn rejects_null_arguments() {
		let param: PF_ParamDef = unsafe { std::mem::zeroed() };
		let mut out: PF_PixelFloat = unsafe { std::mem::zeroed() };
		let effect_ref = 1usize as PF_ProgPtr;
		assert_eq!(
			unsafe { get_floating_point_color_from_color_def_sys(std::ptr::null_mut(), &param, &mut out) },
			PF_Err_BAD_CALLBACK_PARAM as PF_Err
		);
		assert_eq!(
			unsafe { get_floating_point_color_from_color_def_sys(effect_ref, std::ptr::null(), &mut out) },
			PF_Err_BAD_CALLBACK_PARAM as PF_Err
		);
		assert_eq!(
			unsafe { get_floating_point_color_from_color_def_sys(effect_ref, &param, std::ptr::null_mut()) },
			PF_Err_BAD_CALLBACK_PARAM as PF_Err
		);
	}
}
