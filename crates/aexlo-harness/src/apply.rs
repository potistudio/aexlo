//! Putting a [`Variant`] on a [`PluginInstance`]: input, size, time,
//! parameters and layers (§4), and rendering through its render path.

use aexlo::{AexloError, ParamKind, ParamValue, PixelDepthKind, PluginInstance};
use toml::Value;

use crate::error::{Error, Result};
use crate::input;
use crate::preset::{Iterate, RenderMode, Variant};

/// Why `fx` cannot run `variant`, if it cannot (§4.8): such variants are
/// skipped, not failed.
pub fn unsupported(fx: &PluginInstance, variant: &Variant) -> Option<String> {
	let depth = variant.depth_kind();
	if !fx.supports_depth(depth) {
		let flag = match depth {
			PixelDepthKind::U16 => "PF_OutFlag_DEEP_COLOR_AWARE",
			_ => "PF_OutFlag2_FLOAT_COLOR_AWARE",
		};
		return Some(format!("the plugin does not declare {depth} ({flag})"));
	}
	match variant.render {
		RenderMode::Gpu if !fx.supports_gpu() => Some("the plugin cannot render on the GPU here".to_string()),
		RenderMode::Smart if !fx.supports_smart_render() => {
			Some("the plugin does not declare PF_OutFlag2_SUPPORTS_SMART_RENDER".to_string())
		}
		_ => None,
	}
}

/// Configure `fx` for `variant`: input layer at the variant's depth, render
/// size, time, parameters, layer parameters and iteration, then one
/// `PF_Cmd_UPDATE_PARAMS_UI` (as `aexlo render --set` does).
///
/// # Errors
/// [`ErrorKind::Invalid`](crate::ErrorKind::Invalid) for parameters the
/// plugin does not have or values of the wrong kind.
pub fn configure(fx: &mut PluginInstance, variant: &Variant) -> Result<()> {
	let depth = variant.depth_kind();
	let size = variant.size.map(|[w, h]| (w, h));
	let layer = input::build(&variant.input, depth, size)?;
	let (in_w, in_h) = (layer.width(), layer.height());
	set_input(fx, layer);
	let (w, h) = size.unwrap_or((in_w, in_h));
	fx.set_render_size(w, h);

	let (current, step, scale) = variant.time.as_ae()?;
	fx.set_time(current, step, scale);

	for (key, value) in &variant.params {
		let index = resolve_param(fx, key)?;
		let value = param_value(fx, index, value).map_err(|e| e.context(format!("param '{key}'")))?;
		fx.set_param(index, value)
			.map_err(|e| Error::invalid(format!("param '{key}': {e}")))?;
	}

	for (key, spec) in &variant.layers {
		let index = resolve_param(fx, key)?;
		if fx.param_kind(index) != Some(ParamKind::Layer) {
			return Err(Error::invalid(format!(
				"layer '{key}': parameter #{index} is not a layer"
			)));
		}
		let layer = input::build(spec, depth, Some((w, h))).map_err(|e| e.context(format!("layer '{key}'")))?;
		set_layer_param(fx, index, layer)?;
	}

	fx.set_parallel_iterate(variant.iterate == Iterate::Parallel);
	if !variant.params.is_empty() {
		// Cosmetic for the plugin, but some only settle dependent state here.
		let _ = fx.update_params_ui();
	}
	Ok(())
}

fn set_input(fx: &mut PluginInstance, layer: aexlo::AnyLayer) {
	match layer {
		aexlo::AnyLayer::U8(l) => fx.set_input_layer(l),
		aexlo::AnyLayer::U16(l) => fx.set_input_layer(l),
		aexlo::AnyLayer::F32(l) => fx.set_input_layer(l),
	}
}

fn set_layer_param(fx: &mut PluginInstance, index: usize, layer: aexlo::AnyLayer) -> Result<()> {
	match layer {
		aexlo::AnyLayer::U8(l) => fx.set_layer_param(index, Some(l)),
		aexlo::AnyLayer::U16(l) => fx.set_layer_param(index, Some(l)),
		aexlo::AnyLayer::F32(l) => fx.set_layer_param(index, Some(l)),
	}
	.map_err(|e| Error::invalid(e.to_string()))
}

/// Render one frame through `mode`'s path, with no fallback except for
/// `auto` (which is `render_frame`'s own GPU → smart → legacy chain).
pub fn render(fx: &mut PluginInstance, mode: RenderMode) -> aexlo::Result<()> {
	match mode {
		RenderMode::Auto => fx.render_frame(),
		RenderMode::Legacy => fx.render(),
		RenderMode::Smart => fx.render_pre().and_then(|()| fx.render_smart()),
		RenderMode::Gpu => fx.render_gpu(),
	}
}

/// Whether a render error means the variant does not apply rather than
/// that the plugin failed (a GPU frame the plugin declined).
pub fn is_skip(err: &AexloError) -> bool {
	matches!(err, AexloError::GpuRenderDeclined)
}

/// The parameter index `key` names (§4.6): `#24` by index, otherwise the
/// declared name, case-insensitively. A name shared by several parameters is
/// ambiguous and an error listing the candidates.
pub fn resolve_param(fx: &PluginInstance, key: &str) -> Result<usize> {
	if let Some(index) = key.strip_prefix('#') {
		let index: usize = index
			.trim()
			.parse()
			.map_err(|_| Error::invalid(format!("'{key}' is not a parameter index")))?;
		if index == 0 || index >= fx.param_count() {
			return Err(Error::invalid(format!(
				"parameter {key} does not exist (the plugin has #1..#{})",
				fx.param_count().saturating_sub(1)
			)));
		}
		return Ok(index);
	}
	match fx.param_indices(key)[..] {
		[index] => Ok(index),
		[] => {
			let names: Vec<String> = (1..fx.param_count())
				.filter_map(|i| fx.param_name(i).filter(|n| !n.is_empty()))
				.collect();
			Err(Error::invalid(format!(
				"the plugin has no parameter named '{key}' (it has: {})",
				names.join(", ")
			)))
		}
		ref several => Err(Error::invalid(format!(
			"parameter name '{key}' is ambiguous: it matches {}; use one of {}",
			several.iter().map(|i| format!("#{i}")).collect::<Vec<_>>().join(", "),
			several
				.iter()
				.map(|i| format!("\"#{i}\""))
				.collect::<Vec<_>>()
				.join(", "),
		))),
	}
}

fn number(value: &Value) -> Option<f64> {
	match value {
		Value::Integer(i) => Some(*i as f64),
		Value::Float(f) => Some(*f),
		_ => None,
	}
}

fn integer(value: &Value) -> Option<i64> {
	match value {
		Value::Integer(i) => Some(*i),
		Value::Float(f) if f.fract() == 0.0 => Some(*f as i64),
		_ => None,
	}
}

fn numbers(value: &Value) -> Option<Vec<f64>> {
	value.as_array()?.iter().map(number).collect()
}

/// Interpret a TOML `value` for parameter `index` by its kind (§4.6).
pub fn param_value(fx: &PluginInstance, index: usize, value: &Value) -> Result<ParamValue> {
	let kind = fx
		.param_kind(index)
		.ok_or_else(|| Error::invalid(format!("no parameter #{index}")))?;
	let wrong = |expected: &str| Error::invalid(format!("{kind:?} parameter expects {expected}, got {value}"));
	Ok(match kind {
		ParamKind::FloatSlider => ParamValue::Float(number(value).ok_or_else(|| wrong("a number"))?),
		ParamKind::FixedSlider => ParamValue::Fixed(number(value).ok_or_else(|| wrong("a number"))? as f32),
		ParamKind::Angle => ParamValue::Angle(number(value).ok_or_else(|| wrong("a number of degrees"))? as f32),
		ParamKind::Slider => {
			let v = integer(value).ok_or_else(|| wrong("an integer"))?;
			ParamValue::Slider(i32::try_from(v).map_err(|_| wrong("an integer in range"))?)
		}
		ParamKind::Checkbox => ParamValue::Checkbox(value.as_bool().ok_or_else(|| wrong("true or false"))?),
		ParamKind::Popup => match value {
			Value::String(label) => {
				let choices = fx.param_choices(index).unwrap_or_default();
				let position = choices
					.iter()
					.position(|c| c.trim().eq_ignore_ascii_case(label.trim()))
					.ok_or_else(|| {
						Error::invalid(format!(
							"popup has no choice '{label}' (choices: {})",
							choices.join(", ")
						))
					})?;
				ParamValue::Popup(position as i32 + 1)
			}
			_ => {
				let v = integer(value).ok_or_else(|| wrong("a 1-based choice or a choice label"))?;
				let count = fx.param_choices(index).map_or(i64::MAX, |c| c.len() as i64);
				if v < 1 || v > count {
					return Err(Error::invalid(format!("popup choice {v} is out of 1..={count}")));
				}
				ParamValue::Popup(v as i32)
			}
		},
		ParamKind::Point => match numbers(value).as_deref() {
			Some([x, y]) => ParamValue::Point {
				x: *x as f32,
				y: *y as f32,
			},
			_ => return Err(wrong("[x, y] in pixels")),
		},
		ParamKind::Point3D => match numbers(value).as_deref() {
			Some([x, y, z]) => ParamValue::Point3D { x: *x, y: *y, z: *z },
			_ => return Err(wrong("[x, y, z]")),
		},
		ParamKind::Color => {
			let channels = numbers(value).filter(|c| (3..=4).contains(&c.len()));
			let channels = channels.ok_or_else(|| wrong("[r, g, b] or [r, g, b, a], normalized"))?;
			let to8 = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
			ParamValue::Color {
				red: to8(channels[0]),
				green: to8(channels[1]),
				blue: to8(channels[2]),
				alpha: channels.get(3).copied().map_or(255, to8),
			}
		}
		ParamKind::Path => {
			let v = integer(value).ok_or_else(|| wrong("a mask id"))?;
			ParamValue::Path(u32::try_from(v).map_err(|_| wrong("a mask id"))?)
		}
		ParamKind::Layer => {
			return Err(Error::invalid(
				"layer parameters are set through `layers`, not `params`".to_string(),
			));
		}
		other => return Err(Error::invalid(format!("{other:?} parameters take no value"))),
	})
}
