use std::ptr::{null, null_mut};

use crate::sys::*;

use crate::PluginInstance;
use crate::core::constants::{DEFAULT_HEIGHT as HEIGHT, DEFAULT_WIDTH as WIDTH};
use crate::core::diagnostics::diag;

//==== Stub implementations ================================
unsafe extern "C" fn checkout_layer_stub(
	effect_ref: PF_ProgPtr,
	index: PF_ParamIndex,
	checkout_idL: A_long,
	req: *const crate::sys::PF_RenderRequest,
	what_time: A_long,
	_time_step: A_long,
	time_scale: A_u_long,
	checkout_result: *mut crate::sys::PF_CheckoutResult,
) -> PF_Err {
	crate::core::ffi::guard("checkout_layer_stub", || {
		//== Validation ==//
		if effect_ref.is_null() {
			log::error!("checkout_layer: effect_ref is null");
			return PF_Err_BAD_CALLBACK_PARAM;
		}

		if req.is_null() {
			log::error!("checkout_layer: request pointer is null");
			return PF_Err_BAD_CALLBACK_PARAM;
		}

		if checkout_result.is_null() {
			log::error!("checkout_layer: checkout_result pointer is null");
			return PF_Err_BAD_CALLBACK_PARAM;
		}

		//== Implementation ==//
		// Report the actual size of the checked-out layer (the input layer for
		// index 0, a linked layer parameter otherwise), not the compile-time
		// default. An unlinked layer parameter checks out as an empty layer.
		// Remember which layer this checkout id refers to, for
		// `checkout_layer_pixels`.
		let (layer_w, layer_h) = match PluginInstance::get_instance_ptr(effect_ref) {
			Some(mut instance) => {
				// SAFETY: `effect_ref` is the `PluginInstance` aexlo handed the plugin in `in_data`, and the plugin calls back during one of that instance's commands (see `PluginInstance::get_instance_ptr`).
				let instance = unsafe { instance.as_mut() };
				let Ok(index) = usize::try_from(index) else {
					return PF_Err_INVALID_INDEX;
				};
				if instance
					.param_by_index(index)
					.is_none_or(|param| param.param_type != PF_Param_LAYER as PF_ParamType)
				{
					return PF_Err_INVALID_INDEX;
				}
				instance.record_layer_checkout(checkout_idL, index);
				if let Some(at) = crate::TimePoint::new(what_time, time_scale) {
					instance.record_checkout_time(at);
				}
				// A layer's pixels are one picture, whatever the time asked for.
				if instance.project().is_some()
					&& let Some(asked) = crate::TimePoint::new(what_time, time_scale)
					&& asked != crate::TimePoint::from(instance.time())
				{
					crate::observe::unsupported(
						crate::observe::UnsupportedKind::Declined,
						"PF_PreRenderCallbacks/checkout_layer",
						"layer pixels at another time; the pixels of the current time were given",
					);
				}
				instance.layer_param_size(index).unwrap_or((0, 0))
			}
			None => (WIDTH as i32, HEIGHT as i32),
		};

		let requested = unsafe { &(*req).rect };
		let left = requested.left.clamp(0, layer_w);
		let top = requested.top.clamp(0, layer_h);
		let right = requested.right.clamp(left, layer_w);
		let bottom = requested.bottom.clamp(top, layer_h);
		let result = crate::sys::PF_CheckoutResult {
			result_rect: crate::sys::PF_Rect {
				left,
				top,
				right,
				bottom,
			},
			max_result_rect: crate::sys::PF_Rect {
				left: 0,
				top: 0,
				right: layer_w,
				bottom: layer_h,
			},
			par: crate::sys::PF_RationalScale { num: 1, den: 1 },
			// A caller-provided input may contain arbitrary pixels; conservatively
			// report non-solid unless the host actually verifies its contents.
			solid: 0,
			reservedB: [0; 3],
			ref_width: layer_w,
			ref_height: layer_h,
			reserved: [0; 6],
		};

		crate::strict::checkout(crate::CheckoutKind::DeclareLayer, checkout_idL, 0);

		// The plugin passes an uninitialized `PF_CheckoutResult` and reads the layer
		// bounds back out of it; leaving it unwritten hands the plugin stack garbage.
		// SAFETY: `checkout_result` is non-null (checked above) and points at the plugin's `PF_CheckoutResult`.
		unsafe { *checkout_result = result };

		diag!("PF_PreRenderCallbacks/checkout_layer",
			"effect_ref" => format!("{:#x}", effect_ref as usize),
			"index" => index,
			"checkout_idL" => checkout_idL,
			"what_time" => what_time,
			"time_step" => _time_step,
			"time_scale" => time_scale;
			result: format!("{:?}", result),
		);

		PF_Err_NONE
	})
}

/// `PF_PreRenderCallbacks::GuidMixInPtr`: the plugin mixes state that is not a
/// parameter (sequence data, a palette table) into the GUID After Effects
/// keys its frame cache with.
///
/// aexlo keeps no frame cache and renders every frame it is asked for, so
/// there is no GUID for the bytes to change: accepting them is the whole
/// behavior. The call is still checked and recorded, with a digest of the
/// bytes so two pre-renders that mixed in different state can be told apart.
unsafe extern "C" fn guid_mix_in(effect_ref: PF_ProgPtr, buf_size: A_u_long, buf: *const std::ffi::c_void) -> PF_Err {
	crate::core::ffi::guard("guid_mix_in", || {
		//== Validation ==//
		if effect_ref.is_null() {
			log::error!("GuidMixInPtr: effect_ref is null");
			return PF_Err_BAD_CALLBACK_PARAM;
		}

		if buf.is_null() && buf_size != 0 {
			log::error!("GuidMixInPtr: buf is null but buf_sizeLu is {buf_size}");
			return PF_Err_BAD_CALLBACK_PARAM;
		}

		//== Implementation ==//
		let bytes: &[u8] = if buf_size == 0 {
			&[]
		} else {
			// SAFETY: `buf` is non-null (checked above) and the SDK contract is
			// that it points at `buf_sizeLu` readable bytes for this call.
			unsafe { std::slice::from_raw_parts(buf.cast::<u8>(), buf_size as usize) }
		};

		diag!("PF_PreRenderCallbacks/GuidMixInPtr",
			"effect_ref" => format!("{:#x}", effect_ref as usize),
			"buf_sizeLu" => buf_size,
			"buf" => format!("{:#x}", buf as usize);
			result: format!("fnv1a64 {:#018x}", fnv1a64(bytes)),
		);

		PF_Err_NONE
	})
}

/// 64-bit FNV-1a, to name the bytes a plugin mixed into its GUID in a trace.
fn fnv1a64(bytes: &[u8]) -> u64 {
	bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &b| {
		(hash ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
	})
}

unsafe extern "C" fn checkout_layer_pixels_stub(
	effect_ref: PF_ProgPtr,
	_checkout_idL: A_long,
	pixels: *mut *mut PF_EffectWorld,
) -> PF_Err {
	crate::core::ffi::guard("checkout_layer_pixels_stub", || {
		//== Validation ==//
		if effect_ref.is_null() {
			log::error!("checkout_layer_pixels: effect_ref is null");
			return PF_Err_BAD_CALLBACK_PARAM;
		}

		if pixels.is_null() {
			log::warn!("checkout_layer_pixels: pixels pointer is null");
			return PF_Err_BAD_CALLBACK_PARAM;
		}

		//== Implementation ==//
		// Never panic here: unwinding across the plugin's C frames is UB, so an
		// unknown effect_ref is reported as a callback error instead.
		let Some(mut instance) = PluginInstance::get_instance_ptr(effect_ref) else {
			log::error!(
				"checkout_layer_pixels: no instance found for effect_ref {:#x}",
				effect_ref as usize
			);
			return PF_Err_BAD_CALLBACK_PARAM;
		};

		// The caller passes an *uninitialized* `PF_EffectWorld*` slot (see the SDK's
		// `PF_CheckoutLayerPixels` contract) and expects us to write a pointer to the
		// checked-out world into it -- not to dereference the slot. Hand back the
		// instance's persistent world for the layer this id checked out, mirroring
		// `checkout_output`; an unlinked layer parameter yields null.
		crate::strict::checkout(crate::CheckoutKind::CheckoutLayerPixels, _checkout_idL, 0);
		// SAFETY: `effect_ref` is the `PluginInstance` aexlo handed the plugin in `in_data`, and the plugin calls back during one of that instance's commands (see `PluginInstance::get_instance_ptr`). `pixels` is non-null (checked above).
		let instance = unsafe { instance.as_mut() };
		if !instance.has_layer_checkout(_checkout_idL) {
			unsafe { *pixels = null_mut() };
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		let world = instance.checked_out_world(_checkout_idL);
		// SAFETY: `pixels` is non-null (checked above) and is the plugin's out-slot for the world pointer.
		unsafe { *pixels = world.unwrap_or(null_mut()) };

		diag!("PF_SmartRenderCallbacks/checkout_layer_pixels",
			"effect_ref" => format!("{:#x}", effect_ref as usize),
			"checkout_idL" => _checkout_idL,
			"pixels (out)" => format!("{:#x}", pixels as usize);
			result: format!("{:?}", world),
		);

		PF_Err_NONE
	})
}

unsafe extern "C" fn checkin_layer_pixels_stub(_effect_ref: PF_ProgPtr, _checkout_idL: A_long) -> PF_Err {
	crate::core::ffi::guard("checkin_layer_pixels_stub", || {
		crate::strict::checkout(crate::CheckoutKind::CheckinLayerPixels, _checkout_idL, 0);
		diag!("PF_SmartRenderCallbacks/checkin_layer_pixels",
			"effect_ref" => format!("{:#x}", _effect_ref as usize),
			"checkout_idL" => _checkout_idL,
		);

		PF_Err_NONE
	})
}

unsafe extern "C" fn checkout_output_sys(effect_ref: PF_ProgPtr, output: *mut *mut PF_EffectWorld) -> PF_Err {
	crate::core::ffi::guard("checkout_output_sys", || {
		//== Validation ==//
		if effect_ref.is_null() {
			log::error!("checkout_output: effect_ref is null");
			return PF_Err_BAD_CALLBACK_PARAM;
		}

		if output.is_null() {
			log::error!("checkout_output: output pointer is null");
			return PF_Err_BAD_CALLBACK_PARAM;
		}

		//== Implementation ==//
		let Some(mut instance) = PluginInstance::get_instance_ptr(effect_ref) else {
			log::error!(
				"checkout_output: No instance found for effect_ref {:#x}",
				effect_ref as usize
			);
			return PF_Err_BAD_CALLBACK_PARAM;
		};

		// Hand back a pointer into the instance's persistent output world, not a
		// pointer to a temporary `as_sys()` value (which would dangle immediately).
		// SAFETY: `effect_ref` is the `PluginInstance` aexlo handed the plugin in `in_data`, and the plugin calls back during one of that instance's commands (see `PluginInstance::get_instance_ptr`). `output` is non-null (checked above).
		unsafe { *output = instance.as_mut().output_world_ptr() };

		diag!("PF_SmartRenderCallbacks/checkout_output",
			"effect_ref" => format!("{:#x}", effect_ref as usize),
			// SAFETY: `output` is non-null (checked above) and was just written.
			"output (out)" => format!("{:#x}", unsafe { *output } as usize);
			result: format!("`output` is set to internal output layer {:#x}", output as usize),
		);

		PF_Err_NONE
	})
}

/// A fresh `PF_PreRenderOutput`, as handed to every `PF_Cmd_SMART_PRE_RENDER`.
fn empty_pre_render_output() -> PF_PreRenderOutput {
	PF_PreRenderOutput {
		result_rect: PF_Rect {
			left: 0,
			top: 0,
			right: 0,
			bottom: 0,
		},
		max_result_rect: PF_Rect {
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
	}
}

/// Data structure to hold smart render state and provide callbacks.
pub(crate) struct SmartRenderData {
	pre_input: Box<crate::sys::PF_PreRenderInput>,
	pre_output: Box<crate::sys::PF_PreRenderOutput>,
	pre_callbacks: Box<crate::sys::PF_PreRenderCallbacks>,

	input: Box<crate::sys::PF_SmartRenderInput>,
	callbacks: Box<crate::sys::PF_SmartRenderCallbacks>,

	/// Bits per channel of CPU renders: the depth of the instance's worlds.
	cpu_bitdepth: i16,
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::sync::atomic::{AtomicBool, Ordering};

	static CHECKOUT_OK: AtomicBool = AtomicBool::new(false);
	static PIXEL_CHECKOUT_OK: AtomicBool = AtomicBool::new(false);
	unsafe extern "C" fn checkout_test_entry(
		cmd: PF_Cmd,
		input: *mut PF_InData,
		_: *mut PF_OutData,
		_: PF_ParamList,
		_: *mut PF_LayerDef,
		extra: *mut std::ffi::c_void,
	) -> PF_Err {
		if cmd == PF_Cmd_SMART_PRE_RENDER {
			let extra = unsafe { &mut *extra.cast::<PF_PreRenderExtra>() };
			let mut request = unsafe { (*extra.input).output_request };
			request.rect = PF_LRect {
				left: -4,
				top: 5,
				right: 20,
				bottom: 80,
			};
			let mut result: PF_CheckoutResult = unsafe { std::mem::zeroed() };
			let err = unsafe {
				((*extra.cb).checkout_layer.unwrap())((*input).effect_ref, 0, 17, &request, 0, 1, 30, &mut result)
			};
			CHECKOUT_OK.store(
				err == PF_Err_NONE
					&& result.solid == 0
					&& (
						result.result_rect.left,
						result.result_rect.top,
						result.result_rect.right,
						result.result_rect.bottom,
					) == (0, 5, 20, 48)
					&& (result.max_result_rect.right, result.max_result_rect.bottom) == (64, 48),
				Ordering::SeqCst,
			);
		} else if cmd == PF_Cmd_SMART_RENDER {
			let extra = unsafe { &mut *extra.cast::<PF_SmartRenderExtra>() };
			let checkout = unsafe { (*extra.cb).checkout_layer_pixels.unwrap() };
			let mut pixels = null_mut();
			let valid = unsafe { checkout((*input).effect_ref, 17, &mut pixels) };
			let was_nonnull = !pixels.is_null();
			let invalid = unsafe { checkout((*input).effect_ref, 999, &mut pixels) };
			PIXEL_CHECKOUT_OK.store(
				valid == PF_Err_NONE && was_nonnull && invalid == PF_Err_BAD_CALLBACK_PARAM && pixels.is_null(),
				Ordering::SeqCst,
			);
		}
		PF_Err_NONE
	}

	#[test]
	fn pre_render_checkout_clips_request_and_does_not_claim_solid_input() {
		CHECKOUT_OK.store(false, Ordering::SeqCst);
		PIXEL_CHECKOUT_OK.store(false, Ordering::SeqCst);
		let mut instance = unsafe { crate::Host::get().entry_loader(checkout_test_entry) }
			.size(64, 48)
			.load()
			.unwrap();
		instance.render_with(crate::RenderPath::Smart).unwrap();
		assert!(CHECKOUT_OK.load(Ordering::SeqCst));
		assert!(PIXEL_CHECKOUT_OK.load(Ordering::SeqCst));
	}

	#[test]
	fn pre_render_guid_callback_accepts_dependencies() {
		let mut state = SmartRenderData::new();
		let extra = state.pre_render_extra();
		let callback = unsafe { (*extra.cb).GuidMixInPtr.expect("GUID callback") };
		let bytes = [1u8, 2, 3];
		let effect_ref = 1usize as PF_ProgPtr;
		unsafe {
			assert_eq!(
				callback(effect_ref, bytes.len() as A_u_long, bytes.as_ptr().cast()),
				PF_Err_NONE
			);
			assert_eq!(callback(effect_ref, 0, null()), PF_Err_NONE);
			assert_eq!(callback(effect_ref, 1, null()), PF_Err_BAD_CALLBACK_PARAM);
		}
	}

	#[test]
	fn pre_render_releases_owned_handle_without_delete_callback() {
		let mut state = SmartRenderData::new();
		let handle = unsafe { crate::suites::handle::host_new_handle_impl(108) };
		assert!(!handle.is_null());
		state.pre_output.pre_render_data = handle.cast();
		assert!(crate::suites::handle::is_owned_handle(handle as usize));
		state.dispose_pre_render_data();
		assert!(!crate::suites::handle::is_owned_handle(handle as usize));
		state.dispose_pre_render_data();
		state.pre_output.pre_render_data = std::ptr::dangling_mut::<u8>().cast();
		state.dispose_pre_render_data();
	}

	#[test]
	fn pre_render_delete_runs_once_for_cpu_and_gpu_frames() {
		use std::sync::atomic::AtomicUsize;
		static DELETES: AtomicUsize = AtomicUsize::new(0);
		unsafe extern "C" fn delete(_: *mut std::ffi::c_void) {
			DELETES.fetch_add(1, Ordering::SeqCst);
		}
		let mut state = SmartRenderData::new();
		let data = std::ptr::dangling_mut::<u8>().cast();
		state.begin_pre_render();
		state.pre_output.pre_render_data = data;
		state.pre_output.delete_pre_render_data_func = Some(delete);
		state.configure_gpu(null(), 0, crate::sys::PF_GPU_Framework_CUDA);
		state.begin_pre_render();
		assert_eq!(DELETES.load(Ordering::SeqCst), 1);
		state.pre_output.pre_render_data = data;
		state.pre_output.delete_pre_render_data_func = Some(delete);
		state.configure_cpu();
		state.dispose_pre_render_data();
		assert_eq!(DELETES.load(Ordering::SeqCst), 2);
		state.dispose_pre_render_data();
		assert_eq!(DELETES.load(Ordering::SeqCst), 2);
	}

	#[test]
	fn gpu_pre_render_releases_owned_handle_without_delete_callback() {
		let mut state = SmartRenderData::new();
		state.configure_gpu(null(), 0, crate::sys::PF_GPU_Framework_CUDA);
		state.begin_pre_render();
		let handle = unsafe { crate::suites::handle::host_new_handle_impl(108) };
		assert!(!handle.is_null());
		state.pre_output.pre_render_data = handle.cast();
		state.dispose_pre_render_data();
		assert!(!crate::suites::handle::is_owned_handle(handle as usize));
	}
}

impl SmartRenderData {
	/// Creates a new SmartRenderData with default values.
	pub fn new() -> Self {
		Self {
			pre_input: Box::new(crate::sys::PF_PreRenderInput {
				bitdepth: 8,
				device_index: 4294967295,
				output_request: crate::sys::PF_RenderRequest {
					rect: crate::sys::PF_LRect {
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
			pre_output: Box::new(empty_pre_render_output()),
			pre_callbacks: Box::new(crate::sys::PF_PreRenderCallbacks {
				checkout_layer: Some(checkout_layer_stub),
				GuidMixInPtr: Some(guid_mix_in),
			}),

			input: Box::new(crate::sys::PF_SmartRenderInput {
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
			callbacks: Box::new(crate::sys::PF_SmartRenderCallbacks {
				checkout_layer_pixels: Some(checkout_layer_pixels_stub),
				checkin_layer_pixels: Some(checkin_layer_pixels_stub),
				checkout_output: Some(checkout_output_sys),
			}),
			cpu_bitdepth: 8,
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
	pub fn configure_gpu(
		&mut self,
		gpu_data: *const std::os::raw::c_void,
		device_index: A_u_long,
		framework: PF_GPU_Framework,
	) {
		self.pre_input.what_gpu = framework;
		self.pre_input.device_index = device_index;
		self.pre_input.bitdepth = 32;
		self.pre_input.gpu_data = gpu_data;

		self.input.what_gpu = framework;
		self.input.device_index = device_index;
		self.input.bitdepth = 32;
		self.input.gpu_data = gpu_data;
	}

	/// Prepare for a new pre-render: release the previous frame's pre-render
	/// data and reset the output the plugin fills in.
	pub fn begin_pre_render(&mut self) {
		self.dispose_pre_render_data();
		*self.pre_output = empty_pre_render_output();
	}

	/// Release the plugin's pre-render data through the delete callback it
	/// registered, as After Effects does once the frame has rendered.
	pub fn dispose_pre_render_data(&mut self) {
		let data = std::mem::replace(&mut self.pre_output.pre_render_data, null_mut());
		let delete = self.pre_output.delete_pre_render_data_func.take();
		self.input.pre_render_data = null_mut();
		if let (false, Some(delete)) = (data.is_null(), delete) {
			// SAFETY: `data` is the plugin's own pre-render data, released once
			// through the callback it registered alongside it.
			unsafe { delete(data) };
		} else if !data.is_null() && crate::suites::handle::is_owned_handle(data as usize) {
			// Some effects return a host handle without a delete callback. Its
			// ownership transfers to the host with the pre-render result.
			unsafe { crate::suites::handle::host_dispose_handle_impl(data.cast()) };
		}
	}

	/// Whether the last pre-render declared the frame renderable on the GPU
	/// (`PF_RenderOutputFlag_GPU_RENDER_POSSIBLE`).
	pub fn gpu_render_possible(&self) -> bool {
		self.pre_output.flags as i64 & PF_RenderOutputFlag_GPU_RENDER_POSSIBLE as i64 != 0
	}

	/// Whether the current render is a GPU render (see [`Self::configure_gpu`]).
	pub fn is_gpu(&self) -> bool {
		self.input.what_gpu != PF_GPU_Framework_NONE
	}

	/// Set the depth CPU renders advertise (8, 16 or 32), applying it now unless
	/// a GPU render is configured.
	pub fn set_cpu_bitdepth(&mut self, bits: i16) {
		self.cpu_bitdepth = bits;
		if !self.is_gpu() {
			self.pre_input.bitdepth = bits;
			self.input.bitdepth = bits;
		}
	}

	/// Reset the pre-render and render inputs to CPU (the instance's depth, no GPU framework).
	///
	/// Used when GPU render is unavailable or fails, so a subsequent CPU
	/// smart-render fallback does not leave the plugin believing it should still
	/// produce a GPU frame.
	pub fn configure_cpu(&mut self) {
		self.pre_input.what_gpu = PF_GPU_Framework_NONE;
		self.pre_input.device_index = 0;
		self.pre_input.bitdepth = self.cpu_bitdepth;
		self.pre_input.gpu_data = null();

		self.input.what_gpu = PF_GPU_Framework_NONE;
		self.input.device_index = 0;
		self.input.bitdepth = self.cpu_bitdepth;
		self.input.gpu_data = null();
	}
}

#[cfg(test)]
mod guid_tests {
	use super::*;

	#[test]
	fn guid_mix_in_accepts_bytes() {
		let data = [1u8, 2, 3, 4];
		let effect_ref = 1usize as PF_ProgPtr;
		// SAFETY: `data` is 4 readable bytes; the callback only reads them.
		let err = unsafe { guid_mix_in(effect_ref, data.len() as A_u_long, data.as_ptr().cast()) };
		assert_eq!(err, PF_Err_NONE);
		// SAFETY: a zero-length mix-in never reads `buf`.
		assert_eq!(unsafe { guid_mix_in(effect_ref, 0, null()) }, PF_Err_NONE);
	}

	#[test]
	fn guid_mix_in_rejects_bad_pointers() {
		let data = [0u8; 8];
		// SAFETY: both calls are refused before `buf` is read.
		unsafe {
			assert_eq!(
				guid_mix_in(null_mut(), 8, data.as_ptr().cast()),
				PF_Err_BAD_CALLBACK_PARAM
			);
			assert_eq!(guid_mix_in(1usize as PF_ProgPtr, 8, null()), PF_Err_BAD_CALLBACK_PARAM);
		}
	}

	#[test]
	fn pre_render_callbacks_offer_guid_mix_in() {
		let mut data = SmartRenderData::new();
		let extra = data.pre_render_extra();
		// SAFETY: `extra.cb` points into `data`, which is alive.
		assert!(unsafe { (*extra.cb).GuidMixInPtr }.is_some());
	}

	#[test]
	fn fnv1a64_matches_reference() {
		assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
		assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
	}
}
