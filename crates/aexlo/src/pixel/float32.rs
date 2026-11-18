use super::*;

/// 32 bits per channel: `f32` channels, `1.0` being full intensity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Depth32;

impl sealed::Sealed for Depth32 {}

impl PixelDepth for Depth32 {
	type Depth = f32;
	const KIND: PixelDepthKind = PixelDepthKind::F32;

	fn max_value() -> Self::Depth {
		1.0
	}

	fn to_unit(value: f32) -> f32 {
		value
	}

	fn from_unit(value: f32) -> f32 {
		value
	}

	fn wrap(layer: Layer<Self>) -> AnyLayer {
		AnyLayer::F32(layer)
	}

	fn unwrap_ref(layer: &AnyLayer) -> Option<&Layer<Self>> {
		match layer {
			AnyLayer::F32(layer) => Some(layer),
			_ => None,
		}
	}

	fn unwrap_mut(layer: &mut AnyLayer) -> Option<&mut Layer<Self>> {
		match layer {
			AnyLayer::F32(layer) => Some(layer),
			_ => None,
		}
	}
}

impl From<crate::sys::PF_Pixel32> for Pixel<Depth32> {
	fn from(pixel_sys: crate::sys::PF_Pixel32) -> Self {
		Pixel {
			alpha: pixel_sys.alpha,
			red: pixel_sys.red,
			green: pixel_sys.green,
			blue: pixel_sys.blue,
		}
	}
}

/// Create a `Pixel<Depth32>` from a `[f32; 4]` in the native `[a, r, g, b]`
/// order (see [`Pixel::from_argb`]; for `[r, g, b, a]` use
/// [`Pixel::from_rgba`]).
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

/// Converts a `Pixel<Depth32>` into an `after_effects_sys::PF_Pixel32`.
impl From<Pixel<Depth32>> for crate::sys::PF_Pixel32 {
	fn from(val: Pixel<Depth32>) -> Self {
		crate::sys::PF_Pixel32 {
			alpha: val.alpha,
			red: val.red,
			green: val.green,
			blue: val.blue,
		}
	}
}

impl Pixel<Depth32> {
	/// Creates a `Pixel<Depth32>` from a pointer to a `PF_Pixel32` struct.
	/// # Safety
	/// `sys` must be valid for reads and point to an initialized `PF_Pixel32`.
	pub unsafe fn from_sys(sys: *const crate::sys::PF_Pixel32) -> Self {
		// SAFETY: The caller guarantees `sys` is valid for reads and initialized.
		unsafe { (*sys).into() }
	}
}
