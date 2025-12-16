use crate::diagnostics::DiagnosticBuilder;
use after_effects_sys::*;

pub(crate) unsafe extern "C" fn checkout_param_sys(
	effect_ref: PF_ProgPtr,
	index: PF_ParamIndex,
	what_time: A_long,
	time_step: A_long,
	time_scale: A_u_long,
	param: *mut PF_ParamDef,
) -> PF_Err {
	#[cfg(feature = "diagnostics")]
	DiagnosticBuilder::new()
		.set_name("InteractCallbacks/CheckoutParam")
		.add_arg("effect_ref", effect_ref as usize)
		.add_arg("index", index)
		.add_arg("what_time", what_time)
		.add_arg("time_step", time_step)
		.add_arg("time_scale", time_scale)
		.add_arg("param", format! {"{:?}", param})
		.set_result(0)
		.emit();

	PF_Err_NONE as PF_Err
}

pub(crate) unsafe extern "C" fn checkin_param_sys(
	effect_ref: PF_ProgPtr,
	param: *mut PF_ParamDef,
) -> PF_Err {
	#[cfg(feature = "diagnostics")]
	DiagnosticBuilder::new()
		.set_name("InteractCallbacks/CheckinParam")
		.add_arg("effect_ref", effect_ref as usize)
		.add_arg("param", format! {"{:?}", param})
		.set_result(0)
		.emit();

	PF_Err_NONE as PF_Err
}

pub(crate) unsafe extern "C" fn add_param_sys(
	effect_ref: PF_ProgPtr,
	index: PF_ParamIndex,
	def: PF_ParamDefPtr,
) -> PF_Err {
	#[cfg(feature = "diagnostics")]
	DiagnosticBuilder::new()
		.set_name("InteractCallbacks/AddParam")
		.add_arg("effect_ref", effect_ref as usize)
		.add_arg("index", index)
		.add_arg("def", format!("{:?}", def))
		.set_result(0)
		.emit();

	PF_Err_NONE as PF_Err
}

pub(crate) unsafe extern "C" fn abort_sys(effect_ref: PF_ProgPtr) -> PF_Err {
	#[cfg(feature = "diagnostics")]
	DiagnosticBuilder::new()
		.set_name("InteractCallbacks/Abort")
		.add_arg("effect_ref", effect_ref as usize)
		.set_result(0)
		.emit();

	PF_Err_NONE as PF_Err
}

pub(crate) unsafe extern "C" fn progress_sys(
	effect_ref: PF_ProgPtr,
	current: A_long,
	total: A_long,
) -> PF_Err {
	#[cfg(feature = "diagnostics")]
	DiagnosticBuilder::new()
		.set_name("InteractCallbacks/Progress")
		.add_arg("effect_ref", effect_ref as usize)
		.add_arg("current", current)
		.add_arg("total", total)
		.set_result(0)
		.emit();

	PF_Err_NONE as PF_Err
}

pub(crate) unsafe extern "C" fn register_ui_sys(
	effect_ref: PF_ProgPtr,
	cust_info: *mut PF_CustomUIInfo,
) -> PF_Err {
	#[cfg(feature = "diagnostics")]
	DiagnosticBuilder::new()
		.set_name("InteractCallbacks/RegisterUI")
		.add_arg("effect_ref", effect_ref as usize)
		.add_arg("cust_info", format!("{:?}", cust_info))
		.set_result(0)
		.emit();

	PF_Err_NONE as PF_Err
}

pub(crate) unsafe extern "C" fn checkout_layer_audio_sys(
	effect_ref: PF_ProgPtr,
	index: PF_ParamIndex,
	start_time: A_long,
	duration: A_long,
	time_scale: A_u_long,
	rate: PF_UFixed,
	bytes_per_sample: A_long,
	num_channels: A_long,
	fmt_signed: A_long,
	audio: *mut PF_LayerAudio,
) -> PF_Err {
	#[cfg(feature = "diagnostics")]
	DiagnosticBuilder::new()
		.set_name("InteractCallbacks/CheckoutLayerAudio")
		.add_arg("effect_ref", effect_ref as usize)
		.add_arg("index", index)
		.add_arg("start_time", start_time)
		.add_arg("duration", duration)
		.add_arg("time_scale", time_scale)
		.add_arg("rate", rate)
		.add_arg("bytes_per_sample", bytes_per_sample)
		.add_arg("num_channels", num_channels)
		.add_arg("fmt_signed", fmt_signed)
		.add_arg("audio", format!("{:?}", audio))
		.set_result(0)
		.emit();

	PF_Err_NONE as PF_Err
}

pub(crate) unsafe extern "C" fn checkin_layer_audio_sys(
	effect_ref: PF_ProgPtr,
	audio: PF_LayerAudio,
) -> PF_Err {
	#[cfg(feature = "diagnostics")]
	DiagnosticBuilder::new()
		.set_name("InteractCallbacks/CheckinLayerAudio")
		.add_arg("effect_ref", effect_ref as usize)
		.add_arg("audio", format!("{:?}", audio))
		.set_result(0)
		.emit();

	PF_Err_NONE as PF_Err
}

pub(crate) unsafe extern "C" fn get_audio_data_sys(
	effect_ref: PF_ProgPtr,
	audio: PF_LayerAudio,
	data0: *mut PF_SndSamplePtr,
	num_samples0: *mut A_long,
	rate0: *mut PF_UFixed,
	bytes_per_sample0: *mut A_long,
	num_channels0: *mut A_long,
	fmt_signed0: *mut A_long,
) -> PF_Err {
	#[cfg(feature = "diagnostics")]
	DiagnosticBuilder::new()
		.set_name("InteractCallbacks/GetLayerAudioRate")
		.add_arg("effect_ref", effect_ref as usize)
		.add_arg("audio", format!("{:?}", audio))
		.add_arg("data0", format!("{:?}", data0))
		.add_arg("num_samples0", format!("{:?}", num_samples0))
		.add_arg("rate0", format!("{:?}", rate0))
		.add_arg("bytes_per_sample0", format!("{:?}", bytes_per_sample0))
		.add_arg("num_channels0", format!("{:?}", num_channels0))
		.add_arg("fmt_signed0", format!("{:?}", fmt_signed0))
		.set_result(0)
		.emit();

	PF_Err_NONE as PF_Err
}
