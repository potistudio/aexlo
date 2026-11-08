use super::*;

/// 8 bits per channel: `u8` channels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Depth8;

impl sealed::Sealed for Depth8 {}

impl PixelDepth for Depth8 {
	type Depth = u8;
	const KIND: PixelDepthKind = PixelDepthKind::U8;

	fn max_value() -> Self::Depth {
		u8::MAX
	}

	fn to_unit(value: u8) -> f32 {
		value as f32 / 255.0
	}

	fn from_unit(value: f32) -> u8 {
		(value.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
	}

	fn wrap(layer: Layer<Self>) -> AnyLayer {
		AnyLayer::U8(layer)
	}

	fn unwrap_ref(layer: &AnyLayer) -> Option<&Layer<Self>> {
		match layer {
			AnyLayer::U8(layer) => Some(layer),
			_ => None,
		}
	}

	fn unwrap_mut(layer: &mut AnyLayer) -> Option<&mut Layer<Self>> {
		match layer {
			AnyLayer::U8(layer) => Some(layer),
			_ => None,
		}
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

/// Create a `Pixel<Depth8>` from a `[u8; 4]` in the native `[a, r, g, b]`
/// order (see [`Pixel::from_argb`]; for `[r, g, b, a]` use
/// [`Pixel::from_rgba`]).
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
	/// Creates a `Pixel<Depth8>` from a pointer to a `PF_Pixel8` struct.
	/// # Safety
	/// `sys` must be valid for reads and point to an initialized `PF_Pixel8`.
	pub unsafe fn from_sys(sys: *const after_effects_sys::PF_Pixel8) -> Self {
		// SAFETY: The caller guarantees `sys` is valid for reads and initialized.
		unsafe { (*sys).into() }
	}
}
