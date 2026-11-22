//! Dummy implementation for PF Utility Suite (Premiere/AE support callbacks)

use crate::suites::macros::stub_log;
use after_effects_sys::*;
use std::os::raw::c_void;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicI32, Ordering};

stub_log!(get_filter_instance_id_stub,
	_effect_ref: PF_ProgPtr,
	out_filter_instance_id: *mut A_long
);

stub_log!(get_media_timecode_stub,
	_effect_ref: PF_ProgPtr,
	out_current_frame: *mut A_long,
	out_time_display: *mut PF_TimeDisplay
);

stub_log!(get_clip_speed_stub,
	_effect_ref: PF_ProgPtr,
	speed: *mut f64
);

stub_log!(get_clip_duration_stub,
	_effect_ref: PF_ProgPtr,
	frame_duration: *mut A_long
);

stub_log!(get_clip_start_stub,
	_effect_ref: PF_ProgPtr,
	frame_duration: *mut A_long
);

stub_log!(get_unscaled_clip_duration_stub,
	_effect_ref: PF_ProgPtr,
	frame_duration: *mut A_long
);

stub_log!(get_unscaled_clip_start_stub,
	_effect_ref: PF_ProgPtr,
	frame_duration: *mut A_long
);

stub_log!(get_track_item_start_stub,
	_effect_ref: PF_ProgPtr,
	frame_duration: *mut A_long
);

stub_log!(get_media_field_type_stub,
	_effect_ref: PF_ProgPtr,
	out_field_type: *mut prFieldType
);

stub_log!(get_media_frame_rate_stub,
	_effect_ref: PF_ProgPtr,
	out_ticks_per_frame: *mut PrTime
);

stub_log!(get_containing_timeline_id_stub,
	_effect_ref: PF_ProgPtr,
	out_timeline_id: *mut PrTimelineID
);

stub_log!(get_clip_name_stub,
	_effect_ref: PF_ProgPtr,
	_in_get_master_clip_name: A_Boolean,
	out_sdk_string: *mut PrSDKString
);

stub_log!(effect_wants_checked_out_frames_to_match_render_pixel_format_stub,
	_effect_ref: PF_ProgPtr
);

stub_log!(effect_depends_on_clip_name_stub,
	_effect_ref: PF_ProgPtr,
	_in_depends_on_clip_name: A_Boolean
);

stub_log!(set_effect_instance_name_stub,
	_effect_ref: PF_ProgPtr,
	_in_sdk_string: *const PrSDKString
);

stub_log!(get_file_name_stub,
	_effect_ref: PF_ProgPtr,
	out_sdk_string: *mut PrSDKString
);

stub_log!(get_original_clip_frame_rate_stub,
	_effect_ref: PF_ProgPtr,
	out_ticks_per_frame: *mut PrTime
);

stub_log!(get_source_track_media_timecode_stub,
	_effect_ref: PF_ProgPtr,
	_in_layer_param_index: csSDK_uint32,
	_in_apply_transform: bool,
	_in_add_start_time_offset: bool,
	out_current_frame: *mut A_long
);

stub_log!(get_source_track_clip_name_stub,
	_effect_ref: PF_ProgPtr,
	_in_layer_param_index: csSDK_uint32,
	_in_get_master_clip_name: A_Boolean,
	out_sdk_string: *mut PrSDKString
);

stub_log!(get_source_track_file_name_stub,
	_effect_ref: PF_ProgPtr,
	_in_layer_param_index: csSDK_uint32,
	out_sdk_string: *mut PrSDKString
);

stub_log!(effect_depends_on_clip_name2_stub,
	_effect_ref: PF_ProgPtr,
	_in_depends_on_clip_name: A_Boolean,
	_in_layer_param_index: csSDK_uint32
);

stub_log!(get_media_timecode2_stub,
	_effect_ref: PF_ProgPtr,
	_in_apply_trim: bool,
	out_current_frame: *mut A_long,
	out_time_display: *mut PF_TimeDisplay
);

stub_log!(get_source_track_media_timecode2_stub,
	_effect_ref: PF_ProgPtr,
	_in_layer_param_index: csSDK_uint32,
	_in_apply_transform: bool,
	_in_add_start_time_offset: bool,
	_in_sequence_time: PrTime,
	out_current_frame: *mut A_long
);

stub_log!(get_source_track_clip_name2_stub,
	_effect_ref: PF_ProgPtr,
	_in_layer_param_index: csSDK_uint32,
	_in_get_master_clip_name: A_Boolean,
	out_sdk_string: *mut PrSDKString,
	_in_sequence_time: PrTime
);

stub_log!(get_source_track_file_name2_stub,
	_effect_ref: PF_ProgPtr,
	_in_layer_param_index: csSDK_uint32,
	out_sdk_string: *mut PrSDKString,
	_in_sequence_time: PrTime
);

stub_log!(get_comment_string_stub,
	_in_effect_ref: PF_ProgPtr,
	_in_source_track: i32,
	_in_sequence_time: PrTime,
	out_sdk_string: *mut PrSDKString
);

stub_log!(get_log_note_string_stub,
	_in_effect_ref: PF_ProgPtr,
	_in_source_track: i32,
	_in_sequence_time: PrTime,
	out_sdk_string: *mut PrSDKString
);

stub_log!(get_camera_roll_string_stub,
	_in_effect_ref: PF_ProgPtr,
	_in_source_track: i32,
	_in_sequence_time: PrTime,
	out_sdk_string: *mut PrSDKString
);

stub_log!(get_client_metadata_string_stub,
	_in_effect_ref: PF_ProgPtr,
	_in_source_track: i32,
	_in_sequence_time: PrTime,
	out_sdk_string: *mut PrSDKString
);

stub_log!(get_daily_roll_string_stub,
	_in_effect_ref: PF_ProgPtr,
	_in_source_track: i32,
	_in_sequence_time: PrTime,
	out_sdk_string: *mut PrSDKString
);

stub_log!(get_description_string_stub,
	_in_effect_ref: PF_ProgPtr,
	_in_source_track: i32,
	_in_sequence_time: PrTime,
	out_sdk_string: *mut PrSDKString
);

stub_log!(get_lab_roll_string_stub,
	_in_effect_ref: PF_ProgPtr,
	_in_source_track: i32,
	_in_sequence_time: PrTime,
	out_sdk_string: *mut PrSDKString
);

stub_log!(get_scene_string_stub,
	_in_effect_ref: PF_ProgPtr,
	_in_source_track: i32,
	_in_sequence_time: PrTime,
	out_sdk_string: *mut PrSDKString
);

stub_log!(get_shot_string_stub,
	_in_effect_ref: PF_ProgPtr,
	_in_source_track: i32,
	_in_sequence_time: PrTime,
	out_sdk_string: *mut PrSDKString
);

stub_log!(get_tape_name_string_stub,
	_in_effect_ref: PF_ProgPtr,
	_in_source_track: i32,
	_in_sequence_time: PrTime,
	out_sdk_string: *mut PrSDKString
);

stub_log!(get_video_codec_string_stub,
	_in_effect_ref: PF_ProgPtr,
	_in_source_track: i32,
	_in_sequence_time: PrTime,
	out_sdk_string: *mut PrSDKString
);

stub_log!(get_good_metadata_string_stub,
	_in_effect_ref: PF_ProgPtr,
	_in_source_track: i32,
	_in_sequence_time: PrTime,
	out_sdk_string: *mut PrSDKString
);

stub_log!(get_sound_roll_string_stub,
	_in_effect_ref: PF_ProgPtr,
	_in_source_track: i32,
	_in_sequence_time: PrTime,
	out_sdk_string: *mut PrSDKString
);

stub_log!(get_sequence_time_stub,
	_in_effect_ref: PF_ProgPtr,
	out_sequence_time: *mut PrTime
);

stub_log!(get_sound_timecode_stub,
	_in_effect_ref: PF_ProgPtr,
	_in_source_track: i32,
	_in_sequence_time: PrTime,
	out_current_frame: *mut A_long
);

stub_log!(get_original_clip_frame_rate_for_source_track_stub,
	_in_effect_ref: PF_ProgPtr,
	_in_source_track: i32,
	out_ticks_per_frame: *mut PrTime
);

stub_log!(get_media_frame_rate_for_source_track_stub,
	_in_effect_ref: PF_ProgPtr,
	_in_source_track: i32,
	_in_sequence_time: PrTime,
	out_ticks_per_frame: *mut PrTime
);

stub_log!(get_source_track_media_actual_start_time_stub,
	_in_effect_ref: PF_ProgPtr,
	_in_layer_param_index: csSDK_uint32,
	_in_sequence_time: PrTime,
	out_clip_actual_start_time: *mut PrTime
);

stub_log!(is_source_track_media_trimmed_stub,
	_in_effect_ref: PF_ProgPtr,
	_in_layer_param_index: csSDK_uint32,
	_in_sequence_time: PrTime,
	out_trim_applied: *mut bool
);

stub_log!(is_media_trimmed_stub,
	_in_effect_ref: PF_ProgPtr,
	_in_sequence_time: PrTime,
	out_trim_applied: *mut bool
);

stub_log!(is_track_empty_stub,
	_in_effect_ref: PF_ProgPtr,
	_in_layer_param_index: csSDK_uint32,
	_in_sequence_time: PrTime,
	out_is_track_empty: *mut bool
);

stub_log!(is_track_item_effect_applied_to_synthetic_stub,
	_in_effect_ref: PF_ProgPtr,
	out_is_track_item_effect_applied_to_synthetic: *mut bool
);

// ============================================================================
// Factory Function
// ============================================================================

/// Builds the `PF_UtilitySuite` vtable of logging stubs.
///
/// `const` so it can initialize the shared [`SUITE_CONTAINER`](crate::suites::SUITE_CONTAINER)
/// static; the suite is a stateless table of function pointers.
pub const fn create_utility_suite() -> PF_UtilitySuite {
	PF_UtilitySuite {
		GetFilterInstanceID: Some(get_filter_instance_id_stub),
		GetMediaTimecode: Some(get_media_timecode_stub),
		GetClipSpeed: Some(get_clip_speed_stub),
		GetClipDuration: Some(get_clip_duration_stub),
		GetClipStart: Some(get_clip_start_stub),
		GetUnscaledClipDuration: Some(get_unscaled_clip_duration_stub),
		GetUnscaledClipStart: Some(get_unscaled_clip_start_stub),
		GetTrackItemStart: Some(get_track_item_start_stub),
		GetMediaFieldType: Some(get_media_field_type_stub),
		GetMediaFrameRate: Some(get_media_frame_rate_stub),
		GetContainingTimelineID: Some(get_containing_timeline_id_stub),
		GetClipName: Some(get_clip_name_stub),
		EffectWantsCheckedOutFramesToMatchRenderPixelFormat: Some(
			effect_wants_checked_out_frames_to_match_render_pixel_format_stub,
		),
		EffectDependsOnClipName: Some(effect_depends_on_clip_name_stub),
		SetEffectInstanceName: Some(set_effect_instance_name_stub),
		GetFileName: Some(get_file_name_stub),
		GetOriginalClipFrameRate: Some(get_original_clip_frame_rate_stub),
		GetSourceTrackMediaTimecode: Some(get_source_track_media_timecode_stub),
		GetSourceTrackClipName: Some(get_source_track_clip_name_stub),
		GetSourceTrackFileName: Some(get_source_track_file_name_stub),
		EffectDependsOnClipName2: Some(effect_depends_on_clip_name2_stub),
		GetMediaTimecode2: Some(get_media_timecode2_stub),
		GetSourceTrackMediaTimecode2: Some(get_source_track_media_timecode2_stub),
		GetSourceTrackClipName2: Some(get_source_track_clip_name2_stub),
		GetSourceTrackFileName2: Some(get_source_track_file_name2_stub),
		GetCommentString: Some(get_comment_string_stub),
		GetLogNoteString: Some(get_log_note_string_stub),
		GetCameraRollString: Some(get_camera_roll_string_stub),
		GetClientMetadataString: Some(get_client_metadata_string_stub),
		GetDailyRollString: Some(get_daily_roll_string_stub),
		GetDescriptionString: Some(get_description_string_stub),
		GetLabRollString: Some(get_lab_roll_string_stub),
		GetSceneString: Some(get_scene_string_stub),
		GetShotString: Some(get_shot_string_stub),
		GetTapeNameString: Some(get_tape_name_string_stub),
		GetVideoCodecString: Some(get_video_codec_string_stub),
		GetGoodMetadataString: Some(get_good_metadata_string_stub),
		GetSoundRollString: Some(get_sound_roll_string_stub),
		GetSequenceTime: Some(get_sequence_time_stub),
		GetSoundTimecode: Some(get_sound_timecode_stub),
		GetOriginalClipFrameRateForSourceTrack: Some(get_original_clip_frame_rate_for_source_track_stub),
		GetMediaFrameRateForSourceTrack: Some(get_media_frame_rate_for_source_track_stub),
		GetSourceTrackMediaActualStartTime: Some(get_source_track_media_actual_start_time_stub),
		IsSourceTrackMediaTrimmed: Some(is_source_track_media_trimmed_stub),
		IsMediaTrimmed: Some(is_media_trimmed_stub),
		IsTrackEmpty: Some(is_track_empty_stub),
		IsTrackItemEffectAppliedToSynthetic: Some(is_track_item_effect_applied_to_synthetic_stub),
	}
}

// ============================================================================
// AEGP Utility Suite (minimal emulation)
// ============================================================================

static NEXT_AEGP_PLUGIN_ID: AtomicI32 = AtomicI32::new(1);

#[repr(C)]
pub struct AEGPUtilitySuiteCompatV11 {
	slots: [*const c_void; 48],
}

// SAFETY: the slots are all `'static` function pointers (stubs cast to
// `*const c_void`); the table is populated once at init and only ever read
// afterwards, so sharing a single instance across threads is sound.
unsafe impl Sync for AEGPUtilitySuiteCompatV11 {}
// SAFETY: same reasoning - the raw pointers are `'static` and immutable after
// construction, so moving/initializing the table on another thread is sound.
// (Required for `LazyLock<AEGPUtilitySuiteCompatV11>` to be `Sync`.)
unsafe impl Send for AEGPUtilitySuiteCompatV11 {}

/// `AEGP Utility Suite` through version 5 (wire versions 1 to 12).
///
/// `AEGP_RegisterWithAEGP` is slot 8 and `AEGP_GetMainHWND` is slot 9.
static AEGP_UTILITY_SUITE_5: LazyLock<AEGPUtilitySuiteCompatV11> = LazyLock::new(build_aegp_utility_suite_v5);

/// `AEGP Utility Suite` version 6 and later (wire version 13 onwards).
///
/// `AEGP_ReportInfoUnicode` is inserted at slot 1, so registration moves to
/// slot 9 and the main window to slot 10.
static AEGP_UTILITY_SUITE_6: LazyLock<AEGPUtilitySuiteCompatV11> = LazyLock::new(build_aegp_utility_suite_v6);

/// The utility suite whose layout matches `version`.
///
/// Wire version 13 (`AEGP_UtilitySuite6`) is the first one with
/// `AEGP_ReportInfoUnicode`. Earlier versions share the version 5 layout.
pub(crate) fn aegp_utility_suite(version: i32) -> *const c_void {
	let suite = if version >= after_effects_sys::kAEGPUtilitySuiteVersion6 as i32 {
		&*AEGP_UTILITY_SUITE_6
	} else {
		&*AEGP_UTILITY_SUITE_5
	};
	suite as *const AEGPUtilitySuiteCompatV11 as *const c_void
}

unsafe extern "C" fn aegp_noop_ok_stub() -> A_Err {
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn aegp_register_with_aegp_stub(
	global_refcon: AEGP_GlobalRefcon,
	plugin_nameZ: *const A_char,
	plugin_id: *mut AEGP_PluginID,
) -> A_Err {
	if plugin_id.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	}

	if !plugin_nameZ.is_null() {
		#[cfg(feature = "diagnostics")]
		if let Ok(name) = unsafe { std::ffi::CStr::from_ptr(plugin_nameZ) }.to_str() {
			log::debug!("AEGP_RegisterWithAEGP: {}", name);
		}
	}

	unsafe {
		let id = NEXT_AEGP_PLUGIN_ID.fetch_add(1, Ordering::Relaxed);
		if !crate::suites::register::register_plugin(id, global_refcon) {
			return PF_Err_BAD_CALLBACK_PARAM as A_Err;
		}
		*plugin_id = id;
	}

	PF_Err_NONE as A_Err
}

unsafe extern "C" fn aegp_is_scripting_available_stub(out_available_pb: *mut A_Boolean) -> A_Err {
	if !out_available_pb.is_null() {
		unsafe {
			*out_available_pb = 1;
		}
	}
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn aegp_execute_script_stub(
	_in_plugin_id: AEGP_PluginID,
	_in_script_z: *const A_char,
	_platform_encoding_b: A_Boolean,
	out_result_ph0: *mut AEGP_MemHandle,
	out_error_string_ph0: *mut AEGP_MemHandle,
) -> A_Err {
	if !out_result_ph0.is_null() {
		unsafe {
			*out_result_ph0 = std::ptr::null_mut();
		}
	}
	if !out_error_string_ph0.is_null() {
		unsafe {
			*out_error_string_ph0 = std::ptr::null_mut();
		}
	}

	#[cfg(feature = "diagnostics")]
	if !_in_script_z.is_null()
		&& let Ok(script) = unsafe { std::ffi::CStr::from_ptr(_in_script_z) }.to_str()
	{
		log::debug!("AEGP_ExecuteScript(len={}): stubbed", script.len());
	}

	PF_Err_NONE as A_Err
}

/// `AEGP_GetMainHWND`: write the host window into the caller's `HWND*`.
///
/// The SDK types the out-parameter as `void *`. Callers pass the address of
/// an `HWND`, and an editor owned by a hidden window never appears.
unsafe extern "C" fn aegp_get_main_hwnd(main_hwnd: *mut c_void) -> A_Err {
	if main_hwnd.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	}
	#[cfg(windows)]
	let hwnd = crate::host::utility::host_window();
	#[cfg(not(windows))]
	let hwnd = std::ptr::null_mut();
	unsafe { *(main_hwnd as *mut *mut c_void) = hwnd };
	PF_Err_NONE as A_Err
}

fn utility_suite(
	register_at: usize,
	hwnd_at: usize,
	scripting_at: usize,
	execute_at: usize,
) -> AEGPUtilitySuiteCompatV11 {
	let noop = aegp_noop_ok_stub as *const () as *const c_void;
	let mut suite = AEGPUtilitySuiteCompatV11 { slots: [noop; 48] };
	suite.slots[register_at] = aegp_register_with_aegp_stub as *const () as *const c_void;
	suite.slots[hwnd_at] = aegp_get_main_hwnd as *const () as *const c_void;
	suite.slots[scripting_at] = aegp_is_scripting_available_stub as *const () as *const c_void;
	suite.slots[execute_at] = aegp_execute_script_stub as *const () as *const c_void;
	suite
}

/// Version 5 layout: registration at `+0x40`, main window at `+0x48`,
/// scripting at `+0xd0` and `+0xd8`.
fn build_aegp_utility_suite_v5() -> AEGPUtilitySuiteCompatV11 {
	utility_suite(8, 9, 26, 27)
}

/// Version 6 layout: one slot later, after `AEGP_ReportInfoUnicode`.
fn build_aegp_utility_suite_v6() -> AEGPUtilitySuiteCompatV11 {
	utility_suite(9, 10, 27, 28)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn utility_suite_6_registers_a_plugin_and_returns_the_main_window() {
		let owner = crate::suites::register::new_owner();
		let _scope = crate::suites::register::enter_owner(owner);
		let suite = unsafe {
			&*(aegp_utility_suite(after_effects_sys::kAEGPUtilitySuiteVersion6 as i32)
				as *const after_effects_sys::AEGP_UtilitySuite6)
		};
		let mut id = 0;
		let err =
			unsafe { suite.AEGP_RegisterWithAEGP.unwrap()(std::ptr::null_mut(), c"Venustas".as_ptr().cast(), &mut id) };
		assert_eq!(err, PF_Err_NONE as A_Err);
		assert_ne!(id, 0);

		#[cfg(windows)]
		{
			let sentinel = 0x1234 as *mut c_void;
			crate::host::utility::set_main_window(sentinel);
			let mut hwnd = std::ptr::null_mut();
			let err = unsafe { suite.AEGP_GetMainHWND.unwrap()((&mut hwnd) as *mut *mut c_void as *mut c_void) };
			crate::host::utility::set_main_window(std::ptr::null_mut());
			assert_eq!(err, PF_Err_NONE as A_Err);
			assert_eq!(hwnd, sentinel);
		}
	}

	#[test]
	fn utility_suite_5_keeps_registration_one_slot_earlier() {
		let v5 = unsafe {
			&*(aegp_utility_suite(after_effects_sys::kAEGPUtilitySuiteVersion5 as i32)
				as *const after_effects_sys::AEGP_UtilitySuite5)
		};
		let v6 = unsafe {
			&*(aegp_utility_suite(after_effects_sys::kAEGPUtilitySuiteVersion6 as i32)
				as *const after_effects_sys::AEGP_UtilitySuite6)
		};
		let register = aegp_register_with_aegp_stub as *const ();
		assert_eq!(v5.AEGP_RegisterWithAEGP.unwrap() as *const (), register);
		assert_eq!(v6.AEGP_RegisterWithAEGP.unwrap() as *const (), register);
		assert_ne!(v5.AEGP_EndUndoGroup.unwrap() as *const (), register);
		assert_ne!(v6.AEGP_StartUndoGroup.unwrap() as *const (), register);
	}
}
