//! Shared default constants used across the host emulation layer.
//!
//! These are the fallback composition dimensions the emulator advertises to a
//! plugin when no explicit size is configured. Keeping them in one place avoids
//! the previous situation where `1920`/`1080` were duplicated (and occasionally
//! contradicted) across `instance`, `host::smart_render`, and `host::interact`.

/// Default composition/layer width in pixels.
pub(crate) const DEFAULT_WIDTH: u32 = 1920;

/// Default composition/layer height in pixels.
pub(crate) const DEFAULT_HEIGHT: u32 = 1080;
