//! Parameter Manager
//!
//! Stores parameters registered by plugins via `add_param`.
//! Emulates the C++ `ParamManager` class from aexlo.js.

use after_effects_sys::*;
use std::collections::HashMap;
use std::ffi::CStr;
use std::sync::Mutex;

/// Wrapper for parameter storage that implements Send/Sync.
/// This is safe because we only access this from a controlled context.
struct ParamStorage {
	params: HashMap<usize, Vec<PF_ParamDef>>,
}

// SAFETY: We ensure exclusive access via Mutex, and the raw pointers within
// PF_ParamDef are only accessed on the same thread that created them.
unsafe impl Send for ParamStorage {}
unsafe impl Sync for ParamStorage {}

/// Global parameter storage, keyed by effect_ref (as usize for hashing).
static PARAMS: Mutex<Option<ParamStorage>> = Mutex::new(None);

/// Initializes the parameter manager (called once at startup).
pub fn init() {
	let mut params = PARAMS.lock().unwrap();
	if params.is_none() {
		*params = Some(ParamStorage {
			params: HashMap::new(),
		});
	}
}

/// Adds a parameter definition for the given effect_ref.
pub fn add_param(effect_ref: PF_ProgPtr, param: PF_ParamDef) {
	let mut guard = PARAMS.lock().unwrap();
	let storage = guard.get_or_insert_with(|| ParamStorage {
		params: HashMap::new(),
	});

	let key = effect_ref as usize;
	storage
		.params
		.entry(key)
		.or_insert_with(Vec::new)
		.push(param);

	log::debug!(
		"ParamManager: effect_ref={:#x}, index={}, name='{}', type={}(#{}), details={}",
		key,
		storage
			.params
			.get(&key)
			.map(|params| params.len().saturating_sub(1))
			.unwrap_or(0),
		param_name(&param),
		param_type_name(param.param_type),
		param.param_type,
		param_details(&param)
	);
}

/// Returns the name of `param` as a UTF-8 string.
///
/// The name field in `PF_ParamDef` is a null-terminated byte array,
/// so this trims at the first null byte before decoding.
fn param_name(param: &PF_ParamDef) -> String {
	let raw_name = &param.name;
	let end = raw_name
		.iter()
		.position(|&c| c == 0)
		.unwrap_or(param.name.len());

	let bytes: Vec<u8> = param.name[..end].iter().map(|c| *c as u8).collect();
	String::from_utf8_lossy(&bytes).trim().to_string()
}

/// Returns a human-readable name for the given parameter type.
fn param_type_name(param_type: PF_ParamType) -> &'static str {
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
fn param_details(param: &PF_ParamDef) -> String {
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
					CStr::from_ptr(checkbox.u.nameptr)
						.to_string_lossy()
						.to_string()
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

/// Gets all parameters for the given effect_ref.
pub fn get_params(effect_ref: PF_ProgPtr) -> Vec<PF_ParamDef> {
	let guard = PARAMS.lock().unwrap();
	if let Some(storage) = guard.as_ref() {
		storage
			.params
			.get(&(effect_ref as usize))
			.cloned()
			.unwrap_or_default()
	} else {
		Vec::new()
	}
}

/// Gets the number of parameters for the given effect_ref.
pub fn get_params_count(effect_ref: PF_ProgPtr) -> usize {
	let guard = PARAMS.lock().unwrap();
	if let Some(storage) = guard.as_ref() {
		storage
			.params
			.get(&(effect_ref as usize))
			.map(|v| v.len())
			.unwrap_or(0)
	} else {
		0
	}
}

/// Clears all parameters for the given effect_ref.
pub fn clear_params(effect_ref: PF_ProgPtr) {
	let mut guard = PARAMS.lock().unwrap();
	if let Some(storage) = guard.as_mut() {
		storage.params.remove(&(effect_ref as usize));
	}
}

/// Clears all parameters.
pub fn clear_all() {
	let mut guard = PARAMS.lock().unwrap();
	if let Some(storage) = guard.as_mut() {
		storage.params.clear();
	}
}
