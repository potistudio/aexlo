//! `info`: what a plugin says about itself - its PiPL, ABOUT text, the
//! version and flags it declares at GLOBAL_SETUP, and its parameters.

use std::io::Write;
use std::path::Path;

use aexlo::{ParamValue, Pipl, PluginInstance};
use anyhow::{Context, Result};
use serde_json::{Value, json};

use crate::args::resolve_artifact;
use crate::toolkit::Format;

/// `aexlo info`'s flags. No section flag means every section.
#[derive(clap::Args)]
pub struct InfoArgs {
	/// The plugin, or a crate directory to build
	plugin: String,
	/// The ABOUT text (PF_Cmd_ABOUT)
	#[arg(long)]
	about: bool,
	/// The version, out_flags and out_flags2 GLOBAL_SETUP declares
	#[arg(long)]
	flags: bool,
	/// Each parameter's index, name, kind, value, visibility and UI flags
	#[arg(long)]
	params: bool,
	/// The PiPL resource, read from the file without loading the plugin
	#[arg(long)]
	pipl: bool,
	/// Print one JSON object with a key per section
	#[arg(long)]
	json: bool,
	/// The old spelling of --json: human or json
	#[arg(long, value_enum, value_name = "F", hide = true)]
	format: Option<Format>,
}

/// The sections to print, in the order they are printed.
struct Sections {
	pipl: bool,
	about: bool,
	flags: bool,
	params: bool,
}

impl Sections {
	fn of(args: &InfoArgs) -> Self {
		let any = args.pipl || args.about || args.flags || args.params;
		Self {
			pipl: args.pipl || !any,
			about: args.about || !any,
			flags: args.flags || !any,
			params: args.params || !any,
		}
	}

	fn count(&self) -> usize {
		[self.pipl, self.about, self.flags, self.params]
			.iter()
			.filter(|on| **on)
			.count()
	}

	fn needs_loading(&self) -> bool {
		self.about || self.flags || self.params
	}
}

pub fn run(args: InfoArgs) -> Result<()> {
	let sections = Sections::of(&args);
	let format = Format::from_flags(args.json, args.format);
	// A crate directory that does not build is reported like a missing plugin.
	let path = match resolve_artifact(&args.plugin) {
		Ok(path) => path,
		Err(err) => return Err(Output::new(format, Path::new(&args.plugin), false).fail(err)),
	};
	let mut out = Output::new(format, &path, sections.count() > 1);

	if sections.pipl {
		match Pipl::read(&path).with_context(|| format!("reading the PiPL of {}", path.display())) {
			Ok(pipl) => out.pipl(pipl.as_ref()),
			Err(err) => return Err(out.fail(err)),
		}
	}

	if sections.needs_loading() {
		let loaded = aexlo::Host::get()
			.load(&path)
			.with_context(|| format!("loading plugin {}", path.display()));
		let mut instance = match loaded {
			Ok(instance) => instance,
			Err(err) => return Err(out.fail(err)),
		};
		if sections.about {
			match instance.about().context("plugin rejected PF_Cmd_ABOUT") {
				Ok(text) => out.about(text.trim()),
				Err(err) => return Err(out.fail(err)),
			}
		}
		if sections.flags {
			out.flags(&instance);
		}
		if sections.params {
			out.params(&instance);
		}
	}
	out.finish();
	Ok(())
}

/// Prints sections as they come (human) or gathers them into one object
/// (json).
struct Output {
	format: Format,
	/// Whether human output heads each section with its name.
	headed: bool,
	printed: usize,
	json: serde_json::Map<String, Value>,
	/// Where the JSON goes, out of the way of what the plugin prints (see
	/// `json_output`). `None` for human output.
	json_out: Option<Box<dyn std::io::Write>>,
}

impl Output {
	fn new(format: Format, plugin: &Path, headed: bool) -> Self {
		let mut json = serde_json::Map::new();
		json.insert("plugin".into(), json!(plugin.display().to_string()));
		Self {
			format,
			headed,
			printed: 0,
			json,
			json_out: (format == Format::Json).then(crate::inspect::json_output),
		}
	}

	/// Print a human section: its heading when there are several, then
	/// `lines`, indented under the heading.
	fn section(&mut self, heading: &str, lines: &[String]) {
		if !self.headed {
			for line in lines {
				println!("{line}");
			}
			return;
		}
		if self.printed > 0 {
			println!();
		}
		self.printed += 1;
		println!("{heading}");
		for line in lines {
			if line.is_empty() {
				println!();
			} else {
				println!("  {line}");
			}
		}
	}

	fn pipl(&mut self, pipl: Option<&Pipl>) {
		match self.format {
			Format::Json => {
				let value = pipl.map_or(Value::Null, |pipl| {
					let properties: Vec<Value> = pipl
						.properties()
						.iter()
						.map(|p| {
							json!({
								"vendor": p.vendor(),
								"key": p.key(),
								"id": p.id(),
								"value": p.value().to_string(),
								"data": hex(p.data()),
							})
						})
						.collect();
					json!({"source": pipl.source().display().to_string(), "properties": properties})
				});
				self.json.insert("pipl".into(), value);
			}
			Format::Human => {
				let Some(pipl) = pipl else {
					self.section("PiPL", &["(no PiPL resource found)".into()]);
					return;
				};
				let mut lines = vec![format!("from {}", pipl.source().display())];
				for p in pipl.properties() {
					let vendor = if p.vendor() == "8BIM" {
						String::new()
					} else {
						format!(" ({})", p.vendor())
					};
					lines.push(format!("{:<4}  {}{vendor}", p.key(), p.value()));
				}
				self.section("PiPL", &lines);
			}
		}
	}

	fn about(&mut self, text: &str) {
		match self.format {
			Format::Json => {
				self.json.insert("about".into(), json!(text));
			}
			Format::Human => {
				let lines: Vec<String> = text.lines().map(str::to_string).collect();
				self.section("About", &lines);
			}
		}
	}

	fn flags(&mut self, instance: &PluginInstance) {
		let version = instance.version();
		let sets = [
			(
				"out_flags",
				instance.out_flags().bits(),
				names(&format!("{:?}", instance.out_flags())),
			),
			(
				"out_flags2",
				instance.out_flags2().bits(),
				names(&format!("{:?}", instance.out_flags2())),
			),
		];
		match self.format {
			Format::Json => {
				let mut value = json!({"version": version.to_string()});
				for (label, bits, names) in sets {
					value[label] = json!({"bits": bits, "names": names});
				}
				self.json.insert("flags".into(), value);
			}
			Format::Human => {
				let mut lines = vec![format!("{:<10}  {version}", "version")];
				for (label, bits, names) in sets {
					let names = if names.is_empty() {
						"(none)".to_string()
					} else {
						names.join(" | ")
					};
					lines.push(format!("{label:<10}  {bits:#010x}  {names}"));
				}
				self.section("Flags", &lines);
			}
		}
	}

	fn params(&mut self, instance: &PluginInstance) {
		let params: Vec<Param> = (1..instance.param_count()).map(|i| Param::of(instance, i)).collect();
		match self.format {
			Format::Json => {
				let list: Vec<Value> = params
					.iter()
					.map(|p| {
						json!({
							"index": p.index,
							"name": p.name,
							"kind": p.kind,
							"value": p.value,
							"hidden": p.hidden,
							"ui_flags": p.ui_flags,
						})
					})
					.collect();
				self.json.insert("params".into(), Value::Array(list));
			}
			Format::Human => {
				let lines = if params.is_empty() {
					vec!["(plugin declares no parameters)".into()]
				} else {
					params
						.iter()
						.map(|p| {
							let value = p.value.as_deref().unwrap_or("");
							let hidden = if p.hidden { " hidden" } else { "" };
							format!(
								"{:>3}  {:<24}  {:<8} {value}{hidden}  ui={:#x}",
								p.index, p.name, p.kind, p.ui_flags
							)
						})
						.collect()
				};
				self.section("Params", &lines);
			}
		}
	}

	/// The error to return after a section could not be produced. JSON
	/// output still prints what it has, with the error under `error`.
	fn fail(mut self, err: anyhow::Error) -> anyhow::Error {
		if self.format == Format::Json {
			self.json.insert("error".into(), json!(format!("{err:#}")));
			self.finish();
		}
		err
	}

	fn finish(self) {
		if let Some(mut out) = self.json_out {
			let _ = writeln!(out, "{}", Value::Object(self.json));
			let _ = out.flush();
		}
	}
}

/// `OutFlags(A | B | 0x40)` -> `[A, B, 0x40]`; no flags -> `[]`.
fn names(debug: &str) -> Vec<String> {
	let inner = debug
		.split_once('(')
		.map_or(debug, |(_, rest)| rest.trim_end_matches(')'));
	inner
		.split(" | ")
		.filter(|name| !name.is_empty() && *name != "0x0")
		.map(str::to_string)
		.collect()
}

fn hex(bytes: &[u8]) -> String {
	bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A parameter as `--params` lists it.
struct Param {
	index: usize,
	name: String,
	kind: String,
	value: Option<String>,
	hidden: bool,
	ui_flags: u32,
}

impl Param {
	fn of(instance: &PluginInstance, index: usize) -> Self {
		let value = instance.get_param(index);
		let kind = match &value {
			Some(value) => kind_of(value).to_string(),
			None => instance
				.param_kind(index)
				.map(|kind| format!("{kind:?}"))
				.unwrap_or_default(),
		};
		Self {
			index,
			name: instance
				.param_name(index)
				.filter(|name| !name.is_empty())
				.unwrap_or_else(|| format!("Param {index}")),
			kind,
			value: value.as_ref().map(describe),
			hidden: instance.param_hidden(index),
			ui_flags: instance.param_ui_flags(index).unwrap_or_default().bits(),
		}
	}
}

fn kind_of(value: &ParamValue) -> &'static str {
	match value {
		ParamValue::Float(_) => "float",
		ParamValue::Fixed(_) => "fixed",
		ParamValue::Slider(_) => "slider",
		ParamValue::Popup(_) => "popup",
		ParamValue::Angle(_) => "angle",
		ParamValue::Checkbox(_) => "checkbox",
		ParamValue::Point { .. } => "point",
		ParamValue::Color { .. } => "color",
		ParamValue::Path(_) => "path",
		ParamValue::Point3D { .. } => "point3d",
		_ => "other",
	}
}

/// A parameter's value, without its kind.
fn describe(value: &ParamValue) -> String {
	match value {
		ParamValue::Float(v) => v.to_string(),
		ParamValue::Fixed(v) => v.to_string(),
		ParamValue::Slider(v) => v.to_string(),
		ParamValue::Popup(v) => v.to_string(),
		ParamValue::Angle(v) => v.to_string(),
		ParamValue::Checkbox(v) => v.to_string(),
		ParamValue::Point { x, y } => format!("{x},{y}"),
		ParamValue::Color {
			red,
			green,
			blue,
			alpha,
		} => format!("{red},{green},{blue},{alpha}"),
		ParamValue::Path(id) => id.to_string(),
		ParamValue::Point3D { x, y, z } => format!("{x},{y},{z}"),
		other => format!("{other:?}"),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn names_split_the_flags() {
		assert_eq!(names("OutFlags(DEEP_COLOR_AWARE | 0x40)"), ["DEEP_COLOR_AWARE", "0x40"]);
		assert!(names("OutFlags2(0x0)").is_empty());
	}
}
