mod float32;
mod uint16;
mod uint8;

pub trait PixelDepth {
	type Depth: Default + Copy + Clone + PartialEq + Send + Sync;
	fn max_value() -> Self::Depth;
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
}

pub use float32::Depth32;
pub use uint8::Depth8;
pub use uint16::Depth16;
