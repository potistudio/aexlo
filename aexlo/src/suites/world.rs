/// Implements the After Effects World Suite 2 callback functions for memory-managed pixel buffers.
///
/// This module provides FFI-compatible implementations of the PF_WorldSuite2 interface, allowing
/// the plugin to allocate, manage, and query pixel data buffers in various formats supported by
/// After Effects.
///
/// # Safety
///
/// All functions are marked `unsafe extern "C"` as they interface with After Effects' C API
/// and handle raw pointers to allocated memory. Callers must ensure:
/// - Valid effect_ref pointers are passed from the After Effects engine
/// - Output pointers (worldP, pixel_formatP) are properly initialized and aligned
/// - Memory allocated by `new_world_sys` is properly freed via `dispose_world_stub`
///
/// # Supported Pixel Formats
///
/// The implementation supports the following pixel formats with their corresponding bit depths:
/// - ARGB32 / BGRA32 / FORCE_LONG_INT: 4 bytes per pixel (32-bit)
/// - ARGB64: 8 bytes per pixel (64-bit)
/// - ARGB128 / GPU_BGRA128: 16 bytes per pixel (128-bit)
///
/// # Diagnostics
///
/// All functions emit diagnostic information via `DiagnosticBuilder` for debugging and
/// monitoring callback invocations from the After Effects engine.
///
/// # Functions
///
/// - `new_world_sys`: Allocates a new pixel buffer with specified dimensions and format
/// - `dispose_world_stub`: Releases previously allocated pixel buffer memory
/// - `get_pixel_format_stub`: Queries the pixel format of a given world buffer
/// - `create_world_suite_2`: Factory function that constructs the suite vtable
use std::ptr::null_mut;

use after_effects::sys::{PF_PixelFormat_ARGB64, PF_PixelFormat_ARGB128, PF_PixelFormat_GPU_BGRA128};
use after_effects_sys::{
	A_long, PF_Boolean, PF_EffectWorld, PF_Err, PF_Err_BAD_CALLBACK_PARAM, PF_Err_NONE, PF_PixelFormat,
	PF_PixelFormat_ARGB32, PF_ProgPtr, PF_RationalScale, PF_UnionableRect, PF_WorldFlag_WRITEABLE, PF_WorldFlags,
	PF_WorldSuite2,
};

use crate::core::diagnostics::DiagnosticBuilder;

unsafe extern "C" fn dispose_world_stub(effect_ref: PF_ProgPtr, worldP: *mut PF_EffectWorld) -> PF_Err {
	if worldP.is_null() {
		log::warn!("dispose_world: worldP is null");
		return PF_Err_NONE as PF_Err;
	}

	// Reclaim the pixel buffer leaked by `new_world_sys`. Its byte length is exactly
	// `rowbytes * height`, matching the `vec![0u8; ..]` we forgot there.
	let world = unsafe { &mut *worldP };
	if !world.data.is_null() {
		let size = world.rowbytes.max(0) as usize * world.height.max(0) as usize;
		if size > 0 {
			drop(unsafe { Vec::from_raw_parts(world.data as *mut u8, size, size) });
		}
		world.data = null_mut();
	}

	DiagnosticBuilder::new()
		.set_name("PF_WorldSuite2/PF_DisposeWorld")
		.add_arg("effect_ref", format!("{:#x}", effect_ref as usize))
		.add_arg("worldP (out)", worldP as usize)
		.emit();

	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn new_world_sys(
	effect_ref: PF_ProgPtr,
	widthL: A_long,
	heightL: A_long,
	clear_pixB: PF_Boolean,
	pixel_format: PF_PixelFormat,
	worldP: *mut PF_EffectWorld,
) -> PF_Err {
	//== Validation ==//
	if effect_ref.is_null() {
		log::error!("new_world: effect_ref is null");
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	if worldP.is_null() {
		log::error!("new_world: worldP is null");
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	//== Note ==//
	/*
	ARGB32: flag: None, depth: 4,
	ARGB64: flag: RESERVED0, depth: 8,
	ARGB128: flag: RESERVED1, depth: 16,
	GPU_BGRA128: flag: RESERVED1, depth: 16,
	Reserved: flag: RESERVED0, depth: 8,
	BGRA32: flag: RESERVED0, depth: 8, <- ?!
	VUYA32: flag: RESERVED0, depth: 8, <- ?!
	NTSCDV25: flag: RESERVED0, depth: 8,
	PALDV25: flag: RESERVED0, depth: 8,
	INVALID: flag: RESERVED0, depth: 8,
	FORCE_LONG_INT: flag: RESERVED0, depth: 8,
	*/

	//== Implementation ==//
	// Honor the caller's requested dimensions. Plugins allocate intermediate worlds
	// at sizes of their own choosing (e.g. downsampled or padded glow buffers), and
	// handing back a world sized to the output frame instead makes the plugin read or
	// write past the buffer it thinks it got -- a layout-dependent out-of-bounds crash.
	let width = widthL.max(0);
	let height = heightL.max(0);

	#[allow(non_upper_case_globals)]
	let depth = match pixel_format {
		PF_PixelFormat_ARGB32 => 4,
		PF_PixelFormat_ARGB64 => 8,
		PF_PixelFormat_ARGB128 => 16,
		PF_PixelFormat_GPU_BGRA128 => 16,
		_ => {
			log::warn!("Unsupported pixel format: {}. so the depth is set to 8", pixel_format);
			8 // Default to 8 bytes per pixel for unsupported formats
		}
	};

	#[cfg(target_os = "macos")]
	let depth = depth as u32;

	let new_world = PF_EffectWorld {
		reserved0: null_mut(),
		reserved1: null_mut(),
		world_flags: PF_WorldFlag_WRITEABLE as PF_WorldFlags,
		// `data` must point at the pixel bytes themselves. Leaking a `Box<Vec<u8>>`
		// and handing back its address (as the old code did) instead points the plugin
		// at the 24-byte `Vec` header, so any write past the first few pixels corrupts
		// the heap. Take the buffer's own data pointer and leak the allocation; it is
		// reclaimed in `dispose_world` from the world's own dimensions.
		data: {
			let mut buffer = vec![0u8; width as usize * height as usize * depth as usize];
			let data = buffer.as_mut_ptr();
			std::mem::forget(buffer);
			data as *mut _
		},
		rowbytes: width * depth as i32,
		width,
		height,
		extent_hint: PF_UnionableRect {
			left: 0,
			top: 0,
			right: width,
			bottom: height,
		},
		platform_ref: null_mut(),
		reserved_long1: 0,
		reserved_long4: null_mut(),
		pix_aspect_ratio: PF_RationalScale { den: 1, num: 1 },
		reserved_long2: null_mut(),
		origin_x: 0,
		origin_y: 0,
		reserved_long3: 0,
		dephault: 0,
	};

	unsafe { *worldP = new_world };

	//== Diagnostics ==//
	DiagnosticBuilder::new()
		.set_name("PF_WorldSuite2/PF_NewWorld")
		.add_arg("effect_ref", format!("{:#x}", effect_ref as usize))
		.add_arg("widthL", widthL)
		.add_arg("heightL", heightL)
		.add_arg("clear_pixB", clear_pixB)
		.add_arg("pixel_format", pixel_format)
		.add_arg("worldP (out)", format!("{:#x}", worldP as usize))
		.emit();

	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn get_pixel_format_stub(
	worldP: *const PF_EffectWorld,
	pixel_formatP: *mut PF_PixelFormat,
) -> PF_Err {
	if worldP.is_null() {
		log::warn!("PF_GetPixelFormat: worldP is null");
		return PF_Err_NONE as PF_Err;
	}

	if pixel_formatP.is_null() {
		log::warn!("PF_GetPixelFormat: pixel_formatP is null");
		return PF_Err_NONE as PF_Err;
	}

	// GPU-rendered worlds are 32-bit float BGRA (`PF_PixelFormat_GPU_BGRA128`); a
	// world registered as a GPU world (see `crate::gpu`) must report that so the
	// plugin takes its GPU path. Everything else is the CPU 8-bit `ARGB32` world.
	// This callback gets no `effect_ref`, so GPU-ness is looked up by world pointer.
	let format = if crate::gpu::is_gpu_world(worldP as usize) {
		PF_PixelFormat_GPU_BGRA128
	} else {
		PF_PixelFormat_ARGB32
	} as i32;
	unsafe { *pixel_formatP = format };

	DiagnosticBuilder::new()
		.set_name("PF_WorldSuite2/PF_GetPixelFormat")
		.add_arg("worldP", format!("{:#x}", worldP as usize))
		.add_arg("pixel_formatP (out)", pixel_formatP as usize)
		.emit();

	PF_Err_NONE as PF_Err
}

//==== Factory =============================================
/// Builds the `PF_WorldSuite2` vtable.
///
/// `const` so it can initialize the shared [`SUITE_CONTAINER`](crate::suites::SUITE_CONTAINER)
/// static; the suite is a stateless table of function pointers.
pub const fn create_world_suite_2() -> PF_WorldSuite2 {
	PF_WorldSuite2 {
		PF_NewWorld: Some(new_world_sys),
		PF_DisposeWorld: Some(dispose_world_stub),
		PF_GetPixelFormat: Some(get_pixel_format_stub),
	}
}
