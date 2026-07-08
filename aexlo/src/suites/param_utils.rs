//! `PF_ParamUtilsSuite3` — parameter UI and keyframe queries.
//!
//! aexlo drives a single, static frame with no timeline, so the keyframe/state
//! entry points report "no keyframes" / "states identical" rather than pretending
//! to have animation data. The one call effects actually rely on outside a real
//! host is [`update_param_ui`], which lets a plugin refresh a parameter's UI state
//! (twirl collapse, disabled, name, …) in response to a user edit.

use after_effects_sys::*;

use crate::PluginInstance;

/// Copy a plugin's updated UI fields into the stored parameter.
///
/// The plugin hands back a `PF_ParamDef` whose UI-relevant fields (`flags`,
/// `ui_flags`, name, …) it has tweaked; we apply them to the matching stored
/// parameter without disturbing its value.
unsafe extern "C" fn update_param_ui(
	effect_ref: PF_ProgPtr,
	param_index: PF_ParamIndex,
	defP: *const PF_ParamDef,
) -> PF_Err {
	if effect_ref.is_null() || defP.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	let Some(mut instance) = PluginInstance::get_instance_ptr(effect_ref) else {
		log::error!("PF_UpdateParamUI: no plugin instance for effect_ref");
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	};

	// SAFETY: `defP` is non-null (checked) and, per the suite contract, points to a
	// valid `PF_ParamDef` for the duration of the call.
	let def = unsafe { &*defP };
	// SAFETY: `effect_ref` identifies this instance; the plugin is not concurrently
	// mutating it while inside its own suite callback.
	unsafe { instance.as_mut() }.update_param_ui(param_index as usize, def);

	PF_Err_NONE as PF_Err
}

/// Report the parameter's render state as an all-zero [`PF_State`]. With no
/// timeline every checkout is equivalent, so a constant state is consistent with
/// [`are_states_identical`].
unsafe extern "C" fn get_current_state(
	_effect_ref: PF_ProgPtr,
	_param_index: PF_ParamIndex,
	_startPT0: *const A_Time,
	_durationPT0: *const A_Time,
	stateP: *mut PF_State,
) -> PF_Err {
	if stateP.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}
	// SAFETY: `stateP` is non-null; `PF_State` is a plain byte buffer.
	unsafe { std::ptr::write_bytes(stateP, 0, 1) };
	PF_Err_NONE as PF_Err
}

/// Report two states as identical: aexlo has a single static frame, so parameter
/// state never varies over time.
unsafe extern "C" fn are_states_identical(
	_effect_ref: PF_ProgPtr,
	_state1P: *const PF_State,
	_state2P: *const PF_State,
	samePB: *mut A_Boolean,
) -> PF_Err {
	if samePB.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}
	unsafe { *samePB = 1 };
	PF_Err_NONE as PF_Err
}

/// Report a checkout as identical across the two times (no animation).
unsafe extern "C" fn is_identical_checkout(
	_effect_ref: PF_ProgPtr,
	_param_index: PF_ParamIndex,
	_what_time1: A_long,
	_time_step1: A_long,
	_time_scale1: A_u_long,
	_what_time2: A_long,
	_time_step2: A_long,
	_time_scale2: A_u_long,
	identicalPB: *mut PF_Boolean,
) -> PF_Err {
	if identicalPB.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}
	unsafe { *identicalPB = 1 };
	PF_Err_NONE as PF_Err
}

/// Report that no keyframe was found (aexlo has no keyframes).
unsafe extern "C" fn find_keyframe_time(
	_effect_ref: PF_ProgPtr,
	_param_index: PF_ParamIndex,
	_what_time: A_long,
	_time_scale: A_u_long,
	_time_dir: PF_TimeDir,
	foundPB: *mut PF_Boolean,
	_key_indexP0: *mut PF_KeyIndex,
	_key_timeP0: *mut A_long,
	_key_timescaleP0: *mut A_u_long,
) -> PF_Err {
	if !foundPB.is_null() {
		unsafe { *foundPB = 0 };
	}
	PF_Err_NONE as PF_Err
}

/// Report zero keyframes for the parameter.
unsafe extern "C" fn get_keyframe_count(
	_effect_ref: PF_ProgPtr,
	_param_index: PF_ParamIndex,
	key_countP: *mut PF_KeyIndex,
) -> PF_Err {
	if key_countP.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}
	unsafe { *key_countP = 0 };
	PF_Err_NONE as PF_Err
}

/// There are no keyframes to check out.
unsafe extern "C" fn checkout_keyframe(
	_effect_ref: PF_ProgPtr,
	_param_index: PF_ParamIndex,
	_key_index: PF_KeyIndex,
	_key_timeP0: *mut A_long,
	_key_timescaleP0: *mut A_u_long,
	_paramP0: *mut PF_ParamDef,
) -> PF_Err {
	PF_Err_BAD_CALLBACK_PARAM as PF_Err
}

/// Checking a keyframe back in is a no-op (none were ever checked out).
unsafe extern "C" fn checkin_keyframe(_effect_ref: PF_ProgPtr, _paramP: *mut PF_ParamDef) -> PF_Err {
	PF_Err_NONE as PF_Err
}

/// There are no keyframes, so any index maps to time zero.
unsafe extern "C" fn key_index_to_time(
	_effect_ref: PF_ProgPtr,
	_param_index: PF_ParamIndex,
	_key_indexP: PF_KeyIndex,
	key_timeP: *mut A_long,
	key_timescaleP: *mut A_u_long,
) -> PF_Err {
	if !key_timeP.is_null() {
		unsafe { *key_timeP = 0 };
	}
	if !key_timescaleP.is_null() {
		unsafe { *key_timescaleP = 1 };
	}
	PF_Err_NONE as PF_Err
}

/// Build the `PF_ParamUtilsSuite3` vtable.
///
/// `const` so it can initialize the shared [`SUITE_CONTAINER`](crate::suites::SUITE_CONTAINER)
/// static; the suite is a stateless table of function pointers.
pub const fn create_param_utils_suite_3() -> PF_ParamUtilsSuite3 {
	PF_ParamUtilsSuite3 {
		PF_UpdateParamUI: Some(update_param_ui),
		PF_GetCurrentState: Some(get_current_state),
		PF_AreStatesIdentical: Some(are_states_identical),
		PF_IsIdenticalCheckout: Some(is_identical_checkout),
		PF_FindKeyframeTime: Some(find_keyframe_time),
		PF_GetKeyframeCount: Some(get_keyframe_count),
		PF_CheckoutKeyframe: Some(checkout_keyframe),
		PF_CheckinKeyframe: Some(checkin_keyframe),
		PF_KeyIndexToTime: Some(key_index_to_time),
	}
}
