use after_effects::{self as ae};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Params {}

#[derive(Debug, Default)]
struct Plugin {}

ae::define_effect!(Plugin, (), Params);

impl AdobePluginGlobal for Plugin {
	fn params_setup(
		&self,
		_params: &mut Parameters<Params>,
		_in_data: InData,
		_out_data: OutData,
	) -> Result<(), Error> {
		Ok(())
	}

	fn handle_command(
		&mut self,
		command: Command,
		_in_data: InData,
		_out_data: OutData,
		_params: &mut Parameters<Params>,
	) -> Result<(), Error> {
		if let Command::Render { out_layer, .. } = command {
			fill_noise(&out_layer);
		}
		Ok(())
	}
}

/// Paint deterministic per-pixel RGB noise (TV-static look) into `layer`.
///
/// Deterministic so the render is reproducible for previews and golden tests:
/// each pixel's color is a hash of its coordinates, no RNG state.
fn fill_noise(layer: &ae::Layer) {
	let (w, h) = (layer.width(), layer.height());
	for y in 0..h {
		for x in 0..w {
			// One hash per pixel; spread its bytes across the channels so the
			// noise is colored rather than grey.
			let n = hash((y as u32).wrapping_shl(24) ^ x as u32);
			let px = layer.as_pixel8_mut(x, y);
			px.red = n as u8;
			px.green = (n >> 8) as u8;
			px.blue = (n >> 16) as u8;
			px.alpha = 255;
		}
	}
}

/// A cheap integer hash (Murmur3-style finalizer) for reproducible noise.
fn hash(mut x: u32) -> u32 {
	x ^= x >> 16;
	x = x.wrapping_mul(0x7feb_352d);
	x ^= x >> 15;
	x = x.wrapping_mul(0x846c_a68b);
	x ^= x >> 16;
	x
}

// In-process preview: drive this plugin through aexlo with no cdylib/bundle/dlopen.
// Unlike the hello_world demo (which draws nothing), this one paints noise, so the
// preview PNG is a real image — the point being to *see* the render result.
#[cfg(test)]
mod preview {
	#[test]
	fn renders_noise_in_process() {
		let mut fx = unsafe { aexlo::PluginInstance::from_entry_raw(crate::EffectMain as *const () as usize) }
			.expect("plugin should load in-process");

		fx.render_frame().expect("render should succeed");

		let (w, h) = fx.output_size();
		assert!(w > 0 && h > 0, "output frame should have a non-zero size");

		// The noise fill must have written a non-uniform frame: sample a few
		// pixels and confirm they are not all identical.
		let mut pixels = vec![0u8; w as usize * h as usize * 4];
		fx.write_output_rgba(&mut pixels)
			.expect("reading output should succeed");
		let first = &pixels[0..4];
		let varied = pixels.chunks_exact(4).any(|px| px != first);
		assert!(varied, "noise render should not be a flat color");

		// Always drop a full-quality PNG next to the crate; open it only on demand.
		let preview = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("preview.png");
		fx.save_preview(&preview).expect("saving preview should succeed");
		eprintln!("preview written to {}", preview.display());

		if std::env::var_os("AEXLO_PREVIEW").is_some() {
			fx.open_preview().expect("opening preview should succeed");
		}
	}
}
