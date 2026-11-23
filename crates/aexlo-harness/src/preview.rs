//! Dev-tooling helpers for `#[aexlo_test::preview]`.
//!
//! Everything here is about *surfacing* a rendered frame during development --
//! where the PNG goes, writing it, and opening it; `aexlo dev` shows the PNGs
//! in [`preview_path`]'s directory in a live window of its own. None of it is
//! part of hosting a plugin, which is why it lives here and not in `aexlo`
//! (§3.2 of `docs/toolkit.md`: the host offers mechanisms, the harness policy).

use std::path::{Path, PathBuf};

use aexlo::PluginInstance;

use crate::error::{Error, Result};

/// Whether a visual preview was requested, i.e. the `AEXLO_PREVIEW` env var is
/// set. Used by `#[aexlo_test::preview]` to decide whether to open the frame in
/// the OS image viewer.
pub fn preview_requested() -> bool {
	std::env::var_os("AEXLO_PREVIEW").is_some()
}

/// Stable on-disk location for a preview PNG:
/// `<manifest_dir>/target/aexlo-preview/<module>_<name>.png`.
///
/// Lives under `target/` (git-ignored by default) so previews don't clutter the
/// source tree. The directory is created if missing. `manifest_dir` should be
/// the caller crate's `env!("CARGO_MANIFEST_DIR")`.
pub fn preview_path(manifest_dir: &str, module_path: &str, name: &str) -> PathBuf {
	let slug: String = module_path
		.chars()
		.map(|c| if c.is_alphanumeric() { c } else { '_' })
		.collect();
	let dir = Path::new(manifest_dir).join("target").join("aexlo-preview");
	let _ = std::fs::create_dir_all(&dir);
	dir.join(format!("{slug}_{name}.png"))
}

/// Launch the OS image viewer on `path` without waiting (spawn, not status), so
/// it never blocks a test or `--watch` cycle.
pub fn open_in_viewer(path: impl AsRef<Path>) -> Result<()> {
	let path = path.as_ref();
	let program = if cfg!(target_os = "macos") {
		"open"
	} else if cfg!(target_os = "windows") {
		"explorer"
	} else {
		"xdg-open"
	};
	std::process::Command::new(program)
		.arg(path)
		.spawn()
		.map_err(|e| Error::harness(format!("launching image viewer ({program}): {e}")))?;
	Ok(())
}

/// Encode `instance`'s current output frame to an 8-bit RGBA PNG at `path`.
///
/// Full fidelity, no terminal/protocol dependence -- the point of a preview
/// is to *see* the render, so this never degrades quality. The frame must be
/// rendered first (call a `render_*` method before this).
///
/// The PNG is written next to `path` and then renamed onto it, so a reader
/// watching `path` (`aexlo dev`'s window) never sees half a file.
pub fn save_preview(instance: &PluginInstance, path: impl AsRef<Path>) -> Result<()> {
	let path = path.as_ref();
	let (w, h) = instance.output_size();

	let mut pixels = vec![0u8; w as usize * h as usize * 4];
	instance.write_output_rgba8(&mut pixels)?;

	let name = path
		.file_name()
		.map(|n| n.to_string_lossy().into_owned())
		.unwrap_or_default();
	let partial = path.with_file_name(format!(".{name}.partial"));
	let written = encode_png(&partial, w, h, &pixels).and_then(|()| {
		std::fs::rename(&partial, path).map_err(|e| Error::harness(format!("writing preview {}: {e}", path.display())))
	});
	if written.is_err() {
		let _ = std::fs::remove_file(&partial);
	}
	written
}

fn encode_png(path: &Path, w: u32, h: u32, pixels: &[u8]) -> Result<()> {
	let file =
		std::fs::File::create(path).map_err(|e| Error::harness(format!("creating preview {}: {e}", path.display())))?;

	let options = mtpng::encoder::Options::new();
	let mut encoder = mtpng::encoder::Encoder::new(file, &options);
	let mut header = mtpng::Header::new();

	header
		.set_size(w, h)
		.and_then(|()| header.set_color(mtpng::ColorType::TruecolorAlpha, 8))
		.and_then(|()| encoder.write_header(&header))
		.and_then(|()| encoder.write_image_rows(pixels))
		.and_then(|()| encoder.finish().map(drop))
		.map_err(|e| Error::harness(format!("encoding preview PNG {}: {e}", path.display())))
}

/// Save `instance`'s current frame to a temp PNG and open it in the OS image
/// viewer, returning the path written.
///
/// The dev-loop one-liner: render, then eyeball the result in a real viewer
/// at full quality. Spawns the viewer without waiting, so it never blocks a
/// test or `--watch` cycle.
pub fn open_preview(instance: &PluginInstance) -> Result<PathBuf> {
	let path = std::env::temp_dir().join(format!("aexlo-preview-{}.png", std::process::id()));
	save_preview(instance, &path)?;
	open_in_viewer(&path)?;

	Ok(path)
}
