use after_effects_sys::*;
use rayon::prelude::*;
use std::os::raw::c_void;

/// Emulates `PF_WorldTransformSuite1::copy` function
/// # Safety
/// This function is unsafe because it handles raw pointers.
pub unsafe extern "C" fn Copy_sys(
	_effect_ref: PF_ProgPtr,
	src: *mut PF_EffectWorld,
	dst: *mut PF_EffectWorld,
	src_r: *mut PF_Rect,
	dst_r: *mut PF_Rect,
) -> PF_Err {
	// Handle null source/dest pointers gracefully
	if src.is_null() || dst.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	// Check if src and dst point to the same buffer
	if std::ptr::eq(src, dst) {
		// Same buffer - nothing to copy
		return PF_Err_NONE as PF_Err;
	}

	let src_world = &mut unsafe { *src };
	let dst_world = &mut unsafe { *dst };

	// Calculate buffer addresses for overlap detection
	// Buffers overlap if: src_addr <= dst_addr + dst_size && dst_addr <= src_addr + src_size
	let src_addr = src_world.data as usize;
	let dst_addr = dst_world.data as usize;
	let src_size = (src_world.height as usize) * (src_world.rowbytes as usize);
	let dst_size = (dst_world.height as usize) * (dst_world.rowbytes as usize);
	let src_end = src_addr.saturating_add(src_size);
	let dst_end = dst_addr.saturating_add(dst_size);
	let buffers_overlap = !(src_end <= dst_addr || dst_end <= src_addr);

	// Determine source rectangle
	let src_rect = if !src_r.is_null() {
		unsafe { *src_r }
	} else {
		PF_Rect {
			left: 0,
			top: 0,
			right: src_world.width,
			bottom: src_world.height,
		}
	};

	// Determine destination point (top-left)
	let (dst_x, dst_y) = if !dst_r.is_null() {
		((unsafe { *dst_r }).left, (unsafe { *dst_r }).top)
	} else {
		(src_rect.left, src_rect.top)
	};

	// Calculate copy dimensions
	let copy_width = (src_rect.right - src_rect.left).max(0);
	let copy_height = (src_rect.bottom - src_rect.top).max(0);

	if copy_width == 0 || copy_height == 0 {
		return PF_Err_NONE as PF_Err; // Nothing to copy
	}

	// Clipping: Ensure we don't read/write out of bounds
	// Source clipping
	let src_clamped_left = src_rect.left.max(0);
	let src_clamped_top = src_rect.top.max(0);
	// We also need to clip width/height based on src bounds
	let src_avail_width = src_world.width - src_clamped_left;
	let src_avail_height = src_world.height - src_clamped_top;

	// Dest clipping
	let dst_clamped_left = dst_x.max(0); // If negative dst, we must increment src start
	let dst_clamped_top = dst_y.max(0);

	let dst_avail_width = dst_world.width - dst_clamped_left;
	let dst_avail_height = dst_world.height - dst_clamped_top;

	// Adjust for negative destination offsets (if dst_x < 0, we skip pixels in src)
	let skip_x = if dst_x < 0 { -dst_x } else { 0 };
	let skip_y = if dst_y < 0 { -dst_y } else { 0 };

	// Final dimensions to copy
	let final_width = (copy_width - skip_x).min(src_avail_width).min(dst_avail_width);
	let final_height = (copy_height - skip_y).min(src_avail_height).min(dst_avail_height);

	if final_width <= 0 || final_height <= 0 {
		return PF_Err_NONE as PF_Err;
	}

	// Calculate starting offsets
	let actual_src_left = src_clamped_left + skip_x;
	let actual_src_top = src_clamped_top + skip_y;
	let actual_dst_left = dst_x + skip_x;
	let actual_dst_top = dst_y + skip_y;

	// Prepare data for parallel execution
	// We cast to usize to pass across threads safely (assuming buffers are accessible/pinned)
	let src_buffer_addr = src_world.data as usize;
	let src_rowbytes = src_world.rowbytes as isize;
	let dst_buffer_addr = dst_world.data as usize;
	let dst_rowbytes = dst_world.rowbytes as isize;

	// Size of a pixel in bytes. PF_EffectWorld usually PF_Pixel8 (4 bytes).
	// But strictly it depends on deep color.
	// For now assuming PF_Pixel8 (ARGB 8-bit).
	let pixel_size = std::mem::size_of::<PF_Pixel8>();

	// Parallel Copy
	(0..final_height).into_par_iter().for_each(|y| {
		let current_src_y = actual_src_top + y;
		let current_dst_y = actual_dst_top + y;

		// Calculate row start addresses
		// Note: data is *mut c_void, treating as *mut u8 for offset
		let src_row_ptr = (src_buffer_addr as *const u8).wrapping_offset((current_src_y as isize) * src_rowbytes);
		let dst_row_ptr = (dst_buffer_addr as *mut u8).wrapping_offset((current_dst_y as isize) * dst_rowbytes);

		// Calculate signal offsets within the row
		let src_pixel_ptr = src_row_ptr.wrapping_add((actual_src_left as usize) * pixel_size);
		let dst_pixel_ptr = dst_row_ptr.wrapping_add((actual_dst_left as usize) * pixel_size);

		// Use std::ptr::copy if buffers overlap (safe for overlapping regions),
		// otherwise use copy_nonoverlapping for better performance
		unsafe {
			if buffers_overlap {
				std::ptr::copy(src_pixel_ptr, dst_pixel_ptr, (final_width as usize) * pixel_size);
			} else {
				std::ptr::copy_nonoverlapping(src_pixel_ptr, dst_pixel_ptr, (final_width as usize) * pixel_size);
			}
		}
	});

	PF_Err_NONE as PF_Err
}

// ============================================================================
// Shared pixel-access / compositing helpers
//
// Every function below assumes `PF_Pixel8` (straight-alpha 8-bit ARGB), like
// the rest of this crate's suites (see `iterate.rs`, `world.rs`). Deep-color
// (16-bit / float) worlds are not handled.
// ============================================================================

const PIXEL_SIZE: isize = std::mem::size_of::<PF_Pixel8>() as isize;

/// A `Copy`/`Send`/`Sync` view of a `PF_EffectWorld`'s pixel buffer, addressed
/// by raw `usize` rather than a borrowed reference. Mirrors the pattern
/// `Copy_sys`/`iterate_8_sys` already use to move buffer access across rayon's
/// worker threads without fighting Rust's aliasing rules: each pixel touched
/// by the parallel iteration is disjoint, so concurrent raw-pointer access is
/// sound even though a `&mut PF_EffectWorld` could not be shared this way.
#[derive(Clone, Copy)]
struct RawWorld {
	addr: usize,
	rowbytes: isize,
	width: i32,
	height: i32,
}

impl RawWorld {
	fn from(world: &PF_EffectWorld) -> Self {
		Self {
			addr: world.data as usize,
			rowbytes: world.rowbytes as isize,
			width: world.width,
			height: world.height,
		}
	}

	/// Reads the pixel at `(x, y)`, or transparent black if it falls outside
	/// the buffer (this is what gives `convolve`'s "transparent borders" mode
	/// and off-screen mask/transform samples their fall-through behavior).
	#[inline]
	unsafe fn read(&self, x: i32, y: i32) -> PF_Pixel8 {
		if self.addr == 0 || x < 0 || y < 0 || x >= self.width || y >= self.height {
			return PF_Pixel8 {
				alpha: 0,
				red: 0,
				green: 0,
				blue: 0,
			};
		}
		let ptr = (self.addr as *const u8).wrapping_offset(y as isize * self.rowbytes + x as isize * PIXEL_SIZE)
			as *const PF_Pixel8;
		unsafe { *ptr }
	}

	/// Like [`read`](Self::read), but clamps out-of-range coordinates to the
	/// nearest edge pixel instead of returning transparent black, for
	/// `PF_KernelFlag_REPLICATE_BORDERS`.
	#[inline]
	unsafe fn read_edge(&self, x: i32, y: i32, replicate: bool) -> PF_Pixel8 {
		if replicate {
			let cx = x.clamp(0, (self.width - 1).max(0));
			let cy = y.clamp(0, (self.height - 1).max(0));
			unsafe { self.read(cx, cy) }
		} else {
			unsafe { self.read(x, y) }
		}
	}

	#[inline]
	unsafe fn write(&self, x: i32, y: i32, pixel: PF_Pixel8) {
		if self.addr == 0 || x < 0 || y < 0 || x >= self.width || y >= self.height {
			return;
		}
		let ptr = (self.addr as *mut u8).wrapping_offset(y as isize * self.rowbytes + x as isize * PIXEL_SIZE)
			as *mut PF_Pixel8;
		unsafe { *ptr = pixel };
	}

	/// Bilinear sample at continuous coordinate `(x, y)`, where integer
	/// coordinates land exactly on pixel centers. Returns `(a, r, g, b)` on a
	/// 0..255 scale. Taps that fall outside the buffer contribute transparent
	/// black, so sampling near an edge fades out rather than reading garbage.
	unsafe fn sample_bilinear(&self, x: f64, y: f64) -> (f64, f64, f64, f64) {
		let x0 = x.floor();
		let y0 = y.floor();
		let fx = x - x0;
		let fy = y - y0;
		let (x0i, y0i) = (x0 as i32, y0 as i32);
		let p00 = unsafe { self.read(x0i, y0i) };
		let p10 = unsafe { self.read(x0i + 1, y0i) };
		let p01 = unsafe { self.read(x0i, y0i + 1) };
		let p11 = unsafe { self.read(x0i + 1, y0i + 1) };
		let mix = |c00: u8, c10: u8, c01: u8, c11: u8| {
			let top = c00 as f64 + (c10 as f64 - c00 as f64) * fx;
			let bottom = c01 as f64 + (c11 as f64 - c01 as f64) * fx;
			top + (bottom - top) * fy
		};
		(
			mix(p00.alpha, p10.alpha, p01.alpha, p11.alpha),
			mix(p00.red, p10.red, p01.red, p11.red),
			mix(p00.green, p10.green, p01.green, p11.green),
			mix(p00.blue, p10.blue, p01.blue, p11.blue),
		)
	}
}

/// Whether `field` restricts processing to alternating destination scanlines.
/// `UPPER` is treated as the even rows (0, 2, 4, ...) and `LOWER` the odd
/// rows; `FRAME` (and any other value) processes every row.
#[inline]
// `PF_Field_*` is `u32` on macOS, `i32` elsewhere; the casts are redundant here
// but required there.
#[allow(clippy::unnecessary_cast)]
fn skip_row_for_field(y: i32, field: PF_Field) -> bool {
	if field == PF_Field_UPPER as i32 {
		y.rem_euclid(2) != 0
	} else if field == PF_Field_LOWER as i32 {
		y.rem_euclid(2) == 0
	} else {
		false
	}
}

/// Unpacks a stored pixel into (alpha, [r, g, b]) on a 0..1 scale. When
/// `premultiplied` is set (`PF_MF_Alpha_PREMUL`), the stored channels are
/// divided back out by alpha so the rest of the compositing math can always
/// work in straight alpha, matching how the rest of this crate stores worlds.
#[inline]
fn to_straight(p: PF_Pixel8, premultiplied: bool) -> (f64, [f64; 3]) {
	let a = p.alpha as f64 / 255.0;
	let raw = [p.red as f64 / 255.0, p.green as f64 / 255.0, p.blue as f64 / 255.0];
	let c = if premultiplied {
		if a > 1e-6 {
			[raw[0] / a, raw[1] / a, raw[2] / a]
		} else {
			[0.0; 3]
		}
	} else {
		raw
	};
	(a, c)
}

#[inline]
fn from_straight(a: f64, c: [f64; 3]) -> PF_Pixel8 {
	let to_u8 = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
	PF_Pixel8 {
		alpha: to_u8(a),
		red: to_u8(c[0]),
		green: to_u8(c[1]),
		blue: to_u8(c[2]),
	}
}

// ---- Blend-mode formulas (operate on 0..1 normalized channel values) ------
// Separable modes follow the W3C compositing/blending spec's per-channel
// formulas; the four non-separable modes (Hue/Saturation/Color/Luminosity)
// use its Lum/Sat/ClipColor helpers. `cb` is the backdrop (destination),
// `cs` the source, matching the spec's naming.

fn hard_light(cb: f64, cs: f64) -> f64 {
	if cs <= 0.5 {
		2.0 * cb * cs
	} else {
		let cs2 = 2.0 * cs - 1.0;
		cb + cs2 - cb * cs2
	}
}

fn soft_light(cb: f64, cs: f64) -> f64 {
	if cs <= 0.5 {
		cb - (1.0 - 2.0 * cs) * cb * (1.0 - cb)
	} else {
		let d = if cb <= 0.25 {
			((16.0 * cb - 12.0) * cb + 4.0) * cb
		} else {
			cb.sqrt()
		};
		cb + (2.0 * cs - 1.0) * (d - cb)
	}
}

fn color_dodge(cb: f64, cs: f64) -> f64 {
	if cb <= 0.0 {
		0.0
	} else if cs >= 1.0 {
		1.0
	} else {
		(cb / (1.0 - cs)).min(1.0)
	}
}

fn color_burn(cb: f64, cs: f64) -> f64 {
	if cb >= 1.0 {
		1.0
	} else if cs <= 0.0 {
		0.0
	} else {
		1.0 - ((1.0 - cb) / cs).min(1.0)
	}
}

fn vivid_light(cb: f64, cs: f64) -> f64 {
	if cs <= 0.5 {
		color_burn(cb, 2.0 * cs)
	} else {
		color_dodge(cb, 2.0 * cs - 1.0)
	}
}

fn pin_light(cb: f64, cs: f64) -> f64 {
	if cs <= 0.5 {
		cb.min(2.0 * cs)
	} else {
		cb.max(2.0 * cs - 1.0)
	}
}

/// Per-channel separable blend. Modes with no simple per-channel formula
/// (Copy/In Front/Dissolve, the alpha-multiply variants, and anything this
/// emulator doesn't recognise) fall back to `cs` -- i.e. Normal/over -- which
/// is also exactly the identity `composite_pixel` needs for those modes.
#[allow(non_upper_case_globals)]
fn blend_channel(mode: A_long, cb: f64, cs: f64) -> f64 {
	match mode {
		PF_Xfer_MULTIPLY
		| PF_Xfer_MULTIPLY_ALPHA
		| PF_Xfer_MULTIPLY_ALPHA_LUMA
		| PF_Xfer_MULTIPLY_NOT_ALPHA
		| PF_Xfer_MULTIPLY_NOT_ALPHA_LUMA => cb * cs,
		PF_Xfer_SCREEN => cb + cs - cb * cs,
		PF_Xfer_OVERLAY => hard_light(cs, cb),
		PF_Xfer_HARD_LIGHT => hard_light(cb, cs),
		PF_Xfer_SOFT_LIGHT => soft_light(cb, cs),
		PF_Xfer_DARKEN => cb.min(cs),
		PF_Xfer_LIGHTEN => cb.max(cs),
		PF_Xfer_DIFFERENCE | PF_Xfer_DIFFERENCE2 => (cb - cs).abs(),
		PF_Xfer_EXCLUSION => cb + cs - 2.0 * cb * cs,
		PF_Xfer_COLOR_DODGE | PF_Xfer_COLOR_DODGE2 => color_dodge(cb, cs),
		PF_Xfer_COLOR_BURN | PF_Xfer_COLOR_BURN2 => color_burn(cb, cs),
		PF_Xfer_ADD | PF_Xfer_LINEAR_DODGE | PF_Xfer_ADDITIVE_PREMUL | PF_Xfer_ALPHA_ADD => (cb + cs).min(1.0),
		PF_Xfer_SUBTRACT => (cb - cs).max(0.0),
		PF_Xfer_LINEAR_BURN => (cb + cs - 1.0).max(0.0),
		PF_Xfer_LINEAR_LIGHT => (cb + 2.0 * cs - 1.0).clamp(0.0, 1.0),
		PF_Xfer_VIVID_LIGHT => vivid_light(cb, cs),
		PF_Xfer_PIN_LIGHT => pin_light(cb, cs),
		PF_Xfer_HARD_MIX => {
			if vivid_light(cb, cs) < 0.5 {
				0.0
			} else {
				1.0
			}
		}
		PF_Xfer_DIVIDE => {
			if cs <= 0.0 {
				1.0
			} else {
				(cb / cs).min(1.0)
			}
		}
		_ => cs,
	}
}

fn lum(c: [f64; 3]) -> f64 {
	0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

fn clip_color(c: [f64; 3]) -> [f64; 3] {
	let l = lum(c);
	let n = c.iter().cloned().fold(f64::INFINITY, f64::min);
	let x = c.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
	let mut out = c;
	if n < 0.0 && (l - n).abs() > 1e-9 {
		for v in out.iter_mut() {
			*v = l + (*v - l) * l / (l - n);
		}
	}
	if x > 1.0 && (x - l).abs() > 1e-9 {
		for v in out.iter_mut() {
			*v = l + (*v - l) * (1.0 - l) / (x - l);
		}
	}
	out
}

fn set_lum(c: [f64; 3], l: f64) -> [f64; 3] {
	let d = l - lum(c);
	clip_color([c[0] + d, c[1] + d, c[2] + d])
}

fn sat(c: [f64; 3]) -> f64 {
	c.iter().cloned().fold(f64::NEG_INFINITY, f64::max) - c.iter().cloned().fold(f64::INFINITY, f64::min)
}

fn set_sat(c: [f64; 3], s: f64) -> [f64; 3] {
	let mut out = c;
	let mut idx = [0usize, 1, 2];
	idx.sort_by(|&a, &b| out[a].partial_cmp(&out[b]).unwrap());
	let (imin, imid, imax) = (idx[0], idx[1], idx[2]);
	if out[imax] > out[imin] {
		out[imid] = (out[imid] - out[imin]) * s / (out[imax] - out[imin]);
		out[imax] = s;
	} else {
		out[imid] = 0.0;
		out[imax] = 0.0;
	}
	out[imin] = 0.0;
	out
}

/// Full-triple blend, dispatching to the non-separable HSL modes and
/// per-channel [`blend_channel`] for everything else. `cb` is the backdrop
/// (destination), `cs` the source.
#[allow(non_upper_case_globals)]
fn blend_rgb(mode: A_long, cb: [f64; 3], cs: [f64; 3]) -> [f64; 3] {
	match mode {
		PF_Xfer_HUE => set_lum(set_sat(cs, sat(cb)), lum(cb)),
		PF_Xfer_SATURATION => set_lum(set_sat(cb, sat(cs)), lum(cb)),
		PF_Xfer_COLOR => set_lum(cs, lum(cb)),
		PF_Xfer_LUMINOSITY => set_lum(cb, lum(cs)),
		PF_Xfer_LIGHTER_COLOR => {
			if lum(cs) >= lum(cb) {
				cs
			} else {
				cb
			}
		}
		PF_Xfer_DARKER_COLOR => {
			if lum(cs) <= lum(cb) {
				cs
			} else {
				cb
			}
		}
		_ => [
			blend_channel(mode, cb[0], cs[0]),
			blend_channel(mode, cb[1], cs[1]),
			blend_channel(mode, cb[2], cs[2]),
		],
	}
}

/// Standard Porter-Duff "over" composite of `top` onto `bottom`, with `mode`
/// selecting the RGB blend function applied where both are opaque. This is
/// the one compositing primitive `composite_rect`/`transfer_rect`/
/// `transform_world` all reduce to.
fn composite_over(top_a: f64, top_c: [f64; 3], bottom_a: f64, bottom_c: [f64; 3], mode: A_long) -> (f64, [f64; 3]) {
	let out_a = top_a + bottom_a * (1.0 - top_a);
	let blended = blend_rgb(mode, bottom_c, top_c);
	let mut out_c = [0.0; 3];
	for i in 0..3 {
		let premul = top_c[i] * top_a * (1.0 - bottom_a)
			+ bottom_c[i] * bottom_a * (1.0 - top_a)
			+ blended[i] * top_a * bottom_a;
		out_c[i] = if out_a > 1e-6 {
			(premul / out_a).clamp(0.0, 1.0)
		} else {
			0.0
		};
	}
	(out_a, out_c)
}

/// Composites `src` onto `dst` under `mode`. `PF_Xfer_NONE` leaves `dst`
/// untouched; `PF_Xfer_BEHIND` composites `dst` on top of `src` (visible
/// wherever `dst` is transparent) instead of the usual `src`-on-top order.
/// Everything else -- including `COPY`/`IN_FRONT`, which have no distinct
/// blend formula of their own -- is a Normal-mode `over`.
#[allow(non_upper_case_globals)]
fn composite_pixel(src_a: f64, src_c: [f64; 3], dst_a: f64, dst_c: [f64; 3], mode: A_long) -> (f64, [f64; 3]) {
	if mode == PF_Xfer_NONE {
		return (dst_a, dst_c);
	}
	if mode == PF_Xfer_BEHIND {
		return composite_over(dst_a, dst_c, src_a, src_c, PF_Xfer_COPY);
	}
	composite_over(src_a, src_c, dst_a, dst_c, mode)
}

/// Samples a mask world's value at `(x, y)` (in the masked layer's
/// coordinates; `offset` shifts into the mask's own coordinate space),
/// honoring `PF_MaskFlag_LUMINANCE` (use luma instead of alpha) and
/// `PF_MaskFlag_INVERTED`.
#[allow(non_upper_case_globals)]
// `PF_MaskFlag_*` is `u32` on macOS, `i32` elsewhere; the casts are redundant
// here but required there.
#[allow(clippy::unnecessary_cast)]
fn mask_value(mask: RawWorld, offset: PF_Point, what_is_mask: PF_MaskFlags, x: i32, y: i32) -> f64 {
	let p = unsafe { mask.read(x - offset.h, y - offset.v) };
	let v = if what_is_mask & PF_MaskFlag_LUMINANCE as i32 != 0 {
		(0.3 * p.red as f64 + 0.59 * p.green as f64 + 0.11 * p.blue as f64) / 255.0
	} else {
		p.alpha as f64 / 255.0
	};
	if what_is_mask & PF_MaskFlag_INVERTED as i32 != 0 {
		1.0 - v
	} else {
		v
	}
}

// ---- 3x3 affine matrix helpers (row-vector convention: p' = p * M) --------

type Mat3 = [[f64; 3]; 3];
const IDENTITY3: Mat3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

fn apply_matrix(m: &Mat3, x: f64, y: f64) -> (f64, f64) {
	let xh = x * m[0][0] + y * m[1][0] + m[2][0];
	let yh = x * m[0][1] + y * m[1][1] + m[2][1];
	let wh = x * m[0][2] + y * m[1][2] + m[2][2];
	if (wh - 1.0).abs() > 1e-9 && wh.abs() > 1e-9 {
		(xh / wh, yh / wh)
	} else {
		(xh, yh)
	}
}

fn invert3(m: Mat3) -> Option<Mat3> {
	let [[a, b, c], [d, e, f], [g, h, i]] = m;
	let a1 = e * i - f * h;
	let b1 = f * g - d * i;
	let c1 = d * h - e * g;
	let det = a * a1 + b * b1 + c * c1;
	if det.abs() < 1e-12 {
		return None;
	}
	let inv_det = 1.0 / det;
	let d1 = c * h - b * i;
	let e1 = a * i - c * g;
	let f1 = b * g - a * h;
	let g1 = b * f - c * e;
	let h1 = c * d - a * f;
	let i1 = a * e - b * d;
	Some([
		[a1 * inv_det, d1 * inv_det, g1 * inv_det],
		[b1 * inv_det, e1 * inv_det, h1 * inv_det],
		[c1 * inv_det, f1 * inv_det, i1 * inv_det],
	])
}

// ============================================================================
// Suite entry points
// ============================================================================

/// Composites `source_wld`'s `src_rect` onto `dest_wld` at `(dest_x,
/// dest_y)`, scaled by `src_opacity` (0..255) and blended per `xfer_mode`.
unsafe extern "C" fn composite_rect_sys(
	_effect_ref: PF_ProgPtr,
	src_rect: *mut PF_Rect,
	src_opacity: A_long,
	source_wld: *mut PF_EffectWorld,
	dest_x: A_long,
	dest_y: A_long,
	field_rdr: PF_Field,
	xfer_mode: PF_XferMode,
	dest_wld: *mut PF_EffectWorld,
) -> PF_Err {
	if source_wld.is_null() || dest_wld.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}
	let src = unsafe { &*source_wld };
	let dst = unsafe { &*dest_wld };
	if src.data.is_null() || dst.data.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	let rect = if !src_rect.is_null() {
		unsafe { *src_rect }
	} else {
		PF_Rect {
			left: 0,
			top: 0,
			right: src.width,
			bottom: src.height,
		}
	};
	let left = rect.left.max(0);
	let top = rect.top.max(0);
	let width = (rect.right.min(src.width) - left).max(0);
	let height = (rect.bottom.min(src.height) - top).max(0);
	if width == 0 || height == 0 {
		return PF_Err_NONE as PF_Err;
	}

	let opacity = src_opacity.clamp(0, 255) as f64 / 255.0;
	let src_raw = RawWorld::from(src);
	let dst_raw = RawWorld::from(dst);

	(0..height).into_par_iter().for_each(|row| {
		let dy = dest_y + row;
		if skip_row_for_field(dy, field_rdr) {
			return;
		}
		let sy = top + row;
		for col in 0..width {
			let dx = dest_x + col;
			let sx = left + col;
			let sp = unsafe { src_raw.read(sx, sy) };
			let (mut src_a, src_c) = to_straight(sp, false);
			src_a *= opacity;
			let dp = unsafe { dst_raw.read(dx, dy) };
			let (dst_a, dst_c) = to_straight(dp, false);
			let (out_a, out_c) = composite_pixel(src_a, src_c, dst_a, dst_c, xfer_mode);
			unsafe { dst_raw.write(dx, dy, from_straight(out_a, out_c)) };
		}
	});

	PF_Err_NONE as PF_Err
}

/// Cross-fades `src1`/`src2` into `dst` at a constant `ratio` (16.16 fixed,
/// 0.0..1.0), including the alpha channel. Operates over the overlap of all
/// three worlds' dimensions.
unsafe extern "C" fn blend_sys(
	_effect_ref: PF_ProgPtr,
	src1: *const PF_EffectWorld,
	src2: *const PF_EffectWorld,
	ratio: PF_Fixed,
	dst: *mut PF_EffectWorld,
) -> PF_Err {
	if src1.is_null() || src2.is_null() || dst.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}
	let s1 = unsafe { &*src1 };
	let s2 = unsafe { &*src2 };
	let d = unsafe { &*dst };
	if s1.data.is_null() || s2.data.is_null() || d.data.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	let t = (ratio as f64 / 65536.0).clamp(0.0, 1.0);
	let width = s1.width.min(s2.width).min(d.width).max(0);
	let height = s1.height.min(s2.height).min(d.height).max(0);
	if width == 0 || height == 0 {
		return PF_Err_NONE as PF_Err;
	}

	let r1 = RawWorld::from(s1);
	let r2 = RawWorld::from(s2);
	let rd = RawWorld::from(d);

	(0..height).into_par_iter().for_each(|y| {
		for x in 0..width {
			let p1 = unsafe { r1.read(x, y) };
			let p2 = unsafe { r2.read(x, y) };
			let lerp8 = |a: u8, b: u8| ((a as f64) + (b as f64 - a as f64) * t).round().clamp(0.0, 255.0) as u8;
			let out = PF_Pixel8 {
				alpha: lerp8(p1.alpha, p2.alpha),
				red: lerp8(p1.red, p2.red),
				green: lerp8(p1.green, p2.green),
				blue: lerp8(p1.blue, p2.blue),
			};
			unsafe { rd.write(x, y, out) };
		}
	});

	PF_Err_NONE as PF_Err
}

fn quantize(v: f64, no_clamp: bool) -> u8 {
	if no_clamp {
		(v.round() as i64).rem_euclid(256) as u8
	} else {
		v.round().clamp(0.0, 255.0) as u8
	}
}

/// Bundles the parameters shared by every tap of a convolution at a given
/// output pixel, so the per-channel helpers below don't need a long, easily
/// mis-ordered argument list.
#[derive(Clone, Copy)]
struct ConvolveCtx<'a> {
	world: RawWorld,
	taps: &'a [(i32, i32)],
	replicate: bool,
	normalized: bool,
}

fn convolve_value(
	ctx: ConvolveCtx,
	x: i32,
	y: i32,
	weights: &[f64],
	sum: f64,
	extract: impl Fn(PF_Pixel8) -> f64,
) -> f64 {
	let mut acc = 0.0;
	for (i, &(ox, oy)) in ctx.taps.iter().enumerate() {
		let w = weights[i];
		if w == 0.0 {
			continue;
		}
		let px = unsafe { ctx.world.read_edge(x + ox, y + oy, ctx.replicate) };
		acc += w * extract(px);
	}
	if ctx.normalized || sum.abs() < 1e-9 {
		acc
	} else {
		acc / sum
	}
}

/// Convolves premultiplied by each tap's own alpha, then unpremultiplies the
/// result -- avoids the dark/bright fringing a straight convolution produces
/// at partially-transparent edges. Self-normalizing regardless of the
/// `NORMALIZED` flag, since scaling every weight by a constant cancels out of
/// the ratio.
fn convolve_value_alpha_weighted(
	ctx: ConvolveCtx,
	x: i32,
	y: i32,
	weights: &[f64],
	extract: impl Fn(PF_Pixel8) -> f64,
) -> f64 {
	let mut num = 0.0;
	let mut den = 0.0;
	for (i, &(ox, oy)) in ctx.taps.iter().enumerate() {
		let w = weights[i];
		if w == 0.0 {
			continue;
		}
		let px = unsafe { ctx.world.read_edge(x + ox, y + oy, ctx.replicate) };
		let a = px.alpha as f64 / 255.0;
		num += w * extract(px) * a;
		den += w * a;
	}
	if den.abs() > 1e-9 { num / den } else { 0.0 }
}

/// Convolves `src`'s `area` with the given per-channel kernels, writing the
/// result to `dst`. A `null` channel kernel leaves that channel unmodified.
/// `src` and `dst` must be different buffers (a sliding-window filter can't
/// safely run in place, and parallelising by row assumes disjoint reads).
#[allow(non_upper_case_globals)]
unsafe extern "C" fn convolve_sys(
	_effect_ref: PF_ProgPtr,
	src: *mut PF_EffectWorld,
	area: *const PF_Rect,
	flags: PF_KernelFlags,
	kernel_size: A_long,
	a_kernel: *mut c_void,
	r_kernel: *mut c_void,
	g_kernel: *mut c_void,
	b_kernel: *mut c_void,
	dst: *mut PF_EffectWorld,
) -> PF_Err {
	if src.is_null() || dst.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}
	if std::ptr::eq(src, dst) {
		log::error!("PF_WorldTransformSuite1::convolve: src and dst must be different buffers");
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}
	let s = unsafe { &*src };
	let d = unsafe { &*dst };
	if s.data.is_null() || d.data.is_null() || kernel_size <= 0 {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	let rect = if !area.is_null() {
		unsafe { *area }
	} else {
		PF_Rect {
			left: 0,
			top: 0,
			right: s.width,
			bottom: s.height,
		}
	};
	let left = rect.left.max(0);
	let top = rect.top.max(0);
	let width = (rect.right.min(s.width).min(d.width) - left).max(0);
	let height = (rect.bottom.min(s.height).min(d.height) - top).max(0);
	if width == 0 || height == 0 {
		return PF_Err_NONE as PF_Err;
	}

	let is_1d = flags & PF_KernelFlag_1D != 0;
	let vertical = flags & PF_KernelFlag_VERTICAL != 0;
	let normalized = flags & PF_KernelFlag_NORMALIZED != 0;
	let no_clamp = flags & PF_KernelFlag_NO_CLAMP != 0;
	let replicate = flags & PF_KernelFlag_REPLICATE_BORDERS != 0;
	let alpha_weighted = flags & PF_KernelFlag_ALPHA_WEIGHT_CONVOLVE != 0;
	let kernel_type = flags & (PF_KernelFlag_USE_CHAR | PF_KernelFlag_USE_FIXED);

	let taps: Vec<(i32, i32)> = if is_1d {
		(0..kernel_size)
			.map(|i| {
				let offset = i - kernel_size / 2;
				if vertical { (0, offset) } else { (offset, 0) }
			})
			.collect()
	} else {
		let mut v = Vec::with_capacity((kernel_size * kernel_size).max(0) as usize);
		for ky in 0..kernel_size {
			for kx in 0..kernel_size {
				v.push((kx - kernel_size / 2, ky - kernel_size / 2));
			}
		}
		v
	};

	struct Channel {
		weights: Vec<f64>,
		sum: f64,
	}
	let build_channel = |ptr: *mut c_void| -> Option<Channel> {
		if ptr.is_null() {
			return None;
		}
		let weights: Vec<f64> = (0..taps.len())
			.map(|i| match kernel_type {
				PF_KernelFlag_USE_CHAR => (unsafe { *(ptr as *const i8).add(i) }) as f64,
				PF_KernelFlag_USE_FIXED => (unsafe { *(ptr as *const PF_Fixed).add(i) }) as f64 / 65536.0,
				_ => (unsafe { *(ptr as *const A_long).add(i) }) as f64,
			})
			.collect();
		let sum = weights.iter().sum();
		Some(Channel { weights, sum })
	};

	let ch_a = build_channel(a_kernel);
	let ch_r = build_channel(r_kernel);
	let ch_g = build_channel(g_kernel);
	let ch_b = build_channel(b_kernel);

	let src_raw = RawWorld::from(s);
	let dst_raw = RawWorld::from(d);
	let ctx = ConvolveCtx {
		world: src_raw,
		taps: &taps[..],
		replicate,
		normalized,
	};

	(0..height).into_par_iter().for_each(|row| {
		let y = top + row;
		for col in 0..width {
			let x = left + col;
			let here = unsafe { src_raw.read(x, y) };

			let conv = |ch: &Option<Channel>, extract: fn(PF_Pixel8) -> f64, fallback: u8| -> u8 {
				match ch {
					Some(ch) => {
						let v = if alpha_weighted {
							convolve_value_alpha_weighted(ctx, x, y, &ch.weights, extract)
						} else {
							convolve_value(ctx, x, y, &ch.weights, ch.sum, extract)
						};
						quantize(v, no_clamp)
					}
					None => fallback,
				}
			};

			let out = PF_Pixel8 {
				alpha: conv(&ch_a, |p| p.alpha as f64, here.alpha),
				red: conv(&ch_r, |p| p.red as f64, here.red),
				green: conv(&ch_g, |p| p.green as f64, here.green),
				blue: conv(&ch_b, |p| p.blue as f64, here.blue),
			};
			unsafe { dst_raw.write(x, y, out) };
		}
	});

	PF_Err_NONE as PF_Err
}

/// Like [`Copy_sys`], but resamples with bilinear filtering, so `src_r` and
/// `dst_r` may have different sizes (a scaled blit) rather than requiring an
/// exact 1:1 pixel match.
unsafe extern "C" fn copy_hq_sys(
	_effect_ref: PF_ProgPtr,
	src: *mut PF_EffectWorld,
	dst: *mut PF_EffectWorld,
	src_r: *mut PF_Rect,
	dst_r: *mut PF_Rect,
) -> PF_Err {
	if src.is_null() || dst.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}
	let s = unsafe { &*src };
	let d = unsafe { &*dst };
	if s.data.is_null() || d.data.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	let src_rect = if !src_r.is_null() {
		unsafe { *src_r }
	} else {
		PF_Rect {
			left: 0,
			top: 0,
			right: s.width,
			bottom: s.height,
		}
	};
	let dst_rect = if !dst_r.is_null() {
		unsafe { *dst_r }
	} else {
		PF_Rect {
			left: 0,
			top: 0,
			right: d.width,
			bottom: d.height,
		}
	};

	let src_w = (src_rect.right - src_rect.left).max(0);
	let src_h = (src_rect.bottom - src_rect.top).max(0);
	let dst_left = dst_rect.left.max(0);
	let dst_top = dst_rect.top.max(0);
	let dst_w = (dst_rect.right.min(d.width) - dst_left).max(0);
	let dst_h = (dst_rect.bottom.min(d.height) - dst_top).max(0);
	if src_w == 0 || src_h == 0 || dst_w == 0 || dst_h == 0 {
		return PF_Err_NONE as PF_Err;
	}

	let scale_x = src_w as f64 / dst_w as f64;
	let scale_y = src_h as f64 / dst_h as f64;
	let src_raw = RawWorld::from(s);
	let dst_raw = RawWorld::from(d);

	(0..dst_h).into_par_iter().for_each(|row| {
		let dy = dst_top + row;
		let sy = src_rect.top as f64 + (row as f64 + 0.5) * scale_y - 0.5;
		for col in 0..dst_w {
			let dx = dst_left + col;
			let sx = src_rect.left as f64 + (col as f64 + 0.5) * scale_x - 0.5;
			let (a, r, g, b) = unsafe { src_raw.sample_bilinear(sx, sy) };
			let to_u8 = |v: f64| v.round().clamp(0.0, 255.0) as u8;
			let px = PF_Pixel8 {
				alpha: to_u8(a),
				red: to_u8(r),
				green: to_u8(g),
				blue: to_u8(b),
			};
			unsafe { dst_raw.write(dx, dy, px) };
		}
	});

	PF_Err_NONE as PF_Err
}

/// `composite_rect`'s more capable sibling: takes a structured
/// `PF_CompositeMode` (opacity + `rgb_only`) and an optional mask, and
/// respects `m_flags` to unpremultiply `src_world` if needed.
///
/// `comp_mode.rand_seed` and `opacitySu` aren't consulted (dissolve dithering
/// and sub-percent opacity precision aren't modeled); `quality` is accepted
/// but doesn't change behavior, since this emulator has no separate
/// low-quality fast path.
unsafe extern "C" fn transfer_rect_sys(
	_effect_ref: PF_ProgPtr,
	_quality: PF_Quality,
	m_flags: PF_ModeFlags,
	field: PF_Field,
	src_rec: *const PF_Rect,
	src_world: *const PF_EffectWorld,
	comp_mode: *const PF_CompositeMode,
	mask_world0: *const PF_MaskWorld,
	dest_x: A_long,
	dest_y: A_long,
	dst_world: *mut PF_EffectWorld,
) -> PF_Err {
	if src_world.is_null() || dst_world.is_null() || comp_mode.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}
	let src = unsafe { &*src_world };
	let dst = unsafe { &*dst_world };
	if src.data.is_null() || dst.data.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}
	let mode = unsafe { &*comp_mode };

	let rect = if !src_rec.is_null() {
		unsafe { *src_rec }
	} else {
		PF_Rect {
			left: 0,
			top: 0,
			right: src.width,
			bottom: src.height,
		}
	};
	let left = rect.left.max(0);
	let top = rect.top.max(0);
	let width = (rect.right.min(src.width) - left).max(0);
	let height = (rect.bottom.min(src.height) - top).max(0);
	if width == 0 || height == 0 {
		return PF_Err_NONE as PF_Err;
	}

	let opacity = mode.opacity as f64 / 255.0;
	// `PF_MF_Alpha_STRAIGHT` is `u32` on macOS, `i32` elsewhere; the cast is
	// redundant here but required there.
	#[allow(clippy::unnecessary_cast)]
	let premultiplied = m_flags & PF_MF_Alpha_STRAIGHT as i32 == 0;
	let rgb_only = mode.rgb_only != 0;
	let xfer = mode.xfer;

	let src_raw = RawWorld::from(src);
	let dst_raw = RawWorld::from(dst);
	let mask_raw = unsafe { mask_world0.as_ref() }.map(|m| (RawWorld::from(&m.mask), m.offset, m.what_is_mask));

	(0..height).into_par_iter().for_each(|row| {
		let dy = dest_y + row;
		if skip_row_for_field(dy, field) {
			return;
		}
		let sy = top + row;
		for col in 0..width {
			let dx = dest_x + col;
			let sx = left + col;
			let sp = unsafe { src_raw.read(sx, sy) };
			let (mut src_a, src_c) = to_straight(sp, premultiplied);
			src_a *= opacity;
			if let Some((mraw, offset, what)) = mask_raw {
				src_a *= mask_value(mraw, offset, what, dx, dy);
			}
			let dp = unsafe { dst_raw.read(dx, dy) };
			let (dst_a, dst_c) = to_straight(dp, false);
			let (out_a, out_c) = composite_pixel(src_a, src_c, dst_a, dst_c, xfer);
			let final_a = if rgb_only { dst_a } else { out_a };
			unsafe { dst_raw.write(dx, dy, from_straight(final_a, out_c)) };
		}
	});

	PF_Err_NONE as PF_Err
}

/// Applies one (or, for motion blur, several averaged) affine transform(s)
/// to `src_world`, compositing the result into `dst_world`'s `dest_rect`.
///
/// `matrices` map source space to destination space when `src2dst_matrix` is
/// set (so each destination pixel is resampled through the inverse) and
/// destination-to-source otherwise. Multiple matrices are premultiply-
/// averaged (each tap weighted by its own alpha, then unpremultiplied) to
/// approximate motion blur without fringing at transparent edges.
unsafe extern "C" fn transform_world_sys(
	_effect_ref: PF_ProgPtr,
	_quality: PF_Quality,
	m_flags: PF_ModeFlags,
	field: PF_Field,
	src_world: *const PF_EffectWorld,
	comp_mode: *const PF_CompositeMode,
	mask_world0: *const PF_MaskWorld,
	matrices: *const PF_FloatMatrix,
	num_matrices: A_long,
	src2dst_matrix: PF_Boolean,
	dest_rect: *const PF_Rect,
	dst_world: *mut PF_EffectWorld,
) -> PF_Err {
	if src_world.is_null() || dst_world.is_null() || comp_mode.is_null() || matrices.is_null() || num_matrices <= 0 {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}
	let src = unsafe { &*src_world };
	let dst = unsafe { &*dst_world };
	if src.data.is_null() || dst.data.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}
	let mode = unsafe { &*comp_mode };

	let rect = if !dest_rect.is_null() {
		unsafe { *dest_rect }
	} else {
		PF_Rect {
			left: 0,
			top: 0,
			right: dst.width,
			bottom: dst.height,
		}
	};
	let left = rect.left.max(0);
	let top = rect.top.max(0);
	let width = (rect.right.min(dst.width) - left).max(0);
	let height = (rect.bottom.min(dst.height) - top).max(0);
	if width == 0 || height == 0 {
		return PF_Err_NONE as PF_Err;
	}

	// Pre-invert into dst->src matrices up front so the per-pixel loop only
	// ever has to forward-map.
	let mats: Vec<Mat3> = unsafe { std::slice::from_raw_parts(matrices, num_matrices as usize) }
		.iter()
		.map(|m| {
			if src2dst_matrix != 0 {
				invert3(m.mat).unwrap_or(IDENTITY3)
			} else {
				m.mat
			}
		})
		.collect();

	let opacity = mode.opacity as f64 / 255.0;
	// `PF_MF_Alpha_STRAIGHT` is `u32` on macOS, `i32` elsewhere; the cast is
	// redundant here but required there.
	#[allow(clippy::unnecessary_cast)]
	let premultiplied = m_flags & PF_MF_Alpha_STRAIGHT as i32 == 0;
	let rgb_only = mode.rgb_only != 0;
	let xfer = mode.xfer;

	let src_raw = RawWorld::from(src);
	let dst_raw = RawWorld::from(dst);
	let mask_raw = unsafe { mask_world0.as_ref() }.map(|m| (RawWorld::from(&m.mask), m.offset, m.what_is_mask));
	let mats = &mats[..];

	(0..height).into_par_iter().for_each(|row| {
		let dy = top + row;
		if skip_row_for_field(dy, field) {
			return;
		}
		for col in 0..width {
			let dx = left + col;

			let mut acc_a = 0.0;
			let mut acc_c = [0.0; 3];
			for mat in mats {
				let (sx, sy) = apply_matrix(mat, dx as f64 + 0.5, dy as f64 + 0.5);
				let (a, r, g, b) = unsafe { src_raw.sample_bilinear(sx - 0.5, sy - 0.5) };
				let a_n = a / 255.0;
				let (rp, gp, bp) = if premultiplied {
					(r / 255.0, g / 255.0, b / 255.0)
				} else {
					(r / 255.0 * a_n, g / 255.0 * a_n, b / 255.0 * a_n)
				};
				acc_a += a_n;
				acc_c[0] += rp;
				acc_c[1] += gp;
				acc_c[2] += bp;
			}
			let mut src_a = acc_a / mats.len() as f64;
			let src_c = if acc_a > 1e-9 {
				[acc_c[0] / acc_a, acc_c[1] / acc_a, acc_c[2] / acc_a]
			} else {
				[0.0; 3]
			};
			src_a *= opacity;
			if let Some((mraw, offset, what)) = mask_raw {
				src_a *= mask_value(mraw, offset, what, dx, dy);
			}

			let dp = unsafe { dst_raw.read(dx, dy) };
			let (dst_a, dst_c) = to_straight(dp, false);
			let (out_a, out_c) = composite_pixel(src_a, src_c, dst_a, dst_c, xfer);
			let final_a = if rgb_only { dst_a } else { out_a };
			unsafe { dst_raw.write(dx, dy, from_straight(final_a, out_c)) };
		}
	});

	PF_Err_NONE as PF_Err
}

// ============================================================================
// Factory Function
// ============================================================================

/// Builds the `PF_WorldTransformSuite1` vtable.
///
/// `const` so it can initialize the shared [`SUITE_CONTAINER`](crate::suites::SUITE_CONTAINER)
/// static; the suite is a stateless table of function pointers.
pub const fn create_world_transform_suite_1() -> PF_WorldTransformSuite1 {
	PF_WorldTransformSuite1 {
		composite_rect: Some(composite_rect_sys),
		blend: Some(blend_sys),
		convolve: Some(convolve_sys),
		copy: Some(Copy_sys),
		copy_hq: Some(copy_hq_sys),
		transfer_rect: Some(transfer_rect_sys),
		transform_world: Some(transform_world_sys),
	}
}

#[cfg(test)]
// PF_* constants are `u32` on macOS, `i32` elsewhere; `as i32` in these tests is
// redundant here but required there.
#[allow(clippy::unnecessary_cast)]
mod tests {
	use super::*;

	/// An owned, heap-backed `PF_EffectWorld` for exercising suite entry
	/// points without a real host. `world.data` points into `buf`, which is
	/// why `buf` must outlive every raw-pointer read/write into `world`.
	struct TestWorld {
		buf: Vec<u8>,
		world: PF_EffectWorld,
	}

	impl TestWorld {
		fn new(width: i32, height: i32, fill: PF_Pixel8) -> Self {
			let rowbytes = width * PIXEL_SIZE as i32;
			let mut buf = vec![0u8; (rowbytes * height).max(0) as usize];
			for chunk in buf.chunks_exact_mut(PIXEL_SIZE as usize) {
				chunk[0] = fill.alpha;
				chunk[1] = fill.red;
				chunk[2] = fill.green;
				chunk[3] = fill.blue;
			}
			let world = PF_EffectWorld {
				reserved0: std::ptr::null_mut(),
				reserved1: std::ptr::null_mut(),
				world_flags: 0,
				data: buf.as_mut_ptr() as PF_PixelPtr,
				rowbytes,
				width,
				height,
				extent_hint: PF_UnionableRect {
					left: 0,
					top: 0,
					right: width,
					bottom: height,
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
			Self { buf, world }
		}

		fn pixel(&self, x: i32, y: i32) -> PF_Pixel8 {
			let idx = (y * self.world.rowbytes + x * PIXEL_SIZE as i32) as usize;
			PF_Pixel8 {
				alpha: self.buf[idx],
				red: self.buf[idx + 1],
				green: self.buf[idx + 2],
				blue: self.buf[idx + 3],
			}
		}

		fn as_mut_ptr(&mut self) -> *mut PF_EffectWorld {
			&mut self.world
		}
	}

	fn px(a: u8, r: u8, g: u8, b: u8) -> PF_Pixel8 {
		PF_Pixel8 {
			alpha: a,
			red: r,
			green: g,
			blue: b,
		}
	}

	/// `PF_Pixel` (from `after-effects-sys`) doesn't derive `PartialEq`, so
	/// tests compare fields through this helper instead.
	#[track_caller]
	fn assert_px_eq(actual: PF_Pixel8, expected: PF_Pixel8, msg: &str) {
		assert_eq!(
			(actual.alpha, actual.red, actual.green, actual.blue),
			(expected.alpha, expected.red, expected.green, expected.blue),
			"{msg}"
		);
	}

	#[test]
	fn blend_at_endpoints_and_midpoint() {
		let mut s1 = TestWorld::new(2, 2, px(255, 100, 0, 0));
		let mut s2 = TestWorld::new(2, 2, px(255, 0, 200, 0));
		let mut d = TestWorld::new(2, 2, px(0, 0, 0, 0));

		unsafe {
			blend_sys(
				std::ptr::null_mut(),
				s1.as_mut_ptr(),
				s2.as_mut_ptr(),
				0,
				d.as_mut_ptr(),
			)
		};
		assert_px_eq(d.pixel(0, 0), px(255, 100, 0, 0), "ratio=0 should equal src1");

		unsafe {
			blend_sys(
				std::ptr::null_mut(),
				s1.as_mut_ptr(),
				s2.as_mut_ptr(),
				1 << 16,
				d.as_mut_ptr(),
			)
		};
		assert_px_eq(d.pixel(0, 0), px(255, 0, 200, 0), "ratio=1 should equal src2");

		unsafe {
			blend_sys(
				std::ptr::null_mut(),
				s1.as_mut_ptr(),
				s2.as_mut_ptr(),
				1 << 15,
				d.as_mut_ptr(),
			)
		};
		assert_px_eq(d.pixel(0, 0), px(255, 50, 100, 0), "ratio=0.5 should be the midpoint");
	}

	#[test]
	fn composite_rect_copy_opaque_src_replaces_dst() {
		let mut src = TestWorld::new(2, 2, px(255, 10, 20, 30));
		let mut dst = TestWorld::new(2, 2, px(255, 200, 200, 200));

		let err = unsafe {
			composite_rect_sys(
				std::ptr::null_mut(),
				std::ptr::null_mut(),
				255,
				src.as_mut_ptr(),
				0,
				0,
				PF_Field_FRAME as i32,
				PF_Xfer_COPY,
				dst.as_mut_ptr(),
			)
		};
		assert_eq!(err, PF_Err_NONE as PF_Err);
		assert_px_eq(
			dst.pixel(0, 0),
			px(255, 10, 20, 30),
			"opaque COPY should fully replace dst",
		);
	}

	#[test]
	fn composite_rect_zero_opacity_leaves_dst_unchanged() {
		let mut src = TestWorld::new(2, 2, px(255, 10, 20, 30));
		let mut dst = TestWorld::new(2, 2, px(255, 200, 200, 200));

		unsafe {
			composite_rect_sys(
				std::ptr::null_mut(),
				std::ptr::null_mut(),
				0,
				src.as_mut_ptr(),
				0,
				0,
				PF_Field_FRAME as i32,
				PF_Xfer_COPY,
				dst.as_mut_ptr(),
			)
		};
		assert_px_eq(
			dst.pixel(0, 0),
			px(255, 200, 200, 200),
			"zero opacity should leave dst untouched",
		);
	}

	#[test]
	fn copy_hq_same_size_matches_source() {
		let mut src = TestWorld::new(3, 3, px(255, 42, 84, 126));
		let mut dst = TestWorld::new(3, 3, px(0, 0, 0, 0));

		unsafe {
			copy_hq_sys(
				std::ptr::null_mut(),
				src.as_mut_ptr(),
				dst.as_mut_ptr(),
				std::ptr::null_mut(),
				std::ptr::null_mut(),
			)
		};

		for y in 0..3 {
			for x in 0..3 {
				assert_px_eq(dst.pixel(x, y), px(255, 42, 84, 126), &format!("mismatch at ({x},{y})"));
			}
		}
	}

	#[test]
	fn convolve_identity_kernel_is_a_no_op() {
		let mut src = TestWorld::new(3, 3, px(255, 10, 20, 30));
		let mut dst = TestWorld::new(3, 3, px(0, 0, 0, 0));

		// A 3x3 kernel with a single 1 at the center and 0 everywhere else
		// convolves to exactly the source value at every pixel.
		let mut kernel = [0i32; 9];
		kernel[4] = 1;

		let err = unsafe {
			convolve_sys(
				std::ptr::null_mut(),
				src.as_mut_ptr(),
				std::ptr::null(),
				PF_KernelFlag_NORMALIZED,
				3,
				kernel.as_mut_ptr() as *mut c_void,
				kernel.as_mut_ptr() as *mut c_void,
				kernel.as_mut_ptr() as *mut c_void,
				kernel.as_mut_ptr() as *mut c_void,
				dst.as_mut_ptr(),
			)
		};
		assert_eq!(err, PF_Err_NONE as PF_Err);
		for y in 0..3 {
			for x in 0..3 {
				assert_px_eq(dst.pixel(x, y), px(255, 10, 20, 30), &format!("mismatch at ({x},{y})"));
			}
		}
	}

	#[test]
	fn convolve_rejects_aliased_buffers() {
		let mut w = TestWorld::new(2, 2, px(255, 1, 2, 3));
		let ptr = w.as_mut_ptr();
		let kernel = [1i32];
		let err = unsafe {
			convolve_sys(
				std::ptr::null_mut(),
				ptr,
				std::ptr::null(),
				PF_KernelFlag_NORMALIZED,
				1,
				std::ptr::null_mut(),
				std::ptr::null_mut(),
				std::ptr::null_mut(),
				kernel.as_ptr() as *mut c_void,
				ptr,
			)
		};
		assert_eq!(err, PF_Err_BAD_CALLBACK_PARAM as PF_Err);
	}

	#[test]
	fn transform_world_identity_matrix_is_a_no_op() {
		let mut src = TestWorld::new(4, 4, px(255, 5, 6, 7));
		let mut dst = TestWorld::new(4, 4, px(0, 0, 0, 0));
		let comp_mode = PF_CompositeMode {
			xfer: PF_Xfer_COPY,
			rand_seed: 0,
			opacity: 255,
			rgb_only: 0,
			opacitySu: 0,
		};
		let matrix = PF_FloatMatrix { mat: IDENTITY3 };

		let err = unsafe {
			transform_world_sys(
				std::ptr::null_mut(),
				PF_Quality_HI,
				PF_MF_Alpha_STRAIGHT as i32,
				PF_Field_FRAME as i32,
				src.as_mut_ptr(),
				&comp_mode as *const _,
				std::ptr::null(),
				&matrix as *const _,
				1,
				0,
				std::ptr::null(),
				dst.as_mut_ptr(),
			)
		};
		assert_eq!(err, PF_Err_NONE as PF_Err);
		for y in 1..3 {
			for x in 1..3 {
				assert_px_eq(dst.pixel(x, y), px(255, 5, 6, 7), &format!("mismatch at ({x},{y})"));
			}
		}
	}
}
