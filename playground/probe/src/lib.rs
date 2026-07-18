//! aexlo-probe — an instrumented After Effects effect plugin.
//!
//! The probe is the measuring instrument; the host is the variable. Load the
//! same binary into real After Effects and into aexlo, and it verifies host
//! behavior *one function, one suite, one variable at a time*: every check in
//! `checks.rs` feeds a host service fixed inputs and records the exact output
//! as a `fact`. Facts are deterministic by construction, so
//! `cargo run -p playground -- diff` compares them across hosts without any
//! scenario noise — command order, timing, and GUI-driven behavior are logged
//! too, but only as context.
//!
//! Renders a deterministic, parameter-driven test pattern so plumbing
//! problems are visible on screen as well as in the trace.

#![allow(non_snake_case)]

mod checks;
mod inspect;
mod probes;
mod render;
mod trace;

use after_effects::sys as ae;
use serde_json::json;

use trace::trace;

// ---- Identity ----------------------------------------------------------
// Keep in sync with build.rs: AE cross-checks the PiPL resource against what
// GLOBAL_SETUP writes into out_data.

pub const EFFECT_NAME: &str = "Aexlo Probe";
pub const MATCH_NAME: &str = "AEXLO Probe";
pub const CATEGORY: &str = "aexlo";

/// PF_VERSION(1, 0, 0, PF_Stage_RELEASE, 1)
const PROBE_PF_VERSION: u32 = (1 << 19) | (3 << 9) | 1;
const OUT_FLAGS: ae::PF_OutFlags = 0;
/// PF_OutFlag2_SUPPORTS_THREADED_RENDERING — also probes multi-frame
/// rendering: concurrent RENDERs show up as interleaved `tid`s in the trace.
const OUT_FLAGS2: ae::PF_OutFlags2 = 0x0800_0000;

/// Marker written into sequence data so RESETUP/SETDOWN can verify the host
/// preserved our handle contents.
const SEQUENCE_MAGIC: [u8; 8] = *b"AXPROBE1";

// ---- Entry points -------------------------------------------------------

/// `PluginDataEntryFunction2`: self-description protocol used by aexlo (and
/// modern AE) in place of parsing the PiPL resource.
///
/// # Safety
///
/// Called by the host across the FFI boundary. The host-name/version pointers
/// may be null (handled), but `in_ptr` and the callback must be the valid
/// values the host passed in.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn PluginDataEntryFunction2(
	in_ptr: ae::PF_PluginDataPtr,
	in_plugin_data_callback: ae::PF_PluginDataCB2,
	_in_sp_basic_suite: *const ae::SPBasicSuite,
	in_host_name: *const std::os::raw::c_char,
	in_host_version: *const std::os::raw::c_char,
) -> ae::PF_Err {
	let describe = |ptr: *const std::os::raw::c_char| {
		if ptr.is_null() {
			"<null>".to_string()
		} else {
			unsafe { std::ffi::CStr::from_ptr(ptr) }.to_string_lossy().into_owned()
		}
	};

	trace().emit(
		"plugin_data_entry",
		json!({
			"host_name": describe(in_host_name),
			"host_version": describe(in_host_version),
		}),
	);

	let Some(callback) = in_plugin_data_callback else {
		return ae::PF_Err_BAD_CALLBACK_PARAM as ae::PF_Err;
	};

	unsafe {
		callback(
			in_ptr,
			c"Aexlo Probe".as_ptr() as *const ae::A_u_char,
			c"AEXLO Probe".as_ptr() as *const ae::A_u_char,
			c"aexlo".as_ptr() as *const ae::A_u_char,
			c"EffectMain".as_ptr() as *const ae::A_u_char,
			i32::from_be_bytes(*b"eFKT"),
			ae::PF_PLUG_IN_VERSION as ae::A_long,
			ae::PF_PLUG_IN_SUBVERS as ae::A_long,
			0,
			c"https://github.com/potistudio/aexlo-rs".as_ptr() as *const ae::A_u_char,
		) as ae::PF_Err
	}
}

/// The AE effect entry point. Every command is logged on the way in (with an
/// `PF_InData` snapshot) and on the way out (with the error code and the
/// out_data fields the handler touched).
///
/// # Safety
///
/// Called by the host across the FFI boundary; every pointer argument must be
/// the valid `PF_*` structure the host passes for `cmd`, as the AE SDK
/// specifies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn EffectMain(
	cmd: ae::PF_Cmd,
	in_data: *mut ae::PF_InData,
	out_data: *mut ae::PF_OutData,
	params: ae::PF_ParamList,
	output: *mut ae::PF_LayerDef,
	extra: *mut std::ffi::c_void,
) -> ae::PF_Err {
	let name = inspect::cmd_name(cmd);
	trace().emit(
		"cmd",
		json!({
			"phase": "begin",
			"cmd": name,
			"cmd_code": cmd,
			"in": unsafe { inspect::snapshot_in_data(in_data) },
			"params_nonnull": !params.is_null(),
			"output_nonnull": !output.is_null(),
			"extra_nonnull": !extra.is_null(),
		}),
	);

	// Never unwind into the host: a probe bug should show up in the trace,
	// not as a host crash.
	let err = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
		dispatch(cmd, in_data, out_data, params, output, extra)
	}))
	.unwrap_or_else(|panic| {
		let msg = panic
			.downcast_ref::<&str>()
			.map(|s| s.to_string())
			.or_else(|| panic.downcast_ref::<String>().cloned())
			.unwrap_or_else(|| "<non-string panic>".to_string());
		trace().emit("panic", json!({ "cmd": name, "msg": msg }));
		ae::PF_Err_INTERNAL_STRUCT_DAMAGED as ae::PF_Err
	});

	trace().emit(
		"cmd",
		json!({
			"phase": "end",
			"cmd": name,
			"err": err,
			"out": unsafe { snapshot_out_data(out_data) },
		}),
	);

	err
}

unsafe fn snapshot_out_data(out_data: *const ae::PF_OutData) -> serde_json::Value {
	if out_data.is_null() {
		return json!(null);
	}
	let d = unsafe { &*out_data };

	json!({
		"my_version": d.my_version,
		"out_flags": d.out_flags,
		"out_flags2": d.out_flags2,
		"num_params": d.num_params,
		"return_msg": inspect::cstr_field(&d.return_msg),
		"ptrs": {
			"global_data": !d.global_data.is_null(),
			"sequence_data": !d.sequence_data.is_null(),
			"frame_data": !d.frame_data.is_null(),
		},
	})
}

unsafe fn dispatch(
	cmd: ae::PF_Cmd,
	in_data: *mut ae::PF_InData,
	out_data: *mut ae::PF_OutData,
	params: ae::PF_ParamList,
	output: *mut ae::PF_LayerDef,
	extra: *mut std::ffi::c_void,
) -> ae::PF_Err {
	let none = ae::PF_Err_NONE as ae::PF_Err;

	#[allow(non_upper_case_globals)]
	match cmd {
		ae::PF_Cmd_GLOBAL_SETUP => unsafe {
			(*out_data).my_version = PROBE_PF_VERSION as ae::A_u_long;
			(*out_data).out_flags = OUT_FLAGS;
			(*out_data).out_flags2 = OUT_FLAGS2;

			probes::probe_suites(in_data);
			probes::probe_utils(in_data);
			checks::run_all(in_data);
			none
		},

		ae::PF_Cmd_PARAMS_SETUP => unsafe { render::setup_params(in_data, out_data) },

		ae::PF_Cmd_ABOUT => unsafe {
			write_about(in_data, out_data);
			none
		},

		ae::PF_Cmd_SEQUENCE_SETUP => unsafe { sequence_setup(in_data, out_data) },
		ae::PF_Cmd_SEQUENCE_RESETUP | ae::PF_Cmd_SEQUENCE_FLATTEN => unsafe {
			verify_sequence(in_data, out_data, cmd);
			none
		},
		ae::PF_Cmd_SEQUENCE_SETDOWN => unsafe { sequence_setdown(in_data, out_data) },

		ae::PF_Cmd_FRAME_SETUP | ae::PF_Cmd_FRAME_SETDOWN => {
			trace().emit(
				"world",
				json!({
					"which": "output",
					"world": unsafe { inspect::snapshot_world(output, false) },
				}),
			);
			none
		}

		ae::PF_Cmd_RENDER => unsafe { render::render(in_data, params, output) },

		ae::PF_Cmd_USER_CHANGED_PARAM => {
			let index = unsafe {
				(extra as *const ae::PF_UserChangedParamExtra)
					.as_ref()
					.map(|e| e.param_index)
			};
			trace().emit("note", json!({ "msg": "user_changed_param", "param_index": index }));
			none
		}

		// Everything else is already fully captured by the begin/end events.
		_ => none,
	}
}

/// About text doubles as discovery UX: inside AE, the effect's About dialog
/// shows where the trace is being written. Uses the host's `ansi.sprintf`
/// (exercising it) with a plain-copy fallback.
unsafe fn write_about(in_data: *mut ae::PF_InData, out_data: *mut ae::PF_OutData) {
	let mut path = trace().path().display().to_string();
	path.truncate(180);
	let message = format!("Aexlo Probe v{} — trace: {}", trace::PROBE_VERSION, path);

	let sprintf = unsafe { (*in_data).utils.as_ref().and_then(|u| u.ansi.sprintf) };
	if let Some(sprintf) = sprintf {
		let c_message = std::ffi::CString::new(message).unwrap_or_default();
		unsafe { sprintf((*out_data).return_msg.as_mut_ptr(), c"%s".as_ptr(), c_message.as_ptr()) };
	} else {
		let bytes = message.as_bytes();
		let out = unsafe { &mut (*out_data).return_msg };
		for (i, &b) in bytes.iter().take(out.len() - 1).enumerate() {
			out[i] = b as i8;
		}
	}
}

unsafe fn sequence_setup(in_data: *mut ae::PF_InData, out_data: *mut ae::PF_OutData) -> ae::PF_Err {
	let utils = unsafe { (*in_data).utils.as_ref() };
	let handle = utils
		.and_then(|u| u.host_new_handle)
		.map(|new_handle| unsafe { new_handle(SEQUENCE_MAGIC.len() as u64) })
		.unwrap_or(std::ptr::null_mut());

	if handle.is_null() {
		trace().emit(
			"sequence",
			json!({ "what": "setup", "ok": false, "reason": "host_new_handle unavailable" }),
		);
		return ae::PF_Err_NONE as ae::PF_Err;
	}

	let mut written = false;
	if let Some(u) = utils
		&& let (Some(lock), Some(unlock)) = (u.host_lock_handle, u.host_unlock_handle)
	{
		let ptr = unsafe { lock(handle) };
		if !ptr.is_null() {
			unsafe { std::ptr::copy_nonoverlapping(SEQUENCE_MAGIC.as_ptr(), ptr as *mut u8, SEQUENCE_MAGIC.len()) };
			written = true;
		}
		unsafe { unlock(handle) };
	}

	unsafe { (*out_data).sequence_data = handle };
	trace().emit(
		"sequence",
		json!({ "what": "setup", "ok": true, "magic_written": written }),
	);
	ae::PF_Err_NONE as ae::PF_Err
}

/// RESETUP/FLATTEN: check whether the host preserved the bytes we stored at
/// SEQUENCE_SETUP, then hand the same handle back.
unsafe fn verify_sequence(in_data: *mut ae::PF_InData, out_data: *mut ae::PF_OutData, cmd: ae::PF_Cmd) {
	let what = inspect::cmd_name(cmd);
	let handle = unsafe { (*in_data).sequence_data };

	let magic_ok = if handle.is_null() {
		None
	} else {
		// The host keeps sequence data locked around plugin calls, so the
		// contents are reachable through the handle without a lock/unlock.
		let ptr = unsafe { *handle };
		(!ptr.is_null()).then(|| {
			let stored = unsafe { std::slice::from_raw_parts(ptr as *const u8, SEQUENCE_MAGIC.len()) };
			stored == SEQUENCE_MAGIC
		})
	};

	trace().emit(
		"sequence",
		json!({ "what": what, "handle_nonnull": !handle.is_null(), "magic_ok": magic_ok }),
	);

	unsafe { (*out_data).sequence_data = handle };
}

unsafe fn sequence_setdown(in_data: *mut ae::PF_InData, out_data: *mut ae::PF_OutData) -> ae::PF_Err {
	let handle = unsafe { (*in_data).sequence_data };

	if !handle.is_null()
		&& let Some(dispose) = unsafe { (*in_data).utils.as_ref() }.and_then(|u| u.host_dispose_handle)
	{
		unsafe { dispose(handle) };
	}

	unsafe { (*out_data).sequence_data = std::ptr::null_mut() };
	trace().emit("sequence", json!({ "what": "setdown", "disposed": !handle.is_null() }));
	ae::PF_Err_NONE as ae::PF_Err
}
