//! Depth-generic pixel access: converts `PF_Pixel` / `PF_Pixel16` /
//! `PF_PixelFloat` to and from normalized `f64` channels so color math can be
//! written once and monomorphized per depth (used by the color-callback and
//! fill-matte suites).

use after_effects_sys::{PF_MAX_CHAN8, PF_MAX_CHAN16, PF_Pixel, PF_Pixel16, PF_PixelFloat};

/// A pixel whose channels can round-trip through normalized `f64` values.
///
/// Integer depths map their full channel range (`0..=255`, `0..=32768`) onto
/// `0.0..=1.0` and clamp on the way back; the float depth passes values through
/// untouched so HDR data outside `0..1` survives.
pub(crate) trait NormalizedPixel: Copy {
	/// Channels as `[alpha, red, green, blue]`, normalized to `0.0..=1.0`
	/// for the integer depths.
	fn to_norm(self) -> [f64; 4];

	/// Rebuilds a pixel from `[alpha, red, green, blue]` normalized channels.
	fn from_norm(argb: [f64; 4]) -> Self;

	/// The normalized value of a fully-opaque alpha channel.
	const OPAQUE: f64 = 1.0;
}

/// Clamp + scale a normalized channel into an integer channel range.
#[inline]
fn quantize(v: f64, max: f64) -> f64 {
	(v * max).round().clamp(0.0, max)
}

impl NormalizedPixel for PF_Pixel {
	fn to_norm(self) -> [f64; 4] {
		let max = PF_MAX_CHAN8 as f64;
		[
			self.alpha as f64 / max,
			self.red as f64 / max,
			self.green as f64 / max,
			self.blue as f64 / max,
		]
	}

	fn from_norm([a, r, g, b]: [f64; 4]) -> Self {
		let max = PF_MAX_CHAN8 as f64;
		PF_Pixel {
			alpha: quantize(a, max) as u8,
			red: quantize(r, max) as u8,
			green: quantize(g, max) as u8,
			blue: quantize(b, max) as u8,
		}
	}
}

impl NormalizedPixel for PF_Pixel16 {
	fn to_norm(self) -> [f64; 4] {
		let max = PF_MAX_CHAN16 as f64;
		[
			self.alpha as f64 / max,
			self.red as f64 / max,
			self.green as f64 / max,
			self.blue as f64 / max,
		]
	}

	fn from_norm([a, r, g, b]: [f64; 4]) -> Self {
		let max = PF_MAX_CHAN16 as f64;
		PF_Pixel16 {
			alpha: quantize(a, max) as u16,
			red: quantize(r, max) as u16,
			green: quantize(g, max) as u16,
			blue: quantize(b, max) as u16,
		}
	}
}

impl NormalizedPixel for PF_PixelFloat {
	fn to_norm(self) -> [f64; 4] {
		[self.alpha as f64, self.red as f64, self.green as f64, self.blue as f64]
	}

	fn from_norm([a, r, g, b]: [f64; 4]) -> Self {
		// Float worlds are allowed to carry out-of-range (HDR) values; no clamping.
		PF_PixelFloat {
			alpha: a as f32,
			red: r as f32,
			green: g as f32,
			blue: b as f32,
		}
	}
}
