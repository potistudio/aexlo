//! Reading and writing plugin parameters and their UI state.

use super::*;

impl PluginInstance {
	/// Get the number of parameters.
	pub fn param_count(&self) -> usize {
		self.params.len()
	}

	/// Indices of the parameters whose declared name is `name`, compared
	/// case-insensitively and ignoring surrounding whitespace. Several
	/// parameters may share a name; index 0 (the input layer) never matches.
	pub fn param_indices(&self, name: &str) -> Vec<usize> {
		let name = name.trim();
		(1..self.params.len())
			.filter(|&i| {
				crate::host::params::param_name(&self.params[i])
					.trim()
					.eq_ignore_ascii_case(name)
			})
			.collect()
	}

	/// Set a parameter value by index.
	///
	/// Indices follow the plugin's own parameter order — the same space used by
	/// [`Self::get_param`] and [`Self::param_values`]: index 0 is the implicit
	/// input layer (not settable), real parameters start at 1.
	///
	/// # Errors
	/// [`AexloError::ParamIndexOutOfBounds`] for index 0 or past the last
	/// parameter, [`AexloError::ParamTypeMismatch`] if `value` is not of the
	/// parameter's [kind](Self::param_kind).
	pub fn set_param(&mut self, index: usize, value: ParamValue) -> Result<()> {
		if index == 0 || index >= self.params.len() {
			return Err(AexloError::ParamIndexOutOfBounds {
				index,
				max: self.params.len().saturating_sub(1),
			});
		}

		let target = &mut self.params[index];

		// Reject a value whose variant doesn't match the parameter's declared type,
		// so we never write the wrong union member.
		let expected = value.kind();
		let actual = crate::ParamKind::from_sdk(target.param_type);
		if actual != expected {
			return Err(AexloError::ParamTypeMismatch {
				index,
				expected,
				actual,
			});
		}

		// SAFETY: the union variant was verified against `param_type` above.
		match value {
			ParamValue::Float(v) => target.u.fs_d.value = v,
			// `PF_FixedSliderDef::value` is a `PF_Fixed` (16.16 fixed point), the
			// same encoding as ANGLE/POINT — not Q31.
			ParamValue::Fixed(v) => target.u.fd.value = utils::f32_to_fixed16(v),
			ParamValue::Slider(v) => target.u.sd.value = v,
			ParamValue::Checkbox(v) => target.u.bd.value = v as i32,
			ParamValue::Popup(v) => target.u.pd.value = v,
			ParamValue::Angle(deg) => target.u.ad.value = utils::f32_to_fixed16(deg),
			ParamValue::Point { x, y } => {
				target.u.td.x_value = utils::f32_to_fixed16(x);
				target.u.td.y_value = utils::f32_to_fixed16(y);
			}
			ParamValue::Color {
				red,
				green,
				blue,
				alpha,
			} => {
				target.u.cd.value = after_effects_sys::PF_Pixel {
					alpha,
					red,
					green,
					blue,
				};
			}
			ParamValue::Path(id) => target.u.path_d.path_id = id,
			ParamValue::Point3D { x, y, z } => {
				target.u.point3d_d.x_value = x;
				target.u.point3d_d.y_value = y;
				target.u.point3d_d.z_value = z;
			}
		}

		Ok(())
	}

	/// Notify the plugin that the user changed the parameter at `index`
	/// (`PF_Cmd_USER_CHANGED_PARAM`).
	///
	/// This is where an effect reacts to an edit — adjusting dependent parameters
	/// or requesting a UI refresh (typically by raising `PF_OutFlag_SEND_UPDATE_PARAMS_UI`,
	/// after which the host follows up with [`Self::update_params_ui`]).
	///
	/// # Errors
	/// [`AexloError::PluginExecutionFailed`] if the plugin rejects the command.
	pub fn user_changed_param(&mut self, index: usize) -> Result<()> {
		let mut extra = after_effects_sys::PF_UserChangedParamExtra {
			param_index: index as after_effects_sys::PF_ParamIndex,
		};
		self.call_plugin(
			RawCommand::UserChangedParam,
			(&mut extra as *mut after_effects_sys::PF_UserChangedParamExtra).cast(),
		)
	}

	/// Ask the plugin to refresh its parameter UI state (`PF_Cmd_UPDATE_PARAMS_UI`):
	/// showing, hiding, collapsing, or disabling controls via
	/// `PF_ParamUtilsSuite3::PF_UpdateParamUI`. Cosmetic only — the plugin must
	/// not change parameter values in response to this command.
	///
	/// # Errors
	/// [`AexloError::PluginExecutionFailed`] if the plugin rejects the command.
	pub fn update_params_ui(&mut self) -> Result<()> {
		self.call_plugin(RawCommand::UpdateParamsUi, null_mut())
	}

	/// Returns all parameter values as `(index, value)` pairs, using the same
	/// index space as [`Self::set_param`] / [`Self::get_param`].
	/// Index 0 (the input layer) and parameters with unknown types are excluded.
	pub fn param_values(&self) -> Vec<(usize, ParamValue)> {
		(1..self.params.len())
			.filter_map(|i| self.get_param(i).map(|v| (i, v)))
			.collect()
	}

	/// Get a parameter value by index (same index space as [`Self::set_param`]:
	/// index 0 is the input layer, real parameters start at 1).
	/// Returns `None` if the index is out of bounds or the param type is unknown
	/// (which includes index 0, the input layer).
	pub fn get_param(&self, index: usize) -> Option<ParamValue> {
		let param = self.params.get(index)?;

		// SAFETY: the union variant is selected based on `param_type`.
		unsafe {
			match param.param_type {
				t if t == ParamType::FloatSlider as PF_ParamType => Some(ParamValue::Float(param.u.fs_d.value)),
				t if t == ParamType::FixSlider as PF_ParamType => {
					Some(ParamValue::Fixed(utils::fixed16_to_f32(param.u.fd.value)))
				}
				t if t == ParamType::Slider as PF_ParamType => Some(ParamValue::Slider(param.u.sd.value)),
				t if t == ParamType::CheckBox as PF_ParamType => Some(ParamValue::Checkbox(param.u.bd.value != 0)),
				t if t == ParamType::PopUp as PF_ParamType => Some(ParamValue::Popup(param.u.pd.value)),
				t if t == ParamType::Angle as PF_ParamType => {
					Some(ParamValue::Angle(utils::fixed16_to_f32(param.u.ad.value)))
				}
				t if t == ParamType::Point as PF_ParamType => Some(ParamValue::Point {
					x: utils::fixed16_to_f32(param.u.td.x_value),
					y: utils::fixed16_to_f32(param.u.td.y_value),
				}),
				t if t == ParamType::Color as PF_ParamType => {
					let px = param.u.cd.value;
					Some(ParamValue::Color {
						red: px.red,
						green: px.green,
						blue: px.blue,
						alpha: px.alpha,
					})
				}
				t if t == ParamType::Path as PF_ParamType => Some(ParamValue::Path(param.u.path_d.path_id)),
				t if t == ParamType::Point3D as PF_ParamType => Some(ParamValue::Point3D {
					x: param.u.point3d_d.x_value,
					y: param.u.point3d_d.y_value,
					z: param.u.point3d_d.z_value,
				}),
				_ => None,
			}
		}
	}

	/// The parameter's displayed slider range `(min, max)`, in the same units as
	/// its [`ParamValue`], for the slider-like types (`Float`/`Fixed`/`Slider`).
	///
	/// Returns `None` for types that have no slider track (checkbox, popup,
	/// angle dial, point, color) or when the range is degenerate (`min == max`),
	/// so callers can fall back to a plain numeric input.
	pub fn param_slider_range(&self, index: usize) -> Option<(f64, f64)> {
		let param = self.params.get(index)?;

		// SAFETY: the union variant is selected based on `param_type`, matching
		// the reads in `get_param`.
		let (min, max) = unsafe {
			match param.param_type {
				t if t == ParamType::FloatSlider as PF_ParamType => {
					(param.u.fs_d.slider_min as f64, param.u.fs_d.slider_max as f64)
				}
				t if t == ParamType::Slider as PF_ParamType => {
					(param.u.sd.slider_min as f64, param.u.sd.slider_max as f64)
				}
				t if t == ParamType::FixSlider as PF_ParamType => (
					utils::fixed16_to_f32(param.u.fd.slider_min) as f64,
					utils::fixed16_to_f32(param.u.fd.slider_max) as f64,
				),
				_ => return None,
			}
		};
		(min != max).then_some((min, max))
	}

	/// The choice labels of a `Popup` parameter, in selection order (the 1-based
	/// [`ParamValue::Popup`] value indexes into this list).
	///
	/// Returns `None` for non-popup parameters or when the plugin supplied no
	/// choice string.
	pub fn param_choices(&self, index: usize) -> Option<Vec<String>> {
		let param = self.params.get(index)?;
		if param.param_type != ParamType::PopUp as PF_ParamType {
			return None;
		}
		// Read the labels captured at PARAMS_SETUP, not the now-dangling
		// `namesptr` (see `popup_choices`).
		self.popup_choices.get(index).filter(|c| !c.is_empty()).cloned()
	}

	// -----------------------------------------------------

	/// Add a parameter to this instance's parameter storage.
	///
	/// Crate-internal: this exists only to bridge `PF_Cmd_PARAMS_SETUP` -- the
	/// plugin owns its parameter list, the host never appends to it.
	pub(crate) fn add_instance_param(&mut self, param: PF_ParamDef) {
		// Capture popup choice labels now, while the plugin-owned `namesptr`
		// string is still alive (it dies when the add-param callback returns).
		let choices = if param.param_type == ParamType::PopUp as PF_ParamType {
			unsafe { crate::host::params::popup_options(&param.u.pd) }
		} else {
			Vec::new()
		};
		self.params.push(param);
		self.popup_choices.push(choices);
		self.params_dirty = true;
		log::debug!(
			"PluginInstance: added param #{} (type: {:?})",
			self.params.len(),
			param.param_type
		);
	}

	/// Get all instance parameters.
	pub(crate) fn params(&self) -> &[PF_ParamDef] {
		&self.params
	}

	/// Get a specific instance parameter by index (same index space as
	/// [`Self::set_param`]: index 0 is the input layer, real parameters start at 1).
	///
	/// Crate-internal because it exposes the raw `PF_ParamDef` sys type; the
	/// public surface is [`Self::get_param`] / [`Self::param_name`].
	pub(crate) fn param_by_index(&self, index: usize) -> Option<&PF_ParamDef> {
		self.params.get(index)
	}

	/// The plugin-declared display name of the parameter at `index` (same index
	/// space as [`Self::set_param`]), or `None` if the index is out of bounds.
	///
	/// The name is decoded from the plugin's null-terminated byte array and may
	/// be empty when the plugin left it blank.
	pub fn param_name(&self, index: usize) -> Option<String> {
		self.params.get(index).map(crate::host::params::param_name)
	}

	/// The declared type of the parameter at `index` (same index space as
	/// [`Self::set_param`]; index 0 is the input layer), or `None` if out of bounds.
	pub fn param_kind(&self, index: usize) -> Option<crate::ParamKind> {
		self.params.get(index).map(|p| crate::ParamKind::from_sdk(p.param_type))
	}

	/// Whether the plugin hid the parameter at `index` (`PF_PUI_INVISIBLE`),
	/// at setup or later through `PF_UpdateParamUI`.
	pub fn param_hidden(&self, index: usize) -> bool {
		self.params.get(index).is_some_and(|p| {
			p.ui_flags & after_effects_sys::PF_PUI_INVISIBLE as after_effects_sys::PF_ParamUIFlags != 0
		})
	}

	/// Whether the plugin disabled (greyed out) the parameter at `index`
	/// (`PF_PUI_DISABLED`).
	pub fn param_disabled(&self, index: usize) -> bool {
		self.params
			.get(index)
			.is_some_and(|p| p.ui_flags & after_effects_sys::PF_PUI_DISABLED as after_effects_sys::PF_ParamUIFlags != 0)
	}

	/// Apply a plugin's `PF_UpdateParamUI` request: copy the UI-only fields from
	/// `def` into the stored parameter at `index`, leaving its value untouched.
	///
	/// Plugins call this (through [`PF_ParamUtilsSuite3`](crate::suites)) during
	/// `PF_Cmd_UPDATE_PARAMS_UI`/`USER_CHANGED_PARAM` to toggle things like twirl
	/// collapse or disabled state without changing the parameter's value.
	pub(crate) fn update_param_ui(&mut self, index: usize, def: &PF_ParamDef) {
		if let Some(target) = self.params.get_mut(index) {
			target.flags = def.flags;
			target.ui_flags = def.ui_flags;
			target.ui_width = def.ui_width;
			target.ui_height = def.ui_height;
			target.name_do_not_use_directly = def.name_do_not_use_directly;
		}
	}

	/// Clear all instance parameters.
	///
	/// Crate-internal: see [`Self::add_instance_param`]. Currently unused --
	/// kept for the (re-)PARAMS_SETUP bridge described in AGENTS.md.
	#[allow(dead_code)]
	pub(crate) fn clear_instance_params(&mut self) {
		self.params.clear();
		self.popup_choices.clear();
		self.params_dirty = true;
		log::debug!("PluginInstance: cleared all instance params");
	}
}
