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
pub(crate) fn f32_to_fixed16(x: f32) -> i32 {
	(x * 65536.0_f32).round_ties_even() as i32
}

#[cfg(test)]
mod tests {
	use super::*;

	/// 2^-16: the smallest positive step of a 16.16 fixed-point number.
	const STEP: f32 = 1.0 / 65536.0;

	/// Largest `f32` strictly below 32768.0 (the highest value `f32` can feed
	/// into the 16.16 range without saturating).
	fn largest_f32_below_32768() -> f32 {
		f32::from_bits(32768.0_f32.to_bits() - 1)
	}

	#[test]
	fn test_fixed16_known_values() {
		// (float, fixed) pairs that are exactly representable in both formats.
		let table: [(f32, i32); 8] = [
			(0.0, 0),
			(1.0, 0x0001_0000),
			(-1.0, -0x0001_0000),
			(0.5, 0x0000_8000),
			(-0.5, -0x0000_8000),
			(3.75, 0x0003_C000),
			(-2.25, -0x0002_4000),
			(STEP, 1),
		];
		for &(f, x) in &table {
			assert_eq!(f32_to_fixed16(f), x, "f32_to_fixed16({f})");
			assert_eq!(fixed16_to_f32(x), f, "fixed16_to_f32({x:#x})");
		}
	}

	#[test]
	fn test_fixed16_ties_round_to_even() {
		// Exact ties (x.5 steps) round to the even neighbour, on both signs.
		let table: [(f32, i32); 8] = [
			(0.5, 0),
			(1.5, 2),
			(2.5, 2),
			(3.5, 4),
			(-0.5, 0),
			(-1.5, -2),
			(-2.5, -2),
			(-3.5, -4),
		];
		for &(steps, expected) in &table {
			assert_eq!(f32_to_fixed16(steps * STEP), expected, "{steps} steps");
		}
	}

	#[test]
	fn test_fixed16_rounds_non_ties_to_nearest() {
		assert_eq!(f32_to_fixed16(0.49 * STEP), 0);
		assert_eq!(f32_to_fixed16(0.51 * STEP), 1);
		assert_eq!(f32_to_fixed16(-0.49 * STEP), 0);
		assert_eq!(f32_to_fixed16(-0.51 * STEP), -1);
	}

	#[test]
	fn test_fixed16_f32_roundtrip_error_is_half_a_step() {
		let values = [
			0.0_f32,
			0.1,
			-0.1,
			1.0 / 3.0,
			3.75,
			100.0,
			359.9,
			-359.9,
			32767.0,
			-32768.0,
		];
		for &v in &values {
			let err = (fixed16_to_f32(f32_to_fixed16(v)) - v).abs();
			assert!(err <= 0.5 * STEP, "roundtrip error too large for {v}: {err}");
		}
	}

	#[test]
	fn test_fixed16_fixed_roundtrip_is_exact_within_f32_precision() {
		// Every integer with |x| <= 2^24 is exactly representable in f32, so
		// fixed -> f32 -> fixed must be lossless.
		const LIMIT: i32 = 1 << 24;
		let mut x = -LIMIT;
		while x <= LIMIT {
			assert_eq!(f32_to_fixed16(fixed16_to_f32(x)), x);
			x += 4099;
		}
		for x in [-LIMIT, -1, 0, 1, LIMIT] {
			assert_eq!(f32_to_fixed16(fixed16_to_f32(x)), x);
		}
	}

	#[test]
	fn test_fixed16_range_edges() {
		// Lower bound is exact.
		assert_eq!(f32_to_fixed16(-32768.0), i32::MIN);
		assert_eq!(fixed16_to_f32(i32::MIN), -32768.0);

		// Largest f32 below 32768.0 is 32768 - 2^-9, i.e. 0x7FFF_FF80.
		assert_eq!(f32_to_fixed16(largest_f32_below_32768()), 0x7FFF_FF80);

		// i32::MAX is not representable in f32 and rounds up to 2^31 (precision
		// loss documented on `fixed16_to_f32`).
		assert_eq!(fixed16_to_f32(i32::MAX), 32768.0);
	}

	#[test]
	fn test_fixed16_saturates_out_of_range() {
		assert_eq!(f32_to_fixed16(32768.0), i32::MAX);
		assert_eq!(f32_to_fixed16(1.0e10), i32::MAX);
		assert_eq!(f32_to_fixed16(f32::MAX), i32::MAX);
		assert_eq!(f32_to_fixed16(f32::INFINITY), i32::MAX);

		assert_eq!(f32_to_fixed16(-32769.0), i32::MIN);
		assert_eq!(f32_to_fixed16(-1.0e10), i32::MIN);
		assert_eq!(f32_to_fixed16(f32::MIN), i32::MIN);
		assert_eq!(f32_to_fixed16(f32::NEG_INFINITY), i32::MIN);
	}

	#[test]
	fn test_fixed16_nan_becomes_zero() {
		assert_eq!(f32_to_fixed16(f32::NAN), 0);
	}
}
