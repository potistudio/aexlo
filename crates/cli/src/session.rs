//! The live state behind the browser viewer (`docs/toolkit.md` §11): the
//! plugin instance, the selected preset variant and the edits on top of it,
//! the frame time, and what the frame is compared against.
//!
//! `aexlo preview` and `aexlo dev --bin --web` own one session on their main
//! thread, hand it a way to load the plugin whenever it (re)builds, and let it
//! answer the viewer's commands.

use std::path::Path;

use aexlo::{ParamValue, PixelDepthKind, PluginInstance};
use aexlo_harness::bench::Samples;
use aexlo_harness::exec::StrictFindings;
use aexlo_harness::{BenchSpec, Frame, Manifest, RenderMode, StrictFeature, Time, Variant, apply, golden};
use anyhow::{Context, Result, anyhow, bail};

use crate::viewer::{Command, Image, Viewer, json_escape};

/// Loads a fresh instance of the plugin being previewed.
pub(crate) type Loader = Box<dyn FnMut() -> Result<PluginInstance>>;

pub(crate) struct Session {
	manifest: Option<Manifest>,
	/// `--strict`: poison the output and guard its bands while previewing.
	strict: bool,
	variant: Option<Variant>,
	/// Parameter edits made in the viewer, keyed as a manifest would.
	edits: Vec<(String, toml::Value)>,
	/// The frame time when no preset says it.
	time: Time,
	/// `off`, `golden`, `depth:<bits>` or `render:<mode>`.
	compare: String,
	load: Option<Loader>,
	fx: Option<PluginInstance>,
	/// The last render: its frame, timing and strict findings.
	last: Option<(Frame, Samples, Vec<String>, Option<usize>)>,
	error: Option<String>,
	toast: Option<String>,
	compare_info: Option<(String, String, bool)>,
}

/// `name` keyed as a manifest's `params` would: the declared name when
/// unique, `#index` otherwise.
fn param_key(fx: &PluginInstance, index: usize) -> String {
	let name = fx.param_name(index).unwrap_or_default();
	if !name.trim().is_empty() && fx.param_indices(&name).len() == 1 {
		name.trim().to_string()
	} else {
		format!("#{index}")
	}
}

/// `value` as a manifest writes it (§4.6).
fn manifest_value(fx: &PluginInstance, index: usize, value: &ParamValue) -> toml::Value {
	use toml::Value as V;
	let round = |v: f64| (v * 10_000.0).round() / 10_000.0;
	match value {
		ParamValue::Float(v) => V::Float(round(*v)),
		ParamValue::Fixed(v) | ParamValue::Angle(v) => V::Float(round(*v as f64)),
		ParamValue::Slider(v) => V::Integer(*v as i64),
		ParamValue::Checkbox(v) => V::Boolean(*v),
		ParamValue::Popup(v) => match fx
			.param_choices(index)
			.and_then(|c| c.get((*v - 1).max(0) as usize).cloned())
		{
			Some(label)
				if fx
					.param_choices(index)
					.is_some_and(|c| c.iter().filter(|l| **l == label).count() == 1) =>
			{
				V::String(label)
			}
			_ => V::Integer(*v as i64),
		},
		ParamValue::Point { x, y } => V::Array(vec![V::Float(round(*x as f64)), V::Float(round(*y as f64))]),
		ParamValue::Point3D { x, y, z } => {
			V::Array(vec![V::Float(round(*x)), V::Float(round(*y)), V::Float(round(*z))])
		}
		ParamValue::Color {
			red,
			green,
			blue,
			alpha,
		} => V::Array(
			[red, green, blue, alpha]
				.iter()
				.map(|c| V::Float(round(**c as f64 / 255.0)))
				.collect(),
		),
		ParamValue::Path(id) => V::Integer(*id as i64),
	}
}

/// A `toml` value as a `toml_edit` one, for writing into the manifest.
fn edit_value(value: &toml::Value) -> toml_edit::Value {
	match value {
		toml::Value::String(s) => s.as_str().into(),
		toml::Value::Integer(i) => (*i).into(),
		toml::Value::Float(f) => (*f).into(),
		toml::Value::Boolean(b) => (*b).into(),
		toml::Value::Array(items) => {
			let mut array = toml_edit::Array::new();
			for item in items {
				array.push(edit_value(item));
			}
			toml_edit::Value::Array(array)
		}
		other => other.to_string().as_str().into(),
	}
}

fn image_of(frame: &Frame) -> Image {
	let (w, h) = (frame.width(), frame.height());
	let mut rgba = vec![0u8; w as usize * h as usize * 4];
	let _ = frame.layer().write_rgba8(&mut rgba);
	Image { rgba, w, h }
}

impl Session {
	/// A session over `manifest` (if any), starting on `preset`: a preset
	/// name (its first variant) or one variant's display id.
	pub fn new(manifest: Option<Manifest>, preset: Option<&str>, strict: bool) -> Result<Self> {
		let variant = match (&manifest, preset) {
			(Some(manifest), Some(name)) => Some(find_variant(manifest, name)?),
			(None, Some(_)) => bail!("--preset needs a manifest (aexlo.toml, or pass --manifest)"),
			_ => None,
		};
		Ok(Self {
			manifest,
			strict,
			variant,
			edits: Vec::new(),
			time: Time::default(),
			compare: "off".to_string(),
			load: None,
			fx: None,
			last: None,
			error: None,
			toast: None,
			compare_info: None,
		})
	}

	/// How to load the plugin from now on (after a build or a file change).
	pub fn set_loader(&mut self, load: Loader) {
		self.load = Some(load);
	}

	fn variant_time(&self) -> Time {
		self.variant.as_ref().map_or(self.time, |v| v.time)
	}

	/// A fresh, configured instance: the variant (when there is one), then
	/// the edits and the time on top.
	fn instance(&mut self, variant: Option<&Variant>) -> Result<PluginInstance> {
		let load = self.load.as_mut().context("nothing to load yet")?;
		let mut fx = load()?;
		if let Some(variant) = variant {
			if let Some(reason) = apply::unsupported(&fx, variant) {
				bail!("{} does not apply: {reason}", variant.id);
			}
			apply::configure(&mut fx, variant).map_err(anyhow::Error::msg)?;
		}
		for (key, value) in &self.edits {
			let index = apply::resolve_param(&fx, key).map_err(anyhow::Error::msg)?;
			let value = apply::param_value(&fx, index, value).map_err(anyhow::Error::msg)?;
			fx.set_param(index, value)?;
		}
		if !self.edits.is_empty() {
			let _ = fx.update_params_ui();
		}
		let (current, step, scale) = self.variant_time().as_ae().map_err(anyhow::Error::msg)?;
		fx.set_time(current, step, scale);
		let features = variant.map(|v| v.strict.clone()).unwrap_or_default();
		let poison = self.strict || features.contains(&StrictFeature::PoisonOutput);
		let guards = self.strict || features.contains(&StrictFeature::GuardBands);
		fx.set_strict(aexlo::Strict {
			poison_output: poison,
			guard_bands: guards,
			track_allocations: false,
			track_checkouts: self.strict || features.contains(&StrictFeature::TrackCheckouts),
		});
		Ok(fx)
	}

	fn render_mode(&self) -> RenderMode {
		self.variant.as_ref().map_or(RenderMode::Auto, |v| v.render)
	}

	/// Render the live instance, timing it and collecting strict findings.
	fn render(&mut self) -> Result<()> {
		let mode = self.render_mode();
		let fx = self.fx.as_mut().context("no plugin loaded")?;
		let spec = BenchSpec { samples: 1, warmup: 0 };
		let samples = aexlo_harness::bench::measure(fx, spec, |fx| apply::render(fx, mode))
			.with_context(|| format!("{} render failed", mode.name()))?;
		let strict = fx.strict();
		let findings = StrictFindings::from(fx.strict_report());
		let violations: Vec<String> = findings
			.guard_violations
			.into_iter()
			.chain(findings.checkout_violations)
			.collect();
		let unwritten = strict.poison_output.then(|| aexlo::unwritten_pixels(fx.output()));
		self.last = Some((Frame::new(fx.output().clone()), samples, violations, unwritten));
		Ok(())
	}

	/// Reload the plugin (after a build or a selection), render and publish.
	pub fn reload(&mut self, viewer: &Viewer) -> Result<()> {
		self.fx = None;
		let variant = self.variant.clone();
		let result = self.instance(variant.as_ref()).and_then(|fx| {
			self.fx = Some(fx);
			self.render()
		});
		match &result {
			Ok(()) => {
				self.error = None;
				let frame = self.last.as_ref().map(|(f, ..)| image_of(f)).unwrap_or_default();
				if let Some(fx) = &self.fx {
					viewer.publish_reload(fx, frame);
				}
				self.refresh_compare(viewer);
			}
			Err(err) => self.error = Some(format!("{err:#}")),
		}
		self.publish_presets(viewer);
		self.publish_info(viewer);
		match &self.error {
			Some(error) => Err(anyhow!("{error}")),
			None => Ok(()),
		}
	}

	/// Re-render after an edit, publishing the frame (or the error).
	fn rerender(&mut self, viewer: &Viewer) {
		match self.render() {
			Ok(()) => {
				self.error = None;
				let frame = self.last.as_ref().map(|(f, ..)| image_of(f)).unwrap_or_default();
				if let Some(fx) = &self.fx {
					viewer.publish_frame(fx, frame);
				}
				self.refresh_compare(viewer);
			}
			Err(err) => self.error = Some(format!("{err:#}")),
		}
		self.publish_info(viewer);
	}

	/// Answer the viewer's queued commands.
	pub fn step(&mut self, viewer: &Viewer) {
		let mut changed = false;
		let mut reload = false;
		for command in viewer.commands() {
			match command {
				Command::Set { index, raw } => match self.set_param(index, &raw) {
					Ok(()) => changed = true,
					Err(err) => self.toast = Some(format!("set #{index} failed: {err:#}")),
				},
				Command::Select(id) => match &self.manifest {
					Some(manifest) => match find_variant(manifest, &id) {
						Ok(variant) => {
							self.variant = Some(variant);
							self.edits.clear();
							reload = true;
						}
						Err(err) => self.toast = Some(format!("{err:#}")),
					},
					None => self.toast = Some("no manifest to pick presets from".into()),
				},
				Command::Time(frame) => {
					let frame = frame.max(0);
					match &mut self.variant {
						Some(variant) => variant.time.frame = frame,
						None => self.time.frame = frame,
					}
					let time = self.variant_time().as_ae();
					if let (Some(fx), Ok((current, step, scale))) = (&mut self.fx, time) {
						fx.set_time(current, step, scale);
					}
					changed = true;
				}
				Command::Compare(mode) => {
					self.compare = mode;
					self.refresh_compare(viewer);
					self.publish_info(viewer);
				}
				Command::Save(name) => match self.save_preset(&name) {
					// The reload that shows the new preset publishes the toast.
					Ok(path) => {
						self.toast = Some(format!("saved preset '{name}' to {}", path.display()));
						reload = true;
					}
					Err(err) => {
						self.toast = Some(format!("save failed: {err:#}"));
						self.publish_info(viewer);
					}
				},
			}
		}
		if reload {
			if let Err(err) = self.reload(viewer) {
				eprintln!("aexlo: {err:#}");
			}
		} else if changed {
			self.rerender(viewer);
		} else if self.toast.is_some() {
			self.publish_info(viewer);
		}
	}

	fn set_param(&mut self, index: usize, raw: &str) -> Result<()> {
		let fx = self.fx.as_mut().context("no plugin loaded")?;
		let value = crate::parse_param_value(fx, index, raw)?;
		fx.set_param(index, value.clone())
			.with_context(|| format!("setting parameter #{index}"))?;
		let _ = fx.update_params_ui();
		let key = param_key(fx, index);
		let value = manifest_value(fx, index, &value);
		self.edits.retain(|(k, _)| *k != key);
		self.edits.push((key, value));
		Ok(())
	}

	/// Render what `compare` names and publish it as the reference.
	fn refresh_compare(&mut self, viewer: &Viewer) {
		self.compare_info = None;
		let Some((frame, ..)) = self.last.clone() else {
			viewer.publish_reference(None, None);
			return;
		};
		let reference = match self.reference() {
			Ok(Some((against, reference, tolerance))) => {
				let cmp = frame.compare(&reference, &tolerance);
				self.compare_info = Some((against, cmp.summary(&tolerance), cmp.passed));
				Some((reference.clone(), frame.diff_heatmap(&reference, &tolerance)))
			}
			Ok(None) => None,
			Err(err) => {
				self.compare_info = Some((self.compare.clone(), format!("{err:#}"), false));
				None
			}
		};
		match reference {
			Some((reference, diff)) => viewer.publish_reference(Some(image_of(&reference)), Some(image_of(&diff))),
			None => viewer.publish_reference(None, None),
		}
	}

	/// The reference frame `compare` names, what it is, and the tolerance.
	fn reference(&mut self) -> Result<Option<(String, Frame, aexlo_harness::Tolerance)>> {
		let mode = self.compare.clone();
		if mode == "off" {
			return Ok(None);
		}
		let variant = self
			.variant
			.clone()
			.ok_or_else(|| anyhow!("comparing needs a preset (pick one, or pass --preset)"))?;
		let tolerance = variant.golden.as_ref().map(|g| g.tolerance).unwrap_or_default();
		if mode == "golden" {
			let manifest = self.manifest.as_ref().context("no manifest")?;
			let spec = variant.golden.clone().context("the preset has `golden = false`")?;
			let path = golden::path(&variant, &spec, &manifest.dir);
			if !path.exists() {
				bail!(
					"no golden at {} yet (bless it with `aexlo test --bless`)",
					path.display()
				);
			}
			let reference = Frame::load(&path).map_err(anyhow::Error::msg)?;
			return Ok(Some((format!("golden {}", path.display()), reference, spec.tolerance)));
		}
		let mut other = variant.clone();
		let against = if let Some(bits) = mode.strip_prefix("depth:") {
			let bits: u32 = bits.parse().context("bad depth")?;
			PixelDepthKind::from_bits(bits).context("bad depth")?;
			other.depth = bits;
			format!("{bits} bpc")
		} else if let Some(render) = mode.strip_prefix("render:") {
			other.render = RenderMode::parse(render).map_err(anyhow::Error::msg)?;
			format!("{} render", other.render.name())
		} else {
			bail!("unknown comparison '{mode}'");
		};
		let mut fx = self.instance(Some(&other))?;
		let render = other.render;
		apply::render(&mut fx, render).with_context(|| format!("rendering the {against}"))?;
		Ok(Some((against, Frame::new(fx.output().clone()), tolerance)))
	}

	/// Append the current state to the manifest as preset `name` (§11), with
	/// `toml_edit` so the file keeps its formatting and comments.
	fn save_preset(&mut self, name: &str) -> Result<std::path::PathBuf> {
		let manifest = self.manifest.as_ref().context("no manifest to save into")?;
		if name.is_empty() || !name.chars().all(|c| matches!(c, 'a'..='z' | '0'..='9' | '_')) {
			bail!("preset names are [a-z0-9_]+");
		}
		if manifest.preset_names().any(|n| n == name) {
			bail!("preset '{name}' already exists");
		}
		let path = manifest.path.clone();
		let text = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
		let mut doc: toml_edit::DocumentMut = text.parse().with_context(|| format!("parsing {}", path.display()))?;

		let mut table = toml_edit::Table::new();
		table["name"] = toml_edit::value(name);
		let mut params: Vec<(String, toml::Value)> = Vec::new();
		if let Some(variant) = &self.variant {
			table["inherits"] = toml_edit::value(variant.preset.as_str());
			for axis in &variant.axes {
				match axis {
					aexlo_harness::Axis::Depth(bits) => table["depth"] = toml_edit::value(*bits as i64),
					aexlo_harness::Axis::Render(mode) => table["render"] = toml_edit::value(mode.name()),
					aexlo_harness::Axis::Param { key, value } => params.push((key.clone(), value.clone())),
				}
			}
		}
		for (key, value) in &self.edits {
			params.retain(|(k, _)| k != key);
			params.push((key.clone(), value.clone()));
		}
		if !params.is_empty() {
			let mut inline = toml_edit::InlineTable::new();
			for (key, value) in &params {
				inline.insert(key, edit_value(value));
			}
			table["params"] = toml_edit::value(inline);
		}
		let base_frame = match (&self.variant, &self.manifest) {
			(Some(variant), Some(manifest)) => manifest
				.preset_variants(&variant.preset)
				.ok()
				.and_then(|vs| vs.first().map(|v| v.time.frame))
				.unwrap_or(0),
			_ => 0,
		};
		let time = self.variant_time();
		if time.frame != base_frame {
			let mut inline = toml_edit::InlineTable::new();
			inline.insert("frame", time.frame.into());
			table["time"] = toml_edit::value(inline);
		}

		let presets = doc
			.entry("preset")
			.or_insert(toml_edit::Item::ArrayOfTables(toml_edit::ArrayOfTables::new()))
			.as_array_of_tables_mut()
			.context("`preset` in the manifest is not an array of tables")?;
		presets.push(table);
		let updated = doc.to_string();

		// Only write what parses back as a valid manifest.
		let reloaded = Manifest::parse(&updated, &path).map_err(anyhow::Error::msg)?;
		let first = reloaded
			.preset_variants(name)
			.map_err(anyhow::Error::msg)?
			.into_iter()
			.next()
			.context("the saved preset has no variants")?;
		std::fs::write(&path, updated).with_context(|| format!("writing {}", path.display()))?;
		self.manifest = Some(reloaded);
		self.variant = Some(first);
		self.edits.clear();
		Ok(path)
	}

	fn publish_presets(&self, viewer: &Viewer) {
		let json = match &self.manifest {
			None => "null".to_string(),
			Some(manifest) => {
				let ids: Vec<String> = manifest
					.variants()
					.unwrap_or_default()
					.iter()
					.map(|v| format!("\"{}\"", json_escape(&v.id.display)))
					.collect();
				let current = self
					.variant
					.as_ref()
					.map_or("null".to_string(), |v| format!("\"{}\"", json_escape(&v.id.display)));
				format!(
					"{{\"variants\":[{}],\"current\":{current},\"can_save\":true}}",
					ids.join(",")
				)
			}
		};
		viewer.publish_presets(json);
	}

	fn publish_info(&mut self, viewer: &Viewer) {
		let mut info = serde_json::Map::new();
		let time = self.variant_time();
		info.insert("frame".into(), time.frame.into());
		info.insert("time".into(), format!("frame {} @ {} fps", time.frame, time.fps).into());
		if let Some(variant) = &self.variant {
			info.insert("variant".into(), variant.id.display.clone().into());
			info.insert("depth".into(), variant.depth.into());
			info.insert("render".into(), variant.render.name().into());
		}
		if let Some((frame, samples, violations, unwritten)) = &self.last {
			info.insert("size".into(), format!("{}x{}", frame.width(), frame.height()).into());
			if self.variant.is_none() {
				info.insert("depth".into(), frame.depth().bits().into());
			}
			let stats = samples.stats(frame.width() as u64 * frame.height() as u64);
			info.insert(
				"timing".into(),
				serde_json::json!({
					"wall": stats.median,
					"pre_render": stats.phases.pre_render,
					"render": stats.phases.render,
					"gpu": stats.phases.gpu,
					"host": stats.phases.host,
				}),
			);
			let strict = self.fx.as_ref().map(PluginInstance::strict).unwrap_or_default();
			if strict.any() {
				info.insert(
					"strict".into(),
					serde_json::json!({ "unwritten": unwritten, "violations": violations }),
				);
			}
		}
		if let Some((against, summary, passed)) = &self.compare_info {
			info.insert(
				"compare".into(),
				serde_json::json!({ "against": against, "summary": summary, "passed": passed }),
			);
		}
		if let Some(error) = &self.error {
			info.insert("error".into(), error.clone().into());
		}
		if let Some(toast) = self.toast.take() {
			println!("aexlo: {toast}");
			info.insert("toast".into(), toast.into());
		}
		viewer.publish_info(serde_json::Value::Object(info).to_string());
	}
}

/// The variant `name` names: a variant's display id, else a preset's first.
fn find_variant(manifest: &Manifest, name: &str) -> Result<Variant> {
	let all = manifest.variants().map_err(anyhow::Error::msg)?;
	if let Some(exact) = all.iter().find(|v| v.id.display == name) {
		return Ok(exact.clone());
	}
	let preset = name.split('[').next().unwrap_or(name);
	manifest
		.preset_variants(preset)
		.map_err(anyhow::Error::msg)?
		.into_iter()
		.next()
		.ok_or_else(|| anyhow!("preset '{preset}' has no variants"))
}

/// The manifest for a viewer: `--manifest`, else the nearest one when a
/// preset was asked for or one is in reach.
pub(crate) fn viewer_manifest(path: Option<&Path>, wanted: bool) -> Result<Option<Manifest>> {
	match path {
		Some(path) => Ok(Some(Manifest::load(path).map_err(anyhow::Error::msg)?)),
		None => {
			let cwd = std::env::current_dir()?;
			match aexlo_harness::manifest::discover(&cwd) {
				Some(path) => Ok(Some(Manifest::load(&path).map_err(anyhow::Error::msg)?)),
				None if wanted => bail!("--preset needs an aexlo.toml (or --manifest)"),
				None => Ok(None),
			}
		}
	}
}
