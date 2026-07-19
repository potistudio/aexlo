//! Color-space conversion callbacks: `PF_ColorCallbacksSuite1` (8-bpc),
//! `PF_ColorCallbacks16Suite1` and `PF_ColorCallbacksFloatSuite1`, plus the
//! same functions for the legacy `PF_UtilCallbacks::colorCB` block (whose
//! layout matches the 8-bpc suite exactly).
//!
//! All three depths share one `f64` core; only the pixel type at the FFI
//! boundary differs. Value conventions follow the SDK docs:
//!
//! - `RGBtoHLS` / `HLStoRGB`: hue, lightness and saturation each scaled to
//!   `0..=1` as 16.16 fixed point (hue `1.0` = 360°).
//! - `RGBtoYIQ` / `YIQtoRGB`: Y in `0..=1`, I in `±0.5957`, Q in `±0.5226`,
//!   as 16.16 fixed point (NTSC / Rec. 601 weights).
//! - `Luminance`: 100 × the 8-bit-scaled luminance (`0..=25500`).
//! - `Hue`: hue angle mapped to `0..=255` (255 = 360°).
//! - `Lightness` / `Saturation`: scaled to `0..=255`.
//!
//! `effect_ref` is deliberately not validated: the math needs no host state,
//! and plugins may call these during setup phases where the host has not put
//! anything meaningful in `in_data.effect_ref` yet.

use crate::core::diagnostics::diag;
use after_effects_sys::{
	A_long, PF_ColorCallbacks16Suite1, PF_ColorCallbacksFloatSuite1, PF_ColorCallbacksSuite1, PF_Err,
	PF_Err_BAD_CALLBACK_PARAM, PF_Err_NONE, PF_Fixed, PF_Pixel, PF_Pixel16, PF_PixelFloat, PF_ProgPtr,
};

use super::pixel_norm::NormalizedPixel;

// ============================================================================
// Depth-independent color math (normalized f64 channels)
// ============================================================================

/// RGB (each `0..=1`) → HLS (hue as fraction of a turn, lightness, saturation,
/// each `0..=1`), the classic Foley–van Dam bi-hexcone model AE uses.
pub(crate) fn rgb_to_hls([r, g, b]: [f64; 3]) -> [f64; 3] {
	let max = r.max(g).max(b);
	let min = r.min(g).min(b);
	let l = (max + min) / 2.0;

	if max == min {
		// Achromatic: hue is conventionally 0.
		return [0.0, l, 0.0];
	}

	let d = max - min;
	let s = if l <= 0.5 { d / (max + min) } else { d / (2.0 - max - min) };

	let h6 = if max == r {
		((g - b) / d).rem_euclid(6.0)
	} else if max == g {
		(b - r) / d + 2.0
	} else {
		(r - g) / d + 4.0
	};

	[h6 / 6.0, l, s]
}

/// HLS (hue as fraction of a turn, each `0..=1`) → RGB (each `0..=1`).
pub(crate) fn hls_to_rgb([h, l, s]: [f64; 3]) -> [f64; 3] {
	if s <= 0.0 {
		return [l, l, l];
	}

	let q = if l <= 0.5 { l * (1.0 + s) } else { l + s - l * s };
	let p = 2.0 * l - q;

	let channel = |mut t: f64| -> f64 {
		t = t.rem_euclid(1.0);
		if t < 1.0 / 6.0 {
			p + (q - p) * 6.0 * t
		} else if t < 0.5 {
			q
		} else if t < 2.0 / 3.0 {
			p + (q - p) * (2.0 / 3.0 - t) * 6.0
		} else {
			p
		}
	};

	[channel(h + 1.0 / 3.0), channel(h), channel(h - 1.0 / 3.0)]
}

/// RGB (each `0..=1`) → YIQ, NTSC (Rec. 601) weights.
pub(crate) fn rgb_to_yiq([r, g, b]: [f64; 3]) -> [f64; 3] {
	[
		0.299 * r + 0.587 * g + 0.114 * b,
		0.595716 * r - 0.274453 * g - 0.321263 * b,
		0.211456 * r - 0.522591 * g + 0.311135 * b,
	]
}

/// YIQ → RGB; exact inverse of [`rgb_to_yiq`].
pub(crate) fn yiq_to_rgb([y, i, q]: [f64; 3]) -> [f64; 3] {
	[
		y + 0.956296 * i + 0.621024 * q,
		y - 0.272122 * i - 0.647381 * q,
		y - 1.106989 * i + 1.704615 * q,
	]
}

/// Normalized value → 16.16 fixed point, rounded.
fn to_fixed(v: f64) -> PF_Fixed {
	(v * 65536.0).round() as PF_Fixed
}

/// 16.16 fixed point → normalized value.
fn from_fixed(v: PF_Fixed) -> f64 {
	v as f64 / 65536.0
}

/// Scalar result channel: `A_long` for the integer suites, `f32` for the float
/// suite. `v` arrives pre-scaled (e.g. already `0..=255`); integers round.
trait ColorScalar {
	fn from_f64(v: f64) -> Self;
}

impl ColorScalar for A_long {
	fn from_f64(v: f64) -> Self {
		v.round() as A_long
	}
}

impl ColorScalar for f32 {
	fn from_f64(v: f64) -> Self {
		v as f32
	}
}

// ============================================================================
// FFI entry points (macro-generated per depth)
// ============================================================================

/// Generates the eight color-callback entry points for one pixel depth.
macro_rules! define_color_callbacks {
	(
		$pix:ty, $scalar:ty, $diag_prefix:literal,
		$rgb_to_hls:ident, $hls_to_rgb:ident, $rgb_to_yiq:ident, $yiq_to_rgb:ident,
		$luminance:ident, $hue:ident, $lightness:ident, $saturation:ident $(,)?
	) => {
		pub(crate) unsafe extern "C" fn $rgb_to_hls(
			_effect_ref: PF_ProgPtr,
			rgb: *mut $pix,
			hls: *mut PF_Fixed,
		) -> PF_Err {
			if rgb.is_null() || hls.is_null() {
				log::error!(concat!($diag_prefix, "/RGBtoHLS: null argument"));
				return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
			}
			let [_, r, g, b] = unsafe { *rgb }.to_norm();
			let out = rgb_to_hls([r, g, b]);
			for (i, v) in out.iter().enumerate() {
				unsafe { *hls.add(i) = to_fixed(*v) };
			}
			diag!(concat!($diag_prefix, "/RGBtoHLS"), "rgb" => format!("{:?}", (r, g, b)));
			PF_Err_NONE as PF_Err
		}

		pub(crate) unsafe extern "C" fn $hls_to_rgb(
			_effect_ref: PF_ProgPtr,
			hls: *mut PF_Fixed,
			rgb: *mut $pix,
		) -> PF_Err {
			if rgb.is_null() || hls.is_null() {
				log::error!(concat!($diag_prefix, "/HLStoRGB: null argument"));
				return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
			}
			let h = from_fixed(unsafe { *hls });
			let l = from_fixed(unsafe { *hls.add(1) });
			let s = from_fixed(unsafe { *hls.add(2) });
			let [r, g, b] = hls_to_rgb([h, l, s]);
			// The callback carries no source alpha, so the result is fully opaque.
			unsafe { *rgb = <$pix>::from_norm([<$pix>::OPAQUE, r, g, b]) };
			diag!(concat!($diag_prefix, "/HLStoRGB"), "hls" => format!("{:?}", (h, l, s)));
			PF_Err_NONE as PF_Err
		}

		pub(crate) unsafe extern "C" fn $rgb_to_yiq(
			_effect_ref: PF_ProgPtr,
			rgb: *mut $pix,
			yiq: *mut PF_Fixed,
		) -> PF_Err {
			if rgb.is_null() || yiq.is_null() {
				log::error!(concat!($diag_prefix, "/RGBtoYIQ: null argument"));
				return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
			}
			let [_, r, g, b] = unsafe { *rgb }.to_norm();
			let out = rgb_to_yiq([r, g, b]);
			for (i, v) in out.iter().enumerate() {
				unsafe { *yiq.add(i) = to_fixed(*v) };
			}
			diag!(concat!($diag_prefix, "/RGBtoYIQ"), "rgb" => format!("{:?}", (r, g, b)));
			PF_Err_NONE as PF_Err
		}

		pub(crate) unsafe extern "C" fn $yiq_to_rgb(
			_effect_ref: PF_ProgPtr,
			yiq: *mut PF_Fixed,
			rgb: *mut $pix,
		) -> PF_Err {
			if rgb.is_null() || yiq.is_null() {
				log::error!(concat!($diag_prefix, "/YIQtoRGB: null argument"));
				return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
			}
			let y = from_fixed(unsafe { *yiq });
			let i = from_fixed(unsafe { *yiq.add(1) });
			let q = from_fixed(unsafe { *yiq.add(2) });
			let [r, g, b] = yiq_to_rgb([y, i, q]);
			unsafe { *rgb = <$pix>::from_norm([<$pix>::OPAQUE, r, g, b]) };
			diag!(concat!($diag_prefix, "/YIQtoRGB"), "yiq" => format!("{:?}", (y, i, q)));
			PF_Err_NONE as PF_Err
		}

		pub(crate) unsafe extern "C" fn $luminance(
			_effect_ref: PF_ProgPtr,
			rgb: *mut $pix,
			lum100: *mut $scalar,
		) -> PF_Err {
			if rgb.is_null() || lum100.is_null() {
				log::error!(concat!($diag_prefix, "/Luminance: null argument"));
				return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
			}
			let [_, r, g, b] = unsafe { *rgb }.to_norm();
			let [y, _, _] = rgb_to_yiq([r, g, b]);
			// "100 times the luminance": the 8-bit-scaled luminance times 100.
			unsafe { *lum100 = <$scalar>::from_f64(y * 255.0 * 100.0) };
			PF_Err_NONE as PF_Err
		}

		pub(crate) unsafe extern "C" fn $hue(
			_effect_ref: PF_ProgPtr,
			rgb: *mut $pix,
			hue: *mut $scalar,
		) -> PF_Err {
			if rgb.is_null() || hue.is_null() {
				log::error!(concat!($diag_prefix, "/Hue: null argument"));
				return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
			}
			let [_, r, g, b] = unsafe { *rgb }.to_norm();
			let [h, _, _] = rgb_to_hls([r, g, b]);
			// Hue angle mapped onto 0..=255 (255 = 360 degrees).
			unsafe { *hue = <$scalar>::from_f64(h * 255.0) };
			PF_Err_NONE as PF_Err
		}

		pub(crate) unsafe extern "C" fn $lightness(
			_effect_ref: PF_ProgPtr,
			rgb: *mut $pix,
			lightness: *mut $scalar,
		) -> PF_Err {
			if rgb.is_null() || lightness.is_null() {
				log::error!(concat!($diag_prefix, "/Lightness: null argument"));
				return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
			}
			let [_, r, g, b] = unsafe { *rgb }.to_norm();
			let [_, l, _] = rgb_to_hls([r, g, b]);
			unsafe { *lightness = <$scalar>::from_f64(l * 255.0) };
			PF_Err_NONE as PF_Err
		}

		pub(crate) unsafe extern "C" fn $saturation(
			_effect_ref: PF_ProgPtr,
			rgb: *mut $pix,
			saturation: *mut $scalar,
		) -> PF_Err {
			if rgb.is_null() || saturation.is_null() {
				log::error!(concat!($diag_prefix, "/Saturation: null argument"));
				return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
			}
			let [_, r, g, b] = unsafe { *rgb }.to_norm();
			let [_, _, s] = rgb_to_hls([r, g, b]);
			unsafe { *saturation = <$scalar>::from_f64(s * 255.0) };
			PF_Err_NONE as PF_Err
		}
	};
}

define_color_callbacks!(
	PF_Pixel,
	A_long,
	"ColorCallbacksSuite",
	rgb_to_hls_8_sys,
	hls_to_rgb_8_sys,
	rgb_to_yiq_8_sys,
	yiq_to_rgb_8_sys,
	luminance_8_sys,
	hue_8_sys,
	lightness_8_sys,
	saturation_8_sys,
);

define_color_callbacks!(
	PF_Pixel16,
	A_long,
	"ColorCallbacks16Suite",
	rgb_to_hls_16_sys,
	hls_to_rgb_16_sys,
	rgb_to_yiq_16_sys,
	yiq_to_rgb_16_sys,
	luminance_16_sys,
	hue_16_sys,
	lightness_16_sys,
	saturation_16_sys,
);

define_color_callbacks!(
	PF_PixelFloat,
	f32,
	"ColorCallbacksFloatSuite",
	rgb_to_hls_float_sys,
	hls_to_rgb_float_sys,
	rgb_to_yiq_float_sys,
	yiq_to_rgb_float_sys,
	luminance_float_sys,
	hue_float_sys,
	lightness_float_sys,
	saturation_float_sys,
);

// ============================================================================
// Factory Functions
// ============================================================================

/// Builds the `PF_ColorCallbacksSuite1` (8-bpc) vtable.
///
/// `const` so it can initialize the shared [`SUITE_CONTAINER`](crate::suites::SUITE_CONTAINER)
/// static; the suite is a stateless table of function pointers.
pub const fn create_color_callbacks_suite_1() -> PF_ColorCallbacksSuite1 {
	PF_ColorCallbacksSuite1 {
		RGBtoHLS: Some(rgb_to_hls_8_sys),
		HLStoRGB: Some(hls_to_rgb_8_sys),
		RGBtoYIQ: Some(rgb_to_yiq_8_sys),
		YIQtoRGB: Some(yiq_to_rgb_8_sys),
		Luminance: Some(luminance_8_sys),
		Hue: Some(hue_8_sys),
		Lightness: Some(lightness_8_sys),
		Saturation: Some(saturation_8_sys),
	}
}

/// Builds the `PF_ColorCallbacks16Suite1` vtable.
///
/// `const` so it can initialize the shared [`SUITE_CONTAINER`](crate::suites::SUITE_CONTAINER)
/// static; the suite is a stateless table of function pointers.
pub const fn create_color_callbacks_16_suite_1() -> PF_ColorCallbacks16Suite1 {
	PF_ColorCallbacks16Suite1 {
		RGBtoHLS: Some(rgb_to_hls_16_sys),
		HLStoRGB: Some(hls_to_rgb_16_sys),
		RGBtoYIQ: Some(rgb_to_yiq_16_sys),
		YIQtoRGB: Some(yiq_to_rgb_16_sys),
		Luminance: Some(luminance_16_sys),
		Hue: Some(hue_16_sys),
		Lightness: Some(lightness_16_sys),
		Saturation: Some(saturation_16_sys),
	}
}

/// Builds the `PF_ColorCallbacksFloatSuite1` vtable.
///
/// `const` so it can initialize the shared [`SUITE_CONTAINER`](crate::suites::SUITE_CONTAINER)
/// static; the suite is a stateless table of function pointers.
pub const fn create_color_callbacks_float_suite_1() -> PF_ColorCallbacksFloatSuite1 {
	PF_ColorCallbacksFloatSuite1 {
		RGBtoHLS: Some(rgb_to_hls_float_sys),
		HLStoRGB: Some(hls_to_rgb_float_sys),
		RGBtoYIQ: Some(rgb_to_yiq_float_sys),
		YIQtoRGB: Some(yiq_to_rgb_float_sys),
		Luminance: Some(luminance_float_sys),
		Hue: Some(hue_float_sys),
		Lightness: Some(lightness_float_sys),
		Saturation: Some(saturation_float_sys),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	const EPS: f64 = 1e-6;

	fn assert_close(a: [f64; 3], b: [f64; 3], eps: f64) {
		for (x, y) in a.iter().zip(b.iter()) {
			assert!((x - y).abs() < eps, "{a:?} != {b:?}");
		}
	}

	#[test]
	fn hls_math_matches_known_colors() {
		// Pure red: hue 0, lightness 0.5, saturation 1.
		assert_close(rgb_to_hls([1.0, 0.0, 0.0]), [0.0, 0.5, 1.0], EPS);
		// Pure green: hue 1/3 of a turn.
		assert_close(rgb_to_hls([0.0, 1.0, 0.0]), [1.0 / 3.0, 0.5, 1.0], EPS);
		// Pure blue: hue 2/3 of a turn.
		assert_close(rgb_to_hls([0.0, 0.0, 1.0]), [2.0 / 3.0, 0.5, 1.0], EPS);
		// Mid gray: achromatic.
		assert_close(rgb_to_hls([0.5, 0.5, 0.5]), [0.0, 0.5, 0.0], EPS);
	}

	#[test]
	fn hls_round_trips_rgb() {
		for rgb in [[0.8, 0.2, 0.4], [0.1, 0.9, 0.3], [0.0, 0.0, 0.0], [1.0, 1.0, 1.0]] {
			assert_close(hls_to_rgb(rgb_to_hls(rgb)), rgb, 1e-9);
		}
	}

	#[test]
	fn yiq_round_trips_rgb() {
		for rgb in [[1.0, 0.0, 0.0], [0.25, 0.5, 0.75], [1.0, 1.0, 1.0]] {
			assert_close(yiq_to_rgb(rgb_to_yiq(rgb)), rgb, 1e-4);
		}
		// White has Y=1 and no chroma.
		assert_close(rgb_to_yiq([1.0, 1.0, 1.0]), [1.0, 0.0, 0.0], 1e-4);
	}

	#[test]
	fn suite_converts_8bit_red_to_hls_fixed() {
		let mut rgb = PF_Pixel {
			alpha: 255,
			red: 255,
			green: 0,
			blue: 0,
		};
		let mut hls = [0 as PF_Fixed; 3];
		let err = unsafe { rgb_to_hls_8_sys(std::ptr::null_mut(), &mut rgb, hls.as_mut_ptr()) };
		assert_eq!(err, PF_Err_NONE as PF_Err);
		// [0, 0.5, 1.0] in 16.16 fixed point.
		assert_eq!(hls, [0, 32768, 65536]);
	}

	#[test]
	fn suite_round_trips_hls_to_rgb_16() {
		let mut rgb = PF_Pixel16 {
			alpha: 32768,
			red: 32768,
			green: 8192,
			blue: 0,
		};
		let mut hls = [0 as PF_Fixed; 3];
		unsafe { rgb_to_hls_16_sys(std::ptr::null_mut(), &mut rgb, hls.as_mut_ptr()) };
		let mut back = PF_Pixel16 {
			alpha: 0,
			red: 0,
			green: 0,
			blue: 0,
		};
		unsafe { hls_to_rgb_16_sys(std::ptr::null_mut(), hls.as_mut_ptr(), &mut back) };
		// Fixed-point staging costs a little precision; ±2 of 32768 is plenty close.
		assert!((back.red as i32 - 32768).abs() <= 2, "red {}", back.red);
		assert!((back.green as i32 - 8192).abs() <= 2, "green {}", back.green);
		assert!((back.blue as i32).abs() <= 2, "blue {}", back.blue);
		assert_eq!(back.alpha, 32768, "result is defined to be opaque");
	}

	#[test]
	fn suite_scalar_queries_match_docs_scaling() {
		// White: luminance 25500, lightness 255, saturation 0.
		let mut white = PF_Pixel {
			alpha: 255,
			red: 255,
			green: 255,
			blue: 255,
		};
		let mut out: A_long = -1;
		unsafe { luminance_8_sys(std::ptr::null_mut(), &mut white, &mut out) };
		assert_eq!(out, 25500);
		unsafe { lightness_8_sys(std::ptr::null_mut(), &mut white, &mut out) };
		assert_eq!(out, 255);
		unsafe { saturation_8_sys(std::ptr::null_mut(), &mut white, &mut out) };
		assert_eq!(out, 0);

		// Pure green sits a third of the way around the hue circle.
		let mut green = PF_Pixel {
			alpha: 255,
			red: 0,
			green: 255,
			blue: 0,
		};
		unsafe { hue_8_sys(std::ptr::null_mut(), &mut green, &mut out) };
		assert_eq!(out, 85);
	}

	#[test]
	fn float_suite_reports_unquantized_scalars() {
		let mut px = PF_PixelFloat {
			alpha: 1.0,
			red: 1.0,
			green: 1.0,
			blue: 1.0,
		};
		let mut lum: f32 = 0.0;
		unsafe { luminance_float_sys(std::ptr::null_mut(), &mut px, &mut lum) };
		assert!((lum - 25500.0).abs() < 0.1, "lum {lum}");
	}

	#[test]
	fn null_arguments_are_rejected() {
		let mut rgb = PF_Pixel {
			alpha: 0,
			red: 0,
			green: 0,
			blue: 0,
		};
		let mut hls = [0 as PF_Fixed; 3];
		assert_eq!(
			unsafe { rgb_to_hls_8_sys(std::ptr::null_mut(), std::ptr::null_mut(), hls.as_mut_ptr()) },
			PF_Err_BAD_CALLBACK_PARAM as PF_Err
		);
		assert_eq!(
			unsafe { rgb_to_hls_8_sys(std::ptr::null_mut(), &mut rgb, std::ptr::null_mut()) },
			PF_Err_BAD_CALLBACK_PARAM as PF_Err
		);
	}
}
