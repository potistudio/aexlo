//! A one-shot render without a preset uses the input image dimensions.
mod common;

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
		],
		&common::workspace(),
	);
	assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
	assert_eq!(image::image_dimensions(output).unwrap(), (37, 23));
}
