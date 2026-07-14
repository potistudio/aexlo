use after_effects_sys::{A_char, PF_Boolean, PF_Err, PF_Err_NONE, PFAppSuite6};

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

/// Build the "PF AE App Suite" v6 vtable.
///
/// Only the handful of callbacks that plugins actually invoke without a full host
/// are implemented ([`get_language`](PF_AppGetLanguage) and
/// [`is_render_engine`](PF_IsRenderEngine)); every other callback defaults to
/// `None`, since the emulator provides no host-level application services (color
/// picker, progress dialog, …). Plugins that only acquire the suite without
/// invoking those still load correctly.
///
/// ## `PF_SetCursor` slot doubles as `PF_IsRenderEngine`
/// `PF_AppGetLanguage` was inserted into the middle of `PFAppSuite6` in a later
/// SDK. Plugins built against the *earlier* header (DeepGlow2 among them) have no
/// `PF_AppGetLanguage` field, so every entry from there on is shifted up by one
/// slot: their `PF_IsRenderEngine` lands at the byte offset our headers call
/// `PF_SetCursor`. To satisfy both ABIs we install [`is_render_engine`] in *both*
/// the real `PF_IsRenderEngine` slot and the `PF_SetCursor` slot. This means a
/// modern plugin that genuinely calls `PF_SetCursor(PF_CursorType)` would pass an
/// integer where we expect a pointer -- acceptable here because headless
/// benchmark rendering never changes the cursor, and never crashes on a null slot.
///
/// `const` so it can initialize the shared [`SUITE_CONTAINER`](crate::suites::SUITE_CONTAINER)
/// static; the suite is a stateless vtable.
pub(super) const fn create_ae_app_suite_v6() -> PFAppSuite6 {
	// SAFETY: `PFAppSuite6` is a `#[repr(C)]` struct of `Option<extern "C" fn>`
	// fields; an all-zero bit pattern is the valid `None` value for every field.
	let mut suite = unsafe { std::mem::zeroed::<PFAppSuite6>() };
	suite.PF_AppGetLanguage = Some(get_language);
	suite.PF_IsRenderEngine = Some(is_render_engine);
	// See the doc comment above: cover the shifted-ABI offset used by plugins
	// compiled without `PF_AppGetLanguage`. The `PF_SetCursor` slot expects
	// `fn(PF_CursorType) -> PF_Err`; transmute our pointer-taking function into
	// that shape so the raw byte offset holds a valid, non-null callback.
	// SAFETY: both are `extern "C"` fn pointers of identical size/ABI; the only
	// mismatch is argument interpretation, handled as described above.
	suite.PF_SetCursor = Some(unsafe {
		std::mem::transmute::<
			unsafe extern "C" fn(*mut PF_Boolean) -> PF_Err,
			unsafe extern "C" fn(after_effects_sys::PF_CursorType) -> PF_Err,
		>(is_render_engine)
	});
	suite
}
