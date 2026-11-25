use crate::sys::*;

use crate::PluginInstance;
use crate::core::diagnostics::diag;
use crate::host::app::with_app_host_ffi;

// ============================================================================
// Parameter Management
// ============================================================================

/// How many indices past the declared parameters check out successfully, as
/// empty `PF_Param_NO_DATA` parameters.
///
/// The SDK does not document this; it is inferred from Red Giant Universe
/// 2026.5. Its pre-render checks out every index up to `num_params + 1`. On an
/// error it frees that parameter's block but keeps the pointer, and its
/// pre-render delete callback frees it again, so the effect only works in a
/// host where both checkouts succeed. Indices beyond these are still errors.
///
/// The measurement predates the current `InData.num_params`: with 0 in
/// `SMART_PRE_RENDER` (see `in_data_num_params`), Universe reaches index 1
/// only, which is a declared parameter, so nothing here exercises the two
/// trailing indices. Re-measure Universe before relying on the constant.
const TRAILING_PARAMS: usize = 2;

unsafe extern "C" fn checkout_param_sys(
	effect_ref: PF_ProgPtr,
	index: PF_ParamIndex,
	what_time: A_long,
	_time_step: A_long,
	time_scale: A_u_long,
	param: *mut PF_ParamDef,
) -> PF_Err {
	crate::core::ffi::guard("checkout_param_sys", || {
		let index = index as usize;

		//== Validation ==//
		if effect_ref.is_null() {
			log::error!("checkout_param: effect_ref is null");
			return PF_Err_BAD_CALLBACK_PARAM;
		}

		if param.is_null() {
			log::warn!("checkout_param: param pointer is null");
			return PF_Err_BAD_CALLBACK_PARAM;
		}

		//== Implementation ==//
		// Never panic here: unwinding across the plugin's C frames is UB, so an
		// unknown effect_ref is reported as a callback error instead.
		let Some(instance) = PluginInstance::get_instance_ptr(effect_ref) else {
			log::error!(
				"checkout_param: no instance found for effect_ref {:#x}",
				effect_ref as usize
			);
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		// SAFETY: `effect_ref` is the `PluginInstance` aexlo handed the plugin in `in_data`, and the plugin calls back during one of that instance's commands (see `PluginInstance::get_instance_ptr`).
		let instance = unsafe { instance.as_ref() };

		let count = instance.param_count();
		if (count..count + TRAILING_PARAMS).contains(&index) {
			// SAFETY: an all-zero `PF_ParamDef` is a valid value of that C struct.
			let mut empty: PF_ParamDef = unsafe { std::mem::zeroed() };
			empty.param_type = PF_Param_NO_DATA as PF_ParamType;
			// SAFETY: `param` is non-null (checked above) and points at the plugin's `PF_ParamDef` slot.
			unsafe { param.write(empty) };
			log::debug!("checkout_param: index {index} is past the {count} declared; returning an empty parameter");
			return PF_Err_NONE;
		}

		if index >= count {
			// The plugin passed this struct uninitialized and may read it back
			// even after the error, so the failed checkout reads as an empty
			// parameter rather than stack garbage.
			// SAFETY: `param` is non-null (checked above) and points at the plugin's `PF_ParamDef` slot; `write` doesn't read the uninitialized value.
			unsafe { param.write(std::mem::zeroed()) };
			log::debug!(
				"checkout_param: index {} out of bounds (total={})",
				index,
				instance.param_count()
			);
			return PF_Err_INVALID_INDEX;
		}

		if let Some(at) = crate::TimePoint::new(what_time, time_scale) {
			instance.record_checkout_time(at);
		}
		// The value at the time asked for; a zero scale names no time, so now.
		let src_param = instance
			.param_at(index, crate::TimePoint::new(what_time, time_scale))
			.expect("index checked above");
		// SAFETY: `param` is non-null (checked above) and points at the plugin's `PF_ParamDef` slot.
		unsafe { *param = src_param };
		// Checking a param back in matters for layer params (their pixels).
		if src_param.param_type == PF_Param_LAYER as PF_ParamType {
			crate::strict::checkout(crate::CheckoutKind::CheckoutParam, index as i32, param as usize);
		}

		diag!("InteractCallbacks/checkout_param",
			"effect_ref" => format!("{:#x}", effect_ref as usize),
			"index" => index,
			"what_time" => what_time,
			"time_step" => _time_step,
			"time_scale" => time_scale,
			"param (out)" => format!("{:#x}", param as usize);
			result: format!("param is set to {:#x}", param as usize),
		);

		PF_Err_NONE
	})
}

unsafe extern "C" fn checkin_param_sys(_effect_ref: PF_ProgPtr, _param: *mut PF_ParamDef) -> PF_Err {
	crate::core::ffi::guard("checkin_param_sys", || {
		if _param.is_null() {
			log::warn!("checkin_param: param pointer is null");
			return PF_Err_BAD_CALLBACK_PARAM;
		}

		// SAFETY: `_param` is non-null (checked above) and points at a parameter the plugin checked out.
		if unsafe { (*_param).param_type } == PF_Param_LAYER as PF_ParamType {
			crate::strict::checkout(crate::CheckoutKind::CheckinParam, 0, _param as usize);
		}
		log::debug!("checkin_param called for effect_ref={:#x}", _effect_ref as usize);
		PF_Err_NONE
	})
}

unsafe extern "C" fn add_param_sys(effect_ref: PF_ProgPtr, _index: PF_ParamIndex, def: PF_ParamDefPtr) -> PF_Err {
	crate::core::ffi::guard("add_param_sys", || {
		if def.is_null() {
			log::error!("add_param: def is null");
			return PF_Err_BAD_CALLBACK_PARAM;
		}

		// Copy the param definition and store it
		// SAFETY: `def` is non-null (checked above) and points at the plugin's parameter definition for this call.
		let param = unsafe { *def };

		// Store the param in instance via ParamManager
		if let Err(e) = crate::host::params::add_param_to_instance(effect_ref, param) {
			log::error!("add_param: failed to add param: {}", e);
			return PF_Err_BAD_CALLBACK_PARAM;
		}

		diag!("InteractCallbacks/add_param",
			"effect_ref" => format!("{:#x}", effect_ref as usize),
			"index" => _index,
			"def" => format!("{:#x}", def as usize),
		);

		PF_Err_NONE
	})
}

/// `PF_ABORT`: `PF_Interrupt_CANCEL` once the host reports the user cancelled.
///
/// # Safety
///
/// `effect_ref` is an opaque key and is never dereferenced, so any value,
/// null included, is accepted.
unsafe extern "C" fn abort_sys(effect_ref: PF_ProgPtr) -> PF_Err {
	crate::core::ffi::guard("abort_sys", || {
		with_app_host_ffi(|host| {
			if host.abort_requested(effect_ref as usize) {
				PF_Interrupt_CANCEL
			} else {
				PF_Err_NONE
			}
		})
	})
}

/// `PF_PROGRESS`: report progress to the host; `PF_Interrupt_CANCEL` if it
/// cancels the render.
///
/// # Safety
///
/// `effect_ref` is an opaque key and is never dereferenced, so any value,
/// null included, is accepted.
unsafe extern "C" fn progress_sys(effect_ref: PF_ProgPtr, current: A_long, total: A_long) -> PF_Err {
	crate::core::ffi::guard("progress_sys", || {
		with_app_host_ffi(|host| {
			if host.render_progress(effect_ref as usize, current, total) {
				PF_Err_NONE
			} else {
				PF_Interrupt_CANCEL
			}
		})
	})
}

/// Accept a custom-UI registration. aexlo has no UI and never sends
/// `PF_Cmd_EVENT`, so the registration has nothing to drive; succeeding lets
/// plugins with custom controls (e.g. status readouts) set up normally.
unsafe extern "C" fn register_ui_sys(_effect_ref: PF_ProgPtr, custom_info: *mut PF_CustomUIInfo) -> PF_Err {
	crate::core::ffi::guard("register_ui_sys", || {
		// SAFETY: `as_ref` handles null; otherwise `custom_info` points at the plugin's `PF_CustomUIInfo` for this call.
		if let Some(info) = unsafe { custom_info.as_ref() } {
			log::info!(
				"register_ui: events={:#x} comp={}x{} layer={}x{} preview={}x{}",
				info.events,
				info.comp_ui_width,
				info.comp_ui_height,
				info.layer_ui_width,
				info.layer_ui_height,
				info.preview_ui_width,
				info.preview_ui_height
			);
		}
		PF_Err_NONE
	})
}

/// `PF_InteractCallbacks::checkout_layer_audio`: aexlo carries no audio, so
/// the checkout is refused.
///
/// # Safety
///
/// No argument is read or written; `_audio` is never dereferenced.
unsafe extern "C" fn checkout_layer_audio_stub(
	_effect_ref: PF_ProgPtr,
	_index: PF_ParamIndex,
	_start_time: A_long,
	_duration: A_long,
	_time_scale: A_u_long,
	_rate: PF_UFixed,
	_bytes_per_sample: A_long,
	_num_channels: A_long,
	_fmt_signed: A_long,
	_audio: *mut PF_LayerAudio,
) -> PF_Err {
	crate::core::ffi::guard("checkout_layer_audio_stub", || {
		log::warn!("STUB: checkout_layer_audio called");
		PF_Err_INVALID_CALLBACK
	})
}

/// `PF_InteractCallbacks::checkin_layer_audio`: refused, since no audio was
/// ever checked out.
///
/// # Safety
///
/// No argument is read; `_audio` is never dereferenced.
unsafe extern "C" fn checkin_layer_audio_stub(_effect_ref: PF_ProgPtr, _audio: PF_LayerAudio) -> PF_Err {
	crate::core::ffi::guard("checkin_layer_audio_stub", || {
		log::warn!("STUB: checkin_layer_audio called");
		PF_Err_INVALID_CALLBACK
	})
}

/// `PF_InteractCallbacks::get_audio_data`: refused, since no audio was ever
/// checked out.
///
/// # Safety
///
/// No argument is read or written; none of the out-pointers is dereferenced.
unsafe extern "C" fn get_audio_data_stub(
	_effect_ref: PF_ProgPtr,
	_audio: PF_LayerAudio,
	_data: *mut PF_SndSamplePtr,
	_num_samples: *mut A_long,
	_rate: *mut PF_UFixed,
	_bytes_per_sample: *mut A_long,
	_num_channels: *mut A_long,
	_fmt_signed: *mut A_long,
) -> PF_Err {
	crate::core::ffi::guard("get_audio_data_stub", || {
		log::warn!("STUB: get_audio_data called");
		PF_Err_INVALID_CALLBACK
	})
}

// ============================================================================
// Factory Function
// ============================================================================

/// Creates a `PF_InteractCallbacks` instance with all callbacks populated.
pub fn create_interact_callbacks() -> PF_InteractCallbacks {
	PF_InteractCallbacks {
		checkout_param: Some(checkout_param_sys),
		checkin_param: Some(checkin_param_sys),
		add_param: Some(add_param_sys),
		abort: Some(abort_sys),
		progress: Some(progress_sys),
		register_ui: Some(register_ui_sys),
		checkout_layer_audio: Some(checkout_layer_audio_stub),
		checkin_layer_audio: Some(checkin_layer_audio_stub),
		get_audio_data: Some(get_audio_data_stub),
		reserved_str: [std::ptr::null_mut(); 3],
		reserved: [std::ptr::null_mut(); 10],
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn unavailable_audio_callbacks_report_errors() {
		let cb = create_interact_callbacks();
		let mut audio = std::ptr::null_mut();
		let result =
			unsafe { cb.checkout_layer_audio.unwrap()(std::ptr::null_mut(), 0, 0, 1, 1, 0, 1, 1, 0, &mut audio) };
		assert_eq!(result, PF_Err_INVALID_CALLBACK);
		assert!(audio.is_null());
		assert_eq!(
			unsafe { cb.checkin_layer_audio.unwrap()(std::ptr::null_mut(), audio) },
			PF_Err_INVALID_CALLBACK
		);
	}
	#[test]
	fn checkout_param_answers_the_two_indices_past_the_declared_params() {
		unsafe extern "C" fn entry(
			cmd: PF_Cmd,
			input: *mut PF_InData,
			_: *mut PF_OutData,
			_: PF_ParamList,
			_: *mut PF_LayerDef,
			_: *mut std::ffi::c_void,
		) -> PF_Err {
			if cmd != PF_Cmd_SMART_PRE_RENDER {
				return PF_Err_NONE;
			}
			let input = unsafe { &*input };
			let checkout = input.inter.checkout_param.unwrap();
			// The stub declares only the implicit layer parameter (index 0).
			// The two indices past it answer as empty parameters and the third is
			// out of bounds; checkout validates against the declared count, not
			// against InData.num_params (see `in_data_num_params`).
			for index in 1..4 {
				let mut def = std::mem::MaybeUninit::<PF_ParamDef>::uninit();
				// SAFETY: a byte fill of this frame's own slot, standing in for stack garbage.
				unsafe { std::ptr::write_bytes(def.as_mut_ptr(), 0xa5, 1) };
				let err = unsafe { checkout(input.effect_ref, index, 0, 1, 30, def.as_mut_ptr()) };
				let expected = if index < 3 { PF_Err_NONE } else { PF_Err_INVALID_INDEX };
				// Written either way, so it is initialised and has no stray handle.
				let def = unsafe { def.assume_init() };
				if err != expected || !unsafe { def.u.arb_d.value }.is_null() {
					return PF_Err_BAD_CALLBACK_PARAM;
				}
				if index < 3 && def.param_type != PF_Param_NO_DATA as PF_ParamType {
					return PF_Err_BAD_CALLBACK_PARAM;
				}
			}
			PF_Err_NONE
		}
		let mut instance = unsafe { crate::Host::get().entry_loader(entry) }
			.size(16, 16)
			.load()
			.unwrap();
		instance.render_with(crate::RenderPath::Smart).unwrap();
	}

	use crate::host::app::{AppHost, TEST_APP_HOST};

	struct Cancelling;
	impl AppHost for Cancelling {
		fn abort_requested(&self, _: usize) -> bool {
			true
		}
		fn render_progress(&self, _: usize, current: i32, total: i32) -> bool {
			current * 2 < total
		}
	}

	#[test]
	fn abort_and_progress_follow_the_host() {
		let cb = create_interact_callbacks();
		let null = std::ptr::null_mut();
		unsafe {
			assert_eq!(cb.abort.unwrap()(null), PF_Err_NONE);
			TEST_APP_HOST.set(Some(Box::new(Cancelling)));
			assert_eq!(cb.abort.unwrap()(null), PF_Interrupt_CANCEL);
			assert_eq!(cb.progress.unwrap()(null, 1, 10), PF_Err_NONE);
			assert_eq!(cb.progress.unwrap()(null, 6, 10), PF_Interrupt_CANCEL);
		}
		TEST_APP_HOST.set(None);
	}
}
