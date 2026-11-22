//! `info`: what a plugin says about itself - its PiPL, ABOUT text, the flags
//! and version it declares at GLOBAL_SETUP, and its parameters.

use std::path::Path;

use aexlo::{OutFlags, OutFlags2, ParamValue, Pipl, PluginInstance};
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
	/// The version, out_flags and out_flags2 GLOBAL_SETUP declares, beside
	/// the PiPL's eVER, eGLO and eGL2, with the bits that differ marked
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
	let path = resolve_artifact(&args.plugin)?;
	let format = Format::from_flags(args.json, args.format);
	let mut out = Output::new(format, &path, sections.count() > 1);

	let pipl = if sections.pipl || sections.flags {
		match Pipl::read(&path).with_context(|| format!("reading the PiPL of {}", path.display())) {
			Ok(pipl) => pipl,
			Err(err) => return Err(out.fail(err)),
		}
	} else {
		None
	};
	if sections.pipl {
		out.pipl(pipl.as_ref());
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
			out.flags(pipl.as_ref(), &instance);
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

	fn flags(&mut self, pipl: Option<&Pipl>, instance: &PluginInstance) {
		let version = (pipl.and_then(Pipl::effect_version), instance.version());
		let flags = (pipl.and_then(Pipl::out_flags), instance.out_flags());
		let flags2 = (pipl.and_then(Pipl::out_flags2), instance.out_flags2());
		let rows1 = bit_rows(flags.0.map(OutFlags::bits), flags.1.bits(), |bit| {
			name(&format!("{:?}", OutFlags::from_bits(bit)))
		});
		let rows2 = bit_rows(flags2.0.map(OutFlags2::bits), flags2.1.bits(), |bit| {
			name(&format!("{:?}", OutFlags2::from_bits(bit)))
		});
		let version_differs = version.0.is_some_and(|v| v != version.1);

		match self.format {
			Format::Json => {
				let set = |pipl: Option<u32>, setup: u32, rows: &[BitRow]| {
					json!({
						"pipl": pipl,
						"global_setup": setup,
						"differ": rows.iter().filter(|r| r.differs()).map(|r| r.name.clone()).collect::<Vec<_>>(),
					})
				};
				self.json.insert(
					"flags".into(),
					json!({
						"version": {
							"pipl": version.0.map(|v| v.to_string()),
							"global_setup": version.1.to_string(),
							"differs": version_differs,
						},
						"out_flags": set(flags.0.map(OutFlags::bits), flags.1.bits(), &rows1),
						"out_flags2": set(flags2.0.map(OutFlags2::bits), flags2.1.bits(), &rows2),
					}),
				);
			}
			Format::Human => {
				let width = rows1
					.iter()
					.chain(&rows2)
					.map(|r| r.name.len() + 2)
					.chain([10])
					.max()
					.unwrap_or(10);
				let column = |v: Option<String>| v.unwrap_or_else(|| "-".into());
				let pipl_version = column(version.0.map(|v| v.to_string()));
				let cell = pipl_version.len().max(10);
				let mark = |differs: bool| if differs { "  differs" } else { "" };
				let mut lines = vec![format!("{:<width$}  {:<cell$}  GLOBAL_SETUP", "", "PiPL")];
				let differences =
					usize::from(version_differs) + rows1.iter().chain(&rows2).filter(|r| r.differs()).count();
				lines.push(format!(
					"{:<width$}  {pipl_version:<cell$}  {}{}",
					"version",
					version.1,
					mark(version_differs)
				));
				for (label, pipl, setup, rows) in [
					("out_flags", flags.0.map(OutFlags::bits), flags.1.bits(), &rows1),
					("out_flags2", flags2.0.map(OutFlags2::bits), flags2.1.bits(), &rows2),
				] {
					lines.push(format!(
						"{label:<width$}  {:<cell$}  {setup:#010x}{}",
						column(pipl.map(|bits| format!("{bits:#010x}"))),
						mark(pipl.is_some_and(|bits| bits != setup))
					));
					for row in rows {
						let yes = |on: bool| if on { "yes" } else { "no" };
						lines.push(format!(
							"  {:<w$}  {:<cell$}  {}{}",
							row.name,
							row.pipl.map_or("-", yes),
							yes(row.setup),
							mark(row.differs()),
							w = width - 2
						));
					}
				}
				lines.push(String::new());
				lines.push(match (pipl.is_some(), differences) {
					(false, _) => "no PiPL to compare with".into(),
					(true, 0) => "GLOBAL_SETUP matches the PiPL".into(),
					(true, n) => {
						format!("{n} difference(s): After Effects warns when GLOBAL_SETUP and the PiPL disagree")
					}
				});
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
		if self.format == Format::Json {
			println!("{}", Value::Object(self.json));
		}
	}
}

/// One bit of out_flags or out_flags2, set in the PiPL, at GLOBAL_SETUP or
/// both.
struct BitRow {
	name: String,
	/// `None` when there is no PiPL value to compare with.
	pipl: Option<bool>,
	setup: bool,
}

impl BitRow {
	fn differs(&self) -> bool {
		self.pipl.is_some_and(|pipl| pipl != self.setup)
	}
}

fn bit_rows(pipl: Option<u32>, setup: u32, name: impl Fn(u32) -> String) -> Vec<BitRow> {
	let union = pipl.unwrap_or(0) | setup;
	(0..32)
		.map(|i| 1u32 << i)
		.filter(|bit| union & bit != 0)
		.map(|bit| BitRow {
			name: name(bit),
			pipl: pipl.map(|bits| bits & bit != 0),
			setup: setup & bit != 0,
		})
		.collect()
}

/// `OutFlags(DEEP_COLOR_AWARE)` -> `DEEP_COLOR_AWARE`.
fn name(debug: &str) -> String {
	debug
		.split_once('(')
		.map_or(debug, |(_, rest)| rest.trim_end_matches(')'))
		.to_string()
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
	fn bit_rows_mark_bits_set_on_one_side() {
		let rows = bit_rows(Some(0b0110), 0b0011, |bit| bit.to_string());
		let summary: Vec<_> = rows.iter().map(|r| (r.name.as_str(), r.differs())).collect();
		assert_eq!(summary, [("1", true), ("2", false), ("4", true)]);
		assert!(bit_rows(None, 0b1, |bit| bit.to_string()).iter().all(|r| !r.differs()));
	}

	#[test]
	fn names_drop_the_type() {
		assert_eq!(name("OutFlags(DEEP_COLOR_AWARE)"), "DEEP_COLOR_AWARE");
		assert_eq!(name("OutFlags2(0x40)"), "0x40");
	}
}
