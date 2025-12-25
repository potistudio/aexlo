use after_effects_sys::{PF_LayerDef, PF_Pixel};

use super::pixel::{Depth8, Pixel, PixelDepth};
use core::ops::{Index, IndexMut};
use std::ptr::null_mut;

/// A 2D raster of `Pixel<D>` stored in row-major order.
pub struct Layer<D: PixelDepth> {
	width: u32,
	height: u32,
	pub pixels: Vec<Pixel<D>>,
}

impl<D> Layer<D>
where
	D: PixelDepth,
{
	/// Create a layer from dimensions and a pixel buffer.
	///
	/// Panics if `pixels.len() != width * height`.
	pub fn from_vec(width: u32, height: u32, pixels: Vec<Pixel<D>>) -> Self {
		assert_eq!(
			pixels.len(),
			(width * height) as usize,
			"Pixel data length does not match layer dimensions."
		);

		Self {
			width,
			height,
			pixels,
		}
	}

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

	/// Get a reference to a pixel by (x, y) with bounds checking.
	///
	/// Panics if coordinates are out of bounds.
	pub fn at(&self, x: u32, y: u32) -> &Pixel<D> {
		assert!(x < self.width, "X coordinate out of bounds.");
		assert!(y < self.height, "Y coordinate out of bounds.");

		let idx = (y * self.width + x) as usize;
		&self.pixels[idx]
	}

	/// Mutable access to a pixel at (x, y) with bounds checking.
	pub fn at_mut(&mut self, x: u32, y: u32) -> &mut Pixel<D> {
		assert!(x < self.width, "X coordinate out of bounds.");
		assert!(y < self.height, "Y coordinate out of bounds.");

		let idx = (y * self.width + x) as usize;
		&mut self.pixels[idx]
	}

	/// Get a reference to a pixel by linear index (row-major).
	pub fn get_index(&self, index: usize) -> Option<&Pixel<D>> {
		self.pixels.get(index)
	}

	/// Mutable get by linear index.
	pub fn get_index_mut(&mut self, index: usize) -> Option<&mut Pixel<D>> {
		self.pixels.get_mut(index)
	}

	/// Fallible get by coordinates returning Option.
	pub fn get(&self, x: u32, y: u32) -> Option<&Pixel<D>> {
		if x >= self.width || y >= self.height {
			return None;
		}
		let idx = (y * self.width + x) as usize;
		self.pixels.get(idx)
	}

	/// Mutable fallible get by coordinates.
	pub fn get_mut(&mut self, x: u32, y: u32) -> Option<&mut Pixel<D>> {
		if x >= self.width || y >= self.height {
			return None;
		}
		let idx = (y * self.width + x) as usize;
		self.pixels.get_mut(idx)
	}

	/// Iterate over pixels by reference in row-major order.
	pub fn iter(&self) -> core::slice::Iter<'_, Pixel<D>> {
		self.pixels.iter()
	}

	/// Iterate mutably over pixels by reference in row-major order.
	pub fn iter_mut(&mut self) -> core::slice::IterMut<'_, Pixel<D>> {
		self.pixels.iter_mut()
	}

	/// Consume the layer and return the inner pixel buffer.
	pub fn into_vec(self) -> Vec<Pixel<D>> {
		self.pixels
	}

	pub fn as_sys(&self) -> PF_LayerDef {
		PF_LayerDef {
			reserved0: null_mut(),
			reserved1: null_mut(),
			world_flags: 0 as after_effects_sys::PF_WorldFlags,
			width: self.width as i32,
			height: self.height as i32,
			extent_hint: after_effects_sys::PF_UnionableRect {
				left: 0,
				top: 0,
				right: 0,
				bottom: 0,
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
			rowbytes: self.width as i32,
		}
	}
}

impl Layer<Depth8> {
	/// Create a blank layer filled with black `Pixel<Depth8>`.
	pub fn blank(width: u32, height: u32) -> Self {
		let pixel_count = (width * height) as usize;
		let pixels = vec![Pixel::<Depth8>::black(); pixel_count];

		Self {
			width,
			height,
			pixels,
		}
	}
}

// Conditional implements so `Layer<D>` implements common traits when the inner pixel type does.
impl<D> Clone for Layer<D>
where
	D: PixelDepth,
	Pixel<D>: Clone,
{
	fn clone(&self) -> Self {
		Self {
			width: self.width,
			height: self.height,
			pixels: self.pixels.clone(),
		}
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

impl<D> PartialEq for Layer<D>
where
	D: PixelDepth,
	Pixel<D>: PartialEq,
{
	fn eq(&self, other: &Self) -> bool {
		self.width == other.width && self.height == other.height && self.pixels == other.pixels
	}
}

impl<D> Eq for Layer<D>
where
	D: PixelDepth,
	Pixel<D>: Eq,
{
}

impl<D: PixelDepth> Index<(u32, u32)> for Layer<D> {
	type Output = Pixel<D>;

	fn index(&self, index: (u32, u32)) -> &Self::Output {
		let (x, y) = index;
		self.at(x, y)
	}
}

impl<D: PixelDepth> IndexMut<(u32, u32)> for Layer<D> {
	fn index_mut(&mut self, index: (u32, u32)) -> &mut Self::Output {
		let (x, y) = index;
		self.at_mut(x, y)
	}
}
