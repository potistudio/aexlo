/// `PF_Fixed`（16.16 固定小数点, i32）を f32 に変換する。
///
/// FIX_SLIDER / ANGLE / POINT パラメーターの値はこの 16.16 形式で格納される。
/// 例: `0x0001_0000` → `1.0`。
#[inline]
pub fn fixed16_to_f32(x: i32) -> f32 {
	x as f32 / 65536.0_f32
}

/// f32 を `PF_Fixed`（16.16 固定小数点, i32）に変換する。[`fixed16_to_f32`] の逆。
#[inline]
pub fn f32_to_fixed16(x: f32) -> i32 {
	(x * 65536.0_f32).round() as i32
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
	fn test_fixed16_roundtrip() {
		let values = [0.0_f32, 0.5, -0.5, 3.75, 100.0, -359.9];
		for &v in &values {
			let err = (fixed16_to_f32(f32_to_fixed16(v)) - v).abs();
			assert!(err < 1e-4, "roundtrip error too large for {v}: {err}");
		}
	}
}
