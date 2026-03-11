//! SmartRender Data Structures
//!
//! This module provides safe Rust wrappers for After Effects SmartRender data structures,
//! enabling two-phase rendering optimization where expensive computations can be pre-calculated
//! and reused across frames.

use after_effects_sys::PF_ProgPtr;
use std::sync::Mutex;
use wrapper::{Depth8, Layer};

// ============================================================================
// SmartRender Structure Definitions (from AE SDK)
// ============================================================================

/// PF_PreRenderInput structure (simplified version based on AE SDK)
#[repr(C)]
pub struct PF_PreRenderInput {
	pub output_request: PF_RenderRequest,
	pub bitdepth: i16,
	pub gpu_data: *const std::os::raw::c_void,
	pub what_gpu: i32,
	pub device_index: u32,
}

/// PF_RenderRequest structure (simplified version based on AE SDK)
#[repr(C)]
pub struct PF_RenderRequest {
	pub rect: PF_LRect,
	pub field: i32,
	pub channel_mask: u32,
	pub preserve_rgb_of_zero_alpha: bool,
	pub unused: [u8; 3],
	pub reserved: [i32; 4],
}

/// PF_LRect structure (simplified version based on AE SDK)
#[repr(C)]
#[derive(Clone, Debug, Copy, PartialEq, Eq)]
pub struct PF_LRect {
	pub left: i32,
	pub top: i32,
	pub right: i32,
	pub bottom: i32,
}

/// PF_PreRenderOutput structure (simplified version based on AE SDK)
#[repr(C)]
pub struct PF_PreRenderOutput {
	pub result_rect: PF_LRect,
	pub max_result_rect: PF_LRect,
	pub solid: bool,
	pub reserved: bool,
	pub flags: u16,
	pub pre_render_data: *mut std::os::raw::c_void,
	pub delete_pre_render_data_func: Option<unsafe extern "C" fn(*mut std::os::raw::c_void)>,
}

/// PF_SmartRenderInput structure (simplified version based on AE SDK)
#[repr(C)]
pub struct PF_SmartRenderInput {
	pub output_request: PF_RenderRequest,
	pub bitdepth: i16,
	pub pre_render_data: *mut std::os::raw::c_void,
	pub gpu_data: *const std::os::raw::c_void,
	pub what_gpu: i32,
	pub device_index: u32,
}

// ============================================================================
// Safe Wrappers for SmartRender Structures
// ============================================================================

/// Safe wrapper for PF_PreRenderExtra (simplified version)
///
/// This structure encapsulates the data provided by After Effects during the pre-render phase,
/// allowing safe access to input layers and pre-render information.
#[derive(Clone, Debug)]
pub struct PreRenderExtra {
	/// Raw pointer to the PF_PreRenderExtra structure
	raw_ptr: *mut PF_PreRenderExtraRaw,
}

/// Raw PF_PreRenderExtra structure (simplified based on AE SDK)
#[repr(C)]
#[derive(Clone, Debug)]
pub struct PF_PreRenderExtraRaw {
	pub input: *mut PF_PreRenderInput,
	pub output: *mut PF_PreRenderOutput,
	pub cb: *mut PF_PreRenderCallbacksRaw,
}

/// Raw PF_PreRenderCallbacks structure (simplified based on AE SDK)
#[repr(C)]
#[derive(Clone, Debug)]
pub struct PF_PreRenderCallbacksRaw {
	pub checkout_layer: Option<
		unsafe extern "C" fn(
			PF_ProgPtr,
			i32,
			i32,
			*const PF_RenderRequest,
			i64,
			i64,
			u64,
			*mut PF_CheckoutResultRaw,
		) -> i32,
	>,
	pub guid_mix_in_ptr:
		Option<unsafe extern "C" fn(PF_ProgPtr, u64, *const std::os::raw::c_void) -> i32>,
}

/// Raw PF_CheckoutResult structure (simplified based on AE SDK)
#[repr(C)]
#[derive(Clone, Debug)]
pub struct PF_CheckoutResultRaw {
	pub result_rect: PF_LRect,
	pub max_result_rect: PF_LRect,
	pub par: after_effects_sys::PF_RationalScale,
	pub solid: bool,
	pub reserved_b: [bool; 3],
	pub ref_width: i32,
	pub ref_height: i32,
	pub reserved: [i32; 6],
}

impl PreRenderExtra {
	/// Creates a new PreRenderExtra from a raw pointer
	///
	/// # Safety
	/// This function is unsafe because it requires a valid pointer to a PF_PreRenderExtra structure.
	/// The caller must ensure that pointer is valid and the structure it points to is properly initialized.
	pub unsafe fn from_raw_ptr(raw_ptr: *mut std::ffi::c_void) -> Option<Self> {
		if raw_ptr.is_null() {
			return None;
		}

		Some(PreRenderExtra {
			raw_ptr: raw_ptr as *mut PF_PreRenderExtraRaw,
		})
	}

	/// Returns the raw pointer
	pub fn as_raw_ptr(&self) -> *mut PF_PreRenderExtraRaw {
		self.raw_ptr
	}

	/// Returns the input structure if available
	pub fn input(&self) -> Option<&PF_PreRenderInput> {
		if self.raw_ptr.is_null() {
			return None;
		}
		unsafe { (*self.raw_ptr).input.as_ref() }
	}

	/// Returns the output structure if available
	pub fn output(&self) -> Option<&PF_PreRenderOutput> {
		if self.raw_ptr.is_null() {
			return None;
		}
		unsafe { (*self.raw_ptr).output.as_ref() }
	}

	/// Returns the callbacks structure if available
	pub fn callbacks(&self) -> Option<&PF_PreRenderCallbacksRaw> {
		if self.raw_ptr.is_null() {
			return None;
		}
		unsafe { (*self.raw_ptr).cb.as_ref() }
	}
}

impl Drop for PreRenderExtra {
	fn drop(&mut self) {
		log::debug!("PreRenderExtra dropped");
	}
}

/// Safe wrapper for PF_SmartRenderExtra (simplified version)
///
/// This structure encapsulates the data provided by After Effects during the smart render phase,
/// allowing access to pre-computed data from the pre-render phase.
#[derive(Clone, Debug)]
pub struct SmartRenderExtra {
	/// Raw pointer to the PF_SmartRenderExtra structure
	raw_ptr: *mut PF_SmartRenderExtraRaw,
	/// Reference to pre-render data store
	pre_render_data: PreRenderDataRef,
}

/// Raw PF_SmartRenderExtra structure (simplified based on AE SDK)
#[repr(C)]
#[derive(Clone, Debug)]
pub struct PF_SmartRenderExtraRaw {
	pub input: *mut PF_SmartRenderInput,
	pub cb: *mut PF_SmartRenderCallbacksRaw,
}

/// Raw PF_SmartRenderCallbacks structure (simplified based on AE SDK)
#[repr(C)]
#[derive(Clone, Debug)]
pub struct PF_SmartRenderCallbacksRaw {
	pub checkout_layer_pixels: Option<
		unsafe extern "C" fn(PF_ProgPtr, i32, *mut *mut after_effects_sys::PF_EffectWorld) -> i32,
	>,
	pub checkin_layer_pixels: Option<unsafe extern "C" fn(PF_ProgPtr, i32) -> i32>,
	pub checkout_output: Option<
		unsafe extern "C" fn(PF_ProgPtr, *mut *mut after_effects_sys::PF_EffectWorld) -> i32,
	>,
}

impl SmartRenderExtra {
	/// Creates a new SmartRenderExtra from a raw pointer
	///
	/// # Safety
	/// This function is unsafe because it requires a valid pointer to a PF_SmartRenderExtra structure.
	/// The caller must ensure that pointer is valid and the structure it points to is properly initialized.
	pub unsafe fn from_raw_ptr(raw_ptr: *mut std::ffi::c_void) -> Option<Self> {
		if raw_ptr.is_null() {
			return None;
		}

		let extra_raw = raw_ptr as *mut PF_SmartRenderExtraRaw;
		let pre_render_data = PreRenderDataStore::get(0); // Effect ref not available from this structure

		Some(SmartRenderExtra {
			raw_ptr: extra_raw,
			pre_render_data,
		})
	}

	/// Returns the raw pointer
	pub fn as_raw_ptr(&self) -> *mut PF_SmartRenderExtraRaw {
		self.raw_ptr
	}

	/// Returns the input structure if available
	pub fn input(&self) -> Option<&PF_SmartRenderInput> {
		if self.raw_ptr.is_null() {
			return None;
		}
		unsafe { (*self.raw_ptr).input.as_ref() }
	}

	/// Returns the callbacks structure if available
	pub fn callbacks(&self) -> Option<&PF_SmartRenderCallbacksRaw> {
		if self.raw_ptr.is_null() {
			return None;
		}
		unsafe { (*self.raw_ptr).cb.as_ref() }
	}

	/// Returns a reference to the pre-render data
	pub fn pre_render_data(&self) -> &PreRenderDataRef {
		&self.pre_render_data
	}

	/// Returns a mutable reference to the pre-render data
	pub fn pre_render_data_mut(&mut self) -> &mut PreRenderDataRef {
		&mut self.pre_render_data
	}
}

impl Drop for SmartRenderExtra {
	fn drop(&mut self) {
		log::debug!("SmartRenderExtra dropped");
	}
}

// ============================================================================
// PreRender Data Management
// ============================================================================

/// Reference to pre-render data stored between phases
#[derive(Clone, Debug, Default)]
pub struct PreRenderDataRef {
	/// Output layer reference
	output_layer: Option<OutputLayerRef>,
	/// Frame timestamp for this render
	frame_time: i64,
	/// Render width
	width: i32,
	/// Render height
	height: i32,
	/// Whether data is valid
	valid: bool,
}

impl PreRenderDataRef {
	/// Creates a new pre-render data reference
	pub fn new(width: i32, height: i32, frame_time: i64) -> Self {
		Self {
			output_layer: None,
			frame_time,
			width,
			height,
			valid: true,
		}
	}

	/// Sets the output layer reference
	pub fn set_output_layer(&mut self, layer: OutputLayerRef) {
		self.output_layer = Some(layer);
	}

	/// Returns the output layer reference if available
	pub fn output_layer(&self) -> Option<&OutputLayerRef> {
		self.output_layer.as_ref()
	}

	/// Returns the frame time
	pub fn frame_time(&self) -> i64 {
		self.frame_time
	}

	/// Returns the render dimensions
	pub fn dimensions(&self) -> (i32, i32) {
		(self.width, self.height)
	}

	/// Returns whether the data is valid
	pub fn is_valid(&self) -> bool {
		self.valid
	}
}

/// Reference to an output layer stored during pre-render
#[derive(Clone, Debug)]
pub struct OutputLayerRef {
	/// Layer dimensions
	width: i32,
	height: i32,
	/// Whether the layer has been initialized
	initialized: bool,
	/// Timestamp when this layer was created
	timestamp: i64,
}

impl OutputLayerRef {
	/// Creates a new output layer reference
	pub fn new(width: i32, height: i32) -> Self {
		Self {
			width,
			height,
			initialized: false,
			timestamp: 0,
		}
	}

	/// Returns the layer dimensions
	pub fn dimensions(&self) -> (i32, i32) {
		(self.width, self.height)
	}

	/// Returns whether the layer is initialized
	pub fn is_initialized(&self) -> bool {
		self.initialized
	}

	/// Marks the layer as initialized
	pub fn mark_initialized(&mut self, timestamp: i64) {
		self.initialized = true;
		self.timestamp = timestamp;
	}

	/// Returns the timestamp when the layer was initialized
	pub fn timestamp(&self) -> i64 {
		self.timestamp
	}
}

/// Thread-safe storage for pre-render data between render phases
pub(crate) struct PreRenderDataStore {
	/// Storage keyed by effect_ref (as usize for hashing)
	data: std::collections::HashMap<usize, PreRenderDataRef>,
}

// SAFETY: We ensure exclusive access via Mutex
unsafe impl Send for PreRenderDataStore {}
unsafe impl Sync for PreRenderDataStore {}

/// Global pre-render data store
static PRE_RENDER_DATA_STORE: Mutex<Option<PreRenderDataStore>> = Mutex::new(None);

/// PreRender data store management functions
impl PreRenderDataStore {
	/// Stores pre-render data for the given effect_ref
	pub fn store(effect_ref: usize, data: PreRenderDataRef) {
		let mut store = PRE_RENDER_DATA_STORE.lock().unwrap();
		let storage = store.get_or_insert_with(|| PreRenderDataStore {
			data: std::collections::HashMap::new(),
		});
		storage.data.insert(effect_ref, data);
		log::debug!(
			"PreRenderDataStore: stored data for effect_ref={:#x}",
			effect_ref
		);
	}

	/// Retrieves pre-render data for the given effect_ref
	pub fn get(effect_ref: usize) -> PreRenderDataRef {
		let store = PRE_RENDER_DATA_STORE.lock().unwrap();
		store
			.as_ref()
			.and_then(|storage| storage.data.get(&effect_ref).cloned())
			.unwrap_or_default()
	}

	/// Removes pre-render data for the given effect_ref
	pub fn remove(effect_ref: usize) {
		let mut store = PRE_RENDER_DATA_STORE.lock().unwrap();
		if let Some(storage) = store.as_mut() {
			storage.data.remove(&effect_ref);
			log::debug!(
				"PreRenderDataStore: removed data for effect_ref={:#x}",
				effect_ref
			);
		}
	}

	/// Clears all pre-render data
	pub fn clear_all() {
		let mut store = PRE_RENDER_DATA_STORE.lock().unwrap();
		if let Some(storage) = store.as_mut() {
			storage.data.clear();
			log::debug!("PreRenderDataStore: cleared all data");
		}
	}
}

// ============================================================================
// Output Layer Storage
// ============================================================================

/// Global output layer storage for SmartRender
pub(crate) struct OutputLayerStorage {
	/// Storage keyed by effect_ref (as usize for hashing)
	// We store only metadata since Layer doesn't implement Clone
	layer_metadata: std::collections::HashMap<usize, LayerMetadata>,
}

/// Layer metadata for storage
#[derive(Clone, Debug)]
struct LayerMetadata {
	width: u32,
	height: u32,
}

// SAFETY: We ensure exclusive access via Mutex
unsafe impl Send for OutputLayerStorage {}
unsafe impl Sync for OutputLayerStorage {}

/// Global output layer storage
static OUTPUT_LAYER_STORAGE: Mutex<Option<OutputLayerStorage>> = Mutex::new(None);

/// Output layer storage management functions
impl OutputLayerStorage {
	/// Stores output layer metadata for the given effect_ref
	pub fn store(effect_ref: usize, layer: &Layer<Depth8>) {
		let width = layer.width();
		let height = layer.height();
		let metadata = LayerMetadata { width, height };
		let mut storage = OUTPUT_LAYER_STORAGE.lock().unwrap();
		let storage_inner = storage.get_or_insert_with(|| OutputLayerStorage {
			layer_metadata: std::collections::HashMap::new(),
		});
		storage_inner.layer_metadata.insert(effect_ref, metadata);
		log::debug!(
			"OutputLayerStorage: stored layer metadata for effect_ref={:#x}, dimensions={}x{}",
			effect_ref,
			width,
			height
		);
	}

	/// Creates a new output layer for the given effect_ref
	pub fn create(
		effect_ref: usize,
		width: u32,
		height: u32,
	) -> Result<Layer<Depth8>, wrapper::LayerError> {
		let layer = Layer::black(width, height);
		Self::store(effect_ref, &layer);
		Ok(layer)
	}

	/// Removes an output layer for the given effect_ref
	pub fn remove(effect_ref: usize) {
		let mut storage = OUTPUT_LAYER_STORAGE.lock().unwrap();
		if let Some(storage_inner) = storage.as_mut() {
			storage_inner.layer_metadata.remove(&effect_ref);
			log::debug!(
				"OutputLayerStorage: removed layer metadata for effect_ref={:#x}",
				effect_ref
			);
		}
	}

	/// Clears all output layers
	pub fn clear_all() {
		let mut storage = OUTPUT_LAYER_STORAGE.lock().unwrap();
		if let Some(storage_inner) = storage.as_mut() {
			storage_inner.layer_metadata.clear();
			log::debug!("OutputLayerStorage: cleared all layer metadata");
		}
	}

	/// Returns the PF_EffectWorld structure for a stored layer
	pub fn as_effect_world(effect_ref: usize) -> Option<after_effects_sys::PF_EffectWorld> {
		let storage = OUTPUT_LAYER_STORAGE.lock().unwrap();
		storage.as_ref().and_then(|storage_inner| {
			storage_inner
				.layer_metadata
				.get(&effect_ref)
				.map(|metadata| {
					let mut world =
						unsafe { std::mem::zeroed::<after_effects_sys::PF_EffectWorld>() };
					world.width = metadata.width as i32;
					world.height = metadata.height as i32;
					world.rowbytes = (metadata.width * 4) as i32; // Assuming 4 bytes per pixel
					world.data = std::ptr::null_mut(); // Will be set by the layer creation
					world
				})
		})
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_pre_render_data_ref_default() {
		let data = PreRenderDataRef::default();
		assert!(!data.is_valid());
		assert_eq!(data.frame_time(), 0);
		assert_eq!(data.dimensions(), (0, 0));
		assert!(data.output_layer().is_none());
	}

	#[test]
	fn test_pre_render_data_ref_new() {
		let data = PreRenderDataRef::new(1920, 1080, 1000);
		assert!(data.is_valid());
		assert_eq!(data.frame_time(), 1000);
		assert_eq!(data.dimensions(), (1920, 1080));
	}

	#[test]
	fn test_output_layer_ref_new() {
		let layer = OutputLayerRef::new(1280, 720);
		assert!(!layer.is_initialized());
		assert_eq!(layer.dimensions(), (1280, 720));
		assert_eq!(layer.timestamp(), 0);
	}

	#[test]
	fn test_output_layer_ref_initialize() {
		let mut layer = OutputLayerRef::new(1280, 720);
		layer.mark_initialized(5000);
		assert!(layer.is_initialized());
		assert_eq!(layer.timestamp(), 5000);
	}

	#[test]
	fn test_pre_render_data_store() {
		let effect_ref = 0x12345678_usize;
		let data = PreRenderDataRef::new(1920, 1080, 1000);

		PreRenderDataStore::store(effect_ref, data.clone());
		let retrieved = PreRenderDataStore::get(effect_ref);
		assert!(retrieved.is_valid());
		assert_eq!(retrieved.dimensions(), (1920, 1080));
		assert_eq!(retrieved.frame_time(), 1000);

		PreRenderDataStore::remove(effect_ref);
		let after_remove = PreRenderDataStore::get(effect_ref);
		assert!(!after_remove.is_valid());
	}
}
