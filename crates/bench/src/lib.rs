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
//! * `AEXLO_BENCH_PARAMS` -- parameter sweep, `Name=v1,v2;Other=v3,v4`
//!   (see [`param_sweeps`]). Each combination becomes a benchmark point.
//! * `AEXLO_BENCH_INPUT` -- path to an image to feed as the input frame
//!   (resized to each resolution); defaults to a synthetic gradient.
//! * `AEXLO_DISABLE_GPU` -- from `aexlo`, forces the CPU render path.
//!
//! ```text
//! AEXLO_BENCH_PLUGINS=/path/to/MyEffect.plugin cargo bench -p aexlo-bench
//! AEXLO_BENCH_PLUGINS=all AEXLO_BENCH_RESOLUTIONS=1080p cargo bench -p aexlo-bench
//! AEXLO_BENCH_PLUGINS=DeepGlow2 AEXLO_BENCH_PARAMS="Radius=100,500,1000" cargo bench -p aexlo-bench
//! ```

pub mod report;

use aexlo::{Depth8, Layer, ParamValue, PluginInstance, Result};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

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
	Resolution {
		name: "512",
		width: 512,
		height: 512,
	},
	Resolution {
		name: "720p",
		width: 1280,
		height: 720,
	},
	Resolution {
		name: "1080p",
		width: 1920,
		height: 1080,
	},
	Resolution {
		name: "4k",
		width: 3840,
		height: 2160,
	},
];

/// Parse a resolution spec: either a name from [`ALL_RESOLUTIONS`] (matched
/// case-insensitively, e.g. `1080p`) or an explicit `<width>x<height>`.
///
/// Explicit sizes need a `&'static str` label, so the spec is leaked. Callers
/// are CLI/env driven, where the number of distinct specs is bounded by the
/// command line, making the leak a one-off per run.
pub fn parse_resolution(spec: &str) -> std::result::Result<Resolution, String> {
	let spec = spec.trim();
	if let Some(found) = ALL_RESOLUTIONS.iter().find(|r| r.name.eq_ignore_ascii_case(spec)) {
		return Ok(*found);
	}

	let (width, height) = spec
		.split_once(['x', 'X'])
		.ok_or_else(|| format!("unknown resolution '{spec}' (try a name like 1080p, or 1920x1080)"))?;
	let width: u32 = width.trim().parse().map_err(|_| format!("invalid width in '{spec}'"))?;
	let height: u32 = height
		.trim()
		.parse()
		.map_err(|_| format!("invalid height in '{spec}'"))?;
	if width == 0 || height == 0 {
		return Err(format!("resolution '{spec}' must be non-zero in both axes"));
	}
	Ok(Resolution {
		name: Box::leak(format!("{width}x{height}").into_boxed_str()),
		width,
		height,
	})
}

/// The curated default plugin set, used when `AEXLO_BENCH_PLUGINS` is unset:
/// a trivial effect, a noise generator, a heavy GPU-capable glow, and a CUDA
/// pixel sorter, so a bare `cargo bench` still exercises a representative spread.
const DEFAULT_PLUGINS: &[&str] = &["FillColor", "SDK_Noise", "DeepGlow2", "BitonicPixelSorter"];

/// Platform-specific plugin artifact extension.
pub fn plugin_extension() -> &'static str {
	if cfg!(target_os = "windows") { "aex" } else { "plugin" }
}

/// Directory holding the prebuilt plugin fixtures for the current platform.
pub fn fixtures_dir() -> PathBuf {
	let platform_dir = if cfg!(target_os = "windows") {
		"windows"
	} else {
		"macos"
	};
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

/// Human-readable name of parameter `index`, falling back to a positional
/// label when the plugin left it blank.
pub fn param_name(instance: &PluginInstance, index: usize) -> String {
	instance
		.param_name(index)
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

/// The input frame for a `width x height` render.
///
/// If `AEXLO_BENCH_INPUT` names a readable image, it is loaded and resized to
/// the requested size (so the resolution sweep still applies to real footage);
/// otherwise falls back to [`synthetic_input`]. A load/decode failure warns and
/// falls back rather than aborting the run.
pub fn bench_input(width: u32, height: u32) -> Vec<u8> {
	let configured = std::env::var_os("AEXLO_BENCH_INPUT");
	input_frame(configured.as_deref().map(Path::new), width, height)
}

/// The input frame for a `width x height` render, from an explicit path.
///
/// `None` (or an image that fails to load) yields [`synthetic_input`]; a real
/// image is resized to the requested size so the resolution sweep still applies.
/// This is the caller-driven twin of [`bench_input`], for front-ends that take
/// the input path as an argument rather than from the environment.
pub fn input_frame(path: Option<&Path>, width: u32, height: u32) -> Vec<u8> {
	if let Some(path) = path {
		match image::open(path) {
			Ok(img) => {
				let resized = img.resize_exact(width, height, image::imageops::FilterType::Triangle);
				return resized.to_rgba8().into_raw();
			}
			Err(err) => {
				eprintln!(
					"aexlo-bench: input {} failed to load ({err}); using synthetic input",
					path.display()
				);
			}
		}
	}
	synthetic_input(width, height)
}

/// Build the configured input frame (see [`bench_input`]) and install it on
/// `instance` as an 8-bit input layer.
pub fn set_bench_input(instance: &mut PluginInstance, width: u32, height: u32) -> std::result::Result<(), String> {
	set_input_frame(instance, None, width, height)
}

/// Install a `width x height` input layer built from `path` (see [`input_frame`]).
pub fn set_input_frame(
	instance: &mut PluginInstance,
	path: Option<&Path>,
	width: u32,
	height: u32,
) -> std::result::Result<(), String> {
	let pixels = match path {
		Some(_) => input_frame(path, width, height),
		// Preserve the env-driven default for callers that pass no explicit path.
		None => bench_input(width, height),
	};
	let layer = Layer::<Depth8>::from_raw(pixels, width, height).map_err(|err| format!("{err}"))?;
	instance.set_input(layer);
	Ok(())
}

//==== Parameter sweep (AEXLO_BENCH_PARAMS) ============================

/// One parameter's sweep: a name plus the numeric values to try for it.
#[derive(Clone, Debug)]
pub struct ParamSweep {
	pub name: String,
	pub values: Vec<f64>,
}

/// Parse `AEXLO_BENCH_PARAMS` into per-parameter sweeps.
///
/// Grammar: `Name=v1,v2,v3;Other=v4,v5`. Parameter names may contain spaces
/// (they are matched case-insensitively against the plugin's declared names).
/// Returns an empty vec when unset, meaning "use defaults".
pub fn param_sweeps() -> Vec<ParamSweep> {
	let raw = match std::env::var("AEXLO_BENCH_PARAMS") {
		Ok(raw) => raw,
		Err(_) => return Vec::new(),
	};
	raw.split(';')
		.filter_map(|clause| {
			let (name, values) = clause.split_once('=')?;
			let name = name.trim();
			if name.is_empty() {
				return None;
			}
			let values: Vec<f64> = values.split(',').filter_map(|v| v.trim().parse::<f64>().ok()).collect();
			if values.is_empty() {
				eprintln!("aexlo-bench: AEXLO_BENCH_PARAMS: no numeric values for '{name}', ignoring");
				return None;
			}
			Some(ParamSweep {
				name: name.to_string(),
				values,
			})
		})
		.collect()
}

/// One concrete parameter configuration: `(name, value)` pairs to apply together.
pub type ParamConfig = Vec<(String, f64)>;

/// The cartesian product of all [`param_sweeps`], i.e. every parameter
/// combination to benchmark. Always yields at least one config (the empty
/// "defaults" config when nothing is swept).
pub fn param_configs() -> Vec<ParamConfig> {
	let sweeps = param_sweeps();
	let mut configs: Vec<ParamConfig> = vec![Vec::new()];
	for sweep in &sweeps {
		let mut next = Vec::new();
		for base in &configs {
			for &value in &sweep.values {
				let mut combo = base.clone();
				combo.push((sweep.name.clone(), value));
				next.push(combo);
			}
		}
		configs = next;
	}
	configs
}

/// A compact, stable label for a parameter config, e.g. `Radius=500,Iterations=8`.
/// Empty config yields `default`.
pub fn param_config_label(config: &ParamConfig) -> String {
	if config.is_empty() {
		return "default".to_string();
	}
	config
		.iter()
		.map(|(name, value)| format!("{name}={value}"))
		.collect::<Vec<_>>()
		.join(",")
}

/// Resolve a parameter name to its 1-based index, matching declared names
/// case-insensitively.
///
/// A key that is just a number is taken as the index itself, so front-ends can
/// accept `--set 3=0.5` for parameters the plugin left unnamed. Declared names
/// win, so a parameter literally named `3` is still reachable by name.
pub fn resolve_param_index(instance: &PluginInstance, name: &str) -> Option<usize> {
	let name = name.trim();
	let by_name = (1..instance.param_count()).find(|&i| param_name(instance, i).eq_ignore_ascii_case(name));
	by_name.or_else(|| name.parse::<usize>().ok().filter(|&i| i < instance.param_count()))
}

/// Set parameter `name` to numeric `value`, coercing to the parameter's declared
/// type. Returns the value actually written for logging, or an error string when
/// the name is unknown or the parameter type can't take a scalar.
pub fn apply_param(instance: &mut PluginInstance, name: &str, value: f64) -> std::result::Result<ParamValue, String> {
	let index = resolve_param_index(instance, name).ok_or_else(|| format!("no parameter named '{name}'"))?;
	let current = instance
		.get_param(index)
		.ok_or_else(|| format!("parameter '{name}' has an unsupported type"))?;
	let new = match current {
		ParamValue::Float(_) => ParamValue::Float(value),
		ParamValue::Fixed(_) => ParamValue::Fixed(value as f32),
		ParamValue::Slider(_) => ParamValue::Slider(value as i32),
		ParamValue::Popup(_) => ParamValue::Popup(value as i32),
		ParamValue::Angle(_) => ParamValue::Angle(value as f32),
		ParamValue::Checkbox(_) => ParamValue::Checkbox(value != 0.0),
		other => return Err(format!("parameter '{name}' is {other:?}; scalar sweep unsupported")),
	};
	instance
		.set_param(index, new.clone())
		.map_err(|err| format!("{err:?}"))?;
	Ok(new)
}

/// Apply every `(name, value)` in a config, warning (but not failing) on any
/// parameter that can't be set. Returns `false` if any assignment failed, so the
/// caller can decide whether the config is still worth benchmarking.
pub fn apply_param_config(instance: &mut PluginInstance, config: &ParamConfig) -> bool {
	let mut ok = true;
	for (name, value) in config {
		if let Err(err) = apply_param(instance, name, *value) {
			eprintln!("aexlo-bench: param '{name}={value}': {err}");
			ok = false;
		}
	}
	ok
}

//==== Render modes (CPU vs GPU) =======================================

/// Which render path to drive, so the CPU and GPU paths can be timed separately.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderMode {
	/// Auto-dispatch (`render_frame`): GPU if available, else smart, else legacy.
	Auto,
	/// CPU only: the smart pre-render/render pair when declared, else legacy.
	Cpu,
	/// GPU only (`render_gpu`).
	Gpu,
}

impl RenderMode {
	/// Short label used in benchmark ids and summary rows.
	pub fn label(self) -> &'static str {
		match self {
			RenderMode::Auto => "render",
			RenderMode::Cpu => "cpu",
			RenderMode::Gpu => "gpu",
		}
	}

	/// Drive one frame through this path.
	pub fn render(self, instance: &mut PluginInstance) -> Result<()> {
		match self {
			RenderMode::Auto => instance.render_frame(),
			RenderMode::Cpu => render_cpu(instance),
			RenderMode::Gpu => instance.render_gpu(),
		}
	}
}

/// Render one frame strictly on the CPU: the smart pre-render/render pair when
/// the plugin declared smart-render support (falling back to legacy on failure),
/// otherwise the legacy render command. Mirrors [`PluginInstance::render_frame`]
/// minus the GPU attempt.
pub fn render_cpu(instance: &mut PluginInstance) -> Result<()> {
	if instance.supports_smart_render() && instance.render_pre().and_then(|()| instance.render_smart()).is_ok() {
		return Ok(());
	}
	instance.render()
}

/// The render modes to benchmark for a plugin: `[Cpu, Gpu]` when it can render
/// on the GPU (so the two can be compared), otherwise a single [`RenderMode::Auto`].
pub fn bench_modes(instance: &PluginInstance) -> Vec<RenderMode> {
	if instance.supports_gpu() {
		vec![RenderMode::Cpu, RenderMode::Gpu]
	} else {
		vec![RenderMode::Auto]
	}
}

//==== Timing ==========================================================

/// The timings from one measured `(plugin, mode, resolution, params)` point.
///
/// Samples are kept sorted so the order statistics are cheap; the median is the
/// headline number (robust against the occasional scheduler hiccup).
#[derive(Clone, Debug)]
pub struct Timing {
	/// Per-iteration render times, ascending.
	pub samples: Vec<Duration>,
}

impl Timing {
	/// Median render time. Panics if `samples` is empty, which [`measure`] never
	/// produces (it requires `samples >= 1`).
	pub fn median(&self) -> Duration {
		self.samples[self.samples.len() / 2]
	}

	/// Fastest observed render -- the closest thing to a noise-free measurement.
	pub fn min(&self) -> Duration {
		self.samples[0]
	}

	pub fn mean(&self) -> Duration {
		let total: Duration = self.samples.iter().sum();
		total / self.samples.len() as u32
	}

	/// Median throughput in megapixels/second for a frame of `pixels` pixels.
	pub fn mpx_per_s(&self, pixels: u64) -> f64 {
		pixels as f64 / self.median().as_secs_f64() / 1.0e6
	}
}

/// How to measure one plugin: what to render, how to configure it, and how many
/// times to time it.
#[derive(Clone, Copy, Debug)]
pub struct MeasureOptions<'a> {
	pub resolution: Resolution,
	/// Parameters to apply before timing; empty means plugin defaults.
	pub config: &'a ParamConfig,
	/// Input image, resized to `resolution`. `None` falls back to
	/// `AEXLO_BENCH_INPUT`, then to a synthetic gradient.
	pub input: Option<&'a Path>,
	/// Timed iterations. Must be at least 1.
	pub samples: usize,
	/// Untimed iterations run first, to pay first-frame setup costs.
	pub warmup: usize,
}

/// Load a fresh instance, configure it, warm it up, then time `samples` renders.
///
/// Each call loads the plugin from scratch so one measurement can't inherit
/// state (caches, allocated worlds) from another. Errors are strings because
/// every failure here is a "skip this point and keep going" for the caller.
pub fn measure(
	path: &Path,
	mode: RenderMode,
	options: MeasureOptions<'_>,
) -> std::result::Result<(Timing, Caps), String> {
	if options.samples == 0 {
		return Err("samples must be at least 1".to_string());
	}

	let mut instance = PluginInstance::try_load(path).map_err(|e| format!("load failed: {e:?}"))?;
	let _ = instance.about();
	let caps = capabilities(&instance);

	if !apply_param_config(&mut instance, options.config) {
		return Err("parameter setup failed".to_string());
	}

	let (width, height) = (options.resolution.width, options.resolution.height);
	set_input_frame(&mut instance, options.input, width, height).map_err(|e| format!("set_input failed: {e}"))?;

	// Warmup also validates that the mode works before any timing is recorded.
	for _ in 0..options.warmup {
		mode.render(&mut instance)
			.map_err(|e| format!("render failed: {e:?}"))?;
	}

	let mut samples = Vec::with_capacity(options.samples);
	for _ in 0..options.samples {
		let start = Instant::now();
		mode.render(&mut instance)
			.map_err(|e| format!("render failed: {e:?}"))?;
		samples.push(start.elapsed());
	}
	samples.sort();
	Ok((Timing { samples }, caps))
}

//==== Capabilities (#8) ===============================================

/// The capabilities a plugin declared during global setup, for annotating output.
#[derive(Clone, Copy, Debug)]
pub struct Caps {
	pub smart_render: bool,
	pub gpu: bool,
	pub param_count: usize,
}

/// Read a plugin's declared capabilities. Note `gpu` honors `AEXLO_DISABLE_GPU`,
/// i.e. it reflects whether the GPU path will actually be used, not just support.
pub fn capabilities(instance: &PluginInstance) -> Caps {
	Caps {
		smart_render: instance.supports_smart_render(),
		gpu: instance.supports_gpu(),
		param_count: instance.param_count(),
	}
}

impl std::fmt::Display for Caps {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(
			f,
			"smart_render={} gpu={} params={}",
			self.smart_render, self.gpu, self.param_count
		)
	}
}
