use after_effects_sys::PFAppSuite6;

/// Build the "PF AE App Suite" v6 vtable.
///
/// All callbacks default to `None`: the emulator provides no host-level
/// application services (color picker, progress dialog, cursor, …).
/// Plugins that only acquire the suite without invoking it still load correctly.
///
/// `const` so it can initialize the shared [`SUITE_CONTAINER`](crate::suites::SUITE_CONTAINER)
/// static; the suite is a stateless (here, entirely stubbed-out) vtable.
pub(super) const fn create_ae_app_suite_v6() -> PFAppSuite6 {
	// SAFETY: `PFAppSuite6` is a `#[repr(C)]` struct of `Option<extern "C" fn>`
	// fields; an all-zero bit pattern is the valid `None` value for every field.
	unsafe { std::mem::zeroed::<PFAppSuite6>() }
}
