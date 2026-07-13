use after_effects_sys::{
	A_FpLong, PF_AngleParamSuite1, PF_Err, PF_Err_BAD_CALLBACK_PARAM, PF_Err_NONE, PF_ParamDef, PF_ProgPtr,
};

use crate::core::diagnostics::diag;

unsafe extern "C" fn get_floating_point_value_from_angle_def_sys(
	effect_ref: PF_ProgPtr,
	angle_defP: *const PF_ParamDef,
	fp_valueP: *mut A_FpLong,
) -> PF_Err {
	//== Validation ==//
	if effect_ref.is_null() {
		log::error!("the arg `effect_ref` is null.");
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	if angle_defP.is_null() {
		log::error!("the arg `angle_defP` is null.");
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	if fp_valueP.is_null() {
		log::error!("the arg `fp_valueP` is null.");
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	//== Implementation ==//
	// An ANGLE param stores its value as PF_Fixed (16.16); the suite reports it
	// as floating-point degrees. Leaving the out-param unwritten would hand the
	// plugin stack garbage, so always write it.
	let degrees = unsafe { (*angle_defP).u.ad.value } as f64 / 65536.0;
	unsafe { *fp_valueP = degrees };

	diag!("PF_AngleParamSuite1/PF_GetFloatingPointValueFromAngleDef",
		"effect_ref" => format!("{:?}", effect_ref),
		"angle_defP" => format!("{:?}", angle_defP),
		"fp_valueP" => format!("{:?}", fp_valueP);
		result: degrees,
	);
	PF_Err_NONE as PF_Err
}

/// Builds the `PF_AngleParamSuite1` vtable.
///
/// `const` so it can initialize the shared [`SUITE_CONTAINER`](crate::suites::SUITE_CONTAINER)
/// static; the suite is a stateless table of function pointers.
pub const fn create_angle_param_suite() -> PF_AngleParamSuite1 {
	PF_AngleParamSuite1 {
		PF_GetFloatingPointValueFromAngleDef: Some(get_floating_point_value_from_angle_def_sys),
	}
}
