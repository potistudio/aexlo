//! `aexlo render`: render one frame and write it to an image.

use std::path::{Path, PathBuf};

use aexlo::RenderPath;
use anyhow::{Context, Result, bail};

use crate::args::resolve_artifact;
use crate::{param, toolkit};

#[derive(clap::Args)]
pub struct RenderArgs {
	/// The plugin, or a crate directory to build
	#[arg(required_unless_present = "preset")]
	plugin: Option<String>,
	/// Start from a preset (or one variant's id) of the manifest; <PLUGIN> is
	/// then optional and the flags below override the preset
	#[arg(long, value_name = "NAME")]
	preset: Option<String>,
	/// The manifest [default: the nearest aexlo.toml]
	#[arg(long, value_name = "PATH", requires = "preset")]
	manifest: Option<PathBuf>,
	/// Feed a PNG as the effect's input layer
	#[arg(short, long, value_name = "PNG")]
	input: Option<PathBuf>,
	/// Where to write the frame (.exr keeps 32 bpc; 16 bpc writes a 16-bit
	/// PNG)
	#[arg(short, long, value_name = "FILE", default_value = "out.png")]
	output: PathBuf,
	/// Set parameter <P> (index, #index or name) to <V> before rendering
	/// (repeatable)
	#[arg(short, long = "set", value_name = "P=V", value_parser = param::parse_set, allow_hyphen_values = true)]
	sets: Vec<(String, String)>,
	/// Force the smart-render path
	#[arg(long, conflicts_with = "legacy")]
	smart: bool,
	/// Force the legacy render path
	#[arg(long)]
	legacy: bool,
	/// Run the plugin's iterate callbacks on one thread (for plugins whose
	/// pixel callbacks call suites, e.g. ones built on the Rust
	/// after-effects crate)
	#[arg(long)]
	serial_iterate: bool,
}

pub fn run(args: RenderArgs) -> Result<()> {
	let RenderArgs {
		plugin,
		preset,
		manifest,
		input,
		output,
		sets,
		smart: force_smart,
		legacy: force_legacy,
		serial_iterate,
	} = args;

	// A preset sets the stage (plugin, input, size, time, params, depth,
	// render path); the flags below then override it.
	let mut variant = match &preset {
		Some(name) => Some(preset_variant(manifest.as_deref(), name)?),
		None => None,
	};
	// `--smart`/`--legacy` replace the preset's render path, before it is
	// checked against what the plugin supports.
	if let Some(variant) = &mut variant {
		if force_smart {
			variant.render = aexlo_harness::RenderMode::Smart;
		} else if force_legacy {
			variant.render = aexlo_harness::RenderMode::Legacy;
		}
	}
	// `--input` replaces the preset's input, text layer included.
	if input.is_some()
		&& let Some(variant) = &mut variant
	{
		variant.input = aexlo_harness::InputSpec::default();
	}
	// Resolve overrides before setup and decode each input only once.
	let depth = variant.as_ref().map_or(aexlo::PixelDepthKind::U8, |v| v.depth_kind());
	let initial = if let Some(path) = &input {
		Some(
			aexlo_harness::frame::load_image(path)
				.map_err(anyhow::Error::msg)?
				.convert(depth),
		)
	} else if let Some(variant) = &variant {
		let size = variant.size.map(|[w, h]| (w, h));
		Some(aexlo_harness::input::build(&variant.input, depth, size).map_err(anyhow::Error::msg)?)
	} else {
		None
	};
	let path = match (&plugin, &variant) {
		(Some(plugin), _) => resolve_artifact(plugin)?,
		(None, Some(variant)) => {
			let artifacts = toolkit::artifacts(std::slice::from_ref(variant)).map_err(anyhow::Error::msg)?;
			artifacts(&variant.plugin).context("no artifact for preset's plugin")?
		}
		(None, None) => bail!("render: missing <plugin> (or --preset)"),
	};
	let mut instance = match initial {
		Some(layer) => {
			let output_size = variant
				.as_ref()
				.and_then(|v| v.size)
				.map(|[w, h]| (w, h))
				.unwrap_or((layer.width(), layer.height()));
			aexlo::Host::get()
				.loader(&path)
				.input(layer)
				.output_size(output_size.0, output_size.1)
				.load()?
		}
		None => aexlo::Host::get().load(&path)?,
	};
	if let Some(variant) = &variant {
		if let Some(reason) = aexlo_harness::apply::unsupported(&instance, variant) {
			bail!("preset {} does not apply: {reason}", variant.id);
		}
		aexlo_harness::apply::configure_prepared(&mut instance, variant).map_err(anyhow::Error::msg)?;
	}
	if serial_iterate {
		instance.set_parallel_iterate(false);
	}

	for (key, raw) in &sets {
		let index = param::resolve_key(&instance, key)?;
		let value = param::parse_value(&instance, index, raw)?;
		instance
			.set_param(index, value)
			.with_context(|| format!("setting parameter {key}"))?;
	}
	if !sets.is_empty() {
		let _ = instance.update_params_ui();
	}

	if force_smart {
		instance.render_with(RenderPath::Smart).context("smart render failed")?;
	} else if force_legacy {
		instance
			.render_with(RenderPath::Legacy)
			.context("legacy render failed")?;
	} else if let Some(variant) = &variant {
		aexlo_harness::apply::render(&mut instance, variant.render)
			.with_context(|| format!("{} render failed", variant.render.name()))?;
	} else {
		instance.render_frame().context("render failed")?;
	}

	// Deep output keeps its depth when the file can hold it (16-bit PNG,
	// EXR); otherwise it is quantized to an 8-bit PNG.
	let is_exr = output.extension().is_some_and(|e| e.eq_ignore_ascii_case("exr"));
	let frame = aexlo_harness::Frame::new(instance.output().clone());
	if is_exr {
		frame
			.convert(aexlo::PixelDepthKind::F32)
			.save(&output)
			.map_err(anyhow::Error::msg)?;
	} else if instance.output_depth() == aexlo::PixelDepthKind::U16 {
		frame.save(&output).map_err(anyhow::Error::msg)?;
	} else {
		// save_preview encodes with mtpng (multithreaded), the toolkit's fast
		// 8-bit PNG path.
		aexlo_harness::preview::save_preview(&instance, &output).map_err(anyhow::Error::msg)?;
	}

	let (w, h) = instance.output_size();
	println!("rendered {}x{} -> {}", w, h, output.display());
	Ok(())
}

/// The variant `name` names: a preset with a single variant, or one
/// variant's display id exactly (`dot_glow[depth=16]`).
fn preset_variant(manifest: Option<&Path>, name: &str) -> Result<aexlo_harness::Variant> {
	let manifest = toolkit::load_manifest(manifest).map_err(anyhow::Error::msg)?;
	let preset = name.split('[').next().unwrap_or(name);
	let variants = manifest.preset_variants(preset).map_err(anyhow::Error::msg)?;
	if let Some(exact) = variants.iter().find(|v| v.id.display == name) {
		return Ok(exact.clone());
	}
	match &variants[..] {
		[only] if name == preset => Ok(only.clone()),
		[first, ..] if name == preset => {
			eprintln!(
				"aexlo render: preset '{preset}' has {} variants; rendering {} (name one exactly to pick another)",
				variants.len(),
				first.id
			);
			Ok(first.clone())
		}
		_ => bail!(
			"no variant '{name}' (variants: {})",
			variants
				.iter()
				.map(|v| v.id.display.as_str())
				.collect::<Vec<_>>()
				.join(", ")
		),
	}
}
