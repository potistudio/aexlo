//! Parameters as text: the `<param>=<value>` of `--set`, which parameter a
//! key names, and parsing a value into the kind the plugin declared.

use aexlo::{ParamValue, PluginInstance};
use anyhow::{Context, Result, bail};

/// Split a `--set` argument of the form `<name|index>=<value>`.
pub fn parse_set(raw: &str) -> Result<(String, String)> {
	let (key, value) = raw
		.split_once('=')
		.with_context(|| format!("--set expects <name|index>=<value>, got '{raw}'"))?;
	if key.trim().is_empty() {
		bail!("--set: empty parameter name in '{raw}'");
	}
	Ok((key.trim().to_string(), value.to_string()))
}

/// A `--set` key: a plain index (`3`), `#3`, or a declared name (exactly,
/// or else ignoring case when that picks out one parameter).
pub fn resolve_key(instance: &PluginInstance, key: &str) -> Result<usize> {
	let key = key.trim();
	if let Ok(index) = key.parse::<usize>() {
		return Ok(index);
	}
	let exact = aexlo_harness::apply::resolve_param(instance, key).map_err(anyhow::Error::msg);
	if exact.is_ok() || key.starts_with('#') {
		return exact;
	}
	let folded: Vec<usize> = (1..instance.param_count())
		.filter(|&i| {
			instance
				.param_name(i)
				.is_some_and(|n| n.trim().eq_ignore_ascii_case(key))
		})
		.collect();
	match folded[..] {
		[index] => Ok(index),
		_ => exact,
	}
}

/// Parse a textual value into the `ParamValue` variant the plugin already uses
/// at `index`, so callers don't have to spell out the parameter type.
pub fn parse_value(instance: &PluginInstance, index: usize, raw: &str) -> Result<ParamValue> {
	let current = instance
		.get_param(index)
		.with_context(|| format!("no parameter at index {index}"))?;
	let value = match current {
		ParamValue::Float(_) => ParamValue::Float(raw.parse().with_context(|| bad(raw, "a number"))?),
		ParamValue::Fixed(_) => ParamValue::Fixed(raw.parse().with_context(|| bad(raw, "a number"))?),
		ParamValue::Slider(_) => ParamValue::Slider(raw.parse().with_context(|| bad(raw, "an integer"))?),
		ParamValue::Popup(_) => ParamValue::Popup(raw.parse().with_context(|| bad(raw, "an integer"))?),
		ParamValue::Angle(_) => {
			let degrees = raw.trim().trim_end_matches('°').trim_end_matches("deg").trim();
			ParamValue::Angle(degrees.parse().with_context(|| bad(raw, "a number of degrees"))?)
		}
		ParamValue::Checkbox(_) => ParamValue::Checkbox(parse_bool(raw)?),
		ParamValue::Point { .. } => {
			let (x, y) = raw.split_once(',').with_context(|| bad(raw, "'x,y'"))?;
			ParamValue::Point {
				x: x.trim().parse().with_context(|| bad(raw, "'x,y'"))?,
				y: y.trim().parse().with_context(|| bad(raw, "'x,y'"))?,
			}
		}
		ParamValue::Color { .. } => parse_color(raw)?,
		ParamValue::Path(_) => ParamValue::Path(raw.parse().with_context(|| bad(raw, "a mask id"))?),
		ParamValue::Point3D { .. } => {
			let parts: Vec<f64> = raw
				.split(',')
				.map(|v| v.trim().parse())
				.collect::<Result<_, _>>()
				.with_context(|| bad(raw, "'x,y,z'"))?;
			let [x, y, z] = parts[..] else {
				bail!("{}", bad(raw, "'x,y,z'"));
			};
			ParamValue::Point3D { x, y, z }
		}
		other => bail!("parameter {index} ({:?}) can't be set from text", other.kind()),
	};
	Ok(value)
}

fn parse_bool(raw: &str) -> Result<bool> {
	match raw.trim().to_ascii_lowercase().as_str() {
		"true" | "on" | "yes" | "1" => Ok(true),
		"false" | "off" | "no" | "0" => Ok(false),
		_ => bail!("expected a boolean (true/false, on/off, 1/0), got '{raw}'"),
	}
}

/// Parse an `r,g,b` or `r,g,b,a` color (0-255 per channel), or `#rrggbb`
/// / `#rrggbbaa`.
fn parse_color(raw: &str) -> Result<ParamValue> {
	if let Some(hex) = raw.trim().strip_prefix('#') {
		let channel = |i: usize| -> Result<u8> {
			hex.get(i..i + 2)
				.and_then(|h| u8::from_str_radix(h, 16).ok())
				.with_context(|| bad(raw, "'#rrggbb' or '#rrggbbaa'"))
		};
		if hex.len() != 6 && hex.len() != 8 {
			bail!("{}", bad(raw, "'#rrggbb' or '#rrggbbaa'"));
		}
		return Ok(ParamValue::Color {
			red: channel(0)?,
			green: channel(2)?,
			blue: channel(4)?,
			alpha: if hex.len() == 8 { channel(6)? } else { 255 },
		});
	}
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
