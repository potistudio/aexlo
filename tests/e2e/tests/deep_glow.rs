//! End-to-end coverage of `DeepGlow2`. The fixture is unlicensed, so it draws
//! its diagonal watermark; the checks below stay clear of the diagonals.
//!
//! Parameter indices: [24] Radius.

use aexlo::{Depth8, Layer, ParamValue, PluginInstance};

const RADIUS: usize = 24;

fn load() -> Option<PluginInstance> {
	let Some(path) = test_e2e::fixture("DeepGlow2") else {
		eprintln!("skipping: fixture 'DeepGlow2' not present locally");
		return None;
	};
	Some(aexlo::Host::get().try_load(&path).expect("failed to load plugin"))
}

fn render(instance: &mut PluginInstance) -> Vec<u8> {
	instance.render_frame().expect("render failed");
	let (w, h) = instance.output_size();
	let mut out = vec![0u8; (w * h * 4) as usize];
	instance.write_rendered_pixels(&mut out).unwrap();
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
	Layer::<Depth8>::from_raw(px, w, h).unwrap()
}

/// The frame is progressive: every scanline is processed alike. (With an
/// interlaced field reported, odd rows came out brighter and translucent.)
#[test]
fn renders_every_scanline_alike() {
	let Some(mut instance) = load() else { return };
	let img = image::open(test_e2e::sample_input_path()).unwrap().to_rgba8();
	let (w, h) = img.dimensions();
	instance.set_input_layer(Layer::<Depth8>::from_raw(img.into_raw(), w, h).unwrap());
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
#[test]
fn glow_is_centred_and_symmetric() {
	let (w, h) = (640, 480);
	for (cx, cy) in [(150i64, 60i64), (450, 200)] {
		let Some(mut instance) = load() else { return };
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
