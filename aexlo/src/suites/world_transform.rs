use crate::diagnostics::*;
use after_effects_sys::*;

/// Emulates `PF_WorldTransformSuite1::copy` function
/// # Safety
/// This function is unsafe because it handles raw pointers.
pub unsafe extern "C" fn Copy_sys(
	effect_ref: PF_ProgPtr,
	src: *mut PF_EffectWorld,
	dst: *mut PF_EffectWorld,
	src_r: *mut PF_Rect,
	dst_r: *mut PF_Rect,
) -> PF_Err {
	#[cfg(feature = "diagnostics")]
	DiagnosticBuilder::new()
		.set_name("PF World Transform Suite/Copy")
		.add_arg("effect_ref", effect_ref as usize)
		.add_arg("src", src as usize)
		.add_arg("dst", dst as usize)
		.add_arg(
			"src_r",
			if !src_r.is_null() {
				format!("{:?}", src_r)
			} else {
				"(null)".to_string()
			},
		)
		.add_arg(
			"dst_r",
			if !dst_r.is_null() {
				format!("{:?}", dst_r)
			} else {
				"(null)".to_string()
			},
		)
		.set_result(0)
		.emit();

	PF_Err_NONE as PF_Err
}
