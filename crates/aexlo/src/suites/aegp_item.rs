//! `AEGP Item Suite`, wire versions 10 (`AEGP_ItemSuite6`), 13
//! (`AEGP_ItemSuite8`) and 14 (`AEGP_ItemSuite9`). Version 10 names an item
//! with a fixed `char` buffer, while version 13 takes a plugin id and returns
//! a UTF-16 handle. Version 14 drops the comment-length call and reads the
//! comment as a UTF-16 handle.
//!
//! aexlo's project has a comp and a synthetic footage item for its input layer.
//! Creating, deleting, renaming or moving an item is refused.

use crate::sys::*;

use super::aegp_memory::handle_from_bytes;
use super::layer::{Timeline, current_effect, item_handle, source_item_effect, source_item_handle};
use crate::core::diagnostics::diag;

const ITEM_NAME: &str = "aexlo";
const SOURCE_NAME: &str = "Input";
const COMP_TYPE_NAME: &[u8] = b"Composition\0";

fn refuse(name: &str) -> A_Err {
	log::warn!("AEGP_ItemSuite8/{name}: aexlo's project has one fixed comp");
	A_Err_GENERIC
}

fn known(itemH: AEGP_ItemH) -> bool {
	Timeline::of_item(itemH).is_some()
}

unsafe extern "C" fn get_first_proj_item(projectH: AEGP_ProjectH, itemPH: *mut AEGP_ItemH) -> A_Err {
	crate::core::ffi::guard("get_first_proj_item", || {
		diag!("AEGP_ItemSuite8/AEGP_GetFirstProjItem");
		let Some(out) = (unsafe { itemPH.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		let effect = if known(projectH as _) {
			projectH as PF_ProgPtr
		} else {
			current_effect()
		};
		if !known(effect as _) {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		*out = item_handle(effect);
		PF_Err_NONE
	})
}

unsafe extern "C" fn get_next_proj_item(
	_projectH: AEGP_ProjectH,
	itemH: AEGP_ItemH,
	next_itemPH: *mut AEGP_ItemH,
) -> A_Err {
	crate::core::ffi::guard("get_next_proj_item", || {
		diag!("AEGP_ItemSuite8/AEGP_GetNextProjItem");
		let Some(out) = (unsafe { next_itemPH.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		if !known(itemH) {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		*out = if source_item_effect(itemH).is_some() {
			std::ptr::null_mut()
		} else {
			source_item_handle(itemH as PF_ProgPtr)
		};
		PF_Err_NONE
	})
}

unsafe extern "C" fn get_active_item(itemPH: *mut AEGP_ItemH) -> A_Err {
	crate::core::ffi::guard("get_active_item", || {
		diag!("AEGP_ItemSuite8/AEGP_GetActiveItem");
		let Some(out) = (unsafe { itemPH.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		let effect = current_effect();
		if !known(effect as _) {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		*out = item_handle(effect);
		PF_Err_NONE
	})
}

unsafe extern "C" fn is_item_selected(itemH: AEGP_ItemH, selectedPB: *mut A_Boolean) -> A_Err {
	crate::core::ffi::guard("is_item_selected", || {
		diag!("AEGP_ItemSuite8/AEGP_IsItemSelected");
		let Some(out) = (unsafe { selectedPB.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		if !known(itemH) {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		*out = 0;
		PF_Err_NONE
	})
}

unsafe extern "C" fn select_item(_itemH: AEGP_ItemH, _selectB: A_Boolean, _deselect_othersB: A_Boolean) -> A_Err {
	crate::core::ffi::guard("select_item", || {
		diag!("AEGP_ItemSuite8/AEGP_SelectItem");
		refuse("AEGP_SelectItem")
	})
}

unsafe extern "C" fn get_item_type(itemH: AEGP_ItemH, item_typeP: *mut AEGP_ItemType) -> A_Err {
	crate::core::ffi::guard("get_item_type", || {
		diag!("AEGP_ItemSuite8/AEGP_GetItemType");
		let Some(out) = (unsafe { item_typeP.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		if !known(itemH) {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		*out = if source_item_effect(itemH).is_some() {
			AEGP_ItemType_FOOTAGE as AEGP_ItemType
		} else {
			AEGP_ItemType_COMP
		};
		PF_Err_NONE
	})
}

unsafe extern "C" fn get_type_name(item_type: AEGP_ItemType, nameZ: *mut A_char) -> A_Err {
	crate::core::ffi::guard("get_type_name", || {
		diag!("AEGP_ItemSuite8/AEGP_GetTypeName");
		if nameZ.is_null() {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		let name: &[u8] = if item_type == AEGP_ItemType_COMP {
			COMP_TYPE_NAME
		} else if item_type == AEGP_ItemType_FOOTAGE as AEGP_ItemType {
			b"Footage\0"
		} else {
			b"\0"
		};
		unsafe { std::ptr::copy_nonoverlapping(name.as_ptr().cast(), nameZ, name.len()) };
		PF_Err_NONE
	})
}

/// `AEGP_ItemSuite6::AEGP_GetItemName` writes into a `AEGP_MAX_ITEM_NAME_SIZE`
/// char buffer. Suite 8 replaced this with a UTF-16 handle.
unsafe extern "C" fn get_item_name_char(itemH: AEGP_ItemH, nameZ: *mut A_char) -> A_Err {
	crate::core::ffi::guard("get_item_name_char", || {
		diag!("AEGP_ItemSuite6/AEGP_GetItemName");
		if nameZ.is_null() || !known(itemH) {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		let mut bytes = [0u8; AEGP_MAX_ITEM_NAME_SIZE as usize];
		let name = if source_item_effect(itemH).is_some() {
			SOURCE_NAME
		} else {
			ITEM_NAME
		}
		.as_bytes();
		bytes[..name.len()].copy_from_slice(name);
		unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr().cast(), nameZ, bytes.len()) };
		PF_Err_NONE
	})
}

unsafe extern "C" fn set_item_name_char(_itemH: AEGP_ItemH, _nameZ: *const A_char) -> A_Err {
	crate::core::ffi::guard("set_item_name_char", || {
		diag!("AEGP_ItemSuite6/AEGP_SetItemName");
		refuse("AEGP_SetItemName")
	})
}

unsafe extern "C" fn create_new_folder_char(
	_nameZ: *const A_char,
	_parent_folderH0: AEGP_ItemH,
	_new_folderPH: *mut AEGP_ItemH,
) -> A_Err {
	crate::core::ffi::guard("create_new_folder_char", || {
		diag!("AEGP_ItemSuite6/AEGP_CreateNewFolder");
		refuse("AEGP_CreateNewFolder")
	})
}

unsafe extern "C" fn get_item_name(
	_plugin_id: AEGP_PluginID,
	itemH: AEGP_ItemH,
	unicode_namePH: *mut AEGP_MemHandle,
) -> A_Err {
	crate::core::ffi::guard("get_item_name", || {
		diag!("AEGP_ItemSuite8/AEGP_GetItemName");
		let Some(out) = (unsafe { unicode_namePH.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		if !known(itemH) {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		let name = if source_item_effect(itemH).is_some() {
			SOURCE_NAME
		} else {
			ITEM_NAME
		};
		let mut bytes = Vec::with_capacity((name.len() + 1) * 2);
		for unit in name.encode_utf16().chain(std::iter::once(0)) {
			bytes.extend_from_slice(&unit.to_le_bytes());
		}
		*out = handle_from_bytes(bytes);
		PF_Err_NONE
	})
}

unsafe extern "C" fn set_item_name(_itemH: AEGP_ItemH, _nameZ: *const A_UTF16Char) -> A_Err {
	crate::core::ffi::guard("set_item_name", || {
		diag!("AEGP_ItemSuite8/AEGP_SetItemName");
		refuse("AEGP_SetItemName")
	})
}

unsafe extern "C" fn get_item_id(itemH: AEGP_ItemH, item_idPL: *mut A_long) -> A_Err {
	crate::core::ffi::guard("get_item_id", || {
		diag!("AEGP_ItemSuite8/AEGP_GetItemID");
		let Some(out) = (unsafe { item_idPL.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		if !known(itemH) {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		*out = if source_item_effect(itemH).is_some() { 2 } else { 1 };
		PF_Err_NONE
	})
}

unsafe extern "C" fn get_item_flags(itemH: AEGP_ItemH, item_flagsP: *mut AEGP_ItemFlags) -> A_Err {
	crate::core::ffi::guard("get_item_flags", || {
		diag!("AEGP_ItemSuite8/AEGP_GetItemFlags");
		let Some(out) = (unsafe { item_flagsP.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		if !known(itemH) {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		*out = AEGP_ItemFlag_HAS_VIDEO;
		PF_Err_NONE
	})
}

unsafe extern "C" fn set_item_use_proxy(_itemH: AEGP_ItemH, _use_proxyB: A_Boolean) -> A_Err {
	crate::core::ffi::guard("set_item_use_proxy", || {
		diag!("AEGP_ItemSuite8/AEGP_SetItemUseProxy");
		refuse("AEGP_SetItemUseProxy")
	})
}

unsafe extern "C" fn get_item_parent_folder(itemH: AEGP_ItemH, parent_folder_itemPH: *mut AEGP_ItemH) -> A_Err {
	crate::core::ffi::guard("get_item_parent_folder", || {
		diag!("AEGP_ItemSuite8/AEGP_GetItemParentFolder");
		let Some(out) = (unsafe { parent_folder_itemPH.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		if !known(itemH) {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		*out = std::ptr::null_mut();
		PF_Err_NONE
	})
}

unsafe extern "C" fn set_item_parent_folder(_itemH: AEGP_ItemH, _parent_folder_itemH: AEGP_ItemH) -> A_Err {
	crate::core::ffi::guard("set_item_parent_folder", || {
		diag!("AEGP_ItemSuite8/AEGP_SetItemParentFolder");
		refuse("AEGP_SetItemParentFolder")
	})
}

unsafe extern "C" fn get_item_duration(itemH: AEGP_ItemH, durationPT: *mut A_Time) -> A_Err {
	crate::core::ffi::guard("get_item_duration", || {
		diag!("AEGP_ItemSuite8/AEGP_GetItemDuration");
		let (Some(timeline), Some(out)) = (Timeline::of_item(itemH), unsafe { durationPT.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		*out = timeline.duration();
		PF_Err_NONE
	})
}

unsafe extern "C" fn get_item_current_time(itemH: AEGP_ItemH, curr_timePT: *mut A_Time) -> A_Err {
	crate::core::ffi::guard("get_item_current_time", || {
		diag!("AEGP_ItemSuite8/AEGP_GetItemCurrentTime");
		let (Some(timeline), Some(out)) = (Timeline::of_item(itemH), unsafe { curr_timePT.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		*out = timeline.current();
		PF_Err_NONE
	})
}

unsafe extern "C" fn get_item_dimensions(itemH: AEGP_ItemH, widthPL: *mut A_long, heightPL: *mut A_long) -> A_Err {
	crate::core::ffi::guard("get_item_dimensions", || {
		diag!("AEGP_ItemSuite8/AEGP_GetItemDimensions");
		let Some(timeline) = Timeline::of_item(itemH) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		if let Some(width) = unsafe { widthPL.as_mut() } {
			*width = timeline.width as A_long;
		}
		if let Some(height) = unsafe { heightPL.as_mut() } {
			*height = timeline.height as A_long;
		}
		PF_Err_NONE
	})
}

unsafe extern "C" fn get_item_pixel_aspect_ratio(itemH: AEGP_ItemH, pix_aspect_ratioPRt: *mut A_Ratio) -> A_Err {
	crate::core::ffi::guard("get_item_pixel_aspect_ratio", || {
		diag!("AEGP_ItemSuite8/AEGP_GetItemPixelAspectRatio");
		let Some(out) = (unsafe { pix_aspect_ratioPRt.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		if !known(itemH) {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		*out = A_Ratio { num: 1, den: 1 };
		PF_Err_NONE
	})
}

unsafe extern "C" fn delete_item(_itemH: AEGP_ItemH) -> A_Err {
	crate::core::ffi::guard("delete_item", || {
		diag!("AEGP_ItemSuite8/AEGP_DeleteItem");
		refuse("AEGP_DeleteItem")
	})
}

unsafe extern "C" fn create_new_folder(
	_nameZ: *const A_UTF16Char,
	_parent_folderH0: AEGP_ItemH,
	_new_folderPH: *mut AEGP_ItemH,
) -> A_Err {
	crate::core::ffi::guard("create_new_folder", || {
		diag!("AEGP_ItemSuite8/AEGP_CreateNewFolder");
		refuse("AEGP_CreateNewFolder")
	})
}

unsafe extern "C" fn set_item_current_time(_itemH: AEGP_ItemH, _new_timePT: *const A_Time) -> A_Err {
	crate::core::ffi::guard("set_item_current_time", || {
		diag!("AEGP_ItemSuite8/AEGP_SetItemCurrentTime");
		refuse("AEGP_SetItemCurrentTime")
	})
}

unsafe extern "C" fn get_item_comment_length(itemH: AEGP_ItemH, buf_sizePLu: *mut A_u_long) -> A_Err {
	crate::core::ffi::guard("get_item_comment_length", || {
		diag!("AEGP_ItemSuite8/AEGP_GetItemCommentLength");
		let Some(out) = (unsafe { buf_sizePLu.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		if !known(itemH) {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		*out = 1;
		PF_Err_NONE
	})
}

unsafe extern "C" fn get_item_comment(itemH: AEGP_ItemH, buf_sizeLu: A_u_long, commentZ: *mut A_char) -> A_Err {
	crate::core::ffi::guard("get_item_comment", || {
		diag!("AEGP_ItemSuite8/AEGP_GetItemComment");
		if commentZ.is_null() || !known(itemH) {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		if buf_sizeLu > 0 {
			unsafe { *commentZ = 0 };
		}
		PF_Err_NONE
	})
}

unsafe extern "C" fn set_item_comment(_itemH: AEGP_ItemH, _commentZ: *const A_char) -> A_Err {
	crate::core::ffi::guard("set_item_comment", || {
		diag!("AEGP_ItemSuite8/AEGP_SetItemComment");
		refuse("AEGP_SetItemComment")
	})
}

unsafe extern "C" fn get_item_label(itemH: AEGP_ItemH, labelP: *mut AEGP_LabelID) -> A_Err {
	crate::core::ffi::guard("get_item_label", || {
		diag!("AEGP_ItemSuite8/AEGP_GetItemLabel");
		let Some(out) = (unsafe { labelP.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		if !known(itemH) {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		*out = 0;
		PF_Err_NONE
	})
}

unsafe extern "C" fn set_item_label(_itemH: AEGP_ItemH, _label: AEGP_LabelID) -> A_Err {
	crate::core::ffi::guard("set_item_label", || {
		diag!("AEGP_ItemSuite8/AEGP_SetItemLabel");
		refuse("AEGP_SetItemLabel")
	})
}

unsafe extern "C" fn get_item_comment_utf16(itemH: AEGP_ItemH, commentPH: *mut AEGP_MemHandle) -> A_Err {
	diag!("AEGP_ItemSuite9/AEGP_GetItemComment");
	let Some(out) = (unsafe { commentPH.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	if !known(itemH) {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	}
	*out = handle_from_bytes(0u16.to_le_bytes().to_vec());
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn set_item_comment_utf16(_itemH: AEGP_ItemH, _commentZ: *const A_UTF16Char) -> A_Err {
	diag!("AEGP_ItemSuite9/AEGP_SetItemComment");
	refuse("AEGP_SetItemComment")
}

unsafe extern "C" fn get_item_mru_view(itemH: AEGP_ItemH, mru_viewP: *mut AEGP_ItemViewP) -> A_Err {
	crate::core::ffi::guard("get_item_mru_view", || {
		diag!("AEGP_ItemSuite8/AEGP_GetItemMRUView");
		let Some(out) = (unsafe { mru_viewP.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		if !known(itemH) {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		*out = std::ptr::null_mut();
		PF_Err_NONE
	})
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

/// Wire version 14. Same as suite 8 until the comment calls, which no longer
/// take a byte length.
pub const fn suite_9() -> AEGP_ItemSuite9 {
	AEGP_ItemSuite9 {
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
		AEGP_GetItemComment: Some(get_item_comment_utf16),
		AEGP_SetItemComment: Some(set_item_comment_utf16),
		AEGP_GetItemLabel: Some(get_item_label),
		AEGP_SetItemLabel: Some(set_item_label),
		AEGP_GetItemMRUView: Some(get_item_mru_view),
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

	static SOURCE_OK: AtomicBool = AtomicBool::new(false);
	static SOURCE_STALE: AtomicUsize = AtomicUsize::new(0);
	unsafe extern "C" fn source_test_entry(
		cmd: PF_Cmd,
		input: *mut PF_InData,
		_: *mut PF_OutData,
		_: PF_ParamList,
		_: *mut PF_LayerDef,
		_: *mut std::ffi::c_void,
	) -> PF_Err {
		if cmd != PF_Cmd_SEQUENCE_SETUP {
			return PF_Err_NONE;
		}
		let effect = unsafe { (*input).effect_ref };
		let layer = super::super::layer::create_layer_suite_5();
		let item = suite_8();
		let effects = super::super::effect::create_effect_suite_2();
		let mut source = std::ptr::null_mut();
		let mut comp = std::ptr::null_mut();
		let mut next = std::ptr::null_mut();
		let mut source_id = 0;
		let mut layer_id = 0;
		let mut kind = 0;
		let mut count = 0;
		let mut installed_key = 0;
		let mut effect_handle = std::ptr::null_mut();
		let ok = unsafe {
			layer.AEGP_GetLayerSourceItem.unwrap()(effect.cast(), &mut source) == 0
				&& layer.AEGP_GetLayerSourceItemID.unwrap()(effect.cast(), &mut layer_id) == 0
				&& item.AEGP_GetItemID.unwrap()(source, &mut source_id) == 0
				&& item.AEGP_GetItemType.unwrap()(source, &mut kind) == 0
				&& item.AEGP_GetFirstProjItem.unwrap()(std::ptr::null_mut(), &mut comp) == 0
				&& item.AEGP_GetNextProjItem.unwrap()(std::ptr::null_mut(), comp, &mut next) == 0
				&& effects.AEGP_GetLayerNumEffects.unwrap()(effect.cast(), &mut count) == 0
				&& effects.AEGP_GetLayerEffectByIndex.unwrap()(0, effect.cast(), 0, &mut effect_handle) == 0
				&& effects.AEGP_GetInstalledKeyFromLayerEffect.unwrap()(effect_handle, &mut installed_key) == 0
				&& effects.AEGP_DisposeEffect.unwrap()(effect_handle) == 0
		};
		SOURCE_STALE.store(source as usize, Ordering::SeqCst);
		SOURCE_OK.store(
			ok && !source.is_null()
				&& source != comp
				&& source == next
				&& source_id == 2
				&& layer_id == source_id
				&& kind == AEGP_ItemType_FOOTAGE as AEGP_ItemType
				&& count == 1
				&& installed_key != 0
				&& Timeline::of(source.cast()).is_none(),
			Ordering::SeqCst,
		);
		PF_Err_NONE
	}

	#[test]
	fn input_source_is_distinct_and_only_valid_while_plugin_runs() {
		SOURCE_OK.store(false, Ordering::SeqCst);
		let instance = unsafe { crate::Host::get().entry_loader(source_test_entry) }
			.load()
			.unwrap();
		assert!(SOURCE_OK.load(Ordering::SeqCst));
		drop(instance);
		let mut kind = 0;
		let stale = SOURCE_STALE.load(Ordering::SeqCst) as AEGP_ItemH;
		assert_eq!(
			unsafe { suite_8().AEGP_GetItemType.unwrap()(stale, &mut kind) },
			PF_Err_BAD_CALLBACK_PARAM
		);
	}

	#[test]
	fn legacy_suite_names_a_composition() {
		let suite = suite_6();
		let mut name = [1u8; AEGP_MAX_ITEM_NAME_SIZE as usize];
		let err = unsafe { suite.AEGP_GetTypeName.unwrap()(AEGP_ItemType_COMP, name.as_mut_ptr().cast()) };
		assert_eq!(err, PF_Err_NONE);
		assert!(name.starts_with(b"Composition\0"));
	}

	#[test]
	fn comp_type_name_is_composition() {
		let suite = suite_8();
		let mut name = [1u8; AEGP_MAX_ITEM_NAME_SIZE as usize];
		let err = unsafe { suite.AEGP_GetTypeName.unwrap()(AEGP_ItemType_COMP, name.as_mut_ptr().cast()) };
		assert_eq!(err, PF_Err_NONE);
		assert!(name.starts_with(b"Composition\0"));
	}

	#[test]
	fn null_item_has_no_type() {
		let suite = suite_8();
		let mut kind: AEGP_ItemType = 0;
		let err = unsafe { suite.AEGP_GetItemType.unwrap()(std::ptr::null_mut(), &mut kind) };
		assert_eq!(err, PF_Err_BAD_CALLBACK_PARAM);
	}

	#[test]
	fn active_item_needs_a_current_effect() {
		let suite = suite_8();
		let mut item: AEGP_ItemH = 0x1 as AEGP_ItemH;
		let err = unsafe { suite.AEGP_GetActiveItem.unwrap()(&mut item) };
		assert_eq!(err, PF_Err_BAD_CALLBACK_PARAM);
	}
}
