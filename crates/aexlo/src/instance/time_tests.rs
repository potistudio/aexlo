//! Values over time as a plugin reads them: every API at the same instant
//! gives the same value, and a shape or text layer's contents are read as
//! streams.

use std::ffi::CStr;

use super::project_tests::{Plugin, during, load};
use crate::sys::*;
use crate::{
	Comp, Keyframe, LayerContent, MaskPath, MaskVertex, ParamValue, PluginInstance, Project, ProjectLayer, ShapeGroup,
	ShapeItem, ShapePath, TextLayer, Time, TimePoint, Track,
};

const SLIDER: usize = 1;

fn at(value: i32, scale: u32) -> TimePoint {
	TimePoint::new(value, scale).unwrap()
}

/// The test effect with one float slider, at 7.
fn with_slider() -> PluginInstance {
	let mut fx = load();
	// SAFETY: a parameter definition is plain old data.
	let mut def: PF_ParamDef = unsafe { std::mem::zeroed() };
	def.param_type = PF_Param_FLOAT_SLIDER as PF_ParamType;
	def.u.fs_d.value = 7.0;
	fx.add_instance_param(def);
	assert_eq!(fx.get_param(SLIDER), Some(ParamValue::Float(7.0)));
	fx
}

/// From 10 at -1 s to 30 at 1 s, held until 100 at 2 s.
fn slope() -> Track<ParamValue> {
	Track::keyed(vec![
		Keyframe::linear(at(-30, 30), ParamValue::Float(10.0)),
		Keyframe::hold(at(1000, 1000), ParamValue::Float(30.0)),
		Keyframe::linear(at(2, 1), ParamValue::Float(100.0)),
	])
	.unwrap()
}

/// The effect's layer 2 s into the comp, at half speed.
fn stretched(content: LayerContent) -> Project {
	let comp = Comp::new("Main", 320, 180, TimePoint::whole_seconds(10));
	let mut host = ProjectLayer::new(5, "Host", content, &comp);
	host.start = TimePoint::whole_seconds(2);
	host.stretch = (2, 1);
	let other = ProjectLayer::new(6, "Other", LayerContent::Footage, &comp);
	Project::new(comp, vec![host, other], 5).unwrap()
}

struct Streams {
	dynamic: &'static AEGP_DynamicStreamSuite4,
	stream: &'static AEGP_StreamSuite6,
	root: AEGP_StreamRefH,
}

impl Streams {
	unsafe fn of(p: &Plugin, layer: AEGP_LayerH) -> Self {
		let dynamic: &AEGP_DynamicStreamSuite4 = p.suite(c"AEGP Dynamic Stream Suite", kAEGPDynamicStreamSuiteVersion4);
		let stream: &AEGP_StreamSuite6 = p.suite(c"AEGP Stream Suite", kAEGPStreamSuiteVersion6);
		let mut root: AEGP_StreamRefH = std::ptr::null_mut();
		unsafe { dynamic.AEGP_GetNewStreamRefForLayer.unwrap()(0, layer, &mut root) };
		Self { dynamic, stream, root }
	}

	unsafe fn named(&self, group: AEGP_StreamRefH, name: &CStr) -> AEGP_StreamRefH {
		let mut out: AEGP_StreamRefH = std::ptr::null_mut();
		unsafe { self.dynamic.AEGP_GetNewStreamRefByMatchname.unwrap()(0, group, name.as_ptr(), &mut out) };
		out
	}

	unsafe fn child(&self, group: AEGP_StreamRefH, index: usize) -> AEGP_StreamRefH {
		let mut out: AEGP_StreamRefH = std::ptr::null_mut();
		unsafe { self.dynamic.AEGP_GetNewStreamRefByIndex.unwrap()(0, group, index as _, &mut out) };
		out
	}

	unsafe fn children(&self, group: AEGP_StreamRefH) -> A_long {
		let mut count = -1;
		unsafe { self.dynamic.AEGP_GetNumStreamsInGroup.unwrap()(group, &mut count) };
		count
	}

	unsafe fn read(&self, stream: AEGP_StreamRefH, comp_time: bool, time: TimePoint) -> (PF_Err, AEGP_StreamValue2) {
		let mode = if comp_time {
			AEGP_LTimeMode_CompTime
		} else {
			AEGP_LTimeMode_LayerTime
		};
		let time = time.to_sys();
		unsafe {
			let mut value: AEGP_StreamValue2 = std::mem::zeroed();
			let err = self.stream.AEGP_GetNewStreamValue.unwrap()(0, stream, mode as _, &time, 0, &mut value);
			(err, value)
		}
	}

	/// The hosted effect's slider.
	unsafe fn slider(&self) -> AEGP_StreamRefH {
		unsafe {
			let parade = self.named(self.root, c"ADBE Effect Parade");
			self.child(self.child(parade, 0), SLIDER)
		}
	}
}

#[test]
fn every_api_gives_the_same_value_at_the_same_instant() {
	// Layer times, in different scales, and the value there.
	let cases = [
		(-3, 1, 10.0),
		(-30, 30, 10.0),
		(-500, 1000, 15.0),
		(-12, 24, 15.0),
		(0, 30, 20.0),
		(12, 24, 25.0),
		(512, 1024, 25.0),
		(1, 1, 30.0),
		(1999, 1000, 30.0),
		(60, 30, 100.0),
		(250, 25, 100.0),
	];
	for stretch in [false, true] {
		let mut fx = with_slider();
		if stretch {
			fx.set_project(Some(stretched(LayerContent::Footage)));
		}
		fx.set_param_track(SLIDER, slope()).unwrap();
		// Backwards, then forwards: no read depends on the one before.
		for &(value, scale, expected) in cases.iter().rev().chain(&cases) {
			let layer = at(value, scale);
			fx.set_time(Time::new(value, 1, scale).unwrap());
			let params_array = fx.get_param(SLIDER);
			let comp = if stretch {
				at(2 * value + 2 * scale as i32, scale)
			} else {
				layer
			};
			let seen = during(&mut fx, |p| unsafe {
				let input = &*p.input;
				let mut def: PF_ParamDef = std::mem::zeroed();
				input.inter.checkout_param.unwrap()(p.effect, SLIDER as _, value, 1, scale, &mut def);
				let streams = Streams::of(p, p.effect as AEGP_LayerH);
				let slider = streams.slider();
				let in_layer = streams.read(slider, false, layer).1.val.one_d;
				let in_comp = streams.read(slider, true, comp).1.val.one_d;
				[def.u.fs_d.value, in_layer, in_comp]
			});
			let case = format!("{value}/{scale}, stretched: {stretch}");
			assert_eq!(params_array, Some(ParamValue::Float(expected)), "params, {case}");
			assert_eq!(seen, [expected; 3], "checkout, stream in layer and comp time, {case}");
		}
	}
}

#[test]
fn keyframe_queries_agree_with_the_evaluator() {
	let mut fx = with_slider();
	fx.set_project(Some(stretched(LayerContent::Footage)));
	fx.set_param_track(SLIDER, slope()).unwrap();
	let (keys, counted, found, missing) = during(&mut fx, |p| unsafe {
		let keyframes: &AEGP_KeyframeSuite5 = p.suite(c"AEGP Keyframe Suite", 5);
		let utils: &PF_ParamUtilsSuite3 = p.suite(c"PF Param Utils Suite", 3);
		let streams = Streams::of(p, p.effect as AEGP_LayerH);
		let slider = streams.slider();
		let (mut count, mut counted) = (-1, -1);
		keyframes.AEGP_GetStreamNumKFs.unwrap()(slider, &mut count);
		utils.PF_GetKeyframeCount.unwrap()(p.effect, SLIDER as _, &mut counted);
		let mut keys = Vec::new();
		for index in 0..count {
			let mut time = A_Time { value: 0, scale: 1 };
			keyframes.AEGP_GetKeyframeTime.unwrap()(slider, index, AEGP_LTimeMode_CompTime as _, &mut time);
			let mut value: AEGP_StreamValue2 = std::mem::zeroed();
			keyframes.AEGP_GetNewKeyframeValue.unwrap()(0, slider, index, &mut value);
			let mut def: PF_ParamDef = std::mem::zeroed();
			let (mut layer, mut scale) = (0, 1);
			utils.PF_CheckoutKeyframe.unwrap()(p.effect, SLIDER as _, index as _, &mut layer, &mut scale, &mut def);
			let comp_seconds = f64::from(time.value) / f64::from(time.scale);
			let layer_seconds = f64::from(layer) / f64::from(scale);
			keys.push([comp_seconds, layer_seconds, value.val.one_d, def.u.fs_d.value]);
		}
		let mut any = 0;
		let (mut index, mut time, mut scale) = (-1, 0, 1);
		utils.PF_FindKeyframeTime.unwrap()(
			p.effect,
			SLIDER as _,
			15,
			30,
			PF_TimeDir_GREATER_THAN as _,
			&mut any,
			&mut index,
			&mut time,
			&mut scale,
		);
		let mut unused = A_Time { value: 0, scale: 1 };
		let missing = keyframes.AEGP_GetKeyframeTime.unwrap()(slider, count, 0, &mut unused);
		(keys, counted, (any, index), missing)
	});
	assert_eq!(
		keys,
		[
			[0.0, -1.0, 10.0, 10.0],
			[4.0, 1.0, 30.0, 30.0],
			[6.0, 2.0, 100.0, 100.0]
		]
	);
	assert_eq!(counted, 3);
	assert_eq!(found, (1, 1), "the first key after 0.5 s");
	assert_eq!(missing, PF_Err_INVALID_INDEX);
}

#[test]
fn a_keyframed_value_is_the_embedders_to_write() {
	let mut fx = with_slider();
	fx.set_param_track(SLIDER, slope()).unwrap();
	let err = during(&mut fx, |p| unsafe {
		let streams = Streams::of(p, p.effect as AEGP_LayerH);
		let slider = streams.slider();
		let (_, mut value) = streams.read(slider, false, TimePoint::ZERO);
		value.val.one_d = 55.0;
		streams.stream.AEGP_SetStreamValue.unwrap()(0, slider, &mut value)
	});
	assert_ne!(err, PF_Err_NONE, "a write with nowhere to go is refused");
	assert_eq!(fx.get_param(SLIDER), Some(ParamValue::Float(20.0)));
	let wrong = Track::keyed(vec![Keyframe::linear(at(0, 1), ParamValue::Slider(3))]).unwrap();
	assert!(fx.set_param_track(SLIDER, wrong).is_err());

	fx.set_param(SLIDER, ParamValue::Float(4.0)).unwrap();
	fx.set_time(Time::frame(30, 30));
	assert_eq!(
		fx.get_param(SLIDER),
		Some(ParamValue::Float(4.0)),
		"a value replaces the keys"
	);
	assert!(fx.param_track(SLIDER).is_none());
}

#[test]
fn instances_on_threads_do_not_share_a_clock() {
	let threads: Vec<_> = (0..6)
		.map(|thread| {
			std::thread::spawn(move || {
				let mut fx = with_slider();
				let end = f64::from(thread + 1) * 60.0;
				let track = Track::keyed(vec![
					Keyframe::linear(at(0, 1), ParamValue::Float(0.0)),
					Keyframe::linear(at(2, 1), ParamValue::Float(end)),
				]);
				fx.set_param_track(SLIDER, track.unwrap()).unwrap();
				(0..120).all(|step| {
					let frame = step * 37 % 61;
					fx.set_time(Time::frame(frame, 30));
					let seen = during(&mut fx, |p| unsafe {
						let mut def: PF_ParamDef = std::mem::zeroed();
						(*p.input).inter.checkout_param.unwrap()(p.effect, SLIDER as _, frame, 1, 30, &mut def);
						def.u.fs_d.value
					});
					(seen - end * f64::from(frame) / 60.0).abs() < 1e-9
				})
			})
		})
		.collect();
	for thread in threads {
		assert!(thread.join().unwrap(), "every thread read its own values");
	}
}

/// A triangle whose apex moves from x = 0 to x = 100 over two layer seconds,
/// in a group.
fn morphing_shape() -> LayerContent {
	let triangle = |x: f64| MaskPath {
		vertices: vec![
			MaskVertex {
				x,
				y: 10.0,
				tan_out_x: 5.0,
				..Default::default()
			},
			MaskVertex::corner(80.0, 90.0),
			MaskVertex::corner(20.0, 90.0),
		],
		closed: true,
		..Default::default()
	};
	let track = Track::keyed(vec![
		Keyframe::linear(at(0, 1), triangle(0.0)),
		Keyframe::linear(at(2, 1), triangle(100.0)),
	]);
	let path = ShapeItem::Path(ShapePath::new("Path 1", track.unwrap()));
	LayerContent::Shape(vec![ShapeItem::Group(ShapeGroup::new("Shape 1", vec![path]))])
}

#[test]
fn a_shape_layers_paths_are_streams() {
	let mut fx = load();
	fx.set_project(Some(stretched(morphing_shape())));
	let (vertices, effects, has_contents) = during(&mut fx, |p| unsafe {
		let outlines: &AEGP_MaskOutlineSuite3 = p.suite(c"AEGP Mask Outline Suite", kAEGPMaskOutlineSuiteVersion3);
		let streams = Streams::of(p, p.effect as AEGP_LayerH);
		let root = streams.named(streams.root, c"ADBE Root Vectors Group");
		let items = streams.named(streams.child(root, 0), c"ADBE Vectors Group");
		let path = streams.named(streams.child(items, 0), c"ADBE Vector Shape");
		// Comp 2 s, 4 s and 6 s are layer 0 s, 1 s and 2 s.
		let vertices = [2, 4, 6].map(|seconds| {
			let (_, mut value) = streams.read(path, true, TimePoint::whole_seconds(seconds));
			let mut vertex: PF_PathVertex = std::mem::zeroed();
			outlines.AEGP_GetMaskOutlineVertexInfo.unwrap()(value.val.mask, 0, &mut vertex);
			streams.stream.AEGP_DisposeStreamValue.unwrap()(&mut value);
			[vertex.x, vertex.tan_out_x]
		});

		let layers: &AEGP_LayerSuite5 = p.suite(c"AEGP Layer Suite", kAEGPLayerSuiteVersion5);
		let mut other: AEGP_LayerH = std::ptr::null_mut();
		layers.AEGP_GetLayerFromLayerID.unwrap()(p.effect as AEGP_CompH, 6, &mut other);
		// A layer that is not the effect's has a tree of its own.
		let theirs = Streams::of(p, other);
		let parade = theirs.named(theirs.root, c"ADBE Effect Parade");
		(vertices, theirs.children(parade), theirs.children(theirs.root) > 3)
	});
	assert_eq!(vertices, [[0.0, 5.0], [50.0, 5.0], [100.0, 5.0]]);
	assert_eq!(effects, 0, "the effect is on one layer only");
	assert!(!has_contents, "footage has no contents");
}

#[test]
fn a_changed_layers_old_streams_are_refused() {
	let mut fx = load();
	fx.set_project(Some(stretched(morphing_shape())));
	let old = during(&mut fx, |p| unsafe {
		let streams = Streams::of(p, p.effect as AEGP_LayerH);
		streams.named(streams.root, c"ADBE Root Vectors Group") as usize
	});
	assert_ne!(old, 0, "there was a stream to go stale");
	fx.set_project(Some(stretched(LayerContent::Shape(Vec::new()))));
	let err = during(&mut fx, |p| unsafe {
		let streams = Streams::of(p, p.effect as AEGP_LayerH);
		let mut count = -1;
		streams.dynamic.AEGP_GetNumStreamsInGroup.unwrap()(old as AEGP_StreamRefH, &mut count)
	});
	assert_ne!(err, PF_Err_NONE);
}

#[test]
fn source_text_holds_between_its_keys() {
	let mut fx = load();
	let text = Track::keyed(vec![
		Keyframe::linear(at(0, 1), TextLayer::from_outlines("first", Vec::new())),
		Keyframe::linear(at(1, 1), TextLayer::from_outlines("two\nlines", Vec::new())),
	]);
	fx.set_project(Some(stretched(LayerContent::Text(text.unwrap()))));
	let read = during(&mut fx, |p| unsafe {
		let documents: &AEGP_TextDocumentSuite1 = p.suite(c"AEGP Text Document Suite", 1);
		let memory: &AEGP_MemorySuite1 = p.suite(c"AEGP Memory Suite", kAEGPMemorySuiteVersion1);
		let streams = Streams::of(p, p.effect as AEGP_LayerH);
		let group = streams.named(streams.root, c"ADBE Text Properties");
		let stream = streams.named(group, c"ADBE Text Document");
		[(1, 2), (1, 1)].map(|(value, scale)| {
			let (_, mut read) = streams.read(stream, false, at(value, scale));
			let mut handle: AEGP_MemHandle = std::ptr::null_mut();
			let document = *(&raw const read.val).cast::<AEGP_TextDocumentH>();
			documents.AEGP_GetNewText.unwrap()(0, document, &mut handle);
			streams.stream.AEGP_DisposeStreamValue.unwrap()(&mut read);
			let mut data: *mut std::ffi::c_void = std::ptr::null_mut();
			memory.AEGP_LockMemHandle.unwrap()(handle, &mut data);
			let mut units = Vec::new();
			while *data.cast::<u16>().add(units.len()) != 0 {
				units.push(*data.cast::<u16>().add(units.len()));
			}
			memory.AEGP_UnlockMemHandle.unwrap()(handle);
			memory.AEGP_FreeMemHandle.unwrap()(handle);
			String::from_utf16_lossy(&units)
		})
	});
	assert_eq!(read, ["first", "two\rlines"]);
}

/// A legacy effect that fills its output with the slider's value: red as the
/// `params` array has it, green as a checkout at the current time has it.
unsafe extern "C" fn painter(
	cmd: PF_Cmd,
	input: *mut PF_InData,
	_output: *mut PF_OutData,
	params: PF_ParamList,
	world: *mut PF_LayerDef,
	_extra: *mut std::ffi::c_void,
) -> PF_Err {
	if cmd == PF_Cmd_RENDER as PF_Cmd {
		// SAFETY: during a render the host passes its parameters and an 8-bit
		// world of `height` rows, `rowbytes` apart.
		unsafe {
			let input = &*input;
			let mut def: PF_ParamDef = std::mem::zeroed();
			input.inter.checkout_param.unwrap()(
				input.effect_ref,
				SLIDER as _,
				input.current_time,
				input.time_step,
				input.time_scale,
				&mut def,
			);
			let in_array = (**params.add(SLIDER)).u.fs_d.value;
			let pixel = PF_Pixel {
				alpha: 255,
				red: in_array.round() as u8,
				green: def.u.fs_d.value.round() as u8,
				blue: 0,
			};
			let world = &*world;
			for y in 0..world.height as usize {
				let row = world
					.data
					.cast::<u8>()
					.add(y * world.rowbytes as usize)
					.cast::<PF_Pixel>();
				for x in 0..world.width as usize {
					row.add(x).write(pixel);
				}
			}
		}
	}
	PF_Err_NONE
}

#[test]
fn parallel_renders_each_show_their_own_time() {
	let start = std::sync::Arc::new(std::sync::Barrier::new(6));
	let threads: Vec<_> = (1..=6)
		.map(|thread| {
			let start = start.clone();
			std::thread::spawn(move || {
				// SAFETY: `painter` has the effect entry point's signature.
				let mut fx = unsafe { crate::Host::get().entry_loader(painter) }
					.size(64, 32)
					.load()
					.unwrap();
				// SAFETY: a parameter definition is plain old data.
				let mut def: PF_ParamDef = unsafe { std::mem::zeroed() };
				def.param_type = PF_Param_FLOAT_SLIDER as PF_ParamType;
				fx.add_instance_param(def);
				let end = f64::from(thread) * 40.0;
				let track = Track::keyed(vec![
					Keyframe::linear(at(0, 1), ParamValue::Float(0.0)),
					Keyframe::linear(at(2, 1), ParamValue::Float(end)),
				]);
				fx.set_param_track(SLIDER, track.unwrap()).unwrap();
				start.wait();
				(0..90)
					.map(|step| {
						let frame = step * 37 % 80 - 10;
						fx.set_time(Time::frame(frame, 30));
						fx.render_with(crate::RenderPath::Legacy).unwrap();
						let mut out = vec![0; 64 * 32 * 4];
						fx.write_output_rgba8(&mut out).unwrap();
						let expected = (end * f64::from(frame.clamp(0, 60)) / 60.0).round() as u8;
						out.chunks(4).all(|pixel| pixel == [expected, expected, 0, 255])
					})
					.collect::<Vec<bool>>()
			})
		})
		.collect();
	for thread in threads {
		assert_eq!(thread.join().unwrap(), [true; 90], "every render showed its own value");
	}
}
