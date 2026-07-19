//! Host suites handed to plugins through `SPBasicSuite::AcquireSuite`.
//!
//! # Ownership model
//!
//! Every suite is a **stateless vtable** — a table of `extern "C"` function
//! pointers with no per-instance state; any mutable state lives behind the
//! plugin-provided pointers those callbacks receive, not in the suite struct.
//! Because of that, a single **process-wide** instance is shared by every
//! [`PluginInstance`](crate::PluginInstance), and across threads, soundly.
//!
//! Nearly all suites live in the `const` [`SUITE_CONTAINER`] static, so
//! acquiring one just hands back a pointer into it and releasing it is a
//! no-op; nothing is allocated or freed. The sole exception is the AEGP Utility
//! compat suite, whose type-erased pointer slots can't be built in a `const`
//! context — it lives in its own [`LazyLock`](utility::AEGP_UTILITY_SUITE)
//! instead, but is otherwise the same shared-static model.

mod ae_app;
mod angle_param;
pub mod ansi;
pub mod color_callbacks;
mod color_param;
pub mod fill_matte;
pub mod gpu_device;
pub mod handle;
pub mod interface;
pub mod iterate;
pub mod macros;
pub mod param_utils;
pub mod persistent_data;
pub mod pixel_data;
mod pixel_norm;
mod point_param;
pub mod transform;
pub mod ui;
pub mod utility;
pub mod world;

#[cfg(feature = "diagnostics")]
use crate::core::diagnostics::DiagnosticBuilder;
use after_effects_sys::*;
use std::ffi::CStr;
use std::os::raw::c_void;

pub static SUITE_CONTAINER: SuiteContainer = SuiteContainer {
	ansi: PF_ANSICallbacksBlock {
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
		unused_longA: [0; 1],
	},
	effect_ui: PF_EffectUISuite1 {
		PF_SetOptionsButtonName: Some(ui::SetOptionButtonName_sys),
	},
	handle: handle::create_handle_suite(),
	world_transform: transform::create_world_transform_suite_1(),
	world: world::create_world_suite(),
	iterate8: iterate::create_iterate_8_suite_2(),
	iterate16: iterate::create_iterate_16_suite_2(),
	iterate_float: iterate::create_iterate_float_suite_2(),
	utility: utility::create_utility_suite(),
	aegp_interface: interface::create_aegp_pf_interface_suite(),
	angle_param: angle_param::create_angle_param_suite(),
	color_param: color_param::create_color_param_suite_1(),
	point_param: point_param::create_point_param_suite_1(),
	color_callbacks8: color_callbacks::create_color_callbacks_suite_1(),
	color_callbacks16: color_callbacks::create_color_callbacks_16_suite_1(),
	color_callbacks_float: color_callbacks::create_color_callbacks_float_suite_1(),
	fill_matte: fill_matte::create_fill_matte_suite_2(),
	pixel_data: pixel_data::create_pixel_data_suite_2(),
	ae_app: ae_app::create_ae_app_suite_v6(),
	gpu_device: gpu_device::create_gpu_device_suite_1(),
	param_utils: param_utils::create_param_utils_suite_3(),
	persistent_data: persistent_data::create_persistent_data_suite_3(),
};

/// Process-wide storage for the stateless suite vtables handed to plugins.
///
/// Every field is a plain table of `extern "C"` function pointers with no
/// per-instance state, so a single shared `static` instance serves every
/// [`PluginInstance`](crate::PluginInstance) — see the module-level ownership
/// notes. Suites live for the program's lifetime; there is nothing to allocate
/// or free.
pub struct SuiteContainer {
	pub ansi: PF_ANSICallbacksBlock,
	pub effect_ui: PF_EffectUISuite1,
	pub handle: PF_HandleSuite1,
	pub world_transform: PF_WorldTransformSuite1,
	pub world: PF_WorldSuite2,
	pub iterate8: PF_Iterate8Suite2,
	pub iterate16: PF_iterate16Suite2,
	pub iterate_float: PF_iterateFloatSuite2,
	pub utility: PF_UtilitySuite,
	pub aegp_interface: AEGP_PFInterfaceSuite1,
	pub angle_param: PF_AngleParamSuite1,
	pub color_param: PF_ColorParamSuite1,
	pub point_param: PF_PointParamSuite1,
	pub color_callbacks8: PF_ColorCallbacksSuite1,
	pub color_callbacks16: PF_ColorCallbacks16Suite1,
	pub color_callbacks_float: PF_ColorCallbacksFloatSuite1,
	pub fill_matte: PF_FillMatteSuite2,
	pub pixel_data: PF_PixelDataSuite2,
	pub ae_app: PFAppSuite6,
	pub gpu_device: PF_GPUDeviceSuite1,
	pub param_utils: PF_ParamUtilsSuite3,
	pub persistent_data: AEGP_PersistentDataSuite3,
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
		// debug, not info: some plugins re-acquire suites on every render call.
		log::debug!("Acquired {} v{}", $name, $version);
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
		// Iterate suites are append-only, so the v2 tables also satisfy v1 requests.
		("PF Iterate8 Suite", 1..=2) => dispatch_static!(suite, suite_name, version, iterate8),
		("PF iterate16 Suite", 1..=2) => dispatch_static!(suite, suite_name, version, iterate16),
		("PF iterateFloat Suite", 1..=2) => dispatch_static!(suite, suite_name, version, iterate_float),
		("PF Utility Suite", 1..=18) => dispatch_static!(suite, suite_name, version, utility),
		("AEGP PF Interface Suite", 1) => dispatch_static!(suite, suite_name, version, aegp_interface),
		("PF AngleParamSuite", 1) => dispatch_static!(suite, suite_name, version, angle_param),
		("PF ColorParamSuite", 1) => dispatch_static!(suite, suite_name, version, color_param),
		("PF PointParamSuite", 1) => dispatch_static!(suite, suite_name, version, point_param),
		("PF Color Suite", 1) => dispatch_static!(suite, suite_name, version, color_callbacks8),
		("PF Color16 Suite", 1) => dispatch_static!(suite, suite_name, version, color_callbacks16),
		("PF ColorFloat Suite", 1) => dispatch_static!(suite, suite_name, version, color_callbacks_float),
		("PF Fill Matte Suite", 2) => dispatch_static!(suite, suite_name, version, fill_matte),
		// PixelData suites are append-only (v2 adds the GPU accessor), so the v2
		// table also satisfies v1 requests.
		("PF Pixel Data Suite", 1..=2) => dispatch_static!(suite, suite_name, version, pixel_data),
		// AE suites are append-only across versions, so a v6 table safely satisfies
		// older requests (v1..). Plugins that request v1 (e.g. via AEFX_AcquireSuite in
		// their localization path) get a null-deref on a null out_data if we reject it,
		// so accept the whole range rather than only v6.
		("PF AE App Suite", 1..=6) => dispatch_static!(suite, suite_name, version, ae_app),
		("PF GPU Device Suite", 1) => dispatch_static!(suite, suite_name, version, gpu_device),
		// ParamUtils suites are append-only, so the v3 table also satisfies v1/v2 requests.
		("PF Param Utils Suite", 1..=3) => dispatch_static!(suite, suite_name, version, param_utils),
		("AEGP Persistent Data Suite", 3) => {
			dispatch_static!(suite, suite_name, version, persistent_data)
		}
		("AEGP Utility Suite", 1..=18) => {
			// Lives in its own LazyLock rather than SUITE_CONTAINER (see AEGP_UTILITY_SUITE).
			// SAFETY: `suite` was null-checked at the top of this function.
			unsafe { *suite = &*utility::AEGP_UTILITY_SUITE as *const _ as *const c_void };
			log::debug!("Acquired {} v{}", suite_name, version);
			PF_Err_NONE as PF_Err
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
// `version` is only read by the diagnostics build; suppress the unused warning otherwise.
#[cfg_attr(not(feature = "diagnostics"), allow(unused_variables))]
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

	// Every suite is a process-wide shared static (see the module docs); nothing
	// is allocated per acquire, so releasing one is a no-op.
	PF_Err_NONE as PF_Err
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::ffi::CString;

	fn acquire(name: &str, version: i32) -> (PF_Err, *const c_void) {
		let cname = CString::new(name).unwrap();
		let mut out: *const c_void = std::ptr::null();
		let err = unsafe { rusty_acquire_suite(cname.as_ptr(), version, &mut out) };
		(err, out)
	}

	#[test]
	fn acquire_serves_every_registered_suite() {
		for (name, version) in [
			("PF ANSI Suite", 1),
			("PF Iterate8 Suite", 2),
			("PF iterate16 Suite", 1),
			("PF iterate16 Suite", 2),
			("PF iterateFloat Suite", 2),
			("PF AngleParamSuite", 1),
			("PF ColorParamSuite", 1),
			("PF PointParamSuite", 1),
			("PF Color Suite", 1),
			("PF Color16 Suite", 1),
			("PF ColorFloat Suite", 1),
			("PF Fill Matte Suite", 2),
			("PF Pixel Data Suite", 1),
			("PF Pixel Data Suite", 2),
		] {
			let (err, ptr) = acquire(name, version);
			assert_eq!(err, PF_Err_NONE as PF_Err, "'{name}' v{version} should be served");
			assert!(!ptr.is_null(), "'{name}' v{version} returned a null suite");
		}
	}

	#[test]
	fn acquire_rejects_unknown_suites_and_versions() {
		let (err, ptr) = acquire("PF Nonexistent Suite", 1);
		assert_ne!(err, PF_Err_NONE as PF_Err);
		assert!(ptr.is_null());

		// Fill Matte is only served at its known v2 layout.
		let (err, _) = acquire("PF Fill Matte Suite", 3);
		assert_ne!(err, PF_Err_NONE as PF_Err);
	}
}
