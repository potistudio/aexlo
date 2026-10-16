use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Depth32;

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
	pub fn white() -> Self {
		Pixel {
			alpha: 1.0,
			red: 1.0,
			green: 1.0,
			blue: 1.0,
		}
	}
}
