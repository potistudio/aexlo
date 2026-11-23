//! What the commands share in reading their arguments: turning a `<plugin>`
//! argument into something loadable, and `-p <package>` into a crate.

use std::path::PathBuf;

use anyhow::{Context, Result};

/// Resolve a user-supplied plugin argument to a loadable path.
///
/// The argument is used as-is if it exists on disk; otherwise, when it looks
/// like a bare name (no path separator), the platform's plugin extension is
/// appended so `aexlo render SDK_Noise` finds `SDK_Noise.plugin`.
pub fn resolve_plugin(arg: &str) -> PathBuf {
	let direct = PathBuf::from(arg);
	if direct.exists() {
		return direct;
	}
	if !arg.contains(std::path::MAIN_SEPARATOR) {
		let ext = if cfg!(target_os = "windows") { "aex" } else { "plugin" };
		let candidate = PathBuf::from(format!("{arg}.{ext}"));
		if candidate.exists() {
			return candidate;
		}
	}
	direct
}

/// Resolve a plugin argument to an artifact on disk, building it first when the
/// argument is a crate directory.
pub fn resolve_artifact(plugin_arg: &str) -> Result<PathBuf> {
	let path = resolve_plugin(plugin_arg);
	if path.is_dir() {
		let manifest = path.join("Cargo.toml");
		if manifest.exists() {
			return crate::watch::build_cdylib(&manifest);
		}
	}
	Ok(path)
}

/// A workspace member to build: its `Cargo.toml` and its name, which is
/// passed to cargo as `--package` so a workspace root's `default-members` do
/// not pick another one.
pub struct Package {
	pub manifest: PathBuf,
	pub name: String,
}

/// Resolve `-p <package>` (or, if absent, the crate in the current directory)
/// to its `Cargo.toml`, the same way `cargo build [-p <package>]` would.
pub fn cargo_package(package: Option<&str>) -> Result<Package> {
	// No `.no_deps()`: `root_package()` needs the resolve graph cargo omits
	// without it, and dev-only usage doesn't need this to be instant.
	let metadata = cargo_metadata::MetadataCommand::new()
		.exec()
		.context("running cargo metadata")?;

	let pkg = if let Some(name) = package {
		metadata
			.packages
			.iter()
			.find(|p| p.name.as_str() == name && metadata.workspace_members.contains(&p.id))
			.with_context(|| format!("no package named '{name}' in this workspace"))?
	} else {
		metadata
			.root_package()
			.context("no crate in the current directory - pass -p <package>")?
	};
	Ok(Package {
		manifest: pkg.manifest_path.clone().into_std_path_buf(),
		name: pkg.name.to_string(),
	})
}
