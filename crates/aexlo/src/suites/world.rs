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
use after_effects_sys::{
	A_long, PF_Boolean, PF_EffectWorld, PF_Err, PF_Err_BAD_CALLBACK_PARAM, PF_Err_NONE, PF_PixelFormat,
	PF_PixelFormat_ARGB32, PF_PixelFormat_ARGB64, PF_PixelFormat_ARGB128, PF_PixelFormat_GPU_BGRA128, PF_ProgPtr,
	PF_RationalScale, PF_UnionableRect, PF_WorldFlag_WRITEABLE, PF_WorldFlags, PF_WorldSuite2,
};
use std::collections::HashMap;
use std::ptr::null_mut;
use std::sync::{LazyLock, Mutex};

use crate::core::diagnostics::diag;

/// Pixel format of every world allocated through this module, keyed by its
/// `data` pointer (stable however the plugin copies the `PF_EffectWorld`), so
/// `PF_GetPixelFormat` can report what the world was created as.
static WORLD_FORMATS: LazyLock<Mutex<HashMap<usize, u32>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// Worlds allocated with guard bands (strict mode), keyed by `data`.
static GUARDED: LazyLock<Mutex<HashMap<usize, crate::strict::Guarded>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// The format `world` was allocated with by [`alloc_world`], if any.
pub(super) fn allocated_format(world: &PF_EffectWorld) -> Option<u32> {
	WORLD_FORMATS.lock().ok()?.get(&(world.data as usize)).copied()
}

/// Record the pixel format of a host-owned world (the instance's input and
/// output, linked layers) by its `data` address: `ARGB64` for 16 bpc,
/// `ARGB128` for 32 bpc. 8 bpc needs no tag (`ARGB32` is the default).
#[allow(clippy::unnecessary_cast)] // `PF_PixelFormat_*` is `u32` on macOS only.
pub(crate) fn tag_host_world(data: usize, kind: wrapper::PixelDepthKind) {
	let format = match kind {
		wrapper::PixelDepthKind::U8 => return,
		wrapper::PixelDepthKind::U16 => PF_PixelFormat_ARGB64 as u32,
		wrapper::PixelDepthKind::F32 => PF_PixelFormat_ARGB128 as u32,
	};
	if let Ok(mut formats) = WORLD_FORMATS.lock() {
		formats.insert(data, format);
	}
}

/// Forget a tag set by [`tag_host_world`].
pub(crate) fn untag_host_world(data: usize) {
	if let Ok(mut formats) = WORLD_FORMATS.lock() {
		formats.remove(&data);
	}
}

/// Allocate a zeroed, writeable `width` x `height` world of `bytes_per_pixel`
/// pixels, remembering `format` for [`allocated_format`]. Release it with
/// [`free_world`].
pub(super) fn alloc_world(width: A_long, height: A_long, bytes_per_pixel: i32, format: u32) -> PF_EffectWorld {
	let width = width.max(0);
	let height = height.max(0);
	let row_data = width as usize * bytes_per_pixel.max(0) as usize;

	// Strict mode: a guard band around the image, verified on dispose.
	let guarded = crate::strict::guard_bands().then(|| {
		let mut guarded = crate::strict::Guarded::new(row_data, height as usize, bytes_per_pixel.max(1) as usize);
		let data = guarded.data_ptr();
		let rowbytes = guarded.rowbytes() as A_long;
		if let Ok(mut all) = GUARDED.lock() {
			all.insert(data as usize, guarded);
		}
		(data, rowbytes)
	});

	// `data` must point at the pixel bytes themselves. Leaking a `Box<Vec<u8>>`
	// and handing back its address instead points the plugin at the 24-byte
	// `Vec` header, so any write past the first few pixels corrupts the heap.
	// Take the buffer's own data pointer and leak the allocation; it is
	// reclaimed in `free_world` from the world's own dimensions.
	let (data, rowbytes) = guarded.unwrap_or_else(|| {
		let mut buffer = vec![0u8; row_data * height as usize];
		let data = buffer.as_mut_ptr();
		std::mem::forget(buffer);
		(data, width * bytes_per_pixel)
	});
	if let Ok(mut formats) = WORLD_FORMATS.lock() {
		formats.insert(data as usize, format);
	}
	crate::strict::allocated(crate::AllocationKind::World, data as usize, row_data * height as usize);

	PF_EffectWorld {
		reserved0: null_mut(),
		reserved1: null_mut(),
		world_flags: PF_WorldFlag_WRITEABLE as PF_WorldFlags,
		data: data as *mut _,
		rowbytes,
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
	}
}

/// Reclaim the pixel buffer of a world made by [`alloc_world`] and null its `data`.
///
/// # Safety
/// `world.data` must be null or come from [`alloc_world`] with the world's
/// current `rowbytes` and `height`, and not have been freed already.
pub(super) unsafe fn free_world(world: &mut PF_EffectWorld) {
	if world.data.is_null() {
		return;
	}
	if let Ok(mut formats) = WORLD_FORMATS.lock() {
		formats.remove(&(world.data as usize));
	}
	crate::strict::disposed(crate::AllocationKind::World, world.data as usize);
	let guarded = GUARDED
		.lock()
		.ok()
		.and_then(|mut all| all.remove(&(world.data as usize)));
	if let Some(guarded) = guarded {
		let broken = guarded.verify();
		if !broken.is_empty() {
			crate::strict::guards_broken(&format!("PF_NewWorld {}x{}", world.width, world.height), broken);
		}
		world.data = null_mut();
		return;
	}
	// Its byte length is exactly `rowbytes * height`, matching the
	// `vec![0u8; ..]` forgotten in `alloc_world`.
	let size = world.rowbytes.max(0) as usize * world.height.max(0) as usize;
	if size > 0 {
		drop(unsafe { Vec::from_raw_parts(world.data as *mut u8, size, size) });
	}
	world.data = null_mut();
}

pub(super) unsafe extern "C" fn dispose_world_sys(_effect_ref: PF_ProgPtr, worldP: *mut PF_EffectWorld) -> PF_Err {
	if worldP.is_null() {
		log::warn!("dispose_world: worldP is null");
		return PF_Err_NONE as PF_Err;
	}

	unsafe { free_world(&mut *worldP) };

	diag!("PF_WorldSuite2/PF_DisposeWorld",
		"effect_ref" => format!("{:#x}", _effect_ref as usize),
		"worldP (out)" => worldP as usize,
	);

	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn new_world_sys(
	effect_ref: PF_ProgPtr,
	widthL: A_long,
	heightL: A_long,
	_clear_pixB: PF_Boolean,
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

	// Compare as u32 on both sides: bindgen gives the PF_PixelFormat_* constants
	// a platform-dependent sign (i32 here on Windows), so a `match` on a cast
	// scrutinee would mismatch the pattern types. Normalizing both to u32 keeps
	// this signedness-agnostic.
	let fmt = pixel_format as u32;
	#[allow(clippy::unnecessary_cast)] // u32 only on some platforms.
	let depth = if fmt == PF_PixelFormat_ARGB32 as u32 {
		4
	} else if fmt == PF_PixelFormat_ARGB64 as u32 {
		8
	} else if fmt == PF_PixelFormat_ARGB128 as u32 || fmt == PF_PixelFormat_GPU_BGRA128 as u32 {
		16
	} else {
		log::warn!("Unsupported pixel format: {}. so the depth is set to 8", pixel_format);
		8 // Default to 8 bytes per pixel for unsupported formats
	};

	let new_world = alloc_world(widthL, heightL, depth, fmt);

	unsafe { *worldP = new_world };

	diag!("PF_WorldSuite2/PF_NewWorld",
		"effect_ref" => format!("{:#x}", effect_ref as usize),
		"widthL" => widthL,
		"heightL" => heightL,
		"clear_pixB" => _clear_pixB,
		"pixel_format" => pixel_format,
		"worldP (out)" => format!("{:#x}", worldP as usize),
	);

	PF_Err_NONE as PF_Err
}

/// The format of `worldP`: GPU-rendered worlds are 32-bit float BGRA
/// (`PF_PixelFormat_GPU_BGRA128`), which the plugin must see to take its GPU
/// path; worlds allocated here report what they were created as; everything
/// else is the CPU 8-bit `ARGB32` world. The world-format callbacks get no
/// `effect_ref`, so all of this is looked up by world.
///
/// # Safety
/// `worldP` must point to a valid `PF_EffectWorld`.
#[allow(clippy::unnecessary_cast)] // `PF_PixelFormat_*` is `u32` on macOS only.
pub(super) unsafe fn world_pixel_format(worldP: *const PF_EffectWorld) -> u32 {
	if crate::gpu::is_gpu_world(worldP as usize) {
		return PF_PixelFormat_GPU_BGRA128 as u32;
	}
	allocated_format(unsafe { &*worldP }).unwrap_or(PF_PixelFormat_ARGB32 as u32)
}

unsafe extern "C" fn get_pixel_format_sys(worldP: *const PF_EffectWorld, pixel_formatP: *mut PF_PixelFormat) -> PF_Err {
	if worldP.is_null() {
		log::warn!("PF_GetPixelFormat: worldP is null");
		return PF_Err_NONE as PF_Err;
	}

	if pixel_formatP.is_null() {
		log::warn!("PF_GetPixelFormat: pixel_formatP is null");
		return PF_Err_NONE as PF_Err;
	}

	unsafe { *pixel_formatP = world_pixel_format(worldP) as PF_PixelFormat };

	diag!("PF_WorldSuite2/PF_GetPixelFormat",
		"worldP" => format!("{:#x}", worldP as usize),
		"pixel_formatP (out)" => pixel_formatP as usize,
	);

	PF_Err_NONE as PF_Err
}

//==== Factory =============================================
/// Builds the `PF_WorldSuite2` vtable.
///
/// `const` so it can initialize the shared [`SUITE_CONTAINER`](crate::suites::SUITE_CONTAINER)
/// static; the suite is a stateless table of function pointers.
pub const fn create_world_suite() -> PF_WorldSuite2 {
	PF_WorldSuite2 {
		PF_NewWorld: Some(new_world_sys),
		PF_DisposeWorld: Some(dispose_world_sys),
		PF_GetPixelFormat: Some(get_pixel_format_sys),
	}
}
