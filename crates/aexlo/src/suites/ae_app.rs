use after_effects_sys::{
	_PF_AppProgressDialog, A_UTF16Char, A_char, A_long, A_short, PF_App_Color, PF_App_ColorType,
	PF_AppPersonalTextInfo, PF_AppProgressDialogP, PF_Boolean, PF_ContextH, PF_CursorType, PF_Err,
	PF_Err_BAD_CALLBACK_PARAM, PF_Err_NONE, PF_EyeDropperSampleMode, PF_FontName, PF_FontStyleSheet,
	PF_PixelFloat, PF_Point, PF_Rect, PFAppSuite4, PFAppSuite5, PFAppSuite6,
};

/// Report the host UI language to the plugin.
///
/// We advertise "no specific language" by leaving the caller's (pre-zeroed)
/// buffer empty. The SDK's localization helper (`AELocalise::GetStringForAE`)
/// treats an empty tag as "use the base strings", so plugins fall back to their
/// built-in (English) resource strings.
///
/// This callback must exist even though it does almost nothing: plugins invoke it
/// through the suite vtable during `PF_Cmd_PARAMS_SETUP`, and a `None` (null) slot
/// there is a hard crash (`blr` through a null pointer), not a graceful no-op.
///
/// # Safety
/// `lang_tagZ` must be null or point to a writable `A_char` buffer of at least one
/// element, per the `PF_AppGetLanguage` contract.
unsafe extern "C" fn get_language(lang_tagZ: *mut A_char) -> PF_Err {
	if !lang_tagZ.is_null() {
		unsafe { *lang_tagZ = 0 };
	}

	PF_Err_NONE as PF_Err
}

/// Report that we are *not* a render engine.
///
/// `PF_IsRenderEngine` returns TRUE when the host is the command-line renderer
/// (`aerender`), a UI-less/watch-folder instance, etc. We are an interactive-style
/// host, so we always answer FALSE. Some licensing libraries (e.g. the aescripts
/// framework used by DeepGlow2) call this during `PF_Cmd_SEQUENCE_SETUP` to decide
/// whether a "render-only" license applies; if the out-boolean is left
/// uninitialized they may take the wrong branch.
///
/// # Safety
/// `render_enginePB` must be null or point to a writable `PF_Boolean`, per the
/// `PF_IsRenderEngine` contract.
unsafe extern "C" fn is_render_engine(render_enginePB: *mut PF_Boolean) -> PF_Err {
	if !render_enginePB.is_null() {
		unsafe { *render_enginePB = 0 };
	}

	PF_Err_NONE as PF_Err
}

/// Neutral mid-gray reported for every UI color: there is no host UI to theme.
const UI_GRAY: PF_App_Color = PF_App_Color { red: 0x8000, green: 0x8000, blue: 0x8000 };

/// Report the panel background color (neutral gray; there is no host UI).
unsafe extern "C" fn get_bg_color(bg_colorP: *mut PF_App_Color) -> PF_Err {
	if bg_colorP.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}
	unsafe { *bg_colorP = UI_GRAY };
	PF_Err_NONE as PF_Err
}

/// Report a themed UI color. Every `PF_App_ColorType` maps to the same gray.
unsafe extern "C" fn get_color(_color_type: PF_App_ColorType, app_colorP: *mut PF_App_Color) -> PF_Err {
	unsafe { get_bg_color(app_colorP) }
}

/// Report the registered user. We have none, so every string is empty.
unsafe extern "C" fn get_personal_info(ptiP: *mut PF_AppPersonalTextInfo) -> PF_Err {
	if ptiP.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}
	// SAFETY: plain `A_char` arrays; all-zero is three empty C strings.
	unsafe { *ptiP = std::mem::zeroed() };
	PF_Err_NONE as PF_Err
}

/// Report a UI font. All out-params are optional (`0` suffix); we report an
/// empty name and zero metrics, i.e. "use your default".
unsafe extern "C" fn get_font_style_sheet(
	_sheet: PF_FontStyleSheet,
	font_nameP0: *mut PF_FontName,
	font_numPS0: *mut A_short,
	sizePS0: *mut A_short,
	stylePS0: *mut A_short,
) -> PF_Err {
	unsafe {
		if !font_nameP0.is_null() {
			*font_nameP0 = std::mem::zeroed();
		}
		for p in [font_numPS0, sizePS0, stylePS0] {
			if !p.is_null() {
				*p = 0;
			}
		}
	}
	PF_Err_NONE as PF_Err
}

/// Set the mouse cursor. No UI, so nothing to do.
unsafe extern "C" fn set_cursor(_cursor: PF_CursorType) -> PF_Err {
	PF_Err_NONE as PF_Err
}

/// Show a color picker. Headless, so the "user" immediately accepts the
/// sample color unchanged.
unsafe extern "C" fn color_picker_dialog(
	_dialog_titleZ0: *const A_char,
	sample_colorP: *const PF_PixelFloat,
	_use_ws_to_monitor_xformB: PF_Boolean,
	new_colorP: *mut PF_PixelFloat,
) -> PF_Err {
	if sample_colorP.is_null() || new_colorP.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}
	unsafe { *new_colorP = *sample_colorP };
	PF_Err_NONE as PF_Err
}

/// Report the mouse position. There is no pointer; report the origin.
unsafe extern "C" fn get_mouse(pointP: *mut PF_Point) -> PF_Err {
	if pointP.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}
	unsafe { *pointP = PF_Point { h: 0, v: 0 } };
	PF_Err_NONE as PF_Err
}

/// Request a UI redraw. Nothing is drawn, so this is a no-op.
unsafe extern "C" fn invalidate_rect(_contextH: PF_ContextH, _rectP0: *const PF_Rect) -> PF_Err {
	PF_Err_NONE as PF_Err
}

/// Convert panel-local to screen coordinates. With no window, both spaces
/// coincide, so the point is copied through.
unsafe extern "C" fn convert_local_to_global(localP: *const PF_Point, globalP: *mut PF_Point) -> PF_Err {
	if localP.is_null() || globalP.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}
	unsafe { *globalP = *localP };
	PF_Err_NONE as PF_Err
}

/// Sample the screen color (eyedropper). There is no screen; report transparent black.
unsafe extern "C" fn get_color_at_global_point(
	_globalP: *const PF_Point,
	_eyeSize: A_short,
	_mode: PF_EyeDropperSampleMode,
	outColorP: *mut PF_PixelFloat,
) -> PF_Err {
	if outColorP.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}
	unsafe { *outColorP = PF_PixelFloat { alpha: 0.0, red: 0.0, green: 0.0, blue: 0.0 } };
	PF_Err_NONE as PF_Err
}

/// Address handed out as every progress-dialog handle. `_PF_AppProgressDialog`
/// is opaque to plugins, so any stable non-null address works; plugins only
/// pass it back to [`progress_dialog_update`] / [`dispose_progress_dialog`].
static PROGRESS_DIALOG_TOKEN: u8 = 0;

/// Open a progress dialog. Headless, so we hand back a dummy non-null handle
/// (a null one would read as failure to plugins that check it).
unsafe extern "C" fn create_progress_dialog(
	_titleZ: *const A_UTF16Char,
	_cancel_strZ0: *const A_UTF16Char,
	_indeterminateB: PF_Boolean,
	prog_dlgPP: *mut PF_AppProgressDialogP,
) -> PF_Err {
	if prog_dlgPP.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}
	unsafe { *prog_dlgPP = &PROGRESS_DIALOG_TOKEN as *const u8 as *mut _PF_AppProgressDialog };
	PF_Err_NONE as PF_Err
}

/// Advance a progress dialog. Never cancelled, so always `PF_Err_NONE`.
unsafe extern "C" fn progress_dialog_update(
	_prog_dlgP: PF_AppProgressDialogP,
	countL: A_long,
	totalL: A_long,
) -> PF_Err {
	log::trace!("App progress dialog: {countL}/{totalL}");
	PF_Err_NONE as PF_Err
}

/// Close a progress dialog. The handle is a static token, so nothing to free.
unsafe extern "C" fn dispose_progress_dialog(_prog_dlgP: PF_AppProgressDialogP) -> PF_Err {
	PF_Err_NONE as PF_Err
}

// The "PF AE App Suite" tables below.
//
// Every slot is filled with a headless implementation: there is no host UI, so
// queries report neutral defaults and UI actions are no-ops.
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
