//! A layer owned by the host (input, output or a linked layer parameter),
//! whose depth `PF_GetPixelFormat` must report.

use std::cell::OnceCell;
use std::hash::{DefaultHasher, Hash, Hasher};

use rayon::prelude::*;

use crate::pixel::{AnyLayer, PixelDepthKind};

/// An [`AnyLayer`] whose pixel buffer is tagged with its pixel format for as
/// long as it lives, so `PF_GetPixelFormat` reports `ARGB64`/`ARGB128` for a
/// 16/32 bpc world the host hands out (8 bpc is the untagged default).
///
/// The tag is keyed by the buffer's address, which stays put however the
/// layer moves; replacing the buffer must go through a new `HostLayer`.
pub(crate) struct HostLayer {
	layer: AnyLayer,
	digest: OnceCell<u64>,
}

impl HostLayer {
	pub(crate) fn new(layer: AnyLayer) -> Self {
		let bytes = layer.len() * layer.kind().bytes_per_pixel();
		crate::suites::world::tag_host_world(layer.data_ptr() as usize, bytes, layer.kind());
		Self {
			layer,
			digest: OnceCell::new(),
		}
	}

	pub(crate) fn layer(&self) -> &AnyLayer {
		&self.layer
	}

	/// Mutable access to the pixels. Callers must not swap the buffer out.
	pub(crate) fn layer_mut(&mut self) -> &mut AnyLayer {
		self.digest.take();
		&mut self.layer
	}

	/// A hash of the size, depth and pixels, taken when first asked for. By
	/// content, so that the same picture set again is the same.
	pub(crate) fn digest(&self) -> u64 {
		*self.digest.get_or_init(|| {
			let len = self.layer.len() * self.layer.kind().bytes_per_pixel();
			// SAFETY: the layer's pixels are `len` plain bytes, borrowed with it.
			let bytes = unsafe { std::slice::from_raw_parts(self.layer.data_ptr(), len) };
			let parts: Vec<u64> = bytes
				.par_chunks(1 << 20)
				.map(|part| {
					let mut hash = DefaultHasher::new();
					hash.write(part);
					hash.finish()
				})
				.collect();
			let mut hash = DefaultHasher::new();
			(self.width(), self.height(), self.kind().bits(), parts).hash(&mut hash);
			hash.finish()
		})
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

	pub(crate) fn as_sys(&mut self) -> crate::sys::PF_LayerDef {
		self.layer.as_sys()
	}
}

impl Drop for HostLayer {
	fn drop(&mut self) {
		crate::suites::world::untag_host_world(self.layer.data_ptr() as usize);
	}
}
