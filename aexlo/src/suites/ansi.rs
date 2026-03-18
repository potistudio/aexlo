use std::ffi::{CStr, CString};

use after_effects_sys::{A_char, PF_Err, PF_Err_BAD_CALLBACK_PARAM, PF_Err_INTERNAL_STRUCT_DAMAGED, PF_Err_NONE};

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

/// Emulates `sprintf()` function
///
/// # Safety
///
/// This function is unsafe because:
/// 1. It handles raw pointers (`arg1`, `arg2`, and variadic arguments) which must be valid.
/// 2. `arg1` is assumed to be a pointer to a mutable buffer of at least 256 bytes.
///    The function will strictly write no more than 256 bytes (including null terminator),
///    but the caller is responsible for ensuring the buffer allocation.
/// 3. `arg2` must be a valid, null-terminated C-string.
/// 4. Variadic arguments must correspond exactly to the format specifiers in `arg2`.
///    Mismatching arguments (e.g. passing a float for `%d`) leads to undefined behavior.
/// 5. Only `%d` and `%s` format specifiers are actively supported.
pub(crate) unsafe extern "C" fn sprintf_sys(arg1: *mut A_char, arg2: *const A_char, mut args: ...) -> PF_Err {
	const SPRINTF_BUFFER_SIZE: usize = 256;

	if arg1.is_null() || arg2.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	let format_str = match unsafe { CStr::from_ptr(arg2) }.to_str() {
		Ok(s) => s,
		Err(_) => {
			return PF_Err_INTERNAL_STRUCT_DAMAGED as PF_Err;
		}
	};

	// Raw implementation to handle %d and %s format specifiers
	let mut result = String::new();
	let mut chars = format_str.chars().peekable();

	let mut diagnostics = DiagnosticBuilder::new();
	diagnostics
		.set_name("UtilityCallbacks/ansi/sprintf")
		.add_arg("format", format!("{:?}", format_str));

	while let Some(c) = chars.next() {
		if c == '%' {
			if let Some(next) = chars.next() {
				match next {
					'd' => {
						// Get an integer argument
						let arg = unsafe { args.arg::<i32>() };

						diagnostics.add_arg("arg_int", format!("{:?}", arg));
						result.push_str(&arg.to_string());
					}
					's' => {
						// Get a string argument
						let ptr = unsafe { args.arg::<*const i8>() };

						if !ptr.is_null() {
							match unsafe { CStr::from_ptr(ptr) }.to_str() {
								Ok(s) => {
									result.push_str(s);
									diagnostics.add_arg("arg_str", format!("{:?}", s));
								}
								Err(_) => result.push_str("(invalid)"),
							}
						} else {
							result.push_str("(null)");
						}
					}
					'%' => {
						result.push('%');
					}
					_ => {
						// Unsupported format specifier, just include it as-is
						result.push('%');
						result.push(next);
					}
				}
			} else {
				// Trailing %
				result.push('%');
			}
		} else {
			result.push(c);
		}
	}

	// Copy result to the output buffer
	let c_result = match CString::new(result) {
		Ok(s) => s,
		Err(_) => {
			return PF_Err_INTERNAL_STRUCT_DAMAGED as PF_Err;
		}
	};

	let bytes = c_result.as_bytes_with_nul();
	let copy_len = bytes.len().min(SPRINTF_BUFFER_SIZE);

	// Copy bytes
	unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), arg1 as *mut u8, copy_len) };

	// Ensure null termination if we truncated or filled exactly
	// If copy_len < SPRINTF_BUFFER_SIZE, the null byte from as_bytes_with_nul is already copied.
	// If copy_len == SPRINTF_BUFFER_SIZE, we forced a cut, so we must manually null-terminate at the end.
	if copy_len == SPRINTF_BUFFER_SIZE && SPRINTF_BUFFER_SIZE > 0 {
		unsafe { *((arg1 as *mut u8).add(SPRINTF_BUFFER_SIZE - 1)) = 0 };
	}

	diagnostics.set_result(format!("{:?}", c_result)).emit();

	PF_Err_NONE as PF_Err
}

pub unsafe extern "C" fn strcpy_sys(arg1: *mut A_char, arg2: *const A_char) -> *mut A_char {
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

	#[test]
	fn sprintf_test() {
		//TODO: Implement tests for sprintf_sys, which is more complex due to variadic arguments and format parsing.
	}

	#[test]
	fn strcpy_test() {
		let src = "Hello, world!";
		let mut buffer = [0i8; 256];

		let result_ptr = unsafe { strcpy_sys(buffer.as_mut_ptr(), src.as_ptr() as *const i8) };

		assert!(!result_ptr.is_null(), "Expected strcpy to return a non-null pointer");

		let result_str = unsafe { CStr::from_ptr(result_ptr).to_str().unwrap() };

		assert!(
			result_str == src,
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
