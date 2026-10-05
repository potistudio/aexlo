use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Depth16;

/// Full intensity of an After Effects 16 bpc channel (`PF_MAX_CHAN16`): the
/// format is 15 bits plus one, so white is `0x8000`, not `0xFFFF`.
pub const MAX_CHAN16: u16 = 32768;

impl PixelDepth for Depth16 {
	type Depth = u16;
	const KIND: PixelDepthKind = PixelDepthKind::U16;

	fn max_value() -> Self::Depth {
		MAX_CHAN16
	}

	fn to_unit(value: u16) -> f32 {
		value as f32 / MAX_CHAN16 as f32
	}

	fn from_unit(value: f32) -> u16 {
		(value.clamp(0.0, 1.0) * MAX_CHAN16 as f32 + 0.5) as u16
	}

	fn wrap(layer: Layer<Self>) -> AnyLayer {
		AnyLayer::U16(layer)
	}

	fn unwrap_ref(layer: &AnyLayer) -> Option<&Layer<Self>> {
		match layer {
			AnyLayer::U16(layer) => Some(layer),
			_ => None,
		}
	}

	fn unwrap_mut(layer: &mut AnyLayer) -> Option<&mut Layer<Self>> {
		match layer {
			AnyLayer::U16(layer) => Some(layer),
			_ => None,
		}
	}
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
	pub fn white() -> Self {
		Pixel {
			alpha: MAX_CHAN16,
			red: MAX_CHAN16,
			green: MAX_CHAN16,
			blue: MAX_CHAN16,
		}
	}
}
