//! An effect that misbehaves on request, to test that aexlo's toolkit catches
//! what it should: crashes, hangs, errors, out-of-bounds writes, leaks,
//! unwritten output and nondeterminism.
//!
//! Written against the raw SDK (no `after-effects` crate) so every bad thing
//! it does is visible here. Parameters:
//!
//! 1. `Mode` popup: what to do wrong while rendering (see [`Mode`]).
//! 2. `Gain` float slider: output = input × gain, so a golden can change.
//! 3. `Delay` float slider: milliseconds to sleep per render, for benches.
//! 4. `Map` layer: unused, except that `ParamLeak` checks it out.
//!
//! Renders through `PF_Cmd_RENDER` and the smart pre-render/render pair, at
//! 8 and 16 bpc. On the smart path it checks its parameters out and in
//! with `PF_CHECKOUT_PARAM`, as After Effects requires there.

#![allow(non_snake_case)]
#![allow(clippy::missing_safety_doc)]

use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};

use after_effects_sys::*;

/// What `Mode` asks the render to do, in popup order (1-based).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
	Ok = 1,
	/// Write through a null pointer.
	Crash,
	/// Loop until the host aborts the render (`PF_ABORT`).
	Hang,
	/// Loop forever, ignoring the host.
	Freeze,
	/// Return `PF_Err_INTERNAL_STRUCT_DAMAGED`.
	Error,
	/// Write one row past the end of the output world.
	Overrun,
	/// Allocate a handle and never dispose of it.
	Leak,
	/// Leave the bottom half of the output unwritten.
	Unwritten,
	/// Add noise that differs on every render.
	Random,
	/// Smart render: check out a layer pre-render never declared.
	Undeclared,
	/// Smart render: check the input layer in twice.
	Unbalanced,
	/// Smart render: check the `Map` layer param out and never back in.
	ParamLeak,
}

const MODE_NAMES: &[u8] = b"OK|Crash|Hang|Freeze|Error|Overrun|Leak|Unwritten|Random|Undeclared|Unbalanced|ParamLeak\0";
const MODE_COUNT: i16 = 12;

const PARAM_MODE: usize = 1;
const PARAM_GAIN: usize = 2;
const PARAM_DELAY: usize = 3;
const PARAM_MAP: usize = 4;

/// Bumped on every render, so `Random` never repeats itself.
static RENDERS: AtomicU32 = AtomicU32::new(0);

fn name32(name: &str) -> [A_char; 32] {
	let mut out = [0 as A_char; 32];
	for (dst, src) in out.iter_mut().zip(name.bytes().take(31)) {
		*dst = src as A_char;
	}
	out
}

unsafe fn add_param(in_data: *mut PF_InData, mut def: PF_ParamDef) -> PF_Err {
	let in_data = unsafe { &*in_data };
	match in_data.inter.add_param {
		Some(add) => unsafe { add(in_data.effect_ref, -1, &mut def) },
		None => PF_Err_BAD_CALLBACK_PARAM as PF_Err,
	}
}

unsafe fn params_setup(in_data: *mut PF_InData, out_data: *mut PF_OutData) -> PF_Err {
	let mut mode: PF_ParamDef = unsafe { std::mem::zeroed() };
	mode.param_type = PF_Param_POPUP as PF_ParamType;
	mode.name_do_not_use_directly = name32("Mode");
	mode.uu.id = 1;
	mode.u.pd = PF_PopupDef {
		value: 1,
		num_choices: MODE_COUNT,
		dephault: 1,
		u: PF_PopupDef__bindgen_ty_1 {
			namesptr: MODE_NAMES.as_ptr() as *const A_char,
		},
	};

	let mut gain: PF_ParamDef = unsafe { std::mem::zeroed() };
	gain.param_type = PF_Param_FLOAT_SLIDER as PF_ParamType;
	gain.name_do_not_use_directly = name32("Gain");
	gain.uu.id = 2;
	gain.u.fs_d.value = 1.0;
	gain.u.fs_d.dephault = 1.0;
	gain.u.fs_d.valid_min = 0.0;
	gain.u.fs_d.valid_max = 4.0;
	gain.u.fs_d.slider_min = 0.0;
	gain.u.fs_d.slider_max = 4.0;
	gain.u.fs_d.precision = 2;

	let mut delay: PF_ParamDef = unsafe { std::mem::zeroed() };
	delay.param_type = PF_Param_FLOAT_SLIDER as PF_ParamType;
	delay.name_do_not_use_directly = name32("Delay");
	delay.uu.id = 3;
	delay.u.fs_d.valid_min = 0.0;
	delay.u.fs_d.valid_max = 1000.0;
	delay.u.fs_d.slider_min = 0.0;
	delay.u.fs_d.slider_max = 100.0;

	let mut map: PF_ParamDef = unsafe { std::mem::zeroed() };
	map.param_type = PF_Param_LAYER as PF_ParamType;
	map.name_do_not_use_directly = name32("Map");
	map.uu.id = 4;
	map.u.ld.dephault = PF_LayerDefault_NONE as A_long;

	for def in [mode, gain, delay, map] {
		let err = unsafe { add_param(in_data, def) };
		if err != PF_Err_NONE as PF_Err {
			return err;
		}
	}
	unsafe { (*out_data).num_params = 5 };
	PF_Err_NONE as PF_Err
}

fn mode_of(value: i32) -> Mode {
	match value {
		2 => Mode::Crash,
		3 => Mode::Hang,
		4 => Mode::Freeze,
		5 => Mode::Error,
		6 => Mode::Overrun,
		7 => Mode::Leak,
		8 => Mode::Unwritten,
		9 => Mode::Random,
		10 => Mode::Undeclared,
		11 => Mode::Unbalanced,
		12 => Mode::ParamLeak,
		_ => Mode::Ok,
	}
}

/// A cheap integer hash for the `Random` noise.
fn hash(mut x: u32) -> u32 {
	x ^= x >> 16;
	x = x.wrapping_mul(0x7feb_352d);
	x ^= x >> 15;
	x = x.wrapping_mul(0x846c_a68b);
	x ^ (x >> 16)
}

/// Copy `input` into `output` times `gain`, for rows `0..rows`, at 8 or 16 bpc.
unsafe fn shade(input: &PF_LayerDef, output: &PF_LayerDef, rows: i32, gain: f64, noise: Option<u32>) {
	let deep = output.world_flags & PF_WorldFlag_DEEP as PF_WorldFlags != 0;
	let (max, bytes) = if deep { (32768.0, 8) } else { (255.0, 4) };
	let width = output.width.min(input.width);
	for y in 0..rows.min(input.height) {
		for x in 0..width {
			let src =
				unsafe { (input.data as *const u8).add(y as usize * input.rowbytes as usize + x as usize * bytes) };
			let dst =
				unsafe { (output.data as *mut u8).add(y as usize * output.rowbytes as usize + x as usize * bytes) };
			let jitter = noise.map_or(0.0, |seed| {
				(hash(seed ^ (y as u32) << 16 ^ x as u32) & 0xff) as f64 / 255.0
			});
			for channel in 0..4 {
				let read = |i: usize| unsafe {
					if deep {
						(src as *const u16).add(i).read_unaligned() as f64
					} else {
						src.add(i).read() as f64
					}
				};
				let value = if channel == 0 {
					read(0)
				} else {
					((read(channel) * gain) + jitter * max * 0.25).clamp(0.0, max)
				};
				unsafe {
					if deep {
						(dst as *mut u16).add(channel).write_unaligned(value.round() as u16);
					} else {
						dst.add(channel).write(value.round() as u8);
					}
				}
			}
		}
	}
}

/// The parameter values a render uses.
#[derive(Clone, Copy)]
struct Settings {
	mode: Mode,
	gain: f64,
	delay: f64,
}

/// What every render path does, into `output` from `input`.
unsafe fn draw(in_data: &PF_InData, settings: &Settings, input: &PF_LayerDef, output: &PF_LayerDef) -> PF_Err {
	let &Settings { mode, gain, delay } = settings;
	let render_index = RENDERS.fetch_add(1, Ordering::Relaxed);

	if delay > 0.0 {
		std::thread::sleep(std::time::Duration::from_secs_f64(delay / 1000.0));
	}

	match mode {
		Mode::Crash => unsafe {
			std::ptr::null_mut::<u32>().write_volatile(0xdead);
		},
		Mode::Hang => loop {
			std::thread::sleep(std::time::Duration::from_millis(5));
			if let Some(abort) = in_data.inter.abort {
				let err = unsafe { abort(in_data.effect_ref) };
				if err != PF_Err_NONE as PF_Err {
					return err;
				}
			}
		},
		Mode::Freeze => loop {
			std::thread::sleep(std::time::Duration::from_millis(5));
		},
		Mode::Error => return PF_Err_INTERNAL_STRUCT_DAMAGED as PF_Err,
		Mode::Leak => {
			let utils = unsafe { &*in_data.utils };
			if let Some(new_handle) = utils.host_new_handle {
				let _leaked = unsafe { new_handle(64) };
			}
		}
		_ => {}
	}

	let rows = if mode == Mode::Unwritten {
		output.height / 2
	} else {
		output.height
	};
	let noise = (mode == Mode::Random).then_some(render_index.wrapping_add(1));
	unsafe { shade(input, output, rows, gain, noise) };

	if mode == Mode::Overrun {
		// One row past the last: what a plugin miscomputing `rowbytes` does.
		let past = unsafe { (output.data as *mut u8).add(output.rowbytes as usize * output.height as usize) };
		unsafe { std::ptr::write_bytes(past, 0x5a, output.rowbytes as usize) };
	}

	PF_Err_NONE as PF_Err
}

unsafe fn render(in_data: *mut PF_InData, params: *mut *mut PF_ParamDef, output: *mut PF_LayerDef) -> PF_Err {
	let param = |index: usize| unsafe { &**params.add(index) };
	let settings = Settings {
		mode: mode_of(unsafe { param(PARAM_MODE).u.pd.value }),
		gain: unsafe { param(PARAM_GAIN).u.fs_d.value },
		delay: unsafe { param(PARAM_DELAY).u.fs_d.value },
	};
	let input = unsafe { &param(0).u.ld };
	unsafe { draw(&*in_data, &settings, input, &*output) }
}

/// Read the parameters through `PF_CHECKOUT_PARAM`, as smart render must.
unsafe fn checkout_settings(in_data: &PF_InData) -> Result<Settings, PF_Err> {
	let (Some(checkout), Some(checkin)) = (in_data.inter.checkout_param, in_data.inter.checkin_param) else {
		return Err(PF_Err_BAD_CALLBACK_PARAM as PF_Err);
	};
	let read = |index: usize, keep: bool| -> Result<PF_ParamDef, PF_Err> {
		let mut def: PF_ParamDef = unsafe { std::mem::zeroed() };
		let err = unsafe {
			checkout(
				in_data.effect_ref,
				index as PF_ParamIndex,
				in_data.current_time,
				in_data.time_step,
				in_data.time_scale,
				&mut def,
			)
		};
		if err != PF_Err_NONE as PF_Err {
			return Err(err);
		}
		if !keep {
			unsafe { checkin(in_data.effect_ref, &mut def) };
		}
		Ok(def)
	};
	let mode = mode_of(unsafe { read(PARAM_MODE, false)?.u.pd.value });
	let gain = unsafe { read(PARAM_GAIN, false)?.u.fs_d.value };
	let delay = unsafe { read(PARAM_DELAY, false)?.u.fs_d.value };
	// A layer param must be checked back in; `ParamLeak` does not.
	read(PARAM_MAP, mode == Mode::ParamLeak)?;
	Ok(Settings { mode, gain, delay })
}

unsafe fn smart_pre_render(in_data: *mut PF_InData, extra: *mut PF_PreRenderExtra) -> PF_Err {
	let in_data = unsafe { &*in_data };
	let extra = unsafe { &mut *extra };
	let Some(checkout_layer) = (unsafe { (*extra.cb).checkout_layer }) else {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	};
	let mut result: PF_CheckoutResult = unsafe { std::mem::zeroed() };
	let err = unsafe {
		checkout_layer(
			in_data.effect_ref,
			0,
			0,
			&(*extra.input).output_request,
			in_data.current_time,
			in_data.time_step,
			in_data.time_scale,
			&mut result,
		)
	};
	if err != PF_Err_NONE as PF_Err {
		return err;
	}
	let output = unsafe { &mut *extra.output };
	output.result_rect = result.result_rect;
	output.max_result_rect = result.max_result_rect;
	PF_Err_NONE as PF_Err
}

unsafe fn smart_render(in_data: *mut PF_InData, extra: *mut PF_SmartRenderExtra) -> PF_Err {
	let in_data = unsafe { &*in_data };
	let cb = unsafe { &*(*extra).cb };
	let settings = match unsafe { checkout_settings(in_data) } {
		Ok(settings) => settings,
		Err(err) => return err,
	};
	let (Some(checkout_pixels), Some(checkin_pixels), Some(checkout_output)) =
		(cb.checkout_layer_pixels, cb.checkin_layer_pixels, cb.checkout_output)
	else {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	};
	// `Undeclared` asks for checkout id 1; pre-render only declared 0.
	let id = if settings.mode == Mode::Undeclared { 1 } else { 0 };
	let mut input: *mut PF_EffectWorld = std::ptr::null_mut();
	let mut output: *mut PF_EffectWorld = std::ptr::null_mut();
	unsafe {
		let err = checkout_pixels(in_data.effect_ref, id, &mut input);
		if err != PF_Err_NONE as PF_Err {
			return err;
		}
		let err = checkout_output(in_data.effect_ref, &mut output);
		if err != PF_Err_NONE as PF_Err {
			return err;
		}
	}
	if input.is_null() || output.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}
	let err = unsafe { draw(in_data, &settings, &*input, &*output) };
	unsafe { checkin_pixels(in_data.effect_ref, id) };
	if settings.mode == Mode::Unbalanced {
		unsafe { checkin_pixels(in_data.effect_ref, id) };
	}
	err
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn EffectMain(
	cmd: PF_Cmd,
	in_data: *mut PF_InData,
	out_data: *mut PF_OutData,
	params: *mut *mut PF_ParamDef,
	output: *mut PF_LayerDef,
	_extra: *mut c_void,
) -> PF_Err {
	#[allow(non_upper_case_globals)]
	match cmd as _ {
		PF_Cmd_GLOBAL_SETUP => {
			let out = unsafe { &mut *out_data };
			out.my_version = 1 << 19;
			out.out_flags = (PF_OutFlag_DEEP_COLOR_AWARE | PF_OutFlag_PIX_INDEPENDENT) as PF_OutFlags;
			out.out_flags2 = PF_OutFlag2_SUPPORTS_SMART_RENDER as PF_OutFlags2;
			PF_Err_NONE as PF_Err
		}
		PF_Cmd_PARAMS_SETUP => unsafe { params_setup(in_data, out_data) },
		PF_Cmd_RENDER => unsafe { render(in_data, params, output) },
		PF_Cmd_SMART_PRE_RENDER => unsafe { smart_pre_render(in_data, _extra.cast()) },
		PF_Cmd_SMART_RENDER => unsafe { smart_render(in_data, _extra.cast()) },
		_ => PF_Err_NONE as PF_Err,
	}
}
