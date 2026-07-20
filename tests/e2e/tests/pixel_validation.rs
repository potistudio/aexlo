//! Pixel-level correctness checks against real plugin fixtures whose behavior
//! is simple and well-known, as opposed to `render_matrix`'s "did it crash"
//! smoke test. Each fixture is looked up by name and the test skips (rather
//! than failing the suite) when it isn't present on this machine -- see
//! `test_e2e::fixture`.

use aexlo::{Depth8, Layer, ParamValue, PluginInstance};

/// Load `input.png` both as a `Layer` (to feed the plugin) and as raw RGBA
/// bytes (to compare the render against).
fn load_sample_input() -> (Layer<Depth8>, Vec<u8>) {
	let img = image::open(test_e2e::sample_input_path())
		.expect("failed to open workspace input.png")
		.to_rgba8();
	let (width, height) = img.dimensions();
	let raw = img.into_raw();
	let layer = Layer::<Depth8>::from_raw(raw.clone(), width, height).expect("failed to wrap input.png as a Layer");
	(layer, raw)
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

/// `nothing.aex` is a no-op effect: its rendered output must be byte-identical
/// to whatever was checked out as input.
#[test]
fn nothing_plugin_is_a_true_passthrough() {
	let Some(plugin_path) = test_e2e::fixture("nothing") else {
		eprintln!("skipping: fixture 'nothing' not present locally");
		return;
	};

	let mut instance = PluginInstance::try_load(&plugin_path).expect("failed to load plugin");
	let (input, expected) = load_sample_input();
	instance.set_input(input);

	let output = render_rgba(&mut instance);

	assert_eq!(
		output, expected,
		"a no-op plugin must reproduce the input frame exactly"
	);
}

/// `FillColor.aex` overwrites the frame with a solid color when enabled. Setting
/// its checkbox, color and opacity params and rendering must produce a frame
/// that is uniformly that exact RGBA value, regardless of the input pixels.
#[test]
fn fill_color_produces_exact_uniform_output() {
	let Some(plugin_path) = test_e2e::fixture("FillColor") else {
		eprintln!("skipping: fixture 'FillColor' not present locally");
		return;
	};

	let mut instance = PluginInstance::try_load(&plugin_path).expect("failed to load plugin");
	let (input, _) = load_sample_input();
	instance.set_input(input);

	// Indices follow FillColor's own param order (1 = enable checkbox, 2 = color,
	// 3 = opacity); the plugin's declared names come through mojibake locally
	// (probably Shift-JIS misread as UTF-8), so we address params by index.
	instance.set_param(1, ParamValue::Checkbox(true)).unwrap();
	instance
		.set_param(
			2,
			ParamValue::Color {
				red: 10,
				green: 200,
				blue: 30,
				alpha: 255,
			},
		)
		.unwrap();
	instance.set_param(3, ParamValue::Fixed(100.0)).unwrap();

	let output = render_rgba(&mut instance);

	let expected_pixel = [10u8, 200, 30, 255];
	assert!(
		output.chunks_exact(4).all(|px| px == expected_pixel),
		"expected every pixel to equal {expected_pixel:?}, first pixel was {:?}",
		&output[0..4]
	);
}
