//! `PF_ParamUtilsSuite3` - parameter UI and keyframe queries.
//!
//! The keyframe and state queries read the parameter's track
//! (`PluginInstance::set_param_track`) through the evaluator streams and
//! checkouts use, so all of them agree on a value at a time. A state follows
//! parameter values and keyframes, not the pixels of layer parameters.
//! [`update_param_ui`] lets a plugin refresh a parameter's UI state
//! (twirl collapse, disabled, name, …) in response to a user edit.

use crate::core::diagnostics::diag;
use crate::sys::*;

use crate::PluginInstance;
use crate::animation::{Interpolation, TimePoint};
use std::hash::{Hash, Hasher};

/// The instance behind `effect_ref`.
///
/// # Safety
/// `effect_ref` must be null or the instance currently being called into, and
/// the borrow must not outlive the call.
unsafe fn instance_of<'a>(effect_ref: PF_ProgPtr) -> Option<&'a PluginInstance> {
	PluginInstance::get_instance_ptr(effect_ref).map(|instance| unsafe { instance.as_ref() })
}

/// The keyframes of a parameter, or none.
///
/// # Safety
/// See [`instance_of`].
unsafe fn keys_of(effect_ref: PF_ProgPtr, index: PF_ParamIndex) -> Vec<(TimePoint, Interpolation)> {
	unsafe { instance_of(effect_ref) }
		.zip(usize::try_from(index).ok())
		.and_then(|(instance, index)| instance.param_track(index))
		.map(|track| track.keys().iter().map(|key| (key.time, key.interpolation)).collect())
		.unwrap_or_default()
}

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
	crate::core::ffi::guard("update_param_ui", || {
		diag!("PF_ParamUtilsSuite3/PF_UpdateParamUI", "param_index" => param_index);
		if effect_ref.is_null() || defP.is_null() {
			return PF_Err_BAD_CALLBACK_PARAM;
		}

		let Some(mut instance) = PluginInstance::get_instance_ptr(effect_ref) else {
			log::error!("PF_UpdateParamUI: no plugin instance for effect_ref");
			return PF_Err_BAD_CALLBACK_PARAM;
		};

		// SAFETY: `defP` is non-null (checked) and, per the suite contract, points to a
		// valid `PF_ParamDef` for the duration of the call.
		let def = unsafe { &*defP };
		// SAFETY: `effect_ref` identifies this instance; the plugin is not concurrently
		// mutating it while inside its own suite callback.
		unsafe { instance.as_mut() }.update_param_ui(param_index as usize, def);

		PF_Err_NONE
	})
}

/// A state is a hash of the parameter's value or keyframes, so it changes when
/// they do. The time range is not narrowed: a change anywhere counts.
unsafe extern "C" fn get_current_state(
	effect_ref: PF_ProgPtr,
	param_index: PF_ParamIndex,
	_startPT0: *const A_Time,
	_durationPT0: *const A_Time,
	stateP: *mut PF_State,
) -> PF_Err {
	crate::core::ffi::guard("get_current_state", || {
		diag!("PF_ParamUtilsSuite3/PF_GetCurrentState", "param_index" => param_index);
		if stateP.is_null() {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		// SAFETY: `stateP` is non-null; `PF_State` is a plain byte buffer.
		unsafe { std::ptr::write_bytes(stateP, 0, 1) };
		let Some(instance) = (unsafe { instance_of(effect_ref) }) else {
			return PF_Err_NONE;
		};
		let wanted = match usize::try_from(param_index) {
			Ok(index) => index..index.saturating_add(1),
			// The negative indices stand for all of them.
			Err(_) => 0..instance.param_count(),
		};
		let mut hash = std::collections::hash_map::DefaultHasher::new();
		for index in wanted {
			index.hash(&mut hash);
			match instance.param_track(index) {
				Some(track) => format!("{track:?}").hash(&mut hash),
				None => format!("{:?}", instance.get_param(index)).hash(&mut hash),
			}
		}
		let hash = hash.finish().to_ne_bytes();
		let len = hash.len().min(std::mem::size_of::<PF_State>());
		// SAFETY: at most `size_of::<PF_State>()` bytes into that buffer.
		unsafe { std::ptr::copy_nonoverlapping(hash.as_ptr(), stateP.cast::<u8>(), len) };
		PF_Err_NONE
	})
}

/// States are identical when their hashes are.
unsafe extern "C" fn are_states_identical(
	_effect_ref: PF_ProgPtr,
	state1P: *const PF_State,
	state2P: *const PF_State,
	samePB: *mut A_Boolean,
) -> PF_Err {
	crate::core::ffi::guard("are_states_identical", || {
		diag!("PF_ParamUtilsSuite3/PF_AreStatesIdentical");
		if samePB.is_null() {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		let same = state1P.is_null() || state2P.is_null() || {
			let len = std::mem::size_of::<PF_State>();
			// SAFETY: both are non-null `PF_State`s, which are plain bytes.
			unsafe {
				std::slice::from_raw_parts(state1P.cast::<u8>(), len)
					== std::slice::from_raw_parts(state2P.cast::<u8>(), len)
			}
		};
		unsafe { *samePB = same as A_Boolean };
		PF_Err_NONE
	})
}

/// Two checkouts are identical when the track evaluates to the same value.
unsafe extern "C" fn is_identical_checkout(
	effect_ref: PF_ProgPtr,
	param_index: PF_ParamIndex,
	what_time1: A_long,
	_time_step1: A_long,
	time_scale1: A_u_long,
	what_time2: A_long,
	_time_step2: A_long,
	time_scale2: A_u_long,
	identicalPB: *mut PF_Boolean,
) -> PF_Err {
	crate::core::ffi::guard("is_identical_checkout", || {
		diag!("PF_ParamUtilsSuite3/PF_IsIdenticalCheckout", "param_index" => param_index);
		if identicalPB.is_null() {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		let same = (|| {
			// SAFETY: as for every suite call.
			let instance = unsafe { instance_of(effect_ref) }?;
			let track = instance.param_track(usize::try_from(param_index).ok()?)?;
			let (a, b) = (
				TimePoint::new(what_time1, time_scale1)?,
				TimePoint::new(what_time2, time_scale2)?,
			);
			Some(track.at(a) == track.at(b))
		})()
		.unwrap_or(true);
		unsafe { *identicalPB = same as PF_Boolean };
		PF_Err_NONE
	})
}

/// The nearest keyframe in `time_dir` of `what_time`.
unsafe extern "C" fn find_keyframe_time(
	effect_ref: PF_ProgPtr,
	param_index: PF_ParamIndex,
	what_time: A_long,
	time_scale: A_u_long,
	time_dir: PF_TimeDir,
	foundPB: *mut PF_Boolean,
	key_indexP0: *mut PF_KeyIndex,
	key_timeP0: *mut A_long,
	key_timescaleP0: *mut A_u_long,
) -> PF_Err {
	crate::core::ffi::guard("find_keyframe_time", || {
		diag!("PF_ParamUtilsSuite3/PF_FindKeyframeTime", "param_index" => param_index);
		let keys = unsafe { keys_of(effect_ref, param_index) };
		let at = TimePoint::new(what_time, time_scale);
		#[allow(non_upper_case_globals)]
		let found = at.and_then(|at| match time_dir {
			PF_TimeDir_GREATER_THAN => keys.iter().position(|key| key.0 > at),
			PF_TimeDir_GREATER_THAN_OR_EQUAL => keys.iter().position(|key| key.0 >= at),
			PF_TimeDir_LESS_THAN => keys.iter().rposition(|key| key.0 < at),
			PF_TimeDir_LESS_THAN_OR_EQUAL => keys.iter().rposition(|key| key.0 <= at),
			_ => None,
		});
		unsafe {
			if let Some(out) = foundPB.as_mut() {
				*out = found.is_some() as PF_Boolean;
			}
			let Some(found) = found else {
				return PF_Err_NONE;
			};
			if let Some(out) = key_indexP0.as_mut() {
				*out = found as PF_KeyIndex;
			}
			if let Some(out) = key_timeP0.as_mut() {
				*out = keys[found].0.value();
			}
			if let Some(out) = key_timescaleP0.as_mut() {
				*out = keys[found].0.scale();
			}
		}
		PF_Err_NONE
	})
}

unsafe extern "C" fn get_keyframe_count(
	effect_ref: PF_ProgPtr,
	param_index: PF_ParamIndex,
	key_countP: *mut PF_KeyIndex,
) -> PF_Err {
	crate::core::ffi::guard("get_keyframe_count", || {
		diag!("PF_ParamUtilsSuite3/PF_GetKeyframeCount", "param_index" => param_index);
		if key_countP.is_null() {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		unsafe { *key_countP = keys_of(effect_ref, param_index).len() as PF_KeyIndex };
		PF_Err_NONE
	})
}

/// The parameter as it is at keyframe `key_index`.
unsafe extern "C" fn checkout_keyframe(
	effect_ref: PF_ProgPtr,
	param_index: PF_ParamIndex,
	key_index: PF_KeyIndex,
	key_timeP0: *mut A_long,
	key_timescaleP0: *mut A_u_long,
	paramP0: *mut PF_ParamDef,
) -> PF_Err {
	crate::core::ffi::guard("checkout_keyframe", || {
		diag!("PF_ParamUtilsSuite3/PF_CheckoutKeyframe", "param_index" => param_index, "key_index" => key_index);
		let keys = unsafe { keys_of(effect_ref, param_index) };
		let Some(&(time, _)) = usize::try_from(key_index).ok().and_then(|at| keys.get(at)) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		unsafe {
			if let Some(out) = key_timeP0.as_mut() {
				*out = time.value();
			}
			if let Some(out) = key_timescaleP0.as_mut() {
				*out = time.scale();
			}
			if !paramP0.is_null()
				&& let Some(def) = instance_of(effect_ref)
					.and_then(|instance| instance.param_at(usize::try_from(param_index).ok()?, Some(time)))
			{
				paramP0.write(def);
			}
		}
		PF_Err_NONE
	})
}

/// Checking a keyframe back in is a no-op: the checkout was a copy.
unsafe extern "C" fn checkin_keyframe(_effect_ref: PF_ProgPtr, _paramP: *mut PF_ParamDef) -> PF_Err {
	crate::core::ffi::guard("checkin_keyframe", || {
		diag!("PF_ParamUtilsSuite3/PF_CheckinKeyframe");
		PF_Err_NONE
	})
}

/// Without keyframes any index maps to time zero.
unsafe extern "C" fn key_index_to_time(
	effect_ref: PF_ProgPtr,
	param_index: PF_ParamIndex,
	key_indexP: PF_KeyIndex,
	key_timeP: *mut A_long,
	key_timescaleP: *mut A_u_long,
) -> PF_Err {
	crate::core::ffi::guard("key_index_to_time", || {
		diag!("PF_ParamUtilsSuite3/PF_KeyIndexToTime", "param_index" => param_index);
		let keys = unsafe { keys_of(effect_ref, param_index) };
		let time = match usize::try_from(key_indexP).ok().and_then(|at| keys.get(at)) {
			Some(key) => key.0,
			None if keys.is_empty() => TimePoint::ZERO,
			None => return PF_Err_INVALID_INDEX,
		};
		if !key_timeP.is_null() {
			unsafe { *key_timeP = time.value() };
		}
		if !key_timescaleP.is_null() {
			unsafe { *key_timescaleP = time.scale() };
		}
		PF_Err_NONE
	})
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
