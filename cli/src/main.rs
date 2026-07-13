//! `aexlo` — a command-line front-end for the aexlo plugin loader.
//!
//! Load a real After Effects plugin (`.plugin` bundle on macOS, `.aex`/`.dll`
//! on Windows) outside of After Effects, inspect it, and render frames to PNG.
//!
//! ```text
//! aexlo render <plugin> [--input in.png] [--output out.png] [--set 3=0.5 ...]
//! aexlo about  <plugin>
//! aexlo params <plugin>
//! ```

mod view;
mod watch;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use aexlo::{Depth8, Layer, ParamValue, PluginInstance};
use anyhow::{Context, Result, bail};

const USAGE: &str = "\
aexlo — run After Effects plugins without After Effects

USAGE:
    aexlo <COMMAND> <plugin> [OPTIONS]

COMMANDS:
    render <plugin>		Render a frame and write it to a PNG
    about  <plugin>		Print the plugin's ABOUT text
    params <plugin>    List the plugin's parameters (index, name, value)
    watch  <crate>     Live-preview a plugin crate: rebuild + render on save
                       (add --once for a single headless render to PNG)
    view   <png>       Live image window: reload a PNG whenever it changes
                       (pair with `bacon` re-running a #[aexlo::preview])

RENDER OPTIONS:
    -i, --input  <png>     Feed a PNG as the effect's input layer
    -o, --output <png>     Where to write the rendered frame  [default: out.png]
    -s, --set    <i>=<v>   Set parameter #<i> to <v> before rendering (repeatable)
        --smart            Force the smart-render path
        --legacy           Force the legacy render path

<plugin> is a path to the plugin artifact. A bare name (no separator) is also
tried with the platform's extension, e.g. `SDK_Noise` -> `SDK_Noise.plugin`.
";

fn main() -> ExitCode {
	match run() {
		Ok(()) => ExitCode::SUCCESS,
		Err(err) => {
			eprintln!("error: {err:#}");
			ExitCode::FAILURE
		}
	}
}

fn run() -> Result<()> {
	let mut args = std::env::args().skip(1);
	let Some(command) = args.next() else {
		print!("{USAGE}");
		bail!("no command given");
	};

	match command.as_str() {
		"-h" | "--help" | "help" => {
			print!("{USAGE}");
			Ok(())
		}
		"about" => cmd_about(args),
		"params" => cmd_params(args),
		"render" => cmd_render(args),
		"watch" => cmd_watch(args),
		"view" => cmd_view(args),
		other => {
			print!("{USAGE}");
			bail!("unknown command '{other}'");
		}
	}
}

/// Resolve a user-supplied plugin argument to a loadable path.
///
/// The argument is used as-is if it exists on disk; otherwise, when it looks
/// like a bare name (no path separator), the platform's plugin extension is
/// appended so `aexlo render SDK_Noise` finds `SDK_Noise.plugin`.
fn resolve_plugin(arg: &str) -> PathBuf {
	let direct = PathBuf::from(arg);
	if direct.exists() {
		return direct;
	}
	if !arg.contains(std::path::MAIN_SEPARATOR) {
		let ext = if cfg!(target_os = "windows") { "aex" } else { "plugin" };
		let candidate = PathBuf::from(format!("{arg}.{ext}"));
		if candidate.exists() {
			return candidate;
		}
	}
	direct
}

fn load(plugin_arg: &str) -> Result<PluginInstance> {
	let path = resolve_plugin(plugin_arg);
	PluginInstance::try_load(&path).with_context(|| format!("loading plugin {}", path.display()))
}

fn cmd_about(mut args: impl Iterator<Item = String>) -> Result<()> {
	let plugin = args.next().context("about: missing <plugin>")?;
	let mut instance = load(&plugin)?;
	let text = instance.about().context("plugin rejected PF_Cmd_ABOUT")?;
	println!("{}", text.trim());
	Ok(())
}

fn cmd_params(mut args: impl Iterator<Item = String>) -> Result<()> {
	let plugin = args.next().context("params: missing <plugin>")?;
	let instance = load(&plugin)?;
	let values = instance.param_values();
	if values.is_empty() {
		println!("(plugin declares no parameters)");
		return Ok(());
	}
	for (index, value) in values {
		println!("{index:>3}  {:<24}  {}", param_name(&instance, index), describe(&value));
	}
	Ok(())
}

fn cmd_render(args: impl Iterator<Item = String>) -> Result<()> {
	let mut plugin: Option<String> = None;
	let mut input: Option<PathBuf> = None;
	let mut output = PathBuf::from("out.png");
	let mut sets: Vec<(usize, String)> = Vec::new();
	let mut force_smart = false;
	let mut force_legacy = false;

	let mut args = args.peekable();
	while let Some(arg) = args.next() {
		match arg.as_str() {
			"-i" | "--input" => input = Some(PathBuf::from(next_value(&mut args, &arg)?)),
			"-o" | "--output" => output = PathBuf::from(next_value(&mut args, &arg)?),
			"-s" | "--set" => sets.push(parse_set(&next_value(&mut args, &arg)?)?),
			"--smart" => force_smart = true,
			"--legacy" => force_legacy = true,
			other if other.starts_with('-') => bail!("unknown option '{other}'"),
			_ => {
				if plugin.replace(arg).is_some() {
					bail!("render: expected a single <plugin>");
				}
			}
		}
	}

	if force_smart && force_legacy {
		bail!("--smart and --legacy are mutually exclusive");
	}
	let plugin = plugin.context("render: missing <plugin>")?;
	let mut instance = load(&plugin)?;

	if let Some(path) = &input {
		let (bytes, w, h) = load_input(path)?;
		let layer = Layer::<Depth8>::from_raw(bytes, w, h).map_err(|e| anyhow::anyhow!("building input layer: {e}"))?;
		instance.set_input(layer);
	}

	for (index, raw) in &sets {
		let value = parse_param_value(&instance, *index, raw)?;
		instance
			.set_param(*index, value)
			.with_context(|| format!("setting parameter #{index}"))?;
	}
	if !sets.is_empty() {
		let _ = instance.update_params_ui();
	}

	if force_smart {
		instance.render_smart().context("smart render failed")?;
	} else if force_legacy {
		instance.render().context("legacy render failed")?;
	} else {
		instance.render_frame().context("render failed")?;
	}

	let (w, h) = instance.output_size();
	let mut pixels = vec![0u8; w as usize * h as usize * 4];
	instance
		.write_output_rgba(&mut pixels)
		.context("reading rendered output")?;

	image::save_buffer(&output, &pixels, w, h, image::ColorType::Rgba8)
		.with_context(|| format!("writing {}", output.display()))?;

	println!("rendered {}x{} -> {}", w, h, output.display());
	Ok(())
}

fn cmd_watch(args: impl Iterator<Item = String>) -> Result<()> {
	let mut dir: Option<String> = None;
	let mut once = false;
	for arg in args {
		match arg.as_str() {
			"--once" => once = true,
			other if other.starts_with('-') => bail!("unknown option '{other}'"),
			_ => {
				if dir.replace(arg).is_some() {
					bail!("watch: expected a single <crate> directory");
				}
			}
		}
	}
	let dir = dir.context("watch: missing <crate> directory")?;
	if once {
		watch::render_once(Path::new(&dir))
	} else {
		watch::run(Path::new(&dir))
	}
}

fn cmd_view(mut args: impl Iterator<Item = String>) -> Result<()> {
	let path = args.next().context("view: missing <png> path")?;
	view::run(Path::new(&path))
}

/// Pull the value that follows a flag like `--output`, erroring if it's missing.
fn next_value(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String> {
	args.next().with_context(|| format!("option '{flag}' needs a value"))
}

/// Split a `--set` argument of the form `<index>=<value>`.
fn parse_set(raw: &str) -> Result<(usize, String)> {
	let (idx, value) = raw
		.split_once('=')
		.with_context(|| format!("--set expects <index>=<value>, got '{raw}'"))?;
	let index: usize = idx
		.trim()
		.parse()
		.with_context(|| format!("invalid parameter index '{idx}'"))?;
	Ok((index, value.to_string()))
}

/// Parse a textual value into the `ParamValue` variant the plugin already uses
/// at `index`, so callers don't have to spell out the parameter type.
fn parse_param_value(instance: &PluginInstance, index: usize, raw: &str) -> Result<ParamValue> {
	let current = instance
		.get_param(index)
		.with_context(|| format!("no parameter at index {index}"))?;
	let value = match current {
		ParamValue::Float(_) => ParamValue::Float(raw.parse().with_context(|| bad(raw, "a number"))?),
		ParamValue::Fixed(_) => ParamValue::Fixed(raw.parse().with_context(|| bad(raw, "a number"))?),
		ParamValue::Slider(_) => ParamValue::Slider(raw.parse().with_context(|| bad(raw, "an integer"))?),
		ParamValue::Popup(_) => ParamValue::Popup(raw.parse().with_context(|| bad(raw, "an integer"))?),
		ParamValue::Angle(_) => ParamValue::Angle(raw.parse().with_context(|| bad(raw, "a number"))?),
		ParamValue::Checkbox(_) => ParamValue::Checkbox(parse_bool(raw)?),
		ParamValue::Point { .. } => {
			let (x, y) = raw.split_once(',').with_context(|| bad(raw, "'x,y'"))?;
			ParamValue::Point {
				x: x.trim().parse().with_context(|| bad(raw, "'x,y'"))?,
				y: y.trim().parse().with_context(|| bad(raw, "'x,y'"))?,
			}
		}
		ParamValue::Color { .. } => parse_color(raw)?,
	};
	Ok(value)
}

fn parse_bool(raw: &str) -> Result<bool> {
	match raw.trim().to_ascii_lowercase().as_str() {
		"1" | "true" | "on" | "yes" => Ok(true),
		"0" | "false" | "off" | "no" => Ok(false),
		_ => bail!("expected a boolean (true/false), got '{raw}'"),
	}
}

/// Parse an `r,g,b` or `r,g,b,a` color (0-255 per channel).
fn parse_color(raw: &str) -> Result<ParamValue> {
	let parts: Vec<&str> = raw.split(',').map(str::trim).collect();
	if parts.len() != 3 && parts.len() != 4 {
		bail!("color expects 'r,g,b' or 'r,g,b,a', got '{raw}'");
	}
	let channel = |s: &str| -> Result<u8> { s.parse().with_context(|| bad(raw, "0-255 channels")) };
	Ok(ParamValue::Color {
		red: channel(parts[0])?,
		green: channel(parts[1])?,
		blue: channel(parts[2])?,
		alpha: if parts.len() == 4 { channel(parts[3])? } else { 255 },
	})
}

fn bad(raw: &str, expected: &str) -> String {
	format!("'{raw}' is not {expected}")
}

/// Load a PNG as an RGBA8 buffer plus its dimensions.
fn load_input(path: &Path) -> Result<(Vec<u8>, u32, u32)> {
	let img = image::open(path)
		.with_context(|| format!("opening input {}", path.display()))?
		.to_rgba8();
	let (w, h) = img.dimensions();
	Ok((img.into_raw(), w, h))
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
	}
}
