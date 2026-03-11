//! SmartRender Data Management
//!
//! This module provides functions for managing output layers and pre-render data
//! between the pre-render and smart render phases.

use crate::core::smart_render::{OutputLayerStorage, PreRenderDataRef, PreRenderDataStore};
use after_effects_sys::PF_ProgPtr;
use wrapper::{Depth8, Layer};

/// Stores an output layer for the given effect_ref
pub fn store_output_layer(effect_ref: PF_ProgPtr, layer: Layer<Depth8>) {
	OutputLayerStorage::store(effect_ref as usize, &layer);
}

/// Retrieves an output layer for the given effect_ref
pub fn get_output_layer(effect_ref: PF_ProgPtr) -> Option<Layer<Depth8>> {
	let _ = effect_ref;
	None
}

/// Removes an output layer for the given effect_ref
pub fn remove_output_layer(effect_ref: PF_ProgPtr) {
	OutputLayerStorage::remove(effect_ref as usize);
}

/// Clears all output layers
pub fn clear_all_output_layers() {
	OutputLayerStorage::clear_all();
}

/// Creates a new output layer for the given effect_ref
pub fn create_output_layer(
	effect_ref: PF_ProgPtr,
	width: u32,
	height: u32,
) -> Result<Layer<Depth8>, wrapper::LayerError> {
	OutputLayerStorage::create(effect_ref as usize, width, height)
}

/// Returns the PF_EffectWorld structure for a stored layer
pub fn as_effect_world(effect_ref: PF_ProgPtr) -> Option<after_effects_sys::PF_EffectWorld> {
	OutputLayerStorage::as_effect_world(effect_ref as usize)
}

/// Stores pre-render data for the given effect_ref
pub fn store_pre_render_data(effect_ref: PF_ProgPtr, data: PreRenderDataRef) {
	PreRenderDataStore::store(effect_ref as usize, data);
}

/// Retrieves pre-render data for the given effect_ref
pub fn get_pre_render_data(effect_ref: PF_ProgPtr) -> PreRenderDataRef {
	PreRenderDataStore::get(effect_ref as usize)
}

/// Removes pre-render data for the given effect_ref
pub fn remove_pre_render_data(effect_ref: PF_ProgPtr) {
	PreRenderDataStore::remove(effect_ref as usize);
}

/// Clears all pre-render data
pub fn clear_all_pre_render_data() {
	PreRenderDataStore::clear_all();
}
