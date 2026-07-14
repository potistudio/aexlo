use after_effects_sys::*;

use crate::PluginInstance;
use crate::core::diagnostics::diag;

// ============================================================================
// Parameter Management
// ============================================================================

unsafe extern "C" fn checkout_param_stub(
	effect_ref: PF_ProgPtr,
	index: PF_ParamIndex,
	_what_time: A_long,
	_time_step: A_long,
	_time_scale: A_u_long,
	param: *mut PF_ParamDef,
) -> PF_Err {
	let index = index as usize;

	//== Validation ==//
	if effect_ref.is_null() {
		log::error!("checkout_param: effect_ref is null");
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	if param.is_null() {
		log::warn!("checkout_param: param pointer is null");
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	if index == 0 {
		log::warn!("checkout_param: index 0 is reserved for input layer");
		return PF_Err_INVALID_INDEX as PF_Err;
	}

	//== Implementation ==//
	// Never panic here: unwinding across the plugin's C frames is UB, so an
	// unknown effect_ref is reported as a callback error instead.
	let Some(instance) = PluginInstance::get_instance_ptr(effect_ref) else {
		log::error!(
			"checkout_param: no instance found for effect_ref {:#x}",
			effect_ref as usize
		);
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	};
	let instance = unsafe { instance.as_ref() };

	if index >= instance.param_count() {
		log::error!(
			"checkout_param: index {} out of bounds (total={})",
			index,
			instance.param_count()
		);
		return PF_Err_INVALID_INDEX as PF_Err;
	}

	// SAFETY: We have validated that the index is within bounds and the param pointer is not null.
	let src_param = instance.param_by_index(index).unwrap();
	unsafe { *param = *src_param };

	diag!("InteractCallbacks/checkout_param",
		"effect_ref" => format!("{:#x}", effect_ref as usize),
		"index" => index,
		"what_time" => _what_time,
		"time_step" => _time_step,
		"time_scale" => _time_scale,
		"param (out)" => format!("{:#x}", param as usize);
		result: format!("param is set to {:#x}", param as usize),
	);

	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn checkin_param_stub(_effect_ref: PF_ProgPtr, _param: *mut PF_ParamDef) -> PF_Err {
	if _param.is_null() {
		log::warn!("checkin_param: param pointer is null");
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	// For now, just log - no-op for checkin
	log::debug!("checkin_param called for effect_ref={:#x}", _effect_ref as usize);
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn add_param_sys(effect_ref: PF_ProgPtr, _index: PF_ParamIndex, def: PF_ParamDefPtr) -> PF_Err {
	if def.is_null() {
		log::error!("add_param: def is null");
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	// Copy the param definition and store it
	let param = unsafe { *def };

	// Store the param in instance via ParamManager
	if let Err(e) = crate::host::params::add_param_to_instance(effect_ref, param) {
		log::error!("add_param: failed to add param: {}", e);
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	diag!("InteractCallbacks/add_param",
		"effect_ref" => format!("{:#x}", effect_ref as usize),
		"index" => _index,
		"def" => format!("{:#x}", def as usize),
	);

	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn abort_stub(_effect_ref: PF_ProgPtr) -> PF_Err {
	log::warn!("STUB: abort called");
	0
}

unsafe extern "C" fn progress_stub(_effect_ref: PF_ProgPtr, _current: A_long, _total: A_long) -> PF_Err {
	log::warn!("STUB: progress called");
	0
}

unsafe extern "C" fn register_ui_stub(_effect_ref: PF_ProgPtr, _custom_info: *mut PF_CustomUIInfo) -> PF_Err {
	log::warn!("STUB: register_ui called");
	PF_Err_NONE as PF_Err
}

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
	log::warn!("STUB: checkout_layer_audio called");
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn checkin_layer_audio_stub(_effect_ref: PF_ProgPtr, _audio: PF_LayerAudio) -> PF_Err {
	log::warn!("STUB: checkin_layer_audio called");
	PF_Err_NONE as PF_Err
}

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
	log::warn!("STUB: get_audio_data called");
	PF_Err_NONE as PF_Err
}

// ============================================================================
// Factory Function
// ============================================================================

/// Creates a `PF_InteractCallbacks` instance with all callbacks populated.
pub fn create_interact_callbacks() -> PF_InteractCallbacks {
	PF_InteractCallbacks {
		checkout_param: Some(checkout_param_stub),
		checkin_param: Some(checkin_param_stub),
		add_param: Some(add_param_sys),
		abort: Some(abort_stub),
		progress: Some(progress_stub),
		register_ui: Some(register_ui_stub),
		checkout_layer_audio: Some(checkout_layer_audio_stub),
		checkin_layer_audio: Some(checkin_layer_audio_stub),
		get_audio_data: Some(get_audio_data_stub),
		reserved_str: [std::ptr::null_mut(); 3],
		reserved: [std::ptr::null_mut(); 10],
	}
}
