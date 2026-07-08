mod ae_app;
mod angle_param;
pub mod ansi;
pub mod handle;
pub mod interface;
pub mod iterate;
pub mod macros;
pub mod registry;
pub mod transform;
pub mod ui;
pub mod utility;
pub mod world;

#[cfg(feature = "diagnostics")]
use crate::core::diagnostics::DiagnosticBuilder;
use crate::suites::registry::{acquire, release};
use after_effects_sys::*;
use std::ffi::CStr;
use std::os::raw::c_void;

pub static SUITE_CONTAINER: SuiteContainer = SuiteContainer {
	ansi: PF_ANSICallbacks {
		atan: Some(ansi::atan_sys),
		atan2: Some(ansi::atan2_sys),
		ceil: Some(ansi::ceil_sys),
		cos: Some(ansi::cos_sys),
		exp: Some(ansi::exp_sys),
		fabs: Some(ansi::fabs_sys),
		floor: Some(ansi::floor_sys),
		fmod: Some(ansi::fmod_sys),
		hypot: Some(ansi::hypot_sys),
		log: Some(ansi::log_sys),
		log10: Some(ansi::log10_sys),
		pow: Some(ansi::pow_sys),
		sin: Some(ansi::sin_sys),
		sqrt: Some(ansi::sqrt_sys),
		tan: Some(ansi::tan_sys),
		sprintf: Some(ansi::sprintf_sys),
		strcpy: Some(ansi::strcpy_sys),
		asin: Some(ansi::asin_sys),
		acos: Some(ansi::acos_sys),
		ansi_procs: [0; 1],
	},
	effect_ui: PF_EffectUISuite1 {
		PF_SetOptionsButtonName: Some(ui::SetOptionButtonName_sys),
	},
	handle: handle::create_handle_suite_1(),
	world_transform: transform::create_world_transform_suite_1(),
	world: world::create_world_suite_2(),
	iterate8: iterate::create_iterate_8_suite_2(),
	utility: utility::create_utility_suite(),
	aegp_interface: interface::create_aegp_pf_interface_suite(),
	angle_param: angle_param::create_angle_param_suite(),
	ae_app: ae_app::create_ae_app_suite_v6(),
};

/// Process-wide storage for the stateless suite vtables handed to plugins.
///
/// Every field is a plain table of `extern "C"` function pointers with no
/// per-instance state, so a single shared `static` instance serves every
/// [`PluginInstance`](crate::PluginInstance) — see the registry ownership notes
/// in [`registry`]. Suites live for the program's lifetime; there is nothing to
/// allocate or free.
pub struct SuiteContainer {
	pub ansi: PF_ANSICallbacks,
	pub effect_ui: PF_EffectUISuite1,
	pub handle: PF_HandleSuite1,
	pub world_transform: PF_WorldTransformSuite1,
	pub world: PF_WorldSuite2,
	pub iterate8: PF_Iterate8Suite2,
	pub utility: PF_UtilitySuite,
	pub aegp_interface: AEGP_PFInterfaceSuite1,
	pub angle_param: PF_AngleParamSuite1,
	pub ae_app: PFAppSuite6,
}

/// Dispatch a registry-managed (dynamic) suite acquisition.
///
/// Acquires the suite via [`registry::acquire`], and on success writes the
/// resulting pointer into the `*suite` out-param, logs it, and returns
/// `PF_Err_NONE`; on failure it returns the registry's error code. Collapses
/// the otherwise-identical boilerplate of every dynamic arm in
/// [`rusty_acquire_suite`] into a single expression.
macro_rules! dispatch_dynamic {
	($suite:expr, $name:expr, $version:expr, $ctor:expr $(,)?) => {
		match acquire($name, $version, $ctor) {
			Ok(ptr) => {
				// SAFETY: `rusty_acquire_suite` returns early when `suite` is null,
				// so the out-param is a valid place to write here.
				unsafe { *$suite = ptr as *const c_void };
				log::info!("Acquired {} v{} (Registry)", $name, $version);
				PF_Err_NONE as PF_Err
			}
			Err(err) => err,
		}
	};
}

/// Hand back a pointer to one of the shared [`SUITE_CONTAINER`] vtables.
///
/// Writes `&SUITE_CONTAINER.$field` into the `*suite` out-param, logs it, and
/// returns `PF_Err_NONE`. The pointer is valid for the program's lifetime, so
/// there is no matching release step.
macro_rules! dispatch_static {
	($suite:expr, $name:expr, $version:expr, $field:ident $(,)?) => {{
		// SAFETY: `rusty_acquire_suite` returns early when `suite` is null,
		// so the out-param is a valid place to write here.
		unsafe { *$suite = &SUITE_CONTAINER.$field as *const _ as *const c_void };
		log::info!("Acquired {} v{}", $name, $version);
		PF_Err_NONE as PF_Err
	}};
}

/// Emulates `SPBasicSuite::AcquireSuite` function
/// # Safety
/// This function is unsafe because it handles raw pointers.
#[allow(non_snake_case)]
pub unsafe extern "C" fn rusty_acquire_suite(name: *const i8, version: i32, suite: *mut *const c_void) -> i32 {
	if suite.is_null() || name.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	let suite_name = unsafe {
		match CStr::from_ptr(name).to_str() {
			Ok(s) => s,
			Err(_) => return PF_Err_INTERNAL_STRUCT_DAMAGED as PF_Err,
		}
	};

	#[cfg(feature = "diagnostics")]
	DiagnosticBuilder::new()
		.set_name("SPBasicSuite/AcquireSuite")
		.add_arg("name", format!("{:?}", unsafe { CStr::from_ptr(name) }))
		.add_arg("version", version)
		.add_arg("suite", format!("{:?}", suite))
		.emit();

	match (suite_name, version) {
		// Static suites: pointers into the shared SUITE_CONTAINER.
		("PF ANSI Suite", 1) => dispatch_static!(suite, suite_name, version, ansi),
		("PF Effect UI Suite", 1) => dispatch_static!(suite, suite_name, version, effect_ui),
		("PF Handle Suite", 2) => dispatch_static!(suite, suite_name, version, handle),
		("PF World Transform Suite", 1) => dispatch_static!(suite, suite_name, version, world_transform),
		("PF World Suite", 2) => dispatch_static!(suite, suite_name, version, world),
		("PF Iterate8 Suite", 2) => dispatch_static!(suite, suite_name, version, iterate8),
		("PF Utility Suite", 1..=18) => dispatch_static!(suite, suite_name, version, utility),
		("AEGP PF Interface Suite", 1) => dispatch_static!(suite, suite_name, version, aegp_interface),
		("PF AngleParamSuite", 1) => dispatch_static!(suite, suite_name, version, angle_param),
		("PF AE App Suite", 6) => dispatch_static!(suite, suite_name, version, ae_app),
		// Dynamic suites still managed by the registry (converted in the next step).
		("AEGP Utility Suite", 1..=18) => {
			dispatch_dynamic!(suite, suite_name, version, utility::create_aegp_utility_suite_compat_v11)
		}
		_ => {
			log::warn!("Suite '{}' v{} not found.", suite_name, version);
			PF_Err_OUT_OF_MEMORY as PF_Err
		}
	}
}

/// Emulates `SPBasicSuite::ReleaseSuite` function
/// # Safety
/// This function is unsafe because it handles raw pointers.
#[allow(non_snake_case)]
pub unsafe extern "C" fn rusty_release_suite(name: *const ::std::os::raw::c_char, version: i32) -> PF_Err {
	#[cfg(feature = "diagnostics")]
	DiagnosticBuilder::new()
		.set_name("SPBasicSuite/ReleaseSuite")
		.add_arg("name", format!("{:?}", unsafe { CStr::from_ptr(name) }))
		.add_arg("version", version)
		.emit();

	if name.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	let suite_name = match unsafe { CStr::from_ptr(name) }.to_str() {
		Ok(s) => s,
		Err(_) => return PF_Err_INTERNAL_STRUCT_DAMAGED as PF_Err,
	};

	// Static suites are not managed by registry
	if suite_name == "PF ANSI Suite" || suite_name == "PF Effect UI Suite" {
		return PF_Err_NONE as PF_Err;
	}

	// Release from registry (decrements ref count, drops the Box when it hits 0)
	release(suite_name, version)
}
