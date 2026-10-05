//! `PF_GPUDeviceSuite1` - exposes aexlo's GPU device (Metal or CUDA) to
//! GPU-capable plugins.
//!
//! The entry points a smart-GPU effect needs during render are implemented
//! against the active backend's real resources:
//!
//! * [`get_device_count`] - one device (the system default).
//! * [`get_device_info`] - hands back the device/queue/context pointers
//!   (`MTLDevice`/`MTLCommandQueue` on Metal; `CUdevice`/`cudaStream_t`/`CUcontext`
//!   on CUDA).
//! * [`get_gpu_world_data`] - maps a checked-out world to its backing buffer
//!   (an `MTLBuffer` object on Metal, a raw `CUdeviceptr` on CUDA).
//! * [`allocate_device_memory`] / [`free_device_memory`] - scratch allocations
//!   for effects that route intermediates through the suite.
//! * [`get_gpu_world_size`] / [`get_gpu_world_device_index`] - trivial queries.
//!
//! * [`allocate_host_memory`] / [`free_host_memory`] - aligned host staging memory.
//! * [`create_gpu_world`] / [`dispose_gpu_world`] - scratch GPU worlds backed by
//!   a device buffer, reachable through [`get_gpu_world_data`].
//! * The purge calls report nothing purged: aexlo keeps no memory caches.

use std::alloc::Layout;
use std::collections::{HashMap, HashSet};
use std::os::raw::c_void;
use std::sync::{LazyLock, Mutex};

use after_effects_sys::*;

use crate::PluginInstance;

/// Recover the [`PluginInstance`] and its live GPU context from `effect_ref`,
/// returning `PF_Err_BAD_CALLBACK_PARAM` when either is missing.
macro_rules! gpu_context_or_bail {
	($effect_ref:expr, $who:literal) => {{
		if $effect_ref.is_null() {
			log::error!(concat!($who, ": effect_ref is null"));
			return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
		}
		let instance = match PluginInstance::get_instance_ptr($effect_ref) {
			Some(ptr) => unsafe { ptr.as_ref() },
			None => {
				log::error!(concat!($who, ": no plugin instance for effect_ref"));
				return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
			}
		};
		match instance.gpu_context() {
			Some(ctx) => ctx,
			None => {
				log::error!(concat!($who, ": GPU context not initialised"));
				return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
			}
		}
	}};
}

/// Like [`gpu_context_or_bail!`], but yields a mutable GPU context for the
/// memory-allocation entry points.
macro_rules! gpu_context_mut_or_bail {
	($effect_ref:expr, $who:literal) => {{
		if $effect_ref.is_null() {
			log::error!(concat!($who, ": effect_ref is null"));
			return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
		}
		let instance = match PluginInstance::get_instance_ptr($effect_ref) {
			Some(mut ptr) => unsafe { ptr.as_mut() },
			None => {
				log::error!(concat!($who, ": no plugin instance for effect_ref"));
				return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
			}
		};
		match instance.gpu_context_mut() {
			Some(ctx) => ctx,
			None => {
				log::error!(concat!($who, ": GPU context not initialised"));
				return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
			}
		}
	}};
}

/// Report the number of GPU devices: always one (the system default device).
unsafe extern "C" fn get_device_count(effect_ref: PF_ProgPtr, device_countP: *mut A_u_long) -> PF_Err {
	let _ = gpu_context_or_bail!(effect_ref, "GetDeviceCount");
	if device_countP.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}
	unsafe { *device_countP = 1 };
	PF_Err_NONE as PF_Err
}

/// Fill `device_infoP` with the active backend's device pointers so the plugin
/// can build its own pipelines / launch kernels against them.
///
/// Per SDK convention: on Metal `devicePV`/`command_queuePV` are the
/// `MTLDevice`/`MTLCommandQueue` objects; on CUDA `devicePV` is the value-cast
/// `CUdevice`, `contextPV` the `CUcontext`, and `command_queuePV` the
/// `cudaStream_t` the plugin should launch on.
unsafe extern "C" fn get_device_info(
	effect_ref: PF_ProgPtr,
	_device_index: A_u_long,
	device_infoP: *mut PF_GPUDeviceInfo,
) -> PF_Err {
	let ctx = gpu_context_or_bail!(effect_ref, "GetDeviceInfo");
	if device_infoP.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	let info = PF_GPUDeviceInfo {
		device_framework: ctx.framework(),
		compatibleB: 1,
		platformPV: std::ptr::null_mut(),
		devicePV: ctx.device_ptr(),
		contextPV: ctx.context_ptr(),
		command_queuePV: ctx.queue_ptr(),
		offscreen_opengl_contextPV: std::ptr::null_mut(),
		offscreen_opengl_devicePV: std::ptr::null_mut(),
	};
	unsafe { *device_infoP = info };
	PF_Err_NONE as PF_Err
}

/// Hand back the buffer backing `worldP`: an `MTLBuffer` object pointer on
/// Metal (the plugin `__bridge`-casts it), a raw `CUdeviceptr` on CUDA (used
/// directly as a `float4*` device pointer).
///
/// aexlo registers the input/output world buffers before dispatching
/// `PF_Cmd_SMART_RENDER_GPU`, so the lookup always resolves during render.
unsafe extern "C" fn get_gpu_world_data(
	effect_ref: PF_ProgPtr,
	worldP: *mut PF_EffectWorld,
	pixPP: *mut *mut c_void,
) -> PF_Err {
	let ctx = gpu_context_or_bail!(effect_ref, "GetGPUWorldData");
	if worldP.is_null() || pixPP.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	match ctx.buffer_object_ptr(worldP as usize) {
		Some(buffer) => {
			unsafe { *pixPP = buffer };
			PF_Err_NONE as PF_Err
		}
		None => {
			log::error!(
				"GetGPUWorldData: no GPU buffer registered for world {:#x}",
				worldP as usize
			);
			PF_Err_BAD_CALLBACK_PARAM as PF_Err
		}
	}
}

/// Allocate `size` bytes of device memory for the plugin's intermediates,
/// returning the backend's native handle (`id<MTLBuffer>` on Metal, a raw
/// `CUdeviceptr` on CUDA). Released via [`free_device_memory`], or when the
/// GPU context is dropped.
unsafe extern "C" fn allocate_device_memory(
	effect_ref: PF_ProgPtr,
	_device_index: A_u_long,
	size: usize,
	memoryPP: *mut *mut c_void,
) -> PF_Err {
	let ctx = gpu_context_mut_or_bail!(effect_ref, "AllocateDeviceMemory");
	if memoryPP.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	match ctx.alloc_raw(size) {
		Some(memory) => {
			unsafe { *memoryPP = memory };
			PF_Err_NONE as PF_Err
		}
		None => PF_Err_OUT_OF_MEMORY as PF_Err,
	}
}

/// Release memory previously handed out by [`allocate_device_memory`].
unsafe extern "C" fn free_device_memory(
	effect_ref: PF_ProgPtr,
	_device_index: A_u_long,
	memoryP: *mut c_void,
) -> PF_Err {
	let ctx = gpu_context_mut_or_bail!(effect_ref, "FreeDeviceMemory");
	if ctx.free_raw(memoryP) {
		PF_Err_NONE as PF_Err
	} else {
		log::error!("FreeDeviceMemory: unknown allocation {:#x}", memoryP as usize);
		PF_Err_BAD_CALLBACK_PARAM as PF_Err
	}
}

/// Size in bytes of the buffer backing `worldP`. Our GPU worlds are tightly
/// packed (`rowbytes == width * 16`), so `height * rowbytes` is exact.
unsafe extern "C" fn get_gpu_world_size(
	_effect_ref: PF_ProgPtr,
	worldP: *mut PF_EffectWorld,
	size_in_bytesP: *mut usize,
) -> PF_Err {
	if worldP.is_null() || size_in_bytesP.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}
	let world = unsafe { &*worldP };
	unsafe { *size_in_bytesP = world.height as usize * world.rowbytes as usize };
	PF_Err_NONE as PF_Err
}

/// aexlo only ever exposes one GPU device, so every world lives on device 0.
unsafe extern "C" fn get_gpu_world_device_index(
	_effect_ref: PF_ProgPtr,
	worldP: *mut PF_EffectWorld,
	device_indexP: *mut A_u_long,
) -> PF_Err {
	if worldP.is_null() || device_indexP.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}
	unsafe { *device_indexP = 0 };
	PF_Err_NONE as PF_Err
}

/// Exclusive device access is a no-op: aexlo drives one plugin on one thread, so
/// there is never contention over the device.
unsafe extern "C" fn acquire_exclusive_device_access(_effect_ref: PF_ProgPtr, _device_index: A_u_long) -> PF_Err {
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn release_exclusive_device_access(_effect_ref: PF_ProgPtr, _device_index: A_u_long) -> PF_Err {
	PF_Err_NONE as PF_Err
}

/// aexlo keeps no device-memory cache, so there is never anything to purge.
unsafe extern "C" fn purge_device_memory(
	_effect_ref: PF_ProgPtr,
	_device_index: A_u_long,
	_size: usize,
	bytes_purgedP0: *mut usize,
) -> PF_Err {
	if let Some(out) = unsafe { bytes_purgedP0.as_mut() } {
		*out = 0;
	}
	PF_Err_NONE as PF_Err
}

/// Alignment of `AllocateHostMemory` blocks: a cache line, enough for any SIMD type.
const HOST_ALIGN: usize = 64;

/// Live `AllocateHostMemory` blocks and their sizes, so `FreeHostMemory` can
/// rebuild the layout (the plugin only hands back the pointer).
static HOST_ALLOCS: LazyLock<Mutex<HashMap<usize, usize>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// Allocate `size` bytes of host memory for staging GPU transfers. aexlo's
/// devices either share memory with the CPU (Metal) or copy through the
/// driver (CUDA), so ordinary aligned memory serves.
unsafe extern "C" fn allocate_host_memory(
	_effect_ref: PF_ProgPtr,
	_device_index: A_u_long,
	size: usize,
	memoryPP: *mut *mut c_void,
) -> PF_Err {
	let Some(out) = (unsafe { memoryPP.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	};
	let Ok(layout) = Layout::from_size_align(size.max(1), HOST_ALIGN) else {
		return PF_Err_OUT_OF_MEMORY as PF_Err;
	};
	let ptr = unsafe { std::alloc::alloc(layout) };
	if ptr.is_null() {
		return PF_Err_OUT_OF_MEMORY as PF_Err;
	}
	HOST_ALLOCS
		.lock()
		.unwrap_or_else(|e| e.into_inner())
		.insert(ptr as usize, layout.size());
	*out = ptr as *mut c_void;
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn free_host_memory(
	_effect_ref: PF_ProgPtr,
	_device_index: A_u_long,
	memoryP: *mut c_void,
) -> PF_Err {
	let size = HOST_ALLOCS
		.lock()
		.unwrap_or_else(|e| e.into_inner())
		.remove(&(memoryP as usize));
	let Some(size) = size else {
		log::error!("FreeHostMemory: unknown allocation {:#x}", memoryP as usize);
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	};
	// SAFETY: the block came from `allocate_host_memory` with this exact layout.
	unsafe { std::alloc::dealloc(memoryP as *mut u8, Layout::from_size_align_unchecked(size, HOST_ALIGN)) };
	PF_Err_NONE as PF_Err
}

/// No host-memory cache either; nothing to purge.
unsafe extern "C" fn purge_host_memory(
	_effect_ref: PF_ProgPtr,
	_device_index: A_u_long,
	_bytes_to_try: usize,
	bytes_purgedP0: *mut usize,
) -> PF_Err {
	if let Some(out) = unsafe { bytes_purgedP0.as_mut() } {
		*out = 0;
	}
	PF_Err_NONE as PF_Err
}

/// `PF_EffectWorld`s handed out by `CreateGPUWorld` (boxed, so the pointer is
/// stable), as opposed to the instance-owned input/output worlds that also
/// carry GPU buffers. Only these may be freed by `DisposeGPUWorld`.
static CREATED_GPU_WORLDS: LazyLock<Mutex<HashSet<usize>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

/// Create a `PF_PixelFormat_GPU_BGRA128` world backed by a device buffer the
/// plugin reaches through `GetGPUWorldData`. Its CPU `data` pointer is null,
/// as for AE's own GPU worlds.
#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn create_gpu_world(
	effect_ref: PF_ProgPtr,
	_device_index: A_u_long,
	width: A_long,
	height: A_long,
	pixel_aspect_ratio: PF_RationalScale,
	_field: PF_Field,
	pixel_format: PF_PixelFormat,
	clear_pixB: PF_Boolean,
	worldPP: *mut *mut PF_EffectWorld,
) -> PF_Err {
	let ctx = gpu_context_mut_or_bail!(effect_ref, "CreateGPUWorld");
	let Some(out) = (unsafe { worldPP.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	};
	#[allow(clippy::unnecessary_cast)] // `PF_PixelFormat_*` is `u32` on macOS only.
	if pixel_format as u32 != PF_PixelFormat_GPU_BGRA128 as u32 || width <= 0 || height <= 0 {
		log::error!("CreateGPUWorld: unsupported {width}x{height} world of format {pixel_format:#x}");
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	let rowbytes = width * crate::gpu::GPU_BYTES_PER_PIXEL as A_long;
	let byte_len = rowbytes as usize * height as usize;
	let mut world: PF_EffectWorld = unsafe { std::mem::zeroed() };
	world.world_flags = PF_WorldFlag_WRITEABLE as PF_WorldFlags;
	world.rowbytes = rowbytes;
	world.width = width;
	world.height = height;
	world.extent_hint = PF_UnionableRect {
		left: 0,
		top: 0,
		right: width,
		bottom: height,
	};
	world.pix_aspect_ratio = pixel_aspect_ratio;
	let world = Box::into_raw(Box::new(world));
	let key = world as usize;

	// Metal buffers start with undefined contents; CUDA ones are zeroed.
	if !ctx.ensure_buffer(key, byte_len) || (clear_pixB != 0 && !ctx.write_buffer(key, &vec![0; byte_len])) {
		ctx.release_buffer(key);
		drop(unsafe { Box::from_raw(world) });
		return PF_Err_OUT_OF_MEMORY as PF_Err;
	}
	CREATED_GPU_WORLDS.lock().unwrap_or_else(|e| e.into_inner()).insert(key);
	*out = world;
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn dispose_gpu_world(effect_ref: PF_ProgPtr, worldP: *mut PF_EffectWorld) -> PF_Err {
	let ctx = gpu_context_mut_or_bail!(effect_ref, "DisposeGPUWorld");
	if !CREATED_GPU_WORLDS
		.lock()
		.unwrap_or_else(|e| e.into_inner())
		.remove(&(worldP as usize))
	{
		log::error!(
			"DisposeGPUWorld: {:#x} was not created by CreateGPUWorld",
			worldP as usize
		);
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}
	ctx.release_buffer(worldP as usize);
	// SAFETY: boxed in `create_gpu_world` and removed from the live set above.
	drop(unsafe { Box::from_raw(worldP) });
	PF_Err_NONE as PF_Err
}

/// Build the `PF_GPUDeviceSuite1` vtable.
///
/// `const` so it can initialize the shared [`SUITE_CONTAINER`](crate::suites::SUITE_CONTAINER)
/// static; the suite is a stateless table of function pointers (all per-instance
/// state is recovered from `effect_ref`).
pub const fn create_gpu_device_suite_1() -> PF_GPUDeviceSuite1 {
	PF_GPUDeviceSuite1 {
		GetDeviceCount: Some(get_device_count),
		GetDeviceInfo: Some(get_device_info),
		AcquireExclusiveDeviceAccess: Some(acquire_exclusive_device_access),
		ReleaseExclusiveDeviceAccess: Some(release_exclusive_device_access),
		AllocateDeviceMemory: Some(allocate_device_memory),
		FreeDeviceMemory: Some(free_device_memory),
		PurgeDeviceMemory: Some(purge_device_memory),
		AllocateHostMemory: Some(allocate_host_memory),
		FreeHostMemory: Some(free_host_memory),
		PurgeHostMemory: Some(purge_host_memory),
		CreateGPUWorld: Some(create_gpu_world),
		DisposeGPUWorld: Some(dispose_gpu_world),
		GetGPUWorldData: Some(get_gpu_world_data),
		GetGPUWorldSize: Some(get_gpu_world_size),
		GetGPUWorldDeviceIndex: Some(get_gpu_world_device_index),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn host_memory_round_trips_and_rejects_foreign_pointers() {
		let suite = create_gpu_device_suite_1();
		let mut mem: *mut c_void = std::ptr::null_mut();
		unsafe {
			assert_eq!(
				suite.AllocateHostMemory.unwrap()(std::ptr::null_mut(), 0, 100, &mut mem),
				PF_Err_NONE as PF_Err
			);
			assert_eq!(mem as usize % HOST_ALIGN, 0);
			std::ptr::write_bytes(mem as *mut u8, 0xab, 100);
			assert_eq!(
				suite.FreeHostMemory.unwrap()(std::ptr::null_mut(), 0, mem),
				PF_Err_NONE as PF_Err
			);
			assert_ne!(
				suite.FreeHostMemory.unwrap()(std::ptr::null_mut(), 0, mem),
				PF_Err_NONE as PF_Err
			);

			let mut purged = 1;
			suite.PurgeHostMemory.unwrap()(std::ptr::null_mut(), 0, 1 << 20, &mut purged);
			assert_eq!(purged, 0);
		}
	}
}
