//! GPU rendering support: Metal on macOS, CUDA on Windows/Linux.
//!
//! Plugins that declare `PF_OutFlag2_SUPPORTS_GPU_RENDER_F32` render into
//! 32-bit-float `PF_PixelFormat_GPU_BGRA128` worlds that live in GPU memory. To
//! drive such a plugin we expose a real device + command queue/stream through
//! [`PF_GPUDeviceSuite1`](crate::suites), back each input/output world with a
//! device buffer, and translate 8-bit CPU pixels to/from that float layout
//! around the `PF_Cmd_SMART_RENDER_GPU` call.
//!
//! The backend is chosen at compile time by swapping the `imp` module: Metal on
//! macOS, CUDA (via `cudarc`, dlopen'ing the NVIDIA driver at runtime) on
//! Windows and Linux, and an inert stub elsewhere. Runtime availability is
//! dynamic either way: [`GpuContext::new`] returns `None` when the machine has
//! no usable device, and callers fall back to CPU rendering.
//!
//! Unlike Metal's shared (host-visible) buffers, CUDA device memory cannot be
//! addressed from the CPU, so the context exposes explicit
//! [`write_buffer`](GpuContext::write_buffer) / [`read_buffer`](GpuContext::read_buffer)
//! copies instead of raw contents pointers.

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

	use after_effects_sys::{PF_GPU_Framework, PF_GPU_Framework_METAL};
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
		/// Buffers handed out via `AllocateDeviceMemory`, keyed by their object
		/// pointer so `FreeDeviceMemory` can release them.
		raw_allocs: HashMap<usize, Buffer>,
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
				raw_allocs: HashMap::new(),
			})
		}

		/// The GPU framework this backend drives plugins through
		/// (`PF_GPU_Framework_METAL`).
		pub fn framework(&self) -> PF_GPU_Framework {
			PF_GPU_Framework_METAL as PF_GPU_Framework
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

		/// Metal has no separate context object; `PF_GPUDeviceInfo::contextPV` stays null.
		pub fn context_ptr(&self) -> *mut c_void {
			std::ptr::null_mut()
		}

		/// No-op: Metal has no notion of a thread-current context.
		pub fn make_current(&self) {}

		/// Ensure a shared `MTLBuffer` of exactly `byte_len` bytes backs `world_ptr`,
		/// (re)allocating when the size changed, and mark the world as a GPU world.
		pub fn ensure_buffer(&mut self, world_ptr: usize, byte_len: usize) -> bool {
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
			true
		}

		/// Copy `data` into the buffer backing `world_ptr` (buffers use
		/// `StorageModeShared`, so `contents()` is a valid CPU mapping on both
		/// Apple Silicon and Intel).
		pub fn write_buffer(&mut self, world_ptr: usize, data: &[u8]) -> bool {
			let Some(buffer) = self.buffers.get(&world_ptr) else {
				return false;
			};
			if (buffer.length() as usize) < data.len() {
				return false;
			}
			unsafe {
				std::ptr::copy_nonoverlapping(data.as_ptr(), buffer.contents() as *mut u8, data.len());
			}
			true
		}

		/// Copy the buffer backing `world_ptr` out into `out`.
		pub fn read_buffer(&self, world_ptr: usize, out: &mut [u8]) -> bool {
			let Some(buffer) = self.buffers.get(&world_ptr) else {
				return false;
			};
			if (buffer.length() as usize) < out.len() {
				return false;
			}
			unsafe {
				std::ptr::copy_nonoverlapping(buffer.contents() as *const u8, out.as_mut_ptr(), out.len());
			}
			true
		}

		/// The `MTLBuffer` object pointer for `world_ptr`, as returned by
		/// `GetGPUWorldData` (the plugin `__bridge`-casts it to `id<MTLBuffer>`), or
		/// `None` if no buffer is registered for that world.
		pub fn buffer_object_ptr(&self, world_ptr: usize) -> Option<*mut c_void> {
			self.buffers.get(&world_ptr).map(|b| b.as_ptr() as *mut c_void)
		}

		/// Allocate a device buffer for `GPUDeviceSuite/AllocateDeviceMemory`,
		/// returning its `id<MTLBuffer>` object pointer (AE's Metal convention).
		pub fn alloc_raw(&mut self, byte_len: usize) -> Option<*mut c_void> {
			let buffer = self
				.device
				.new_buffer(byte_len as u64, MTLResourceOptions::StorageModeShared);
			let ptr = buffer.as_ptr() as *mut c_void;
			self.raw_allocs.insert(ptr as usize, buffer);
			Some(ptr)
		}

		/// Release a buffer previously returned by [`Self::alloc_raw`].
		pub fn free_raw(&mut self, ptr: *mut c_void) -> bool {
			self.raw_allocs.remove(&(ptr as usize)).is_some()
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

#[cfg(any(target_os = "windows", target_os = "linux"))]
mod imp {
	use std::collections::HashMap;
	use std::ffi::c_void;
	use std::sync::Arc;

	use after_effects_sys::{PF_GPU_Framework, PF_GPU_Framework_CUDA};
	use cudarc::driver::{CudaContext, CudaSlice, CudaStream, DevicePtr};

	use super::{register_gpu_world, unregister_gpu_world};

	/// Owns the CUDA primary context and stream for one plugin instance plus the
	/// device buffers that back its GPU worlds.
	///
	/// `ctx` retains the device *primary* context (`cuDevicePrimaryCtxRetain`), so
	/// CUDA *runtime* API calls made inside the plugin (`cudaMalloc`, kernel
	/// launches) operate in the same context as our allocations. The stream is
	/// handed to the plugin as `PF_GPUDeviceInfo::command_queuePV` (a
	/// `cudaStream_t`), matching AE's convention that plugin kernels launch on the
	/// host-provided stream.
	pub struct GpuContext {
		ctx: Arc<CudaContext>,
		stream: Arc<CudaStream>,
		buffers: HashMap<usize, CudaSlice<u8>>,
		/// Allocations handed out via `AllocateDeviceMemory`, keyed by their
		/// `CUdeviceptr` so `FreeDeviceMemory` can release them.
		raw_allocs: HashMap<usize, CudaSlice<u8>>,
	}

	impl GpuContext {
		/// Create a context on CUDA device 0, or `None` when no NVIDIA driver or
		/// device is available (so callers fall back to CPU rendering).
		pub fn new() -> Option<Self> {
			// cudarc panics (rather than erroring) when the driver library is
			// missing entirely; probe for it first so machines without an NVIDIA
			// driver degrade to the CPU path.
			if !unsafe { cudarc::driver::sys::is_culib_present() } {
				log::info!("CUDA driver library not found; GPU rendering unavailable");
				return None;
			}

			let ctx = match CudaContext::new(0) {
				Ok(ctx) => ctx,
				Err(err) => {
					log::info!("CUDA context creation failed ({err}); GPU rendering unavailable");
					return None;
				}
			};
			let stream = match ctx.new_stream() {
				Ok(stream) => stream,
				Err(err) => {
					log::info!("CUDA stream creation failed ({err}); GPU rendering unavailable");
					return None;
				}
			};

			Some(Self {
				ctx,
				stream,
				buffers: HashMap::new(),
				raw_allocs: HashMap::new(),
			})
		}

		/// The GPU framework this backend drives plugins through
		/// (`PF_GPU_Framework_CUDA`).
		pub fn framework(&self) -> PF_GPU_Framework {
			PF_GPU_Framework_CUDA as PF_GPU_Framework
		}

		/// The `CUdevice` ordinal, value-cast into `PF_GPUDeviceInfo::devicePV`
		/// (the SDK convention for CUDA; it is a small integer, not a pointer).
		pub fn device_ptr(&self) -> *mut c_void {
			self.ctx.cu_device() as usize as *mut c_void
		}

		/// The raw `CUstream` (== `cudaStream_t`), for
		/// `PF_GPUDeviceInfo::command_queuePV`.
		pub fn queue_ptr(&self) -> *mut c_void {
			self.stream.cu_stream() as *mut c_void
		}

		/// The raw `CUcontext`, for `PF_GPUDeviceInfo::contextPV`.
		pub fn context_ptr(&self) -> *mut c_void {
			self.ctx.cu_ctx() as *mut c_void
		}

		/// Bind the CUDA context to the calling thread (`cuCtxSetCurrent`).
		///
		/// Must run before handing control to the plugin: its runtime-API calls
		/// resolve against the thread-current context.
		pub fn make_current(&self) {
			if let Err(err) = self.ctx.bind_to_thread() {
				log::warn!("Failed to bind CUDA context to thread: {err}");
			}
		}

		/// Ensure a device buffer of exactly `byte_len` bytes backs `world_ptr`,
		/// (re)allocating when the size changed, and mark the world as a GPU world.
		pub fn ensure_buffer(&mut self, world_ptr: usize, byte_len: usize) -> bool {
			let needs_alloc = self
				.buffers
				.get(&world_ptr)
				.is_none_or(|b| b.len() != byte_len);

			if needs_alloc {
				match self.stream.alloc_zeros::<u8>(byte_len) {
					Ok(buffer) => {
						self.buffers.insert(world_ptr, buffer);
						// Allocation may be stream-ordered (cuMemAllocAsync); make the
						// pointer valid device-wide before the plugin can touch it from
						// another stream.
						let _ = self.stream.synchronize();
					}
					Err(err) => {
						log::error!("CUDA buffer allocation of {byte_len} bytes failed: {err}");
						return false;
					}
				}
			}

			register_gpu_world(world_ptr);
			true
		}

		/// Copy `data` into the device buffer backing `world_ptr` (`cuMemcpyHtoD`).
		pub fn write_buffer(&mut self, world_ptr: usize, data: &[u8]) -> bool {
			let Some(buffer) = self.buffers.get_mut(&world_ptr) else {
				return false;
			};
			if buffer.len() < data.len() {
				return false;
			}
			if let Err(err) = self.stream.memcpy_htod(data, buffer) {
				log::error!("CUDA host-to-device copy failed: {err}");
				return false;
			}
			// The copy is stream-ordered; flush it so the plugin sees the pixels no
			// matter which stream it launches on.
			if let Err(err) = self.stream.synchronize() {
				log::error!("CUDA stream synchronize failed after upload: {err}");
				return false;
			}
			true
		}

		/// Copy the device buffer backing `world_ptr` out into `out` (`cuMemcpyDtoH`).
		pub fn read_buffer(&self, world_ptr: usize, out: &mut [u8]) -> bool {
			let Some(buffer) = self.buffers.get(&world_ptr) else {
				return false;
			};
			if out.len() < buffer.len() {
				return false;
			}
			if let Err(err) = self.stream.memcpy_dtoh(buffer, out) {
				log::error!("CUDA device-to-host copy failed: {err}");
				return false;
			}
			// memcpy_dtoh is asynchronous; the host buffer is only valid after the
			// stream drains.
			if let Err(err) = self.stream.synchronize() {
				log::error!("CUDA stream synchronize failed after readback: {err}");
				return false;
			}
			true
		}

		/// The raw `CUdeviceptr` for `world_ptr`'s buffer, as returned by
		/// `GetGPUWorldData` (the plugin uses it directly as a device pointer), or
		/// `None` if no buffer is registered for that world.
		pub fn buffer_object_ptr(&self, world_ptr: usize) -> Option<*mut c_void> {
			self.buffers.get(&world_ptr).map(|b| {
				let (ptr, _sync) = b.device_ptr(&self.stream);
				ptr as usize as *mut c_void
			})
		}

		/// Allocate device memory for `GPUDeviceSuite/AllocateDeviceMemory`,
		/// returning the raw `CUdeviceptr` (AE's CUDA convention).
		pub fn alloc_raw(&mut self, byte_len: usize) -> Option<*mut c_void> {
			match self.stream.alloc_zeros::<u8>(byte_len) {
				Ok(buffer) => {
					let key = {
						let (ptr, _sync) = buffer.device_ptr(&self.stream);
						ptr as usize
					};
					// Stream-ordered allocation: settle it before the plugin uses the
					// pointer on a stream of its own.
					let _ = self.stream.synchronize();
					self.raw_allocs.insert(key, buffer);
					Some(key as *mut c_void)
				}
				Err(err) => {
					log::error!("CUDA AllocateDeviceMemory of {byte_len} bytes failed: {err}");
					None
				}
			}
		}

		/// Release memory previously returned by [`Self::alloc_raw`].
		pub fn free_raw(&mut self, ptr: *mut c_void) -> bool {
			self.raw_allocs.remove(&(ptr as usize)).is_some()
		}

		/// Block until all outstanding GPU work has finished.
		///
		/// Synchronizes our stream (where well-behaved plugins launch, via
		/// `command_queuePV`) and then the whole context, in case the plugin
		/// launched on the default stream or one of its own.
		pub fn wait_for_completion(&self) {
			if let Err(err) = self.stream.synchronize() {
				log::warn!("CUDA stream synchronize failed: {err}");
			}
			if let Err(err) = self.ctx.synchronize() {
				log::warn!("CUDA context synchronize failed: {err}");
			}
		}

		/// Drop the GPU-world registrations for every buffer this context owns.
		///
		/// Called when tearing down GPU rendering so stale world pointers stop being
		/// reported as GPU worlds. The device buffers themselves are retained for
		/// reuse until the context is dropped.
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

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
mod imp {
	use std::ffi::c_void;

	use after_effects_sys::{PF_GPU_Framework, PF_GPU_Framework_NONE};

	/// Fallback stub: no GPU backend exists for this platform, so a context can
	/// never be created and every accessor is inert. Keeps the crate compiling.
	pub struct GpuContext;

	impl GpuContext {
		pub fn new() -> Option<Self> {
			None
		}
		pub fn framework(&self) -> PF_GPU_Framework {
			PF_GPU_Framework_NONE as PF_GPU_Framework
		}
		pub fn device_ptr(&self) -> *mut c_void {
			std::ptr::null_mut()
		}
		pub fn queue_ptr(&self) -> *mut c_void {
			std::ptr::null_mut()
		}
		pub fn context_ptr(&self) -> *mut c_void {
			std::ptr::null_mut()
		}
		pub fn make_current(&self) {}
		pub fn ensure_buffer(&mut self, _world_ptr: usize, _byte_len: usize) -> bool {
			false
		}
		pub fn write_buffer(&mut self, _world_ptr: usize, _data: &[u8]) -> bool {
			false
		}
		pub fn read_buffer(&self, _world_ptr: usize, _out: &mut [u8]) -> bool {
			false
		}
		pub fn buffer_object_ptr(&self, _world_ptr: usize) -> Option<*mut c_void> {
			None
		}
		pub fn alloc_raw(&mut self, _byte_len: usize) -> Option<*mut c_void> {
			None
		}
		pub fn free_raw(&mut self, _ptr: *mut c_void) -> bool {
			false
		}
		pub fn unregister_all_worlds(&self) {}
		pub fn wait_for_completion(&self) {}
	}
}

pub use imp::GpuContext;
