//! SmartRender Host Callbacks
//!
//! This module implements the callback structures and functions required for After Effects
//! SmartRender support. SmartRender enables two-phase rendering where expensive operations
//! are pre-computed and reused across frames.

pub mod callbacks;
pub mod data;
pub mod pre_render;
pub mod render;

// Re-exports for convenience
pub use callbacks::{create_pre_render_callbacks, create_smart_render_callbacks};
pub use data::{
	clear_all_output_layers, get_output_layer, remove_output_layer, store_output_layer,
};
pub use pre_render::PreRenderContext;
pub use render::SmartRenderContext;
