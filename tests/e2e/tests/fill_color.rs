//! End-to-end coverage of `FillColor` (F's Plugins), which fills its layer
//! with a color through `PF_Cmd_SMART_RENDER`.
//!
//! Parameter indices: [1] Execute, [2] Color.

use aexlo::{Depth8, Layer, ParamValue, PluginInstance, RenderPath};

const EXECUTE: usize = 1;

fn load() -> Option<PluginInstance> {
	let Some(path) = test_e2e::fixture("FillColor") else {
		eprintln!("skipping: fixture 'FillColor' not present locally");
		return None;
	};
	Some(aexlo::Host::get().load(&path).expect("failed to load plugin"))
}

/// Its pixel helper asks the legacy `new_world` for a `height / 4` x 8
/// lookup table, so a layer under 4 pixels tall gets a world with no pixels.
/// That world's `data` is null, which the plugin checks for; a dangling
/// pointer crashed it.
#[test]
fn fills_a_layer_shorter_than_four_pixels() {
	for (w, h) in [(1, 1), (5, 3)] {
		let Some(mut instance) = load() else { return };
		let grey = Layer::<Depth8>::from_rgba8(vec![128; (w * h * 4) as usize], w, h).unwrap();
		instance.set_input_layer(grey);
		instance.set_render_size(w, h);
		instance.set_param(EXECUTE, ParamValue::Checkbox(true)).unwrap();
		instance
			.render_with(RenderPath::Smart)
			.unwrap_or_else(|e| panic!("{w}x{h}: {e}"));
		let mut out = vec![0u8; (w * h * 4) as usize];
		instance.write_output_rgba8(&mut out).unwrap();
		for px in out.chunks(4) {
			assert_eq!(px[..3], [255, 0, 0], "{w}x{h}: filled with the default red");
		}
	}
}
