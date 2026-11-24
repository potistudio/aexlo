//! `AEGP Text Document Suite` v1: reading the string of a source-text
//! stream's value (`ADBE Text Document`).
//!
//! Like an outline, the document is a copy that lives until
//! `AEGP_DisposeStreamValue`. It cannot be edited.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use crate::core::diagnostics::diag;
use crate::observe::{UnsupportedKind, unsupported};
use crate::sys::*;

static DOCUMENTS: LazyLock<Mutex<HashMap<usize, String>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// A handle to a document holding `text`, until [`dispose`].
pub(super) fn publish(text: &str) -> AEGP_TextDocumentH {
	let handle = super::value_handle();
	// After Effects separates lines with carriage returns.
	DOCUMENTS.lock().unwrap().insert(handle, text.replace('\n', "\r"));
	handle as AEGP_TextDocumentH
}

/// Drop the document behind `handle`, if it is one.
pub(super) fn dispose(handle: usize) -> bool {
	DOCUMENTS.lock().unwrap().remove(&handle).is_some()
}

unsafe extern "C" fn get_new_text(
	_plugin_id: AEGP_PluginID,
	document: AEGP_TextDocumentH,
	unicodePH: *mut AEGP_MemHandle,
) -> A_Err {
	crate::core::ffi::guard("get_new_text", || {
		diag!("AEGP_TextDocumentSuite1/AEGP_GetNewText");
		let Some(out) = (unsafe { unicodePH.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		let Some(text) = DOCUMENTS.lock().unwrap().get(&(document as usize)).cloned() else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		*out = super::layer::utf16_handle(&text);
		PF_Err_NONE
	})
}

unsafe extern "C" fn set_text(_document: AEGP_TextDocumentH, _unicodePS: *const A_u_short, _lengthL: A_long) -> A_Err {
	crate::core::ffi::guard("set_text", || {
		diag!("AEGP_TextDocumentSuite1/AEGP_SetText");
		unsupported(
			UnsupportedKind::Declined,
			"AEGP_TextDocumentSuite/AEGP_SetText",
			"the project is read-only",
		);
		A_Err_GENERIC
	})
}

pub(super) const fn suite_1() -> AEGP_TextDocumentSuite1 {
	AEGP_TextDocumentSuite1 {
		AEGP_GetNewText: Some(get_new_text),
		AEGP_SetText: Some(set_text),
	}
}
