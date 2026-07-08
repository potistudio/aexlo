use after_effects_sys::{A_char, PF_Err, PF_Err_NONE, PFAppSuite6};

/// Report the host UI language to the plugin.
///
/// We advertise "no specific language" by leaving the caller's (pre-zeroed)
/// buffer empty. The SDK's localization helper (`AELocalise::GetStringForAE`)
/// treats an empty tag as "use the base strings", so plugins fall back to their
/// built-in (English) resource strings.
///
/// This callback must exist even though it does almost nothing: plugins invoke it
/// through the suite vtable during `PF_Cmd_PARAMS_SETUP`, and a `None` (null) slot
/// there is a hard crash (`blr` through a null pointer), not a graceful no-op.
///
/// # Safety
/// `lang_tagZ` must be null or point to a writable `A_char` buffer of at least one
/// element, per the `PF_AppGetLanguage` contract.
unsafe extern "C" fn get_language(lang_tagZ: *mut A_char) -> PF_Err {
	if !lang_tagZ.is_null() {
		unsafe { *lang_tagZ = 0 };
	}
	PF_Err_NONE as PF_Err
}

/// Build the "PF AE App Suite" v6 vtable.
///
/// Only [`get_language`](PF_AppGetLanguage) is implemented; every other callback
/// defaults to `None`, since the emulator provides no host-level application
/// services (color picker, progress dialog, cursor, …). Plugins that only acquire
/// the suite without invoking those still load correctly.
///
/// `const` so it can initialize the shared [`SUITE_CONTAINER`](crate::suites::SUITE_CONTAINER)
/// static; the suite is a stateless vtable.
pub(super) const fn create_ae_app_suite_v6() -> PFAppSuite6 {
	// SAFETY: `PFAppSuite6` is a `#[repr(C)]` struct of `Option<extern "C" fn>`
	// fields; an all-zero bit pattern is the valid `None` value for every field.
	let mut suite = unsafe { std::mem::zeroed::<PFAppSuite6>() };
	suite.PF_AppGetLanguage = Some(get_language);
	suite
}
