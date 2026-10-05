//! The resolved shape of a preset (§4.3) and of the variants a preset expands
//! into (§4.8). Parsing and inheritance live in [`crate::manifest`]; this
//! module is the data every front-end and the worker share.

use std::path::{Path, PathBuf};

use aexlo::PixelDepthKind;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// How a variant is rendered (§4.3 `render`).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum RenderMode {
	/// `render_frame`: GPU when possible, else smart, else legacy.
	Auto,
	/// `PF_Cmd_RENDER` only.
	Legacy,
	/// `PF_Cmd_SMART_PRE_RENDER` + `PF_Cmd_SMART_RENDER` only.
	Smart,
	/// `PF_Cmd_SMART_RENDER_GPU` only.
	Gpu,
}

impl RenderMode {
	pub fn name(self) -> &'static str {
		match self {
			Self::Auto => "auto",
			Self::Legacy => "legacy",
			Self::Smart => "smart",
			Self::Gpu => "gpu",
		}
	}

	pub fn parse(name: &str) -> Result<Self> {
		match name.trim().to_ascii_lowercase().as_str() {
			"auto" => Ok(Self::Auto),
			"legacy" => Ok(Self::Legacy),
			"smart" => Ok(Self::Smart),
			"gpu" => Ok(Self::Gpu),
			other => Err(Error::invalid(format!(
				"unknown render mode '{other}' (expected auto, legacy, smart or gpu)"
			))),
		}
	}
}

/// Whether iterate callbacks may run on worker threads (§4.3 `iterate`).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "lowercase")]
pub enum Iterate {
	#[default]
	Parallel,
	Serial,
}

/// A frame rate as the rational `num / den` frames per second.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(try_from = "FpsField", into = "FpsField")]
pub struct Fps {
	pub num: u32,
	pub den: u32,
}

impl Default for Fps {
	fn default() -> Self {
		Self { num: 24, den: 1 }
	}
}

impl Fps {
	/// `num / den` in lowest terms.
	pub fn new(num: u32, den: u32) -> Result<Self> {
		if num == 0 || den == 0 {
			return Err(Error::invalid(format!("fps {num}/{den} must be positive")));
		}
		let g = gcd(num, den);
		Ok(Self {
			num: num / g,
			den: den / g,
		})
	}

	/// Parse `24`, `23.976` or `"30000/1001"`.
	pub fn parse(text: &str) -> Result<Self> {
		let text = text.trim();
		if let Some((num, den)) = text.split_once('/') {
			let parse = |s: &str| {
				s.trim()
					.parse::<u32>()
					.map_err(|_| Error::invalid(format!("invalid fps '{text}'")))
			};
			return Self::new(parse(num)?, parse(den)?);
		}
		let value: f64 = text
			.parse()
			.map_err(|_| Error::invalid(format!("invalid fps '{text}'")))?;
		Self::from_f64(value)
	}

	/// An integral rate exactly, anything else to a thousandth of a frame.
	pub fn from_f64(value: f64) -> Result<Self> {
		if !value.is_finite() || value <= 0.0 {
			return Err(Error::invalid(format!("fps {value} must be positive")));
		}
		if value.fract() == 0.0 && value <= u32::MAX as f64 {
			return Self::new(value as u32, 1);
		}
		Self::new((value * 1000.0).round() as u32, 1000)
	}

	pub fn as_f64(self) -> f64 {
		self.num as f64 / self.den as f64
	}
}

impl std::fmt::Display for Fps {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		if self.den == 1 {
			write!(f, "{}", self.num)
		} else {
			write!(f, "{}/{}", self.num, self.den)
		}
	}
}

fn gcd(mut a: u32, mut b: u32) -> u32 {
	while b != 0 {
		(a, b) = (b, a % b);
	}
	a
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum FpsField {
	Integer(u32),
	Float(f64),
	Text(String),
}

impl TryFrom<FpsField> for Fps {
	type Error = String;

	fn try_from(field: FpsField) -> std::result::Result<Self, String> {
		match field {
			FpsField::Integer(n) => Fps::new(n, 1),
			FpsField::Float(v) => Fps::from_f64(v),
			FpsField::Text(text) => Fps::parse(&text),
		}
		.map_err(|e| e.message)
	}
}

impl From<Fps> for FpsField {
	fn from(fps: Fps) -> Self {
		if fps.den == 1 {
			FpsField::Integer(fps.num)
		} else {
			FpsField::Text(format!("{}/{}", fps.num, fps.den))
		}
	}
}

/// The comp time a variant renders at (§4.7).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct Time {
	#[serde(default)]
	pub frame: i64,
	#[serde(default)]
	pub fps: Fps,
}

impl Time {
	/// `(current_time, time_step, time_scale)` for
	/// [`PluginInstance::set_time`](aexlo::PluginInstance::set_time).
	pub fn as_ae(self) -> Result<(i32, i32, u32)> {
		let step = self.fps.den as i64;
		let current = self.frame * step;
		let current = i32::try_from(current)
			.map_err(|_| Error::invalid(format!("frame {} is out of range at {} fps", self.frame, self.fps)))?;
		let step = i32::try_from(step).map_err(|_| Error::invalid(format!("fps {} is out of range", self.fps)))?;
		Ok((current, step, self.fps.num))
	}
}

/// How an input file is fitted to the render size (§4.5).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Fit {
	#[default]
	Stretch,
	None,
}

/// A built-in input generator (§4.5).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Generator {
	/// Red ramps left to right, green top to bottom, blue along the diagonal.
	Gradient,
	/// One color everywhere.
	Solid,
	/// Black and white cells.
	Checker,
	/// A white disc on black.
	Dot,
	/// Seeded per-pixel noise.
	Noise,
}

impl Generator {
	pub fn name(self) -> &'static str {
		match self {
			Self::Gradient => "gradient",
			Self::Solid => "solid",
			Self::Checker => "checker",
			Self::Dot => "dot",
			Self::Noise => "noise",
		}
	}
}

/// Where a layer's pixels come from (§4.5): a file, or a generator.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct InputSpec {
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub path: Option<PathBuf>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub generate: Option<Generator>,
	/// `solid`: the color; `dot`: the disc's color. Normalized `[r, g, b(, a)]`.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub color: Option<Vec<f32>>,
	/// `checker`: cell size in pixels.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub cell: Option<u32>,
	/// `dot`: center in pixels.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub at: Option<[f32; 2]>,
	/// `dot`: radius in pixels.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub radius: Option<f32>,
	/// `noise`: the seed.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub seed: Option<u64>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub fit: Option<Fit>,
}

impl InputSpec {
	pub fn generated(generator: Generator) -> Self {
		Self {
			generate: Some(generator),
			..Self::default()
		}
	}

	/// The generator, `gradient` when neither a path nor a generator is given.
	pub fn generator(&self) -> Option<Generator> {
		match (&self.path, self.generate) {
			(Some(_), _) => None,
			(None, generator) => Some(generator.unwrap_or(Generator::Gradient)),
		}
	}

	pub fn fit(&self) -> Fit {
		self.fit.unwrap_or_default()
	}

	/// Reject contradictory or meaningless keys.
	pub fn validate(&self) -> Result<()> {
		if self.path.is_some() && self.generate.is_some() {
			return Err(Error::invalid("input: give either `path` or `generate`, not both"));
		}
		let generator = self.generator();
		let allowed = |key: &str, present: bool, generators: &[Generator]| -> Result<()> {
			if present && !generator.is_some_and(|g| generators.contains(&g)) {
				let used = generator.map_or("a file input", Generator::name);
				return Err(Error::invalid(format!("input: `{key}` does not apply to {used}")));
			}
			Ok(())
		};
		allowed("color", self.color.is_some(), &[Generator::Solid, Generator::Dot])?;
		allowed("cell", self.cell.is_some(), &[Generator::Checker])?;
		allowed("at", self.at.is_some(), &[Generator::Dot])?;
		allowed("radius", self.radius.is_some(), &[Generator::Dot])?;
		allowed("seed", self.seed.is_some(), &[Generator::Noise])?;
		if let Some(color) = &self.color
			&& !(3..=4).contains(&color.len())
		{
			return Err(Error::invalid("input: `color` must be [r, g, b] or [r, g, b, a]"));
		}
		if self.cell == Some(0) {
			return Err(Error::invalid("input: `cell` must be positive"));
		}
		Ok(())
	}

	/// Resolve a relative `path` against `dir`.
	pub fn resolve_paths(&mut self, dir: &Path) {
		if let Some(path) = &self.path
			&& path.is_relative()
		{
			self.path = Some(dir.join(path));
		}
	}
}

/// Golden comparison tolerances (§8.2), on values normalized to `[0, 1]`.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(deny_unknown_fields, default)]
pub struct Tolerance {
	/// Largest allowed per-channel difference.
	pub max_abs: f32,
	/// Fraction of pixels allowed to exceed `max_abs`.
	pub max_bad: f32,
	/// Optional floor on the PSNR, in dB.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub min_psnr: Option<f32>,
}

impl Default for Tolerance {
	fn default() -> Self {
		Self {
			max_abs: 1.0 / 255.0,
			max_bad: 0.0,
			min_psnr: None,
		}
	}
}

/// Who rendered a golden (§8.4).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum GoldenSource {
	/// Written by `aexlo test --bless`.
	#[default]
	Aexlo,
	/// Rendered by real After Effects; never overwritten by `--bless`.
	Ae,
}

/// How a variant is compared against its golden (§8).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(deny_unknown_fields, default)]
pub struct GoldenSpec {
	/// Explicit golden file, relative to the manifest.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub path: Option<PathBuf>,
	/// Extra axes the golden is split by: `render`, `iterate`, `platform`.
	pub split_by: Vec<String>,
	pub tolerance: Tolerance,
	pub source: GoldenSource,
}

/// Bench sampling (§10.1).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct BenchSpec {
	pub samples: usize,
	pub warmup: usize,
}

impl Default for BenchSpec {
	fn default() -> Self {
		Self { samples: 30, warmup: 5 }
	}
}

/// A strict-mode instrumentation feature (§5.3).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum StrictFeature {
	PoisonOutput,
	GuardBands,
	TrackAllocations,
	TrackCheckouts,
}

impl StrictFeature {
	pub const ALL: [StrictFeature; 4] = [
		Self::PoisonOutput,
		Self::GuardBands,
		Self::TrackAllocations,
		Self::TrackCheckouts,
	];

	pub fn name(self) -> &'static str {
		match self {
			Self::PoisonOutput => "poison_output",
			Self::GuardBands => "guard_bands",
			Self::TrackAllocations => "track_allocations",
			Self::TrackCheckouts => "track_checkouts",
		}
	}

	/// The check that reports what this feature detects.
	pub fn check(self) -> &'static str {
		match self {
			Self::PoisonOutput => "coverage",
			Self::GuardBands => "bounds",
			Self::TrackAllocations => "allocations",
			Self::TrackCheckouts => "checkouts",
		}
	}
}

/// Where a variant's plugin comes from (§4.2).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum PluginSource {
	/// A prebuilt artifact (absolute path).
	Artifact(PathBuf),
	/// A crate directory (absolute path) whose cdylib must be built first.
	Crate(PathBuf),
}

/// The plugin a variant drives.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
pub struct PluginRef {
	pub source: PluginSource,
	/// Entry symbol for in-process runs.
	pub entry: String,
}

impl PluginRef {
	/// A short label: the artifact's or crate's file name.
	pub fn label(&self) -> String {
		let path = match &self.source {
			PluginSource::Artifact(path) | PluginSource::Crate(path) => path,
		};
		path.file_stem()
			.map(|s| s.to_string_lossy().into_owned())
			.unwrap_or_else(|| path.display().to_string())
	}
}

/// The ids of a variant (§4.8).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VariantId {
	/// `dot_glow[depth=16,render=gpu,Radius=100]`
	pub display: String,
	/// `dot_glow.d16.gpu.radius-100`
	pub file_safe: String,
	/// The file-safe id without the `render` axis: the golden's name (§8.1).
	pub golden: String,
}

impl std::fmt::Display for VariantId {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str(&self.display)
	}
}

/// One axis value of a variant, for its ids.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum Axis {
	Depth(u32),
	Render(RenderMode),
	Param { key: String, value: toml::Value },
}

/// One concrete, fully resolved combination of a preset's matrix (§4.8): no
/// `inherits`, no sweeps, absolute paths. This is what the worker receives.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Variant {
	pub preset: String,
	pub id: VariantId,
	/// The axes this variant took a value on (the ones with several values).
	pub axes: Vec<Axis>,
	pub plugin: PluginRef,
	pub input: InputSpec,
	pub size: Option<[u32; 2]>,
	pub time: Time,
	/// Parameter keys (names or `#index`) to their values, in manifest order.
	pub params: Vec<(String, toml::Value)>,
	pub layers: Vec<(String, InputSpec)>,
	pub render: RenderMode,
	/// Bits per channel: 8, 16 or 32.
	pub depth: u32,
	pub iterate: Iterate,
	pub checks: Vec<String>,
	pub strict: Vec<StrictFeature>,
	pub golden: Option<GoldenSpec>,
	pub bench: Option<BenchSpec>,
	/// Seconds.
	pub timeout: f64,
}

impl Variant {
	pub fn depth_kind(&self) -> PixelDepthKind {
		PixelDepthKind::from_bits(self.depth).unwrap_or(PixelDepthKind::U8)
	}

	pub fn has_check(&self, id: &str) -> bool {
		self.checks.iter().any(|c| c == id)
	}

	pub fn has_strict(&self, feature: StrictFeature) -> bool {
		self.strict.contains(&feature)
	}

	/// The value this variant gives parameter `key`, matched as written.
	pub fn param(&self, key: &str) -> Option<&toml::Value> {
		self.params.iter().find(|(k, _)| k == key).map(|(_, v)| v)
	}
}

/// Every check id §7 knows.
pub const CHECKS: &[&str] = &[
	"finite",
	"coverage",
	"deterministic",
	"iterate-parallel",
	"depth-consistency",
	"bounds",
	"allocations",
	"checkouts",
	"flags",
];

/// Checks that run unless a preset says otherwise (§7).
pub const DEFAULT_CHECKS: &[&str] = &["finite", "coverage", "deterministic"];

/// Short display of a TOML value inside an id.
pub fn display_value(value: &toml::Value) -> String {
	match value {
		toml::Value::String(s) => s.clone(),
		toml::Value::Integer(i) => i.to_string(),
		toml::Value::Float(f) => f.to_string(),
		toml::Value::Boolean(b) => b.to_string(),
		toml::Value::Array(items) => items.iter().map(display_value).collect::<Vec<_>>().join(";"),
		other => other.to_string(),
	}
}

/// `[a-z0-9-]` version of an axis name or value for file names.
pub fn file_safe(text: &str) -> String {
	text.chars()
		.map(|c| match c {
			'a'..='z' | '0'..='9' | '-' => c,
			'A'..='Z' => c.to_ascii_lowercase(),
			'.' => 'p',
			_ => '_',
		})
		.collect()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn fps_reduces_to_lowest_terms() {
		assert_eq!(Fps::parse("24").unwrap(), Fps { num: 24, den: 1 });
		assert_eq!(Fps::parse("30000/1001").unwrap(), Fps { num: 30000, den: 1001 });
		assert_eq!(Fps::parse("48/2").unwrap(), Fps { num: 24, den: 1 });
		assert_eq!(Fps::parse("23.976").unwrap(), Fps { num: 2997, den: 125 });
		assert!(Fps::parse("0").is_err());
	}

	#[test]
	fn time_maps_frames_to_ae_time() {
		let time = Time {
			frame: 12,
			fps: Fps::parse("30000/1001").unwrap(),
		};
		assert_eq!(time.as_ae().unwrap(), (12 * 1001, 1001, 30000));
		let time = Time {
			frame: 12,
			fps: Fps::default(),
		};
		assert_eq!(time.as_ae().unwrap(), (12, 1, 24));
	}

	#[test]
	fn input_rejects_keys_of_other_generators() {
		let mut spec = InputSpec::generated(Generator::Dot);
		spec.cell = Some(4);
		assert!(spec.validate().is_err());
		spec.cell = None;
		spec.radius = Some(2.0);
		assert!(spec.validate().is_ok());
		assert!(
			InputSpec {
				path: Some("a.png".into()),
				seed: Some(1),
				..InputSpec::default()
			}
			.validate()
			.is_err()
		);
	}

	#[test]
	fn file_safe_ids_are_lowercase_ascii() {
		assert_eq!(file_safe("Radius"), "radius");
		assert_eq!(file_safe("0.5"), "0p5");
		assert_eq!(file_safe("Blend Mode"), "blend_mode");
	}
}
