//! Where plugins come from: the workspace's prebuilt fixtures, referenced by
//! name, or any plugin on disk, referenced by path.

use std::path::{Path, PathBuf};

const EXTENSION: &str = if cfg!(target_os = "windows") { "aex" } else { "plugin" };

pub fn fixtures_dir() -> PathBuf {
	let platform = if cfg!(target_os = "windows") {
		"windows"
	} else {
		"macos"
	};
	PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("../../fixtures/plugins")
		.join(platform)
}

/// The bundled fixtures, by name.
pub fn list_fixtures() -> Vec<String> {
	let mut names: Vec<String> = std::fs::read_dir(fixtures_dir())
		.into_iter()
		.flatten()
		.flatten()
		.map(|entry| entry.path())
		.filter(|path| path.extension().and_then(|e| e.to_str()) == Some(EXTENSION))
		.filter_map(|path| path.file_stem().map(|s| s.to_string_lossy().into_owned()))
		.collect();
	names.sort();
	names
}

/// A plugin reference as stored in [`crate::model::Effect::plugin`]: a bare
/// fixture name, or a path.
pub fn resolve(plugin: &str) -> PathBuf {
	if plugin.contains('/') || plugin.contains('\\') {
		PathBuf::from(plugin)
	} else {
		fixtures_dir().join(format!("{plugin}.{EXTENSION}"))
	}
}

/// The reference to store for a plugin the user picked from disk: its fixture
/// name if it is one, so projects stay portable.
pub fn reference_for(path: &Path) -> String {
	let is_fixture = path.parent().and_then(|p| p.canonicalize().ok()) == fixtures_dir().canonicalize().ok();
	match path.file_stem() {
		Some(stem) if is_fixture => stem.to_string_lossy().into_owned(),
		_ => path.to_string_lossy().into_owned(),
	}
}

/// Short display name of a plugin reference.
pub fn display_name(plugin: &str) -> String {
	Path::new(plugin)
		.file_stem()
		.map(|s| s.to_string_lossy().into_owned())
		.unwrap_or_else(|| plugin.to_string())
}
