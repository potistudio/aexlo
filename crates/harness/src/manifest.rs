//! `aexlo.toml` (§4): discovery, parsing, `inherits` resolution and matrix
//! expansion into [`Variant`]s.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use toml::{Table, Value};

use crate::error::{Error, Result};
use crate::preset::{
	Axis, BenchSpec, CHECKS, DEFAULT_CHECKS, GoldenSpec, InputSpec, Iterate, PluginRef, PluginSource, RenderMode,
	StrictFeature, Time, Variant, VariantId, display_value, file_safe,
};

/// The manifest's file name.
pub const FILE_NAME: &str = "aexlo.toml";

/// Default per-run timeout, seconds (§4.3).
pub const DEFAULT_TIMEOUT: f64 = 30.0;

/// Keys whose tables merge key by key through `inherits` (§4.4).
const MERGED_TABLES: &[&str] = &["params", "layers", "input", "golden", "bench", "time"];

/// Keys that describe a preset rather than its run conditions.
const PRESET_ONLY: &[&str] = &["name", "hidden", "inherits"];

/// The nearest `aexlo.toml` in `start` or one of its ancestors.
pub fn discover(start: &Path) -> Option<PathBuf> {
	start
		.ancestors()
		.map(|dir| dir.join(FILE_NAME))
		.find(|candidate| candidate.is_file())
}

/// A loaded manifest: the plugin, `[defaults]` and the presets as written.
#[derive(Debug, Clone)]
pub struct Manifest {
	/// The manifest file.
	pub path: PathBuf,
	/// Its directory; relative paths inside the manifest resolve against it.
	pub dir: PathBuf,
	pub plugin: PluginRef,
	defaults: Table,
	presets: Vec<(String, Table)>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawManifest {
	plugin: PluginTable,
	#[serde(default)]
	defaults: Table,
	#[serde(default)]
	preset: Vec<Table>,
}

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct PluginTable {
	artifact: Option<ArtifactField>,
	#[serde(rename = "crate")]
	krate: Option<String>,
	entry: Option<String>,
}

#[derive(Deserialize, Clone)]
#[serde(untagged)]
enum ArtifactField {
	Path(String),
	PerPlatform {
		macos: Option<String>,
		windows: Option<String>,
	},
}

#[derive(Deserialize)]
#[serde(untagged)]
enum OneOrMany<T> {
	One(T),
	Many(Vec<T>),
}

impl<T> OneOrMany<T> {
	fn into_vec(self) -> Vec<T> {
		match self {
			Self::One(one) => vec![one],
			Self::Many(many) => many,
		}
	}
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Toggle<T> {
	Bool(bool),
	Spec(T),
}

/// Every field a preset (or `[defaults]`) may carry (§4.3), for validation.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fields {
	#[allow(dead_code)]
	name: Option<String>,
	#[allow(dead_code)]
	hidden: Option<bool>,
	#[allow(dead_code)]
	inherits: Option<OneOrMany<String>>,
	plugin: Option<PluginTable>,
	input: Option<InputSpec>,
	size: Option<[u32; 2]>,
	time: Option<Time>,
	params: Option<Table>,
	layers: Option<BTreeMap<String, InputSpec>>,
	render: Option<OneOrMany<String>>,
	depth: Option<OneOrMany<u32>>,
	iterate: Option<Iterate>,
	checks: Option<Vec<String>>,
	strict: Option<Toggle<Vec<StrictFeature>>>,
	golden: Option<Toggle<GoldenSpec>>,
	bench: Option<Toggle<BenchSpec>>,
	timeout: Option<f64>,
}

impl Fields {
	fn parse(table: &Table, what: &str) -> Result<Self> {
		Value::Table(table.clone())
			.try_into::<Fields>()
			.map_err(|e| Error::invalid(format!("{what}: {}", e.message())))
	}
}

impl Manifest {
	/// Load the manifest at `path`.
	///
	/// # Errors
	/// [`ErrorKind::Invalid`](crate::ErrorKind::Invalid) for a missing or
	/// malformed manifest.
	pub fn load(path: &Path) -> Result<Self> {
		let text =
			std::fs::read_to_string(path).map_err(|e| Error::invalid(format!("reading {}: {e}", path.display())))?;
		let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
		Self::parse(&text, &path)
	}

	/// Parse manifest `text` as if it were the file at `path`.
	pub fn parse(text: &str, path: &Path) -> Result<Self> {
		let shown = path.display();
		let raw: RawManifest = toml::from_str(text).map_err(|e| Error::invalid(format!("{shown}: {e}")))?;
		let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
		let plugin = plugin_ref(&raw.plugin, &dir).map_err(|e| e.context(format!("{shown}: [plugin]")))?;

		for key in PRESET_ONLY {
			if raw.defaults.contains_key(*key) {
				return Err(Error::invalid(format!("{shown}: [defaults] cannot set `{key}`")));
			}
		}
		Fields::parse(&raw.defaults, &format!("{shown}: [defaults]"))?;

		let mut presets = Vec::with_capacity(raw.preset.len());
		let mut seen = HashSet::new();
		for (i, table) in raw.preset.into_iter().enumerate() {
			let name = match table.get("name") {
				Some(Value::String(name)) => name.clone(),
				Some(_) => {
					return Err(Error::invalid(format!(
						"{shown}: preset #{}: `name` must be a string",
						i + 1
					)));
				}
				None => return Err(Error::invalid(format!("{shown}: preset #{} has no `name`", i + 1))),
			};
			if name.is_empty() || !name.chars().all(|c| matches!(c, 'a'..='z' | '0'..='9' | '_')) {
				return Err(Error::invalid(format!(
					"{shown}: preset name '{name}' must match [a-z0-9_]+"
				)));
			}
			if !seen.insert(name.clone()) {
				return Err(Error::invalid(format!("{shown}: preset '{name}' is defined twice")));
			}
			Fields::parse(&table, &format!("{shown}: preset '{name}'"))?;
			presets.push((name, table));
		}

		Ok(Self {
			path: path.to_path_buf(),
			dir,
			plugin,
			defaults: raw.defaults,
			presets,
		})
	}

	/// Preset names in manifest order, hidden ones included.
	pub fn preset_names(&self) -> impl Iterator<Item = &str> {
		self.presets.iter().map(|(name, _)| name.as_str())
	}

	/// Whether preset `name` is a hidden base.
	pub fn is_hidden(&self, name: &str) -> bool {
		self.raw(name)
			.and_then(|t| t.get("hidden"))
			.and_then(Value::as_bool)
			.unwrap_or(false)
	}

	fn raw(&self, name: &str) -> Option<&Table> {
		self.presets.iter().find(|(n, _)| n == name).map(|(_, t)| t)
	}

	/// Preset `name` with its whole `inherits` chain applied, `[defaults]`
	/// included, as a table of run conditions.
	pub fn resolve(&self, name: &str) -> Result<Table> {
		let mut stack = Vec::new();
		let table = self.resolve_inner(name, &mut stack)?;
		Ok(table)
	}

	fn resolve_inner(&self, name: &str, stack: &mut Vec<String>) -> Result<Table> {
		if stack.iter().any(|n| n == name) {
			stack.push(name.to_string());
			return Err(Error::invalid(format!("`inherits` cycle: {}", stack.join(" -> "))));
		}
		let raw = self
			.raw(name)
			.ok_or_else(|| match stack.last() {
				Some(child) => Error::invalid(format!("preset '{child}' inherits unknown preset '{name}'")),
				None => Error::invalid(format!("no preset named '{name}'")),
			})?
			.clone();
		stack.push(name.to_string());

		let parents: Vec<String> = match raw.get("inherits") {
			None => Vec::new(),
			Some(Value::String(parent)) => vec![parent.clone()],
			Some(Value::Array(items)) => items
				.iter()
				.map(|v| {
					v.as_str()
						.map(str::to_string)
						.ok_or_else(|| Error::invalid(format!("preset '{name}': `inherits` must name presets")))
				})
				.collect::<Result<_>>()?,
			Some(_) => return Err(Error::invalid(format!("preset '{name}': `inherits` must name presets"))),
		};

		let mut own = raw;
		for key in PRESET_ONLY {
			own.remove(*key);
		}

		// Earlier parents win over later ones; `[defaults]` is the root.
		let mut inherited = if parents.is_empty() {
			self.defaults.clone()
		} else {
			let mut merged: Option<Table> = None;
			for parent in &parents {
				let resolved = self.resolve_inner(parent, stack)?;
				merged = Some(match merged {
					None => resolved,
					Some(earlier) => merge(earlier, resolved),
				});
			}
			merged.unwrap_or_default()
		};
		inherited = merge(own, inherited);

		stack.pop();
		Ok(inherited)
	}

	/// Every variant of every non-hidden preset, in manifest order.
	pub fn variants(&self) -> Result<Vec<Variant>> {
		let mut variants = Vec::new();
		for (name, _) in &self.presets {
			if !self.is_hidden(name) {
				variants.extend(self.preset_variants(name)?);
			}
		}
		Ok(variants)
	}

	/// The variants of preset `name` (hidden presets expand too, for tools
	/// that start from one).
	pub fn preset_variants(&self, name: &str) -> Result<Vec<Variant>> {
		let table = self.resolve(name)?;
		expand(name, &table, &self.plugin, &self.dir).map_err(|e| e.context(format!("preset '{name}'")))
	}

	/// The variants of preset `name` whose display id matches `filter`
	/// ([`matches_filter`]).
	pub fn filtered(&self, filter: Option<&str>) -> Result<Vec<Variant>> {
		Ok(self
			.variants()?
			.into_iter()
			.filter(|v| filter.is_none_or(|f| matches_filter(&v.id.display, f)))
			.collect())
	}
}

/// `hi` over `lo` (§4.4): `hi`'s keys win; [`MERGED_TABLES`] merge key by key.
fn merge(hi: Table, lo: Table) -> Table {
	let mut out = lo;
	for (key, hi_value) in hi {
		let merged = match (key.as_str(), out.remove(&key), hi_value) {
			("checks", Some(Value::Array(lo)), Value::Array(hi)) => Value::Array(merge_checks(lo, hi)),
			("input", Some(Value::Table(lo)), Value::Table(hi)) => Value::Table(merge_input(hi, lo)),
			(key, Some(Value::Table(lo)), Value::Table(hi)) if MERGED_TABLES.contains(&key) => {
				Value::Table(if matches!(key, "params" | "layers") {
					merge_flat(hi, lo)
				} else {
					merge_deep(hi, lo)
				})
			}
			// `golden = true` keeps an inherited table.
			("golden" | "bench", Some(Value::Table(lo)), Value::Boolean(true)) => Value::Table(lo),
			(_, _, hi) => hi,
		};
		out.insert(key, merged);
	}
	out
}

/// Key-by-key merge whose values replace whole (params, layers).
fn merge_flat(hi: Table, mut lo: Table) -> Table {
	for (key, value) in hi {
		lo.insert(key, value);
	}
	lo
}

/// Recursive key-by-key merge (golden tolerances, ...).
fn merge_deep(hi: Table, mut lo: Table) -> Table {
	for (key, value) in hi {
		let merged = match (lo.remove(&key), value) {
			(Some(Value::Table(lo)), Value::Table(hi)) => Value::Table(merge_deep(hi, lo)),
			(_, value) => value,
		};
		lo.insert(key, merged);
	}
	lo
}

/// Inputs merge key by key unless `hi` switches to another source, which
/// replaces the inherited input whole (its generator keys mean nothing then).
fn merge_input(hi: Table, lo: Table) -> Table {
	let source = |t: &Table| (t.get("path").cloned(), t.get("generate").cloned());
	let (hi_path, hi_gen) = source(&hi);
	let (lo_path, lo_gen) = source(&lo);
	let switches = (hi_path.is_some() || hi_gen.is_some()) && (hi_path != lo_path || hi_gen != lo_gen);
	if switches { hi } else { merge_flat(hi, lo) }
}

/// A `checks` list with plain ids replaces; one of only `+id`/`-id` adjusts
/// the inherited list (normalized later by [`normalize_checks`]).
fn merge_checks(lo: Vec<Value>, hi: Vec<Value>) -> Vec<Value> {
	let plain = |v: &Value| v.as_str().is_some_and(|s| !s.starts_with(['+', '-']));
	if hi.is_empty() || hi.iter().any(plain) {
		hi
	} else {
		lo.into_iter().chain(hi).collect()
	}
}

/// The final check list: plain ids form the base (else §7's defaults), then
/// `+id`/`-id` adjust it in order.
fn normalize_checks(list: Option<&[String]>) -> Result<Vec<String>> {
	let Some(list) = list else {
		return Ok(DEFAULT_CHECKS.iter().map(|s| s.to_string()).collect());
	};
	if list.is_empty() {
		return Ok(Vec::new());
	}
	let known = |id: &str| -> Result<()> {
		if CHECKS.contains(&id) {
			Ok(())
		} else {
			Err(Error::invalid(format!(
				"unknown check '{id}' (known: {})",
				CHECKS.join(", ")
			)))
		}
	};
	let plain: Vec<&String> = list.iter().filter(|s| !s.starts_with(['+', '-'])).collect();
	let mut checks: Vec<String> = if plain.is_empty() {
		DEFAULT_CHECKS.iter().map(|s| s.to_string()).collect()
	} else {
		plain.iter().map(|s| s.to_string()).collect()
	};
	for id in &plain {
		known(id)?;
	}
	for entry in list {
		if let Some(id) = entry.strip_prefix('+') {
			known(id)?;
			if !checks.iter().any(|c| c == id) {
				checks.push(id.to_string());
			}
		} else if let Some(id) = entry.strip_prefix('-') {
			known(id)?;
			checks.retain(|c| c != id);
		}
	}
	Ok(checks)
}

fn plugin_ref(table: &PluginTable, dir: &Path) -> Result<PluginRef> {
	let entry = table.entry.clone().unwrap_or_else(|| "EffectMain".to_string());
	let source = match (&table.artifact, &table.krate) {
		(Some(_), Some(_)) => return Err(Error::invalid("give either `artifact` or `crate`, not both")),
		(None, None) => return Err(Error::invalid("needs `artifact` or `crate`")),
		(Some(artifact), None) => {
			let path = match artifact {
				ArtifactField::Path(path) => path.clone(),
				ArtifactField::PerPlatform { macos, windows } => {
					let (platform, path) = if cfg!(target_os = "windows") {
						("windows", windows)
					} else {
						("macos", macos)
					};
					path.clone()
						.ok_or_else(|| Error::invalid(format!("`artifact` has no `{platform}` entry")))?
				}
			};
			PluginSource::Artifact(dir.join(path))
		}
		(None, Some(krate)) => PluginSource::Crate(dir.join(krate)),
	};
	Ok(PluginRef { source, entry })
}

/// Expand a resolved preset table into its variants (§4.8).
fn expand(name: &str, table: &Table, plugin: &PluginRef, dir: &Path) -> Result<Vec<Variant>> {
	let fields = Fields::parse(table, "resolved preset")?;

	let plugin = match &fields.plugin {
		Some(table) => plugin_ref(table, dir).map_err(|e| e.context("plugin"))?,
		None => plugin.clone(),
	};

	let mut input = fields.input.unwrap_or_default();
	input.validate()?;
	input.resolve_paths(dir);

	let mut layers = Vec::new();
	if let Some(map) = fields.layers {
		// Keep manifest order rather than the BTreeMap's.
		let order: Vec<String> = table
			.get("layers")
			.and_then(Value::as_table)
			.map(|t| t.keys().cloned().collect())
			.unwrap_or_default();
		let mut map = map;
		for key in order {
			if let Some(mut spec) = map.remove(&key) {
				spec.validate().map_err(|e| e.context(format!("layer '{key}'")))?;
				spec.resolve_paths(dir);
				layers.push((key, spec));
			}
		}
	}

	if let Some([w, h]) = fields.size
		&& (w == 0 || h == 0)
	{
		return Err(Error::invalid(format!("size [{w}, {h}] must be positive")));
	}

	let depths = fields.depth.map(OneOrMany::into_vec).unwrap_or_else(|| vec![8]);
	for depth in &depths {
		if ![8, 16, 32].contains(depth) {
			return Err(Error::invalid(format!("depth {depth} must be 8, 16 or 32")));
		}
	}
	let renders = fields
		.render
		.map(OneOrMany::into_vec)
		.unwrap_or_else(|| vec!["auto".to_string()])
		.iter()
		.map(|r| RenderMode::parse(r))
		.collect::<Result<Vec<_>>>()?;
	if depths.is_empty() || renders.is_empty() {
		return Err(Error::invalid("`depth` and `render` cannot be empty"));
	}

	// Parameters: fixed values, and swept ones that become matrix axes.
	let mut fixed: Vec<(String, Value)> = Vec::new();
	let mut sweeps: Vec<(String, Vec<Value>)> = Vec::new();
	for (key, value) in fields.params.unwrap_or_default() {
		match value {
			Value::Table(t) => {
				let values = match (t.get("sweep"), t.len()) {
					(Some(Value::Array(values)), 1) if !values.is_empty() => values.clone(),
					_ => {
						return Err(Error::invalid(format!(
							"param '{key}': a table value must be `{{ sweep = [v1, v2, ...] }}`"
						)));
					}
				};
				sweeps.push((key, values));
			}
			value => fixed.push((key, value)),
		}
	}

	let checks = normalize_checks(fields.checks.as_deref())?;
	let mut strict: Vec<StrictFeature> = match fields.strict {
		None | Some(Toggle::Bool(false)) => Vec::new(),
		Some(Toggle::Bool(true)) => StrictFeature::ALL.to_vec(),
		Some(Toggle::Spec(features)) => features,
	};
	// `coverage` reads the poison `poison_output` fills the output with.
	if checks.iter().any(|c| c == "coverage") && !strict.contains(&StrictFeature::PoisonOutput) {
		strict.push(StrictFeature::PoisonOutput);
	}
	strict.sort();
	strict.dedup();

	let golden = match fields.golden {
		None | Some(Toggle::Bool(true)) => Some(GoldenSpec::default()),
		Some(Toggle::Bool(false)) => None,
		Some(Toggle::Spec(spec)) => Some(spec),
	};
	let bench = match fields.bench {
		None | Some(Toggle::Bool(true)) => Some(BenchSpec::default()),
		Some(Toggle::Bool(false)) => None,
		Some(Toggle::Spec(spec)) => Some(spec),
	};
	if let Some(bench) = &bench
		&& bench.samples == 0
	{
		return Err(Error::invalid("bench.samples must be at least 1"));
	}
	let timeout = fields.timeout.unwrap_or(DEFAULT_TIMEOUT);
	if timeout.is_nan() || timeout <= 0.0 {
		return Err(Error::invalid(format!("timeout {timeout} must be positive")));
	}

	// The cartesian product: depth, then render, then sweeps in manifest order.
	let mut combos: Vec<Vec<Axis>> = vec![Vec::new()];
	let mut axis_lists: Vec<Vec<Axis>> = vec![
		depths.iter().map(|d| Axis::Depth(*d)).collect(),
		renders.iter().map(|r| Axis::Render(*r)).collect(),
	];
	for (key, values) in &sweeps {
		axis_lists.push(
			values
				.iter()
				.map(|value| Axis::Param {
					key: key.clone(),
					value: value.clone(),
				})
				.collect(),
		);
	}
	for list in &axis_lists {
		combos = combos
			.into_iter()
			.flat_map(|combo| {
				list.iter().map(move |axis| {
					let mut next = combo.clone();
					next.push(axis.clone());
					next
				})
			})
			.collect();
	}
	let multi: Vec<bool> = axis_lists.iter().map(|l| l.len() > 1).collect();

	let mut variants = Vec::with_capacity(combos.len());
	for combo in combos {
		let mut depth = depths[0];
		let mut render = renders[0];
		let mut params = fixed.clone();
		let mut axes = Vec::new();
		for (axis, multi) in combo.into_iter().zip(&multi) {
			match &axis {
				Axis::Depth(d) => depth = *d,
				Axis::Render(r) => render = *r,
				Axis::Param { key, value } => params.push((key.clone(), value.clone())),
			}
			if *multi {
				axes.push(axis);
			}
		}
		// Swept params keep their manifest position among the fixed ones.
		let order: Vec<&String> = table
			.get("params")
			.and_then(Value::as_table)
			.map(|t| t.keys().collect())
			.unwrap_or_default();
		params.sort_by_key(|(k, _)| order.iter().position(|o| *o == k).unwrap_or(usize::MAX));

		variants.push(Variant {
			preset: name.to_string(),
			id: variant_id(name, &axes),
			axes,
			plugin: plugin.clone(),
			input: input.clone(),
			size: fields.size,
			time: fields.time.unwrap_or_default(),
			params,
			layers: layers.clone(),
			render,
			depth,
			iterate: fields.iterate.unwrap_or_default(),
			checks: checks.clone(),
			strict: strict.clone(),
			golden: golden.clone(),
			bench,
			timeout,
		});
	}
	Ok(variants)
}

/// The display, file-safe and golden ids of a variant (§4.8, §8.1).
pub fn variant_id(preset: &str, axes: &[Axis]) -> VariantId {
	let mut display = Vec::new();
	let mut file = Vec::new();
	let mut golden = Vec::new();
	for axis in axes {
		match axis {
			Axis::Depth(d) => {
				display.push(format!("depth={d}"));
				file.push(format!("d{d}"));
				golden.push(format!("d{d}"));
			}
			Axis::Render(r) => {
				display.push(format!("render={}", r.name()));
				file.push(r.name().to_string());
			}
			Axis::Param { key, value } => {
				let shown = display_value(value);
				display.push(format!("{key}={shown}"));
				let part = format!("{}-{}", file_safe(key), file_safe(&shown));
				file.push(part.clone());
				golden.push(part);
			}
		}
	}
	let join = |parts: &[String], sep: &str| {
		if parts.is_empty() {
			preset.to_string()
		} else {
			format!("{preset}{sep}{}", parts.join("."))
		}
	};
	VariantId {
		display: if display.is_empty() {
			preset.to_string()
		} else {
			format!("{preset}[{}]", display.join(","))
		},
		file_safe: join(&file, "."),
		golden: join(&golden, "."),
	}
}

/// Whether `id` matches `filter`: a glob when `filter` contains `*` (`?`
/// matches one character), a substring otherwise.
pub fn matches_filter(id: &str, filter: &str) -> bool {
	if !filter.contains('*') {
		return id.contains(filter);
	}
	fn glob(pattern: &[char], text: &[char]) -> bool {
		match (pattern.first(), text.first()) {
			(None, None) => true,
			(Some('*'), _) => glob(&pattern[1..], text) || (!text.is_empty() && glob(pattern, &text[1..])),
			(Some('?'), Some(_)) => glob(&pattern[1..], &text[1..]),
			(Some(p), Some(t)) if p == t => glob(&pattern[1..], &text[1..]),
			_ => false,
		}
	}
	let pattern: Vec<char> = filter.chars().collect();
	let text: Vec<char> = id.chars().collect();
	glob(&pattern, &text)
}

#[cfg(test)]
mod tests {
	use super::*;

	const SPEC_EXAMPLE: &str = r#"
[plugin]
artifact = { macos = "build/MyGlow.plugin", windows = "build/MyGlow.aex" }

[defaults]
size   = [1920, 1080]
depth  = [8, 16, 32]
render = ["smart", "gpu"]
checks = ["finite", "coverage", "deterministic"]

[[preset]]
name   = "base"
hidden = true
input  = { generate = "gradient" }

[[preset]]
name     = "dot_glow"
inherits = "base"
input    = { generate = "dot", at = [320, 240], radius = 2 }
size     = [640, 480]
time     = { frame = 12, fps = 24 }
params   = { Radius = 100.0, Mode = "Screen", Center = [320, 240] }
golden   = { tolerance = { max_abs = 0.004 } }

[[preset]]
name     = "radius_sweep"
inherits = "dot_glow"
params   = { Radius = { sweep = [10, 100, 1000] } }
golden   = false
bench    = { samples = 30, warmup = 5 }
"#;

	fn parse(text: &str) -> Result<Manifest> {
		Manifest::parse(text, Path::new("/project/aexlo.toml"))
	}

	#[test]
	fn spec_example_expands_as_described() {
		let manifest = parse(SPEC_EXAMPLE).unwrap();
		let variants = manifest.variants().unwrap();
		// base is hidden; dot_glow: 3 depths x 2 renders; sweep: x3 radii.
		assert_eq!(variants.len(), 6 + 18);
		let first = &variants[0];
		assert_eq!(first.id.display, "dot_glow[depth=8,render=smart]");
		assert_eq!(first.id.file_safe, "dot_glow.d8.smart");
		assert_eq!(first.id.golden, "dot_glow.d8");
		assert_eq!(first.size, Some([640, 480]));
		assert_eq!(first.time.frame, 12);
		assert_eq!(first.golden.as_ref().unwrap().tolerance.max_abs, 0.004);
		assert_eq!(
			first.plugin.source,
			PluginSource::Artifact(if cfg!(target_os = "windows") {
				"/project/build/MyGlow.aex".into()
			} else {
				"/project/build/MyGlow.plugin".into()
			})
		);

		let sweep: Vec<_> = variants.iter().filter(|v| v.preset == "radius_sweep").collect();
		assert_eq!(sweep[3].id.display, "radius_sweep[depth=8,render=gpu,Radius=10]");
		assert_eq!(sweep[3].id.file_safe, "radius_sweep.d8.gpu.radius-10");
		assert!(sweep.iter().all(|v| v.golden.is_none()));
		// The swept value replaces the inherited one, in place.
		let keys: Vec<_> = sweep[3].params.iter().map(|(k, _)| k.as_str()).collect();
		assert_eq!(keys, ["Radius", "Mode", "Center"]);
		assert_eq!(sweep[3].param("Radius"), Some(&Value::Integer(10)));
		// Inherited from dot_glow: the dot input, merged key by key.
		assert_eq!(sweep[0].input.radius, Some(2.0));
	}

	#[test]
	fn earlier_parents_win() {
		let manifest = parse(
			r#"
[plugin]
artifact = "x.plugin"
[[preset]]
name = "a"
hidden = true
size = [10, 10]
params = { A = 1 }
[[preset]]
name = "b"
hidden = true
size = [20, 20]
params = { A = 2, B = 2 }
[[preset]]
name = "c"
inherits = ["a", "b"]
"#,
		)
		.unwrap();
		let v = &manifest.variants().unwrap()[0];
		assert_eq!(v.size, Some([10, 10]));
		assert_eq!(v.param("A"), Some(&Value::Integer(1)));
		assert_eq!(v.param("B"), Some(&Value::Integer(2)));
	}

	#[test]
	fn checks_adjust_or_replace() {
		let manifest = parse(
			r#"
[plugin]
artifact = "x.plugin"
[defaults]
checks = ["-deterministic"]
[[preset]]
name = "adjusted"
checks = ["+iterate-parallel"]
[[preset]]
name = "replaced"
inherits = "adjusted"
checks = ["finite"]
[[preset]]
name = "none"
inherits = "adjusted"
checks = []
"#,
		)
		.unwrap();
		let variants = manifest.variants().unwrap();
		assert_eq!(variants[0].checks, ["finite", "coverage", "iterate-parallel"]);
		assert_eq!(variants[1].checks, ["finite"]);
		assert!(variants[2].checks.is_empty());
		// `coverage` implies poisoning the output.
		assert_eq!(variants[0].strict, [StrictFeature::PoisonOutput]);
		assert!(variants[1].strict.is_empty());
	}

	#[test]
	fn input_switching_source_replaces_it() {
		let manifest = parse(
			r#"
[plugin]
artifact = "x.plugin"
[defaults]
input = { generate = "dot", radius = 3 }
[[preset]]
name = "file"
input = { path = "plates/a.png" }
[[preset]]
name = "moved"
input = { at = [1, 2] }
"#,
		)
		.unwrap();
		let variants = manifest.variants().unwrap();
		assert_eq!(variants[0].input.path, Some(PathBuf::from("/project/plates/a.png")));
		assert_eq!(variants[0].input.radius, None);
		assert_eq!(variants[1].input.radius, Some(3.0));
		assert_eq!(variants[1].input.at, Some([1.0, 2.0]));
	}

	#[test]
	fn invalid_manifests_are_rejected() {
		let cases = [
			(
				"[plugin]\nartifact = \"x\"\n[[preset]]\nname = \"a\"\nsizee = [1, 1]\n",
				"sizee",
			),
			(
				"[plugin]\nartifact = \"x\"\n[[preset]]\nname = \"a\"\ninherits = \"nope\"\n",
				"unknown preset",
			),
			(
				"[plugin]\nartifact = \"x\"\n[[preset]]\nname = \"a\"\ninherits = \"b\"\n[[preset]]\nname = \"b\"\ninherits = \"a\"\n",
				"cycle",
			),
			("[plugin]\nartifact = \"x\"\n[[preset]]\nname = \"Bad\"\n", "[a-z0-9_]"),
			("[plugin]\n[[preset]]\nname = \"a\"\n", "artifact"),
			(
				"[plugin]\nartifact = \"x\"\n[[preset]]\nname = \"a\"\ndepth = 12\n",
				"depth",
			),
			(
				"[plugin]\nartifact = \"x\"\n[[preset]]\nname = \"a\"\nchecks = [\"bogus\"]\n",
				"unknown check",
			),
			(
				"[plugin]\nartifact = \"x\"\n[[preset]]\nname = \"a\"\nrender = \"fast\"\n",
				"render mode",
			),
			(
				"[plugin]\nartifact = \"x\"\n[[preset]]\nname = \"a\"\nparams = { A = { x = 1 } }\n",
				"sweep",
			),
		];
		for (text, needle) in cases {
			let err = parse(text).and_then(|m| m.variants().map(|_| ())).unwrap_err();
			assert_eq!(err.kind, crate::ErrorKind::Invalid, "{text}");
			assert!(
				err.message.contains(needle),
				"'{}' should mention '{needle}'",
				err.message
			);
		}
	}

	#[test]
	fn filters_by_substring_or_glob() {
		assert!(matches_filter("dot_glow[depth=16,render=gpu]", "depth=16"));
		assert!(matches_filter("dot_glow[depth=16,render=gpu]", "dot*gpu]"));
		assert!(!matches_filter("dot_glow[depth=16,render=gpu]", "dot*smart*"));
		assert!(matches_filter("abc", "a?c*"));
		assert!(!matches_filter("abc", "a?c"), "no `*`: a plain substring");
	}

	#[test]
	fn discovers_the_nearest_manifest() {
		let root = std::env::temp_dir().join(format!("aexlo-discover-{}", std::process::id()));
		let nested = root.join("a/b");
		std::fs::create_dir_all(&nested).unwrap();
		std::fs::write(root.join(FILE_NAME), "").unwrap();
		assert_eq!(discover(&nested), Some(root.join(FILE_NAME)));
		std::fs::remove_dir_all(&root).unwrap();
	}
}
