//! Deep-color I/O: plugins that declare 16/32 bpc render at that depth, and
//! the output reads back as a layer of the same depth.

use std::sync::{Arc, Mutex};

use aexlo::{
	CommandEvent, CommandPhase, Depth8, Depth16, Depth32, Layer, ObserveLevel, Observer, PixelDepthKind, SuiteCallEvent,
};

fn sample_input<D: aexlo::PixelDepth>() -> Layer<D> {
	let img = image::open(test_e2e::sample_input_path())
		.expect("failed to open workspace input.png")
		.to_rgba8();
	let (w, h) = img.dimensions();
	Layer::<Depth8>::from_raw(img.into_raw(), w, h).unwrap().convert::<D>()
}

/// Every fixture's declared depths, for the record.
#[test]
fn report_declared_depths() {
	for name in [
		"SDK_Noise",
		"FillColor",
		"DeepGlow2",
		"AnimatedNoise",
		"RainyDay",
		"Furikake",
		"DisplacerPro",
	] {
		let Some(path) = test_e2e::fixture(name) else { continue };
		let Ok(fx) = aexlo::Host::get().try_load(&path) else {
			continue;
		};
		eprintln!(
			"{name}: 16bpc={} 32bpc={} smart={} gpu={}",
			fx.supports_depth(PixelDepthKind::U16),
			fx.supports_depth(PixelDepthKind::F32),
			fx.supports_smart_render(),
			fx.supports_gpu(),
		);
	}
}

/// SDK_Noise declares deep color: a 16 bpc input yields a 16 bpc output that
/// reads back as `Layer<Depth16>` and resembles the 8 bpc render.
#[test]
fn sdk_noise_renders_at_16_bpc() {
	let Some(path) = test_e2e::fixture("SDK_Noise") else {
		eprintln!("skipping: fixture 'SDK_Noise' not present locally");
		return;
	};

	let mut fx8 = aexlo::Host::get().try_load(&path).unwrap();
	fx8.set_input_layer(sample_input::<Depth8>());
	fx8.render_frame().unwrap();
	let out8 = fx8.read_output::<Depth8>().unwrap();

	let mut fx = aexlo::Host::get().try_load(&path).unwrap();
	assert!(fx.supports_depth(PixelDepthKind::U16), "SDK_Noise declares deep color");
	fx.set_input_layer(sample_input::<Depth16>());
	assert_eq!(fx.output_depth(), PixelDepthKind::U16);
	fx.render_frame().unwrap();

	let out16 = fx.read_output::<Depth16>().expect("16 bpc output");
	assert!(
		fx.read_output::<Depth8>().is_err(),
		"reading at the wrong depth is an error"
	);
	assert_eq!((out16.width(), out16.height()), (out8.width(), out8.height()));

	// Every 16 bpc channel stays within AE's 15+1-bit range.
	assert!(
		out16
			.iter()
			.all(|p| p.red <= 32768 && p.green <= 32768 && p.blue <= 32768 && p.alpha <= 32768)
	);

	// Noise is random per depth path, but both renders keep the image's
	// overall brightness.
	let mean = |rgba: Vec<[f32; 4]>| rgba.iter().map(|p| p[0] + p[1] + p[2]).sum::<f32>() / rgba.len() as f32;
	let m8 = mean(aexlo::AnyLayer::new(out8).to_unit_rgba());
	let m16 = mean(aexlo::AnyLayer::new(out16).to_unit_rgba());
	assert!((m8 - m16).abs() < 0.1, "8 bpc mean {m8} vs 16 bpc mean {m16}");
}

/// A 32 bpc input renders on plugins declaring float color.
#[test]
fn float_capable_fixture_renders_at_32_bpc() {
	for name in ["SDK_Noise", "DeepGlow2", "AnimatedNoise"] {
		let Some(path) = test_e2e::fixture(name) else { continue };
		let mut fx = aexlo::Host::get().try_load(&path).unwrap();
		if !fx.supports_depth(PixelDepthKind::F32) {
			continue;
		}
		fx.set_input_layer(sample_input::<Depth32>());
		// The CPU path: the GPU one is float regardless of the project depth.
		if fx.supports_smart_render() {
			fx.render_pre().and_then(|()| fx.render_smart()).unwrap();
		} else {
			fx.render().unwrap();
		}
		let out = fx.read_output::<Depth32>().expect("32 bpc output");
		assert!(out.iter().any(|p| p.alpha > 0.0), "{name}: output should not be empty");
		eprintln!("{name}: rendered at 32 bpc");
		return;
	}
	eprintln!("skipping: no float-capable fixture present locally");
}

#[derive(Default)]
struct Recorder {
	commands: Mutex<Vec<(String, CommandPhase, bool)>>,
	calls: Mutex<usize>,
}

impl Observer for Recorder {
	fn command(&self, event: &CommandEvent) {
		self.commands
			.lock()
			.unwrap()
			.push((event.name.to_string(), event.phase, event.duration.is_some()));
	}

	fn suite_call(&self, _event: &SuiteCallEvent) {
		*self.calls.lock().unwrap() += 1;
	}
}

/// Command timings are observable; suite calls only at `Calls` level.
#[test]
fn observer_sees_command_timings() {
	let Some(path) = test_e2e::fixture("SDK_Noise") else {
		eprintln!("skipping: fixture 'SDK_Noise' not present locally");
		return;
	};
	let mut fx = aexlo::Host::get().try_load(&path).unwrap();
	let recorder = Arc::new(Recorder::default());
	fx.set_observer(Some(recorder.clone()), ObserveLevel::Commands);
	fx.render_frame().unwrap();

	let commands = recorder.commands.lock().unwrap().clone();
	assert!(!commands.is_empty());
	assert!(
		commands
			.iter()
			.any(|(name, phase, timed)| (name == "RENDER" || name == "SMART_RENDER")
				&& *phase == CommandPhase::End
				&& *timed),
		"expected a timed render command, got {commands:?}"
	);
	assert_eq!(*recorder.calls.lock().unwrap(), 0, "no suite calls at Commands level");

	fx.set_observer(Some(recorder.clone()), ObserveLevel::Calls);
	fx.render_frame().unwrap();
	assert!(
		*recorder.calls.lock().unwrap() > 0,
		"suite calls reported at Calls level"
	);
}

/// FillColor at 16 and 32 bpc paints the exact color its 8-bit parameter names.
#[test]
fn fill_color_is_exact_at_every_depth() {
	let Some(path) = test_e2e::fixture("FillColor") else {
		eprintln!("skipping: fixture 'FillColor' not present locally");
		return;
	};
	let color = aexlo::ParamValue::Color {
		red: 10,
		green: 200,
		blue: 30,
		alpha: 255,
	};
	let expected = [10.0 / 255.0, 200.0 / 255.0, 30.0 / 255.0, 1.0];

	for depth in [PixelDepthKind::U16, PixelDepthKind::F32] {
		let mut fx = aexlo::Host::get().try_load(&path).unwrap();
		match depth {
			PixelDepthKind::U16 => fx.set_input_layer(sample_input::<Depth16>()),
			_ => fx.set_input_layer(sample_input::<Depth32>()),
		}
		fx.set_param(1, aexlo::ParamValue::Checkbox(true)).unwrap();
		fx.set_param(2, color.clone()).unwrap();
		fx.set_param(3, aexlo::ParamValue::Fixed(100.0)).unwrap();
		fx.render_frame().unwrap();

		assert_eq!(fx.output_depth(), depth);
		for rgba in fx.output().to_unit_rgba() {
			for (got, want) in rgba.iter().zip(expected) {
				assert!(
					(got - want).abs() < 1.0 / 255.0,
					"{depth}: pixel {rgba:?}, expected {expected:?}"
				);
			}
		}
	}
}
