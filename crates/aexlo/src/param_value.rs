//! Parameter value type for reading and writing plugin parameters.

use after_effects::ParamType;

/// A parameter value for an After Effects plugin.
#[derive(Debug, Clone, PartialEq)]
pub enum ParamValue {
	/// `PF_Param_FLOAT_SLIDER` - a floating-point slider value.
	Float(f64),

	/// `PF_Param_FIX_SLIDER` - a fixed-point slider value (surfaced as `f32`).
	Fixed(f32),

	/// `PF_Param_SLIDER` - an integer slider value.
	Slider(i32),

	/// `PF_Param_CHECKBOX` - a boolean toggle.
	Checkbox(bool),

	/// `PF_Param_POPUP` - the selected 1-based choice index.
	Popup(i32),

	/// `PF_Param_ANGLE` - an angle in degrees.
	Angle(f32),

	/// `PF_Param_POINT` - a 2D point, in pixels.
	Point { x: f32, y: f32 },

	/// `PF_Param_COLOR` - an 8-bit RGBA color.
	Color { red: u8, green: u8, blue: u8, alpha: u8 },
}

impl ParamValue {
	/// The `PF_ParamType` this value must be written to, plus a human-readable name
	/// for error reporting. Used to guard union writes in
	/// [`PluginInstance::set_param`].
	pub(crate) fn expected_param_type(&self) -> (ParamType, &'static str) {
		match self {
			ParamValue::Float(_) => (ParamType::FloatSlider, "FloatSlider"),
			ParamValue::Fixed(_) => (ParamType::FixSlider, "FixSlider"),
			ParamValue::Slider(_) => (ParamType::Slider, "Slider"),
			ParamValue::Checkbox(_) => (ParamType::CheckBox, "Checkbox"),
			ParamValue::Popup(_) => (ParamType::PopUp, "Popup"),
			ParamValue::Angle(_) => (ParamType::Angle, "Angle"),
			ParamValue::Point { .. } => (ParamType::Point, "Point"),
			ParamValue::Color { .. } => (ParamType::Color, "Color"),
		}
	}
}
