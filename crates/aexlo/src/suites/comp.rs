//! `AEGP Comp Suite` (wire version 21, `AEGP_CompSuite10`) over aexlo's
//! one-layer timeline (see [`layer`](super::layer)).
//!
//! The comp is the effect's layer: same handle, the input layer's size, the
//! render context's frame rate, [`COMP_DURATION_SECONDS`](super::layer::COMP_DURATION_SECONDS) long, with After
//! Effects' defaults for everything a new comp has (black background, full
//! resolution, motion blur off with a 180° shutter at -90°, 16 suggested
//! samples, adaptive limit 128, the work area spanning the comp). It is not a
//! project item, and creating or changing anything is refused with
//! `A_Err_GENERIC`.

use after_effects_sys::*;

use super::layer::{Timeline, answer};
use crate::core::diagnostics::diag;

/// After Effects' default shutter angle and phase, in degrees.
const SHUTTER_ANGLE: A_long = 180;
const SHUTTER_PHASE: A_long = -90;

fn unsupported(name: &str) -> A_Err {
	log::warn!("AEGP_CompSuite10/{name}: aexlo's comp is fixed and is not a project item");
	A_Err_GENERIC as A_Err
}

/// Stubs for the calls that create, edit or select things.
macro_rules! refuse {
	($($fn_name:ident => $name:literal ( $($arg:ident : $ty:ty),* $(,)? );)*) => {
		$(
			unsafe extern "C" fn $fn_name($($arg: $ty),*) -> A_Err {
				$(let _ = $arg;)*
				diag!(concat!("AEGP_CompSuite10/", $name));
				unsupported($name)
			}
		)*
	};
}

refuse! {
	get_comp_from_item => "AEGP_GetCompFromItem"(itemH: AEGP_ItemH, compPH: *mut AEGP_CompH);
	get_item_from_comp => "AEGP_GetItemFromComp"(compH: AEGP_CompH, itemPH: *mut AEGP_ItemH);
	set_comp_downsample_factor => "AEGP_SetCompDownsampleFactor"(compH: AEGP_CompH, dsfP: *const AEGP_DownsampleFactor);
	set_comp_bg_color => "AEGP_SetCompBGColor"(compH: AEGP_CompH, bg_colorP: *const AEGP_ColorVal);
	set_show_layer_name_or_source_name => "AEGP_SetShowLayerNameOrSourceName"(compH: AEGP_CompH, show_layer_namesB: A_Boolean);
	set_show_blend_modes => "AEGP_SetShowBlendModes"(compH: AEGP_CompH, show_blend_modesB: A_Boolean);
	set_comp_frame_rate => "AEGP_SetCompFrameRate"(compH: AEGP_CompH, fpsPF: *const A_FpLong);
	set_comp_suggested_motion_blur_samples => "AEGP_SetCompSuggestedMotionBlurSamples"(compH: AEGP_CompH, samplesL: A_long);
	set_comp_motion_blur_adaptive_sample_limit => "AEGP_SetCompMotionBlurAdaptiveSampleLimit"(compH: AEGP_CompH, samplesL: A_long);
	set_comp_work_area_start_and_duration => "AEGP_SetCompWorkAreaStartAndDuration"(
		compH: AEGP_CompH,
		work_area_startPT: *const A_Time,
		work_area_durationPT: *const A_Time,
	);
	create_solid_in_comp => "AEGP_CreateSolidInComp"(
		utf_nameZ: *const A_UTF16Char,
		width: A_long,
		height: A_long,
		color: *const AEGP_ColorVal,
		parent_compH: AEGP_CompH,
		durationPT0: *const A_Time,
		new_solidPH: *mut AEGP_LayerH,
	);
	create_camera_in_comp => "AEGP_CreateCameraInComp"(
		utf_nameZ: *const A_UTF16Char,
		center_point: A_FloatPoint,
		parent_compH: AEGP_CompH,
		new_cameraPH: *mut AEGP_LayerH,
	);
	create_light_in_comp => "AEGP_CreateLightInComp"(
		utf_nameZ: *const A_UTF16Char,
		center_point: A_FloatPoint,
		parent_compH: AEGP_CompH,
		new_lightPH: *mut AEGP_LayerH,
	);
	create_comp => "AEGP_CreateComp"(
		parent_folderH0: AEGP_ItemH,
		utf_nameZ: *const A_UTF16Char,
		widthL: A_long,
		heightL: A_long,
		pixel_aspect_ratioPRt: *const A_Ratio,
		durationPT: *const A_Time,
		frameratePRt: *const A_Ratio,
		new_compPH: *mut AEGP_CompH,
	);
	get_new_collection_from_comp_selection => "AEGP_GetNewCollectionFromCompSelection"(
		plugin_id: AEGP_PluginID,
		compH: AEGP_CompH,
		collectionPH: *mut AEGP_Collection2H,
	);
	set_selection => "AEGP_SetSelection"(compH: AEGP_CompH, collectionH: AEGP_Collection2H);
	set_comp_display_start_time => "AEGP_SetCompDisplayStartTime"(compH: AEGP_CompH, start_timePT: *const A_Time);
	set_comp_duration => "AEGP_SetCompDuration"(compH: AEGP_CompH, durationPT: *const A_Time);
	create_null_in_comp => "AEGP_CreateNullInComp"(
		utf_nameZ: *const A_UTF16Char,
		parent_compH: AEGP_CompH,
		durationPT0: *const A_Time,
		new_null_solidPH: *mut AEGP_LayerH,
	);
	set_comp_pixel_aspect_ratio => "AEGP_SetCompPixelAspectRatio"(compH: AEGP_CompH, pix_aspectratioPRt: *const A_Ratio);
	create_text_layer_in_comp => "AEGP_CreateTextLayerInComp"(
		parent_compH: AEGP_CompH,
		select_new_layerB: A_Boolean,
		new_text_layerPH: *mut AEGP_LayerH,
	);
	create_box_text_layer_in_comp => "AEGP_CreateBoxTextLayerInComp"(
		parent_compH: AEGP_CompH,
		select_new_layerB: A_Boolean,
		box_dimensions: A_FloatPoint,
		new_text_layerPH: *mut AEGP_LayerH,
	);
	set_comp_dimensions => "AEGP_SetCompDimensions"(compH: AEGP_CompH, widthL: A_long, heightL: A_long);
	duplicate_comp => "AEGP_DuplicateComp"(compH: AEGP_CompH, new_compPH: *mut AEGP_CompH);
	get_most_recently_used_comp => "AEGP_GetMostRecentlyUsedComp"(compPH: *mut AEGP_CompH);
	create_vector_layer_in_comp => "AEGP_CreateVectorLayerInComp"(parent_compH: AEGP_CompH, new_vector_layerPH: *mut AEGP_LayerH);
	get_new_comp_marker_stream => "AEGP_GetNewCompMarkerStream"(
		aegp_plugin_id: AEGP_PluginID,
		parent_compH: AEGP_CompH,
		streamPH: *mut AEGP_StreamRefH,
	);
	set_comp_display_drop_frame => "AEGP_SetCompDisplayDropFrame"(compH: AEGP_CompH, dropFrameB: A_Boolean);
}

unsafe extern "C" fn get_comp_downsample_factor(compH: AEGP_CompH, dsfP: *mut AEGP_DownsampleFactor) -> A_Err {
	diag!("AEGP_CompSuite10/AEGP_GetCompDownsampleFactor", "compH" => format!("{:#x}", compH as usize));
	answer(compH as _, dsfP, |_| AEGP_DownsampleFactor { xS: 1, yS: 1 })
}

unsafe extern "C" fn get_comp_bg_color(compH: AEGP_CompH, bg_colorP: *mut AEGP_ColorVal) -> A_Err {
	diag!("AEGP_CompSuite10/AEGP_GetCompBGColor", "compH" => format!("{:#x}", compH as usize));
	answer(compH as _, bg_colorP, |_| AEGP_ColorVal {
		alphaF: 1.0,
		redF: 0.0,
		greenF: 0.0,
		blueF: 0.0,
	})
}

/// No switches are on: motion blur, frame blending and the rest are off.
unsafe extern "C" fn get_comp_flags(compH: AEGP_CompH, comp_flagsP: *mut AEGP_CompFlags) -> A_Err {
	diag!("AEGP_CompSuite10/AEGP_GetCompFlags", "compH" => format!("{:#x}", compH as usize));
	answer(compH as _, comp_flagsP, |_| 0)
}

unsafe extern "C" fn get_show_layer_name_or_source_name(
	compH: AEGP_CompH,
	layer_names_shownPB: *mut A_Boolean,
) -> A_Err {
	diag!("AEGP_CompSuite10/AEGP_GetShowLayerNameOrSourceName", "compH" => format!("{:#x}", compH as usize));
	answer(compH as _, layer_names_shownPB, |_| 1)
}

unsafe extern "C" fn get_show_blend_modes(compH: AEGP_CompH, blend_modes_shownPB: *mut A_Boolean) -> A_Err {
	diag!("AEGP_CompSuite10/AEGP_GetShowBlendModes", "compH" => format!("{:#x}", compH as usize));
	answer(compH as _, blend_modes_shownPB, |_| 0)
}

unsafe extern "C" fn get_comp_framerate(compH: AEGP_CompH, fpsPF: *mut A_FpLong) -> A_Err {
	diag!("AEGP_CompSuite10/AEGP_GetCompFramerate", "compH" => format!("{:#x}", compH as usize));
	answer(compH as _, fpsPF, |t| {
		t.time_scale as A_FpLong / t.time_step.max(1) as A_FpLong
	})
}

unsafe extern "C" fn get_comp_shutter_angle_phase(
	compH: AEGP_CompH,
	angle: *mut A_Ratio,
	phase: *mut A_Ratio,
) -> A_Err {
	diag!("AEGP_CompSuite10/AEGP_GetCompShutterAnglePhase", "compH" => format!("{:#x}", compH as usize));
	if Timeline::of(compH as _).is_none() || angle.is_null() || phase.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	}
	unsafe {
		*angle = A_Ratio {
			num: SHUTTER_ANGLE,
			den: 1,
		};
		*phase = A_Ratio {
			num: SHUTTER_PHASE,
			den: 1,
		};
	}
	PF_Err_NONE as A_Err
}

/// The span the shutter is open for the frame at `comp_timeP`: it opens
/// `phase / 360` of a frame from the frame's time and stays open for
/// `angle / 360` of a frame. Expressed at 360 ticks per frame.
unsafe extern "C" fn get_comp_shutter_frame_range(
	compH: AEGP_CompH,
	comp_timeP: *const A_Time,
	start: *mut A_Time,
	duration: *mut A_Time,
) -> A_Err {
	diag!("AEGP_CompSuite10/AEGP_GetCompShutterFrameRange", "compH" => format!("{:#x}", compH as usize));
	let Some(t) = Timeline::of(compH as _) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	let Some(time) = (unsafe { comp_timeP.as_ref() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	if start.is_null() || duration.is_null() || time.scale == 0 {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	}
	// One frame is `time_step / time_scale` s; at a scale of `time_scale * 360`
	// it is `time_step * 360` ticks, so a degree of shutter is `time_step` ticks.
	let scale = t.time_scale as i64 * 360;
	let at = time.value as i64 * scale / time.scale as i64;
	let step = t.time_step as i64;
	unsafe {
		*start = A_Time {
			value: (at + SHUTTER_PHASE as i64 * step) as A_long,
			scale: scale as A_u_long,
		};
		*duration = A_Time {
			value: (SHUTTER_ANGLE as i64 * step) as A_long,
			scale: scale as A_u_long,
		};
	}
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn get_comp_suggested_motion_blur_samples(compH: AEGP_CompH, samplesPL: *mut A_long) -> A_Err {
	diag!("AEGP_CompSuite10/AEGP_GetCompSuggestedMotionBlurSamples", "compH" => format!("{:#x}", compH as usize));
	answer(compH as _, samplesPL, |_| 16)
}

unsafe extern "C" fn get_comp_motion_blur_adaptive_sample_limit(compH: AEGP_CompH, samplesPL: *mut A_long) -> A_Err {
	diag!("AEGP_CompSuite10/AEGP_GetCompMotionBlurAdaptiveSampleLimit", "compH" => format!("{:#x}", compH as usize));
	answer(compH as _, samplesPL, |_| 128)
}

unsafe extern "C" fn get_comp_work_area_start(compH: AEGP_CompH, work_area_startPT: *mut A_Time) -> A_Err {
	diag!("AEGP_CompSuite10/AEGP_GetCompWorkAreaStart", "compH" => format!("{:#x}", compH as usize));
	answer(compH as _, work_area_startPT, |t| A_Time {
		value: 0,
		scale: t.time_scale,
	})
}

unsafe extern "C" fn get_comp_work_area_duration(compH: AEGP_CompH, work_area_durationPT: *mut A_Time) -> A_Err {
	diag!("AEGP_CompSuite10/AEGP_GetCompWorkAreaDuration", "compH" => format!("{:#x}", compH as usize));
	answer(compH as _, work_area_durationPT, Timeline::duration)
}

unsafe extern "C" fn get_comp_display_start_time(compH: AEGP_CompH, start_timePT: *mut A_Time) -> A_Err {
	diag!("AEGP_CompSuite10/AEGP_GetCompDisplayStartTime", "compH" => format!("{:#x}", compH as usize));
	answer(compH as _, start_timePT, |t| A_Time {
		value: 0,
		scale: t.time_scale,
	})
}

unsafe extern "C" fn get_comp_frame_duration(compH: AEGP_CompH, timeP: *mut A_Time) -> A_Err {
	diag!("AEGP_CompSuite10/AEGP_GetCompFrameDuration", "compH" => format!("{:#x}", compH as usize));
	answer(compH as _, timeP, |t| A_Time {
		value: t.time_step,
		scale: t.time_scale,
	})
}

unsafe extern "C" fn get_comp_display_drop_frame(compH: AEGP_CompH, dropFramePB: *mut A_Boolean) -> A_Err {
	diag!("AEGP_CompSuite10/AEGP_GetCompDisplayDropFrame", "compH" => format!("{:#x}", compH as usize));
	answer(compH as _, dropFramePB, |_| 0)
}

/// Builds the `AEGP_CompSuite10` vtable.
pub(super) const fn create_comp_suite_10() -> AEGP_CompSuite10 {
	AEGP_CompSuite10 {
		AEGP_GetCompFromItem: Some(get_comp_from_item),
		AEGP_GetItemFromComp: Some(get_item_from_comp),
		AEGP_GetCompDownsampleFactor: Some(get_comp_downsample_factor),
		AEGP_SetCompDownsampleFactor: Some(set_comp_downsample_factor),
		AEGP_GetCompBGColor: Some(get_comp_bg_color),
		AEGP_SetCompBGColor: Some(set_comp_bg_color),
		AEGP_GetCompFlags: Some(get_comp_flags),
		AEGP_GetShowLayerNameOrSourceName: Some(get_show_layer_name_or_source_name),
		AEGP_SetShowLayerNameOrSourceName: Some(set_show_layer_name_or_source_name),
		AEGP_GetShowBlendModes: Some(get_show_blend_modes),
		AEGP_SetShowBlendModes: Some(set_show_blend_modes),
		AEGP_GetCompFramerate: Some(get_comp_framerate),
		AEGP_SetCompFrameRate: Some(set_comp_frame_rate),
		AEGP_GetCompShutterAnglePhase: Some(get_comp_shutter_angle_phase),
		AEGP_GetCompShutterFrameRange: Some(get_comp_shutter_frame_range),
		AEGP_GetCompSuggestedMotionBlurSamples: Some(get_comp_suggested_motion_blur_samples),
		AEGP_SetCompSuggestedMotionBlurSamples: Some(set_comp_suggested_motion_blur_samples),
		AEGP_GetCompMotionBlurAdaptiveSampleLimit: Some(get_comp_motion_blur_adaptive_sample_limit),
		AEGP_SetCompMotionBlurAdaptiveSampleLimit: Some(set_comp_motion_blur_adaptive_sample_limit),
		AEGP_GetCompWorkAreaStart: Some(get_comp_work_area_start),
		AEGP_GetCompWorkAreaDuration: Some(get_comp_work_area_duration),
		AEGP_SetCompWorkAreaStartAndDuration: Some(set_comp_work_area_start_and_duration),
		AEGP_CreateSolidInComp: Some(create_solid_in_comp),
		AEGP_CreateCameraInComp: Some(create_camera_in_comp),
		AEGP_CreateLightInComp: Some(create_light_in_comp),
		AEGP_CreateComp: Some(create_comp),
		AEGP_GetNewCollectionFromCompSelection: Some(get_new_collection_from_comp_selection),
		AEGP_SetSelection: Some(set_selection),
		AEGP_GetCompDisplayStartTime: Some(get_comp_display_start_time),
		AEGP_SetCompDisplayStartTime: Some(set_comp_display_start_time),
		AEGP_SetCompDuration: Some(set_comp_duration),
		AEGP_CreateNullInComp: Some(create_null_in_comp),
		AEGP_SetCompPixelAspectRatio: Some(set_comp_pixel_aspect_ratio),
		AEGP_CreateTextLayerInComp: Some(create_text_layer_in_comp),
		AEGP_CreateBoxTextLayerInComp: Some(create_box_text_layer_in_comp),
		AEGP_SetCompDimensions: Some(set_comp_dimensions),
		AEGP_DuplicateComp: Some(duplicate_comp),
		AEGP_GetCompFrameDuration: Some(get_comp_frame_duration),
		AEGP_GetMostRecentlyUsedComp: Some(get_most_recently_used_comp),
		AEGP_CreateVectorLayerInComp: Some(create_vector_layer_in_comp),
		AEGP_GetNewCompMarkerStream: Some(get_new_comp_marker_stream),
		AEGP_GetCompDisplayDropFrame: Some(get_comp_display_drop_frame),
		AEGP_SetCompDisplayDropFrame: Some(set_comp_display_drop_frame),
	}
}

#[cfg(test)]
mod tests {
	use super::super::layer::COMP_DURATION_SECONDS;
	use super::*;

	#[test]
	fn null_comps_are_rejected() {
		let suite = create_comp_suite_10();
		let mut fps: A_FpLong = -1.0;
		let err = unsafe { suite.AEGP_GetCompFramerate.unwrap()(std::ptr::null_mut(), &mut fps) };
		assert_eq!(err, PF_Err_BAD_CALLBACK_PARAM as A_Err);
		assert_eq!(fps, -1.0);
	}

	#[test]
	fn duration_is_whole_seconds() {
		let t = Timeline {
			width: 1,
			height: 1,
			current_time: 0,
			time_step: 1,
			time_scale: 30,
		};
		let d = t.duration();
		assert_eq!(d.value as i64 / d.scale as i64, COMP_DURATION_SECONDS as i64);
	}
}
