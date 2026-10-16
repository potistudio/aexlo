//! Input layers (§4.5): files, fitted to the render size, and generators
//! produced directly at the variant's depth.

use aexlo::{AnyLayer, PixelDepthKind};

use crate::error::{Error, Result};
use crate::frame::load_image;
use crate::preset::{Fit, Generator, InputSpec};

/// Size of a generated input when the preset names no `size`: the host's
/// own default frame.
pub const DEFAULT_SIZE: (u32, u32) = (1920, 1080);

/// Build `spec` at `depth`. Generated inputs are `size` (else
/// [`DEFAULT_SIZE`]); files keep their own size unless `size` is given and
/// `fit` is `stretch`.
pub fn build(spec: &InputSpec, depth: PixelDepthKind, size: Option<(u32, u32)>) -> Result<AnyLayer> {
	spec.validate()?;
	if let Some(path) = &spec.path {
		let mut layer = load_image(path).map_err(|e| Error::invalid(e.message))?;
		if let Some((w, h)) = size
			&& spec.fit() == Fit::Stretch
			&& (layer.width(), layer.height()) != (w, h)
		{
			layer = resize(&layer, w, h);
		}
		return Ok(if layer.kind() == depth {
			layer
		} else {
			layer.converted(depth)
		});
	}

	let (w, h) = size.unwrap_or(DEFAULT_SIZE);
	let generator = spec.generator().unwrap_or(Generator::Gradient);
	let mut layer = AnyLayer::filled(depth, w, h, [0.0, 0.0, 0.0, 1.0]);
	let color = |default: [f32; 4]| -> [f32; 4] {
		match spec.color.as_deref() {
			Some([r, g, b]) => [*r, *g, *b, 1.0],
			Some([r, g, b, a]) => [*r, *g, *b, *a],
			_ => default,
		}
	};

	let pixels: Box<dyn Iterator<Item = [f32; 4]>> = match generator {
		Generator::Gradient => {
			let (fw, fh) = ((w.max(2) - 1) as f32, (h.max(2) - 1) as f32);
			Box::new(coords(w, h).map(move |(x, y)| {
				let (x, y) = (x as f32, y as f32);
				[x / fw, y / fh, (x + y) / (fw + fh), 1.0]
			}))
		}
		Generator::Solid => {
			let c = color([0.5, 0.5, 0.5, 1.0]);
			Box::new(coords(w, h).map(move |_| c))
		}
		Generator::Checker => {
			let cell = spec.cell.unwrap_or(32);
			Box::new(coords(w, h).map(move |(x, y)| {
				let v = ((x / cell + y / cell) % 2) as f32;
				[v, v, v, 1.0]
			}))
		}
		Generator::Dot => {
			let [cx, cy] = spec.at.unwrap_or([w as f32 / 2.0, h as f32 / 2.0]);
			let radius = spec.radius.unwrap_or(2.0);
			let c = color([1.0, 1.0, 1.0, 1.0]);
			Box::new(coords(w, h).map(move |(x, y)| {
				// Anti-aliased: coverage of the pixel's center by the disc's edge.
				let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
				let cover = (radius + 0.5 - d).clamp(0.0, 1.0);
				[c[0] * cover, c[1] * cover, c[2] * cover, 1.0 - cover + c[3] * cover]
			}))
		}
		Generator::Noise => {
			let seed = spec.seed.unwrap_or(0);
			Box::new(coords(w, h).map(move |(x, y)| {
				let n = hash(seed ^ ((y as u64) << 32 | x as u64));
				let channel = |shift: u32| ((n >> shift) & 0xffff) as f32 / 65535.0;
				[channel(0), channel(16), channel(32), 1.0]
			}))
		}
	};
	layer.set_from_unit_rgba(pixels);
	Ok(layer)
}

fn coords(w: u32, h: u32) -> impl Iterator<Item = (u32, u32)> {
	(0..h).flat_map(move |y| (0..w).map(move |x| (x, y)))
}

/// SplitMix64: a stable, seedable per-pixel hash.
fn hash(mut x: u64) -> u64 {
	x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
	x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
	x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
	x ^ (x >> 31)
}

/// `layer` resampled to `w` x `h` (triangle filter, in float), at its depth.
fn resize(layer: &AnyLayer, w: u32, h: u32) -> AnyLayer {
	let data: Vec<f32> = layer.to_unit_rgba().into_iter().flatten().collect();
	let img = image::Rgba32FImage::from_raw(layer.width(), layer.height(), data).expect("sized from the layer");
	let resized = image::imageops::resize(&img, w, h, image::imageops::FilterType::Triangle);
	let mut out = AnyLayer::filled(layer.kind(), w, h, [0.0; 4]);
	out.set_from_unit_rgba(resized.into_raw().as_chunks::<4>().0.iter().copied());
	out
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn generators_are_identical_across_depths() {
		for generator in [
			Generator::Gradient,
			Generator::Checker,
			Generator::Dot,
			Generator::Noise,
		] {
			let spec = InputSpec::generated(generator);
			let a = build(&spec, PixelDepthKind::F32, Some((33, 17))).unwrap();
			let b = build(&spec, PixelDepthKind::U16, Some((33, 17))).unwrap();
			let c = build(&spec, PixelDepthKind::U8, Some((33, 17))).unwrap();
			assert_eq!(b.kind(), PixelDepthKind::U16);
			for ((pa, pb), pc) in a.to_unit_rgba().iter().zip(b.to_unit_rgba()).zip(c.to_unit_rgba()) {
				for ch in 0..4 {
					assert!((pa[ch] - pb[ch]).abs() <= 0.5 / 32768.0 + 1e-6, "{generator:?}");
					assert!((pa[ch] - pc[ch]).abs() <= 0.5 / 255.0 + 1e-6, "{generator:?}");
				}
			}
		}
	}

	#[test]
	fn dot_is_centered_where_asked() {
		let spec = InputSpec {
			generate: Some(Generator::Dot),
			at: Some([10.0, 5.0]),
			radius: Some(1.5),
			..InputSpec::default()
		};
		let layer = build(&spec, PixelDepthKind::F32, Some((20, 10))).unwrap();
		let at = |x: u32, y: u32| layer.unit_rgba((y * 20 + x) as usize).unwrap();
		assert_eq!(at(9, 4)[0], 1.0);
		assert_eq!(at(0, 0)[0], 0.0);
		assert_eq!(at(0, 0)[3], 1.0);
	}

	#[test]
	fn missing_input_file_is_invalid() {
		let spec = InputSpec {
			path: Some("/definitely/not/here.png".into()),
			..InputSpec::default()
		};
		let err = build(&spec, PixelDepthKind::U8, None).unwrap_err();
		assert_eq!(err.kind, crate::ErrorKind::Invalid);
	}
}
