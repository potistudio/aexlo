use after_effects_sys::PFAppSuite6;

/// Create the "PF AE App Suite" v6.
///
/// All callbacks default to `None`: the emulator provides no host-level
/// application services (color picker, progress dialog, cursor, …).
/// Plugins that only acquire the suite without invoking it still load correctly.
pub(super) fn create_ae_app_suite_v6() -> Box<PFAppSuite6> {
	// SAFETY: `PFAppSuite6` is a `#[repr(C)]` struct of `Option<extern "C" fn>`
	// fields; an all-zero bit pattern is the valid `None` value for every field.
	Box::new(unsafe { std::mem::zeroed::<PFAppSuite6>() })
}
