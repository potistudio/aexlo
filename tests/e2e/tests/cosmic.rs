//! `Cosmic` is a gradient generator whose smart pre-render mixes its palette
//! table into After Effects' frame-cache GUID through
//! `PF_PreRenderCallbacks::GuidMixInPtr` (84 bytes). It calls the pointer
//! without checking it, so a host that leaves it null crashes the plugin in
//! `PF_Cmd_SMART_PRE_RENDER`.
//!
//! The fixture is unregistered, so it overlays a red X; the checks compare
//! whole frames, and the X is the same in each.
//!
//! Parameter indices: [2] Colors (popup: 1 = Cold … 5 = Deep Space).

use aexlo::{Depth8, Layer, ParamValue, PluginInstance, RenderPath};

const COLORS: usize = 2;

const W: u32 = 320;
const H: u32 = 180;

fn render(path: RenderPath, params: &[(usize, ParamValue)]) -> Option<Vec<u8>> {
	let Some(plugin) = test_e2e::fixture("Cosmic") else {
		eprintln!("skipping: fixture 'Cosmic' not present locally");
		return None;
	};
	let mut instance: PluginInstance = aexlo::Host::get().load(&plugin).expect("failed to load plugin");
	// Opaque grey: the plugin keeps the layer's alpha.
	let input = [128, 128, 128, 255].repeat((W * H) as usize);
	instance.set_input_layer(Layer::<Depth8>::from_rgba8(input, W, H).unwrap());
	instance.set_render_size(W, H);
	for (index, value) in params {
		instance.set_param(*index, value.clone()).unwrap();
		// Picking a palette copies it into Color 1-5 on USER_CHANGED_PARAM.
		instance.user_changed_param(*index).unwrap();
	}
	instance.render_with(path).expect("render failed");
	let mut out = vec![0u8; (W * H * 4) as usize];
	instance.write_output_rgba8(&mut out).unwrap();
	Some(out)
}

fn mean_abs_diff(a: &[u8], b: &[u8]) -> f64 {
	let sum: u64 = a.iter().zip(b).map(|(x, y)| u64::from(x.abs_diff(*y))).sum();
	sum as f64 / a.len() as f64
}

/// The smart path gets through pre-render (where the plugin mixes in its GUID)
/// and draws the same frame as the legacy path.
#[test]
fn smart_render_matches_legacy() {
	let Some(smart) = render(RenderPath::Smart, &[]) else {
		return;
	};
	let Some(legacy) = render(RenderPath::Legacy, &[]) else {
		return;
	};
	let diff = mean_abs_diff(&smart, &legacy);
	assert!(diff < 2.0, "smart and legacy differ by {diff} per channel");
	assert!(smart.chunks(4).any(|p| p[3] == 255 && p[2] > p[0]), "no gradient drawn");
}

/// Picking another palette changes the smart-rendered frame.
#[test]
fn smart_render_follows_palette() {
	let Some(cold) = render(RenderPath::Smart, &[(COLORS, ParamValue::Popup(1))]) else {
		return;
	};
	let Some(deep_space) = render(RenderPath::Smart, &[(COLORS, ParamValue::Popup(5))]) else {
		return;
	};
	let diff = mean_abs_diff(&cold, &deep_space);
	assert!(diff > 5.0, "palettes differ by only {diff} per channel");
}
