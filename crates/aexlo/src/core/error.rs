//! Error types for the aexlo library.
//!
//! This module defines structured error types using `thiserror`,
//! providing clear error messages and proper interoperability with
//! `dlopen2::Error` and other error types.

use thiserror::Error;

/// The main error type for aexlo operations.
#[derive(Debug, Error)]
pub enum AexloError {
	/// Error occurred while loading the plugin file.
	#[error("Failed to load plugin: {0}")]
	PluginLoad(#[from] dlopen2::Error),

	/// The plugin file was not found at the specified path.
	#[error("Plugin not found: {path}")]
	PluginNotFound { path: String },

	/// Invalid path configuration (missing directory or file).
	#[error("Invalid path: {message}")]
	InvalidPath { message: String },

	/// The plugin container is not loaded.
	#[error("Plugin container is not loaded.")]
	ContainerNotLoaded,

	/// The plugin returned a non-zero `PF_Err` code during execution.
	///
	/// `command` names the `PF_Cmd_*` that failed — essential context, since
	/// hosts like [`render_frame`](crate::PluginInstance::render_frame) chain
	/// GPU → smart → legacy fallbacks and the final error alone doesn't say
	/// which stage rejected the call.
	#[error("Plugin rejected {command} with error code: {code}")]
	PluginExecutionFailed { command: String, code: i64 },

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

	/// A pixel-buffer operation failed (dimension mismatch, ...).
	#[error("Layer error: {0}")]
	Layer(#[from] wrapper::LayerError),

	#[error("Unexpected error: {0}")]
	Unexpected(String),
}

/// A specialized Result type for aexlo operations.
pub type Result<T> = std::result::Result<T, AexloError>;
