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
		let actual = crate::ParamKind::from_sys(target.param_type);
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
		)?;
		self.follow_params_ui()
	}

	/// After Effects follows `USER_CHANGED_PARAM` and handled UI events with
	/// `PF_Cmd_UPDATE_PARAMS_UI` when the plugin raises
	/// `PF_OutFlag_SEND_UPDATE_PARAMS_UI`.
	fn follow_params_ui(&mut self) -> Result<()> {
		let flag = after_effects_sys::PF_OutFlag_SEND_UPDATE_PARAMS_UI as PF_OutFlags;
		if self.out_data.out_flags & flag == 0 {
			return Ok(());
		}
		self.out_data.out_flags &= !flag;
		self.update_params_ui()
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
				t if t == PF_Param_FLOAT_SLIDER as PF_ParamType => Some(ParamValue::Float(param.u.fs_d.value)),
				t if t == PF_Param_FIX_SLIDER as PF_ParamType => {
					Some(ParamValue::Fixed(utils::fixed16_to_f32(param.u.fd.value)))
				}
				t if t == PF_Param_SLIDER as PF_ParamType => Some(ParamValue::Slider(param.u.sd.value)),
				t if t == PF_Param_CHECKBOX as PF_ParamType => Some(ParamValue::Checkbox(param.u.bd.value != 0)),
				t if t == PF_Param_POPUP as PF_ParamType => Some(ParamValue::Popup(param.u.pd.value)),
				t if t == PF_Param_ANGLE as PF_ParamType => {
					Some(ParamValue::Angle(utils::fixed16_to_f32(param.u.ad.value)))
				}
				t if t == PF_Param_POINT as PF_ParamType => Some(ParamValue::Point {
					x: utils::fixed16_to_f32(param.u.td.x_value),
					y: utils::fixed16_to_f32(param.u.td.y_value),
				}),
				t if t == PF_Param_COLOR as PF_ParamType => {
					let px = param.u.cd.value;
					Some(ParamValue::Color {
						red: px.red,
						green: px.green,
						blue: px.blue,
						alpha: px.alpha,
					})
				}
				t if t == PF_Param_PATH as PF_ParamType => Some(ParamValue::Path(param.u.path_d.path_id)),
				t if t == PF_Param_POINT_3D as PF_ParamType => Some(ParamValue::Point3D {
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
				t if t == PF_Param_FLOAT_SLIDER as PF_ParamType => {
					(param.u.fs_d.slider_min as f64, param.u.fs_d.slider_max as f64)
				}
				t if t == PF_Param_SLIDER as PF_ParamType => {
					(param.u.sd.slider_min as f64, param.u.sd.slider_max as f64)
				}
				t if t == PF_Param_FIX_SLIDER as PF_ParamType => (
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
		if param.param_type != PF_Param_POPUP as PF_ParamType {
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
		let choices = if param.param_type == PF_Param_POPUP as PF_ParamType {
			// SAFETY: Only read for popup parameters, whose `u` holds `pd`.
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
	/// Whether this instance's arbitrary-data values exist yet (see
	/// [`Self::setup_arbitrary_params`]).
	pub(crate) fn arb_streams_live(&self) -> bool {
		self.arb_streams_live
	}

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
		self.params.get(index).map(|p| crate::ParamKind::from_sys(p.param_type))
	}

	/// UI flags the plugin set on the parameter (`PF_PUI_*`), or `None` if `index` is out of range.
	pub fn param_ui_flags(&self, index: usize) -> Option<crate::ParamUiFlags> {
		self.params
			.get(index)
			.map(|p| crate::ParamUiFlags::from_bits(p.ui_flags as u32))
	}

	/// Whether any parameter draws a custom control (`PF_PUI_CONTROL`).
	///
	/// After Effects opens that control from `PF_Cmd_EVENT`. The Options button
	/// is a separate `PF_Cmd_DO_DIALOG`.
	pub fn has_custom_ui(&self) -> bool {
		let control = after_effects_sys::PF_PUI_CONTROL as i32;
		self.params.iter().any(|param| param.ui_flags & control != 0)
	}

	/// Whether the parameter at `index` draws its own control (`PF_PUI_CONTROL`).
	pub fn param_has_custom_ui(&self, index: usize) -> bool {
		self.param_ui_flags(index)
			.is_some_and(|flags| flags.contains(crate::ParamUiFlags::CONTROL))
	}

	/// The size the custom control at `index` declared (`ui_width`,
	/// `ui_height`), with `None` for an edge left at 0, which After Effects
	/// fills from the Effect Controls panel.
	pub fn custom_control_size(&self, index: usize) -> Option<(Option<u32>, Option<u32>)> {
		let param = self.params.get(index)?;
		let edge = |value: i16| u32::try_from(value).ok().filter(|v| *v > 0);
		Some((edge(param.ui_width), edge(param.ui_height)))
	}

	/// Latest pixels painted by the custom control at `index`.
	pub fn custom_control_image(&self, index: usize) -> Option<&CustomControlImage> {
		self.custom_controls.get(&index)
	}

	/// Send `PF_Event_DRAW` for one custom control and keep the raster.
	///
	/// The returned flag is true when the plugin asked to be drawn again
	/// (`PF_EO_ALWAYS_UPDATE` or `PF_EO_UPDATE_NOW`).
	///
	/// # Errors
	/// Currently never fails; the plugin's own return code for the event is
	/// only logged.
	pub fn draw_custom_control(&mut self, index: usize, width: u32, height: u32) -> Result<bool> {
		let width = width.clamp(1, 2048) as i32;
		let height = height.clamp(1, 2048) as i32;
		self.prepare_custom_control(index, width, height);
		let (flags, _) = self.dispatch_effect_event(index, after_effects_sys::PF_Event_DRAW, width, height, None);
		Ok(wants_redraw(flags))
	}

	/// Send `PF_Event_DO_CLICK` at a point in the control's own coordinates.
	///
	/// This is the mouse going down. When [`CustomControlEvent::send_drag`] is
	/// set, follow it with [`Self::drag_custom_control`] through mouse-up.
	///
	/// # Errors
	/// [`AexloError::PluginExecutionFailed`] if the plugin rejects the
	/// parameter-UI update that follows the click.
	pub fn click_custom_control(
		&mut self,
		index: usize,
		x: i32,
		y: i32,
		width: u32,
		height: u32,
	) -> Result<CustomControlEvent> {
		let width = width.clamp(1, 2048) as i32;
		let height = height.clamp(1, 2048) as i32;
		self.prepare_custom_control(index, width, height);
		let (flags, send_drag) = self.dispatch_effect_event(
			index,
			after_effects_sys::PF_Event_DO_CLICK,
			width,
			height,
			// `last_time` belongs to drag samples. A click is the mouse going down.
			Some(ControlPointer { x, y, last: false }),
		);
		self.follow_params_ui()?;
		Ok(CustomControlEvent {
			redraw: wants_redraw(flags),
			send_drag,
		})
	}

	/// Send `PF_Event_DRAG` at a point in the control's own coordinates.
	///
	/// # Errors
	/// [`AexloError::PluginExecutionFailed`] if the plugin rejects the
	/// parameter-UI update that follows the drag.
	pub fn drag_custom_control(
		&mut self,
		index: usize,
		x: i32,
		y: i32,
		width: u32,
		height: u32,
		last: bool,
	) -> Result<CustomControlEvent> {
		let width = width.clamp(1, 2048) as i32;
		let height = height.clamp(1, 2048) as i32;
		self.prepare_custom_control(index, width, height);
		let (flags, send_drag) = self.dispatch_effect_event(
			index,
			after_effects_sys::PF_Event_DRAG,
			width,
			height,
			Some(ControlPointer { x, y, last }),
		);
		self.follow_params_ui()?;
		Ok(CustomControlEvent {
			redraw: wants_redraw(flags),
			send_drag,
		})
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
