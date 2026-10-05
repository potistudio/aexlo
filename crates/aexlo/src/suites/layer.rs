//! `AEGP Layer Suite` (wire versions 11 to 15, `AEGP_LayerSuite5` to `9`) and
//! `AEGP Light Suite` v1 over aexlo's one-layer timeline.
//!
//! aexlo renders a single effect on a single layer, and that layer is the
//! whole comp: the comp is the input layer's size, the layer starts at comp
//! time 0 with no stretch (so layer time is comp time), runs for
//! [`COMP_DURATION_SECONDS`], is a 2D AV layer with an identity transform, and
//! has no parent, track matte or source item. The comp's frame rate and the
//! current time are the instance's render context (frame 0 at 30 fps by
//! default).
//!
//! Layer and comp handles are the effect's `PF_ProgPtr` itself: After Effects
//! never asks plugins to dispose of them, so there is nothing to allocate.
//! Anything that edits the timeline is refused with `A_Err_GENERIC`.

use after_effects_sys::*;

use crate::PluginInstance;
use crate::core::diagnostics::diag;

/// Length of aexlo's comp, and of the layer that fills it.
pub(crate) const COMP_DURATION_SECONDS: A_long = 30;

/// The id of the one layer (`AEGP_GetLayerID`).
const LAYER_ID: AEGP_LayerIDVal = 1;

/// The layer handle for the effect behind `effect_ref`.
pub(crate) fn layer_handle(effect_ref: PF_ProgPtr) -> AEGP_LayerH {
	effect_ref as AEGP_LayerH
}

/// The comp handle for the effect behind `effect_ref`.
pub(crate) fn comp_handle(effect_ref: PF_ProgPtr) -> AEGP_CompH {
	effect_ref as AEGP_CompH
}

/// What the timeline looks like from the effect behind a layer or comp handle.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Timeline {
	pub width: u32,
	pub height: u32,
	pub current_time: A_long,
	pub time_step: A_long,
	pub time_scale: A_u_long,
}

impl Timeline {
	/// The timeline of the effect behind a layer or comp handle, or `None` for
	/// a null handle.
	pub(crate) fn of(handle: *mut std::ffi::c_void) -> Option<Self> {
		let instance = PluginInstance::get_instance_ptr(handle as PF_ProgPtr)?;
		// SAFETY: the handle is the live instance that is running the plugin call.
		let instance = unsafe { instance.as_ref() };
		let (width, height) = instance.input_size();
		let (current_time, time_step, time_scale) = instance.render_context.time();
		Some(Self {
			width,
			height,
			current_time,
			time_step,
			time_scale,
		})
	}

	pub(crate) fn current(&self) -> A_Time {
		A_Time {
			value: self.current_time,
			scale: self.time_scale,
		}
	}

	pub(crate) fn duration(&self) -> A_Time {
		A_Time {
			value: COMP_DURATION_SECONDS * self.time_scale as A_long,
			scale: self.time_scale,
		}
	}

	/// The frame `time` falls on.
	fn frame_of(&self, time: &A_Time) -> i64 {
		if time.scale == 0 || self.time_step == 0 {
			return 0;
		}
		(time.value as i64 * self.time_scale as i64).div_euclid(time.scale as i64 * self.time_step as i64)
	}
}

fn unsupported(name: &str) -> A_Err {
	log::warn!("AEGP_LayerSuite/{name}: aexlo's timeline is a fixed single layer");
	A_Err_GENERIC as A_Err
}

/// Resolve `handle` and write `value(timeline)` to `out`, or report a bad
/// handle or a null out-param.
pub(crate) fn answer<T>(handle: *mut std::ffi::c_void, out: *mut T, value: impl FnOnce(&Timeline) -> T) -> A_Err {
	let Some(timeline) = Timeline::of(handle) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	let Some(out) = (unsafe { out.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	*out = value(&timeline);
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn get_comp_num_layers(compH: AEGP_CompH, num_layersPL: *mut A_long) -> A_Err {
	diag!("AEGP_LayerSuite/AEGP_GetCompNumLayers", "compH" => format!("{:#x}", compH as usize));
	answer(compH as _, num_layersPL, |_| 1)
}

unsafe extern "C" fn get_comp_layer_by_index(
	compH: AEGP_CompH,
	layer_indexL: A_long,
	layerPH: *mut AEGP_LayerH,
) -> A_Err {
	diag!("AEGP_LayerSuite/AEGP_GetCompLayerByIndex",
		"compH" => format!("{:#x}", compH as usize),
		"layer_indexL" => layer_indexL,
	);
	if layer_indexL != 0 && Timeline::of(compH as _).is_some() {
		return PF_Err_INVALID_INDEX as A_Err;
	}
	answer(compH as _, layerPH, |_| layer_handle(compH as PF_ProgPtr))
}

unsafe extern "C" fn get_active_layer(_layerPH: *mut AEGP_LayerH) -> A_Err {
	diag!("AEGP_LayerSuite/AEGP_GetActiveLayer");
	unsupported("AEGP_GetActiveLayer")
}

unsafe extern "C" fn get_layer_index(layerH: AEGP_LayerH, layer_indexPL: *mut A_long) -> A_Err {
	diag!("AEGP_LayerSuite/AEGP_GetLayerIndex", "layerH" => format!("{:#x}", layerH as usize));
	answer(layerH as _, layer_indexPL, |_| 0)
}

unsafe extern "C" fn get_layer_source_item(_layerH: AEGP_LayerH, _source_itemPH: *mut AEGP_ItemH) -> A_Err {
	diag!("AEGP_LayerSuite/AEGP_GetLayerSourceItem");
	unsupported("AEGP_GetLayerSourceItem")
}

unsafe extern "C" fn get_layer_source_item_id(_layerH: AEGP_LayerH, _source_item_idPL: *mut A_long) -> A_Err {
	diag!("AEGP_LayerSuite/AEGP_GetLayerSourceItemID");
	unsupported("AEGP_GetLayerSourceItemID")
}

unsafe extern "C" fn get_layer_parent_comp(layerH: AEGP_LayerH, compPH: *mut AEGP_CompH) -> A_Err {
	diag!("AEGP_LayerSuite/AEGP_GetLayerParentComp", "layerH" => format!("{:#x}", layerH as usize));
	answer(layerH as _, compPH, |_| comp_handle(layerH as PF_ProgPtr))
}

/// The layer has never been renamed and has no source item, so both names
/// are empty.
unsafe extern "C" fn get_layer_name(
	layerH: AEGP_LayerH,
	layer_nameZ0: *mut A_char,
	source_nameZ0: *mut A_char,
) -> A_Err {
	diag!("AEGP_LayerSuite/AEGP_GetLayerName", "layerH" => format!("{:#x}", layerH as usize));
	if Timeline::of(layerH as _).is_none() {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	}
	unsafe {
		if let Some(out) = layer_nameZ0.as_mut() {
			*out = 0;
		}
		if let Some(out) = source_nameZ0.as_mut() {
			*out = 0;
		}
	}
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn get_layer_quality(layerH: AEGP_LayerH, qualityP: *mut AEGP_LayerQuality) -> A_Err {
	diag!("AEGP_LayerSuite/AEGP_GetLayerQuality", "layerH" => format!("{:#x}", layerH as usize));
	answer(layerH as _, qualityP, |_| AEGP_LayerQual_BEST as AEGP_LayerQuality)
}

unsafe extern "C" fn set_layer_quality(_layerH: AEGP_LayerH, _quality: AEGP_LayerQuality) -> A_Err {
	diag!("AEGP_LayerSuite/AEGP_SetLayerQuality");
	unsupported("AEGP_SetLayerQuality")
}

unsafe extern "C" fn get_layer_flags(layerH: AEGP_LayerH, layer_flagsP: *mut AEGP_LayerFlags) -> A_Err {
	diag!("AEGP_LayerSuite/AEGP_GetLayerFlags", "layerH" => format!("{:#x}", layerH as usize));
	answer(layerH as _, layer_flagsP, |_| {
		AEGP_LayerFlag_VIDEO_ACTIVE as AEGP_LayerFlags
	})
}

unsafe extern "C" fn set_layer_flag(_layerH: AEGP_LayerH, _single_flag: AEGP_LayerFlags, _valueB: A_Boolean) -> A_Err {
	diag!("AEGP_LayerSuite/AEGP_SetLayerFlag");
	unsupported("AEGP_SetLayerFlag")
}

unsafe extern "C" fn is_layer_video_really_on(layerH: AEGP_LayerH, onPB: *mut A_Boolean) -> A_Err {
	diag!("AEGP_LayerSuite/AEGP_IsLayerVideoReallyOn", "layerH" => format!("{:#x}", layerH as usize));
	answer(layerH as _, onPB, |_| 1)
}

unsafe extern "C" fn is_layer_audio_really_on(layerH: AEGP_LayerH, onPB: *mut A_Boolean) -> A_Err {
	diag!("AEGP_LayerSuite/AEGP_IsLayerAudioReallyOn", "layerH" => format!("{:#x}", layerH as usize));
	answer(layerH as _, onPB, |_| 0)
}

/// Layer time is comp time, so `time_mode` doesn't matter here or below.
unsafe extern "C" fn get_layer_current_time(
	layerH: AEGP_LayerH,
	_time_mode: AEGP_LTimeMode,
	curr_timePT: *mut A_Time,
) -> A_Err {
	diag!("AEGP_LayerSuite/AEGP_GetLayerCurrentTime", "layerH" => format!("{:#x}", layerH as usize));
	answer(layerH as _, curr_timePT, Timeline::current)
}

unsafe extern "C" fn get_layer_in_point(
	layerH: AEGP_LayerH,
	_time_mode: AEGP_LTimeMode,
	in_pointPT: *mut A_Time,
) -> A_Err {
	diag!("AEGP_LayerSuite/AEGP_GetLayerInPoint", "layerH" => format!("{:#x}", layerH as usize));
	answer(layerH as _, in_pointPT, |t| A_Time {
		value: 0,
		scale: t.time_scale,
	})
}

unsafe extern "C" fn get_layer_duration(
	layerH: AEGP_LayerH,
	_time_mode: AEGP_LTimeMode,
	durationPT: *mut A_Time,
) -> A_Err {
	diag!("AEGP_LayerSuite/AEGP_GetLayerDuration", "layerH" => format!("{:#x}", layerH as usize));
	answer(layerH as _, durationPT, Timeline::duration)
}

unsafe extern "C" fn set_layer_in_point_and_duration(
	_layerH: AEGP_LayerH,
	_time_mode: AEGP_LTimeMode,
	_in_pointPT: *const A_Time,
	_durationPT: *const A_Time,
) -> A_Err {
	diag!("AEGP_LayerSuite/AEGP_SetLayerInPointAndDuration");
	unsupported("AEGP_SetLayerInPointAndDuration")
}

unsafe extern "C" fn get_layer_offset(layerH: AEGP_LayerH, offsetPT: *mut A_Time) -> A_Err {
	diag!("AEGP_LayerSuite/AEGP_GetLayerOffset", "layerH" => format!("{:#x}", layerH as usize));
	answer(layerH as _, offsetPT, |t| A_Time {
		value: 0,
		scale: t.time_scale,
	})
}

unsafe extern "C" fn set_layer_offset(_layerH: AEGP_LayerH, _offsetPT: *const A_Time) -> A_Err {
	diag!("AEGP_LayerSuite/AEGP_SetLayerOffset");
	unsupported("AEGP_SetLayerOffset")
}

unsafe extern "C" fn get_layer_stretch(layerH: AEGP_LayerH, stretchPRt: *mut A_Ratio) -> A_Err {
	diag!("AEGP_LayerSuite/AEGP_GetLayerStretch", "layerH" => format!("{:#x}", layerH as usize));
	answer(layerH as _, stretchPRt, |_| A_Ratio { num: 1, den: 1 })
}

unsafe extern "C" fn set_layer_stretch(_layerH: AEGP_LayerH, _stretchPRt: *const A_Ratio) -> A_Err {
	diag!("AEGP_LayerSuite/AEGP_SetLayerStretch");
	unsupported("AEGP_SetLayerStretch")
}

unsafe extern "C" fn get_layer_transfer_mode(
	layerH: AEGP_LayerH,
	transfer_modeP: *mut AEGP_LayerTransferMode,
) -> A_Err {
	diag!("AEGP_LayerSuite/AEGP_GetLayerTransferMode", "layerH" => format!("{:#x}", layerH as usize));
	answer(layerH as _, transfer_modeP, |_| AEGP_LayerTransferMode {
		mode: PF_Xfer_IN_FRONT as PF_TransferMode,
		flags: 0,
		track_matte: AEGP_TrackMatte_NO_TRACK_MATTE as AEGP_TrackMatte,
	})
}

unsafe extern "C" fn set_layer_transfer_mode(
	_layerH: AEGP_LayerH,
	_transfer_modeP: *const AEGP_LayerTransferMode,
) -> A_Err {
	diag!("AEGP_LayerSuite/AEGP_SetLayerTransferMode");
	unsupported("AEGP_SetLayerTransferMode")
}

/// There are no items to add.
unsafe extern "C" fn is_add_layer_valid(
	_item_to_addH: AEGP_ItemH,
	into_compH: AEGP_CompH,
	validPB: *mut A_Boolean,
) -> A_Err {
	diag!("AEGP_LayerSuite/AEGP_IsAddLayerValid", "into_compH" => format!("{:#x}", into_compH as usize));
	answer(into_compH as _, validPB, |_| 0)
}

unsafe extern "C" fn add_layer(
	_item_to_addH: AEGP_ItemH,
	_into_compH: AEGP_CompH,
	_added_layerPH0: *mut AEGP_LayerH,
) -> A_Err {
	diag!("AEGP_LayerSuite/AEGP_AddLayer");
	unsupported("AEGP_AddLayer")
}

unsafe extern "C" fn reorder_layer(_layerH: AEGP_LayerH, _layer_indexL: A_long) -> A_Err {
	diag!("AEGP_LayerSuite/AEGP_ReorderLayer");
	unsupported("AEGP_ReorderLayer")
}

/// The layer has no masks, so its bounds are its full frame.
unsafe extern "C" fn get_layer_masked_bounds(
	layerH: AEGP_LayerH,
	_time_mode: AEGP_LTimeMode,
	_timePT: *const A_Time,
	boundsPR: *mut A_FloatRect,
) -> A_Err {
	diag!("AEGP_LayerSuite/AEGP_GetLayerMaskedBounds", "layerH" => format!("{:#x}", layerH as usize));
	answer(layerH as _, boundsPR, |t| A_FloatRect {
		left: 0.0,
		top: 0.0,
		right: t.width as A_FpLong,
		bottom: t.height as A_FpLong,
	})
}

unsafe extern "C" fn get_layer_object_type(layerH: AEGP_LayerH, object_type: *mut AEGP_ObjectType) -> A_Err {
	diag!("AEGP_LayerSuite/AEGP_GetLayerObjectType", "layerH" => format!("{:#x}", layerH as usize));
	answer(layerH as _, object_type, |_| AEGP_ObjectType_AV as AEGP_ObjectType)
}

unsafe extern "C" fn is_layer_3d(layerH: AEGP_LayerH, is_3DPB: *mut A_Boolean) -> A_Err {
	diag!("AEGP_LayerSuite/AEGP_IsLayer3D", "layerH" => format!("{:#x}", layerH as usize));
	answer(layerH as _, is_3DPB, |_| 0)
}

unsafe extern "C" fn is_layer_2d(layerH: AEGP_LayerH, is_2DPB: *mut A_Boolean) -> A_Err {
	diag!("AEGP_LayerSuite/AEGP_IsLayer2D", "layerH" => format!("{:#x}", layerH as usize));
	answer(layerH as _, is_2DPB, |_| 1)
}

unsafe extern "C" fn is_video_active(
	layerH: AEGP_LayerH,
	_time_mode: AEGP_LTimeMode,
	timePT: *const A_Time,
	is_activePB: *mut A_Boolean,
) -> A_Err {
	diag!("AEGP_LayerSuite/AEGP_IsVideoActive", "layerH" => format!("{:#x}", layerH as usize));
	let Some(time) = (unsafe { timePT.as_ref() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	answer(layerH as _, is_activePB, |t| {
		let frame = t.frame_of(time);
		(0..t.frame_of(&t.duration())).contains(&frame) as A_Boolean
	})
}

unsafe extern "C" fn is_layer_used_as_track_matte(
	layerH: AEGP_LayerH,
	_fill_must_be_activeB: A_Boolean,
	is_track_mattePB: *mut A_Boolean,
) -> A_Err {
	diag!("AEGP_LayerSuite/AEGP_IsLayerUsedAsTrackMatte", "layerH" => format!("{:#x}", layerH as usize));
	answer(layerH as _, is_track_mattePB, |_| 0)
}

unsafe extern "C" fn does_layer_have_track_matte(layerH: AEGP_LayerH, has_track_mattePB: *mut A_Boolean) -> A_Err {
	diag!("AEGP_LayerSuite/AEGP_DoesLayerHaveTrackMatte", "layerH" => format!("{:#x}", layerH as usize));
	answer(layerH as _, has_track_mattePB, |_| 0)
}

unsafe extern "C" fn convert_comp_to_layer_time(
	layerH: AEGP_LayerH,
	comp_timePT: *const A_Time,
	layer_timePT: *mut A_Time,
) -> A_Err {
	diag!("AEGP_LayerSuite/AEGP_ConvertCompToLayerTime", "layerH" => format!("{:#x}", layerH as usize));
	let Some(time) = (unsafe { comp_timePT.as_ref() }).copied() else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	answer(layerH as _, layer_timePT, |_| time)
}

unsafe extern "C" fn convert_layer_to_comp_time(
	layerH: AEGP_LayerH,
	layer_timePT: *const A_Time,
	comp_timePT: *mut A_Time,
) -> A_Err {
	diag!("AEGP_LayerSuite/AEGP_ConvertLayerToCompTime", "layerH" => format!("{:#x}", layerH as usize));
	let Some(time) = (unsafe { layer_timePT.as_ref() }).copied() else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	answer(layerH as _, comp_timePT, |_| time)
}

/// A value that changes from frame to frame but is fixed for a given frame
/// (the `wiggle`-style randomness AE derives per layer and frame).
unsafe extern "C" fn get_layer_dancing_rand_value(
	layerH: AEGP_LayerH,
	comp_timePT: *const A_Time,
	rand_valuePL: *mut A_long,
) -> A_Err {
	diag!("AEGP_LayerSuite/AEGP_GetLayerDancingRandValue", "layerH" => format!("{:#x}", layerH as usize));
	let Some(time) = (unsafe { comp_timePT.as_ref() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	answer(layerH as _, rand_valuePL, |t| {
		// splitmix64 of the frame number.
		let mut z = (t.frame_of(time) as u64).wrapping_add(0x9e37_79b9_7f4a_7c15);
		z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
		z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
		((z ^ (z >> 31)) >> 33) as A_long
	})
}

unsafe extern "C" fn get_layer_id(layerH: AEGP_LayerH, id_valP: *mut AEGP_LayerIDVal) -> A_Err {
	diag!("AEGP_LayerSuite/AEGP_GetLayerID", "layerH" => format!("{:#x}", layerH as usize));
	answer(layerH as _, id_valP, |_| LAYER_ID)
}

fn identity() -> A_Matrix4 {
	let mut mat = [[0.0; 4]; 4];
	for (i, row) in mat.iter_mut().enumerate() {
		row[i] = 1.0;
	}
	A_Matrix4 { mat }
}

/// The layer fills the comp untransformed, so layer space is comp space.
unsafe extern "C" fn get_layer_to_world_xform(
	aegp_layerH: AEGP_LayerH,
	_comp_timeP: *const A_Time,
	transform: *mut A_Matrix4,
) -> A_Err {
	diag!("AEGP_LayerSuite/AEGP_GetLayerToWorldXform", "aegp_layerH" => format!("{:#x}", aegp_layerH as usize));
	answer(aegp_layerH as _, transform, |_| identity())
}

unsafe extern "C" fn get_layer_to_world_xform_from_view(
	aegp_layerH: AEGP_LayerH,
	_view_timeP: *const A_Time,
	_comp_timeP: *const A_Time,
	transform: *mut A_Matrix4,
) -> A_Err {
	diag!("AEGP_LayerSuite/AEGP_GetLayerToWorldXformFromView", "aegp_layerH" => format!("{:#x}", aegp_layerH as usize));
	answer(aegp_layerH as _, transform, |_| identity())
}

unsafe extern "C" fn set_layer_name(_aegp_layerH: AEGP_LayerH, _new_nameZ: *const A_char) -> A_Err {
	diag!("AEGP_LayerSuite/AEGP_SetLayerName");
	unsupported("AEGP_SetLayerName")
}

/// No parent: a null handle, as in After Effects.
unsafe extern "C" fn get_layer_parent(layerH: AEGP_LayerH, parent_layerPH: *mut AEGP_LayerH) -> A_Err {
	diag!("AEGP_LayerSuite/AEGP_GetLayerParent", "layerH" => format!("{:#x}", layerH as usize));
	answer(layerH as _, parent_layerPH, |_| std::ptr::null_mut())
}

unsafe extern "C" fn set_layer_parent(_layerH: AEGP_LayerH, _parent_layerH0: AEGP_LayerH) -> A_Err {
	diag!("AEGP_LayerSuite/AEGP_SetLayerParent");
	unsupported("AEGP_SetLayerParent")
}

unsafe extern "C" fn delete_layer(_layerH: AEGP_LayerH) -> A_Err {
	diag!("AEGP_LayerSuite/AEGP_DeleteLayer");
	unsupported("AEGP_DeleteLayer")
}

unsafe extern "C" fn duplicate_layer(_orig_layerH: AEGP_LayerH, _duplicate_layerPH: *mut AEGP_LayerH) -> A_Err {
	diag!("AEGP_LayerSuite/AEGP_DuplicateLayer");
	unsupported("AEGP_DuplicateLayer")
}

unsafe extern "C" fn get_layer_from_layer_id(
	parent_compH: AEGP_CompH,
	id: AEGP_LayerIDVal,
	layerPH: *mut AEGP_LayerH,
) -> A_Err {
	diag!("AEGP_LayerSuite/AEGP_GetLayerFromLayerID",
		"parent_compH" => format!("{:#x}", parent_compH as usize),
		"id" => id,
	);
	if id != LAYER_ID && Timeline::of(parent_compH as _).is_some() {
		return PF_Err_INVALID_INDEX as A_Err;
	}
	answer(parent_compH as _, layerPH, |_| layer_handle(parent_compH as PF_ProgPtr))
}

/// From v6 the names come back as UTF-16 `AEGP_MemHandle`s, which need the
/// AEGP Memory Suite.
unsafe extern "C" fn get_layer_name_utf16(
	_pluginID: AEGP_PluginID,
	_layerH: AEGP_LayerH,
	_utf_layer_namePH0: *mut AEGP_MemHandle,
	_utf_source_namePH0: *mut AEGP_MemHandle,
) -> A_Err {
	diag!("AEGP_LayerSuite/AEGP_GetLayerName");
	unsupported("AEGP_GetLayerName")
}

unsafe extern "C" fn set_layer_name_utf16(_aegp_layerH: AEGP_LayerH, _new_nameZ: *const A_UTF16Char) -> A_Err {
	diag!("AEGP_LayerSuite/AEGP_SetLayerName");
	unsupported("AEGP_SetLayerName")
}

unsafe extern "C" fn get_layer_label(layerH: AEGP_LayerH, labelP: *mut AEGP_LabelID) -> A_Err {
	diag!("AEGP_LayerSuite/AEGP_GetLayerLabel", "layerH" => format!("{:#x}", layerH as usize));
	answer(layerH as _, labelP, |_| AEGP_Label_NO_LABEL as AEGP_LabelID)
}

unsafe extern "C" fn set_layer_label(_layerH: AEGP_LayerH, _label: AEGP_LabelID) -> A_Err {
	diag!("AEGP_LayerSuite/AEGP_SetLayerLabel");
	unsupported("AEGP_SetLayerLabel")
}

unsafe extern "C" fn get_layer_sampling_quality(
	layerH: AEGP_LayerH,
	qualityP: *mut AEGP_LayerSamplingQuality,
) -> A_Err {
	diag!("AEGP_LayerSuite/AEGP_GetLayerSamplingQuality", "layerH" => format!("{:#x}", layerH as usize));
	answer(layerH as _, qualityP, |_| {
		AEGP_LayerSamplingQual_BILINEAR as AEGP_LayerSamplingQuality
	})
}

unsafe extern "C" fn set_layer_sampling_quality(_layerH: AEGP_LayerH, _quality: AEGP_LayerSamplingQuality) -> A_Err {
	diag!("AEGP_LayerSuite/AEGP_SetLayerSamplingQuality");
	unsupported("AEGP_SetLayerSamplingQuality")
}

/// No track matte: a null handle.
unsafe extern "C" fn get_track_matte_layer(layerH: AEGP_LayerH, track_matte_layerPH: *mut AEGP_LayerH) -> A_Err {
	diag!("AEGP_LayerSuite/AEGP_GetTrackMatteLayer", "layerH" => format!("{:#x}", layerH as usize));
	answer(layerH as _, track_matte_layerPH, |_| std::ptr::null_mut())
}

unsafe extern "C" fn set_track_matte(
	_layerH: AEGP_LayerH,
	_track_matte_layerH0: AEGP_LayerH,
	_track_matte_type: AEGP_TrackMatte,
) -> A_Err {
	diag!("AEGP_LayerSuite/AEGP_SetTrackMatte");
	unsupported("AEGP_SetTrackMatte")
}

unsafe extern "C" fn remove_track_matte(_layerH: AEGP_LayerH) -> A_Err {
	diag!("AEGP_LayerSuite/AEGP_RemoveTrackMatte");
	unsupported("AEGP_RemoveTrackMatte")
}

/// The one layer is an AV layer, never a light.
unsafe extern "C" fn get_light_type(light_layerH: AEGP_LayerH, _light_typeP: *mut AEGP_LightType) -> A_Err {
	diag!("AEGP_LightSuite1/AEGP_GetLightType", "light_layerH" => format!("{:#x}", light_layerH as usize));
	if Timeline::of(light_layerH as _).is_none() {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	}
	A_Err_GENERIC as A_Err
}

/// Builds the `AEGP_LightSuite1` vtable.
pub(super) const fn create_light_suite_1() -> AEGP_LightSuite1 {
	AEGP_LightSuite1 {
		AEGP_GetLightType: Some(get_light_type),
	}
}

/// Builds an `AEGP_LayerSuite*` vtable: the calls every version shares, the
/// version's name getter/setter, then whatever it appends.
macro_rules! layer_suite {
	($suite:ident, $get_name:ident, $set_name:ident $(, $field:ident: $fn_name:ident)* $(,)?) => {
		$suite {
			AEGP_GetCompNumLayers: Some(get_comp_num_layers),
			AEGP_GetCompLayerByIndex: Some(get_comp_layer_by_index),
			AEGP_GetActiveLayer: Some(get_active_layer),
			AEGP_GetLayerIndex: Some(get_layer_index),
			AEGP_GetLayerSourceItem: Some(get_layer_source_item),
			AEGP_GetLayerSourceItemID: Some(get_layer_source_item_id),
			AEGP_GetLayerParentComp: Some(get_layer_parent_comp),
			AEGP_GetLayerQuality: Some(get_layer_quality),
			AEGP_SetLayerQuality: Some(set_layer_quality),
			AEGP_GetLayerFlags: Some(get_layer_flags),
			AEGP_SetLayerFlag: Some(set_layer_flag),
			AEGP_IsLayerVideoReallyOn: Some(is_layer_video_really_on),
			AEGP_IsLayerAudioReallyOn: Some(is_layer_audio_really_on),
			AEGP_GetLayerCurrentTime: Some(get_layer_current_time),
			AEGP_GetLayerInPoint: Some(get_layer_in_point),
			AEGP_GetLayerDuration: Some(get_layer_duration),
			AEGP_SetLayerInPointAndDuration: Some(set_layer_in_point_and_duration),
			AEGP_GetLayerOffset: Some(get_layer_offset),
			AEGP_SetLayerOffset: Some(set_layer_offset),
			AEGP_GetLayerStretch: Some(get_layer_stretch),
			AEGP_SetLayerStretch: Some(set_layer_stretch),
			AEGP_GetLayerTransferMode: Some(get_layer_transfer_mode),
			AEGP_SetLayerTransferMode: Some(set_layer_transfer_mode),
			AEGP_IsAddLayerValid: Some(is_add_layer_valid),
			AEGP_AddLayer: Some(add_layer),
			AEGP_ReorderLayer: Some(reorder_layer),
			AEGP_GetLayerMaskedBounds: Some(get_layer_masked_bounds),
			AEGP_GetLayerObjectType: Some(get_layer_object_type),
			AEGP_IsLayer3D: Some(is_layer_3d),
			AEGP_IsLayer2D: Some(is_layer_2d),
			AEGP_IsVideoActive: Some(is_video_active),
			AEGP_IsLayerUsedAsTrackMatte: Some(is_layer_used_as_track_matte),
			AEGP_DoesLayerHaveTrackMatte: Some(does_layer_have_track_matte),
			AEGP_ConvertCompToLayerTime: Some(convert_comp_to_layer_time),
			AEGP_ConvertLayerToCompTime: Some(convert_layer_to_comp_time),
			AEGP_GetLayerDancingRandValue: Some(get_layer_dancing_rand_value),
			AEGP_GetLayerID: Some(get_layer_id),
			AEGP_GetLayerToWorldXform: Some(get_layer_to_world_xform),
			AEGP_GetLayerToWorldXformFromView: Some(get_layer_to_world_xform_from_view),
			AEGP_GetLayerParent: Some(get_layer_parent),
			AEGP_SetLayerParent: Some(set_layer_parent),
			AEGP_DeleteLayer: Some(delete_layer),
			AEGP_DuplicateLayer: Some(duplicate_layer),
			AEGP_GetLayerFromLayerID: Some(get_layer_from_layer_id),
			AEGP_GetLayerName: Some($get_name),
			AEGP_SetLayerName: Some($set_name),
			$($field: Some($fn_name),)*
		}
	};
}

/// Wire version 11.
pub(super) const fn create_layer_suite_5() -> AEGP_LayerSuite5 {
	layer_suite!(AEGP_LayerSuite5, get_layer_name, set_layer_name)
}

/// Wire version 12: names become UTF-16 `AEGP_MemHandle`s.
pub(super) const fn create_layer_suite_6() -> AEGP_LayerSuite6 {
	layer_suite!(AEGP_LayerSuite6, get_layer_name_utf16, set_layer_name_utf16)
}

/// Wire version 13: adds labels.
pub(super) const fn create_layer_suite_7() -> AEGP_LayerSuite7 {
	layer_suite!(
		AEGP_LayerSuite7,
		get_layer_name_utf16,
		set_layer_name_utf16,
		AEGP_GetLayerLabel: get_layer_label,
		AEGP_SetLayerLabel: set_layer_label,
	)
}

/// Wire version 14: adds sampling quality.
pub(super) const fn create_layer_suite_8() -> AEGP_LayerSuite8 {
	layer_suite!(
		AEGP_LayerSuite8,
		get_layer_name_utf16,
		set_layer_name_utf16,
		AEGP_GetLayerLabel: get_layer_label,
		AEGP_SetLayerLabel: set_layer_label,
		AEGP_GetLayerSamplingQuality: get_layer_sampling_quality,
		AEGP_SetLayerSamplingQuality: set_layer_sampling_quality,
	)
}

/// Wire version 15: adds track matte layers.
pub(super) const fn create_layer_suite_9() -> AEGP_LayerSuite9 {
	layer_suite!(
		AEGP_LayerSuite9,
		get_layer_name_utf16,
		set_layer_name_utf16,
		AEGP_GetLayerLabel: get_layer_label,
		AEGP_SetLayerLabel: set_layer_label,
		AEGP_GetLayerSamplingQuality: get_layer_sampling_quality,
		AEGP_SetLayerSamplingQuality: set_layer_sampling_quality,
		AEGP_GetTrackMatteLayer: get_track_matte_layer,
		AEGP_SetTrackMatte: set_track_matte,
		AEGP_RemoveTrackMatte: remove_track_matte,
	)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn timeline() -> Timeline {
		Timeline {
			width: 1920,
			height: 1080,
			current_time: 0,
			time_step: 1,
			time_scale: 30,
		}
	}

	#[test]
	fn frames_follow_the_time_scale() {
		let t = timeline();
		assert_eq!(t.frame_of(&A_Time { value: 2, scale: 1 }), 60);
		assert_eq!(t.frame_of(&A_Time { value: -1, scale: 30 }), -1);
		assert_eq!(t.frame_of(&t.duration()), 30 * 30);
	}

	#[test]
	fn null_handles_are_rejected() {
		let suite = create_layer_suite_5();
		let mut time = A_Time { value: 7, scale: 7 };
		let err = unsafe { suite.AEGP_GetLayerDuration.unwrap()(std::ptr::null_mut(), 0, &mut time) };
		assert_eq!(err, PF_Err_BAD_CALLBACK_PARAM as A_Err);
		assert_eq!((time.value, time.scale), (7, 7));
	}
}
