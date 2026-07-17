//! aexlo — After Effects Plugin(.aex) Loader and Emulator
//!
//! This crate provides functionality to load and execute After Effects plugins (.aex)
//! outside of After Effects, enabling testing, automation, and custom rendering pipelines.
//!
//! # Example
//!
//! ```no_run
//! use aexlo::PluginInstance;
//!
//! # fn main() -> aexlo::Result<()> {
//! // `try_load` loads the library, runs GLOBAL_SETUP and PARAMS_SETUP.
//! let mut instance = PluginInstance::try_load(std::path::Path::new("SDK_Noise"))?;
//!
//! // Query plugin info (PF_Cmd_ABOUT).
//! let message = instance.about()?;
//! println!("{message}");
//!
//! // Render a frame.
//! instance.render()?;
//! # Ok(())
//! # }
//! ```
//!
//! # Features
//!
//! - `diagnostics` — Enable detailed diagnostic logging for debugging

// Required for variadic sprintf emulation in ANSI callbacks.
// This is an implementation detail and should not affect public API stability.
#![feature(c_variadic)]
// Enforce code quality
#![warn(clippy::all)]
// The crate mirrors the After Effects C SDK, whose suite struct fields and entry
// points (`PF_GetAppName`, `EffectMain`, …) use non-snake-case names. Allowing it
// crate-wide keeps the FFI surface readable without per-item annotations.
#![allow(non_snake_case)]

mod core;
mod gpu;
mod host;
mod instance;
mod preview;
mod utils;

pub(crate) mod suites;

pub use core::error::{AexloError, Result};
pub use instance::PluginInstance;

/// Entry point ABI for driving an in-process effect via
/// [`PluginInstance::from_entry`].
pub use instance::PluginEntryPoint;

/// Preview helpers used by the [`macro@preview`] attribute macro (and usable
/// directly): where to write a preview PNG, whether one was requested, how to
/// open it, and how to drive a live `aexlo view` window.
///
/// These are dev-tooling, not plugin hosting; they live in their own module
/// (see `src/preview.rs`) and are re-exported here for the macro's benefit.
pub use preview::{
	acquire_viewer_lock, ensure_live_viewer, open_in_viewer, preview_mode, preview_path, preview_requested,
	viewer_is_running, PreviewMode, ViewerLock,
};

/// `#[aexlo::preview]` — render a plugin in-process and drop a preview PNG.
pub use aexlo_macros::preview;

/// Parameter value type for reading and writing plugin parameters.
pub use instance::ParamValue;

/// Diagnostic utilities (feature-gated).
pub use core::diagnostics::{Diagnostic, DiagnosticBuilder};

/// Safe pixel/layer wrappers, re-exported explicitly so additions to the
/// `wrapper` crate don't silently widen this crate's public API.
pub use wrapper::{Depth16, Depth32, Depth8, Layer, LayerError, Pixel, PixelDepth};
