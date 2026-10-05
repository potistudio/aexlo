//! Image resampling: `PF_Sampling8Suite1`, `PF_Sampling16Suite1`,
//! `PF_SamplingFloatSuite1` and the deprecated `PF_BatchSamplingSuite1`, shared
//! with the matching legacy `PF_UtilCallbacks` entries.
//!
//! Coordinates are `PF_Fixed` (16.16) with integer values landing on pixel
//! centers, so sampling at `INT2FIX(x), INT2FIX(y)` returns that pixel exactly
//! (what the SDK's Shifter sample relies on). Pixels outside the source read as
//! transparent black, the only `PF_SampleEdgeBehav` AE supports.
//!
//! * `nn_sample` picks the nearest pixel.
//! * `subpixel_sample` interpolates bilinearly.
//! * `area_sample` box-filters the `2 * x_radius` by `2 * y_radius` area around
//!   the point, weighting each pixel by its coverage; a zero radius falls back
//!   to `subpixel_sample`.
//!
//! Channels are averaged as stored, i.e. premultiplied for AE's worlds.

use super::pixel_norm::NormalizedPixel;
use after_effects_sys::{
	PF_BatchSample16Func, PF_BatchSampleFunc, PF_BatchSamplingSuite1, PF_EffectWorld, PF_Err,
	PF_Err_BAD_CALLBACK_PARAM, PF_Err_NONE, PF_Fixed, PF_ModeFlags, PF_Pixel, PF_Pixel16, PF_PixelFloat, PF_ProgPtr,
	PF_Quality, PF_SampPB, PF_Sampling8Suite1, PF_Sampling16Suite1, PF_SamplingFloatSuite1,
};

fn fixed_to_f64(v: PF_Fixed) -> f64 {
	v as f64 / 65536.0
}

/// Read pixel `(x, y)` of `world` as normalized `[a, r, g, b]`; outside the
/// world reads as transparent black.
///
/// # Safety
/// `world.data` must hold `height` rows of `rowbytes` bytes of `P` pixels.
unsafe fn read<P: NormalizedPixel>(world: &PF_EffectWorld, x: i32, y: i32) -> [f64; 4] {
	if x < 0 || y < 0 || x >= world.width || y >= world.height {
		return [0.0; 4];
	}
	let row = unsafe { (world.data as *const u8).offset(y as isize * world.rowbytes as isize) };
	unsafe { *(row as *const P).add(x as usize) }.to_norm()
}

/// Nearest-pixel sample.
unsafe fn nearest<P: NormalizedPixel>(world: &PF_EffectWorld, x: f64, y: f64) -> [f64; 4] {
	unsafe { read::<P>(world, (x + 0.5).floor() as i32, (y + 0.5).floor() as i32) }
}

/// Bilinear sample.
unsafe fn bilinear<P: NormalizedPixel>(world: &PF_EffectWorld, x: f64, y: f64) -> [f64; 4] {
	let (x0, y0) = (x.floor(), y.floor());
	let (fx, fy) = (x - x0, y - y0);
	let (xi, yi) = (x0 as i32, y0 as i32);
	let taps = unsafe {
		[
			(read::<P>(world, xi, yi), (1.0 - fx) * (1.0 - fy)),
			(read::<P>(world, xi + 1, yi), fx * (1.0 - fy)),
			(read::<P>(world, xi, yi + 1), (1.0 - fx) * fy),
			(read::<P>(world, xi + 1, yi + 1), fx * fy),
		]
	};
	let mut out = [0.0; 4];
	for (p, w) in taps {
		for c in 0..4 {
			out[c] += p[c] * w;
		}
	}
	out
}

/// Coverage-weighted box filter over `[x - rx, x + rx] x [y - ry, y + ry]`,
/// where pixel `i` covers `[i - 0.5, i + 0.5]`.
unsafe fn area<P: NormalizedPixel>(world: &PF_EffectWorld, x: f64, y: f64, rx: f64, ry: f64) -> [f64; 4] {
	if rx <= 0.0 || ry <= 0.0 {
		return unsafe { bilinear::<P>(world, x, y) };
	}
	let (left, right, top, bottom) = (x - rx, x + rx, y - ry, y + ry);
	let overlap = |lo: f64, hi: f64, i: i32| (hi.min(i as f64 + 0.5) - lo.max(i as f64 - 0.5)).max(0.0);

	let mut out = [0.0; 4];
	for py in (top + 0.5).floor() as i32..=(bottom + 0.5).floor() as i32 {
		let wy = overlap(top, bottom, py);
		for px in (left + 0.5).floor() as i32..=(right + 0.5).floor() as i32 {
			let w = wy * overlap(left, right, px);
			if w > 0.0 {
				let p = unsafe { read::<P>(world, px, py) };
				for c in 0..4 {
					out[c] += p[c] * w;
				}
			}
		}
	}
	let total = (right - left) * (bottom - top);
	out.map(|v| v / total)
}

#[derive(Clone, Copy)]
enum Kind {
	Nearest,
	Bilinear,
	Area,
}

/// Validate the arguments, sample `params->src` and write the result to `dst`.
///
/// # Safety
/// `params` must be null or valid, with `src` null or a valid world of `P`
/// pixels; `dst` must be null or writable.
unsafe fn sample<P: NormalizedPixel>(
	x: PF_Fixed,
	y: PF_Fixed,
	params: *const PF_SampPB,
	dst: *mut P,
	kind: Kind,
) -> PF_Err {
	let Some(params) = (unsafe { params.as_ref() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	};
	let Some(world) = (unsafe { params.src.as_ref() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	};
	if dst.is_null() || world.data.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}
	let (x, y) = (fixed_to_f64(x), fixed_to_f64(y));
	let argb = unsafe {
		match kind {
			Kind::Nearest => nearest::<P>(world, x, y),
			Kind::Bilinear => bilinear::<P>(world, x, y),
			Kind::Area => area::<P>(
				world,
				x,
				y,
				fixed_to_f64(params.x_radius),
				fixed_to_f64(params.y_radius),
			),
		}
	};
	unsafe { *dst = P::from_norm(argb) };
	PF_Err_NONE as PF_Err
}

macro_rules! sampler {
	($name:ident, $pixel:ty, $kind:expr) => {
		pub(crate) unsafe extern "C" fn $name(
			_effect_ref: PF_ProgPtr,
			x: PF_Fixed,
			y: PF_Fixed,
			params: *const PF_SampPB,
			dst_pixel: *mut $pixel,
		) -> PF_Err {
			unsafe { sample::<$pixel>(x, y, params, dst_pixel, $kind) }
		}
	};
}

sampler!(nn_sample_8_sys, PF_Pixel, Kind::Nearest);
sampler!(subpixel_sample_8_sys, PF_Pixel, Kind::Bilinear);
sampler!(area_sample_8_sys, PF_Pixel, Kind::Area);
sampler!(nn_sample_16_sys, PF_Pixel16, Kind::Nearest);
sampler!(subpixel_sample_16_sys, PF_Pixel16, Kind::Bilinear);
sampler!(area_sample_16_sys, PF_Pixel16, Kind::Area);
sampler!(nn_sample_float_sys, PF_PixelFloat, Kind::Nearest);
sampler!(subpixel_sample_float_sys, PF_PixelFloat, Kind::Bilinear);
sampler!(area_sample_float_sys, PF_PixelFloat, Kind::Area);

/// Sampling needs no setup; per the SDK, `reserved` is zeroed here.
pub(crate) unsafe extern "C" fn begin_sampling_sys(
	_effect_ref: PF_ProgPtr,
	_qual: PF_Quality,
	_mf: PF_ModeFlags,
	params: *mut PF_SampPB,
) -> PF_Err {
	if let Some(params) = unsafe { params.as_mut() } {
		params.reserved = [0; 8];
	}
	PF_Err_NONE as PF_Err
}

pub(crate) unsafe extern "C" fn end_sampling_sys(
	_effect_ref: PF_ProgPtr,
	_qual: PF_Quality,
	_mf: PF_ModeFlags,
	_params: *mut PF_SampPB,
) -> PF_Err {
	PF_Err_NONE as PF_Err
}

/// Batch sampling functions were withdrawn in AE 7.0; report none so the
/// plugin falls back to the per-pixel samplers.
unsafe extern "C" fn get_batch_func(
	_effect_ref: PF_ProgPtr,
	_quality: PF_Quality,
	_mode_flags: PF_ModeFlags,
	_params: *const PF_SampPB,
	batch: *mut PF_BatchSampleFunc,
) -> PF_Err {
	log::warn!("PF_BatchSamplingSuite1/get_batch_func: batch sampling is unsupported (deprecated since AE 7.0)");
	if let Some(out) = unsafe { batch.as_mut() } {
		*out = std::ptr::null_mut();
	}
	PF_Err_BAD_CALLBACK_PARAM as PF_Err
}

unsafe extern "C" fn get_batch_func16(
	_effect_ref: PF_ProgPtr,
	_quality: PF_Quality,
	_mode_flags: PF_ModeFlags,
	_params: *const PF_SampPB,
	batch: *mut PF_BatchSample16Func,
) -> PF_Err {
	log::warn!("PF_BatchSamplingSuite1/get_batch_func16: batch sampling is unsupported (deprecated since AE 7.0)");
	if let Some(out) = unsafe { batch.as_mut() } {
		*out = std::ptr::null_mut();
	}
	PF_Err_BAD_CALLBACK_PARAM as PF_Err
}

pub(super) const fn create_batch_sampling_suite_1() -> PF_BatchSamplingSuite1 {
	PF_BatchSamplingSuite1 {
		begin_sampling: Some(begin_sampling_sys),
		end_sampling: Some(end_sampling_sys),
		get_batch_func: Some(get_batch_func),
		get_batch_func16: Some(get_batch_func16),
	}
}

pub(super) const fn create_sampling_8_suite_1() -> PF_Sampling8Suite1 {
	PF_Sampling8Suite1 {
		nn_sample: Some(nn_sample_8_sys),
		subpixel_sample: Some(subpixel_sample_8_sys),
		area_sample: Some(area_sample_8_sys),
	}
}

pub(super) const fn create_sampling_16_suite_1() -> PF_Sampling16Suite1 {
	PF_Sampling16Suite1 {
		nn_sample16: Some(nn_sample_16_sys),
		subpixel_sample16: Some(subpixel_sample_16_sys),
		area_sample16: Some(area_sample_16_sys),
	}
}

pub(super) const fn create_sampling_float_suite_1() -> PF_SamplingFloatSuite1 {
	PF_SamplingFloatSuite1 {
		nn_sample_float: Some(nn_sample_float_sys),
		subpixel_sample_float: Some(subpixel_sample_float_sys),
		area_sample_float: Some(area_sample_float_sys),
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use wrapper::{Depth8, Layer};

	const ONE: PF_Fixed = 1 << 16;

	/// A 2x1 8-bpc world: opaque black, then opaque white.
	fn black_white() -> (Layer<Depth8>, PF_EffectWorld) {
		let mut layer = Layer::<Depth8>::black(2, 1);
		let world = layer.as_sys();
		unsafe {
			*world.data.cast::<PF_Pixel>().add(1) = PF_Pixel {
				alpha: 255,
				red: 255,
				green: 255,
				blue: 255,
			};
		}
		(layer, world)
	}

	fn samp(world: &mut PF_EffectWorld, radius: PF_Fixed) -> PF_SampPB {
		let mut pb: PF_SampPB = unsafe { std::mem::zeroed() };
		pb.src = world;
		pb.x_radius = radius;
		pb.y_radius = radius;
		pb
	}

	fn run(
		f: unsafe extern "C" fn(PF_ProgPtr, PF_Fixed, PF_Fixed, *const PF_SampPB, *mut PF_Pixel) -> PF_Err,
		x: PF_Fixed,
		y: PF_Fixed,
		pb: &PF_SampPB,
	) -> PF_Pixel {
		let mut out = PF_Pixel {
			alpha: 7,
			red: 7,
			green: 7,
			blue: 7,
		};
		assert_eq!(
			unsafe { f(std::ptr::null_mut(), x, y, pb, &mut out) },
			PF_Err_NONE as PF_Err
		);
		out
	}

	#[test]
	fn integer_coordinates_hit_pixel_centers() {
		let (_layer, mut world) = black_white();
		let pb = samp(&mut world, 0);
		assert_eq!(run(subpixel_sample_8_sys, ONE, 0, &pb).red, 255);
		assert_eq!(run(subpixel_sample_8_sys, 0, 0, &pb).red, 0);
		assert_eq!(run(nn_sample_8_sys, ONE * 6 / 10, 0, &pb).red, 255);
	}

	#[test]
	fn bilinear_blends_and_fades_outside() {
		let (_layer, mut world) = black_white();
		let pb = samp(&mut world, 0);
		let mid = run(subpixel_sample_8_sys, ONE / 2, 0, &pb);
		assert_eq!((mid.alpha, mid.red), (255, 128));
		// Half a pixel below the only row: half transparent.
		let below = run(subpixel_sample_8_sys, 0, ONE / 2, &pb);
		assert_eq!(below.alpha, 128);
	}

	#[test]
	fn area_averages_covered_pixels() {
		let (_layer, mut world) = black_white();
		// A 2x1 box centered between the pixels covers each half; a 1-pixel
		// vertical radius covers the row plus transparent rows above/below.
		let pb = PF_SampPB {
			y_radius: ONE / 2,
			..samp(&mut world, ONE / 2)
		};
		let p = run(area_sample_8_sys, ONE / 2, 0, &pb);
		assert_eq!((p.alpha, p.red), (255, 128));
		let pb = samp(&mut world, ONE);
		let p = run(area_sample_8_sys, ONE / 2, 0, &pb);
		assert_eq!(p.alpha, 128);
	}

	#[test]
	fn rejects_missing_source() {
		let pb: PF_SampPB = unsafe { std::mem::zeroed() };
		let mut out = PF_Pixel {
			alpha: 0,
			red: 0,
			green: 0,
			blue: 0,
		};
		let err = unsafe { subpixel_sample_8_sys(std::ptr::null_mut(), 0, 0, &pb, &mut out) };
		assert_eq!(err, PF_Err_BAD_CALLBACK_PARAM as PF_Err);
	}
}
