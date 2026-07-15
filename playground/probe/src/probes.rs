//! Active probes: exercise host services and record what actually happened.
//!
//! Suite probing walks a fixed table of (name, version) pairs through
//! `SPBasicSuite::AcquireSuite`, mapping which suites the host actually
//! vends. Utils probing calls the always-present `PF_UtilCallbacks` members
//! (ANSI math/string helpers, host handle allocator) with fixed inputs so
//! two hosts can be compared result-for-result.

use std::ffi::c_void;

use after_effects_sys as ae;
use serde_json::json;

use crate::trace::trace;

/// Suites to attempt to acquire, mirroring the coverage table in the aexlo
/// README. Names/versions come straight from the SDK headers via
/// after-effects-sys (the version constants look odd — kPFHandleSuiteVersion1
/// really is 2 — but that is faithful to the headers).
const SUITES: &[(&[u8], u32)] = &[
	(ae::kPFANSISuite, ae::kPFANSISuiteVersion1),
	(ae::kPFHandleSuite, ae::kPFHandleSuiteVersion1),
	(ae::kPFWorldSuite, ae::kPFWorldSuiteVersion1),
	(ae::kPFWorldSuite, ae::kPFWorldSuiteVersion2),
	(ae::kPFWorldTransformSuite, ae::kPFWorldTransformSuiteVersion1),
	(ae::kPFIterate8Suite, ae::kPFIterate8SuiteVersion1),
	(ae::kPFIterate8Suite, ae::kPFIterate8SuiteVersion2),
	(ae::kPFIterate16Suite, ae::kPFIterate16SuiteVersion1),
	(ae::kPFIterate16Suite, ae::kPFIterate16SuiteVersion2),
	(ae::kPFIterateFloatSuite, ae::kPFIterateFloatSuiteVersion1),
	(ae::kPFIterateFloatSuite, ae::kPFIterateFloatSuiteVersion2),
	(ae::kPFPixelDataSuite, ae::kPFPixelDataSuiteVersion1),
	(ae::kPFPixelFormatSuite, ae::kPFPixelFormatSuiteVersion1),
	(ae::kPFPixelFormatSuite, ae::kPFPixelFormatSuiteVersion2),
	(ae::kPFColorCallbacksSuite, ae::kPFColorCallbacksSuiteVersion1),
	(ae::kPFColorCallbacks16Suite, ae::kPFColorCallbacks16SuiteVersion1),
	(ae::kPFColorCallbacksFloatSuite, ae::kPFColorCallbacksFloatSuiteVersion1),
	(ae::kPFBatchSamplingSuite, ae::kPFBatchSamplingSuiteVersion1),
	(ae::kPFSampling8Suite, ae::kPFSampling8SuiteVersion1),
	(ae::kPFSampling16Suite, ae::kPFSampling16SuiteVersion1),
	(ae::kPFSamplingFloatSuite, ae::kPFSamplingFloatSuiteVersion1),
	(ae::kPFFillMatteSuite, ae::kPFFillMatteSuiteVersion2),
	(ae::kPFGPUDeviceSuite, ae::kPFGPUDeviceSuiteVersion1),
	(ae::kPFParamUtilsSuite, ae::kPFParamUtilsSuiteVersion3),
	(ae::kPFAngleParamSuite, ae::kPFAngleParamSuiteVersion1),
	(ae::kPFColorParamSuite, ae::kPFColorParamSuiteVersion1),
	(ae::kPFPointParamSuite, ae::kPFPointParamSuiteVersion1),
	(ae::kPFAppSuite, ae::kPFAppSuiteVersion4),
	(ae::kPFAppSuite, ae::kPFAppSuiteVersion5),
	(ae::kPFAppSuite, ae::kPFAppSuiteVersion6),
	(ae::kPFAdvAppSuite, ae::kPFAdvAppSuiteVersion1),
	(ae::kPFAdvAppSuite, ae::kPFAdvAppSuiteVersion2),
	(ae::kPFAdvTimeSuite, ae::kPFAdvTimeSuiteVersion4),
	(ae::kPFAdvItemSuite, ae::kPFAdvItemSuiteVersion1),
	(ae::kPFCacheOnLoadSuite, ae::kPFCacheOnLoadSuiteVersion1),
	(ae::kPFChannelSuite1, ae::kPFChannelSuiteVersion1),
	(ae::kPFPathQuerySuite, ae::kPFPathQuerySuiteVersion1),
	(ae::kPFPathDataSuite, ae::kPFPathDataSuiteVersion1),
	(ae::kPFEffectUISuite, ae::kPFEffectUISuiteVersion1),
	(ae::kPFEffectCustomUISuite, ae::kPFEffectCustomUISuiteVersion1),
	(ae::kPFEffectCustomUISuite, ae::kPFEffectCustomUISuiteVersion2),
	(
		ae::kPFEffectCustomUIOverlayThemeSuite,
		ae::kPFEffectCustomUIOverlayThemeSuiteVersion1,
	),
	(ae::kPFHelperSuite, ae::kPFHelperSuiteVersion1),
	(ae::kPFEffectSequenceDataSuite, ae::kPFEffectSequenceDataSuiteVersion1),
];

/// Try to acquire (and immediately release) every known suite through the
/// host's SPBasicSuite, recording one `suite` event per attempt.
pub unsafe fn probe_suites(in_data: *const ae::PF_InData) {
	let pica = unsafe { (*in_data).pica_basicP };
	if pica.is_null() {
		trace().emit("note", json!({ "msg": "pica_basicP is null; skipping suite probe" }));
		return;
	}

	let (Some(acquire), release) = (unsafe { (*pica).AcquireSuite }, unsafe { (*pica).ReleaseSuite }) else {
		trace().emit(
			"note",
			json!({ "msg": "SPBasicSuite::AcquireSuite is null; skipping suite probe" }),
		);
		return;
	};

	for &(name, version) in SUITES {
		let display_name = String::from_utf8_lossy(&name[..name.len() - 1]).into_owned();
		let mut suite: *const c_void = std::ptr::null();
		let err = unsafe { acquire(name.as_ptr() as *const _, version as i32, &mut suite) };
		let ok = err == 0 && !suite.is_null();

		trace().emit(
			"suite",
			json!({
				"name": display_name,
				"version": version,
				"err": err,
				"ok": ok,
			}),
		);

		if ok && let Some(release) = release {
			unsafe { release(name.as_ptr() as *const _, version as i32) };
		}
	}
}

/// Exercise the inline `PF_UtilCallbacks`: log which entries the host filled
/// in at all, then call the safe ones (ANSI block, host handles) with fixed
/// inputs and record their results.
pub unsafe fn probe_utils(in_data: *const ae::PF_InData) {
	let utils = unsafe { (*in_data).utils };
	if utils.is_null() {
		trace().emit("note", json!({ "msg": "in_data->utils is null; skipping utils probe" }));
		return;
	}
	let u = unsafe { &*utils };

	trace().emit(
		"utils_presence",
		json!({
			"begin_sampling": u.begin_sampling.is_some(),
			"subpixel_sample": u.subpixel_sample.is_some(),
			"area_sample": u.area_sample.is_some(),
			"composite_rect": u.composite_rect.is_some(),
			"blend": u.blend.is_some(),
			"convolve": u.convolve.is_some(),
			"copy": u.copy.is_some(),
			"fill": u.fill.is_some(),
			"gaussian_kernel": u.gaussian_kernel.is_some(),
			"iterate": u.iterate.is_some(),
			"premultiply": u.premultiply.is_some(),
			"new_world": u.new_world.is_some(),
			"dispose_world": u.dispose_world.is_some(),
			"iterate_origin": u.iterate_origin.is_some(),
			"iterate_lut": u.iterate_lut.is_some(),
			"transfer_rect": u.transfer_rect.is_some(),
			"transform_world": u.transform_world.is_some(),
			"host_new_handle": u.host_new_handle.is_some(),
			"host_lock_handle": u.host_lock_handle.is_some(),
			"host_unlock_handle": u.host_unlock_handle.is_some(),
			"host_dispose_handle": u.host_dispose_handle.is_some(),
			"host_get_handle_size": u.host_get_handle_size.is_some(),
			"host_resize_handle": u.host_resize_handle.is_some(),
			"get_callback_addr": u.get_callback_addr.is_some(),
			"app": u.app.is_some(),
			"get_platform_data": u.get_platform_data.is_some(),
			"ansi.sprintf": u.ansi.sprintf.is_some(),
			"ansi.strcpy": u.ansi.strcpy.is_some(),
			"colorCB.RGBtoHLS": u.colorCB.RGBtoHLS.is_some(),
		}),
	);

	unsafe {
		probe_ansi(u);
		probe_handles(u);
	}
}

unsafe fn probe_ansi(u: &ae::_PF_UtilCallbacks) {
	if let Some(sprintf) = u.ansi.sprintf {
		let mut buffer = [0i8; 128];
		let written = unsafe {
			sprintf(
				buffer.as_mut_ptr(),
				c"int=%d str=%s float=%.3f".as_ptr(),
				42i32,
				c"aexlo".as_ptr(),
				2.5f64,
			)
		};
		trace().emit(
			"callback",
			json!({
				"name": "ansi.sprintf",
				"result": written,
				"buffer": crate::inspect::cstr_field(&buffer),
			}),
		);
	}

	if let Some(strcpy) = u.ansi.strcpy {
		let mut buffer = [0i8; 32];
		let returned = unsafe { strcpy(buffer.as_mut_ptr(), c"probe".as_ptr()) };
		trace().emit(
			"callback",
			json!({
				"name": "ansi.strcpy",
				"returned_dst": returned == buffer.as_mut_ptr(),
				"buffer": crate::inspect::cstr_field(&buffer),
			}),
		);
	}

	// A couple of math entries; enough to notice a host wiring them to the
	// wrong libc symbol without dumping the whole table.
	if let (Some(sin), Some(pow), Some(atan2)) = (u.ansi.sin, u.ansi.pow, u.ansi.atan2) {
		trace().emit(
			"callback",
			json!({
				"name": "ansi.math",
				"sin(0.5)": unsafe { sin(0.5) },
				"pow(2,10)": unsafe { pow(2.0, 10.0) },
				"atan2(1,1)": unsafe { atan2(1.0, 1.0) },
			}),
		);
	}
}

unsafe fn probe_handles(u: &ae::_PF_UtilCallbacks) {
	let (Some(new_handle), Some(lock), Some(unlock), Some(dispose)) = (
		u.host_new_handle,
		u.host_lock_handle,
		u.host_unlock_handle,
		u.host_dispose_handle,
	) else {
		trace().emit(
			"callback",
			json!({ "name": "host_handles", "ok": false, "reason": "callbacks missing" }),
		);
		return;
	};

	let mut handle = unsafe { new_handle(64) };
	if handle.is_null() {
		trace().emit(
			"callback",
			json!({ "name": "host_handles", "ok": false, "reason": "host_new_handle returned null" }),
		);
		return;
	}

	let ptr = unsafe { lock(handle) };
	let roundtrip = if ptr.is_null() {
		false
	} else {
		unsafe {
			std::ptr::write_bytes(ptr as *mut u8, 0xA5, 64);
			(ptr as *const u8).read() == 0xA5
		}
	};
	unsafe { unlock(handle) };

	let reported_size = u.host_get_handle_size.map(|get_size| unsafe { get_size(handle) });

	// `resize` may relocate; it updates `handle` in place so the dispose below
	// always sees the current one.
	let mut resized_size = None;
	if let Some(resize) = u.host_resize_handle {
		let err = unsafe { resize(128, &mut handle) };
		if err == 0 {
			resized_size = u.host_get_handle_size.map(|get_size| unsafe { get_size(handle) });
		}
	}

	unsafe { dispose(handle) };

	trace().emit(
		"callback",
		json!({
			"name": "host_handles",
			"ok": true,
			"lock_nonnull": !ptr.is_null(),
			"write_read_roundtrip": roundtrip,
			"size_after_new(64)": reported_size,
			"size_after_resize(128)": resized_size,
		}),
	);
}
