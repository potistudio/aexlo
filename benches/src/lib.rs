//! Shared harness for aexlo's benchmark platform.
//!
//! The individual bench targets ([`render_matrix`](../render_matrix/index.html)
//! and [`load`](../load/index.html)) are deliberately thin: everything about
//! *which* plugins to drive, *what* resolutions to sweep, and *how* to build an
//! input frame lives here so external plugins can be benchmarked without
//! touching the bench code itself.
//!
//! # Pointing the platform at arbitrary plugins
//!
//! By default the harness benchmarks a small curated set of bundled fixtures.
//! Override the selection at runtime via environment variables:
//!
//! * `AEXLO_BENCH_PLUGINS` -- comma-separated list of plugin names (resolved
//!   against the fixtures directory) or absolute paths to `.plugin`/`.aex`
//!   artifacts. The special value `all` benchmarks every bundled fixture.
//! * `AEXLO_BENCH_RESOLUTIONS` -- comma-separated list of resolution names
//!   (see [`ALL_RESOLUTIONS`]) to restrict the sweep, e.g. `1080p,4k`.
//!
//! ```text
//! AEXLO_BENCH_PLUGINS=/path/to/MyEffect.plugin cargo bench -p aexlo-bench
//! AEXLO_BENCH_PLUGINS=all AEXLO_BENCH_RESOLUTIONS=1080p cargo bench -p aexlo-bench
//! ```

use aexlo::PluginInstance;
use std::path::{Path, PathBuf};

/// A named frame size to sweep the render benchmarks over.
#[derive(Clone, Copy, Debug)]
pub struct Resolution {
	/// Short, stable label used as the criterion parameter (e.g. `1080p`).
	pub name: &'static str,
	pub width: u32,
	pub height: u32,
}

impl Resolution {
	/// Total pixel count -- handed to criterion as throughput so results read as
	/// megapixels/second rather than raw wall-clock, making effects comparable
	/// across resolutions.
	pub fn pixels(&self) -> u64 {
		self.width as u64 * self.height as u64
	}
}

/// The full resolution matrix, ordered small to large. Restrict it at runtime
/// with `AEXLO_BENCH_RESOLUTIONS`.
pub const ALL_RESOLUTIONS: &[Resolution] = &[
	Resolution { name: "512", width: 512, height: 512 },
	Resolution { name: "720p", width: 1280, height: 720 },
	Resolution { name: "1080p", width: 1920, height: 1080 },
	Resolution { name: "4k", width: 3840, height: 2160 },
];

/// The curated default plugin set, used when `AEXLO_BENCH_PLUGINS` is unset:
/// a trivial effect, a noise generator, and a heavy GPU-capable glow, so a bare
/// `cargo bench` still exercises a representative spread.
const DEFAULT_PLUGINS: &[&str] = &["FillColor", "SDK_Noise", "DeepGlow2"];

/// Platform-specific plugin artifact extension.
pub fn plugin_extension() -> &'static str {
	if cfg!(target_os = "windows") { "aex" } else { "plugin" }
}

/// Directory holding the prebuilt plugin fixtures for the current platform.
pub fn fixtures_dir() -> PathBuf {
	let platform_dir = if cfg!(target_os = "windows") { "windows" } else { "macos" };
	PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("../fixtures/plugins")
		.join(platform_dir)
}

/// Resolve a plugin spec to an artifact path.
///
/// A spec is either an existing path (used as-is, so callers can point at
/// external plugins anywhere on disk) or a bare fixture name resolved against
/// [`fixtures_dir`] with the platform extension appended.
pub fn resolve_plugin(spec: &str) -> PathBuf {
	let as_path = Path::new(spec);
	if as_path.exists() {
		return as_path.to_path_buf();
	}
	fixtures_dir().join(format!("{spec}.{}", plugin_extension()))
}

/// The set of plugins to benchmark, as `(label, path)` pairs.
///
/// Honors `AEXLO_BENCH_PLUGINS` (`all`, or a comma list of names/paths);
/// falls back to [`DEFAULT_PLUGINS`]. Entries whose artifact is missing are
/// dropped with a warning so a stray name never aborts the whole run.
pub fn bench_plugins() -> Vec<(String, PathBuf)> {
	let specs: Vec<String> = match std::env::var("AEXLO_BENCH_PLUGINS") {
		Ok(value) if value.trim().eq_ignore_ascii_case("all") => list_all_fixtures(),
		Ok(value) => value
			.split(',')
			.map(|s| s.trim().to_string())
			.filter(|s| !s.is_empty())
			.collect(),
		Err(_) => DEFAULT_PLUGINS.iter().map(|s| s.to_string()).collect(),
	};

	specs
		.into_iter()
		.filter_map(|spec| {
			let path = resolve_plugin(&spec);
			if path.exists() {
				// Label by file stem so `/abs/path/MyFx.plugin` reads as `MyFx`.
				let label = path
					.file_stem()
					.map(|s| s.to_string_lossy().into_owned())
					.unwrap_or(spec);
				Some((label, path))
			} else {
				eprintln!("aexlo-bench: skipping '{spec}' -- no artifact at {}", path.display());
				None
			}
		})
		.collect()
}

/// The resolutions to sweep, honoring `AEXLO_BENCH_RESOLUTIONS`.
pub fn bench_resolutions() -> Vec<Resolution> {
	match std::env::var("AEXLO_BENCH_RESOLUTIONS") {
		Ok(value) => {
			let wanted: Vec<String> = value
				.split(',')
				.map(|s| s.trim().to_ascii_lowercase())
				.filter(|s| !s.is_empty())
				.collect();
			ALL_RESOLUTIONS
				.iter()
				.copied()
				.filter(|r| wanted.iter().any(|w| w == r.name))
				.collect()
		}
		Err(_) => ALL_RESOLUTIONS.to_vec(),
	}
}

/// Every bundled fixture name for the current platform, sorted.
fn list_all_fixtures() -> Vec<String> {
	let extension = plugin_extension();
	let mut names: Vec<String> = std::fs::read_dir(fixtures_dir())
		.into_iter()
		.flatten()
		.flatten()
		.map(|entry| entry.path())
		.filter(|path| path.extension().and_then(|e| e.to_str()) == Some(extension))
		.filter_map(|path| path.file_stem().map(|s| s.to_string_lossy().into_owned()))
		.collect();
	names.sort();
	names
}

/// Human-readable name of parameter `index`, read from the plugin's stored
/// `PF_ParamDef`, falling back to a positional label when the plugin left it
/// blank.
pub fn param_name(instance: &PluginInstance, index: usize) -> String {
	instance
		.param_by_index(index)
		.map(|def| {
			let bytes: Vec<u8> = def.name.iter().take_while(|&&c| c != 0).map(|&c| c as u8).collect();
			String::from_utf8_lossy(&bytes).trim().to_string()
		})
		.filter(|name| !name.is_empty())
		.unwrap_or_else(|| format!("Param {index}"))
}

/// Print the plugin's current parameter settings to stdout so a benchmark
/// number can be tied to the exact configuration that produced it.
///
/// The benches drive plugins with their default parameter values; dumping them
/// once per plugin makes the run self-documenting and reproducible. Prints
/// `name = value` for every non-input parameter (index 0 is the input layer).
pub fn print_params(label: &str, instance: &PluginInstance) {
	let params = instance.param_values();
	println!("aexlo-bench: {label}: parameter settings ({} params)", params.len());
	for (index, value) in params {
		println!("  [{index}] {:<24} = {value:?}", param_name(instance, index));
	}
}

/// Build a deterministic synthetic RGBA8 input frame of `width * height` pixels.
///
/// A cheap diagonal gradient with a non-trivial alpha ramp -- deterministic so
/// runs are comparable, and non-uniform so effects that early-out on flat or
/// fully-opaque input still do real work.
///
/// NOTE: the current public input API ([`PluginInstance::set_input_raw`]) only
/// accepts 8-bit (`Depth8`) pixels, so the platform sweeps *resolution* but not
/// bit depth. Once a 16/32-bit input path exists this is the natural place to
/// add a depth axis.
///
/// [`PluginInstance::set_input_raw`]: aexlo::PluginInstance::set_input_raw
pub fn synthetic_input(width: u32, height: u32) -> Vec<u8> {
	let mut pixels = Vec::with_capacity((width as usize) * (height as usize) * 4);
	for y in 0..height {
		for x in 0..width {
			let r = (x & 0xff) as u8;
			let g = (y & 0xff) as u8;
			let b = ((x + y) & 0xff) as u8;
			let a = (128 + ((x ^ y) & 0x7f)) as u8;
			pixels.extend_from_slice(&[r, g, b, a]);
		}
	}
	pixels
}
