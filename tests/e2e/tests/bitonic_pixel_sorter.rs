//! End-to-end coverage of `BitonicPixelSorter` (fixture v1.1.1): every sort
//! mode on the CPU and GPU paths, point-parameter defaults, key-source layer
//! parameters, mask-driven Path mode and the GPU-declined CPU fallback.
//!
//! Parameter indices: [2] Mode, [4]/[5] Threshold Min/Max, [7] Trigger Source
//! Layer, [12] Direction, [13] Angle, [14] Centre, [16] Path.
//!
//! Each test skips when the fixture isn't present locally; GPU comparisons
//! skip when the machine has no GPU device.

use aexlo::{AexloError, Depth8, Layer, MaskPath, MaskVertex, ParamValue, PluginInstance};

const MODE: usize = 2;
const THRESHOLD_MIN: usize = 4;
const THRESHOLD_MAX: usize = 5;
const TRIGGER_SOURCE: usize = 7;
const ANGLE: usize = 13;
const CENTRE: usize = 14;
const PATH: usize = 16;

fn load() -> Option<PluginInstance> {
	let Some(path) = test_e2e::fixture("BitonicPixelSorter") else {
		eprintln!("skipping: fixture 'BitonicPixelSorter' not present locally");
		return None;
	};
	Some(aexlo::Host::get().try_load(&path).expect("failed to load plugin"))
}

fn sample_input() -> Layer<Depth8> {
	let img = image::open(test_e2e::sample_input_path())
		.expect("failed to open workspace input.png")
		.to_rgba8();
	let (width, height) = img.dimensions();
	Layer::<Depth8>::from_raw(img.into_raw(), width, height).unwrap()
}

fn rgba(layer: &Layer<Depth8>) -> Vec<u8> {
	let mut out = vec![0u8; (layer.width() * layer.height() * 4) as usize];
	layer.write_rgba_bytes(&mut out).unwrap();
	out
}

fn output(instance: &PluginInstance) -> Vec<u8> {
	let (w, h) = instance.output_size();
	let mut out = vec![0u8; (w * h * 4) as usize];
	instance.write_rendered_pixels(&mut out).unwrap();
	out
}

fn render_cpu(instance: &mut PluginInstance) -> Vec<u8> {
	instance.render_pre().expect("SMART_PRE_RENDER failed");
	instance.render_smart().expect("SMART_RENDER failed");
	output(instance)
}

/// `None` when the machine has no GPU device.
fn render_gpu(instance: &mut PluginInstance) -> Option<Vec<u8>> {
	match instance.render_gpu() {
		Ok(()) => Some(output(instance)),
		Err(AexloError::Unexpected(msg)) if msg.contains("No GPU device available") => {
			eprintln!("skipping GPU comparison: {msg}");
			None
		}
		Err(err) => panic!("GPU render failed: {err:?}"),
	}
}

/// A mode's name, its parameters, and how many pixels CPU and GPU may differ
/// in (`None` when the paths use different algorithms).
type ModeCase = (&'static str, Vec<(usize, ParamValue)>, Option<usize>);

fn changed_pixels(a: &[u8], b: &[u8]) -> usize {
	a.chunks(4).zip(b.chunks(4)).filter(|(p, q)| p != q).count()
}

/// A closed, curved mask around the middle of a 1920x1080 frame. (Open masks
/// make this plugin build write a leftover debug dump file, so tests avoid them.)
fn closed_mask(id: u32) -> MaskPath {
	MaskPath {
		id,
		vertices: vec![
			MaskVertex {
				x: 500.0,
				y: 300.0,
				tan_in_x: -150.0,
				tan_in_y: 200.0,
				tan_out_x: 150.0,
				tan_out_y: -200.0,
			},
			MaskVertex {
				x: 1400.0,
				y: 250.0,
				tan_in_x: -200.0,
				tan_in_y: -100.0,
				tan_out_x: 200.0,
				tan_out_y: 100.0,
			},
			MaskVertex::corner(1500.0, 850.0),
			MaskVertex {
				x: 600.0,
				y: 800.0,
				tan_in_x: 250.0,
				tan_in_y: 150.0,
				tan_out_x: -250.0,
				tan_out_y: -150.0,
			},
		],
		closed: true,
		..Default::default()
	}
}

/// Load, feed the sample input with full thresholds (every pixel sorts, so
/// each mode leaves a large, unambiguous footprint) and apply `params`.
fn configured(params: &[(usize, ParamValue)]) -> Option<PluginInstance> {
	let mut instance = load()?;
	instance.set_input_layer(sample_input());
	instance.set_mask_paths(vec![closed_mask(5)]);
	instance.set_param(THRESHOLD_MIN, ParamValue::Float(0.0)).unwrap();
	instance.set_param(THRESHOLD_MAX, ParamValue::Float(100.0)).unwrap();
	for (index, value) in params {
		instance.set_param(*index, value.clone()).unwrap();
	}
	Some(instance)
}

/// AE declares point defaults as layer percentages; the 50%,50% Centre must
/// reach the plugin as the frame centre in pixels, and follow a resized input.
#[test]
fn centre_default_is_the_input_frame_centre() {
	let Some(mut instance) = load() else { return };
	instance.set_input_layer(sample_input());
	assert_eq!(
		instance.get_param(CENTRE),
		Some(ParamValue::Point { x: 960.0, y: 540.0 })
	);

	instance.set_input_layer(Layer::<Depth8>::black(400, 200));
	assert_eq!(
		instance.get_param(CENTRE),
		Some(ParamValue::Point { x: 200.0, y: 100.0 })
	);

	// An edited point keeps its pixel value across input changes.
	instance
		.set_param(CENTRE, ParamValue::Point { x: 10.0, y: 20.0 })
		.unwrap();
	instance.set_input_layer(sample_input());
	assert_eq!(instance.get_param(CENTRE), Some(ParamValue::Point { x: 10.0, y: 20.0 }));
}

/// Every mode sorts a large share of the frame on both paths. Axis and Path
/// share their algorithm across CPU and GPU, so their outputs must agree;
/// Free Angle/Rotation/Radial/Swirl use a host transform map on the CPU but an
/// analytic domain sort on the GPU, so only their footprint is checked.
#[test]
fn every_mode_sorts_on_cpu_and_gpu() {
	let modes: [ModeCase; 6] = [
		("axis", vec![(MODE, ParamValue::Popup(1))], Some(0)),
		(
			"free angle",
			vec![(MODE, ParamValue::Popup(2)), (ANGLE, ParamValue::Angle(30.0))],
			None,
		),
		("rotation", vec![(MODE, ParamValue::Popup(3))], None),
		("radial", vec![(MODE, ParamValue::Popup(4))], None),
		("swirl", vec![(MODE, ParamValue::Popup(5))], None),
		// Path's GPU map classifies a handful of boundary pixels differently.
		(
			"path",
			vec![(MODE, ParamValue::Popup(6)), (PATH, ParamValue::Path(5))],
			Some(1920 * 1080 / 100),
		),
	];
	let input = rgba(&sample_input());

	for (name, params, max_cpu_gpu_diff) in modes {
		let Some(mut cpu) = configured(&params) else { return };
		let cpu_out = render_cpu(&mut cpu);
		let changed = changed_pixels(&cpu_out, &input);
		assert!(
			changed > input.len() / 4 / 2,
			"{name}: CPU sorted only {changed} pixels"
		);

		let mut gpu = configured(&params).unwrap();
		let Some(gpu_out) = render_gpu(&mut gpu) else { continue };
		let changed = changed_pixels(&gpu_out, &input);
		assert!(
			changed > input.len() / 4 / 2,
			"{name}: GPU sorted only {changed} pixels"
		);
		if let Some(max) = max_cpu_gpu_diff {
			let diff = changed_pixels(&cpu_out, &gpu_out);
			assert!(diff <= max, "{name}: CPU and GPU differ in {diff} pixels");
		}
	}
}

/// Path mode sorts along the selected mask: no mask selected leaves the frame
/// alone, and different masks sort differently.
#[test]
fn path_mode_follows_the_selected_mask() {
	let Some(mut none) = configured(&[(MODE, ParamValue::Popup(6)), (PATH, ParamValue::Path(0))]) else {
		return;
	};
	let input = rgba(&sample_input());
	assert_eq!(changed_pixels(&render_cpu(&mut none), &input), 0);

	let mut first = configured(&[(MODE, ParamValue::Popup(6)), (PATH, ParamValue::Path(5))]).unwrap();
	let first_out = render_cpu(&mut first);

	let mut second = configured(&[(MODE, ParamValue::Popup(6)), (PATH, ParamValue::Path(9))]).unwrap();
	let mut other = closed_mask(9);
	other.vertices.reverse();
	other.vertices[0].x += 300.0;
	second.set_mask_paths(vec![closed_mask(5), other]);
	assert_eq!(second.get_param(PATH), Some(ParamValue::Path(9)));
	let second_out = render_cpu(&mut second);

	assert!(changed_pixels(&first_out, &input) > 0);
	assert!(changed_pixels(&first_out, &second_out) > 0);
}

/// A linked Trigger Source Layer decides which pixels sort: with a horizontal
/// gradient as the trigger and the default 40-60% band, only the middle
/// columns may change -- identically on the CPU and GPU -- and unlinking it
/// restores the effect-source trigger.
#[test]
fn trigger_source_layer_selects_the_sorted_band() {
	let Some(mut instance) = load() else { return };
	let source = sample_input();
	let (w, h) = (source.width(), source.height());
	let input = rgba(&source);
	instance.set_input_layer(source);
	let baseline = render_cpu(&mut instance);

	let mut gradient = Vec::with_capacity((w * h * 4) as usize);
	for _ in 0..h {
		for x in 0..w {
			let v = (x * 255 / (w - 1)) as u8;
			gradient.extend_from_slice(&[v, v, v, 255]);
		}
	}
	let trigger = Layer::<Depth8>::from_raw(gradient, w, h).unwrap();
	instance.set_layer_param(TRIGGER_SOURCE, Some(trigger)).unwrap();
	assert!(instance.layer_param(TRIGGER_SOURCE).is_some());
	let cpu_out = render_cpu(&mut instance);

	let changed_columns: Vec<u32> = (0..w)
		.filter(|&x| {
			(0..h).any(|y| {
				let i = ((y * w + x) * 4) as usize;
				cpu_out[i..i + 4] != input[i..i + 4]
			})
		})
		.collect();
	assert!(!changed_columns.is_empty(), "the gradient band sorted nothing");
	let band = (w * 39 / 100)..=(w * 61 / 100);
	assert!(
		changed_columns.iter().all(|x| band.contains(x)),
		"pixels outside the 40-60% trigger band changed: {:?}..{:?}",
		changed_columns.first(),
		changed_columns.last()
	);

	if let Some(gpu_out) = render_gpu(&mut instance) {
		assert_eq!(
			changed_pixels(&cpu_out, &gpu_out),
			0,
			"GPU ignored or misread the trigger layer"
		);
	}

	instance.clear_layer_param(TRIGGER_SOURCE).unwrap();
	assert_eq!(render_cpu(&mut instance), baseline);
}

/// Setting a layer parameter is validated like `set_param`.
#[test]
fn set_layer_param_rejects_non_layer_params() {
	let Some(mut instance) = load() else { return };
	assert!(matches!(
		instance.clear_layer_param(MODE),
		Err(AexloError::ParamTypeMismatch { index: MODE, .. })
	));
	assert!(matches!(
		instance.clear_layer_param(0),
		Err(AexloError::ParamIndexOutOfBounds { index: 0, .. })
	));
}

/// Axis sorts wider than 4096 px are not GPU-renderable for this plugin; its
/// pre-render says so, and `render_frame` must render the frame on the CPU
/// (not on the GPU, and not via the legacy command) exactly as a CPU render.
#[test]
fn gpu_declined_frame_renders_on_the_cpu() {
	let (w, h) = (5000u32, 32u32);
	let mut pixels = Vec::with_capacity((w * h * 4) as usize);
	for y in 0..h {
		for x in 0..w {
			let v = ((x / 7 + y * 3) % 256) as u8;
			pixels.extend_from_slice(&[v, 255 - v, v / 2, 255]);
		}
	}
	let make = || -> Option<PluginInstance> {
		let mut instance = load()?;
		instance.set_input_layer(Layer::<Depth8>::from_raw(pixels.clone(), w, h).unwrap());
		instance.set_render_size(w, h);
		instance.set_param(THRESHOLD_MIN, ParamValue::Float(0.0)).unwrap();
		instance.set_param(THRESHOLD_MAX, ParamValue::Float(100.0)).unwrap();
		Some(instance)
	};
	let Some(mut cpu) = make() else { return };
	let expected = render_cpu(&mut cpu);
	assert!(changed_pixels(&expected, &pixels) > 0);

	let mut auto = make().unwrap();
	if auto.supports_gpu() {
		match auto.render_gpu() {
			Err(AexloError::GpuRenderDeclined) => {}
			Err(AexloError::Unexpected(msg)) if msg.contains("No GPU device available") => {}
			other => panic!("expected the plugin to decline the GPU render, got {other:?}"),
		}
	}
	auto.render_frame().unwrap();
	assert_eq!(output(&auto), expected);
}
