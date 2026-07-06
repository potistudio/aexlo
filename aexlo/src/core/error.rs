//! Error types for the aexlo library.
//!
//! This module defines structured error types using `thiserror`,
//! providing clear error messages and proper interoperability with
//! `dlopen2::Error` and other error types.

use thiserror::Error;

/// The main error type for aexlo operations.
#[derive(Debug, Error)]
pub enum AexloError {
	/// Error occurred while loading the plugin DLL/dylib.
	#[error("Failed to load plugin: {0}")]
	PluginLoad(#[from] dlopen2::Error),

	/// The plugin file was not found at the specified path.
	#[error("Plugin not found: {path}")]
	PluginNotFound { path: String },

	/// The operating system is not supported.
	#[error("Unsupported OS: {os}. Supported platforms are Windows and macOS.")]
	UnsupportedOS { os: String },

	/// Invalid path configuration (missing directory or filename).
	#[error("Invalid path: {message}")]
	InvalidPath { message: String },

	/// The plugin container is not loaded.
	#[error("Plugin container is not loaded. Call load() before calling the plugin.")]
	ContainerNotLoaded,

	/// The plugin returned a non-zero `PF_Err` code during execution.
	///
	/// The raw code is widened to `i64` so this variant has the same shape on
	/// every platform (the underlying `PF_Err` is `u32` on macOS and `i32`
	/// elsewhere), keeping downstream `match` arms portable.
	#[error("Plugin execution failed with error code: {code}")]
	PluginExecutionFailed { code: i64 },

	/// Parameter index is out of bounds.
	#[error("Parameter index {index} out of bounds (max {max})")]
	ParamIndexOutOfBounds { index: usize, max: usize },

	/// Parameter type mismatch.
	#[error("Parameter {index} type mismatch: expected {expected}, got type {actual}")]
	ParamTypeMismatch {
		index: usize,
		expected: &'static str,
		actual: i32,
	},

	#[error("Unexpected error: {0}")]
	Unexpected(String),
}

/// A specialized Result type for aexlo operations.
pub type Result<T> = std::result::Result<T, AexloError>;
