//! `PF_FillMatteSuite2`: rectangle fills and (un)premultiplication at all three
//! pixel depths, shared with the matching legacy `PF_UtilCallbacks` entries.
//!
//! A null `color` means transparent black (the SDK's "clear" behavior), and a
//! null `dst_rect` means the whole world. `premultiply` (the color-less
//! variant) treats the world as 8-bpc, matching the worlds this host hands to
//! plugins; the `premultiply_color*` variants are depth-typed by the entry
//! point the plugin picked. When unmultiplying a pixel whose alpha is zero the
//! color channels are passed through unchanged (the result is undefined in AE;
//! passing through avoids inventing data).

use crate::core::diagnostics::diag;
use after_effects_sys::{
	A_long, PF_EffectWorld, PF_Err, PF_Err_BAD_CALLBACK_PARAM, PF_Err_NONE, PF_FillMatteSuite2, PF_Pixel, PF_Pixel16,
	PF_PixelFloat, PF_ProgPtr, PF_Rect,
};

use super::pixel_norm::NormalizedPixel;

/// Iteration bounds for a world: `rect` (or the full world when null) clipped
/// to the world's extent. Returns `None` when the intersection is empty.
fn clipped_rect(rect: *const PF_Rect, world: &PF_EffectWorld) -> Option<PF_Rect> {
	let mut r = if rect.is_null() {
		PF_Rect {
			left: 0,
			top: 0,
			right: world.width,
			bottom: world.height,
		}
	} else {
		unsafe { *rect }
	};
	r.left = r.left.max(0);
	r.top = r.top.max(0);
	r.right = r.right.min(world.width);
	r.bottom = r.bottom.min(world.height);
	(r.right > r.left && r.bottom > r.top).then_some(r)
}

/// Row base pointer at `y` for a world's pixel buffer.
#[inline]
fn row_ptr(world: &PF_EffectWorld, y: i32) -> *mut u8 {
	(world.data as *mut u8).wrapping_offset(y as isize * world.rowbytes as isize)
}

/// Depth-generic fill: writes `color` (or transparent black when null) into
/// `rect ∩ world`.
unsafe fn fill_world<P: Copy>(color: *const P, dst_rect: *const PF_Rect, world: *mut PF_EffectWorld) -> PF_Err {
	if world.is_null() {
		log::error!("fill: world is null");
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}
	let world = &unsafe { *world };
	if world.data.is_null() {
		log::error!("fill: world.data is null");
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	// SAFETY: every PF pixel type is plain-old-data, so zeroed = transparent black.
	let color = if color.is_null() {
		unsafe { std::mem::zeroed() }
	} else {
		unsafe { *color }
	};

	let Some(rect) = clipped_rect(dst_rect, world) else {
		return PF_Err_NONE as PF_Err;
	};

	for y in rect.top..rect.bottom {
		let row = row_ptr(world, y) as *mut P;
		for x in rect.left..rect.right {
			// SAFETY: (x, y) is clipped inside the world; rows are `rowbytes` apart.
			unsafe { *row.add(x as usize) = color };
		}
	}

	PF_Err_NONE as PF_Err
}

/// Depth-generic (un)premultiplication against a matte color.
///
/// Forward: `rgb' = rgb·a + matte·(1-a)`; reverse: `rgb' = (rgb - matte·(1-a)) / a`.
/// `src` may alias `dst` (the color-less `premultiply` works in place).
unsafe fn premultiply_world<P: NormalizedPixel>(
	src: *mut PF_EffectWorld,
	color: *const P,
	forward: A_long,
	dst: *mut PF_EffectWorld,
) -> PF_Err {
	if dst.is_null() {
		log::error!("premultiply: dst is null");
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}
	let src = if src.is_null() { dst } else { src };
	let src_world = &unsafe { *src };
	let dst_world = &unsafe { *dst };
	if src_world.data.is_null() || dst_world.data.is_null() {
		log::error!("premultiply: world data is null");
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	let matte = if color.is_null() {
		[0.0, 0.0, 0.0]
	} else {
		let [_, r, g, b] = unsafe { *color }.to_norm();
		[r, g, b]
	};

	let width = src_world.width.min(dst_world.width).max(0);
	let height = src_world.height.min(dst_world.height).max(0);

	for y in 0..height {
		let src_row = row_ptr(src_world, y) as *const P;
		let dst_row = row_ptr(dst_world, y) as *mut P;
		for x in 0..width {
			// SAFETY: (x, y) lies inside both worlds; reading then writing keeps
			// in-place operation (src == dst) well-defined.
			let [a, r, g, b] = unsafe { *src_row.add(x as usize) }.to_norm();
			let rgb = [r, g, b];
			let out = if forward != 0 {
				std::array::from_fn(|i| rgb[i] * a + matte[i] * (1.0 - a))
			} else if a > 0.0 {
				std::array::from_fn(|i| (rgb[i] - matte[i] * (1.0 - a)) / a)
			} else {
				rgb
			};
			unsafe { *dst_row.add(x as usize) = P::from_norm([a, out[0], out[1], out[2]]) };
		}
	}

	PF_Err_NONE as PF_Err
}

// ============================================================================
// FFI entry points
// ============================================================================

pub(crate) unsafe extern "C" fn fill_8_sys(
	_effect_ref: PF_ProgPtr,
	color: *const PF_Pixel,
	dst_rect: *const PF_Rect,
	world: *mut PF_EffectWorld,
) -> PF_Err {
	diag!("FillMatteSuite/fill",
		"color" => format!("{:?}", color),
		"dst_rect" => format!("{:?}", dst_rect),
		"world" => format!("{:?}", world),
	);
	unsafe { fill_world::<PF_Pixel>(color, dst_rect, world) }
}

pub(crate) unsafe extern "C" fn fill_16_sys(
	_effect_ref: PF_ProgPtr,
	color: *const PF_Pixel16,
	dst_rect: *const PF_Rect,
	world: *mut PF_EffectWorld,
) -> PF_Err {
	diag!("FillMatteSuite/fill16",
		"color" => format!("{:?}", color),
		"dst_rect" => format!("{:?}", dst_rect),
		"world" => format!("{:?}", world),
	);
	unsafe { fill_world::<PF_Pixel16>(color, dst_rect, world) }
}

pub(crate) unsafe extern "C" fn fill_float_sys(
	_effect_ref: PF_ProgPtr,
	color: *const PF_PixelFloat,
	dst_rect: *const PF_Rect,
	world: *mut PF_EffectWorld,
) -> PF_Err {
	diag!("FillMatteSuite/fill_float",
		"color" => format!("{:?}", color),
		"dst_rect" => format!("{:?}", dst_rect),
		"world" => format!("{:?}", world),
	);
	unsafe { fill_world::<PF_PixelFloat>(color, dst_rect, world) }
}

pub(crate) unsafe extern "C" fn premultiply_sys(
	_effect_ref: PF_ProgPtr,
	forward: A_long,
	dst: *mut PF_EffectWorld,
) -> PF_Err {
	diag!("FillMatteSuite/premultiply",
		"forward" => forward,
		"dst" => format!("{:?}", dst),
	);
	// In-place against a black matte; this host's plain worlds are 8-bpc.
	unsafe { premultiply_world::<PF_Pixel>(dst, std::ptr::null(), forward, dst) }
}

pub(crate) unsafe extern "C" fn premultiply_color_8_sys(
	_effect_ref: PF_ProgPtr,
	src: *mut PF_EffectWorld,
	color: *const PF_Pixel,
	forward: A_long,
	dst: *mut PF_EffectWorld,
) -> PF_Err {
	diag!("FillMatteSuite/premultiply_color",
		"src" => format!("{:?}", src),
		"color" => format!("{:?}", color),
		"forward" => forward,
		"dst" => format!("{:?}", dst),
	);
	unsafe { premultiply_world::<PF_Pixel>(src, color, forward, dst) }
}

pub(crate) unsafe extern "C" fn premultiply_color_16_sys(
	_effect_ref: PF_ProgPtr,
	src: *mut PF_EffectWorld,
	color: *const PF_Pixel16,
	forward: A_long,
	dst: *mut PF_EffectWorld,
) -> PF_Err {
	diag!("FillMatteSuite/premultiply_color16",
		"src" => format!("{:?}", src),
		"color" => format!("{:?}", color),
		"forward" => forward,
		"dst" => format!("{:?}", dst),
	);
	unsafe { premultiply_world::<PF_Pixel16>(src, color, forward, dst) }
}

pub(crate) unsafe extern "C" fn premultiply_color_float_sys(
	_effect_ref: PF_ProgPtr,
	src: *mut PF_EffectWorld,
	color: *const PF_PixelFloat,
	forward: A_long,
	dst: *mut PF_EffectWorld,
) -> PF_Err {
	diag!("FillMatteSuite/premultiply_color_float",
		"src" => format!("{:?}", src),
		"color" => format!("{:?}", color),
		"forward" => forward,
		"dst" => format!("{:?}", dst),
	);
	unsafe { premultiply_world::<PF_PixelFloat>(src, color, forward, dst) }
}

// ============================================================================
// Factory Function
// ============================================================================

/// Builds the `PF_FillMatteSuite2` vtable.
///
/// `const` so it can initialize the shared [`SUITE_CONTAINER`](crate::suites::SUITE_CONTAINER)
/// static; the suite is a stateless table of function pointers.
pub const fn create_fill_matte_suite_2() -> PF_FillMatteSuite2 {
	PF_FillMatteSuite2 {
		fill: Some(fill_8_sys),
		fill16: Some(fill_16_sys),
		fill_float: Some(fill_float_sys),
		premultiply: Some(premultiply_sys),
		premultiply_color: Some(premultiply_color_8_sys),
		premultiply_color16: Some(premultiply_color_16_sys),
		premultiply_color_float: Some(premultiply_color_float_sys),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn world_over<P>(width: i32, height: i32, buf: &mut [P]) -> PF_EffectWorld {
		assert_eq!(buf.len(), (width * height) as usize);
		let mut world: PF_EffectWorld = unsafe { std::mem::zeroed() };
		world.width = width;
		world.height = height;
		world.rowbytes = width * std::mem::size_of::<P>() as i32;
		world.data = buf.as_mut_ptr() as *mut _;
		world
	}

	#[test]
	fn fill_writes_only_the_clipped_rect() {
		let w = 4;
		let h = 4;
		let sentinel = PF_Pixel {
			alpha: 1,
			red: 1,
			green: 1,
			blue: 1,
		};
		let mut buf = vec![sentinel; (w * h) as usize];
		let mut world = world_over(w, h, &mut buf);
		let color = PF_Pixel {
			alpha: 255,
			red: 10,
			green: 20,
			blue: 30,
		};
		// Deliberately over-sized rect: must clip to the world.
		let rect = PF_Rect {
			left: 2,
			top: 1,
			right: 99,
			bottom: 3,
		};

		let err = unsafe { fill_8_sys(std::ptr::null_mut(), &color, &rect, &mut world) };
		assert_eq!(err, PF_Err_NONE as PF_Err);
		for y in 0..h {
			for x in 0..w {
				let p = buf[(y * w + x) as usize];
				let inside = x >= 2 && (1..3).contains(&y);
				assert_eq!(p.red == 10, inside, "({x},{y})");
			}
		}
	}

	#[test]
	fn fill_null_color_clears_to_transparent_black() {
		let mut buf = vec![
			PF_Pixel16 {
				alpha: 32768,
				red: 32768,
				green: 32768,
				blue: 32768,
			};
			4
		];
		let mut world = world_over(2, 2, &mut buf);
		let err = unsafe { fill_16_sys(std::ptr::null_mut(), std::ptr::null(), std::ptr::null(), &mut world) };
		assert_eq!(err, PF_Err_NONE as PF_Err);
		for p in &buf {
			assert_eq!((p.alpha, p.red, p.green, p.blue), (0, 0, 0, 0));
		}
	}

	#[test]
	fn fill_float_fills_whole_world() {
		let mut buf = vec![
			PF_PixelFloat {
				alpha: 0.0,
				red: 0.0,
				green: 0.0,
				blue: 0.0,
			};
			4
		];
		let mut world = world_over(2, 2, &mut buf);
		let color = PF_PixelFloat {
			alpha: 1.0,
			red: 2.5,
			green: 0.5,
			blue: -0.25,
		};
		let err = unsafe { fill_float_sys(std::ptr::null_mut(), &color, std::ptr::null(), &mut world) };
		assert_eq!(err, PF_Err_NONE as PF_Err);
		for p in &buf {
			// Float fills carry HDR values through untouched.
			assert_eq!((p.red, p.green, p.blue), (2.5, 0.5, -0.25));
		}
	}

	#[test]
	fn premultiply_forward_scales_rgb_by_alpha() {
		let mut buf = vec![
			PF_Pixel {
				alpha: 128,
				red: 255,
				green: 100,
				blue: 0,
			};
			4
		];
		let mut world = world_over(2, 2, &mut buf);
		let err = unsafe { premultiply_sys(std::ptr::null_mut(), 1, &mut world) };
		assert_eq!(err, PF_Err_NONE as PF_Err);
		let p = buf[0];
		// a = 128/255: red 255 -> 128, green 100 -> ~50.
		assert_eq!(p.alpha, 128);
		assert_eq!(p.red, 128);
		assert!((p.green as i32 - 50).abs() <= 1, "green {}", p.green);
		assert_eq!(p.blue, 0);
	}

	#[test]
	fn premultiply_round_trips_at_16bpc_precision() {
		let orig = PF_Pixel16 {
			alpha: 16384, // 0.5
			red: 32768,
			green: 10000,
			blue: 4000,
		};
		let mut buf = vec![orig; 1];
		let mut world = world_over(1, 1, &mut buf);
		let err = unsafe { premultiply_color_16_sys(std::ptr::null_mut(), &mut world, std::ptr::null(), 1, &mut world) };
		assert_eq!(err, PF_Err_NONE as PF_Err);
		let err = unsafe { premultiply_color_16_sys(std::ptr::null_mut(), &mut world, std::ptr::null(), 0, &mut world) };
		assert_eq!(err, PF_Err_NONE as PF_Err);
		let p = buf[0];
		assert!((p.red as i32 - orig.red as i32).abs() <= 2, "red {}", p.red);
		assert!((p.green as i32 - orig.green as i32).abs() <= 2, "green {}", p.green);
		assert!((p.blue as i32 - orig.blue as i32).abs() <= 2, "blue {}", p.blue);
	}

	#[test]
	fn premultiply_color_blends_toward_matte() {
		// Fully transparent pixel premultiplied against a white matte becomes white.
		let mut buf = vec![
			PF_Pixel {
				alpha: 0,
				red: 40,
				green: 50,
				blue: 60,
			};
			1
		];
		let mut world = world_over(1, 1, &mut buf);
		let matte = PF_Pixel {
			alpha: 255,
			red: 255,
			green: 255,
			blue: 255,
		};
		let err = unsafe { premultiply_color_8_sys(std::ptr::null_mut(), &mut world, &matte, 1, &mut world) };
		assert_eq!(err, PF_Err_NONE as PF_Err);
		let p = buf[0];
		assert_eq!((p.alpha, p.red, p.green, p.blue), (0, 255, 255, 255));
	}

	#[test]
	fn null_worlds_are_rejected() {
		assert_eq!(
			unsafe { fill_8_sys(std::ptr::null_mut(), std::ptr::null(), std::ptr::null(), std::ptr::null_mut()) },
			PF_Err_BAD_CALLBACK_PARAM as PF_Err
		);
		assert_eq!(
			unsafe { premultiply_sys(std::ptr::null_mut(), 1, std::ptr::null_mut()) },
			PF_Err_BAD_CALLBACK_PARAM as PF_Err
		);
	}
}
