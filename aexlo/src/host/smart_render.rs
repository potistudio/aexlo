use std::ptr::{null, null_mut};

use after_effects_sys::*;

use crate::core::constants::{DEFAULT_HEIGHT as HEIGHT, DEFAULT_WIDTH as WIDTH};
use crate::core::diagnostics::diag;
use crate::PluginInstance;

//==== Stub implementations ================================
unsafe extern "C" fn checkout_layer_stub(
	effect_ref: PF_ProgPtr,
	_index: PF_ParamIndex,
	_checkout_idL: A_long,
	req: *const after_effects_sys::PF_RenderRequest,
	_what_time: A_long,
	_time_step: A_long,
	_time_scale: A_u_long,
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
	// Report the instance's actual input layer size, not the compile-time
	// default: with a custom input (or `set_render_size`) the default rect
	// would describe a frame that doesn't exist.
	let (layer_w, layer_h) = match PluginInstance::get_instance_ptr(effect_ref) {
		Some(instance) => {
			let (w, h) = unsafe { instance.as_ref() }.input_size();
			(w as i32, h as i32)
		}
		None => (WIDTH as i32, HEIGHT as i32),
	};

	let result = after_effects_sys::PF_CheckoutResult {
		result_rect: after_effects_sys::PF_Rect {
			left: 0,
			top: 0,
			right: layer_w,
			bottom: layer_h,
		},
		max_result_rect: after_effects_sys::PF_Rect {
			left: 0,
			top: 0,
			right: layer_w,
			bottom: layer_h,
		},
		par: after_effects_sys::PF_RationalScale { num: 1, den: 1 },
		solid: 1,
		reservedB: [0; 3],
		ref_width: layer_w,
		ref_height: layer_h,
		reserved: [0; 6],
	};

	// The plugin passes an uninitialized `PF_CheckoutResult` and reads the layer
	// bounds back out of it; leaving it unwritten hands the plugin stack garbage.
	unsafe { *checkout_result = result };

	diag!("PF_PreRenderCallbacks/checkout_layer",
		"effect_ref" => format!("{:#x}", effect_ref as usize),
		"index" => _index,
		"checkout_idL" => _checkout_idL,
		"what_time" => _what_time,
		"time_step" => _time_step,
		"time_scale" => _time_scale;
		result: format!("{:?}", result),
	);

	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn checkout_layer_pixels_stub(
	effect_ref: PF_ProgPtr,
	_checkout_idL: A_long,
	pixels: *mut *mut PF_EffectWorld,
) -> PF_Err {
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
	// Never panic here: unwinding across the plugin's C frames is UB, so an
	// unknown effect_ref is reported as a callback error instead.
	let Some(mut instance) = PluginInstance::get_instance_ptr(effect_ref) else {
		log::error!(
			"checkout_layer_pixels: no instance found for effect_ref {:#x}",
			effect_ref as usize
		);
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	};

	// The caller passes an *uninitialized* `PF_EffectWorld*` slot (see the SDK's
	// `PF_CheckoutLayerPixels` contract) and expects us to write a pointer to the
	// checked-out input world into it -- not to dereference the slot. Hand back the
	// instance's persistent input world, mirroring `checkout_output`.
	let input_world = unsafe { instance.as_mut() }.input_world_ptr();
	unsafe { *pixels = input_world };

	diag!("PF_SmartRenderCallbacks/checkout_layer_pixels",
		"effect_ref" => format!("{:#x}", effect_ref as usize),
		"checkout_idL" => _checkout_idL,
		"pixels (out)" => format!("{:#x}", pixels as usize);
		result: unsafe { (*input_world).data } as usize,
	);

	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn checkin_layer_pixels_stub(_effect_ref: PF_ProgPtr, _checkout_idL: A_long) -> PF_Err {
	diag!("PF_SmartRenderCallbacks/checkin_layer_pixels",
		"effect_ref" => format!("{:#x}", _effect_ref as usize),
		"checkout_idL" => _checkout_idL,
	);

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
	let Some(mut instance) = PluginInstance::get_instance_ptr(effect_ref) else {
		log::error!(
			"checkout_output: No instance found for effect_ref {:#x}",
			effect_ref as usize
		);
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	};

	// Hand back a pointer into the instance's persistent output world, not a
	// pointer to a temporary `as_sys()` value (which would dangle immediately).
	unsafe { *output = instance.as_mut().output_world_ptr() };

	diag!("PF_SmartRenderCallbacks/checkout_output",
		"effect_ref" => format!("{:#x}", effect_ref as usize),
		"output (out)" => format!("{:#x}", unsafe { *output } as usize);
		result: format!("`output` is set to internal output layer {:#x}", output as usize),
	);

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

	/// Point the pre-render and render output-request rects at a new frame size,
	/// keeping them in step with the instance's output world (see
	/// [`PluginInstance::set_render_size`](crate::PluginInstance::set_render_size)).
	pub fn set_output_rect(&mut self, width: i32, height: i32) {
		let rect = PF_LRect {
			left: 0,
			top: 0,
			right: width,
			bottom: height,
		};
		self.pre_input.output_request.rect = rect;
		self.input.output_request.rect = rect;
	}

	/// Configure the pre-render and render inputs for GPU rendering: advertise
	/// the framework (Metal or CUDA), device, plugin-owned GPU data, and
	/// 32-bit-float depth so the plugin dispatches its `PF_Cmd_SMART_RENDER_GPU` path.
	///
	/// `gpu_data` is the handle the plugin returned from `PF_Cmd_GPU_DEVICE_SETUP`;
	/// `framework` is the active backend's `PF_GPU_Framework` constant.
	pub fn configure_gpu(&mut self, gpu_data: *const std::os::raw::c_void, device_index: A_u_long, framework: PF_GPU_Framework) {
		self.pre_input.what_gpu = framework;
		self.pre_input.device_index = device_index;
		self.pre_input.bitdepth = 32;
		self.pre_input.gpu_data = gpu_data;

		self.input.what_gpu = framework;
		self.input.device_index = device_index;
		self.input.bitdepth = 32;
		self.input.gpu_data = gpu_data;
	}

	/// Reset the pre-render and render inputs to CPU (8-bit, no GPU framework).
	///
	/// Used when GPU render is unavailable or fails, so a subsequent CPU
	/// smart-render fallback does not leave the plugin believing it should still
	/// produce a GPU frame.
	pub fn configure_cpu(&mut self) {
		self.pre_input.what_gpu = PF_GPU_Framework_NONE as PF_GPU_Framework;
		self.pre_input.device_index = 0;
		self.pre_input.bitdepth = 8;
		self.pre_input.gpu_data = null();

		self.input.what_gpu = PF_GPU_Framework_NONE as PF_GPU_Framework;
		self.input.device_index = 0;
		self.input.bitdepth = 8;
		self.input.gpu_data = null();
	}
}
