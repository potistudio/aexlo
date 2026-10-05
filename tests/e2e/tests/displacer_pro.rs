//! `DisplacerPro` reads its transform parameters through the AEGP Stream and
//! Keyframe suites (to detect animated transforms) before every smart render;
//! without them the smart render fails and the legacy fallback paints white.
//!
//! The displacement is `(map - 0.5) * 2 * transform`, with the map's luminance
//! in [0, 1]: a white map applies the full transform, black its inverse and
//! mid-gray nothing. Translate X is a percentage of the layer width.
//!
//! Parameter indices: [1] Map Layer, [3] View Map, [15] Translate X.

use aexlo::{Depth8, Layer, ParamValue, PluginInstance};

const MAP_LAYER: usize = 1;
const VIEW_MAP: usize = 3;
const TRANSLATE_X: usize = 15;

const W: u32 = 128;
const H: u32 = 64;

/// An opaque frame whose red channel is `2x` and green channel `4y`, so a
/// pixel's source position can be read back from its color.
fn pattern() -> Vec<u8> {
	let mut px = Vec::with_capacity((W * H * 4) as usize);
	for y in 0..H {
		for x in 0..W {
			px.extend_from_slice(&[(x * 2) as u8, (y * 4) as u8, ((x * 7 + y * 13) % 256) as u8, 255]);
		}
	}
	px
}

fn gray(level: u8) -> Vec<u8> {
	[level, level, level, 255].repeat((W * H) as usize)
}

fn render(map: Option<Vec<u8>>, params: &[(usize, ParamValue)]) -> Option<Vec<u8>> {
	let Some(path) = test_e2e::fixture("DisplacerPro") else {
		eprintln!("skipping: fixture 'DisplacerPro' not present locally");
		return None;
	};
	let mut instance: PluginInstance = aexlo::Host::get().try_load(&path).expect("failed to load plugin");
	instance.set_input_layer(Layer::<Depth8>::from_raw(pattern(), W, H).unwrap());
	instance.set_render_size(W, H);
	if let Some(map) = map {
		let map = Layer::<Depth8>::from_raw(map, W, H).unwrap();
		instance.set_layer_param(MAP_LAYER, Some(map)).unwrap();
	}
	for (index, value) in params {
		instance.set_param(*index, value.clone()).unwrap();
	}
	instance.render_frame().expect("render failed");
	let mut out = vec![0u8; (W * H * 4) as usize];
	instance.write_rendered_pixels(&mut out).unwrap();
	Some(out)
}

fn at(buf: &[u8], x: u32, y: u32) -> [u8; 4] {
	let i = ((y * W + x) * 4) as usize;
	buf[i..i + 4].try_into().unwrap()
}

/// Every interior pixel of `out` samples the input `shift` pixels to its left.
fn assert_shifted(out: &[u8], shift: f32) {
	for y in 0..H {
		for x in 20..W - 20 {
			let [r, g, ..] = at(out, x, y);
			let expected = 2.0 * (x as f32 - shift);
			assert!(
				(r as f32 - expected).abs() <= 2.0,
				"pixel ({x},{y}): red {r}, expected {expected}"
			);
			assert_eq!(g, (y * 4) as u8, "pixel ({x},{y}) moved vertically");
		}
	}
}

/// With the default (zero) transform nothing moves.
#[test]
fn default_parameters_leave_the_image_untouched() {
	let Some(out) = render(Some(gray(255)), &[]) else {
		return;
	};
	assert_eq!(out, pattern());
}

/// A white map applies the full 10% (12.8 px) translation.
#[test]
fn white_map_translates_forward() {
	let Some(out) = render(Some(gray(255)), &[(TRANSLATE_X, ParamValue::Float(10.0))]) else {
		return;
	};
	assert_shifted(&out, 12.8);
}

/// A black map applies the inverse translation.
#[test]
fn black_map_translates_backward() {
	let Some(out) = render(Some(gray(0)), &[(TRANSLATE_X, ParamValue::Float(10.0))]) else {
		return;
	};
	assert_shifted(&out, -12.8);
}

/// "View Map" shows the map instead of the result.
#[test]
fn view_map_shows_the_map() {
	let Some(out) = render(Some(gray(255)), &[(VIEW_MAP, ParamValue::Checkbox(true))]) else {
		return;
	};
	assert!(out.iter().all(|&c| c == 255));
}
