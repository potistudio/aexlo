use super::pixel::{Pixel, PixelDepth, Depth8};

pub struct Layer<T: PixelDepth> {
	width: u32,
	height: u32,
	pub pixels: Vec<Pixel<T>>,
}

impl<D> Layer<D>
where
	D: PixelDepth,
{
	pub fn new(width: u32, height: u32, pixels: Vec<Pixel<D>>) -> Self {
		assert_eq!(pixels.len(), (width * height) as usize, "Pixel data length does not match layer dimensions.");

		Layer {
			width,
			height,
			pixels,
		}
	}

	pub fn at(&self, x: u32, y: u32) -> &Pixel<D> {
		assert!(x < self.width, "X coordinate out of bounds.");
		assert!(y < self.height, "Y coordinate out of bounds.");

		let index = (y * self.width + x) as usize;
		&self.pixels[index]
	}
}

impl Layer<Depth8> {
	pub fn blank(width: u32, height: u32) -> Self {
		let pixel_count = (width * height) as usize;
		let pixels = vec![Pixel::<Depth8>::black(); pixel_count];

		Layer { width, height, pixels }
	}

	pub fn width(&self) -> u32 {
		self.width
	}

	pub fn height(&self) -> u32 {
		self.height
	}
}
