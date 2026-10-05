//! String marshalling shared by the suite callbacks.

use after_effects_sys::A_char;
use std::ffi::CStr;

/// Copy `src` into a fixed `A_char` buffer as a NUL-terminated C string,
/// truncating to `dst.len() - 1` bytes.
pub(super) fn write_c_str(dst: &mut [A_char], src: &str) {
	let n = src.len().min(dst.len().saturating_sub(1));
	for (d, s) in dst.iter_mut().zip(&src.as_bytes()[..n]) {
		*d = *s as A_char;
	}
	if let Some(end) = dst.get_mut(n) {
		*end = 0;
	}
}

/// Read a nullable NUL-terminated C string (lossy UTF-8).
///
/// # Safety
/// `p` must be null or point to a NUL-terminated `A_char` array.
pub(super) unsafe fn read_c_str(p: *const A_char) -> Option<String> {
	(!p.is_null()).then(|| unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
}
