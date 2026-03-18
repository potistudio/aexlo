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

use crate::core::diagnostics::*;
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
};

pub struct SuiteContainer {
	pub ansi: PF_ANSICallbacks,
	pub effect_ui: PF_EffectUISuite1,
}

/// Emulates `SPBasicSuite::AcquireSuite` function
/// # Safety
/// This function is unsafe because it handles raw pointers.
#[allow(non_snake_case)]
pub unsafe extern "C" fn rusty_acquire_suite(
	name: *const i8,
	version: i32,
	suite: *mut *const c_void,
) -> i32 {
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

	// Select creator function (returns Box<Suite_type>)
	match (suite_name, version) {
		// Static suites (managed directly)
		("PF ANSI Suite", 1) => {
			unsafe {
				*suite = &SUITE_CONTAINER.ansi as *const _ as *const c_void;
			}
			log::info!("Acquired PF ANSI Suite v1");
			return PF_Err_NONE as PF_Err;
		}
		("PF Effect UI Suite", 1) => {
			unsafe {
				*suite = &SUITE_CONTAINER.effect_ui as *const _ as *const c_void;
			}
			log::info!("Acquired PF Effect UI Suite v1");
			return PF_Err_NONE as PF_Err;
		}
		// Dynamic suites (managed by registry)
		("PF Handle Suite", 2) => unsafe {
			match acquire(suite_name, version, || handle::create_handle_suite_1()) {
				Ok(ptr) => {
					*suite = ptr as *const c_void;
					log::info!("Acquired {} Suite v{} (Registry)", suite_name, version);
					PF_Err_NONE as PF_Err
				}
				Err(err) => err,
			}
		},
		("PF World Transform Suite", 1) => unsafe {
			match acquire(suite_name, version, || {
				transform::create_world_transform_suite_1()
			}) {
				Ok(ptr) => {
					*suite = ptr as *const c_void;
					log::info!("Acquired {} Suite v{} (Registry)", suite_name, version);
					PF_Err_NONE as PF_Err
				}
				Err(err) => err,
			}
		},
		("PF World Suite", 2) => unsafe {
			match acquire(suite_name, version, || world::create_world_suite_2()) {
				Ok(ptr) => {
					*suite = ptr as *const c_void;
					log::info!("Acquired {} Suite v{} (Registry)", suite_name, version);
					PF_Err_NONE as PF_Err
				}
				Err(err) => err,
			}
		},
		("PF Iterate8 Suite", 2) => unsafe {
			match acquire(suite_name, version, || iterate::create_iterate_8_suite_2()) {
				Ok(ptr) => {
					*suite = ptr as *const c_void;
					log::info!("Acquired {} Suite v{} (Registry)", suite_name, version);
					PF_Err_NONE as PF_Err
				}
				Err(err) => err,
			}
		},
		("PF Utility Suite", 1..=18) => unsafe {
			match acquire(suite_name, version, || utility::create_utility_suite()) {
				Ok(ptr) => {
					*suite = ptr as *const c_void;
					log::info!("Acquired {} Suite v{} (Registry)", suite_name, version);
					PF_Err_NONE as PF_Err
				}
				Err(err) => err,
			}
		},
		("AEGP Utility Suite", 1..=18) => unsafe {
			match acquire(suite_name, version, || {
				utility::create_aegp_utility_suite_compat_v11()
			}) {
				Ok(ptr) => {
					*suite = ptr as *const c_void;
					log::info!("Acquired {} Suite v{} (Registry)", suite_name, version);
					PF_Err_NONE as PF_Err
				}
				Err(err) => err,
			}
		},
		("AEGP PF Interface Suite", 1) => {
			match acquire(suite_name, version, || interface::create_aegp_pf_interface_suite()) {
				Ok(ptr) => {
					unsafe {
						*suite = ptr as *const c_void;
					}
					log::info!("Acquired {} Suite v{} (Registry)", suite_name, version);
					PF_Err_NONE as PF_Err
				}
				Err(err) => err,
			}
		}
		_ => PF_Err_OUT_OF_MEMORY as PF_Err,
	}
}

/// Emulates `SPBasicSuite::ReleaseSuite` function
/// # Safety
/// This function is unsafe because it handles raw pointers.
#[allow(non_snake_case)]
pub unsafe extern "C" fn rusty_release_suite(
	name: *const ::std::os::raw::c_char,
	version: i32,
) -> PF_Err {
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

	// Release from registry (decrements ref count, drops Arc when 0)
	release(suite_name, version)
}
