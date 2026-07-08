//! Metal-backed GPU rendering support.
//!
//! Plugins that declare `PF_OutFlag2_SUPPORTS_GPU_RENDER_F32` render into
//! 32-bit-float `PF_PixelFormat_GPU_BGRA128` worlds that live in GPU memory. To
//! drive such a plugin we expose a real Metal device + command queue through
//! [`PF_GPUDeviceSuite1`](crate::suites), back each input/output world with an
//! `MTLBuffer`, and translate 8-bit CPU pixels to/from that float layout around
//! the `PF_Cmd_SMART_RENDER_GPU` call.
//!
//! Everything here is macOS-only (Metal is an Apple framework); the module
//! compiles to inert stubs elsewhere so the rest of the crate is
//! platform-agnostic.

use std::collections::HashSet;
use std::sync::{LazyLock, Mutex};

/// Bytes per pixel for AE's F32 GPU worlds (`PF_PixelFormat_GPU_BGRA128`: 4 × f32).
pub const GPU_BYTES_PER_PIXEL: usize = 16;

/// Process-global set of `PF_EffectWorld*` values (as `usize`) currently backed by
/// a GPU float buffer.
///
/// `PF_WorldSuite2::PF_GetPixelFormat` receives only a world pointer -- no
/// `effect_ref` -- so it cannot recover the owning [`crate::PluginInstance`]. It
/// consults this set to decide whether to report the GPU float format
/// (`PF_PixelFormat_GPU_BGRA128`) or the CPU `ARGB32` format for a given world.
static GPU_WORLD_PTRS: LazyLock<Mutex<HashSet<usize>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

/// Whether `world_ptr` is currently a GPU (float) world.
pub fn is_gpu_world(world_ptr: usize) -> bool {
	GPU_WORLD_PTRS.lock().map(|set| set.contains(&world_ptr)).unwrap_or(false)
}

fn register_gpu_world(world_ptr: usize) {
	if let Ok(mut set) = GPU_WORLD_PTRS.lock() {
		set.insert(world_ptr);
	}
}

fn unregister_gpu_world(world_ptr: usize) {
	if let Ok(mut set) = GPU_WORLD_PTRS.lock() {
		set.remove(&world_ptr);
	}
}

#[cfg(target_os = "macos")]
mod imp {
	use std::collections::HashMap;
	use std::ffi::c_void;

	use foreign_types::ForeignType;
	use metal::{Buffer, CommandQueue, Device, MTLResourceOptions};

	use super::{register_gpu_world, unregister_gpu_world};

	/// Owns the Metal device/queue for one plugin instance plus the `MTLBuffer`s
	/// that back its GPU worlds.
	///
	/// The buffers are keyed by the `PF_EffectWorld*` (as `usize`) they stand in
	/// for, so `GetGPUWorldData` can map a world the plugin was handed back to the
	/// buffer object it should render into.
	pub struct GpuContext {
		device: Device,
		queue: CommandQueue,
		buffers: HashMap<usize, Buffer>,
	}

	impl GpuContext {
		/// Create a context on the system default Metal device, or `None` if the
		/// machine has no usable Metal device.
		pub fn new() -> Option<Self> {
			let device = Device::system_default()?;
			let queue = device.new_command_queue();
			Some(Self {
				device,
				queue,
				buffers: HashMap::new(),
			})
		}

		/// Raw `id<MTLDevice>` pointer, as handed to the plugin via
		/// `PF_GPUDeviceInfo::devicePV` (it `__bridge`-casts it back to `id<MTLDevice>`).
		pub fn device_ptr(&self) -> *mut c_void {
			self.device.as_ptr() as *mut c_void
		}

		/// Raw `id<MTLCommandQueue>` pointer, for `PF_GPUDeviceInfo::command_queuePV`.
		pub fn queue_ptr(&self) -> *mut c_void {
			self.queue.as_ptr() as *mut c_void
		}

		/// Ensure a shared `MTLBuffer` of exactly `byte_len` bytes backs `world_ptr`,
		/// (re)allocating when the size changed, and mark the world as a GPU world.
		///
		/// Returns the buffer's CPU-visible contents pointer so the caller can upload
		/// or read back pixels (buffers use `StorageModeShared`, so `contents()` is a
		/// valid CPU mapping on both Apple Silicon and Intel).
		pub fn ensure_buffer(&mut self, world_ptr: usize, byte_len: usize) -> *mut c_void {
			let needs_alloc = self
				.buffers
				.get(&world_ptr)
				.is_none_or(|b| b.length() as usize != byte_len);

			if needs_alloc {
				let buffer = self
					.device
					.new_buffer(byte_len as u64, MTLResourceOptions::StorageModeShared);
				self.buffers.insert(world_ptr, buffer);
			}

			register_gpu_world(world_ptr);
			self.buffers[&world_ptr].contents()
		}

		/// The `MTLBuffer` object pointer for `world_ptr`, as returned by
		/// `GetGPUWorldData` (the plugin `__bridge`-casts it to `id<MTLBuffer>`), or
		/// `None` if no buffer is registered for that world.
		pub fn buffer_object_ptr(&self, world_ptr: usize) -> Option<*mut c_void> {
			self.buffers.get(&world_ptr).map(|b| b.as_ptr() as *mut c_void)
		}

		/// The CPU-visible contents pointer for `world_ptr`'s buffer, for reading a
		/// rendered result back.
		pub fn contents(&self, world_ptr: usize) -> Option<*mut c_void> {
			self.buffers.get(&world_ptr).map(|b| b.contents())
		}

		/// Block until every command buffer committed on our queue has finished.
		///
		/// The targeted plugins commit their render command buffer but deliberately
		/// do not wait (they expect the host to own queue synchronisation). Because a
		/// `MTLCommandQueue` executes committed buffers in order, committing one more
		/// empty buffer and waiting on it flushes all prior GPU work — so the output
		/// buffer is safe to read back afterwards.
		pub fn wait_for_completion(&self) {
			let command_buffer = self.queue.new_command_buffer();
			command_buffer.commit();
			command_buffer.wait_until_completed();
		}

		/// Drop the GPU-world registrations for every buffer this context owns.
		///
		/// Called when tearing down GPU rendering so stale world pointers stop being
		/// reported as GPU worlds. The `MTLBuffer`s themselves are retained for reuse
		/// until the context is dropped.
		pub fn unregister_all_worlds(&self) {
			for world_ptr in self.buffers.keys() {
				unregister_gpu_world(*world_ptr);
			}
		}
	}

	impl Drop for GpuContext {
		fn drop(&mut self) {
			self.unregister_all_worlds();
		}
	}
}

#[cfg(not(target_os = "macos"))]
mod imp {
	use std::ffi::c_void;

	/// Non-macOS stub: Metal is unavailable, so a context can never be created and
	/// every accessor is inert. Keeps the crate compiling on other platforms.
	pub struct GpuContext;

	impl GpuContext {
		pub fn new() -> Option<Self> {
			None
		}
		pub fn device_ptr(&self) -> *mut c_void {
			std::ptr::null_mut()
		}
		pub fn queue_ptr(&self) -> *mut c_void {
			std::ptr::null_mut()
		}
		pub fn ensure_buffer(&mut self, _world_ptr: usize, _byte_len: usize) -> *mut c_void {
			std::ptr::null_mut()
		}
		pub fn buffer_object_ptr(&self, _world_ptr: usize) -> Option<*mut c_void> {
			None
		}
		pub fn contents(&self, _world_ptr: usize) -> Option<*mut c_void> {
			None
		}
		pub fn unregister_all_worlds(&self) {}
		pub fn wait_for_completion(&self) {}
	}
}

pub use imp::GpuContext;
