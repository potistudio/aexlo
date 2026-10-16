//! Goldens (§8): where a variant's reference frame lives, comparing against
//! it, blessing it, and the artifacts written when it does not match.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::frame::{Comparison, Frame};
use crate::preset::{GoldenSpec, Iterate, Tolerance, Variant};

/// Axes a golden may be split by (§8.1).
pub const SPLIT_AXES: &[&str] = &["render", "iterate", "platform", "depth"];

/// This platform's name for `split_by = ["platform"]`.
pub fn platform() -> &'static str {
	if cfg!(target_os = "windows") {
		"windows"
	} else if cfg!(target_os = "macos") {
		"macos"
	} else {
		"linux"
	}
}

/// The id parts `split_by` adds to a golden's name.
fn split_parts(variant: &Variant, spec: &GoldenSpec) -> Vec<String> {
	spec.split_by
		.iter()
		.filter_map(|axis| match axis.as_str() {
			"render" => Some(variant.render.name().to_string()),
			"iterate" => Some(
				match variant.iterate {
					Iterate::Parallel => "parallel",
					Iterate::Serial => "serial",
				}
				.to_string(),
			),
			"platform" => Some(platform().to_string()),
			// Only when the depth axis did not already name it.
			"depth" if !variant.id.golden.contains(&format!(".d{}", variant.depth)) => {
				Some(format!("d{}", variant.depth))
			}
			_ => None,
		})
		.collect()
}

/// Where `variant`'s golden lives: `golden/<golden id>.<png|exr>` next to the
/// manifest, or the preset's `path`, with any `split_by` parts inserted
/// before the extension.
pub fn path(variant: &Variant, spec: &GoldenSpec, manifest_dir: &Path) -> PathBuf {
	let parts = split_parts(variant, spec);
	let ext = Frame::extension(variant.depth_kind());
	match &spec.path {
		Some(custom) => {
			let custom = manifest_dir.join(custom);
			if parts.is_empty() {
				return custom;
			}
			let stem = custom
				.file_stem()
				.map(|s| s.to_string_lossy().into_owned())
				.unwrap_or_default();
			let ext = custom
				.extension()
				.map(|e| e.to_string_lossy().into_owned())
				.unwrap_or_else(|| ext.to_string());
			custom.with_file_name(format!("{stem}.{}.{ext}", parts.join(".")))
		}
		None => {
			let mut name = variant.id.golden.clone();
			for part in parts {
				name.push('.');
				name.push_str(&part);
			}
			manifest_dir.join("golden").join(format!("{name}.{ext}"))
		}
	}
}

/// Where a variant's failure artifacts go (§8.3).
pub fn artifacts_dir(manifest_dir: &Path, name: &str) -> PathBuf {
	manifest_dir.join("target").join("aexlo").join(name)
}

/// Write `actual`, `expected` (when there is one) and their `diff` heatmap
/// into `dir`, returning the directory.
pub fn write_artifacts(dir: &Path, actual: &Frame, expected: Option<&Frame>, tolerance: &Tolerance) -> Result<PathBuf> {
	std::fs::create_dir_all(dir).map_err(|e| Error::harness(format!("creating {}: {e}", dir.display())))?;
	actual.save(&dir.join(format!("actual.{}", Frame::extension(actual.depth()))))?;
	if let Some(expected) = expected {
		expected.save(&dir.join(format!("expected.{}", Frame::extension(expected.depth()))))?;
		actual.diff_heatmap(expected, tolerance).save(&dir.join("diff.png"))?;
	}
	Ok(dir.to_path_buf())
}

/// What became of a golden comparison.
#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
	Matched(Comparison),
	Mismatched(Comparison),
	Missing,
	/// `--bless` wrote it; `true` when it did not exist before.
	Blessed {
		created: bool,
	},
}

/// Compare `frame` with the golden at `path`, blessing it when asked (and
/// when it is not an After Effects reference).
///
/// # Errors
/// A golden that exists but cannot be read or decoded is a harness error
/// (§12: exit 3); so is failing to write a blessed one.
pub fn judge(frame: &Frame, path: &Path, spec: &GoldenSpec, bless: bool) -> Result<(Verdict, Option<Frame>)> {
	let ae = spec.source == crate::preset::GoldenSource::Ae;
	if !path.exists() {
		if bless && !ae {
			frame.save(path)?;
			return Ok((Verdict::Blessed { created: true }, None));
		}
		return Ok((Verdict::Missing, None));
	}
	let expected =
		Frame::load(path).map_err(|e| Error::harness(format!("golden {}: {}", path.display(), e.message)))?;
	let cmp = frame.compare(&expected, &spec.tolerance);
	if cmp.passed {
		return Ok((Verdict::Matched(cmp), Some(expected)));
	}
	if bless && !ae {
		frame.save(path)?;
		return Ok((Verdict::Blessed { created: false }, Some(expected)));
	}
	Ok((Verdict::Mismatched(cmp), Some(expected)))
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::manifest::Manifest;

	fn variants(text: &str) -> Vec<Variant> {
		Manifest::parse(text, Path::new("/p/aexlo.toml"))
			.unwrap()
			.variants()
			.unwrap()
	}

	#[test]
	fn golden_paths_follow_the_golden_id() {
		let vs = variants(
			"[plugin]\nartifact = 'x'\n[[preset]]\nname = 'glow'\ndepth = [8, 32]\nrender = ['smart', 'gpu']\n",
		);
		let spec = GoldenSpec::default();
		let paths: Vec<_> = vs.iter().map(|v| path(v, &spec, Path::new("/p"))).collect();
		assert_eq!(paths[0], Path::new("/p/golden/glow.d8.png"));
		assert_eq!(paths[1], paths[0], "render paths share one reference");
		assert_eq!(paths[2], Path::new("/p/golden/glow.d32.exr"));

		let split = GoldenSpec {
			split_by: vec!["render".into(), "platform".into()],
			..GoldenSpec::default()
		};
		assert_eq!(
			path(&vs[1], &split, Path::new("/p")),
			Path::new(&format!("/p/golden/glow.d8.gpu.{}.png", platform()))
		);
		let custom = GoldenSpec {
			path: Some("golden/custom.png".into()),
			split_by: vec!["render".into()],
			..GoldenSpec::default()
		};
		assert_eq!(
			path(&vs[1], &custom, Path::new("/p")),
			Path::new("/p/golden/custom.gpu.png")
		);
	}

	#[test]
	fn missing_goldens_fail_unless_blessed_and_ae_ones_are_never_overwritten() {
		let dir = std::env::temp_dir().join(format!("aexlo-golden-{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		let a = Frame::new(aexlo::AnyLayer::filled(aexlo::PixelDepthKind::U8, 4, 4, [0.5; 4]));
		let b = Frame::new(aexlo::AnyLayer::filled(aexlo::PixelDepthKind::U8, 4, 4, [0.6; 4]));
		let path = dir.join("g.png");
		let spec = GoldenSpec::default();

		assert_eq!(judge(&a, &path, &spec, false).unwrap().0, Verdict::Missing);
		assert_eq!(
			judge(&a, &path, &spec, true).unwrap().0,
			Verdict::Blessed { created: true }
		);
		assert!(matches!(judge(&a, &path, &spec, false).unwrap().0, Verdict::Matched(_)));
		assert!(matches!(
			judge(&b, &path, &spec, false).unwrap().0,
			Verdict::Mismatched(_)
		));

		let ae = GoldenSpec {
			source: crate::preset::GoldenSource::Ae,
			..GoldenSpec::default()
		};
		assert!(matches!(judge(&b, &path, &ae, true).unwrap().0, Verdict::Mismatched(_)));
		assert_eq!(
			judge(&b, &path, &spec, true).unwrap().0,
			Verdict::Blessed { created: false }
		);

		std::fs::write(&path, b"not a png").unwrap();
		assert_eq!(
			judge(&a, &path, &spec, false).unwrap_err().kind,
			crate::ErrorKind::Harness
		);
		std::fs::remove_dir_all(&dir).unwrap();
	}
}
