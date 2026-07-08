// Renders a single plugin fixture in its own process, so a crash in one
// plugin binary can't take down the whole render-matrix test suite. Invoked
// by `all_fixtures_render_test` in `src/lib.rs`, not meant to be run by hand.
use std::error::Error;
use std::path::PathBuf;

use aexlo::{Depth8, Layer, PluginInstance};

fn main() -> Result<(), Box<dyn Error>> {
	let mut args = std::env::args().skip(1);
	let usage = "usage: render_one <plugin_path> <input_png> <output_png>";

	let plugin_path = PathBuf::from(args.next().ok_or(usage)?);
	let input_path = PathBuf::from(args.next().ok_or(usage)?);
	let output_path = PathBuf::from(args.next().ok_or(usage)?);

	let mut instance = PluginInstance::try_load(&plugin_path)?;
	instance.about()?;

	let img = image::open(&input_path)?.to_rgba8();
	let (width, height) = img.dimensions();
	instance.set_input(Layer::<Depth8>::from_raw(img.into_raw(), width, height)?);

	// Most AE effects are smart-render only; driving them with the legacy
	// PF_Cmd_RENDER makes them fail or emit garbage. Dispatch on what the plugin
	// declared during global setup.
	instance.render_frame()?;

	let (out_width, out_height) = instance.output_size();
	let mut buffer = vec![0u8; (out_width * out_height * 4) as usize];
	instance.write_output_rgba(&mut buffer)?;

	let mut writer = Vec::<u8>::new();
	let mut header = mtpng::Header::new();
	header.set_size(out_width, out_height)?;
	header.set_color(mtpng::ColorType::TruecolorAlpha, 8)?;

	let mut encoder = mtpng::encoder::Encoder::new(&mut writer, &mtpng::encoder::Options::default());
	encoder.write_header(&header)?;
	encoder.write_image_rows(&buffer)?;
	encoder.finish()?;

	std::fs::write(&output_path, writer)?;

	Ok(())
}
