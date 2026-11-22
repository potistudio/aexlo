//! `AEGP Stream Suite`, served at wire versions 7 (`AEGP_StreamSuite2`), 8
//! (`AEGP_StreamSuite3`), 9 (`AEGP_StreamSuite4`), 10 (`AEGP_StreamSuite5`) and
//! 11 (`AEGP_StreamSuite6`), plus the `AEGP_StreamRefH` handles it hands out. Version 8 keeps version 7's
//! char names and uses version 9's `AEGP_StreamValue2`.
//!
//! A stream handle is the effect's `PF_ProgPtr`, a layer, a node in that
//! layer's tree and, for a parameter, that parameter's index (the same index
//! space as `PF_ParamDef`s, 0 being the input layer). Values are read at the
//! time asked for through the one evaluator (`crate::animation`), so a stream,
//! its keyframes and a parameter checkout agree. There are no expressions.
//! `AEGP_SetStreamValue` writes the value of a parameter without keyframes, as
//! a plugin's Reset button does. Mask streams and every other write are
//! refused with `A_Err_GENERIC`.
//!
//! The same handle also names a node in the layer's dynamic-stream tree
//! ([`super::dynamic_stream`]): the transform, and what a text or shape layer
//! has in it.
//!
//! The table versions differ only in their value structs (`AEGP_StreamValue`
//! vs `AEGP_StreamValue2`, whose unions share the members written here) and,
//! from v5, a UTF-16 `AEGP_SetExpression`. v6 appends `AEGP_GetUniqueStreamID`.

use crate::sys::*;

use crate::PluginInstance;
use crate::animation::{Interpolation, TimePoint, Track};
use crate::core::diagnostics::diag;
use crate::project::{Property, PropertyNode};
use crate::suites::effect::{effect_ref_of, with_param};
use crate::suites::layer::{Kind, Timeline};
use crate::utils;

/// Where an `AEGP_StreamRefH` sits in the layer's stream tree.
///
/// Parameter nodes are the effect's `PF_ParamDef`s. Around them are the
/// layer's fixed groups (its transform, an effect parade, an empty mask
/// parade) and the properties of what is on the layer.
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
	/// A node of the layer's contents, by its place in the `PropertyTree`.
	Property(usize),
}

/// What an `AEGP_StreamRefH` points at.
#[derive(Clone, Copy)]
pub(crate) struct Stream {
	pub(crate) effect_ref: PF_ProgPtr,
	/// Index of this node in its parent. For [`Node::Param`], the parameter index.
	pub(crate) index: usize,
	pub(crate) node: Node,
	/// The layer's id, or [`EFFECT_LAYER`].
	pub(crate) layer: u32,
	/// For [`Node::Property`], the revision of the tree it is a node of.
	pub(crate) revision: u64,
}

/// `Stream::layer` for the layer the effect is on, whichever that is.
pub(crate) const EFFECT_LAYER: u32 = 0;

impl Stream {
	/// A node of the layer the effect is on.
	pub(crate) fn own(effect_ref: PF_ProgPtr, index: usize, node: Node) -> Self {
		Self {
			effect_ref,
			index,
			node,
			layer: EFFECT_LAYER,
			revision: 0,
		}
	}

	/// The root of the layer behind `timeline`.
	pub(crate) fn root(timeline: &Timeline) -> Self {
		Self {
			layer: if timeline.hosts_effect {
				EFFECT_LAYER
			} else {
				timeline.id
			},
			..Self::own(timeline.effect, 0, Node::Layer)
		}
	}

	/// The stream's layer, while it exists and its effect is in a plugin call.
	pub(crate) fn timeline(self) -> Option<Timeline> {
		let effect = self.effect_ref;
		if self.layer == EFFECT_LAYER {
			Timeline::of_effect(effect.cast())
		} else {
			Timeline::of(super::layer::handle_of_layer(effect, self.layer)?.cast())
		}
	}

	/// The node a [`Node::Property`] names, unless the tree has changed.
	pub(crate) fn property<R>(self, f: impl FnOnce(&PropertyNode) -> R) -> Option<R> {
		let Node::Property(id) = self.node else {
			return None;
		};
		super::layer::with_contents(&self.timeline()?, |tree, revision| {
			tree.nodes.get(id).filter(|_| revision == self.revision).map(f)
		})?
	}

	fn instance<'a>(self) -> Option<&'a PluginInstance> {
		// SAFETY: effect_ref is the live instance that is running the plugin call.
		PluginInstance::get_instance_ptr(self.effect_ref).map(|instance| unsafe { instance.as_ref() })
	}

	/// The value at `time` in layer time, or now.
	pub(crate) fn value(self, time: Option<TimePoint>) -> Option<StreamValue> {
		match self.node {
			Node::Param => {
				let instance = self.instance()?;
				let def = instance.param_at(self.index, time)?;
				let linked = || instance.project().and_then(|project| project.param_layer(self.index));
				Some(match StreamValue::of(&def, instance.arb_streams_live()) {
					StreamValue::LayerId(_) => StreamValue::LayerId(linked().unwrap_or(0) as A_long),
					value => value,
				})
			}
			Node::Anchor | Node::Position | Node::Scale | Node::Rotation | Node::Opacity => self.transform(time),
			Node::Property(_) => {
				let now = self.timeline()?.layer_time;
				let at = time.unwrap_or(now);
				self.property(|node| match &node.value {
					Property::Group => StreamValue::NoData,
					Property::OneD(track) => StreamValue::OneD(track.at(at)),
					Property::TwoD { value, spatial } => {
						let [x, y] = value.at(at);
						StreamValue::TwoD {
							x,
							y,
							spatial: *spatial,
						}
					}
					Property::Colour(track) => {
						let [red, green, blue, alpha] = track.at(at);
						StreamValue::Color {
							alpha,
							red,
							green,
							blue,
						}
					}
					Property::Path(_) => StreamValue::Outline,
					Property::Text(_) => StreamValue::TextDocument,
				})
			}
			_ => Some(StreamValue::NoData),
		}
	}

	/// A transform leaf: the project's, or identity (the anchor and position
	/// on the layer's centre).
	fn transform(self, time: Option<TimePoint>) -> Option<StreamValue> {
		let timeline = self.timeline()?;
		let at = time.unwrap_or(timeline.layer_time);
		let layer = self
			.instance()
			.and_then(|instance| instance.project())
			.and_then(|project| project.layer(timeline.id));
		let two = |[x, y]: [f64; 2], spatial| StreamValue::TwoD { x, y, spatial };
		let centre = [timeline.width as f64 / 2.0, timeline.height as f64 / 2.0];
		Some(match (self.node, layer.map(|layer| &layer.transform)) {
			(Node::Anchor, Some(t)) => two(t.anchor.at(at), true),
			(Node::Position, Some(t)) => two(t.position.at(at), true),
			(Node::Scale, Some(t)) => two(t.scale.at(at), false),
			(Node::Rotation, Some(t)) => StreamValue::OneD(t.rotation.at(at)),
			(Node::Opacity, Some(t)) => StreamValue::OneD(t.opacity.at(at)),
			(Node::Anchor | Node::Position, None) => two(centre, true),
			(Node::Scale, None) => two([100.0, 100.0], false),
			(Node::Rotation, None) => StreamValue::OneD(0.0),
			(Node::Opacity, None) => StreamValue::OneD(100.0),
			_ => StreamValue::NoData,
		})
	}

	/// The keyframes, as layer times and how each leads on.
	pub(crate) fn keys(self) -> Vec<(TimePoint, Interpolation)> {
		fn keys_of<T>(track: &Track<T>) -> Vec<(TimePoint, Interpolation)> {
			track.keys().iter().map(|key| (key.time, key.interpolation)).collect()
		}
		match self.node {
			Node::Param => self
				.instance()
				.and_then(|instance| instance.param_track(self.index))
				.map(keys_of),
			Node::Anchor | Node::Position | Node::Scale | Node::Rotation | Node::Opacity => (|| {
				let timeline = self.timeline()?;
				let t = &self.instance()?.project()?.layer(timeline.id)?.transform;
				Some(match self.node {
					Node::Anchor => keys_of(&t.anchor),
					Node::Position => keys_of(&t.position),
					Node::Scale => keys_of(&t.scale),
					Node::Rotation => keys_of(&t.rotation),
					_ => keys_of(&t.opacity),
				})
			})(),
			Node::Property(_) => self.property(|node| match &node.value {
				Property::Group => Vec::new(),
				Property::OneD(track) => keys_of(track),
				Property::TwoD { value, .. } => keys_of(value),
				Property::Colour(track) => keys_of(track),
				Property::Path(track) => keys_of(track),
				// Source text only holds.
				Property::Text(track) => keys_of(track)
					.into_iter()
					.map(|(time, _)| (time, Interpolation::Hold))
					.collect(),
			}),
			_ => None,
		}
		.unwrap_or_default()
	}

	fn outline(self, time: Option<TimePoint>) -> Option<crate::MaskPath> {
		let at = time.unwrap_or(self.timeline()?.layer_time);
		self.property(|node| match &node.value {
			Property::Path(track) => Some(track.at(at)),
			_ => None,
		})?
	}

	fn document(self, time: Option<TimePoint>) -> Option<String> {
		let at = time.unwrap_or(self.timeline()?.layer_time);
		self.property(|node| match &node.value {
			Property::Text(track) => Some(track.at(at).text().to_string()),
			_ => None,
		})?
	}
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
	/// The layer chosen in a layer parameter, or 0 when the project says none.
	LayerId(A_long),
	/// A path. Reading the value publishes a copy (see `mask_outline`).
	Outline,
	/// Source text. Reading the value publishes a copy (see `text_document`).
	TextDocument,
	/// An arbitrary-data parameter. The handle is the parameter's own value,
	/// not a copy, so disposing the stream value must not free it.
	Arb(PF_ArbitraryH),
}

impl StreamValue {
	fn of(def: &PF_ParamDef, arb_live: bool) -> Self {
		// SAFETY: every union read is selected by `param_type`.
		unsafe {
			match def.param_type {
				t if t == PF_Param_LAYER as PF_ParamType => Self::LayerId(0),
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
			Self::LayerId(_) => AEGP_StreamType_LAYER_ID,
			Self::Outline => AEGP_StreamType_MASK,
			Self::TextDocument => AEGP_StreamType_TEXT_DOCUMENT,
			Self::Arb(_) => AEGP_StreamType_ARB,
		}) as AEGP_StreamType
	}

	/// Number of values in one sample (`AEGP_GetStreamValueDimensionality`).
	pub(crate) fn dimensionality(self) -> A_short {
		match self {
			Self::NoData => 0,
			Self::OneD(_) | Self::LayerId(_) | Self::Arb(_) | Self::Outline | Self::TextDocument => 1,
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
			StreamValue::LayerId(id) => out.layer_id = id as _,
			// These own a copy, which `get_new_stream_value` publishes.
			StreamValue::Outline | StreamValue::TextDocument => {}
			// `arbH` is bound as `A_Handle` (`*mut c_void`); the parameter stores
			// a `PF_Handle` (`*mut *mut c_void`). The bit pattern is what the
			// plugin casts back.
			StreamValue::Arb(handle) => out.arbH = handle.cast(),
		}
	}};
}

/// The current value of the stream behind `streamH`.
///
/// # Safety
/// `streamH` must satisfy [`stream_of`]'s contract.
pub(crate) unsafe fn stream_value(streamH: AEGP_StreamRefH) -> Option<StreamValue> {
	unsafe { stream_of(streamH) }?.value(None)
}

fn unsupported(name: &str) -> A_Err {
	crate::observe::unsupported(
		crate::observe::UnsupportedKind::Declined,
		format!("AEGP_StreamSuite/{name}"),
		"not in the read-only project",
	);
	A_Err_GENERIC
}

/// The stream behind one of `AEGP_LayerStream_*`: `Ok(None)` for one this
/// layer has not, `Err` for one aexlo does not know about.
fn layer_stream(timeline: &Timeline, which: AEGP_LayerStream) -> Result<Option<Stream>, ()> {
	let root = Stream::root(timeline);
	let leaf = |index, node| Ok(Some(Stream { index, node, ..root }));
	#[allow(non_upper_case_globals)]
	match which {
		AEGP_LayerStream_ANCHORPOINT => leaf(0, Node::Anchor),
		AEGP_LayerStream_POSITION => leaf(1, Node::Position),
		AEGP_LayerStream_SCALE => leaf(2, Node::Scale),
		AEGP_LayerStream_ROTATE_Z => leaf(3, Node::Rotation),
		AEGP_LayerStream_OPACITY => leaf(4, Node::Opacity),
		AEGP_LayerStream_SOURCE_TEXT if timeline.kind == Kind::Text => {
			let found = super::layer::with_contents(timeline, |tree, revision| {
				let id = tree
					.nodes
					.iter()
					.position(|node| node.match_name == "ADBE Text Document")?;
				Some(Stream {
					index: tree.nodes[id].index,
					node: Node::Property(id),
					revision,
					..root
				})
			});
			Ok(found.flatten())
		}
		AEGP_LayerStream_SOURCE_TEXT => Ok(None),
		_ => Err(()),
	}
}

unsafe extern "C" fn is_stream_legal(
	layerH: AEGP_LayerH,
	which_stream: AEGP_LayerStream,
	is_legalP: *mut A_Boolean,
) -> A_Err {
	crate::core::ffi::guard("is_stream_legal", || {
		diag!("AEGP_StreamSuite/AEGP_IsStreamLegal", "which_stream" => which_stream);
		let (Some(timeline), Some(out)) = (Timeline::of(layerH.cast()), unsafe { is_legalP.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		let Ok(stream) = layer_stream(&timeline, which_stream) else {
			return unsupported("AEGP_IsStreamLegal");
		};
		*out = stream.is_some() as A_Boolean;
		PF_Err_NONE
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
			StreamValue::LayerId(_) | StreamValue::Arb(_) | StreamValue::TextDocument => AEGP_KeyInterpMask_HOLD,
			_ => AEGP_KeyInterpMask_LINEAR | AEGP_KeyInterpMask_HOLD,
		} as AEGP_KeyInterpolationMask;
		PF_Err_NONE
	})
}

unsafe extern "C" fn get_new_layer_stream(
	_aegp_plugin_id: AEGP_PluginID,
	layerH: AEGP_LayerH,
	which_stream: AEGP_LayerStream,
	streamPH: *mut AEGP_StreamRefH,
) -> A_Err {
	crate::core::ffi::guard("get_new_layer_stream", || {
		diag!("AEGP_StreamSuite/AEGP_GetNewLayerStream", "which_stream" => which_stream);
		let (Some(timeline), Some(out)) = (Timeline::of(layerH.cast()), unsafe { streamPH.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		*out = std::ptr::null_mut();
		let Ok(Some(stream)) = layer_stream(&timeline, which_stream) else {
			return unsupported("AEGP_GetNewLayerStream");
		};
		*out = new_stream_handle(stream);
		PF_Err_NONE
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
		*out = new_stream_handle(Stream::own(effect_ref, index, Node::Param));
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
		if let Some(name) = stream.property(|node| node.name.clone()) {
			let buf = unsafe { std::slice::from_raw_parts_mut(nameZ, AEGP_MAX_STREAM_NAME_SIZE as usize) };
			super::ffi::write_c_str(buf, &name);
			return PF_Err_NONE;
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

/// The v4 form returns a UTF-16 `AEGP_MemHandle`, the plugin's to free.
unsafe extern "C" fn get_stream_name_v2(
	_pluginID: AEGP_PluginID,
	streamH: AEGP_StreamRefH,
	force_englishB: A_Boolean,
	utf_stream_namePH: *mut AEGP_MemHandle,
) -> A_Err {
	crate::core::ffi::guard("get_stream_name_v2", || {
		diag!("AEGP_StreamSuite/AEGP_GetStreamName");
		let Some(out) = (unsafe { utf_stream_namePH.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		let mut name = [0 as A_char; AEGP_MAX_STREAM_NAME_SIZE as usize];
		// SAFETY: the buffer is as big as the v1 call asks for.
		let err = unsafe { get_stream_name_v1(streamH, force_englishB, name.as_mut_ptr()) };
		if err != PF_Err_NONE {
			return err;
		}
		let name = unsafe { super::ffi::read_c_str(name.as_ptr()) }.unwrap_or_default();
		*out = super::layer::utf16_handle(&name);
		PF_Err_NONE
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
			*out = match stream.value(None) {
				Some(StreamValue::TwoD { spatial: true, .. }) => AEGP_StreamFlag_IS_SPATIAL,
				_ => AEGP_StreamFlag_NONE,
			} as AEGP_StreamFlags;
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

/// Time-varying means keyframed: there are no expressions.
unsafe extern "C" fn is_stream_timevarying(streamH: AEGP_StreamRefH, is_timevaryingPB: *mut A_Boolean) -> A_Err {
	crate::core::ffi::guard("is_stream_timevarying", || {
		diag!("AEGP_StreamSuite/AEGP_IsStreamTimevarying", "streamH" => format!("{:#x}", streamH as usize));
		let Some(stream) = (unsafe { stream_of(streamH) }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		let Some(out) = (unsafe { is_timevaryingPB.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		*out = !stream.keys().is_empty() as A_Boolean;
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

/// `time`, told in `mode`, in the stream's layer time. A null time is now,
/// and `Err` a time with no scale.
fn layer_time(stream: Stream, mode: AEGP_LTimeMode, time: *const A_Time) -> Result<Option<TimePoint>, ()> {
	// SAFETY: null, or the plugin's `A_Time` for this call.
	let Some(time) = (unsafe { time.as_ref() }).copied() else {
		return Ok(None);
	};
	let time = TimePoint::from_sys(time).ok_or(())?;
	Ok(Some(match stream.timeline() {
		Some(timeline) => timeline.layer_time_of(time, mode),
		None => time,
	}))
}

/// The stream's value at a time. A path or a document is a copy, the plugin's
/// until `AEGP_DisposeStreamValue`.
macro_rules! get_new_stream_value {
	($name:ident, $value_ty:ty) => {
		unsafe extern "C" fn $name(
			_aegp_plugin_id: AEGP_PluginID,
			streamH: AEGP_StreamRefH,
			time_mode: AEGP_LTimeMode,
			timePT: *const A_Time,
			_pre_expressionB: A_Boolean,
			valueP: *mut $value_ty,
		) -> A_Err {
			crate::core::ffi::guard("get_new_stream_value", || {
				diag!("AEGP_StreamSuite/AEGP_GetNewStreamValue", "streamH" => format!("{:#x}", streamH as usize));

				let Some(stream) = (unsafe { stream_of(streamH) }) else {
					return PF_Err_BAD_CALLBACK_PARAM;
				};
				let Ok(time) = layer_time(stream, time_mode, timePT) else {
					return PF_Err_BAD_CALLBACK_PARAM;
				};
				let Some(value) = stream.value(time) else {
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
				let owned = match value {
					StreamValue::Outline => stream.outline(time).map(|path| super::mask_outline::publish(path) as usize),
					StreamValue::TextDocument => {
						stream.document(time).map(|text| super::text_document::publish(&text) as usize)
					}
					_ => None,
				};
				match owned {
					// SAFETY: the union's handle members are pointers at its start.
					Some(handle) => unsafe { *(&raw mut out.val).cast::<usize>() = handle },
					None => write_val!(value, &mut out.val),
				}
				PF_Err_NONE
			})
		}
	};
}

get_new_stream_value!(get_new_stream_value_v1, AEGP_StreamValue);
get_new_stream_value!(get_new_stream_value_v2, AEGP_StreamValue2);

/// Read `stream` at a keyframe's own time.
pub(crate) fn keyframe_value(streamH: AEGP_StreamRefH, time: TimePoint, out: *mut AEGP_StreamValue2) -> A_Err {
	let time = time.to_sys();
	// SAFETY: `out` is the plugin's, null-checked by the call, and `time` is a local.
	unsafe { get_new_stream_value_v2(0, streamH, AEGP_LTimeMode_LayerTime as AEGP_LTimeMode, &time, 0, out) }
}

/// A path or a document is the value's own copy, dropped here. An arbitrary
/// handle is the parameter's own value, so it is left alone.
macro_rules! dispose_stream_value {
	($name:ident, $value_ty:ty) => {
		unsafe extern "C" fn $name(valueP: *mut $value_ty) -> A_Err {
			diag!("AEGP_StreamSuite/AEGP_DisposeStreamValue", "valueP" => format!("{:#x}", valueP as usize));

			if valueP.is_null() {
				return PF_Err_BAD_CALLBACK_PARAM;
			}
			// SAFETY: non-null, and the union starts with a word however it was filled in.
			let held = unsafe { *(&raw const (*valueP).val).cast::<usize>() };
			if !super::mask_outline::dispose(held) {
				super::text_document::dispose(held);
			}
			PF_Err_NONE
		}
	};
}

dispose_stream_value!(dispose_stream_value_v1, AEGP_StreamValue);
dispose_stream_value!(dispose_stream_value_v2, AEGP_StreamValue2);

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
				P::Slider((v.round() as i32).clamp(def.u.sd.valid_min, def.u.sd.valid_max))
			}
			(t, StreamValue::OneD(v)) if t == PF_Param_FIX_SLIDER as PF_ParamType => P::Fixed((v as f32).clamp(
				utils::fixed16_to_f32(def.u.fd.valid_min),
				utils::fixed16_to_f32(def.u.fd.valid_max),
			)),
			(t, StreamValue::OneD(v)) if t == PF_Param_FLOAT_SLIDER as PF_ParamType => {
				P::Float(v.clamp(def.u.fs_d.valid_min as f64, def.u.fs_d.valid_max as f64))
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
				if !stream.keys().is_empty() {
					return unsupported("AEGP_SetStreamValue (the parameter is keyframed)");
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
			layerH: AEGP_LayerH,
			which_stream: AEGP_LayerStream,
			time_mode: AEGP_LTimeMode,
			timePT: *const A_Time,
			_pre_expressionB: A_Boolean,
			stream_valP: *mut $val_ty,
			stream_typeP0: *mut AEGP_StreamType,
		) -> A_Err {
			crate::core::ffi::guard("get_layer_stream_value", || {
				diag!("AEGP_StreamSuite/AEGP_GetLayerStreamValue", "which_stream" => which_stream);
				let (Some(timeline), Some(out)) = (Timeline::of(layerH.cast()), unsafe { stream_valP.as_mut() }) else {
					return PF_Err_BAD_CALLBACK_PARAM;
				};
				let Ok(Some(stream)) = layer_stream(&timeline, which_stream) else {
					return unsupported(&format!("AEGP_GetLayerStreamValue (stream {which_stream})"));
				};
				let Ok(time) = layer_time(stream, time_mode, timePT) else {
					return PF_Err_BAD_CALLBACK_PARAM;
				};
				let Some(value) = stream.value(time) else {
					return PF_Err_BAD_CALLBACK_PARAM;
				};
				// Only primitive values: nothing disposes what this hands out.
				if matches!(value, StreamValue::Outline | StreamValue::TextDocument) {
					return unsupported("AEGP_GetLayerStreamValue (a value that needs disposing)");
				}
				// SAFETY: every member of the value union is plain old data.
				*out = unsafe { std::mem::zeroed() };
				write_val!(value, &mut *out);
				if let Some(kind) = unsafe { stream_typeP0.as_mut() } {
					*kind = value.stream_type();
				}
				PF_Err_NONE
			})
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
static UNIQUE_IDS: std::sync::Mutex<Vec<(usize, Node, usize, u32)>> = std::sync::Mutex::new(Vec::new());

/// The id of `stream`, assigning the next one on first use. Ids start at 1.
fn unique_stream_id(stream: Stream) -> Option<i32> {
	let key = (stream.effect_ref as usize, stream.node, stream.index, stream.layer);
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

/// Wire version 4 (Extrude's 3D Camera mode asks for it): version 7 without
/// `AEGP_GetValidInterpolations` and `AEGP_DuplicateStreamRef`.
pub(super) const fn create_stream_suite_1() -> AEGP_StreamSuite1 {
	AEGP_StreamSuite1 {
		AEGP_IsStreamLegal: Some(is_stream_legal),
		AEGP_CanVaryOverTime: Some(can_vary_over_time),
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
	}
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
	fn unique_stream_ids_follow_the_stream_not_the_handle() {
		let effect_ref = 0x5eed as PF_ProgPtr;
		let a = new_stream_handle(Stream::own(effect_ref, 3, Node::Param));
		let b = new_stream_handle(Stream::own(effect_ref, 3, Node::Param));
		let c = new_stream_handle(Stream::own(effect_ref, 4, Node::Param));
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
		let handle = new_stream_handle(Stream::own(std::ptr::null_mut(), 3, Node::Param));
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
