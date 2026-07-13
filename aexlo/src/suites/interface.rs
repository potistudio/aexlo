use after_effects_sys::{
	A_Err, A_FpLong, A_Matrix4, A_Time, A_long, A_short, A_u_long, AEGP_EffectRefH, AEGP_LayerH,
	AEGP_PFInterfaceSuite1, AEGP_PluginID, PF_Err_NONE, PF_ProgPtr,
};

use crate::core::diagnostics::diag;

unsafe extern "C" fn get_effect_layer_sys(_effect_pp_ref: PF_ProgPtr, _layerPH: *mut AEGP_LayerH) -> A_Err {
	diag!("AEGP_PFInterfaceSuite1/AEGP_GetEffectLayer",
		"effect_pp_ref" => format!("{:#x}", _effect_pp_ref as usize),
		"layerPH" => format!("{:#x}", _layerPH as usize),
	);

	PF_Err_NONE as A_Err
}

unsafe extern "C" fn get_new_effect_for_effect_sys(
	_aegp_plugin_id: AEGP_PluginID,
	_effect_pp_ref: PF_ProgPtr,
	_effect_refPH: *mut AEGP_EffectRefH,
) -> A_Err {
	diag!("AEGP_PFInterfaceSuite1/AEGP_GetNewEffectForEffect",
		"aegp_plugin_id" => _aegp_plugin_id as usize,
		"effect_pp_ref" => format!("{:#x}", _effect_pp_ref as usize),
		"effect_refPH" => format!("{:#x}", _effect_refPH as usize),
	);

	PF_Err_NONE as A_Err
}

unsafe extern "C" fn convert_effect_to_comp_time_sys(
	_effect_pp_ref: PF_ProgPtr,
	_what_timeL: A_long,
	_time_scaleLu: A_u_long,
	_comp_timePT: *mut A_Time,
) -> A_Err {
	diag!("AEGP_PFInterfaceSuite1/AEGP_ConvertEffectToCompTime",
		"effect_pp_ref" => format!("{:#x}", _effect_pp_ref as usize),
		"what_timeL" => _what_timeL as usize,
		"time_scaleLu" => _time_scaleLu as usize,
		"comp_timePT" => format!("{:#x}", _comp_timePT as usize),
	);

	PF_Err_NONE as A_Err
}

unsafe extern "C" fn get_effect_camera_sys(
	_effect_pp_ref: PF_ProgPtr,
	_comp_timePT: *const A_Time,
	_camera_layerPH: *mut AEGP_LayerH,
) -> A_Err {
	diag!("AEGP_PFInterfaceSuite1/AEGP_GetEffectCamera",
		"effect_pp_ref" => format!("{:#x}", _effect_pp_ref as usize),
		"comp_timePT" => format!("{:#x}", _comp_timePT as usize),
		"camera_layerPH" => format!("{:#x}", _camera_layerPH as usize),
	);

	PF_Err_NONE as A_Err
}

unsafe extern "C" fn get_effect_camera_matrix(
	_effect_pp_ref: PF_ProgPtr,
	_comp_timePT: *const A_Time,
	_camera_matrixP: *mut A_Matrix4,
	_dist_to_image_planePF: *mut A_FpLong,
	_image_plane_widthPL: *mut A_short,
	_image_plane_heightPL: *mut A_short,
) -> A_Err {
	diag!("AEGP_PFInterfaceSuite1/AEGP_GetEffectCameraMatrix",
		"effect_pp_ref" => format!("{:#x}", _effect_pp_ref as usize),
		"comp_timePT" => format!("{:#x}", _comp_timePT as usize),
		"camera_matrixP" => format!("{:#x}", _camera_matrixP as usize),
		"dist_to_image_planePF" => format!("{:#x}", _dist_to_image_planePF as usize),
		"image_plane_widthPL" => format!("{:#x}", _image_plane_widthPL as usize),
		"image_plane_heightPL" => format!("{:#x}", _image_plane_heightPL as usize),
	);

	PF_Err_NONE as A_Err
}

/// Builds the `AEGP_PFInterfaceSuite1` vtable.
///
/// `const` so it can initialize the shared [`SUITE_CONTAINER`](crate::suites::SUITE_CONTAINER)
/// static; the suite is a stateless table of function pointers.
pub(super) const fn create_aegp_pf_interface_suite() -> AEGP_PFInterfaceSuite1 {
	AEGP_PFInterfaceSuite1 {
		AEGP_GetEffectLayer: Some(get_effect_layer_sys),
		AEGP_GetNewEffectForEffect: Some(get_new_effect_for_effect_sys),
		AEGP_ConvertEffectToCompTime: Some(convert_effect_to_comp_time_sys),
		AEGP_GetEffectCamera: Some(get_effect_camera_sys),
		AEGP_GetEffectCameraMatrix: Some(get_effect_camera_matrix),
	}
}
