//! Safe newtype wrappers around After Effects low-level pixel/layer types.

mod float32;
mod layer;
mod uint16;
mod uint8;

pub use layer::*;

/// The channel depth of a pixel buffer, as a value (see [`PixelDepth::KIND`]).
///
/// After Effects calls these 8, 16 and 32 bits per channel (bpc): 8 and 16 bpc
/// are integers (16 bpc is 15-bit, white is `32768`), 32 bpc is float.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PixelDepthKind {
	/// 8 bpc, `u8` channels ([`Depth8`]).
	U8,
	/// 16 bpc, `u16` channels in `0..=32768` ([`Depth16`]).
	U16,
	/// 32 bpc, `f32` channels with `1.0` as full intensity ([`Depth32`]).
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

mod sealed {
	/// Keeps [`PixelDepth`](super::PixelDepth) implementable only here: the
	/// closed set of depths is what lets [`AnyLayer`](super::AnyLayer) be a
	/// plain enum.
	pub trait Sealed {}
}

/// A channel depth at the type level: [`Depth8`], [`Depth16`] or [`Depth32`].
/// [`Pixel`] and [`Layer`] are generic over it.
///
/// This trait is sealed: the three depths After Effects has are the only ones.
pub trait PixelDepth: sealed::Sealed + Copy + Default + core::fmt::Debug + Send + Sync + 'static {
	/// The type of one channel (`u8`, `u16` or `f32`).
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
/// Note that this is the *native After Effects* channel order (ARGB), which is
/// also the order of the `From<[_; 4]>` conversions and of
/// [`Pixel::from_argb`]. Conversion helpers such as `Layer::from_raw` /
/// `Layer::write_rgba_bytes` deal in the `RGBA` byte order used by external
/// image formats ([`Pixel::from_rgba`]).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Pixel<T: PixelDepth> {
	/// Alpha, straight (not premultiplied).
	pub alpha: T::Depth,
	/// Red.
	pub red: T::Depth,
	/// Green.
	pub green: T::Depth,
	/// Blue.
	pub blue: T::Depth,
}

impl<T: PixelDepth> Pixel<T> {
	/// Fully transparent black: every channel zero.
	pub fn blank() -> Self {
		Pixel {
			alpha: T::Depth::default(),
			red: T::Depth::default(),
			green: T::Depth::default(),
			blue: T::Depth::default(),
		}
	}

	/// Opaque black.
	pub fn black() -> Self {
		Self::opaque(false, false, false)
	}

	/// Opaque white.
	pub fn white() -> Self {
		Self::opaque(true, true, true)
	}

	/// Opaque red.
	pub fn red() -> Self {
		Self::opaque(true, false, false)
	}

	/// Opaque green.
	pub fn green() -> Self {
		Self::opaque(false, true, false)
	}

	/// Opaque blue.
	pub fn blue() -> Self {
		Self::opaque(false, false, true)
	}

	/// Opaque cyan.
	pub fn cyan() -> Self {
		Self::opaque(false, true, true)
	}

	/// Opaque magenta.
	pub fn magenta() -> Self {
		Self::opaque(true, false, true)
	}

	/// Opaque magenta (`#ff00ff`), under its old name.
	#[deprecated(note = "this is magenta; use `Pixel::magenta`")]
	pub fn purple() -> Self {
		Self::magenta()
	}

	/// Opaque yellow.
	pub fn yellow() -> Self {
		Self::opaque(true, true, false)
	}

	/// Fully transparent black; same as [`Pixel::blank`].
	pub fn transparent() -> Self {
		Self::blank()
	}

	/// An opaque pixel whose channels are each either full intensity or zero.
	fn opaque(red: bool, green: bool, blue: bool) -> Self {
		let channel = |on| if on { T::max_value() } else { T::Depth::default() };
		Pixel {
			alpha: T::max_value(),
			red: channel(red),
			green: channel(green),
			blue: channel(blue),
		}
	}

	/// A pixel from channels in the native After Effects order, `[a, r, g, b]`.
	pub fn from_argb([alpha, red, green, blue]: [T::Depth; 4]) -> Self {
		Pixel { alpha, red, green, blue }
	}

	/// The channels in the native After Effects order, `[a, r, g, b]`.
	pub fn to_argb(&self) -> [T::Depth; 4] {
		[self.alpha, self.red, self.green, self.blue]
	}

	/// A pixel from channels in the order image formats use, `[r, g, b, a]`.
	pub fn from_rgba([red, green, blue, alpha]: [T::Depth; 4]) -> Self {
		Pixel { alpha, red, green, blue }
	}

	/// The channels in the order image formats use, `[r, g, b, a]`.
	pub fn to_rgba(&self) -> [T::Depth; 4] {
		[self.red, self.green, self.blue, self.alpha]
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
