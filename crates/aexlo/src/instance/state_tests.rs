//! `PF_GetCurrentState`: a state changes when what it covers does, and only
//! then.

use super::project_tests::{during, load};
use crate::sys::*;
use crate::{
	Comp, Depth8, Depth16, Keyframe, Layer, LayerContent, MaskPath, MaskVertex, ParamValue, PluginInstance, Project,
	ProjectLayer, ShapeItem, ShapePath, TextLayer, Time, TimePoint, Track,
};

const SLIDER: i32 = 1;
const LAYER: i32 = 2;
const PATH: i32 = 3;
const ARBITRARY: i32 = 4;
const EXCLUDED: i32 = 5;
const ALL: i32 = PF_ParamIndex_CHECK_ALL;
const NO_LAYERS: i32 = PF_ParamIndex_CHECK_ALL_EXCEPT_LAYER_PARAMS;
const HONOUR: i32 = PF_ParamIndex_CHECK_ALL_HONOR_EXCLUDE;

/// The test effect with a parameter of each kind a state treats differently,
/// and the arbitrary data's handle.
fn effect() -> (PluginInstance, PF_Handle) {
	let mut fx = load();
	fx.set_input_layer(grey(10));
	// SAFETY: a fresh handle of the host's.
	let handle = unsafe { crate::suites::handle::host_new_handle_impl(4) };
	let kinds = [
		PF_Param_FLOAT_SLIDER as PF_ParamType,
		PF_Param_LAYER as PF_ParamType,
		PF_Param_PATH as PF_ParamType,
		PF_Param_ARBITRARY_DATA as PF_ParamType,
		PF_Param_FLOAT_SLIDER as PF_ParamType,
	];
	for (index, kind) in (1..).zip(kinds) {
		// SAFETY: a parameter definition is plain old data.
		let mut def: PF_ParamDef = unsafe { std::mem::zeroed() };
		def.param_type = kind;
		if index == ARBITRARY {
			def.u.arb_d.value = handle;
		}
		if index == EXCLUDED {
			def.flags = PF_ParamFlag_EXCLUDE_FROM_HAVE_INPUTS_CHANGED as PF_ParamFlags;
		}
		fx.add_instance_param(def);
	}
	(fx, handle)
}

fn grey(value: u8) -> Layer<Depth8> {
	Layer::<Depth8>::from_rgba8([value, value, value, 255].repeat(64 * 32), 64, 32).unwrap()
}

fn mask(x: f64) -> MaskPath {
	MaskPath {
		id: 1,
		vertices: vec![
			MaskVertex::corner(x, 1.0),
			MaskVertex::corner(30.0, 20.0),
			MaskVertex::corner(2.0, 20.0),
		],
		closed: true,
		..Default::default()
	}
}

fn at(value: i32, scale: u32) -> TimePoint {
	TimePoint::new(value, scale).unwrap()
}

/// The state of `index`, over a range given in 30ths of a second.
fn state(fx: &mut PluginInstance, index: i32, range: Option<(i32, i32)>) -> PF_State {
	state_in(fx, index, range.map(|(start, length)| (at(start, 30), at(length, 30))))
}

fn state_in(fx: &mut PluginInstance, index: i32, range: Option<(TimePoint, TimePoint)>) -> PF_State {
	let (err, state) = during(fx, |p| unsafe {
		let utils: &PF_ParamUtilsSuite3 = p.suite(c"PF Param Utils Suite", 3);
		let times = range.map(|(start, length)| (start.to_sys(), length.to_sys()));
		let (start, length) = match &times {
			Some((start, length)) => (start as *const A_Time, length as *const A_Time),
			None => (std::ptr::null(), std::ptr::null()),
		};
		let mut state: PF_State = std::mem::zeroed();
		let err = utils.PF_GetCurrentState.unwrap()(p.effect, index as _, start, length, &mut state);
		(err, state)
	});
	assert_eq!(err, PF_Err_NONE, "index {index}");
	state
}

fn identical(fx: &mut PluginInstance, a: &PF_State, b: &PF_State) -> bool {
	let (err, same) = during(fx, |p| unsafe {
		let utils: &PF_ParamUtilsSuite3 = p.suite(c"PF Param Utils Suite", 3);
		let mut same = 0;
		let err = utils.PF_AreStatesIdentical.unwrap()(p.effect, a, b, &mut same);
		(err, same)
	});
	assert_eq!(err, PF_Err_NONE);
	same != 0
}

/// Whether each of `indices`' states stays the same through `change`.
fn unchanged_by<const N: usize>(
	fx: &mut PluginInstance,
	indices: [i32; N],
	change: impl FnOnce(&mut PluginInstance),
) -> [bool; N] {
	let before = indices.map(|index| state(fx, index, None));
	change(fx);
	std::array::from_fn(|i| {
		let after = state(fx, indices[i], None);
		identical(fx, &before[i], &after)
	})
}

#[test]
fn the_input_layers_pixels_are_covered() {
	let (mut fx, _) = effect();
	let seen = unchanged_by(&mut fx, [0, ALL, HONOUR, SLIDER, NO_LAYERS], |fx| {
		fx.set_input_layer(grey(200))
	});
	assert_eq!(seen, [false, false, false, true, true]);
	let seen = unchanged_by(&mut fx, [0, ALL], |fx| fx.set_input_layer(grey(200)));
	assert_eq!(seen, [true, true], "the same picture set again is no change");
	let seen = unchanged_by(&mut fx, [0], |fx| fx.set_input_layer(grey(200).convert::<Depth16>()));
	assert_eq!(seen, [false], "another depth is");
}

#[test]
fn a_layer_parameters_pixels_and_link_are_covered() {
	let (mut fx, _) = effect();
	let index = LAYER as usize;
	let seen = unchanged_by(&mut fx, [LAYER, ALL, 0, NO_LAYERS], |fx| {
		fx.set_layer_param(index, Some(grey(20))).unwrap();
	});
	assert_eq!(seen, [false, false, true, true], "linking");
	let seen = unchanged_by(&mut fx, [LAYER], |fx| {
		fx.set_layer_param(index, Some(grey(90))).unwrap()
	});
	assert_eq!(seen, [false], "other pixels");
	let seen = unchanged_by(&mut fx, [LAYER], |fx| {
		fx.set_layer_param(index, Some(grey(90))).unwrap()
	});
	assert_eq!(seen, [true], "the same pixels again");
	let seen = unchanged_by(&mut fx, [LAYER], |fx| fx.clear_layer_param(index).unwrap());
	assert_eq!(seen, [false], "unlinking");
}

#[test]
fn values_paths_and_arbitrary_data_are_covered() {
	let (mut fx, handle) = effect();
	let seen = unchanged_by(&mut fx, [SLIDER, ALL, NO_LAYERS, 0, PATH], |fx| {
		fx.set_param(SLIDER as usize, ParamValue::Float(9.0)).unwrap();
	});
	assert_eq!(seen, [false, false, false, true, true], "a value");

	let seen = unchanged_by(&mut fx, [0, PATH, SLIDER], |fx| fx.set_mask_paths(vec![mask(4.0)]));
	assert_eq!(seen, [false, false, true], "a mask");

	let seen = unchanged_by(&mut fx, [ARBITRARY, ALL, SLIDER], |_| {
		// SAFETY: the handle has four bytes.
		unsafe { *(*handle).cast::<u8>().add(3) = 7 };
	});
	assert_eq!(seen, [false, false, true], "arbitrary data edited in place");

	let seen = unchanged_by(&mut fx, [0], |fx| {
		fx.set_text_layer(Some(TextLayer::from_outlines("text", vec![mask(9.0)])));
	});
	assert_eq!(seen, [false], "the text on the layer");

	let seen = unchanged_by(&mut fx, [EXCLUDED, ALL, NO_LAYERS, HONOUR], |fx| {
		fx.set_param(EXCLUDED as usize, ParamValue::Float(3.0)).unwrap();
	});
	assert_eq!(seen, [false, false, false, true], "only HONOR_EXCLUDE leaves it out");
}

/// From 0 at 0 s to 100 at 1 s, held from there.
fn ramp() -> Track<ParamValue> {
	Track::keyed(vec![
		Keyframe::linear(at(0, 1), ParamValue::Float(0.0)),
		Keyframe::linear(at(1, 1), ParamValue::Float(100.0)),
	])
	.unwrap()
}

#[test]
fn a_keyframed_value_is_covered_over_the_range_asked_about() {
	let (mut fx, _) = effect();
	let slider = SLIDER as usize;
	fx.set_param_track(slider, ramp()).unwrap();

	let (frame0, frame15) = (
		state(&mut fx, SLIDER, Some((0, 1))),
		state(&mut fx, SLIDER, Some((15, 1))),
	);
	assert!(!identical(&mut fx, &frame0, &frame15), "0 and 50 are different");
	let (late, later) = (
		state(&mut fx, SLIDER, Some((45, 1))),
		state(&mut fx, SLIDER, Some((200, 1))),
	);
	assert!(identical(&mut fx, &late, &later), "past the last key nothing moves");
	let in_24ths = state_in(&mut fx, SLIDER, Some((at(12, 24), at(1, 30))));
	assert!(
		identical(&mut fx, &frame15, &in_24ths),
		"the same instant in another scale"
	);
	let (instant, range) = (
		state(&mut fx, SLIDER, Some((15, 0))),
		state(&mut fx, SLIDER, Some((15, 10))),
	);
	assert!(!identical(&mut fx, &instant, &range), "what moves in the range counts");

	// The caller's clock is not in the state.
	fx.set_time(Time::frame(20, 30));
	let again = state(&mut fx, SLIDER, Some((15, 1)));
	assert!(
		identical(&mut fx, &frame15, &again),
		"the same question, the same answer"
	);

	// Editing outside a range leaves it; inside does not, though both ends stay.
	let (early, span, whole) = (
		state(&mut fx, SLIDER, Some((0, 10))),
		state(&mut fx, SLIDER, Some((30, 30))),
		state(&mut fx, SLIDER, None),
	);
	let mut keys = ramp().keys().to_vec();
	keys.push(Keyframe::linear(at(45, 30), ParamValue::Float(70.0)));
	keys.push(Keyframe::linear(at(2, 1), ParamValue::Float(100.0)));
	fx.set_param_track(slider, Track::keyed(keys).unwrap()).unwrap();
	let after = (
		state(&mut fx, SLIDER, Some((0, 10))),
		state(&mut fx, SLIDER, Some((30, 30))),
		state(&mut fx, SLIDER, None),
	);
	assert!(identical(&mut fx, &early, &after.0), "an edit outside the range");
	assert!(!identical(&mut fx, &span, &after.1), "a dip between two equal ends");
	assert!(!identical(&mut fx, &whole, &after.2), "null is all time");
}

#[test]
fn how_a_segment_runs_is_covered() {
	let (mut fx, _) = effect();
	fx.set_param_track(SLIDER as usize, ramp()).unwrap();
	let linear = state(&mut fx, SLIDER, Some((0, 30)));
	let held = Track::keyed(vec![
		Keyframe::hold(at(0, 1), ParamValue::Float(0.0)),
		Keyframe::linear(at(1, 1), ParamValue::Float(100.0)),
	]);
	fx.set_param_track(SLIDER as usize, held.unwrap()).unwrap();
	let hold = state(&mut fx, SLIDER, Some((0, 30)));
	assert!(
		!identical(&mut fx, &linear, &hold),
		"equal ends, another way between them"
	);
}

#[test]
fn a_still_value_has_one_state_in_every_range() {
	let (mut fx, _) = effect();
	let states = [
		state(&mut fx, SLIDER, Some((0, 1))),
		state(&mut fx, SLIDER, Some((900, 45))),
		state(&mut fx, SLIDER, None),
	];
	assert!(identical(&mut fx, &states[0], &states[1]));
	assert!(identical(&mut fx, &states[0], &states[2]));
}

#[test]
fn wide_time_input_covers_all_time_before_any_checkout() {
	let (mut fx, _) = effect();
	fx.global_out_flags2 |= PF_OutFlag2_AUTOMATIC_WIDE_TIME_INPUT as PF_OutFlags2;
	fx.set_param_track(SLIDER as usize, ramp()).unwrap();
	let early = state(&mut fx, SLIDER, Some((0, 1)));
	let later = state(&mut fx, SLIDER, Some((15, 1)));
	assert!(!identical(&mut fx, &early, &later), "and still tells times apart");
	let mut keys = ramp().keys().to_vec();
	keys.push(Keyframe::linear(at(3, 1), ParamValue::Float(5.0)));
	fx.set_param_track(SLIDER as usize, Track::keyed(keys).unwrap())
		.unwrap();
	let edited = state(&mut fx, SLIDER, Some((0, 1)));
	assert!(
		!identical(&mut fx, &early, &edited),
		"a change outside the range counts"
	);
}

#[test]
fn a_shape_moving_on_the_layer_is_covered() {
	let (mut fx, _) = effect();
	let comp = Comp::new("Comp", 64, 32, TimePoint::whole_seconds(5));
	let moving = Track::keyed(vec![
		Keyframe::linear(at(0, 1), mask(0.0)),
		Keyframe::linear(at(1, 1), mask(40.0)),
	]);
	let content = LayerContent::Shape(vec![ShapeItem::Path(ShapePath::new("Path", moving.unwrap()))]);
	let layer = ProjectLayer::new(1, "Shape", content, &comp);
	fx.set_project(Some(Project::new(comp, vec![layer], 1).unwrap()));
	let (frame0, frame15) = (state(&mut fx, 0, Some((0, 1))), state(&mut fx, 0, Some((15, 1))));
	assert!(!identical(&mut fx, &frame0, &frame15));
}

#[test]
fn identical_checkout_compares_the_two_times_only() {
	let (mut fx, _) = effect();
	let there_and_back = Track::keyed(vec![
		Keyframe::linear(at(0, 1), ParamValue::Float(1.0)),
		Keyframe::linear(at(1, 1), ParamValue::Float(9.0)),
		Keyframe::linear(at(2, 1), ParamValue::Float(1.0)),
	]);
	fx.set_param_track(SLIDER as usize, there_and_back.unwrap()).unwrap();
	let seen = during(&mut fx, |p| unsafe {
		let utils: &PF_ParamUtilsSuite3 = p.suite(c"PF Param Utils Suite", 3);
		[(SLIDER, 60), (SLIDER, 30), (ALL, 30), (LAYER, 30)].map(|(index, other)| {
			let mut same = 0;
			utils.PF_IsIdenticalCheckout.unwrap()(p.effect, index as _, 0, 1, 30, other, 1, 30, &mut same);
			same != 0
		})
	});
	assert_eq!(seen, [true, false, false, true]);
}

#[test]
fn what_cannot_be_answered_is_not_called_identical() {
	let (mut fx, _) = effect();
	let (null, unknown, same) = during(&mut fx, |p| unsafe {
		let utils: &PF_ParamUtilsSuite3 = p.suite(c"PF Param Utils Suite", 3);
		let mut state: PF_State = std::mem::zeroed();
		let mut same = 0;
		let null = utils.PF_AreStatesIdentical.unwrap()(p.effect, &state, std::ptr::null(), &mut same);
		let unknown = utils.PF_GetCurrentState.unwrap()(p.effect, -9, std::ptr::null(), std::ptr::null(), &mut state);
		(null, unknown, same)
	});
	assert_eq!((null, unknown), (PF_Err_BAD_CALLBACK_PARAM, PF_Err_BAD_CALLBACK_PARAM));
	assert_eq!(same, 0);
}

/// A mask that moves from x = 0 at 0 s to x = 10 at 1 s.
fn moving_mask() -> Track<MaskPath> {
	Track::keyed(vec![
		Keyframe::linear(at(0, 1), mask(0.0)),
		Keyframe::linear(at(1, 1), mask(10.0)),
	])
	.unwrap()
}

#[test]
fn moving_masks_are_covered_over_the_range_and_compared_at_two_times() {
	let (mut fx, _) = effect();
	fx.set_mask_tracks(vec![moving_mask()]);
	assert_eq!(fx.mask_paths()[0].vertices[0].x, 0.0, "the mask now, at 0 s");

	// Over a range the mask moves in, the path parameter and the input change
	// with where the range is; over one after the last key they do not.
	for index in [PATH, 0] {
		let (early, later) = (state(&mut fx, index, Some((0, 3))), state(&mut fx, index, Some((6, 3))));
		assert!(!identical(&mut fx, &early, &later), "{index}");
		let (after, after_too) = (
			state(&mut fx, index, Some((40, 3))),
			state(&mut fx, index, Some((50, 3))),
		);
		assert!(identical(&mut fx, &after, &after_too), "{index}");
	}

	let seen = during(&mut fx, |p| unsafe {
		let utils: &PF_ParamUtilsSuite3 = p.suite(c"PF Param Utils Suite", 3);
		// 0 s against 0.5 s, and 1 s against 2 s.
		[(PATH, 0, 15), (PATH, 30, 60), (0, 0, 15), (SLIDER, 0, 15)].map(|(index, t1, t2)| {
			let mut same = 0;
			utils.PF_IsIdenticalCheckout.unwrap()(p.effect, index as _, t1, 1, 30, t2, 1, 30, &mut same);
			same != 0
		})
	});
	assert_eq!(seen, [false, true, false, true]);
}

#[test]
fn a_path_is_checked_out_at_the_time_asked_for() {
	let (mut fx, _) = effect();
	fx.set_mask_tracks(vec![moving_mask()]);
	let (now, half) = during(&mut fx, |p| unsafe {
		let query: &PF_PathQuerySuite1 = p.suite(c"PF Path Query Suite", 1);
		let data: &PF_PathDataSuite1 = p.suite(c"PF Path Data Suite", 1);
		let first_x = |path: PF_PathOutlinePtr| {
			let mut vertex: PF_PathVertex = std::mem::zeroed();
			data.PF_PathVertexInfo.unwrap()(p.effect, path, 0, &mut vertex);
			vertex.x
		};
		let mut path = std::ptr::null_mut();
		query.PF_CheckoutPath.unwrap()(p.effect, 1, 0, 1, 30, &mut path);
		let now = first_x(path);
		query.PF_CheckinPath.unwrap()(p.effect, 1, 0, path);
		query.PF_CheckoutPath.unwrap()(p.effect, 1, 15, 1, 30, &mut path);
		let half = first_x(path);
		query.PF_CheckinPath.unwrap()(p.effect, 1, 0, path);
		(now, half)
	});
	assert_eq!((now, half), (0.0, 5.0));
	assert!(
		fx.checked_out_paths.lock().unwrap().is_empty(),
		"a checked-in path is freed"
	);

	fx.set_time(Time::frame(30, 30));
	assert_eq!(fx.mask_paths()[0].vertices[0].x, 10.0, "the mask follows the time");
}

#[test]
fn a_mask_stream_reads_the_outline_at_the_time_asked_for() {
	let (mut fx, _) = effect();
	fx.set_mask_tracks(vec![moving_mask()]);
	let (xs, keys) = during(&mut fx, |p| unsafe {
		let masks: &AEGP_MaskSuite6 = p.suite(c"AEGP Layer Mask Suite", kAEGPMaskSuiteVersion6);
		let streams: &AEGP_StreamSuite6 = p.suite(c"AEGP Stream Suite", kAEGPStreamSuiteVersion6);
		let outlines: &AEGP_MaskOutlineSuite3 = p.suite(c"AEGP Mask Outline Suite", kAEGPMaskOutlineSuiteVersion3);
		let keyframes: &AEGP_KeyframeSuite5 = p.suite(c"AEGP Keyframe Suite", kAEGPKeyframeSuiteVersion5);
		let mut mask = std::ptr::null_mut();
		masks.AEGP_GetLayerMaskByIndex.unwrap()(p.effect as AEGP_LayerH, 0, &mut mask);
		let mut stream = std::ptr::null_mut();
		streams.AEGP_GetNewMaskStream.unwrap()(0, mask, AEGP_MaskStream_OUTLINE, &mut stream);
		let xs = [0, 15, 30].map(|frame| {
			let time = A_Time {
				value: frame,
				scale: 30,
			};
			let mut value: AEGP_StreamValue2 = std::mem::zeroed();
			streams.AEGP_GetNewStreamValue.unwrap()(0, stream, AEGP_LTimeMode_LayerTime as _, &time, 0, &mut value);
			let mut vertex: AEGP_MaskVertex = std::mem::zeroed();
			outlines.AEGP_GetMaskOutlineVertexInfo.unwrap()(value.val.mask, 0, &mut vertex);
			streams.AEGP_DisposeStreamValue.unwrap()(&mut value);
			vertex.x
		});
		let mut keys = 0;
		keyframes.AEGP_GetStreamNumKFs.unwrap()(stream, &mut keys);
		streams.AEGP_DisposeStream.unwrap()(stream);
		masks.AEGP_DisposeMask.unwrap()(mask);
		(xs, keys)
	});
	assert_eq!(xs, [0.0, 5.0, 10.0]);
	assert_eq!(keys, 2);
}

/// What the flattening effect's arbitrary value points to.
static POINTED_TO: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);

/// The test effect, plus flattening its arbitrary value: the flat form is
/// what the value points to, which its handle's bytes do not show.
unsafe extern "C" fn flattening_entry(
	cmd: PF_Cmd,
	input: *mut PF_InData,
	output: *mut PF_OutData,
	params: PF_ParamList,
	world: *mut PF_LayerDef,
	extra: *mut std::ffi::c_void,
) -> PF_Err {
	if cmd != PF_Cmd_ARBITRARY_CALLBACK as PF_Cmd {
		return unsafe { super::project_tests::entry(cmd, input, output, params, world, extra) };
	}
	let extra = unsafe { &mut *extra.cast::<PF_ArbParamsExtra>() };
	let pointed = POINTED_TO.load(std::sync::atomic::Ordering::SeqCst).to_le_bytes();
	unsafe {
		if extra.which_function == PF_Arbitrary_FLAT_SIZE_FUNC as _ {
			*extra.u.flat_size_func_params.flat_data_sizePLu = pointed.len() as A_u_long;
		} else if extra.which_function == PF_Arbitrary_FLATTEN_FUNC as _ {
			let out = extra.u.flatten_func_params.flat_dataPV.cast::<u8>();
			std::ptr::copy_nonoverlapping(pointed.as_ptr(), out, pointed.len());
		}
	}
	PF_Err_NONE
}

#[test]
fn arbitrary_data_is_covered_as_the_plugin_flattens_it() {
	// SAFETY: `flattening_entry` has the effect entry point's signature.
	let mut fx = unsafe { crate::Host::get().entry_loader(flattening_entry) }
		.size(64, 32)
		.load()
		.unwrap();
	// SAFETY: a fresh handle of the host's, and a parameter definition is plain old data.
	let mut def: PF_ParamDef = unsafe { std::mem::zeroed() };
	def.param_type = PF_Param_ARBITRARY_DATA as PF_ParamType;
	def.u.arb_d.value = unsafe { crate::suites::handle::host_new_handle_impl(4) };
	fx.add_instance_param(def);
	let index = fx.param_count() - 1;

	// Taken in when the plugin may have changed its values.
	fx.user_changed_param(index).unwrap();
	assert!(fx.arb_digest(index).is_some());
	let before = state(&mut fx, index as i32, None);
	POINTED_TO.store(2, std::sync::atomic::Ordering::SeqCst);
	fx.user_changed_param(index).unwrap();
	let after = state(&mut fx, index as i32, None);
	assert!(!identical(&mut fx, &before, &after), "what the value points to changed");
	fx.user_changed_param(index).unwrap();
	let again = state(&mut fx, index as i32, None);
	assert!(identical(&mut fx, &after, &again));
}

#[test]
fn wide_time_input_widens_a_range_by_the_checkouts_seen() {
	let (mut fx, _) = effect();
	fx.global_out_flags2 |= PF_OutFlag2_AUTOMATIC_WIDE_TIME_INPUT as PF_OutFlags2;
	// At 1 s the effect checked out 10 frames back, and now.
	fx.set_time(Time::frame(30, 30));
	fx.record_checkout_time(at(20, 30));
	fx.record_checkout_time(at(30, 30));
	let with = |extra: Option<(i32, f64)>| {
		let mut keys = vec![
			Keyframe::hold(at(0, 1), ParamValue::Float(1.0)),
			Keyframe::hold(at(4, 1), ParamValue::Float(1.0)),
		];
		keys.extend(extra.map(|(frame, value)| Keyframe::hold(at(frame, 30), ParamValue::Float(value))));
		Track::keyed(keys).unwrap()
	};
	// Asked about 2 s; it needs from 10 frames before, and everything the
	// checkouts reached, 20 frames to 1 s.
	let covered = |fx: &mut PluginInstance, extra| {
		fx.set_param_track(SLIDER as usize, with(extra)).unwrap();
		state(fx, SLIDER, Some((60, 1)))
	};
	let plain = covered(&mut fx, None);
	let needed = covered(&mut fx, Some((54, 7.0)));
	let reached = covered(&mut fx, Some((25, 7.0)));
	let unneeded = covered(&mut fx, Some((100, 7.0)));
	assert!(
		!identical(&mut fx, &plain, &needed),
		"a change before the range it needs counts"
	);
	assert!(
		!identical(&mut fx, &plain, &reached),
		"so does one where a checkout reached"
	);
	assert!(identical(&mut fx, &plain, &unneeded), "one after the range does not");
}
