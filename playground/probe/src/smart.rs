//! SmartRender and GPU-device measurements.
//!
//! `PF_Cmd_SMART_PRE_RENDER` and `PF_Cmd_SMART_RENDER` hand the probe a
//! scenario: the output request comes from the comp, the input layer from
//! the project, the bit depth from the project settings, and the render time
//! from the timeline. Everything that depends on the scenario is recorded as
//! context (`smart_pre_render` / `smart_render` events), never as a fact.
//!
//! The facts are the host's answers to requests the probe builds itself:
//!
//! - checkouts with a fixed rectangle inside any layer of at least 10x9
//!   pixels, and an empty rectangle;
//! - a checkout that overhangs the layer by 4 pixels on every side, reported
//!   relative to the layer's `max_result_rect`, so the layer size cancels out;
//! - `checkin_layer_pixels` on a checked-out id;
//! - `GuidMixInPtr` with fixed bytes;
//! - whether `pre_render_data` reaches SMART_RENDER intact;
//! - the pixel format and world flags of the checked-out input and output
//!   worlds.
//!
//! Every fact is recorded once per bit depth (`smart.<bpc>.*`), since the
//! depth is the one scenario value these answers may depend on.
//!
//! The probe makes only calls the SDK defines: no second checkin of one id,
//! no `checkout_layer_pixels` on an id pre-render did not check out, no null
//! `GuidMixInPtr` buffer, and no `checkout_layer` on an out-of-range parameter
//! index. A host may abort on the first, and After Effects 27 throws a foreign
//! exception on the last instead of answering; the probe runs inside live
//! After Effects sessions.
//!
//! GPU: the smart variant declares `PF_OutFlag2_SUPPORTS_GPU_RENDER_F32`, so
//! a host with a GPU sends `PF_Cmd_GPU_DEVICE_SETUP`. Its inputs and the GPU
//! Device Suite's answers depend on the machine, so they are context only
//! (`gpu_device` events). The device pointers are recorded as null/non-null
//! and never dereferenced. The pre-render never sets
//! `PF_RenderOutputFlag_GPU_RENDER_POSSIBLE`, so frames render on the CPU and
//! `PF_Cmd_SMART_RENDER_GPU` is not expected; if a host sends it anyway, the
//! command's begin/end events in the trace show it.

use std::ffi::c_void;
use std::ptr::null_mut;

use after_effects::sys as ae;
use serde_json::{Value, json};

use crate::checks::{acquire, fact};
use crate::inspect::{self, rect};
use crate::render::{self, Bpc};
use crate::trace::trace;

// ---- Checkout ids and fixed requests ---------------------------------------

/// The input layer for the pattern, with the host's own output request.
const ID_MAIN: ae::A_long = 0;
/// [`INNER`]: inside any layer of at least 10x9 pixels.
const ID_INNER: ae::A_long = 1;
/// The layer's `max_result_rect` grown by [`OVERHANG`] on every side.
const ID_OVERHANG: ae::A_long = 2;
/// [`EMPTY`]: a zero-area rectangle.
const ID_EMPTY: ae::A_long = 3;

const INNER: ae::PF_LRect = ae::PF_LRect {
	left: 2,
	top: 3,
	right: 10,
	bottom: 9,
};
const EMPTY: ae::PF_LRect = ae::PF_LRect {
	left: 5,
	top: 5,
	right: 5,
	bottom: 5,
};
const OVERHANG: ae::A_long = 4;

const PRE_RENDER_MAGIC: u64 = u64::from_be_bytes(*b"AXSMART1");

/// What pre-render passes to render through `pre_render_data`.
struct PreRenderData {
	magic: u64,
	/// Whether the main checkout succeeded.
	main_ok: bool,
	/// The main checkout's `max_result_rect`, when it succeeded.
	main_max: Option<ae::PF_LRect>,
	/// The inner checkout's `result_rect`, when it succeeded.
	inner: Option<ae::PF_LRect>,
}

/// `delete_pre_render_data_func`: free the block and record that the host
/// released it. Called by the host after the frame, possibly from another
/// thread, so the event is context.
unsafe extern "C" fn delete_pre_render_data(data: *mut c_void) {
	let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
		if data.is_null() {
			trace().emit("pre_render_data", json!({ "what": "deleted", "null": true }));
			return;
		}
		let data = unsafe { Box::from_raw(data as *mut PreRenderData) };
		trace().emit(
			"pre_render_data",
			json!({ "what": "deleted", "magic_ok": data.magic == PRE_RENDER_MAGIC }),
		);
	}));
}

// ---- JSON helpers ----------------------------------------------------------

fn framework_name(what_gpu: ae::PF_GPU_Framework) -> Value {
	#[allow(non_upper_case_globals)]
	let name = match what_gpu as _ {
		ae::PF_GPU_Framework_NONE => "NONE",
		ae::PF_GPU_Framework_OPENCL => "OPENCL",
		ae::PF_GPU_Framework_METAL => "METAL",
		ae::PF_GPU_Framework_CUDA => "CUDA",
		ae::PF_GPU_Framework_DIRECTX => "DIRECTX",
		_ => return json!(what_gpu),
	};
	json!(name)
}

fn request_json(r: &ae::PF_RenderRequest) -> Value {
	json!({
		"rect": rect(&r.rect),
		"field": r.field,
		"channel_mask": r.channel_mask,
		"preserve_rgb_of_zero_alpha": r.preserve_rgb_of_zero_alpha,
	})
}

fn checkout_json(err: ae::PF_Err, r: &ae::PF_CheckoutResult) -> Value {
	json!({
		"err": err,
		"result_rect": rect(&r.result_rect),
		"max_result_rect": rect(&r.max_result_rect),
		"par": inspect::rational(&r.par),
		"solid": r.solid,
		"ref_width": r.ref_width,
		"ref_height": r.ref_height,
	})
}

fn same_rect(a: &ae::PF_LRect, b: &ae::PF_LRect) -> bool {
	(a.left, a.top, a.right, a.bottom) == (b.left, b.top, b.right, b.bottom)
}

/// A world snapshot with a pixel hash only when the pixel size is known and
/// every row fits in `rowbytes`; otherwise the layout alone.
unsafe fn snapshot_known(world: *mut ae::PF_EffectWorld, bpc: Option<Bpc>) -> Value {
	match bpc {
		Some(bpc) if unsafe { render::rows_fit(world, bpc) } => unsafe {
			inspect::snapshot_world_bpp(world, bpc.bytes_per_pixel())
		},
		_ => unsafe { inspect::snapshot_world(world, false) },
	}
}

// ---- SMART_PRE_RENDER ------------------------------------------------------

/// Check out the input layer for the pattern plus the fixed requests behind
/// the checkout facts, and hand the results to SMART_RENDER.
pub unsafe fn pre_render(in_data: *mut ae::PF_InData, extra: *mut ae::PF_PreRenderExtra) -> ae::PF_Err {
	let extra = unsafe { extra.as_mut() };
	let Some((input, output, cb)) =
		extra.and_then(|e| unsafe { Some((e.input.as_ref()?, e.output.as_mut()?, e.cb.as_ref()?)) })
	else {
		trace().emit("note", json!({ "msg": "SMART_PRE_RENDER extra or a member is null" }));
		return ae::PF_Err_BAD_CALLBACK_PARAM as ae::PF_Err;
	};
	let Some(checkout_layer) = cb.checkout_layer else {
		trace().emit(
			"note",
			json!({ "msg": "PF_PreRenderCallbacks::checkout_layer is null" }),
		);
		return ae::PF_Err_BAD_CALLBACK_PARAM as ae::PF_Err;
	};

	let d = unsafe { &*in_data };
	// One set of checkout facts per project depth: a host may answer the
	// same request differently at another depth, and the summary keeps only
	// the last value written under a name.
	let bpc = input.bitdepth;
	let checkout = |index: ae::PF_ParamIndex, id: ae::A_long, request: &ae::PF_RenderRequest| {
		let mut result: ae::PF_CheckoutResult = unsafe { std::mem::zeroed() };
		let err = unsafe {
			checkout_layer(
				d.effect_ref,
				index,
				id,
				request,
				d.current_time,
				d.time_step,
				d.time_scale,
				&mut result,
			)
		};
		(err, result)
	};

	// The fixed requests do not take the field or channel mask from the host,
	// so they stay the same input in every scenario.
	let fixed = |rect: ae::PF_LRect| {
		let mut request: ae::PF_RenderRequest = unsafe { std::mem::zeroed() };
		request.rect = rect;
		request.field = ae::PF_Field_FRAME as ae::PF_Field;
		request.channel_mask = ae::PF_ChannelMask_ARGB as ae::PF_ChannelMask;
		request
	};

	// Main: the host's own request, as every SmartRender effect does it.
	let (main_err, main) = checkout(0, ID_MAIN, &input.output_request);
	let main_ok = main_err == ae::PF_Err_NONE as ae::PF_Err;

	let (inner_err, inner) = checkout(0, ID_INNER, &fixed(INNER));
	fact(
		&format!("smart.{bpc}.checkout_layer.inner(2,3,10,9)"),
		json!({
			"err": inner_err,
			"result_rect": rect(&inner.result_rect),
			"max_matches_main": main_ok.then(|| same_rect(&inner.max_result_rect, &main.max_result_rect)),
		}),
	);

	let overhang = main_ok.then(|| {
		let max = main.max_result_rect;
		let grown = ae::PF_LRect {
			left: max.left - OVERHANG,
			top: max.top - OVERHANG,
			right: max.right + OVERHANG,
			bottom: max.bottom + OVERHANG,
		};
		let (err, result) = checkout(0, ID_OVERHANG, &fixed(grown));
		let r = result.result_rect;
		fact(
			&format!("smart.{bpc}.checkout_layer.overhang(max+-4)"),
			json!({
				"err": err,
				// [0,0,0,0]: clipped to the layer; [-4,-4,4,4]: as requested.
				"result_minus_max": [r.left - max.left, r.top - max.top, r.right - max.right, r.bottom - max.bottom],
			}),
		);
		(err, result)
	});
	if overhang.is_none() {
		fact(
			&format!("smart.{bpc}.checkout_layer.overhang(max+-4)"),
			json!("main checkout failed"),
		);
	}

	let (empty_err, empty) = checkout(0, ID_EMPTY, &fixed(EMPTY));
	fact(
		&format!("smart.{bpc}.checkout_layer.empty(5,5,5,5)"),
		json!({ "err": empty_err, "result_rect": rect(&empty.result_rect) }),
	);

	match cb.GuidMixInPtr {
		Some(mix_in) => {
			let bytes = *b"AXPR";
			let err = unsafe { mix_in(d.effect_ref, bytes.len() as ae::A_u_long, bytes.as_ptr().cast()) };
			fact(&format!("smart.{bpc}.guid_mix_in(4 bytes)"), json!({ "err": err }));
		}
		None => fact(&format!("smart.{bpc}.guid_mix_in"), json!("unavailable")),
	}

	// The output covers what the main checkout delivered, as in the SDK
	// samples. No GPU_RENDER_POSSIBLE flag: every frame renders on the CPU.
	if main_ok {
		output.result_rect = main.result_rect;
		output.max_result_rect = main.max_result_rect;
	}
	let data = Box::new(PreRenderData {
		magic: PRE_RENDER_MAGIC,
		main_ok,
		main_max: main_ok.then_some(main.max_result_rect),
		inner: (inner_err == ae::PF_Err_NONE as ae::PF_Err).then_some(inner.result_rect),
	});
	output.pre_render_data = Box::into_raw(data).cast();
	output.delete_pre_render_data_func = Some(delete_pre_render_data);

	trace().emit(
		"smart_pre_render",
		json!({
			"input": {
				"output_request": request_json(&input.output_request),
				"bitdepth": input.bitdepth,
				"what_gpu": framework_name(input.what_gpu),
				"device_index": input.device_index,
				"gpu_data": !input.gpu_data.is_null(),
			},
			"checkout": {
				"main": checkout_json(main_err, &main),
				"inner": checkout_json(inner_err, &inner),
				"overhang": overhang.map(|(err, result)| checkout_json(err, &result)),
				"empty": checkout_json(empty_err, &empty),
			},
			"output": {
				"result_rect": rect(&output.result_rect),
				"max_result_rect": rect(&output.max_result_rect),
				"solid": output.solid,
				"flags": output.flags,
			},
		}),
	);

	// A failed main checkout is recorded above; SMART_RENDER still runs and
	// collects its facts.
	ae::PF_Err_NONE as ae::PF_Err
}

// ---- SMART_RENDER ----------------------------------------------------------

/// Parameters checked out through `PF_InData::inter`: SMART_RENDER gets no
/// parameter list. Index 0 (the input layer) is checked out as pixels instead.
struct CheckedOutParams {
	in_data: *mut ae::PF_InData,
	defs: Vec<ae::PF_ParamDef>,
	ok: Vec<bool>,
}

impl CheckedOutParams {
	unsafe fn checkout(in_data: *mut ae::PF_InData) -> Self {
		let d = unsafe { &*in_data };
		// InData.num_params is 0 outside PARAMS_SETUP (see render::render), so
		// the declared count describes the parameter list on both hosts.
		let count = render::PARAM_COUNT;
		let mut params = Self {
			in_data,
			defs: (0..count).map(|_| unsafe { std::mem::zeroed() }).collect(),
			ok: vec![false; count],
		};
		let Some(checkout) = d.inter.checkout_param else {
			trace().emit("note", json!({ "msg": "inter.checkout_param is null" }));
			return params;
		};

		for index in 1..count {
			let def = &mut params.defs[index];
			let err = unsafe {
				checkout(
					d.effect_ref,
					index as ae::PF_ParamIndex,
					d.current_time,
					d.time_step,
					d.time_scale,
					def,
				)
			};
			params.ok[index] = err == ae::PF_Err_NONE as ae::PF_Err;
			if params.ok[index] {
				trace().emit("param", unsafe { inspect::snapshot_param(index, def) });
			} else {
				trace().emit(
					"note",
					json!({ "msg": "checkout_param failed", "index": index, "err": err }),
				);
			}
		}
		params
	}

	/// A `PF_ParamList`-shaped array: null where nothing was checked out.
	fn list(&mut self) -> Vec<*mut ae::PF_ParamDef> {
		self.defs
			.iter_mut()
			.zip(&self.ok)
			.map(|(def, &ok)| if ok { def as *mut _ } else { null_mut() })
			.collect()
	}

	unsafe fn checkin(mut self) {
		let d = unsafe { &*self.in_data };
		let Some(checkin) = d.inter.checkin_param else {
			return;
		};
		for (def, ok) in self.defs.iter_mut().zip(&self.ok) {
			if *ok {
				unsafe { checkin(d.effect_ref, def) };
			}
		}
	}
}

/// The world's pixel format through World Suite v2, as `(err, format)`.
unsafe fn pixel_format(
	in_data: *mut ae::PF_InData,
	world: *mut ae::PF_EffectWorld,
) -> (Option<ae::PF_Err>, Option<ae::PF_PixelFormat>) {
	if world.is_null() {
		return (None, None);
	}
	let Some(guard) = (unsafe { acquire(in_data, ae::kPFWorldSuite, ae::kPFWorldSuiteVersion2) }) else {
		return (None, None);
	};
	let Some(get_format) = (unsafe { &*(guard.ptr as *const ae::PF_WorldSuite2) }).PF_GetPixelFormat else {
		return (None, None);
	};
	let mut format: ae::PF_PixelFormat = 0;
	let err = unsafe { get_format(world, &mut format) };
	(Some(err), (err == ae::PF_Err_NONE as ae::PF_Err).then_some(format))
}

/// One checked-out world's layout, for a `smart.<bpc>.*` fact.
fn layout_json(
	err: ae::PF_Err,
	world: *mut ae::PF_EffectWorld,
	format: (Option<ae::PF_Err>, Option<ae::PF_PixelFormat>),
) -> Value {
	let w = unsafe { world.as_ref() };
	json!({
		"err": err,
		"nonnull": w.is_some(),
		"format_err": format.0,
		"format": format.1.map(inspect::fourcc),
		"world_flags": w.map(|w| w.world_flags),
	})
}

/// Check the input layer and the output out, record their layouts, and write
/// the test pattern, as the legacy RENDER does.
pub unsafe fn render(in_data: *mut ae::PF_InData, extra: *mut ae::PF_SmartRenderExtra) -> ae::PF_Err {
	let extra = unsafe { extra.as_mut() };
	let Some((input, cb)) = extra.and_then(|e| unsafe { Some((e.input.as_ref()?, e.cb.as_ref()?)) }) else {
		trace().emit("note", json!({ "msg": "SMART_RENDER extra or a member is null" }));
		return ae::PF_Err_BAD_CALLBACK_PARAM as ae::PF_Err;
	};
	let (Some(checkout_pixels), Some(checkin_pixels), Some(checkout_output)) =
		(cb.checkout_layer_pixels, cb.checkin_layer_pixels, cb.checkout_output)
	else {
		trace().emit("note", json!({ "msg": "a PF_SmartRenderCallbacks entry is null" }));
		return ae::PF_Err_BAD_CALLBACK_PARAM as ae::PF_Err;
	};
	let effect_ref = unsafe { (*in_data).effect_ref };
	let bpc = input.bitdepth;

	// The block pre-render returned. Only our own pointer is ever stored
	// there, so it is read only when non-null.
	let data = unsafe { (input.pre_render_data as *const PreRenderData).as_ref() };
	let data = data.filter(|d| d.magic == PRE_RENDER_MAGIC);
	fact(
		&format!("smart.{bpc}.pre_render_data"),
		json!({ "nonnull": !input.pre_render_data.is_null(), "magic_ok": data.is_some() }),
	);

	match data.and_then(|d| d.inner) {
		Some(inner) => {
			let mut world = null_mut();
			let err = unsafe { checkout_pixels(effect_ref, ID_INNER, &mut world) };
			let w = unsafe { world.as_ref() };
			let main_max = data.and_then(|d| d.main_max);
			fact(
				&format!("smart.{bpc}.checkout_layer_pixels.inner(2,3,10,9)"),
				json!({
					"err": err,
					"nonnull": w.is_some(),
					"size_matches_result_rect": w.map(|w| {
						(w.width, w.height) == (inner.right - inner.left, inner.bottom - inner.top)
					}),
					"size_matches_max_result_rect": w.zip(main_max).map(|(w, max)| {
						(w.width, w.height) == (max.right - max.left, max.bottom - max.top)
					}),
					"origin_minus_result_lt": w.map(|w| [w.origin_x - inner.left, w.origin_y - inner.top]),
				}),
			);
			if err == ae::PF_Err_NONE as ae::PF_Err {
				// Checked in once only: the SDK leaves a second checkin of the
				// same id undefined, and a measuring instrument must not risk
				// the host's session on it.
				let err = unsafe { checkin_pixels(effect_ref, ID_INNER) };
				fact(&format!("smart.{bpc}.checkin_layer_pixels"), json!({ "err": err }));
			}
		}
		None => fact(
			&format!("smart.{bpc}.checkout_layer_pixels.inner(2,3,10,9)"),
			json!("not checked out in pre-render"),
		),
	}

	let mut input_world = null_mut();
	let main_err = if data.is_some_and(|d| d.main_ok) {
		unsafe { checkout_pixels(effect_ref, ID_MAIN, &mut input_world) }
	} else {
		ae::PF_Err_BAD_CALLBACK_PARAM as ae::PF_Err
	};
	let in_format = unsafe { pixel_format(in_data, input_world) };
	fact(
		&format!("smart.{bpc}.checkout_layer_pixels[main]"),
		layout_json(main_err, input_world, in_format),
	);

	let mut output_world = null_mut();
	let out_err = unsafe { checkout_output(effect_ref, &mut output_world) };
	let out_format = unsafe { pixel_format(in_data, output_world) };
	fact(
		&format!("smart.{bpc}.checkout_output"),
		layout_json(out_err, output_world, out_format),
	);

	// The pixel size comes from PF_GetPixelFormat only. Guessing it from the
	// announced depth would write past the end of a world whose depth the
	// host got wrong, inside the host. `None`: no answer, or a format the
	// pattern does not handle (a GPU or video format).
	let in_bpc = in_format.1.and_then(Bpc::from_pixel_format);
	let out_bpc = out_format.1.and_then(Bpc::from_pixel_format);
	let input_for_pattern = if in_bpc.is_some() { input_world } else { null_mut() };

	let mut params = unsafe { CheckedOutParams::checkout(in_data) };
	if out_err != ae::PF_Err_NONE as ae::PF_Err || output_world.is_null() {
		trace().emit(
			"note",
			json!({ "msg": "checkout_output gave no world; nothing rendered" }),
		);
	} else if let Some(out_bpc) = out_bpc {
		let mut list = params.list();
		let settings = unsafe { render::read_settings(list.as_mut_ptr(), list.len()) };
		let in_bpc = in_bpc.unwrap_or(Bpc::Eight); // unused when the input is null
		unsafe { render::fill_pattern(output_world, out_bpc, input_for_pattern, in_bpc, &settings) };
	} else {
		trace().emit(
			"note",
			json!({ "msg": "output pixel format unknown or not ARGB32/64/128; nothing rendered" }),
		);
	}
	unsafe { params.checkin() };

	trace().emit(
		"smart_render",
		json!({
			"input": {
				"output_request": request_json(&input.output_request),
				"bitdepth": bpc,
				"what_gpu": framework_name(input.what_gpu),
				"device_index": input.device_index,
				"gpu_data": !input.gpu_data.is_null(),
			},
			"input_world": unsafe { snapshot_known(input_world, in_bpc) },
			"output_world": unsafe { snapshot_known(output_world, out_bpc) },
			"formats": {
				"input": in_format.1.map(inspect::fourcc),
				"output": out_format.1.map(inspect::fourcc),
			},
		}),
	);

	if !input_world.is_null() {
		unsafe { checkin_pixels(effect_ref, ID_MAIN) };
	}

	out_err
}

// ---- GPU device setup ------------------------------------------------------

/// Record the device the host offers and what the GPU Device Suite says
/// about it, then accept the device only if GLOBAL_SETUP claimed GPU
/// support (the legacy variant never does). No GPU data is allocated: the
/// probe never renders on the GPU.
pub unsafe fn gpu_device_setup(
	in_data: *mut ae::PF_InData,
	out_data: *mut ae::PF_OutData,
	extra: *mut ae::PF_GPUDeviceSetupExtra,
	out_flags2: ae::PF_OutFlags2,
) -> ae::PF_Err {
	let extra = unsafe { extra.as_ref() };
	let input = extra.and_then(|e| unsafe { e.input.as_ref() });
	let device_index = input.map(|i| i.device_index);
	let effect_ref = unsafe { (*in_data).effect_ref };

	let suite = match unsafe { acquire(in_data, ae::kPFGPUDeviceSuite, ae::kPFGPUDeviceSuiteVersion1) } {
		None => json!("unavailable"),
		Some(guard) => {
			let s = unsafe { &*(guard.ptr as *const ae::PF_GPUDeviceSuite1) };
			let mut count: ae::A_u_long = 0;
			let count_err = s.GetDeviceCount.map(|f| unsafe { f(effect_ref, &mut count) });
			let info = match (s.GetDeviceInfo, device_index) {
				(Some(get_info), Some(index)) => {
					let mut info: ae::PF_GPUDeviceInfo = unsafe { std::mem::zeroed() };
					let err = unsafe { get_info(effect_ref, index, &mut info) };
					json!({
						"err": err,
						"device_framework": framework_name(info.device_framework),
						"compatible": info.compatibleB,
						"platform": !info.platformPV.is_null(),
						"device": !info.devicePV.is_null(),
						"context": !info.contextPV.is_null(),
						"command_queue": !info.command_queuePV.is_null(),
						"offscreen_opengl_context": !info.offscreen_opengl_contextPV.is_null(),
						"offscreen_opengl_device": !info.offscreen_opengl_devicePV.is_null(),
					})
				}
				(None, _) => json!("null entry"),
				(_, None) => json!("no device index"),
			};
			json!({ "device_count": { "err": count_err, "count": count }, "device_info": info })
		}
	};

	let gpu_f32 = ae::PF_OutFlag2_SUPPORTS_GPU_RENDER_F32 as ae::PF_OutFlags2;
	// OR-ed in: the flag is documented for GLOBAL_SETUP and GPU_DEVICE_SETUP
	// alike, and the other out_flags2 bits must not be cleared here.
	if let Some(out) = unsafe { out_data.as_mut() } {
		out.out_flags2 |= out_flags2 & gpu_f32;
	}
	// The probe allocates no GPU data, whatever the host left in the slot.
	if let Some(output) = extra.and_then(|e| unsafe { e.output.as_mut() }) {
		output.gpu_data = null_mut();
	}

	trace().emit(
		"gpu_device",
		json!({
			"what": "setup",
			"input": input.map(|i| json!({
				"what_gpu": framework_name(i.what_gpu),
				"device_index": i.device_index,
			})),
			"output_nonnull": extra.is_some_and(|e| !e.output.is_null()),
			"suite": suite,
			"accepted": out_flags2 & gpu_f32 != 0,
		}),
	);
	ae::PF_Err_NONE as ae::PF_Err
}

/// Record what the host hands back at setdown. There is no GPU data to free.
pub unsafe fn gpu_device_setdown(extra: *mut ae::PF_GPUDeviceSetdownExtra) {
	let input = unsafe { extra.as_ref() }.and_then(|e| unsafe { e.input.as_ref() });
	trace().emit(
		"gpu_device",
		json!({
			"what": "setdown",
			"input": input.map(|i| json!({
				"gpu_data": !i.gpu_data.is_null(),
				"what_gpu": framework_name(i.what_gpu),
				"device_index": i.device_index,
			})),
		}),
	);
}
