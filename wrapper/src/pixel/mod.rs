mod uint8;
mod uint16;
mod float32;

pub trait PixelDepth {
	type Depth;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Pixel<T: PixelDepth> {
	pub alpha: T::Depth,
	pub red: T::Depth,
	pub green: T::Depth,
	pub blue: T::Depth,
}

pub use uint8::Depth8;
pub use uint16::Depth16;
pub use float32::Depth32;
