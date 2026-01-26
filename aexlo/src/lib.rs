//! aexlo — After Effects Plugin Loader and Emulator
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
//! instance.about().expect("About command failed"); // Get plugin info
//! instance.setup_global().expect("Global setup failed"); // Setup global state (flags, etc.)
//! instance.setup_params().expect("Params setup failed"); // Setup parameters
//! instance.render().expect("Render failed"); // Render frame
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

// Internal modules (not exposed)
pub mod core;
pub mod host;

// Internal modules exposed for advanced use
pub(crate) mod suites;

// ============================================================================
// Public API
// ============================================================================

/// Re-export `PF_Pixel` for working with pixel data.
pub use after_effects_sys::PF_Pixel;

/// Error types for aexlo operations.
pub use core::error::{AexloError, Result};

/// The core plugin loader and executor.
pub use core::instance::PluginInstance;

/// Diagnostic utilities (feature-gated).
#[cfg(feature = "diagnostics")]
pub use core::diagnostics::{Diagnostic, DiagnosticBuilder};
