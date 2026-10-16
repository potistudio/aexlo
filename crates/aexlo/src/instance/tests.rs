use super::*;

/// A zeroed `PF_ParamDef` of the given type, bypassing `add_instance_param`'s
/// default-normalisation concerns (all defaults are zero anyway).
fn param_of_type(param_type: ParamType) -> PF_ParamDef {
	let mut def: PF_ParamDef = unsafe { std::mem::zeroed() };
	def.param_type = param_type as PF_ParamType;
	def
}

/// An instance with no plugin loaded: params[0] is the implicit input layer.
fn bare_instance() -> PluginInstance {
	PluginInstance::new(Path::new("<test>"))
}

#[test]
fn param_kind_covers_value_less_params() {
	let mut fx = bare_instance();
	fx.add_instance_param(param_of_type(ParamType::GroupStart));
	let mut button = param_of_type(ParamType::Button);
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
	fx.add_instance_param(param_of_type(ParamType::Point3D));
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
	let mut float_def = param_of_type(ParamType::FloatSlider);
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
		Some(ParamType::FloatSlider as PF_ParamType)
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
	fx.add_instance_param(param_of_type(ParamType::FixSlider));

	fx.set_param(1, ParamValue::Fixed(2.5)).unwrap();

	// The raw union value must be PF_Fixed (16.16), the encoding plugins read.
	let raw = unsafe { fx.param_by_index(1).unwrap().u.fd.value };
	assert_eq!(raw, 2 * 65536 + 32768);
	assert_eq!(fx.get_param(1), Some(ParamValue::Fixed(2.5)));
}
