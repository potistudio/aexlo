/// Converts a signed 16.16 fixed-point number (`i32`) to an `f32`.
///
/// The value uses 16 bits for the integral part and 16 bits for the fractional
/// part, scaled by a factor of $2^{16}$ (65,536).
///
/// # Precision
///
/// Because `f32` has a 24-bit significand (including the implicit leading bit),
/// converting values that use the lower bits of the 16-bit fractional part may
/// incur minor floating-point rounding errors.
#[inline]
pub(crate) fn fixed16_to_f32(x: i32) -> f32 {
	x as f32 / 65536.0_f32
}

/// Converts an `f32` to a signed 16.16 fixed-point number (`i32`).
///
/// Multiplies the input by $2^{16}$ (65,536), rounds using round-to-even
/// (banker's rounding), and casts the result to `i32`.
///
/// # Rounding
///
/// Uses [`f32::round_ties_even`]. Values halfway between two integers
/// are rounded toward the nearest even number to avoid directional bias.
///
/// # Range and Overflow
///
/// * Representable domain: approximately `[-32768.0, 32767.9999847]`.
/// * Values outside this range, as well as `f32::NAN` and `f32::INFINITY`,
///   saturate according to Rust's floating-point-to-integer cast semantics
///   (`i32::MIN`, `i32::MAX`, or `0` for `NAN`).
#[inline]
pub fn f32_to_fixed16(x: f32) -> i32 {
	(x * 65536.0_f32).round_ties_even() as i32
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_fixed16_one() {
		assert_eq!(f32_to_fixed16(1.0), 0x0001_0000);
		assert_eq!(fixed16_to_f32(0x0001_0000), 1.0);
	}

	#[test]
	fn test_fixed16_half() {
		assert_eq!(f32_to_fixed16(0.5), 0x0000_8000);
	}

	#[test]
	fn test_fixed16_negative() {
		assert_eq!(fixed16_to_f32(f32_to_fixed16(-2.25)), -2.25);
	}

	#[test]
	fn test_fixed16_known_values() {
		assert_eq!(fixed16_to_f32(0x0000_8000), 0.5);
		assert_eq!(fixed16_to_f32(-0x0001_0000), -1.0);
		assert_eq!(f32_to_fixed16(-1.0), -0x0001_0000);

		// Smallest positive step (2^-16).
		assert_eq!(fixed16_to_f32(1), 0.000015258789);
	}

	#[test]
	fn test_fixed16_ties_round_to_even() {
		// 0.5 and 1.5 steps are exact ties, so they round to the even neighbour.
		assert_eq!(f32_to_fixed16(0.5 / 65536.0), 0);
		assert_eq!(f32_to_fixed16(1.5 / 65536.0), 2);
	}

	#[test]
	fn test_fixed16_roundtrip() {
		let values = [0.0_f32, 0.5, -0.5, 3.75, 100.0, -359.9];
		for &v in &values {
			let err = (fixed16_to_f32(f32_to_fixed16(v)) - v).abs();
			assert!(err < 1e-4, "roundtrip error too large for {v}: {err}");
		}
	}
}
