//! `AEGP Effect Suite` (version 2) and the `AEGP_EffectRefH` handles it takes.
//!
//! aexlo hosts exactly one effect per [`PluginInstance`] and has no layers, so
//! an effect handle is just the effect's `PF_ProgPtr` in a box: it is minted
//! by `AEGP_GetNewEffectForEffect` (PF Interface Suite), read by this suite and
//! the Stream Suite, and freed by `AEGP_DisposeEffect`. Everything that needs a
//! layer or the installed-effect registry is refused with `A_Err_GENERIC`.

use after_effects_sys::{
	A_Err, A_Err_GENERIC, A_Time, A_char, A_long, AEGP_EffectFlags, AEGP_EffectFlags_ACTIVE, AEGP_EffectIndex,
	AEGP_EffectRefH, AEGP_EffectSuite2, AEGP_InstalledEffectKey, AEGP_LayerH, AEGP_PluginID, PF_Err_BAD_CALLBACK_PARAM,
	PF_Err_INVALID_INDEX, PF_Err_NONE, PF_ParamDef, PF_ParamDefUnion, PF_ParamIndex, PF_ParamType, PF_ProgPtr,
};
use std::os::raw::c_void;

use crate::PluginInstance;
use crate::core::diagnostics::diag;

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
	A_Err_GENERIC as A_Err
}

unsafe extern "C" fn get_layer_num_effects(_layerH: AEGP_LayerH, _num_effectsPL: *mut A_long) -> A_Err {
	diag!("AEGP_EffectSuite2/AEGP_GetLayerNumEffects");
	unsupported("AEGP_GetLayerNumEffects")
}

unsafe extern "C" fn get_layer_effect_by_index(
	_aegp_plugin_id: AEGP_PluginID,
	_layerH: AEGP_LayerH,
	_layer_effect_indexL: AEGP_EffectIndex,
	_effectPH: *mut AEGP_EffectRefH,
) -> A_Err {
	diag!("AEGP_EffectSuite2/AEGP_GetLayerEffectByIndex");
	unsupported("AEGP_GetLayerEffectByIndex")
}

unsafe extern "C" fn get_installed_key_from_layer_effect(
	_effect_refH: AEGP_EffectRefH,
	_installed_effect_keyP: *mut AEGP_InstalledEffectKey,
) -> A_Err {
	diag!("AEGP_EffectSuite2/AEGP_GetInstalledKeyFromLayerEffect");
	unsupported("AEGP_GetInstalledKeyFromLayerEffect")
}

unsafe extern "C" fn get_effect_param_union_by_index(
	_aegp_plugin_id: AEGP_PluginID,
	effect_refH: AEGP_EffectRefH,
	param_index: PF_ParamIndex,
	param_typeP: *mut PF_ParamType,
	uP0: *mut PF_ParamDefUnion,
) -> A_Err {
	diag!("AEGP_EffectSuite2/AEGP_GetEffectParamUnionByIndex",
		"effect_refH" => format!("{:#x}", effect_refH as usize),
		"param_index" => param_index,
	);

	let Some(effect_ref) = (unsafe { effect_ref_of(effect_refH) }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	let Ok(index) = usize::try_from(param_index) else {
		return PF_Err_INVALID_INDEX as A_Err;
	};
	let Some((param_type, u)) = with_param(effect_ref, index, |def| (def.param_type, def.u)) else {
		return PF_Err_INVALID_INDEX as A_Err;
	};
	unsafe {
		if let Some(out) = param_typeP.as_mut() {
			*out = param_type;
		}
		if let Some(out) = uP0.as_mut() {
			*out = u;
		}
	}
	PF_Err_NONE as A_Err
}

/// The effect is always on: aexlo only runs effects it is rendering.
unsafe extern "C" fn get_effect_flags(effect_refH: AEGP_EffectRefH, effect_flagsP: *mut AEGP_EffectFlags) -> A_Err {
	diag!("AEGP_EffectSuite2/AEGP_GetEffectFlags",
		"effect_refH" => format!("{:#x}", effect_refH as usize),
	);

	if unsafe { effect_ref_of(effect_refH) }.is_none() {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	}
	let Some(out) = (unsafe { effect_flagsP.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	*out = AEGP_EffectFlags_ACTIVE as AEGP_EffectFlags;
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn set_effect_flags(
	_effect_refH: AEGP_EffectRefH,
	_effect_flags_set_mask: AEGP_EffectFlags,
	_effect_flags: AEGP_EffectFlags,
) -> A_Err {
	diag!("AEGP_EffectSuite2/AEGP_SetEffectFlags");
	unsupported("AEGP_SetEffectFlags")
}

unsafe extern "C" fn reorder_effect(_effect_refH: AEGP_EffectRefH, _effect_indexL: A_long) -> A_Err {
	diag!("AEGP_EffectSuite2/AEGP_ReorderEffect");
	unsupported("AEGP_ReorderEffect")
}

unsafe extern "C" fn effect_call_generic(
	_aegp_plugin_id: AEGP_PluginID,
	_effect_refH: AEGP_EffectRefH,
	_timePT: *const A_Time,
	_effect_extraPV: *mut c_void,
) -> A_Err {
	diag!("AEGP_EffectSuite2/AEGP_EffectCallGeneric");
	unsupported("AEGP_EffectCallGeneric")
}

unsafe extern "C" fn dispose_effect(effect_refH: AEGP_EffectRefH) -> A_Err {
	diag!("AEGP_EffectSuite2/AEGP_DisposeEffect",
		"effect_refH" => format!("{:#x}", effect_refH as usize),
	);

	if effect_refH.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	}
	// SAFETY: non-null effect handles only come from `new_effect_handle`.
	drop(unsafe { Box::from_raw(effect_refH as *mut Effect) });
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn apply_effect(
	_aegp_plugin_id: AEGP_PluginID,
	_layerH: AEGP_LayerH,
	_installed_effect_key: AEGP_InstalledEffectKey,
	_effect_refPH: *mut AEGP_EffectRefH,
) -> A_Err {
	diag!("AEGP_EffectSuite2/AEGP_ApplyEffect");
	unsupported("AEGP_ApplyEffect")
}

unsafe extern "C" fn delete_layer_effect(_effect_refH: AEGP_EffectRefH) -> A_Err {
	diag!("AEGP_EffectSuite2/AEGP_DeleteLayerEffect");
	unsupported("AEGP_DeleteLayerEffect")
}

unsafe extern "C" fn get_num_installed_effects(_num_installed_effectsPL: *mut A_long) -> A_Err {
	diag!("AEGP_EffectSuite2/AEGP_GetNumInstalledEffects");
	unsupported("AEGP_GetNumInstalledEffects")
}

unsafe extern "C" fn get_next_installed_effect(
	_installed_effect_key: AEGP_InstalledEffectKey,
	_next_effectPH: *mut AEGP_InstalledEffectKey,
) -> A_Err {
	diag!("AEGP_EffectSuite2/AEGP_GetNextInstalledEffect");
	unsupported("AEGP_GetNextInstalledEffect")
}

unsafe extern "C" fn get_effect_name(_installed_effect_key: AEGP_InstalledEffectKey, _nameZ: *mut A_char) -> A_Err {
	diag!("AEGP_EffectSuite2/AEGP_GetEffectName");
	unsupported("AEGP_GetEffectName")
}

unsafe extern "C" fn get_effect_match_name(
	_installed_effect_key: AEGP_InstalledEffectKey,
	_match_nameZ: *mut A_char,
) -> A_Err {
	diag!("AEGP_EffectSuite2/AEGP_GetEffectMatchName");
	unsupported("AEGP_GetEffectMatchName")
}

unsafe extern "C" fn get_effect_category(
	_installed_effect_key: AEGP_InstalledEffectKey,
	_categoryZ: *mut A_char,
) -> A_Err {
	diag!("AEGP_EffectSuite2/AEGP_GetEffectCategory");
	unsupported("AEGP_GetEffectCategory")
}

unsafe extern "C" fn duplicate_effect(
	original_effect_refH: AEGP_EffectRefH,
	duplicate_effect_refPH: *mut AEGP_EffectRefH,
) -> A_Err {
	diag!("AEGP_EffectSuite2/AEGP_DuplicateEffect",
		"original_effect_refH" => format!("{:#x}", original_effect_refH as usize),
	);

	let Some(effect_ref) = (unsafe { effect_ref_of(original_effect_refH) }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	let Some(out) = (unsafe { duplicate_effect_refPH.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	*out = new_effect_handle(effect_ref);
	PF_Err_NONE as A_Err
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

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn handles_round_trip_and_dispose() {
		let suite = create_effect_suite_2();
		let effect_ref = 0x1234 as PF_ProgPtr;
		let handle = new_effect_handle(effect_ref);
		assert_eq!(unsafe { effect_ref_of(handle) }, Some(effect_ref));

		let mut dup: AEGP_EffectRefH = std::ptr::null_mut();
		let mut flags: AEGP_EffectFlags = 0;
		unsafe {
			assert_eq!(
				suite.AEGP_DuplicateEffect.unwrap()(handle, &mut dup),
				PF_Err_NONE as A_Err
			);
			assert_eq!(effect_ref_of(dup), Some(effect_ref));
			assert_eq!(
				suite.AEGP_GetEffectFlags.unwrap()(dup, &mut flags),
				PF_Err_NONE as A_Err
			);
			assert_eq!(suite.AEGP_DisposeEffect.unwrap()(dup), PF_Err_NONE as A_Err);
			assert_eq!(suite.AEGP_DisposeEffect.unwrap()(handle), PF_Err_NONE as A_Err);
		}
		assert_eq!(flags, AEGP_EffectFlags_ACTIVE as AEGP_EffectFlags);
	}

	#[test]
	fn null_handles_are_rejected() {
		let suite = create_effect_suite_2();
		let mut flags: AEGP_EffectFlags = 0;
		unsafe {
			assert_ne!(
				suite.AEGP_GetEffectFlags.unwrap()(std::ptr::null_mut(), &mut flags),
				PF_Err_NONE as A_Err
			);
			assert_ne!(
				suite.AEGP_DisposeEffect.unwrap()(std::ptr::null_mut()),
				PF_Err_NONE as A_Err
			);
		}
	}
}
