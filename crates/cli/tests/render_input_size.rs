//! A one-shot render without a preset uses the input image dimensions.
mod common;

#[test]
fn preset_and_input_override_set_geometry_before_setup() {
	let plugin = common::misbehave_artifact();
	let dir = common::scratch("preset_initial_geometry");
	let source = dir.join("source.png");
	let override_file = dir.join("override.png");
	image::RgbImage::new(19, 13).save(&source).unwrap();
	image::RgbImage::new(11, 7).save(&override_file).unwrap();
	let manifest = common::manifest(
		&dir,
		&format!(
			"[plugin]\nartifact = {}\n[[preset]]\nname = 'file'\ninput = {{ path = {}, fit = 'none' }}\nsize = [37, 23]\n[[preset]]\nname = 'generated'\ninput = {{ generate = 'gradient' }}\nsize = [37, 23]\n",
			common::toml_path(&plugin),
			common::toml_path(&source)
		),
	);
	for (preset, override_path, expected) in [
		("file", None, "19x13:37x23"),
		("file", Some(&override_file), "11x7:37x23"),
		("generated", None, "37x23:37x23"),
	] {
		let output = dir.join(format!("{preset}-{}.png", expected.replace(':', "-")));
		let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_aexlo"));
		command
			.current_dir(&dir)
			.env("AEXLO_TEST_SETUP_GEOMETRY", expected)
			.args(["render", "--manifest"])
			.arg(&manifest)
			.args(["--preset", preset, "--output"])
			.arg(&output);
		if let Some(path) = override_path {
			command.arg("--input").arg(path);
		}
		let result = command.output().unwrap();
		assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
		assert_eq!(image::image_dimensions(output).unwrap(), (37, 23));
	}
}

#[test]
fn render_without_preset_matches_input_dimensions() {
	let plugin = common::misbehave_artifact();
	let dir = common::scratch("render-input-dimensions");
	let input = dir.join("input.png");
	let output = dir.join("output.png");
	image::RgbaImage::from_pixel(37, 23, image::Rgba([32, 64, 128, 255]))
		.save(&input)
		.unwrap();
	let result = common::aexlo(
		&[
			"render",
			plugin.to_str().unwrap(),
			"--input",
			input.to_str().unwrap(),
			"--output",
			output.to_str().unwrap(),
			"--set",
			"Gain=0",
		],
		&common::workspace(),
	);
	assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
	assert_eq!(image::image_dimensions(&output).unwrap(), (37, 23));
	let pixel = image::open(output).unwrap().to_rgba8().get_pixel(0, 0).0;
	assert_eq!(&pixel[..3], &[0, 0, 0]);
}
