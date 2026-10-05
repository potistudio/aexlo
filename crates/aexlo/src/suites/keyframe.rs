//! `AEGP Keyframe Suite` (version 4) over the effect parameter streams of the
//! [Stream Suite](super::stream).
//!
//! aexlo has no timeline, so every stream has zero keyframes: the queries
//! answer accordingly, anything addressing a keyframe by index gets
//! `PF_Err_INVALID_INDEX`, and adding keyframes is refused with `A_Err_GENERIC`.

use after_effects_sys::{
	A_Boolean, A_Err, A_Err_GENERIC, A_Time, A_long, A_short, AEGP_AddKeyframesInfoH, AEGP_KeyframeEase,
	AEGP_KeyframeFlags, AEGP_KeyframeIndex, AEGP_KeyframeInterpolationType, AEGP_KeyframeSuite4, AEGP_LTimeMode,
	AEGP_NumKF_NO_DATA, AEGP_PluginID, AEGP_StreamRefH, AEGP_StreamValue2, PF_Err_BAD_CALLBACK_PARAM,
	PF_Err_INVALID_INDEX, PF_Err_NONE,
};

use crate::core::diagnostics::diag;
use crate::suites::stream::{StreamValue, stream_value};

fn no_such_keyframe(name: &'static str, streamH: AEGP_StreamRefH) -> A_Err {
	diag!(name, "streamH" => format!("{:#x}", streamH as usize));
	if unsafe { stream_value(streamH) }.is_none() {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	}
	PF_Err_INVALID_INDEX as A_Err
}

fn unsupported(name: &'static str) -> A_Err {
	diag!(name);
	log::warn!("{name}: aexlo has no timeline to add keyframes to");
	A_Err_GENERIC as A_Err
}

unsafe extern "C" fn get_stream_num_kfs(streamH: AEGP_StreamRefH, num_kfsPL: *mut A_long) -> A_Err {
	diag!("AEGP_KeyframeSuite4/AEGP_GetStreamNumKFs", "streamH" => format!("{:#x}", streamH as usize));

	let Some(value) = (unsafe { stream_value(streamH) }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	let Some(out) = (unsafe { num_kfsPL.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	*out = if value == StreamValue::NoData {
		AEGP_NumKF_NO_DATA as A_long
	} else {
		0
	};
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn get_keyframe_time(
	streamH: AEGP_StreamRefH,
	_key_index: AEGP_KeyframeIndex,
	_time_mode: AEGP_LTimeMode,
	_timePT: *mut A_Time,
) -> A_Err {
	no_such_keyframe("AEGP_KeyframeSuite4/AEGP_GetKeyframeTime", streamH)
}

unsafe extern "C" fn insert_keyframe(
	_streamH: AEGP_StreamRefH,
	_time_mode: AEGP_LTimeMode,
	_timePT: *const A_Time,
	_key_indexP: *mut AEGP_KeyframeIndex,
) -> A_Err {
	unsupported("AEGP_KeyframeSuite4/AEGP_InsertKeyframe")
}

unsafe extern "C" fn delete_keyframe(streamH: AEGP_StreamRefH, _key_index: AEGP_KeyframeIndex) -> A_Err {
	no_such_keyframe("AEGP_KeyframeSuite4/AEGP_DeleteKeyframe", streamH)
}

unsafe extern "C" fn get_new_keyframe_value(
	_aegp_plugin_id: AEGP_PluginID,
	streamH: AEGP_StreamRefH,
	_key_index: AEGP_KeyframeIndex,
	_valueP: *mut AEGP_StreamValue2,
) -> A_Err {
	no_such_keyframe("AEGP_KeyframeSuite4/AEGP_GetNewKeyframeValue", streamH)
}

unsafe extern "C" fn set_keyframe_value(
	streamH: AEGP_StreamRefH,
	_key_index: AEGP_KeyframeIndex,
	_valueP: *const AEGP_StreamValue2,
) -> A_Err {
	no_such_keyframe("AEGP_KeyframeSuite4/AEGP_SetKeyframeValue", streamH)
}

unsafe extern "C" fn get_stream_value_dimensionality(streamH: AEGP_StreamRefH, value_dimPS: *mut A_short) -> A_Err {
	diag!("AEGP_KeyframeSuite4/AEGP_GetStreamValueDimensionality", "streamH" => format!("{:#x}", streamH as usize));

	let Some(value) = (unsafe { stream_value(streamH) }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	let Some(out) = (unsafe { value_dimPS.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	*out = value.dimensionality();
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn get_stream_temporal_dimensionality(
	streamH: AEGP_StreamRefH,
	temporal_dimPS: *mut A_short,
) -> A_Err {
	diag!("AEGP_KeyframeSuite4/AEGP_GetStreamTemporalDimensionality", "streamH" => format!("{:#x}", streamH as usize));

	let Some(value) = (unsafe { stream_value(streamH) }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	let Some(out) = (unsafe { temporal_dimPS.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	*out = value.temporal_dimensionality();
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn get_new_keyframe_spatial_tangents(
	_aegp_plugin_id: AEGP_PluginID,
	streamH: AEGP_StreamRefH,
	_key_index: AEGP_KeyframeIndex,
	_in_tanP0: *mut AEGP_StreamValue2,
	_out_tanP0: *mut AEGP_StreamValue2,
) -> A_Err {
	no_such_keyframe("AEGP_KeyframeSuite4/AEGP_GetNewKeyframeSpatialTangents", streamH)
}

unsafe extern "C" fn set_keyframe_spatial_tangents(
	streamH: AEGP_StreamRefH,
	_key_index: AEGP_KeyframeIndex,
	_in_tanP0: *const AEGP_StreamValue2,
	_out_tanP0: *const AEGP_StreamValue2,
) -> A_Err {
	no_such_keyframe("AEGP_KeyframeSuite4/AEGP_SetKeyframeSpatialTangents", streamH)
}

unsafe extern "C" fn get_keyframe_temporal_ease(
	streamH: AEGP_StreamRefH,
	_key_index: AEGP_KeyframeIndex,
	_dimensionL: A_long,
	_in_easeP0: *mut AEGP_KeyframeEase,
	_out_easeP0: *mut AEGP_KeyframeEase,
) -> A_Err {
	no_such_keyframe("AEGP_KeyframeSuite4/AEGP_GetKeyframeTemporalEase", streamH)
}

unsafe extern "C" fn set_keyframe_temporal_ease(
	streamH: AEGP_StreamRefH,
	_key_index: AEGP_KeyframeIndex,
	_dimensionL: A_long,
	_in_easeP0: *const AEGP_KeyframeEase,
	_out_easeP0: *const AEGP_KeyframeEase,
) -> A_Err {
	no_such_keyframe("AEGP_KeyframeSuite4/AEGP_SetKeyframeTemporalEase", streamH)
}

unsafe extern "C" fn get_keyframe_flags(
	streamH: AEGP_StreamRefH,
	_key_index: AEGP_KeyframeIndex,
	_flagsP: *mut AEGP_KeyframeFlags,
) -> A_Err {
	no_such_keyframe("AEGP_KeyframeSuite4/AEGP_GetKeyframeFlags", streamH)
}

unsafe extern "C" fn set_keyframe_flag(
	streamH: AEGP_StreamRefH,
	_key_index: AEGP_KeyframeIndex,
	_flag: AEGP_KeyframeFlags,
	_true_falseB: A_Boolean,
) -> A_Err {
	no_such_keyframe("AEGP_KeyframeSuite4/AEGP_SetKeyframeFlag", streamH)
}

unsafe extern "C" fn get_keyframe_interpolation(
	streamH: AEGP_StreamRefH,
	_key_index: AEGP_KeyframeIndex,
	_in_interpP0: *mut AEGP_KeyframeInterpolationType,
	_out_interpP0: *mut AEGP_KeyframeInterpolationType,
) -> A_Err {
	no_such_keyframe("AEGP_KeyframeSuite4/AEGP_GetKeyframeInterpolation", streamH)
}

unsafe extern "C" fn set_keyframe_interpolation(
	streamH: AEGP_StreamRefH,
	_key_index: AEGP_KeyframeIndex,
	_in_interp: AEGP_KeyframeInterpolationType,
	_out_interp: AEGP_KeyframeInterpolationType,
) -> A_Err {
	no_such_keyframe("AEGP_KeyframeSuite4/AEGP_SetKeyframeInterpolation", streamH)
}

unsafe extern "C" fn start_add_keyframes(_streamH: AEGP_StreamRefH, _akPH: *mut AEGP_AddKeyframesInfoH) -> A_Err {
	unsupported("AEGP_KeyframeSuite4/AEGP_StartAddKeyframes")
}

unsafe extern "C" fn add_keyframes(
	_akH: AEGP_AddKeyframesInfoH,
	_time_mode: AEGP_LTimeMode,
	_timePT: *const A_Time,
	_key_indexPL: *mut A_long,
) -> A_Err {
	unsupported("AEGP_KeyframeSuite4/AEGP_AddKeyframes")
}

unsafe extern "C" fn set_add_keyframe(
	_akH: AEGP_AddKeyframesInfoH,
	_key_indexL: A_long,
	_valueP: *const AEGP_StreamValue2,
) -> A_Err {
	unsupported("AEGP_KeyframeSuite4/AEGP_SetAddKeyframe")
}

unsafe extern "C" fn end_add_keyframes(_addB: A_Boolean, _akH: AEGP_AddKeyframesInfoH) -> A_Err {
	unsupported("AEGP_KeyframeSuite4/AEGP_EndAddKeyframes")
}

/// Builds the `AEGP_KeyframeSuite4` vtable.
pub(super) const fn create_keyframe_suite_4() -> AEGP_KeyframeSuite4 {
	AEGP_KeyframeSuite4 {
		AEGP_GetStreamNumKFs: Some(get_stream_num_kfs),
		AEGP_GetKeyframeTime: Some(get_keyframe_time),
		AEGP_InsertKeyframe: Some(insert_keyframe),
		AEGP_DeleteKeyframe: Some(delete_keyframe),
		AEGP_GetNewKeyframeValue: Some(get_new_keyframe_value),
		AEGP_SetKeyframeValue: Some(set_keyframe_value),
		AEGP_GetStreamValueDimensionality: Some(get_stream_value_dimensionality),
		AEGP_GetStreamTemporalDimensionality: Some(get_stream_temporal_dimensionality),
		AEGP_GetNewKeyframeSpatialTangents: Some(get_new_keyframe_spatial_tangents),
		AEGP_SetKeyframeSpatialTangents: Some(set_keyframe_spatial_tangents),
		AEGP_GetKeyframeTemporalEase: Some(get_keyframe_temporal_ease),
		AEGP_SetKeyframeTemporalEase: Some(set_keyframe_temporal_ease),
		AEGP_GetKeyframeFlags: Some(get_keyframe_flags),
		AEGP_SetKeyframeFlag: Some(set_keyframe_flag),
		AEGP_GetKeyframeInterpolation: Some(get_keyframe_interpolation),
		AEGP_SetKeyframeInterpolation: Some(set_keyframe_interpolation),
		AEGP_StartAddKeyframes: Some(start_add_keyframes),
		AEGP_AddKeyframes: Some(add_keyframes),
		AEGP_SetAddKeyframe: Some(set_add_keyframe),
		AEGP_EndAddKeyframes: Some(end_add_keyframes),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn null_streams_are_rejected() {
		let suite = create_keyframe_suite_4();
		let mut n: A_long = 7;
		unsafe {
			assert_eq!(
				suite.AEGP_GetStreamNumKFs.unwrap()(std::ptr::null_mut(), &mut n),
				PF_Err_BAD_CALLBACK_PARAM as A_Err
			);
		}
		assert_eq!(n, 7);
	}
}
