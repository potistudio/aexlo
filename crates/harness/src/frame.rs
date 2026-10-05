//! Rendered frames (§6.4): reading pixels at any depth, comparing against a
//! reference (§8.2), and the files frames travel and rest in.

use std::io::{Read, Write};
use std::path::Path;

use aexlo::{AnyLayer, Depth8, Depth16, Depth32, Layer, Pixel, PixelDepthKind};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::preset::Tolerance;

/// A rendered (or reference) frame at 8, 16 or 32 bpc.
#[derive(Clone, Debug)]
pub struct Frame {
	layer: AnyLayer,
}

/// How a frame differs from a reference (§8.2).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Comparison {
	/// `(actual, expected)` sizes, when they differ (nothing else is compared).
	pub size_mismatch: Option<([u32; 2], [u32; 2])>,
	/// Largest per-channel difference, normalized.
	pub max_abs: f32,
	/// Pixels with a channel beyond the tolerance's `max_abs`.
	pub bad_pixels: usize,
	pub bad_fraction: f32,
	/// Peak signal-to-noise ratio in dB (infinite when identical).
	pub psnr: f64,
	pub passed: bool,
}

impl Comparison {
	/// One line saying how it went.
	pub fn summary(&self, tolerance: &Tolerance) -> String {
		if let Some((actual, expected)) = self.size_mismatch {
			return format!(
				"size {}x{} differs from the reference's {}x{}",
				actual[0], actual[1], expected[0], expected[1]
			);
		}
		format!(
			"max |Δ| {:.5} (tolerance {:.5}), {} pixel(s) beyond ({:.3}%, allowed {:.3}%), PSNR {}",
			self.max_abs,
			tolerance.max_abs,
			self.bad_pixels,
			self.bad_fraction * 100.0,
			tolerance.max_bad * 100.0,
			if self.psnr.is_infinite() {
				"∞".to_string()
			} else {
				format!("{:.2} dB", self.psnr)
			}
		)
	}
}

/// Per-channel difference that treats two NaNs as equal and one as infinite.
fn channel_diff(a: f32, b: f32) -> f32 {
	match (a.is_nan(), b.is_nan()) {
		(true, true) => 0.0,
		(false, false) if a == b => 0.0,
		(false, false) => (a - b).abs(),
		_ => f32::INFINITY,
	}
}

impl Frame {
	pub fn new(layer: AnyLayer) -> Self {
		Self { layer }
	}

	pub fn layer(&self) -> &AnyLayer {
		&self.layer
	}

	pub fn into_layer(self) -> AnyLayer {
		self.layer
	}

	pub fn width(&self) -> u32 {
		self.layer.width()
	}

	pub fn height(&self) -> u32 {
		self.layer.height()
	}

	pub fn depth(&self) -> PixelDepthKind {
		self.layer.kind()
	}

	/// The pixel at `(x, y)` as normalized `[r, g, b, a]`, whatever the depth.
	///
	/// # Panics
	/// When `(x, y)` is outside the frame.
	pub fn pixel(&self, x: u32, y: u32) -> [f32; 4] {
		assert!(
			x < self.width() && y < self.height(),
			"pixel ({x}, {y}) is outside the {}x{} frame",
			self.width(),
			self.height()
		);
		self.layer
			.unit_rgba((y * self.width() + x) as usize)
			.expect("in bounds")
	}

	/// Every pixel as normalized `[r, g, b, a]`, row-major.
	pub fn unit_rgba(&self) -> Vec<[f32; 4]> {
		self.layer.to_unit_rgba()
	}

	/// This frame at another depth.
	pub fn converted(&self, depth: PixelDepthKind) -> Frame {
		Frame::new(self.layer.converted(depth))
	}

	/// Compare against `reference` within `tolerance` (§8.2).
	pub fn compare(&self, reference: &Frame, tolerance: &Tolerance) -> Comparison {
		let (actual, expected) = ([self.width(), self.height()], [reference.width(), reference.height()]);
		if actual != expected {
			return Comparison {
				size_mismatch: Some((actual, expected)),
				max_abs: f32::INFINITY,
				bad_pixels: (actual[0] * actual[1]) as usize,
				bad_fraction: 1.0,
				psnr: 0.0,
				passed: false,
			};
		}

		let a = self.unit_rgba();
		let b = reference.unit_rgba();
		let mut max_abs = 0.0f32;
		let mut bad_pixels = 0usize;
		let mut squared = 0.0f64;
		for (pa, pb) in a.iter().zip(&b) {
			let mut worst = 0.0f32;
			for c in 0..4 {
				let d = channel_diff(pa[c], pb[c]);
				worst = worst.max(d);
				squared += if d.is_finite() { (d as f64).powi(2) } else { 1.0 };
			}
			max_abs = max_abs.max(worst);
			if worst > tolerance.max_abs {
				bad_pixels += 1;
			}
		}
		let count = a.len().max(1);
		let mse = squared / (count * 4) as f64;
		let psnr = if mse == 0.0 {
			f64::INFINITY
		} else {
			10.0 * (1.0 / mse).log10()
		};
		let bad_fraction = bad_pixels as f32 / count as f32;
		let passed = bad_fraction <= tolerance.max_bad && tolerance.min_psnr.is_none_or(|floor| psnr >= floor as f64);
		Comparison {
			size_mismatch: None,
			max_abs,
			bad_pixels,
			bad_fraction,
			psnr,
			passed,
		}
	}

	/// An 8 bpc heatmap of where this frame differs from `reference`: black
	/// where equal, brightening to grey at the tolerance, red beyond it.
	pub fn diff_heatmap(&self, reference: &Frame, tolerance: &Tolerance) -> Frame {
		let (w, h) = (
			self.width().min(reference.width()),
			self.height().min(reference.height()),
		);
		let limit = tolerance.max_abs.max(f32::EPSILON);
		let heat = Layer::<Depth8>::from_fn(w, h, |x, y| {
			let (pa, pb) = (self.pixel(x, y), reference.pixel(x, y));
			let worst = (0..4).map(|c| channel_diff(pa[c], pb[c])).fold(0.0f32, f32::max);
			let rgba = if worst == 0.0 {
				[0.0, 0.0, 0.0, 1.0]
			} else if worst <= limit {
				let g = 0.15 + 0.35 * worst / limit;
				[g, g, g, 1.0]
			} else {
				let r = (0.6 + 0.4 * (worst / limit).log10().min(1.0)).min(1.0);
				[r, 0.0, 0.0, 1.0]
			};
			Pixel::<Depth8>::from_unit(rgba)
		});
		Frame::new(AnyLayer::new(heat))
	}

	/// The file extension a frame of `depth` is stored with: PNG for 8 and
	/// 16 bpc, OpenEXR for 32 bpc (§8.1).
	pub fn extension(depth: PixelDepthKind) -> &'static str {
		match depth {
			PixelDepthKind::F32 => "exr",
			_ => "png",
		}
	}

	/// Write as PNG (8/16 bpc, RGBA) or EXR (32 bpc), by depth.
	pub fn save(&self, path: &Path) -> Result<()> {
		if let Some(dir) = path.parent() {
			std::fs::create_dir_all(dir).map_err(|e| Error::harness(format!("creating {}: {e}", dir.display())))?;
		}
		let (w, h) = (self.width(), self.height());
		let result = match &self.layer {
			AnyLayer::U8(layer) => {
				let mut rgba = vec![0u8; layer.len() * 4];
				layer
					.write_rgba_bytes(&mut rgba)
					.map_err(|e| Error::harness(e.to_string()))?;
				image::RgbaImage::from_raw(w, h, rgba).map(|img| img.save_with_format(path, image::ImageFormat::Png))
			}
			AnyLayer::U16(layer) => {
				let data: Vec<u16> = layer
					.iter()
					.flat_map(|p| [p.red, p.green, p.blue, p.alpha])
					.map(ae16_to_png16)
					.collect();
				image::ImageBuffer::<image::Rgba<u16>, _>::from_raw(w, h, data)
					.map(|img| img.save_with_format(path, image::ImageFormat::Png))
			}
			AnyLayer::F32(layer) => {
				let data: Vec<f32> = layer.iter().flat_map(|p| [p.red, p.green, p.blue, p.alpha]).collect();
				image::Rgba32FImage::from_raw(w, h, data)
					.map(|img| img.save_with_format(path, image::ImageFormat::OpenExr))
			}
		};
		match result {
			Some(Ok(())) => Ok(()),
			Some(Err(e)) => Err(Error::harness(format!("writing {}: {e}", path.display()))),
			None => Err(Error::harness(format!("writing {}: bad frame buffer", path.display()))),
		}
	}

	/// Read a PNG (8 or 16 bit) or EXR file at the depth it was stored at.
	pub fn load(path: &Path) -> Result<Frame> {
		load_image(path).map(Frame::new)
	}

	/// Write the pixels as raw little-endian RGBA channel values: how frames
	/// travel between a worker and the harness (§6.2).
	pub fn write_raw(&self, path: &Path) -> Result<()> {
		let mut bytes = Vec::with_capacity(self.width() as usize * self.height() as usize * 16);
		match &self.layer {
			AnyLayer::U8(l) => l.iter().for_each(|p| bytes.extend([p.red, p.green, p.blue, p.alpha])),
			AnyLayer::U16(l) => l.iter().for_each(|p| {
				for c in [p.red, p.green, p.blue, p.alpha] {
					bytes.extend(c.to_le_bytes());
				}
			}),
			AnyLayer::F32(l) => l.iter().for_each(|p| {
				for c in [p.red, p.green, p.blue, p.alpha] {
					bytes.extend(c.to_le_bytes());
				}
			}),
		}
		std::fs::File::create(path)
			.and_then(|mut f| f.write_all(&bytes))
			.map_err(|e| Error::harness(format!("writing frame {}: {e}", path.display())))
	}

	/// Read a frame written by [`Self::write_raw`].
	pub fn read_raw(path: &Path, width: u32, height: u32, depth: PixelDepthKind) -> Result<Frame> {
		let mut bytes = Vec::new();
		std::fs::File::open(path)
			.and_then(|mut f| f.read_to_end(&mut bytes))
			.map_err(|e| Error::harness(format!("reading frame {}: {e}", path.display())))?;
		let count = width as usize * height as usize;
		let expected = count * depth.bytes_per_pixel();
		if bytes.len() != expected {
			return Err(Error::harness(format!(
				"frame {} has {} bytes, expected {expected} for {width}x{height} at {depth}",
				path.display(),
				bytes.len()
			)));
		}
		let layer = match depth {
			PixelDepthKind::U8 => AnyLayer::new(
				Layer::<Depth8>::new(
					width,
					height,
					bytes
						.as_chunks::<4>()
						.0
						.iter()
						.map(|&[red, green, blue, alpha]| Pixel {
							alpha,
							red,
							green,
							blue,
						})
						.collect(),
				)
				.map_err(|e| Error::harness(e.to_string()))?,
			),
			PixelDepthKind::U16 => {
				let c: Vec<u16> = bytes
					.as_chunks::<2>()
					.0
					.iter()
					.map(|b| u16::from_le_bytes(*b))
					.collect();
				AnyLayer::new(
					Layer::<Depth16>::new(
						width,
						height,
						c.as_chunks::<4>()
							.0
							.iter()
							.map(|&[red, green, blue, alpha]| Pixel {
								alpha,
								red,
								green,
								blue,
							})
							.collect(),
					)
					.map_err(|e| Error::harness(e.to_string()))?,
				)
			}
			PixelDepthKind::F32 => {
				let c: Vec<f32> = bytes
					.as_chunks::<4>()
					.0
					.iter()
					.map(|b| f32::from_le_bytes(*b))
					.collect();
				AnyLayer::new(
					Layer::<Depth32>::new(
						width,
						height,
						c.as_chunks::<4>()
							.0
							.iter()
							.map(|&[red, green, blue, alpha]| Pixel {
								alpha,
								red,
								green,
								blue,
							})
							.collect(),
					)
					.map_err(|e| Error::harness(e.to_string()))?,
				)
			}
		};
		Ok(Frame::new(layer))
	}
}

/// AE's 16 bpc range (`0..=32768`) to PNG's (`0..=65535`).
fn ae16_to_png16(value: u16) -> u16 {
	((value.min(32768) as u32 * 65535 + 16384) / 32768) as u16
}

/// PNG's 16-bit range to AE's 16 bpc range; inverts [`ae16_to_png16`].
fn png16_to_ae16(value: u16) -> u16 {
	((value as u32 * 32768 + 32767) / 65535) as u16
}

/// Decode an image file into a layer at the depth it was stored at: 8-bit
/// files as 8 bpc, 16-bit as 16 bpc, float (EXR, ...) as 32 bpc.
pub fn load_image(path: &Path) -> Result<AnyLayer> {
	let img = image::open(path).map_err(|e| Error::harness(format!("reading {}: {e}", path.display())))?;
	let (w, h) = (img.width(), img.height());
	use image::DynamicImage as D;
	Ok(match img {
		D::ImageRgb32F(_) | D::ImageRgba32F(_) => {
			let data = img.into_rgba32f().into_raw();
			let pixels = data
				.as_chunks::<4>()
				.0
				.iter()
				.map(|&[red, green, blue, alpha]| Pixel {
					alpha,
					red,
					green,
					blue,
				})
				.collect();
			AnyLayer::new(Layer::<Depth32>::new(w, h, pixels).map_err(|e| Error::harness(e.to_string()))?)
		}
		D::ImageLuma16(_) | D::ImageLumaA16(_) | D::ImageRgb16(_) | D::ImageRgba16(_) => {
			let data = img.into_rgba16().into_raw();
			let pixels = data
				.as_chunks::<4>()
				.0
				.iter()
				.map(|&[r, g, b, a]| Pixel {
					alpha: png16_to_ae16(a),
					red: png16_to_ae16(r),
					green: png16_to_ae16(g),
					blue: png16_to_ae16(b),
				})
				.collect();
			AnyLayer::new(Layer::<Depth16>::new(w, h, pixels).map_err(|e| Error::harness(e.to_string()))?)
		}
		other => {
			let data = other.into_rgba8().into_raw();
			AnyLayer::new(Layer::<Depth8>::from_raw(data, w, h).map_err(|e| Error::harness(e.to_string()))?)
		}
	})
}

#[cfg(test)]
mod tests {
	use super::*;

	fn temp(name: &str) -> std::path::PathBuf {
		let dir = std::env::temp_dir().join(format!("aexlo-frame-{}", std::process::id()));
		std::fs::create_dir_all(&dir).unwrap();
		dir.join(name)
	}

	fn ramp(depth: PixelDepthKind) -> Frame {
		let mut layer = AnyLayer::filled(depth, 7, 3, [0.0; 4]);
		layer.set_from_unit_rgba((0..21).map(|i| [i as f32 / 20.0, 1.0 - i as f32 / 20.0, 0.25, 1.0]));
		Frame::new(layer)
	}

	#[test]
	fn png16_round_trips_every_ae_value() {
		for v in 0..=32768u16 {
			assert_eq!(png16_to_ae16(ae16_to_png16(v)), v);
		}
		assert_eq!(ae16_to_png16(32768), 65535);
	}

	#[test]
	fn files_round_trip_at_every_depth() {
		for depth in [PixelDepthKind::U8, PixelDepthKind::U16, PixelDepthKind::F32] {
			let frame = ramp(depth);
			let path = temp(&format!("ramp.{}.{}", depth.bits(), Frame::extension(depth)));
			frame.save(&path).unwrap();
			let back = Frame::load(&path).unwrap();
			assert_eq!(back.depth(), depth);
			let cmp = back.compare(&frame, &Tolerance::default());
			assert_eq!(cmp.max_abs, 0.0, "{depth}");

			let raw = temp(&format!("ramp.{}.raw", depth.bits()));
			frame.write_raw(&raw).unwrap();
			let back = Frame::read_raw(&raw, 7, 3, depth).unwrap();
			assert_eq!(back.compare(&frame, &Tolerance::default()).max_abs, 0.0);
		}
	}

	#[test]
	fn comparison_counts_bad_pixels_and_handles_nan() {
		let a = ramp(PixelDepthKind::F32);
		let mut layer = a.layer().clone();
		let mut values = layer.to_unit_rgba();
		values[0][0] += 0.5;
		values[1][1] = f32::NAN;
		layer.set_from_unit_rgba(values);
		let b = Frame::new(layer);

		let cmp = b.compare(&a, &Tolerance::default());
		assert_eq!(cmp.bad_pixels, 2);
		assert!(!cmp.passed);
		assert!(cmp.max_abs.is_infinite());

		let loose = Tolerance {
			max_bad: 0.1,
			..Tolerance::default()
		};
		assert!(b.compare(&a, &loose).passed);
		assert!(a.compare(&a, &Tolerance::default()).psnr.is_infinite());
		assert!(a.compare(&ramp(PixelDepthKind::U8), &Tolerance::default()).passed);
	}

	#[test]
	fn size_mismatch_fails() {
		let a = ramp(PixelDepthKind::U8);
		let b = Frame::new(AnyLayer::filled(PixelDepthKind::U8, 3, 3, [0.0; 4]));
		let cmp = a.compare(&b, &Tolerance::default());
		assert!(!cmp.passed);
		assert_eq!(cmp.size_mismatch, Some(([7, 3], [3, 3])));
	}
}
