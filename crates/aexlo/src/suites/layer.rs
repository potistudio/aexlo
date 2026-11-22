//! `AEGP Layer Suite` (wire versions 5 and 8, `AEGP_LayerSuite1` and `3`, and
//! wire versions 11 to 15, `AEGP_LayerSuite5` to `9`) and `AEGP Light Suite` v1, and the
//! [`Timeline`] every suite that describes the project answers from.
//!
//! The project is the embedder's (`PluginInstance::set_project`). Without one,
//! the effect is on a single layer that is the whole comp: the comp is the
//! input layer's size, the layer starts at comp time 0 with no stretch (so
//! layer time is comp time), runs for [`COMP_DURATION_SECONDS`], is 2D with an
//! identity transform and has no parent. It is an AV layer, or a text layer
//! once the embedder gives it text (`PluginInstance::set_text_layer`). Its
//! synthetic footage source is the input image. The comp's frame rate and the
//! current time are the instance's render context (frame 0 at 30 fps by
//! default).
//!
//! Handles: the comp, its item and the layer the effect is on are the effect's
//! `PF_ProgPtr` itself. After Effects never asks plugins to dispose of them,
//! so there is nothing to allocate. Every other layer has a [`LayerHandle`].
//! Both are good only while their instance is inside a plugin call. What is
//! not known is refused, never made up, and so is every edit.

use std::cell::Cell;

use crate::sys::*;

use crate::PluginInstance;
use crate::animation::TimePoint;
use crate::core::diagnostics::diag;
use crate::project::{self, LayerContent, PropertyTree};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{LazyLock, Mutex};

// Tokens are only resolved while their owning instance is executing a plugin call.
static SOURCE_ITEMS: LazyLock<Mutex<HashMap<usize, (usize, usize)>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

thread_local! {
	static CURRENT_EFFECT: Cell<PF_ProgPtr> = const { Cell::new(std::ptr::null_mut()) };
}

/// Restores the effect that was current before [`enter_effect`].
pub(crate) struct EffectGuard(PF_ProgPtr, usize);

/// The effect behind the plugin call on this thread. Suite callbacks that take
/// no effect reference (`AEGP_GetActiveItem`) recover the instance from it.
pub(crate) fn enter_effect(effect_ref: PF_ProgPtr, source: *mut std::ffi::c_void) -> EffectGuard {
	let token = source as usize;
	let mut items = SOURCE_ITEMS.lock().unwrap();
	let entry = items.entry(token).or_insert((effect_ref as usize, 0));
	entry.0 = effect_ref as usize;
	entry.1 += 1;
	EffectGuard(CURRENT_EFFECT.with(|slot| slot.replace(effect_ref)), token)
}

impl Drop for EffectGuard {
	fn drop(&mut self) {
		let mut items = SOURCE_ITEMS.lock().unwrap();
		if let Some(entry) = items.get_mut(&self.1) {
			entry.1 -= 1;
			if entry.1 == 0 {
				items.remove(&self.1);
			}
		}
		CURRENT_EFFECT.with(|slot| slot.set(self.0));
	}
}

pub(crate) fn current_effect() -> PF_ProgPtr {
	CURRENT_EFFECT.with(Cell::get)
}

/// Length of aexlo's comp, and of the layer that fills it.
pub(crate) const COMP_DURATION_SECONDS: A_long = 30;

/// The id of the layer when there is no project (`AEGP_GetLayerID`).
const LAYER_ID: u32 = 1;

/// Handle -> (the owning instance's token, layer id), for layers the effect is
/// not on.
static LAYER_HANDLES: LazyLock<Mutex<HashMap<usize, (usize, u32)>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// The handle of a layer the effect is not on, refused once dropped.
///
/// It is a number, not an address, so it is never given out again and cannot
/// come to name another layer.
#[derive(Debug)]
pub(crate) struct LayerHandle(usize);

impl LayerHandle {
	pub(crate) fn new(owner: usize, id: u32) -> Self {
		static NEXT: AtomicUsize = AtomicUsize::new(1);
		// Odd, so never the address of an instance or of an item token.
		let handle = NEXT.fetch_add(1, Ordering::Relaxed) << 4 | 0xb;
		LAYER_HANDLES.lock().unwrap().insert(handle, (owner, id));
		Self(handle)
	}

	fn get(&self) -> AEGP_LayerH {
		self.0 as AEGP_LayerH
	}
}

impl Drop for LayerHandle {
	fn drop(&mut self) {
		LAYER_HANDLES.lock().unwrap().remove(&self.0);
	}
}

/// The layer handle for the effect behind `effect_ref`.
pub(crate) fn layer_handle(effect_ref: PF_ProgPtr) -> AEGP_LayerH {
	effect_ref as AEGP_LayerH
}

/// The comp handle for the effect behind `effect_ref`.
pub(crate) fn comp_handle(effect_ref: PF_ProgPtr) -> AEGP_CompH {
	effect_ref as AEGP_CompH
}

/// The project item for that comp. Same pointer: aexlo has one item, the comp.
pub(crate) fn item_handle(effect_ref: PF_ProgPtr) -> AEGP_ItemH {
	effect_ref as AEGP_ItemH
}

pub(crate) fn source_item_handle(effect_ref: PF_ProgPtr) -> AEGP_ItemH {
	SOURCE_ITEMS
		.lock()
		.unwrap()
		.iter()
		.find_map(|(token, (owner, _))| (*owner == effect_ref as usize).then_some(*token as AEGP_ItemH))
		.unwrap_or(std::ptr::null_mut())
}

pub(crate) fn source_item_effect(item: AEGP_ItemH) -> Option<PF_ProgPtr> {
	SOURCE_ITEMS
		.lock()
		.unwrap()
		.get(&(item as usize))
		.map(|(owner, _)| *owner as PF_ProgPtr)
}

/// Whether the instance behind `effect` is inside a plugin call.
fn is_active(effect: usize) -> bool {
	SOURCE_ITEMS.lock().unwrap().values().any(|(owner, _)| *owner == effect)
}

/// A layer handle as its effect and, for a layer the effect is not on, its id.
fn resolve(handle: *mut std::ffi::c_void) -> Option<(PF_ProgPtr, Option<u32>)> {
	let key = handle as usize;
	if is_active(key) {
		return Some((handle as PF_ProgPtr, None));
	}
	let (owner, id) = LAYER_HANDLES.lock().unwrap().get(&key).copied()?;
	let effect = SOURCE_ITEMS.lock().unwrap().get(&owner).map(|(effect, _)| *effect)?;
	Some((effect as PF_ProgPtr, Some(id)))
}

/// The instance behind an active effect.
///
/// # Safety
/// `effect` must come from [`resolve`], or be an effect found active, during
/// the suite call the borrow is used in.
unsafe fn instance_of<'a>(effect: PF_ProgPtr) -> Option<&'a PluginInstance> {
	// The instance is inside a plugin call, so it is alive, and its project
	// and layers cannot be replaced until that call returns.
	PluginInstance::get_instance_ptr(effect).map(|instance| unsafe { instance.as_ref() })
}

/// What a layer is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
	Footage,
	Null,
	Text,
	Shape,
}

impl Kind {
	fn of(content: &LayerContent) -> Self {
		match content {
			LayerContent::Footage => Self::Footage,
			LayerContent::Null => Self::Null,
			LayerContent::Text(_) => Self::Text,
			LayerContent::Shape(_) => Self::Shape,
		}
	}
}

/// What the timeline looks like from one layer of the comp around an effect.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Timeline {
	pub effect: PF_ProgPtr,
	/// The layer's id.
	pub id: u32,
	/// Whether it is the layer the effect is on.
	pub hosts_effect: bool,
	/// Whether an embedder's project describes it.
	pub described: bool,
	pub kind: Kind,
	pub visible: bool,
	pub index: usize,
	pub layers: usize,
	/// The layer's size.
	pub width: u32,
	pub height: u32,
	pub comp_width: u32,
	pub comp_height: u32,
	pub background: [f64; 4],
	/// A frame is `time_step / time_scale` seconds.
	pub time_step: A_long,
	pub time_scale: A_u_long,
	/// Now, in comp time.
	pub comp_time: TimePoint,
	/// Now, in this layer's time.
	pub layer_time: TimePoint,
	start: TimePoint,
	stretch: (i32, u32),
	/// In comp time.
	in_point: TimePoint,
	out_point: TimePoint,
	comp_duration: TimePoint,
}

impl Timeline {
	/// The layer behind a layer handle, or `None` for one that is null, stale
	/// or of an effect that is not inside a plugin call.
	pub(crate) fn of(handle: *mut std::ffi::c_void) -> Option<Self> {
		let (effect, id) = resolve(handle)?;
		Self::read(effect, id)
	}

	/// The effect's own layer, from a handle that can only be the effect's: a
	/// comp, project or item handle, or an effect reference.
	pub(crate) fn of_effect(handle: *mut std::ffi::c_void) -> Option<Self> {
		if !is_active(handle as usize) {
			return None;
		}
		Self::read(handle as PF_ProgPtr, None)
	}

	fn read(effect: PF_ProgPtr, id: Option<u32>) -> Option<Self> {
		// SAFETY: callers pass an effect they found active.
		let instance = unsafe { instance_of(effect) }?;
		let (value, time_step, time_scale) = instance.render_context.time();
		let render_time = TimePoint::new(value, time_scale)?;
		let Some(state) = instance.project_state() else {
			if id.is_some() {
				return None;
			}
			let (width, height) = instance.input_size();
			let text = instance.text_layer().is_some();
			return Some(Self::alone(
				effect,
				(width, height),
				render_time,
				time_step,
				time_scale,
				text,
			));
		};
		let project = &state.project;
		let host = project.effect_layer();
		let id = id.unwrap_or(host.id);
		let index = project.layers().iter().position(|layer| layer.id == id)?;
		let layer = &project.layers()[index];
		let comp = project.comp();
		// The effect is told the time in its layer's time; everyone else's
		// follows from the comp time that is.
		let comp_time = host.to_comp_time(render_time);
		let hosts_effect = id == host.id;
		Some(Self {
			effect,
			id,
			hosts_effect,
			described: true,
			kind: Kind::of(&layer.content),
			visible: layer.visible,
			index,
			layers: project.layers().len(),
			width: layer.width,
			height: layer.height,
			comp_width: comp.width,
			comp_height: comp.height,
			background: comp.background,
			time_step,
			time_scale,
			comp_time,
			layer_time: if hosts_effect {
				render_time
			} else {
				layer.to_layer_time(comp_time)
			},
			start: layer.start,
			stretch: layer.stretch,
			in_point: layer.in_point,
			out_point: layer.out_point,
			comp_duration: comp.duration,
		})
	}

	/// The timeline without a project: the layer is the comp.
	fn alone(
		effect: PF_ProgPtr,
		(width, height): (u32, u32),
		now: TimePoint,
		time_step: A_long,
		time_scale: A_u_long,
		text: bool,
	) -> Self {
		let scale = time_scale.max(1);
		let zero = TimePoint::from_ratio(0, 1, scale);
		let duration = TimePoint::from_ratio(i128::from(COMP_DURATION_SECONDS), 1, scale);
		Self {
			effect,
			id: LAYER_ID,
			hosts_effect: true,
			described: false,
			kind: if text { Kind::Text } else { Kind::Footage },
			visible: true,
			index: 0,
			layers: 1,
			width,
			height,
			comp_width: width,
			comp_height: height,
			background: [0.0, 0.0, 0.0, 1.0],
			time_step,
			time_scale,
			comp_time: now,
			layer_time: now,
			start: zero,
			stretch: (1, 1),
			in_point: zero,
			out_point: duration,
			comp_duration: duration,
		}
	}

	pub(crate) fn to_layer_time(self, comp: TimePoint) -> TimePoint {
		project::to_layer_time(self.start, self.stretch, comp)
	}

	pub(crate) fn to_comp_time(self, layer: TimePoint) -> TimePoint {
		project::to_comp_time(self.start, self.stretch, layer)
	}

	/// `time`, given in `mode`, in layer time.
	pub(crate) fn layer_time_of(self, time: TimePoint, mode: AEGP_LTimeMode) -> TimePoint {
		if is_comp_time(mode) {
			self.to_layer_time(time)
		} else {
			time
		}
	}

	/// The layer time `time`, told in `mode`.
	pub(crate) fn told_in(self, time: TimePoint, mode: AEGP_LTimeMode) -> TimePoint {
		if is_comp_time(mode) {
			self.to_comp_time(time)
		} else {
			time
		}
	}

	/// A comp time that is a whole number of the comp's units where it can be.
	fn in_comp_units(self, time: TimePoint) -> A_Time {
		TimePoint::from_ratio(i128::from(time.value()), i128::from(time.scale()), self.time_scale).to_sys()
	}

	fn current_in(&self, mode: AEGP_LTimeMode) -> A_Time {
		if is_comp_time(mode) {
			self.comp_time.to_sys()
		} else {
			self.layer_time.to_sys()
		}
	}

	fn in_point_in(&self, mode: AEGP_LTimeMode) -> A_Time {
		if is_comp_time(mode) {
			self.in_comp_units(self.in_point)
		} else {
			self.in_comp_units(self.to_layer_time(self.in_point))
		}
	}

	/// Negative in layer time for a layer played backwards.
	fn duration_in(&self, mode: AEGP_LTimeMode) -> A_Time {
		let span = if is_comp_time(mode) {
			self.out_point.since(self.in_point)
		} else {
			self.to_layer_time(self.out_point)
				.since(self.to_layer_time(self.in_point))
		};
		self.in_comp_units(span)
	}

	fn offset(&self) -> A_Time {
		self.in_comp_units(self.start)
	}

	pub(crate) fn comp_now(&self) -> A_Time {
		self.comp_time.to_sys()
	}

	pub(crate) fn comp_length(&self) -> A_Time {
		self.in_comp_units(self.comp_duration)
	}

	/// The frame `time` falls on.
	fn frame_of(&self, time: &A_Time) -> i64 {
		if time.scale == 0 || self.time_step == 0 {
			return 0;
		}
		(time.value as i64 * self.time_scale as i64).div_euclid(time.scale as i64 * self.time_step as i64)
	}
}

fn is_comp_time(mode: AEGP_LTimeMode) -> bool {
	mode == AEGP_LTimeMode_CompTime as AEGP_LTimeMode
}

/// What an item handle names.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Item {
	pub width: u32,
	pub height: u32,
	pub duration: A_Time,
	pub current: A_Time,
}

impl Item {
	/// The comp, or the footage under the effect's layer.
	pub(crate) fn of(handle: AEGP_ItemH) -> Option<Self> {
		let source = source_item_effect(handle);
		let timeline = Timeline::of_effect(source.unwrap_or(handle as PF_ProgPtr).cast())?;
		let layer_time = AEGP_LTimeMode_LayerTime as AEGP_LTimeMode;
		Some(if source.is_some() {
			Self {
				width: timeline.width,
				height: timeline.height,
				duration: timeline.duration_in(layer_time),
				current: timeline.current_in(layer_time),
			}
		} else {
			Self {
				width: timeline.comp_width,
				height: timeline.comp_height,
				duration: timeline.comp_length(),
				current: timeline.comp_now(),
			}
		})
	}
}

/// The handle of layer `id` of the comp around `effect`.
pub(crate) fn handle_of_layer(effect: PF_ProgPtr, id: u32) -> Option<AEGP_LayerH> {
	if !is_active(effect as usize) {
		return None;
	}
	// SAFETY: found active.
	match unsafe { instance_of(effect) }?.project_state() {
		None => (id == LAYER_ID).then(|| layer_handle(effect)),
		Some(state) => Some(match &state.layer(id)?.handle {
			Some(handle) => handle.get(),
			None => layer_handle(effect),
		}),
	}
}

/// The outlines of the text layer behind `handle`, at `time` in layer time or
/// now. `None` when it is no text layer.
pub(crate) fn text_at(handle: AEGP_LayerH, time: Option<TimePoint>) -> Option<crate::TextLayer> {
	let timeline = Timeline::of(handle.cast())?;
	// SAFETY: `Timeline::of` found the effect active.
	let instance = unsafe { instance_of(timeline.effect) }?;
	match instance.project() {
		None => instance.text_layer().cloned(),
		Some(project) => match &project.layer(timeline.id)?.content {
			LayerContent::Text(text) => Some(text.at(time.unwrap_or(timeline.layer_time))),
			_ => None,
		},
	}
}

/// Run `f` over the property tree and its revision of a layer's contents.
pub(crate) fn with_contents<R>(timeline: &Timeline, f: impl FnOnce(&PropertyTree, u64) -> R) -> Option<R> {
	if !is_active(timeline.effect as usize) {
		return None;
	}
	// SAFETY: found active.
	let instance = unsafe { instance_of(timeline.effect) }?;
	match instance.project_state() {
		Some(state) => state.layer(timeline.id).map(|layer| f(&layer.tree, layer.revision)),
		None => {
			let tree = instance.text_layer().map_or_else(PropertyTree::default, |text| {
				PropertyTree::of(&LayerContent::Text(text.clone().into()))
			});
			Some(f(&tree, 0))
		}
	}
}

fn declined(name: &str, why: &str) {
	crate::observe::unsupported(
		crate::observe::UnsupportedKind::Declined,
		format!("AEGP_LayerSuite/{name}"),
		why,
	);
}

fn unsupported(name: &str) -> A_Err {
	declined(name, "the project is read-only");
	A_Err_GENERIC
}

/// Resolve a layer `handle` and write `value(timeline)` to `out`, or report a
/// bad handle or a null out-param.
pub(crate) fn answer<T>(handle: *mut std::ffi::c_void, out: *mut T, value: impl FnOnce(&Timeline) -> T) -> A_Err {
	write(Timeline::of(handle), out, value)
}

/// [`answer`], for a comp or project handle.
pub(crate) fn answer_comp<T>(handle: *mut std::ffi::c_void, out: *mut T, value: impl FnOnce(&Timeline) -> T) -> A_Err {
	write(Timeline::of_effect(handle), out, value)
}

fn write<T>(timeline: Option<Timeline>, out: *mut T, value: impl FnOnce(&Timeline) -> T) -> A_Err {
	let (Some(timeline), Some(out)) = (timeline, unsafe { out.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM;
	};
	*out = value(&timeline);
	PF_Err_NONE
}

unsafe extern "C" fn get_comp_num_layers(compH: AEGP_CompH, num_layersPL: *mut A_long) -> A_Err {
	crate::core::ffi::guard("get_comp_num_layers", || {
		diag!("AEGP_LayerSuite/AEGP_GetCompNumLayers", "compH" => format!("{:#x}", compH as usize));
		answer_comp(compH as _, num_layersPL, |t| t.layers as A_long)
	})
}

unsafe extern "C" fn get_comp_layer_by_index(
	compH: AEGP_CompH,
	layer_indexL: A_long,
	layerPH: *mut AEGP_LayerH,
) -> A_Err {
	crate::core::ffi::guard("get_comp_layer_by_index", || {
		diag!("AEGP_LayerSuite/AEGP_GetCompLayerByIndex",
			"compH" => format!("{:#x}", compH as usize),
			"layer_indexL" => layer_indexL,
		);
		let (Some(timeline), Some(out)) = (Timeline::of_effect(compH as _), unsafe { layerPH.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		let Some(index) = usize::try_from(layer_indexL)
			.ok()
			.filter(|&index| index < timeline.layers)
		else {
			return PF_Err_INVALID_INDEX;
		};
		// SAFETY: `of_effect` found the effect active.
		let project = unsafe { instance_of(timeline.effect) }.and_then(PluginInstance::project);
		let id = project.map_or(LAYER_ID, |project| project.layers()[index].id);
		let Some(handle) = handle_of_layer(timeline.effect, id) else {
			return PF_Err_INVALID_INDEX;
		};
		*out = handle;
		PF_Err_NONE
	})
}

unsafe extern "C" fn get_active_layer(_layerPH: *mut AEGP_LayerH) -> A_Err {
	crate::core::ffi::guard("get_active_layer", || {
		diag!("AEGP_LayerSuite/AEGP_GetActiveLayer");
		unsupported("AEGP_GetActiveLayer")
	})
}

unsafe extern "C" fn get_layer_index(layerH: AEGP_LayerH, layer_indexPL: *mut A_long) -> A_Err {
	crate::core::ffi::guard("get_layer_index", || {
		diag!("AEGP_LayerSuite/AEGP_GetLayerIndex", "layerH" => format!("{:#x}", layerH as usize));
		answer(layerH as _, layer_indexPL, |t| t.index as A_long)
	})
}

/// What a layer has as its source item.
enum Source {
	/// The synthetic footage item of the input image.
	Input,
	/// None, as After Effects has for text and shape layers.
	None,
	/// The project does not say.
	Unknown,
}

fn source_of(timeline: &Timeline) -> Source {
	match timeline.kind {
		_ if !timeline.described => Source::Input,
		Kind::Text | Kind::Shape => Source::None,
		_ if timeline.hosts_effect => Source::Input,
		_ => Source::Unknown,
	}
}

unsafe extern "C" fn get_layer_source_item(layerH: AEGP_LayerH, source_itemPH: *mut AEGP_ItemH) -> A_Err {
	crate::core::ffi::guard("get_layer_source_item", || {
		diag!("AEGP_LayerSuite/AEGP_GetLayerSourceItem");
		let Some(out) = (unsafe { source_itemPH.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		*out = std::ptr::null_mut();
		let Some(timeline) = Timeline::of(layerH as _) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		match source_of(&timeline) {
			Source::Input => {
				*out = source_item_handle(timeline.effect);
				PF_Err_NONE
			}
			Source::None => PF_Err_NONE,
			Source::Unknown => {
				declined(
					"AEGP_GetLayerSourceItem",
					"only the footage under the effect's layer is known",
				);
				A_Err_GENERIC
			}
		}
	})
}

unsafe extern "C" fn get_layer_source_item_id(layerH: AEGP_LayerH, source_item_idPL: *mut A_long) -> A_Err {
	crate::core::ffi::guard("get_layer_source_item_id", || {
		diag!("AEGP_LayerSuite/AEGP_GetLayerSourceItemID");
		let (Some(timeline), Some(out)) = (Timeline::of(layerH as _), unsafe { source_item_idPL.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		*out = match source_of(&timeline) {
			Source::Input => 2,
			Source::None => 0,
			Source::Unknown => {
				declined(
					"AEGP_GetLayerSourceItemID",
					"only the footage under the effect's layer is known",
				);
				return A_Err_GENERIC;
			}
		};
		PF_Err_NONE
	})
}

unsafe extern "C" fn get_layer_parent_comp(layerH: AEGP_LayerH, compPH: *mut AEGP_CompH) -> A_Err {
	crate::core::ffi::guard("get_layer_parent_comp", || {
		diag!("AEGP_LayerSuite/AEGP_GetLayerParentComp", "layerH" => format!("{:#x}", layerH as usize));
		answer(layerH as _, compPH, |t| comp_handle(t.effect))
	})
}

/// Without a project the layer has never been renamed. A source is never
/// named: the project does not say.
unsafe extern "C" fn get_layer_name(
	layerH: AEGP_LayerH,
	layer_nameZ0: *mut A_char,
	source_nameZ0: *mut A_char,
) -> A_Err {
	crate::core::ffi::guard("get_layer_name", || {
		diag!("AEGP_LayerSuite/AEGP_GetLayerName", "layerH" => format!("{:#x}", layerH as usize));
		let Some(timeline) = Timeline::of(layerH as _) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		let name = layer_name(&timeline);
		unsafe {
			if !layer_nameZ0.is_null() {
				let buf = std::slice::from_raw_parts_mut(layer_nameZ0, AEGP_MAX_LAYER_NAME_SIZE as usize);
				super::ffi::write_c_str(buf, &name);
			}
			if let Some(out) = source_nameZ0.as_mut() {
				*out = 0;
			}
		}
		PF_Err_NONE
	})
}

fn layer_name(timeline: &Timeline) -> String {
	// SAFETY: the timeline was just read from an active effect.
	unsafe { instance_of(timeline.effect) }
		.and_then(PluginInstance::project)
		.and_then(|project| project.layer(timeline.id))
		.map(|layer| layer.name.clone())
		.unwrap_or_default()
}

unsafe extern "C" fn get_layer_quality(layerH: AEGP_LayerH, qualityP: *mut AEGP_LayerQuality) -> A_Err {
	crate::core::ffi::guard("get_layer_quality", || {
		diag!("AEGP_LayerSuite/AEGP_GetLayerQuality", "layerH" => format!("{:#x}", layerH as usize));
		answer(layerH as _, qualityP, |_| AEGP_LayerQual_BEST as AEGP_LayerQuality)
	})
}

unsafe extern "C" fn set_layer_quality(_layerH: AEGP_LayerH, _quality: AEGP_LayerQuality) -> A_Err {
	crate::core::ffi::guard("set_layer_quality", || {
		diag!("AEGP_LayerSuite/AEGP_SetLayerQuality");
		unsupported("AEGP_SetLayerQuality")
	})
}

unsafe extern "C" fn get_layer_flags(layerH: AEGP_LayerH, layer_flagsP: *mut AEGP_LayerFlags) -> A_Err {
	crate::core::ffi::guard("get_layer_flags", || {
		diag!("AEGP_LayerSuite/AEGP_GetLayerFlags", "layerH" => format!("{:#x}", layerH as usize));
		answer(layerH as _, layer_flagsP, |t| {
			let mut flags = 0;
			if t.visible {
				flags |= AEGP_LayerFlag_VIDEO_ACTIVE;
			}
			if t.kind == Kind::Null {
				flags |= AEGP_LayerFlag_NULL_LAYER;
			}
			flags
		})
	})
}

unsafe extern "C" fn set_layer_flag(_layerH: AEGP_LayerH, _single_flag: AEGP_LayerFlags, _valueB: A_Boolean) -> A_Err {
	crate::core::ffi::guard("set_layer_flag", || {
		diag!("AEGP_LayerSuite/AEGP_SetLayerFlag");
		unsupported("AEGP_SetLayerFlag")
	})
}

unsafe extern "C" fn is_layer_video_really_on(layerH: AEGP_LayerH, onPB: *mut A_Boolean) -> A_Err {
	crate::core::ffi::guard("is_layer_video_really_on", || {
		diag!("AEGP_LayerSuite/AEGP_IsLayerVideoReallyOn", "layerH" => format!("{:#x}", layerH as usize));
		answer(layerH as _, onPB, |t| t.visible as A_Boolean)
	})
}

unsafe extern "C" fn is_layer_audio_really_on(layerH: AEGP_LayerH, onPB: *mut A_Boolean) -> A_Err {
	crate::core::ffi::guard("is_layer_audio_really_on", || {
		diag!("AEGP_LayerSuite/AEGP_IsLayerAudioReallyOn", "layerH" => format!("{:#x}", layerH as usize));
		answer(layerH as _, onPB, |_| 0)
	})
}

unsafe extern "C" fn get_layer_current_time(
	layerH: AEGP_LayerH,
	time_mode: AEGP_LTimeMode,
	curr_timePT: *mut A_Time,
) -> A_Err {
	crate::core::ffi::guard("get_layer_current_time", || {
		diag!("AEGP_LayerSuite/AEGP_GetLayerCurrentTime", "layerH" => format!("{:#x}", layerH as usize));
		answer(layerH as _, curr_timePT, |t| t.current_in(time_mode))
	})
}

unsafe extern "C" fn get_layer_in_point(
	layerH: AEGP_LayerH,
	time_mode: AEGP_LTimeMode,
	in_pointPT: *mut A_Time,
) -> A_Err {
	crate::core::ffi::guard("get_layer_in_point", || {
		diag!("AEGP_LayerSuite/AEGP_GetLayerInPoint", "layerH" => format!("{:#x}", layerH as usize));
		answer(layerH as _, in_pointPT, |t| t.in_point_in(time_mode))
	})
}

unsafe extern "C" fn get_layer_duration(
	layerH: AEGP_LayerH,
	time_mode: AEGP_LTimeMode,
	durationPT: *mut A_Time,
) -> A_Err {
	crate::core::ffi::guard("get_layer_duration", || {
		diag!("AEGP_LayerSuite/AEGP_GetLayerDuration", "layerH" => format!("{:#x}", layerH as usize));
		answer(layerH as _, durationPT, |t| t.duration_in(time_mode))
	})
}

unsafe extern "C" fn set_layer_in_point_and_duration(
	_layerH: AEGP_LayerH,
	_time_mode: AEGP_LTimeMode,
	_in_pointPT: *const A_Time,
	_durationPT: *const A_Time,
) -> A_Err {
	crate::core::ffi::guard("set_layer_in_point_and_duration", || {
		diag!("AEGP_LayerSuite/AEGP_SetLayerInPointAndDuration");
		unsupported("AEGP_SetLayerInPointAndDuration")
	})
}

unsafe extern "C" fn get_layer_offset(layerH: AEGP_LayerH, offsetPT: *mut A_Time) -> A_Err {
	crate::core::ffi::guard("get_layer_offset", || {
		diag!("AEGP_LayerSuite/AEGP_GetLayerOffset", "layerH" => format!("{:#x}", layerH as usize));
		answer(layerH as _, offsetPT, Timeline::offset)
	})
}

unsafe extern "C" fn set_layer_offset(_layerH: AEGP_LayerH, _offsetPT: *const A_Time) -> A_Err {
	crate::core::ffi::guard("set_layer_offset", || {
		diag!("AEGP_LayerSuite/AEGP_SetLayerOffset");
		unsupported("AEGP_SetLayerOffset")
	})
}

unsafe extern "C" fn get_layer_stretch(layerH: AEGP_LayerH, stretchPRt: *mut A_Ratio) -> A_Err {
	crate::core::ffi::guard("get_layer_stretch", || {
		diag!("AEGP_LayerSuite/AEGP_GetLayerStretch", "layerH" => format!("{:#x}", layerH as usize));
		answer(layerH as _, stretchPRt, |t| A_Ratio {
			num: t.stretch.0,
			den: t.stretch.1,
		})
	})
}

unsafe extern "C" fn set_layer_stretch(_layerH: AEGP_LayerH, _stretchPRt: *const A_Ratio) -> A_Err {
	crate::core::ffi::guard("set_layer_stretch", || {
		diag!("AEGP_LayerSuite/AEGP_SetLayerStretch");
		unsupported("AEGP_SetLayerStretch")
	})
}

unsafe extern "C" fn get_layer_transfer_mode(
	layerH: AEGP_LayerH,
	transfer_modeP: *mut AEGP_LayerTransferMode,
) -> A_Err {
	crate::core::ffi::guard("get_layer_transfer_mode", || {
		diag!("AEGP_LayerSuite/AEGP_GetLayerTransferMode", "layerH" => format!("{:#x}", layerH as usize));
		answer(layerH as _, transfer_modeP, |_| AEGP_LayerTransferMode {
			mode: PF_Xfer_IN_FRONT as PF_TransferMode,
			flags: 0,
			track_matte: AEGP_TrackMatte_NO_TRACK_MATTE,
		})
	})
}

unsafe extern "C" fn set_layer_transfer_mode(
	_layerH: AEGP_LayerH,
	_transfer_modeP: *const AEGP_LayerTransferMode,
) -> A_Err {
	crate::core::ffi::guard("set_layer_transfer_mode", || {
		diag!("AEGP_LayerSuite/AEGP_SetLayerTransferMode");
		unsupported("AEGP_SetLayerTransferMode")
	})
}

/// There are no items to add.
unsafe extern "C" fn is_add_layer_valid(
	_item_to_addH: AEGP_ItemH,
	into_compH: AEGP_CompH,
	validPB: *mut A_Boolean,
) -> A_Err {
	crate::core::ffi::guard("is_add_layer_valid", || {
		diag!("AEGP_LayerSuite/AEGP_IsAddLayerValid", "into_compH" => format!("{:#x}", into_compH as usize));
		answer(into_compH as _, validPB, |_| 0)
	})
}

unsafe extern "C" fn add_layer(
	_item_to_addH: AEGP_ItemH,
	_into_compH: AEGP_CompH,
	_added_layerPH0: *mut AEGP_LayerH,
) -> A_Err {
	crate::core::ffi::guard("add_layer", || {
		diag!("AEGP_LayerSuite/AEGP_AddLayer");
		unsupported("AEGP_AddLayer")
	})
}

unsafe extern "C" fn reorder_layer(_layerH: AEGP_LayerH, _layer_indexL: A_long) -> A_Err {
	crate::core::ffi::guard("reorder_layer", || {
		diag!("AEGP_LayerSuite/AEGP_ReorderLayer");
		unsupported("AEGP_ReorderLayer")
	})
}

/// The layer has no masks, so its bounds are its full frame.
unsafe extern "C" fn get_layer_masked_bounds(
	layerH: AEGP_LayerH,
	_time_mode: AEGP_LTimeMode,
	_timePT: *const A_Time,
	boundsPR: *mut A_FloatRect,
) -> A_Err {
	crate::core::ffi::guard("get_layer_masked_bounds", || {
		diag!("AEGP_LayerSuite/AEGP_GetLayerMaskedBounds", "layerH" => format!("{:#x}", layerH as usize));
		answer(layerH as _, boundsPR, |t| A_FloatRect {
			left: 0.0,
			top: 0.0,
			right: t.width as A_FpLong,
			bottom: t.height as A_FpLong,
		})
	})
}

unsafe extern "C" fn get_layer_object_type(layerH: AEGP_LayerH, object_type: *mut AEGP_ObjectType) -> A_Err {
	crate::core::ffi::guard("get_layer_object_type", || {
		diag!("AEGP_LayerSuite/AEGP_GetLayerObjectType", "layerH" => format!("{:#x}", layerH as usize));
		answer(layerH as _, object_type, |t| {
			(match t.kind {
				Kind::Footage | Kind::Null => AEGP_ObjectType_AV,
				Kind::Text => AEGP_ObjectType_TEXT,
				Kind::Shape => AEGP_ObjectType_VECTOR,
			}) as AEGP_ObjectType
		})
	})
}

unsafe extern "C" fn is_layer_3d(layerH: AEGP_LayerH, is_3DPB: *mut A_Boolean) -> A_Err {
	crate::core::ffi::guard("is_layer_3d", || {
		diag!("AEGP_LayerSuite/AEGP_IsLayer3D", "layerH" => format!("{:#x}", layerH as usize));
		answer(layerH as _, is_3DPB, |_| 0)
	})
}

unsafe extern "C" fn is_layer_2d(layerH: AEGP_LayerH, is_2DPB: *mut A_Boolean) -> A_Err {
	crate::core::ffi::guard("is_layer_2d", || {
		diag!("AEGP_LayerSuite/AEGP_IsLayer2D", "layerH" => format!("{:#x}", layerH as usize));
		answer(layerH as _, is_2DPB, |_| 1)
	})
}

unsafe extern "C" fn is_video_active(
	layerH: AEGP_LayerH,
	time_mode: AEGP_LTimeMode,
	timePT: *const A_Time,
	is_activePB: *mut A_Boolean,
) -> A_Err {
	crate::core::ffi::guard("is_video_active", || {
		diag!("AEGP_LayerSuite/AEGP_IsVideoActive", "layerH" => format!("{:#x}", layerH as usize));
		let Some(time) = (unsafe { timePT.as_ref() }).copied().and_then(TimePoint::from_sys) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		answer(layerH as _, is_activePB, |t| {
			let comp = if is_comp_time(time_mode) {
				time
			} else {
				t.to_comp_time(time)
			};
			(t.visible && t.in_point <= comp && comp < t.out_point) as A_Boolean
		})
	})
}

unsafe extern "C" fn is_layer_used_as_track_matte(
	layerH: AEGP_LayerH,
	_fill_must_be_activeB: A_Boolean,
	is_track_mattePB: *mut A_Boolean,
) -> A_Err {
	crate::core::ffi::guard("is_layer_used_as_track_matte", || {
		diag!("AEGP_LayerSuite/AEGP_IsLayerUsedAsTrackMatte", "layerH" => format!("{:#x}", layerH as usize));
		answer(layerH as _, is_track_mattePB, |_| 0)
	})
}

unsafe extern "C" fn does_layer_have_track_matte(layerH: AEGP_LayerH, has_track_mattePB: *mut A_Boolean) -> A_Err {
	crate::core::ffi::guard("does_layer_have_track_matte", || {
		diag!("AEGP_LayerSuite/AEGP_DoesLayerHaveTrackMatte", "layerH" => format!("{:#x}", layerH as usize));
		answer(layerH as _, has_track_mattePB, |_| 0)
	})
}

unsafe extern "C" fn convert_comp_to_layer_time(
	layerH: AEGP_LayerH,
	comp_timePT: *const A_Time,
	layer_timePT: *mut A_Time,
) -> A_Err {
	crate::core::ffi::guard("convert_comp_to_layer_time", || {
		diag!("AEGP_LayerSuite/AEGP_ConvertCompToLayerTime", "layerH" => format!("{:#x}", layerH as usize));
		let Some(time) = (unsafe { comp_timePT.as_ref() }).copied().and_then(TimePoint::from_sys) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		answer(layerH as _, layer_timePT, |t| t.to_layer_time(time).to_sys())
	})
}

unsafe extern "C" fn convert_layer_to_comp_time(
	layerH: AEGP_LayerH,
	layer_timePT: *const A_Time,
	comp_timePT: *mut A_Time,
) -> A_Err {
	crate::core::ffi::guard("convert_layer_to_comp_time", || {
		diag!("AEGP_LayerSuite/AEGP_ConvertLayerToCompTime", "layerH" => format!("{:#x}", layerH as usize));
		let Some(time) = (unsafe { layer_timePT.as_ref() })
			.copied()
			.and_then(TimePoint::from_sys)
		else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		answer(layerH as _, comp_timePT, |t| t.to_comp_time(time).to_sys())
	})
}

/// A value that changes from frame to frame but is fixed for a given frame
/// (the `wiggle`-style randomness AE derives per layer and frame).
unsafe extern "C" fn get_layer_dancing_rand_value(
	layerH: AEGP_LayerH,
	comp_timePT: *const A_Time,
	rand_valuePL: *mut A_long,
) -> A_Err {
	crate::core::ffi::guard("get_layer_dancing_rand_value", || {
		diag!("AEGP_LayerSuite/AEGP_GetLayerDancingRandValue", "layerH" => format!("{:#x}", layerH as usize));
		let Some(time) = (unsafe { comp_timePT.as_ref() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		answer(layerH as _, rand_valuePL, |t| {
			// splitmix64 of the frame number, apart for each layer.
			let seed = t.frame_of(time) as u64 ^ u64::from(t.id.wrapping_sub(LAYER_ID)) << 32;
			let mut z = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
			z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
			z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
			((z ^ (z >> 31)) >> 33) as A_long
		})
	})
}

unsafe extern "C" fn get_layer_id(layerH: AEGP_LayerH, id_valP: *mut AEGP_LayerIDVal) -> A_Err {
	crate::core::ffi::guard("get_layer_id", || {
		diag!("AEGP_LayerSuite/AEGP_GetLayerID", "layerH" => format!("{:#x}", layerH as usize));
		answer(layerH as _, id_valP, |t| t.id as AEGP_LayerIDVal)
	})
}

/// The layer's transform and its parents', at comp time `comp_timeP` or now.
fn world_xform(layerH: AEGP_LayerH, comp_timeP: *const A_Time, out: *mut A_Matrix4) -> A_Err {
	let Some(timeline) = Timeline::of(layerH as _) else {
		return PF_Err_BAD_CALLBACK_PARAM;
	};
	let time = match unsafe { comp_timeP.as_ref() } {
		None => timeline.comp_time,
		Some(time) => TimePoint::from_sys(*time).unwrap_or(timeline.comp_time),
	};
	// SAFETY: `Timeline::of` found the effect active.
	let [a, b, c, d, x, y] = unsafe { instance_of(timeline.effect) }
		.and_then(PluginInstance::project)
		.and_then(|project| project.layer_to_comp(timeline.id, time))
		.unwrap_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
	let mat = [
		[a, b, 0.0, 0.0],
		[c, d, 0.0, 0.0],
		[0.0, 0.0, 1.0, 0.0],
		[x, y, 0.0, 1.0],
	];
	write(Some(timeline), out, |_| A_Matrix4 { mat })
}

unsafe extern "C" fn get_layer_to_world_xform(
	aegp_layerH: AEGP_LayerH,
	comp_timeP: *const A_Time,
	transform: *mut A_Matrix4,
) -> A_Err {
	crate::core::ffi::guard("get_layer_to_world_xform", || {
		diag!("AEGP_LayerSuite/AEGP_GetLayerToWorldXform", "aegp_layerH" => format!("{:#x}", aegp_layerH as usize));
		world_xform(aegp_layerH, comp_timeP, transform)
	})
}

unsafe extern "C" fn get_layer_to_world_xform_from_view(
	aegp_layerH: AEGP_LayerH,
	_view_timeP: *const A_Time,
	comp_timeP: *const A_Time,
	transform: *mut A_Matrix4,
) -> A_Err {
	crate::core::ffi::guard("get_layer_to_world_xform_from_view", || {
		diag!("AEGP_LayerSuite/AEGP_GetLayerToWorldXformFromView", "aegp_layerH" => format!("{:#x}", aegp_layerH as usize));
		world_xform(aegp_layerH, comp_timeP, transform)
	})
}

unsafe extern "C" fn set_layer_name(_aegp_layerH: AEGP_LayerH, _new_nameZ: *const A_char) -> A_Err {
	crate::core::ffi::guard("set_layer_name", || {
		diag!("AEGP_LayerSuite/AEGP_SetLayerName");
		unsupported("AEGP_SetLayerName")
	})
}

/// A null handle without a parent, as in After Effects.
unsafe extern "C" fn get_layer_parent(layerH: AEGP_LayerH, parent_layerPH: *mut AEGP_LayerH) -> A_Err {
	crate::core::ffi::guard("get_layer_parent", || {
		diag!("AEGP_LayerSuite/AEGP_GetLayerParent", "layerH" => format!("{:#x}", layerH as usize));
		answer(layerH as _, parent_layerPH, |t| {
			// SAFETY: `answer` found the effect active.
			unsafe { instance_of(t.effect) }
				.and_then(PluginInstance::project)
				.and_then(|project| project.layer(t.id)?.parent)
				.and_then(|parent| handle_of_layer(t.effect, parent))
				.unwrap_or(std::ptr::null_mut())
		})
	})
}

unsafe extern "C" fn set_layer_parent(_layerH: AEGP_LayerH, _parent_layerH0: AEGP_LayerH) -> A_Err {
	crate::core::ffi::guard("set_layer_parent", || {
		diag!("AEGP_LayerSuite/AEGP_SetLayerParent");
		unsupported("AEGP_SetLayerParent")
	})
}

unsafe extern "C" fn delete_layer(_layerH: AEGP_LayerH) -> A_Err {
	crate::core::ffi::guard("delete_layer", || {
		diag!("AEGP_LayerSuite/AEGP_DeleteLayer");
		unsupported("AEGP_DeleteLayer")
	})
}

unsafe extern "C" fn duplicate_layer(_orig_layerH: AEGP_LayerH, _duplicate_layerPH: *mut AEGP_LayerH) -> A_Err {
	crate::core::ffi::guard("duplicate_layer", || {
		diag!("AEGP_LayerSuite/AEGP_DuplicateLayer");
		unsupported("AEGP_DuplicateLayer")
	})
}

unsafe extern "C" fn get_layer_from_layer_id(
	parent_compH: AEGP_CompH,
	id: AEGP_LayerIDVal,
	layerPH: *mut AEGP_LayerH,
) -> A_Err {
	crate::core::ffi::guard("get_layer_from_layer_id", || {
		diag!("AEGP_LayerSuite/AEGP_GetLayerFromLayerID",
			"parent_compH" => format!("{:#x}", parent_compH as usize),
			"id" => id,
		);
		let (Some(timeline), Some(out)) = (Timeline::of_effect(parent_compH as _), unsafe { layerPH.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		let Some(handle) = u32::try_from(id)
			.ok()
			.and_then(|id| handle_of_layer(timeline.effect, id))
		else {
			return PF_Err_INVALID_INDEX;
		};
		*out = handle;
		PF_Err_NONE
	})
}

/// From v6 the names come back as UTF-16 `AEGP_MemHandle`s.
unsafe extern "C" fn get_layer_name_utf16(
	_pluginID: AEGP_PluginID,
	layerH: AEGP_LayerH,
	utf_layer_namePH0: *mut AEGP_MemHandle,
	utf_source_namePH0: *mut AEGP_MemHandle,
) -> A_Err {
	crate::core::ffi::guard("get_layer_name_utf16", || {
		diag!("AEGP_LayerSuite/AEGP_GetLayerName", "layerH" => format!("{:#x}", layerH as usize));
		let Some(timeline) = Timeline::of(layerH as _) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		for (out, name) in [
			(utf_layer_namePH0, layer_name(&timeline)),
			(utf_source_namePH0, String::new()),
		] {
			if let Some(out) = unsafe { out.as_mut() } {
				*out = utf16_handle(&name);
			}
		}
		PF_Err_NONE
	})
}

/// `text` as a NUL-terminated UTF-16 `AEGP_MemHandle`, the plugin's to free.
pub(crate) fn utf16_handle(text: &str) -> AEGP_MemHandle {
	super::aegp_memory::handle_from_bytes(
		text.encode_utf16()
			.chain(std::iter::once(0))
			.flat_map(u16::to_ne_bytes)
			.collect(),
	)
}

unsafe extern "C" fn set_layer_name_utf16(_aegp_layerH: AEGP_LayerH, _new_nameZ: *const A_UTF16Char) -> A_Err {
	crate::core::ffi::guard("set_layer_name_utf16", || {
		diag!("AEGP_LayerSuite/AEGP_SetLayerName");
		unsupported("AEGP_SetLayerName")
	})
}

unsafe extern "C" fn get_layer_label(layerH: AEGP_LayerH, labelP: *mut AEGP_LabelID) -> A_Err {
	crate::core::ffi::guard("get_layer_label", || {
		diag!("AEGP_LayerSuite/AEGP_GetLayerLabel", "layerH" => format!("{:#x}", layerH as usize));
		answer(layerH as _, labelP, |_| AEGP_Label_NO_LABEL as AEGP_LabelID)
	})
}

unsafe extern "C" fn set_layer_label(_layerH: AEGP_LayerH, _label: AEGP_LabelID) -> A_Err {
	crate::core::ffi::guard("set_layer_label", || {
		diag!("AEGP_LayerSuite/AEGP_SetLayerLabel");
		unsupported("AEGP_SetLayerLabel")
	})
}

unsafe extern "C" fn get_layer_sampling_quality(
	layerH: AEGP_LayerH,
	qualityP: *mut AEGP_LayerSamplingQuality,
) -> A_Err {
	crate::core::ffi::guard("get_layer_sampling_quality", || {
		diag!("AEGP_LayerSuite/AEGP_GetLayerSamplingQuality", "layerH" => format!("{:#x}", layerH as usize));
		answer(layerH as _, qualityP, |_| AEGP_LayerSamplingQual_BILINEAR)
	})
}

unsafe extern "C" fn set_layer_sampling_quality(_layerH: AEGP_LayerH, _quality: AEGP_LayerSamplingQuality) -> A_Err {
	crate::core::ffi::guard("set_layer_sampling_quality", || {
		diag!("AEGP_LayerSuite/AEGP_SetLayerSamplingQuality");
		unsupported("AEGP_SetLayerSamplingQuality")
	})
}

/// No track matte: a null handle.
unsafe extern "C" fn get_track_matte_layer(layerH: AEGP_LayerH, track_matte_layerPH: *mut AEGP_LayerH) -> A_Err {
	crate::core::ffi::guard("get_track_matte_layer", || {
		diag!("AEGP_LayerSuite/AEGP_GetTrackMatteLayer", "layerH" => format!("{:#x}", layerH as usize));
		answer(layerH as _, track_matte_layerPH, |_| std::ptr::null_mut())
	})
}

unsafe extern "C" fn set_track_matte(
	_layerH: AEGP_LayerH,
	_track_matte_layerH0: AEGP_LayerH,
	_track_matte_type: AEGP_TrackMatte,
) -> A_Err {
	crate::core::ffi::guard("set_track_matte", || {
		diag!("AEGP_LayerSuite/AEGP_SetTrackMatte");
		unsupported("AEGP_SetTrackMatte")
	})
}

unsafe extern "C" fn remove_track_matte(_layerH: AEGP_LayerH) -> A_Err {
	crate::core::ffi::guard("remove_track_matte", || {
		diag!("AEGP_LayerSuite/AEGP_RemoveTrackMatte");
		unsupported("AEGP_RemoveTrackMatte")
	})
}

/// No layer is a light.
unsafe extern "C" fn get_light_type(light_layerH: AEGP_LayerH, _light_typeP: *mut AEGP_LightType) -> A_Err {
	crate::core::ffi::guard("get_light_type", || {
		diag!("AEGP_LightSuite1/AEGP_GetLightType", "light_layerH" => format!("{:#x}", light_layerH as usize));
		if Timeline::of(light_layerH as _).is_none() {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		A_Err_GENERIC
	})
}

/// Builds the `AEGP_LightSuite1` vtable.
pub(super) const fn create_light_suite_1() -> AEGP_LightSuite1 {
	AEGP_LightSuite1 {
		AEGP_GetLightType: Some(get_light_type),
	}
}

/// Builds an `AEGP_LayerSuite*` vtable: the calls every version shares, the
/// version's name getter/setter, then whatever it appends.
macro_rules! layer_suite {
	($suite:ident, $get_name:ident, $set_name:ident $(, $field:ident: $fn_name:ident)* $(,)?) => {
		$suite {
			AEGP_GetCompNumLayers: Some(get_comp_num_layers),
			AEGP_GetCompLayerByIndex: Some(get_comp_layer_by_index),
			AEGP_GetActiveLayer: Some(get_active_layer),
			AEGP_GetLayerIndex: Some(get_layer_index),
			AEGP_GetLayerSourceItem: Some(get_layer_source_item),
			AEGP_GetLayerSourceItemID: Some(get_layer_source_item_id),
			AEGP_GetLayerParentComp: Some(get_layer_parent_comp),
			AEGP_GetLayerQuality: Some(get_layer_quality),
			AEGP_SetLayerQuality: Some(set_layer_quality),
			AEGP_GetLayerFlags: Some(get_layer_flags),
			AEGP_SetLayerFlag: Some(set_layer_flag),
			AEGP_IsLayerVideoReallyOn: Some(is_layer_video_really_on),
			AEGP_IsLayerAudioReallyOn: Some(is_layer_audio_really_on),
			AEGP_GetLayerCurrentTime: Some(get_layer_current_time),
			AEGP_GetLayerInPoint: Some(get_layer_in_point),
			AEGP_GetLayerDuration: Some(get_layer_duration),
			AEGP_SetLayerInPointAndDuration: Some(set_layer_in_point_and_duration),
			AEGP_GetLayerOffset: Some(get_layer_offset),
			AEGP_SetLayerOffset: Some(set_layer_offset),
			AEGP_GetLayerStretch: Some(get_layer_stretch),
			AEGP_SetLayerStretch: Some(set_layer_stretch),
			AEGP_GetLayerTransferMode: Some(get_layer_transfer_mode),
			AEGP_SetLayerTransferMode: Some(set_layer_transfer_mode),
			AEGP_IsAddLayerValid: Some(is_add_layer_valid),
			AEGP_AddLayer: Some(add_layer),
			AEGP_ReorderLayer: Some(reorder_layer),
			AEGP_GetLayerMaskedBounds: Some(get_layer_masked_bounds),
			AEGP_GetLayerObjectType: Some(get_layer_object_type),
			AEGP_IsLayer3D: Some(is_layer_3d),
			AEGP_IsLayer2D: Some(is_layer_2d),
			AEGP_IsVideoActive: Some(is_video_active),
			AEGP_IsLayerUsedAsTrackMatte: Some(is_layer_used_as_track_matte),
			AEGP_DoesLayerHaveTrackMatte: Some(does_layer_have_track_matte),
			AEGP_ConvertCompToLayerTime: Some(convert_comp_to_layer_time),
			AEGP_ConvertLayerToCompTime: Some(convert_layer_to_comp_time),
			AEGP_GetLayerDancingRandValue: Some(get_layer_dancing_rand_value),
			AEGP_GetLayerID: Some(get_layer_id),
			AEGP_GetLayerToWorldXform: Some(get_layer_to_world_xform),
			AEGP_GetLayerToWorldXformFromView: Some(get_layer_to_world_xform_from_view),
			AEGP_GetLayerParent: Some(get_layer_parent),
			AEGP_SetLayerParent: Some(set_layer_parent),
			AEGP_DeleteLayer: Some(delete_layer),
			AEGP_DuplicateLayer: Some(duplicate_layer),
			AEGP_GetLayerFromLayerID: Some(get_layer_from_layer_id),
			AEGP_GetLayerName: Some($get_name),
			AEGP_SetLayerName: Some($set_name),
			$($field: Some($fn_name),)*
		}
	};
}

/// Wire version 5 (`AEGP_LayerSuite1`), which Shine asks for to look for a
/// light: version 8 without parents, deleting and layer to comp time. The SDK froze its order, which is not version 8's.
pub(super) const fn create_layer_suite_1() -> AEGP_LayerSuite1 {
	AEGP_LayerSuite1 {
		AEGP_GetCompNumLayers: Some(get_comp_num_layers),
		AEGP_GetCompLayerByIndex: Some(get_comp_layer_by_index),
		AEGP_GetActiveLayer: Some(get_active_layer),
		AEGP_GetLayerIndex: Some(get_layer_index),
		AEGP_GetLayerSourceItem: Some(get_layer_source_item),
		AEGP_GetLayerParentComp: Some(get_layer_parent_comp),
		AEGP_GetLayerQuality: Some(get_layer_quality),
		AEGP_SetLayerQuality: Some(set_layer_quality),
		AEGP_GetLayerFlags: Some(get_layer_flags),
		AEGP_SetLayerFlag: Some(set_layer_flag),
		AEGP_IsLayerVideoReallyOn: Some(is_layer_video_really_on),
		AEGP_IsLayerAudioReallyOn: Some(is_layer_audio_really_on),
		AEGP_GetLayerCurrentTime: Some(get_layer_current_time),
		AEGP_GetLayerInPoint: Some(get_layer_in_point),
		AEGP_GetLayerDuration: Some(get_layer_duration),
		AEGP_SetLayerInPointAndDuration: Some(set_layer_in_point_and_duration),
		AEGP_GetLayerOffset: Some(get_layer_offset),
		AEGP_SetLayerOffset: Some(set_layer_offset),
		AEGP_GetLayerStretch: Some(get_layer_stretch),
		AEGP_SetLayerStretch: Some(set_layer_stretch),
		AEGP_GetLayerTransferMode: Some(get_layer_transfer_mode),
		AEGP_SetLayerTransferMode: Some(set_layer_transfer_mode),
		AEGP_IsAddLayerValid: Some(is_add_layer_valid),
		AEGP_AddLayer: Some(add_layer),
		AEGP_ReorderLayer: Some(reorder_layer),
		AEGP_GetLayerMaskedBounds: Some(get_layer_masked_bounds),
		AEGP_GetLayerObjectType: Some(get_layer_object_type),
		AEGP_IsLayer3D: Some(is_layer_3d),
		AEGP_IsLayer2D: Some(is_layer_2d),
		AEGP_IsVideoActive: Some(is_video_active),
		AEGP_IsLayerUsedAsTrackMatte: Some(is_layer_used_as_track_matte),
		AEGP_DoesLayerHaveTrackMatte: Some(does_layer_have_track_matte),
		AEGP_ConvertCompToLayerTime: Some(convert_comp_to_layer_time),
		AEGP_GetLayerDancingRandValue: Some(get_layer_dancing_rand_value),
		AEGP_GetLayerID: Some(get_layer_id),
		AEGP_GetLayerToWorldXform: Some(get_layer_to_world_xform),
		AEGP_GetLayerToWorldXformFromView: Some(get_layer_to_world_xform_from_view),
		AEGP_GetLayerName: Some(get_layer_name),
		AEGP_SetLayerName: Some(set_layer_name),
	}
}

/// Wire version 8 (`AEGP_LayerSuite3`). It has no source-item id and cannot
/// duplicate a layer or look one up by id. Names are still `char` buffers.
pub(super) const fn create_layer_suite_3() -> AEGP_LayerSuite3 {
	AEGP_LayerSuite3 {
		AEGP_GetCompNumLayers: Some(get_comp_num_layers),
		AEGP_GetCompLayerByIndex: Some(get_comp_layer_by_index),
		AEGP_GetActiveLayer: Some(get_active_layer),
		AEGP_GetLayerIndex: Some(get_layer_index),
		AEGP_GetLayerSourceItem: Some(get_layer_source_item),
		AEGP_GetLayerParentComp: Some(get_layer_parent_comp),
		AEGP_GetLayerQuality: Some(get_layer_quality),
		AEGP_SetLayerQuality: Some(set_layer_quality),
		AEGP_GetLayerFlags: Some(get_layer_flags),
		AEGP_SetLayerFlag: Some(set_layer_flag),
		AEGP_IsLayerVideoReallyOn: Some(is_layer_video_really_on),
		AEGP_IsLayerAudioReallyOn: Some(is_layer_audio_really_on),
		AEGP_GetLayerCurrentTime: Some(get_layer_current_time),
		AEGP_GetLayerInPoint: Some(get_layer_in_point),
		AEGP_GetLayerDuration: Some(get_layer_duration),
		AEGP_SetLayerInPointAndDuration: Some(set_layer_in_point_and_duration),
		AEGP_GetLayerOffset: Some(get_layer_offset),
		AEGP_SetLayerOffset: Some(set_layer_offset),
		AEGP_GetLayerStretch: Some(get_layer_stretch),
		AEGP_SetLayerStretch: Some(set_layer_stretch),
		AEGP_GetLayerTransferMode: Some(get_layer_transfer_mode),
		AEGP_SetLayerTransferMode: Some(set_layer_transfer_mode),
		AEGP_IsAddLayerValid: Some(is_add_layer_valid),
		AEGP_AddLayer: Some(add_layer),
		AEGP_ReorderLayer: Some(reorder_layer),
		AEGP_GetLayerMaskedBounds: Some(get_layer_masked_bounds),
		AEGP_GetLayerObjectType: Some(get_layer_object_type),
		AEGP_IsLayer3D: Some(is_layer_3d),
		AEGP_IsLayer2D: Some(is_layer_2d),
		AEGP_IsVideoActive: Some(is_video_active),
		AEGP_IsLayerUsedAsTrackMatte: Some(is_layer_used_as_track_matte),
		AEGP_DoesLayerHaveTrackMatte: Some(does_layer_have_track_matte),
		AEGP_ConvertCompToLayerTime: Some(convert_comp_to_layer_time),
		AEGP_ConvertLayerToCompTime: Some(convert_layer_to_comp_time),
		AEGP_GetLayerDancingRandValue: Some(get_layer_dancing_rand_value),
		AEGP_GetLayerID: Some(get_layer_id),
		AEGP_GetLayerToWorldXform: Some(get_layer_to_world_xform),
		AEGP_GetLayerToWorldXformFromView: Some(get_layer_to_world_xform_from_view),
		AEGP_GetLayerParent: Some(get_layer_parent),
		AEGP_SetLayerParent: Some(set_layer_parent),
		AEGP_DeleteLayer: Some(delete_layer),
		AEGP_GetLayerName: Some(get_layer_name),
		AEGP_SetLayerName: Some(set_layer_name),
	}
}

/// Wire version 11.
pub(super) const fn create_layer_suite_5() -> AEGP_LayerSuite5 {
	layer_suite!(AEGP_LayerSuite5, get_layer_name, set_layer_name)
}

/// Wire version 12: names become UTF-16 `AEGP_MemHandle`s.
pub(super) const fn create_layer_suite_6() -> AEGP_LayerSuite6 {
	layer_suite!(AEGP_LayerSuite6, get_layer_name_utf16, set_layer_name_utf16)
}

/// Wire version 13: adds labels.
pub(super) const fn create_layer_suite_7() -> AEGP_LayerSuite7 {
	layer_suite!(
		AEGP_LayerSuite7,
		get_layer_name_utf16,
		set_layer_name_utf16,
		AEGP_GetLayerLabel: get_layer_label,
		AEGP_SetLayerLabel: set_layer_label,
	)
}

/// Wire version 14: adds sampling quality.
pub(super) const fn create_layer_suite_8() -> AEGP_LayerSuite8 {
	layer_suite!(
		AEGP_LayerSuite8,
		get_layer_name_utf16,
		set_layer_name_utf16,
		AEGP_GetLayerLabel: get_layer_label,
		AEGP_SetLayerLabel: set_layer_label,
		AEGP_GetLayerSamplingQuality: get_layer_sampling_quality,
		AEGP_SetLayerSamplingQuality: set_layer_sampling_quality,
	)
}

/// Wire version 15: adds track matte layers.
pub(super) const fn create_layer_suite_9() -> AEGP_LayerSuite9 {
	layer_suite!(
		AEGP_LayerSuite9,
		get_layer_name_utf16,
		set_layer_name_utf16,
		AEGP_GetLayerLabel: get_layer_label,
		AEGP_SetLayerLabel: set_layer_label,
		AEGP_GetLayerSamplingQuality: get_layer_sampling_quality,
		AEGP_SetLayerSamplingQuality: set_layer_sampling_quality,
		AEGP_GetTrackMatteLayer: get_track_matte_layer,
		AEGP_SetTrackMatte: set_track_matte,
		AEGP_RemoveTrackMatte: remove_track_matte,
	)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn timeline() -> Timeline {
		Timeline::alone(
			std::ptr::null_mut(),
			(1920, 1080),
			TimePoint::frame(0, 30),
			1,
			30,
			false,
		)
	}

	#[test]
	fn frames_follow_the_time_scale() {
		let t = timeline();
		assert_eq!(t.frame_of(&A_Time { value: 2, scale: 1 }), 60);
		assert_eq!(t.frame_of(&A_Time { value: -1, scale: 30 }), -1);
		assert_eq!(t.frame_of(&t.comp_length()), 30 * 30);
		// The default comp is told in the render context's units.
		let length = t.comp_length();
		assert_eq!((length.value, length.scale), (COMP_DURATION_SECONDS * 30, 30));
	}

	#[test]
	fn null_handles_are_rejected() {
		let suite = create_layer_suite_5();
		let mut time = A_Time { value: 7, scale: 7 };
		let err = unsafe { suite.AEGP_GetLayerDuration.unwrap()(std::ptr::null_mut(), 0, &mut time) };
		assert_eq!(err, PF_Err_BAD_CALLBACK_PARAM);
		assert_eq!((time.value, time.scale), (7, 7));
	}
}
