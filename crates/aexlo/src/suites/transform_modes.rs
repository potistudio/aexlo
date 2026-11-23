//! How After Effects composites one pixel for `transfer_rect`,
//! `transform_world` and `composite_rect`.
//!
//! The SDK names the transfer modes but gives no formulas, so these are fitted
//! to a trace of the probe (`playground/probe/src/xform.rs`) taken in After
//! Effects 2026: every mode at 8, 16 and 32 bpc, with partial alpha, half
//! opacity and float values above 1. At 32 bpc they match After Effects
//! exactly, except for Hue and Saturation where both layers are partly
//! transparent; at 8 and 16 bpc they match it to within a few levels of
//! rounding. Dissolve follows After Effects' rule with a pattern of its own:
//! After Effects' random numbers cannot be told from outside.
//!
//! Most modes are the W3C "source over" composite of a blend `B(cb, cs)`:
//! `co = (cs·αs·(1−αb) + cb·αb·(1−αs) + B·αs·αb) / αo`. Float values are not
//! clamped; in 8 and 16 bpc a blend's result is (except for Add). Colours are
//! straight whatever the mode flags say: After Effects gives the same result
//! for `PF_MF_Alpha_PREMUL` and `PF_MF_Alpha_STRAIGHT`.

use crate::sys::*;

/// A straight-alpha pixel on a 0..1 scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Px {
	pub a: f64,
	pub c: [f64; 3],
}

/// Rec. 601 luma, which After Effects uses for every mode that needs one.
fn luma(c: [f64; 3]) -> f64 {
	0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2]
}

/// The smallest divisor Divide uses: After Effects divides by it where the
/// source is 0 (`b / s` with `s` raised to this).
const DIVIDE_FLOOR: f64 = 1e6 / (1u64 << 39) as f64;

fn burn2(b: f64, s: f64) -> f64 {
	if s <= 0.0 {
		return if b >= 1.0 { b } else { 0.0 };
	}
	(1.0 - (1.0 - b) / s).max(0.0)
}

fn dodge2(b: f64, s: f64) -> f64 {
	if b <= 0.0 {
		return 0.0;
	}
	if s >= 1.0 {
		return 1.0;
	}
	(b / (1.0 - s)).min(1.0)
}

/// Vivid Light's dodge half, which saturates before it looks at the backdrop.
fn vivid_dodge(b: f64, s: f64) -> f64 {
	if s >= 1.0 {
		return 1.0;
	}
	if b <= 0.0 {
		return 0.0;
	}
	(b / (1.0 - s)).min(1.0)
}

fn soft_light(b: f64, s: f64) -> f64 {
	if b >= 1.0 {
		return b;
	}
	if s <= 0.5 {
		return b - (1.0 - 2.0 * s) * b * (1.0 - b);
	}
	let d = if b <= 0.25 {
		((16.0 * b - 12.0) * b + 4.0) * b
	} else {
		b.sqrt()
	};
	b + (2.0 * s - 1.0) * (d - b)
}

/// A separable blend `B(backdrop, source)`, or `None` for a mode that is not one.
#[allow(non_upper_case_globals)]
fn separable(mode: A_long, b: f64, s: f64) -> Option<f64> {
	Some(match mode {
		PF_Xfer_ADD | PF_Xfer_LINEAR_DODGE => b + s,
		PF_Xfer_MULTIPLY => b * s,
		PF_Xfer_SCREEN => b + s - b * s,
		PF_Xfer_OVERLAY => {
			if b <= 0.5 {
				2.0 * b * s
			} else {
				1.0 - 2.0 * (1.0 - b) * (1.0 - s)
			}
		}
		PF_Xfer_HARD_LIGHT => {
			if s <= 0.5 {
				2.0 * b * s
			} else {
				1.0 - 2.0 * (1.0 - b) * (1.0 - s)
			}
		}
		PF_Xfer_SOFT_LIGHT => soft_light(b, s),
		PF_Xfer_DARKEN => b.min(s),
		PF_Xfer_LIGHTEN => b.max(s),
		PF_Xfer_DIFFERENCE2 => (b - s).abs(),
		PF_Xfer_EXCLUSION => b + s - 2.0 * b * s,
		PF_Xfer_SUBTRACT => b - s,
		PF_Xfer_LINEAR_BURN => (b + s - 1.0).max(0.0),
		PF_Xfer_COLOR_BURN2 => burn2(b, s),
		PF_Xfer_COLOR_DODGE2 => dodge2(b, s),
		PF_Xfer_LINEAR_LIGHT => b + 2.0 * s - 1.0,
		PF_Xfer_VIVID_LIGHT => {
			if s <= 0.5 {
				burn2(b, 2.0 * s)
			} else {
				vivid_dodge(b, 2.0 * s - 1.0)
			}
		}
		PF_Xfer_PIN_LIGHT => {
			if s <= 0.5 {
				b.min(2.0 * s)
			} else {
				b.max(2.0 * s - 1.0)
			}
		}
		PF_Xfer_HARD_MIX => {
			if b + s > 1.0 {
				1.0
			} else {
				0.0
			}
		}
		PF_Xfer_DIVIDE => b / s.max(DIVIDE_FLOOR),
		_ => return None,
	})
}

/// `top` over `bottom` with `blend(bottom, top)` where both are present.
fn over(top: Px, bottom: Px, blend: impl Fn([f64; 3], [f64; 3]) -> [f64; 3]) -> Px {
	let a = top.a + bottom.a - top.a * bottom.a;
	if a <= 0.0 {
		return Px { a: 0.0, c: [0.0; 3] };
	}
	let mixed = blend(bottom.c, top.c);
	Px {
		a,
		c: std::array::from_fn(|i| {
			(top.c[i] * top.a * (1.0 - bottom.a) + bottom.c[i] * bottom.a * (1.0 - top.a) + mixed[i] * top.a * bottom.a)
				/ a
		}),
	}
}

fn clip_colour(c: [f64; 3]) -> [f64; 3] {
	let l = luma(c);
	let n = c[0].min(c[1]).min(c[2]);
	let x = c[0].max(c[1]).max(c[2]);
	let mut out = c;
	if n < 0.0 && l - n > 1e-12 {
		out = out.map(|v| l + (v - l) * l / (l - n));
	}
	if x > 1.0 && x - l > 1e-12 {
		out = out.map(|v| l + (v - l) * (1.0 - l) / (x - l));
	}
	out
}

fn set_luma(c: [f64; 3], l: f64) -> [f64; 3] {
	let d = l - luma(c);
	clip_colour(c.map(|v| v + d))
}

fn saturation(c: [f64; 3]) -> f64 {
	c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
}

fn set_saturation(c: [f64; 3], s: f64) -> [f64; 3] {
	let (mx, mn) = (c[0].max(c[1]).max(c[2]), c[0].min(c[1]).min(c[2]));
	if mx <= mn {
		return [0.0; 3];
	}
	c.map(|v| (v - mn) * s / (mx - mn))
}

/// A non-separable blend `B(backdrop, source)`, or `None`.
#[allow(non_upper_case_globals)]
fn non_separable(mode: A_long, b: [f64; 3], s: [f64; 3]) -> Option<[f64; 3]> {
	Some(match mode {
		PF_Xfer_HUE => set_luma(set_saturation(s, saturation(b)), luma(b)),
		PF_Xfer_SATURATION => set_luma(set_saturation(b, saturation(s)), luma(b)),
		PF_Xfer_COLOR => set_luma(s, luma(b)),
		PF_Xfer_LUMINOSITY => set_luma(b, luma(s)),
		_ => return None,
	})
}

/// The classic Difference, Color Dodge and Color Burn work on premultiplied
/// colours: `backdrop` and `source` are those, `bottom` and `top` the alphas.
#[allow(non_upper_case_globals)]
fn classic(mode: A_long, b: f64, s: f64, bottom: f64, top: f64, out: f64) -> Option<f64> {
	// x / (1 − y), saturating where y reaches 1.
	let dodge = |x: f64, y: f64| if y >= 1.0 { 1.0 } else { x / (1.0 - y) };
	Some(match mode {
		PF_Xfer_DIFFERENCE => (b - s).abs(),
		PF_Xfer_COLOR_DODGE => dodge(b, s).min(1.0),
		PF_Xfer_COLOR_BURN => out - dodge(bottom - b, top - s),
		_ => return None,
	})
}

/// Composite `src` at `opacity` onto `dst` under `mode`. `integer` is an 8 or
/// 16 bpc destination. `roll` is this pixel's random number in `0..1` for
/// Dissolve.
#[allow(non_upper_case_globals)]
pub(super) fn composite(mode: A_long, src: Px, opacity: f64, dst: Px, integer: bool, roll: f64) -> Px {
	let top = Px {
		a: src.a * opacity,
		c: src.c,
	};
	let (sa, da) = (top.a, dst.a);
	let out_a = sa + da - sa * da;
	match mode {
		PF_Xfer_NONE => dst,
		PF_Xfer_COPY => top,
		// A pixel either takes the source, opaque, or keeps the destination.
		PF_Xfer_DISSOLVE => {
			if roll < sa {
				Px { a: 1.0, c: src.c }
			} else {
				dst
			}
		}
		PF_Xfer_BEHIND => over(dst, top, |_, t| t),
		PF_Xfer_MULTIPLY_ALPHA => Px { a: da * sa, c: dst.c },
		PF_Xfer_MULTIPLY_NOT_ALPHA => Px {
			a: da * (1.0 - sa),
			c: dst.c,
		},
		PF_Xfer_MULTIPLY_ALPHA_LUMA => Px {
			a: da * luma(src.c).min(1.0) * sa,
			c: dst.c,
		},
		PF_Xfer_MULTIPLY_NOT_ALPHA_LUMA => Px {
			a: da * (1.0 - luma(src.c).min(1.0) * sa),
			c: dst.c,
		},
		// The source's colours count as premultiplied already.
		PF_Xfer_ADDITIVE_PREMUL => {
			if out_a <= 0.0 {
				return Px { a: 0.0, c: [0.0; 3] };
			}
			Px {
				a: out_a,
				c: std::array::from_fn(|i| (src.c[i] * opacity + dst.c[i] * da * (1.0 - sa)) / out_a),
			}
		}
		// Coverage adds up, the destination taking what the source leaves.
		PF_Xfer_ALPHA_ADD => {
			let covered = da.min(1.0 - sa);
			let a = sa + covered;
			if a <= 0.0 {
				return Px { a: 0.0, c: [0.0; 3] };
			}
			Px {
				a,
				c: std::array::from_fn(|i| (src.c[i] * sa + dst.c[i] * covered) / a),
			}
		}
		PF_Xfer_LIGHTER_COLOR => over(top, dst, |b, s| if luma(s) > luma(b) { s } else { b }),
		PF_Xfer_DARKER_COLOR => over(top, dst, |b, s| if luma(s) < luma(b) { s } else { b }),
		PF_Xfer_DIFFERENCE | PF_Xfer_COLOR_DODGE | PF_Xfer_COLOR_BURN => {
			if out_a <= 0.0 {
				return Px { a: 0.0, c: [0.0; 3] };
			}
			Px {
				a: out_a,
				c: std::array::from_fn(|i| {
					classic(mode, dst.c[i] * da, src.c[i] * sa, da, sa, out_a).unwrap_or(0.0) / out_a
				}),
			}
		}
		PF_Xfer_HUE | PF_Xfer_SATURATION | PF_Xfer_COLOR | PF_Xfer_LUMINOSITY => {
			if out_a <= 0.0 {
				return Px { a: 0.0, c: [0.0; 3] };
			}
			// The blend sees the destination over the source.
			let backdrop = std::array::from_fn(|i| dst.c[i] * da + src.c[i] * (1.0 - da));
			let mixed = non_separable(mode, backdrop, src.c).unwrap_or(src.c);
			Px {
				a: out_a,
				c: std::array::from_fn(|i| (mixed[i] * sa + dst.c[i] * da * (1.0 - sa)) / out_a),
			}
		}
		_ => match separable(mode, 0.5, 0.5) {
			Some(_) => over(top, dst, |b, s| {
				std::array::from_fn(|i| {
					let v = separable(mode, b[i], s[i]).unwrap_or(s[i]);
					if integer && mode != PF_Xfer_ADD {
						v.clamp(0.0, 1.0)
					} else {
						v
					}
				})
			}),
			// In Front, and any mode the SDK does not name: Normal.
			None => over(top, dst, |_, s| s),
		},
	}
}

/// Dissolve's random number for pixel `(x, y)` under `seed`, in `0..1`.
pub(super) fn dissolve_roll(seed: A_long, x: i32, y: i32) -> f64 {
	// SplitMix64 over the seed and the position.
	let mut z = (seed as u32 as u64) << 32 ^ (x as u32 as u64) << 16 ^ y as u32 as u64;
	z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
	z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
	z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
	z ^= z >> 31;
	(z >> 11) as f64 / (1u64 << 53) as f64
}
