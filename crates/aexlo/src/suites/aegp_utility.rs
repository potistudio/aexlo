//! `AEGP Utility Suite`, wire versions 3 to 13 (`AEGP_UtilitySuite1` to `6`).
//!
//! Each version appends to the one before, except two insertions that move
//! everything after them:
//!
//! | Wire | Suite | Layout |
//! | ---- | ----- | ------ |
//! | 3, 5, 7 | 1, 2, 3 | [`AEGP_UtilitySuite3`] (1 and 2 are prefixes of it) |
//! | 10, 11 | 4, 5 | [`AEGP_UtilitySuite5`]: inserts `AEGP_GetLastErrorMessage` after `AEGP_EndQuietErrors` (4 is a prefix of 5) |
//! | 13 | 6 | [`AEGP_UtilitySuite6`]: inserts `AEGP_ReportInfoUnicode` after `AEGP_ReportInfo` |
//!
//! So each layout gets its own table. A plugin asking for v11 and given the v13
//! table would call `AEGP_ReportInfoUnicode` for `AEGP_GetDriverPluginInitFuncVersion`.
//!
//! aexlo has no application window, palettes, undo stack or script engine.
//! Calls that need none of them are served (registration, versions, quiet
//! errors, ratios, console output). The palette colours are kept for the
//! process, so a set is read back. A call that would need what aexlo lacks
//! reports itself through [`observe::unsupported`](crate::observe::unsupported).

use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Mutex, PoisonError};

use super::ffi::read_c_str;
use crate::core::diagnostics::diag;
use crate::host::app::with_app_host_ffi as with_host;
use crate::observe::{UnsupportedKind, unsupported};
use crate::sys::{
	A_Boolean, A_Err, A_FpLong, A_Ratio, A_UTF16Char, A_char, A_long, A_short, AEGP_ColorVal, AEGP_ErrReportState,
	AEGP_GetPathTypes, AEGP_GetPathTypes_APP, AEGP_GlobalRefcon, AEGP_MemHandle, AEGP_PluginID, AEGP_UtilitySuite3,
	AEGP_UtilitySuite5, AEGP_UtilitySuite6, PF_Err_BAD_CALLBACK_PARAM, PF_Err_INVALID_CALLBACK, PF_Err_NONE,
};

/// `AEGP_INITFUNC_MAJOR_VERSION` / `AEGP_INITFUNC_MINOR_VERSION` of the SDK
/// aexlo is built against (`AE_GeneralPlug.h`).
const INITFUNC_VERSION: (A_short, A_short) = (1, 9);

/// Version of the AEGP driver aexlo reports. After Effects reports the same
/// pair as for the init function.
const IMPLEMENTATION_VERSION: (A_short, A_short) = INITFUNC_VERSION;

/// The largest denominator [`convert_fp_long_to_hsf_ratio`] picks.
const MAX_RATIO_DEN: u64 = 1 << 16;

static NEXT_AEGP_PLUGIN_ID: AtomicI32 = AtomicI32::new(1);

/// Paint and Character palette colours. aexlo shows no palette, so these are
/// only what plugins set, starting from After Effects' defaults.
struct Palettes {
	fore: AEGP_ColorVal,
	back: AEGP_ColorVal,
	fill: AEGP_ColorVal,
	stroke: AEGP_ColorVal,
}

const WHITE: AEGP_ColorVal = AEGP_ColorVal {
	alphaF: 1.0,
	redF: 1.0,
	greenF: 1.0,
	blueF: 1.0,
};
const BLACK: AEGP_ColorVal = AEGP_ColorVal {
	alphaF: 1.0,
	redF: 0.0,
	greenF: 0.0,
	blueF: 0.0,
};

static PALETTES: Mutex<Palettes> = Mutex::new(Palettes {
	fore: WHITE,
	back: BLACK,
	fill: WHITE,
	stroke: BLACK,
});

fn palettes() -> std::sync::MutexGuard<'static, Palettes> {
	PALETTES.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Read a nullable NUL-terminated UTF-16 string.
///
/// # Safety
/// `p` must be null or point to a NUL-terminated `A_UTF16Char` array.
unsafe fn read_utf16(p: *const A_UTF16Char) -> Option<String> {
	if p.is_null() {
		return None;
	}
	let mut len = 0;
	while unsafe { *p.add(len) } != 0 {
		len += 1;
	}
	Some(String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(p, len) }))
}

fn ptr(p: *const impl Sized) -> String {
	format!("{:#x}", p as usize)
}

/// Write `value` through `out` when it is non-null.
///
/// # Safety
/// `out` must be null or valid for a write of `T`.
unsafe fn put<T>(out: *mut T, value: T) -> A_Err {
	match unsafe { out.as_mut() } {
		Some(out) => {
			*out = value;
			PF_Err_NONE
		}
		None => PF_Err_BAD_CALLBACK_PARAM,
	}
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

/// After Effects shows a dialog, or writes to the console when it runs
/// without UI. aexlo has no UI, so it logs the message.
fn report(name: &'static str, plugin_id: AEGP_PluginID, text: Option<String>) -> A_Err {
	let text = text.unwrap_or_default();
	diag!(name, "aegp_plugin_id" => plugin_id, "info" => format!("{text:?}"));
	log::info!("AEGP_ReportInfo ({plugin_id}): {text}");
	PF_Err_NONE
}

unsafe extern "C" fn report_info(aegp_plugin_id: AEGP_PluginID, info_stringZ: *const A_char) -> A_Err {
	crate::core::ffi::guard("AEGP_ReportInfo", || {
		report("AEGPUtilitySuite/AEGP_ReportInfo", aegp_plugin_id, unsafe {
			read_c_str(info_stringZ)
		})
	})
}

unsafe extern "C" fn report_info_unicode(aegp_plugin_id: AEGP_PluginID, info_stringP: *const A_UTF16Char) -> A_Err {
	crate::core::ffi::guard("AEGP_ReportInfoUnicode", || {
		report("AEGPUtilitySuite/AEGP_ReportInfoUnicode", aegp_plugin_id, unsafe {
			read_utf16(info_stringP)
		})
	})
}

unsafe extern "C" fn write_to_os_console(textZ: *const A_char) -> A_Err {
	crate::core::ffi::guard("AEGP_WriteToOSConsole", || {
		let text = unsafe { read_c_str(textZ) }.unwrap_or_default();
		diag!("AEGPUtilitySuite/AEGP_WriteToOSConsole", "textZ" => format!("{text:?}"));
		log::info!("AEGP_WriteToOSConsole: {text}");
		PF_Err_NONE
	})
}

unsafe extern "C" fn write_to_debug_log(
	subsystemZ: *const A_char,
	event_typeZ: *const A_char,
	infoZ: *const A_char,
) -> A_Err {
	crate::core::ffi::guard("AEGP_WriteToDebugLog", || {
		let subsystem = unsafe { read_c_str(subsystemZ) }.unwrap_or_default();
		let event_type = unsafe { read_c_str(event_typeZ) }.unwrap_or_default();
		let info = unsafe { read_c_str(infoZ) }.unwrap_or_default();
		diag!("AEGPUtilitySuite/AEGP_WriteToDebugLog",
			"subsystemZ" => format!("{subsystem:?}"),
			"event_typeZ" => format!("{event_type:?}"),
			"infoZ" => format!("{info:?}"),
		);
		log::debug!("AEGP_WriteToDebugLog [{subsystem}] {event_type}: {info}");
		PF_Err_NONE
	})
}

// ---------------------------------------------------------------------------
// Versions and errors
// ---------------------------------------------------------------------------

unsafe extern "C" fn get_driver_plugin_init_func_version(
	major_versionPS: *mut A_short,
	minor_versionPS: *mut A_short,
) -> A_Err {
	crate::core::ffi::guard("AEGP_GetDriverPluginInitFuncVersion", || {
		diag!("AEGPUtilitySuite/AEGP_GetDriverPluginInitFuncVersion",
			"major_versionPS" => ptr(major_versionPS),
			"minor_versionPS" => ptr(minor_versionPS);
			result: format!("{}.{}", INITFUNC_VERSION.0, INITFUNC_VERSION.1),
		);
		let err = unsafe { put(major_versionPS, INITFUNC_VERSION.0) };
		if err != PF_Err_NONE {
			return err;
		}
		unsafe { put(minor_versionPS, INITFUNC_VERSION.1) }
	})
}

unsafe extern "C" fn get_driver_implementation_version(
	major_versionPS: *mut A_short,
	minor_versionPS: *mut A_short,
) -> A_Err {
	crate::core::ffi::guard("AEGP_GetDriverImplementationVersion", || {
		diag!("AEGPUtilitySuite/AEGP_GetDriverImplementationVersion",
			"major_versionPS" => ptr(major_versionPS),
			"minor_versionPS" => ptr(minor_versionPS);
			result: format!("{}.{}", IMPLEMENTATION_VERSION.0, IMPLEMENTATION_VERSION.1),
		);
		let err = unsafe { put(major_versionPS, IMPLEMENTATION_VERSION.0) };
		if err != PF_Err_NONE {
			return err;
		}
		unsafe { put(minor_versionPS, IMPLEMENTATION_VERSION.1) }
	})
}

/// aexlo never shows an error dialog, so quieting errors only has to hand back
/// a state for the matching [`end_quiet_errors`].
unsafe extern "C" fn start_quiet_errors(err_stateP: *mut AEGP_ErrReportState) -> A_Err {
	crate::core::ffi::guard("AEGP_StartQuietErrors", || {
		diag!("AEGPUtilitySuite/AEGP_StartQuietErrors", "err_stateP" => ptr(err_stateP));
		unsafe { put(err_stateP, AEGP_ErrReportState { reservedAL: [0; 12] }) }
	})
}

unsafe extern "C" fn end_quiet_errors(
	report_quieted_errorsB: A_Boolean,
	err_stateP: *mut AEGP_ErrReportState,
) -> A_Err {
	crate::core::ffi::guard("AEGP_EndQuietErrors", || {
		diag!("AEGPUtilitySuite/AEGP_EndQuietErrors",
			"report_quieted_errorsB" => report_quieted_errorsB,
			"err_stateP" => ptr(err_stateP),
		);
		if err_stateP.is_null() {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		PF_Err_NONE
	})
}

/// aexlo reports no errors through After Effects' error mechanism, so the last
/// one is always "none": an empty message and error 0.
unsafe extern "C" fn get_last_error_message(
	buffer_size: A_long,
	error_string: *mut A_char,
	error_num: *mut A_Err,
) -> A_Err {
	crate::core::ffi::guard("AEGP_GetLastErrorMessage", || {
		diag!("AEGPUtilitySuite/AEGP_GetLastErrorMessage",
			"buffer_size" => buffer_size,
			"error_string" => ptr(error_string),
			"error_num" => ptr(error_num),
		);
		if !error_string.is_null() && buffer_size > 0 {
			unsafe { *error_string = 0 };
		}
		if !error_num.is_null() {
			unsafe { *error_num = PF_Err_NONE };
		}
		PF_Err_NONE
	})
}

// ---------------------------------------------------------------------------
// Undo, windows, idle
// ---------------------------------------------------------------------------

/// aexlo keeps no undo history; there is nothing to group.
unsafe extern "C" fn start_undo_group(undo_nameZ: *const A_char) -> A_Err {
	crate::core::ffi::guard("AEGP_StartUndoGroup", || {
		let name = unsafe { read_c_str(undo_nameZ) }.unwrap_or_default();
		diag!("AEGPUtilitySuite/AEGP_StartUndoGroup", "undo_nameZ" => format!("{name:?}"));
		PF_Err_NONE
	})
}

unsafe extern "C" fn end_undo_group() -> A_Err {
	crate::core::ffi::guard("AEGP_EndUndoGroup", || {
		diag!("AEGPUtilitySuite/AEGP_EndUndoGroup");
		PF_Err_NONE
	})
}

unsafe extern "C" fn register_with_aegp(
	global_refcon: AEGP_GlobalRefcon,
	plugin_nameZ: *const A_char,
	plugin_id: *mut AEGP_PluginID,
) -> A_Err {
	crate::core::ffi::guard("AEGP_RegisterWithAEGP", || {
		let name = unsafe { read_c_str(plugin_nameZ) }.unwrap_or_default();
		diag!("AEGPUtilitySuite/AEGP_RegisterWithAEGP",
			"global_refcon" => ptr(global_refcon),
			"plugin_nameZ" => format!("{name:?}"),
			"plugin_id" => ptr(plugin_id),
		);
		if plugin_id.is_null() {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		let id = NEXT_AEGP_PLUGIN_ID.fetch_add(1, Ordering::Relaxed);
		if !crate::suites::register::register_plugin(id, global_refcon) {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		unsafe { *plugin_id = id };
		PF_Err_NONE
	})
}

/// The window plugins parent dialogs to: the same one `PF_PlatData_MAIN_WND`
/// gives. Only Windows has one.
unsafe extern "C" fn get_main_hwnd(main_hwnd: *mut std::ffi::c_void) -> A_Err {
	crate::core::ffi::guard("AEGP_GetMainHWND", || {
		diag!("AEGPUtilitySuite/AEGP_GetMainHWND", "main_hwnd" => ptr(main_hwnd));
		if main_hwnd.is_null() {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		#[cfg(windows)]
		{
			let hwnd = crate::host::utility::host_window();
			if hwnd.is_null() {
				return PF_Err_BAD_CALLBACK_PARAM;
			}
			// SAFETY: `main_hwnd` is non-null and, per the SDK, the address of an `HWND`.
			unsafe { *(main_hwnd as *mut *mut std::ffi::c_void) = hwnd };
			PF_Err_NONE
		}
		#[cfg(not(windows))]
		{
			PF_Err_BAD_CALLBACK_PARAM
		}
	})
}

/// aexlo shows no floating palettes, so there is nothing to show or hide.
unsafe extern "C" fn show_hide_all_floaters(include_tool_palB: A_Boolean) -> A_Err {
	crate::core::ffi::guard("AEGP_ShowHideAllFloaters", || {
		diag!("AEGPUtilitySuite/AEGP_ShowHideAllFloaters", "include_tool_palB" => include_tool_palB);
		PF_Err_NONE
	})
}

/// Idle hooks run at the boundaries the host controls (see
/// [`register::dispatch_idle`](crate::suites::register::dispatch_idle)); the
/// SDK only promises they run some time after this returns.
unsafe extern "C" fn cause_idle_routines_to_be_called() -> A_Err {
	crate::core::ffi::guard("AEGP_CauseIdleRoutinesToBeCalled", || {
		diag!("AEGPUtilitySuite/AEGP_CauseIdleRoutinesToBeCalled");
		PF_Err_NONE
	})
}

/// UI is suppressed when the host says it is a render engine, as `aerender`
/// suppresses it.
unsafe extern "C" fn get_suppress_interactive_ui(ui_is_suppressedPB: *mut A_Boolean) -> A_Err {
	crate::core::ffi::guard("AEGP_GetSuppressInteractiveUI", || {
		diag!("AEGPUtilitySuite/AEGP_GetSuppressInteractiveUI", "ui_is_suppressedPB" => ptr(ui_is_suppressedPB));
		if ui_is_suppressedPB.is_null() {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		with_host(|host| {
			unsafe { *ui_is_suppressedPB = host.is_render_engine() as A_Boolean };
			PF_Err_NONE
		})
	})
}

unsafe extern "C" fn host_is_activated(is_activatedPB: *mut A_Boolean) -> A_Err {
	crate::core::ffi::guard("AEGP_HostIsActivated", || {
		diag!("AEGPUtilitySuite/AEGP_HostIsActivated", "is_activatedPB" => ptr(is_activatedPB); result: true);
		unsafe { put(is_activatedPB, 1) }
	})
}

/// aexlo does not keep a font list; text is set with the font the host gives.
unsafe extern "C" fn update_font_list() -> A_Err {
	crate::core::ffi::guard("AEGP_UpdateFontList", || {
		diag!("AEGPUtilitySuite/AEGP_UpdateFontList");
		PF_Err_NONE
	})
}

// ---------------------------------------------------------------------------
// Palettes
// ---------------------------------------------------------------------------

unsafe extern "C" fn paint_pal_get_fore_color(fore_colorP: *mut AEGP_ColorVal) -> A_Err {
	crate::core::ffi::guard("AEGP_PaintPalGetForeColor", || {
		diag!("AEGPUtilitySuite/AEGP_PaintPalGetForeColor", "fore_colorP" => ptr(fore_colorP));
		unsafe { put(fore_colorP, palettes().fore) }
	})
}

unsafe extern "C" fn paint_pal_get_back_color(back_colorP: *mut AEGP_ColorVal) -> A_Err {
	crate::core::ffi::guard("AEGP_PaintPalGetBackColor", || {
		diag!("AEGPUtilitySuite/AEGP_PaintPalGetBackColor", "back_colorP" => ptr(back_colorP));
		unsafe { put(back_colorP, palettes().back) }
	})
}

/// Store `*color` in the palette slot `pick` selects.
///
/// # Safety
/// `color` must be null or point to a readable `AEGP_ColorVal`.
unsafe fn set_palette(color: *const AEGP_ColorVal, pick: fn(&mut Palettes) -> &mut AEGP_ColorVal) -> A_Err {
	let Some(color) = (unsafe { color.as_ref() }) else {
		return PF_Err_BAD_CALLBACK_PARAM;
	};
	*pick(&mut palettes()) = *color;
	PF_Err_NONE
}

unsafe extern "C" fn paint_pal_set_fore_color(fore_colorP: *const AEGP_ColorVal) -> A_Err {
	crate::core::ffi::guard("AEGP_PaintPalSetForeColor", || {
		diag!("AEGPUtilitySuite/AEGP_PaintPalSetForeColor", "fore_colorP" => ptr(fore_colorP));
		unsafe { set_palette(fore_colorP, |p| &mut p.fore) }
	})
}

unsafe extern "C" fn paint_pal_set_back_color(back_colorP: *const AEGP_ColorVal) -> A_Err {
	crate::core::ffi::guard("AEGP_PaintPalSetBackColor", || {
		diag!("AEGPUtilitySuite/AEGP_PaintPalSetBackColor", "back_colorP" => ptr(back_colorP));
		unsafe { set_palette(back_colorP, |p| &mut p.back) }
	})
}

unsafe extern "C" fn char_pal_get_fill_color(
	is_fill_color_definedPB: *mut A_Boolean,
	fill_colorP: *mut AEGP_ColorVal,
) -> A_Err {
	crate::core::ffi::guard("AEGP_CharPalGetFillColor", || {
		diag!("AEGPUtilitySuite/AEGP_CharPalGetFillColor",
			"is_fill_color_definedPB" => ptr(is_fill_color_definedPB),
			"fill_colorP" => ptr(fill_colorP),
		);
		let err = unsafe { put(is_fill_color_definedPB, 1) };
		if err != PF_Err_NONE {
			return err;
		}
		unsafe { put(fill_colorP, palettes().fill) }
	})
}

unsafe extern "C" fn char_pal_get_stroke_color(
	is_stroke_color_definedPB: *mut A_Boolean,
	stroke_colorP: *mut AEGP_ColorVal,
) -> A_Err {
	crate::core::ffi::guard("AEGP_CharPalGetStrokeColor", || {
		diag!("AEGPUtilitySuite/AEGP_CharPalGetStrokeColor",
			"is_stroke_color_definedPB" => ptr(is_stroke_color_definedPB),
			"stroke_colorP" => ptr(stroke_colorP),
		);
		let err = unsafe { put(is_stroke_color_definedPB, 1) };
		if err != PF_Err_NONE {
			return err;
		}
		unsafe { put(stroke_colorP, palettes().stroke) }
	})
}

unsafe extern "C" fn char_pal_set_fill_color(fill_colorP: *const AEGP_ColorVal) -> A_Err {
	crate::core::ffi::guard("AEGP_CharPalSetFillColor", || {
		diag!("AEGPUtilitySuite/AEGP_CharPalSetFillColor", "fill_colorP" => ptr(fill_colorP));
		unsafe { set_palette(fill_colorP, |p| &mut p.fill) }
	})
}

unsafe extern "C" fn char_pal_set_stroke_color(stroke_colorP: *const AEGP_ColorVal) -> A_Err {
	crate::core::ffi::guard("AEGP_CharPalSetStrokeColor", || {
		diag!("AEGPUtilitySuite/AEGP_CharPalSetStrokeColor", "stroke_colorP" => ptr(stroke_colorP));
		unsafe { set_palette(stroke_colorP, |p| &mut p.stroke) }
	})
}

/// The fill colour, After Effects' default.
unsafe extern "C" fn char_pal_is_fill_color_ui_frontmost(is_fill_color_selectedPB: *mut A_Boolean) -> A_Err {
	crate::core::ffi::guard("AEGP_CharPalIsFillColorUIFrontmost", || {
		diag!("AEGPUtilitySuite/AEGP_CharPalIsFillColorUIFrontmost",
			"is_fill_color_selectedPB" => ptr(is_fill_color_selectedPB);
			result: true,
		);
		unsafe { put(is_fill_color_selectedPB, 1) }
	})
}

// ---------------------------------------------------------------------------
// Ratios
// ---------------------------------------------------------------------------

/// The closest `num / den` to `x` with `den <= MAX_RATIO_DEN`, found from the
/// continued fraction of `x`. `None` when `x` is not finite or does not fit an
/// `A_long`.
fn ratio_from_f64(x: f64) -> Option<A_Ratio> {
	if !x.is_finite() || x.abs() > A_long::MAX as f64 {
		return None;
	}
	let negative = x < 0.0;
	let mut rest = x.abs();
	// Convergents h/k, starting from 1/0 and 0/1.
	let (mut h0, mut h1) = (0u64, 1u64);
	let (mut k0, mut k1) = (1u64, 0u64);
	loop {
		let a = rest.floor();
		let a_int = a as u64;
		let h = a_int.checked_mul(h1).and_then(|v| v.checked_add(h0));
		let k = a_int.checked_mul(k1).and_then(|v| v.checked_add(k0));
		let (Some(h), Some(k)) = (h, k) else { break };
		if k > MAX_RATIO_DEN || h > A_long::MAX as u64 {
			break;
		}
		(h0, h1, k0, k1) = (h1, h, k1, k);
		let frac = rest - a;
		if frac < 1e-12 {
			break;
		}
		rest = 1.0 / frac;
	}
	if k1 == 0 {
		return None;
	}
	let num = h1 as A_long;
	Some(A_Ratio {
		num: if negative { -num } else { num },
		den: k1 as _,
	})
}

unsafe extern "C" fn convert_fp_long_to_hsf_ratio(numberF: A_FpLong, ratioPR: *mut A_Ratio) -> A_Err {
	crate::core::ffi::guard("AEGP_ConvertFpLongToHSFRatio", || {
		let ratio = ratio_from_f64(numberF);
		diag!("AEGPUtilitySuite/AEGP_ConvertFpLongToHSFRatio",
			"numberF" => numberF,
			"ratioPR" => ptr(ratioPR);
			result: ratio.map_or_else(|| "none".to_string(), |r| format!("{}/{}", r.num, r.den)),
		);
		match ratio {
			Some(ratio) => unsafe { put(ratioPR, ratio) },
			None => PF_Err_BAD_CALLBACK_PARAM,
		}
	})
}

unsafe extern "C" fn convert_hsf_ratio_to_fp_long(ratioR: A_Ratio, numberPF: *mut A_FpLong) -> A_Err {
	crate::core::ffi::guard("AEGP_ConvertHSFRatioToFpLong", || {
		diag!("AEGPUtilitySuite/AEGP_ConvertHSFRatioToFpLong",
			"ratioR" => format!("{}/{}", ratioR.num, ratioR.den),
			"numberPF" => ptr(numberPF),
		);
		if ratioR.den == 0 {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		unsafe { put(numberPF, ratioR.num as A_FpLong / ratioR.den as A_FpLong) }
	})
}

// ---------------------------------------------------------------------------
// Scripting and plugin locations
// ---------------------------------------------------------------------------

/// aexlo has no ExtendScript engine, and says so.
unsafe extern "C" fn is_scripting_available(outAvailablePB: *mut A_Boolean) -> A_Err {
	crate::core::ffi::guard("AEGP_IsScriptingAvailable", || {
		diag!("AEGPUtilitySuite/AEGP_IsScriptingAvailable", "outAvailablePB" => ptr(outAvailablePB); result: false);
		unsafe { put(outAvailablePB, 0) }
	})
}

unsafe extern "C" fn execute_script(
	inPlugin_id: AEGP_PluginID,
	inScriptZ: *const A_char,
	platform_encodingB: A_Boolean,
	outResultPH0: *mut AEGP_MemHandle,
	outErrorStringPH0: *mut AEGP_MemHandle,
) -> A_Err {
	crate::core::ffi::guard("AEGP_ExecuteScript", || {
		let script = unsafe { read_c_str(inScriptZ) }.unwrap_or_default();
		diag!("AEGPUtilitySuite/AEGP_ExecuteScript",
			"inPlugin_id" => inPlugin_id,
			"inScriptZ" => format!("{script:?}"),
			"platform_encodingB" => platform_encodingB,
		);
		for out in [outResultPH0, outErrorStringPH0] {
			if !out.is_null() {
				unsafe { *out = std::ptr::null_mut() };
			}
		}
		unsupported(
			UnsupportedKind::Stub,
			"AEGPUtilitySuite/AEGP_ExecuteScript",
			format!("{} bytes of script", script.len()),
		);
		// The script has not run: success would have the plugin read a result.
		PF_Err_INVALID_CALLBACK
	})
}

/// A `CFBundleRef` to the plugin on macOS. aexlo does not create one yet.
/// Windows gets null, as from After Effects.
unsafe extern "C" fn get_plugin_platform_ref(plug_id: AEGP_PluginID, plat_refPPV: *mut *mut std::ffi::c_void) -> A_Err {
	crate::core::ffi::guard("AEGP_GetPluginPlatformRef", || {
		diag!("AEGPUtilitySuite/AEGP_GetPluginPlatformRef",
			"plug_id" => plug_id,
			"plat_refPPV" => ptr(plat_refPPV),
		);
		let err = unsafe { put(plat_refPPV, std::ptr::null_mut()) };
		#[cfg(not(windows))]
		unsupported(UnsupportedKind::Stub, "AEGPUtilitySuite/AEGP_GetPluginPlatformRef", "");
		err
	})
}

/// `APP` is the executable hosting aexlo. aexlo has no plugin folders, and
/// After Effects itself does not implement `PLUGIN`.
unsafe extern "C" fn get_plugin_paths(
	aegp_plugin_id: AEGP_PluginID,
	path_type: AEGP_GetPathTypes,
	unicode_pathPH: *mut AEGP_MemHandle,
) -> A_Err {
	crate::core::ffi::guard("AEGP_GetPluginPaths", || {
		diag!("AEGPUtilitySuite/AEGP_GetPluginPaths",
			"aegp_plugin_id" => aegp_plugin_id,
			"path_type" => path_type,
			"unicode_pathPH" => ptr(unicode_pathPH),
		);
		if unicode_pathPH.is_null() {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		if path_type != AEGP_GetPathTypes_APP as AEGP_GetPathTypes {
			unsupported(
				UnsupportedKind::Declined,
				"AEGPUtilitySuite/AEGP_GetPluginPaths",
				format!("path_type={path_type}"),
			);
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		let Ok(exe) = std::env::current_exe() else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		let bytes = utf16_bytes(&exe.to_string_lossy());
		unsafe { *unicode_pathPH = super::aegp_memory::handle_from_bytes(bytes) };
		PF_Err_NONE
	})
}

/// `text` as NUL-terminated UTF-16 in native byte order.
fn utf16_bytes(text: &str) -> Vec<u8> {
	text.encode_utf16()
		.chain(std::iter::once(0))
		.flat_map(u16::to_ne_bytes)
		.collect()
}

// ---------------------------------------------------------------------------
// Tables
// ---------------------------------------------------------------------------

/// Wire versions 3, 5 and 7.
pub(super) const fn suite_3() -> AEGP_UtilitySuite3 {
	AEGP_UtilitySuite3 {
		AEGP_ReportInfo: Some(report_info),
		AEGP_GetDriverPluginInitFuncVersion: Some(get_driver_plugin_init_func_version),
		AEGP_GetDriverImplementationVersion: Some(get_driver_implementation_version),
		AEGP_StartQuietErrors: Some(start_quiet_errors),
		AEGP_EndQuietErrors: Some(end_quiet_errors),
		AEGP_StartUndoGroup: Some(start_undo_group),
		AEGP_EndUndoGroup: Some(end_undo_group),
		AEGP_RegisterWithAEGP: Some(register_with_aegp),
		AEGP_GetMainHWND: Some(get_main_hwnd),
		AEGP_ShowHideAllFloaters: Some(show_hide_all_floaters),
		AEGP_PaintPalGetForeColor: Some(paint_pal_get_fore_color),
		AEGP_PaintPalGetBackColor: Some(paint_pal_get_back_color),
		AEGP_PaintPalSetForeColor: Some(paint_pal_set_fore_color),
		AEGP_PaintPalSetBackColor: Some(paint_pal_set_back_color),
		AEGP_CharPalGetFillColor: Some(char_pal_get_fill_color),
		AEGP_CharPalGetStrokeColor: Some(char_pal_get_stroke_color),
		AEGP_CharPalSetFillColor: Some(char_pal_set_fill_color),
		AEGP_CharPalSetStrokeColor: Some(char_pal_set_stroke_color),
		AEGP_CharPalIsFillColorUIFrontmost: Some(char_pal_is_fill_color_ui_frontmost),
		AEGP_ConvertFpLongToHSFRatio: Some(convert_fp_long_to_hsf_ratio),
		AEGP_ConvertHSFRatioToFpLong: Some(convert_hsf_ratio_to_fp_long),
		AEGP_CauseIdleRoutinesToBeCalled: Some(cause_idle_routines_to_be_called),
		AEGP_GetSuppressInteractiveUI: Some(get_suppress_interactive_ui),
		AEGP_WriteToOSConsole: Some(write_to_os_console),
		AEGP_WriteToDebugLog: Some(write_to_debug_log),
	}
}

/// Wire versions 10 and 11.
pub(super) const fn suite_5() -> AEGP_UtilitySuite5 {
	AEGP_UtilitySuite5 {
		AEGP_ReportInfo: Some(report_info),
		AEGP_GetDriverPluginInitFuncVersion: Some(get_driver_plugin_init_func_version),
		AEGP_GetDriverImplementationVersion: Some(get_driver_implementation_version),
		AEGP_StartQuietErrors: Some(start_quiet_errors),
		AEGP_EndQuietErrors: Some(end_quiet_errors),
		AEGP_GetLastErrorMessage: Some(get_last_error_message),
		AEGP_StartUndoGroup: Some(start_undo_group),
		AEGP_EndUndoGroup: Some(end_undo_group),
		AEGP_RegisterWithAEGP: Some(register_with_aegp),
		AEGP_GetMainHWND: Some(get_main_hwnd),
		AEGP_ShowHideAllFloaters: Some(show_hide_all_floaters),
		AEGP_PaintPalGetForeColor: Some(paint_pal_get_fore_color),
		AEGP_PaintPalGetBackColor: Some(paint_pal_get_back_color),
		AEGP_PaintPalSetForeColor: Some(paint_pal_set_fore_color),
		AEGP_PaintPalSetBackColor: Some(paint_pal_set_back_color),
		AEGP_CharPalGetFillColor: Some(char_pal_get_fill_color),
		AEGP_CharPalGetStrokeColor: Some(char_pal_get_stroke_color),
		AEGP_CharPalSetFillColor: Some(char_pal_set_fill_color),
		AEGP_CharPalSetStrokeColor: Some(char_pal_set_stroke_color),
		AEGP_CharPalIsFillColorUIFrontmost: Some(char_pal_is_fill_color_ui_frontmost),
		AEGP_ConvertFpLongToHSFRatio: Some(convert_fp_long_to_hsf_ratio),
		AEGP_ConvertHSFRatioToFpLong: Some(convert_hsf_ratio_to_fp_long),
		AEGP_CauseIdleRoutinesToBeCalled: Some(cause_idle_routines_to_be_called),
		AEGP_GetSuppressInteractiveUI: Some(get_suppress_interactive_ui),
		AEGP_WriteToOSConsole: Some(write_to_os_console),
		AEGP_WriteToDebugLog: Some(write_to_debug_log),
		AEGP_IsScriptingAvailable: Some(is_scripting_available),
		AEGP_ExecuteScript: Some(execute_script),
		AEGP_HostIsActivated: Some(host_is_activated),
		AEGP_GetPluginPlatformRef: Some(get_plugin_platform_ref),
		AEGP_UpdateFontList: Some(update_font_list),
	}
}

/// Wire version 13.
pub(super) const fn suite_6() -> AEGP_UtilitySuite6 {
	AEGP_UtilitySuite6 {
		AEGP_ReportInfo: Some(report_info),
		AEGP_ReportInfoUnicode: Some(report_info_unicode),
		AEGP_GetDriverPluginInitFuncVersion: Some(get_driver_plugin_init_func_version),
		AEGP_GetDriverImplementationVersion: Some(get_driver_implementation_version),
		AEGP_StartQuietErrors: Some(start_quiet_errors),
		AEGP_EndQuietErrors: Some(end_quiet_errors),
		AEGP_GetLastErrorMessage: Some(get_last_error_message),
		AEGP_StartUndoGroup: Some(start_undo_group),
		AEGP_EndUndoGroup: Some(end_undo_group),
		AEGP_RegisterWithAEGP: Some(register_with_aegp),
		AEGP_GetMainHWND: Some(get_main_hwnd),
		AEGP_ShowHideAllFloaters: Some(show_hide_all_floaters),
		AEGP_PaintPalGetForeColor: Some(paint_pal_get_fore_color),
		AEGP_PaintPalGetBackColor: Some(paint_pal_get_back_color),
		AEGP_PaintPalSetForeColor: Some(paint_pal_set_fore_color),
		AEGP_PaintPalSetBackColor: Some(paint_pal_set_back_color),
		AEGP_CharPalGetFillColor: Some(char_pal_get_fill_color),
		AEGP_CharPalGetStrokeColor: Some(char_pal_get_stroke_color),
		AEGP_CharPalSetFillColor: Some(char_pal_set_fill_color),
		AEGP_CharPalSetStrokeColor: Some(char_pal_set_stroke_color),
		AEGP_CharPalIsFillColorUIFrontmost: Some(char_pal_is_fill_color_ui_frontmost),
		AEGP_ConvertFpLongToHSFRatio: Some(convert_fp_long_to_hsf_ratio),
		AEGP_ConvertHSFRatioToFpLong: Some(convert_hsf_ratio_to_fp_long),
		AEGP_CauseIdleRoutinesToBeCalled: Some(cause_idle_routines_to_be_called),
		AEGP_GetSuppressInteractiveUI: Some(get_suppress_interactive_ui),
		AEGP_WriteToOSConsole: Some(write_to_os_console),
		AEGP_WriteToDebugLog: Some(write_to_debug_log),
		AEGP_IsScriptingAvailable: Some(is_scripting_available),
		AEGP_ExecuteScript: Some(execute_script),
		AEGP_HostIsActivated: Some(host_is_activated),
		AEGP_GetPluginPlatformRef: Some(get_plugin_platform_ref),
		AEGP_UpdateFontList: Some(update_font_list),
		AEGP_GetPluginPaths: Some(get_plugin_paths),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn layouts_put_register_where_each_version_expects_it() {
		// The slot index of AEGP_RegisterWithAEGP: 7 before GetLastErrorMessage
		// was inserted, 8 after, 9 once ReportInfoUnicode was.
		let slot = |offset: usize| offset / std::mem::size_of::<usize>();
		assert_eq!(slot(std::mem::offset_of!(AEGP_UtilitySuite3, AEGP_RegisterWithAEGP)), 7);
		assert_eq!(slot(std::mem::offset_of!(AEGP_UtilitySuite5, AEGP_RegisterWithAEGP)), 8);
		assert_eq!(slot(std::mem::offset_of!(AEGP_UtilitySuite6, AEGP_RegisterWithAEGP)), 9);
		assert_eq!(slot(std::mem::offset_of!(AEGP_UtilitySuite5, AEGP_ExecuteScript)), 27);
		assert_eq!(slot(std::mem::offset_of!(AEGP_UtilitySuite6, AEGP_ExecuteScript)), 28);
	}

	#[test]
	fn versions_are_the_sdk_init_func_version() {
		let (mut major, mut minor) = (0, 0);
		assert_eq!(
			unsafe { get_driver_plugin_init_func_version(&mut major, &mut minor) },
			PF_Err_NONE
		);
		assert_eq!((major, minor), (1, 9));
		assert_ne!(
			unsafe { get_driver_plugin_init_func_version(std::ptr::null_mut(), &mut minor) },
			PF_Err_NONE
		);
	}

	#[test]
	fn ratios_round_trip() {
		for x in [0.0, 1.0, -2.5, 1.0 / 3.0, 29.97, 23.976, 1000.125] {
			let mut r = A_Ratio { num: 0, den: 0 };
			assert_eq!(unsafe { convert_fp_long_to_hsf_ratio(x, &mut r) }, PF_Err_NONE, "{x}");
			let mut back = 0.0;
			assert_eq!(unsafe { convert_hsf_ratio_to_fp_long(r, &mut back) }, PF_Err_NONE);
			assert!((back - x).abs() < 1e-6, "{x} -> {}/{} -> {back}", r.num, r.den);
		}
		let r = ratio_from_f64(0.5).unwrap();
		assert_eq!((r.num, r.den), (1, 2));
		assert!(ratio_from_f64(f64::NAN).is_none());
		assert!(ratio_from_f64(1e12).is_none());
		let zero_den = A_Ratio { num: 1, den: 0 };
		let mut out = 0.0;
		assert_ne!(unsafe { convert_hsf_ratio_to_fp_long(zero_den, &mut out) }, PF_Err_NONE);
	}

	#[test]
	fn palette_colors_read_back_what_was_set() {
		let red = AEGP_ColorVal {
			alphaF: 1.0,
			redF: 1.0,
			greenF: 0.0,
			blueF: 0.0,
		};
		let mut before = BLACK;
		assert_eq!(unsafe { paint_pal_get_back_color(&mut before) }, PF_Err_NONE);
		assert_eq!(unsafe { paint_pal_set_back_color(&red) }, PF_Err_NONE);
		let mut got = BLACK;
		assert_eq!(unsafe { paint_pal_get_back_color(&mut got) }, PF_Err_NONE);
		assert_eq!((got.redF, got.greenF), (1.0, 0.0));
		assert_eq!(unsafe { paint_pal_set_back_color(&before) }, PF_Err_NONE);
	}

	#[test]
	fn last_error_is_none() {
		let mut buf = [b'x' as A_char; 8];
		let mut num: A_Err = 7;
		assert_eq!(
			unsafe { get_last_error_message(8, buf.as_mut_ptr(), &mut num) },
			PF_Err_NONE
		);
		assert_eq!((buf[0], num), (0, PF_Err_NONE));
	}

	#[test]
	fn scripting_is_unavailable() {
		let mut available: A_Boolean = 1;
		assert_eq!(unsafe { is_scripting_available(&mut available) }, PF_Err_NONE);
		assert_eq!(available, 0);
	}

	#[test]
	fn execute_script_does_not_claim_to_have_run() {
		let mut result = 1usize as AEGP_MemHandle;
		let mut error = 1usize as AEGP_MemHandle;
		assert_eq!(
			unsafe { execute_script(0, c"1".as_ptr().cast(), 0, &mut result, &mut error) },
			PF_Err_INVALID_CALLBACK
		);
		assert!(result.is_null() && error.is_null());
	}

	#[test]
	fn registers_a_plugin_and_returns_the_main_window() {
		let owner = crate::suites::register::new_owner();
		let _scope = crate::suites::register::enter_owner(owner);
		let mut id = 0;
		let err = unsafe { register_with_aegp(std::ptr::null_mut(), c"Venustas".as_ptr().cast(), &mut id) };
		assert_eq!(err, PF_Err_NONE);
		assert_ne!(id, 0);

		#[cfg(windows)]
		{
			let sentinel = 0x1234 as *mut std::ffi::c_void;
			crate::host::utility::set_main_window(sentinel);
			let mut hwnd: *mut std::ffi::c_void = std::ptr::null_mut();
			let err = unsafe { get_main_hwnd((&raw mut hwnd).cast()) };
			crate::host::utility::set_main_window(std::ptr::null_mut());
			assert_eq!(err, PF_Err_NONE);
			assert_eq!(hwnd, sentinel);
		}
	}

	#[test]
	fn app_path_is_utf16() {
		let mut handle: AEGP_MemHandle = std::ptr::null_mut();
		assert_eq!(
			unsafe { get_plugin_paths(1, AEGP_GetPathTypes_APP as _, &mut handle) },
			PF_Err_NONE
		);
		assert!(!handle.is_null());
		let bytes = utf16_bytes("ab");
		assert_eq!(bytes.len(), 6);
		assert_ne!(unsafe { get_plugin_paths(1, 1, &mut handle) }, PF_Err_NONE);
	}
}
