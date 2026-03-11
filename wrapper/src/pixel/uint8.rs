use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Depth8;

impl PixelDepth for Depth8 {
	type Depth = u8;
	fn max_value() -> Self::Depth {
		u8::MAX
	}
}

/// Create a `Pixel<Depth8>` from an `after_effects_sys::PF_Pixel8`.
impl From<after_effects_sys::PF_Pixel8> for Pixel<Depth8> {
	fn from(pixel_sys: after_effects_sys::PF_Pixel8) -> Self {
		Pixel {
			alpha: pixel_sys.alpha,
			red: pixel_sys.red,
			green: pixel_sys.green,
			blue: pixel_sys.blue,
		}
	}
}

/// Create a `Pixel<Depth8>` from a `[u8; 4]` buffer.
impl From<[u8; 4]> for Pixel<Depth8> {
	fn from(buffer: [u8; 4]) -> Self {
		Pixel {
			alpha: buffer[0],
			red: buffer[1],
			green: buffer[2],
			blue: buffer[3],
		}
	}
}

/// Converts a `Pixel<Depth8>` into an `after_effects_sys::PF_Pixel8`.
impl From<Pixel<Depth8>> for after_effects_sys::PF_Pixel8 {
	fn from(val: Pixel<Depth8>) -> Self {
		after_effects_sys::PF_Pixel8 {
			alpha: val.alpha,
			red: val.red,
			green: val.green,
			blue: val.blue,
		}
	}
}

impl Pixel<Depth8> {
	pub fn blue() -> Self {
		Pixel {
			alpha: 255,
			red: 0,
			green: 0,
			blue: 255,
		}
	}

	pub fn cyan() -> Self {
		Pixel {
			alpha: 255,
			red: 0,
			green: 255,
			blue: 255,
		}
	}

	pub fn green() -> Self {
		Pixel {
			alpha: 255,
			red: 0,
			green: 255,
			blue: 0,
		}
	}

	pub fn purple() -> Self {
		Pixel {
			alpha: 255,
			red: 255,
			green: 0,
			blue: 255,
		}
	}

	pub fn random() -> Self {
		use rand::Rng;
		let mut rng = rand::rng();

		Pixel {
			alpha: 255,
			red: rng.random(),
			green: rng.random(),
			blue: rng.random(),
		}
	}

	pub fn red() -> Self {
		Pixel {
			alpha: 255,
			red: 255,
			green: 0,
			blue: 0,
		}
	}

	pub fn transparent() -> Self {
		Pixel {
			alpha: 0,
			red: 0,
			green: 0,
			blue: 0,
		}
	}

	pub fn white() -> Self {
		Pixel {
			alpha: 255,
			red: 255,
			green: 255,
			blue: 255,
		}
	}

	pub fn yellow() -> Self {
		Pixel {
			alpha: 255,
			red: 255,
			green: 255,
			blue: 0,
		}
	}

	/// Creates a `Pixel<Depth8>` from a pointer to an `PF_Pixel8` struct.
	/// # Safety
	/// This function dereferences a raw pointer.
	pub unsafe fn from_sys(sys: *mut after_effects_sys::PF_Pixel8) -> Self {
		unsafe { (*sys).into() }
	}
}
