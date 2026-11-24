use super::*;

/// A zeroed `PF_ParamDef` of the given type, bypassing `add_instance_param`'s
/// default-normalisation concerns (all defaults are zero anyway).
fn param_of_type(param_type: PF_ParamType) -> PF_ParamDef {
	let mut def: PF_ParamDef = unsafe { std::mem::zeroed() };
	def.param_type = param_type;
	def
}

/// An instance with no plugin loaded: params[0] is the implicit input layer.
fn bare_instance() -> PluginInstance {
	PluginInstance::new(Path::new("<test>"))
}

#[test]
fn param_kind_covers_value_less_params() {
	let mut fx = bare_instance();
	fx.add_instance_param(param_of_type(crate::sys::PF_Param_GROUP_START as PF_ParamType));
	let mut button = param_of_type(crate::sys::PF_Param_BUTTON as PF_ParamType);
	button.ui_flags = crate::sys::PF_PUI_INVISIBLE as crate::sys::PF_ParamUIFlags;
	fx.add_instance_param(button);

	assert_eq!(fx.param_kind(0), Some(crate::ParamKind::Layer));
	assert_eq!(fx.param_kind(1), Some(crate::ParamKind::GroupStart));
	assert_eq!(fx.param_kind(2), Some(crate::ParamKind::Button));
	assert_eq!(fx.param_kind(3), None);
	assert!(!fx.param_hidden(1));
	assert!(fx.param_hidden(2));
}

#[test]
fn param_flags_read_what_the_plugin_declared() {
	let mut fx = bare_instance();
	let mut popup = param_of_type(crate::sys::PF_Param_POPUP as PF_ParamType);
	popup.flags =
		(crate::sys::PF_ParamFlag_SUPERVISE | crate::sys::PF_ParamFlag_CANNOT_TIME_VARY) as crate::sys::PF_ParamFlags;
	fx.add_instance_param(popup);

	let flags = fx.param_flags(1).unwrap();
	assert!(flags.contains(crate::ParamFlags::SUPERVISE | crate::ParamFlags::CANNOT_TIME_VARY));
	assert!(!flags.contains(crate::ParamFlags::CANNOT_INTERP));
	assert_eq!(fx.param_flags(0), Some(crate::ParamFlags::empty()));
	assert_eq!(fx.param_flags(2), None);
}

#[test]
fn point3d_round_trips() {
	let mut fx = bare_instance();
	fx.add_instance_param(param_of_type(crate::sys::PF_Param_POINT_3D as PF_ParamType));
	let value = ParamValue::Point3D {
		x: 1.5,
		y: -2.0,
		z: 300.25,
	};
	fx.set_param(1, value.clone()).unwrap();
	assert_eq!(fx.get_param(1), Some(value));
	assert!(fx.set_param(1, ParamValue::Point { x: 0.0, y: 0.0 }).is_err());
}

#[test]
fn set_time_reaches_in_data() {
	let mut fx = bare_instance();
	assert_eq!(fx.time(), Time::default());
	fx.set_time(Time::frame(48, 24));
	assert_eq!(fx.time(), Time::frame(48, 24));
}

#[test]
fn param_index_space_is_shared_across_accessors() {
	let mut fx = bare_instance();
	let mut float_def = param_of_type(crate::sys::PF_Param_FLOAT_SLIDER as PF_ParamType);
	float_def.u.fs_d.value = 1.5;
	fx.add_instance_param(float_def);

	// Index 0 is the input layer: unreadable, unsettable.
	assert!(fx.get_param(0).is_none());
	assert!(fx.set_param(0, ParamValue::Float(0.0)).is_err());

	// The first real parameter lives at index 1 in every accessor.
	assert_eq!(fx.get_param(1), Some(ParamValue::Float(1.5)));
	assert_eq!(fx.param_values(), vec![(1, ParamValue::Float(1.5))]);
	assert_eq!(
		fx.param_by_index(1).map(|def| def.param_type),
		Some(crate::sys::PF_Param_FLOAT_SLIDER as PF_ParamType)
	);

	fx.set_param(1, ParamValue::Float(2.0)).unwrap();
	assert_eq!(fx.get_param(1), Some(ParamValue::Float(2.0)));

	// Out of bounds and type mismatches are rejected.
	assert!(fx.set_param(2, ParamValue::Float(0.0)).is_err());
	assert!(fx.set_param(1, ParamValue::Checkbox(true)).is_err());
}

#[test]
fn set_render_size_updates_every_dimension_view() {
	let mut fx = bare_instance();
	fx.set_render_size(640, 360);

	assert_eq!(fx.output_size(), (640, 360));
	assert_eq!((fx.world.width, fx.world.height), (640, 360));
	// The world must point at the freshly sized output layer's pixels.
	assert_eq!(fx.world.data as *const u8, fx.output_layer.layer().data_ptr());
	assert_eq!(fx.world.rowbytes, 640 * 4);
}

#[test]
fn in_data_size_follows_input_not_output() {
	let mut fx = bare_instance();
	let (in_w, in_h) = fx.input_size();
	fx.set_render_size(640, 360);
	assert_eq!(fx.render_context.size(), (in_w as i32, in_h as i32));

	fx.set_input_layer(PluginInstance::build_layer(320, 240, Pixel::<Depth8>::black()));
	assert_eq!(fx.render_context.size(), (320, 240));
	assert_eq!(fx.output_size(), (640, 360));
}

#[test]
fn fixed_slider_uses_16_16_fixed_point() {
	let mut fx = bare_instance();
	fx.add_instance_param(param_of_type(crate::sys::PF_Param_FIX_SLIDER as PF_ParamType));

	fx.set_param(1, ParamValue::Fixed(2.5)).unwrap();

	// The raw union value must be PF_Fixed (16.16), the encoding plugins read.
	let raw = unsafe { fx.param_by_index(1).unwrap().u.fd.value };
	assert_eq!(raw, 2 * 65536 + 32768);
	assert_eq!(fx.get_param(1), Some(ParamValue::Fixed(2.5)));
}

#[test]
fn initial_size_is_visible_during_sequence_setup() {
	use std::sync::atomic::{AtomicI32, Ordering};
	static WIDTH_SEEN: AtomicI32 = AtomicI32::new(0);
	static HEIGHT_SEEN: AtomicI32 = AtomicI32::new(0);
	unsafe extern "C" fn entry(
		cmd: PF_Cmd,
		input: *mut crate::sys::PF_InData,
		_output: *mut crate::sys::PF_OutData,
		_params: crate::sys::PF_ParamList,
		_world: *mut crate::sys::PF_LayerDef,
		_extra: *mut c_void,
	) -> crate::sys::PF_Err {
		if cmd == RawCommand::SequenceSetup.to_sys() {
			WIDTH_SEEN.store(unsafe { (*input).width }, Ordering::SeqCst);
			HEIGHT_SEEN.store(unsafe { (*input).height }, Ordering::SeqCst);
		}
		PF_Err_NONE
	}
	let fx = unsafe { Host::get().entry_loader(entry) }
		.size(768, 432)
		.load()
		.unwrap();
	assert_eq!(WIDTH_SEEN.load(Ordering::SeqCst), 768);
	assert_eq!(HEIGHT_SEEN.load(Ordering::SeqCst), 432);
	assert_eq!(fx.output_size(), (768, 432));
}

#[test]
fn initial_input_and_distinct_output_are_visible_in_all_setup_commands() {
	use std::sync::atomic::{AtomicUsize, Ordering};
	static CHECKED: AtomicUsize = AtomicUsize::new(0);
	unsafe extern "C" fn entry(
		cmd: PF_Cmd,
		input: *mut crate::sys::PF_InData,
		_output: *mut crate::sys::PF_OutData,
		_params: crate::sys::PF_ParamList,
		world: *mut crate::sys::PF_LayerDef,
		_extra: *mut c_void,
	) -> crate::sys::PF_Err {
		if [
			RawCommand::GlobalSetup,
			RawCommand::ParamsSetup,
			RawCommand::SequenceSetup,
		]
		.map(RawCommand::to_sys)
		.contains(&cmd)
		{
			let (iw, ih, ow, oh) = unsafe { ((*input).width, (*input).height, (*world).width, (*world).height) };
			if (iw, ih, ow, oh) == (19, 13, 37, 23) {
				CHECKED.fetch_add(1, Ordering::SeqCst);
			}
		}
		PF_Err_NONE
	}
	CHECKED.store(0, Ordering::SeqCst);
	let input = AnyLayer::filled(PixelDepthKind::U16, 19, 13, OPAQUE_BLACK);
	let fx = unsafe { Host::get().entry_loader(entry) }
		.input(input)
		.output_size(37, 23)
		.load()
		.unwrap();
	assert_eq!(CHECKED.load(Ordering::SeqCst), 3);
	assert_eq!(fx.input_size(), (19, 13));
	assert_eq!(fx.output_size(), (37, 23));
	assert_eq!(fx.input_layer.kind(), PixelDepthKind::U16);
}

/// A click stores its drag state in `continue_refcon`. After Effects hands it
/// back with every drag that follows, and a new click starts from zero.
#[test]
fn custom_ui_drags_get_back_the_continue_refcon_of_their_click() {
	use std::sync::Mutex;
	static SEEN: Mutex<Vec<(crate::sys::PF_EventType, crate::sys::A_intptr_t)>> = Mutex::new(Vec::new());
	unsafe extern "C" fn entry(
		cmd: PF_Cmd,
		_input: *mut crate::sys::PF_InData,
		_output: *mut crate::sys::PF_OutData,
		_params: crate::sys::PF_ParamList,
		_world: *mut crate::sys::PF_LayerDef,
		extra: *mut c_void,
	) -> crate::sys::PF_Err {
		if cmd == RawCommand::Event.to_sys() && !extra.is_null() {
			let extra = unsafe { &mut *(extra.cast::<crate::sys::PF_EventExtra>()) };
			if matches!(extra.e_type, crate::sys::PF_Event_DO_CLICK | crate::sys::PF_Event_DRAG) {
				let click = unsafe { &mut extra.u.do_click };
				SEEN.lock().unwrap().push((extra.e_type, click.continue_refcon[0]));
				click.continue_refcon[0] += 7;
				click.send_drag = 1;
			}
		}
		PF_Err_NONE
	}
	SEEN.lock().unwrap().clear();
	let mut fx = unsafe { Host::get().entry_loader(entry) }.size(64, 64).load().unwrap();
	fx.click_custom_control(1, 4, 4, 32, 16).unwrap();
	fx.drag_custom_control(1, 5, 5, 32, 16, false).unwrap();
	fx.drag_custom_control(1, 6, 6, 32, 16, true).unwrap();
	fx.click_custom_control(1, 4, 4, 32, 16).unwrap();

	let (click, drag) = (crate::sys::PF_Event_DO_CLICK, crate::sys::PF_Event_DRAG);
	assert_eq!(*SEEN.lock().unwrap(), [(click, 0), (drag, 7), (drag, 14), (click, 0)]);
}

#[test]
fn custom_ui_click_sees_the_context_from_new_context() {
	use std::sync::atomic::{AtomicU32, Ordering};
	static CLICK: AtomicU32 = AtomicU32::new(0);
	unsafe extern "C" fn entry(
		cmd: PF_Cmd,
		_input: *mut crate::sys::PF_InData,
		_output: *mut crate::sys::PF_OutData,
		_params: crate::sys::PF_ParamList,
		_world: *mut crate::sys::PF_LayerDef,
		extra: *mut c_void,
	) -> crate::sys::PF_Err {
		if cmd == RawCommand::Event.to_sys() && !extra.is_null() {
			let extra = unsafe { &*(extra.cast::<crate::sys::PF_EventExtra>()) };
			let context = unsafe { *extra.contextH };
			if context.is_null() {
				CLICK.store(3, Ordering::SeqCst);
			} else if extra.e_type == crate::sys::PF_Event_NEW_CONTEXT {
				unsafe { (*context).plugin_state[0] = 0xA11 };
			} else if extra.e_type == crate::sys::PF_Event_DO_CLICK {
				let stamp = unsafe { (*context).plugin_state[0] };
				CLICK.store(if stamp == 0xA11 { 1 } else { 2 }, Ordering::SeqCst);
			}
		}
		PF_Err_NONE
	}
	CLICK.store(0, Ordering::SeqCst);
	let mut fx = unsafe { Host::get().entry_loader(entry) }.size(64, 64).load().unwrap();
	fx.click_custom_control(1, 4, 4, 32, 16).unwrap();
	assert_eq!(CLICK.load(Ordering::SeqCst), 1);
}

#[test]
fn an_observer_from_the_loader_sees_setup_memory_and_can_read_it() {
	unsafe extern "C" fn entry(
		cmd: PF_Cmd,
		input: *mut crate::sys::PF_InData,
		output: *mut crate::sys::PF_OutData,
		_params: crate::sys::PF_ParamList,
		_world: *mut crate::sys::PF_LayerDef,
		_extra: *mut c_void,
	) -> crate::sys::PF_Err {
		let utils = unsafe { &*(*input).utils };
		let fill = |size: u64, byte: u8| {
			let handle = unsafe { utils.host_new_handle.unwrap()(size) };
			unsafe { std::ptr::write_bytes(*handle as *mut u8, byte, size as usize) };
			handle
		};
		if cmd == RawCommand::GlobalSetup.to_sys() {
			unsafe { (*output).global_data = fill(8, 0xAB) };
		} else if cmd == RawCommand::SequenceSetup.to_sys() {
			unsafe { (*output).sequence_data = fill(4, 0x5E) };
		} else if cmd == RawCommand::About.to_sys() {
			// A scratch handle that only lives for the command.
			let scratch = fill(16, 0);
			unsafe { utils.host_dispose_handle.unwrap()(scratch) };
		}
		PF_Err_NONE
	}
	let observer: Arc<dyn Observer> = Arc::new(crate::LogObserver);
	let mut fx = unsafe { Host::get().entry_loader(entry) }
		.observer(observer, ObserveLevel::Commands)
		.size(8, 8)
		.load()
		.unwrap();
	fx.about().unwrap();

	let state = fx.state();
	let global = state.global_data.expect("global_data");
	let sequence = state.sequence_data.expect("sequence_data");
	let mut live: Vec<(usize, usize, &str)> = fx
		.allocations()
		.iter()
		.map(|a| (a.address, a.bytes, a.allocated_in))
		.collect();
	live.sort_by_key(|(_, bytes, _)| *bytes);
	assert_eq!(live, [(sequence, 4, "SEQUENCE_SETUP"), (global, 8, "GLOBAL_SETUP")]);
	assert_eq!(fx.read_allocation(global, 0, 64), Some(vec![0xAB; 8]));
	assert_eq!(fx.read_allocation(sequence, 2, 1), Some(vec![0x5E]));
	assert_eq!(fx.read_allocation(sequence, 5, 1), None, "past the end");
	assert_eq!(fx.read_allocation(global + 1, 0, 1), None, "not an allocation");

	// Without an observer, disposals go unseen: nothing is kept.
	fx.set_observer(None, ObserveLevel::Commands);
	assert!(fx.allocations().is_empty());
	assert_eq!(fx.read_allocation(global, 0, 1), None);
}

#[test]
fn arguments_reach_only_an_observer_that_asked_for_them() {
	use std::sync::Mutex;
	#[derive(Default)]
	struct Calls(Mutex<Vec<String>>);
	impl Observer for Calls {
		fn suite_call(&self, event: &crate::SuiteCallEvent) {
			let args: Vec<String> = event.args.iter().map(|(n, v)| format!("{n}={v}")).collect();
			self.0
				.lock()
				.unwrap()
				.push(format!("{}::{}({})", event.suite, event.function, args.join(",")));
		}
	}
	unsafe extern "C" fn entry(
		cmd: PF_Cmd,
		input: *mut crate::sys::PF_InData,
		_output: *mut crate::sys::PF_OutData,
		_params: crate::sys::PF_ParamList,
		_world: *mut crate::sys::PF_LayerDef,
		_extra: *mut c_void,
	) -> crate::sys::PF_Err {
		if cmd == RawCommand::About.to_sys() {
			let utils = unsafe { &*(*input).utils };
			let handle = unsafe { utils.host_new_handle.unwrap()(24) };
			unsafe { utils.host_dispose_handle.unwrap()(handle) };
		}
		PF_Err_NONE
	}
	for (level, expected) in [
		(ObserveLevel::Calls, "PF_HandleSuite1::host_new_handle()"),
		(ObserveLevel::Arguments, "PF_HandleSuite1::host_new_handle(size=24)"),
	] {
		let calls = Arc::new(Calls::default());
		let mut fx = unsafe { Host::get().entry_loader(entry) }
			.observer(calls.clone(), level)
			.size(8, 8)
			.load()
			.unwrap();
		fx.about().unwrap();
		let seen = calls.0.lock().unwrap();
		assert_eq!(seen.first().map(String::as_str), Some(expected), "{level:?}: {seen:?}");
		assert!(seen[1].starts_with("PF_HandleSuite1::host_dispose_handle"), "{seen:?}");
	}
}

#[test]
fn an_observer_hears_about_missing_suites_and_stubs_at_any_level() {
	use std::sync::Mutex;
	#[derive(Default)]
	struct Gaps(Mutex<Vec<String>>);
	impl Observer for Gaps {
		fn unsupported(&self, event: &crate::UnsupportedEvent) {
			self.0
				.lock()
				.unwrap()
				.push(format!("{:?} {} {}", event.kind, event.name, event.detail));
		}
	}
	unsafe extern "C" fn entry(
		cmd: PF_Cmd,
		input: *mut crate::sys::PF_InData,
		_output: *mut crate::sys::PF_OutData,
		_params: crate::sys::PF_ParamList,
		_world: *mut crate::sys::PF_LayerDef,
		_extra: *mut c_void,
	) -> crate::sys::PF_Err {
		if cmd == RawCommand::About.to_sys() {
			let pica = unsafe { &*(*input).pica_basicP };
			let mut suite: *const c_void = std::ptr::null();
			let err = unsafe { pica.AcquireSuite.unwrap()(c"AEGP Nonexistent Suite".as_ptr(), 3, &mut suite) };
			assert_ne!(err, PF_Err_NONE);
			let stub = crate::suites::utility::create_utility_suite()
				.GetFilterInstanceID
				.unwrap();
			let mut id = 0;
			let err = unsafe { stub((*input).effect_ref, &mut id) };
			assert_eq!(
				err,
				crate::sys::PF_Err_INVALID_CALLBACK,
				"a stub does not claim success"
			);
		}
		PF_Err_NONE
	}
	let gaps = Arc::new(Gaps::default());
	let mut fx = unsafe { Host::get().entry_loader(entry) }
		.observer(gaps.clone(), ObserveLevel::Commands)
		.size(8, 8)
		.load()
		.unwrap();
	fx.about().unwrap();
	assert_eq!(
		*gaps.0.lock().unwrap(),
		["MissingSuite AEGP Nonexistent Suite v3", "Stub get_filter_instance_id "]
	);
}

/// After Effects hands every effect the same `SPBasicSuite` for the life of
/// the process. Plugins keep the first `pica_basicP` in a static and use it
/// from later instances (AutoFill2 does), so it must outlive the instance it
/// was handed to.
#[test]
fn every_instance_gets_the_same_pica_suite_and_it_outlives_them() {
	use std::sync::Mutex;
	static SEEN: Mutex<Vec<usize>> = Mutex::new(Vec::new());
	unsafe extern "C" fn entry(
		cmd: PF_Cmd,
		input: *mut crate::sys::PF_InData,
		_output: *mut crate::sys::PF_OutData,
		_params: crate::sys::PF_ParamList,
		_world: *mut crate::sys::PF_LayerDef,
		_extra: *mut c_void,
	) -> crate::sys::PF_Err {
		if cmd == RawCommand::GlobalSetup.to_sys() {
			SEEN.lock().unwrap().push(unsafe { (*input).pica_basicP } as usize);
		}
		PF_Err_NONE
	}
	// Both alive at once, so that the allocator cannot hand the second
	// instance the first one's freed block and make two suites look like one.
	let first = unsafe { Host::get().entry_loader(entry) }.size(8, 8).load().unwrap();
	let second = unsafe { Host::get().entry_loader(entry) }.size(8, 8).load().unwrap();
	drop(first);

	let seen = SEEN.lock().unwrap().clone();
	assert_eq!(seen.len(), 2, "{seen:?}");
	assert_eq!(seen[0], seen[1], "each instance got its own pica suite");
	// The first instance is gone; its pointer still acquires suites.
	let pica = unsafe { &*(seen[0] as *const crate::sys::SPBasicSuite) };
	let mut suite: *const c_void = std::ptr::null();
	let err = unsafe { pica.AcquireSuite.unwrap()(c"PF ANSI Suite".as_ptr(), 1, &mut suite) };
	assert_eq!(err, PF_Err_NONE);
	assert!(!suite.is_null());
	drop(second);
}
