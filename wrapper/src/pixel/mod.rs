mod float32;
mod uint16;
mod uint8;

pub trait PixelDepth {
	type Depth;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Pixel<T: PixelDepth> {
	pub alpha: T::Depth,
	pub red: T::Depth,
	pub green: T::Depth,
	pub blue: T::Depth,
}

pub use float32::Depth32;
pub use uint8::Depth8;
pub use uint16::Depth16;
