mod ansi;
mod effect_ui;
mod handle;
mod iterate_8;
mod world_transform;

use crate::diagnostics::*;
use after_effects_sys::*;
use std::ffi::CStr;
use std::os::raw::c_void;

pub static SUITE_CONTAINER: SuiteContainer = SuiteContainer {
	ansi: PF_ANSICallbacks {
		atan: Some(crate::ansi::atan_sys),
		atan2: Some(crate::ansi::atan2_sys),
		ceil: Some(crate::ansi::ceil_sys),
		cos: Some(crate::ansi::cos_sys),
		exp: None,
		fabs: None,
		floor: None,
		fmod: None,
		hypot: None,
		log: None,
		log10: None,
		pow: None,
		sin: Some(crate::ansi::sin_sys),
		sqrt: None,
		tan: None,
		sprintf: Some(crate::ansi::sprintf_sys),
		strcpy: None,
		asin: None,
		acos: None,
		ansi_procs: [0; 1],
	},
	effect_ui: PF_EffectUISuite1 {
		PF_SetOptionsButtonName: Some(effect_ui::SetOptionButtonName_sys),
	},
	handle_suite: PF_HandleSuite1 {
		host_new_handle: None,
		host_lock_handle: None,
		host_unlock_handle: None,
		host_dispose_handle: None,
		host_get_handle_size: None,
		host_resize_handle: None,
	},
	iterate_8_suite: PF_Iterate8Suite2 {
		iterate: Some(iterate_8::iterate_8_sys),
		iterate_origin: None,
		iterate_lut: None,
		iterate_origin_non_clip_src: None,
		iterate_generic: None,
	},
	world_transform_suite: PF_WorldTransformSuite1 {
		composite_rect: None,
		blend: None,
		convolve: None,
		copy: Some(world_transform::Copy_sys),
		copy_hq: None,
		transfer_rect: None,
		transform_world: None,
	},
};

pub(super) struct SuiteContainer {
	pub(super) ansi: PF_ANSICallbacks,
	pub(super) effect_ui: PF_EffectUISuite1,
	pub(super) handle_suite: PF_HandleSuite1,
	pub(super) iterate_8_suite: PF_Iterate8Suite2,
	pub(super) world_transform_suite: PF_WorldTransformSuite1,
}

/// Emulates `SPBasicSuite::AcquireSuite` function
/// # Safety
/// This function is unsafe because it handles raw pointers.
pub unsafe extern "C" fn rusty_acquire_suite(
	name: *const i8,
	version: i32,
	suite: *mut *const c_void,
) -> i32 {
	if suite.is_null() || name.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	unsafe {
		let suite_name = match CStr::from_ptr(name).to_str() {
			Ok(s) => s,
			Err(_) => return PF_Err_INTERNAL_STRUCT_DAMAGED as PF_Err,
		};

		#[cfg(feature = "diagnostics")]
		DiagnosticBuilder::new()
			.set_name("SPBasicSuite/AcquireSuite")
			.add_arg("name", format!("{:?}", CStr::from_ptr(name)))
			.add_arg("version", version)
			.add_arg("suite", format!("{:?}", suite))
			.emit();

		match (suite_name, version) {
			("PF ANSI Suite", 1) => {
				*suite = &SUITE_CONTAINER.ansi as *const _ as *mut c_void;

				log::info!("Acquired PF ANSI Suite v1");
				PF_Err_NONE as PF_Err
			}
			("PF Effect UI Suite", 1) => {
				*suite = &SUITE_CONTAINER.effect_ui as *const _ as *mut c_void;

				log::info!("Acquired PF Effect UI Suite v1");
				PF_Err_NONE as PF_Err
			}
			("PF Handle Suite", 2) => {
				*suite = &SUITE_CONTAINER.handle_suite as *const _ as *mut c_void;

				log::info!("Acquired PF Handle Suite v2");
				PF_Err_NONE as PF_Err
			}
			("PF World Transform Suite", 1) => {
				*suite = &SUITE_CONTAINER.world_transform_suite as *const _ as *mut c_void;

				log::info!("Acquired PF World Transform Suite v1");
				PF_Err_NONE as PF_Err
			}
			("PF Iterate8 Suite", 2) => {
				*suite = &SUITE_CONTAINER.iterate_8_suite as *const _ as *mut c_void;

				log::info!("Acquired PF Iterate8 Suite v2");
				PF_Err_NONE as PF_Err
			}
			_ => {
				log::warn!("Requested unknown suite: {} v{}", suite_name, version);
				PF_Err_OUT_OF_MEMORY as PF_Err
			}
		}
	}
}

/// Emulates `SPBasicSuite::ReleaseSuite` function
/// # Safety
/// This function is unsafe because it handles raw pointers.
pub unsafe extern "C" fn rusty_release_suite(
	name: *const ::std::os::raw::c_char,
	version: int32,
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

	PF_Err_NONE as PF_Err
}
