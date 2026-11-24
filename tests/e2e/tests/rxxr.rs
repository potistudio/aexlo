//! `RXXR` applies a look from its Preset popup when the popup changes: it
//! registers with `AEGP Utility Suite` v13, wraps the edit in an undo group,
//! and writes each parameter of the look through `AEGP Stream Suite` v11
//! after checking `AEGP Keyframe Suite` v5 for keyframes.
//!
//! Parameter indices: [210] Preset, [255] Enable Glitch, [261] Glitch Amount,
//! [264] Chunk Width.

use aexlo::{Depth8, Layer, ParamValue, PluginInstance};
use aexlo_fixture_gate::needs_fixture;

const PRESET: usize = 210;
const ENABLE_GLITCH: usize = 255;
const GLITCH_AMOUNT: usize = 261;
const CHUNK_WIDTH: usize = 264;

/// The "GLITCH: Wide Slice" choice of the Preset popup.
const GLITCH_WIDE_SLICE: i32 = 17;

const W: u32 = 96;
const H: u32 = 64;

fn load() -> PluginInstance {
	let path = test_e2e::fixture("RXXR");
	let mut instance: PluginInstance = aexlo::Host::get().load(&path).expect("failed to load plugin");
	let gradient = (0..W * H)
		.flat_map(|i| {
			let v = (i % W * 255 / W) as u8;
			[v, v, v, 255]
		})
		.collect();
	instance.set_input_layer(Layer::<Depth8>::from_rgba8(gradient, W, H).unwrap());
	instance.set_render_size(W, H);
	instance
}

#[needs_fixture("RXXR")]
#[test]
fn choosing_a_preset_writes_its_parameters() {
	let mut instance = load();
	assert_eq!(instance.get_param(ENABLE_GLITCH), Some(ParamValue::Checkbox(false)));

	instance
		.set_param(PRESET, ParamValue::Popup(GLITCH_WIDE_SLICE))
		.unwrap();
	instance.user_changed_param(PRESET).expect("the preset was not applied");

	assert_eq!(instance.get_param(ENABLE_GLITCH), Some(ParamValue::Checkbox(true)));
	assert_eq!(instance.get_param(GLITCH_AMOUNT), Some(ParamValue::Fixed(50.0)));
	assert_eq!(instance.get_param(CHUNK_WIDTH), Some(ParamValue::Slider(12)));
	instance.render_frame().expect("render failed");
}

#[needs_fixture("RXXR")]
#[test]
fn every_preset_applies_and_renders() {
	let mut instance = load();
	// RXXR 1.2.0 lists 38 choices, separators included.
	for choice in 1..=38 {
		instance.set_param(PRESET, ParamValue::Popup(choice)).unwrap();
		instance
			.user_changed_param(PRESET)
			.unwrap_or_else(|err| panic!("preset {choice}: {err}"));
		instance
			.render_frame()
			.unwrap_or_else(|err| panic!("preset {choice}: {err}"));
	}
}
