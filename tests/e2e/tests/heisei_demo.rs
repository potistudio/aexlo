//! End-to-end coverage of `HEISEI_DEMO`, a Windows-only effect that gives its
//! layer the look of analog video tape through `PF_Cmd_RENDER`. These tests
//! render at 8 bpc.
//!
//! Parameter indices: [1] Factor, [2] Blur Radius, ..., [19] Even Blur Radius.
//!
//! Its render first scales the layer down to `(width / Factor) x (height /
//! Factor)` pixels, with `Factor` truncated to an integer, into a
//! `std::vector`. A layer narrower or shorter than `Factor` gives an empty
//! vector, whose null data the plugin then reads: the crash is in its own
//! code, whichever host runs it. `compat`'s `size:1x1` scenario hits this at the
//! default `Factor` of 4. These tests cannot reach the crash, which would take
//! the test process down; they render the layers that `Factor` fits.
//!
//! It declares `PF_OutFlag_DEEP_COLOR_AWARE`, but reads and writes 16 bpc
//! worlds as if their pixels were 8 bpc: the left half of each row comes out
//! garbled and the right half is left untouched. No test covers 16 bpc.

use std::sync::{Arc, Mutex};

use aexlo::{Depth8, Layer, ObserveLevel, Observer, ParamValue, PluginInstance, SuiteCallEvent, UnsupportedEvent};
use aexlo_fixture_gate::needs_fixture;

const FACTOR: usize = 1;

#[derive(Default)]
struct Record {
	calls: Mutex<Vec<String>>,
	unsupported: Mutex<Vec<String>>,
}

impl Observer for Record {
	fn suite_call(&self, event: &SuiteCallEvent) {
		let call = format!("{}::{}", event.suite, event.function);
		self.calls.lock().unwrap().push(call);
	}

	fn unsupported(&self, event: &UnsupportedEvent) {
		let gap = format!("{:?} {} {}", event.kind, event.name, event.detail);
		self.unsupported.lock().unwrap().push(gap);
	}
}

fn render(instance: &mut PluginInstance) -> Vec<u8> {
	instance.render_frame().expect("render failed");
	let (w, h) = instance.output_size();
	let mut out = vec![0u8; (w * h * 4) as usize];
	instance.write_output_rgba8(&mut out).unwrap();
	out
}

/// The sample frame renders opaque and changed. On the thread that dispatches
/// it, the render calls nothing on the host (loading only registers parameters
/// through `add_param`); `ObserveLevel::Calls` does not see iterate worker
/// threads. The plugin asks for nothing aexlo does not serve.
#[needs_fixture("HEISEI_DEMO")]
#[test]
fn renders_the_sample_frame() {
	let record = Arc::new(Record::default());
	let mut instance = aexlo::Host::get()
		.loader(test_e2e::fixture("HEISEI_DEMO"))
		.observer(record.clone(), ObserveLevel::Calls)
		.load()
		.expect("failed to load plugin");
	record.calls.lock().unwrap().clear();
	let img = image::open(test_e2e::sample_input_path()).unwrap().to_rgba8();
	let (w, h) = img.dimensions();
	let input = img.into_raw();
	instance.set_input_layer(Layer::<Depth8>::from_rgba8(input.clone(), w, h).unwrap());
	instance.set_render_size(w, h);
	let out = render(&mut instance);

	assert!(out.chunks(4).all(|px| px[3] == 255), "the output is not opaque");
	let changed = out.iter().zip(&input).filter(|(a, b)| a != b).count();
	assert!(changed > out.len() / 2, "only {changed} of {} bytes changed", out.len());
	let calls = record.calls.lock().unwrap();
	assert!(calls.is_empty(), "host calls during the render: {calls:?}");
	let unsupported = record.unsupported.lock().unwrap();
	assert!(unsupported.is_empty(), "unsupported calls: {unsupported:?}");
}

/// A layer no bigger than `Factor` renders once `trunc(Factor)` fits in it,
/// so the crash at the default `Factor` comes from the plugin's own integer
/// division, not from a gap in aexlo.
#[needs_fixture("HEISEI_DEMO")]
#[test]
fn renders_a_tiny_layer_when_factor_fits() {
	for (w, h, factor) in [(1, 1, 1.0), (3, 3, 3.0), (1, 1, 1.9), (7, 7, 7.0)] {
		let mut instance = aexlo::Host::get()
			.load(test_e2e::fixture("HEISEI_DEMO"))
			.expect("failed to load plugin");
		let grey = [128, 128, 128, 255].repeat((w * h) as usize);
		instance.set_input_layer(Layer::<Depth8>::from_rgba8(grey.clone(), w, h).unwrap());
		instance.set_render_size(w, h);
		instance.set_param(FACTOR, ParamValue::Float(factor)).unwrap();
		let out = render(&mut instance);
		assert!(
			out.chunks(4).all(|px| px[3] == 255),
			"{w}x{h} at Factor {factor}: not opaque"
		);
		assert_ne!(out, grey, "{w}x{h} at Factor {factor}: the plugin returned its input");
	}
}
