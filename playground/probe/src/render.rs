//! Parameter registration, the legacy-render implementation, and the test
//! pattern both render paths write.
//!
//! The render output is a deterministic test pattern driven by the params, so
//! host-side parameter plumbing is visible both in the trace (exact values)
//! and on screen (the picture changes when a control changes).

use after_effects::sys as ae;
use serde_json::json;

use crate::inspect::{self, fixed_to_f64};
use crate::trace::trace;

// Parameter indices (0 is the implicit input layer).
pub const PARAM_GAIN: usize = 1;
pub const PARAM_INVERT: usize = 2;
pub const PARAM_MODE: usize = 3;
pub const PARAM_TINT: usize = 4;
pub const PARAM_ANGLE: usize = 5;
pub const PARAM_CENTER: usize = 6;
pub const PARAM_COUNT: usize = 7;

pub const MODE_GRADIENT: i32 = 1;
pub const MODE_CHECKER: i32 = 2;
pub const MODE_COPY_INPUT: i32 = 3;

/// Register one control of every common type, logging each `add_param` result.
pub unsafe fn setup_params(in_data: *mut ae::PF_InData, out_data: *mut ae::PF_OutData) -> ae::PF_Err {
	let Some(add_param) = (unsafe { (*in_data).inter.add_param }) else {
		trace().emit(
			"note",
			json!({ "msg": "inter.add_param is null; cannot register params" }),
		);
		return ae::PF_Err_BAD_CALLBACK_PARAM as ae::PF_Err;
	};
	let effect_ref = unsafe { (*in_data).effect_ref };

	let register = |name: &str, index: usize, fill: &dyn Fn(&mut ae::PF_ParamDef)| {
		let mut def: ae::PF_ParamDef = unsafe { std::mem::zeroed() };
		def.uu.id = index as ae::A_long;

		let bytes = name.as_bytes();
		for (i, &b) in bytes.iter().take(31).enumerate() {
			def.name_do_not_use_directly[i] = b as i8;
		}

		fill(&mut def);

		// -1 appends, exactly like the SDK's PF_ADD_PARAM convention.
		let err = unsafe { add_param(effect_ref, -1, &mut def) };
		trace().emit("add_param", json!({ "index": index, "name": name, "err": err }));
	};

	register("Gain", PARAM_GAIN, &|def| {
		def.param_type = ae::PF_Param_FLOAT_SLIDER;
		def.u.fs_d = ae::PF_FloatSliderDef {
			value: 1.0,
			phase: 0.0,
			value_desc: [0; 32],
			valid_min: 0.0,
			valid_max: 4.0,
			slider_min: 0.0,
			slider_max: 2.0,
			dephault: 1.0,
			precision: 2, // hundredths
			display_flags: 0,
			fs_flags: 0,
			curve_tolerance: 0.0,
			useExponent: 0,
			exponent: 1.0,
		};
	});

	register("Invert", PARAM_INVERT, &|def| {
		def.param_type = ae::PF_Param_CHECKBOX;
		def.u.bd.dephault = 0;
		def.u.bd.value = 0;
		def.u.bd.u.nameptr = c"On".as_ptr();
	});

	register("Mode", PARAM_MODE, &|def| {
		def.param_type = ae::PF_Param_POPUP;
		def.u.pd.num_choices = 3;
		def.u.pd.dephault = MODE_GRADIENT as ae::A_short;
		def.u.pd.value = MODE_GRADIENT;
		def.u.pd.u.namesptr = c"Gradient|Checker|Copy Input".as_ptr();
	});

	register("Tint", PARAM_TINT, &|def| {
		def.param_type = ae::PF_Param_COLOR;
		let white = ae::PF_Pixel {
			alpha: 255,
			red: 255,
			green: 255,
			blue: 255,
		};
		def.u.cd.value = white;
		def.u.cd.dephault = white;
	});

	register("Angle", PARAM_ANGLE, &|def| {
		def.param_type = ae::PF_Param_ANGLE;
		def.u.ad.value = 0;
		def.u.ad.dephault = 0;
		def.u.ad.valid_min = ae::A_long::MIN;
		def.u.ad.valid_max = ae::A_long::MAX;
	});

	register("Center", PARAM_CENTER, &|def| {
		def.param_type = ae::PF_Param_POINT;
		// Point dephaults are percentages in fixed point: 50% = center.
		def.u.td.x_dephault = 50 << 16;
		def.u.td.y_dephault = 50 << 16;
		def.u.td.x_value = 0;
		def.u.td.y_value = 0;
		def.u.td.restrict_bounds = 0;
	});

	unsafe { (*out_data).num_params = PARAM_COUNT as ae::A_long };
	ae::PF_Err_NONE as ae::PF_Err
}

pub(crate) struct Settings {
	gain: f64,
	invert: bool,
	mode: i32,
	tint: ae::PF_Pixel,
	angle_deg: f64,
	center: (f64, f64),
}

pub(crate) unsafe fn param_at(params: ae::PF_ParamList, index: usize) -> Option<*const ae::PF_ParamDef> {
	if params.is_null() {
		return None;
	}
	let ptr = unsafe { *params.add(index) };
	(!ptr.is_null()).then_some(ptr as *const _)
}

pub(crate) unsafe fn read_settings(params: ae::PF_ParamList, num_params: usize) -> Settings {
	let mut settings = Settings {
		gain: 1.0,
		invert: false,
		mode: MODE_GRADIENT,
		tint: ae::PF_Pixel {
			alpha: 255,
			red: 255,
			green: 255,
			blue: 255,
		},
		angle_deg: 0.0,
		center: (0.0, 0.0),
	};

	unsafe {
		if num_params > PARAM_GAIN
			&& let Some(p) = param_at(params, PARAM_GAIN)
		{
			settings.gain = (*p).u.fs_d.value;
		}
		if num_params > PARAM_INVERT
			&& let Some(p) = param_at(params, PARAM_INVERT)
		{
			settings.invert = (*p).u.bd.value != 0;
		}
		if num_params > PARAM_MODE
			&& let Some(p) = param_at(params, PARAM_MODE)
		{
			settings.mode = (*p).u.pd.value;
		}
		if num_params > PARAM_TINT
			&& let Some(p) = param_at(params, PARAM_TINT)
		{
			settings.tint = (*p).u.cd.value;
		}
		if num_params > PARAM_ANGLE
			&& let Some(p) = param_at(params, PARAM_ANGLE)
		{
			settings.angle_deg = fixed_to_f64((*p).u.ad.value);
		}
		if num_params > PARAM_CENTER
			&& let Some(p) = param_at(params, PARAM_CENTER)
		{
			settings.center = (fixed_to_f64((*p).u.td.x_value), fixed_to_f64((*p).u.td.y_value));
		}
	}

	settings
}

/// Legacy `PF_Cmd_RENDER`: log every param value and both worlds (with pixel
/// hashes), then write the test pattern into the output world.
pub unsafe fn render(
	_in_data: *mut ae::PF_InData,
	params: ae::PF_ParamList,
	output: *mut ae::PF_LayerDef,
) -> ae::PF_Err {
	// After Effects fills InData.num_params only at PARAMS_SETUP and sends 0
	// in RENDER and SMART_RENDER, so the declared count is the only source
	// that describes the parameter list on both hosts.
	let num_params = PARAM_COUNT;

	for index in 0..num_params {
		let def = unsafe { param_at(params, index) }.unwrap_or(std::ptr::null());
		trace().emit("param", unsafe { inspect::snapshot_param(index, def) });
	}

	let input = unsafe { param_at(params, 0) }
		.map(|p| unsafe { &raw const (*p).u.ld })
		.unwrap_or(std::ptr::null());

	trace().emit(
		"world",
		json!({ "which": "input", "world": unsafe { inspect::snapshot_world(input, true) } }),
	);

	if output.is_null() {
		trace().emit("note", json!({ "msg": "output world is null; nothing to render" }));
		return ae::PF_Err_NONE as ae::PF_Err;
	}

	let settings = unsafe { read_settings(params, num_params) };
	// The legacy path has no PF_GetPixelFormat answer to go by; the deep flag
	// is all a legacy world says about its depth.
	let in_bpc = unsafe { input.as_ref() }.map_or(Bpc::Eight, Bpc::from_world_flags);
	let out_bpc = Bpc::from_world_flags(unsafe { &*output });
	unsafe { fill_pattern(output, out_bpc, input, in_bpc, &settings) };

	trace().emit(
		"world",
		json!({ "which": "output", "world": unsafe { inspect::snapshot_world(output, true) } }),
	);

	ae::PF_Err_NONE as ae::PF_Err
}

/// Channel depth of a world the pattern reads or writes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Bpc {
	Eight,
	Sixteen,
	Float,
}

impl Bpc {
	/// From `PF_WorldFlag_DEEP` alone, which cannot tell 32 bpc apart.
	pub(crate) fn from_world_flags(world: &ae::PF_LayerDef) -> Self {
		if world.world_flags & ae::PF_WorldFlag_DEEP as ae::PF_WorldFlags != 0 {
			Bpc::Sixteen
		} else {
			Bpc::Eight
		}
	}

	/// From a `PF_GetPixelFormat` answer; `None` for formats the pattern does
	/// not write (GPU and video formats).
	#[allow(non_upper_case_globals)]
	pub(crate) fn from_pixel_format(format: ae::PF_PixelFormat) -> Option<Self> {
		match format as _ {
			ae::PF_PixelFormat_ARGB32 => Some(Bpc::Eight),
			ae::PF_PixelFormat_ARGB64 => Some(Bpc::Sixteen),
			ae::PF_PixelFormat_ARGB128 => Some(Bpc::Float),
			_ => None,
		}
	}

	pub(crate) fn bytes_per_pixel(self) -> usize {
		match self {
			Bpc::Eight => 4,
			Bpc::Sixteen => 8,
			Bpc::Float => 16,
		}
	}
}

/// Read the pixel at (x, y), clamped to the world bounds. Deep (16-bit) and
/// float input worlds are down-converted so the pattern math stays in one
/// depth.
/// Whether `world` has pixels and each row of `width` pixels at `bpc` fits
/// in `rowbytes`. A host that reports a pixel format the buffer does not
/// have would otherwise make the probe read or write past its end.
pub(crate) unsafe fn rows_fit(world: *const ae::PF_LayerDef, bpc: Bpc) -> bool {
	let Some(w) = (unsafe { world.as_ref() }) else {
		return false;
	};
	!w.data.is_null()
		&& w.width > 0
		&& w.height > 0
		&& w.rowbytes > 0
		&& w.rowbytes as usize >= w.width as usize * bpc.bytes_per_pixel()
}

unsafe fn sample_input(input: *const ae::PF_LayerDef, bpc: Bpc, x: i32, y: i32) -> Option<ae::PF_Pixel> {
	if !unsafe { rows_fit(input, bpc) } {
		return None;
	}
	let w = unsafe { &*input };

	let x = x.clamp(0, w.width - 1) as usize;
	let y = y.clamp(0, w.height - 1) as usize;
	let row = unsafe { (w.data as *const u8).add(y * w.rowbytes as usize) };

	match bpc {
		Bpc::Sixteen => {
			let px = unsafe { &*(row as *const ae::PF_Pixel16).add(x) };
			let to8 = |v: u16| ((v as u32 * 255) / 32768).min(255) as u8;
			Some(ae::PF_Pixel {
				alpha: to8(px.alpha),
				red: to8(px.red),
				green: to8(px.green),
				blue: to8(px.blue),
			})
		}
		Bpc::Float => {
			let px = unsafe { &*(row as *const ae::PF_PixelFloat).add(x) };
			let to8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
			Some(ae::PF_Pixel {
				alpha: to8(px.alpha),
				red: to8(px.red),
				green: to8(px.green),
				blue: to8(px.blue),
			})
		}
		Bpc::Eight => Some(unsafe { *(row as *const ae::PF_Pixel).add(x) }),
	}
}

/// Write the test pattern into `output`. Every depth gets the same 8-bit
/// values, scaled, so the picture is the same at 8, 16 and 32 bpc.
pub(crate) unsafe fn fill_pattern(
	output: *mut ae::PF_LayerDef,
	out_bpc: Bpc,
	input: *const ae::PF_LayerDef,
	in_bpc: Bpc,
	settings: &Settings,
) {
	if !unsafe { rows_fit(output, out_bpc) } {
		trace().emit(
			"note",
			json!({ "msg": "output world has no writable pixels, or rowbytes is short for its pixel format" }),
		);
		return;
	}
	let out = unsafe { &*output };

	let (width, height) = (out.width, out.height);

	for y in 0..height {
		for x in 0..width {
			let base = match settings.mode {
				MODE_CHECKER => {
					let on = ((x / 16) + (y / 16)) % 2 == 0;
					let v = if on { 230 } else { 25 };
					(v, v, v, 255)
				}
				MODE_COPY_INPUT => {
					let px = unsafe { sample_input(input, in_bpc, x, y) }.unwrap_or(ae::PF_Pixel {
						alpha: 255,
						red: 0,
						green: 0,
						blue: 0,
					});
					(px.red as i64, px.green as i64, px.blue as i64, px.alpha as i64)
				}
				_ => {
					// Gradient: red along x, green along y; blue marks the angle
					// param so a control change is visible at a glance.
					let r = (x as i64 * 255) / width.max(1) as i64;
					let g = (y as i64 * 255) / height.max(1) as i64;
					let b = ((settings.angle_deg.rem_euclid(360.0) / 360.0) * 255.0) as i64;
					(r, g, b, 255)
				}
			};

			let apply = |channel: i64| -> u8 {
				let mut value = (channel as f64 * settings.gain).round().clamp(0.0, 255.0) as i64;
				if settings.invert {
					value = 255 - value;
				}
				value.clamp(0, 255) as u8
			};

			// 25% tint blend keeps the underlying pattern recognizable.
			let blend = |value: u8, tint: u8| -> u8 { ((value as u32 * 3 + tint as u32) / 4) as u8 };

			let red = blend(apply(base.0), settings.tint.red);
			let green = blend(apply(base.1), settings.tint.green);
			let blue = blend(apply(base.2), settings.tint.blue);
			let alpha = base.3.clamp(0, 255) as u8;

			unsafe {
				let row = (out.data as *mut u8).add(y as usize * out.rowbytes as usize);
				match out_bpc {
					Bpc::Sixteen => {
						let to16 = |v: u8| ((v as u32 * 32768) / 255) as u16;
						*(row as *mut ae::PF_Pixel16).add(x as usize) = ae::PF_Pixel16 {
							alpha: to16(alpha),
							red: to16(red),
							green: to16(green),
							blue: to16(blue),
						};
					}
					Bpc::Float => {
						let to_float = |v: u8| v as f32 / 255.0;
						*(row as *mut ae::PF_PixelFloat).add(x as usize) = ae::PF_PixelFloat {
							alpha: to_float(alpha),
							red: to_float(red),
							green: to_float(green),
							blue: to_float(blue),
						};
					}
					Bpc::Eight => {
						*(row as *mut ae::PF_Pixel).add(x as usize) = ae::PF_Pixel {
							alpha,
							red,
							green,
							blue,
						};
					}
				}
			}
		}
	}

	let _ = settings.center; // logged via the param snapshot; not used by the pattern
}
