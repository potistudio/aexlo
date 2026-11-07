//! `AEGP Dynamic Stream Suite`, served at wire versions 1, 2, 3 and 5
//! (`AEGP_DynamicStreamSuite1` to `4`). `kAEGPDynamicStreamSuiteVersion4` is 5,
//! so there is no wire version 4.
//!
//! aexlo has one 2D AV layer and one effect. The layer root (`ADBE AV Layer`)
//! contains the transform group, the effect parade and an empty mask parade.
//! The parade's only child is the hosted effect, and that effect's children are
//! its parameters (the same handles the Stream Suite samples). The effect
//! group's match name is parameter 0's name, which After Effects uses as the
//! effect's display name, or `ADBE Effect` when that name is empty.
//!
//! Nothing in the tree can be added, removed, renamed or separated. Those calls
//! return `A_Err_GENERIC`. A failed lookup writes a null stream ref so the
//! caller does not dispose an uninitialised pointer.

use after_effects_sys::*;

use super::stream::{Node, Stream, new_stream_handle, stream_of};
use crate::PluginInstance;
use crate::core::diagnostics::diag;
use crate::suites::effect::with_param;

const NAME_CAP: usize = AEGP_MAX_STREAM_MATCH_NAME_SIZE as usize;

fn none() -> A_Err {
	PF_Err_NONE as A_Err
}

fn bad() -> A_Err {
	PF_Err_BAD_CALLBACK_PARAM as A_Err
}

fn missing() -> A_Err {
	PF_Err_INVALID_INDEX as A_Err
}

fn refuse(name: &str) -> A_Err {
	log::warn!("AEGP_DynamicStreamSuite/{name}: aexlo's streams are fixed");
	A_Err_GENERIC as A_Err
}

fn clear_stream(out: *mut AEGP_StreamRefH) {
	if let Some(slot) = unsafe { out.as_mut() } {
		*slot = std::ptr::null_mut();
	}
}

fn param_count(effect_ref: PF_ProgPtr) -> Option<usize> {
	let instance = PluginInstance::get_instance_ptr(effect_ref)?;
	// SAFETY: a non-null effect ref that reached a suite call is the live instance.
	Some(unsafe { instance.as_ref() }.params().len())
}

fn num_children(stream: Stream) -> Option<usize> {
	Some(match stream.node {
		Node::Layer => 3,
		Node::Transform => 5,
		Node::Effects => 1,
		Node::Masks => 0,
		Node::Effect => param_count(stream.effect_ref)?,
		_ => 0,
	})
}

fn child_at(parent: Stream, index: usize) -> Option<Stream> {
	let node = match parent.node {
		Node::Layer => match index {
			0 => Node::Transform,
			1 => Node::Effects,
			2 => Node::Masks,
			_ => return None,
		},
		Node::Transform => match index {
			0 => Node::Anchor,
			1 => Node::Position,
			2 => Node::Scale,
			3 => Node::Rotation,
			4 => Node::Opacity,
			_ => return None,
		},
		Node::Effects if index == 0 => Node::Effect,
		Node::Effect => {
			let count = param_count(parent.effect_ref)?;
			if index >= count {
				return None;
			}
			return Some(Stream {
				effect_ref: parent.effect_ref,
				index,
				node: Node::Param,
			});
		}
		_ => return None,
	};
	Some(Stream {
		effect_ref: parent.effect_ref,
		index,
		node,
	})
}

fn parent_of(stream: Stream) -> Option<Stream> {
	let (node, index) = match stream.node {
		Node::Layer => return None,
		Node::Transform | Node::Effects | Node::Masks => (Node::Layer, 0),
		Node::Anchor | Node::Position | Node::Scale | Node::Rotation | Node::Opacity => (Node::Transform, 0),
		Node::Effect => (Node::Effects, 1),
		Node::Param => (Node::Effect, 0),
	};
	Some(Stream {
		effect_ref: stream.effect_ref,
		index,
		node,
	})
}

fn depth(node: Node) -> A_long {
	match node {
		Node::Layer => 0,
		Node::Transform | Node::Effects | Node::Masks => 1,
		Node::Anchor | Node::Position | Node::Scale | Node::Rotation | Node::Opacity | Node::Effect => 2,
		Node::Param => 3,
	}
}

fn grouping(node: Node) -> AEGP_StreamGroupingType {
	(match node {
		Node::Layer | Node::Transform | Node::Effect => AEGP_StreamGroupingType_NAMED_GROUP,
		Node::Effects | Node::Masks => AEGP_StreamGroupingType_INDEXED_GROUP,
		_ => AEGP_StreamGroupingType_LEAF,
	}) as AEGP_StreamGroupingType
}

fn is_leader(node: Node) -> bool {
	matches!(node, Node::Anchor | Node::Position | Node::Scale)
}

/// The match name, without a trailing NUL. At most [`NAME_CAP`] - 1 bytes.
fn name_bytes(stream: Stream) -> Vec<u8> {
	let fixed: &[u8] = match stream.node {
		Node::Layer => b"ADBE AV Layer",
		Node::Transform => b"ADBE Transform Group",
		Node::Anchor => b"ADBE Anchor Point",
		Node::Position => b"ADBE Position",
		Node::Scale => b"ADBE Scale",
		Node::Rotation => b"ADBE Rotate Z",
		Node::Opacity => b"ADBE Opacity",
		Node::Effects => b"ADBE Effect Parade",
		Node::Masks => b"ADBE Mask Parade",
		Node::Effect => {
			let from_param = param_name(stream.effect_ref, 0);
			if from_param.is_empty() {
				return b"ADBE Effect".to_vec();
			}
			return from_param;
		}
		Node::Param => return param_name(stream.effect_ref, stream.index),
	};
	fixed.to_vec()
}

fn param_name(effect_ref: PF_ProgPtr, index: usize) -> Vec<u8> {
	let Some(name) = with_param(effect_ref, index, |def| def.name_do_not_use_directly) else {
		return Vec::new();
	};
	name.iter()
		.take(NAME_CAP - 1)
		.take_while(|c| **c != 0)
		.map(|c| *c as u8)
		.collect()
}

/// Write `stream`'s match name into `dst`, which holds `cap` bytes including the NUL.
pub(crate) fn write_name(stream: Stream, dst: *mut A_char, cap: usize) -> A_Err {
	if dst.is_null() || cap == 0 {
		return bad();
	}
	let bytes = name_bytes(stream);
	let len = bytes.len().min(cap - 1);
	unsafe {
		std::ptr::copy_nonoverlapping(bytes.as_ptr() as *const A_char, dst, len);
		*dst.add(len) = 0;
	}
	none()
}

fn caller_name(ptr: *const A_char) -> Option<Vec<u8>> {
	if ptr.is_null() {
		return None;
	}
	let mut out = Vec::new();
	for i in 0..(NAME_CAP as isize) {
		let byte = unsafe { *ptr.offset(i) } as u8;
		if byte == 0 {
			return Some(out);
		}
		out.push(byte);
	}
	None
}

fn child_by_name(parent: Stream, name: *const A_char) -> Result<Stream, A_Err> {
	let wanted = caller_name(name).ok_or_else(bad)?;
	let count = num_children(parent).ok_or_else(bad)?;
	for index in 0..count {
		let Some(child) = child_at(parent, index) else {
			continue;
		};
		if name_bytes(child) == wanted {
			return Ok(child);
		}
	}
	Err(missing())
}

fn publish(out: *mut AEGP_StreamRefH, stream: Stream) -> A_Err {
	let Some(slot) = (unsafe { out.as_mut() }) else {
		return bad();
	};
	*slot = new_stream_handle(stream);
	none()
}

unsafe extern "C" fn get_new_stream_ref_for_layer(
	_aegp_plugin_id: AEGP_PluginID,
	layerH: AEGP_LayerH,
	streamPH: *mut AEGP_StreamRefH,
) -> A_Err {
	diag!("AEGP_DynamicStreamSuite/AEGP_GetNewStreamRefForLayer", "layerH" => format!("{:#x}", layerH as usize));

	if super::layer::Timeline::of(layerH as _).is_none() {
		clear_stream(streamPH);
		return bad();
	}
	publish(
		streamPH,
		Stream {
			effect_ref: layerH as PF_ProgPtr,
			index: 0,
			node: Node::Layer,
		},
	)
}

unsafe extern "C" fn get_new_stream_ref_for_mask(
	_aegp_plugin_id: AEGP_PluginID,
	_maskH: AEGP_MaskRefH,
	streamPH: *mut AEGP_StreamRefH,
) -> A_Err {
	diag!("AEGP_DynamicStreamSuite/AEGP_GetNewStreamRefForMask");
	clear_stream(streamPH);
	refuse("AEGP_GetNewStreamRefForMask")
}

unsafe extern "C" fn get_stream_depth(streamH: AEGP_StreamRefH, depthPL: *mut A_long) -> A_Err {
	diag!("AEGP_DynamicStreamSuite/AEGP_GetStreamDepth", "streamH" => format!("{:#x}", streamH as usize));
	let Some(stream) = (unsafe { stream_of(streamH) }) else {
		return bad();
	};
	let Some(out) = (unsafe { depthPL.as_mut() }) else {
		return bad();
	};
	*out = depth(stream.node);
	none()
}

unsafe extern "C" fn get_stream_grouping_type(
	streamH: AEGP_StreamRefH,
	group_typeP: *mut AEGP_StreamGroupingType,
) -> A_Err {
	diag!("AEGP_DynamicStreamSuite/AEGP_GetStreamGroupingType", "streamH" => format!("{:#x}", streamH as usize));
	let Some(stream) = (unsafe { stream_of(streamH) }) else {
		return bad();
	};
	let Some(out) = (unsafe { group_typeP.as_mut() }) else {
		return bad();
	};
	*out = grouping(stream.node);
	none()
}

unsafe extern "C" fn get_num_streams_in_group(streamH: AEGP_StreamRefH, num_streamsPL: *mut A_long) -> A_Err {
	diag!("AEGP_DynamicStreamSuite/AEGP_GetNumStreamsInGroup", "streamH" => format!("{:#x}", streamH as usize));
	let Some(stream) = (unsafe { stream_of(streamH) }) else {
		return bad();
	};
	let Some(count) = num_children(stream) else {
		return bad();
	};
	let Some(out) = (unsafe { num_streamsPL.as_mut() }) else {
		return bad();
	};
	*out = count as A_long;
	none()
}

unsafe extern "C" fn get_dynamic_stream_flags(
	streamH: AEGP_StreamRefH,
	stream_flagsP: *mut AEGP_DynStreamFlags,
) -> A_Err {
	diag!("AEGP_DynamicStreamSuite/AEGP_GetDynamicStreamFlags", "streamH" => format!("{:#x}", streamH as usize));
	if unsafe { stream_of(streamH) }.is_none() {
		return bad();
	}
	let Some(out) = (unsafe { stream_flagsP.as_mut() }) else {
		return bad();
	};
	*out = AEGP_DynStreamFlag_ACTIVE_EYEBALL as AEGP_DynStreamFlags;
	none()
}

unsafe extern "C" fn set_dynamic_stream_flag_v1(
	streamH: AEGP_StreamRefH,
	_one_flag: AEGP_DynStreamFlags,
	_setB: A_Boolean,
) -> A_Err {
	diag!("AEGP_DynamicStreamSuite/AEGP_SetDynamicStreamFlag");
	if unsafe { stream_of(streamH) }.is_none() {
		return bad();
	}
	refuse("AEGP_SetDynamicStreamFlag")
}

unsafe extern "C" fn set_dynamic_stream_flag(
	streamH: AEGP_StreamRefH,
	one_flag: AEGP_DynStreamFlags,
	_undoableB: A_Boolean,
	setB: A_Boolean,
) -> A_Err {
	unsafe { set_dynamic_stream_flag_v1(streamH, one_flag, setB) }
}

unsafe extern "C" fn get_new_stream_ref_by_index(
	_aegp_plugin_id: AEGP_PluginID,
	parent_groupH: AEGP_StreamRefH,
	indexL: A_long,
	streamPH: *mut AEGP_StreamRefH,
) -> A_Err {
	diag!("AEGP_DynamicStreamSuite/AEGP_GetNewStreamRefByIndex", "indexL" => indexL);
	let Some(parent) = (unsafe { stream_of(parent_groupH) }) else {
		clear_stream(streamPH);
		return bad();
	};
	let Ok(index) = usize::try_from(indexL) else {
		clear_stream(streamPH);
		return missing();
	};
	let Some(child) = child_at(parent, index) else {
		clear_stream(streamPH);
		return missing();
	};
	publish(streamPH, child)
}

unsafe extern "C" fn get_new_stream_ref_by_matchname(
	_aegp_plugin_id: AEGP_PluginID,
	parent_groupH: AEGP_StreamRefH,
	match_nameZ: *const A_char,
	streamPH: *mut AEGP_StreamRefH,
) -> A_Err {
	diag!("AEGP_DynamicStreamSuite/AEGP_GetNewStreamRefByMatchname");
	let Some(parent) = (unsafe { stream_of(parent_groupH) }) else {
		clear_stream(streamPH);
		return bad();
	};
	match child_by_name(parent, match_nameZ) {
		Ok(child) => publish(streamPH, child),
		Err(err) => {
			clear_stream(streamPH);
			err
		}
	}
}

unsafe extern "C" fn delete_stream(streamH: AEGP_StreamRefH) -> A_Err {
	diag!("AEGP_DynamicStreamSuite/AEGP_DeleteStream");
	if unsafe { stream_of(streamH) }.is_none() {
		return bad();
	}
	refuse("AEGP_DeleteStream")
}

unsafe extern "C" fn reorder_stream(streamH: AEGP_StreamRefH, _new_indexL: A_long) -> A_Err {
	diag!("AEGP_DynamicStreamSuite/AEGP_ReorderStream");
	if unsafe { stream_of(streamH) }.is_none() {
		return bad();
	}
	refuse("AEGP_ReorderStream")
}

unsafe extern "C" fn duplicate_stream(
	_aegp_plugin_id: AEGP_PluginID,
	streamH: AEGP_StreamRefH,
	new_indexPL0: *mut A_long,
) -> A_Err {
	diag!("AEGP_DynamicStreamSuite/AEGP_DuplicateStream");
	if let Some(out) = unsafe { new_indexPL0.as_mut() } {
		*out = 0;
	}
	if unsafe { stream_of(streamH) }.is_none() {
		return bad();
	}
	refuse("AEGP_DuplicateStream")
}

unsafe extern "C" fn set_stream_name(streamH: AEGP_StreamRefH, _nameZ: *const A_char) -> A_Err {
	diag!("AEGP_DynamicStreamSuite/AEGP_SetStreamName");
	if unsafe { stream_of(streamH) }.is_none() {
		return bad();
	}
	refuse("AEGP_SetStreamName")
}

unsafe extern "C" fn set_stream_name_utf16(streamH: AEGP_StreamRefH, _nameZ: *const A_UTF16Char) -> A_Err {
	diag!("AEGP_DynamicStreamSuite/AEGP_SetStreamName");
	if unsafe { stream_of(streamH) }.is_none() {
		return bad();
	}
	refuse("AEGP_SetStreamName")
}

unsafe extern "C" fn can_add_stream(
	group_streamH: AEGP_StreamRefH,
	_match_nameZ: *const A_char,
	can_addPB: *mut A_Boolean,
) -> A_Err {
	diag!("AEGP_DynamicStreamSuite/AEGP_CanAddStream");
	if unsafe { stream_of(group_streamH) }.is_none() {
		return bad();
	}
	let Some(out) = (unsafe { can_addPB.as_mut() }) else {
		return bad();
	};
	*out = 0;
	none()
}

unsafe extern "C" fn add_stream(
	_aegp_plugin_id: AEGP_PluginID,
	indexed_group_streamH: AEGP_StreamRefH,
	_match_nameZ: *const A_char,
	streamPH0: *mut AEGP_StreamRefH,
) -> A_Err {
	diag!("AEGP_DynamicStreamSuite/AEGP_AddStream");
	clear_stream(streamPH0);
	if unsafe { stream_of(indexed_group_streamH) }.is_none() {
		return bad();
	}
	refuse("AEGP_AddStream")
}

unsafe extern "C" fn get_match_name(streamH: AEGP_StreamRefH, nameZ: *mut A_char) -> A_Err {
	diag!("AEGP_DynamicStreamSuite/AEGP_GetMatchName", "streamH" => format!("{:#x}", streamH as usize));
	let Some(stream) = (unsafe { stream_of(streamH) }) else {
		return bad();
	};
	write_name(stream, nameZ, NAME_CAP)
}

unsafe extern "C" fn get_new_parent_stream_ref(
	_aegp_plugin_id: AEGP_PluginID,
	streamH: AEGP_StreamRefH,
	parent_streamPH: *mut AEGP_StreamRefH,
) -> A_Err {
	diag!("AEGP_DynamicStreamSuite/AEGP_GetNewParentStreamRef", "streamH" => format!("{:#x}", streamH as usize));
	let Some(stream) = (unsafe { stream_of(streamH) }) else {
		clear_stream(parent_streamPH);
		return bad();
	};
	let Some(parent) = parent_of(stream) else {
		clear_stream(parent_streamPH);
		return refuse("AEGP_GetNewParentStreamRef");
	};
	publish(parent_streamPH, parent)
}

unsafe extern "C" fn get_stream_is_modified(streamH: AEGP_StreamRefH, modifiedPB: *mut A_Boolean) -> A_Err {
	diag!("AEGP_DynamicStreamSuite/AEGP_GetStreamIsModified");
	if unsafe { stream_of(streamH) }.is_none() {
		return bad();
	}
	let Some(out) = (unsafe { modifiedPB.as_mut() }) else {
		return bad();
	};
	*out = 0;
	none()
}

unsafe extern "C" fn get_stream_index_in_parent(streamH: AEGP_StreamRefH, indexPL: *mut A_long) -> A_Err {
	diag!("AEGP_DynamicStreamSuite/AEGP_GetStreamIndexInParent");
	let Some(stream) = (unsafe { stream_of(streamH) }) else {
		return bad();
	};
	if stream.node == Node::Layer {
		return refuse("AEGP_GetStreamIndexInParent");
	}
	let Some(out) = (unsafe { indexPL.as_mut() }) else {
		return bad();
	};
	*out = stream.index as A_long;
	none()
}

unsafe extern "C" fn is_separation_leader(streamH: AEGP_StreamRefH, leaderPB: *mut A_Boolean) -> A_Err {
	diag!("AEGP_DynamicStreamSuite/AEGP_IsSeparationLeader");
	let Some(stream) = (unsafe { stream_of(streamH) }) else {
		return bad();
	};
	let Some(out) = (unsafe { leaderPB.as_mut() }) else {
		return bad();
	};
	*out = is_leader(stream.node) as A_Boolean;
	none()
}

unsafe extern "C" fn are_dimensions_separated(leader_streamH: AEGP_StreamRefH, separatedPB: *mut A_Boolean) -> A_Err {
	diag!("AEGP_DynamicStreamSuite/AEGP_AreDimensionsSeparated");
	let Some(stream) = (unsafe { stream_of(leader_streamH) }) else {
		return bad();
	};
	if !is_leader(stream.node) {
		return refuse("AEGP_AreDimensionsSeparated");
	}
	let Some(out) = (unsafe { separatedPB.as_mut() }) else {
		return bad();
	};
	*out = 0;
	none()
}

unsafe extern "C" fn set_dimensions_separated(leader_streamH: AEGP_StreamRefH, _separatedB: A_Boolean) -> A_Err {
	diag!("AEGP_DynamicStreamSuite/AEGP_SetDimensionsSeparated");
	if unsafe { stream_of(leader_streamH) }.is_none() {
		return bad();
	}
	refuse("AEGP_SetDimensionsSeparated")
}

unsafe extern "C" fn get_separation_follower(
	leader_streamH: AEGP_StreamRefH,
	_dimS: A_short,
	follower_streamPH: *mut AEGP_StreamRefH,
) -> A_Err {
	diag!("AEGP_DynamicStreamSuite/AEGP_GetSeparationFollower");
	clear_stream(follower_streamPH);
	if unsafe { stream_of(leader_streamH) }.is_none() {
		return bad();
	}
	refuse("AEGP_GetSeparationFollower")
}

unsafe extern "C" fn is_separation_follower(streamH: AEGP_StreamRefH, followerPB: *mut A_Boolean) -> A_Err {
	diag!("AEGP_DynamicStreamSuite/AEGP_IsSeparationFollower");
	if unsafe { stream_of(streamH) }.is_none() {
		return bad();
	}
	let Some(out) = (unsafe { followerPB.as_mut() }) else {
		return bad();
	};
	*out = 0;
	none()
}

unsafe extern "C" fn get_separation_leader(
	follower_streamH: AEGP_StreamRefH,
	leader_streamPH: *mut AEGP_StreamRefH,
) -> A_Err {
	diag!("AEGP_DynamicStreamSuite/AEGP_GetSeparationLeader");
	clear_stream(leader_streamPH);
	if unsafe { stream_of(follower_streamH) }.is_none() {
		return bad();
	}
	refuse("AEGP_GetSeparationLeader")
}

unsafe extern "C" fn get_separation_dimension(follower_streamH: AEGP_StreamRefH, _dimPS: *mut A_short) -> A_Err {
	diag!("AEGP_DynamicStreamSuite/AEGP_GetSeparationDimension");
	if unsafe { stream_of(follower_streamH) }.is_none() {
		return bad();
	}
	refuse("AEGP_GetSeparationDimension")
}

/// Builds the `AEGP_DynamicStreamSuite1` vtable (wire version 1).
pub(super) const fn suite_1() -> AEGP_DynamicStreamSuite1 {
	AEGP_DynamicStreamSuite1 {
		AEGP_GetNewStreamRefForLayer: Some(get_new_stream_ref_for_layer),
		AEGP_GetStreamDepth: Some(get_stream_depth),
		AEGP_GetStreamGroupingType: Some(get_stream_grouping_type),
		AEGP_GetNumStreamsInGroup: Some(get_num_streams_in_group),
		AEGP_GetDynamicStreamFlags: Some(get_dynamic_stream_flags),
		AEGP_SetDynamicStreamFlag: Some(set_dynamic_stream_flag_v1),
		AEGP_GetNewStreamRefByIndex: Some(get_new_stream_ref_by_index),
		AEGP_GetNewStreamRefByMatchname: Some(get_new_stream_ref_by_matchname),
		AEGP_DeleteStream: Some(delete_stream),
		AEGP_ReorderStream: Some(reorder_stream),
		AEGP_DuplicateStream: Some(duplicate_stream),
		AEGP_SetStreamName: Some(set_stream_name),
		AEGP_CanAddStream: Some(can_add_stream),
		AEGP_AddStream: Some(add_stream),
		AEGP_GetMatchName: Some(get_match_name),
		AEGP_GetNewParentStreamRef: Some(get_new_parent_stream_ref),
		AEGP_GetStreamIsModified: Some(get_stream_is_modified),
	}
}

/// Builds the `AEGP_DynamicStreamSuite2` vtable (wire version 2).
pub(super) const fn suite_2() -> AEGP_DynamicStreamSuite2 {
	AEGP_DynamicStreamSuite2 {
		AEGP_GetNewStreamRefForLayer: Some(get_new_stream_ref_for_layer),
		AEGP_GetStreamDepth: Some(get_stream_depth),
		AEGP_GetStreamGroupingType: Some(get_stream_grouping_type),
		AEGP_GetNumStreamsInGroup: Some(get_num_streams_in_group),
		AEGP_GetDynamicStreamFlags: Some(get_dynamic_stream_flags),
		AEGP_SetDynamicStreamFlag: Some(set_dynamic_stream_flag),
		AEGP_GetNewStreamRefByIndex: Some(get_new_stream_ref_by_index),
		AEGP_GetNewStreamRefByMatchname: Some(get_new_stream_ref_by_matchname),
		AEGP_DeleteStream: Some(delete_stream),
		AEGP_ReorderStream: Some(reorder_stream),
		AEGP_DuplicateStream: Some(duplicate_stream),
		AEGP_SetStreamName: Some(set_stream_name),
		AEGP_CanAddStream: Some(can_add_stream),
		AEGP_AddStream: Some(add_stream),
		AEGP_GetMatchName: Some(get_match_name),
		AEGP_GetNewParentStreamRef: Some(get_new_parent_stream_ref),
		AEGP_GetStreamIsModified: Some(get_stream_is_modified),
	}
}

/// Builds the `AEGP_DynamicStreamSuite3` vtable (wire version 3).
pub(super) const fn suite_3() -> AEGP_DynamicStreamSuite3 {
	AEGP_DynamicStreamSuite3 {
		AEGP_GetNewStreamRefForLayer: Some(get_new_stream_ref_for_layer),
		AEGP_GetStreamDepth: Some(get_stream_depth),
		AEGP_GetStreamGroupingType: Some(get_stream_grouping_type),
		AEGP_GetNumStreamsInGroup: Some(get_num_streams_in_group),
		AEGP_GetDynamicStreamFlags: Some(get_dynamic_stream_flags),
		AEGP_SetDynamicStreamFlag: Some(set_dynamic_stream_flag),
		AEGP_GetNewStreamRefByIndex: Some(get_new_stream_ref_by_index),
		AEGP_GetNewStreamRefByMatchname: Some(get_new_stream_ref_by_matchname),
		AEGP_DeleteStream: Some(delete_stream),
		AEGP_ReorderStream: Some(reorder_stream),
		AEGP_DuplicateStream: Some(duplicate_stream),
		AEGP_SetStreamName: Some(set_stream_name),
		AEGP_CanAddStream: Some(can_add_stream),
		AEGP_AddStream: Some(add_stream),
		AEGP_GetMatchName: Some(get_match_name),
		AEGP_GetNewParentStreamRef: Some(get_new_parent_stream_ref),
		AEGP_GetStreamIsModified: Some(get_stream_is_modified),
		AEGP_GetStreamIndexInParent: Some(get_stream_index_in_parent),
	}
}

/// Builds the `AEGP_DynamicStreamSuite4` vtable (wire version 5).
pub(super) const fn suite_4() -> AEGP_DynamicStreamSuite4 {
	AEGP_DynamicStreamSuite4 {
		AEGP_GetNewStreamRefForLayer: Some(get_new_stream_ref_for_layer),
		AEGP_GetNewStreamRefForMask: Some(get_new_stream_ref_for_mask),
		AEGP_GetStreamDepth: Some(get_stream_depth),
		AEGP_GetStreamGroupingType: Some(get_stream_grouping_type),
		AEGP_GetNumStreamsInGroup: Some(get_num_streams_in_group),
		AEGP_GetDynamicStreamFlags: Some(get_dynamic_stream_flags),
		AEGP_SetDynamicStreamFlag: Some(set_dynamic_stream_flag),
		AEGP_GetNewStreamRefByIndex: Some(get_new_stream_ref_by_index),
		AEGP_GetNewStreamRefByMatchname: Some(get_new_stream_ref_by_matchname),
		AEGP_DeleteStream: Some(delete_stream),
		AEGP_ReorderStream: Some(reorder_stream),
		AEGP_DuplicateStream: Some(duplicate_stream),
		AEGP_SetStreamName: Some(set_stream_name_utf16),
		AEGP_CanAddStream: Some(can_add_stream),
		AEGP_AddStream: Some(add_stream),
		AEGP_GetMatchName: Some(get_match_name),
		AEGP_GetNewParentStreamRef: Some(get_new_parent_stream_ref),
		AEGP_GetStreamIsModified: Some(get_stream_is_modified),
		AEGP_GetStreamIndexInParent: Some(get_stream_index_in_parent),
		AEGP_IsSeparationLeader: Some(is_separation_leader),
		AEGP_AreDimensionsSeparated: Some(are_dimensions_separated),
		AEGP_SetDimensionsSeparated: Some(set_dimensions_separated),
		AEGP_GetSeparationFollower: Some(get_separation_follower),
		AEGP_IsSeparationFollower: Some(is_separation_follower),
		AEGP_GetSeparationLeader: Some(get_separation_leader),
		AEGP_GetSeparationDimension: Some(get_separation_dimension),
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::suites::stream::create_stream_suite_4;

	fn read_name(ptr: *const A_char) -> String {
		let mut bytes = Vec::new();
		for i in 0..NAME_CAP as isize {
			let byte = unsafe { *ptr.offset(i) } as u8;
			if byte == 0 {
				break;
			}
			bytes.push(byte);
		}
		String::from_utf8_lossy(&bytes).into_owned()
	}

	fn layer() -> AEGP_StreamRefH {
		new_stream_handle(Stream {
			effect_ref: std::ptr::null_mut(),
			index: 0,
			node: Node::Layer,
		})
	}

	#[test]
	fn walks_the_effect_parade_by_match_name() {
		let suite = suite_4();
		let streams = create_stream_suite_4();
		let root = layer();
		let mut name = [1i8; NAME_CAP];
		let parade = std::ffi::CString::new("ADBE Effect Parade").unwrap();
		let mut effects: AEGP_StreamRefH = std::ptr::null_mut();
		let mut effect: AEGP_StreamRefH = std::ptr::null_mut();
		let mut missing_child: AEGP_StreamRefH = 0x1 as AEGP_StreamRefH;
		let mut count = -1;
		let mut depth_out = -1;
		unsafe {
			assert_eq!(suite.AEGP_GetMatchName.unwrap()(root, name.as_mut_ptr()), none());
			assert_eq!(read_name(name.as_ptr()), "ADBE AV Layer");
			assert_eq!(
				suite.AEGP_GetNewStreamRefByMatchname.unwrap()(0, root, parade.as_ptr(), &mut effects),
				none()
			);
			assert_eq!(suite.AEGP_GetNumStreamsInGroup.unwrap()(effects, &mut count), none());
			assert_eq!(count, 1);
			assert_eq!(suite.AEGP_GetStreamDepth.unwrap()(effects, &mut depth_out), none());
			assert_eq!(depth_out, 1);
			assert_eq!(
				suite.AEGP_GetNewStreamRefByIndex.unwrap()(0, effects, 0, &mut effect),
				none()
			);
			assert_eq!(suite.AEGP_GetMatchName.unwrap()(effect, name.as_mut_ptr()), none());
			assert_eq!(read_name(name.as_ptr()), "ADBE Effect");
			assert_ne!(suite.AEGP_GetNumStreamsInGroup.unwrap()(effect, &mut count), none());
			assert_ne!(
				suite.AEGP_GetNewStreamRefByMatchname.unwrap()(
					0,
					root,
					std::ffi::CString::new("no such stream").unwrap().as_ptr(),
					&mut missing_child
				),
				none()
			);
			assert!(missing_child.is_null());
			assert_eq!(
				suite.AEGP_GetNewStreamRefForLayer.unwrap()(0, std::ptr::null_mut(), &mut missing_child),
				bad()
			);
			for handle in [root, effects, effect] {
				assert_eq!(streams.AEGP_DisposeStream.unwrap()(handle), none());
			}
		}
	}

	#[test]
	fn position_is_a_leader_whose_dimensions_stay_together() {
		let suite = suite_4();
		let streams = create_stream_suite_4();
		let root = layer();
		let mut transform: AEGP_StreamRefH = std::ptr::null_mut();
		let mut position: AEGP_StreamRefH = std::ptr::null_mut();
		let mut leader = 7;
		let mut separated = 7;
		let mut follower = 7;
		unsafe {
			assert_eq!(
				suite.AEGP_GetNewStreamRefByMatchname.unwrap()(
					0,
					root,
					std::ffi::CString::new("ADBE Transform Group").unwrap().as_ptr(),
					&mut transform,
				),
				none()
			);
			assert_eq!(
				suite.AEGP_GetNewStreamRefByIndex.unwrap()(0, transform, 1, &mut position),
				none()
			);
			assert_eq!(suite.AEGP_IsSeparationLeader.unwrap()(position, &mut leader), none());
			assert_eq!(leader, 1);
			assert_eq!(
				suite.AEGP_AreDimensionsSeparated.unwrap()(position, &mut separated),
				none()
			);
			assert_eq!(separated, 0);
			assert_ne!(suite.AEGP_SetDimensionsSeparated.unwrap()(position, 1), none());
			assert_eq!(
				suite.AEGP_IsSeparationFollower.unwrap()(position, &mut follower),
				none()
			);
			assert_eq!(follower, 0);
			for handle in [root, transform, position] {
				assert_eq!(streams.AEGP_DisposeStream.unwrap()(handle), none());
			}
		}
	}
}
