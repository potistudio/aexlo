use std::path::PathBuf;

/// Loads and renders every plugin fixture under the workspace's shared
/// `fixtures/plugins/` directory, one subprocess per plugin (via the
/// `render_one` helper binary) so a crash in one real, third-party plugin
/// binary can't take the rest of the matrix down with it. Each plugin's
/// output is written to `target/render_test_output/<name>.png` for visual
/// inspection.
#[test]
fn all_fixtures_render_test() {
	let (platform_dir, extension) = if cfg!(target_os = "windows") {
		("windows", "aex")
	} else {
		("macos", "plugin")
	};

	let workspace_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
	let fixtures_dir = workspace_dir.join("fixtures/plugins").join(platform_dir);
	let input_path = workspace_dir.join("input.png");
	let output_dir = workspace_dir.join("target/render_test_output");
	std::fs::create_dir_all(&output_dir).expect("failed to create output dir");

	let render_one = PathBuf::from(env!("CARGO_BIN_EXE_render_one"));

	let mut plugins: Vec<PathBuf> = std::fs::read_dir(&fixtures_dir)
		.unwrap_or_else(|e| panic!("fixtures dir not found at {}: {e}", fixtures_dir.display()))
		.map(|entry| entry.unwrap().path())
		.filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some(extension))
		.collect();
	plugins.sort();
	assert!(
		!plugins.is_empty(),
		"no plugin fixtures found in {}",
		fixtures_dir.display()
	);

	let mut failures = Vec::new();

	for plugin in &plugins {
		let name = plugin.file_stem().unwrap().to_string_lossy().to_string();
		let output_path = output_dir.join(format!("{name}.png"));

		let status = std::process::Command::new(&render_one)
			.arg(plugin)
			.arg(&input_path)
			.arg(&output_path)
			.status()
			.expect("failed to spawn render_one");

		if !status.success() {
			failures.push(format!("{name}: {status}"));
		}
	}

	assert!(
		failures.is_empty(),
		"{}/{} plugins failed to render:\n{}",
		failures.len(),
		plugins.len(),
		failures.join("\n")
	);
}
