//! `AEGP Layer Render Options Suite` (wire version 2), `AEGP Render Options
//! Suite` (wire version 4) and `AEGP Render Suite` (wire version 8,
//! `AEGP_RenderSuite5`).
//!
//! Looks checks out the upstream frame at 16 or 32 bits per channel and blurs
//! that copy for Edge Softness. The effect's own input is often 8-bit, which
//! those kernels do not touch. Checkout, whether from a layer or from a comp
//! item, returns the effect's input, converted to the requested depth, and
//! never re-enters the effect.

use std::collections::HashSet;
use std::sync::{LazyLock, Mutex};

use after_effects_sys::{
	A_Boolean, A_Err, A_LRect, A_Time, A_short, A_u_long, AEGP_AsyncFrameRequestRefcon, AEGP_AsyncRequestId,
	AEGP_CancelRefcon, AEGP_ChannelOrder, AEGP_EffectRefH, AEGP_FrameReceiptH, AEGP_ItemH, AEGP_ItemQuality,
	AEGP_ItemQuality_BEST, AEGP_LayerH, AEGP_LayerRenderOptionsH, AEGP_LayerRenderOptionsSuite2, AEGP_MatteMode,
	AEGP_MemHandle, AEGP_PlatformWorldH, AEGP_PluginID, AEGP_RenderOptionsH, AEGP_RenderOptionsSuite4,
	AEGP_RenderSuite5, AEGP_RenderSuiteCheckForCancel, AEGP_SoundDataFormat, AEGP_SoundDataH, AEGP_TimeStamp,
	AEGP_WorldH, AEGP_WorldType, PF_Err_BAD_CALLBACK_PARAM, PF_Err_NONE, PF_Field, PF_ProgPtr,
};

use super::aegp_world;
use super::effect::effect_ref_of;
use crate::PluginInstance;

struct Options {
	effect_ref: PF_ProgPtr,
	world_type: i32,
	time: A_Time,
	time_step: A_Time,
	down_x: A_short,
	down_y: A_short,
	matte: AEGP_MatteMode,
}

struct Receipt {
	world: AEGP_WorldH,
	width: i32,
	height: i32,
}

static OPTIONS: LazyLock<Mutex<HashSet<usize>>> = LazyLock::new(|| Mutex::new(HashSet::new()));
static RECEIPTS: LazyLock<Mutex<HashSet<usize>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

fn options_set() -> std::sync::MutexGuard<'static, HashSet<usize>> {
	OPTIONS.lock().unwrap_or_else(|err| err.into_inner())
}

fn receipts_set() -> std::sync::MutexGuard<'static, HashSet<usize>> {
	RECEIPTS.lock().unwrap_or_else(|err| err.into_inner())
}

fn publish_options(options: Options) -> AEGP_LayerRenderOptionsH {
	let inner = Box::into_raw(Box::new(options));
	let handle = Box::into_raw(Box::new(inner)) as AEGP_LayerRenderOptionsH;
	options_set().insert(handle as usize);
	handle
}

/// # Safety
/// `handle` is an options handle from [`publish_options`], or null.
unsafe fn options_mut<'a>(handle: AEGP_LayerRenderOptionsH) -> Option<&'a mut Options> {
	if handle.is_null() || !options_set().contains(&(handle as usize)) {
		return None;
	}
	let inner = unsafe { *(handle as *mut *mut Options) };
	if inner.is_null() {
		return None;
	}
	Some(unsafe { &mut *inner })
}

fn effect_ref_from(effect_refH: AEGP_EffectRefH) -> PF_ProgPtr {
	unsafe { effect_ref_of(effect_refH) }.unwrap_or(effect_refH as PF_ProgPtr)
}

fn fresh_options(effect_ref: PF_ProgPtr) -> Options {
	Options {
		effect_ref,
		world_type: 0,
		time: A_Time { value: 0, scale: 1 },
		time_step: A_Time { value: 1, scale: 1 },
		down_x: 1,
		down_y: 1,
		matte: 0,
	}
}

unsafe extern "C" fn new_from_layer(
	_plugin_id: AEGP_PluginID,
	layerH: AEGP_LayerH,
	options_out: *mut AEGP_LayerRenderOptionsH,
) -> A_Err {
	let Some(out) = (unsafe { options_out.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	if PluginInstance::get_instance_ptr(layerH as PF_ProgPtr).is_none() {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	}
	*out = publish_options(fresh_options(layerH as PF_ProgPtr));
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn new_from_upstream(
	_plugin_id: AEGP_PluginID,
	effect_refH: AEGP_EffectRefH,
	options_out: *mut AEGP_LayerRenderOptionsH,
) -> A_Err {
	let Some(out) = (unsafe { options_out.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	let effect_ref = effect_ref_from(effect_refH);
	if PluginInstance::get_instance_ptr(effect_ref).is_none() {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	}
	*out = publish_options(fresh_options(effect_ref));
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn new_from_downstream(
	_plugin_id: AEGP_PluginID,
	_effect_refH: AEGP_EffectRefH,
	_options_out: *mut AEGP_LayerRenderOptionsH,
) -> A_Err {
	// Downstream would render this effect again. aexlo has nothing after it.
	PF_Err_BAD_CALLBACK_PARAM as A_Err
}

unsafe extern "C" fn duplicate(
	_plugin_id: AEGP_PluginID,
	optionsH: AEGP_LayerRenderOptionsH,
	copy_out: *mut AEGP_LayerRenderOptionsH,
) -> A_Err {
	let (Some(options), Some(out)) = (unsafe { options_mut(optionsH) }, unsafe { copy_out.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	*out = publish_options(Options {
		effect_ref: options.effect_ref,
		world_type: options.world_type,
		time: options.time,
		time_step: options.time_step,
		down_x: options.down_x,
		down_y: options.down_y,
		matte: options.matte,
	});
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn dispose_options(optionsH: AEGP_LayerRenderOptionsH) -> A_Err {
	if !options_set().remove(&(optionsH as usize)) {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	}
	let slot = unsafe { Box::from_raw(optionsH as *mut *mut Options) };
	if !slot.is_null() {
		drop(unsafe { Box::from_raw(*slot) });
	}
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn set_time(optionsH: AEGP_LayerRenderOptionsH, time: A_Time) -> A_Err {
	let Some(options) = (unsafe { options_mut(optionsH) }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	options.time = time;
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn get_time(optionsH: AEGP_LayerRenderOptionsH, time_out: *mut A_Time) -> A_Err {
	let (Some(options), Some(out)) = (unsafe { options_mut(optionsH) }, unsafe { time_out.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	*out = options.time;
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn set_time_step(optionsH: AEGP_LayerRenderOptionsH, time_step: A_Time) -> A_Err {
	let Some(options) = (unsafe { options_mut(optionsH) }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	options.time_step = time_step;
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn get_time_step(optionsH: AEGP_LayerRenderOptionsH, time_out: *mut A_Time) -> A_Err {
	let (Some(options), Some(out)) = (unsafe { options_mut(optionsH) }, unsafe { time_out.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	*out = options.time_step;
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn set_world_type(optionsH: AEGP_LayerRenderOptionsH, kind: AEGP_WorldType) -> A_Err {
	let Some(options) = (unsafe { options_mut(optionsH) }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	options.world_type = kind;
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn get_world_type(optionsH: AEGP_LayerRenderOptionsH, kind_out: *mut AEGP_WorldType) -> A_Err {
	let (Some(options), Some(out)) = (unsafe { options_mut(optionsH) }, unsafe { kind_out.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	*out = options.world_type;
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn set_downsample(optionsH: AEGP_LayerRenderOptionsH, x: A_short, y: A_short) -> A_Err {
	let Some(options) = (unsafe { options_mut(optionsH) }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	options.down_x = x;
	options.down_y = y;
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn get_downsample(
	optionsH: AEGP_LayerRenderOptionsH,
	x_out: *mut A_short,
	y_out: *mut A_short,
) -> A_Err {
	let Some(options) = (unsafe { options_mut(optionsH) }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	if let Some(x) = unsafe { x_out.as_mut() } {
		*x = options.down_x;
	}
	if let Some(y) = unsafe { y_out.as_mut() } {
		*y = options.down_y;
	}
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn set_matte(optionsH: AEGP_LayerRenderOptionsH, mode: AEGP_MatteMode) -> A_Err {
	let Some(options) = (unsafe { options_mut(optionsH) }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	options.matte = mode;
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn get_matte(optionsH: AEGP_LayerRenderOptionsH, mode_out: *mut AEGP_MatteMode) -> A_Err {
	let (Some(options), Some(out)) = (unsafe { options_mut(optionsH) }, unsafe { mode_out.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	*out = options.matte;
	PF_Err_NONE as A_Err
}

pub const fn layer_render_options_suite_2() -> AEGP_LayerRenderOptionsSuite2 {
	AEGP_LayerRenderOptionsSuite2 {
		AEGP_NewFromLayer: Some(new_from_layer),
		AEGP_NewFromUpstreamOfEffect: Some(new_from_upstream),
		AEGP_NewFromDownstreamOfEffect: Some(new_from_downstream),
		AEGP_Duplicate: Some(duplicate),
		AEGP_Dispose: Some(dispose_options),
		AEGP_SetTime: Some(set_time),
		AEGP_GetTime: Some(get_time),
		AEGP_SetTimeStep: Some(set_time_step),
		AEGP_GetTimeStep: Some(get_time_step),
		AEGP_SetWorldType: Some(set_world_type),
		AEGP_GetWorldType: Some(get_world_type),
		AEGP_SetDownsampleFactor: Some(set_downsample),
		AEGP_GetDownsampleFactor: Some(get_downsample),
		AEGP_SetMatteMode: Some(set_matte),
		AEGP_GetMatteMode: Some(get_matte),
	}
}

fn checkout(optionsH: AEGP_LayerRenderOptionsH) -> Result<AEGP_FrameReceiptH, A_Err> {
	let (effect_ref, world_type, down_x, down_y) = {
		let options = unsafe { options_mut(optionsH) }.ok_or(PF_Err_BAD_CALLBACK_PARAM as A_Err)?;
		(options.effect_ref, options.world_type, options.down_x, options.down_y)
	};
	checkout_input(effect_ref, world_type, down_x, down_y)
}

fn checkout_input(
	effect_ref: PF_ProgPtr,
	world_type: i32,
	down_x: A_short,
	down_y: A_short,
) -> Result<AEGP_FrameReceiptH, A_Err> {
	let instance = PluginInstance::get_instance_ptr(effect_ref).ok_or(PF_Err_BAD_CALLBACK_PARAM as A_Err)?;
	let instance = unsafe { instance.as_ref() };
	let layer = instance.input_layer.layer();
	let step_x = down_x.max(1) as u32;
	let step_y = down_y.max(1) as u32;
	let width = (layer.width() / step_x).max(1) as i32;
	let height = (layer.height() / step_y).max(1) as i32;
	let world = aegp_world::world_from_layer(world_type, layer, down_x as i32, down_y as i32);
	let inner = Box::into_raw(Box::new(Receipt { world, width, height }));
	let handle = Box::into_raw(Box::new(inner)) as AEGP_FrameReceiptH;
	receipts_set().insert(handle as usize);
	Ok(handle)
}

/// # Safety
/// `handle` is a receipt from [`checkout`], or null.
unsafe fn receipt_mut<'a>(handle: AEGP_FrameReceiptH) -> Option<&'a mut Receipt> {
	if handle.is_null() || !receipts_set().contains(&(handle as usize)) {
		return None;
	}
	let inner = unsafe { *(handle as *mut *mut Receipt) };
	if inner.is_null() {
		return None;
	}
	Some(unsafe { &mut *inner })
}

unsafe extern "C" fn render_and_checkout_frame(
	optionsH: AEGP_RenderOptionsH,
	_cancel: AEGP_RenderSuiteCheckForCancel,
	_refcon: AEGP_CancelRefcon,
	receipt_out: *mut AEGP_FrameReceiptH,
) -> A_Err {
	let Some(out) = (unsafe { receipt_out.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	let (effect_ref, world_type, down_x, down_y) = {
		let Some(options) = (unsafe { comp_options_mut(optionsH) }) else {
			return PF_Err_BAD_CALLBACK_PARAM as A_Err;
		};
		(options.effect_ref, options.world_type, options.down_x, options.down_y)
	};
	// `AEGP_WorldType` is `i32` on Windows and `u32` on macOS.
	#[allow(clippy::unnecessary_cast)]
	let world_type = world_type as i32;
	match checkout_input(effect_ref, world_type, down_x, down_y) {
		Ok(receipt) => {
			*out = receipt;
			PF_Err_NONE as A_Err
		}
		Err(err) => err,
	}
}

unsafe extern "C" fn render_and_checkout_layer_frame(
	optionsH: AEGP_LayerRenderOptionsH,
	_cancel: AEGP_RenderSuiteCheckForCancel,
	_refcon: AEGP_CancelRefcon,
	receipt_out: *mut AEGP_FrameReceiptH,
) -> A_Err {
	let Some(out) = (unsafe { receipt_out.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	match checkout(optionsH) {
		Ok(receipt) => {
			*out = receipt;
			PF_Err_NONE as A_Err
		}
		Err(err) => err,
	}
}

unsafe extern "C" fn render_layer_async(
	_optionsH: AEGP_LayerRenderOptionsH,
	_callback: after_effects_sys::AEGP_AsyncFrameReadyCallback,
	_refcon: AEGP_AsyncFrameRequestRefcon,
	_request_out: *mut AEGP_AsyncRequestId,
) -> A_Err {
	PF_Err_BAD_CALLBACK_PARAM as A_Err
}

unsafe extern "C" fn cancel_async(_request: AEGP_AsyncRequestId) -> A_Err {
	PF_Err_BAD_CALLBACK_PARAM as A_Err
}

unsafe extern "C" fn checkin_frame(receiptH: AEGP_FrameReceiptH) -> A_Err {
	if !receipts_set().remove(&(receiptH as usize)) {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	}
	let slot = unsafe { Box::from_raw(receiptH as *mut *mut Receipt) };
	if !slot.is_null() {
		let receipt = unsafe { Box::from_raw(*slot) };
		unsafe { aegp_world::dispose(receipt.world) };
	}
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn get_receipt_world(receiptH: AEGP_FrameReceiptH, world_out: *mut AEGP_WorldH) -> A_Err {
	let (Some(receipt), Some(out)) = (unsafe { receipt_mut(receiptH) }, unsafe { world_out.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	*out = receipt.world;
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn get_rendered_region(receiptH: AEGP_FrameReceiptH, region_out: *mut A_LRect) -> A_Err {
	let (Some(receipt), Some(out)) = (unsafe { receipt_mut(receiptH) }, unsafe { region_out.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	*out = A_LRect {
		left: 0,
		top: 0,
		right: receipt.width,
		bottom: receipt.height,
	};
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn is_rendered_sufficient(
	_rendered: AEGP_RenderOptionsH,
	_proposed: AEGP_RenderOptionsH,
	_out: *mut A_Boolean,
) -> A_Err {
	PF_Err_BAD_CALLBACK_PARAM as A_Err
}

unsafe extern "C" fn render_new_item_sound(
	_itemH: AEGP_ItemH,
	_start: *const A_Time,
	_duration: *const A_Time,
	_format: *const AEGP_SoundDataFormat,
	_cancel: AEGP_RenderSuiteCheckForCancel,
	_refcon: AEGP_CancelRefcon,
	_sound_out: *mut AEGP_SoundDataH,
) -> A_Err {
	PF_Err_BAD_CALLBACK_PARAM as A_Err
}

unsafe extern "C" fn get_current_timestamp(_stamp_out: *mut AEGP_TimeStamp) -> A_Err {
	PF_Err_BAD_CALLBACK_PARAM as A_Err
}

unsafe extern "C" fn has_item_changed(
	_itemH: AEGP_ItemH,
	_start: *const A_Time,
	_duration: *const A_Time,
	_stamp: *const AEGP_TimeStamp,
	_changed_out: *mut A_Boolean,
) -> A_Err {
	PF_Err_BAD_CALLBACK_PARAM as A_Err
}

unsafe extern "C" fn is_item_worthwhile(
	_optionsH: AEGP_RenderOptionsH,
	_stamp: *const AEGP_TimeStamp,
	_worthwhile_out: *mut A_Boolean,
) -> A_Err {
	PF_Err_BAD_CALLBACK_PARAM as A_Err
}

unsafe extern "C" fn checkin_rendered_frame(
	_optionsH: AEGP_RenderOptionsH,
	_stamp: *const AEGP_TimeStamp,
	_ticks: A_u_long,
	_imageH: AEGP_PlatformWorldH,
) -> A_Err {
	PF_Err_BAD_CALLBACK_PARAM as A_Err
}

unsafe extern "C" fn get_receipt_guid(_receiptH: AEGP_FrameReceiptH, _guid_out: *mut AEGP_MemHandle) -> A_Err {
	PF_Err_BAD_CALLBACK_PARAM as A_Err
}

pub const fn render_suite_5() -> AEGP_RenderSuite5 {
	AEGP_RenderSuite5 {
		AEGP_RenderAndCheckoutFrame: Some(render_and_checkout_frame),
		AEGP_RenderAndCheckoutLayerFrame: Some(render_and_checkout_layer_frame),
		AEGP_RenderAndCheckoutLayerFrame_Async: Some(render_layer_async),
		AEGP_CancelAsyncRequest: Some(cancel_async),
		AEGP_CheckinFrame: Some(checkin_frame),
		AEGP_GetReceiptWorld: Some(get_receipt_world),
		AEGP_GetRenderedRegion: Some(get_rendered_region),
		AEGP_IsRenderedFrameSufficient: Some(is_rendered_sufficient),
		AEGP_RenderNewItemSoundData: Some(render_new_item_sound),
		AEGP_GetCurrentTimestamp: Some(get_current_timestamp),
		AEGP_HasItemChangedSinceTimestamp: Some(has_item_changed),
		AEGP_IsItemWorthwhileToRender: Some(is_item_worthwhile),
		AEGP_CheckinRenderedFrame: Some(checkin_rendered_frame),
		AEGP_GetReceiptGuid: Some(get_receipt_guid),
	}
}

struct CompOptions {
	effect_ref: PF_ProgPtr,
	world_type: AEGP_WorldType,
	time: A_Time,
	time_step: A_Time,
	field: PF_Field,
	down_x: A_short,
	down_y: A_short,
	roi: A_LRect,
	matte: AEGP_MatteMode,
	channel_order: AEGP_ChannelOrder,
	guide_layers: A_Boolean,
	quality: AEGP_ItemQuality,
}

static COMP_OPTIONS: LazyLock<Mutex<HashSet<usize>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

fn comp_options_set() -> std::sync::MutexGuard<'static, HashSet<usize>> {
	COMP_OPTIONS.lock().unwrap_or_else(|err| err.into_inner())
}

fn publish_comp_options(options: CompOptions) -> AEGP_RenderOptionsH {
	let inner = Box::into_raw(Box::new(options));
	let handle = Box::into_raw(Box::new(inner)) as AEGP_RenderOptionsH;
	comp_options_set().insert(handle as usize);
	handle
}

/// # Safety
/// `handle` is an options handle from [`publish_comp_options`], or null.
unsafe fn comp_options_mut<'a>(handle: AEGP_RenderOptionsH) -> Option<&'a mut CompOptions> {
	if handle.is_null() || !comp_options_set().contains(&(handle as usize)) {
		return None;
	}
	let inner = unsafe { *(handle as *mut *mut CompOptions) };
	if inner.is_null() {
		return None;
	}
	Some(unsafe { &mut *inner })
}

fn fresh_comp_options(effect_ref: PF_ProgPtr) -> CompOptions {
	CompOptions {
		effect_ref,
		world_type: 0,
		time: A_Time { value: 0, scale: 1 },
		time_step: A_Time { value: 1, scale: 1 },
		field: 0,
		down_x: 1,
		down_y: 1,
		roi: A_LRect {
			left: 0,
			top: 0,
			right: 0,
			bottom: 0,
		},
		matte: 0,
		channel_order: 0,
		guide_layers: 0,
		quality: AEGP_ItemQuality_BEST as AEGP_ItemQuality,
	}
}

unsafe extern "C" fn comp_new_from_item(
	_plugin_id: AEGP_PluginID,
	itemH: AEGP_ItemH,
	options_out: *mut AEGP_RenderOptionsH,
) -> A_Err {
	let Some(out) = (unsafe { options_out.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	if PluginInstance::get_instance_ptr(itemH as PF_ProgPtr).is_none() {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	}
	*out = publish_comp_options(fresh_comp_options(itemH as PF_ProgPtr));
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn comp_duplicate(
	_plugin_id: AEGP_PluginID,
	optionsH: AEGP_RenderOptionsH,
	copy_out: *mut AEGP_RenderOptionsH,
) -> A_Err {
	let (Some(options), Some(out)) = (unsafe { comp_options_mut(optionsH) }, unsafe { copy_out.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	*out = publish_comp_options(CompOptions {
		effect_ref: options.effect_ref,
		world_type: options.world_type,
		time: options.time,
		time_step: options.time_step,
		field: options.field,
		down_x: options.down_x,
		down_y: options.down_y,
		roi: options.roi,
		matte: options.matte,
		channel_order: options.channel_order,
		guide_layers: options.guide_layers,
		quality: options.quality,
	});
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn dispose_comp_options(optionsH: AEGP_RenderOptionsH) -> A_Err {
	if !comp_options_set().remove(&(optionsH as usize)) {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	}
	let slot = unsafe { Box::from_raw(optionsH as *mut *mut CompOptions) };
	if !slot.is_null() {
		drop(unsafe { Box::from_raw(*slot) });
	}
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn comp_set_time(optionsH: AEGP_RenderOptionsH, time: A_Time) -> A_Err {
	let Some(options) = (unsafe { comp_options_mut(optionsH) }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	options.time = time;
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn comp_get_time(optionsH: AEGP_RenderOptionsH, time_out: *mut A_Time) -> A_Err {
	let (Some(options), Some(out)) = (unsafe { comp_options_mut(optionsH) }, unsafe { time_out.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	*out = options.time;
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn comp_set_time_step(optionsH: AEGP_RenderOptionsH, time_step: A_Time) -> A_Err {
	let Some(options) = (unsafe { comp_options_mut(optionsH) }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	options.time_step = time_step;
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn comp_get_time_step(optionsH: AEGP_RenderOptionsH, time_out: *mut A_Time) -> A_Err {
	let (Some(options), Some(out)) = (unsafe { comp_options_mut(optionsH) }, unsafe { time_out.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	*out = options.time_step;
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn comp_set_field(optionsH: AEGP_RenderOptionsH, field: PF_Field) -> A_Err {
	let Some(options) = (unsafe { comp_options_mut(optionsH) }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	options.field = field;
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn comp_get_field(optionsH: AEGP_RenderOptionsH, field_out: *mut PF_Field) -> A_Err {
	let (Some(options), Some(out)) = (unsafe { comp_options_mut(optionsH) }, unsafe { field_out.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	*out = options.field;
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn comp_set_world_type(optionsH: AEGP_RenderOptionsH, kind: AEGP_WorldType) -> A_Err {
	let Some(options) = (unsafe { comp_options_mut(optionsH) }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	options.world_type = kind;
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn comp_get_world_type(optionsH: AEGP_RenderOptionsH, kind_out: *mut AEGP_WorldType) -> A_Err {
	let (Some(options), Some(out)) = (unsafe { comp_options_mut(optionsH) }, unsafe { kind_out.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	*out = options.world_type;
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn comp_set_downsample(optionsH: AEGP_RenderOptionsH, x: A_short, y: A_short) -> A_Err {
	let Some(options) = (unsafe { comp_options_mut(optionsH) }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	options.down_x = x;
	options.down_y = y;
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn comp_get_downsample(
	optionsH: AEGP_RenderOptionsH,
	x_out: *mut A_short,
	y_out: *mut A_short,
) -> A_Err {
	let Some(options) = (unsafe { comp_options_mut(optionsH) }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	if let Some(x) = unsafe { x_out.as_mut() } {
		*x = options.down_x;
	}
	if let Some(y) = unsafe { y_out.as_mut() } {
		*y = options.down_y;
	}
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn comp_set_roi(optionsH: AEGP_RenderOptionsH, roi: *const A_LRect) -> A_Err {
	let (Some(options), Some(roi)) = (unsafe { comp_options_mut(optionsH) }, unsafe { roi.as_ref() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	options.roi = *roi;
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn comp_get_roi(optionsH: AEGP_RenderOptionsH, roi_out: *mut A_LRect) -> A_Err {
	let (Some(options), Some(out)) = (unsafe { comp_options_mut(optionsH) }, unsafe { roi_out.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	*out = options.roi;
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn comp_set_matte(optionsH: AEGP_RenderOptionsH, mode: AEGP_MatteMode) -> A_Err {
	let Some(options) = (unsafe { comp_options_mut(optionsH) }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	options.matte = mode;
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn comp_get_matte(optionsH: AEGP_RenderOptionsH, mode_out: *mut AEGP_MatteMode) -> A_Err {
	let (Some(options), Some(out)) = (unsafe { comp_options_mut(optionsH) }, unsafe { mode_out.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	*out = options.matte;
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn comp_set_channel_order(optionsH: AEGP_RenderOptionsH, order: AEGP_ChannelOrder) -> A_Err {
	let Some(options) = (unsafe { comp_options_mut(optionsH) }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	options.channel_order = order;
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn comp_get_channel_order(optionsH: AEGP_RenderOptionsH, order_out: *mut AEGP_ChannelOrder) -> A_Err {
	let (Some(options), Some(out)) = (unsafe { comp_options_mut(optionsH) }, unsafe { order_out.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	*out = options.channel_order;
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn comp_get_guide_layers(optionsH: AEGP_RenderOptionsH, render_out: *mut A_Boolean) -> A_Err {
	let (Some(options), Some(out)) = (unsafe { comp_options_mut(optionsH) }, unsafe { render_out.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	*out = options.guide_layers;
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn comp_set_guide_layers(optionsH: AEGP_RenderOptionsH, render: A_Boolean) -> A_Err {
	let Some(options) = (unsafe { comp_options_mut(optionsH) }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	options.guide_layers = render;
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn comp_get_quality(optionsH: AEGP_RenderOptionsH, quality_out: *mut AEGP_ItemQuality) -> A_Err {
	let (Some(options), Some(out)) = (unsafe { comp_options_mut(optionsH) }, unsafe { quality_out.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	*out = options.quality;
	PF_Err_NONE as A_Err
}

unsafe extern "C" fn comp_set_quality(optionsH: AEGP_RenderOptionsH, quality: AEGP_ItemQuality) -> A_Err {
	let Some(options) = (unsafe { comp_options_mut(optionsH) }) else {
		return PF_Err_BAD_CALLBACK_PARAM as A_Err;
	};
	options.quality = quality;
	PF_Err_NONE as A_Err
}

pub const fn render_options_suite_4() -> AEGP_RenderOptionsSuite4 {
	AEGP_RenderOptionsSuite4 {
		AEGP_NewFromItem: Some(comp_new_from_item),
		AEGP_Duplicate: Some(comp_duplicate),
		AEGP_Dispose: Some(dispose_comp_options),
		AEGP_SetTime: Some(comp_set_time),
		AEGP_GetTime: Some(comp_get_time),
		AEGP_SetTimeStep: Some(comp_set_time_step),
		AEGP_GetTimeStep: Some(comp_get_time_step),
		AEGP_SetFieldRender: Some(comp_set_field),
		AEGP_GetFieldRender: Some(comp_get_field),
		AEGP_SetWorldType: Some(comp_set_world_type),
		AEGP_GetWorldType: Some(comp_get_world_type),
		AEGP_SetDownsampleFactor: Some(comp_set_downsample),
		AEGP_GetDownsampleFactor: Some(comp_get_downsample),
		AEGP_SetRegionOfInterest: Some(comp_set_roi),
		AEGP_GetRegionOfInterest: Some(comp_get_roi),
		AEGP_SetMatteMode: Some(comp_set_matte),
		AEGP_GetMatteMode: Some(comp_get_matte),
		AEGP_SetChannelOrder: Some(comp_set_channel_order),
		AEGP_GetChannelOrder: Some(comp_get_channel_order),
		AEGP_GetRenderGuideLayers: Some(comp_get_guide_layers),
		AEGP_SetRenderGuideLayers: Some(comp_set_guide_layers),
		AEGP_GetRenderQuality: Some(comp_get_quality),
		AEGP_SetRenderQuality: Some(comp_set_quality),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn render_options_reject_an_unknown_item() {
		let suite = render_options_suite_4();
		let mut options: AEGP_RenderOptionsH = std::ptr::null_mut();
		let err = unsafe { suite.AEGP_NewFromItem.unwrap()(0, std::ptr::null_mut(), &mut options) };
		assert_eq!(err, PF_Err_BAD_CALLBACK_PARAM as A_Err);
		assert!(options.is_null());
	}

	#[test]
	fn render_options_keep_the_requested_depth() {
		let suite = render_options_suite_4();
		let options = publish_comp_options(fresh_comp_options(std::ptr::null_mut()));
		let err = unsafe { suite.AEGP_SetWorldType.unwrap()(options, 2) };
		assert_eq!(err, PF_Err_NONE as A_Err);
		let mut kind = 0;
		let err = unsafe { suite.AEGP_GetWorldType.unwrap()(options, &mut kind) };
		assert_eq!(err, PF_Err_NONE as A_Err);
		assert_eq!(kind, 2);
		let mut receipt = std::ptr::null_mut();
		let err = unsafe {
			render_suite_5().AEGP_RenderAndCheckoutFrame.unwrap()(options, None, std::ptr::null_mut(), &mut receipt)
		};
		assert_eq!(err, PF_Err_BAD_CALLBACK_PARAM as A_Err);
		assert_eq!(unsafe { suite.AEGP_Dispose.unwrap()(options) }, PF_Err_NONE as A_Err);
	}
}
