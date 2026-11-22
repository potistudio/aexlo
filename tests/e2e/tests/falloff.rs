//! `Falloff` blurs a layer by a falloff map. Its smart pre-render reads the
//! Matte Layer parameter through the AEGP Stream Suite, frees the effect
//! handle with `AEGP Effect Suite` v5 (no older version will do) and mixes its
//! settings into the frame GUID with `GuidMixInPtr`. Without v5 it gave up on
//! most renders; without `GuidMixInPtr` it crashed.
//!
//! The fixture is unregistered, so it draws grey "FALLOFF" labels over the
//! frame.
//!
//! Parameter indices: [2] Max Radius, [5] Show Falloff (1 = Off).

use std::sync::{Arc, Mutex};

use aexlo::{Depth8, Layer, ObserveLevel, Observer, ParamValue, PluginInstance, RenderPath, UnsupportedEvent};

const MAX_RADIUS: usize = 2;

const W: u32 = 320;
const H: u32 = 180;

/// Collects what the plugin asked for and aexlo did not do.
#[derive(Default)]
struct Gaps(Mutex<Vec<String>>);

impl Observer for Gaps {
	fn unsupported(&self, event: &UnsupportedEvent) {
		self.0
			.lock()
			.unwrap()
			.push(format!("{:?} {} {}", event.kind, event.name, event.detail));
	}
}

/// Render a smart frame; returns the RGBA8 output and the gaps it hit.
fn render(params: &[(usize, ParamValue)]) -> Option<(Vec<u8>, Vec<String>)> {
	let Some(plugin) = test_e2e::fixture("Falloff") else {
		eprintln!("skipping: fixture 'Falloff' not present locally");
		return None;
	};
	let mut instance: PluginInstance = aexlo::Host::get().load(&plugin).expect("failed to load plugin");
	// An opaque black frame: the labels are the only thing drawn on it.
	let input = [0, 0, 0, 255].repeat((W * H) as usize);
	instance.set_input_layer(Layer::<Depth8>::from_rgba8(input, W, H).unwrap());
	instance.set_render_size(W, H);
	for (index, value) in params {
		instance.set_param(*index, value.clone()).unwrap();
	}
	let gaps = Arc::new(Gaps::default());
	instance.set_observer(Some(gaps.clone()), ObserveLevel::Commands);
	instance.render_with(RenderPath::Smart).expect("smart render failed");
	let mut out = vec![0u8; (W * H * 4) as usize];
	instance.write_output_rgba8(&mut out).unwrap();
	let gaps = gaps.0.lock().unwrap().clone();
	Some((out, gaps))
}

/// Grey (label) pixels in the frame.
fn grey_pixels(out: &[u8]) -> usize {
	out.chunks(4)
		.filter(|p| p[0] > 32 && p[0].abs_diff(p[1]) < 8 && p[1].abs_diff(p[2]) < 8)
		.count()
}

/// The smart path renders, keeps the layer opaque and draws over it.
#[test]
fn smart_render_draws() {
	let Some((out, _)) = render(&[]) else { return };
	assert!(out.chunks(4).all(|p| p[3] == 255), "the layer lost its alpha");
	let n = grey_pixels(&out);
	assert!(n > 100, "only {n} label pixels");
}

/// Pre-render reads a stream and frees the effect handle through Effect
/// Suite v5; with a blur radius set, nothing it asks for is missing.
#[test]
fn smart_render_hits_no_gaps() {
	let Some((out, gaps)) = render(&[(MAX_RADIUS, ParamValue::Float(60.0))]) else {
		return;
	};
	assert!(gaps.is_empty(), "unsupported calls: {gaps:?}");
	assert!(grey_pixels(&out) > 100);
}
