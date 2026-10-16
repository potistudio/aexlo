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
//! instance.render_frame()?;
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
mod observe;
mod param_value;
mod pixel;
mod strict;
mod time;
mod utils;

pub(crate) mod suites;

pub use core::error::{AexloError, Result};
pub use instance::{PluginInstance, RenderPath};
pub use time::Time;

/// Entry point ABI for driving an in-process effect via
/// [`Host::from_entry`].
pub use instance::PluginEntryPoint;

pub use param_value::{ParamKind, ParamValue};

/// The GPU this host renders on, for machine fingerprints.
pub use gpu::device_name as gpu_device_name;

/// Strict mode ([`PluginInstance::set_strict`]).
pub use strict::{
	CheckoutViolation, GuardViolation, Leak, POISON_BYTE, POISON_F32_BITS, Strict, StrictReport, is_unwritten,
	unwritten_pixels,
};

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

/// Safe pixel/layer wrappers, re-exported explicitly so additions to the
/// `pixel` module don't silently widen this crate's public API.
pub use pixel::{AnyLayer, Depth8, Depth16, Depth32, Layer, LayerError, Pixel, PixelDepth, PixelDepthKind};

/// Observation hooks ([`PluginInstance::set_observer`]).
pub use observe::{
	AllocationEvent, AllocationKind, CheckoutEvent, CheckoutKind, CommandEvent, CommandPhase, LogObserver,
	ObserveLevel, Observer, SuiteCallEvent, command_name,
};
