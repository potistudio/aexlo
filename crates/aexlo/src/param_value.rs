//! Parameter value type for reading and writing plugin parameters.

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
	Point {
		/// Horizontal position.
		x: f32,
		/// Vertical position.
		y: f32,
	},

	/// `PF_Param_COLOR` - an 8-bit RGBA color.
	Color {
		/// Red, `0..=255`.
		red: u8,
		/// Green, `0..=255`.
		green: u8,
		/// Blue, `0..=255`.
		blue: u8,
		/// Alpha, `0..=255`.
		alpha: u8,
	},

	/// `PF_Param_PATH` - the [`MaskPath::id`](crate::MaskPath::id) of the
	/// selected mask (`0` for none), resolved against
	/// [`PluginInstance::set_mask_paths`](crate::PluginInstance::set_mask_paths).
	Path(u32),

	/// `PF_Param_POINT_3D` - a 3D point, in pixels.
	Point3D {
		/// Horizontal position.
		x: f64,
		/// Vertical position.
		y: f64,
		/// Position along the comp's depth.
		z: f64,
	},
}

impl ParamValue {
	/// The parameter kind this value can be set on. Guards the union writes in
	/// [`PluginInstance::set_param`](crate::PluginInstance::set_param).
	pub fn kind(&self) -> ParamKind {
		match self {
			ParamValue::Float(_) => ParamKind::FloatSlider,
			ParamValue::Fixed(_) => ParamKind::FixedSlider,
			ParamValue::Slider(_) => ParamKind::Slider,
			ParamValue::Checkbox(_) => ParamKind::Checkbox,
			ParamValue::Popup(_) => ParamKind::Popup,
			ParamValue::Angle(_) => ParamKind::Angle,
			ParamValue::Point { .. } => ParamKind::Point,
			ParamValue::Color { .. } => ParamKind::Color,
			ParamValue::Path(_) => ParamKind::Path,
			ParamValue::Point3D { .. } => ParamKind::Point3D,
		}
	}
}

/// The declared type of a plugin parameter (`PF_ParamType`), including the
/// kinds [`ParamValue`] does not carry a value for (layers, groups, buttons,
/// arbitrary data, ...). See [`PluginInstance::param_kind`](crate::PluginInstance::param_kind).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ParamKind {
	/// `PF_Param_LAYER` - index 0 is the effect's own input layer; others are
	/// linked with [`PluginInstance::set_layer_param`](crate::PluginInstance::set_layer_param).
	Layer,
	/// `PF_Param_SLIDER` - an integer slider ([`ParamValue::Slider`]).
	Slider,
	/// `PF_Param_FIX_SLIDER` - a 16.16 fixed-point slider ([`ParamValue::Fixed`]).
	FixedSlider,
	/// `PF_Param_ANGLE` - an angle ([`ParamValue::Angle`]).
	Angle,
	/// `PF_Param_CHECKBOX` - a toggle ([`ParamValue::Checkbox`]).
	Checkbox,
	/// `PF_Param_COLOR` - an 8-bit color ([`ParamValue::Color`]).
	Color,
	/// `PF_Param_POINT` - a 2D point ([`ParamValue::Point`]).
	Point,
	/// `PF_Param_POPUP` - a choice list ([`ParamValue::Popup`]).
	Popup,
	/// `PF_Param_CUSTOM` - a parameter drawn by the plugin's own UI.
	Custom,
	/// `PF_Param_NO_DATA` - a parameter with a UI but no value.
	NoData,
	/// `PF_Param_FLOAT_SLIDER` - a floating-point slider ([`ParamValue::Float`]).
	FloatSlider,
	/// `PF_Param_ARBITRARY_DATA` - plugin-defined data the host stores opaquely.
	ArbitraryData,
	/// `PF_Param_PATH` - a mask selector ([`ParamValue::Path`]).
	Path,
	/// `PF_Param_GROUP_START` - the following parameters, up to the matching
	/// [`ParamKind::GroupEnd`], form a twirl-down group named after this one.
	GroupStart,
	/// `PF_Param_GROUP_END` - closes the group opened by the matching
	/// [`ParamKind::GroupStart`].
	GroupEnd,
	/// `PF_Param_BUTTON` - a push button; pressing it sends
	/// `PF_Cmd_USER_CHANGED_PARAM`.
	Button,
	/// `PF_Param_POINT_3D` - a 3D point ([`ParamValue::Point3D`]).
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
