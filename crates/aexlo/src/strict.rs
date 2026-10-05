//! Strict mode: host-side instrumentation that reports plugin bugs After
//! Effects would hide (see [`PluginInstance::set_strict`]).
//!
//! The host only detects and records; whether a finding fails anything is
//! up to the caller.
//!
//! [`PluginInstance::set_strict`]: crate::PluginInstance::set_strict

use wrapper::{AnyLayer, Layer, PixelDepth};

/// Which strict-mode instrumentation an instance runs. All off by default,
/// in which case the host's allocation paths and costs are unchanged.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Strict {
	/// Fill output worlds with a sentinel before each render, so pixels the
	/// plugin never wrote can be found afterwards ([`unwritten_pixels`]).
	pub poison_output: bool,
	/// Surround host-allocated worlds with guard bands, verified on dispose.
	pub guard_bands: bool,
	/// Count handle and world allocations against disposals.
	pub track_allocations: bool,
	/// Record checkout/checkin of layers and params in smart render.
	pub track_checkouts: bool,
}

impl Strict {
	/// Every feature on.
	pub fn all() -> Self {
		Self {
			poison_output: true,
			guard_bands: true,
			track_allocations: true,
			track_checkouts: true,
		}
	}

	/// Whether any feature is on.
	pub fn any(&self) -> bool {
		self.poison_output || self.guard_bands || self.track_allocations || self.track_checkouts
	}
}

/// The 8 and 16 bpc output poison: every byte of the pixel is `0xCD`.
pub const POISON_BYTE: u8 = 0xCD;

/// The 32 bpc output poison: a signalling NaN with a recognizable payload.
pub const POISON_F32_BITS: u32 = 0x7FA0_CDCD;

/// The poison value of one channel at depth `D`.
fn poison_channel<D: PixelDepth>() -> D::Depth {
	let mut value = D::Depth::default();
	// SAFETY: `D::Depth` is `u8`, `u16` or `f32`; every bit pattern is valid.
	unsafe {
		let bytes = std::slice::from_raw_parts_mut(&mut value as *mut D::Depth as *mut u8, size_of::<D::Depth>());
		if size_of::<D::Depth>() == 4 {
			bytes.copy_from_slice(&POISON_F32_BITS.to_ne_bytes());
		} else {
			bytes.fill(POISON_BYTE);
		}
	}
	value
}

/// Whether `value` is the poison channel at depth `D`, bit for bit.
fn is_poison_channel<D: PixelDepth>(value: &D::Depth) -> bool {
	let poison = poison_channel::<D>();
	// SAFETY: both are plain `size_of::<D::Depth>()`-byte values.
	unsafe {
		std::slice::from_raw_parts(value as *const D::Depth as *const u8, size_of::<D::Depth>())
			== std::slice::from_raw_parts(&poison as *const D::Depth as *const u8, size_of::<D::Depth>())
	}
}

/// Fill every channel of `layer` with the poison.
pub(crate) fn poison(layer: &mut AnyLayer) {
	fn fill<D: PixelDepth>(layer: &mut Layer<D>) {
		let value = poison_channel::<D>();
		for pixel in layer.iter_mut() {
			pixel.alpha = value;
			pixel.red = value;
			pixel.green = value;
			pixel.blue = value;
		}
	}
	match layer {
		AnyLayer::U8(l) => fill(l),
		AnyLayer::U16(l) => fill(l),
		AnyLayer::F32(l) => fill(l),
	}
}

/// Whether the pixel at linear `index` still holds the output poison.
///
/// At 8 bpc all four channels must be poisoned (`0xCD` is a legal value);
/// at 16 bpc (`0xCDCD` is above AE's 32768) and 32 bpc (a signalling NaN)
/// any one channel is enough.
pub fn is_unwritten(layer: &AnyLayer, index: usize) -> bool {
	fn check<D: PixelDepth>(layer: &Layer<D>, index: usize, all: bool) -> bool {
		let Some(p) = layer.get_linear(index) else {
			return false;
		};
		let channels = [&p.alpha, &p.red, &p.green, &p.blue];
		if all {
			channels.iter().all(|c| is_poison_channel::<D>(c))
		} else {
			channels.iter().any(|c| is_poison_channel::<D>(c))
		}
	}
	match layer {
		AnyLayer::U8(l) => check(l, index, true),
		AnyLayer::U16(l) => check(l, index, false),
		AnyLayer::F32(l) => check(l, index, false),
	}
}

/// How many pixels of a poisoned output the plugin left unwritten.
pub fn unwritten_pixels(layer: &AnyLayer) -> usize {
	let count = layer.width() as usize * layer.height() as usize;
	(0..count).filter(|&i| is_unwritten(layer, i)).count()
}

/// A guard band around a host world was overwritten.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardViolation {
	/// What the world was (`"PF_NewWorld 64x48"`, `"output"`, ...).
	pub world: String,
	/// Where: `"above"`, `"below"` or `"row tail"`.
	pub band: &'static str,
	/// Bytes found changed.
	pub bytes: usize,
}

/// A handle or world still alive when its scope ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Leak {
	pub kind: crate::AllocationKind,
	pub address: usize,
	pub bytes: usize,
	/// The command during which it was allocated.
	pub allocated_in: &'static str,
	/// Where it should have been released by: `"SEQUENCE_SETDOWN"`, ...
	pub scope: &'static str,
}

/// A smart-render checkout rule broken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckoutViolation {
	pub message: String,
}

/// What strict mode found since the last [`PluginInstance::strict_report`].
///
/// [`PluginInstance::strict_report`]: crate::PluginInstance::strict_report
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StrictReport {
	pub guard_violations: Vec<GuardViolation>,
	pub leaks: Vec<Leak>,
	pub checkout_violations: Vec<CheckoutViolation>,
}

impl StrictReport {
	pub fn is_empty(&self) -> bool {
		self.guard_violations.is_empty() && self.leaks.is_empty() && self.checkout_violations.is_empty()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use wrapper::PixelDepthKind;

	#[test]
	fn poison_is_found_at_every_depth_and_only_where_left() {
		for kind in [PixelDepthKind::U8, PixelDepthKind::U16, PixelDepthKind::F32] {
			let mut layer = AnyLayer::filled(kind, 4, 2, [0.0; 4]);
			poison(&mut layer);
			assert_eq!(unwritten_pixels(&layer), 8, "{kind}");
			fn write<D: PixelDepth>(layer: &mut AnyLayer) {
				let layer = layer.get_mut::<D>().unwrap();
				for pixel in layer.iter_mut().take(3) {
					*pixel = wrapper::Pixel::from_unit([0.2, 0.4, 0.6, 1.0]);
				}
			}
			match kind {
				PixelDepthKind::U8 => write::<wrapper::Depth8>(&mut layer),
				PixelDepthKind::U16 => write::<wrapper::Depth16>(&mut layer),
				PixelDepthKind::F32 => write::<wrapper::Depth32>(&mut layer),
			}
			assert_eq!(unwritten_pixels(&layer), 5, "{kind}");
		}
	}

	#[test]
	fn float_poison_is_a_signalling_nan() {
		let value = f32::from_bits(POISON_F32_BITS);
		assert!(value.is_nan());
		assert_eq!(POISON_F32_BITS & 0x0040_0000, 0, "quiet bit clear");
	}
}
