//! End-to-end coverage of `AutoFill2` (AutoFill v2.0.4, Plugin Everything),
//! which grows a fill over its layer through `PF_Cmd_SMART_RENDER`. The
//! fixture is unlicensed, so it draws a red cross over its output.
//!
//! In `PF_Cmd_SMART_PRE_RENDER` it acquires `AEGP Render Suite` at wire
//! version 5 (`AEGP_RenderSuite4`) and calls `AEGP_GetCurrentTimestamp`
//! through it without checking that the acquire succeeded. It also caches
//! its simulation between renders.
//!
//! Parameter indices: [28] Radius.

use std::sync::{Arc, Mutex};

use aexlo::{
	Depth8, Layer, ObserveLevel, Observer, ParamValue, PluginInstance, RenderPath, SuiteCallEvent, Time,
	UnsupportedEvent,
};
use aexlo_fixture_gate::needs_fixture;

const RADIUS: usize = 28;
const SIZE: (u32, u32) = (128, 96);

#[derive(Default)]
struct Record {
	/// What each `AEGP_GetCurrentTimestamp` returned.
	stamps: Mutex<Vec<Option<String>>>,
	gaps: Mutex<Vec<String>>,
}

impl Observer for Record {
	fn suite_call(&self, event: &SuiteCallEvent) {
		if (event.suite, event.function) == ("AEGP_RenderSuite", "AEGP_GetCurrentTimestamp") {
			self.stamps.lock().unwrap().push(event.result.clone());
		}
	}

	fn unsupported(&self, event: &UnsupportedEvent) {
		let gap = format!("{:?} {} {}", event.kind, event.name, event.detail);
		self.gaps.lock().unwrap().push(gap);
	}
}

/// An opaque gradient, so that the fill reveals something other than black.
fn gradient() -> Layer<Depth8> {
	gradient_with_blue(128)
}

fn gradient_with_blue(blue: u8) -> Layer<Depth8> {
	let (w, h) = SIZE;
	let mut px = Vec::with_capacity((w * h * 4) as usize);
	for y in 0..h {
		for x in 0..w {
			px.extend([(x * 255 / w) as u8, (y * 255 / h) as u8, blue, 255]);
		}
	}
	Layer::<Depth8>::from_rgba8(px, w, h).unwrap()
}

fn load(record: Arc<Record>) -> PluginInstance {
	let mut instance = aexlo::Host::get()
		.loader(test_e2e::fixture("AutoFill2"))
		.observer(record, ObserveLevel::Arguments)
		.size(SIZE.0, SIZE.1)
		.load()
		.expect("failed to load plugin");
	instance.set_input_layer(gradient());
	instance.set_render_size(SIZE.0, SIZE.1);
	instance
}

fn render_at(instance: &mut PluginInstance, frame: i32) -> Vec<u8> {
	instance.set_time(Time::frame(frame, 30));
	instance.render_with(RenderPath::Smart).expect("smart render failed");
	let mut out = vec![0u8; (SIZE.0 * SIZE.1 * 4) as usize];
	instance.write_output_rgba8(&mut out).unwrap();
	out
}

/// The fill grows over time, the plugin gets a timestamp from every
/// `AEGP_GetCurrentTimestamp` call, and nothing it asks for is reported
/// unsupported.
#[needs_fixture("AutoFill2")]
#[test]
fn grows_a_fill_through_smart_render() {
	let record = Arc::new(Record::default());
	let mut instance = load(record.clone());
	let start = render_at(&mut instance, 0);
	let later = render_at(&mut instance, 45);
	assert_ne!(start, later, "the fill did not grow between frames 0 and 45");
	let stamps = record.stamps.lock().unwrap();
	assert!(!stamps.is_empty(), "the plugin never asked for the timestamp");
	assert!(
		stamps
			.iter()
			.all(|stamp| stamp.as_deref().is_some_and(|s| s.parse::<u32>().is_ok())),
		"a timestamp call was refused: {stamps:?}"
	);
	let gaps = record.gaps.lock().unwrap();
	assert!(gaps.is_empty(), "unsupported calls: {gaps:?}");
}

/// A render after a parameter change matches one from a fresh instance with
/// that value, at a frame where the fill has not covered the layer yet. This
/// shows that the plugin does not reuse its cached simulation.
#[needs_fixture("AutoFill2")]
#[test]
fn re_renders_after_a_parameter_change() {
	let mut reused = load(Arc::default());
	let before = render_at(&mut reused, 15);
	reused.set_param(RADIUS, ParamValue::Float(80.0)).unwrap();
	let after = render_at(&mut reused, 15);

	let mut fresh = load(Arc::default());
	fresh.set_param(RADIUS, ParamValue::Float(80.0)).unwrap();
	let expected = render_at(&mut fresh, 15);

	assert_ne!(before, after, "Radius 80 rendered as the default did");
	assert_eq!(after, expected, "the reused instance rendered a stale frame");
}

/// A render after an input change matches a fresh instance with that input.
#[needs_fixture("AutoFill2")]
#[test]
fn re_renders_after_an_input_change() {
	let mut reused = load(Arc::default());
	let before = render_at(&mut reused, 15);
	reused.set_input_layer(gradient_with_blue(0));
	let after = render_at(&mut reused, 15);

	let mut fresh = load(Arc::default());
	fresh.set_input_layer(gradient_with_blue(0));
	let expected = render_at(&mut fresh, 15);

	assert_ne!(before, after, "the new input rendered as the old one did");
	assert_eq!(after, expected, "the reused instance rendered a stale frame");
}
