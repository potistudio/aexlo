//! Effect UI suites: "PF Effect UI Suite", "PF Effect Custom UI Suite" and
//! "PF Effect Custom UI Overlay Theme Suite".
//!
//! * The Options button label is per instance; it is recorded on the
//!   [`PluginInstance`] (see [`PluginInstance::options_button_name`]).
//! * aexlo never sends `PF_Cmd_EVENT`, so there is never a live drawing
//!   context: `PF_GetDrawingReference` / `PF_GetContextAsyncManager` report
//!   failure with null out-params, and the overlay drawing calls draw nothing.
//! * The overlay theme (colors, stroke width, vertex size, shadow offset) comes
//!   from the installed [`AppHost`](crate::host::app::AppHost).

use super::ffi::read_c_str;
use crate::PluginInstance;
use crate::core::diagnostics::diag;
use crate::host::app::{AppPixelF, with_app_host_ffi as with_host};
use after_effects_sys::{
	A_FloatPoint, A_LPoint, A_char, DRAWBOT_ColorRGBA, DRAWBOT_DrawRef, DRAWBOT_PathRef, PF_AsyncManagerP, PF_Boolean,
	PF_ContextH, PF_EffectCustomUIOverlayThemeSuite1, PF_EffectCustomUISuite1, PF_EffectCustomUISuite2,
	PF_EffectUISuite1, PF_Err, PF_Err_BAD_CALLBACK_PARAM, PF_Err_NONE, PF_EventExtra, PF_InData, PF_ProgPtr,
};

// ---- PF Effect UI Suite ----------------------------------------------------

unsafe extern "C" fn set_options_button_name(effect_ref: PF_ProgPtr, nameZ: *const A_char) -> PF_Err {
	diag!("PF_EffectUISuite1/PF_SetOptionsButtonName",
		"effect_ref" => format!("{:#x}", effect_ref as usize),
		"nameZ" => format!("{:?}", unsafe { read_c_str(nameZ) }),
	);

	let Some(mut instance) = PluginInstance::get_instance_ptr(effect_ref) else {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	};
	let Some(name) = (unsafe { read_c_str(nameZ) }) else {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	};
	// SAFETY: effect_ref is the instance currently being called into; nothing
	// else holds a reference to this field during the callback.
	unsafe { instance.as_mut() }.set_options_button_name(name);
	PF_Err_NONE as PF_Err
}

pub(super) const fn create_effect_ui_suite_1() -> PF_EffectUISuite1 {
	PF_EffectUISuite1 {
		PF_SetOptionsButtonName: Some(set_options_button_name),
	}
}

// ---- PF Effect Custom UI Suite ---------------------------------------------

unsafe extern "C" fn get_drawing_reference(effect_contextH: PF_ContextH, referenceP0: *mut DRAWBOT_DrawRef) -> PF_Err {
	diag!("PF_EffectCustomUISuite/PF_GetDrawingReference",
		"effect_contextH" => format!("{:#x}", effect_contextH as usize),
		"referenceP0" => format!("{:#x}", referenceP0 as usize),
	);

	let _ = effect_contextH;
	if let Some(out) = unsafe { referenceP0.as_mut() } {
		*out = std::ptr::null_mut();
	}
	PF_Err_BAD_CALLBACK_PARAM as PF_Err
}

unsafe extern "C" fn get_context_async_manager(
	in_data: *mut PF_InData,
	extra: *mut PF_EventExtra,
	managerPP0: *mut PF_AsyncManagerP,
) -> PF_Err {
	diag!("PF_EffectCustomUISuite/PF_GetContextAsyncManager",
		"in_data" => format!("{:#x}", in_data as usize),
		"extra" => format!("{:#x}", extra as usize),
		"managerPP0" => format!("{:#x}", managerPP0 as usize),
	);

	let _ = (in_data, extra);
	if let Some(out) = unsafe { managerPP0.as_mut() } {
		*out = std::ptr::null_mut();
	}
	PF_Err_BAD_CALLBACK_PARAM as PF_Err
}

pub(super) const fn create_effect_custom_ui_suite_1() -> PF_EffectCustomUISuite1 {
	PF_EffectCustomUISuite1 {
		PF_GetDrawingReference: Some(get_drawing_reference),
	}
}

pub(super) const fn create_effect_custom_ui_suite_2() -> PF_EffectCustomUISuite2 {
	PF_EffectCustomUISuite2 {
		PF_GetDrawingReference: Some(get_drawing_reference),
		PF_GetContextAsyncManager: Some(get_context_async_manager),
	}
}

// ---- PF Effect Custom UI Overlay Theme Suite --------------------------------

fn to_drawbot(c: AppPixelF) -> DRAWBOT_ColorRGBA {
	DRAWBOT_ColorRGBA {
		red: c.red,
		green: c.green,
		blue: c.blue,
		alpha: c.alpha,
	}
}

/// Write one field of the host's overlay theme into a nullable out-param.
macro_rules! theme_getter {
	($name:ident, $diag:literal, $ty:ty, |$theme:ident| $value:expr) => {
		unsafe extern "C" fn $name(outP: *mut $ty) -> PF_Err {
			diag!($diag, "outP" => format!("{:#x}", outP as usize));

			if outP.is_null() {
				return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
			}
			with_host(|host| {
				let $theme = host.overlay_theme();
				unsafe { *outP = $value };
				PF_Err_NONE as PF_Err
			})
		}
	};
}

theme_getter!(
	get_preferred_foreground_color,
	"PF_EffectCustomUIOverlayThemeSuite1/PF_GetPreferredForegroundColor",
	DRAWBOT_ColorRGBA,
	|t| to_drawbot(t.foreground)
);
theme_getter!(
	get_preferred_shadow_color,
	"PF_EffectCustomUIOverlayThemeSuite1/PF_GetPreferredShadowColor",
	DRAWBOT_ColorRGBA,
	|t| to_drawbot(t.shadow)
);
theme_getter!(
	get_preferred_stroke_width,
	"PF_EffectCustomUIOverlayThemeSuite1/PF_GetPreferredStrokeWidth",
	f32,
	|t| t.stroke_width
);
theme_getter!(
	get_preferred_vertex_size,
	"PF_EffectCustomUIOverlayThemeSuite1/PF_GetPreferredVertexSize",
	f32,
	|t| t.vertex_size
);
theme_getter!(
	get_preferred_shadow_offset,
	"PF_EffectCustomUIOverlayThemeSuite1/PF_GetPreferredShadowOffset",
	A_LPoint,
	|t| A_LPoint {
		x: t.shadow_offset.h,
		y: t.shadow_offset.v
	}
);

unsafe extern "C" fn stroke_path(
	drawbot_ref: DRAWBOT_DrawRef,
	path_ref: DRAWBOT_PathRef,
	draw_shadowB: PF_Boolean,
) -> PF_Err {
	diag!("PF_EffectCustomUIOverlayThemeSuite1/PF_StrokePath",
		"drawbot_ref" => format!("{:#x}", drawbot_ref as usize),
		"path_ref" => format!("{:#x}", path_ref as usize),
		"draw_shadowB" => draw_shadowB,
	);
	let _ = (drawbot_ref, path_ref, draw_shadowB);
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn fill_path(
	drawbot_ref: DRAWBOT_DrawRef,
	path_ref: DRAWBOT_PathRef,
	draw_shadowB: PF_Boolean,
) -> PF_Err {
	diag!("PF_EffectCustomUIOverlayThemeSuite1/PF_FillPath",
		"drawbot_ref" => format!("{:#x}", drawbot_ref as usize),
		"path_ref" => format!("{:#x}", path_ref as usize),
		"draw_shadowB" => draw_shadowB,
	);
	let _ = (drawbot_ref, path_ref, draw_shadowB);
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn fill_vertex(
	drawbot_ref: DRAWBOT_DrawRef,
	center_pointP: *const A_FloatPoint,
	draw_shadowB: PF_Boolean,
) -> PF_Err {
	diag!("PF_EffectCustomUIOverlayThemeSuite1/PF_FillVertex",
		"drawbot_ref" => format!("{:#x}", drawbot_ref as usize),
		"center_pointP" => format!("{:#x}", center_pointP as usize),
		"draw_shadowB" => draw_shadowB,
	);
	let _ = (drawbot_ref, center_pointP, draw_shadowB);
	PF_Err_NONE as PF_Err
}

pub(super) const fn create_overlay_theme_suite_1() -> PF_EffectCustomUIOverlayThemeSuite1 {
	PF_EffectCustomUIOverlayThemeSuite1 {
		PF_GetPreferredForegroundColor: Some(get_preferred_foreground_color),
		PF_GetPreferredShadowColor: Some(get_preferred_shadow_color),
		PF_GetPreferredStrokeWidth: Some(get_preferred_stroke_width),
		PF_GetPreferredVertexSize: Some(get_preferred_vertex_size),
		PF_GetPreferredShadowOffset: Some(get_preferred_shadow_offset),
		PF_StrokePath: Some(stroke_path),
		PF_FillPath: Some(fill_path),
		PF_FillVertex: Some(fill_vertex),
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::host::app::{AppHost, AppPoint, OverlayTheme, TEST_APP_HOST};

	struct ThickHost;
	impl AppHost for ThickHost {
		fn overlay_theme(&self) -> OverlayTheme {
			OverlayTheme {
				stroke_width: 3.0,
				shadow_offset: AppPoint { h: 2, v: -1 },
				..Default::default()
			}
		}
	}

	#[test]
	fn overlay_theme_comes_from_host() {
		TEST_APP_HOST.set(Some(Box::new(ThickHost)));
		let suite = create_overlay_theme_suite_1();
		let (mut width, mut offset) = (0.0f32, A_LPoint { x: 0, y: 0 });
		let mut fg = DRAWBOT_ColorRGBA {
			red: 0.0,
			green: 0.0,
			blue: 0.0,
			alpha: 0.0,
		};
		unsafe {
			suite.PF_GetPreferredStrokeWidth.unwrap()(&mut width);
			suite.PF_GetPreferredShadowOffset.unwrap()(&mut offset);
			suite.PF_GetPreferredForegroundColor.unwrap()(&mut fg);
		}
		TEST_APP_HOST.set(None);
		assert_eq!(width, 3.0);
		assert_eq!((offset.x, offset.y), (2, -1));
		assert_eq!((fg.red, fg.alpha), (1.0, 1.0));
	}

	#[test]
	fn no_drawing_context_outside_events() {
		let mut r: DRAWBOT_DrawRef = 1 as DRAWBOT_DrawRef;
		let err =
			unsafe { create_effect_custom_ui_suite_2().PF_GetDrawingReference.unwrap()(std::ptr::null_mut(), &mut r) };
		assert_ne!(err, PF_Err_NONE as PF_Err);
		assert!(r.is_null());
	}
}
