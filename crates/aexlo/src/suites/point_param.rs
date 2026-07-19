//! `PF_PointParamSuite1`: reads a POINT param's value as floating-point
//! coordinates. A POINT param stores its value as two 16.16 `PF_Fixed`
//! coordinates; the suite reports them as `A_FloatPoint` in the same units
//! (the fixed→float conversion does not change the coordinate space).

use after_effects_sys::{
	A_FloatPoint, PF_Err, PF_Err_BAD_CALLBACK_PARAM, PF_Err_NONE, PF_ParamDef, PF_PointParamSuite1, PF_ProgPtr,
};

use crate::core::diagnostics::diag;

unsafe extern "C" fn get_floating_point_value_from_point_def_sys(
	effect_ref: PF_ProgPtr,
	point_defP: *const PF_ParamDef,
	fp_pointP: *mut A_FloatPoint,
) -> PF_Err {
	//== Validation ==//
	if effect_ref.is_null() {
		log::error!("the arg `effect_ref` is null.");
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	if point_defP.is_null() {
		log::error!("the arg `point_defP` is null.");
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	if fp_pointP.is_null() {
		log::error!("the arg `fp_pointP` is null.");
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	//== Implementation ==//
	let td = unsafe { &(*point_defP).u.td };
	let point = A_FloatPoint {
		x: td.x_value as f64 / 65536.0,
		y: td.y_value as f64 / 65536.0,
	};
	unsafe { *fp_pointP = point };

	diag!("PF_PointParamSuite1/PF_GetFloatingPointValueFromPointDef",
		"effect_ref" => format!("{:?}", effect_ref),
		"point_defP" => format!("{:?}", point_defP);
		result: format!("({}, {})", point.x, point.y),
	);
	PF_Err_NONE as PF_Err
}

/// Builds the `PF_PointParamSuite1` vtable.
///
/// `const` so it can initialize the shared [`SUITE_CONTAINER`](crate::suites::SUITE_CONTAINER)
/// static; the suite is a stateless table of function pointers.
pub const fn create_point_param_suite_1() -> PF_PointParamSuite1 {
	PF_PointParamSuite1 {
		PF_GetFloatingPointValueFromPointDef: Some(get_floating_point_value_from_point_def_sys),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn converts_fixed_point_coordinates_to_float() {
		let mut param: PF_ParamDef = unsafe { std::mem::zeroed() };
		param.u.td.x_value = 3 * 65536 + 32768; // 3.5
		param.u.td.y_value = -2 * 65536; // -2.0

		let mut out = A_FloatPoint { x: 0.0, y: 0.0 };
		let effect_ref = 1usize as PF_ProgPtr;
		let err = unsafe { get_floating_point_value_from_point_def_sys(effect_ref, &param, &mut out) };
		assert_eq!(err, PF_Err_NONE as PF_Err);
		assert_eq!(out.x, 3.5);
		assert_eq!(out.y, -2.0);
	}

	#[test]
	fn rejects_null_arguments() {
		let param: PF_ParamDef = unsafe { std::mem::zeroed() };
		let mut out = A_FloatPoint { x: 0.0, y: 0.0 };
		let effect_ref = 1usize as PF_ProgPtr;
		assert_eq!(
			unsafe { get_floating_point_value_from_point_def_sys(std::ptr::null_mut(), &param, &mut out) },
			PF_Err_BAD_CALLBACK_PARAM as PF_Err
		);
		assert_eq!(
			unsafe { get_floating_point_value_from_point_def_sys(effect_ref, std::ptr::null(), &mut out) },
			PF_Err_BAD_CALLBACK_PARAM as PF_Err
		);
		assert_eq!(
			unsafe { get_floating_point_value_from_point_def_sys(effect_ref, &param, std::ptr::null_mut()) },
			PF_Err_BAD_CALLBACK_PARAM as PF_Err
		);
	}
}
