//! Exercises `PluginInstance`'s parameter API (`set_param`/`get_param`/
//! `param_values`/`param_choices`) against real plugin fixtures: storage
//! round-trips, type-mismatch/bounds error paths, and proof that a changed
//! param actually reaches the render (not just instance-side storage).
//!
//! Fixtures are looked up by name and each test skips when its fixture isn't
//! present locally -- see `test_e2e::fixture`.

use aexlo::{AexloError, Depth8, Layer, ParamValue, PluginInstance};

fn load_sample_input() -> Layer<Depth8> {
	let img = image::open(test_e2e::sample_input_path())
		.expect("failed to open workspace input.png")
		.to_rgba8();
	let (width, height) = img.dimensions();
	Layer::<Depth8>::from_raw(img.into_raw(), width, height).expect("failed to wrap input.png as a Layer")
}

fn render_rgba(instance: &mut PluginInstance) -> Vec<u8> {
	instance.render_frame().expect("render_frame failed");
	let (width, height) = instance.output_size();
	let mut buffer = vec![0u8; (width * height * 4) as usize];
	instance
		.write_output_rgba(&mut buffer)
		.expect("write_output_rgba failed");
	buffer
}

/// `BitonicPixelSorter.aex` has a small, well-typed param list (two popups,
/// two float sliders) -- set each to a non-default value and confirm
/// `get_param` reflects exactly what was set.
#[test]
fn bitonic_pixel_sorter_param_set_get_roundtrip() {
	let Some(plugin_path) = test_e2e::fixture("BitonicPixelSorter") else {
		eprintln!("skipping: fixture 'BitonicPixelSorter' not present locally");
		return;
	};
	let mut instance = PluginInstance::try_load(&plugin_path).expect("failed to load plugin");

	// [2] Direction (popup), [3] Order (popup), [4] Threshold Min (float), [5] Threshold Max (float).
	instance.set_param(2, ParamValue::Popup(2)).unwrap();
	instance.set_param(3, ParamValue::Popup(1)).unwrap();
	instance.set_param(4, ParamValue::Float(10.0)).unwrap();
	instance.set_param(5, ParamValue::Float(90.0)).unwrap();

	assert_eq!(instance.get_param(2), Some(ParamValue::Popup(2)));
	assert_eq!(instance.get_param(3), Some(ParamValue::Popup(1)));
	assert_eq!(instance.get_param(4), Some(ParamValue::Float(10.0)));
	assert_eq!(instance.get_param(5), Some(ParamValue::Float(90.0)));
}

/// `set_param` must reject the input-layer index, an out-of-bounds index, and
/// a value whose variant doesn't match the parameter's declared type -- all
/// without touching plugin state.
#[test]
fn bitonic_pixel_sorter_set_param_rejects_invalid_calls() {
	let Some(plugin_path) = test_e2e::fixture("BitonicPixelSorter") else {
		eprintln!("skipping: fixture 'BitonicPixelSorter' not present locally");
		return;
	};
	let mut instance = PluginInstance::try_load(&plugin_path).expect("failed to load plugin");

	assert!(matches!(
		instance.set_param(0, ParamValue::Popup(1)),
		Err(AexloError::ParamIndexOutOfBounds { index: 0, .. })
	));

	let out_of_bounds = instance.param_count();
	assert!(matches!(
		instance.set_param(out_of_bounds, ParamValue::Popup(1)),
		Err(AexloError::ParamIndexOutOfBounds { index, .. }) if index == out_of_bounds
	));

	// Index 2 is a Popup param; feeding it a Float must be rejected as a type mismatch.
	assert!(matches!(
		instance.set_param(2, ParamValue::Float(1.0)),
		Err(AexloError::ParamTypeMismatch { index: 2, .. })
	));
}

/// Changing `Direction` on `BitonicPixelSorter.aex` must change the rendered
/// output -- proof the param actually reaches the render path, not just
/// instance-side storage.
#[test]
fn bitonic_pixel_sorter_direction_param_changes_render_output() {
	let Some(plugin_path) = test_e2e::fixture("BitonicPixelSorter") else {
		eprintln!("skipping: fixture 'BitonicPixelSorter' not present locally");
		return;
	};

	let mut horizontal = PluginInstance::try_load(&plugin_path).expect("failed to load plugin");
	horizontal.set_input(load_sample_input());
	horizontal.set_param(2, ParamValue::Popup(1)).unwrap(); // Horizontal
	let horizontal_output = render_rgba(&mut horizontal);

	let mut vertical = PluginInstance::try_load(&plugin_path).expect("failed to load plugin");
	vertical.set_input(load_sample_input());
	vertical.set_param(2, ParamValue::Popup(2)).unwrap(); // Vertical
	let vertical_output = render_rgba(&mut vertical);

	assert_ne!(
		horizontal_output, vertical_output,
		"Direction=Horizontal and Direction=Vertical should sort pixels differently"
	);
}

/// `DeepGlow2.aex` declares a much larger, more varied param list (float
/// sliders, checkboxes, popups, an angle dial, and a color) -- round-trip one
/// of each through `set_param`/`get_param` to cover types the smaller
/// fixtures above don't exercise.
#[test]
fn deep_glow_covers_remaining_param_types_roundtrip() {
	let Some(plugin_path) = test_e2e::fixture("DeepGlow2") else {
		eprintln!("skipping: fixture 'DeepGlow2' not present locally");
		return;
	};
	let mut instance = PluginInstance::try_load(&plugin_path).expect("failed to load plugin");

	// [24] Radius (float), [40] Mask Invert (checkbox), [50] Iris Sampling Quality (popup),
	// [67] Aspect Angle (angle), [84] Color.
	instance.set_param(24, ParamValue::Float(123.0)).unwrap();
	instance.set_param(40, ParamValue::Checkbox(true)).unwrap();
	instance.set_param(50, ParamValue::Popup(2)).unwrap();
	instance.set_param(67, ParamValue::Angle(45.0)).unwrap();
	instance
		.set_param(
			84,
			ParamValue::Color {
				red: 1,
				green: 2,
				blue: 3,
				alpha: 255,
			},
		)
		.unwrap();

	assert_eq!(instance.get_param(24), Some(ParamValue::Float(123.0)));
	assert_eq!(instance.get_param(40), Some(ParamValue::Checkbox(true)));
	assert_eq!(instance.get_param(50), Some(ParamValue::Popup(2)));
	assert_eq!(instance.get_param(67), Some(ParamValue::Angle(45.0)));
	assert_eq!(
		instance.get_param(84),
		Some(ParamValue::Color {
			red: 1,
			green: 2,
			blue: 3,
			alpha: 255,
		})
	);
}
