//! Error types for crate.
//!
//! This module defines structured error types using `thiserror`,
//! providing clear error messages and proper interoperability with
//! `dlopen2::Error` and other error types.

use thiserror::Error;

/// The main error type for crate's operations.
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
	PluginNotLoaded,

	/// The plugin returned a non-zero error code during execution.
	///
	/// `command` names the `PF_Cmd_*` that failed - essential context, since
	/// hosts like [`render_frame`](crate::PluginInstance::render_frame) chain
	/// GPU → smart → legacy fallbacks and the final error alone doesn't say
	/// which stage rejected the call.
	#[error("Plugin rejected {command} with error code: {code}")]
	PluginExecutionFailed { command: String, code: i64 },

	/// [`Host::install`](crate::Host::install) was called after the host was
	/// already fixed (by an earlier `install` or [`Host::get`](crate::Host::get)).
	#[error("App host is already fixed; call Host::install once, before Host::get")]
	AppHostAlreadySet,

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

	/// The plugin's smart pre-render did not set
	/// `PF_RenderOutputFlag_GPU_RENDER_POSSIBLE`, so the frame must render on
	/// the CPU (as After Effects would);
	/// [`render_frame`](crate::PluginInstance::render_frame) does so.
	#[error("The plugin declared this frame not renderable on the GPU")]
	GpuRenderDeclined,

	/// A pixel-buffer operation failed (dimension mismatch, ...).
	#[error("Layer error: {0}")]
	Layer(#[from] wrapper::LayerError),

	/// A layer was requested at a depth other than the one it has (see
	/// [`PluginInstance::read_output`](crate::PluginInstance::read_output)).
	#[error("Requested a {requested} layer, but it is {actual}")]
	DepthMismatch {
		requested: wrapper::PixelDepthKind,
		actual: wrapper::PixelDepthKind,
	},

	#[error("Unexpected error: {0}")]
	Unexpected(String),
}

impl AexloError {
	/// Whether the plugin stopped because the render was cancelled
	/// (`PF_Interrupt_CANCEL`, e.g. after [`AppHost::abort_requested`](crate::AppHost::abort_requested)).
	pub fn is_cancelled(&self) -> bool {
		matches!(self, AexloError::PluginExecutionFailed { code, .. }
			if *code == after_effects_sys::PF_Interrupt_CANCEL as i64)
	}
}

/// A specialized Result type for aexlo operations.
pub type Result<T> = std::result::Result<T, AexloError>;
