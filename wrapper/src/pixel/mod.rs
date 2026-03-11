mod float32;
mod uint16;
mod uint8;

pub trait PixelDepth {
	type Depth: Default + Copy + Clone + PartialEq + Send + Sync;
	fn max_value() -> Self::Depth;
}

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
