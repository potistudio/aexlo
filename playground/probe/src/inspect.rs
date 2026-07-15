//! Read-only JSON snapshots of host-provided SDK structures.
//!
//! Everything here only *reads* what the host handed us; pointers are logged
//! as null/non-null so traces stay comparable across hosts and runs.

use after_effects_sys as ae;
use serde_json::{Value, json};

/// Human-readable name for a `PF_Cmd` code.
pub fn cmd_name(cmd: ae::PF_Cmd) -> &'static str {
	#[allow(non_upper_case_globals)]
	match cmd {
		ae::PF_Cmd_ABOUT => "ABOUT",
		ae::PF_Cmd_GLOBAL_SETUP => "GLOBAL_SETUP",
		ae::PF_Cmd_GLOBAL_SETDOWN => "GLOBAL_SETDOWN",
		ae::PF_Cmd_PARAMS_SETUP => "PARAMS_SETUP",
		ae::PF_Cmd_SEQUENCE_SETUP => "SEQUENCE_SETUP",
		ae::PF_Cmd_SEQUENCE_RESETUP => "SEQUENCE_RESETUP",
		ae::PF_Cmd_SEQUENCE_FLATTEN => "SEQUENCE_FLATTEN",
		ae::PF_Cmd_SEQUENCE_SETDOWN => "SEQUENCE_SETDOWN",
		ae::PF_Cmd_DO_DIALOG => "DO_DIALOG",
		ae::PF_Cmd_FRAME_SETUP => "FRAME_SETUP",
		ae::PF_Cmd_RENDER => "RENDER",
		ae::PF_Cmd_FRAME_SETDOWN => "FRAME_SETDOWN",
		ae::PF_Cmd_USER_CHANGED_PARAM => "USER_CHANGED_PARAM",
		ae::PF_Cmd_UPDATE_PARAMS_UI => "UPDATE_PARAMS_UI",
		ae::PF_Cmd_EVENT => "EVENT",
		ae::PF_Cmd_GET_EXTERNAL_DEPENDENCIES => "GET_EXTERNAL_DEPENDENCIES",
		ae::PF_Cmd_COMPLETELY_GENERAL => "COMPLETELY_GENERAL",
		ae::PF_Cmd_QUERY_DYNAMIC_FLAGS => "QUERY_DYNAMIC_FLAGS",
		ae::PF_Cmd_AUDIO_RENDER => "AUDIO_RENDER",
		ae::PF_Cmd_AUDIO_SETUP => "AUDIO_SETUP",
		ae::PF_Cmd_AUDIO_SETDOWN => "AUDIO_SETDOWN",
		ae::PF_Cmd_ARBITRARY_CALLBACK => "ARBITRARY_CALLBACK",
		ae::PF_Cmd_SMART_PRE_RENDER => "SMART_PRE_RENDER",
		ae::PF_Cmd_SMART_RENDER => "SMART_RENDER",
		ae::PF_Cmd_GET_FLATTENED_SEQUENCE_DATA => "GET_FLATTENED_SEQUENCE_DATA",
		ae::PF_Cmd_TRANSLATE_PARAMS_TO_PREFS => "TRANSLATE_PARAMS_TO_PREFS",
		ae::PF_Cmd_SMART_RENDER_GPU => "SMART_RENDER_GPU",
		ae::PF_Cmd_GPU_DEVICE_SETUP => "GPU_DEVICE_SETUP",
		ae::PF_Cmd_GPU_DEVICE_SETDOWN => "GPU_DEVICE_SETDOWN",
		_ => "UNKNOWN",
	}
}

/// Render a fourcc code (e.g. `appl_id`) as text, or hex if unprintable.
pub fn fourcc(value: i32) -> String {
	let bytes = (value as u32).to_be_bytes();
	if bytes.iter().all(|b| b.is_ascii_graphic() || *b == b' ') {
		bytes.iter().map(|b| *b as char).collect()
	} else {
		format!("0x{:08X}", value as u32)
	}
}

/// Decode a NUL-terminated `A_char` buffer (e.g. param names, return_msg).
pub fn cstr_field(bytes: &[i8]) -> String {
	let unsigned: Vec<u8> = bytes.iter().take_while(|&&b| b != 0).map(|&b| b as u8).collect();
	String::from_utf8_lossy(&unsigned).into_owned()
}

fn rect(r: &ae::PF_LRect) -> Value {
	json!({ "l": r.left, "t": r.top, "r": r.right, "b": r.bottom })
}

fn rational(r: &ae::PF_RationalScale) -> Value {
	json!({ "num": r.num, "den": r.den })
}

/// Snapshot of the `PF_InData` block the host passed with a command.
pub unsafe fn snapshot_in_data(in_data: *const ae::PF_InData) -> Value {
	if in_data.is_null() {
		return json!(null);
	}
	let d = unsafe { &*in_data };

	json!({
		"spec_version": format!("{}.{}", d.version.major, d.version.minor),
		"serial_num": d.serial_num,
		"appl_id": fourcc(d.appl_id),
		"quality": d.quality,
		"num_params": d.num_params,
		"what_cpu": d.what_cpu,
		"what_fpu": d.what_fpu,
		"time": {
			"current": d.current_time,
			"step": d.time_step,
			"local_step": d.local_time_step,
			"total": d.total_time,
			"scale": d.time_scale,
		},
		"field": d.field,
		"width": d.width,
		"height": d.height,
		"extent_hint": rect(&d.extent_hint),
		"output_origin": [d.output_origin_x, d.output_origin_y],
		"pre_effect_source_origin": [d.pre_effect_source_origin_x, d.pre_effect_source_origin_y],
		"downsample": [rational(&d.downsample_x), rational(&d.downsample_y)],
		"pixel_aspect_ratio": rational(&d.pixel_aspect_ratio),
		"in_flags": d.in_flags,
		"shutter_angle": d.shutter_angle,
		"shutter_phase": d.shutter_phase,
		"ptrs": {
			"effect_ref": !d.effect_ref.is_null(),
			"utils": !d.utils.is_null(),
			"pica_basic": !d.pica_basicP.is_null(),
			"global_data": !d.global_data.is_null(),
			"sequence_data": !d.sequence_data.is_null(),
			"frame_data": !d.frame_data.is_null(),
		},
		"inter": {
			"add_param": d.inter.add_param.is_some(),
			"checkout_param": d.inter.checkout_param.is_some(),
			"checkin_param": d.inter.checkin_param.is_some(),
			"abort": d.inter.abort.is_some(),
			"progress": d.inter.progress.is_some(),
			"register_ui": d.inter.register_ui.is_some(),
		},
	})
}

/// Snapshot of a `PF_LayerDef` (effect world). `with_hash` additionally
/// fingerprints the pixel content so hosts can be compared bit-for-bit.
pub unsafe fn snapshot_world(world: *const ae::PF_LayerDef, with_hash: bool) -> Value {
	if world.is_null() {
		return json!(null);
	}
	let w = unsafe { &*world };
	let deep = w.world_flags & ae::PF_WorldFlag_DEEP != 0;

	let mut value = json!({
		"width": w.width,
		"height": w.height,
		"rowbytes": w.rowbytes,
		"world_flags": w.world_flags,
		"deep": deep,
		"extent_hint": rect(&w.extent_hint),
		"origin": [w.origin_x, w.origin_y],
		"pix_aspect_ratio": rational(&w.pix_aspect_ratio),
		"data": !w.data.is_null(),
	});

	if with_hash && let Some(hash) = unsafe { hash_world(w) } {
		value["pixels_fnv1a"] = json!(format!("{hash:016x}"));
	}

	value
}

/// FNV-1a over the meaningful bytes of each row (skips rowbytes padding).
unsafe fn hash_world(w: &ae::PF_LayerDef) -> Option<u64> {
	if w.data.is_null() || w.width <= 0 || w.height <= 0 || w.rowbytes <= 0 {
		return None;
	}

	let bytes_per_pixel = if w.world_flags & ae::PF_WorldFlag_DEEP != 0 {
		8
	} else {
		4
	};
	let row_len = (w.width as usize) * bytes_per_pixel;
	if row_len > w.rowbytes as usize {
		return None;
	}

	let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
	for y in 0..w.height as usize {
		let row = unsafe { (w.data as *const u8).byte_add(y * w.rowbytes as usize) };
		let row = unsafe { std::slice::from_raw_parts(row, row_len) };
		for &byte in row {
			hash ^= byte as u64;
			hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
		}
	}
	Some(hash)
}

/// Snapshot of one `PF_ParamDef` as handed to us at render time.
pub unsafe fn snapshot_param(index: usize, def: *const ae::PF_ParamDef) -> Value {
	if def.is_null() {
		return json!({ "index": index, "null": true });
	}
	let d = unsafe { &*def };
	let name = cstr_field(&d.name_do_not_use_directly);

	#[allow(non_upper_case_globals)]
	let (type_name, value) = unsafe {
		match d.param_type {
			ae::PF_Param_LAYER => ("LAYER", snapshot_world(&raw const d.u.ld, false)),
			ae::PF_Param_SLIDER => ("SLIDER", json!(d.u.sd.value)),
			ae::PF_Param_FIX_SLIDER => ("FIX_SLIDER", json!(fixed_to_f64(d.u.fd.value))),
			ae::PF_Param_ANGLE => ("ANGLE", json!(fixed_to_f64(d.u.ad.value))),
			ae::PF_Param_CHECKBOX => ("CHECKBOX", json!(d.u.bd.value != 0)),
			ae::PF_Param_COLOR => (
				"COLOR",
				json!({
					"a": d.u.cd.value.alpha,
					"r": d.u.cd.value.red,
					"g": d.u.cd.value.green,
					"b": d.u.cd.value.blue,
				}),
			),
			ae::PF_Param_POINT => (
				"POINT",
				json!({ "x": fixed_to_f64(d.u.td.x_value), "y": fixed_to_f64(d.u.td.y_value) }),
			),
			ae::PF_Param_POPUP => ("POPUP", json!(d.u.pd.value)),
			ae::PF_Param_FLOAT_SLIDER => ("FLOAT_SLIDER", json!(d.u.fs_d.value)),
			other => ("OTHER", json!(other)),
		}
	};

	json!({
		"index": index,
		"name": name,
		"type": type_name,
		"value": value,
		"id": unsafe { d.uu.id },
		"flags": d.flags,
		"ui_flags": d.ui_flags,
	})
}

pub fn fixed_to_f64(fixed: ae::PF_Fixed) -> f64 {
	fixed as f64 / 65536.0
}
