//! The project as a plugin sees it: through `AcquireSuite`, while one of its
//! commands runs.

use std::cell::Cell;
use std::ffi::{CStr, c_void};

use crate::sys::*;
use crate::{Comp, Host, LayerContent, PluginInstance, Project, ProjectLayer, Time, TimePoint};

/// What a command's body is given.
pub(super) struct Plugin {
	pub effect: PF_ProgPtr,
	pub input: *mut PF_InData,
}

impl Plugin {
	pub fn suite<T>(&self, name: &CStr, version: u32) -> &'static T {
		let mut table: *const c_void = std::ptr::null();
		// SAFETY: the pica pointer is live during the command, and every suite
		// is a static table of the type that name and version stand for.
		unsafe {
			let pica = &*(*self.input).pica_basicP;
			let err = pica.AcquireSuite.unwrap()(name.as_ptr(), version as _, &mut table);
			if err != PF_Err_NONE || table.is_null() {
				eprintln!("no suite {name:?} v{version}");
				std::process::abort();
			}
			&*table.cast::<T>()
		}
	}
}

type Body<'a> = &'a mut dyn FnMut(&Plugin);

thread_local! {
	static BODY: Cell<Option<*mut c_void>> = const { Cell::new(None) };
}

/// A test effect that runs the body given to [`during`] in `PF_Cmd_ABOUT`.
pub(super) unsafe extern "C" fn entry(
	cmd: PF_Cmd,
	input: *mut PF_InData,
	_output: *mut PF_OutData,
	_params: PF_ParamList,
	_world: *mut PF_LayerDef,
	_extra: *mut c_void,
) -> PF_Err {
	if cmd == PF_Cmd_ABOUT as PF_Cmd
		&& let Some(body) = BODY.get()
	{
		let plugin = Plugin {
			effect: unsafe { (*input).effect_ref },
			input,
		};
		// SAFETY: `during` keeps the body alive until the command returns.
		unsafe { (*body.cast::<Body<'_>>())(&plugin) };
	}
	PF_Err_NONE
}

/// Run `body` as the plugin, inside a command, and give back what it saw.
/// Assert on that afterwards: a panic cannot cross the plugin boundary.
pub(super) fn during<R>(fx: &mut PluginInstance, body: impl FnOnce(&Plugin) -> R) -> R {
	let (mut body, mut result) = (Some(body), None);
	let mut run = |plugin: &Plugin| result = body.take().map(|body| body(plugin));
	let mut erased: Body<'_> = &mut run;
	BODY.set(Some((&raw mut erased).cast()));
	fx.about().expect("the test command runs");
	BODY.set(None);
	result.expect("the body ran")
}

pub(super) fn load() -> PluginInstance {
	// SAFETY: `entry` has the effect entry point's signature.
	unsafe { Host::get().entry_loader(entry) }.size(64, 32).load().unwrap()
}

fn seconds(time: A_Time) -> f64 {
	f64::from(time.value) / f64::from(time.scale)
}

fn comp() -> Comp {
	Comp::new("Main", 320, 180, TimePoint::whole_seconds(8))
}

/// Top, the effect's layer (2 s in, at half speed, parented) and a null.
fn three_layers() -> Project {
	let comp = comp();
	let top = ProjectLayer::new(40, "Top", LayerContent::Footage, &comp);
	let mut host = ProjectLayer::new(7, "Host", LayerContent::Footage, &comp);
	(host.width, host.height) = (100, 50);
	host.start = TimePoint::whole_seconds(2);
	host.in_point = TimePoint::whole_seconds(3);
	host.out_point = TimePoint::whole_seconds(7);
	host.stretch = (2, 1);
	host.parent = Some(9);
	let null = ProjectLayer::new(9, "Null", LayerContent::Null, &comp);
	Project::new(comp, vec![top, host, null], 7).unwrap()
}

#[derive(Debug, PartialEq)]
struct Seen {
	id: AEGP_LayerIDVal,
	index: A_long,
	kind: AEGP_ObjectType,
	effects: A_long,
	name: String,
}

#[test]
fn layers_enumerate_with_their_ids_names_and_references() {
	let mut fx = load();
	fx.set_project(Some(three_layers()));
	let (seen, effect_layer, by_id, parent) = during(&mut fx, |p| unsafe {
		let layers: &AEGP_LayerSuite5 = p.suite(c"AEGP Layer Suite", kAEGPLayerSuiteVersion5);
		let effects: &AEGP_EffectSuite4 = p.suite(c"AEGP Effect Suite", 4);
		let bridge: &AEGP_PFInterfaceSuite1 = p.suite(c"AEGP PF Interface Suite", 1);
		let comp = p.effect as AEGP_CompH;
		let mut count = 0;
		layers.AEGP_GetCompNumLayers.unwrap()(comp, &mut count);
		let mut seen = Vec::new();
		let mut handles = Vec::new();
		for at in 0..count {
			let mut layer: AEGP_LayerH = std::ptr::null_mut();
			layers.AEGP_GetCompLayerByIndex.unwrap()(comp, at, &mut layer);
			let (mut id, mut index, mut effect_count) = (0, -1, -1);
			let mut kind: AEGP_ObjectType = -1;
			let mut name = [0 as A_char; AEGP_MAX_LAYER_NAME_SIZE as usize];
			layers.AEGP_GetLayerID.unwrap()(layer, &mut id);
			layers.AEGP_GetLayerIndex.unwrap()(layer, &mut index);
			layers.AEGP_GetLayerObjectType.unwrap()(layer, &mut kind);
			effects.AEGP_GetLayerNumEffects.unwrap()(layer, &mut effect_count);
			layers.AEGP_GetLayerName.unwrap()(layer, name.as_mut_ptr(), std::ptr::null_mut());
			seen.push(Seen {
				id,
				index,
				kind,
				effects: effect_count,
				name: CStr::from_ptr(name.as_ptr()).to_string_lossy().into_owned(),
			});
			handles.push(layer as usize);
		}
		let mut effect_layer: AEGP_LayerH = std::ptr::null_mut();
		bridge.AEGP_GetEffectLayer.unwrap()(p.effect, &mut effect_layer);
		let mut null_layer: AEGP_LayerH = std::ptr::null_mut();
		layers.AEGP_GetLayerFromLayerID.unwrap()(comp, 9, &mut null_layer);
		let mut parent: AEGP_LayerH = std::ptr::null_mut();
		layers.AEGP_GetLayerParent.unwrap()(effect_layer, &mut parent);
		(
			seen,
			effect_layer as usize == handles[1],
			null_layer as usize == handles[2],
			parent as usize == handles[2],
		)
	});
	let av = AEGP_ObjectType_AV as AEGP_ObjectType;
	let layer = |id, index, effects, name: &str| Seen {
		id,
		index,
		kind: av,
		effects,
		name: name.into(),
	};
	assert_eq!(
		seen,
		[layer(40, 0, 0, "Top"), layer(7, 1, 1, "Host"), layer(9, 2, 0, "Null")]
	);
	assert!(effect_layer, "GetEffectLayer is the host's handle");
	assert!(by_id, "GetLayerFromLayerID is the same handle");
	assert!(parent, "GetLayerParent is the same handle");
}

#[test]
fn every_suite_tells_the_same_time() {
	let mut fx = load();
	fx.set_project(Some(three_layers()));
	for (frame, fps) in [(30, 30), (-45, 30), (30_030, 30_000), (0, 24), (99, 25)] {
		fx.set_time(Time::new(frame, 1, fps).unwrap());
		let layer_now = f64::from(frame) / f64::from(fps);
		let comp_now = 2.0 + 2.0 * layer_now;
		let seen = during(&mut fx, |p| unsafe {
			let layers: &AEGP_LayerSuite5 = p.suite(c"AEGP Layer Suite", kAEGPLayerSuiteVersion5);
			let bridge: &AEGP_PFInterfaceSuite1 = p.suite(c"AEGP PF Interface Suite", 1);
			let items: &AEGP_ItemSuite9 = p.suite(c"AEGP Item Suite", kAEGPItemSuiteVersion9);
			let host = p.effect as AEGP_LayerH;
			let zero = A_Time { value: 0, scale: 1 };
			let (mut layer, mut comp, mut effect, mut back, mut item) = (zero, zero, zero, zero, zero);
			layers.AEGP_GetLayerCurrentTime.unwrap()(host, AEGP_LTimeMode_LayerTime as _, &mut layer);
			layers.AEGP_GetLayerCurrentTime.unwrap()(host, AEGP_LTimeMode_CompTime as _, &mut comp);
			bridge.AEGP_ConvertEffectToCompTime.unwrap()(p.effect, frame, fps, &mut effect);
			layers.AEGP_ConvertCompToLayerTime.unwrap()(host, &comp, &mut back);
			items.AEGP_GetItemCurrentTime.unwrap()(p.effect as AEGP_ItemH, &mut item);
			[layer, comp, effect, back, item].map(seconds)
		});
		for (got, want) in seen
			.into_iter()
			.zip([layer_now, comp_now, comp_now, layer_now, comp_now])
		{
			assert!((got - want).abs() < 1e-9, "{frame}/{fps}: {seen:?}");
		}
	}
}

#[test]
fn spans_and_sizes_are_the_projects() {
	let mut fx = load();
	fx.set_project(Some(three_layers()));
	let (spans, sizes, active, total) = during(&mut fx, |p| unsafe {
		let layers: &AEGP_LayerSuite5 = p.suite(c"AEGP Layer Suite", kAEGPLayerSuiteVersion5);
		let items: &AEGP_ItemSuite9 = p.suite(c"AEGP Item Suite", kAEGPItemSuiteVersion9);
		let host = p.effect as AEGP_LayerH;
		let mut spans = [A_Time { value: 0, scale: 1 }; 5];
		let (layer_time, comp_time) = (AEGP_LTimeMode_LayerTime as _, AEGP_LTimeMode_CompTime as _);
		layers.AEGP_GetLayerInPoint.unwrap()(host, comp_time, &mut spans[0]);
		layers.AEGP_GetLayerDuration.unwrap()(host, comp_time, &mut spans[1]);
		layers.AEGP_GetLayerInPoint.unwrap()(host, layer_time, &mut spans[2]);
		layers.AEGP_GetLayerDuration.unwrap()(host, layer_time, &mut spans[3]);
		items.AEGP_GetItemDuration.unwrap()(p.effect as AEGP_ItemH, &mut spans[4]);
		let (mut width, mut height) = (0, 0);
		items.AEGP_GetItemDimensions.unwrap()(p.effect as AEGP_ItemH, &mut width, &mut height);
		// In at 3 s, out at 7 s, exclusive.
		let active = [2, 3, 6, 7].map(|value| {
			let mut on = 9;
			layers.AEGP_IsVideoActive.unwrap()(host, comp_time, &A_Time { value, scale: 1 }, &mut on);
			on
		});
		let total = f64::from((*p.input).total_time) / f64::from((*p.input).time_scale);
		(spans.map(seconds), (width, height), active, total)
	});
	assert_eq!(spans, [3.0, 4.0, 0.5, 2.0, 8.0]);
	assert_eq!(sizes, (320, 180));
	assert_eq!(active, [0, 1, 1, 0]);
	assert_eq!(total, 2.0, "total_time is the layer's duration in its own time");
}

/// The handle of layer `id`, and what asking with `handle` gives.
fn ask(fx: &mut PluginInstance, id: u32, handle: usize) -> (usize, PF_Err) {
	during(fx, |p| unsafe {
		let layers: &AEGP_LayerSuite5 = p.suite(c"AEGP Layer Suite", kAEGPLayerSuiteVersion5);
		let mut found: AEGP_LayerH = std::ptr::null_mut();
		layers.AEGP_GetLayerFromLayerID.unwrap()(p.effect as AEGP_CompH, id as _, &mut found);
		let mut out = 0;
		(
			found as usize,
			layers.AEGP_GetLayerID.unwrap()(handle as AEGP_LayerH, &mut out),
		)
	})
}

#[test]
fn a_deleted_layers_handle_is_refused_and_never_reused() {
	let mut fx = load();
	fx.set_project(Some(three_layers()));
	let (top, _) = ask(&mut fx, 40, 0);
	let (null, _) = ask(&mut fx, 9, 0);
	assert_eq!(ask(&mut fx, 40, top).1, PF_Err_NONE);

	let comp = comp();
	let layer = |id| ProjectLayer::new(id, "l", LayerContent::Footage, &comp);
	fx.set_project(Some(Project::new(comp.clone(), vec![layer(7), layer(9)], 7).unwrap()));
	assert_eq!(
		ask(&mut fx, 9, top),
		(null, PF_Err_BAD_CALLBACK_PARAM),
		"gone, and 9 keeps its handle"
	);

	fx.set_project(Some(three_layers()));
	let (again, stale) = ask(&mut fx, 40, top);
	assert_eq!(stale, PF_Err_BAD_CALLBACK_PARAM);
	assert_ne!(again, top);

	fx.set_project(None);
	assert_eq!(ask(&mut fx, 1, again).1, PF_Err_BAD_CALLBACK_PARAM);

	let mut other = load();
	other.set_project(Some(three_layers()));
	assert_ne!(ask(&mut other, 40, 0).0, again, "two instances share no handle");
}

#[test]
fn another_instances_layer_is_not_visible_outside_its_call() {
	let mut owner = load();
	owner.set_project(Some(three_layers()));
	let (top, _) = ask(&mut owner, 40, 0);
	assert_eq!(ask(&mut load(), 1, top).1, PF_Err_BAD_CALLBACK_PARAM);
}

#[test]
fn without_a_project_the_layer_is_still_the_comp() {
	let mut fx = load();
	fx.set_time(Time::frame(12, 24));
	let seen = during(&mut fx, |p| unsafe {
		let layers: &AEGP_LayerSuite5 = p.suite(c"AEGP Layer Suite", kAEGPLayerSuiteVersion5);
		let (host, comp) = (p.effect as AEGP_LayerH, p.effect as AEGP_CompH);
		let (mut count, mut id) = (0, 0);
		let mut now = A_Time { value: 0, scale: 0 };
		let mut length = now;
		layers.AEGP_GetCompNumLayers.unwrap()(comp, &mut count);
		layers.AEGP_GetLayerID.unwrap()(host, &mut id);
		layers.AEGP_GetLayerCurrentTime.unwrap()(host, AEGP_LTimeMode_CompTime as _, &mut now);
		layers.AEGP_GetLayerDuration.unwrap()(host, AEGP_LTimeMode_LayerTime as _, &mut length);
		(count, id, (now.value, now.scale), (length.value, length.scale))
	});
	assert_eq!(seen, (1, 1, (12, 24), (30 * 24, 24)));
}
