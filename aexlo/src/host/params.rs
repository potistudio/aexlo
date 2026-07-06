//! Parameter Manager
//!
//! Provides helper functions for parameter management.
//! Parameters are now stored in individual PluginInstance objects instead of global storage.

use after_effects_sys::*;
use std::ffi::CStr;

/// Normalizes a parameter value to its default value.
pub(crate) fn normalize_param_value_to_default(mut param: PF_ParamDef) -> PF_ParamDef {
	unsafe {
		#[allow(non_upper_case_globals)]
		match param.param_type {
			PF_Param_SLIDER => {
				param.u.sd.value = param.u.sd.dephault;
			}
			PF_Param_FIX_SLIDER => {
				param.u.fd.value = param.u.fd.dephault;
			}
			PF_Param_FLOAT_SLIDER => {
				param.u.fs_d.value = param.u.fs_d.dephault as _;
			}
			PF_Param_ANGLE => {
				param.u.ad.value = param.u.ad.dephault;
			}
			PF_Param_CHECKBOX => {
				param.u.bd.value = param.u.bd.dephault as _;
			}
			PF_Param_POPUP => {
				param.u.pd.value = param.u.pd.dephault as _;
			}
			PF_Param_COLOR => {
				param.u.cd.value = param.u.cd.dephault;
			}
			PF_Param_POINT => {
				param.u.td.x_value = param.u.td.x_dephault;
				param.u.td.y_value = param.u.td.y_dephault;
			}
			PF_Param_POINT_3D => {
				param.u.point3d_d.x_value = param.u.point3d_d.x_dephault;
				param.u.point3d_d.y_value = param.u.point3d_d.y_dephault;
				param.u.point3d_d.z_value = param.u.point3d_d.z_dephault;
			}
			_ => {}
		}
	}

	param
}

/// Returns the name of `param` as a UTF-8 string.
///
/// The name field in `PF_ParamDef` is a null-terminated byte array,
/// so this trims at the first null byte before decoding.
pub(crate) fn param_name(param: &PF_ParamDef) -> String {
	let raw_name = &param.name;
	let end = raw_name.iter().position(|&c| c == 0).unwrap_or(param.name.len());

	let bytes: Vec<u8> = param.name[..end].iter().map(|c| *c as u8).collect();
	String::from_utf8_lossy(&bytes).trim().to_string()
}

/// Returns a human-readable name for the given parameter type.
pub(crate) fn param_type_name(param_type: PF_ParamType) -> &'static str {
	#[allow(non_upper_case_globals)]
	match param_type {
		PF_Param_RESERVED => "Reserved",
		PF_Param_LAYER => "Layer",
		PF_Param_SLIDER => "Slider",
		PF_Param_FIX_SLIDER => "Fixed Slider",
		PF_Param_ANGLE => "Angle",
		PF_Param_CHECKBOX => "Checkbox",
		PF_Param_COLOR => "Color",
		PF_Param_POINT => "Point",
		PF_Param_POPUP => "Popup",
		PF_Param_CUSTOM => "Custom",
		PF_Param_NO_DATA => "No Data",
		PF_Param_FLOAT_SLIDER => "Float Slider",
		PF_Param_ARBITRARY_DATA => "Arbitrary",
		PF_Param_PATH => "Path",
		PF_Param_GROUP_START => "Group Start",
		PF_Param_GROUP_END => "Group End",
		PF_Param_BUTTON => "Button",
		PF_Param_RESERVED2 => "Reserved 2",
		PF_Param_RESERVED3 => "Reserved 3",
		PF_Param_POINT_3D => "Point 3D",
		_ => "Unknown",
	}
}

/// Returns a string with details about the parameter, based on its type and fields.
pub(crate) fn param_details(param: &PF_ParamDef) -> String {
	unsafe {
		#[allow(non_upper_case_globals)]
		match param.param_type {
			PF_Param_SLIDER => {
				let slider = param.u.sd;
				format!(
					"value={}, default={}, valid=[{}, {}], slider=[{}, {}]",
					slider.value,
					slider.dephault,
					slider.valid_min,
					slider.valid_max,
					slider.slider_min,
					slider.slider_max
				)
			}
			PF_Param_FLOAT_SLIDER => {
				let slider = param.u.fs_d;
				format!(
					"value={:.4}, default={:.4}, valid=[{:.4}, {:.4}], slider=[{:.4}, {:.4}], precision={}",
					slider.value,
					slider.dephault,
					slider.valid_min,
					slider.valid_max,
					slider.slider_min,
					slider.slider_max,
					slider.precision
				)
			}
			PF_Param_POPUP => {
				let popup = param.u.pd;
				let options = popup_options(&popup);
				format!(
					"value={}, default={}, choices={}, options={:?}",
					popup.value, popup.dephault, popup.num_choices, options
				)
			}
			PF_Param_CHECKBOX => {
				let checkbox = param.u.bd;
				let label = if checkbox.u.nameptr.is_null() {
					String::new()
				} else {
					CStr::from_ptr(checkbox.u.nameptr).to_string_lossy().to_string()
				};
				format!(
					"value={}, default={}, label='{}'",
					checkbox.value, checkbox.dephault, label
				)
			}
			PF_Param_POINT => {
				let point = param.u.td;
				format!(
					"value=({:.3}, {:.3}), default=({:.3}, {:.3}), restrict_bounds={}",
					fixed_to_f64(point.x_value),
					fixed_to_f64(point.y_value),
					fixed_to_f64(point.x_dephault),
					fixed_to_f64(point.y_dephault),
					point.restrict_bounds
				)
			}
			PF_Param_COLOR => {
				let color = param.u.cd;
				format!(
					"value=({}, {}, {}), default=({}, {}, {})",
					color.value.red,
					color.value.green,
					color.value.blue,
					color.dephault.red,
					color.dephault.green,
					color.dephault.blue
				)
			}
			PF_Param_GROUP_START => "group start".to_string(),
			PF_Param_GROUP_END => "group end".to_string(),
			_ => "n/a".to_string(),
		}
	}
}

fn popup_options(popup: &PF_PopupDef) -> Vec<String> {
	unsafe {
		if popup.u.namesptr.is_null() {
			return Vec::new();
		}

		let options_raw = CStr::from_ptr(popup.u.namesptr).to_string_lossy();
		options_raw
			.split('|')
			.map(str::trim)
			.filter(|value| !value.is_empty())
			.map(ToString::to_string)
			.collect()
	}
}

fn fixed_to_f64(value: PF_Fixed) -> f64 {
	(value as f64) / 65536.0
}

// ============================================================================
// Instance Access Helpers
// ============================================================================

/// Add a parameter to a plugin instance
pub fn add_param_to_instance(effect_ref: PF_ProgPtr, param: PF_ParamDef) -> Result<(), String> {
	if effect_ref.is_null() {
		return Err("effect_ref is null".to_string());
	}

	let instance = crate::instance::PluginInstance::get_instance_ptr(effect_ref);
	if let Some(mut instance_ptr) = instance {
		let instance = unsafe { instance_ptr.as_mut() };
		let normalized_param = normalize_param_value_to_default(param);
		instance.add_instance_param(normalized_param);

		// Log parameter details
		let params = instance.params();
		let index = params.len().saturating_sub(1);

		log::debug!(
			"ParamManager: effect_ref={:#x}, index={}, name='{}', type={}(#{}), details={}",
			effect_ref as usize,
			index,
			param_name(&normalized_param),
			param_type_name(normalized_param.param_type),
			normalized_param.param_type,
			param_details(&normalized_param)
		);

		Ok(())
	} else {
		Err(format!(
			"Failed to get instance for effect_ref={:#x}",
			effect_ref as usize
		))
	}
}

/// Get the number of parameters from a plugin instance
pub fn get_params_count_from_instance(effect_ref: PF_ProgPtr) -> usize {
	if effect_ref.is_null() {
		return 0;
	}

	let instance = crate::instance::PluginInstance::get_instance_ptr(effect_ref);
	if let Some(instance_ptr) = instance {
		let instance = unsafe { instance_ptr.as_ref() };
		instance.params().len()
	} else {
		0
	}
}
