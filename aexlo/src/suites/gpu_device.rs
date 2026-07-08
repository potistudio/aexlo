//! `PF_GPUDeviceSuite1` — exposes aexlo's Metal device to GPU-capable plugins.
//!
//! Only the entry points a smart-GPU effect actually needs during render are
//! implemented against real Metal resources:
//!
//! * [`get_device_count`] — one device (the system default).
//! * [`get_device_info`] — hands back the `MTLDevice`/`MTLCommandQueue` pointers.
//! * [`get_gpu_world_data`] — maps a checked-out world to its backing `MTLBuffer`.
//!
//! The remaining memory-/world-management calls are stubs: the effects we target
//! allocate their own Metal buffers straight off the device we hand them, so they
//! never route through the suite. Each stub fails loudly rather than returning
//! uninitialised out-parameters, so an unexpected caller degrades gracefully.

use std::os::raw::c_void;

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

/// Report the number of GPU devices: always one (the system default Metal device).
unsafe extern "C" fn get_device_count(effect_ref: PF_ProgPtr, device_countP: *mut A_u_long) -> PF_Err {
	let _ = gpu_context_or_bail!(effect_ref, "GetDeviceCount");
	if device_countP.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}
	unsafe { *device_countP = 1 };
	PF_Err_NONE as PF_Err
}

/// Fill `device_infoP` with our Metal device and command-queue pointers so the
/// plugin can build its own pipelines and command buffers against them.
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
		device_framework: PF_GPU_Framework_METAL as PF_GPU_Framework,
		compatibleB: 1,
		platformPV: std::ptr::null_mut(),
		devicePV: ctx.device_ptr(),
		contextPV: std::ptr::null_mut(),
		command_queuePV: ctx.queue_ptr(),
		offscreen_opengl_contextPV: std::ptr::null_mut(),
		offscreen_opengl_devicePV: std::ptr::null_mut(),
	};
	unsafe { *device_infoP = info };
	PF_Err_NONE as PF_Err
}

/// Hand back the `MTLBuffer` object backing `worldP`.
///
/// The plugin `__bridge`-casts the returned pointer to `id<MTLBuffer>` and reads
/// or writes it as `device float4*` (BGRA128). aexlo registers the input/output
/// world buffers before dispatching `PF_Cmd_SMART_RENDER_GPU`, so the lookup
/// always resolves during render.
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
			log::error!("GetGPUWorldData: no MTLBuffer registered for world {:#x}", worldP as usize);
			PF_Err_BAD_CALLBACK_PARAM as PF_Err
		}
	}
}

/// Exclusive device access is a no-op: aexlo drives one plugin on one thread, so
/// there is never contention over the device.
unsafe extern "C" fn acquire_exclusive_device_access(_effect_ref: PF_ProgPtr, _device_index: A_u_long) -> PF_Err {
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn release_exclusive_device_access(_effect_ref: PF_ProgPtr, _device_index: A_u_long) -> PF_Err {
	PF_Err_NONE as PF_Err
}

/// Fallback for the suite's memory-/world-management calls that aexlo does not
/// service (the targeted Metal effects allocate straight off the device).
macro_rules! unsupported_stub {
	($name:ident ( $($arg:ident : $ty:ty),* $(,)? ) $who:literal) => {
		unsafe extern "C" fn $name($($arg : $ty),*) -> PF_Err {
			$( let _ = $arg; )*
			log::warn!(concat!("STUB: ", $who, " is not implemented"));
			PF_Err_OUT_OF_MEMORY as PF_Err
		}
	};
}

unsupported_stub!(allocate_device_memory(_e: PF_ProgPtr, _i: A_u_long, _s: usize, _m: *mut *mut c_void) "GPUDeviceSuite/AllocateDeviceMemory");
unsupported_stub!(free_device_memory(_e: PF_ProgPtr, _i: A_u_long, _m: *mut c_void) "GPUDeviceSuite/FreeDeviceMemory");
unsupported_stub!(purge_device_memory(_e: PF_ProgPtr, _i: A_u_long, _s: usize, _p: *mut usize) "GPUDeviceSuite/PurgeDeviceMemory");
unsupported_stub!(allocate_host_memory(_e: PF_ProgPtr, _i: A_u_long, _s: usize, _m: *mut *mut c_void) "GPUDeviceSuite/AllocateHostMemory");
unsupported_stub!(free_host_memory(_e: PF_ProgPtr, _i: A_u_long, _m: *mut c_void) "GPUDeviceSuite/FreeHostMemory");
unsupported_stub!(purge_host_memory(_e: PF_ProgPtr, _i: A_u_long, _b: usize, _p: *mut usize) "GPUDeviceSuite/PurgeHostMemory");
unsupported_stub!(create_gpu_world(_e: PF_ProgPtr, _i: A_u_long, _w: A_long, _h: A_long, _par: PF_RationalScale, _f: PF_Field, _pf: PF_PixelFormat, _c: PF_Boolean, _wp: *mut *mut PF_EffectWorld) "GPUDeviceSuite/CreateGPUWorld");
unsupported_stub!(dispose_gpu_world(_e: PF_ProgPtr, _w: *mut PF_EffectWorld) "GPUDeviceSuite/DisposeGPUWorld");
unsupported_stub!(get_gpu_world_size(_e: PF_ProgPtr, _w: *mut PF_EffectWorld, _s: *mut usize) "GPUDeviceSuite/GetGPUWorldSize");
unsupported_stub!(get_gpu_world_device_index(_e: PF_ProgPtr, _w: *mut PF_EffectWorld, _d: *mut A_u_long) "GPUDeviceSuite/GetGPUWorldDeviceIndex");

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
