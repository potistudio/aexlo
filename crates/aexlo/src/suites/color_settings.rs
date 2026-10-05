//! `PF Color Settings Suite` (wire version 3, `AEGP_ColorSettingsSuite2`).
//!
//! aexlo does no color management: pixels go in and out untouched, which is
//! what an sRGB working space without linearization means. So the comp's
//! working-space profile is sRGB, an RGB profile with an approximate gamma of
//! 2.2. Profiles are opaque boxes the plugin frees with
//! `AEGP_DisposeColorProfile`. ICC data, descriptions (which need the AEGP
//! Memory Suite), views and blending tables are refused with `A_Err_GENERIC`.

use after_effects_sys::*;

use super::layer::Timeline;
use crate::core::diagnostics::diag;

/// What an `AEGP_ColorProfileP` points at.
struct ColorProfile {
	approximate_gamma: A_FpShort,
}

/// The sRGB working space.
const SRGB: ColorProfile = ColorProfile { approximate_gamma: 2.2 };

fn unsupported(name: &str) -> A_Err {
	log::warn!("AEGP_ColorSettingsSuite2/{name}: aexlo does no color management");
	A_Err_GENERIC as A_Err
}

/// # Safety
/// `color_profileP` must be null or a profile from this module that has not
/// been disposed.
unsafe fn profile_of<'a>(color_profileP: AEGP_ConstColorProfileP) -> Option<&'a ColorProfile> {
	unsafe { (color_profileP as *const ColorProfile).as_ref() }
}

unsafe extern "C" fn get_blending_tables(
	_render_contextH: PR_RenderContextH,
	_blending_tables: *mut PF_EffectBlendingTables,
) -> A_Err {
	diag!("AEGP_ColorSettingsSuite2/AEGP_GetBlendingTables");
	unsupported("AEGP_GetBlendingTables")
}

unsafe extern "C" fn does_view_have_color_space_xform(_viewP: AEGP_ItemViewP, _has_xformPB: *mut A_Boolean) -> A_Err {
	diag!("AEGP_ColorSettingsSuite2/AEGP_DoesViewHaveColorSpaceXform");
	unsupported("AEGP_DoesViewHaveColorSpaceXform")
}

unsafe extern "C" fn xform_working_to_view_color_space(
	_viewP: AEGP_ItemViewP,
	_srcH: AEGP_WorldH,
	_dstH: AEGP_WorldH,
) -> A_Err {
	diag!("AEGP_ColorSettingsSuite2/AEGP_XformWorkingToViewColorSpace");
	unsupported("AEGP_XformWorkingToViewColorSpace")
}

unsafe extern "C" fn get_new_working_space_color_profile(
	_aegp_plugin_id: AEGP_PluginID,
	compH: AEGP_CompH,
	color_profilePP: *mut AEGP_ColorProfileP,
) -> A_Err {
	diag!("AEGP_ColorSettingsSuite2/AEGP_GetNewWorkingSpaceColorProfile",
		"compH" => format!("{:#x}", compH as usize),
	);

	if Timeline::of(compH as _).is_none() {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	}
	let Some(out) = (unsafe { color_profilePP.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	*out = Box::into_raw(Box::new(SRGB)) as AEGP_ColorProfileP;
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn get_new_color_profile_from_icc_profile(
	_aegp_plugin_id: AEGP_PluginID,
	_icc_sizeL: A_long,
	_icc_dataPV: *const std::ffi::c_void,
	_color_profilePP: *mut AEGP_ColorProfileP,
) -> A_Err {
	diag!("AEGP_ColorSettingsSuite2/AEGP_GetNewColorProfileFromICCProfile");
	unsupported("AEGP_GetNewColorProfileFromICCProfile")
}

unsafe extern "C" fn get_new_icc_profile_from_color_profile(
	_aegp_plugin_id: AEGP_PluginID,
	_color_profileP: AEGP_ConstColorProfileP,
	_icc_profilePH: *mut AEGP_MemHandle,
) -> A_Err {
	diag!("AEGP_ColorSettingsSuite2/AEGP_GetNewICCProfileFromColorProfile");
	unsupported("AEGP_GetNewICCProfileFromColorProfile")
}

unsafe extern "C" fn get_new_color_profile_description(
	_aegp_plugin_id: AEGP_PluginID,
	_color_profileP: AEGP_ConstColorProfileP,
	_unicode_descPH: *mut AEGP_MemHandle,
) -> A_Err {
	diag!("AEGP_ColorSettingsSuite2/AEGP_GetNewColorProfileDescription");
	unsupported("AEGP_GetNewColorProfileDescription")
}

unsafe extern "C" fn dispose_color_profile(color_profileP: AEGP_ColorProfileP) -> A_Err {
	diag!("AEGP_ColorSettingsSuite2/AEGP_DisposeColorProfile",
		"color_profileP" => format!("{:#x}", color_profileP as usize),
	);

	if color_profileP.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	}
	// SAFETY: non-null profiles only come from `get_new_working_space_color_profile`.
	drop(unsafe { Box::from_raw(color_profileP as *mut ColorProfile) });
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn get_color_profile_approximate_gamma(
	color_profileP: AEGP_ConstColorProfileP,
	approx_gammaP: *mut A_FpShort,
) -> A_Err {
	diag!("AEGP_ColorSettingsSuite2/AEGP_GetColorProfileApproximateGamma");

	let Some(profile) = (unsafe { profile_of(color_profileP) }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	let Some(out) = (unsafe { approx_gammaP.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	*out = profile.approximate_gamma;
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn is_rgb_color_profile(color_profileP: AEGP_ConstColorProfileP, is_rgbPB: *mut A_Boolean) -> A_Err {
	diag!("AEGP_ColorSettingsSuite2/AEGP_IsRGBColorProfile");

	if unsafe { profile_of(color_profileP) }.is_none() {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	}
	let Some(out) = (unsafe { is_rgbPB.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	*out = 1;
	PF_Err_NONE as A_Err
}

/// Builds the `AEGP_ColorSettingsSuite2` vtable.
pub(super) const fn create_color_settings_suite_2() -> AEGP_ColorSettingsSuite2 {
	AEGP_ColorSettingsSuite2 {
		AEGP_GetBlendingTables: Some(get_blending_tables),
		AEGP_DoesViewHaveColorSpaceXform: Some(does_view_have_color_space_xform),
		AEGP_XformWorkingToViewColorSpace: Some(xform_working_to_view_color_space),
		AEGP_GetNewWorkingSpaceColorProfile: Some(get_new_working_space_color_profile),
		AEGP_GetNewColorProfileFromICCProfile: Some(get_new_color_profile_from_icc_profile),
		AEGP_GetNewICCProfileFromColorProfile: Some(get_new_icc_profile_from_color_profile),
		AEGP_GetNewColorProfileDescription: Some(get_new_color_profile_description),
		AEGP_DisposeColorProfile: Some(dispose_color_profile),
		AEGP_GetColorProfileApproximateGamma: Some(get_color_profile_approximate_gamma),
		AEGP_IsRGBColorProfile: Some(is_rgb_color_profile),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn profiles_report_srgb_and_dispose() {
		let suite = create_color_settings_suite_2();
		let profile = Box::into_raw(Box::new(SRGB)) as AEGP_ColorProfileP;
		let mut gamma: A_FpShort = 0.0;
		let mut rgb: A_Boolean = 0;
		unsafe {
			assert_eq!(
				suite.AEGP_GetColorProfileApproximateGamma.unwrap()(profile, &mut gamma),
				PF_Err_NONE as A_Err
			);
			assert_eq!(
				suite.AEGP_IsRGBColorProfile.unwrap()(profile, &mut rgb),
				PF_Err_NONE as A_Err
			);
			assert_eq!(suite.AEGP_DisposeColorProfile.unwrap()(profile), PF_Err_NONE as A_Err);
		}
		assert_eq!((gamma, rgb), (2.2, 1));
	}

	#[test]
	fn null_comps_get_no_profile() {
		let suite = create_color_settings_suite_2();
		let mut profile: AEGP_ColorProfileP = std::ptr::null_mut();
		let err = unsafe { suite.AEGP_GetNewWorkingSpaceColorProfile.unwrap()(0, std::ptr::null_mut(), &mut profile) };
		assert_eq!(err, PF_Err_BAD_CALLBACK_PARAM as A_Err);
		assert!(profile.is_null());
	}
}
