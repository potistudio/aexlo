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
//! let mut fx = host.load("MyEffect.aex")?;
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
//! # Trust and process model
//!
//! A plugin is native code, loaded into this process with no sandbox. Loading
//! one is what this crate is for, so [`Host::load`] and [`Loader::load`] are
//! safe functions, the way spawning a process is: what the plugin's code does
//! is outside Rust's guarantees. A buggy plugin can corrupt memory, crash or
//! abort the process that embeds aexlo, so only load plugins you trust.
//!
//! aexlo's own callbacks catch their panics and report
//! `PF_Err_INTERNAL_STRUCT_DAMAGED` to the plugin rather than letting them
//! abort the process, but a crash inside the plugin cannot be caught
//! in-process. When a crashing plugin must not take the application down, run
//! it in a worker process, as the `aexlo` CLI and its toolkit do for every
//! variant.
//!
//! ## Process-wide state
//!
//! Some host state belongs to the process, not to a [`PluginInstance`]:
//!
//! - The [`AppHost`] is fixed once per process ([`Host::install`] or
//!   [`Host::get`]) and cannot be swapped afterwards, even between tests.
//! - AEGP Persistent Data (preferences) is one in-memory store shared by every
//!   plugin and instance, and is never written to disk.
//! - AEGP registrations, GPU worlds and allocation tracking live in
//!   process-wide registries keyed by instance, so instances coexist.
//!
//! ## Threads
//!
//! [`PluginInstance`] is neither `Send` nor `Sync`. After Effects drives an
//! effect instance from one thread at a time, and plugins keep thread-affine
//! state (GPU contexts, Windows `HWND`s, thread-local storage), so create, use
//! and drop each instance on the same thread. To render off a UI thread,
//! create the instance on a worker thread and send that thread commands.
//!
//! ```compile_fail
//! fn assert_send<T: Send>() {}
//! assert_send::<aexlo::PluginInstance>();
//! ```
//!
//! Separate instances may run on separate threads at once: aexlo's
//! process-wide state is behind locks, and its own test suites run instances
//! in parallel. Whether a plugin's globals tolerate that is up to the plugin
//! (it declares `OutFlags2::SUPPORTS_THREADED_RENDERING` when they do).
//!
//! # Features
//!
//! - `diagnostics` - Print every suite call the plugin makes, with its
//!   arguments, to stdout

#![warn(
	clippy::all,
	missing_docs,
	clippy::missing_errors_doc,
	clippy::undocumented_unsafe_blocks
)]
// Tests call FFI fakes directly; their `unsafe` blocks need no proof.
#![cfg_attr(test, allow(clippy::undocumented_unsafe_blocks))]
// The crate mirrors the After Effects C SDK, whose suite struct fields and entry
// points (`PF_GetAppName`, `EffectMain`, …) use non-snake-case names. Allowing it
// crate-wide keeps the FFI surface readable without per-item annotations.
#![allow(non_snake_case)]

mod core;
mod flags;
mod gpu;
mod host;
mod instance;
mod mask;
mod observe;
mod param_value;
mod pipl;
mod pixel;
mod strict;
pub mod sys;
mod time;
mod utils;

pub(crate) mod suites;

pub use core::error::{AexloError, Result};
pub use instance::{CustomControlEvent, CustomControlImage, Loader, PluginInstance, RenderPath};
pub use time::Time;

/// Window plugins should treat as After Effects' main window
/// (`PF_PlatData_MAIN_WND`).
///
/// Editors parented here stay invisible while this window is hidden. A GUI
/// host passes its own visible window; `null` falls back to a window aexlo
/// creates. Only Windows has a main window; elsewhere this does nothing.
///
/// # Safety
/// `hwnd` must be null or a valid `HWND` that stays valid for as long as any
/// plugin may use it: plugins parent their windows to it and send it
/// messages. Pass null before destroying the window.
pub unsafe fn set_main_window(hwnd: *mut std::ffi::c_void) {
	#[cfg(windows)]
	crate::host::utility::set_main_window(hwnd);
	#[cfg(not(windows))]
	{
		let _ = hwnd;
	}
}

/// Entry point ABI for driving an in-process effect via
/// [`Host::from_entry`].
pub use instance::PluginEntryPoint;

pub use param_value::{ParamKind, ParamValue};

/// Typed flags and pixel formats a plugin declares.
pub use flags::{OutFlags, OutFlags2, ParamUiFlags, PixelFormat};

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
