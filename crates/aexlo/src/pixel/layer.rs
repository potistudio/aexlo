use after_effects_sys::{PF_LayerDef, PF_Pixel};

use super::{Depth8, Depth16, Depth32, Pixel, PixelDepth, PixelDepthKind};
use core::ops::{Index, IndexMut};
use std::ptr::null_mut;

/// An error building a [`Layer`] or copying its pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum LayerError {
	/// A buffer's length does not match the layer's dimensions.
	DimensionMismatch {
		/// The length the dimensions call for.
		expected: usize,
		/// The length of the buffer passed.
		actual: usize,
	},
	/// The dimensions are too large for a layer: a row (`width` pixels) must
	/// fit in `i32` bytes and `height` in `i32`, since that is how the host
	/// and plugins describe a world, and the pixel count must fit in `usize`.
	TooLarge {
		/// The width passed.
		width: u32,
		/// The height passed.
		height: u32,
	},
}

impl core::fmt::Display for LayerError {
	fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
		match self {
			Self::DimensionMismatch { expected, actual } => write!(
				f,
				"Pixel data length ({}) does not match layer dimensions ({}).",
				actual, expected
			),
			Self::TooLarge { width, height } => write!(f, "Layer dimensions {width}x{height} are too large."),
		}
	}
}

impl std::error::Error for LayerError {}

/// The number of pixels in a `width` x `height` layer at depth `D`, or `None`
/// if the layer could not be described to the host: `as_sys` passes the height
/// and the row stride (`width` pixels, in bytes) as `i32`.
fn pixel_count<D: PixelDepth>(width: u32, height: u32) -> Option<usize> {
	let row_bytes = (width as usize).checked_mul(core::mem::size_of::<Pixel<D>>())?;
	i32::try_from(row_bytes).ok()?;
	i32::try_from(height).ok()?;
	(width as usize).checked_mul(height as usize)
}

/// [`pixel_count`] for constructors that cannot fail.
fn pixel_count_or_panic<D: PixelDepth>(width: u32, height: u32) -> usize {
	pixel_count::<D>(width, height).unwrap_or_else(|| panic!("{}", LayerError::TooLarge { width, height }))
}

/// A 2D raster of `Pixel<D>` stored in row-major order.
///
/// The pixels are tightly packed (a row is exactly `width` pixels, no padding),
/// and `pixels.len() == width * height` always holds.
#[derive(Clone)]
pub struct Layer<D: PixelDepth> {
	width: u32,
	height: u32,
	pixels: Vec<Pixel<D>>,
}

impl<D> Layer<D>
where
	D: PixelDepth,
{
	/// Create a layer from dimensions and a pixel buffer in row-major order.
	///
	/// # Errors
	/// - [`LayerError::TooLarge`] if the dimensions cannot be described to the
	///   host (see the variant).
	/// - [`LayerError::DimensionMismatch`] if `pixels.len() != width * height`.
	pub fn new(width: u32, height: u32, pixels: Vec<Pixel<D>>) -> Result<Self, LayerError> {
		let expected = pixel_count::<D>(width, height).ok_or(LayerError::TooLarge { width, height })?;
		if pixels.len() != expected {
			return Err(LayerError::DimensionMismatch {
				expected,
				actual: pixels.len(),
			});
		}

		Ok(Self { width, height, pixels })
	}

	/// A layer of fully transparent black pixels ([`Pixel::blank`]).
	///
	/// # Panics
	/// If the dimensions are too large (see [`LayerError::TooLarge`]).
	pub fn blank(width: u32, height: u32) -> Self {
		let pixel_count = pixel_count_or_panic::<D>(width, height);
		let pixels = vec![<Pixel<D>>::blank(); pixel_count];

		Self { width, height, pixels }
	}

	/// A layer of opaque black pixels ([`Pixel::black`]).
	///
	/// # Panics
	/// If the dimensions are too large (see [`LayerError::TooLarge`]).
	pub fn black(width: u32, height: u32) -> Self {
		let pixel_count = pixel_count_or_panic::<D>(width, height);
		let pixels = vec![<Pixel<D>>::black(); pixel_count];

		Self { width, height, pixels }
	}

	/// A layer whose pixel at `(x, y)` is `f(x, y)`.
	///
	/// # Panics
	/// If the dimensions are too large (see [`LayerError::TooLarge`]).
	pub fn from_fn(width: u32, height: u32, mut f: impl FnMut(u32, u32) -> Pixel<D>) -> Self {
		let mut pixels = Vec::with_capacity(pixel_count_or_panic::<D>(width, height));
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
		self.index_of(x, y).and_then(|idx| self.pixels.get(idx))
	}

	/// Get a mutable reference to a pixel by coordinates (x, y).
	/// None if the coordinates is out of bounds.
	pub fn get_mut(&mut self, x: u32, y: u32) -> Option<&mut Pixel<D>> {
		self.index_of(x, y).and_then(|idx| self.pixels.get_mut(idx))
	}

	/// The linear index of `(x, y)`, or `None` if it is out of bounds.
	fn index_of(&self, x: u32, y: u32) -> Option<usize> {
		(x < self.width && y < self.height).then(|| y as usize * self.width as usize + x as usize)
	}

	/// Iterate over pixels by reference in row-major order.
	pub fn iter(&self) -> core::slice::Iter<'_, Pixel<D>> {
		self.pixels.iter()
	}

	/// Iterate mutably over pixels by reference in row-major order.
	pub fn iter_mut(&mut self) -> core::slice::IterMut<'_, Pixel<D>> {
		self.pixels.iter_mut()
	}

	/// A `PF_LayerDef` (effect world) over this layer's pixels, for passing to
	/// a plugin.
	///
	/// The world borrows the pixel buffer through a raw pointer: it is valid
	/// only while this layer is alive and not resized.
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
			pix_aspect_ratio: after_effects_sys::PF_RationalScale { num: 1, den: 1 },
			reserved_long2: null_mut(),
			origin_x: 0,
			origin_y: 0,
			reserved_long3: 0,
			dephault: 0,
			data,
			// `pixel_count` guarantees the row stride fits in `i32`.
			rowbytes: (self.width as usize * std::mem::size_of::<Pixel<D>>()) as i32,
		}
	}
}

impl Layer<Depth8> {
	/// An 8 bpc layer from straight RGBA bytes in row-major order, as image
	/// decoders produce them.
	///
	/// # Errors
	/// [`LayerError::DimensionMismatch`] if `pixels.len() != width * height * 4`.
	pub fn from_raw(pixels: Vec<u8>, width: u32, height: u32) -> Result<Self, LayerError> {
		let expected = pixel_count::<Depth8>(width, height)
			.and_then(|count| count.checked_mul(4))
			.ok_or(LayerError::TooLarge { width, height })?;
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
			.map(|&rgba| Pixel::from_rgba(rgba))
			.collect();

		Layer::new(width, height, converted)
	}

	/// Write RGBA bytes directly into an existing buffer (zero-allocation).
	///
	/// # Errors
	/// [`LayerError::DimensionMismatch`] unless the buffer has exactly
	/// `width * height * 4` bytes.
	pub fn write_rgba_bytes(&self, buffer: &mut [u8]) -> Result<(), LayerError> {
		let required = self.pixels.len() * 4;

		if buffer.len() != required {
			return Err(LayerError::DimensionMismatch {
				expected: required,
				actual: buffer.len(),
			});
		}

		for (chunk, pixel) in buffer.as_chunks_mut::<4>().0.iter_mut().zip(self.pixels.iter()) {
			*chunk = pixel.to_rgba();
		}

		Ok(())
	}
}

/// A [`Layer`] of any depth, for code that picks the depth at run time.
///
/// Deliberately exhaustive: it has one variant per [`PixelDepthKind`], a
/// closed set.
#[derive(Clone)]
pub enum AnyLayer {
	/// An 8 bpc layer.
	U8(Layer<Depth8>),
	/// A 16 bpc layer.
	U16(Layer<Depth16>),
	/// A 32 bpc layer.
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
	/// Wrap a typed layer.
	pub fn new<D: PixelDepth>(layer: Layer<D>) -> Self {
		D::wrap(layer)
	}

	/// A `width` x `height` layer at `kind`, every pixel the normalized `rgba`.
	///
	/// # Panics
	/// If the dimensions are too large (see [`LayerError::TooLarge`]).
	pub fn filled(kind: PixelDepthKind, width: u32, height: u32, rgba: [f32; 4]) -> Self {
		fn fill<D: PixelDepth>(width: u32, height: u32, rgba: [f32; 4]) -> AnyLayer {
			let count = pixel_count_or_panic::<D>(width, height);
			AnyLayer::new(Layer::<D> {
				width,
				height,
				pixels: vec![Pixel::<D>::from_unit(rgba); count],
			})
		}
		match kind {
			PixelDepthKind::U8 => fill::<Depth8>(width, height, rgba),
			PixelDepthKind::U16 => fill::<Depth16>(width, height, rgba),
			PixelDepthKind::F32 => fill::<Depth32>(width, height, rgba),
		}
	}

	/// The layer's depth.
	pub fn kind(&self) -> PixelDepthKind {
		match self {
			Self::U8(_) => PixelDepthKind::U8,
			Self::U16(_) => PixelDepthKind::U16,
			Self::F32(_) => PixelDepthKind::F32,
		}
	}

	/// The width in pixels.
	pub fn width(&self) -> u32 {
		with_layer!(self, l => l.width())
	}

	/// The number of pixels (width * height).
	pub fn len(&self) -> usize {
		with_layer!(self, l => l.len())
	}

	/// True when the layer has zero pixels.
	pub fn is_empty(&self) -> bool {
		self.len() == 0
	}

	/// The height in pixels.
	pub fn height(&self) -> u32 {
		with_layer!(self, l => l.height())
	}

	/// The typed layer, if it is at depth `D`.
	pub fn get<D: PixelDepth>(&self) -> Option<&Layer<D>> {
		D::unwrap_ref(self)
	}

	/// The typed layer mutably, if it is at depth `D`.
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

	/// Write the layer as 8-bit RGBA bytes, quantizing deeper channels.
	///
	/// # Errors
	/// [`LayerError::DimensionMismatch`] unless the buffer has exactly
	/// `width * height * 4` bytes.
	pub fn write_rgba8(&self, buffer: &mut [u8]) -> Result<(), LayerError> {
		if let Self::U8(layer) = self {
			return layer.write_rgba_bytes(buffer);
		}
		let required = self.len() * 4;
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

impl<D: PixelDepth> core::fmt::Debug for Layer<D> {
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
		&self.pixels[y as usize * self.width as usize + x as usize]
	}
}

impl<D: PixelDepth> IndexMut<(u32, u32)> for Layer<D> {
	fn index_mut(&mut self, index: (u32, u32)) -> &mut Self::Output {
		let (x, y) = index;
		assert!(x < self.width, "X coordinate out of bounds.");
		assert!(y < self.height, "Y coordinate out of bounds.");
		&mut self.pixels[y as usize * self.width as usize + x as usize]
	}
}
