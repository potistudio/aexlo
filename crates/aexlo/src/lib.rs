//! aexlo - After Effects Plugin(.aex) Loader and Emulator
//!
//! This crate provides functionality to load and execute After Effects plugins (.aex)
//! outside of After Effects, enabling testing, automation, and custom rendering pipelines.
//!
//! # Example
//!
//! ```no_run
//! use aexlo::{Depth8, Host, Layer, ParamValue, Time};
//!
//! # fn main() -> aexlo::Result<()> {
//! // Fix the host services first (`Host::install(my_app_host)?` to customize).
//! let host = Host::get();
//!
//! // Load the plugin and run GLOBAL_SETUP, PARAMS_SETUP and SEQUENCE_SETUP.
//! // On Windows pass the `.aex` file, on macOS the `.plugin` bundle.
//! let mut fx = host.try_load("MyEffect.aex")?;
//! println!("{}", fx.about()?);
//!
//! // Feed an input frame and set parameters (index 0 is the input layer).
//! fx.set_input_layer(Layer::<Depth8>::black(1920, 1080));
//! fx.set_param(1, ParamValue::Float(0.5))?;
//! fx.set_time(Time::frame(0, 30));
//!
//! // Render the way After Effects picks the path: GPU, smart, then legacy.
//! fx.render_frame()?;
//! let output: Layer<Depth8> = fx.read_output()?;
//! # Ok(())
//! # }
//! ```
//!
//! # Features
//!
//! - `diagnostics` - Print every suite call the plugin makes, with its
//!   arguments, to stdout

#![warn(clippy::all, missing_docs, clippy::missing_errors_doc)]
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
