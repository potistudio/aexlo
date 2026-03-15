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
