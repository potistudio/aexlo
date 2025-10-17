use std::ffi::{CStr, CString};

use crate::diagnostics::DiagnosticBuilder;

/// Raw `atan()` function implementation
pub(crate) extern "C" fn atan_sys(x: f64) -> f64 {
	let result = x.atan();

	#[cfg(feature = "diagnostics")]
	DiagnosticBuilder::new()
		.set_name("InData/utils/ansi/atan")
		.add_arg("x", x)
		.set_result(result)
		.emit();

	result
}

/// Wrapper for `atan()` function
pub(crate) fn atan(x: f64) -> f64 {
	x.atan()
}

/// Raw `atan2()` function implementation
pub(crate) extern "C" fn atan2_sys(y: f64, x: f64) -> f64 {
	let result = y.atan2(x);

	#[cfg(feature = "diagnostics")]
	DiagnosticBuilder::new()
		.set_name("InData/utils/ansi/atan2")
		.add_arg("y", y)
		.add_arg("x", x)
		.set_result(result)
		.emit();

	result
}

/// Wrapper for `atan2()` function
pub(crate) fn atan2(y: f64, x: f64) -> f64 {
	y.atan2(x)
}

/// Raw `ceil()` function implementation
pub(crate) extern "C" fn ceil_sys(x: f64) -> f64 {
	let result = x.ceil();

	#[cfg(feature = "diagnostics")]
	DiagnosticBuilder::new()
		.set_name("InData/utils/ansi/ceil")
		.add_arg("x", x)
		.set_result(result)
		.emit();

	result
}

/// Wrapper for `ceil()` function
pub(crate) fn ceil(x: f64) -> f64 {
	x.ceil()
}

/// Raw `cos()` function implementation
#[inline(always)]
pub(crate) extern "C" fn cos_sys(x: f64) -> f64 {
	let result = x.cos();

	#[cfg(feature = "diagnostics")]
	DiagnosticBuilder::new()
		.set_name("InData/utils/ansi/cos")
		.add_arg("x", x)
		.set_result(result)
		.emit();

	result
}

/// Wrapper for `cos()` function
pub(crate) fn cos(x: f64) -> f64 {
	x.cos()
}

/// Raw `sin()` function implementation
#[inline(always)]
pub(crate) extern "C" fn sin_sys(x: f64) -> f64 {
	let result = x.sin();

	#[cfg(feature = "diagnostics")]
	DiagnosticBuilder::new()
		.set_name("InData/utils/ansi/sin")
		.add_arg("x", x)
		.set_result(result)
		.emit();

	result
}

/// Wrapper for `sin()` function
pub(crate) fn sin(x: f64) -> f64 {
	x.sin()
}

/// Emulates `sprintf()` function
///
/// # Safety
///
/// This function is unsafe because it handles raw pointers.
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

	let mut d = DiagnosticBuilder::new();
	d.set_name("InData/utils/ansi/sin")
		.add_arg("arg1", format!("{:?}", format_str));

	while let Some(c) = chars.next() {
		if c == '%' {
			if let Some(next) = chars.next() {
				match next {
					'd' => {
						// Get an integer argument
						let arg = unsafe { args.arg::<i32>() };
						result.push_str(&arg.to_string());
						d.add_arg("arg", format!("{:?}", arg));
					}
					's' => {
						// Get a string argument
						let ptr = unsafe { args.arg::<*const i8>() };
						if !ptr.is_null() {
							match unsafe { CStr::from_ptr(ptr) }.to_str() {
								Ok(s) => {
									result.push_str(s);
									d.add_arg("arg", format!("{:?}", s));
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
			}
		} else {
			result.push(c);
		}
	}

	println!(
		"sprintf called with format: {:?}, result: {:?}",
		format_str, result
	);

	// Copy result to the output buffer
	let c_result = match CString::new(result) {
		Ok(s) => s,
		Err(_) => {
			eprintln!("[ERROR] sprintf: Formatted string contains NUL bytes");
			return after_effects_sys::PF_Err_INTERNAL_STRUCT_DAMAGED as after_effects_sys::PF_Err;
		}
	};

	let bytes = c_result.as_bytes_with_nul();
	let copy_len = bytes.len().min(SPRINTF_BUFFER_SIZE);
	unsafe {
		std::ptr::copy_nonoverlapping(bytes.as_ptr(), arg1 as *mut u8, copy_len);

		// Ensure null termination if we hit buffer limit
		if copy_len == SPRINTF_BUFFER_SIZE && copy_len > 0 {
			*((arg1 as *mut u8).add(SPRINTF_BUFFER_SIZE - 1)) = 0;
		}
	}

	d.set_result(format!("{:?}", c_result)).emit();

	after_effects_sys::PF_Err_NONE as after_effects_sys::PF_Err
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_sin() {
		let angle = std::f64::consts::PI / 2.0; // 90 degrees
		let result = sin(angle);
		assert!(
			(result - 1.0).abs() < 1e-10,
			"sin(π/2) should be approximately 1.0"
		);
	}

	#[test]
	fn test_cos() {
		let angle = std::f64::consts::PI; // 180 degrees
		let result = cos(angle);
		assert!(
			(result + 1.0).abs() < 1e-10,
			"cos(π) should be approximately -1.0"
		);
	}
}
