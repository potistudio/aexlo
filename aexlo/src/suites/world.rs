use after_effects_sys::{
	A_long, PF_Boolean, PF_EffectWorld, PF_Err, PF_Err_NONE, PF_PixelFormat, PF_PixelFormat_ARGB32,
	PF_ProgPtr, PF_WorldSuite2,
};

use crate::core::diagnostics::DiagnosticBuilder;

unsafe extern "C" fn dispose_world_stub(
	effect_ref: PF_ProgPtr,
	worldP: *mut PF_EffectWorld,
) -> PF_Err {
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

unsafe extern "C" fn new_world(
	effect_ref: PF_ProgPtr,
	widthL: A_long,
	heightL: A_long,
	clear_pixB: PF_Boolean,
	pixel_format: PF_PixelFormat,
	worldP: *mut PF_EffectWorld,
) -> PF_Err {
	if worldP.is_null() {
		log::warn!("new_world: worldP is null");
		return PF_Err_NONE as PF_Err;
	}

	DiagnosticBuilder::new()
		.set_name("PF_WorldSuite2/PF_NewWorld")
		.add_arg("effect_ref", format!("{:#x}", effect_ref as usize))
		.add_arg("widthL", widthL)
		.add_arg("heightL", heightL)
		.add_arg("clear_pixB", clear_pixB)
		.add_arg("pixel_format", pixel_format)
		.add_arg("worldP (out)", worldP as usize)
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
		PF_NewWorld: Some(new_world),
		PF_DisposeWorld: Some(dispose_world_stub),
		PF_GetPixelFormat: Some(get_pixel_format_stub),
	})
}
