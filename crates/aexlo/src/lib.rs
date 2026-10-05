//! aexlo - After Effects Plugin(.aex) Loader and Emulator
//!
//! This crate provides functionality to load and execute After Effects plugins (.aex)
//! outside of After Effects, enabling testing, automation, and custom rendering pipelines.
//!
//! # Example
//!
//! ```no_run
//! use aexlo::{Host, PluginInstance};
//! use std::path::Path;
//!
//! # fn main() -> aexlo::Result<()> {
//! // Fix the host services first (`Host::install(my_app_host)?` to customize).
//! let host = Host::get();
//!
//! // `try_load` loads the library, runs GLOBAL_SETUP and PARAMS_SETUP.
//! let mut instance = host.try_load(Path::new("ExamplePlugin"))?;
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
//! - `diagnostics` - Enable detailed diagnostic logging for debugging

#![warn(clippy::all)]
// The crate mirrors the After Effects C SDK, whose suite struct fields and entry
// points (`PF_GetAppName`, `EffectMain`, …) use non-snake-case names. Allowing it
// crate-wide keeps the FFI surface readable without per-item annotations.
#![allow(non_snake_case)]

mod core;
mod gpu;
mod host;
mod instance;
mod mask;
mod param_value;
mod preview;
mod utils;

pub(crate) mod suites;

pub use core::error::{AexloError, Result};
pub use instance::PluginInstance;

/// Entry point ABI for driving an in-process effect via
/// [`Host::from_entry`].
pub use instance::PluginEntryPoint;

/// Preview helpers used by the [`macro@preview`] attribute macro (and usable
/// directly): where to write a preview PNG, whether one was requested, how to
/// open it, and how to drive a live `aexlo view` window.
///
/// These are dev-tooling, not plugin hosting; they live in their own module
/// (see `src/preview.rs`) and are re-exported here for the macro's benefit.
pub use preview::{
	PreviewMode, ViewerLock, acquire_viewer_lock, ensure_live_viewer, open_in_viewer, open_preview, preview_mode,
	preview_path, preview_requested, save_preview, viewer_is_running,
};

/// `#[aexlo::preview]` - render a plugin in-process and drop a preview PNG.
pub use aexlo_macros::preview;

pub use param_value::ParamValue;

/// Layer masks served through the Path Query/Data suites
/// ([`PluginInstance::set_mask_paths`]).
pub use mask::{MaskMode, MaskPath, MaskVertex};

/// Injectable host-application services (App, Adv App/Item/Time, Plugin
/// Helper and Custom UI Overlay Theme suites): UI colors, fonts, language,
/// color picker, progress dialogs, Info panel, time display, tools, ...
/// Headless by default.
pub use host::app::{
	AppColor, AppHost, AppPixel8, AppPixelF, AppPoint, FontInfo, HeadlessAppHost, Host, InfoLine, OverlayTheme,
	PersonalInfo, ProgressId, TimeDisplayMode, TimeDisplayPref,
};

/// Diagnostic utilities (feature-gated).
pub use core::diagnostics::{Diagnostic, DiagnosticBuilder};

/// Safe pixel/layer wrappers, re-exported explicitly so additions to the
/// `wrapper` crate don't silently widen this crate's public API.
pub use wrapper::{Depth8, Depth16, Depth32, Layer, LayerError, Pixel, PixelDepth};
