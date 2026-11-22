//! `AEGP Keyframe Suite` (versions 4 and 5) over the streams of the
//! [Stream Suite](super::stream).
//!
//! Keyframes are the embedder's (`PluginInstance::set_param_track`, and the
//! tracks of a project) and read-only here: an index there is no keyframe at
//! gets `PF_Err_INVALID_INDEX`, and adding or changing one is refused with
//! `A_Err_GENERIC`. They are linear or held, so bezier eases and spatial
//! tangents, which are not known, are refused rather than answered with zeros.
//! Version 5 appends the keyframe label colour calls, which address a keyframe
//! by index too.

use crate::sys::{
	A_Boolean, A_Err, A_Err_GENERIC, A_Time, A_long, A_short, AEGP_AddKeyframesInfoH, AEGP_KeyframeEase,
	AEGP_KeyframeFlags, AEGP_KeyframeIndex, AEGP_KeyframeInterpolationType, AEGP_KeyframeSuite4, AEGP_KeyframeSuite5,
	AEGP_LTimeMode, AEGP_NumKF_NO_DATA, AEGP_PluginID, AEGP_StreamRefH, AEGP_StreamValue2, PF_Err_BAD_CALLBACK_PARAM,
	PF_Err_INVALID_INDEX, PF_Err_NONE,
};

use crate::animation::{Interpolation, TimePoint};
use crate::core::diagnostics::diag;
use crate::suites::stream::{Stream, StreamValue, stream_of, stream_value};

/// A stream, the index of one of its keyframes, and all of them.
type Key = (Stream, usize, Vec<(TimePoint, Interpolation)>);

/// The stream and its keyframe `index`, or the error to give.
fn key(name: &'static str, streamH: AEGP_StreamRefH, index: AEGP_KeyframeIndex) -> Result<Key, A_Err> {
	diag!(name, "streamH" => format!("{:#x}", streamH as usize), "key_index" => index);
	let stream = unsafe { stream_of(streamH) }.ok_or(PF_Err_BAD_CALLBACK_PARAM)?;
	if stream.value(None).is_none() {
		return Err(PF_Err_BAD_CALLBACK_PARAM);
	}
	let keys = stream.keys();
	match usize::try_from(index).ok().filter(|&at| at < keys.len()) {
		Some(at) => Ok((stream, at, keys)),
		None => Err(PF_Err_INVALID_INDEX),
	}
}

/// A call aexlo refuses for the keyframes there are.
fn refuse_key(name: &'static str, streamH: AEGP_StreamRefH, index: AEGP_KeyframeIndex) -> A_Err {
	match key(name, streamH, index) {
		Ok(_) => unsupported(name),
		Err(err) => err,
	}
}

fn unsupported(name: &'static str) -> A_Err {
	diag!(name);
	crate::observe::unsupported(
		crate::observe::UnsupportedKind::Declined,
		name,
		"keyframes are read-only, linear or held",
	);
	A_Err_GENERIC
}

unsafe extern "C" fn get_stream_num_kfs(streamH: AEGP_StreamRefH, num_kfsPL: *mut A_long) -> A_Err {
	crate::core::ffi::guard("get_stream_num_kfs", || {
		diag!("AEGP_KeyframeSuite4/AEGP_GetStreamNumKFs", "streamH" => format!("{:#x}", streamH as usize));

		let Some(value) = (unsafe { stream_value(streamH) }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		let Some(out) = (unsafe { num_kfsPL.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		*out = if value == StreamValue::NoData {
			AEGP_NumKF_NO_DATA as A_long
		} else {
			// SAFETY: `stream_value` found the handle good.
			unsafe { stream_of(streamH) }.map_or(0, |stream| stream.keys().len() as A_long)
		};
		PF_Err_NONE
	})
}

unsafe extern "C" fn get_keyframe_time(
	streamH: AEGP_StreamRefH,
	key_index: AEGP_KeyframeIndex,
	time_mode: AEGP_LTimeMode,
	timePT: *mut A_Time,
) -> A_Err {
	crate::core::ffi::guard("get_keyframe_time", || {
		let (stream, at, keys) = match key("AEGP_KeyframeSuite4/AEGP_GetKeyframeTime", streamH, key_index) {
			Ok(found) => found,
			Err(err) => return err,
		};
		let Some(out) = (unsafe { timePT.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		*out = match stream.timeline() {
			Some(timeline) => timeline.told_in(keys[at].0, time_mode),
			None => keys[at].0,
		}
		.to_sys();
		PF_Err_NONE
	})
}

unsafe extern "C" fn insert_keyframe(
	_streamH: AEGP_StreamRefH,
	_time_mode: AEGP_LTimeMode,
	_timePT: *const A_Time,
	_key_indexP: *mut AEGP_KeyframeIndex,
) -> A_Err {
	crate::core::ffi::guard("insert_keyframe", || {
		unsupported("AEGP_KeyframeSuite4/AEGP_InsertKeyframe")
	})
}

unsafe extern "C" fn delete_keyframe(streamH: AEGP_StreamRefH, key_index: AEGP_KeyframeIndex) -> A_Err {
	crate::core::ffi::guard("delete_keyframe", || {
		refuse_key("AEGP_KeyframeSuite4/AEGP_DeleteKeyframe", streamH, key_index)
	})
}

unsafe extern "C" fn get_new_keyframe_value(
	_aegp_plugin_id: AEGP_PluginID,
	streamH: AEGP_StreamRefH,
	key_index: AEGP_KeyframeIndex,
	valueP: *mut AEGP_StreamValue2,
) -> A_Err {
	crate::core::ffi::guard("get_new_keyframe_value", || {
		let (_, at, keys) = match key("AEGP_KeyframeSuite4/AEGP_GetNewKeyframeValue", streamH, key_index) {
			Ok(found) => found,
			Err(err) => return err,
		};
		crate::suites::stream::keyframe_value(streamH, keys[at].0, valueP)
	})
}

unsafe extern "C" fn set_keyframe_value(
	streamH: AEGP_StreamRefH,
	key_index: AEGP_KeyframeIndex,
	_valueP: *const AEGP_StreamValue2,
) -> A_Err {
	crate::core::ffi::guard("set_keyframe_value", || {
		refuse_key("AEGP_KeyframeSuite4/AEGP_SetKeyframeValue", streamH, key_index)
	})
}

unsafe extern "C" fn get_stream_value_dimensionality(streamH: AEGP_StreamRefH, value_dimPS: *mut A_short) -> A_Err {
	crate::core::ffi::guard("get_stream_value_dimensionality", || {
		diag!("AEGP_KeyframeSuite4/AEGP_GetStreamValueDimensionality", "streamH" => format!("{:#x}", streamH as usize));

		let Some(value) = (unsafe { stream_value(streamH) }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		let Some(out) = (unsafe { value_dimPS.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		*out = value.dimensionality();
		PF_Err_NONE
	})
}

unsafe extern "C" fn get_stream_temporal_dimensionality(
	streamH: AEGP_StreamRefH,
	temporal_dimPS: *mut A_short,
) -> A_Err {
	crate::core::ffi::guard("get_stream_temporal_dimensionality", || {
		diag!("AEGP_KeyframeSuite4/AEGP_GetStreamTemporalDimensionality", "streamH" => format!("{:#x}", streamH as usize));

		let Some(value) = (unsafe { stream_value(streamH) }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		let Some(out) = (unsafe { temporal_dimPS.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		*out = value.temporal_dimensionality();
		PF_Err_NONE
	})
}

unsafe extern "C" fn get_new_keyframe_spatial_tangents(
	_aegp_plugin_id: AEGP_PluginID,
	streamH: AEGP_StreamRefH,
	key_index: AEGP_KeyframeIndex,
	_in_tanP0: *mut AEGP_StreamValue2,
	_out_tanP0: *mut AEGP_StreamValue2,
) -> A_Err {
	crate::core::ffi::guard("get_new_keyframe_spatial_tangents", || {
		refuse_key(
			"AEGP_KeyframeSuite4/AEGP_GetNewKeyframeSpatialTangents",
			streamH,
			key_index,
		)
	})
}

unsafe extern "C" fn set_keyframe_spatial_tangents(
	streamH: AEGP_StreamRefH,
	key_index: AEGP_KeyframeIndex,
	_in_tanP0: *const AEGP_StreamValue2,
	_out_tanP0: *const AEGP_StreamValue2,
) -> A_Err {
	crate::core::ffi::guard("set_keyframe_spatial_tangents", || {
		refuse_key(
			"AEGP_KeyframeSuite4/AEGP_SetKeyframeSpatialTangents",
			streamH,
			key_index,
		)
	})
}

unsafe extern "C" fn get_keyframe_temporal_ease(
	streamH: AEGP_StreamRefH,
	key_index: AEGP_KeyframeIndex,
	_dimensionL: A_long,
	_in_easeP0: *mut AEGP_KeyframeEase,
	_out_easeP0: *mut AEGP_KeyframeEase,
) -> A_Err {
	crate::core::ffi::guard("get_keyframe_temporal_ease", || {
		refuse_key("AEGP_KeyframeSuite4/AEGP_GetKeyframeTemporalEase", streamH, key_index)
	})
}

unsafe extern "C" fn set_keyframe_temporal_ease(
	streamH: AEGP_StreamRefH,
	key_index: AEGP_KeyframeIndex,
	_dimensionL: A_long,
	_in_easeP0: *const AEGP_KeyframeEase,
	_out_easeP0: *const AEGP_KeyframeEase,
) -> A_Err {
	crate::core::ffi::guard("set_keyframe_temporal_ease", || {
		refuse_key("AEGP_KeyframeSuite4/AEGP_SetKeyframeTemporalEase", streamH, key_index)
	})
}

unsafe extern "C" fn get_keyframe_flags(
	streamH: AEGP_StreamRefH,
	key_index: AEGP_KeyframeIndex,
	flagsP: *mut AEGP_KeyframeFlags,
) -> A_Err {
	crate::core::ffi::guard("get_keyframe_flags", || {
		if let Err(err) = key("AEGP_KeyframeSuite4/AEGP_GetKeyframeFlags", streamH, key_index) {
			return err;
		}
		let Some(out) = (unsafe { flagsP.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		*out = 0;
		PF_Err_NONE
	})
}

unsafe extern "C" fn set_keyframe_flag(
	streamH: AEGP_StreamRefH,
	key_index: AEGP_KeyframeIndex,
	_flag: AEGP_KeyframeFlags,
	_true_falseB: A_Boolean,
) -> A_Err {
	crate::core::ffi::guard("set_keyframe_flag", || {
		refuse_key("AEGP_KeyframeSuite4/AEGP_SetKeyframeFlag", streamH, key_index)
	})
}

unsafe extern "C" fn get_keyframe_interpolation(
	streamH: AEGP_StreamRefH,
	key_index: AEGP_KeyframeIndex,
	in_interpP0: *mut AEGP_KeyframeInterpolationType,
	out_interpP0: *mut AEGP_KeyframeInterpolationType,
) -> A_Err {
	crate::core::ffi::guard("get_keyframe_interpolation", || {
		let (_, at, keys) = match key("AEGP_KeyframeSuite4/AEGP_GetKeyframeInterpolation", streamH, key_index) {
			Ok(found) => found,
			Err(err) => return err,
		};
		let told = |interpolation| match interpolation {
			Interpolation::Linear => crate::sys::AEGP_KeyInterp_LINEAR,
			Interpolation::Hold => crate::sys::AEGP_KeyInterp_HOLD,
		} as AEGP_KeyframeInterpolationType;
		unsafe {
			if let Some(out) = in_interpP0.as_mut() {
				// It arrives as the keyframe before it leaves.
				*out = told(keys[at.saturating_sub(1)].1);
			}
			if let Some(out) = out_interpP0.as_mut() {
				*out = told(keys[at].1);
			}
		}
		PF_Err_NONE
	})
}

unsafe extern "C" fn set_keyframe_interpolation(
	streamH: AEGP_StreamRefH,
	key_index: AEGP_KeyframeIndex,
	_in_interp: AEGP_KeyframeInterpolationType,
	_out_interp: AEGP_KeyframeInterpolationType,
) -> A_Err {
	crate::core::ffi::guard("set_keyframe_interpolation", || {
		refuse_key("AEGP_KeyframeSuite4/AEGP_SetKeyframeInterpolation", streamH, key_index)
	})
}

unsafe extern "C" fn start_add_keyframes(_streamH: AEGP_StreamRefH, _akPH: *mut AEGP_AddKeyframesInfoH) -> A_Err {
	crate::core::ffi::guard("start_add_keyframes", || {
		unsupported("AEGP_KeyframeSuite4/AEGP_StartAddKeyframes")
	})
}

unsafe extern "C" fn add_keyframes(
	_akH: AEGP_AddKeyframesInfoH,
	_time_mode: AEGP_LTimeMode,
	_timePT: *const A_Time,
	_key_indexPL: *mut A_long,
) -> A_Err {
	crate::core::ffi::guard("add_keyframes", || unsupported("AEGP_KeyframeSuite4/AEGP_AddKeyframes"))
}

unsafe extern "C" fn set_add_keyframe(
	_akH: AEGP_AddKeyframesInfoH,
	_key_indexL: A_long,
	_valueP: *const AEGP_StreamValue2,
) -> A_Err {
	crate::core::ffi::guard("set_add_keyframe", || {
		unsupported("AEGP_KeyframeSuite4/AEGP_SetAddKeyframe")
	})
}

unsafe extern "C" fn end_add_keyframes(_addB: A_Boolean, _akH: AEGP_AddKeyframesInfoH) -> A_Err {
	crate::core::ffi::guard("end_add_keyframes", || {
		unsupported("AEGP_KeyframeSuite4/AEGP_EndAddKeyframes")
	})
}

unsafe extern "C" fn get_keyframe_label_color_index(
	streamH: AEGP_StreamRefH,
	key_index: AEGP_KeyframeIndex,
	_key_labelP: *mut A_long,
) -> A_Err {
	crate::core::ffi::guard("get_keyframe_label_color_index", || {
		refuse_key(
			"AEGP_KeyframeSuite5/AEGP_GetKeyframeLabelColorIndex",
			streamH,
			key_index,
		)
	})
}

unsafe extern "C" fn set_keyframe_label_color_index(
	streamH: AEGP_StreamRefH,
	key_index: AEGP_KeyframeIndex,
	_key_label: A_long,
) -> A_Err {
	crate::core::ffi::guard("set_keyframe_label_color_index", || {
		refuse_key(
			"AEGP_KeyframeSuite5/AEGP_SetKeyframeLabelColorIndex",
			streamH,
			key_index,
		)
	})
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

/// Builds the `AEGP_KeyframeSuite5` vtable: v4 plus the label colour calls.
pub(super) const fn create_keyframe_suite_5() -> AEGP_KeyframeSuite5 {
	let v4 = create_keyframe_suite_4();
	AEGP_KeyframeSuite5 {
		AEGP_GetStreamNumKFs: v4.AEGP_GetStreamNumKFs,
		AEGP_GetKeyframeTime: v4.AEGP_GetKeyframeTime,
		AEGP_InsertKeyframe: v4.AEGP_InsertKeyframe,
		AEGP_DeleteKeyframe: v4.AEGP_DeleteKeyframe,
		AEGP_GetNewKeyframeValue: v4.AEGP_GetNewKeyframeValue,
		AEGP_SetKeyframeValue: v4.AEGP_SetKeyframeValue,
		AEGP_GetStreamValueDimensionality: v4.AEGP_GetStreamValueDimensionality,
		AEGP_GetStreamTemporalDimensionality: v4.AEGP_GetStreamTemporalDimensionality,
		AEGP_GetNewKeyframeSpatialTangents: v4.AEGP_GetNewKeyframeSpatialTangents,
		AEGP_SetKeyframeSpatialTangents: v4.AEGP_SetKeyframeSpatialTangents,
		AEGP_GetKeyframeTemporalEase: v4.AEGP_GetKeyframeTemporalEase,
		AEGP_SetKeyframeTemporalEase: v4.AEGP_SetKeyframeTemporalEase,
		AEGP_GetKeyframeFlags: v4.AEGP_GetKeyframeFlags,
		AEGP_SetKeyframeFlag: v4.AEGP_SetKeyframeFlag,
		AEGP_GetKeyframeInterpolation: v4.AEGP_GetKeyframeInterpolation,
		AEGP_SetKeyframeInterpolation: v4.AEGP_SetKeyframeInterpolation,
		AEGP_StartAddKeyframes: v4.AEGP_StartAddKeyframes,
		AEGP_AddKeyframes: v4.AEGP_AddKeyframes,
		AEGP_SetAddKeyframe: v4.AEGP_SetAddKeyframe,
		AEGP_EndAddKeyframes: v4.AEGP_EndAddKeyframes,
		AEGP_GetKeyframeLabelColorIndex: Some(get_keyframe_label_color_index),
		AEGP_SetKeyframeLabelColorIndex: Some(set_keyframe_label_color_index),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn label_colors_address_keyframes_that_do_not_exist() {
		let suite = create_keyframe_suite_5();
		let mut label: A_long = 0;
		unsafe {
			assert_eq!(
				suite.AEGP_GetKeyframeLabelColorIndex.unwrap()(std::ptr::null_mut(), 0, &mut label),
				PF_Err_BAD_CALLBACK_PARAM
			);
			assert_eq!(
				suite.AEGP_SetKeyframeLabelColorIndex.unwrap()(std::ptr::null_mut(), 0, 1),
				PF_Err_BAD_CALLBACK_PARAM
			);
		}
	}

	#[test]
	fn null_streams_are_rejected() {
		let suite = create_keyframe_suite_4();
		let mut n: A_long = 7;
		unsafe {
			assert_eq!(
				suite.AEGP_GetStreamNumKFs.unwrap()(std::ptr::null_mut(), &mut n),
				PF_Err_BAD_CALLBACK_PARAM
			);
		}
		assert_eq!(n, 7);
	}
}
