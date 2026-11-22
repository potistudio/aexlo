use std::ffi::{CStr, CString};

use crate::sys::{A_char, PF_Err, PF_Err_BAD_CALLBACK_PARAM, PF_Err_INTERNAL_STRUCT_DAMAGED, PF_Err_NONE};

#[cfg(feature = "diagnostics")]
use crate::core::diagnostics::DiagnosticBuilder;

macro_rules! impl_math_sys {
	($name:ident, $func:ident, $arg:literal, $diag_path:literal) => {
		#[inline(always)]
		pub(crate) extern "C" fn $name(x: f64) -> f64 {
			let result = x.$func();

			#[cfg(feature = "diagnostics")]
			DiagnosticBuilder::new()
				.set_name($diag_path)
				.add_arg($arg, x)
				.set_result(result)
				.emit();

			result
		}
	};
	($name:ident, $func:ident, $arg1:literal, $arg2:literal, $diag_path:literal) => {
		#[inline(always)]
		pub(crate) extern "C" fn $name(a: f64, b: f64) -> f64 {
			let result = a.$func(b);

			#[cfg(feature = "diagnostics")]
			DiagnosticBuilder::new()
				.set_name($diag_path)
				.add_arg($arg1, a)
				.add_arg($arg2, b)
				.set_result(result)
				.emit();

			result
		}
	};
}

impl_math_sys!(atan_sys, atan, "x", "InData/utils/ansi/atan");
impl_math_sys!(atan2_sys, atan2, "y", "x", "InData/utils/ansi/atan2");
impl_math_sys!(ceil_sys, ceil, "x", "InData/utils/ansi/ceil");
impl_math_sys!(cos_sys, cos, "x", "InData/utils/ansi/cos");
impl_math_sys!(exp_sys, exp, "x", "InData/utils/ansi/exp");
impl_math_sys!(fabs_sys, abs, "x", "InData/utils/ansi/fabs");
impl_math_sys!(floor_sys, floor, "x", "InData/utils/ansi/floor");
impl_math_sys!(fmod_sys, rem_euclid, "x", "y", "InData/utils/ansi/fmod");
impl_math_sys!(hypot_sys, hypot, "x", "y", "InData/utils/ansi/hypot");
impl_math_sys!(log_sys, ln, "x", "InData/utils/ansi/log");
impl_math_sys!(log10_sys, log10, "x", "InData/utils/ansi/log10");
impl_math_sys!(pow_sys, powf, "x", "y", "InData/utils/ansi/pow");
impl_math_sys!(sin_sys, sin, "x", "InData/utils/ansi/sin");
impl_math_sys!(sqrt_sys, sqrt, "x", "InData/utils/ansi/sqrt");
impl_math_sys!(tan_sys, tan, "x", "InData/utils/ansi/tan");
impl_math_sys!(asin_sys, asin, "x", "InData/utils/ansi/asin");
impl_math_sys!(acos_sys, acos, "x", "InData/utils/ansi/acos");

/// The buffer `sprintf` writes into is assumed to hold this many bytes, the
/// size of `PF_OutData::return_msg` (`PF_MAX_EFFECT_MSG_LEN + 1`).
const SPRINTF_BUFFER_SIZE: usize = 256;

// The platform's own `vsnprintf`. The UCRT defines it inline in its headers;
// `legacy_stdio_definitions` is the library that exports it.
#[cfg_attr(windows, link(name = "legacy_stdio_definitions"))]
unsafe extern "C" {
	fn vsnprintf(
		buffer: *mut std::ffi::c_char,
		size: usize,
		format: *const std::ffi::c_char,
		args: std::ffi::VaList,
	) -> std::ffi::c_int;
}

/// `sprintf()`, formatted by the C library's `vsnprintf`, so every
/// conversion, flag, width, precision and length modifier works as it does in
/// After Effects, and the format may be in any 8-bit encoding.
///
/// # Safety
///
/// 1. `arg1` points to a buffer of at least 256 bytes. At most 256 bytes are
///    written, the null terminator included, cutting the text if needed.
/// 2. `arg2` is a valid, null-terminated C string.
/// 3. The variadic arguments match the format's conversions, as for any
///    `sprintf`.
pub(crate) unsafe extern "C" fn sprintf_sys(arg1: *mut A_char, arg2: *const A_char, args: ...) -> PF_Err {
	crate::core::ffi::guard("sprintf_sys", || {
		if arg1.is_null() || arg2.is_null() {
			return PF_Err_BAD_CALLBACK_PARAM;
		}

		let written = unsafe { vsnprintf(arg1, SPRINTF_BUFFER_SIZE, arg2, args) };
		if written < 0 {
			return PF_Err_INTERNAL_STRUCT_DAMAGED;
		}

		#[cfg(feature = "diagnostics")]
		DiagnosticBuilder::new()
			.set_name("UtilityCallbacks/ansi/sprintf")
			.add_arg("format", format!("{:?}", unsafe { CStr::from_ptr(arg2) }))
			.set_result(format!("{:?}", unsafe { CStr::from_ptr(arg1) }))
			.emit();

		PF_Err_NONE
	})
}

pub unsafe extern "C" fn strcpy_sys(arg1: *mut A_char, arg2: *const A_char) -> *mut A_char {
	//== Validation ==//
	if arg1.is_null() || arg2.is_null() {
		return std::ptr::null_mut();
	}

	let src = match unsafe { CStr::from_ptr(arg2) }.to_str() {
		Ok(s) => s,
		Err(_) => return std::ptr::null_mut(),
	};

	let cstring = match CString::new(src) {
		Ok(s) => s,
		Err(_) => return std::ptr::null_mut(),
	};

	let bytes = cstring.as_bytes_with_nul();
	unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), arg1 as *mut u8, bytes.len()) };

	arg1
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::ffi::CStr;

	#[test]
	fn atan_test() {
		let x = 1.0;
		let result = atan_sys(x);

		assert!(
			(result - std::f64::consts::FRAC_PI_4).abs() < 1e-10,
			"Expected atan(1.0) to be close to π/4, got {}",
			result
		);
	}

	#[test]
	fn atan2_test() {
		let y = 1.0;
		let x = 1.0;
		let result = atan2_sys(y, x);

		assert!(
			(result - std::f64::consts::FRAC_PI_4).abs() < 1e-10,
			"Expected atan2(1.0, 1.0) to be close to π/4, got {}",
			result
		);
	}

	#[test]
	fn ceil_test() {
		let x = 1.5;
		let result = ceil_sys(x);

		assert!(
			(result - 2.0).abs() < 1e-10,
			"Expected ceil(1.5) to be close to 2.0, got {}",
			result
		);
	}

	#[test]
	fn cos_test() {
		let x = 0.0;
		let result = cos_sys(x);

		assert!(
			(result - 1.0).abs() < 1e-10,
			"Expected cos(0.0) to be close to 1.0, got {}",
			result
		);
	}

	#[test]
	fn exp_test() {
		let x = 1.0;
		let result = exp_sys(x);

		assert!(
			(result - std::f64::consts::E).abs() < 1e-10,
			"Expected exp(1.0) to be close to e, got {}",
			result
		);
	}

	#[test]
	fn fabs_test() {
		let x = -3.5;
		let result = fabs_sys(x);

		assert!(
			(result - 3.5).abs() < 1e-10,
			"Expected fabs(-3.5) to be close to 3.5, got {}",
			result
		);
	}

	#[test]
	fn floor_test() {
		let x = 1.5;
		let result = floor_sys(x);

		assert!(
			(result - 1.0).abs() < 1e-10,
			"Expected floor(1.5) to be close to 1.0, got {}",
			result
		);
	}

	#[test]
	fn fmod_test() {
		let x = 5.5;
		let y = 2.0;
		let result = fmod_sys(x, y);

		assert!(
			(result - 1.5).abs() < 1e-10,
			"Expected fmod(5.5, 2.0) to be close to 1.5, got {}",
			result
		);
	}

	#[test]
	fn hypot_test() {
		let x = 3.0;
		let y = 4.0;
		let result = hypot_sys(x, y);

		assert!(
			(result - 5.0).abs() < 1e-10,
			"Expected hypot(3.0, 4.0) to be close to 5.0, got {}",
			result
		);
	}

	#[test]
	fn log_test() {
		let x = std::f64::consts::E;
		let result = log_sys(x);

		assert!(
			(result - 1.0).abs() < 1e-10,
			"Expected log(e) to be close to 1.0, got {}",
			result
		);
	}

	#[test]
	fn log10_test() {
		let x = 100.0;
		let result = log10_sys(x);

		assert!(
			(result - 2.0).abs() < 1e-10,
			"Expected log10(100) to be close to 2.0, got {}",
			result
		);
	}

	#[test]
	fn pow_test() {
		let x = 2.0;
		let y = 3.0;
		let result = pow_sys(x, y);

		assert!(
			(result - 8.0).abs() < 1e-10,
			"Expected pow(2.0, 3.0) to be close to 8.0, got {}",
			result
		);
	}

	#[test]
	fn sin_test() {
		let x = std::f64::consts::FRAC_PI_2; // 90 degrees
		let result = sin_sys(x);

		assert!(
			(result - 1.0).abs() < 1e-10,
			"Expected sin(π/2) to be close to 1.0, got {}",
			result
		);
	}

	#[test]
	fn sqrt_test() {
		let x = 16.0;
		let result = sqrt_sys(x);

		assert!(
			(result - 4.0).abs() < 1e-10,
			"Expected sqrt(16) to be close to 4.0, got {}",
			result
		);
	}

	#[test]
	fn tan_test() {
		let x = std::f64::consts::FRAC_PI_4; // 45 degrees
		let result = tan_sys(x);

		assert!(
			(result - 1.0).abs() < 1e-10,
			"Expected tan(π/4) to be close to 1.0, got {}",
			result
		);
	}

	/// Run `sprintf_sys` with the given format and variadic args, asserting it
	/// succeeds, and return the string written into the output buffer.
	///
	/// A macro rather than a function because the argument list is variadic.
	macro_rules! sprintf {
		($fmt:expr $(, $arg:expr)* $(,)?) => {{
			let fmt = CString::new($fmt).unwrap();
			let mut buffer = [0 as A_char; 256];
			let err = unsafe { sprintf_sys(buffer.as_mut_ptr(), fmt.as_ptr() $(, $arg)*) };
			assert_eq!(err, PF_Err_NONE, "sprintf_sys returned error {err}");
			unsafe { CStr::from_ptr(buffer.as_ptr()) }.to_str().unwrap().to_owned()
		}};
	}

	#[test]
	fn sprintf_no_specifier_copies_verbatim() {
		assert_eq!(sprintf!("Hello, world!"), "Hello, world!");
	}

	#[test]
	fn sprintf_single_int() {
		assert_eq!(sprintf!("%d", 42i32), "42");
	}

	#[test]
	fn sprintf_several_ints() {
		assert_eq!(sprintf!("%d + %d = %d", 2i32, 3i32, 5i32), "2 + 3 = 5");
	}

	#[test]
	fn sprintf_negative_int() {
		assert_eq!(sprintf!("%d", -7i32), "-7");
	}

	#[test]
	fn sprintf_string() {
		let s = CString::new("aexlo").unwrap();
		assert_eq!(sprintf!("%s", s.as_ptr()), "aexlo");
	}

	#[test]
	fn sprintf_null_string_arg() {
		assert_eq!(sprintf!("%s", std::ptr::null::<A_char>()), "(null)");
	}

	#[test]
	fn sprintf_mixed_specifiers() {
		let name = CString::new("frame").unwrap();
		assert_eq!(sprintf!("%s=%d", name.as_ptr(), 7i32), "frame=7");
	}

	#[test]
	fn sprintf_literal_percent() {
		assert_eq!(sprintf!("100%% done"), "100% done");
	}

	#[test]
	fn sprintf_precision_width_and_length() {
		let s = CString::new("abcdef").unwrap();
		assert_eq!(
			sprintf!(
				"%.3s|%5.2f|%lld|%04x|%c",
				s.as_ptr(),
				2.5f64,
				1234567890123i64,
				255u32,
				'Z' as i32
			),
			"abc| 2.50|1234567890123|00ff|Z"
		);
	}

	#[test]
	fn sprintf_cuts_at_the_buffer_size() {
		let long = CString::new("x".repeat(300)).unwrap();
		assert_eq!(sprintf!("%s", long.as_ptr()), "x".repeat(255));
	}

	#[test]
	fn sprintf_copies_non_utf8_bytes() {
		// "\xa9 2024" (Latin-1 copyright sign), as plugins built for Windows write it.
		let fmt = c"\xa9 %d";
		let mut buffer = [0 as A_char; 256];
		let err = unsafe { sprintf_sys(buffer.as_mut_ptr(), fmt.as_ptr(), 2024i32) };
		assert_eq!(err, PF_Err_NONE);
		assert_eq!(unsafe { CStr::from_ptr(buffer.as_ptr()) }.to_bytes(), b"\xa9 2024");
	}

	#[test]
	fn sprintf_null_buffer_is_bad_callback_param() {
		let fmt = CString::new("%d").unwrap();
		let err = unsafe { sprintf_sys(std::ptr::null_mut(), fmt.as_ptr(), 1i32) };
		assert_eq!(err, PF_Err_BAD_CALLBACK_PARAM);
	}

	#[test]
	fn sprintf_null_format_is_bad_callback_param() {
		let mut buffer = [0 as A_char; 256];
		let err = unsafe { sprintf_sys(buffer.as_mut_ptr(), std::ptr::null()) };
		assert_eq!(err, PF_Err_BAD_CALLBACK_PARAM);
	}

	// Note: passing fewer args than the format has specifiers (e.g. `sprintf!("%d")`)
	// is genuine C-variadic UB - the reader pulls a nonexistent arg - so it is
	// intentionally not exercised here.

	#[test]
	fn strcpy_test() {
		let src = std::ffi::CString::new("Hello, world!").unwrap();
		let mut buffer = [0i8; 256];

		let result_ptr = unsafe { strcpy_sys(buffer.as_mut_ptr(), src.as_ptr()) };

		assert!(!result_ptr.is_null(), "Expected strcpy to return a non-null pointer");

		let result_str = unsafe { CStr::from_ptr(result_ptr).to_str().unwrap() };

		assert!(
			result_str == src.to_str().unwrap(),
			"Expected strcpy to copy the string correctly, got {:?}",
			result_str
		);
	}

	#[test]
	fn asin_test() {
		let x = 0.5;
		let result = asin_sys(x);

		assert!(
			(result - std::f64::consts::FRAC_PI_6).abs() < 1e-10,
			"Expected asin(0.5) to be close to π/6, got {}",
			result
		);
	}

	#[test]
	fn acos_test() {
		let x = 0.5;
		let result = acos_sys(x);

		assert!(
			(result - std::f64::consts::FRAC_PI_3).abs() < 1e-10,
			"Expected acos(0.5) to be close to π/3, got {}",
			result
		);
	}
}
