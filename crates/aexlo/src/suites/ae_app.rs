use after_effects_sys::{A_char, PF_Boolean, PF_Err, PF_Err_NONE, PFAppSuite4, PFAppSuite5, PFAppSuite6};

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

/// Report that we are *not* a render engine.
///
/// `PF_IsRenderEngine` returns TRUE when the host is the command-line renderer
/// (`aerender`), a UI-less/watch-folder instance, etc. We are an interactive-style
/// host, so we always answer FALSE. Some licensing libraries (e.g. the aescripts
/// framework used by DeepGlow2) call this during `PF_Cmd_SEQUENCE_SETUP` to decide
/// whether a "render-only" license applies; if the out-boolean is left
/// uninitialized they may take the wrong branch.
///
/// # Safety
/// `render_enginePB` must be null or point to a writable `PF_Boolean`, per the
/// `PF_IsRenderEngine` contract.
unsafe extern "C" fn is_render_engine(render_enginePB: *mut PF_Boolean) -> PF_Err {
	if !render_enginePB.is_null() {
		unsafe { *render_enginePB = 0 };
	}

	PF_Err_NONE as PF_Err
}

// The "PF AE App Suite" tables below.
//
// Only the callbacks plugins actually invoke without a full host are implemented
// ([`get_language`] and [`is_render_engine`]); every other slot is `None`, since
// the emulator provides no host-level application services (color picker,
// progress dialog, ...).
//
// The three layouts are NOT append-only: `PFAppSuite5` inserts
// `PF_AppGetLanguage` as its third entry, shifting every later slot relative to
// `PFAppSuite4`. Each wire version therefore gets its own exactly-shaped table
// (see the dispatch in `rusty_acquire_suite`). Note the wire version numbers do
// not match the struct names (`kPFAppSuiteVersion4 = 6`, `5 = 7`, `6 = 1`).
//
// SAFETY (all three): each `PFAppSuiteN` is a `#[repr(C)]` struct of
// `Option<extern "C" fn>` fields; an all-zero bit pattern is `None` for every field.

/// Build the `PFAppSuite4` table (wire version `kPFAppSuiteVersion4`).
pub(super) const fn create_ae_app_suite_4() -> PFAppSuite4 {
	let mut suite = unsafe { std::mem::zeroed::<PFAppSuite4>() };
	suite.PF_IsRenderEngine = Some(is_render_engine);
	suite
}

/// Build the `PFAppSuite5` table (wire version `kPFAppSuiteVersion5`).
pub(super) const fn create_ae_app_suite_5() -> PFAppSuite5 {
	let mut suite = unsafe { std::mem::zeroed::<PFAppSuite5>() };
	suite.PF_AppGetLanguage = Some(get_language);
	suite.PF_IsRenderEngine = Some(is_render_engine);
	suite
}

/// Build the `PFAppSuite6` table (wire version `kPFAppSuiteVersion6`).
pub(super) const fn create_ae_app_suite_6() -> PFAppSuite6 {
	let mut suite = unsafe { std::mem::zeroed::<PFAppSuite6>() };
	suite.PF_AppGetLanguage = Some(get_language);
	suite.PF_IsRenderEngine = Some(is_render_engine);
	suite
}
