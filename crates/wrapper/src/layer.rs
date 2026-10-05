use after_effects_sys::{PF_LayerDef, PF_Pixel};

use super::pixel::{Depth8, Depth16, Depth32, Pixel, PixelDepth, PixelDepthKind};
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
			.as_chunks::<4>()
			.0
			.iter()
			.map(|&[red, green, blue, alpha]| Pixel::<Depth8> {
				red,
				green,
				blue,
				alpha,
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

	/// A layer whose pixel at `(x, y)` is `f(x, y)`.
	pub fn from_fn(width: u32, height: u32, mut f: impl FnMut(u32, u32) -> Pixel<D>) -> Self {
		let mut pixels = Vec::with_capacity(width as usize * height as usize);
		for y in 0..height {
			for x in 0..width {
				pixels.push(f(x, y));
			}
		}
		Self { width, height, pixels }
	}

	/// This layer at another depth, through normalized `[0, 1]` values.
	pub fn convert<E: PixelDepth>(&self) -> Layer<E> {
		Layer {
			width: self.width,
			height: self.height,
			pixels: self.pixels.iter().map(|p| Pixel::<E>::from_unit(p.to_unit())).collect(),
		}
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
		// `data` must come from `as_mut_ptr`: the host/plugin writes through it,
		// and a pointer derived from `as_ptr` only carries read provenance.
		let data = self.pixels.as_mut_ptr() as *mut PF_Pixel;
		PF_LayerDef {
			reserved0: null_mut(),
			reserved1: null_mut(),
			// AE marks 16 bpc worlds deep (`PF_WORLD_IS_DEEP`); 32 bpc worlds are
			// told apart through `PF_GetPixelFormat` instead.
			world_flags: if D::KIND == PixelDepthKind::U16 {
				after_effects_sys::PF_WorldFlag_DEEP as after_effects_sys::PF_WorldFlags
			} else {
				0
			},
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
			data,
			rowbytes: (self.width as i32) * (std::mem::size_of::<Pixel<D>>() as i32),
		}
	}
}

impl Layer<Depth8> {
	/// Write RGBA bytes directly into an existing buffer (zero-allocation).
	/// The buffer must have exactly `width * height * 4` bytes.
	pub fn write_rgba_bytes(&self, buffer: &mut [u8]) -> Result<(), LayerError> {
		let required = self.pixels.len() * 4;

		if buffer.len() != required {
			return Err(LayerError::DimensionMismatch {
				expected: required,
				actual: buffer.len(),
			});
		}

		for (chunk, pixel) in buffer.as_chunks_mut::<4>().0.iter_mut().zip(self.pixels.iter()) {
			*chunk = [pixel.red, pixel.green, pixel.blue, pixel.alpha];
		}

		Ok(())
	}
}

impl<D: PixelDepth> Clone for Layer<D> {
	fn clone(&self) -> Self {
		Self {
			width: self.width,
			height: self.height,
			pixels: self.pixels.clone(),
		}
	}
}

/// A [`Layer`] of any depth, for code that picks the depth at run time.
#[derive(Clone)]
pub enum AnyLayer {
	U8(Layer<Depth8>),
	U16(Layer<Depth16>),
	F32(Layer<Depth32>),
}

/// Run `$body` with `$layer` bound to the typed layer inside `$any`.
macro_rules! with_layer {
	($any:expr, $layer:ident => $body:expr) => {
		match $any {
			AnyLayer::U8($layer) => $body,
			AnyLayer::U16($layer) => $body,
			AnyLayer::F32($layer) => $body,
		}
	};
}

impl AnyLayer {
	pub fn new<D: PixelDepth>(layer: Layer<D>) -> Self {
		D::wrap(layer)
	}

	/// A `width` x `height` layer at `kind`, every pixel the normalized `rgba`.
	pub fn filled(kind: PixelDepthKind, width: u32, height: u32, rgba: [f32; 4]) -> Self {
		let count = width as usize * height as usize;
		fn fill<D: PixelDepth>(width: u32, height: u32, count: usize, rgba: [f32; 4]) -> AnyLayer {
			AnyLayer::new(Layer::<D> {
				width,
				height,
				pixels: vec![Pixel::<D>::from_unit(rgba); count],
			})
		}
		match kind {
			PixelDepthKind::U8 => fill::<Depth8>(width, height, count, rgba),
			PixelDepthKind::U16 => fill::<Depth16>(width, height, count, rgba),
			PixelDepthKind::F32 => fill::<Depth32>(width, height, count, rgba),
		}
	}

	pub fn kind(&self) -> PixelDepthKind {
		match self {
			Self::U8(_) => PixelDepthKind::U8,
			Self::U16(_) => PixelDepthKind::U16,
			Self::F32(_) => PixelDepthKind::F32,
		}
	}

	pub fn width(&self) -> u32 {
		with_layer!(self, l => l.width())
	}

	pub fn height(&self) -> u32 {
		with_layer!(self, l => l.height())
	}

	/// The typed layer, if it is at depth `D`.
	pub fn get<D: PixelDepth>(&self) -> Option<&Layer<D>> {
		D::unwrap_ref(self)
	}

	/// The typed layer, if it is at depth `D`.
	pub fn get_mut<D: PixelDepth>(&mut self) -> Option<&mut Layer<D>> {
		D::unwrap_mut(self)
	}

	/// A world over this layer's pixels (see [`Layer::as_sys`]).
	pub fn as_sys(&mut self) -> PF_LayerDef {
		with_layer!(self, l => l.as_sys())
	}

	/// Base address of the pixel buffer.
	pub fn data_ptr(&self) -> *const u8 {
		with_layer!(self, l => l.pixels().as_ptr() as *const u8)
	}

	/// Mutable base address of the pixel buffer.
	pub fn data_mut_ptr(&mut self) -> *mut u8 {
		with_layer!(self, l => l.pixels_mut().as_mut_ptr() as *mut u8)
	}

	/// The pixel at linear index `index`, as normalized `[r, g, b, a]`.
	pub fn unit_rgba(&self, index: usize) -> Option<[f32; 4]> {
		with_layer!(self, l => l.get_linear(index).map(Pixel::to_unit))
	}

	/// Every pixel as normalized `[r, g, b, a]`, in row-major order.
	pub fn to_unit_rgba(&self) -> Vec<[f32; 4]> {
		with_layer!(self, l => l.iter().map(Pixel::to_unit).collect())
	}

	/// Overwrite every pixel from normalized `[r, g, b, a]` values in
	/// row-major order. Extra or missing values are ignored.
	pub fn set_from_unit_rgba(&mut self, values: impl IntoIterator<Item = [f32; 4]>) {
		fn fill<D: PixelDepth>(l: &mut Layer<D>, values: impl IntoIterator<Item = [f32; 4]>) {
			for (pixel, rgba) in l.iter_mut().zip(values) {
				*pixel = Pixel::from_unit(rgba);
			}
		}
		with_layer!(self, l => fill(l, values))
	}

	/// This layer at another depth (a copy, even when `kind` is unchanged).
	pub fn converted(&self, kind: PixelDepthKind) -> AnyLayer {
		fn to<E: PixelDepth>(layer: &AnyLayer) -> AnyLayer {
			AnyLayer::new(with_layer!(layer, l => l.convert::<E>()))
		}
		match kind {
			PixelDepthKind::U8 => to::<Depth8>(self),
			PixelDepthKind::U16 => to::<Depth16>(self),
			PixelDepthKind::F32 => to::<Depth32>(self),
		}
	}

	/// Write the layer as 8-bit RGBA bytes, quantizing deeper channels. The
	/// buffer must have exactly `width * height * 4` bytes.
	pub fn write_rgba8(&self, buffer: &mut [u8]) -> Result<(), LayerError> {
		if let Self::U8(layer) = self {
			return layer.write_rgba_bytes(buffer);
		}
		let required = self.width() as usize * self.height() as usize * 4;
		if buffer.len() != required {
			return Err(LayerError::DimensionMismatch {
				expected: required,
				actual: buffer.len(),
			});
		}
		fn write<D: PixelDepth>(l: &Layer<D>, buffer: &mut [u8]) {
			for (chunk, pixel) in buffer.as_chunks_mut::<4>().0.iter_mut().zip(l.iter()) {
				*chunk = pixel.to_unit().map(Depth8::from_unit);
			}
		}
		with_layer!(self, l => write(l, buffer));
		Ok(())
	}
}

impl<D: PixelDepth> From<Layer<D>> for AnyLayer {
	fn from(layer: Layer<D>) -> Self {
		Self::new(layer)
	}
}

impl core::fmt::Debug for AnyLayer {
	fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
		f.debug_struct("AnyLayer")
			.field("kind", &self.kind())
			.field("width", &self.width())
			.field("height", &self.height())
			.finish_non_exhaustive()
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
