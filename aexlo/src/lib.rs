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
//! let mut instance = PluginInstance::new(std::path::Path::new("SDK_Noise"));
//! instance.load().expect("Failed to load plugin"); // Initialize plugin
//!
//! // Get plugin info
//! instance.about().expect("About command failed");
//!
//! // Setup global state (flags, etc.)
//! instance.setup_global().expect("Global setup failed");
//!
//! // Setup parameters
//! instance.setup_params().expect("Params setup failed");
//!
//! // Render frame
//! instance.render().expect("Render failed");
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
#![allow(non_snake_case)]

pub mod core;
pub mod host;
mod instance;

pub(crate) mod suites;

pub use suites::macros;
pub use suites::registry;

// ============================================================================
// Public API
// ============================================================================

/// Re-export `PF_Pixel` for working with pixel data.
pub use after_effects_sys::PF_Pixel;

/// Error types for aexlo operations.
pub use core::error::{AexloError, Result};

/// The core plugin loader and executor.
pub use instance::PluginInstance;

/// Diagnostic utilities (feature-gated).
#[cfg(feature = "diagnostics")]
pub use core::diagnostics::{Diagnostic, DiagnosticBuilder};

pub use wrapper::*;
