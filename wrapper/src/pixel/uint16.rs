use super::*;

pub struct Depth16;

impl PixelDepth for Depth16 {
	type Depth = u16;
}

impl From<after_effects_sys::PF_Pixel16> for Pixel<Depth16> {
	fn from(pixel_sys: after_effects_sys::PF_Pixel16) -> Self {
		Pixel {
			alpha: pixel_sys.alpha,
			red: pixel_sys.red,
			green: pixel_sys.green,
			blue: pixel_sys.blue,
		}
	}
}

impl From<[u16; 4]> for Pixel<Depth16> {
	fn from(buffer: [u16; 4]) -> Self {
		Pixel {
			alpha: buffer[0],
			red: buffer[1],
			green: buffer[2],
			blue: buffer[3],
		}
	}
}

impl Pixel<Depth16> {
	pub fn black() -> Self {
		Pixel {
			alpha: 65535,
			red: 0,
			green: 0,
			blue: 0,
		}
	}
}
