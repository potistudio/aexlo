use after_effects_sys::*;

// ============================================================================
// Stub Implementations (Logging Only)
// ============================================================================

unsafe extern "C" fn checkout_param_stub(
	_effect_ref: PF_ProgPtr,
	_index: PF_ParamIndex,
	_what_time: A_long,
	_time_step: A_long,
	_time_scale: A_u_long,
	_param: *mut PF_ParamDef,
) -> PF_Err {
	log::warn!("STUB: checkout_param called");
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn checkin_param_stub(
	_effect_ref: PF_ProgPtr,
	_param: *mut PF_ParamDef,
) -> PF_Err {
	log::warn!("STUB: checkin_param called");
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn add_param_impl(
	_effect_ref: PF_ProgPtr,
	_index: PF_ParamIndex,
	def: PF_ParamDefPtr,
) -> PF_Err {
	if def.is_null() {
		log::warn!("add_param: def is null");
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	// Copy the param definition and store it
	let param = unsafe { *def };
	crate::param_manager::add_param(_effect_ref, param);

	log::info!(
		"add_param: stored param, effect_ref={:#x}, total={}",
		_effect_ref as usize,
		crate::param_manager::get_params_count(_effect_ref)
	);

	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn abort_stub(_effect_ref: PF_ProgPtr) -> PF_Err {
	log::warn!("STUB: abort called");
	0
}

unsafe extern "C" fn progress_stub(
	_effect_ref: PF_ProgPtr,
	_current: A_long,
	_total: A_long,
) -> PF_Err {
	log::warn!("STUB: progress called");
	0
}

unsafe extern "C" fn register_ui_stub(
	_effect_ref: PF_ProgPtr,
	_custom_info: *mut PF_CustomUIInfo,
) -> PF_Err {
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

unsafe extern "C" fn checkin_layer_audio_stub(
	_effect_ref: PF_ProgPtr,
	_audio: PF_LayerAudio,
) -> PF_Err {
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
		add_param: Some(add_param_impl),
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
