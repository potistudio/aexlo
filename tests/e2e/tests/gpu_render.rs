//! Drives `PluginInstance::render_gpu` against a real GPU-capable plugin
//! fixture. `supports_gpu()` reflects a capability the plugin *declared*
//! during global setup, so it's deterministic regardless of hardware; but
//! actually running the GPU path needs a real device (CUDA on Windows/Linux,
//! Metal on macOS), which CI/dev machines may or may not have. When no device
//! is available `render_gpu` fails with a clear "no GPU device" error -- this
//! test treats that specific failure as a skip, not a bug.

use aexlo::{AexloError, Depth8, Layer, ParamValue, PluginInstance};

fn load_sample_input() -> Layer<Depth8> {
	let img = image::open(test_e2e::sample_input_path())
		.expect("failed to open workspace input.png")
		.to_rgba8();
	let (width, height) = img.dimensions();
	Layer::<Depth8>::from_raw(img.into_raw(), width, height).expect("failed to wrap input.png as a Layer")
}

fn read_output_rgba(instance: &PluginInstance) -> Vec<u8> {
	let (width, height) = instance.output_size();
	let mut buffer = vec![0u8; (width * height * 4) as usize];
	instance
		.write_output_rgba(&mut buffer)
		.expect("write_output_rgba failed");
	buffer
}

/// `true` if `err` is the "no GPU device available" error `gpu_device_setup`
/// raises when the machine has no usable CUDA/Metal device -- an environment
/// gap, not a regression.
fn is_missing_device(err: &AexloError) -> bool {
	matches!(err, AexloError::Unexpected(msg) if msg.contains("No GPU device available"))
}

/// `BitonicPixelSorter.aex` declares `PF_OutFlag2_SUPPORTS_GPU_RENDER_F32`.
/// Driving it through `render_gpu` should produce a real, non-degenerate
/// sorted frame -- not a passthrough of the input and not a flat color.
#[test]
fn bitonic_pixel_sorter_gpu_render_produces_real_output() {
	let Some(plugin_path) = test_e2e::fixture("BitonicPixelSorter") else {
		eprintln!("skipping: fixture 'BitonicPixelSorter' not present locally");
		return;
	};

	let mut instance = PluginInstance::try_load(&plugin_path).expect("failed to load plugin");
	assert!(
		instance.supports_gpu(),
		"BitonicPixelSorter is expected to declare GPU render support"
	);

	let input = load_sample_input();
	let mut input_rgba = vec![0u8; (input.width() * input.height() * 4) as usize];
	{
		// Read the raw input bytes back out before handing the layer to the
		// instance, so we can compare the GPU render against them below.
		let px = input.pixels();
		for (dst, src) in input_rgba.chunks_exact_mut(4).zip(px.iter()) {
			dst.copy_from_slice(&[src.red, src.green, src.blue, src.alpha]);
		}
	}
	instance.set_input(input);

	match instance.render_gpu() {
		Ok(()) => {}
		Err(err) if is_missing_device(&err) => {
			eprintln!("skipping: no GPU device available on this machine ({err:?})");
			return;
		}
		Err(err) => panic!("render_gpu failed: {err:?}"),
	}

	let output = read_output_rgba(&instance);
	let first = &output[0..4];
	assert!(
		output.chunks_exact(4).any(|px| px != first),
		"GPU render should not produce a flat color"
	);
	assert_ne!(
		output, input_rgba,
		"GPU render should actually sort pixels, not pass the input through untouched"
	);
}

/// `render_frame` must prefer the GPU path automatically when the plugin
/// supports it, and its output must match calling `render_gpu` directly.
#[test]
fn render_frame_prefers_gpu_path_when_supported() {
	let Some(plugin_path) = test_e2e::fixture("BitonicPixelSorter") else {
		eprintln!("skipping: fixture 'BitonicPixelSorter' not present locally");
		return;
	};

	let mut via_gpu = PluginInstance::try_load(&plugin_path).expect("failed to load plugin");
	via_gpu.set_input(load_sample_input());
	via_gpu.set_param(4, ParamValue::Float(25.0)).unwrap();
	via_gpu.set_param(5, ParamValue::Float(75.0)).unwrap();
	if let Err(err) = via_gpu.render_gpu() {
		if is_missing_device(&err) {
			eprintln!("skipping: no GPU device available on this machine ({err:?})");
			return;
		}
		panic!("render_gpu failed: {err:?}");
	}
	let gpu_output = read_output_rgba(&via_gpu);

	let mut via_render_frame = PluginInstance::try_load(&plugin_path).expect("failed to load plugin");
	via_render_frame.set_input(load_sample_input());
	via_render_frame.set_param(4, ParamValue::Float(25.0)).unwrap();
	via_render_frame.set_param(5, ParamValue::Float(75.0)).unwrap();
	via_render_frame.render_frame().expect("render_frame failed");
	let render_frame_output = read_output_rgba(&via_render_frame);

	assert_eq!(
		gpu_output, render_frame_output,
		"render_frame's automatic GPU path should match an explicit render_gpu call"
	);
}
