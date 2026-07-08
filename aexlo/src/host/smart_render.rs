use std::ptr::{null, null_mut};

use after_effects_sys::*;

use crate::core::constants::{DEFAULT_HEIGHT as HEIGHT, DEFAULT_WIDTH as WIDTH};
use crate::{DiagnosticBuilder, PluginInstance};

//==== Stub implementations ================================
unsafe extern "C" fn checkout_layer_stub(
	effect_ref: PF_ProgPtr,
	index: PF_ParamIndex,
	checkout_idL: A_long,
	req: *const after_effects_sys::PF_RenderRequest,
	what_time: A_long,
	time_step: A_long,
	time_scale: A_u_long,
	checkout_result: *mut after_effects_sys::PF_CheckoutResult,
) -> PF_Err {
	//== Validation ==//
	if effect_ref.is_null() {
		log::error!("checkout_layer: effect_ref is null");
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	if req.is_null() {
		log::error!("checkout_layer: request pointer is null");
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	if checkout_result.is_null() {
		log::error!("checkout_layer: checkout_result pointer is null");
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	//== Implementation ==//
	let result = after_effects_sys::PF_CheckoutResult {
		result_rect: after_effects_sys::PF_Rect {
			left: 0,
			top: 0,
			right: WIDTH as i32,
			bottom: HEIGHT as i32,
		},
		max_result_rect: after_effects_sys::PF_Rect {
			left: 0,
			top: 0,
			right: WIDTH as i32,
			bottom: HEIGHT as i32,
		},
		par: after_effects_sys::PF_RationalScale { num: 1, den: 1 },
		solid: 1,
		reservedB: [0; 3],
		ref_width: WIDTH as i32,
		ref_height: HEIGHT as i32,
		reserved: [0; 6],
	};

	//== Diagnostics ==//
	DiagnosticBuilder::new()
		.set_name("PF_PreRenderCallbacks/checkout_layer")
		.add_arg("effect_ref", format!("{:#x}", effect_ref as usize))
		.add_arg("index", index)
		.add_arg("checkout_idL", checkout_idL)
		.add_arg("what_time", what_time)
		.add_arg("time_step", time_step)
		.add_arg("time_scale", time_scale)
		.set_result(format!("{:?}", result))
		.emit();

	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn checkout_layer_pixels_stub(
	effect_ref: PF_ProgPtr,
	checkout_idL: A_long,
	pixels: *mut *mut PF_EffectWorld,
) -> PF_Err {
	//== Diagnostics ==//
	let mut diagnostics = DiagnosticBuilder::new();
	diagnostics
		.set_name("PF_SmartRenderCallbacks/checkout_layer_pixels")
		.add_arg("effect_ref", format!("{:#x}", effect_ref as usize))
		.add_arg("checkout_idL", checkout_idL)
		.add_arg("pixels (out)", format!("{:#x}", pixels as usize));

	//== Validation ==//
	if effect_ref.is_null() {
		log::error!("checkout_layer_pixels: effect_ref is null");
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	if pixels.is_null() {
		log::warn!("checkout_layer_pixels: pixels pointer is null");
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	//== Implementation ==//
	let instance = unsafe {
		PluginInstance::get_instance_ptr(effect_ref)
			.expect("checkout_layer_pixels: No plugin instance found for effect_ref")
			.as_mut()
	};

	// The caller passes an *uninitialized* `PF_EffectWorld*` slot (see the SDK's
	// `PF_CheckoutLayerPixels` contract) and expects us to write a pointer to the
	// checked-out input world into it -- not to dereference the slot. Hand back the
	// instance's persistent input world, mirroring `checkout_output`.
	let input_world = instance.input_world_ptr();
	unsafe { *pixels = input_world };

	diagnostics
		.set_result(unsafe { (*input_world).data } as usize)
		.emit();

	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn checkin_layer_pixels_stub(effect_ref: PF_ProgPtr, checkout_idL: A_long) -> PF_Err {
	DiagnosticBuilder::new()
		.set_name("PF_SmartRenderCallbacks/checkin_layer_pixels")
		.add_arg("effect_ref", format!("{:#x}", effect_ref as usize))
		.add_arg("checkout_idL", checkout_idL)
		.emit();

	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn checkout_output_sys(effect_ref: PF_ProgPtr, output: *mut *mut PF_EffectWorld) -> PF_Err {
	//== Validation ==//
	if effect_ref.is_null() {
		log::error!("checkout_output: effect_ref is null");
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	if output.is_null() {
		log::error!("checkout_output: output pointer is null");
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	//== Implementation ==//
	if let Some(mut instance) = PluginInstance::get_instance_ptr(effect_ref) {
		// Hand back a pointer into the instance's persistent output world, not a
		// pointer to a temporary `as_sys()` value (which would dangle immediately).
		unsafe { *output = instance.as_mut().output_world_ptr() };
	} else {
		log::error!(
			"checkout_output: No instance found for effect_ref {:#x}",
			effect_ref as usize
		);
	}

	//== Diagnostics ==//
	DiagnosticBuilder::new()
		.set_name("PF_SmartRenderCallbacks/checkout_output")
		.add_arg("effect_ref", format!("{:#x}", effect_ref as usize))
		.add_arg("output (out)", format!("{:#x}", unsafe { *output } as usize))
		.set_result(format!(
			"`output` is set to internal output layer {:#x}",
			output as usize
		))
		.emit();

	PF_Err_NONE as PF_Err
}

/// Data structure to hold smart render state and provide callbacks.
pub(crate) struct SmartRenderData {
	pre_input: Box<after_effects_sys::PF_PreRenderInput>,
	pre_output: Box<after_effects_sys::PF_PreRenderOutput>,
	pre_callbacks: Box<after_effects_sys::PF_PreRenderCallbacks>,

	input: Box<after_effects_sys::PF_SmartRenderInput>,
	callbacks: Box<after_effects_sys::PF_SmartRenderCallbacks>,
}

impl SmartRenderData {
	/// Creates a new SmartRenderData with default values.
	pub fn new() -> Self {
		Self {
			pre_input: Box::new(after_effects_sys::PF_PreRenderInput {
				bitdepth: 8,
				device_index: 4294967295,
				output_request: after_effects_sys::PF_RenderRequest {
					rect: after_effects_sys::PF_LRect {
						left: 0,
						top: 0,
						right: WIDTH as i32,
						bottom: HEIGHT as i32,
					},
					field: 0,
					channel_mask: 15,
					preserve_rgb_of_zero_alpha: 0,
					unused: [0; 3],
					reserved: [0; 4],
				},
				what_gpu: 0,
				gpu_data: null(),
			}),
			pre_output: Box::new(after_effects_sys::PF_PreRenderOutput {
				result_rect: after_effects_sys::PF_Rect {
					left: 0,
					top: 0,
					right: 0,
					bottom: 0,
				},
				max_result_rect: after_effects_sys::PF_Rect {
					left: -1,
					top: -1,
					right: -1,
					bottom: -1,
				},
				solid: 0,
				reserved: 0,
				flags: 0,
				pre_render_data: null_mut(),
				delete_pre_render_data_func: None,
			}),
			pre_callbacks: Box::new(after_effects_sys::PF_PreRenderCallbacks {
				checkout_layer: Some(checkout_layer_stub),
				GuidMixInPtr: None,
			}),

			input: Box::new(after_effects_sys::PF_SmartRenderInput {
				output_request: PF_RenderRequest {
					rect: PF_LRect {
						left: 0,
						top: 0,
						right: WIDTH as i32,
						bottom: HEIGHT as i32,
					},
					field: 0,
					channel_mask: 15,
					preserve_rgb_of_zero_alpha: 0,
					unused: [0; 3],
					reserved: [0; 4],
				},
				bitdepth: 8,
				pre_render_data: null_mut(),
				gpu_data: null(),
				what_gpu: 0,
				device_index: 4294967295,
			}),
			callbacks: Box::new(after_effects_sys::PF_SmartRenderCallbacks {
				checkout_layer_pixels: Some(checkout_layer_pixels_stub),
				checkin_layer_pixels: Some(checkin_layer_pixels_stub),
				checkout_output: Some(checkout_output_sys),
			}),
		}
	}

	/// Returns a pointer to the PF_PreRenderExtra struct for pre-render callbacks.
	pub fn pre_render_extra(&mut self) -> PF_PreRenderExtra {
		PF_PreRenderExtra {
			input: self.pre_input.as_mut() as *mut _,
			output: self.pre_output.as_mut() as *mut _,
			cb: self.pre_callbacks.as_mut() as *mut _,
		}
	}

	/// Returns a pointer to the PF_SmartRenderExtra struct for smart render callbacks.
	pub fn smart_render_extra(&mut self) -> PF_SmartRenderExtra {
		PF_SmartRenderExtra {
			input: self.input.as_mut() as *mut _,
			cb: self.callbacks.as_mut() as *mut _,
		}
	}

	/// Syncs the smart render input with the pre-render output. Call this at the end of your pre-render callback implementation to pass data to the smart render phase.
	pub fn sync(&mut self) {
		self.input.pre_render_data = self.pre_output.pre_render_data;
	}
}
