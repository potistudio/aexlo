use crate::diagnostics::*;
use after_effects_sys::*;

pub(super) unsafe extern "C" fn HostNewHandle_sys(size: A_HandleSize) -> A_Handle {
	#[cfg(feature = "diagnostics")]
	DiagnosticBuilder::new()
		.set_name("PF Handle Suite/HostNewHandle")
		.add_arg("size", size)
		.emit();

	let handle: Vec<*mut i8> = Vec::new();
	handle.as_ptr() as A_Handle
}
