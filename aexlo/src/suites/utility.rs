//! Dummy implementation for PF Utility Suite (Premiere/AE support callbacks)

use crate::suites::macros::stub_log;
use after_effects_sys::*;
use crate::suites::macros::stub_log;
use after_effects_sys::*;

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

/// Creates a dynamically allocated `PF_UtilitySuite` instance.
/// Returns a Box<> that will be converted to Arc by the registry.
pub fn create_utility_suite() -> Box<PF_UtilitySuite> {
	let suite = Box::new(PF_UtilitySuite {
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
		GetOriginalClipFrameRateForSourceTrack: Some(
			get_original_clip_frame_rate_for_source_track_stub,
		),
		GetMediaFrameRateForSourceTrack: Some(get_media_frame_rate_for_source_track_stub),
		GetSourceTrackMediaActualStartTime: Some(get_source_track_media_actual_start_time_stub),
		IsSourceTrackMediaTrimmed: Some(is_source_track_media_trimmed_stub),
		IsMediaTrimmed: Some(is_media_trimmed_stub),
		IsTrackEmpty: Some(is_track_empty_stub),
		IsTrackItemEffectAppliedToSynthetic: Some(is_track_item_effect_applied_to_synthetic_stub),
	});
	suite
}
