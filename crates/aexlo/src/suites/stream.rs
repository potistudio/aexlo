//! `AEGP Stream Suite`, served at wire versions 7 (`AEGP_StreamSuite2`), 8
//! (`AEGP_StreamSuite3`), 9 (`AEGP_StreamSuite4`), 10 (`AEGP_StreamSuite5`) and
//! 11 (`AEGP_StreamSuite6`), plus the `AEGP_StreamRefH` handles it hands out. Version 8 keeps version 7's
//! char names and uses version 9's `AEGP_StreamValue2`.
//!
//! A stream handle is the effect's `PF_ProgPtr`, a node in the layer tree and,
//! for a parameter, that parameter's index (the same index space as
//! `PF_ParamDef`s, 0 being the input layer). aexlo has no timeline, so
//! a stream's value is the parameter's current value at every time and it never
//! has keyframes or expressions. `AEGP_SetStreamValue` writes a parameter's
//! value, as a plugin's Reset button does. Mask streams, names that need the
//! AEGP Memory Suite and every other write are refused with `A_Err_GENERIC`.
//!
//! The same handle also names a node in the layer's dynamic-stream tree
//! ([`super::dynamic_stream`]). A parameter node is what the calls above read.
//! The layer, its transform and the effect group have no keyframes; the
//! transform leaves report an identity 2D transform.
//!
//! The table versions differ only in their value structs (`AEGP_StreamValue`
//! vs `AEGP_StreamValue2`, whose unions share the members written here) and,
//! from v5, a UTF-16 `AEGP_SetExpression`. v6 appends `AEGP_GetUniqueStreamID`.

use crate::sys::*;

use crate::core::diagnostics::diag;
use crate::suites::effect::{effect_ref_of, with_param};
use crate::utils;

/// Where an `AEGP_StreamRefH` sits in the layer's stream tree.
///
/// Parameter nodes are the effect's `PF_ParamDef`s. The other nodes are the
/// fixed groups around that effect: one 2D AV layer, its transform, an effect
/// parade of one effect, and an empty mask parade.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Node {
	/// `ADBE AV Layer`, the layer root.
	Layer,
	/// `ADBE Transform Group`.
	Transform,
	/// `ADBE Anchor Point`.
	Anchor,
	/// `ADBE Position`.
	Position,
	/// `ADBE Scale`.
	Scale,
	/// `ADBE Rotate Z`.
	Rotation,
	/// `ADBE Opacity`.
	Opacity,
	/// `ADBE Effect Parade`.
	Effects,
	/// The one hosted effect, a named group of its parameters.
	Effect,
	/// `ADBE Mask Parade`. Always empty.
	Masks,
	/// A parameter of the hosted effect. `index` is the parameter index.
	Param,
}

/// What an `AEGP_StreamRefH` points at.
#[derive(Clone, Copy)]
pub(crate) struct Stream {
	pub(crate) effect_ref: PF_ProgPtr,
	/// Index of this node in its parent. For [`Node::Param`], the parameter index.
	pub(crate) index: usize,
	pub(crate) node: Node,
}

/// Whether the instance behind `effect_ref` has built its arbitrary-data
/// values. Until then arbitrary streams stay `NO_DATA`: reporting `ARB`
/// while `PF_Cmd_PARAMS_SETUP` is still on the stack makes Particular throw
/// out of its entry point. Kept per instance, so another instance's setup
/// (or its failure) on the same thread does not change the answer.
fn arb_streams_live(effect_ref: PF_ProgPtr) -> bool {
	crate::PluginInstance::get_instance_ptr(effect_ref)
		// SAFETY: effect_ref is the live instance that is running the plugin call.
		.is_some_and(|instance| unsafe { instance.as_ref() }.arb_streams_live())
}

pub(crate) fn new_stream_handle(stream: Stream) -> AEGP_StreamRefH {
	Box::into_raw(Box::new(stream)) as AEGP_StreamRefH
}

/// # Safety
/// `streamH` must be null or a handle from [`new_stream_handle`] that has not
/// been disposed.
pub(crate) unsafe fn stream_of(streamH: AEGP_StreamRefH) -> Option<Stream> {
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
	/// An arbitrary-data parameter. The handle is the parameter's own value,
	/// not a copy, so disposing the stream value must not free it.
	Arb(PF_ArbitraryH),
}

impl StreamValue {
	fn of(def: &PF_ParamDef, arb_live: bool) -> Self {
		// SAFETY: every union read is selected by `param_type`.
		unsafe {
			match def.param_type {
				t if t == PF_Param_LAYER as PF_ParamType => Self::LayerId,
				t if t == PF_Param_SLIDER as PF_ParamType => Self::OneD(def.u.sd.value as f64),
				t if t == PF_Param_FIX_SLIDER as PF_ParamType => {
					Self::OneD(utils::fixed16_to_f32(def.u.fd.value) as f64)
				}
				t if t == PF_Param_FLOAT_SLIDER as PF_ParamType => Self::OneD(def.u.fs_d.value),
				t if t == PF_Param_ANGLE as PF_ParamType => Self::OneD(utils::fixed16_to_f32(def.u.ad.value) as f64),
				t if t == PF_Param_CHECKBOX as PF_ParamType => Self::OneD(def.u.bd.value as f64),
				t if t == PF_Param_POPUP as PF_ParamType => Self::OneD(def.u.pd.value as f64),
				t if t == PF_Param_POINT as PF_ParamType => Self::TwoD {
					x: utils::fixed16_to_f32(def.u.td.x_value) as f64,
					y: utils::fixed16_to_f32(def.u.td.y_value) as f64,
					spatial: true,
				},
				t if t == PF_Param_POINT_3D as PF_ParamType => Self::ThreeD {
					x: def.u.point3d_d.x_value,
					y: def.u.point3d_d.y_value,
					z: def.u.point3d_d.z_value,
					spatial: true,
				},
				t if t == PF_Param_COLOR as PF_ParamType => {
					let px = def.u.cd.value;
					Self::Color {
						alpha: px.alpha as f64 / 255.0,
						red: px.red as f64 / 255.0,
						green: px.green as f64 / 255.0,
						blue: px.blue as f64 / 255.0,
					}
				}
				t if t == PF_Param_ARBITRARY_DATA as PF_ParamType => {
					let handle = def.u.arb_d.value;
					if handle.is_null() || !arb_live {
						Self::NoData
					} else {
						Self::Arb(handle)
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
			Self::Arb(_) => AEGP_StreamType_ARB,
		}) as AEGP_StreamType
	}

	/// Number of values in one sample (`AEGP_GetStreamValueDimensionality`).
	pub(crate) fn dimensionality(self) -> A_short {
		match self {
			Self::NoData => 0,
			Self::OneD(_) | Self::LayerId | Self::Arb(_) => 1,
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
			// `arbH` is bound as `A_Handle` (`*mut c_void`); the parameter stores
			// a `PF_Handle` (`*mut *mut c_void`). The bit pattern is what the
			// plugin casts back.
			StreamValue::Arb(handle) => out.arbH = handle.cast(),
		}
	}};
}

/// The current value of the parameter behind `streamH`.
///
/// # Safety
/// `streamH` must satisfy [`stream_of`]'s contract.
pub(crate) unsafe fn stream_value(streamH: AEGP_StreamRefH) -> Option<StreamValue> {
	let stream = unsafe { stream_of(streamH) }?;
	match stream.node {
		Node::Param => {
			let arb_live = arb_streams_live(stream.effect_ref);
			with_param(stream.effect_ref, stream.index, |def| StreamValue::of(def, arb_live))
		}
		// Identity 2D transform: the anchor and the position sit on the layer
		// centre, scale is 100%, rotation is 0 and opacity is fully on.
		Node::Anchor | Node::Position => {
			let timeline = super::layer::Timeline::of(stream.effect_ref as *mut std::ffi::c_void)?;
			Some(StreamValue::TwoD {
				x: timeline.width as f64 / 2.0,
				y: timeline.height as f64 / 2.0,
				spatial: true,
			})
		}
		Node::Scale => Some(StreamValue::TwoD {
			x: 100.0,
			y: 100.0,
			spatial: false,
		}),
		Node::Rotation => Some(StreamValue::OneD(0.0)),
		Node::Opacity => Some(StreamValue::OneD(100.0)),
		_ => Some(StreamValue::NoData),
	}
}

fn unsupported(name: &str) -> A_Err {
	log::warn!("AEGP_StreamSuite/{name}: aexlo only serves read-only effect parameter streams");
	A_Err_GENERIC
}

unsafe extern "C" fn is_stream_legal(
	_layerH: AEGP_LayerH,
	_which_stream: AEGP_LayerStream,
	_is_legalP: *mut A_Boolean,
) -> A_Err {
	crate::core::ffi::guard("is_stream_legal", || {
		diag!("AEGP_StreamSuite/AEGP_IsStreamLegal");
		unsupported("AEGP_IsStreamLegal")
	})
}

unsafe extern "C" fn can_vary_over_time(streamH: AEGP_StreamRefH, can_varyPB: *mut A_Boolean) -> A_Err {
	crate::core::ffi::guard("can_vary_over_time", || {
		diag!("AEGP_StreamSuite/AEGP_CanVaryOverTime", "streamH" => format!("{:#x}", streamH as usize));

		let Some(value) = (unsafe { stream_value(streamH) }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		let Some(out) = (unsafe { can_varyPB.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		*out = (value != StreamValue::NoData) as A_Boolean;
		PF_Err_NONE
	})
}

unsafe extern "C" fn get_valid_interpolations(
	streamH: AEGP_StreamRefH,
	valid_interpolationsP: *mut AEGP_KeyInterpolationMask,
) -> A_Err {
	crate::core::ffi::guard("get_valid_interpolations", || {
		diag!("AEGP_StreamSuite/AEGP_GetValidInterpolations", "streamH" => format!("{:#x}", streamH as usize));

		let Some(value) = (unsafe { stream_value(streamH) }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		let Some(out) = (unsafe { valid_interpolationsP.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		*out = match value {
			StreamValue::NoData => AEGP_KeyInterpMask_NONE,
			StreamValue::LayerId | StreamValue::Arb(_) => AEGP_KeyInterpMask_HOLD,
			_ => AEGP_KeyInterpMask_LINEAR | AEGP_KeyInterpMask_BEZIER | AEGP_KeyInterpMask_HOLD,
		} as AEGP_KeyInterpolationMask;
		PF_Err_NONE
	})
}

unsafe extern "C" fn get_new_layer_stream(
	_aegp_plugin_id: AEGP_PluginID,
	_layerH: AEGP_LayerH,
	_which_stream: AEGP_LayerStream,
	_streamPH: *mut AEGP_StreamRefH,
) -> A_Err {
	crate::core::ffi::guard("get_new_layer_stream", || {
		diag!("AEGP_StreamSuite/AEGP_GetNewLayerStream");
		unsupported("AEGP_GetNewLayerStream")
	})
}

unsafe extern "C" fn get_effect_num_param_streams(effect_refH: AEGP_EffectRefH, num_paramsPL: *mut A_long) -> A_Err {
	crate::core::ffi::guard("get_effect_num_param_streams", || {
		diag!("AEGP_StreamSuite/AEGP_GetEffectNumParamStreams",
			"effect_refH" => format!("{:#x}", effect_refH as usize),
		);

		let Some(effect_ref) = (unsafe { effect_ref_of(effect_refH) }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		let Some(instance) = crate::PluginInstance::get_instance_ptr(effect_ref) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		let Some(out) = (unsafe { num_paramsPL.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		// SAFETY: effect_ref is the live instance that is running the plugin call.
		*out = unsafe { instance.as_ref() }.params().len() as A_long;
		PF_Err_NONE
	})
}

unsafe extern "C" fn get_new_effect_stream_by_index(
	_aegp_plugin_id: AEGP_PluginID,
	effect_refH: AEGP_EffectRefH,
	param_index: PF_ParamIndex,
	streamPH: *mut AEGP_StreamRefH,
) -> A_Err {
	crate::core::ffi::guard("get_new_effect_stream_by_index", || {
		diag!("AEGP_StreamSuite/AEGP_GetNewEffectStreamByIndex",
			"effect_refH" => format!("{:#x}", effect_refH as usize),
			"param_index" => param_index,
		);

		let Some(effect_ref) = (unsafe { effect_ref_of(effect_refH) }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		let Some(out) = (unsafe { streamPH.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		let Ok(index) = usize::try_from(param_index) else {
			return PF_Err_INVALID_INDEX;
		};
		if with_param(effect_ref, index, |_| ()).is_none() {
			return PF_Err_INVALID_INDEX;
		}
		*out = new_stream_handle(Stream {
			effect_ref,
			index,
			node: Node::Param,
		});
		PF_Err_NONE
	})
}

unsafe extern "C" fn get_new_mask_stream(
	_aegp_plugin_id: AEGP_PluginID,
	_mask_refH: AEGP_MaskRefH,
	_which_stream: AEGP_MaskStream,
	_mask_streamPH: *mut AEGP_StreamRefH,
) -> A_Err {
	crate::core::ffi::guard("get_new_mask_stream", || {
		diag!("AEGP_StreamSuite/AEGP_GetNewMaskStream");
		unsupported("AEGP_GetNewMaskStream")
	})
}

unsafe extern "C" fn dispose_stream(streamH: AEGP_StreamRefH) -> A_Err {
	crate::core::ffi::guard("dispose_stream", || {
		diag!("AEGP_StreamSuite/AEGP_DisposeStream", "streamH" => format!("{:#x}", streamH as usize));

		if streamH.is_null() {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		// SAFETY: non-null stream handles only come from `new_stream_handle`.
		drop(unsafe { Box::from_raw(streamH as *mut Stream) });
		PF_Err_NONE
	})
}

/// The v2 form writes the parameter's name into a fixed-size buffer.
unsafe extern "C" fn get_stream_name_v1(
	streamH: AEGP_StreamRefH,
	_force_englishB: A_Boolean,
	nameZ: *mut A_char,
) -> A_Err {
	crate::core::ffi::guard("get_stream_name_v1", || {
		diag!("AEGP_StreamSuite/AEGP_GetStreamName", "streamH" => format!("{:#x}", streamH as usize));

		let Some(stream) = (unsafe { stream_of(streamH) }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		if nameZ.is_null() {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		if stream.node != Node::Param {
			return super::dynamic_stream::write_name(stream, nameZ, AEGP_MAX_STREAM_NAME_SIZE as usize);
		}
		let Some(name) = with_param(stream.effect_ref, stream.index, |def| def.name_do_not_use_directly) else {
			return PF_Err_BAD_CALLBACK_PARAM;
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
		PF_Err_NONE
	})
}

/// The v4 form returns a `AEGP_MemHandle`, which needs the AEGP Memory Suite.
unsafe extern "C" fn get_stream_name_v2(
	_pluginID: AEGP_PluginID,
	_streamH: AEGP_StreamRefH,
	_force_englishB: A_Boolean,
	_utf_stream_namePH: *mut AEGP_MemHandle,
) -> A_Err {
	crate::core::ffi::guard("get_stream_name_v2", || {
		diag!("AEGP_StreamSuite/AEGP_GetStreamName");
		unsupported("AEGP_GetStreamName")
	})
}

/// Effect parameters carry no units text.
unsafe extern "C" fn get_stream_units_text(
	streamH: AEGP_StreamRefH,
	_force_englishB: A_Boolean,
	unitsZ: *mut A_char,
) -> A_Err {
	crate::core::ffi::guard("get_stream_units_text", || {
		diag!("AEGP_StreamSuite/AEGP_GetStreamUnitsText", "streamH" => format!("{:#x}", streamH as usize));

		if unsafe { stream_of(streamH) }.is_none() || unitsZ.is_null() {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		unsafe { *unitsZ = 0 };
		PF_Err_NONE
	})
}

unsafe extern "C" fn get_stream_properties(
	streamH: AEGP_StreamRefH,
	flagsP: *mut AEGP_StreamFlags,
	minP0: *mut A_FpLong,
	maxP0: *mut A_FpLong,
) -> A_Err {
	crate::core::ffi::guard("get_stream_properties", || {
		diag!("AEGP_StreamSuite/AEGP_GetStreamProperties", "streamH" => format!("{:#x}", streamH as usize));

		let Some(stream) = (unsafe { stream_of(streamH) }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		if stream.node != Node::Param {
			let Some(out) = (unsafe { flagsP.as_mut() }) else {
				return PF_Err_BAD_CALLBACK_PARAM;
			};
			*out = AEGP_StreamFlag_NONE;
			return PF_Err_NONE;
		}
		let arb_live = arb_streams_live(stream.effect_ref);
		let Some((value, range)) = with_param(stream.effect_ref, stream.index, |def| {
			// SAFETY: every union read is selected by `param_type`.
			let range = unsafe {
				match def.param_type {
					t if t == PF_Param_SLIDER as PF_ParamType => {
						Some((def.u.sd.valid_min as f64, def.u.sd.valid_max as f64))
					}
					t if t == PF_Param_FIX_SLIDER as PF_ParamType => Some((
						utils::fixed16_to_f32(def.u.fd.valid_min) as f64,
						utils::fixed16_to_f32(def.u.fd.valid_max) as f64,
					)),
					t if t == PF_Param_FLOAT_SLIDER as PF_ParamType => {
						Some((def.u.fs_d.valid_min as f64, def.u.fs_d.valid_max as f64))
					}
					_ => None,
				}
			};
			(StreamValue::of(def, arb_live), range)
		}) else {
			return PF_Err_BAD_CALLBACK_PARAM;
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
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		*out = flags as AEGP_StreamFlags;
		PF_Err_NONE
	})
}

/// Nothing is keyframed or expression-driven in aexlo.
unsafe extern "C" fn is_stream_timevarying(streamH: AEGP_StreamRefH, is_timevaryingPB: *mut A_Boolean) -> A_Err {
	crate::core::ffi::guard("is_stream_timevarying", || {
		diag!("AEGP_StreamSuite/AEGP_IsStreamTimevarying", "streamH" => format!("{:#x}", streamH as usize));

		if unsafe { stream_of(streamH) }.is_none() {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		let Some(out) = (unsafe { is_timevaryingPB.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		*out = 0;
		PF_Err_NONE
	})
}

unsafe extern "C" fn get_stream_type(streamH: AEGP_StreamRefH, stream_typeP: *mut AEGP_StreamType) -> A_Err {
	crate::core::ffi::guard("get_stream_type", || {
		diag!("AEGP_StreamSuite/AEGP_GetStreamType", "streamH" => format!("{:#x}", streamH as usize));

		let Some(value) = (unsafe { stream_value(streamH) }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		let Some(out) = (unsafe { stream_typeP.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		if value == StreamValue::NoData
			&& let Some(stream) = (unsafe { stream_of(streamH) })
		{
			let detail = if stream.node == Node::Param {
				with_param(stream.effect_ref, stream.index, |def| def.param_type)
			} else {
				None
			};
			let name = if stream.node == Node::Param {
				with_param(stream.effect_ref, stream.index, crate::host::params::param_name)
			} else {
				None
			};
			log::debug!(
				"stream type NO_DATA node={:?} index={} param_type={detail:?} name={name:?}",
				stream.node,
				stream.index
			);
		}
		*out = value.stream_type();
		PF_Err_NONE
	})
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
				return PF_Err_BAD_CALLBACK_PARAM;
			};
			if value == StreamValue::NoData {
				return unsupported("AEGP_GetNewStreamValue (stream has no data)");
			}
			let Some(out) = (unsafe { valueP.as_mut() }) else {
				return PF_Err_BAD_CALLBACK_PARAM;
			};
			// SAFETY: every member of the value union is plain old data.
			*out = unsafe { std::mem::zeroed() };
			out.streamH = streamH;
			write_val!(value, &mut out.val);
			PF_Err_NONE
		}
	};
}

get_new_stream_value!(get_new_stream_value_v1, AEGP_StreamValue);
get_new_stream_value!(get_new_stream_value_v2, AEGP_StreamValue2);

/// Stream values never own memory here. An arbitrary handle is the parameter's
/// own value, so disposing the stream value must not free it.
macro_rules! dispose_stream_value {
	($name:ident, $value_ty:ty) => {
		unsafe extern "C" fn $name(valueP: *mut $value_ty) -> A_Err {
			diag!("AEGP_StreamSuite/AEGP_DisposeStreamValue", "valueP" => format!("{:#x}", valueP as usize));

			if valueP.is_null() {
				return PF_Err_BAD_CALLBACK_PARAM;
			}
			PF_Err_NONE
		}
	};
}

dispose_stream_value!(dispose_stream_value_v1, AEGP_StreamValue);
dispose_stream_value!(dispose_stream_value_v2, AEGP_StreamValue2);

/// `value` clamped between `a` and `b`, whichever is the larger. A plugin
/// declares the range, so it may be inverted or have a NaN end, where `clamp`
/// would panic; a NaN end does not bound.
fn within<T: PartialOrd + Copy>(value: T, a: T, b: T) -> T {
	let (lo, hi) = if b < a { (b, a) } else { (a, b) };
	if value < lo {
		lo
	} else if value > hi {
		hi
	} else {
		value
	}
}

/// The parameter value `value` sets on the parameter `def`, or `None` when
/// the two do not fit together. Numbers are clamped to the parameter's valid
/// range and colors to `0..=1`, as After Effects does. A color keeps the
/// parameter's alpha: color parameters have no alpha control, and plugins
/// such as Deep Glow write a zero there.
fn param_value_of(def: &PF_ParamDef, value: StreamValue) -> Option<crate::ParamValue> {
	use crate::ParamValue as P;
	let channel = |c: f64| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
	// SAFETY: every union read is selected by `param_type`.
	unsafe {
		Some(match (def.param_type, value) {
			(t, StreamValue::OneD(v)) if t == PF_Param_SLIDER as PF_ParamType => {
				P::Slider(within(v.round() as i32, def.u.sd.valid_min, def.u.sd.valid_max))
			}
			(t, StreamValue::OneD(v)) if t == PF_Param_FIX_SLIDER as PF_ParamType => P::Fixed(within(
				v as f32,
				utils::fixed16_to_f32(def.u.fd.valid_min),
				utils::fixed16_to_f32(def.u.fd.valid_max),
			)),
			(t, StreamValue::OneD(v)) if t == PF_Param_FLOAT_SLIDER as PF_ParamType => {
				P::Float(within(v, def.u.fs_d.valid_min as f64, def.u.fs_d.valid_max as f64))
			}
			(t, StreamValue::OneD(v)) if t == PF_Param_ANGLE as PF_ParamType => P::Angle(v as f32),
			(t, StreamValue::OneD(v)) if t == PF_Param_CHECKBOX as PF_ParamType => P::Checkbox(v != 0.0),
			(t, StreamValue::OneD(v)) if t == PF_Param_POPUP as PF_ParamType => {
				P::Popup((v.round() as i32).clamp(1, def.u.pd.num_choices.max(1) as i32))
			}
			(t, StreamValue::TwoD { x, y, .. }) if t == PF_Param_POINT as PF_ParamType => P::Point {
				x: x as f32,
				y: y as f32,
			},
			(t, StreamValue::ThreeD { x, y, z, .. }) if t == PF_Param_POINT_3D as PF_ParamType => {
				P::Point3D { x, y, z }
			}
			(t, StreamValue::Color { red, green, blue, .. }) if t == PF_Param_COLOR as PF_ParamType => P::Color {
				red: channel(red),
				green: channel(green),
				blue: channel(blue),
				alpha: def.u.cd.value.alpha,
			},
			_ => return None,
		})
	}
}

/// Set the parameter behind `stream` to `value`.
fn write_stream_value(stream: Stream, value: StreamValue) -> A_Err {
	let Some(param) = with_param(stream.effect_ref, stream.index, |def| param_value_of(def, value)) else {
		return PF_Err_BAD_CALLBACK_PARAM;
	};
	let Some(param) = param else {
		return unsupported("AEGP_SetStreamValue (a layer, arbitrary-data or valueless stream)");
	};
	let Some(mut instance) = crate::PluginInstance::get_instance_ptr(stream.effect_ref) else {
		return PF_Err_BAD_CALLBACK_PARAM;
	};
	// SAFETY: effect_ref is the live instance that is running the plugin call,
	// and `set_param` writes the value in place, as the plugin's own
	// `PF_ParamDef`s see it.
	match unsafe { instance.as_mut() }.set_param(stream.index, param) {
		Ok(()) => PF_Err_NONE,
		Err(err) => {
			log::warn!("AEGP_StreamSuite/AEGP_SetStreamValue: {err}");
			PF_Err_BAD_CALLBACK_PARAM
		}
	}
}

/// Set an effect parameter's value. The union member read is the one the
/// stream's type names, as in After Effects.
macro_rules! set_stream_value {
	($name:ident, $value_ty:ty) => {
		unsafe extern "C" fn $name(
			_aegp_plugin_id: AEGP_PluginID,
			streamH: AEGP_StreamRefH,
			valueP: *mut $value_ty,
		) -> A_Err {
			crate::core::ffi::guard("set_stream_value", || {
				let stream = unsafe { stream_of(streamH) };
				let current = unsafe { stream_value(streamH) };
				let value = unsafe { valueP.as_ref() };
				// SAFETY: the member read is the one the stream's type selects.
				let incoming = value.zip(current).and_then(|(value, current)| unsafe {
					let val = &value.val;
					Some(match current {
						StreamValue::OneD(_) => StreamValue::OneD(val.one_d),
						StreamValue::TwoD { spatial, .. } => StreamValue::TwoD {
							x: val.two_d.x,
							y: val.two_d.y,
							spatial,
						},
						StreamValue::ThreeD { spatial, .. } => StreamValue::ThreeD {
							x: val.three_d.x,
							y: val.three_d.y,
							z: val.three_d.z,
							spatial,
						},
						StreamValue::Color { .. } => StreamValue::Color {
							alpha: val.color.alphaF,
							red: val.color.redF,
							green: val.color.greenF,
							blue: val.color.blueF,
						},
						_ => return None,
					})
				});
				diag!("AEGP_StreamSuite/AEGP_SetStreamValue",
					"streamH" => format!("{:#x}", streamH as usize),
					"param_index" => stream.map_or_else(|| "-".to_string(), |s| s.index.to_string()),
					"value" => incoming.map_or_else(|| "-".to_string(), |v| format!("{v:?}")),
				);

				let (Some(stream), Some(_), Some(current)) = (stream, value, current) else {
					return PF_Err_BAD_CALLBACK_PARAM;
				};
				if stream.node != Node::Param {
					return unsupported("AEGP_SetStreamValue (not an effect parameter)");
				}
				let _ = current;
				let Some(incoming) = incoming else {
					return unsupported("AEGP_SetStreamValue (a layer, arbitrary-data or valueless stream)");
				};
				write_stream_value(stream, incoming)
			})
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
	crate::core::ffi::guard("get_expression_state", || {
		diag!("AEGP_StreamSuite/AEGP_GetExpressionState", "streamH" => format!("{:#x}", streamH as usize));

		if unsafe { stream_of(streamH) }.is_none() {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		let Some(out) = (unsafe { enabledPB.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		*out = 0;
		PF_Err_NONE
	})
}

unsafe extern "C" fn set_expression_state(
	_aegp_plugin_id: AEGP_PluginID,
	_streamH: AEGP_StreamRefH,
	_enabledB: A_Boolean,
) -> A_Err {
	crate::core::ffi::guard("set_expression_state", || {
		diag!("AEGP_StreamSuite/AEGP_SetExpressionState");
		unsupported("AEGP_SetExpressionState")
	})
}

unsafe extern "C" fn get_expression(
	_aegp_plugin_id: AEGP_PluginID,
	_streamH: AEGP_StreamRefH,
	_expressionHZ: *mut AEGP_MemHandle,
) -> A_Err {
	crate::core::ffi::guard("get_expression", || {
		diag!("AEGP_StreamSuite/AEGP_GetExpression");
		unsupported("AEGP_GetExpression")
	})
}

unsafe extern "C" fn set_expression(
	_aegp_plugin_id: AEGP_PluginID,
	_streamH: AEGP_StreamRefH,
	_expressionP: *const A_char,
) -> A_Err {
	crate::core::ffi::guard("set_expression", || {
		diag!("AEGP_StreamSuite/AEGP_SetExpression");
		unsupported("AEGP_SetExpression")
	})
}

/// From v5 the expression text is UTF-16.
unsafe extern "C" fn set_expression_utf16(
	_aegp_plugin_id: AEGP_PluginID,
	_streamH: AEGP_StreamRefH,
	_expressionP: *const A_UTF16Char,
) -> A_Err {
	crate::core::ffi::guard("set_expression_utf16", || {
		diag!("AEGP_StreamSuite/AEGP_SetExpression");
		unsupported("AEGP_SetExpression")
	})
}

unsafe extern "C" fn duplicate_stream_ref(
	_aegp_plugin_id: AEGP_PluginID,
	streamH: AEGP_StreamRefH,
	dup_streamPH: *mut AEGP_StreamRefH,
) -> A_Err {
	crate::core::ffi::guard("duplicate_stream_ref", || {
		diag!("AEGP_StreamSuite/AEGP_DuplicateStreamRef", "streamH" => format!("{:#x}", streamH as usize));

		let Some(stream) = (unsafe { stream_of(streamH) }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		let Some(out) = (unsafe { dup_streamPH.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		*out = new_stream_handle(stream);
		PF_Err_NONE
	})
}

/// Ids handed out by [`get_unique_stream_id`], one per stream: two handles to
/// the same node of the same effect get the same id.
static UNIQUE_IDS: std::sync::Mutex<Vec<(usize, Node, usize)>> = std::sync::Mutex::new(Vec::new());

/// The id of `stream`, assigning the next one on first use. Ids start at 1.
fn unique_stream_id(stream: Stream) -> Option<i32> {
	let key = (stream.effect_ref as usize, stream.node, stream.index);
	let mut ids = UNIQUE_IDS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
	let position = match ids.iter().position(|k| *k == key) {
		Some(position) => position,
		None => {
			ids.push(key);
			ids.len() - 1
		}
	};
	i32::try_from(position + 1).ok()
}

unsafe extern "C" fn get_unique_stream_id(streamH: AEGP_StreamRefH, outID: *mut i32) -> A_Err {
	crate::core::ffi::guard("get_unique_stream_id", || {
		let stream = unsafe { stream_of(streamH) };
		let id = stream.and_then(unique_stream_id);
		diag!("AEGP_StreamSuite/AEGP_GetUniqueStreamID",
			"streamH" => format!("{:#x}", streamH as usize);
			result: format!("{id:?}"),
		);
		let (Some(id), Some(out)) = (id, unsafe { outID.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		*out = id;
		PF_Err_NONE
	})
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

/// Builds the `AEGP_StreamSuite3` vtable (wire version 8).
pub(super) const fn create_stream_suite_3() -> AEGP_StreamSuite3 {
	AEGP_StreamSuite3 {
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

/// Builds the `AEGP_StreamSuite5` vtable (wire version 10).
pub(super) const fn create_stream_suite_5() -> AEGP_StreamSuite5 {
	AEGP_StreamSuite5 {
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
		AEGP_SetExpression: Some(set_expression_utf16),
		AEGP_DuplicateStreamRef: Some(duplicate_stream_ref),
	}
}

/// Builds the `AEGP_StreamSuite6` vtable (wire version 11): v5 plus
/// `AEGP_GetUniqueStreamID`.
pub(super) const fn create_stream_suite_6() -> AEGP_StreamSuite6 {
	let v5 = create_stream_suite_5();
	AEGP_StreamSuite6 {
		AEGP_IsStreamLegal: v5.AEGP_IsStreamLegal,
		AEGP_CanVaryOverTime: v5.AEGP_CanVaryOverTime,
		AEGP_GetValidInterpolations: v5.AEGP_GetValidInterpolations,
		AEGP_GetNewLayerStream: v5.AEGP_GetNewLayerStream,
		AEGP_GetEffectNumParamStreams: v5.AEGP_GetEffectNumParamStreams,
		AEGP_GetNewEffectStreamByIndex: v5.AEGP_GetNewEffectStreamByIndex,
		AEGP_GetNewMaskStream: v5.AEGP_GetNewMaskStream,
		AEGP_DisposeStream: v5.AEGP_DisposeStream,
		AEGP_GetStreamName: v5.AEGP_GetStreamName,
		AEGP_GetStreamUnitsText: v5.AEGP_GetStreamUnitsText,
		AEGP_GetStreamProperties: v5.AEGP_GetStreamProperties,
		AEGP_IsStreamTimevarying: v5.AEGP_IsStreamTimevarying,
		AEGP_GetStreamType: v5.AEGP_GetStreamType,
		AEGP_GetNewStreamValue: v5.AEGP_GetNewStreamValue,
		AEGP_DisposeStreamValue: v5.AEGP_DisposeStreamValue,
		AEGP_SetStreamValue: v5.AEGP_SetStreamValue,
		AEGP_GetLayerStreamValue: v5.AEGP_GetLayerStreamValue,
		AEGP_GetExpressionState: v5.AEGP_GetExpressionState,
		AEGP_SetExpressionState: v5.AEGP_SetExpressionState,
		AEGP_GetExpression: v5.AEGP_GetExpression,
		AEGP_SetExpression: v5.AEGP_SetExpression,
		AEGP_DuplicateStreamRef: v5.AEGP_DuplicateStreamRef,
		AEGP_GetUniqueStreamID: Some(get_unique_stream_id),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn values_are_clamped_by_ranges_a_plugin_may_declare_badly() {
		assert_eq!(within(150, 0, 100), 100);
		assert_eq!(within(150, 100, 0), 100, "inverted");
		assert_eq!(within(-5, 100, 0), 0, "inverted");
		assert_eq!(within(2.0, f64::NAN, 1.0), 1.0, "a NaN end does not bound");
		assert_eq!(within(-2.0, f64::NAN, 1.0), -2.0);
		assert_eq!(within(0.5_f32, 0.0, 1.0), 0.5);
	}

	#[test]
	fn unique_stream_ids_follow_the_stream_not_the_handle() {
		let effect_ref = 0x5eed as PF_ProgPtr;
		let a = new_stream_handle(Stream {
			effect_ref,
			index: 3,
			node: Node::Param,
		});
		let b = new_stream_handle(Stream {
			effect_ref,
			index: 3,
			node: Node::Param,
		});
		let c = new_stream_handle(Stream {
			effect_ref,
			index: 4,
			node: Node::Param,
		});
		let id = |h| {
			let mut out = 0;
			assert_eq!(unsafe { get_unique_stream_id(h, &mut out) }, PF_Err_NONE);
			out
		};
		assert_eq!(id(a), id(b));
		assert_ne!(id(a), id(c));
		assert!(id(a) > 0);
		assert_ne!(unsafe { get_unique_stream_id(a, std::ptr::null_mut()) }, PF_Err_NONE);
		assert_ne!(
			unsafe { get_unique_stream_id(std::ptr::null_mut(), &mut 0) },
			PF_Err_NONE
		);
		for h in [a, b, c] {
			drop(unsafe { Box::from_raw(h as *mut Stream) });
		}
	}

	#[test]
	fn values_map_to_stream_types() {
		assert_eq!(StreamValue::NoData.stream_type(), AEGP_StreamType_NO_DATA);
		assert_eq!(
			StreamValue::Arb(std::ptr::null_mut()).stream_type(),
			AEGP_StreamType_ARB
		);
		let point = StreamValue::TwoD {
			x: 1.0,
			y: 2.0,
			spatial: true,
		};
		assert_eq!(point.stream_type(), AEGP_StreamType_TwoD_SPATIAL);
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
	fn written_values_fit_the_parameter() {
		let mut slider: PF_ParamDef = unsafe { std::mem::zeroed() };
		slider.param_type = PF_Param_FLOAT_SLIDER as PF_ParamType;
		slider.u.fs_d.valid_min = 0.0;
		slider.u.fs_d.valid_max = 4.0;
		assert_eq!(
			param_value_of(&slider, StreamValue::OneD(9.0)),
			Some(crate::ParamValue::Float(4.0))
		);
		let point = StreamValue::TwoD {
			x: 1.0,
			y: 2.0,
			spatial: true,
		};
		assert_eq!(param_value_of(&slider, point), None);

		let mut popup: PF_ParamDef = unsafe { std::mem::zeroed() };
		popup.param_type = PF_Param_POPUP as PF_ParamType;
		popup.u.pd.num_choices = 3;
		assert_eq!(
			param_value_of(&popup, StreamValue::OneD(7.0)),
			Some(crate::ParamValue::Popup(3))
		);

		// A color keeps its alpha, which has no control of its own.
		let mut color: PF_ParamDef = unsafe { std::mem::zeroed() };
		color.param_type = PF_Param_COLOR as PF_ParamType;
		color.u.cd.value.alpha = 255;
		let written = StreamValue::Color {
			alpha: 0.0,
			red: 1.0,
			green: 0.5,
			blue: 2.0,
		};
		assert_eq!(
			param_value_of(&color, written),
			Some(crate::ParamValue::Color {
				red: 255,
				green: 128,
				blue: 255,
				alpha: 255,
			})
		);
	}

	#[test]
	fn handles_duplicate_and_dispose() {
		let suite = create_stream_suite_4();
		let handle = new_stream_handle(Stream {
			effect_ref: std::ptr::null_mut(),
			index: 3,
			node: Node::Param,
		});
		let mut dup: AEGP_StreamRefH = std::ptr::null_mut();
		unsafe {
			assert_eq!(suite.AEGP_DuplicateStreamRef.unwrap()(0, handle, &mut dup), PF_Err_NONE);
			assert_eq!(stream_of(dup).map(|s| s.index), Some(3));
			assert_eq!(suite.AEGP_DisposeStream.unwrap()(dup), PF_Err_NONE);
			assert_eq!(suite.AEGP_DisposeStream.unwrap()(handle), PF_Err_NONE);
			assert_ne!(suite.AEGP_DisposeStream.unwrap()(std::ptr::null_mut()), PF_Err_NONE);
		}
	}
}
