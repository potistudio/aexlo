//! `AEGP Persistent Data Suite` (version 3), the host-preferences store.
//!
//! In real After Effects this suite is a window onto the application preference
//! blob (the `Prefs.txt` files). aexlo is not After Effects and keeps no such
//! blob, so we back it with a process-wide **in-memory** store: reads of unknown
//! keys honestly report "not present" (and return the caller's default, per the
//! SDK contract), while writes round-trip within the session.
//!
//! ## Why this matters beyond preferences
//! Plugins built on the aescripts licensing library (DeepGlow2, and every other
//! aescripts product) call this suite from `getLicense` during
//! `PF_Cmd_SEQUENCE_SETUP` -- `AEGP_GetApplicationBlob`, then `AEGP_DoesKeyExist`
//! on their product section. If the suite is missing, the plugin's
//! `AEGP_SuiteHandler::LoadSuite` throws `MissingSuiteError`, and that C++
//! exception unwinds through our `extern "C"` boundary and aborts the process.
//! Providing a faithful (if empty) suite lets the license check run to
//! completion: a genuinely licensed plugin validates and renders clean; an
//! unlicensed one concludes "no license" and watermarks -- either way it *runs*
//! instead of aborting.

use std::collections::HashMap;
use std::ffi::CStr;
use std::sync::{LazyLock, Mutex};

use after_effects_sys::{
	AEGP_MemHandle, AEGP_PersistentBlobH, AEGP_PersistentDataSuite3, AEGP_PluginID, A_Boolean,
	A_FpLong, A_char, A_Err, A_long, A_u_long,
};

/// A single stored preference value, tagged by the setter that wrote it.
enum Value {
	Data(Vec<u8>),
	Str(Vec<u8>),
	Long(A_long),
	FpLong(A_FpLong),
}

/// Process-wide preference store, keyed by `(section, value)`.
static STORE: LazyLock<Mutex<HashMap<(String, String), Value>>> =
	LazyLock::new(|| Mutex::new(HashMap::new()));

/// Opaque, never-dereferenced token handed back as the "application blob".
///
/// Plugins treat `AEGP_PersistentBlobH` as opaque and only pass it back to us, so
/// any stable non-null address serves; we ignore it and operate on [`STORE`].
static BLOB_SENTINEL: u8 = 0;

fn blob_handle() -> AEGP_PersistentBlobH {
	&BLOB_SENTINEL as *const u8 as AEGP_PersistentBlobH
}

/// # Safety
/// `p` must be null or a valid null-terminated C string.
unsafe fn key(p: *const A_char) -> String {
	if p.is_null() {
		return String::new();
	}
	unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

/// Copy `bytes` (plus a NUL) into `bufZ`, honoring `buf_size`, and report the
/// size actually required via `actual` -- matching the `AEGP_GetString` contract
/// (an undersized buffer yields `""` and the needed size).
///
/// # Safety
/// `bufZ` must be null or writable for `buf_size` bytes; `actual` must be null or
/// a writable `A_u_long`.
unsafe fn write_str(bytes: &[u8], bufZ: *mut A_char, buf_size: A_u_long, actual: *mut A_u_long) {
	let needed = bytes.len() as A_u_long + 1;
	if !actual.is_null() {
		unsafe { *actual = needed };
	}
	if bufZ.is_null() || buf_size == 0 {
		return;
	}
	if needed <= buf_size {
		unsafe {
			std::ptr::copy_nonoverlapping(bytes.as_ptr() as *const A_char, bufZ, bytes.len());
			*bufZ.add(bytes.len()) = 0;
		}
	} else {
		unsafe { *bufZ = 0 };
	}
}

unsafe extern "C" fn get_application_blob(blobPH: *mut AEGP_PersistentBlobH) -> A_Err {
	if !blobPH.is_null() {
		unsafe { *blobPH = blob_handle() };
	}
	0
}

unsafe extern "C" fn get_num_sections(_blobH: AEGP_PersistentBlobH, num: *mut A_long) -> A_Err {
	if !num.is_null() {
		let store = STORE.lock().unwrap();
		let mut sections: Vec<&String> = store.keys().map(|(s, _)| s).collect();
		sections.sort();
		sections.dedup();
		unsafe { *num = sections.len() as A_long };
	}
	0
}

unsafe extern "C" fn get_section_key_by_index(
	_blobH: AEGP_PersistentBlobH,
	section_index: A_long,
	max_section_size: A_long,
	section_keyZ: *mut A_char,
) -> A_Err {
	let store = STORE.lock().unwrap();
	let mut sections: Vec<String> = store.keys().map(|(s, _)| s.clone()).collect();
	sections.sort();
	sections.dedup();
	let name = sections
		.get(section_index as usize)
		.cloned()
		.unwrap_or_default();
	unsafe { write_str(name.as_bytes(), section_keyZ, max_section_size.max(0) as A_u_long, std::ptr::null_mut()) };
	0
}

unsafe extern "C" fn does_key_exist(
	_blobH: AEGP_PersistentBlobH,
	section_keyZ: *const A_char,
	value_keyZ: *const A_char,
	existsPB: *mut A_Boolean,
) -> A_Err {
	let exists = {
		let store = STORE.lock().unwrap();
		store.contains_key(&(unsafe { key(section_keyZ) }, unsafe { key(value_keyZ) }))
	};
	if !existsPB.is_null() {
		unsafe { *existsPB = A_Boolean::from(exists) };
	}
	0
}

unsafe extern "C" fn get_num_keys(
	_blobH: AEGP_PersistentBlobH,
	section_keyZ: *const A_char,
	num: *mut A_long,
) -> A_Err {
	if !num.is_null() {
		let section = unsafe { key(section_keyZ) };
		let store = STORE.lock().unwrap();
		let count = store.keys().filter(|(s, _)| *s == section).count();
		unsafe { *num = count as A_long };
	}
	0
}

unsafe extern "C" fn get_value_key_by_index(
	_blobH: AEGP_PersistentBlobH,
	section_keyZ: *const A_char,
	key_index: A_long,
	max_key_size: A_long,
	value_keyZ: *mut A_char,
) -> A_Err {
	let section = unsafe { key(section_keyZ) };
	let store = STORE.lock().unwrap();
	let mut values: Vec<String> = store
		.keys()
		.filter(|(s, _)| *s == section)
		.map(|(_, v)| v.clone())
		.collect();
	values.sort();
	let name = values.get(key_index as usize).cloned().unwrap_or_default();
	unsafe { write_str(name.as_bytes(), value_keyZ, max_key_size.max(0) as A_u_long, std::ptr::null_mut()) };
	0
}

unsafe extern "C" fn get_data_handle(
	_plugin_id: AEGP_PluginID,
	_blobH: AEGP_PersistentBlobH,
	_section_keyZ: *const A_char,
	_value_keyZ: *const A_char,
	_defaultH0: AEGP_MemHandle,
	valuePH: *mut AEGP_MemHandle,
) -> A_Err {
	// We don't vend AEGP_MemHandles; report "no data" (NULL handle) per the SDK's
	// zero-sized-handle convention. Not reached by the license path.
	if !valuePH.is_null() {
		unsafe { *valuePH = std::ptr::null_mut() };
	}
	0
}

unsafe extern "C" fn get_data(
	_blobH: AEGP_PersistentBlobH,
	section_keyZ: *const A_char,
	value_keyZ: *const A_char,
	data_sizeLu: A_u_long,
	defaultPV0: *const std::os::raw::c_void,
	bufPV: *mut std::os::raw::c_void,
) -> A_Err {
	let k = (unsafe { key(section_keyZ) }, unsafe { key(value_keyZ) });
	let size = data_sizeLu as usize;
	let mut store = STORE.lock().unwrap();
	if let Some(Value::Data(bytes)) = store.get(&k) {
		if bytes.len() == size && !bufPV.is_null() {
			unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), bufPV as *mut u8, size) };
			return 0;
		}
	}
	// Not found (or size mismatch): fall back to the default, writing it back to
	// the store as the SDK specifies.
	let default: Vec<u8> = if defaultPV0.is_null() {
		vec![0u8; size]
	} else {
		unsafe { std::slice::from_raw_parts(defaultPV0 as *const u8, size) }.to_vec()
	};
	if !bufPV.is_null() && size > 0 {
		unsafe { std::ptr::copy_nonoverlapping(default.as_ptr(), bufPV as *mut u8, size) };
	}
	store.insert(k, Value::Data(default));
	0
}

unsafe extern "C" fn get_string(
	_blobH: AEGP_PersistentBlobH,
	section_keyZ: *const A_char,
	value_keyZ: *const A_char,
	defaultZ0: *const A_char,
	buf_sizeLu: A_u_long,
	bufZ: *mut A_char,
	actual_buf_sizeLu0: *mut A_u_long,
) -> A_Err {
	let k = (unsafe { key(section_keyZ) }, unsafe { key(value_keyZ) });
	let mut store = STORE.lock().unwrap();
	if let Some(Value::Str(bytes)) = store.get(&k) {
		unsafe { write_str(bytes, bufZ, buf_sizeLu, actual_buf_sizeLu0) };
		return 0;
	}
	let default = unsafe { key(defaultZ0) };
	unsafe { write_str(default.as_bytes(), bufZ, buf_sizeLu, actual_buf_sizeLu0) };
	store.insert(k, Value::Str(default.into_bytes()));
	0
}

unsafe extern "C" fn get_long(
	_blobH: AEGP_PersistentBlobH,
	section_keyZ: *const A_char,
	value_keyZ: *const A_char,
	defaultL: A_long,
	valuePL: *mut A_long,
) -> A_Err {
	let k = (unsafe { key(section_keyZ) }, unsafe { key(value_keyZ) });
	let mut store = STORE.lock().unwrap();
	let value = match store.get(&k) {
		Some(Value::Long(v)) => *v,
		_ => {
			store.insert(k, Value::Long(defaultL));
			defaultL
		}
	};
	if !valuePL.is_null() {
		unsafe { *valuePL = value };
	}
	0
}

unsafe extern "C" fn get_fp_long(
	_blobH: AEGP_PersistentBlobH,
	section_keyZ: *const A_char,
	value_keyZ: *const A_char,
	defaultF: A_FpLong,
	valuePF: *mut A_FpLong,
) -> A_Err {
	let k = (unsafe { key(section_keyZ) }, unsafe { key(value_keyZ) });
	let mut store = STORE.lock().unwrap();
	let value = match store.get(&k) {
		Some(Value::FpLong(v)) => *v,
		_ => {
			store.insert(k, Value::FpLong(defaultF));
			defaultF
		}
	};
	if !valuePF.is_null() {
		unsafe { *valuePF = value };
	}
	0
}

unsafe extern "C" fn set_data_handle(
	_blobH: AEGP_PersistentBlobH,
	_section_keyZ: *const A_char,
	_value_keyZ: *const A_char,
	_valueH: AEGP_MemHandle,
) -> A_Err {
	// We can't read foreign AEGP_MemHandles without the memory suite; accept and
	// drop. Not reached by the license path.
	0
}

unsafe extern "C" fn set_data(
	_blobH: AEGP_PersistentBlobH,
	section_keyZ: *const A_char,
	value_keyZ: *const A_char,
	data_sizeLu: A_u_long,
	dataPV: *const std::os::raw::c_void,
) -> A_Err {
	let k = (unsafe { key(section_keyZ) }, unsafe { key(value_keyZ) });
	let bytes = if dataPV.is_null() {
		Vec::new()
	} else {
		unsafe { std::slice::from_raw_parts(dataPV as *const u8, data_sizeLu as usize) }.to_vec()
	};
	STORE.lock().unwrap().insert(k, Value::Data(bytes));
	0
}

unsafe extern "C" fn set_string(
	_blobH: AEGP_PersistentBlobH,
	section_keyZ: *const A_char,
	value_keyZ: *const A_char,
	strZ: *const A_char,
) -> A_Err {
	let k = (unsafe { key(section_keyZ) }, unsafe { key(value_keyZ) });
	STORE
		.lock()
		.unwrap()
		.insert(k, Value::Str(unsafe { key(strZ) }.into_bytes()));
	0
}

unsafe extern "C" fn set_long(
	_blobH: AEGP_PersistentBlobH,
	section_keyZ: *const A_char,
	value_keyZ: *const A_char,
	valueL: A_long,
) -> A_Err {
	let k = (unsafe { key(section_keyZ) }, unsafe { key(value_keyZ) });
	STORE.lock().unwrap().insert(k, Value::Long(valueL));
	0
}

unsafe extern "C" fn set_fp_long(
	_blobH: AEGP_PersistentBlobH,
	section_keyZ: *const A_char,
	value_keyZ: *const A_char,
	valueF: A_FpLong,
) -> A_Err {
	let k = (unsafe { key(section_keyZ) }, unsafe { key(value_keyZ) });
	STORE.lock().unwrap().insert(k, Value::FpLong(valueF));
	0
}

unsafe extern "C" fn delete_entry(
	_blobH: AEGP_PersistentBlobH,
	section_keyZ: *const A_char,
	value_keyZ: *const A_char,
) -> A_Err {
	let k = (unsafe { key(section_keyZ) }, unsafe { key(value_keyZ) });
	STORE.lock().unwrap().remove(&k);
	0
}

unsafe extern "C" fn get_prefs_directory(unicode_pathPH: *mut AEGP_MemHandle) -> A_Err {
	// "empty string if no file": we have no prefs directory, so report none.
	if !unicode_pathPH.is_null() {
		unsafe { *unicode_pathPH = std::ptr::null_mut() };
	}
	0
}

/// Build the `AEGP Persistent Data Suite` v3 vtable.
///
/// `const` so it can initialize the shared [`SUITE_CONTAINER`](crate::suites::SUITE_CONTAINER)
/// static; all per-call state lives in the [`STORE`] global, not the vtable.
pub(super) const fn create_persistent_data_suite_3() -> AEGP_PersistentDataSuite3 {
	AEGP_PersistentDataSuite3 {
		AEGP_GetApplicationBlob: Some(get_application_blob),
		AEGP_GetNumSections: Some(get_num_sections),
		AEGP_GetSectionKeyByIndex: Some(get_section_key_by_index),
		AEGP_DoesKeyExist: Some(does_key_exist),
		AEGP_GetNumKeys: Some(get_num_keys),
		AEGP_GetValueKeyByIndex: Some(get_value_key_by_index),
		AEGP_GetDataHandle: Some(get_data_handle),
		AEGP_GetData: Some(get_data),
		AEGP_GetString: Some(get_string),
		AEGP_GetLong: Some(get_long),
		AEGP_GetFpLong: Some(get_fp_long),
		AEGP_SetDataHandle: Some(set_data_handle),
		AEGP_SetData: Some(set_data),
		AEGP_SetString: Some(set_string),
		AEGP_SetLong: Some(set_long),
		AEGP_SetFpLong: Some(set_fp_long),
		AEGP_DeleteEntry: Some(delete_entry),
		AEGP_GetPrefsDirectory: Some(get_prefs_directory),
	}
}
