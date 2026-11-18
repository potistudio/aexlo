use super::*;
use core::mem::{offset_of, size_of};

//==== Depth ==========================================
#[test]
fn depth_kind_bits_round_trip() {
	for kind in [PixelDepthKind::U8, PixelDepthKind::U16, PixelDepthKind::F32] {
		assert_eq!(PixelDepthKind::from_bits(kind.bits()), Some(kind));
	}
	assert_eq!(PixelDepthKind::from_bits(0), None);
	assert_eq!(PixelDepthKind::from_bits(24), None);
	assert_eq!(PixelDepthKind::U16.to_string(), "16 bpc");
}

#[test]
fn bytes_per_pixel_matches_the_pixel_size() {
	assert_eq!(PixelDepthKind::U8.bytes_per_pixel(), size_of::<Pixel<Depth8>>());
	assert_eq!(PixelDepthKind::U16.bytes_per_pixel(), size_of::<Pixel<Depth16>>());
	assert_eq!(PixelDepthKind::F32.bytes_per_pixel(), size_of::<Pixel<Depth32>>());
}

#[test]
fn full_intensity_is_each_depths_white() {
	assert_eq!(Depth8::max_value(), 255);
	assert_eq!(Depth16::max_value(), 32768);
	assert_eq!(Depth32::max_value(), 1.0);
	assert_eq!(Depth8::to_unit(Depth8::max_value()), 1.0);
	assert_eq!(Depth16::to_unit(Depth16::max_value()), 1.0);
	assert_eq!(Depth32::to_unit(Depth32::max_value()), 1.0);
}

#[test]
fn every_8_bit_value_survives_a_unit_round_trip() {
	for v in 0..=u8::MAX {
		assert_eq!(Depth8::from_unit(Depth8::to_unit(v)), v);
	}
}

#[test]
fn every_16_bit_value_survives_a_unit_round_trip() {
	for v in 0..=Depth16::max_value() {
		assert_eq!(Depth16::from_unit(Depth16::to_unit(v)), v);
	}
}

#[test]
fn integer_depths_clamp_and_round() {
	assert_eq!(Depth8::from_unit(-1.0), 0);
	assert_eq!(Depth8::from_unit(2.0), 255);
	assert_eq!(Depth8::from_unit(f32::NAN), 0);
	assert_eq!(Depth8::from_unit(0.5), 128);
	assert_eq!(Depth16::from_unit(-1.0), 0);
	assert_eq!(Depth16::from_unit(2.0), 32768);
	assert_eq!(Depth16::from_unit(f32::NAN), 0);
	assert_eq!(Depth16::from_unit(0.5), 16384);
}

#[test]
fn float_depth_passes_values_through_unclamped() {
	assert_eq!(Depth32::from_unit(2.5), 2.5);
	assert_eq!(Depth32::to_unit(-0.25), -0.25);
	assert!(Depth32::to_unit(f32::NAN).is_nan());
	assert_eq!(Depth32::to_unit(f32::INFINITY), f32::INFINITY);
}

//==== Pixel ==========================================
#[test]
fn pixels_are_bit_compatible_with_the_sys_pixels() {
	use crate::sys::{PF_Pixel8, PF_Pixel16, PF_Pixel32};

	assert_eq!(size_of::<Pixel<Depth8>>(), size_of::<PF_Pixel8>());
	assert_eq!(size_of::<Pixel<Depth16>>(), size_of::<PF_Pixel16>());
	assert_eq!(size_of::<Pixel<Depth32>>(), size_of::<PF_Pixel32>());

	assert_eq!(offset_of!(Pixel<Depth8>, alpha), offset_of!(PF_Pixel8, alpha));
	assert_eq!(offset_of!(Pixel<Depth8>, red), offset_of!(PF_Pixel8, red));
	assert_eq!(offset_of!(Pixel<Depth8>, green), offset_of!(PF_Pixel8, green));
	assert_eq!(offset_of!(Pixel<Depth8>, blue), offset_of!(PF_Pixel8, blue));

	assert_eq!(offset_of!(Pixel<Depth16>, alpha), offset_of!(PF_Pixel16, alpha));
	assert_eq!(offset_of!(Pixel<Depth16>, blue), offset_of!(PF_Pixel16, blue));

	assert_eq!(offset_of!(Pixel<Depth32>, alpha), offset_of!(PF_Pixel32, alpha));
	assert_eq!(offset_of!(Pixel<Depth32>, blue), offset_of!(PF_Pixel32, blue));
}

#[test]
fn blank_and_black() {
	assert_eq!(Pixel::<Depth8>::blank().to_argb(), [0, 0, 0, 0]);
	assert_eq!(Pixel::<Depth8>::black().to_argb(), [255, 0, 0, 0]);
	assert_eq!(Pixel::<Depth16>::black().to_argb(), [32768, 0, 0, 0]);
	assert_eq!(Pixel::<Depth32>::black().to_argb(), [1.0, 0.0, 0.0, 0.0]);
	assert_eq!(Pixel::<Depth8>::transparent(), Pixel::blank());
}

fn colours<T: PixelDepth>() -> [(&'static str, Pixel<T>, [bool; 3]); 7] {
	[
		("white", Pixel::white(), [true, true, true]),
		("red", Pixel::red(), [true, false, false]),
		("green", Pixel::green(), [false, true, false]),
		("blue", Pixel::blue(), [false, false, true]),
		("cyan", Pixel::cyan(), [false, true, true]),
		("magenta", Pixel::magenta(), [true, false, true]),
		("yellow", Pixel::yellow(), [true, true, false]),
	]
}

fn check_colours<T: PixelDepth>() {
	for (name, pixel, [r, g, b]) in colours::<T>() {
		let on = T::max_value();
		let off = T::Depth::default();
		assert_eq!(pixel.alpha, on, "{name} alpha at {}", T::KIND);
		assert_eq!(pixel.red, if r { on } else { off }, "{name} red at {}", T::KIND);
		assert_eq!(pixel.green, if g { on } else { off }, "{name} green at {}", T::KIND);
		assert_eq!(pixel.blue, if b { on } else { off }, "{name} blue at {}", T::KIND);
	}
}

#[test]
fn colours_are_full_intensity_at_every_depth() {
	check_colours::<Depth8>();
	check_colours::<Depth16>();
	check_colours::<Depth32>();
}

#[test]
#[allow(deprecated)]
fn purple_is_magenta() {
	assert_eq!(Pixel::<Depth8>::purple(), Pixel::magenta());
}

#[test]
fn argb_and_rgba_orders() {
	let p = Pixel::<Depth8>::from_argb([1, 2, 3, 4]);
	assert_eq!((p.alpha, p.red, p.green, p.blue), (1, 2, 3, 4));
	assert_eq!(p.to_argb(), [1, 2, 3, 4]);
	assert_eq!(p.to_rgba(), [2, 3, 4, 1]);
	assert_eq!(Pixel::<Depth8>::from_rgba([2, 3, 4, 1]), p);
	// `From<[_; 4]>` is the native ARGB order.
	assert_eq!(Pixel::<Depth8>::from([1, 2, 3, 4]), p);
	assert_eq!(Pixel::<Depth16>::from([1, 2, 3, 4]).to_argb(), [1, 2, 3, 4]);
	assert_eq!(
		Pixel::<Depth32>::from([1.0, 2.0, 3.0, 4.0]).to_argb(),
		[1.0, 2.0, 3.0, 4.0]
	);
}

#[test]
fn unit_conversion_is_rgba() {
	let p = Pixel::<Depth8>::from_rgba([255, 0, 0, 255]);
	assert_eq!(p.to_unit(), [1.0, 0.0, 0.0, 1.0]);
	assert_eq!(Pixel::<Depth8>::from_unit([1.0, 0.0, 0.0, 1.0]), p);
}

#[test]
fn sys_pixels_convert_both_ways() {
	use crate::sys::{PF_Pixel8, PF_Pixel16, PF_Pixel32};

	let p8 = Pixel::<Depth8>::from_argb([9, 8, 7, 6]);
	let s8: PF_Pixel8 = p8.into();
	assert_eq!((s8.alpha, s8.red, s8.green, s8.blue), (9, 8, 7, 6));
	assert_eq!(Pixel::<Depth8>::from(s8), p8);
	assert_eq!(unsafe { Pixel::<Depth8>::from_sys(&s8) }, p8);

	let p16 = Pixel::<Depth16>::from_argb([9, 8, 7, 6]);
	let s16: PF_Pixel16 = p16.into();
	assert_eq!((s16.alpha, s16.red, s16.green, s16.blue), (9, 8, 7, 6));
	assert_eq!(Pixel::<Depth16>::from(s16), p16);
	assert_eq!(unsafe { Pixel::<Depth16>::from_sys(&s16) }, p16);

	let p32 = Pixel::<Depth32>::from_argb([0.9, 0.8, 0.7, 0.6]);
	let s32: PF_Pixel32 = p32.into();
	assert_eq!((s32.alpha, s32.red, s32.green, s32.blue), (0.9, 0.8, 0.7, 0.6));
	assert_eq!(Pixel::<Depth32>::from(s32), p32);
	assert_eq!(unsafe { Pixel::<Depth32>::from_sys(&s32) }, p32);
}

//==== Layer: construction ============================
#[test]
fn new_checks_the_buffer_length() {
	assert!(Layer::<Depth8>::new(2, 3, vec![Pixel::blank(); 6]).is_ok());
	assert_eq!(
		Layer::<Depth8>::new(2, 3, vec![Pixel::blank(); 5]).unwrap_err(),
		LayerError::DimensionMismatch { expected: 6, actual: 5 }
	);
	assert!(Layer::<Depth8>::new(0, 0, vec![]).is_ok());
	assert!(Layer::<Depth8>::new(0, 7, vec![]).unwrap().is_empty());
}

#[test]
fn new_does_not_let_dimensions_overflow_into_a_matching_length() {
	// 65536 * 65536 wraps to 0 in u32: an empty buffer must not pass for it.
	assert_eq!(
		Layer::<Depth8>::new(65536, 65536, vec![]).unwrap_err(),
		LayerError::DimensionMismatch {
			expected: 65536 * 65536,
			actual: 0
		}
	);
}

#[test]
fn new_rejects_dimensions_the_host_cannot_describe() {
	// The row stride (in bytes) and the height are passed to plugins as i32.
	let too_wide = 1u32 << 29;
	assert_eq!(
		Layer::<Depth8>::new(too_wide, 1, vec![]).unwrap_err(),
		LayerError::TooLarge {
			width: too_wide,
			height: 1
		}
	);
	// A wider layer is fine at a depth whose stride still fits.
	assert!(Layer::<Depth8>::new(1 << 28, 0, vec![]).is_ok());
	assert_eq!(
		Layer::<Depth32>::new(1 << 28, 0, vec![]).unwrap_err(),
		LayerError::TooLarge {
			width: 1 << 28,
			height: 0
		}
	);
	assert_eq!(
		Layer::<Depth8>::new(0, 1 << 31, vec![]).unwrap_err(),
		LayerError::TooLarge {
			width: 0,
			height: 1 << 31
		}
	);
	assert_eq!(
		Layer::<Depth8>::new(u32::MAX, u32::MAX, vec![]).unwrap_err(),
		LayerError::TooLarge {
			width: u32::MAX,
			height: u32::MAX
		}
	);
}

#[test]
fn layer_errors_display() {
	assert_eq!(
		LayerError::DimensionMismatch { expected: 4, actual: 3 }.to_string(),
		"Pixel data length (3) does not match layer dimensions (4)."
	);
	assert_eq!(
		LayerError::TooLarge { width: 7, height: 9 }.to_string(),
		"Layer dimensions 7x9 are too large."
	);
}

#[test]
#[should_panic(expected = "too large")]
fn blank_panics_on_unrepresentable_dimensions() {
	let _ = Layer::<Depth8>::blank(1 << 29, 1);
}

#[test]
#[should_panic(expected = "too large")]
fn black_panics_on_unrepresentable_dimensions() {
	let _ = Layer::<Depth32>::black(1 << 28, 1);
}

#[test]
#[should_panic(expected = "too large")]
fn from_fn_panics_on_unrepresentable_dimensions() {
	let _ = Layer::<Depth8>::from_fn(1 << 29, 1, |_, _| Pixel::blank());
}

#[test]
#[should_panic(expected = "too large")]
fn filled_panics_on_unrepresentable_dimensions() {
	let _ = AnyLayer::filled(PixelDepthKind::U8, 1 << 29, 1, [0.0; 4]);
}

#[test]
fn blank_and_black_layers() {
	let blank = Layer::<Depth16>::blank(3, 2);
	assert_eq!((blank.width(), blank.height(), blank.len()), (3, 2, 6));
	assert!(blank.iter().all(|p| *p == Pixel::blank()));

	let black = Layer::<Depth16>::black(3, 2);
	assert!(black.iter().all(|p| *p == Pixel::black()));
}

#[test]
fn from_fn_is_row_major() {
	let layer = Layer::<Depth8>::from_fn(3, 2, |x, y| Pixel::from_argb([x as u8, y as u8, 0, 0]));
	assert_eq!(layer.get_linear(0).unwrap().to_argb(), [0, 0, 0, 0]);
	assert_eq!(layer.get_linear(2).unwrap().to_argb(), [2, 0, 0, 0]);
	assert_eq!(layer.get_linear(3).unwrap().to_argb(), [0, 1, 0, 0]);
	assert_eq!(layer.get_linear(5).unwrap().to_argb(), [2, 1, 0, 0]);
	assert_eq!(layer.get_linear(6), None);
}

//==== Layer: access ==================================
#[test]
fn get_is_bounds_checked() {
	let mut layer = Layer::<Depth8>::from_fn(3, 2, |x, y| Pixel::from_argb([x as u8, y as u8, 0, 0]));
	assert_eq!(layer.get(2, 1).unwrap().to_argb(), [2, 1, 0, 0]);
	assert!(layer.get(3, 0).is_none());
	assert!(layer.get(0, 2).is_none());
	assert!(layer.get(u32::MAX, u32::MAX).is_none());

	layer.get_mut(1, 1).unwrap().red = 99;
	assert_eq!(layer[(1, 1)].red, 99);
	assert!(layer.get_mut(3, 1).is_none());
	assert!(layer.get_mut(1, 2).is_none());
}

#[test]
fn index_reads_and_writes() {
	let mut layer = Layer::<Depth8>::blank(2, 2);
	layer[(1, 0)] = Pixel::red();
	assert_eq!(layer.pixels()[1], Pixel::red());
	assert_eq!(layer[(1, 0)], Pixel::red());
}

#[test]
#[should_panic(expected = "X coordinate out of bounds")]
fn index_panics_past_the_width() {
	let layer = Layer::<Depth8>::blank(2, 2);
	let _ = layer[(2, 0)];
}

#[test]
#[should_panic(expected = "Y coordinate out of bounds")]
fn index_mut_panics_past_the_height() {
	let mut layer = Layer::<Depth8>::blank(2, 2);
	layer[(0, 2)] = Pixel::red();
}

//==== Layer: raw bytes ===============================
#[test]
fn from_raw_reads_rgba_bytes() {
	let layer = Layer::<Depth8>::from_rgba8(vec![1, 2, 3, 4, 5, 6, 7, 8], 2, 1).unwrap();
	assert_eq!(layer[(0, 0)].to_rgba(), [1, 2, 3, 4]);
	assert_eq!(layer[(0, 0)].to_argb(), [4, 1, 2, 3]);
	assert_eq!(layer[(1, 0)].to_rgba(), [5, 6, 7, 8]);
}

#[test]
fn from_raw_checks_the_length_and_dimensions() {
	assert_eq!(
		Layer::<Depth8>::from_rgba8(vec![0; 7], 2, 1).unwrap_err(),
		LayerError::DimensionMismatch { expected: 8, actual: 7 }
	);
	assert!(matches!(
		Layer::<Depth8>::from_rgba8(vec![], 1 << 29, 1),
		Err(LayerError::TooLarge { .. })
	));
	// Would wrap to a matching length if multiplied in u32.
	assert!(matches!(
		Layer::<Depth8>::from_rgba8(vec![], 1 << 16, 1 << 14),
		Err(LayerError::DimensionMismatch { .. })
	));
}

#[test]
fn write_rgba_bytes_round_trips_from_raw() {
	let bytes: Vec<u8> = (0..24).collect();
	let layer = Layer::<Depth8>::from_rgba8(bytes.clone(), 3, 2).unwrap();
	let mut out = vec![0u8; 24];
	layer.write_rgba8(&mut out).unwrap();
	assert_eq!(out, bytes);

	assert_eq!(
		layer.write_rgba8(&mut [0u8; 23]).unwrap_err(),
		LayerError::DimensionMismatch {
			expected: 24,
			actual: 23
		}
	);
}

//==== Layer: conversion ==============================
#[test]
fn convert_goes_through_normalized_values() {
	let white8 = Layer::<Depth8>::from_fn(2, 2, |_, _| Pixel::white());
	let white16 = white8.convert::<Depth16>();
	assert!(white16.iter().all(|p| *p == Pixel::white()));
	let white32 = white8.convert::<Depth32>();
	assert!(white32.iter().all(|p| *p == Pixel::white()));
	assert_eq!(white32.convert::<Depth8>().pixels(), white8.pixels());
	assert_eq!((white16.width(), white16.height()), (2, 2));
}

#[test]
fn convert_8_to_16_and_back_is_lossless() {
	let layer = Layer::<Depth8>::from_fn(16, 16, |x, y| {
		Pixel::from_argb([(x * 16 + y) as u8, x as u8 * 3, y as u8 * 7, 255 - x as u8])
	});
	let back = layer.convert::<Depth16>().convert::<Depth8>();
	assert_eq!(back.pixels(), layer.pixels());
}

//==== Layer: the sys world ===========================
#[test]
fn as_sys_describes_the_buffer() {
	fn check<D: PixelDepth>(deep: bool) {
		let mut layer = Layer::<D>::blank(5, 3);
		let base = layer.pixels_mut().as_mut_ptr() as *mut _;
		let world = layer.as_sys();
		assert_eq!((world.width, world.height), (5, 3));
		assert_eq!(world.rowbytes, 5 * size_of::<Pixel<D>>() as i32);
		assert_eq!(world.data, base);
		assert_eq!(world.extent_hint.left, 0);
		assert_eq!(world.extent_hint.top, 0);
		assert_eq!((world.extent_hint.right, world.extent_hint.bottom), (5, 3));
		assert_eq!((world.pix_aspect_ratio.num, world.pix_aspect_ratio.den), (1, 1));
		let is_deep = world.world_flags & crate::sys::PF_WorldFlag_DEEP as crate::sys::PF_WorldFlags != 0;
		assert_eq!(is_deep, deep, "deep flag at {}", D::KIND);
	}
	check::<Depth8>(false);
	check::<Depth16>(true);
	check::<Depth32>(false);
}

//==== AnyLayer =======================================
#[test]
fn any_layer_reports_its_depth_and_size() {
	for kind in [PixelDepthKind::U8, PixelDepthKind::U16, PixelDepthKind::F32] {
		let layer = AnyLayer::filled(kind, 4, 3, [0.0, 0.0, 0.0, 1.0]);
		assert_eq!(layer.kind(), kind);
		assert_eq!((layer.width(), layer.height(), layer.len()), (4, 3, 12));
		assert!(!layer.is_empty());
	}
	assert!(AnyLayer::filled(PixelDepthKind::U8, 0, 5, [0.0; 4]).is_empty());
}

#[test]
fn any_layer_gets_only_the_matching_depth() {
	let mut layer = AnyLayer::new(Layer::<Depth16>::blank(2, 2));
	assert!(layer.get::<Depth16>().is_some());
	assert!(layer.get::<Depth8>().is_none());
	assert!(layer.get::<Depth32>().is_none());
	assert!(layer.get_mut::<Depth16>().is_some());
	assert!(layer.get_mut::<Depth8>().is_none());
	assert!(layer.get_mut::<Depth32>().is_none());
	assert!(matches!(
		AnyLayer::from(Layer::<Depth32>::blank(1, 1)),
		AnyLayer::F32(_)
	));
}

#[test]
fn any_layer_unit_rgba() {
	let mut layer = AnyLayer::filled(PixelDepthKind::U8, 2, 1, [1.0, 0.0, 0.0, 1.0]);
	assert_eq!(layer.unit_rgba(1), Some([1.0, 0.0, 0.0, 1.0]));
	assert_eq!(layer.unit_rgba(2), None);
	assert_eq!(layer.to_unit_rgba(), vec![[1.0, 0.0, 0.0, 1.0]; 2]);

	layer.set_from_unit_rgba([[0.0, 1.0, 0.0, 1.0]]);
	assert_eq!(layer.unit_rgba(0), Some([0.0, 1.0, 0.0, 1.0]));
	// Missing values leave the rest untouched.
	assert_eq!(layer.unit_rgba(1), Some([1.0, 0.0, 0.0, 1.0]));
	// Extra values are ignored.
	layer.set_from_unit_rgba(vec![[0.0, 0.0, 1.0, 1.0]; 5]);
	assert_eq!(layer.to_unit_rgba(), vec![[0.0, 0.0, 1.0, 1.0]; 2]);
}

#[test]
fn any_layer_converted_changes_the_depth_and_copies() {
	let layer = AnyLayer::filled(PixelDepthKind::U8, 2, 2, [1.0, 0.5, 0.0, 1.0]);
	for kind in [PixelDepthKind::U8, PixelDepthKind::U16, PixelDepthKind::F32] {
		let converted = layer.convert(kind);
		assert_eq!(converted.kind(), kind);
		assert_eq!((converted.width(), converted.height()), (2, 2));
		let [r, g, b, a] = converted.unit_rgba(0).unwrap();
		assert_eq!((r, b, a), (1.0, 0.0, 1.0));
		assert!((g - 0.5).abs() < 0.01, "green {g} at {kind}");
	}
	assert_ne!(layer.data_ptr(), layer.convert(PixelDepthKind::U8).data_ptr());
}

#[test]
fn any_layer_exposes_its_buffer() {
	let mut layer = AnyLayer::filled(PixelDepthKind::U16, 2, 2, [0.0; 4]);
	let read = layer.data_ptr();
	assert_eq!(layer.data_mut_ptr() as *const u8, read);
	assert_eq!(layer.as_sys().data as *const u8, read);
}

#[test]
fn write_rgba8_quantizes_deeper_layers() {
	let layer = AnyLayer::filled(PixelDepthKind::F32, 2, 1, [1.0, 0.5, -1.0, 2.0]);
	let mut out = [0u8; 8];
	layer.write_rgba8(&mut out).unwrap();
	assert_eq!(out, [255, 128, 0, 255, 255, 128, 0, 255]);

	let layer = AnyLayer::filled(PixelDepthKind::U16, 1, 1, [1.0, 0.0, 0.5, 1.0]);
	let mut out = [0u8; 4];
	layer.write_rgba8(&mut out).unwrap();
	assert_eq!(out, [255, 0, 128, 255]);
}

#[test]
fn write_rgba8_copies_8_bit_layers_verbatim_and_checks_the_length() {
	let layer = AnyLayer::new(Layer::<Depth8>::from_rgba8(vec![1, 2, 3, 4], 1, 1).unwrap());
	let mut out = [0u8; 4];
	layer.write_rgba8(&mut out).unwrap();
	assert_eq!(out, [1, 2, 3, 4]);

	for kind in [PixelDepthKind::U8, PixelDepthKind::U16, PixelDepthKind::F32] {
		let layer = AnyLayer::filled(kind, 2, 2, [0.0; 4]);
		assert_eq!(
			layer.write_rgba8(&mut [0u8; 15]).unwrap_err(),
			LayerError::DimensionMismatch {
				expected: 16,
				actual: 15
			},
			"{kind}"
		);
	}
}

#[test]
fn debug_output_does_not_dump_pixels_of_any_layer() {
	let layer = AnyLayer::filled(PixelDepthKind::U8, 2, 2, [0.0; 4]);
	let text = format!("{layer:?}");
	assert!(
		text.contains("U8") && text.contains("width") && text.contains("height"),
		"{text}"
	);
	assert!(format!("{:?}", Layer::<Depth8>::blank(1, 1)).contains("pixels"));
}

#[test]
fn layers_clone_independently() {
	let a = Layer::<Depth8>::blank(2, 2);
	let mut b = a.clone();
	b[(0, 0)] = Pixel::white();
	assert_eq!(a[(0, 0)], Pixel::blank());
	assert_eq!(b[(0, 0)], Pixel::white());
}
