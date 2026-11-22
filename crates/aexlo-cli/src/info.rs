//! `about` and `params`: ask a plugin about itself.

use aexlo::{ParamValue, PluginInstance};
use anyhow::{Context, Result};

use crate::args::load;

/// The one argument `about` and `params` take.
#[derive(clap::Args)]
pub struct PluginArgs {
	/// The plugin, or a crate directory to build
	plugin: String,
}

pub fn about(args: PluginArgs) -> Result<()> {
	let mut instance = load(&args.plugin)?;

	let text = instance.about().context("plugin rejected PF_Cmd_ABOUT")?;
	println!("{}", text.trim());

	Ok(())
}

pub fn params(args: PluginArgs) -> Result<()> {
	let instance = load(&args.plugin)?;
	print_params(&instance);
	Ok(())
}

fn print_params(instance: &PluginInstance) {
	if instance.param_count() <= 1 {
		println!("(plugin declares no parameters)");
		return;
	}
	for index in 1..instance.param_count() {
		let kind = instance
			.param_kind(index)
			.map(|kind| format!("{kind:?}"))
			.unwrap_or_default();
		let value = instance
			.get_param(index)
			.map(|value| describe(&value))
			.unwrap_or_else(|| kind);
		let hidden = if instance.param_hidden(index) { " hidden" } else { "" };
		let flags = instance.param_ui_flags(index).unwrap_or_default();
		println!(
			"{index:>3}  {:<24}  {value}{hidden}  ui={flags:#x}",
			param_name(instance, index)
		);
	}
}

/// The plugin-declared name for a parameter, falling back to a positional label.
fn param_name(instance: &PluginInstance, index: usize) -> String {
	instance
		.param_name(index)
		.filter(|name| !name.is_empty())
		.unwrap_or_else(|| format!("Param {index}"))
}

/// Render a `ParamValue` for the `params` listing.
fn describe(value: &ParamValue) -> String {
	match value {
		ParamValue::Float(v) => format!("float    {v}"),
		ParamValue::Fixed(v) => format!("fixed    {v}"),
		ParamValue::Slider(v) => format!("slider   {v}"),
		ParamValue::Popup(v) => format!("popup    {v}"),
		ParamValue::Angle(v) => format!("angle    {v}"),
		ParamValue::Checkbox(v) => format!("checkbox {v}"),
		ParamValue::Point { x, y } => format!("point    {x},{y}"),
		ParamValue::Color {
			red,
			green,
			blue,
			alpha,
		} => format!("color    {red},{green},{blue},{alpha}"),
		ParamValue::Path(id) => format!("path     {id}"),
		ParamValue::Point3D { x, y, z } => format!("point3d  {x},{y},{z}"),
		other => format!("{other:?}"),
	}
}
