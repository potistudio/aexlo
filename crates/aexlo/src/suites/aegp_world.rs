//! `AEGP World Suite` (wire versions 1, 2 and 3).
//!
//! Worlds allocated here are independent of the effect's input and output.
//! `FastBlur` is a separable box blur: high quality is three passes, anything
//! else is one, and the mode flags are ignored. Platform worlds are refused.

use after_effects_sys::{
	A_Err, A_FpLong, A_long, A_u_long, AEGP_PlatformWorldH, AEGP_PluginID, AEGP_WorldH, AEGP_WorldSuite1,
	AEGP_WorldSuite2, AEGP_WorldSuite3, AEGP_WorldType, AEGP_WorldType_8, AEGP_WorldType_16, AEGP_WorldType_32,
	PF_EffectWorld, PF_Err_BAD_CALLBACK_PARAM, PF_Err_NONE, PF_ModeFlags, PF_Pixel8, PF_Pixel16, PF_PixelFloat,
	PF_Quality, PF_Quality_HI, PF_RationalScale, PF_UnionableRect, PF_WorldFlag_DEEP, PF_WorldFlag_WRITEABLE,
	PF_WorldFlags,
};

struct World {
	kind: i32,
	width: i32,
	height: i32,
	row_bytes: i32,
	pixels: Vec<u8>,
}

fn bytes_per_pixel(kind: i32) -> usize {
	if kind == AEGP_WorldType_16 as i32 {
		8
	} else if kind == AEGP_WorldType_32 as i32 {
		16
	} else {
		4
	}
}

fn alloc(kind: i32, width: i32, height: i32) -> AEGP_WorldH {
	let row_bytes = width.saturating_mul(bytes_per_pixel(kind) as i32);
	let pixels = vec![0u8; row_bytes as usize * height.max(0) as usize];
	let world = Box::into_raw(Box::new(World {
		kind,
		width,
		height,
		row_bytes,
		pixels,
	}));
	Box::into_raw(Box::new(world)) as AEGP_WorldH
}

/// A world filled from `layer`, subsampled by `down_x`/`down_y` (values below 1
/// mean no subsample). Channel order is ARGB, matching `PF_Pixel`.
fn depth_kind(kind: i32) -> crate::pixel::PixelDepthKind {
	if kind == AEGP_WorldType_16 as i32 {
		crate::pixel::PixelDepthKind::U16
	} else if kind == AEGP_WorldType_32 as i32 {
		crate::pixel::PixelDepthKind::F32
	} else {
		crate::pixel::PixelDepthKind::U8
	}
}

pub(crate) fn world_from_layer(kind: i32, layer: &crate::pixel::AnyLayer, down_x: i32, down_y: i32) -> AEGP_WorldH {
	let down_x = down_x.max(1) as usize;
	let down_y = down_y.max(1) as usize;
	let width = (layer.width() as usize / down_x).max(1);
	let height = (layer.height() as usize / down_y).max(1);
	let kind = if kind == AEGP_WorldType_16 as i32 || kind == AEGP_WorldType_32 as i32 {
		kind
	} else {
		AEGP_WorldType_8 as i32
	};
	let handle = alloc(kind, width as i32, height as i32);
	let units = layer.to_unit_rgba();
	let src_w = layer.width() as usize;
	let Some(world) = (unsafe { world_mut(handle) }) else {
		return handle;
	};
	let bpp = bytes_per_pixel(kind);
	for y in 0..height {
		for x in 0..width {
			let src =
				(y * down_y).min(layer.height() as usize - 1) * src_w + (x * down_x).min(layer.width() as usize - 1);
			let [red, green, blue, alpha] = units[src];
			let at = y * world.row_bytes as usize + x * bpp;
			write_pixel(&mut world.pixels[at..at + bpp], kind, alpha, red, green, blue);
		}
	}
	handle
}

fn write_pixel(dest: &mut [u8], kind: i32, alpha: f32, red: f32, green: f32, blue: f32) {
	let store = |channel: f32, max: f64| (channel as f64 * max).round().clamp(0.0, max);
	if kind == AEGP_WorldType_32 as i32 {
		for (offset, channel) in [alpha, red, green, blue].into_iter().enumerate() {
			dest[offset * 4..offset * 4 + 4].copy_from_slice(&channel.to_le_bytes());
		}
	} else if kind == AEGP_WorldType_16 as i32 {
		for (offset, channel) in [alpha, red, green, blue].into_iter().enumerate() {
			let bytes = (store(channel, 32768.0) as u16).to_le_bytes();
			dest[offset * 2..offset * 2 + 2].copy_from_slice(&bytes);
		}
	} else {
		for (offset, channel) in [alpha, red, green, blue].into_iter().enumerate() {
			dest[offset] = store(channel, 255.0) as u8;
		}
	}
}

pub(crate) unsafe fn dispose(handle: AEGP_WorldH) {
	unsafe { dispose_world(handle) };
}

/// # Safety
/// `handle` is a world allocated by [`alloc`], or null.
unsafe fn world_mut<'a>(handle: AEGP_WorldH) -> Option<&'a mut World> {
	if handle.is_null() {
		return None;
	}
	let inner = unsafe { *(handle as *mut *mut World) };
	if inner.is_null() {
		return None;
	}
	Some(unsafe { &mut *inner })
}

unsafe extern "C" fn new_world(
	_plugin_id: AEGP_PluginID,
	kind: AEGP_WorldType,
	width: A_long,
	height: A_long,
	world_out: *mut AEGP_WorldH,
) -> A_Err {
	crate::core::ffi::guard("new_world", || {
		if world_out.is_null() || width <= 0 || height <= 0 {
			return PF_Err_BAD_CALLBACK_PARAM as A_Err;
		}
		let kind = if kind == AEGP_WorldType_16 as i32 || kind == AEGP_WorldType_32 as i32 {
			kind
		} else {
			AEGP_WorldType_8 as i32
		};
		unsafe { *world_out = alloc(kind, width, height) };
		PF_Err_NONE as A_Err
	})
}

unsafe extern "C" fn dispose_world(handle: AEGP_WorldH) -> A_Err {
	crate::core::ffi::guard("dispose_world", || {
		if handle.is_null() {
			return PF_Err_BAD_CALLBACK_PARAM as A_Err;
		}
		let slot = unsafe { Box::from_raw(handle as *mut *mut World) };
		if !slot.is_null() {
			let world = unsafe { Box::from_raw(*slot) };
			super::world::untag_host_world(world.pixels.as_ptr() as usize);
		}
		PF_Err_NONE as A_Err
	})
}

unsafe extern "C" fn get_type(handle: AEGP_WorldH, out: *mut AEGP_WorldType) -> A_Err {
	crate::core::ffi::guard("get_type", || {
		let (Some(world), Some(out)) = (unsafe { world_mut(handle) }, unsafe { out.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM as A_Err;
		};
		*out = world.kind;
		PF_Err_NONE as A_Err
	})
}

unsafe extern "C" fn get_size(handle: AEGP_WorldH, width: *mut A_long, height: *mut A_long) -> A_Err {
	crate::core::ffi::guard("get_size", || {
		let Some(world) = (unsafe { world_mut(handle) }) else {
			return PF_Err_BAD_CALLBACK_PARAM as A_Err;
		};
		if let Some(width) = unsafe { width.as_mut() } {
			*width = world.width;
		}
		if let Some(height) = unsafe { height.as_mut() } {
			*height = world.height;
		}
		PF_Err_NONE as A_Err
	})
}

unsafe extern "C" fn get_row_bytes(handle: AEGP_WorldH, row_bytes: *mut A_u_long) -> A_Err {
	crate::core::ffi::guard("get_row_bytes", || {
		let (Some(world), Some(row_bytes)) = (unsafe { world_mut(handle) }, unsafe { row_bytes.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM as A_Err;
		};
		*row_bytes = world.row_bytes as A_u_long;
		PF_Err_NONE as A_Err
	})
}

unsafe extern "C" fn base_addr<T>(handle: AEGP_WorldH, expected: i32, out: *mut *mut T) -> A_Err {
	crate::core::ffi::guard("base_addr", || {
		let (Some(world), Some(out)) = (unsafe { world_mut(handle) }, unsafe { out.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM as A_Err;
		};
		if world.kind != expected {
			return PF_Err_BAD_CALLBACK_PARAM as A_Err;
		}
		*out = world.pixels.as_mut_ptr().cast();
		PF_Err_NONE as A_Err
	})
}

unsafe extern "C" fn get_base8(handle: AEGP_WorldH, out: *mut *mut PF_Pixel8) -> A_Err {
	crate::core::ffi::guard("get_base8", || unsafe {
		base_addr(handle, AEGP_WorldType_8 as i32, out)
	})
}

unsafe extern "C" fn get_base16(handle: AEGP_WorldH, out: *mut *mut PF_Pixel16) -> A_Err {
	crate::core::ffi::guard("get_base16", || unsafe {
		base_addr(handle, AEGP_WorldType_16 as i32, out)
	})
}

unsafe extern "C" fn get_base32(handle: AEGP_WorldH, out: *mut *mut PF_PixelFloat) -> A_Err {
	crate::core::ffi::guard("get_base32", || unsafe {
		base_addr(handle, AEGP_WorldType_32 as i32, out)
	})
}

unsafe extern "C" fn fill_out(handle: AEGP_WorldH, out: *mut PF_EffectWorld) -> A_Err {
	crate::core::ffi::guard("fill_out", || {
		let (Some(world), Some(out)) = (unsafe { world_mut(handle) }, unsafe { out.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM as A_Err;
		};
		let mut flags = PF_WorldFlag_WRITEABLE as PF_WorldFlags;
		if world.kind == AEGP_WorldType_16 as i32 {
			flags |= PF_WorldFlag_DEEP as PF_WorldFlags;
		}
		// `PF_GetPixelFormat` looks worlds up by `data`; without a tag a deep
		// world reads back as `ARGB32` and the plugin iterates it as 8 bpc.
		super::world::tag_host_world(world.pixels.as_ptr() as usize, depth_kind(world.kind));
		*out = PF_EffectWorld {
			reserved0: std::ptr::null_mut(),
			reserved1: std::ptr::null_mut(),
			world_flags: flags,
			data: world.pixels.as_mut_ptr().cast(),
			rowbytes: world.row_bytes,
			width: world.width,
			height: world.height,
			extent_hint: PF_UnionableRect {
				left: 0,
				top: 0,
				right: world.width,
				bottom: world.height,
			},
			platform_ref: std::ptr::null_mut(),
			reserved_long1: 0,
			reserved_long4: std::ptr::null_mut(),
			pix_aspect_ratio: PF_RationalScale { num: 1, den: 1 },
			reserved_long2: std::ptr::null_mut(),
			origin_x: 0,
			origin_y: 0,
			reserved_long3: 0,
			dephault: 0,
		};
		PF_Err_NONE as A_Err
	})
}

/// Separable box blur. `passes` of 3 approximates the Gaussian that Edge
/// Softness asks for at high quality.
fn box_blur(samples: &mut [f64], width: usize, height: usize, radius: usize, passes: usize) {
	if radius == 0 || width == 0 || height == 0 {
		return;
	}
	let mut scratch = vec![0.0; samples.len()];
	let window = (radius * 2 + 1) as f64;
	let last_x = width as isize - 1;
	let last_y = height as isize - 1;
	for _ in 0..passes {
		for y in 0..height {
			let row = y * width;
			let mut acc = 0.0;
			for k in 0..radius * 2 + 1 {
				let x = (k as isize - radius as isize).clamp(0, last_x) as usize;
				acc += samples[row + x];
			}
			for x in 0..width {
				scratch[row + x] = acc / window;
				if x + 1 == width {
					break;
				}
				let remove = (x as isize - radius as isize).clamp(0, last_x) as usize;
				let add = (x as isize + radius as isize + 1).clamp(0, last_x) as usize;
				acc += samples[row + add] - samples[row + remove];
			}
		}
		for x in 0..width {
			let mut acc = 0.0;
			for k in 0..radius * 2 + 1 {
				let y = (k as isize - radius as isize).clamp(0, last_y) as usize;
				acc += scratch[y * width + x];
			}
			for y in 0..height {
				samples[y * width + x] = acc / window;
				if y + 1 == height {
					break;
				}
				let remove = (y as isize - radius as isize).clamp(0, last_y) as usize;
				let add = (y as isize + radius as isize + 1).clamp(0, last_y) as usize;
				acc += scratch[add * width + x] - scratch[remove * width + x];
			}
		}
		if passes > 1 {
			scratch.copy_from_slice(samples);
		}
	}
}

fn fast_blur(world: &mut World, radius: f64, quality: PF_Quality) {
	let radius = radius.round().max(0.0) as usize;
	if radius == 0 {
		return;
	}
	let passes = if quality == PF_Quality_HI as PF_Quality { 3 } else { 1 };
	let width = world.width as usize;
	let height = world.height as usize;
	let bpp = bytes_per_pixel(world.kind);
	let channels = 4;
	let sample = bpp / channels;
	let mut plane = vec![0.0; width * height];
	for channel in 0..channels {
		for y in 0..height {
			let row = y * world.row_bytes as usize;
			for x in 0..width {
				let at = row + x * bpp + channel * sample;
				plane[y * width + x] = match sample {
					1 => world.pixels[at] as f64,
					2 => u16::from_le_bytes(world.pixels[at..at + 2].try_into().unwrap()) as f64,
					_ => f32::from_le_bytes(world.pixels[at..at + 4].try_into().unwrap()) as f64,
				};
			}
		}
		box_blur(&mut plane, width, height, radius, passes);
		for y in 0..height {
			let row = y * world.row_bytes as usize;
			for x in 0..width {
				let at = row + x * bpp + channel * sample;
				let value = plane[y * width + x];
				match sample {
					1 => world.pixels[at] = value.round().clamp(0.0, 255.0) as u8,
					2 => {
						let bytes = (value.round().clamp(0.0, 32768.0) as u16).to_le_bytes();
						world.pixels[at..at + 2].copy_from_slice(&bytes);
					}
					_ => {
						let bytes = (value as f32).to_le_bytes();
						world.pixels[at..at + 4].copy_from_slice(&bytes);
					}
				}
			}
		}
	}
}

unsafe extern "C" fn fast_blur_sys(
	radius: A_FpLong,
	_mode: PF_ModeFlags,
	quality: PF_Quality,
	handle: AEGP_WorldH,
) -> A_Err {
	crate::core::ffi::guard("fast_blur_sys", || {
		let Some(world) = (unsafe { world_mut(handle) }) else {
			return PF_Err_BAD_CALLBACK_PARAM as A_Err;
		};
		fast_blur(world, radius, quality);
		PF_Err_NONE as A_Err
	})
}

unsafe extern "C" fn unsupported_platform() -> A_Err {
	crate::core::ffi::guard("unsupported_platform", || PF_Err_BAD_CALLBACK_PARAM as A_Err)
}

unsafe extern "C" fn new_platform(
	_plugin_id: AEGP_PluginID,
	_kind: AEGP_WorldType,
	_width: A_long,
	_height: A_long,
	_out: *mut AEGP_PlatformWorldH,
) -> A_Err {
	crate::core::ffi::guard("new_platform", || unsafe { unsupported_platform() })
}

unsafe extern "C" fn dispose_platform(_handle: AEGP_PlatformWorldH) -> A_Err {
	crate::core::ffi::guard("dispose_platform", || unsafe { unsupported_platform() })
}

unsafe extern "C" fn reference_from_platform(
	_plugin_id: AEGP_PluginID,
	_platform: AEGP_PlatformWorldH,
	_out: *mut AEGP_WorldH,
) -> A_Err {
	crate::core::ffi::guard("reference_from_platform", || unsafe { unsupported_platform() })
}

pub const fn suite_1() -> AEGP_WorldSuite1 {
	AEGP_WorldSuite1 {
		AEGP_New: Some(new_world),
		AEGP_Dispose: Some(dispose_world),
		AEGP_GetType: Some(get_type),
		AEGP_GetSize: Some(get_size),
		AEGP_GetRowBytes: Some(get_row_bytes),
		AEGP_GetBaseAddr8: Some(get_base8),
		AEGP_GetBaseAddr16: Some(get_base16),
		AEGP_FillOutPFEffectWorld: Some(fill_out),
		AEGP_FastBlur: Some(fast_blur_sys),
	}
}

pub const fn suite_2() -> AEGP_WorldSuite2 {
	AEGP_WorldSuite2 {
		AEGP_New: Some(new_world),
		AEGP_Dispose: Some(dispose_world),
		AEGP_GetType: Some(get_type),
		AEGP_GetSize: Some(get_size),
		AEGP_GetRowBytes: Some(get_row_bytes),
		AEGP_GetBaseAddr8: Some(get_base8),
		AEGP_GetBaseAddr16: Some(get_base16),
		AEGP_FillOutPFEffectWorld: Some(fill_out),
		AEGP_FastBlur: Some(fast_blur_sys),
		AEGP_NewPlatformWorld: Some(new_platform),
		AEGP_DisposePlatformWorld: Some(dispose_platform),
		AEGP_NewReferenceFromPlatformWorld: Some(reference_from_platform),
	}
}

pub const fn suite_3() -> AEGP_WorldSuite3 {
	AEGP_WorldSuite3 {
		AEGP_New: Some(new_world),
		AEGP_Dispose: Some(dispose_world),
		AEGP_GetType: Some(get_type),
		AEGP_GetSize: Some(get_size),
		AEGP_GetRowBytes: Some(get_row_bytes),
		AEGP_GetBaseAddr8: Some(get_base8),
		AEGP_GetBaseAddr16: Some(get_base16),
		AEGP_GetBaseAddr32: Some(get_base32),
		AEGP_FillOutPFEffectWorld: Some(fill_out),
		AEGP_FastBlur: Some(fast_blur_sys),
		AEGP_NewPlatformWorld: Some(new_platform),
		AEGP_DisposePlatformWorld: Some(dispose_platform),
		AEGP_NewReferenceFromPlatformWorld: Some(reference_from_platform),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn fast_blur_spreads_a_pixel_into_its_neighbours() {
		let handle = alloc(AEGP_WorldType_8 as i32, 5, 1);
		{
			let world = unsafe { world_mut(handle) }.unwrap();
			// ARGB, pixel x=2 is opaque white.
			let pixel = &mut world.pixels[2 * 4..3 * 4];
			pixel.copy_from_slice(&[255, 255, 255, 255]);
			fast_blur(world, 1.0, 0);
			let centre = world.pixels[2 * 4];
			let side = world.pixels[4];
			let far = world.pixels[0];
			assert!(centre < 255, "centre should be averaged, was {centre}");
			assert!(side > 0, "neighbour should receive blur, was {side}");
			assert_eq!(far, 0);
		}
		unsafe { dispose_world(handle) };
	}

	#[test]
	fn world_from_layer_writes_argb() {
		let layer = crate::pixel::AnyLayer::U8(crate::pixel::Layer::from_fn(1, 1, |_, _| crate::pixel::Pixel::red()));
		let handle = world_from_layer(AEGP_WorldType_8 as i32, &layer, 1, 1);
		let world = unsafe { world_mut(handle) }.unwrap();
		assert_eq!(&world.pixels[..4], &[255, 255, 0, 0]);
		unsafe { dispose_world(handle) };
	}

	#[test]
	#[allow(clippy::unnecessary_cast)] // `PF_PixelFormat_*` is `u32` on macOS only.
	fn filled_out_float_world_reports_its_pixel_format() {
		let handle = alloc(AEGP_WorldType_32 as i32, 2, 2);
		let mut out: PF_EffectWorld = unsafe { std::mem::zeroed() };
		assert_eq!(unsafe { fill_out(handle, &mut out) }, PF_Err_NONE as A_Err);
		assert_eq!(
			unsafe { super::super::world::world_pixel_format(&out) },
			after_effects_sys::PF_PixelFormat_ARGB128 as u32
		);
		unsafe { dispose_world(handle) };
		assert_eq!(super::super::world::allocated_format(&out), None);
	}
}
