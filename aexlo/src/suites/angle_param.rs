use after_effects_sys::{
	A_FpLong, PF_AngleParamSuite1, PF_Err, PF_Err_BAD_CALLBACK_PARAM, PF_Err_NONE, PF_ParamDef, PF_ProgPtr,
};

use crate::DiagnosticBuilder;

unsafe extern "C" fn get_floating_point_value_from_angle_def_sys(
	effect_ref: PF_ProgPtr,
	angle_defP: *const PF_ParamDef,
	fp_valueP: *mut A_FpLong,
) -> PF_Err {
	//== Diagnostics ==//
	let mut diagnostics = DiagnosticBuilder::new();
	diagnostics
		.set_name("PF_AngleParamSuite1/PF_GetFloatingPointValueFromAngleDef")
		.add_arg("effect_ref", format!("{:?}", effect_ref))
		.add_arg("angle_defP", format!("{:?}", angle_defP))
		.add_arg("fp_valueP", format!("{:?}", fp_valueP));

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

	//TODO

	diagnostics.set_result(0).emit();
	PF_Err_NONE as PF_Err
}

pub fn create_angle_param_suite() -> Box<PF_AngleParamSuite1> {
	Box::new(PF_AngleParamSuite1 {
		PF_GetFloatingPointValueFromAngleDef: Some(get_floating_point_value_from_angle_def_sys),
	})
}
