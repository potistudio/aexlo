//! `TextGrad` paints a gradient across each character of a text layer. It
//! asks `AEGP_GetLayerObjectType` whether its layer is a text layer, reads the
//! glyph outlines through the "AEGP Text Layer Suite" and the "PF Path Data
//! Suite" (tangents as absolute points), groups them into characters and fills
//! each with the gradient stored in its arbitrary-data parameter, whose
//! initial value is a copy of the default.
//!
//! The fixture is unregistered, so it overlays a thin X; the checks count
//! pixels and tolerate it.

use aexlo::{AnyLayer, PixelDepthKind, PluginInstance, TextLayer, TextStyle};

const W: u32 = 640;
const H: u32 = 360;

fn load() -> Option<PluginInstance> {
	let Some(path) = test_e2e::fixture("TextGrad") else {
		eprintln!("skipping: fixture 'TextGrad' not present locally");
		return None;
	};
	Some(aexlo::Host::get().load(&path).expect("failed to load plugin"))
}

fn set_input(instance: &mut PluginInstance, layer: AnyLayer) {
	match layer {
		AnyLayer::U8(l) => instance.set_input_layer(l),
		AnyLayer::U16(l) => instance.set_input_layer(l),
		AnyLayer::F32(l) => instance.set_input_layer(l),
	}
}

fn output(instance: &PluginInstance) -> Vec<u8> {
	let mut out = vec![0u8; (W * H * 4) as usize];
	instance.write_output_rgba8(&mut out).unwrap();
	out
}

/// On a text layer, the characters come out filled and coloured, and the
/// rest of the frame stays transparent.
#[test]
fn fills_the_characters_of_a_text_layer() {
	let Some(mut instance) = load() else { return };
	let Ok(text) = TextLayer::layout("TextGrad", &TextStyle::sized(140.0), W, H) else {
		eprintln!("skipping: no system font to set the text in");
		return;
	};
	let input = text.rasterize(PixelDepthKind::U8, W, H, [1.0; 4]);
	let coverage: Vec<f32> = input.to_unit_rgba().iter().map(|p| p[3]).collect();
	set_input(&mut instance, input);
	instance.set_text_layer(Some(text));
	instance.set_render_size(W, H);
	instance.render_frame().expect("render failed");
	let out = output(&instance);

	let pixel = |i: usize| &out[i * 4..i * 4 + 4];
	let inside: Vec<usize> = (0..coverage.len()).filter(|&i| coverage[i] >= 1.0).collect();
	let filled = inside.iter().filter(|&&i| pixel(i)[3] == 255).count();
	assert!(
		filled > inside.len() * 9 / 10,
		"{filled} of {} glyph pixels are opaque",
		inside.len()
	);
	// A gradient, not a flat or grey fill.
	let colourful = inside
		.iter()
		.filter(|&&i| {
			let p = pixel(i);
			p.iter().take(3).max().unwrap() - p.iter().take(3).min().unwrap() > 64
		})
		.count();
	assert!(
		colourful > inside.len() / 2,
		"{colourful} of {} glyph pixels are coloured",
		inside.len()
	);
	let mut hues: Vec<[u8; 3]> = inside
		.iter()
		.map(|&i| [pixel(i)[0], pixel(i)[1], pixel(i)[2]])
		.collect();
	hues.sort_unstable();
	hues.dedup();
	assert!(hues.len() > 100, "only {} distinct colours", hues.len());

	let outside = (0..coverage.len()).filter(|&i| coverage[i] == 0.0).count();
	let transparent = (0..coverage.len())
		.filter(|&i| coverage[i] == 0.0 && pixel(i)[3] == 0)
		.count();
	assert!(
		transparent > outside * 9 / 10,
		"{transparent} of {outside} pixels around the text are transparent"
	);
}

/// On an AV layer there is no text to fill: the plugin passes its input
/// through (under the watermark).
#[test]
fn copies_its_input_on_an_av_layer() {
	let Some(mut instance) = load() else { return };
	let mut input = AnyLayer::filled(PixelDepthKind::U8, W, H, [0.0; 4]);
	input.set_from_unit_rgba((0..W * H).map(|i| [(i % W) as f32 / W as f32, 0.5, 0.25, 1.0]));
	let expected = {
		let mut rgba = vec![0u8; (W * H * 4) as usize];
		input.write_rgba8(&mut rgba).unwrap();
		rgba
	};
	set_input(&mut instance, input);
	instance.set_render_size(W, H);
	instance.render_frame().expect("render failed");
	let out = output(&instance);

	let same = out.chunks(4).zip(expected.chunks(4)).filter(|(a, b)| a == b).count();
	assert!(
		same > (W * H) as usize * 95 / 100,
		"only {same} pixels pass through unchanged"
	);
}
