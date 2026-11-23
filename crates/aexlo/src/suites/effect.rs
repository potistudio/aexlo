//! `AEGP Effect Suite` (wire versions 1 to 5) and the `AEGP_EffectRefH`
//! handles it takes.
//!
//! aexlo hosts one effect on a synthetic layer per [`PluginInstance`]. An
//! effect handle is the effect's `PF_ProgPtr` in a box: it is minted
//! by `AEGP_GetNewEffectForEffect` (PF Interface Suite), read by this suite and
//! the Stream Suite, and freed by `AEGP_DisposeEffect`. Everything that needs a
//! installed-effect registry is refused with `A_Err_GENERIC`.
//!
//! Wire version 1 is version 2 without `AEGP_DuplicateEffect`. Version 3 adds
//! the command argument to `AEGP_EffectCallGeneric` (the same signature as
//! version 4) but has no effect-mask calls. Version 4 appends those calls;
//! aexlo's effects have no effect masks. Version 5 appends
//! `AEGP_GetIsInternalEffect`, which takes an installed-effect key and so is
//! refused like the rest of the registry.

use crate::sys::{
	A_Boolean, A_Err, A_Err_GENERIC, A_Time, A_char, A_long, A_u_long, AEGP_EffectFlags, AEGP_EffectFlags_ACTIVE,
	AEGP_EffectIndex, AEGP_EffectRefH, AEGP_EffectSuite1, AEGP_EffectSuite2, AEGP_EffectSuite3, AEGP_EffectSuite4,
	AEGP_EffectSuite5, AEGP_InstalledEffectKey, AEGP_LayerH, AEGP_MaskIDVal, AEGP_PluginID, AEGP_StreamRefH, PF_Cmd,
	PF_Err_BAD_CALLBACK_PARAM, PF_Err_INVALID_INDEX, PF_Err_NONE, PF_ParamDef, PF_ParamDefUnion, PF_ParamIndex,
	PF_ParamType, PF_ProgPtr,
};
use std::collections::{BTreeMap, HashSet};
use std::os::raw::c_void;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{LazyLock, Mutex};

use crate::PluginInstance;
use crate::core::diagnostics::diag;

#[derive(Clone, Default, PartialEq, Eq)]
pub(crate) struct EffectMetadata {
	pub name: Option<Vec<u8>>,
	pub match_name: Option<Vec<u8>>,
	pub category: Option<Vec<u8>>,
}
static INSTALLED: LazyLock<Mutex<BTreeMap<AEGP_InstalledEffectKey, (Option<EffectMetadata>, HashSet<u64>)>>> =
	LazyLock::new(|| Mutex::new(BTreeMap::new()));
static NEXT_KEY: AtomicI32 = AtomicI32::new(1);
pub(crate) fn register_metadata(owner: u64, metadata: Option<EffectMetadata>) {
	let mut entries = INSTALLED.lock().unwrap();
	if metadata
		.as_ref()
		.and_then(|m| m.match_name.as_ref())
		.is_some_and(|name| !name.is_empty())
	{
		if let Some((_, (_, owners))) = entries.iter_mut().find(|(_, (existing, _))| *existing == metadata) {
			owners.insert(owner);
			return;
		}
	}
	let key = NEXT_KEY.fetch_add(1, Ordering::Relaxed);
	if key > 0 {
		entries.insert(key, (metadata, HashSet::from([owner])));
	}
}
pub(crate) fn key_for_owner(owner: u64) -> Option<AEGP_InstalledEffectKey> {
	INSTALLED
		.lock()
		.unwrap()
		.iter()
		.find_map(|(&key, (_, owners))| owners.contains(&owner).then_some(key))
}
pub(crate) fn remove_metadata(owner: u64) {
	let mut entries = INSTALLED.lock().unwrap();
	entries.retain(|_, (_, owners)| {
		owners.remove(&owner);
		!owners.is_empty()
	});
}

/// What an `AEGP_EffectRefH` points at.
struct Effect {
	effect_ref: PF_ProgPtr,
}

/// Mint a new effect handle for the effect behind `effect_ref`. The plugin
/// owns it and frees it with `AEGP_DisposeEffect`.
pub(crate) fn new_effect_handle(effect_ref: PF_ProgPtr) -> AEGP_EffectRefH {
	Box::into_raw(Box::new(Effect { effect_ref })) as AEGP_EffectRefH
}

/// The `PF_ProgPtr` behind an effect handle, or `None` for a null handle.
///
/// # Safety
/// `effect_refH` must be null or a handle from [`new_effect_handle`] that has
/// not been disposed.
pub(crate) unsafe fn effect_ref_of(effect_refH: AEGP_EffectRefH) -> Option<PF_ProgPtr> {
	unsafe { (effect_refH as *const Effect).as_ref() }.map(|effect| effect.effect_ref)
}

/// Run `f` on the parameter at `index` of the effect behind `effect_ref`.
///
/// Returns `None` when the effect is unknown or the index is out of range.
pub(crate) fn with_param<R>(effect_ref: PF_ProgPtr, index: usize, f: impl FnOnce(&PF_ParamDef) -> R) -> Option<R> {
	let instance = PluginInstance::get_instance_ptr(effect_ref)?;
	// SAFETY: effect_ref is the live instance that is running the plugin call.
	unsafe { instance.as_ref() }.param_by_index(index).map(f)
}

fn unsupported(name: &str) -> A_Err {
	log::warn!("AEGP_EffectSuite2/{name}: aexlo has no layers or installed-effect registry");
	A_Err_GENERIC
}

unsafe extern "C" fn get_layer_num_effects(layerH: AEGP_LayerH, num_effectsPL: *mut A_long) -> A_Err {
	crate::core::ffi::guard("get_layer_num_effects", || {
		diag!("AEGP_EffectSuite2/AEGP_GetLayerNumEffects");
		super::layer::answer(layerH.cast(), num_effectsPL, |t| t.hosts_effect as A_long)
	})
}

unsafe extern "C" fn get_layer_effect_by_index(
	_aegp_plugin_id: AEGP_PluginID,
	layerH: AEGP_LayerH,
	layer_effect_indexL: AEGP_EffectIndex,
	effectPH: *mut AEGP_EffectRefH,
) -> A_Err {
	crate::core::ffi::guard("get_layer_effect_by_index", || {
		diag!("AEGP_EffectSuite2/AEGP_GetLayerEffectByIndex");
		let Some(out) = (unsafe { effectPH.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		*out = std::ptr::null_mut();
		let Some(timeline) = super::layer::Timeline::of(layerH.cast()) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		// The project does not say what effects are on the other layers.
		if layer_effect_indexL != 0 || !timeline.hosts_effect {
			return PF_Err_INVALID_INDEX;
		}
		*out = new_effect_handle(layerH.cast());
		PF_Err_NONE
	})
}

unsafe extern "C" fn get_installed_key_from_layer_effect(
	effect_refH: AEGP_EffectRefH,
	installed_effect_keyP: *mut AEGP_InstalledEffectKey,
) -> A_Err {
	crate::core::ffi::guard("get_installed_key_from_layer_effect", || {
		diag!("AEGP_EffectSuite2/AEGP_GetInstalledKeyFromLayerEffect");
		let Some(out) = (unsafe { installed_effect_keyP.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		let Some(effect_ref) = (unsafe { effect_ref_of(effect_refH) }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		if super::layer::Timeline::of_effect(effect_ref.cast()).is_none() {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		let instance = unsafe { PluginInstance::get_instance_ptr(effect_ref).unwrap().as_ref() };
		let Some(key) = instance.installed_effect_key() else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		if !INSTALLED.lock().unwrap().contains_key(&key) {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		*out = key;
		PF_Err_NONE
	})
}

unsafe extern "C" fn get_effect_param_union_by_index(
	_aegp_plugin_id: AEGP_PluginID,
	effect_refH: AEGP_EffectRefH,
	param_index: PF_ParamIndex,
	param_typeP: *mut PF_ParamType,
	uP0: *mut PF_ParamDefUnion,
) -> A_Err {
	crate::core::ffi::guard("get_effect_param_union_by_index", || {
		diag!("AEGP_EffectSuite2/AEGP_GetEffectParamUnionByIndex",
			"effect_refH" => format!("{:#x}", effect_refH as usize),
			"param_index" => param_index,
		);

		let Some(effect_ref) = (unsafe { effect_ref_of(effect_refH) }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		let Ok(index) = usize::try_from(param_index) else {
			return PF_Err_INVALID_INDEX;
		};
		let Some((param_type, u)) = with_param(effect_ref, index, |def| (def.param_type, def.u)) else {
			return PF_Err_INVALID_INDEX;
		};
		unsafe {
			if let Some(out) = param_typeP.as_mut() {
				*out = param_type;
			}
			if let Some(out) = uP0.as_mut() {
				*out = u;
			}
		}
		PF_Err_NONE
	})
}

/// The effect is always on: aexlo only runs effects it is rendering.
unsafe extern "C" fn get_effect_flags(effect_refH: AEGP_EffectRefH, effect_flagsP: *mut AEGP_EffectFlags) -> A_Err {
	crate::core::ffi::guard("get_effect_flags", || {
		diag!("AEGP_EffectSuite2/AEGP_GetEffectFlags",
			"effect_refH" => format!("{:#x}", effect_refH as usize),
		);

		if unsafe { effect_ref_of(effect_refH) }.is_none() {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		let Some(out) = (unsafe { effect_flagsP.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		*out = AEGP_EffectFlags_ACTIVE;
		PF_Err_NONE
	})
}

unsafe extern "C" fn set_effect_flags(
	_effect_refH: AEGP_EffectRefH,
	_effect_flags_set_mask: AEGP_EffectFlags,
	_effect_flags: AEGP_EffectFlags,
) -> A_Err {
	crate::core::ffi::guard("set_effect_flags", || {
		diag!("AEGP_EffectSuite2/AEGP_SetEffectFlags");
		unsupported("AEGP_SetEffectFlags")
	})
}

unsafe extern "C" fn reorder_effect(_effect_refH: AEGP_EffectRefH, _effect_indexL: A_long) -> A_Err {
	crate::core::ffi::guard("reorder_effect", || {
		diag!("AEGP_EffectSuite2/AEGP_ReorderEffect");
		unsupported("AEGP_ReorderEffect")
	})
}

unsafe extern "C" fn effect_call_generic(
	_aegp_plugin_id: AEGP_PluginID,
	_effect_refH: AEGP_EffectRefH,
	_timePT: *const A_Time,
	_effect_extraPV: *mut c_void,
) -> A_Err {
	crate::core::ffi::guard("effect_call_generic", || {
		diag!("AEGP_EffectSuite2/AEGP_EffectCallGeneric");
		unsupported("AEGP_EffectCallGeneric")
	})
}

unsafe extern "C" fn effect_call_generic_cmd(
	_aegp_plugin_id: AEGP_PluginID,
	_effect_refH: AEGP_EffectRefH,
	_timePT: *const A_Time,
	_effect_cmd: PF_Cmd,
	_effect_extraPV: *mut c_void,
) -> A_Err {
	crate::core::ffi::guard("effect_call_generic_cmd", || {
		diag!("AEGP_EffectSuite4/AEGP_EffectCallGeneric");
		unsupported("AEGP_EffectCallGeneric")
	})
}

unsafe extern "C" fn dispose_effect(effect_refH: AEGP_EffectRefH) -> A_Err {
	crate::core::ffi::guard("dispose_effect", || {
		diag!("AEGP_EffectSuite2/AEGP_DisposeEffect",
			"effect_refH" => format!("{:#x}", effect_refH as usize),
		);

		if effect_refH.is_null() {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		// SAFETY: non-null effect handles only come from `new_effect_handle`.
		drop(unsafe { Box::from_raw(effect_refH as *mut Effect) });
		PF_Err_NONE
	})
}

unsafe extern "C" fn apply_effect(
	_aegp_plugin_id: AEGP_PluginID,
	_layerH: AEGP_LayerH,
	_installed_effect_key: AEGP_InstalledEffectKey,
	_effect_refPH: *mut AEGP_EffectRefH,
) -> A_Err {
	crate::core::ffi::guard("apply_effect", || {
		diag!("AEGP_EffectSuite2/AEGP_ApplyEffect");
		unsupported("AEGP_ApplyEffect")
	})
}

unsafe extern "C" fn delete_layer_effect(_effect_refH: AEGP_EffectRefH) -> A_Err {
	crate::core::ffi::guard("delete_layer_effect", || {
		diag!("AEGP_EffectSuite2/AEGP_DeleteLayerEffect");
		unsupported("AEGP_DeleteLayerEffect")
	})
}

unsafe extern "C" fn get_num_installed_effects(num_installed_effectsPL: *mut A_long) -> A_Err {
	crate::core::ffi::guard("get_num_installed_effects", || {
		diag!("AEGP_EffectSuite2/AEGP_GetNumInstalledEffects");
		let Some(out) = (unsafe { num_installed_effectsPL.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		*out = INSTALLED.lock().unwrap().len() as A_long;
		PF_Err_NONE
	})
}

unsafe extern "C" fn get_next_installed_effect(
	installed_effect_key: AEGP_InstalledEffectKey,
	next_effectPH: *mut AEGP_InstalledEffectKey,
) -> A_Err {
	crate::core::ffi::guard("get_next_installed_effect", || {
		diag!("AEGP_EffectSuite2/AEGP_GetNextInstalledEffect");
		let Some(out) = (unsafe { next_effectPH.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		let effects = INSTALLED.lock().unwrap();
		if installed_effect_key != 0 && !effects.contains_key(&installed_effect_key) {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		*out = effects
			.range((
				std::ops::Bound::Excluded(installed_effect_key),
				std::ops::Bound::Unbounded,
			))
			.next()
			.map_or(0, |(&key, _)| key);
		PF_Err_NONE
	})
}

unsafe fn copy_metadata(
	key: AEGP_InstalledEffectKey,
	out: *mut A_char,
	capacity: usize,
	field: impl FnOnce(&EffectMetadata) -> &Option<Vec<u8>>,
) -> A_Err {
	if out.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM;
	}
	let effects = INSTALLED.lock().unwrap();
	let Some((Some(metadata), _)) = effects.get(&key) else {
		return A_Err_GENERIC;
	};
	let Some(bytes) = field(metadata) else {
		return A_Err_GENERIC;
	};
	if bytes.len() >= capacity {
		return PF_Err_BAD_CALLBACK_PARAM;
	}
	unsafe {
		std::ptr::copy_nonoverlapping(bytes.as_ptr(), out.cast::<u8>(), bytes.len());
		*out.add(bytes.len()) = 0;
	}
	PF_Err_NONE
}
unsafe extern "C" fn get_effect_name(installed_effect_key: AEGP_InstalledEffectKey, nameZ: *mut A_char) -> A_Err {
	crate::core::ffi::guard("get_effect_name", || {
		diag!("AEGP_EffectSuite2/AEGP_GetEffectName");
		unsafe {
			copy_metadata(
				installed_effect_key,
				nameZ,
				crate::sys::AEGP_MAX_EFFECT_NAME_SIZE as usize,
				|m| &m.name,
			)
		}
	})
}

unsafe extern "C" fn get_effect_match_name(
	installed_effect_key: AEGP_InstalledEffectKey,
	match_nameZ: *mut A_char,
) -> A_Err {
	crate::core::ffi::guard("get_effect_match_name", || {
		diag!("AEGP_EffectSuite2/AEGP_GetEffectMatchName");
		unsafe {
			copy_metadata(
				installed_effect_key,
				match_nameZ,
				crate::sys::AEGP_MAX_EFFECT_MATCH_NAME_SIZE as usize,
				|m| &m.match_name,
			)
		}
	})
}

unsafe extern "C" fn get_effect_category(
	installed_effect_key: AEGP_InstalledEffectKey,
	categoryZ: *mut A_char,
) -> A_Err {
	crate::core::ffi::guard("get_effect_category", || {
		diag!("AEGP_EffectSuite2/AEGP_GetEffectCategory");
		unsafe {
			copy_metadata(
				installed_effect_key,
				categoryZ,
				crate::sys::AEGP_MAX_EFFECT_CATEGORY_NAME_SIZE as usize,
				|m| &m.category,
			)
		}
	})
}

unsafe extern "C" fn duplicate_effect(
	original_effect_refH: AEGP_EffectRefH,
	duplicate_effect_refPH: *mut AEGP_EffectRefH,
) -> A_Err {
	crate::core::ffi::guard("duplicate_effect", || {
		diag!("AEGP_EffectSuite2/AEGP_DuplicateEffect",
			"original_effect_refH" => format!("{:#x}", original_effect_refH as usize),
		);

		let Some(effect_ref) = (unsafe { effect_ref_of(original_effect_refH) }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		let Some(out) = (unsafe { duplicate_effect_refPH.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		*out = new_effect_handle(effect_ref);
		PF_Err_NONE
	})
}

unsafe extern "C" fn num_effect_mask(effect_refH: AEGP_EffectRefH, num_masksPL: *mut A_u_long) -> A_Err {
	crate::core::ffi::guard("num_effect_mask", || {
		diag!("AEGP_EffectSuite4/AEGP_NumEffectMask",
			"effect_refH" => format!("{:#x}", effect_refH as usize),
		);

		if unsafe { effect_ref_of(effect_refH) }.is_none() {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		let Some(out) = (unsafe { num_masksPL.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		*out = 0;
		PF_Err_NONE
	})
}

unsafe extern "C" fn get_effect_mask_id(
	effect_refH: AEGP_EffectRefH,
	_mask_indexL: A_u_long,
	_id_valP: *mut AEGP_MaskIDVal,
) -> A_Err {
	crate::core::ffi::guard("get_effect_mask_id", || {
		diag!("AEGP_EffectSuite4/AEGP_GetEffectMaskID",
			"effect_refH" => format!("{:#x}", effect_refH as usize),
		);

		if unsafe { effect_ref_of(effect_refH) }.is_none() {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		PF_Err_INVALID_INDEX
	})
}

unsafe extern "C" fn add_effect_mask(
	_effect_refH: AEGP_EffectRefH,
	_id_val: AEGP_MaskIDVal,
	_streamPH0: *mut AEGP_StreamRefH,
) -> A_Err {
	crate::core::ffi::guard("add_effect_mask", || {
		diag!("AEGP_EffectSuite4/AEGP_AddEffectMask");
		unsupported("AEGP_AddEffectMask")
	})
}

unsafe extern "C" fn remove_effect_mask(_effect_refH: AEGP_EffectRefH, _id_val: AEGP_MaskIDVal) -> A_Err {
	crate::core::ffi::guard("remove_effect_mask", || {
		diag!("AEGP_EffectSuite4/AEGP_RemoveEffectMask");
		unsupported("AEGP_RemoveEffectMask")
	})
}

unsafe extern "C" fn set_effect_mask(
	_effect_refH: AEGP_EffectRefH,
	_mask_indexL: A_u_long,
	_id_val: AEGP_MaskIDVal,
	_streamPH0: *mut AEGP_StreamRefH,
) -> A_Err {
	crate::core::ffi::guard("set_effect_mask", || {
		diag!("AEGP_EffectSuite4/AEGP_SetEffectMask");
		unsupported("AEGP_SetEffectMask")
	})
}

unsafe extern "C" fn get_is_internal_effect(
	_installed_effect_key: AEGP_InstalledEffectKey,
	_isInternal: *mut A_Boolean,
) -> A_Err {
	crate::core::ffi::guard("get_is_internal_effect", || {
		diag!("AEGP_EffectSuite5/AEGP_GetIsInternalEffect");
		unsupported("AEGP_GetIsInternalEffect")
	})
}

/// Builds the `AEGP_EffectSuite1` vtable. Same calls as version 2, without
/// `AEGP_DuplicateEffect`.
pub(super) const fn create_effect_suite_1() -> AEGP_EffectSuite1 {
	AEGP_EffectSuite1 {
		AEGP_GetLayerNumEffects: Some(get_layer_num_effects),
		AEGP_GetLayerEffectByIndex: Some(get_layer_effect_by_index),
		AEGP_GetInstalledKeyFromLayerEffect: Some(get_installed_key_from_layer_effect),
		AEGP_GetEffectParamUnionByIndex: Some(get_effect_param_union_by_index),
		AEGP_GetEffectFlags: Some(get_effect_flags),
		AEGP_SetEffectFlags: Some(set_effect_flags),
		AEGP_ReorderEffect: Some(reorder_effect),
		AEGP_EffectCallGeneric: Some(effect_call_generic),
		AEGP_DisposeEffect: Some(dispose_effect),
		AEGP_ApplyEffect: Some(apply_effect),
		AEGP_DeleteLayerEffect: Some(delete_layer_effect),
		AEGP_GetNumInstalledEffects: Some(get_num_installed_effects),
		AEGP_GetNextInstalledEffect: Some(get_next_installed_effect),
		AEGP_GetEffectName: Some(get_effect_name),
		AEGP_GetEffectMatchName: Some(get_effect_match_name),
		AEGP_GetEffectCategory: Some(get_effect_category),
	}
}

/// Builds the `AEGP_EffectSuite2` vtable.
pub(super) const fn create_effect_suite_2() -> AEGP_EffectSuite2 {
	AEGP_EffectSuite2 {
		AEGP_GetLayerNumEffects: Some(get_layer_num_effects),
		AEGP_GetLayerEffectByIndex: Some(get_layer_effect_by_index),
		AEGP_GetInstalledKeyFromLayerEffect: Some(get_installed_key_from_layer_effect),
		AEGP_GetEffectParamUnionByIndex: Some(get_effect_param_union_by_index),
		AEGP_GetEffectFlags: Some(get_effect_flags),
		AEGP_SetEffectFlags: Some(set_effect_flags),
		AEGP_ReorderEffect: Some(reorder_effect),
		AEGP_EffectCallGeneric: Some(effect_call_generic),
		AEGP_DisposeEffect: Some(dispose_effect),
		AEGP_ApplyEffect: Some(apply_effect),
		AEGP_DeleteLayerEffect: Some(delete_layer_effect),
		AEGP_GetNumInstalledEffects: Some(get_num_installed_effects),
		AEGP_GetNextInstalledEffect: Some(get_next_installed_effect),
		AEGP_GetEffectName: Some(get_effect_name),
		AEGP_GetEffectMatchName: Some(get_effect_match_name),
		AEGP_GetEffectCategory: Some(get_effect_category),
		AEGP_DuplicateEffect: Some(duplicate_effect),
	}
}

/// Builds the `AEGP_EffectSuite3` vtable. Same calls as version 2, but
/// `AEGP_EffectCallGeneric` takes the command added in this version.
pub(super) const fn create_effect_suite_3() -> AEGP_EffectSuite3 {
	AEGP_EffectSuite3 {
		AEGP_GetLayerNumEffects: Some(get_layer_num_effects),
		AEGP_GetLayerEffectByIndex: Some(get_layer_effect_by_index),
		AEGP_GetInstalledKeyFromLayerEffect: Some(get_installed_key_from_layer_effect),
		AEGP_GetEffectParamUnionByIndex: Some(get_effect_param_union_by_index),
		AEGP_GetEffectFlags: Some(get_effect_flags),
		AEGP_SetEffectFlags: Some(set_effect_flags),
		AEGP_ReorderEffect: Some(reorder_effect),
		AEGP_EffectCallGeneric: Some(effect_call_generic_cmd),
		AEGP_DisposeEffect: Some(dispose_effect),
		AEGP_ApplyEffect: Some(apply_effect),
		AEGP_DeleteLayerEffect: Some(delete_layer_effect),
		AEGP_GetNumInstalledEffects: Some(get_num_installed_effects),
		AEGP_GetNextInstalledEffect: Some(get_next_installed_effect),
		AEGP_GetEffectName: Some(get_effect_name),
		AEGP_GetEffectMatchName: Some(get_effect_match_name),
		AEGP_GetEffectCategory: Some(get_effect_category),
		AEGP_DuplicateEffect: Some(duplicate_effect),
	}
}

/// Builds the `AEGP_EffectSuite4` vtable.
pub(super) const fn create_effect_suite_4() -> AEGP_EffectSuite4 {
	AEGP_EffectSuite4 {
		AEGP_GetLayerNumEffects: Some(get_layer_num_effects),
		AEGP_GetLayerEffectByIndex: Some(get_layer_effect_by_index),
		AEGP_GetInstalledKeyFromLayerEffect: Some(get_installed_key_from_layer_effect),
		AEGP_GetEffectParamUnionByIndex: Some(get_effect_param_union_by_index),
		AEGP_GetEffectFlags: Some(get_effect_flags),
		AEGP_SetEffectFlags: Some(set_effect_flags),
		AEGP_ReorderEffect: Some(reorder_effect),
		AEGP_EffectCallGeneric: Some(effect_call_generic_cmd),
		AEGP_DisposeEffect: Some(dispose_effect),
		AEGP_ApplyEffect: Some(apply_effect),
		AEGP_DeleteLayerEffect: Some(delete_layer_effect),
		AEGP_GetNumInstalledEffects: Some(get_num_installed_effects),
		AEGP_GetNextInstalledEffect: Some(get_next_installed_effect),
		AEGP_GetEffectName: Some(get_effect_name),
		AEGP_GetEffectMatchName: Some(get_effect_match_name),
		AEGP_GetEffectCategory: Some(get_effect_category),
		AEGP_DuplicateEffect: Some(duplicate_effect),
		AEGP_NumEffectMask: Some(num_effect_mask),
		AEGP_GetEffectMaskID: Some(get_effect_mask_id),
		AEGP_AddEffectMask: Some(add_effect_mask),
		AEGP_RemoveEffectMask: Some(remove_effect_mask),
		AEGP_SetEffectMask: Some(set_effect_mask),
	}
}

/// Builds the `AEGP_EffectSuite5` vtable: version 4 plus
/// `AEGP_GetIsInternalEffect`.
pub(super) const fn create_effect_suite_5() -> AEGP_EffectSuite5 {
	AEGP_EffectSuite5 {
		AEGP_GetLayerNumEffects: Some(get_layer_num_effects),
		AEGP_GetLayerEffectByIndex: Some(get_layer_effect_by_index),
		AEGP_GetInstalledKeyFromLayerEffect: Some(get_installed_key_from_layer_effect),
		AEGP_GetEffectParamUnionByIndex: Some(get_effect_param_union_by_index),
		AEGP_GetEffectFlags: Some(get_effect_flags),
		AEGP_SetEffectFlags: Some(set_effect_flags),
		AEGP_ReorderEffect: Some(reorder_effect),
		AEGP_EffectCallGeneric: Some(effect_call_generic_cmd),
		AEGP_DisposeEffect: Some(dispose_effect),
		AEGP_ApplyEffect: Some(apply_effect),
		AEGP_DeleteLayerEffect: Some(delete_layer_effect),
		AEGP_GetNumInstalledEffects: Some(get_num_installed_effects),
		AEGP_GetNextInstalledEffect: Some(get_next_installed_effect),
		AEGP_GetEffectName: Some(get_effect_name),
		AEGP_GetEffectMatchName: Some(get_effect_match_name),
		AEGP_GetEffectCategory: Some(get_effect_category),
		AEGP_DuplicateEffect: Some(duplicate_effect),
		AEGP_NumEffectMask: Some(num_effect_mask),
		AEGP_GetEffectMaskID: Some(get_effect_mask_id),
		AEGP_AddEffectMask: Some(add_effect_mask),
		AEGP_RemoveEffectMask: Some(remove_effect_mask),
		AEGP_SetEffectMask: Some(set_effect_mask),
		AEGP_GetIsInternalEffect: Some(get_is_internal_effect),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn installed_identity_is_shared_and_removed_with_last_owner() {
		let first = super::super::register::new_owner();
		let second = super::super::register::new_owner();
		let metadata = EffectMetadata {
			name: Some(b"Test effect".to_vec()),
			match_name: Some(b"aexlo.test.unique.effect".to_vec()),
			category: Some(b"Test category".to_vec()),
		};
		register_metadata(first, Some(metadata.clone()));
		register_metadata(second, Some(metadata));
		let key = key_for_owner(first).unwrap();
		assert_eq!(key_for_owner(second), Some(key));
		let mut name = [0i8; crate::sys::AEGP_MAX_EFFECT_MATCH_NAME_SIZE as usize];
		assert_eq!(unsafe { get_effect_match_name(key, name.as_mut_ptr()) }, PF_Err_NONE);
		assert_eq!(
			unsafe { std::ffi::CStr::from_ptr(name.as_ptr()) }.to_bytes(),
			b"aexlo.test.unique.effect"
		);
		assert_eq!(
			unsafe { get_effect_name(key, std::ptr::null_mut()) },
			PF_Err_BAD_CALLBACK_PARAM
		);
		remove_metadata(first);
		assert_eq!(key_for_owner(second), Some(key));
		remove_metadata(second);
		assert_ne!(unsafe { get_effect_match_name(key, name.as_mut_ptr()) }, PF_Err_NONE);
		super::super::register::remove_owner(first);
		super::super::register::remove_owner(second);
	}

	#[test]
	fn handles_round_trip_and_dispose() {
		let suite = create_effect_suite_2();
		let effect_ref = 0x1234 as PF_ProgPtr;
		let handle = new_effect_handle(effect_ref);
		assert_eq!(unsafe { effect_ref_of(handle) }, Some(effect_ref));

		let mut dup: AEGP_EffectRefH = std::ptr::null_mut();
		let mut flags: AEGP_EffectFlags = 0;
		unsafe {
			assert_eq!(suite.AEGP_DuplicateEffect.unwrap()(handle, &mut dup), PF_Err_NONE);
			assert_eq!(effect_ref_of(dup), Some(effect_ref));
			assert_eq!(suite.AEGP_GetEffectFlags.unwrap()(dup, &mut flags), PF_Err_NONE);
			assert_eq!(suite.AEGP_DisposeEffect.unwrap()(dup), PF_Err_NONE);
			assert_eq!(suite.AEGP_DisposeEffect.unwrap()(handle), PF_Err_NONE);
		}
		assert_eq!(flags, AEGP_EffectFlags_ACTIVE);
	}

	#[test]
	fn effects_have_no_masks() {
		let suite = create_effect_suite_4();
		let handle = new_effect_handle(0x1234 as PF_ProgPtr);
		let mut n: A_u_long = 9;
		let mut id: AEGP_MaskIDVal = 0;
		unsafe {
			assert_eq!(suite.AEGP_NumEffectMask.unwrap()(handle, &mut n), PF_Err_NONE);
			assert_eq!(
				suite.AEGP_GetEffectMaskID.unwrap()(handle, 0, &mut id),
				PF_Err_INVALID_INDEX
			);
			assert_eq!(suite.AEGP_DisposeEffect.unwrap()(handle), PF_Err_NONE);
		}
		assert_eq!(n, 0);
	}

	#[test]
	fn version_5_serves_handles_and_refuses_the_registry() {
		let suite = create_effect_suite_5();
		let handle = new_effect_handle(0x1234 as PF_ProgPtr);
		let mut flags: AEGP_EffectFlags = 0;
		let mut internal: A_Boolean = 7;
		unsafe {
			assert_eq!(suite.AEGP_GetEffectFlags.unwrap()(handle, &mut flags), PF_Err_NONE);
			assert_ne!(suite.AEGP_GetIsInternalEffect.unwrap()(1, &mut internal), PF_Err_NONE);
			assert_eq!(suite.AEGP_DisposeEffect.unwrap()(handle), PF_Err_NONE);
		}
		assert_eq!(flags, AEGP_EffectFlags_ACTIVE);
		assert_eq!(internal, 7, "a refused call leaves the out-parameter alone");
	}

	#[test]
	fn null_handles_are_rejected() {
		let suite = create_effect_suite_2();
		let mut flags: AEGP_EffectFlags = 0;
		unsafe {
			assert_ne!(
				suite.AEGP_GetEffectFlags.unwrap()(std::ptr::null_mut(), &mut flags),
				PF_Err_NONE
			);
			assert_ne!(suite.AEGP_DisposeEffect.unwrap()(std::ptr::null_mut()), PF_Err_NONE);
		}
	}
}
