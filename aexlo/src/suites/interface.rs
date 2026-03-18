use after_effects_sys::{
	A_Err, A_FpLong, A_Matrix4, A_Time, A_long, A_short, A_u_long, AEGP_EffectRefH, AEGP_LayerH,
	AEGP_PFInterfaceSuite1, AEGP_PluginID, PF_Err_NONE, PF_ProgPtr,
};

use crate::DiagnosticBuilder;

unsafe extern "C" fn get_effect_layer_sys(effect_pp_ref: PF_ProgPtr, layerPH: *mut AEGP_LayerH) -> A_Err {
	DiagnosticBuilder::new()
		.set_name("AEGP_PFInterfaceSuite1/AEGP_GetEffectLayer")
		.add_arg("effect_pp_ref", effect_pp_ref as usize)
		.add_arg("layerPH", layerPH as usize)
		.emit();

	PF_Err_NONE as A_Err
}

unsafe extern "C" fn get_new_effect_for_effect_sys(
	aegp_plugin_id: AEGP_PluginID,
	effect_pp_ref: PF_ProgPtr,
	effect_refPH: *mut AEGP_EffectRefH,
) -> A_Err {
	DiagnosticBuilder::new()
		.set_name("AEGP_PFInterfaceSuite1/AEGP_GetNewEffectForEffect")
		.add_arg("aegp_plugin_id", aegp_plugin_id as usize)
		.add_arg("effect_pp_ref", effect_pp_ref as usize)
		.add_arg("effect_refPH", effect_refPH as usize)
		.emit();

	PF_Err_NONE as A_Err
}

unsafe extern "C" fn convert_effect_to_comp_time_sys(
	effect_pp_ref: PF_ProgPtr,
	what_timeL: A_long,
	time_scaleLu: A_u_long,
	comp_timePT: *mut A_Time,
) -> A_Err {
	DiagnosticBuilder::new()
		.set_name("AEGP_PFInterfaceSuite1/AEGP_ConvertEffectToCompTime")
		.add_arg("effect_pp_ref", effect_pp_ref as usize)
		.add_arg("what_timeL", what_timeL as usize)
		.add_arg("time_scaleLu", time_scaleLu as usize)
		.add_arg("comp_timePT", comp_timePT as usize)
		.emit();

	PF_Err_NONE as A_Err
}

unsafe extern "C" fn get_effect_camera_sys(
	effect_pp_ref: PF_ProgPtr,
	comp_timePT: *const A_Time,
	camera_layerPH: *mut AEGP_LayerH,
) -> A_Err {
	DiagnosticBuilder::new()
		.set_name("AEGP_PFInterfaceSuite1/AEGP_GetEffectCamera")
		.add_arg("effect_pp_ref", effect_pp_ref as usize)
		.add_arg("comp_timePT", comp_timePT as usize)
		.add_arg("camera_layerPH", camera_layerPH as usize)
		.emit();

	PF_Err_NONE as A_Err
}

unsafe extern "C" fn get_effect_camera_matrix(
	effect_pp_ref: PF_ProgPtr,
	comp_timePT: *const A_Time,
	camera_matrixP: *mut A_Matrix4,
	dist_to_image_planePF: *mut A_FpLong,
	image_plane_widthPL: *mut A_short,
	image_plane_heightPL: *mut A_short,
) -> A_Err {
	DiagnosticBuilder::new()
		.set_name("AEGP_PFInterfaceSuite1/AEGP_GetEffectCameraMatrix")
		.add_arg("effect_pp_ref", effect_pp_ref as usize)
		.add_arg("comp_timePT", comp_timePT as usize)
		.add_arg("camera_matrixP", camera_matrixP as usize)
		.add_arg("dist_to_image_planePF", dist_to_image_planePF as usize)
		.add_arg("image_plane_widthPL", image_plane_widthPL as usize)
		.add_arg("image_plane_heightPL", image_plane_heightPL as usize)
		.emit();

	PF_Err_NONE as A_Err
}

pub(super) fn create_aegp_pf_interface_suite() -> Box<AEGP_PFInterfaceSuite1> {
	Box::new(AEGP_PFInterfaceSuite1 {
		AEGP_GetEffectLayer: Some(get_effect_layer_sys),
		AEGP_GetNewEffectForEffect: Some(get_new_effect_for_effect_sys),
		AEGP_ConvertEffectToCompTime: Some(convert_effect_to_comp_time_sys),
		AEGP_GetEffectCamera: Some(get_effect_camera_sys),
		AEGP_GetEffectCameraMatrix: Some(get_effect_camera_matrix),
	})
}
