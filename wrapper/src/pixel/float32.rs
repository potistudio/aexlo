use super::*;

pub struct Depth32;

impl PixelDepth for Depth32 {
	type Depth = f32;
}

impl From<after_effects_sys::PF_Pixel32> for Pixel<Depth32> {
	fn from(pixel_sys: after_effects_sys::PF_Pixel32) -> Self {
		Pixel {
			alpha: pixel_sys.alpha,
			red: pixel_sys.red,
			green: pixel_sys.green,
			blue: pixel_sys.blue,
		}
	}
}

impl From<[f32; 4]> for Pixel<Depth32> {
	fn from(buffer: [f32; 4]) -> Self {
		Pixel {
			alpha: buffer[0],
			red: buffer[1],
			green: buffer[2],
			blue: buffer[3],
		}
	}
}

impl Pixel<Depth32> {
	pub fn black() -> Self {
		Pixel {
			alpha: 1.0,
			red: 0.0,
			green: 0.0,
			blue: 0.0,
		}
	}
}
