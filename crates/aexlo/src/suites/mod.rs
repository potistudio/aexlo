//! Host suites handed to plugins through `SPBasicSuite::AcquireSuite`.
//!
//! # Ownership model
//!
//! Every suite is a **stateless vtable** - a table of `extern "C"` function
//! pointers with no per-instance state; any mutable state lives behind the
//! plugin-provided pointers those callbacks receive, not in the suite struct.
//! Because of that, a single **process-wide** instance is shared by every
//! [`PluginInstance`](crate::PluginInstance), and across threads, soundly.
//!
//! Nearly all suites live in the `const` [`SUITE_CONTAINER`] static, so
//! acquiring one just hands back a pointer into it and releasing it is a
//! no-op; nothing is allocated or freed. The sole exception is the AEGP Utility
//! compat suite, whose type-erased pointer slots can't be built in a `const`
//! context - it lives in its own [`LazyLock`](utility::AEGP_UTILITY_SUITE)
//! instead, but is otherwise the same shared-static model.

mod adv_app;
mod adv_item;
mod adv_time;
mod ae_app;
mod angle_param;
pub mod ansi;
mod cache_on_load;
mod channel;
pub mod color_callbacks;
mod color_param;
mod effect_ui;
mod ffi;
pub mod fill_matte;
pub mod gpu_device;
pub mod handle;
mod helper;
pub mod interface;
pub mod iterate;
pub mod macros;
pub mod param_utils;
mod path;
pub mod persistent_data;
pub mod pixel_data;
mod pixel_format;
mod pixel_norm;
mod point_param;
pub mod sampling;
pub mod transform;
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
	effect_ui: effect_ui::create_effect_ui_suite_1(),
	effect_custom_ui1: effect_ui::create_effect_custom_ui_suite_1(),
	effect_custom_ui2: effect_ui::create_effect_custom_ui_suite_2(),
	overlay_theme: effect_ui::create_overlay_theme_suite_1(),
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
	batch_sampling: sampling::create_batch_sampling_suite_1(),
	sampling8: sampling::create_sampling_8_suite_1(),
	sampling16: sampling::create_sampling_16_suite_1(),
	sampling_float: sampling::create_sampling_float_suite_1(),
	pixel_data: pixel_data::create_pixel_data_suite_2(),
	pixel_format1: pixel_format::create_pixel_format_suite_1(),
	pixel_format2: pixel_format::create_pixel_format_suite_2(),
	cache_on_load: cache_on_load::create_cache_on_load_suite_1(),
	channel: channel::create_channel_suite_1(),
	ae_app4: ae_app::create_ae_app_suite_4(),
	ae_app5: ae_app::create_ae_app_suite_5(),
	ae_app6: ae_app::create_ae_app_suite_6(),
	adv_app1: adv_app::create_adv_app_suite_1(),
	adv_app2: adv_app::create_adv_app_suite_2(),
	adv_item: adv_item::create_adv_item_suite_1(),
	adv_time1: adv_time::create_adv_time_suite_1(),
	adv_time2: adv_time::create_adv_time_suite_2(),
	adv_time3: adv_time::create_adv_time_suite_3(),
	adv_time4: adv_time::create_adv_time_suite_4(),
	helper1: helper::create_helper_suite_1(),
	helper2: helper::create_helper_suite_2(),
	gpu_device: gpu_device::create_gpu_device_suite_1(),
	param_utils: param_utils::create_param_utils_suite_3(),
	path_query: path::create_path_query_suite_1(),
	path_data: path::create_path_data_suite_1(),
	persistent_data: persistent_data::create_persistent_data_suite_3(),
};

/// Process-wide storage for the stateless suite vtables handed to plugins.
///
/// Every field is a plain table of `extern "C"` function pointers with no
/// per-instance state, so a single shared `static` instance serves every
/// [`PluginInstance`](crate::PluginInstance) - see the module-level ownership
/// notes. Suites live for the program's lifetime; there is nothing to allocate
/// or free.
pub struct SuiteContainer {
	pub ansi: PF_ANSICallbacksBlock,
	pub effect_ui: PF_EffectUISuite1,
	pub effect_custom_ui1: PF_EffectCustomUISuite1,
	pub effect_custom_ui2: PF_EffectCustomUISuite2,
	pub overlay_theme: PF_EffectCustomUIOverlayThemeSuite1,
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
	pub batch_sampling: PF_BatchSamplingSuite1,
	pub sampling8: PF_Sampling8Suite1,
	pub sampling16: PF_Sampling16Suite1,
	pub sampling_float: PF_SamplingFloatSuite1,
	pub pixel_data: PF_PixelDataSuite2,
	pub pixel_format1: PF_PixelFormatSuite,
	pub pixel_format2: PF_PixelFormatSuite2,
	pub cache_on_load: PF_CacheOnLoadSuite1,
	pub channel: PF_ChannelSuite1,
	pub ae_app4: PFAppSuite4,
	pub ae_app5: PFAppSuite5,
	pub ae_app6: PFAppSuite6,
	pub adv_app1: PF_AdvAppSuite1,
	pub adv_app2: PF_AdvAppSuite2,
	pub adv_item: PF_AdvItemSuite1,
	pub adv_time1: PF_AdvTimeSuite1,
	pub adv_time2: PF_AdvTimeSuite2,
	pub adv_time3: PF_AdvTimeSuite3,
	pub adv_time4: PF_AdvTimeSuite4,
	pub helper1: PF_HelperSuite1,
	pub helper2: PF_HelperSuite2,
	pub gpu_device: PF_GPUDeviceSuite1,
	pub param_utils: PF_ParamUtilsSuite3,
	pub path_query: PF_PathQuerySuite1,
	pub path_data: PF_PathDataSuite1,
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
		("PF Effect Custom UI Suite", 1) => dispatch_static!(suite, suite_name, version, effect_custom_ui1),
		("PF Effect Custom UI Suite", 2) => dispatch_static!(suite, suite_name, version, effect_custom_ui2),
		("PF Effect Custom UI Overlay Theme Suite", 1) => {
			dispatch_static!(suite, suite_name, version, overlay_theme)
		}
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
		("PF Batch Sampling Suite", 1) => dispatch_static!(suite, suite_name, version, batch_sampling),
		("PF Sampling8 Suite", 1) => dispatch_static!(suite, suite_name, version, sampling8),
		("PF Sampling16 Suite", 1) => dispatch_static!(suite, suite_name, version, sampling16),
		("PF SamplingFloat Suite", 1) => dispatch_static!(suite, suite_name, version, sampling_float),
		// PixelData suites are append-only (v2 adds the GPU accessor), so the v2
		// table also satisfies v1 requests.
		("PF Pixel Data Suite", 1..=2) => dispatch_static!(suite, suite_name, version, pixel_data),
		// v1 is Premiere's full `PF_PixelFormatSuite`; v2 is AE's two-entry
		// registration-only table. Same name, unrelated layouts.
		("PF Pixel Format Suite", 1) => dispatch_static!(suite, suite_name, version, pixel_format1),
		("PF Pixel Format Suite", 2) => dispatch_static!(suite, suite_name, version, pixel_format2),
		("PF Cache On Load Suite", 1) => dispatch_static!(suite, suite_name, version, cache_on_load),
		("PF AE Channel Suite", 1) => dispatch_static!(suite, suite_name, version, channel),
		// The App Suite's wire versions do NOT follow the struct names, and the
		// layouts are not append-only (v5 inserts `PF_AppGetLanguage` mid-table),
		// so each wire version must get its own exactly-shaped table.
		("PF AE App Suite", v) if v == kPFAppSuiteVersion6 as i32 => dispatch_static!(suite, suite_name, version, ae_app6),
		("PF AE App Suite", v) if v == kPFAppSuiteVersion5 as i32 => dispatch_static!(suite, suite_name, version, ae_app5),
		("PF AE App Suite", v) if v == kPFAppSuiteVersion4 as i32 => dispatch_static!(suite, suite_name, version, ae_app4),
		// Adv App v2 only appends `PF_AppendInfoText`; Adv Time versions differ
		// in their preference struct, so each gets an exactly-shaped table.
		("PF AE Adv App Suite", 1) => dispatch_static!(suite, suite_name, version, adv_app1),
		("PF AE Adv App Suite", 2) => dispatch_static!(suite, suite_name, version, adv_app2),
		("PF AE Adv Item Suite", 1) => dispatch_static!(suite, suite_name, version, adv_item),
		("PF AE Adv Time Suite", 1) => dispatch_static!(suite, suite_name, version, adv_time1),
		("PF AE Adv Time Suite", 2) => dispatch_static!(suite, suite_name, version, adv_time2),
		("PF AE Adv Time Suite", 3) => dispatch_static!(suite, suite_name, version, adv_time3),
		("PF AE Adv Time Suite", 4) => dispatch_static!(suite, suite_name, version, adv_time4),
		("AE Plugin Helper Suite", 1) => dispatch_static!(suite, suite_name, version, helper1),
		("AE Plugin Helper Suite2", 1..=2) => dispatch_static!(suite, suite_name, version, helper2),
		("PF GPU Device Suite", 1) => dispatch_static!(suite, suite_name, version, gpu_device),
		// ParamUtils suites are append-only, so the v3 table also satisfies v1/v2 requests.
		("PF Param Utils Suite", 1..=3) => dispatch_static!(suite, suite_name, version, param_utils),
		("PF Path Query Suite", 1) => dispatch_static!(suite, suite_name, version, path_query),
		("PF Path Data Suite", 1) => dispatch_static!(suite, suite_name, version, path_data),
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
			("PF Pixel Format Suite", 1),
			("PF Pixel Format Suite", 2),
			("PF Batch Sampling Suite", 1),
			("PF Sampling8 Suite", 1),
			("PF Sampling16 Suite", 1),
			("PF SamplingFloat Suite", 1),
			("PF Effect UI Suite", 1),
			("PF Effect Custom UI Suite", 1),
			("PF Effect Custom UI Suite", 2),
			("PF Effect Custom UI Overlay Theme Suite", 1),
			("PF Path Query Suite", 1),
			("PF Path Data Suite", 1),
			("PF AE Adv App Suite", 1),
			("PF AE Adv App Suite", 2),
			("PF AE Adv Item Suite", 1),
			("PF AE Adv Time Suite", 1),
			("PF AE Adv Time Suite", 4),
			("PF AE Channel Suite", 1),
			("PF Cache On Load Suite", 1),
			("AE Plugin Helper Suite", 1),
			("AE Plugin Helper Suite2", 2),
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
