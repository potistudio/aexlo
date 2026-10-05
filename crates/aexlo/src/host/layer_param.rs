//! Layers bound to a plugin's own `PF_Param_LAYER` parameters (e.g. a "source
//! layer" popup), set through
//! [`PluginInstance::set_layer_param`](crate::PluginInstance::set_layer_param).

use after_effects_sys::{A_long, PF_LayerDef, PF_LayerDefault_MYSELF};

use crate::gpu::GPU_BYTES_PER_PIXEL;
use crate::host::host_layer::HostLayer;

/// A layer linked to a layer parameter, with the worlds handed to the plugin.
///
/// The worlds are boxed so their addresses stay stable: smart-render checkouts
/// hand them out by pointer, and GPU buffers are keyed by the GPU world's address.
pub(crate) struct LinkedLayer {
	layer: HostLayer,
	/// World over `layer`'s pixels at its own depth, for CPU renders and `checkout_param`.
	cpu_world: Box<PF_LayerDef>,
	/// `PF_PixelFormat_GPU_BGRA128` view of the same layer for GPU renders; its
	/// pixels live in a device buffer keyed by this world's address.
	gpu_world: Box<PF_LayerDef>,
	/// Whether the device buffer holds `layer`'s pixels.
	pub(crate) gpu_uploaded: bool,
	/// The parameter's definition before linking, restored on unlink.
	pub(crate) unlinked: PF_LayerDef,
}

impl LinkedLayer {
	pub(crate) fn new(layer: wrapper::AnyLayer, unlinked: PF_LayerDef) -> Self {
		let mut layer = HostLayer::new(layer);
		let mut cpu_world = layer.as_sys();
		// `PF_LayerDefault_NONE` marks an unlinked layer parameter, which plugins
		// test for; anything else reads as "a layer is selected".
		cpu_world.dephault = PF_LayerDefault_MYSELF as A_long;
		let mut gpu_world = cpu_world;
		gpu_world.rowbytes = layer.width() as i32 * GPU_BYTES_PER_PIXEL as i32;
		Self {
			layer,
			cpu_world: Box::new(cpu_world),
			gpu_world: Box::new(gpu_world),
			gpu_uploaded: false,
			unlinked,
		}
	}

	pub(crate) fn layer(&self) -> &wrapper::AnyLayer {
		self.layer.layer()
	}

	/// The definition to store in the parameter (what `checkout_param` returns).
	pub(crate) fn param_def(&self) -> PF_LayerDef {
		*self.cpu_world
	}

	pub(crate) fn size(&self) -> (i32, i32) {
		(self.layer.width() as i32, self.layer.height() as i32)
	}

	pub(crate) fn world_ptr(&mut self, gpu: bool) -> *mut PF_LayerDef {
		if gpu {
			&mut *self.gpu_world
		} else {
			&mut *self.cpu_world
		}
	}

	/// Byte length of the GPU world's device buffer.
	pub(crate) fn gpu_len(&self) -> usize {
		self.layer.width() as usize * self.layer.height() as usize * GPU_BYTES_PER_PIXEL
	}
}
