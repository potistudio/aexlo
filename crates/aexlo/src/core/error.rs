//! Error types for crate.
//!
//! This module defines structured error types using `thiserror`,
//! providing clear error messages and proper interoperability with
//! `dlopen2::Error` and other error types.

use std::path::PathBuf;

use thiserror::Error;

use crate::ParamKind;
use crate::pixel::{LayerError, PixelDepthKind};

/// The main error type for crate's operations.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum AexloError {
	/// Error occurred while loading the plugin file.
	#[error("Failed to load plugin: {0}")]
	PluginLoad(#[from] dlopen2::Error),

	/// No plugin binary exists at `path`: neither a file nor a `.plugin`
	/// bundle with an executable in `Contents/MacOS`.
	#[error("Plugin not found: {}", path.display())]
	PluginNotFound {
		/// The path that was looked up.
		path: PathBuf,
	},

	/// A `.plugin` bundle holds more than one executable in `Contents/MacOS`,
	/// so which one to load is ambiguous.
	#[error("Ambiguous plugin bundle {}: expected exactly one executable in Contents/MacOS", path.display())]
	AmbiguousBundle {
		/// The bundle directory.
		path: PathBuf,
	},

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
	PluginExecutionFailed {
		/// The `PF_Cmd_*` name, without the prefix (e.g. `SMART_RENDER`).
		command: String,
		/// The `PF_Err` the plugin returned.
		code: i64,
	},

	/// [`Host::install`](crate::Host::install) was called after the host was
	/// already fixed (by an earlier `install` or [`Host::get`](crate::Host::get)).
	#[error("App host is already fixed; call Host::install once, before Host::get")]
	AppHostAlreadySet,

	/// A parameter index outside `1..=max`. Index 0 is the input layer, which
	/// is set with [`set_input_layer`](crate::PluginInstance::set_input_layer).
	#[error("Parameter index {index} out of bounds (max {max})")]
	ParamIndexOutOfBounds {
		/// The index that was passed.
		index: usize,
		/// The highest valid parameter index.
		max: usize,
	},

	/// A value of one kind was set on a parameter declared as another.
	#[error("Parameter {index} type mismatch: expected {expected:?}, got {actual:?}")]
	ParamTypeMismatch {
		/// The parameter index.
		index: usize,
		/// The kind the passed value is for.
		expected: ParamKind,
		/// The kind the plugin declared the parameter as.
		actual: ParamKind,
	},

	/// The plugin's smart pre-render did not set
	/// `PF_RenderOutputFlag_GPU_RENDER_POSSIBLE`, so the frame must render on
	/// the CPU (as After Effects would);
	/// [`render_frame`](crate::PluginInstance::render_frame) does so.
	#[error("The plugin declared this frame not renderable on the GPU")]
	GpuRenderDeclined,

	/// No GPU device (Metal on macOS, CUDA on Windows) could be opened for a
	/// GPU render; [`render_frame`](crate::PluginInstance::render_frame)
	/// renders on the CPU instead.
	#[error("No GPU device available for GPU render")]
	GpuUnavailable,

	/// Setting up GPU buffers or moving pixels to or from the device failed.
	#[error("GPU error: {0}")]
	Gpu(String),

	/// A pixel-buffer operation failed (dimension mismatch, ...).
	#[error("Layer error: {0}")]
	Layer(#[from] LayerError),

	/// A layer was requested at a depth other than the one it has (see
	/// [`PluginInstance::read_output`](crate::PluginInstance::read_output)).
	#[error("Requested a {requested} layer, but it is {actual}")]
	DepthMismatch {
		/// The depth asked for.
		requested: PixelDepthKind,
		/// The depth the layer has.
		actual: PixelDepthKind,
	},
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
