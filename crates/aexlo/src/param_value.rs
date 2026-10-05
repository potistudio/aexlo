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

	/// `PF_Param_PATH` - the [`MaskPath::id`](crate::MaskPath::id) of the
	/// selected mask (`0` for none), resolved against
	/// [`PluginInstance::set_mask_paths`](crate::PluginInstance::set_mask_paths).
	Path(u32),

	/// `PF_Param_POINT_3D` - a 3D point, in pixels (`z` along the comp's depth).
	Point3D { x: f64, y: f64, z: f64 },
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
			ParamValue::Path(_) => (ParamType::Path, "Path"),
			ParamValue::Point3D { .. } => (ParamType::Point3D, "Point3D"),
		}
	}
}

/// The declared type of a plugin parameter (`PF_ParamType`), including the
/// kinds [`ParamValue`] does not carry a value for (layers, groups, buttons,
/// arbitrary data, ...). See [`PluginInstance::param_kind`](crate::PluginInstance::param_kind).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamKind {
	/// `PF_Param_LAYER` - index 0 is the effect's own input layer; others are
	/// linked with [`PluginInstance::set_layer_param`](crate::PluginInstance::set_layer_param).
	Layer,
	Slider,
	FixedSlider,
	Angle,
	Checkbox,
	Color,
	Point,
	Popup,
	Custom,
	NoData,
	FloatSlider,
	ArbitraryData,
	Path,
	/// `PF_Param_GROUP_START` - the following parameters, up to the matching
	/// [`ParamKind::GroupEnd`], form a twirl-down group named after this one.
	GroupStart,
	GroupEnd,
	Button,
	Point3D,
	/// Any other `PF_ParamType` value.
	Other(i32),
}

impl ParamKind {
	pub(crate) fn from_sdk(param_type: after_effects_sys::PF_ParamType) -> Self {
		use after_effects_sys::*;
		#[allow(non_upper_case_globals)]
		match param_type {
			PF_Param_LAYER => Self::Layer,
			PF_Param_SLIDER => Self::Slider,
			PF_Param_FIX_SLIDER => Self::FixedSlider,
			PF_Param_ANGLE => Self::Angle,
			PF_Param_CHECKBOX => Self::Checkbox,
			PF_Param_COLOR => Self::Color,
			PF_Param_POINT => Self::Point,
			PF_Param_POPUP => Self::Popup,
			PF_Param_CUSTOM => Self::Custom,
			PF_Param_NO_DATA => Self::NoData,
			PF_Param_FLOAT_SLIDER => Self::FloatSlider,
			PF_Param_ARBITRARY_DATA => Self::ArbitraryData,
			PF_Param_PATH => Self::Path,
			PF_Param_GROUP_START => Self::GroupStart,
			PF_Param_GROUP_END => Self::GroupEnd,
			PF_Param_BUTTON => Self::Button,
			PF_Param_POINT_3D => Self::Point3D,
			other => Self::Other(other),
		}
	}
}
