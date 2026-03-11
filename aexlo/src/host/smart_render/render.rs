//! SmartRender Callback Implementations
//!
//! This module implements the callbacks required during the smart render phase of SmartRender.
//! Smart render allows plugins to access pre-computed data from the pre-render phase.

use crate::core::diagnostics::*;
use after_effects_sys::*;
use std::ptr;

/// SmartRender context for tracking state during smart render operations
pub struct SmartRenderContext {
	/// Effect reference for this smart render operation
	effect_ref: PF_ProgPtr,
	/// Input layer count
	input_layer_count: i32,
	/// Frame time
	frame_time: i64,
	/// Width
	width: i32,
	/// Height
	height: i32,
}

impl SmartRenderContext {
	/// Creates a new smart render context
	pub fn new(effect_ref: PF_ProgPtr) -> Self {
		Self {
			effect_ref,
			input_layer_count: 0,
			frame_time: 0,
			width: 0,
			height: 0,
		}
	}

	/// Sets the input layer count
	pub fn set_input_layer_count(&mut self, count: i32) {
		self.input_layer_count = count;
	}

	/// Sets the frame time
	pub fn set_frame_time(&mut self, time: i64) {
		self.frame_time = time;
	}

	/// Sets the dimensions
	pub fn set_dimensions(&mut self, width: i32, height: i32) {
		self.width = width;
		self.height = height;
	}

	/// Returns the effect reference
	pub fn effect_ref(&self) -> PF_ProgPtr {
		self.effect_ref
	}

	/// Returns the dimensions
	pub fn dimensions(&self) -> (i32, i32) {
		(self.width, self.height)
	}
}

// ============================================================================
// SmartRender Callback Implementations
// ============================================================================

unsafe extern "C" fn checkout_layer_pixels_render_impl(
	effect_ref: PF_ProgPtr,
	checkout_id: A_long,
	pixels: *mut *mut PF_EffectWorld,
) -> PF_Err {
	#[cfg(feature = "diagnostics")]
	DiagnosticBuilder::new()
		.set_name("PF_SmartRenderCallbacks/checkout_layer_pixels")
		.add_arg("effect_ref", format!("{:#x}", effect_ref as usize))
		.add_arg("checkout_id", checkout_id)
		.add_arg("pixels", format!("{:?}", pixels))
		.emit();

	if pixels.is_null() {
		log::warn!("checkout_layer_pixels: pixels pointer is null");
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	// For now, return a stub implementation
	// In production, this would allocate and return pixel data for the specified checkout_id
	log::debug!(
		"checkout_layer_pixels: checkout_id={}, effect_ref={:#x}",
		checkout_id,
		effect_ref as usize
	);

	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn checkin_layer_pixels_render_impl(
	effect_ref: PF_ProgPtr,
	checkout_id: A_long,
) -> PF_Err {
	#[cfg(feature = "diagnostics")]
	DiagnosticBuilder::new()
		.set_name("PF_SmartRenderCallbacks/checkin_layer_pixels")
		.add_arg("effect_ref", format!("{:#x}", effect_ref as usize))
		.add_arg("checkout_id", checkout_id)
		.emit();

	// Clean up pixel data for the specified checkout_id
	log::debug!(
		"checkin_layer_pixels: checkout_id={}, effect_ref={:#x}",
		checkout_id,
		effect_ref as usize
	);

	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn checkout_output_render_impl(
	effect_ref: PF_ProgPtr,
	output: *mut *mut PF_EffectWorld,
) -> PF_Err {
	#[cfg(feature = "diagnostics")]
	DiagnosticBuilder::new()
		.set_name("PF_SmartRenderCallbacks/checkout_output")
		.add_arg("effect_ref", format!("{:#x}", effect_ref as usize))
		.add_arg("output", format!("{:?}", output))
		.emit();

	if output.is_null() {
		log::warn!("checkout_output: output pointer is null");
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	// Return the output world for the stored layer
	if let Some(world) = crate::host::smart_render::data::as_effect_world(effect_ref) {
		*output = Box::into_raw(Box::new(world)) as *mut PF_EffectWorld;
		log::debug!(
			"checkout_output: returned output for effect_ref={:#x}",
			effect_ref as usize
		);
	} else {
		log::warn!("checkout_output: no output layer available");
		*output = ptr::null_mut();
	}

	PF_Err_NONE as PF_Err
}

/// Creates a PF_SmartRenderCallbacks instance with all callbacks populated
pub fn create_smart_render_callbacks() -> PF_SmartRenderCallbacks {
	PF_SmartRenderCallbacks {
		checkout_layer_pixels: Some(checkout_layer_pixels_render_impl),
		checkin_layer_pixels: Some(checkin_layer_pixels_render_impl),
		checkout_output: Some(checkout_output_render_impl),
	}
}
