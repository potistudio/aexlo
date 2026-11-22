//! End-to-end coverage of `DeepGlow2`. The fixture is unlicensed, so it draws
//! its diagonal watermark; the checks below stay clear of the diagonals.
//!
//! Parameter indices: [24] Radius.

use aexlo::{Depth8, Layer, ParamValue, PluginInstance};
use aexlo_fixture_gate::needs_fixture;

const RADIUS: usize = 24;

fn load() -> PluginInstance {
	let path = test_e2e::fixture("DeepGlow2");
	aexlo::Host::get().load(&path).expect("failed to load plugin")
}

fn render(instance: &mut PluginInstance) -> Vec<u8> {
	instance.render_frame().expect("render failed");
	let (w, h) = instance.output_size();
	let mut out = vec![0u8; (w * h * 4) as usize];
	instance.write_output_rgba8(&mut out).unwrap();
	out
}

/// An opaque black `w` x `h` frame with a 5x5 white square centred on `(cx, cy)`.
fn dot(w: u32, h: u32, cx: u32, cy: u32) -> Layer<Depth8> {
	let mut px = vec![0u8; (w * h * 4) as usize];
	for (i, p) in px.chunks_mut(4).enumerate() {
		let (x, y) = (i as u32 % w, i as u32 / w);
		p[3] = 255;
		if x.abs_diff(cx) <= 2 && y.abs_diff(cy) <= 2 {
			p[..3].fill(255);
		}
	}
	Layer::<Depth8>::from_rgba8(px, w, h).unwrap()
}

/// The frame is progressive: every scanline is processed alike. (With an
/// interlaced field reported, odd rows came out brighter and translucent.)
#[needs_fixture("DeepGlow2")]
#[test]
fn renders_every_scanline_alike() {
	let mut instance = load();
	let img = image::open(test_e2e::sample_input_path()).unwrap().to_rgba8();
	let (w, h) = img.dimensions();
	instance.set_input_layer(Layer::<Depth8>::from_rgba8(img.into_raw(), w, h).unwrap());
	let out = render(&mut instance);

	// Columns 900..1020 at rows 300..340 sit between the watermark diagonals.
	let row_mean = |y: u32, c: usize| -> f64 {
		(900..1020)
			.map(|x| out[((y * w + x) * 4) as usize + c] as f64)
			.sum::<f64>()
			/ 120.0
	};
	for y in 300..340 {
		assert_eq!(row_mean(y, 3), 255.0, "row {y} is not opaque");
		let step = (row_mean(y, 0) - row_mean(y + 1, 0)).abs();
		assert!(step < 6.0, "rows {y}/{} differ by {step}: scanline artifact", y + 1);
	}
}

/// A glow is centred on its source and falls off evenly in every direction,
/// with the source itself kept at full intensity.
#[needs_fixture("DeepGlow2")]
#[test]
fn glow_is_centred_and_symmetric() {
	let (w, h) = (640, 480);
	for (cx, cy) in [(150i64, 60i64), (450, 200)] {
		let mut instance = load();
		instance.set_input_layer(dot(w, h, cx as u32, cy as u32));
		instance.set_render_size(w, h);
		instance.set_param(RADIUS, ParamValue::Float(100.0)).unwrap();
		let out = render(&mut instance);
		let red = |x: i64, y: i64| out[((y * w as i64 + x) * 4) as usize] as i64;

		assert_eq!(red(cx, cy), 255);
		assert!(red(cx + 5, cy) > 50, "no glow around the dot");
		for d in [5, 10, 20] {
			let around = [red(cx + d, cy), red(cx - d, cy), red(cx, cy + d), red(cx, cy - d)];
			let spread = around.iter().max().unwrap() - around.iter().min().unwrap();
			assert!(spread <= 3, "glow at ({cx},{cy}) distance {d} is lopsided: {around:?}");
		}
		assert_eq!(
			red(cx + 150, cy + 150).max(red(cx - 100, cy)),
			0,
			"glow leaked far from the dot"
		);
	}
}

/// The buttons reset and swap parameters through `AEGP_SetStreamValue`, and
/// `PF_Cmd_UPDATE_PARAMS_UI` hides and disables them through
/// `AEGP_SetDynamicStreamFlag`: "Swap Colors" is only on with a two-color tint.
#[needs_fixture("DeepGlow2")]
#[test]
fn buttons_set_parameters_through_streams() {
	const GAMMA_CORRECTION: usize = 60;
	const RESET_GAMMA: usize = 62;
	const TINT_MODE: usize = 82;
	const COLOR: usize = 84;
	const OUTER_COLOR: usize = 85;
	const SWAP_COLORS: usize = 86;
	let mut instance = load();
	assert_eq!(instance.param_button_label(RESET_GAMMA), Some("Reset Gamma"));

	instance.set_param(GAMMA_CORRECTION, ParamValue::Float(30.0)).unwrap();
	instance.user_changed_param(RESET_GAMMA).unwrap();
	assert_eq!(instance.get_param(GAMMA_CORRECTION), Some(ParamValue::Float(100.0)));

	instance.update_params_ui().unwrap();
	assert!(instance.param_disabled(SWAP_COLORS), "tint is off");
	instance.set_param(TINT_MODE, ParamValue::Popup(4)).unwrap();
	instance.user_changed_param(TINT_MODE).unwrap();
	// After Effects sends this whenever it redraws the controls; the plugin
	// does not always ask for it.
	instance.update_params_ui().unwrap();
	assert!(
		!instance.param_disabled(SWAP_COLORS),
		"a duocolor tint has two colors to swap"
	);

	let (color, outer) = (instance.get_param(COLOR), instance.get_param(OUTER_COLOR));
	instance.user_changed_param(SWAP_COLORS).unwrap();
	assert_eq!(instance.get_param(COLOR), outer);
	assert_eq!(instance.get_param(OUTER_COLOR), color);
}
