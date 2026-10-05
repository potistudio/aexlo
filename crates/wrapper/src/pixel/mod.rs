mod float32;
mod uint16;
mod uint8;

use crate::layer::{AnyLayer, Layer};

/// The channel depth of a pixel buffer, as a value (see [`PixelDepth::KIND`]).
///
/// After Effects calls these 8, 16 and 32 bits per channel (bpc): 8 and 16 bpc
/// are integers (16 bpc is 15-bit, white is `32768`), 32 bpc is float.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PixelDepthKind {
	U8,
	U16,
	F32,
}

impl PixelDepthKind {
	/// Bits per channel: 8, 16 or 32.
	pub fn bits(self) -> u32 {
		match self {
			Self::U8 => 8,
			Self::U16 => 16,
			Self::F32 => 32,
		}
	}

	/// The depth with `bits` bits per channel, if it is one of 8, 16 or 32.
	pub fn from_bits(bits: u32) -> Option<Self> {
		match bits {
			8 => Some(Self::U8),
			16 => Some(Self::U16),
			32 => Some(Self::F32),
			_ => None,
		}
	}

	/// Bytes per ARGB pixel: 4, 8 or 16.
	pub fn bytes_per_pixel(self) -> usize {
		self.bits() as usize / 2
	}
}

impl core::fmt::Display for PixelDepthKind {
	fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
		write!(f, "{} bpc", self.bits())
	}
}

pub trait PixelDepth: Sized + Send + Sync + 'static {
	type Depth: Default + Copy + Clone + PartialEq + Send + Sync + core::fmt::Debug;

	/// This depth as a value.
	const KIND: PixelDepthKind;

	/// The channel value of full intensity (`255`, `32768` or `1.0`).
	fn max_value() -> Self::Depth;

	/// A channel value normalized to `[0, 1]` (float channels pass through
	/// unclamped, so out-of-range and non-finite values survive).
	fn to_unit(value: Self::Depth) -> f32;

	/// A normalized channel value at this depth, rounded and clamped for
	/// integer depths.
	fn from_unit(value: f32) -> Self::Depth;

	#[doc(hidden)]
	fn wrap(layer: Layer<Self>) -> AnyLayer;

	#[doc(hidden)]
	fn unwrap_ref(layer: &AnyLayer) -> Option<&Layer<Self>>;

	#[doc(hidden)]
	fn unwrap_mut(layer: &mut AnyLayer) -> Option<&mut Layer<Self>>;
}

/// A single ARGB pixel parameterized over its channel depth.
///
/// # Memory layout
///
/// The fields are laid out in `alpha, red, green, blue` order and the struct is
/// `#[repr(C)]`, so a `Pixel<Depth8>` is bit-compatible with `PF_Pixel8` (and
/// likewise for the 16-bit / 32-bit depths). This guarantee is relied upon when
/// a `Layer`'s pixel buffer is handed to the host as a raw `*mut PF_Pixel`.
///
/// Note that this is the *native After Effects* channel order (ARGB). Conversion
/// helpers such as `Layer::from_raw` / `Layer::write_rgba_bytes` deal in the
/// `RGBA` byte order used by external image formats.
#[repr(C)]
#[derive(Debug, Copy, PartialEq, Eq, Hash, Default)]
pub struct Pixel<T: PixelDepth> {
	pub alpha: T::Depth,
	pub red: T::Depth,
	pub green: T::Depth,
	pub blue: T::Depth,
}

impl<T: PixelDepth> Clone for Pixel<T> {
	fn clone(&self) -> Self {
		Pixel {
			alpha: self.alpha,
			red: self.red,
			green: self.green,
			blue: self.blue,
		}
	}
}

impl<T: PixelDepth> Pixel<T> {
	pub fn blank() -> Self {
		Pixel {
			alpha: T::Depth::default(),
			red: T::Depth::default(),
			green: T::Depth::default(),
			blue: T::Depth::default(),
		}
	}

	pub fn black() -> Self {
		Pixel {
			alpha: T::max_value(),
			red: T::Depth::default(),
			green: T::Depth::default(),
			blue: T::Depth::default(),
		}
	}

	/// The pixel as normalized `[r, g, b, a]`.
	pub fn to_unit(&self) -> [f32; 4] {
		[
			T::to_unit(self.red),
			T::to_unit(self.green),
			T::to_unit(self.blue),
			T::to_unit(self.alpha),
		]
	}

	/// A pixel from normalized `[r, g, b, a]`.
	pub fn from_unit([red, green, blue, alpha]: [f32; 4]) -> Self {
		Pixel {
			alpha: T::from_unit(alpha),
			red: T::from_unit(red),
			green: T::from_unit(green),
			blue: T::from_unit(blue),
		}
	}
}

pub use float32::Depth32;
pub use uint8::Depth8;
pub use uint16::Depth16;
