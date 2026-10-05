//! A layer owned by the host (input, output or a linked layer parameter),
//! whose depth `PF_GetPixelFormat` must report.

use wrapper::{AnyLayer, PixelDepthKind};

/// An [`AnyLayer`] whose pixel buffer is tagged with its pixel format for as
/// long as it lives, so `PF_GetPixelFormat` reports `ARGB64`/`ARGB128` for a
/// 16/32 bpc world the host hands out (8 bpc is the untagged default).
///
/// The tag is keyed by the buffer's address, which stays put however the
/// layer moves; replacing the buffer must go through a new `HostLayer`.
pub(crate) struct HostLayer {
	layer: AnyLayer,
}

impl HostLayer {
	pub(crate) fn new(layer: AnyLayer) -> Self {
		crate::suites::world::tag_host_world(layer.data_ptr() as usize, layer.kind());
		Self { layer }
	}

	pub(crate) fn layer(&self) -> &AnyLayer {
		&self.layer
	}

	/// Mutable access to the pixels. Callers must not swap the buffer out.
	pub(crate) fn layer_mut(&mut self) -> &mut AnyLayer {
		&mut self.layer
	}

	pub(crate) fn kind(&self) -> PixelDepthKind {
		self.layer.kind()
	}

	pub(crate) fn width(&self) -> u32 {
		self.layer.width()
	}

	pub(crate) fn height(&self) -> u32 {
		self.layer.height()
	}

	pub(crate) fn as_sys(&mut self) -> after_effects_sys::PF_LayerDef {
		self.layer.as_sys()
	}
}

impl Drop for HostLayer {
	fn drop(&mut self) {
		crate::suites::world::untag_host_world(self.layer.data_ptr() as usize);
	}
}
