//! SmartRender Callback Factory Functions
//!
//! This module provides factory functions for creating callback structures
//! required for SmartRender support.

use after_effects_sys::*;

/// Creates a PF_PreRenderCallbacks structure with all callbacks populated
pub fn create_pre_render_callbacks() -> PF_PreRenderCallbacks {
	crate::host::smart_render::pre_render::create_pre_render_callbacks()
}

/// Creates a PF_SmartRenderCallbacks structure with all callbacks populated
pub fn create_smart_render_callbacks() -> PF_SmartRenderCallbacks {
	crate::host::smart_render::render::create_smart_render_callbacks()
}
