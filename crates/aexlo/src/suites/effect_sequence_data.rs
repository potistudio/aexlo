//! `PF Effect Sequence Data Suite` v1: the instance's sequence data, read-only.
//!
//! After Effects hands a Multi-Frame Rendering effect a null
//! `in_data->sequence_data` while it renders, and the effect reads its
//! sequence data through `PF_GetConstSequenceData` instead. aexlo always
//! passes the handle in `PF_InData` as well, so this returns the same handle:
//! whatever the plugin returned from `PF_Cmd_SEQUENCE_SETUP` (or
//! `SEQUENCE_RESETUP`), and null before that, as in `PF_Cmd_PARAMS_SETUP`.

use crate::PluginInstance;
use crate::core::diagnostics::diag;
use crate::sys::{
	PF_ConstHandle, PF_EffectSequenceDataSuite1, PF_Err, PF_Err_BAD_CALLBACK_PARAM, PF_Err_NONE, PF_ProgPtr,
};

unsafe extern "C" fn get_const_sequence_data(effect_ref: PF_ProgPtr, sequence_data: *mut PF_ConstHandle) -> PF_Err {
	crate::core::ffi::guard("get_const_sequence_data", || {
		diag!("PF_EffectSequenceDataSuite1/PF_GetConstSequenceData",
			"effect_ref" => format!("{:#x}", effect_ref as usize),
			"sequence_data" => format!("{:#x}", sequence_data as usize),
		);
		let Some(instance) = PluginInstance::get_instance_ptr(effect_ref) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		let Some(out) = (unsafe { sequence_data.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		// SAFETY: effect_ref is the instance running the plugin call.
		*out = unsafe { instance.as_ref() }.sequence_handle() as PF_ConstHandle;
		PF_Err_NONE
	})
}

pub(super) const fn create_effect_sequence_data_suite_1() -> PF_EffectSequenceDataSuite1 {
	PF_EffectSequenceDataSuite1 {
		PF_GetConstSequenceData: Some(get_const_sequence_data),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn refuses_a_null_effect_or_out_pointer() {
		let mut out: PF_ConstHandle = std::ptr::null();
		assert_eq!(
			unsafe { get_const_sequence_data(std::ptr::null_mut(), &mut out) },
			PF_Err_BAD_CALLBACK_PARAM
		);
	}
}
