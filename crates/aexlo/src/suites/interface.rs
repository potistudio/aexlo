//! `AEGP_PFInterfaceSuite1`: bridges from an effect to the AEGP object model.
//!
//! aexlo has no AEGP object model (no layers, effects or cameras to hand out),
//! so the handle getters report null handles -- the SDK's own answer for "no
//! camera" -- rather than leaving the plugin's out-params uninitialised.
//! Everything that can be computed is: effect time maps 1:1 to comp time (the
//! layer starts at comp time 0), and the camera geometry is After Effects'
//! default comp camera (50 mm preset) for the effect's layer.

use after_effects_sys::{
	A_Err, A_FpLong, A_Matrix4, A_Time, A_long, A_short, A_u_long, AEGP_EffectRefH, AEGP_LayerH,
	AEGP_PFInterfaceSuite1, AEGP_PluginID, PF_Err_BAD_CALLBACK_PARAM, PF_Err_NONE, PF_ProgPtr,
};

use crate::PluginInstance;
use crate::core::diagnostics::diag;

unsafe extern "C" fn get_effect_layer_sys(_effect_pp_ref: PF_ProgPtr, layerPH: *mut AEGP_LayerH) -> A_Err {
	diag!("AEGP_PFInterfaceSuite1/AEGP_GetEffectLayer",
		"effect_pp_ref" => format!("{:#x}", _effect_pp_ref as usize),
		"layerPH" => format!("{:#x}", layerPH as usize),
	);

	let Some(out) = (unsafe { layerPH.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	*out = std::ptr::null_mut();
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn get_new_effect_for_effect_sys(
	_aegp_plugin_id: AEGP_PluginID,
	_effect_pp_ref: PF_ProgPtr,
	effect_refPH: *mut AEGP_EffectRefH,
) -> A_Err {
	diag!("AEGP_PFInterfaceSuite1/AEGP_GetNewEffectForEffect",
		"aegp_plugin_id" => _aegp_plugin_id as usize,
		"effect_pp_ref" => format!("{:#x}", _effect_pp_ref as usize),
		"effect_refPH" => format!("{:#x}", effect_refPH as usize),
	);

	let Some(out) = (unsafe { effect_refPH.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	*out = std::ptr::null_mut();
	PF_Err_NONE as A_Err
}

/// The effect's layer starts at comp time 0 with no stretch, so layer time is
/// comp time.
unsafe extern "C" fn convert_effect_to_comp_time_sys(
	_effect_pp_ref: PF_ProgPtr,
	what_timeL: A_long,
	time_scaleLu: A_u_long,
	comp_timePT: *mut A_Time,
) -> A_Err {
	diag!("AEGP_PFInterfaceSuite1/AEGP_ConvertEffectToCompTime",
		"effect_pp_ref" => format!("{:#x}", _effect_pp_ref as usize),
		"what_timeL" => what_timeL,
		"time_scaleLu" => time_scaleLu,
		"comp_timePT" => format!("{:#x}", comp_timePT as usize),
	);

	let Some(out) = (unsafe { comp_timePT.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	*out = A_Time {
		value: what_timeL,
		scale: time_scaleLu,
	};
	PF_Err_NONE as A_Err
}

/// No camera layer exists; the SDK documents a null handle for that case.
unsafe extern "C" fn get_effect_camera_sys(
	_effect_pp_ref: PF_ProgPtr,
	_comp_timePT: *const A_Time,
	camera_layerPH: *mut AEGP_LayerH,
) -> A_Err {
	diag!("AEGP_PFInterfaceSuite1/AEGP_GetEffectCamera",
		"effect_pp_ref" => format!("{:#x}", _effect_pp_ref as usize),
		"comp_timePT" => format!("{:#x}", _comp_timePT as usize),
		"camera_layerPH" => format!("{:#x}", camera_layerPH as usize),
	);

	let Some(out) = (unsafe { camera_layerPH.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	*out = std::ptr::null_mut();
	PF_Err_NONE as A_Err
}

/// After Effects' default comp camera for a `width` x `height` comp: the 50 mm
/// preset (36 mm film width), centred on the comp and looking down +z, with
/// its image plane at the comp.
///
/// Returns the camera-to-world matrix (row vectors, translation in the last
/// row) and the distance to the image plane (the camera's zoom, in pixels).
pub(crate) fn default_camera(width: f64, height: f64) -> (A_Matrix4, A_FpLong) {
	let zoom = width * 50.0 / 36.0;
	let mut mat = [[0.0; 4]; 4];
	for (i, row) in mat.iter_mut().enumerate() {
		row[i] = 1.0;
	}
	mat[3][0] = width / 2.0;
	mat[3][1] = height / 2.0;
	mat[3][2] = -zoom;
	(A_Matrix4 { mat }, zoom)
}

unsafe extern "C" fn get_effect_camera_matrix(
	effect_pp_ref: PF_ProgPtr,
	_comp_timePT: *const A_Time,
	camera_matrixP: *mut A_Matrix4,
	dist_to_image_planePF: *mut A_FpLong,
	image_plane_widthPL: *mut A_short,
	image_plane_heightPL: *mut A_short,
) -> A_Err {
	diag!("AEGP_PFInterfaceSuite1/AEGP_GetEffectCameraMatrix",
		"effect_pp_ref" => format!("{:#x}", effect_pp_ref as usize),
		"comp_timePT" => format!("{:#x}", _comp_timePT as usize),
		"camera_matrixP" => format!("{:#x}", camera_matrixP as usize),
		"dist_to_image_planePF" => format!("{:#x}", dist_to_image_planePF as usize),
		"image_plane_widthPL" => format!("{:#x}", image_plane_widthPL as usize),
		"image_plane_heightPL" => format!("{:#x}", image_plane_heightPL as usize),
	);

	let Some(instance) = PluginInstance::get_instance_ptr(effect_pp_ref) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	// The comp is the effect's layer.
	let (width, height) = unsafe { instance.as_ref() }.input_size();
	let (matrix, zoom) = default_camera(width as f64, height as f64);
	unsafe {
		if let Some(out) = camera_matrixP.as_mut() {
			*out = matrix;
		}
		if let Some(out) = dist_to_image_planePF.as_mut() {
			*out = zoom;
		}
		if let Some(out) = image_plane_widthPL.as_mut() {
			*out = width.min(A_short::MAX as u32) as A_short;
		}
		if let Some(out) = image_plane_heightPL.as_mut() {
			*out = height.min(A_short::MAX as u32) as A_short;
		}
	}
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

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn handles_are_null_and_time_maps_through() {
		let suite = create_aegp_pf_interface_suite();
		let mut layer: AEGP_LayerH = 1 as AEGP_LayerH;
		let mut effect: AEGP_EffectRefH = 1 as AEGP_EffectRefH;
		let mut time = A_Time { value: -1, scale: 0 };
		unsafe {
			suite.AEGP_GetEffectLayer.unwrap()(std::ptr::null_mut(), &mut layer);
			suite.AEGP_GetNewEffectForEffect.unwrap()(0, std::ptr::null_mut(), &mut effect);
			suite.AEGP_ConvertEffectToCompTime.unwrap()(std::ptr::null_mut(), 12, 30, &mut time);
		}
		assert!(layer.is_null() && effect.is_null());
		assert_eq!((time.value, time.scale), (12, 30));
	}

	#[test]
	fn default_camera_centres_on_the_comp() {
		let (m, zoom) = default_camera(1920.0, 1080.0);
		assert!((zoom - 1920.0 * 50.0 / 36.0).abs() < 1e-9);
		assert_eq!(m.mat[3][..3], [960.0, 540.0, -zoom]);
		assert_eq!(m.mat[0][0], 1.0);
	}
}
