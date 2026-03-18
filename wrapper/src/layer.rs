use after_effects_sys::{PF_LayerDef, PF_Pixel};

use super::pixel::{Depth8, Pixel, PixelDepth};
use core::ops::{Index, IndexMut};
use std::ptr::null_mut;

/// A 2D raster of `Pixel<D>` stored in row-major order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerError {
	DimensionMismatch { expected: usize, actual: usize },
}

impl core::fmt::Display for LayerError {
	fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
		match self {
			Self::DimensionMismatch { expected, actual } => write!(
				f,
				"Pixel data length ({}) does not match layer dimensions ({}).",
				actual, expected
			),
		}
	}
}

impl std::error::Error for LayerError {}

/// A 2D raster of `Pixel<D>` stored in row-major order.
pub struct Layer<D: PixelDepth> {
	width: u32,
	height: u32,
	pixels: Vec<Pixel<D>>,
}

impl<D> Layer<D>
where
	D: PixelDepth,
{
	/// Create a layer from dimensions and a pixel buffer.
	///
	/// Returns an error if `pixels.len() != width * height`.
	pub fn new(width: u32, height: u32, pixels: Vec<Pixel<D>>) -> Result<Self, LayerError> {
		let expected = (width * height) as usize;
		if pixels.len() != expected {
			return Err(LayerError::DimensionMismatch {
				expected,
				actual: pixels.len(),
			});
		}

		Ok(Self { width, height, pixels })
	}

	pub fn from_raw(pixels: Vec<u8>, width: u32, height: u32) -> Result<Layer<Depth8>, LayerError> {
		let expected = (width * height * 4) as usize;
		if pixels.len() != expected {
			return Err(LayerError::DimensionMismatch {
				expected,
				actual: pixels.len(),
			});
		}

		let converted = pixels
			.chunks_exact(4)
			.map(|chunk| Pixel::<Depth8> {
				red: chunk[0],
				green: chunk[1],
				blue: chunk[2],
				alpha: chunk[3],
			})
			.collect();

		Layer::<Depth8>::new(width, height, converted)
	}

	pub fn blank(width: u32, height: u32) -> Self
	where
		Pixel<D>: Default,
	{
		let pixel_count = (width * height) as usize;
		let pixels = vec![<Pixel<D>>::blank(); pixel_count];

		Self { width, height, pixels }
	}

	pub fn black(width: u32, height: u32) -> Self
	where
		Pixel<D>: Default,
	{
		let pixel_count = (width * height) as usize;
		let pixels = vec![<Pixel<D>>::black(); pixel_count];

		Self { width, height, pixels }
	}

	//==== Getter ==========================================
	/// Return the width in pixels.
	pub fn width(&self) -> u32 {
		self.width
	}

	/// Return the height in pixels.
	pub fn height(&self) -> u32 {
		self.height
	}

	/// Number of pixels (width * height).
	pub fn len(&self) -> usize {
		self.pixels.len()
	}

	/// True when this layer has zero pixels.
	pub fn is_empty(&self) -> bool {
		self.pixels.is_empty()
	}

	/// Get a reference to the underlying pixel buffer.
	pub fn pixels(&self) -> &[Pixel<D>] {
		&self.pixels
	}

	/// Get a mutable reference to the underlying pixel buffer.
	pub fn pixels_mut(&mut self) -> &mut [Pixel<D>] {
		&mut self.pixels
	}

	/// Get a reference to a pixel by linear index (row-major).
	pub fn get_linear(&self, index: usize) -> Option<&Pixel<D>> {
		self.pixels.get(index)
	}

	/// Get a mutable reference to a pixel by linear index (row-major).
	pub fn get_linear_mut(&mut self, index: usize) -> Option<&mut Pixel<D>> {
		self.pixels.get_mut(index)
	}

	/// Get a reference to a pixel by coordinates (x, y).
	/// None if the coordinates is out of bounds.
	pub fn get(&self, x: u32, y: u32) -> Option<&Pixel<D>> {
		if x >= self.width || y >= self.height {
			return None;
		}

		let idx = (y * self.width + x) as usize;

		// SAFETY: We have already checked that x and y are within bounds, so idx is guaranteed to be valid.
		Some(unsafe { self.pixels.get_unchecked(idx) })
	}

	/// Get a mutable reference to a pixel by coordinates (x, y).
	/// None if the coordinates is out of bounds.
	pub fn get_mut(&mut self, x: u32, y: u32) -> Option<&mut Pixel<D>> {
		if x >= self.width || y >= self.height {
			return None;
		}

		let idx = (y * self.width + x) as usize;

		// SAFETY: We have already checked that x and y are within bounds, so idx is guaranteed to be valid.
		Some(unsafe { self.pixels.get_unchecked_mut(idx) })
	}

	/// Iterate over pixels by reference in row-major order.
	pub fn iter(&self) -> core::slice::Iter<'_, Pixel<D>> {
		self.pixels.iter()
	}

	/// Iterate mutably over pixels by reference in row-major order.
	pub fn iter_mut(&mut self) -> core::slice::IterMut<'_, Pixel<D>> {
		self.pixels.iter_mut()
	}

	pub fn as_sys(&mut self) -> PF_LayerDef {
		PF_LayerDef {
			reserved0: null_mut(),
			reserved1: null_mut(),
			world_flags: 0 as after_effects_sys::PF_WorldFlags,
			width: self.width as i32,
			height: self.height as i32,
			extent_hint: after_effects_sys::PF_UnionableRect {
				left: 0,
				top: 0,
				right: self.width as i32,
				bottom: self.height as i32,
			},
			platform_ref: null_mut(),
			reserved_long1: 0,
			reserved_long4: null_mut(),
			pix_aspect_ratio: after_effects_sys::PF_RationalScale { num: 1, den: 1 }, // Fixed: den should not be 0
			reserved_long2: null_mut(),
			origin_x: 0,
			origin_y: 0,
			reserved_long3: 0,
			dephault: 0,
			data: self.pixels.as_ptr() as *mut PF_Pixel,
			rowbytes: (self.width as i32) * (std::mem::size_of::<Pixel<D>>() as i32),
		}
	}
}

impl Layer<Depth8> {
	/// Write RGBA bytes directly into an existing buffer (zero-allocation).
	/// The buffer must have exactly `width * height * 4` bytes.
	pub fn write_rgba_bytes(&self, buffer: &mut [u8]) -> Result<(), String> {
		let required = self.pixels.len() * 4;

		if buffer.len() != required {
			return Err(format!(
				"Buffer length ({}) does not match required size ({}).",
				buffer.len(),
				required
			));
		}

		for (i, pixel) in self.pixels.iter().enumerate() {
			let offset = i * 4;
			buffer[offset] = pixel.red;
			buffer[offset + 1] = pixel.green;
			buffer[offset + 2] = pixel.blue;
			buffer[offset + 3] = pixel.alpha;
		}

		Ok(())
	}
}

impl<D> core::fmt::Debug for Layer<D>
where
	D: PixelDepth,
	Pixel<D>: core::fmt::Debug,
{
	fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
		f.debug_struct("Layer")
			.field("width", &self.width)
			.field("height", &self.height)
			.field("pixels", &self.pixels)
			.finish()
	}
}

impl<D: PixelDepth> Index<(u32, u32)> for Layer<D> {
	type Output = Pixel<D>;

	fn index(&self, index: (u32, u32)) -> &Self::Output {
		let (x, y) = index;
		assert!(x < self.width, "X coordinate out of bounds.");
		assert!(y < self.height, "Y coordinate out of bounds.");
		let idx = (y * self.width + x) as usize;
		&self.pixels[idx]
	}
}

impl<D: PixelDepth> IndexMut<(u32, u32)> for Layer<D> {
	fn index_mut(&mut self, index: (u32, u32)) -> &mut Self::Output {
		let (x, y) = index;
		assert!(x < self.width, "X coordinate out of bounds.");
		assert!(y < self.height, "Y coordinate out of bounds.");
		let idx = (y * self.width + x) as usize;
		&mut self.pixels[idx]
	}
}
