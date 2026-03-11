//! PreRender Callback Implementations
//!
//! This module provides minimal, API-compatible pre-render callbacks.

use crate::core::diagnostics::*;
use crate::core::smart_render::PreRenderDataRef;
use crate::host::smart_render::data;
use after_effects_sys::*;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

/// Pre-render context for tracking state during pre-render operations
pub struct PreRenderContext {
	/// Effect key for this pre-render operation
	effect_key: usize,
	/// Input layer count
	input_layer_count: i32,
	/// Frame time
	frame_time: i64,
	/// Width
	width: i32,
	/// Height
	height: i32,
}

impl PreRenderContext {
	/// Creates a new pre-render context
	pub fn new(effect_ref: PF_ProgPtr) -> Self {
		Self {
			effect_key: effect_ref as usize,
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

	/// Finalizes the pre-render data and stores it
	pub fn finalize(self) {
		let pre_render_data = PreRenderDataRef::new(self.width, self.height, self.frame_time);
		let effect_ref = self.effect_key as PF_ProgPtr;
		data::store_pre_render_data(effect_ref, pre_render_data);
	}

	/// Returns the effect reference
	pub fn effect_ref(&self) -> PF_ProgPtr {
		self.effect_key as PF_ProgPtr
	}

	/// Returns the dimensions
	pub fn dimensions(&self) -> (i32, i32) {
		(self.width, self.height)
	}
}

/// Thread-safe storage for pre-render contexts
static PRE_RENDER_CONTEXTS: LazyLock<Mutex<HashMap<usize, PreRenderContext>>> =
	LazyLock::new(|| Mutex::new(HashMap::new()));

/// Gets or creates a pre-render context for the given effect_ref
fn get_or_create_context(effect_ref: PF_ProgPtr) -> PreRenderContext {
	let contexts = PRE_RENDER_CONTEXTS.lock().unwrap();
	let key = effect_ref as usize;
	if let Some(existing) = contexts.get(&key) {
		return PreRenderContext {
			effect_key: existing.effect_key,
			input_layer_count: existing.input_layer_count,
			frame_time: existing.frame_time,
			width: existing.width,
			height: existing.height,
		};
	}
	PreRenderContext::new(effect_ref)
}

/// Stores a pre-render context
fn store_context(context: PreRenderContext) {
	let mut contexts = PRE_RENDER_CONTEXTS.lock().unwrap();
	let key = context.effect_ref() as usize;
	contexts.insert(key, context);
}

/// Removes and finalizes a pre-render context
fn remove_context(effect_ref: PF_ProgPtr) -> Option<PreRenderContext> {
	let mut contexts = PRE_RENDER_CONTEXTS.lock().unwrap();
	let key = effect_ref as usize;
	contexts.remove(&key)
}

// ============================================================================
// PreRender Callback Implementations
// ============================================================================

/// Checks out a layer during pre-render.
///
/// # Safety
/// The host must provide valid callback pointers according to the AE SDK ABI.
pub unsafe extern "C" fn checkout_layer_impl(
	progress: *mut PF_ProgressInfo,
	layer_index: i32,
	_which: i32,
	_request: *const PF_RenderRequest,
	_purpose: i32,
	_quality: i32,
	_field: u32,
	checkout: *mut PF_CheckoutResult,
) -> PF_Err {
	#[cfg(feature = "diagnostics")]
	DiagnosticBuilder::new()
		.set_name("PF_PreRenderCallbacks/checkout_layer")
		.add_arg("progress", format!("{:?}", progress))
		.add_arg("layer_index", layer_index)
		.add_arg("which", _which)
		.add_arg("request", format!("{:?}", _request))
		.add_arg("purpose", _purpose)
		.add_arg("quality", _quality)
		.add_arg("field", _field)
		.add_arg("checkout", format!("{:?}", checkout))
		.emit();

	if checkout.is_null() {
		log::warn!("checkout_layer: checkout pointer is null");
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	let mut context = get_or_create_context(progress as PF_ProgPtr);
	context.set_input_layer_count(layer_index);
	context.set_frame_time(0);
	context.set_dimensions(1920, 1080);
	store_context(context);

	unsafe {
		(*checkout).result_rect = PF_Rect {
			left: 0,
			top: 0,
			right: 1920,
			bottom: 1080,
		};
		(*checkout).max_result_rect = PF_Rect {
			left: 0,
			top: 0,
			right: 1920,
			bottom: 1080,
		};
		(*checkout).solid = 0;
		(*checkout).ref_width = 1920;
		(*checkout).ref_height = 1080;
	}

	log::debug!("checkout_layer: completed successfully");
	PF_Err_NONE as PF_Err
}

/// Creates a PF_PreRenderCallbacks instance with all callbacks populated
pub fn create_pre_render_callbacks() -> PF_PreRenderCallbacks {
	PF_PreRenderCallbacks {
		checkout_layer: Some(checkout_layer_impl),
		GuidMixInPtr: None,
	}
}

/// Finalizes pre-render for an effect reference
pub fn finalize_pre_render(effect_ref: PF_ProgPtr) {
	if let Some(context) = remove_context(effect_ref) {
		context.finalize();
	}
}
