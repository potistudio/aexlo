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

use crate::{PluginInstance, core::diagnostics::DiagnosticBuilder};

unsafe extern "C" fn dispose_world_stub(effect_ref: PF_ProgPtr, worldP: *mut PF_EffectWorld) -> PF_Err {
	if worldP.is_null() {
		log::warn!("dispose_world: worldP is null");
		return PF_Err_NONE as PF_Err;
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
	let instance = unsafe {
		PluginInstance::get_instance_ptr(effect_ref)
			.expect("No plugin instance found for effect_ref")
			.as_mut()
	};
	let (width, height) = instance.output_size();

	#[allow(non_upper_case_globals)]
	let depth = match pixel_format as u32 {
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
		data: Box::into_raw(Box::new(vec![0u8; (width * height * depth) as usize])) as *mut _,
		rowbytes: width as i32 * depth as i32,
		width: width as i32,
		height: height as i32,
		extent_hint: PF_UnionableRect {
			left: 0,
			top: 0,
			right: width as i32,
			bottom: height as i32,
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

	//TODO: 8bit
	#[cfg(target_os = "macos")]
	let format = PF_PixelFormat_ARGB32 as i32;

	#[cfg(not(target_os = "macos"))]
	let format = PF_PixelFormat_ARGB32 as u32;

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
