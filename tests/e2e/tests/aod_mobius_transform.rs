//! `AOD_MobiusTransform` samples its input with `Layer::bit_depth()` inside the
//! iterate callback, which acquires a suite through the Rust after-effects
//! crate's thread-local; it only runs with `set_parallel_iterate(false)`.
//!
//! Parameter indices: [1..=8] a/b/c/d (re, im), [12] Edge.

use aexlo::{Depth8, Layer, ParamValue, PluginInstance};

const B_RE: usize = 3;

/// A 201x101 opaque frame with a distinct color per pixel. The auto scale is
/// `(min(w, h) - 1) / 2 = 50` px, so b = 0.2 shifts by exactly 10 px.
fn pattern() -> (Vec<u8>, u32, u32) {
	let (w, h) = (201u32, 101u32);
	let mut px = Vec::with_capacity((w * h * 4) as usize);
	for y in 0..h {
		for x in 0..w {
			px.extend_from_slice(&[(x % 256) as u8, (y * 2) as u8, ((x * 7 + y * 13) % 256) as u8, 255]);
		}
	}
	(px, w, h)
}

fn render(params: &[(usize, ParamValue)]) -> Option<Vec<u8>> {
	let Some(path) = test_e2e::fixture("AOD_MobiusTransform") else {
		eprintln!("skipping: fixture 'AOD_MobiusTransform' not present locally");
		return None;
	};
	let mut instance: PluginInstance = aexlo::Host::get().try_load(&path).expect("failed to load plugin");
	instance.set_parallel_iterate(false);
	let (px, w, h) = pattern();
	instance.set_input_layer(Layer::<Depth8>::from_raw(px, w, h).unwrap());
	instance.set_render_size(w, h);
	for (index, value) in params {
		instance.set_param(*index, value.clone()).unwrap();
	}
	instance.render_frame().expect("render failed");
	let mut out = vec![0u8; (w * h * 4) as usize];
	instance.write_rendered_pixels(&mut out).unwrap();
	Some(out)
}

/// The default coefficients (a = d = 1, b = c = 0) are the identity map.
#[test]
fn default_coefficients_are_the_identity() {
	let Some(out) = render(&[]) else { return };
	assert_eq!(out, pattern().0);
}

/// f(z) = z + b moves the image by b * scale; with Edge = None the uncovered
/// strip is transparent.
#[test]
fn translation_moves_the_image() {
	let Some(out) = render(&[(B_RE, ParamValue::Float(0.2))]) else {
		return;
	};
	let (input, w, h) = pattern();
	let at = |buf: &[u8], x: u32, y: u32| -> [u8; 4] {
		let i = ((y * w + x) * 4) as usize;
		buf[i..i + 4].try_into().unwrap()
	};
	for y in 0..h {
		for x in 0..w {
			if x >= 10 {
				assert_eq!(at(&out, x, y), at(&input, x - 10, y), "pixel ({x},{y})");
			} else {
				assert_eq!(at(&out, x, y)[3], 0, "pixel ({x},{y}) should be transparent");
			}
		}
	}
}
