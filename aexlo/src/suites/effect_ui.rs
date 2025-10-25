use crate::diagnostics::*;
use after_effects_sys::*;
use std::ffi::CStr;

pub(super) unsafe extern "C" fn SetOptionButtonName_sys(
	effect_ref: PF_ProgPtr,
	nameZ: *const A_char,
) -> PF_Err {
	#[cfg(feature = "diagnostics")]
	DiagnosticBuilder::new()
		.set_name("EffectUISuite/SetOptionButtonName")
		.add_arg("effect_ref", effect_ref as usize)
		.add_arg("nameZ", format!("{:?}", unsafe { CStr::from_ptr(nameZ) }))
		.set_result(0)
		.emit();

	PF_Err_NONE as PF_Err
}
