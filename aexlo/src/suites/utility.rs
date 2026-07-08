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
// SAFETY: same reasoning — the raw pointers are `'static` and immutable after
// construction, so moving/initializing the table on another thread is sound.
// (Required for `LazyLock<AEGPUtilitySuiteCompatV11>` to be `Sync`.)
unsafe impl Send for AEGPUtilitySuiteCompatV11 {}

/// Process-wide `AEGP Utility Suite` (compat v11) instance.
///
/// Unlike the other suites this one cannot live in the `const` [`SUITE_CONTAINER`]
/// (crate::suites::SUITE_CONTAINER): its `slots` are `*const c_void`, and casting a
/// function pointer to a raw pointer is not permitted in a `const` initializer.
/// A [`LazyLock`] performs that cast once, on first acquire.
pub static AEGP_UTILITY_SUITE: LazyLock<AEGPUtilitySuiteCompatV11> = LazyLock::new(build_aegp_utility_suite_compat_v11);

unsafe extern "C" fn aegp_noop_ok_stub() -> A_Err {
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn aegp_register_with_aegp_stub(
	_global_refcon: AEGP_GlobalRefcon,
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
		*plugin_id = NEXT_AEGP_PLUGIN_ID.fetch_add(1, Ordering::Relaxed);
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

/// Builds the offset-compatible `AEGP Utility Suite` table for newer suite versions.
///
/// The target plugin requests v11 and calls at least offsets `+0x38` and `+0xd8`.
/// We keep all slots non-null to avoid indirect-call crashes from unimplemented entries.
///
/// Backs the [`AEGP_UTILITY_SUITE`] `LazyLock`; run once, on first acquire.
fn build_aegp_utility_suite_compat_v11() -> AEGPUtilitySuiteCompatV11 {
	let noop = aegp_noop_ok_stub as *const () as *const c_void;
	let mut suite = AEGPUtilitySuiteCompatV11 { slots: [noop; 48] };

	// +0x38 -> RegisterWithAEGP
	suite.slots[7] = aegp_register_with_aegp_stub as *const () as *const c_void;
	// +0xd0 -> IsScriptingAvailable (best effort)
	suite.slots[26] = aegp_is_scripting_available_stub as *const () as *const c_void;
	// +0xd8 -> ExecuteScript
	suite.slots[27] = aegp_execute_script_stub as *const () as *const c_void;

	suite
}
