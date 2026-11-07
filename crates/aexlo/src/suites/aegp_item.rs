//! `AEGP Item Suite`, wire versions 10 (`AEGP_ItemSuite6`) and 13
//! (`AEGP_ItemSuite8`). The two are not the same layout: version 10 names an
//! item with a fixed `char` buffer, while version 13 takes a plugin id and
//! returns a UTF-16 handle.
//!
//! aexlo's project has one item, and it is the comp (the same pointer as the
//! layer and the comp; see [`layer`](super::layer)). Reads report that comp.
//! Creating, deleting, renaming or moving an item is refused.

use after_effects_sys::*;

use super::aegp_memory::handle_from_bytes;
use super::layer::{Timeline, current_effect, item_handle};
use crate::core::diagnostics::diag;

const ITEM_NAME: &str = "aexlo";
const COMP_TYPE_NAME: &[u8] = b"Composition\0";

fn refuse(name: &str) -> A_Err {
	log::warn!("AEGP_ItemSuite8/{name}: aexlo's project has one fixed comp");
	A_Err_GENERIC as A_Err
}

fn known(itemH: AEGP_ItemH) -> bool {
	Timeline::of(itemH as _).is_some()
}

unsafe extern "C" fn get_first_proj_item(projectH: AEGP_ProjectH, itemPH: *mut AEGP_ItemH) -> A_Err {
	diag!("AEGP_ItemSuite8/AEGP_GetFirstProjItem");
	let Some(out) = (unsafe { itemPH.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	let effect = if known(projectH as _) {
		projectH as PF_ProgPtr
	} else {
		current_effect()
	};
	if !known(effect as _) {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	}
	*out = item_handle(effect);
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn get_next_proj_item(
	_projectH: AEGP_ProjectH,
	itemH: AEGP_ItemH,
	next_itemPH: *mut AEGP_ItemH,
) -> A_Err {
	diag!("AEGP_ItemSuite8/AEGP_GetNextProjItem");
	let Some(out) = (unsafe { next_itemPH.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	if !known(itemH) {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	}
	*out = std::ptr::null_mut();
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn get_active_item(itemPH: *mut AEGP_ItemH) -> A_Err {
	diag!("AEGP_ItemSuite8/AEGP_GetActiveItem");
	let Some(out) = (unsafe { itemPH.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	let effect = current_effect();
	if !known(effect as _) {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	}
	*out = item_handle(effect);
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn is_item_selected(itemH: AEGP_ItemH, selectedPB: *mut A_Boolean) -> A_Err {
	diag!("AEGP_ItemSuite8/AEGP_IsItemSelected");
	let Some(out) = (unsafe { selectedPB.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	if !known(itemH) {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	}
	*out = 0;
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn select_item(_itemH: AEGP_ItemH, _selectB: A_Boolean, _deselect_othersB: A_Boolean) -> A_Err {
	diag!("AEGP_ItemSuite8/AEGP_SelectItem");
	refuse("AEGP_SelectItem")
}

unsafe extern "C" fn get_item_type(itemH: AEGP_ItemH, item_typeP: *mut AEGP_ItemType) -> A_Err {
	diag!("AEGP_ItemSuite8/AEGP_GetItemType");
	let Some(out) = (unsafe { item_typeP.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	if !known(itemH) {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	}
	*out = AEGP_ItemType_COMP as AEGP_ItemType;
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn get_type_name(item_type: AEGP_ItemType, nameZ: *mut A_char) -> A_Err {
	diag!("AEGP_ItemSuite8/AEGP_GetTypeName");
	if nameZ.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	}
	let name: &[u8] = if item_type == AEGP_ItemType_COMP as AEGP_ItemType {
		COMP_TYPE_NAME
	} else {
		b"\0"
	};
	unsafe { std::ptr::copy_nonoverlapping(name.as_ptr().cast(), nameZ, name.len()) };
	PF_Err_NONE as A_Err
}

/// `AEGP_ItemSuite6::AEGP_GetItemName` writes into a `AEGP_MAX_ITEM_NAME_SIZE`
/// char buffer. Suite 8 replaced this with a UTF-16 handle.
unsafe extern "C" fn get_item_name_char(itemH: AEGP_ItemH, nameZ: *mut A_char) -> A_Err {
	diag!("AEGP_ItemSuite6/AEGP_GetItemName");
	if nameZ.is_null() || !known(itemH) {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	}
	let mut bytes = [0u8; AEGP_MAX_ITEM_NAME_SIZE as usize];
	let name = ITEM_NAME.as_bytes();
	bytes[..name.len()].copy_from_slice(name);
	unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr().cast(), nameZ, bytes.len()) };
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn set_item_name_char(_itemH: AEGP_ItemH, _nameZ: *const A_char) -> A_Err {
	diag!("AEGP_ItemSuite6/AEGP_SetItemName");
	refuse("AEGP_SetItemName")
}

unsafe extern "C" fn create_new_folder_char(
	_nameZ: *const A_char,
	_parent_folderH0: AEGP_ItemH,
	_new_folderPH: *mut AEGP_ItemH,
) -> A_Err {
	diag!("AEGP_ItemSuite6/AEGP_CreateNewFolder");
	refuse("AEGP_CreateNewFolder")
}

unsafe extern "C" fn get_item_name(
	_plugin_id: AEGP_PluginID,
	itemH: AEGP_ItemH,
	unicode_namePH: *mut AEGP_MemHandle,
) -> A_Err {
	diag!("AEGP_ItemSuite8/AEGP_GetItemName");
	let Some(out) = (unsafe { unicode_namePH.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	if !known(itemH) {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	}
	let mut bytes = Vec::with_capacity((ITEM_NAME.len() + 1) * 2);
	for unit in ITEM_NAME.encode_utf16().chain(std::iter::once(0)) {
		bytes.extend_from_slice(&unit.to_le_bytes());
	}
	*out = handle_from_bytes(bytes);
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn set_item_name(_itemH: AEGP_ItemH, _nameZ: *const A_UTF16Char) -> A_Err {
	diag!("AEGP_ItemSuite8/AEGP_SetItemName");
	refuse("AEGP_SetItemName")
}

unsafe extern "C" fn get_item_id(itemH: AEGP_ItemH, item_idPL: *mut A_long) -> A_Err {
	diag!("AEGP_ItemSuite8/AEGP_GetItemID");
	let Some(out) = (unsafe { item_idPL.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	if !known(itemH) {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	}
	*out = 1;
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn get_item_flags(itemH: AEGP_ItemH, item_flagsP: *mut AEGP_ItemFlags) -> A_Err {
	diag!("AEGP_ItemSuite8/AEGP_GetItemFlags");
	let Some(out) = (unsafe { item_flagsP.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	if !known(itemH) {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	}
	*out = AEGP_ItemFlag_HAS_VIDEO as AEGP_ItemFlags;
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn set_item_use_proxy(_itemH: AEGP_ItemH, _use_proxyB: A_Boolean) -> A_Err {
	diag!("AEGP_ItemSuite8/AEGP_SetItemUseProxy");
	refuse("AEGP_SetItemUseProxy")
}

unsafe extern "C" fn get_item_parent_folder(itemH: AEGP_ItemH, parent_folder_itemPH: *mut AEGP_ItemH) -> A_Err {
	diag!("AEGP_ItemSuite8/AEGP_GetItemParentFolder");
	let Some(out) = (unsafe { parent_folder_itemPH.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	if !known(itemH) {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	}
	*out = std::ptr::null_mut();
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn set_item_parent_folder(_itemH: AEGP_ItemH, _parent_folder_itemH: AEGP_ItemH) -> A_Err {
	diag!("AEGP_ItemSuite8/AEGP_SetItemParentFolder");
	refuse("AEGP_SetItemParentFolder")
}

unsafe extern "C" fn get_item_duration(itemH: AEGP_ItemH, durationPT: *mut A_Time) -> A_Err {
	diag!("AEGP_ItemSuite8/AEGP_GetItemDuration");
	let (Some(timeline), Some(out)) = (Timeline::of(itemH as _), unsafe { durationPT.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	*out = timeline.duration();
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn get_item_current_time(itemH: AEGP_ItemH, curr_timePT: *mut A_Time) -> A_Err {
	diag!("AEGP_ItemSuite8/AEGP_GetItemCurrentTime");
	let (Some(timeline), Some(out)) = (Timeline::of(itemH as _), unsafe { curr_timePT.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	*out = timeline.current();
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn get_item_dimensions(itemH: AEGP_ItemH, widthPL: *mut A_long, heightPL: *mut A_long) -> A_Err {
	diag!("AEGP_ItemSuite8/AEGP_GetItemDimensions");
	let Some(timeline) = Timeline::of(itemH as _) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	if let Some(width) = unsafe { widthPL.as_mut() } {
		*width = timeline.width as A_long;
	}
	if let Some(height) = unsafe { heightPL.as_mut() } {
		*height = timeline.height as A_long;
	}
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn get_item_pixel_aspect_ratio(itemH: AEGP_ItemH, pix_aspect_ratioPRt: *mut A_Ratio) -> A_Err {
	diag!("AEGP_ItemSuite8/AEGP_GetItemPixelAspectRatio");
	let Some(out) = (unsafe { pix_aspect_ratioPRt.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	if !known(itemH) {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	}
	*out = A_Ratio { num: 1, den: 1 };
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn delete_item(_itemH: AEGP_ItemH) -> A_Err {
	diag!("AEGP_ItemSuite8/AEGP_DeleteItem");
	refuse("AEGP_DeleteItem")
}

unsafe extern "C" fn create_new_folder(
	_nameZ: *const A_UTF16Char,
	_parent_folderH0: AEGP_ItemH,
	_new_folderPH: *mut AEGP_ItemH,
) -> A_Err {
	diag!("AEGP_ItemSuite8/AEGP_CreateNewFolder");
	refuse("AEGP_CreateNewFolder")
}

unsafe extern "C" fn set_item_current_time(_itemH: AEGP_ItemH, _new_timePT: *const A_Time) -> A_Err {
	diag!("AEGP_ItemSuite8/AEGP_SetItemCurrentTime");
	refuse("AEGP_SetItemCurrentTime")
}

unsafe extern "C" fn get_item_comment_length(itemH: AEGP_ItemH, buf_sizePLu: *mut A_u_long) -> A_Err {
	diag!("AEGP_ItemSuite8/AEGP_GetItemCommentLength");
	let Some(out) = (unsafe { buf_sizePLu.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	if !known(itemH) {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	}
	*out = 1;
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn get_item_comment(itemH: AEGP_ItemH, buf_sizeLu: A_u_long, commentZ: *mut A_char) -> A_Err {
	diag!("AEGP_ItemSuite8/AEGP_GetItemComment");
	if commentZ.is_null() || !known(itemH) {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	}
	if buf_sizeLu > 0 {
		unsafe { *commentZ = 0 };
	}
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn set_item_comment(_itemH: AEGP_ItemH, _commentZ: *const A_char) -> A_Err {
	diag!("AEGP_ItemSuite8/AEGP_SetItemComment");
	refuse("AEGP_SetItemComment")
}

unsafe extern "C" fn get_item_label(itemH: AEGP_ItemH, labelP: *mut AEGP_LabelID) -> A_Err {
	diag!("AEGP_ItemSuite8/AEGP_GetItemLabel");
	let Some(out) = (unsafe { labelP.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	if !known(itemH) {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	}
	*out = 0;
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn set_item_label(_itemH: AEGP_ItemH, _label: AEGP_LabelID) -> A_Err {
	diag!("AEGP_ItemSuite8/AEGP_SetItemLabel");
	refuse("AEGP_SetItemLabel")
}

unsafe extern "C" fn get_item_mru_view(itemH: AEGP_ItemH, mru_viewP: *mut AEGP_ItemViewP) -> A_Err {
	diag!("AEGP_ItemSuite8/AEGP_GetItemMRUView");
	let Some(out) = (unsafe { mru_viewP.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	if !known(itemH) {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	}
	*out = std::ptr::null_mut();
	PF_Err_NONE as A_Err
}

pub const fn suite_6() -> AEGP_ItemSuite6 {
	AEGP_ItemSuite6 {
		AEGP_GetFirstProjItem: Some(get_first_proj_item),
		AEGP_GetNextProjItem: Some(get_next_proj_item),
		AEGP_GetActiveItem: Some(get_active_item),
		AEGP_IsItemSelected: Some(is_item_selected),
		AEGP_SelectItem: Some(select_item),
		AEGP_GetItemType: Some(get_item_type),
		AEGP_GetTypeName: Some(get_type_name),
		AEGP_GetItemName: Some(get_item_name_char),
		AEGP_SetItemName: Some(set_item_name_char),
		AEGP_GetItemID: Some(get_item_id),
		AEGP_GetItemFlags: Some(get_item_flags),
		AEGP_SetItemUseProxy: Some(set_item_use_proxy),
		AEGP_GetItemParentFolder: Some(get_item_parent_folder),
		AEGP_SetItemParentFolder: Some(set_item_parent_folder),
		AEGP_GetItemDuration: Some(get_item_duration),
		AEGP_GetItemCurrentTime: Some(get_item_current_time),
		AEGP_GetItemDimensions: Some(get_item_dimensions),
		AEGP_GetItemPixelAspectRatio: Some(get_item_pixel_aspect_ratio),
		AEGP_DeleteItem: Some(delete_item),
		AEGP_CreateNewFolder: Some(create_new_folder_char),
		AEGP_SetItemCurrentTime: Some(set_item_current_time),
		AEGP_GetItemCommentLength: Some(get_item_comment_length),
		AEGP_GetItemComment: Some(get_item_comment),
		AEGP_SetItemComment: Some(set_item_comment),
		AEGP_GetItemLabel: Some(get_item_label),
		AEGP_SetItemLabel: Some(set_item_label),
	}
}

pub const fn suite_8() -> AEGP_ItemSuite8 {
	AEGP_ItemSuite8 {
		AEGP_GetFirstProjItem: Some(get_first_proj_item),
		AEGP_GetNextProjItem: Some(get_next_proj_item),
		AEGP_GetActiveItem: Some(get_active_item),
		AEGP_IsItemSelected: Some(is_item_selected),
		AEGP_SelectItem: Some(select_item),
		AEGP_GetItemType: Some(get_item_type),
		AEGP_GetTypeName: Some(get_type_name),
		AEGP_GetItemName: Some(get_item_name),
		AEGP_SetItemName: Some(set_item_name),
		AEGP_GetItemID: Some(get_item_id),
		AEGP_GetItemFlags: Some(get_item_flags),
		AEGP_SetItemUseProxy: Some(set_item_use_proxy),
		AEGP_GetItemParentFolder: Some(get_item_parent_folder),
		AEGP_SetItemParentFolder: Some(set_item_parent_folder),
		AEGP_GetItemDuration: Some(get_item_duration),
		AEGP_GetItemCurrentTime: Some(get_item_current_time),
		AEGP_GetItemDimensions: Some(get_item_dimensions),
		AEGP_GetItemPixelAspectRatio: Some(get_item_pixel_aspect_ratio),
		AEGP_DeleteItem: Some(delete_item),
		AEGP_CreateNewFolder: Some(create_new_folder),
		AEGP_SetItemCurrentTime: Some(set_item_current_time),
		AEGP_GetItemCommentLength: Some(get_item_comment_length),
		AEGP_GetItemComment: Some(get_item_comment),
		AEGP_SetItemComment: Some(set_item_comment),
		AEGP_GetItemLabel: Some(get_item_label),
		AEGP_SetItemLabel: Some(set_item_label),
		AEGP_GetItemMRUView: Some(get_item_mru_view),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	#[test]
	fn legacy_suite_names_a_composition() {
		let suite = suite_6();
		let mut name = [1u8; AEGP_MAX_ITEM_NAME_SIZE as usize];
		let err =
			unsafe { suite.AEGP_GetTypeName.unwrap()(AEGP_ItemType_COMP as AEGP_ItemType, name.as_mut_ptr().cast()) };
		assert_eq!(err, PF_Err_NONE as A_Err);
		assert!(name.starts_with(b"Composition\0"));
	}

	#[test]
	fn comp_type_name_is_composition() {
		let suite = suite_8();
		let mut name = [1u8; AEGP_MAX_ITEM_NAME_SIZE as usize];
		let err =
			unsafe { suite.AEGP_GetTypeName.unwrap()(AEGP_ItemType_COMP as AEGP_ItemType, name.as_mut_ptr().cast()) };
		assert_eq!(err, PF_Err_NONE as A_Err);
		assert!(name.starts_with(b"Composition\0"));
	}

	#[test]
	fn null_item_has_no_type() {
		let suite = suite_8();
		let mut kind: AEGP_ItemType = 0;
		let err = unsafe { suite.AEGP_GetItemType.unwrap()(std::ptr::null_mut(), &mut kind) };
		assert_eq!(err, PF_Err_BAD_CALLBACK_PARAM as A_Err);
	}

	#[test]
	fn active_item_needs_a_current_effect() {
		let suite = suite_8();
		let mut item: AEGP_ItemH = 0x1 as AEGP_ItemH;
		let err = unsafe { suite.AEGP_GetActiveItem.unwrap()(&mut item) };
		assert_eq!(err, PF_Err_BAD_CALLBACK_PARAM as A_Err);
	}
}
