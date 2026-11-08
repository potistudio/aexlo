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
	fx.add_instance_param(param_of_type(after_effects_sys::PF_Param_GROUP_START as PF_ParamType));
	let mut button = param_of_type(after_effects_sys::PF_Param_BUTTON as PF_ParamType);
	button.ui_flags = after_effects_sys::PF_PUI_INVISIBLE as after_effects_sys::PF_ParamUIFlags;
	fx.add_instance_param(button);

	assert_eq!(fx.param_kind(0), Some(crate::ParamKind::Layer));
	assert_eq!(fx.param_kind(1), Some(crate::ParamKind::GroupStart));
	assert_eq!(fx.param_kind(2), Some(crate::ParamKind::Button));
	assert_eq!(fx.param_kind(3), None);
	assert!(!fx.param_hidden(1));
	assert!(fx.param_hidden(2));
}

#[test]
fn point3d_round_trips() {
	let mut fx = bare_instance();
	fx.add_instance_param(param_of_type(after_effects_sys::PF_Param_POINT_3D as PF_ParamType));
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
	let mut float_def = param_of_type(after_effects_sys::PF_Param_FLOAT_SLIDER as PF_ParamType);
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
		Some(after_effects_sys::PF_Param_FLOAT_SLIDER as PF_ParamType)
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
	fx.add_instance_param(param_of_type(after_effects_sys::PF_Param_FIX_SLIDER as PF_ParamType));

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
		input: *mut after_effects_sys::PF_InData,
		_output: *mut after_effects_sys::PF_OutData,
		_params: after_effects_sys::PF_ParamList,
		_world: *mut after_effects_sys::PF_LayerDef,
		_extra: *mut c_void,
	) -> after_effects_sys::PF_Err {
		if cmd == RawCommand::SequenceSetup.to_sdk() {
			WIDTH_SEEN.store(unsafe { (*input).width }, Ordering::SeqCst);
			HEIGHT_SEEN.store(unsafe { (*input).height }, Ordering::SeqCst);
		}
		PF_Err_NONE as after_effects_sys::PF_Err
	}
	let mut fx = PluginInstance::new_sized(Path::new("<test>"), 768, 432);
	fx.entry_point = Some(entry);
	fx.entry_point_name = Some(DEFAULT_ENTRY_POINT_NAME.to_owned());
	fx.finalize().unwrap();
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
		input: *mut after_effects_sys::PF_InData,
		_output: *mut after_effects_sys::PF_OutData,
		_params: after_effects_sys::PF_ParamList,
		world: *mut after_effects_sys::PF_LayerDef,
		_extra: *mut c_void,
	) -> after_effects_sys::PF_Err {
		if [RawCommand::GlobalSetup, RawCommand::ParamsSetup, RawCommand::SequenceSetup]
			.map(RawCommand::to_sdk)
			.contains(&cmd)
		{
			let (iw, ih, ow, oh) = unsafe { ((*input).width, (*input).height, (*world).width, (*world).height) };
			if (iw, ih, ow, oh) == (19, 13, 37, 23) {
				CHECKED.fetch_add(1, Ordering::SeqCst);
			}
		}
		PF_Err_NONE as after_effects_sys::PF_Err
	}
	CHECKED.store(0, Ordering::SeqCst);
	let input = AnyLayer::filled(PixelDepthKind::U16, 19, 13, OPAQUE_BLACK);
	let mut fx = PluginInstance::new_with_initial(Path::new("<test>"), input, (37, 23)).unwrap();
	fx.entry_point = Some(entry);
	fx.entry_point_name = Some(DEFAULT_ENTRY_POINT_NAME.to_owned());
	fx.finalize().unwrap();
	assert_eq!(CHECKED.load(Ordering::SeqCst), 3);
	assert_eq!(fx.input_size(), (19, 13));
	assert_eq!(fx.output_size(), (37, 23));
	assert_eq!(fx.input_layer.kind(), PixelDepthKind::U16);
}

#[test]
fn custom_ui_click_sees_the_context_from_new_context() {
	use std::sync::atomic::{AtomicU32, Ordering};
	static CLICK: AtomicU32 = AtomicU32::new(0);
	unsafe extern "C" fn entry(
		cmd: PF_Cmd,
		_input: *mut after_effects_sys::PF_InData,
		_output: *mut after_effects_sys::PF_OutData,
		_params: after_effects_sys::PF_ParamList,
		_world: *mut after_effects_sys::PF_LayerDef,
		extra: *mut c_void,
	) -> after_effects_sys::PF_Err {
		if cmd == RawCommand::Event.to_sdk() && !extra.is_null() {
			let extra = unsafe { &*(extra.cast::<after_effects_sys::PF_EventExtra>()) };
			let context = unsafe { *extra.contextH };
			if context.is_null() {
				CLICK.store(3, Ordering::SeqCst);
			} else if extra.e_type == after_effects_sys::PF_Event_NEW_CONTEXT {
				unsafe { (*context).plugin_state[0] = 0xA11 };
			} else if extra.e_type == after_effects_sys::PF_Event_DO_CLICK {
				let stamp = unsafe { (*context).plugin_state[0] };
				CLICK.store(if stamp == 0xA11 { 1 } else { 2 }, Ordering::SeqCst);
			}
		}
		PF_Err_NONE as after_effects_sys::PF_Err
	}
	CLICK.store(0, Ordering::SeqCst);
	let mut fx = PluginInstance::new_sized(Path::new("<test>"), 64, 64);
	fx.entry_point = Some(entry);
	fx.entry_point_name = Some(DEFAULT_ENTRY_POINT_NAME.to_owned());
	fx.finalize().unwrap();
	fx.click_custom_control(1, 4, 4, 32, 16).unwrap();
	assert_eq!(CLICK.load(Ordering::SeqCst), 1);
}
