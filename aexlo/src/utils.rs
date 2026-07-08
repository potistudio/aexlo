/// f32 を Q31 固定小数点数 (i32) に変換する
/// 範囲: [-1.0, 1.0) → [-2147483648, 2147483647]
#[inline]
pub fn f32_to_q31(x: f32) -> i32 {
	// クランプして [-1.0, 1.0) に収める
	let clamped = x.clamp(-1.0, 1.0 - f32::EPSILON);
	(clamped * 2147483648.0_f32) as i32
}

/// Q31 固定小数点数 (i32) を f32 に変換する
#[inline]
pub fn q31_to_f32(x: i32) -> f32 {
	x as f32 / 2147483648.0_f32
}

/// `PF_Fixed`（16.16 固定小数点, i32）を f32 に変換する。
///
/// ANGLE / POINT パラメーターの値はこの 16.16 形式で格納される（Q31 の
/// [`q31_to_f32`] とは別物）。例: `0x0001_0000` → `1.0`。
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
	fn test_zero() {
		assert_eq!(f32_to_q31(0.0), 0);
	}

	#[test]
	fn test_positive_one() {
		// 1.0 はクランプ → f32精度の都合で i32::MAX にはならない
		// 実際の最大値を確認するだけ
		let result = f32_to_q31(1.0);
		assert!(result > 0);
	}

	#[test]
	fn test_negative_one() {
		assert_eq!(f32_to_q31(-1.0), i32::MIN);
	}

	#[test]
	fn test_half() {
		assert_eq!(f32_to_q31(0.5), 1073741824);
	}

	#[test]
	fn test_roundtrip() {
		let values = [0.0_f32, 0.5, -0.5, 0.25, -0.999];
		for &v in &values {
			let err = (q31_to_f32(f32_to_q31(v)) - v).abs();
			assert!(err < 1e-7, "roundtrip error too large for {v}: {err}");
		}
	}
}
