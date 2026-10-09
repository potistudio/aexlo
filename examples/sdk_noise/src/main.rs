//! Load a plugin, provide an RGBA input layer, render one frame, and save PNG.
use aexlo::{Depth8, Host, Layer};
use std::{error::Error, path::PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
	env_logger::init();
	let args: Vec<_> = std::env::args_os().skip(1).collect();
	if args.first().is_some_and(|arg| arg == "--help" || arg == "-h") {
		println!(
			"Usage: cargo run -p sdk_noise -- [plugin path] [input.png] [output.png]\nDefaults: bundled SDK_Noise, repository input.png, target/sdk-noise.png."
		);
		return Ok(());
	}
	if args.len() > 3 {
		return Err("Expected at most three paths; use --help".into());
	}
	let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
	let fixture = if cfg!(target_os = "windows") {
		"fixtures/plugins/windows/SDK_Noise.aex"
	} else {
		"fixtures/plugins/macos/SDK_Noise.plugin"
	};
	let plugin = args.first().map(PathBuf::from).unwrap_or_else(|| root.join(fixture));
	let input = args.get(1).map(PathBuf::from).unwrap_or_else(|| root.join("input.png"));
	let output = args
		.get(2)
		.map(PathBuf::from)
		.unwrap_or_else(|| root.join("target/sdk-noise.png"));

	// 1. Load a compiled plugin through the host.
	println!("Loading {}", plugin.display());
	let mut instance = Host::get().try_load(&plugin)?;
	println!("{}", instance.about()?);

	// 2. Use the actual image dimensions when constructing the input layer.
	let image = image::open(&input)?.to_rgba8();
	let (width, height) = image.dimensions();
	instance.set_render_size(width, height);
	instance.set_input_layer(Layer::<Depth8>::from_raw(image.into_raw(), width, height)?);

	// 3. Select the appropriate render path and copy the result to RGBA bytes.
	instance.render_frame()?;
	let (width, height) = instance.output_size();
	let mut pixels = vec![0; width as usize * height as usize * 4];
	instance.write_rendered_pixels(&mut pixels)?;

	// 4. Save a PNG. Errors propagate to the terminal instead of panicking.
	if let Some(parent) = output.parent().filter(|path| !path.as_os_str().is_empty()) {
		std::fs::create_dir_all(parent)?;
	}
	image::save_buffer_with_format(
		&output,
		&pixels,
		width,
		height,
		image::ColorType::Rgba8,
		image::ImageFormat::Png,
	)?;
	println!("Saved {} ({} × {})", output.display(), width, height);
	Ok(())
}
