//! The callback vtables aexlo hands a plugin.
//!
//! A callback named `*_stub` reports `PF_Err_INVALID_CALLBACK` without doing
//! what it was asked; every other callback does what the SDK describes.

pub mod app;
pub mod host_layer;
pub mod interact;
pub mod layer_param;
pub mod params;
pub mod smart_render;
pub mod utility;
