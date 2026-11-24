//! `AEGP Camera Suite` v2, for comps without a camera layer.
//!
//! A project has no camera layers (see [`layer`](super::layer)): the camera is
//! null, the answer for a comp seen through its default camera, whose distance
//! is the one "AEGP PF Interface Suite" uses. A layer asked for its camera's
//! properties is not one, so those are refused.

use crate::core::diagnostics::diag;
use crate::sys::*;

use super::layer::Timeline;

/// No camera, whatever the render and the time.
unsafe extern "C" fn get_camera(
	_render_contextH: PR_RenderContextH,
	_comp_timeP: *const A_Time,
	camera_layerPH: *mut AEGP_LayerH,
) -> A_Err {
	crate::core::ffi::guard("get_camera", || {
		diag!("AEGP_CameraSuite2/AEGP_GetCamera");
		// SAFETY: null, or the plugin's out-pointer.
		let Some(out) = (unsafe { camera_layerPH.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		*out = std::ptr::null_mut();
		PF_Err_NONE
	})
}

unsafe extern "C" fn get_default_distance(compH: AEGP_CompH, dist_to_planePF: *mut A_FpLong) -> A_Err {
	crate::core::ffi::guard("get_default_camera_distance", || {
		diag!("AEGP_CameraSuite2/AEGP_GetDefaultCameraDistanceToImagePlane", "compH" => format!("{:#x}", compH as usize));
		// SAFETY: null, or the plugin's out-pointer.
		let (Some(timeline), Some(out)) = (Timeline::of_effect(compH.cast()), unsafe { dist_to_planePF.as_mut() })
		else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		let (_, zoom) = super::interface::default_camera(timeline.comp_width as f64, timeline.comp_height as f64);
		*out = zoom;
		PF_Err_NONE
	})
}

/// A layer that is not a camera, or no layer at all.
fn not_a_camera(layer: AEGP_LayerH) -> A_Err {
	match Timeline::of(layer.cast()) {
		Some(_) => A_Err_GENERIC,
		None => PF_Err_BAD_CALLBACK_PARAM,
	}
}

unsafe extern "C" fn get_camera_type(camera_layerH: AEGP_LayerH, _camera_typeP: *mut AEGP_CameraType) -> A_Err {
	crate::core::ffi::guard("get_camera_type", || {
		diag!("AEGP_CameraSuite2/AEGP_GetCameraType", "camera_layerH" => format!("{:#x}", camera_layerH as usize));
		not_a_camera(camera_layerH)
	})
}

unsafe extern "C" fn get_film_size(
	camera_layerH: AEGP_LayerH,
	_film_size_unitsP: *mut AEGP_FilmSizeUnits,
	_film_sizePF0: *mut A_FpLong,
) -> A_Err {
	crate::core::ffi::guard("get_camera_film_size", || {
		diag!("AEGP_CameraSuite2/AEGP_GetCameraFilmSize", "camera_layerH" => format!("{:#x}", camera_layerH as usize));
		not_a_camera(camera_layerH)
	})
}

unsafe extern "C" fn set_film_size(
	_camera_layerH: AEGP_LayerH,
	_film_size_units: AEGP_FilmSizeUnits,
	_film_sizePF0: *mut A_FpLong,
) -> A_Err {
	crate::core::ffi::guard("set_camera_film_size", || {
		diag!("AEGP_CameraSuite2/AEGP_SetCameraFilmSize");
		crate::observe::unsupported(
			crate::observe::UnsupportedKind::Declined,
			"AEGP_CameraSuite/AEGP_SetCameraFilmSize",
			"the project is read-only",
		);
		A_Err_GENERIC
	})
}

/// Wire version 2.
pub(super) const fn suite_2() -> AEGP_CameraSuite2 {
	AEGP_CameraSuite2 {
		AEGP_GetCamera: Some(get_camera),
		AEGP_GetCameraType: Some(get_camera_type),
		AEGP_GetDefaultCameraDistanceToImagePlane: Some(get_default_distance),
		AEGP_GetCameraFilmSize: Some(get_film_size),
		AEGP_SetCameraFilmSize: Some(set_film_size),
	}
}
