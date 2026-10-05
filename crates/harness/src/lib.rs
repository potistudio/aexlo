//! The aexlo toolkit's engine (see `docs/toolkit.md`): one declarative
//! description of "render this plugin under these conditions" (a preset in
//! `aexlo.toml`) that drives tests, benchmarks and previews alike.
//!
//! - [`manifest`]: `aexlo.toml`, `inherits`, matrix expansion into [`Variant`]s.
//! - [`input`]: generated and file inputs.
//! - [`apply`]: putting a variant on a [`PluginInstance`](aexlo::PluginInstance).
//! - [`frame`]: rendered frames, comparison, PNG/EXR files.
//! - [`exec`]: running a variant in-process or in an `aexlo worker`.
//! - [`runner`]: outcomes for a set of variants.

pub mod apply;
pub mod check;
pub mod error;
pub mod exec;
pub mod frame;
pub mod golden;
pub mod input;
pub mod judge;
pub mod manifest;
pub mod preset;
pub mod preview;
pub mod runner;
pub mod worker;

pub use error::{Error, ErrorKind, Result};
pub use exec::{Executor, InProcess, RunFailure, RunOptions, RunOutput, Timing, Trace};
pub use frame::{Comparison, Frame};
pub use manifest::Manifest;
pub use preset::{
	Axis, BenchSpec, Fit, Fps, Generator, GoldenSource, GoldenSpec, InputSpec, Iterate, PluginRef, PluginSource,
	RenderMode, StrictFeature, Time, Tolerance, Variant, VariantId,
};
