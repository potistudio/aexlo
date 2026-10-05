//! `Furikake` is a particle generator that walks the AEGP object model before
//! every smart render: the Stream/Keyframe/Effect suites for its parameters,
//! the Layer and Comp suites for the timeline (duration, frame rate, shutter),
//! and `PF_Cmd_ARBITRARY_CALLBACK` for its curve-valued arbitrary parameters,
//! which it dereferences.
//!
//! The fixture is unregistered, so it overlays a red X and "DEMO" labels; the
//! checks below look only at the centre of the frame, between the labels.
//!
//! Parameter indices: [255] Pre Run[Sec].

use aexlo::{Depth8, Layer, ParamValue, PluginInstance};

const PRE_RUN: usize = 255;

const W: u32 = 640;
const H: u32 = 360;

fn render(params: &[(usize, ParamValue)]) -> Option<Vec<u8>> {
	let Some(path) = test_e2e::fixture("Furikake") else {
		eprintln!("skipping: fixture 'Furikake' not present locally");
		return None;
	};
	let mut instance: PluginInstance = aexlo::Host::get().try_load(&path).expect("failed to load plugin");
	instance.set_input_layer(Layer::<Depth8>::from_raw(vec![0; (W * H * 4) as usize], W, H).unwrap());
	instance.set_render_size(W, H);
	for (index, value) in params {
		instance.set_param(*index, value.clone()).unwrap();
	}
	instance.render_frame().expect("render failed");
	let mut out = vec![0u8; (W * H * 4) as usize];
	instance.write_rendered_pixels(&mut out).unwrap();
	Some(out)
}

/// Opaque white pixels (particles) in the centre of the frame.
fn particles_near_centre(out: &[u8]) -> usize {
	let (cx, cy) = (W / 2, H / 2);
	let mut n = 0;
	for y in cy - 40..cy + 40 {
		for x in cx - 40..cx + 40 {
			let i = ((y * W + x) * 4) as usize;
			if out[i..i + 4] == [255, 255, 255, 255] {
				n += 1;
			}
		}
	}
	n
}

/// A generator over a transparent frame; at frame 0 nothing has been emitted.
#[test]
fn frame_zero_is_empty() {
	let Some(out) = render(&[]) else { return };
	assert_eq!(particles_near_centre(&out), 0);
	let transparent = out.chunks(4).filter(|p| p[3] == 0).count();
	assert!(
		transparent > (W * H) as usize * 9 / 10,
		"only {transparent} transparent pixels"
	);
}

/// Pre-running the simulation fills the centre (where the emitter sits) with
/// white particles.
#[test]
fn pre_run_emits_particles() {
	let Some(out) = render(&[(PRE_RUN, ParamValue::Float(2.0))]) else {
		return;
	};
	let n = particles_near_centre(&out);
	assert!(n > 100, "only {n} particle pixels near the centre");
}
