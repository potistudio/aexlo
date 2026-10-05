//! "PF AE App Suite" callbacks.
//!
//! Every callback marshals its FFI arguments and delegates to the installed
//! [`AppHost`](crate::host::app::AppHost) (headless by default; see
//! [`Host::install`](crate::Host::install)). This module owns only the
//! ABI: pointer checks, string/struct conversion and error codes.

use super::ffi::{read_c_str, write_c_str};
use crate::core::diagnostics::diag;
use crate::host::app::{AppPixelF, AppPoint, ProgressId, with_app_host_ffi as with_host};
use after_effects_sys::{
	_PF_AppProgressDialog, A_UTF16Char, A_char, A_long, A_short, PF_App_Color, PF_App_ColorType,
	PF_AppPersonalTextInfo, PF_AppProgressDialogP, PF_Boolean, PF_ContextH, PF_CursorType, PF_Err,
	PF_Err_BAD_CALLBACK_PARAM, PF_Err_NONE, PF_EyeDropperSampleMode, PF_FontName, PF_FontStyleSheet,
	PF_Interrupt_CANCEL, PF_PixelFloat, PF_Point, PF_Rect, PFAppSuite4, PFAppSuite5, PFAppSuite6,
};

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

fn to_ffi_pixel(c: AppPixelF) -> PF_PixelFloat {
	PF_PixelFloat { alpha: c.alpha, red: c.red, green: c.green, blue: c.blue }
}

fn from_ffi_pixel(c: &PF_PixelFloat) -> AppPixelF {
	AppPixelF { alpha: c.alpha, red: c.red, green: c.green, blue: c.blue }
}

/// Progress-dialog handles encode the host's [`ProgressId`] as `id + 1`, so
/// the handle is never null (a null one reads as failure to plugins).
fn progress_handle(id: ProgressId) -> PF_AppProgressDialogP {
	id.0.wrapping_add(1) as usize as *mut _PF_AppProgressDialog
}

fn progress_id(handle: PF_AppProgressDialogP) -> ProgressId {
	ProgressId((handle as usize as u64).wrapping_sub(1))
}

/// Report the host UI language to the plugin.
///
/// The default host leaves the tag empty, which the SDK's localization helper
/// (`AELocalise::GetStringForAE`) treats as "use the base strings".
///
/// This callback must exist even when the host has nothing to say: plugins invoke
/// it through the suite vtable during `PF_Cmd_PARAMS_SETUP`, and a `None` (null)
/// slot there is a hard crash (`blr` through a null pointer), not a graceful no-op.
///
/// # Safety
/// `lang_tagZ` must be null or point to a writable buffer of
/// `PF_APP_LANG_TAG_SIZE` `A_char`s, per the `PF_AppGetLanguage` contract.
unsafe extern "C" fn get_language(lang_tagZ: *mut A_char) -> PF_Err {
	diag!("PFAppSuite/PF_AppGetLanguage",
		"lang_tagZ" => format!("{:#x}", lang_tagZ as usize),
	);

	if lang_tagZ.is_null() {
		return PF_Err_NONE as PF_Err;
	}
	with_host(|host| {
		let buf = unsafe {
			std::slice::from_raw_parts_mut(lang_tagZ, after_effects_sys::PF_APP_LANG_TAG_SIZE as usize)
		};
		write_c_str(buf, &host.language());
		PF_Err_NONE as PF_Err
	})
}

/// Report whether the host is a render engine.
///
/// Some licensing libraries (e.g. the aescripts framework used by DeepGlow2)
/// call this during `PF_Cmd_SEQUENCE_SETUP` to decide whether a "render-only"
/// license applies, so the out-boolean must always be written.
///
/// # Safety
/// `render_enginePB` must be null or point to a writable `PF_Boolean`.
unsafe extern "C" fn is_render_engine(render_enginePB: *mut PF_Boolean) -> PF_Err {
	diag!("PFAppSuite/PF_IsRenderEngine",
		"render_enginePB" => format!("{:#x}", render_enginePB as usize),
	);

	if render_enginePB.is_null() {
		return PF_Err_NONE as PF_Err;
	}
	with_host(|host| {
		unsafe { *render_enginePB = host.is_render_engine() as PF_Boolean };
		PF_Err_NONE as PF_Err
	})
}

unsafe extern "C" fn get_bg_color(bg_colorP: *mut PF_App_Color) -> PF_Err {
	diag!("PFAppSuite/PF_AppGetBgColor",
		"bg_colorP" => format!("{:#x}", bg_colorP as usize),
	);

	if bg_colorP.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}
	with_host(|host| {
		let c = host.bg_color();
		unsafe { *bg_colorP = PF_App_Color { red: c.red, green: c.green, blue: c.blue } };
		PF_Err_NONE as PF_Err
	})
}

unsafe extern "C" fn get_color(color_type: PF_App_ColorType, app_colorP: *mut PF_App_Color) -> PF_Err {
	diag!("PFAppSuite/PF_AppGetColor",
		"color_type" => color_type,
		"app_colorP" => format!("{:#x}", app_colorP as usize),
	);

	if app_colorP.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}
	with_host(|host| {
		let c = host.color(color_type);
		unsafe { *app_colorP = PF_App_Color { red: c.red, green: c.green, blue: c.blue } };
		PF_Err_NONE as PF_Err
	})
}

unsafe extern "C" fn get_personal_info(ptiP: *mut PF_AppPersonalTextInfo) -> PF_Err {
	diag!("PFAppSuite/PF_GetPersonalInfo",
		"ptiP" => format!("{:#x}", ptiP as usize),
	);

	if ptiP.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}
	with_host(|host| {
		let info = host.personal_info();
		let pti = unsafe { &mut *ptiP };
		write_c_str(&mut pti.name, &info.name);
		write_c_str(&mut pti.org, &info.org);
		write_c_str(&mut pti.serial_str, &info.serial);
		PF_Err_NONE as PF_Err
	})
}

/// All out-params are optional (`0` suffix); null ones are skipped.
unsafe extern "C" fn get_font_style_sheet(
	sheet: PF_FontStyleSheet,
	font_nameP0: *mut PF_FontName,
	font_numPS0: *mut A_short,
	sizePS0: *mut A_short,
	stylePS0: *mut A_short,
) -> PF_Err {
	diag!("PFAppSuite/PF_GetFontStyleSheet",
		"sheet" => sheet,
		"font_nameP0" => format!("{:#x}", font_nameP0 as usize),
		"font_numPS0" => format!("{:#x}", font_numPS0 as usize),
		"sizePS0" => format!("{:#x}", sizePS0 as usize),
		"stylePS0" => format!("{:#x}", stylePS0 as usize),
	);

	with_host(|host| {
		let font = host.font_style_sheet(sheet as i32);
		unsafe {
			if let Some(name) = font_nameP0.as_mut() {
				write_c_str(&mut name.font_nameAC, &font.name);
			}
			for (p, v) in [(font_numPS0, font.font_num), (sizePS0, font.size), (stylePS0, font.style)] {
				if let Some(p) = p.as_mut() {
					*p = v;
				}
			}
		}
		PF_Err_NONE as PF_Err
	})
}

unsafe extern "C" fn set_cursor(cursor: PF_CursorType) -> PF_Err {
	diag!("PFAppSuite/PF_SetCursor",
		"cursor" => cursor,
	);

	with_host(|host| {
		host.set_cursor(cursor);
		PF_Err_NONE as PF_Err
	})
}

/// Returns `PF_Interrupt_CANCEL` when the host reports the user cancelled.
unsafe extern "C" fn color_picker_dialog(
	dialog_titleZ0: *const A_char,
	sample_colorP: *const PF_PixelFloat,
	use_ws_to_monitor_xformB: PF_Boolean,
	new_colorP: *mut PF_PixelFloat,
) -> PF_Err {
	diag!("PFAppSuite/PF_AppColorPickerDialog",
		"dialog_titleZ0" => format!("{:#x}", dialog_titleZ0 as usize),
		"sample_colorP" => format!("{:#x}", sample_colorP as usize),
		"use_ws_to_monitor_xformB" => use_ws_to_monitor_xformB,
		"new_colorP" => format!("{:#x}", new_colorP as usize),
	);

	if sample_colorP.is_null() || new_colorP.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}
	with_host(|host| {
		let title = unsafe { read_c_str(dialog_titleZ0) };
		let sample = from_ffi_pixel(unsafe { &*sample_colorP });
		match host.pick_color(title.as_deref(), sample, use_ws_to_monitor_xformB != 0) {
			Some(c) => {
				unsafe { *new_colorP = to_ffi_pixel(c) };
				PF_Err_NONE as PF_Err
			}
			None => PF_Interrupt_CANCEL as PF_Err,
		}
	})
}

unsafe extern "C" fn get_mouse(pointP: *mut PF_Point) -> PF_Err {
	diag!("PFAppSuite/PF_GetMouse",
		"pointP" => format!("{:#x}", pointP as usize),
	);

	if pointP.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}
	with_host(|host| {
		let p = host.mouse();
		unsafe { *pointP = PF_Point { h: p.h, v: p.v } };
		PF_Err_NONE as PF_Err
	})
}

unsafe extern "C" fn invalidate_rect(contextH: PF_ContextH, rectP0: *const PF_Rect) -> PF_Err {
	diag!("PFAppSuite/PF_InvalidateRect",
		"contextH" => format!("{:#x}", contextH as usize),
		"rectP0" => format!("{:#x}", rectP0 as usize),
	);

	with_host(|host| {
		let rect = unsafe { rectP0.as_ref() }.map(|r| (r.left, r.top, r.right, r.bottom));
		host.invalidate_rect(contextH as usize, rect);
		PF_Err_NONE as PF_Err
	})
}

unsafe extern "C" fn convert_local_to_global(localP: *const PF_Point, globalP: *mut PF_Point) -> PF_Err {
	diag!("PFAppSuite/PF_ConvertLocalToGlobal",
		"localP" => format!("{:#x}", localP as usize),
		"globalP" => format!("{:#x}", globalP as usize),
	);

	if localP.is_null() || globalP.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}
	with_host(|host| {
		let l = unsafe { &*localP };
		let g = host.local_to_global(AppPoint { h: l.h, v: l.v });
		unsafe { *globalP = PF_Point { h: g.h, v: g.v } };
		PF_Err_NONE as PF_Err
	})
}

unsafe extern "C" fn get_color_at_global_point(
	globalP: *const PF_Point,
	eyeSize: A_short,
	mode: PF_EyeDropperSampleMode,
	outColorP: *mut PF_PixelFloat,
) -> PF_Err {
	diag!("PFAppSuite/PF_GetColorAtGlobalPoint",
		"globalP" => format!("{:#x}", globalP as usize),
		"eyeSize" => eyeSize,
		"mode" => mode,
		"outColorP" => format!("{:#x}", outColorP as usize),
	);

	if globalP.is_null() || outColorP.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}
	with_host(|host| {
		let g = unsafe { &*globalP };
		let c = host.color_at_global_point(AppPoint { h: g.h, v: g.v }, eyeSize, mode);
		unsafe { *outColorP = to_ffi_pixel(c) };
		PF_Err_NONE as PF_Err
	})
}

unsafe extern "C" fn create_progress_dialog(
	titleZ: *const A_UTF16Char,
	cancel_strZ0: *const A_UTF16Char,
	indeterminateB: PF_Boolean,
	prog_dlgPP: *mut PF_AppProgressDialogP,
) -> PF_Err {
	diag!("PFAppSuite/PF_CreateNewAppProgressDialog",
		"titleZ" => format!("{:#x}", titleZ as usize),
		"cancel_strZ0" => format!("{:#x}", cancel_strZ0 as usize),
		"indeterminateB" => indeterminateB,
		"prog_dlgPP" => format!("{:#x}", prog_dlgPP as usize),
	);

	if prog_dlgPP.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}
	with_host(|host| {
		let title = unsafe { read_utf16(titleZ) }.unwrap_or_default();
		let cancel = unsafe { read_utf16(cancel_strZ0) };
		let id = host.progress_begin(&title, cancel.as_deref(), indeterminateB != 0);
		unsafe { *prog_dlgPP = progress_handle(id) };
		PF_Err_NONE as PF_Err
	})
}

/// Returns `PF_Interrupt_CANCEL` when the host reports the user cancelled.
unsafe extern "C" fn progress_dialog_update(prog_dlgP: PF_AppProgressDialogP, countL: A_long, totalL: A_long) -> PF_Err {
	diag!("PFAppSuite/PF_AppProgressDialogUpdate",
		"prog_dlgP" => format!("{:#x}", prog_dlgP as usize),
		"countL" => countL,
		"totalL" => totalL,
	);

	if prog_dlgP.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}
	with_host(|host| {
		if host.progress_update(progress_id(prog_dlgP), countL, totalL) {
			PF_Err_NONE as PF_Err
		} else {
			PF_Interrupt_CANCEL as PF_Err
		}
	})
}

unsafe extern "C" fn dispose_progress_dialog(prog_dlgP: PF_AppProgressDialogP) -> PF_Err {
	diag!("PFAppSuite/PF_DisposeAppProgressDialog",
		"prog_dlgP" => format!("{:#x}", prog_dlgP as usize),
	);

	if prog_dlgP.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}
	with_host(|host| {
		host.progress_end(progress_id(prog_dlgP));
		PF_Err_NONE as PF_Err
	})
}

// The "PF AE App Suite" tables below.
//
// Every slot is filled; behavior comes from the installed `AppHost` (by
// default `HeadlessAppHost`: neutral answers, UI actions are no-ops).
//
// The three layouts are NOT append-only: `PFAppSuite5` inserts
// `PF_AppGetLanguage` as its third entry, shifting every later slot relative to
// `PFAppSuite4`. Each wire version therefore gets its own exactly-shaped table
// (see the dispatch in `rusty_acquire_suite`). Note the wire version numbers do
// not match the struct names (`kPFAppSuiteVersion4 = 6`, `5 = 7`, `6 = 1`).

/// Build the `PFAppSuite4` table (wire version `kPFAppSuiteVersion4`).
pub(super) const fn create_ae_app_suite_4() -> PFAppSuite4 {
	PFAppSuite4 {
		PF_AppGetBgColor: Some(get_bg_color),
		PF_AppGetColor: Some(get_color),
		PF_GetPersonalInfo: Some(get_personal_info),
		PF_GetFontStyleSheet: Some(get_font_style_sheet),
		PF_SetCursor: Some(set_cursor),
		PF_IsRenderEngine: Some(is_render_engine),
		PF_AppColorPickerDialog: Some(color_picker_dialog),
		PF_GetMouse: Some(get_mouse),
		PF_InvalidateRect: Some(invalidate_rect),
		PF_ConvertLocalToGlobal: Some(convert_local_to_global),
		PF_GetColorAtGlobalPoint: Some(get_color_at_global_point),
	}
}

/// Build the `PFAppSuite5` table (wire version `kPFAppSuiteVersion5`).
pub(super) const fn create_ae_app_suite_5() -> PFAppSuite5 {
	PFAppSuite5 {
		PF_AppGetBgColor: Some(get_bg_color),
		PF_AppGetColor: Some(get_color),
		PF_AppGetLanguage: Some(get_language),
		PF_GetPersonalInfo: Some(get_personal_info),
		PF_GetFontStyleSheet: Some(get_font_style_sheet),
		PF_SetCursor: Some(set_cursor),
		PF_IsRenderEngine: Some(is_render_engine),
		PF_AppColorPickerDialog: Some(color_picker_dialog),
		PF_GetMouse: Some(get_mouse),
		PF_InvalidateRect: Some(invalidate_rect),
		PF_ConvertLocalToGlobal: Some(convert_local_to_global),
		PF_GetColorAtGlobalPoint: Some(get_color_at_global_point),
	}
}

/// Build the `PFAppSuite6` table (wire version `kPFAppSuiteVersion6`).
pub(super) const fn create_ae_app_suite_6() -> PFAppSuite6 {
	PFAppSuite6 {
		PF_AppGetBgColor: Some(get_bg_color),
		PF_AppGetColor: Some(get_color),
		PF_AppGetLanguage: Some(get_language),
		PF_GetPersonalInfo: Some(get_personal_info),
		PF_GetFontStyleSheet: Some(get_font_style_sheet),
		PF_SetCursor: Some(set_cursor),
		PF_IsRenderEngine: Some(is_render_engine),
		PF_AppColorPickerDialog: Some(color_picker_dialog),
		PF_GetMouse: Some(get_mouse),
		PF_InvalidateRect: Some(invalidate_rect),
		PF_ConvertLocalToGlobal: Some(convert_local_to_global),
		PF_GetColorAtGlobalPoint: Some(get_color_at_global_point),
		PF_CreateNewAppProgressDialog: Some(create_progress_dialog),
		PF_AppProgressDialogUpdate: Some(progress_dialog_update),
		PF_DisposeAppProgressDialog: Some(dispose_progress_dialog),
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::host::app::{AppColor, AppHost, HeadlessAppHost, PersonalInfo, TEST_APP_HOST};
	use std::ffi::CStr;

	struct TestHost;
	impl AppHost for TestHost {
		fn bg_color(&self) -> AppColor {
			AppColor { red: 1, green: 2, blue: 3 }
		}
		fn language(&self) -> String {
			"ja_JP".into()
		}
		fn is_render_engine(&self) -> bool {
			true
		}
		fn personal_info(&self) -> PersonalInfo {
			PersonalInfo { name: "x".repeat(100), ..Default::default() }
		}
		fn pick_color(&self, _: Option<&str>, _: AppPixelF, _: bool) -> Option<AppPixelF> {
			None
		}
		fn progress_begin(&self, title: &str, _: Option<&str>, _: bool) -> ProgressId {
			assert_eq!(title, "Hi");
			ProgressId(41)
		}
		fn progress_update(&self, id: ProgressId, count: i32, _: i32) -> bool {
			assert_eq!(id, ProgressId(41));
			count < 10
		}
	}

	fn use_host(host: impl AppHost + 'static) {
		TEST_APP_HOST.set(Some(Box::new(host)));
	}

	#[test]
	fn callbacks_delegate_to_injected_host() {
		use_host(TestHost);
		let suite = create_ae_app_suite_6();
		unsafe {
			let mut c = std::mem::zeroed::<PF_App_Color>();
			suite.PF_AppGetBgColor.unwrap()(&mut c);
			assert_eq!((c.red, c.green, c.blue), (1, 2, 3));

			let mut lang = [0 as A_char; after_effects_sys::PF_APP_LANG_TAG_SIZE as usize];
			suite.PF_AppGetLanguage.unwrap()(lang.as_mut_ptr());
			assert_eq!(CStr::from_ptr(lang.as_ptr()).to_str().unwrap(), "ja_JP");

			let mut b: PF_Boolean = 0;
			suite.PF_IsRenderEngine.unwrap()(&mut b);
			assert_eq!(b, 1);

			let mut pti = std::mem::zeroed::<PF_AppPersonalTextInfo>();
			suite.PF_GetPersonalInfo.unwrap()(&mut pti);
			assert_eq!(CStr::from_ptr(pti.name.as_ptr()).to_bytes().len(), 63);

			let sample = std::mem::zeroed::<PF_PixelFloat>();
			let mut out = sample;
			let err = suite.PF_AppColorPickerDialog.unwrap()(std::ptr::null(), &sample, 0, &mut out);
			assert_eq!(err, PF_Interrupt_CANCEL as PF_Err);

			let title: Vec<u16> = "Hi\0".encode_utf16().collect();
			let mut dlg: PF_AppProgressDialogP = std::ptr::null_mut();
			suite.PF_CreateNewAppProgressDialog.unwrap()(title.as_ptr(), std::ptr::null(), 0, &mut dlg);
			assert!(!dlg.is_null());
			assert_eq!(suite.PF_AppProgressDialogUpdate.unwrap()(dlg, 1, 20), PF_Err_NONE as PF_Err);
			assert_eq!(suite.PF_AppProgressDialogUpdate.unwrap()(dlg, 10, 20), PF_Interrupt_CANCEL as PF_Err);
		}
		use_host(HeadlessAppHost);
		unsafe {
			let mut b: PF_Boolean = 1;
			suite.PF_IsRenderEngine.unwrap()(&mut b);
			assert_eq!(b, 0);
		}
	}
}
