use std::ffi::{CStr, CString};

use crate::diagnostics::DiagnosticBuilder;

macro_rules! impl_math_sys {
	($name:ident, $func:ident, 1, $arg:literal, $diag_path:literal) => {
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
	($name:ident, $func:ident, 2, $arg1:literal, $arg2:literal, $diag_path:literal) => {
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

impl_math_sys!(atan_sys, atan, 1, "x", "InData/utils/ansi/atan");
impl_math_sys!(atan2_sys, atan2, 2, "y", "x", "InData/utils/ansi/atan2");
impl_math_sys!(ceil_sys, ceil, 1, "x", "InData/utils/ansi/ceil");
impl_math_sys!(cos_sys, cos, 1, "x", "InData/utils/ansi/cos");
impl_math_sys!(sin_sys, sin, 1, "x", "InData/utils/ansi/sin");

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
pub unsafe extern "C" fn sprintf_sys(
	arg1: *mut after_effects_sys::A_char,
	arg2: *const after_effects_sys::A_char,
	mut args: ...
) -> after_effects_sys::PF_Err {
	const SPRINTF_BUFFER_SIZE: usize = 256;

	// Safety checks
	if arg1.is_null() || arg2.is_null() {
		return after_effects_sys::PF_Err_BAD_CALLBACK_PARAM as after_effects_sys::PF_Err;
	}

	let format_str = match unsafe { CStr::from_ptr(arg2) }.to_str() {
		Ok(s) => s,
		Err(_) => {
			return after_effects_sys::PF_Err_INTERNAL_STRUCT_DAMAGED as after_effects_sys::PF_Err;
		}
	};

	// Raw implementation to handle %d and %s format specifiers
	let mut result = String::new();
	let mut chars = format_str.chars().peekable();

	#[cfg(feature = "diagnostics")]
	let mut d = DiagnosticBuilder::new();
	#[cfg(feature = "diagnostics")]
	d.set_name("InData/utils/ansi/sprintf")
		.add_arg("format", format!("{:?}", format_str));

	while let Some(c) = chars.next() {
		if c == '%' {
			if let Some(next) = chars.next() {
				match next {
					'd' => {
						// Get an integer argument
						let arg = args.arg::<i32>();
						result.push_str(&arg.to_string());
						#[cfg(feature = "diagnostics")]
						d.add_arg("arg_int", format!("{:?}", arg));
					}
					's' => {
						// Get a string argument
						let ptr = args.arg::<*const i8>();
						if !ptr.is_null() {
							match CStr::from_ptr(ptr).to_str() {
								Ok(s) => {
									result.push_str(s);
									#[cfg(feature = "diagnostics")]
									d.add_arg("arg_str", format!("{:?}", s));
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
			return after_effects_sys::PF_Err_INTERNAL_STRUCT_DAMAGED as after_effects_sys::PF_Err;
		}
	};

	let bytes = c_result.as_bytes_with_nul();
	let copy_len = bytes.len().min(SPRINTF_BUFFER_SIZE);

	// Copy bytes
	std::ptr::copy_nonoverlapping(bytes.as_ptr(), arg1 as *mut u8, copy_len);

	// Ensure null termination if we truncated or filled exactly
	// If copy_len < SPRINTF_BUFFER_SIZE, the null byte from as_bytes_with_nul is already copied.
	// If copy_len == SPRINTF_BUFFER_SIZE, we forced a cut, so we must manually null-terminate at the end.
	if copy_len == SPRINTF_BUFFER_SIZE && SPRINTF_BUFFER_SIZE > 0 {
		*((arg1 as *mut u8).add(SPRINTF_BUFFER_SIZE - 1)) = 0;
	}

	#[cfg(feature = "diagnostics")]
	d.set_result(format!("{:?}", c_result)).emit();

	after_effects_sys::PF_Err_NONE as after_effects_sys::PF_Err
}
