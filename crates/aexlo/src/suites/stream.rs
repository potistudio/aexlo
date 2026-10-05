//! `AEGP Stream Suite`, served at wire versions 7 (`AEGP_StreamSuite2`) and 9
//! (`AEGP_StreamSuite4`), plus the `AEGP_StreamRefH` handles it hands out.
//!
//! The only streams aexlo can offer are an effect's own parameters: a stream
//! handle is the effect's `PF_ProgPtr` plus a parameter index (the same index
//! space as `PF_ParamDef`s, 0 being the input layer). aexlo has no timeline, so
//! a stream's value is the parameter's current value at every time and it never
//! has keyframes or expressions. Layer and mask streams, names and expressions
//! (which need the AEGP Memory Suite) and every write are refused with
//! `A_Err_GENERIC`.
//!
//! The two table versions differ only in their value structs
//! (`AEGP_StreamValue` vs `AEGP_StreamValue2`), whose unions share the members
//! written here.

use after_effects::ParamType;
use after_effects_sys::*;

use crate::core::diagnostics::diag;
use crate::suites::effect::{effect_ref_of, with_param};
use crate::utils;

/// What an `AEGP_StreamRefH` points at.
#[derive(Clone, Copy)]
struct Stream {
	effect_ref: PF_ProgPtr,
	index: usize,
}

fn new_stream_handle(stream: Stream) -> AEGP_StreamRefH {
	Box::into_raw(Box::new(stream)) as AEGP_StreamRefH
}

/// # Safety
/// `streamH` must be null or a handle from [`new_stream_handle`] that has not
/// been disposed.
unsafe fn stream_of(streamH: AEGP_StreamRefH) -> Option<Stream> {
	unsafe { (streamH as *const Stream).as_ref() }.copied()
}

/// A parameter's value as a stream sees it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum StreamValue {
	NoData,
	OneD(f64),
	TwoD {
		x: f64,
		y: f64,
		spatial: bool,
	},
	ThreeD {
		x: f64,
		y: f64,
		z: f64,
		spatial: bool,
	},
	Color {
		alpha: f64,
		red: f64,
		green: f64,
		blue: f64,
	},
	/// The layer chosen in a layer parameter; aexlo has no layer ids, so 0.
	LayerId,
}

impl StreamValue {
	fn of(def: &PF_ParamDef) -> Self {
		// SAFETY: every union read is selected by `param_type`.
		unsafe {
			match def.param_type {
				t if t == ParamType::Layer as PF_ParamType => Self::LayerId,
				t if t == ParamType::Slider as PF_ParamType => Self::OneD(def.u.sd.value as f64),
				t if t == ParamType::FixSlider as PF_ParamType => {
					Self::OneD(utils::fixed16_to_f32(def.u.fd.value) as f64)
				}
				t if t == ParamType::FloatSlider as PF_ParamType => Self::OneD(def.u.fs_d.value),
				t if t == ParamType::Angle as PF_ParamType => Self::OneD(utils::fixed16_to_f32(def.u.ad.value) as f64),
				t if t == ParamType::CheckBox as PF_ParamType => Self::OneD(def.u.bd.value as f64),
				t if t == ParamType::PopUp as PF_ParamType => Self::OneD(def.u.pd.value as f64),
				t if t == ParamType::Point as PF_ParamType => Self::TwoD {
					x: utils::fixed16_to_f32(def.u.td.x_value) as f64,
					y: utils::fixed16_to_f32(def.u.td.y_value) as f64,
					spatial: true,
				},
				t if t == ParamType::Point3D as PF_ParamType => Self::ThreeD {
					x: def.u.point3d_d.x_value,
					y: def.u.point3d_d.y_value,
					z: def.u.point3d_d.z_value,
					spatial: true,
				},
				t if t == ParamType::Color as PF_ParamType => {
					let px = def.u.cd.value;
					Self::Color {
						alpha: px.alpha as f64 / 255.0,
						red: px.red as f64 / 255.0,
						green: px.green as f64 / 255.0,
						blue: px.blue as f64 / 255.0,
					}
				}
				_ => Self::NoData,
			}
		}
	}

	fn stream_type(self) -> AEGP_StreamType {
		(match self {
			Self::NoData => AEGP_StreamType_NO_DATA,
			Self::OneD(_) => AEGP_StreamType_OneD,
			Self::TwoD { spatial: true, .. } => AEGP_StreamType_TwoD_SPATIAL,
			Self::TwoD { .. } => AEGP_StreamType_TwoD,
			Self::ThreeD { spatial: true, .. } => AEGP_StreamType_ThreeD_SPATIAL,
			Self::ThreeD { .. } => AEGP_StreamType_ThreeD,
			Self::Color { .. } => AEGP_StreamType_COLOR,
			Self::LayerId => AEGP_StreamType_LAYER_ID,
		}) as AEGP_StreamType
	}

	/// Number of values in one sample (`AEGP_GetStreamValueDimensionality`).
	pub(crate) fn dimensionality(self) -> A_short {
		match self {
			Self::NoData => 0,
			Self::OneD(_) | Self::LayerId => 1,
			Self::TwoD { .. } => 2,
			Self::ThreeD { .. } => 3,
			Self::Color { .. } => 4,
		}
	}

	/// Spatial streams interpolate along a single path, so their temporal
	/// dimensionality is 1 (`AEGP_GetStreamTemporalDimensionality`).
	pub(crate) fn temporal_dimensionality(self) -> A_short {
		match self {
			Self::TwoD { spatial: true, .. } | Self::ThreeD { spatial: true, .. } => 1,
			other => other.dimensionality(),
		}
	}
}

/// Write `value` into an `AEGP_StreamVal` or `AEGP_StreamVal2`, which share
/// these members.
macro_rules! write_val {
	($value:expr, $out:expr) => {{
		let out = $out;
		match $value {
			StreamValue::NoData => {}
			StreamValue::OneD(v) => out.one_d = v,
			StreamValue::TwoD { x, y, .. } => out.two_d = AEGP_TwoDVal { x, y },
			StreamValue::ThreeD { x, y, z, .. } => out.three_d = AEGP_ThreeDVal { x, y, z },
			StreamValue::Color {
				alpha,
				red,
				green,
				blue,
			} => {
				out.color = AEGP_ColorVal {
					alphaF: alpha,
					redF: red,
					greenF: green,
					blueF: blue,
				}
			}
			StreamValue::LayerId => out.layer_id = 0,
		}
	}};
}

/// The current value of the parameter behind `streamH`.
///
/// # Safety
/// `streamH` must satisfy [`stream_of`]'s contract.
pub(crate) unsafe fn stream_value(streamH: AEGP_StreamRefH) -> Option<StreamValue> {
	let stream = unsafe { stream_of(streamH) }?;
	with_param(stream.effect_ref, stream.index, StreamValue::of)
}

fn unsupported(name: &str) -> A_Err {
	log::warn!("AEGP_StreamSuite/{name}: aexlo only serves read-only effect parameter streams");
	A_Err_GENERIC as A_Err
}

unsafe extern "C" fn is_stream_legal(
	_layerH: AEGP_LayerH,
	_which_stream: AEGP_LayerStream,
	_is_legalP: *mut A_Boolean,
) -> A_Err {
	diag!("AEGP_StreamSuite/AEGP_IsStreamLegal");
	unsupported("AEGP_IsStreamLegal")
}

unsafe extern "C" fn can_vary_over_time(streamH: AEGP_StreamRefH, can_varyPB: *mut A_Boolean) -> A_Err {
	diag!("AEGP_StreamSuite/AEGP_CanVaryOverTime", "streamH" => format!("{:#x}", streamH as usize));

	let Some(value) = (unsafe { stream_value(streamH) }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	let Some(out) = (unsafe { can_varyPB.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	*out = (value != StreamValue::NoData) as A_Boolean;
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn get_valid_interpolations(
	streamH: AEGP_StreamRefH,
	valid_interpolationsP: *mut AEGP_KeyInterpolationMask,
) -> A_Err {
	diag!("AEGP_StreamSuite/AEGP_GetValidInterpolations", "streamH" => format!("{:#x}", streamH as usize));

	let Some(value) = (unsafe { stream_value(streamH) }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	let Some(out) = (unsafe { valid_interpolationsP.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	*out = match value {
		StreamValue::NoData => AEGP_KeyInterpMask_NONE,
		StreamValue::LayerId => AEGP_KeyInterpMask_HOLD,
		_ => AEGP_KeyInterpMask_LINEAR | AEGP_KeyInterpMask_BEZIER | AEGP_KeyInterpMask_HOLD,
	} as AEGP_KeyInterpolationMask;
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn get_new_layer_stream(
	_aegp_plugin_id: AEGP_PluginID,
	_layerH: AEGP_LayerH,
	_which_stream: AEGP_LayerStream,
	_streamPH: *mut AEGP_StreamRefH,
) -> A_Err {
	diag!("AEGP_StreamSuite/AEGP_GetNewLayerStream");
	unsupported("AEGP_GetNewLayerStream")
}

unsafe extern "C" fn get_effect_num_param_streams(effect_refH: AEGP_EffectRefH, num_paramsPL: *mut A_long) -> A_Err {
	diag!("AEGP_StreamSuite/AEGP_GetEffectNumParamStreams",
		"effect_refH" => format!("{:#x}", effect_refH as usize),
	);

	let Some(effect_ref) = (unsafe { effect_ref_of(effect_refH) }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	let Some(instance) = crate::PluginInstance::get_instance_ptr(effect_ref) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	let Some(out) = (unsafe { num_paramsPL.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	// SAFETY: effect_ref is the live instance that is running the plugin call.
	*out = unsafe { instance.as_ref() }.params().len() as A_long;
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn get_new_effect_stream_by_index(
	_aegp_plugin_id: AEGP_PluginID,
	effect_refH: AEGP_EffectRefH,
	param_index: PF_ParamIndex,
	streamPH: *mut AEGP_StreamRefH,
) -> A_Err {
	diag!("AEGP_StreamSuite/AEGP_GetNewEffectStreamByIndex",
		"effect_refH" => format!("{:#x}", effect_refH as usize),
		"param_index" => param_index,
	);

	let Some(effect_ref) = (unsafe { effect_ref_of(effect_refH) }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	let Some(out) = (unsafe { streamPH.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	let Ok(index) = usize::try_from(param_index) else {
		return PF_Err_INVALID_INDEX as A_Err;
	};
	if with_param(effect_ref, index, |_| ()).is_none() {
		return PF_Err_INVALID_INDEX as A_Err;
	}
	*out = new_stream_handle(Stream { effect_ref, index });
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn get_new_mask_stream(
	_aegp_plugin_id: AEGP_PluginID,
	_mask_refH: AEGP_MaskRefH,
	_which_stream: AEGP_MaskStream,
	_mask_streamPH: *mut AEGP_StreamRefH,
) -> A_Err {
	diag!("AEGP_StreamSuite/AEGP_GetNewMaskStream");
	unsupported("AEGP_GetNewMaskStream")
}

unsafe extern "C" fn dispose_stream(streamH: AEGP_StreamRefH) -> A_Err {
	diag!("AEGP_StreamSuite/AEGP_DisposeStream", "streamH" => format!("{:#x}", streamH as usize));

	if streamH.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	}
	// SAFETY: non-null stream handles only come from `new_stream_handle`.
	drop(unsafe { Box::from_raw(streamH as *mut Stream) });
	PF_Err_NONE as A_Err
}

/// The v2 form writes the parameter's name into a fixed-size buffer.
unsafe extern "C" fn get_stream_name_v1(
	streamH: AEGP_StreamRefH,
	_force_englishB: A_Boolean,
	nameZ: *mut A_char,
) -> A_Err {
	diag!("AEGP_StreamSuite/AEGP_GetStreamName", "streamH" => format!("{:#x}", streamH as usize));

	let Some(stream) = (unsafe { stream_of(streamH) }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	if nameZ.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	}
	let Some(name) = with_param(stream.effect_ref, stream.index, |def| def.name_do_not_use_directly) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	let max = AEGP_MAX_STREAM_NAME_SIZE as usize - 1;
	let len = name
		.iter()
		.take(max)
		.position(|&c| c == 0)
		.unwrap_or(max.min(name.len()));
	unsafe {
		std::ptr::copy_nonoverlapping(name.as_ptr(), nameZ, len);
		*nameZ.add(len) = 0;
	}
	PF_Err_NONE as A_Err
}

/// The v4 form returns a `AEGP_MemHandle`, which needs the AEGP Memory Suite.
unsafe extern "C" fn get_stream_name_v2(
	_pluginID: AEGP_PluginID,
	_streamH: AEGP_StreamRefH,
	_force_englishB: A_Boolean,
	_utf_stream_namePH: *mut AEGP_MemHandle,
) -> A_Err {
	diag!("AEGP_StreamSuite/AEGP_GetStreamName");
	unsupported("AEGP_GetStreamName")
}

/// Effect parameters carry no units text.
unsafe extern "C" fn get_stream_units_text(
	streamH: AEGP_StreamRefH,
	_force_englishB: A_Boolean,
	unitsZ: *mut A_char,
) -> A_Err {
	diag!("AEGP_StreamSuite/AEGP_GetStreamUnitsText", "streamH" => format!("{:#x}", streamH as usize));

	if unsafe { stream_of(streamH) }.is_none() || unitsZ.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	}
	unsafe { *unitsZ = 0 };
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn get_stream_properties(
	streamH: AEGP_StreamRefH,
	flagsP: *mut AEGP_StreamFlags,
	minP0: *mut A_FpLong,
	maxP0: *mut A_FpLong,
) -> A_Err {
	diag!("AEGP_StreamSuite/AEGP_GetStreamProperties", "streamH" => format!("{:#x}", streamH as usize));

	let Some(stream) = (unsafe { stream_of(streamH) }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	let Some((value, range)) = with_param(stream.effect_ref, stream.index, |def| {
		// SAFETY: every union read is selected by `param_type`.
		let range = unsafe {
			match def.param_type {
				t if t == ParamType::Slider as PF_ParamType => {
					Some((def.u.sd.valid_min as f64, def.u.sd.valid_max as f64))
				}
				t if t == ParamType::FixSlider as PF_ParamType => Some((
					utils::fixed16_to_f32(def.u.fd.valid_min) as f64,
					utils::fixed16_to_f32(def.u.fd.valid_max) as f64,
				)),
				t if t == ParamType::FloatSlider as PF_ParamType => {
					Some((def.u.fs_d.valid_min as f64, def.u.fs_d.valid_max as f64))
				}
				_ => None,
			}
		};
		(StreamValue::of(def), range)
	}) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};

	let mut flags = AEGP_StreamFlag_NONE;
	if matches!(
		value,
		StreamValue::TwoD { spatial: true, .. } | StreamValue::ThreeD { spatial: true, .. }
	) {
		flags |= AEGP_StreamFlag_IS_SPATIAL;
	}
	if let Some((min, max)) = range {
		flags |= AEGP_StreamFlag_HAS_MIN | AEGP_StreamFlag_HAS_MAX;
		unsafe {
			if let Some(out) = minP0.as_mut() {
				*out = min;
			}
			if let Some(out) = maxP0.as_mut() {
				*out = max;
			}
		}
	}
	let Some(out) = (unsafe { flagsP.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	*out = flags as AEGP_StreamFlags;
	PF_Err_NONE as A_Err
}

/// Nothing is keyframed or expression-driven in aexlo.
unsafe extern "C" fn is_stream_timevarying(streamH: AEGP_StreamRefH, is_timevaryingPB: *mut A_Boolean) -> A_Err {
	diag!("AEGP_StreamSuite/AEGP_IsStreamTimevarying", "streamH" => format!("{:#x}", streamH as usize));

	if unsafe { stream_of(streamH) }.is_none() {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	}
	let Some(out) = (unsafe { is_timevaryingPB.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	*out = 0;
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn get_stream_type(streamH: AEGP_StreamRefH, stream_typeP: *mut AEGP_StreamType) -> A_Err {
	diag!("AEGP_StreamSuite/AEGP_GetStreamType", "streamH" => format!("{:#x}", streamH as usize));

	let Some(value) = (unsafe { stream_value(streamH) }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	let Some(out) = (unsafe { stream_typeP.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	*out = value.stream_type();
	PF_Err_NONE as A_Err
}

/// The stream's value at any time: the parameter's current value.
macro_rules! get_new_stream_value {
	($name:ident, $value_ty:ty) => {
		unsafe extern "C" fn $name(
			_aegp_plugin_id: AEGP_PluginID,
			streamH: AEGP_StreamRefH,
			_time_mode: AEGP_LTimeMode,
			_timePT: *const A_Time,
			_pre_expressionB: A_Boolean,
			valueP: *mut $value_ty,
		) -> A_Err {
			diag!("AEGP_StreamSuite/AEGP_GetNewStreamValue", "streamH" => format!("{:#x}", streamH as usize));

			let Some(value) = (unsafe { stream_value(streamH) }) else {
				return PF_Err_BAD_CALLBACK_PARAM as A_Err;
			};
			if value == StreamValue::NoData {
				return unsupported("AEGP_GetNewStreamValue (stream has no data)");
			}
			let Some(out) = (unsafe { valueP.as_mut() }) else {
				return PF_Err_BAD_CALLBACK_PARAM as A_Err;
			};
			// SAFETY: every member of the value union is plain old data.
			*out = unsafe { std::mem::zeroed() };
			out.streamH = streamH;
			write_val!(value, &mut out.val);
			PF_Err_NONE as A_Err
		}
	};
}

get_new_stream_value!(get_new_stream_value_v1, AEGP_StreamValue);
get_new_stream_value!(get_new_stream_value_v2, AEGP_StreamValue2);

/// Stream values never own memory here (no arb, marker, mask or text data).
macro_rules! dispose_stream_value {
	($name:ident, $value_ty:ty) => {
		unsafe extern "C" fn $name(valueP: *mut $value_ty) -> A_Err {
			diag!("AEGP_StreamSuite/AEGP_DisposeStreamValue", "valueP" => format!("{:#x}", valueP as usize));

			if valueP.is_null() {
				return PF_Err_BAD_CALLBACK_PARAM as A_Err;
			}
			PF_Err_NONE as A_Err
		}
	};
}

dispose_stream_value!(dispose_stream_value_v1, AEGP_StreamValue);
dispose_stream_value!(dispose_stream_value_v2, AEGP_StreamValue2);

macro_rules! set_stream_value {
	($name:ident, $value_ty:ty) => {
		unsafe extern "C" fn $name(
			_aegp_plugin_id: AEGP_PluginID,
			_streamH: AEGP_StreamRefH,
			_valueP: *mut $value_ty,
		) -> A_Err {
			diag!("AEGP_StreamSuite/AEGP_SetStreamValue");
			unsupported("AEGP_SetStreamValue")
		}
	};
}

set_stream_value!(set_stream_value_v1, AEGP_StreamValue);
set_stream_value!(set_stream_value_v2, AEGP_StreamValue2);

macro_rules! get_layer_stream_value {
	($name:ident, $val_ty:ty) => {
		unsafe extern "C" fn $name(
			_layerH: AEGP_LayerH,
			_which_stream: AEGP_LayerStream,
			_time_mode: AEGP_LTimeMode,
			_timePT: *const A_Time,
			_pre_expressionB: A_Boolean,
			_stream_valP: *mut $val_ty,
			_stream_typeP0: *mut AEGP_StreamType,
		) -> A_Err {
			diag!("AEGP_StreamSuite/AEGP_GetLayerStreamValue");
			unsupported("AEGP_GetLayerStreamValue")
		}
	};
}

get_layer_stream_value!(get_layer_stream_value_v1, AEGP_StreamVal);
get_layer_stream_value!(get_layer_stream_value_v2, AEGP_StreamVal2);

unsafe extern "C" fn get_expression_state(
	_aegp_plugin_id: AEGP_PluginID,
	streamH: AEGP_StreamRefH,
	enabledPB: *mut A_Boolean,
) -> A_Err {
	diag!("AEGP_StreamSuite/AEGP_GetExpressionState", "streamH" => format!("{:#x}", streamH as usize));

	if unsafe { stream_of(streamH) }.is_none() {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	}
	let Some(out) = (unsafe { enabledPB.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	*out = 0;
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn set_expression_state(
	_aegp_plugin_id: AEGP_PluginID,
	_streamH: AEGP_StreamRefH,
	_enabledB: A_Boolean,
) -> A_Err {
	diag!("AEGP_StreamSuite/AEGP_SetExpressionState");
	unsupported("AEGP_SetExpressionState")
}

unsafe extern "C" fn get_expression(
	_aegp_plugin_id: AEGP_PluginID,
	_streamH: AEGP_StreamRefH,
	_expressionHZ: *mut AEGP_MemHandle,
) -> A_Err {
	diag!("AEGP_StreamSuite/AEGP_GetExpression");
	unsupported("AEGP_GetExpression")
}

unsafe extern "C" fn set_expression(
	_aegp_plugin_id: AEGP_PluginID,
	_streamH: AEGP_StreamRefH,
	_expressionP: *const A_char,
) -> A_Err {
	diag!("AEGP_StreamSuite/AEGP_SetExpression");
	unsupported("AEGP_SetExpression")
}

unsafe extern "C" fn duplicate_stream_ref(
	_aegp_plugin_id: AEGP_PluginID,
	streamH: AEGP_StreamRefH,
	dup_streamPH: *mut AEGP_StreamRefH,
) -> A_Err {
	diag!("AEGP_StreamSuite/AEGP_DuplicateStreamRef", "streamH" => format!("{:#x}", streamH as usize));

	let Some(stream) = (unsafe { stream_of(streamH) }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	let Some(out) = (unsafe { dup_streamPH.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	*out = new_stream_handle(stream);
	PF_Err_NONE as A_Err
}

/// Builds the `AEGP_StreamSuite2` vtable (wire version 7).
pub(super) const fn create_stream_suite_2() -> AEGP_StreamSuite2 {
	AEGP_StreamSuite2 {
		AEGP_IsStreamLegal: Some(is_stream_legal),
		AEGP_CanVaryOverTime: Some(can_vary_over_time),
		AEGP_GetValidInterpolations: Some(get_valid_interpolations),
		AEGP_GetNewLayerStream: Some(get_new_layer_stream),
		AEGP_GetEffectNumParamStreams: Some(get_effect_num_param_streams),
		AEGP_GetNewEffectStreamByIndex: Some(get_new_effect_stream_by_index),
		AEGP_GetNewMaskStream: Some(get_new_mask_stream),
		AEGP_DisposeStream: Some(dispose_stream),
		AEGP_GetStreamName: Some(get_stream_name_v1),
		AEGP_GetStreamUnitsText: Some(get_stream_units_text),
		AEGP_GetStreamProperties: Some(get_stream_properties),
		AEGP_IsStreamTimevarying: Some(is_stream_timevarying),
		AEGP_GetStreamType: Some(get_stream_type),
		AEGP_GetNewStreamValue: Some(get_new_stream_value_v1),
		AEGP_DisposeStreamValue: Some(dispose_stream_value_v1),
		AEGP_SetStreamValue: Some(set_stream_value_v1),
		AEGP_GetLayerStreamValue: Some(get_layer_stream_value_v1),
		AEGP_GetExpressionState: Some(get_expression_state),
		AEGP_SetExpressionState: Some(set_expression_state),
		AEGP_GetExpression: Some(get_expression),
		AEGP_SetExpression: Some(set_expression),
		AEGP_DuplicateStreamRef: Some(duplicate_stream_ref),
	}
}

/// Builds the `AEGP_StreamSuite4` vtable (wire version 9).
pub(super) const fn create_stream_suite_4() -> AEGP_StreamSuite4 {
	AEGP_StreamSuite4 {
		AEGP_IsStreamLegal: Some(is_stream_legal),
		AEGP_CanVaryOverTime: Some(can_vary_over_time),
		AEGP_GetValidInterpolations: Some(get_valid_interpolations),
		AEGP_GetNewLayerStream: Some(get_new_layer_stream),
		AEGP_GetEffectNumParamStreams: Some(get_effect_num_param_streams),
		AEGP_GetNewEffectStreamByIndex: Some(get_new_effect_stream_by_index),
		AEGP_GetNewMaskStream: Some(get_new_mask_stream),
		AEGP_DisposeStream: Some(dispose_stream),
		AEGP_GetStreamName: Some(get_stream_name_v2),
		AEGP_GetStreamUnitsText: Some(get_stream_units_text),
		AEGP_GetStreamProperties: Some(get_stream_properties),
		AEGP_IsStreamTimevarying: Some(is_stream_timevarying),
		AEGP_GetStreamType: Some(get_stream_type),
		AEGP_GetNewStreamValue: Some(get_new_stream_value_v2),
		AEGP_DisposeStreamValue: Some(dispose_stream_value_v2),
		AEGP_SetStreamValue: Some(set_stream_value_v2),
		AEGP_GetLayerStreamValue: Some(get_layer_stream_value_v2),
		AEGP_GetExpressionState: Some(get_expression_state),
		AEGP_SetExpressionState: Some(set_expression_state),
		AEGP_GetExpression: Some(get_expression),
		AEGP_SetExpression: Some(set_expression),
		AEGP_DuplicateStreamRef: Some(duplicate_stream_ref),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn values_map_to_stream_types() {
		assert_eq!(
			StreamValue::NoData.stream_type(),
			AEGP_StreamType_NO_DATA as AEGP_StreamType
		);
		assert_eq!(
			StreamValue::OneD(1.0).stream_type(),
			AEGP_StreamType_OneD as AEGP_StreamType
		);
		let point = StreamValue::TwoD {
			x: 1.0,
			y: 2.0,
			spatial: true,
		};
		assert_eq!(point.stream_type(), AEGP_StreamType_TwoD_SPATIAL as AEGP_StreamType);
		assert_eq!((point.dimensionality(), point.temporal_dimensionality()), (2, 1));
	}

	#[test]
	fn stream_values_fill_both_union_versions() {
		let mut v1: AEGP_StreamVal = unsafe { std::mem::zeroed() };
		let mut v2: AEGP_StreamVal2 = unsafe { std::mem::zeroed() };
		let point = StreamValue::TwoD {
			x: 3.0,
			y: 4.0,
			spatial: true,
		};
		write_val!(point, &mut v1);
		write_val!(point, &mut v2);
		unsafe {
			assert_eq!((v1.two_d.x, v1.two_d.y), (3.0, 4.0));
			assert_eq!((v2.two_d.x, v2.two_d.y), (3.0, 4.0));
		}
	}

	#[test]
	fn handles_duplicate_and_dispose() {
		let suite = create_stream_suite_4();
		let handle = new_stream_handle(Stream {
			effect_ref: 0x10 as PF_ProgPtr,
			index: 3,
		});
		let mut dup: AEGP_StreamRefH = std::ptr::null_mut();
		unsafe {
			assert_eq!(
				suite.AEGP_DuplicateStreamRef.unwrap()(0, handle, &mut dup),
				PF_Err_NONE as A_Err
			);
			assert_eq!(stream_of(dup).map(|s| s.index), Some(3));
			assert_eq!(suite.AEGP_DisposeStream.unwrap()(dup), PF_Err_NONE as A_Err);
			assert_eq!(suite.AEGP_DisposeStream.unwrap()(handle), PF_Err_NONE as A_Err);
			assert_ne!(
				suite.AEGP_DisposeStream.unwrap()(std::ptr::null_mut()),
				PF_Err_NONE as A_Err
			);
		}
	}
}
