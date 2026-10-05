//! "PF Pixel Format Suite": v1 is Premiere's `PF_PixelFormatSuite` (format
//! registration, world allocation and pixel conversion), v2 is AE's
//! `PF_PixelFormatSuite2` (format registration only).
//!
//! Registered formats are recorded on the [`PluginInstance`] (see
//! [`PluginInstance::supported_pixel_formats`]); `PF_PixelFormat` and
//! `PrPixelFormat` share one FourCC space, so both versions feed one list.
//!
//! Worlds and colors are supported for the packed 4-channel formats
//! (`BGRA`/`ARGB`/`VUYA` at `8u`/`16u`/`32f`). 16-bit channels use AE's
//! `0..=32768` range; `VUYA` uses BT.601, video range at `8u`/`16u`.

use super::world::{alloc_world, dispose_world_sys, world_pixel_format};
use crate::PluginInstance;
use crate::core::diagnostics::diag;
use after_effects_sys::*;
use std::os::raw::c_void;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Layout {
	Bgra,
	Argb,
	Vuya,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Depth {
	U8,
	U16,
	F32,
}

impl Depth {
	fn bytes(self) -> i32 {
		match self {
			Depth::U8 => 1,
			Depth::U16 => 2,
			Depth::F32 => 4,
		}
	}
}

/// Decode the packed 4-channel formats this suite can allocate and fill.
fn layout_of(format: PrPixelFormat) -> Option<(Layout, Depth)> {
	let table = [
		(PrPixelFormat_PrPixelFormat_BGRA_4444_8u, Layout::Bgra, Depth::U8),
		(PrPixelFormat_PrPixelFormat_BGRA_4444_16u, Layout::Bgra, Depth::U16),
		(PrPixelFormat_PrPixelFormat_BGRA_4444_32f, Layout::Bgra, Depth::F32),
		(PrPixelFormat_PrPixelFormat_ARGB_4444_8u, Layout::Argb, Depth::U8),
		(PrPixelFormat_PrPixelFormat_ARGB_4444_16u, Layout::Argb, Depth::U16),
		(PrPixelFormat_PrPixelFormat_ARGB_4444_32f, Layout::Argb, Depth::F32),
		(PrPixelFormat_PrPixelFormat_VUYA_4444_8u, Layout::Vuya, Depth::U8),
		(PrPixelFormat_PrPixelFormat_VUYA_4444_32f, Layout::Vuya, Depth::F32),
	];
	table
		.iter()
		.find(|(f, ..)| *f == format)
		.map(|&(_, layout, depth)| (layout, depth))
}

/// Encode straight `(a, r, g, b)` (nominally `0..=1`) as one pixel of `format`
/// into `out`. Returns `false` for unsupported formats.
///
/// # Safety
/// `out` must be writable for one pixel of `format`.
unsafe fn write_color(format: PrPixelFormat, [a, r, g, b]: [f32; 4], out: *mut c_void) -> bool {
	let Some((layout, depth)) = layout_of(format) else {
		return false;
	};
	// Channels in memory order, normalized so integer depths scale by their max.
	let channels: [f32; 4] = match layout {
		Layout::Bgra => [b, g, r, a],
		Layout::Argb => [a, r, g, b],
		Layout::Vuya => {
			let y = 0.299 * r + 0.587 * g + 0.114 * b;
			let (u, v) = (0.564 * (b - y), 0.713 * (r - y));
			match depth {
				Depth::F32 => [v, u, y, a],
				// Studio swing: luma 16..235, chroma 16..240 centered on 128 (of 255).
				_ => [
					(128.0 + 224.0 * v) / 255.0,
					(128.0 + 224.0 * u) / 255.0,
					(16.0 + 219.0 * y) / 255.0,
					a,
				],
			}
		}
	};
	unsafe {
		match depth {
			Depth::U8 => {
				let out = out as *mut u8;
				for (i, c) in channels.into_iter().enumerate() {
					*out.add(i) = (c * 255.0).round().clamp(0.0, 255.0) as u8;
				}
			}
			Depth::U16 => {
				let out = out as *mut u16;
				let max = PF_MAX_CHAN16 as f32;
				for (i, c) in channels.into_iter().enumerate() {
					*out.add(i) = (c * max).round().clamp(0.0, max) as u16;
				}
			}
			Depth::F32 => {
				let out = out as *mut f32;
				for (i, c) in channels.into_iter().enumerate() {
					*out.add(i) = c;
				}
			}
		}
	}
	true
}

/// Run `f` on the instance behind `effect_ref`.
fn with_instance(effect_ref: PF_ProgPtr, f: impl FnOnce(&mut PluginInstance)) -> PF_Err {
	let Some(mut instance) = PluginInstance::get_instance_ptr(effect_ref) else {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	};
	// SAFETY: effect_ref is the instance currently being called into; nothing
	// else touches its supported-format list during the callback.
	f(unsafe { instance.as_mut() });
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn add_supported_pixel_format(effect_ref: PF_ProgPtr, pixelFormat: PrPixelFormat) -> PF_Err {
	diag!("PF_PixelFormatSuite/AddSupportedPixelFormat",
		"effect_ref" => format!("{:#x}", effect_ref as usize),
		"pixelFormat" => format!("{:#x}", pixelFormat),
	);
	with_instance(effect_ref, |i| i.add_supported_pixel_format(pixelFormat))
}

unsafe extern "C" fn add_supported_pixel_format_2(effect_ref: PF_ProgPtr, pixel_format: PF_PixelFormat) -> PF_Err {
	unsafe { add_supported_pixel_format(effect_ref, pixel_format as PrPixelFormat) }
}

unsafe extern "C" fn clear_supported_pixel_formats(effect_ref: PF_ProgPtr) -> PF_Err {
	diag!("PF_PixelFormatSuite/ClearSupportedPixelFormats",
		"effect_ref" => format!("{:#x}", effect_ref as usize),
	);
	with_instance(effect_ref, PluginInstance::clear_supported_pixel_formats)
}

unsafe extern "C" fn new_world_of_pixel_format(
	effect_ref: PF_ProgPtr,
	width: A_u_long,
	height: A_u_long,
	flags: PF_NewWorldFlags,
	pixelFormat: PrPixelFormat,
	world: *mut PF_EffectWorld,
) -> PF_Err {
	diag!("PF_PixelFormatSuite/NewWorldOfPixelFormat",
		"effect_ref" => format!("{:#x}", effect_ref as usize),
		"width" => width,
		"height" => height,
		"flags" => flags,
		"pixelFormat" => format!("{:#x}", pixelFormat),
		"world" => format!("{:#x}", world as usize),
	);
	let _ = (effect_ref, flags); // Worlds are always allocated cleared.

	if world.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}
	let Some((_, depth)) = layout_of(pixelFormat) else {
		log::warn!("NewWorldOfPixelFormat: unsupported pixel format {pixelFormat:#x}");
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	};
	let clamp = |v: A_u_long| v.min(A_long::MAX as A_u_long) as A_long;
	unsafe { *world = alloc_world(clamp(width), clamp(height), 4 * depth.bytes(), pixelFormat) };
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn get_pixel_format(inWorld: *mut PF_EffectWorld, pixelFormat: *mut PrPixelFormat) -> PF_Err {
	diag!("PF_PixelFormatSuite/GetPixelFormat",
		"inWorld" => format!("{:#x}", inWorld as usize),
		"pixelFormat" => format!("{:#x}", pixelFormat as usize),
	);

	if inWorld.is_null() || pixelFormat.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}
	unsafe { *pixelFormat = world_pixel_format(inWorld) as PrPixelFormat };
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn convert_color_to_pixel_formatted_data(
	pixelFormat: PrPixelFormat,
	alpha: f32,
	red: f32,
	green: f32,
	blue: f32,
	pixelData: *mut c_void,
) -> PF_Err {
	diag!("PF_PixelFormatSuite/ConvertColorToPixelFormattedData",
		"pixelFormat" => format!("{:#x}", pixelFormat),
		"argb" => format!("{:?}", (alpha, red, green, blue)),
		"pixelData" => format!("{:#x}", pixelData as usize),
	);

	if pixelData.is_null() || !unsafe { write_color(pixelFormat, [alpha, red, green, blue], pixelData) } {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn get_black_for_pixel_format(pixelFormat: PrPixelFormat, pixelData: *mut c_void) -> PF_Err {
	unsafe { convert_color_to_pixel_formatted_data(pixelFormat, 1.0, 0.0, 0.0, 0.0, pixelData) }
}

unsafe extern "C" fn get_white_for_pixel_format(pixelFormat: PrPixelFormat, pixelData: *mut c_void) -> PF_Err {
	unsafe { convert_color_to_pixel_formatted_data(pixelFormat, 1.0, 1.0, 1.0, 1.0, pixelData) }
}

/// Build Premiere's `PF_PixelFormatSuite` table (wire version 1).
pub(super) const fn create_pixel_format_suite_1() -> PF_PixelFormatSuite {
	PF_PixelFormatSuite {
		AddSupportedPixelFormat: Some(add_supported_pixel_format),
		ClearSupportedPixelFormats: Some(clear_supported_pixel_formats),
		NewWorldOfPixelFormat: Some(new_world_of_pixel_format),
		DisposeWorld: Some(dispose_world_sys),
		GetPixelFormat: Some(get_pixel_format),
		GetBlackForPixelFormat: Some(get_black_for_pixel_format),
		GetWhiteForPixelFormat: Some(get_white_for_pixel_format),
		ConvertColorToPixelFormattedData: Some(convert_color_to_pixel_formatted_data),
	}
}

/// Build AE's `PF_PixelFormatSuite2` table (wire version 2).
pub(super) const fn create_pixel_format_suite_2() -> PF_PixelFormatSuite2 {
	PF_PixelFormatSuite2 {
		PF_AddSupportedPixelFormat: Some(add_supported_pixel_format_2),
		PF_ClearSupportedPixelFormats: Some(clear_supported_pixel_formats),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn black_white_and_colors_per_layout() {
		let mut px = [0u8; 4];
		let p = px.as_mut_ptr() as *mut c_void;
		unsafe {
			assert!(write_color(
				PrPixelFormat_PrPixelFormat_BGRA_4444_8u,
				[1.0, 1.0, 0.5, 0.0],
				p
			));
			assert_eq!(px, [0, 128, 255, 255]);
			assert!(write_color(
				PrPixelFormat_PrPixelFormat_ARGB_4444_8u,
				[1.0, 1.0, 0.5, 0.0],
				p
			));
			assert_eq!(px, [255, 255, 128, 0]);
			assert!(write_color(
				PrPixelFormat_PrPixelFormat_VUYA_4444_8u,
				[1.0, 0.0, 0.0, 0.0],
				p
			));
			assert_eq!(px, [128, 128, 16, 255]);
			assert!(write_color(
				PrPixelFormat_PrPixelFormat_VUYA_4444_8u,
				[1.0, 1.0, 1.0, 1.0],
				p
			));
			assert_eq!(px, [128, 128, 235, 255]);
			assert!(!write_color(PrPixelFormat_PrPixelFormat_Invalid, [1.0; 4], p));
		}

		let mut wide = [0u16; 4];
		let err = unsafe {
			get_white_for_pixel_format(
				PrPixelFormat_PrPixelFormat_BGRA_4444_16u,
				wide.as_mut_ptr() as *mut c_void,
			)
		};
		assert_eq!(err, PF_Err_NONE as PF_Err);
		assert_eq!(wide, [32768; 4]);
	}

	#[test]
	fn worlds_report_their_format() {
		let mut world: PF_EffectWorld = unsafe { std::mem::zeroed() };
		let fmt = PrPixelFormat_PrPixelFormat_BGRA_4444_32f;
		let err = unsafe { new_world_of_pixel_format(std::ptr::null_mut(), 3, 2, 0, fmt, &mut world) };
		assert_eq!(err, PF_Err_NONE as PF_Err);
		assert_eq!((world.width, world.height, world.rowbytes), (3, 2, 48));

		let mut out: PrPixelFormat = 0;
		unsafe { get_pixel_format(&mut world, &mut out) };
		assert_eq!(out, fmt);

		unsafe { dispose_world_sys(std::ptr::null_mut(), &mut world) };
		assert!(world.data.is_null());
	}
}
