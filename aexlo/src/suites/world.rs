use std::ptr::null_mut;

use after_effects::sys::{
	PF_Pixel, PF_PixelFormat_ARGB64, PF_PixelFormat_ARGB128, PF_PixelFormat_BGRA32, PF_PixelFormat_GPU_BGRA128,
};
use after_effects_sys::{
	A_long, PF_Boolean, PF_EffectWorld, PF_Err, PF_Err_BAD_CALLBACK_PARAM, PF_Err_NONE, PF_PixelFormat,
	PF_PixelFormat_ARGB32, PF_PixelFormat_FORCE_LONG_INT, PF_ProgPtr, PF_RationalScale, PF_UnionableRect,
	PF_WorldFlag_WRITEABLE, PF_WorldFlags, PF_WorldSuite2,
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
	let depth = match pixel_format {
		PF_PixelFormat_ARGB32 => 4,
		PF_PixelFormat_BGRA32 => 4,
		PF_PixelFormat_ARGB64 => 8,
		PF_PixelFormat_ARGB128 => 16,
		PF_PixelFormat_GPU_BGRA128 => 16,
		PF_PixelFormat_FORCE_LONG_INT => 4,
		_ => {
			log::error!("Unsupported pixel format: {}", pixel_format);
			return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
		}
	};

	let new_world = PF_EffectWorld {
		reserved0: null_mut(),
		reserved1: null_mut(),
		world_flags: PF_WorldFlag_WRITEABLE as PF_WorldFlags,
		data: null_mut(),
		rowbytes: width as i32 * depth,
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

	unsafe { *pixel_formatP = PF_PixelFormat_ARGB32 };

	DiagnosticBuilder::new()
		.set_name("PF_WorldSuite2/PF_GetPixelFormat")
		.add_arg("worldP", format!("{:#x}", worldP as usize))
		.add_arg("pixel_formatP (out)", pixel_formatP as usize)
		.emit();

	PF_Err_NONE as PF_Err
}

//=== Factory ==============================================
/// Creates a PF_WorldSuite2 and returns a boxed pointer to it.
pub fn create_world_suite_2() -> Box<PF_WorldSuite2> {
	Box::new(PF_WorldSuite2 {
		PF_NewWorld: Some(new_world_sys),
		PF_DisposeWorld: Some(dispose_world_stub),
		PF_GetPixelFormat: Some(get_pixel_format_stub),
	})
}
