//! The input and output layers, layers linked to layer parameters, and masks.

use super::*;

impl PluginInstance {
	/// Replace the input layer.
	///
	/// The layer's depth is the depth of the whole render, as the project depth
	/// is in After Effects: when it differs from the current one the output is
	/// reallocated (black, same size) at the new depth, and smart render
	/// advertises it. Check [`Self::supports_depth`] first; a plugin handed a
	/// depth it never declared typically errors or renders garbage.
	///
	/// Point parameters still at their default are re-resolved against the new
	/// layer size (AE declares point defaults as layer percentages), so e.g. a
	/// "centre" stays centred; edited points keep their pixel values.
	pub fn set_input_layer<D: PixelDepth>(&mut self, input: Layer<D>) {
		let (old_w, old_h) = self.input_size();
		let depth_changed = D::KIND != self.output_layer.kind();
		self.input_layer = HostLayer::new(AnyLayer::new(input));
		let (new_w, new_h) = self.input_size();
		if (old_w, old_h) != (new_w, new_h) {
			for param in &mut self.params {
				if crate::host::params::point_at_default(param, old_w, old_h) {
					crate::host::params::resolve_point_default(param, new_w, new_h);
				}
			}
		}

		// In After Effects `in_data.width/height` (and `extent_hint`) are the
		// source layer's size, independent of the output world.
		self.render_context
			.set_size(self.input_layer.width() as i32, self.input_layer.height() as i32);

		// The GPU input buffer (if any) now holds stale pixels.
		self.gpu_input_uploaded = false;

		// Keep the persistent input world (used by smart-render `checkout_layer_pixels`)
		// pointing at the new layer's pixels.
		self.input_world = self.input_layer.as_sys();

		// Keep params[0] (`PF_Param_LAYER`) synchronized with the new input layer.
		if let Some(input_param) = self.params.get_mut(0) {
			input_param.u = PF_ParamDefUnion {
				ld: self.input_layer.as_sys(),
			};
		}

		if depth_changed {
			let (w, h) = self.output_size();
			self.output_layer = HostLayer::new(AnyLayer::filled(D::KIND, w, h, OPAQUE_BLACK));
			self.sync_output_world();
			self.smart_render_data.set_cpu_bitdepth(D::KIND.bits() as i16);
		}
	}

	/// Write output pixels directly to an RGBA buffer (zero-allocation).
	/// Deeper output is quantized to 8 bits per channel.
	///
	/// # Errors
	/// [`AexloError::Layer`] unless the buffer has exactly
	/// `width * height * 4` bytes ([`Self::output_size`]).
	pub fn write_output_rgba8(&self, buffer: &mut [u8]) -> Result<()> {
		self.output_layer.layer().write_rgba8(buffer)?;
		Ok(())
	}

	/// The depth of the input layer, which is the depth of the render.
	pub fn input_depth(&self) -> PixelDepthKind {
		self.input_layer.kind()
	}

	/// The depth of the output world. It follows the input layer's depth.
	pub fn output_depth(&self) -> PixelDepthKind {
		self.output_layer.kind()
	}

	/// The rendered output at whatever depth it is.
	pub fn output(&self) -> &AnyLayer {
		self.output_layer.layer()
	}

	/// A copy of the rendered output as a `Layer<D>`.
	///
	/// # Errors
	/// [`AexloError::DepthMismatch`] when `D` is not [`Self::output_depth`];
	/// convert with [`Layer::convert`] after reading it at its own depth.
	pub fn read_output<D: PixelDepth>(&self) -> Result<Layer<D>> {
		self.output_layer
			.layer()
			.get::<D>()
			.cloned()
			.ok_or(AexloError::DepthMismatch {
				requested: D::KIND,
				actual: self.output_layer.kind(),
			})
	}

	/// Whether the plugin declared it can render at `depth`: 8 bpc always,
	/// 16 bpc with `PF_OutFlag_DEEP_COLOR_AWARE`, 32 bpc with
	/// `PF_OutFlag2_FLOAT_COLOR_AWARE`.
	///
	/// Only meaningful once global setup has run (i.e. after [`Host::load`]).
	pub fn supports_depth(&self, depth: PixelDepthKind) -> bool {
		match depth {
			PixelDepthKind::U8 => true,
			PixelDepthKind::U16 => self.global_out_flags & crate::sys::PF_OutFlag_DEEP_COLOR_AWARE as PF_OutFlags != 0,
			PixelDepthKind::F32 => {
				self.global_out_flags2 & crate::sys::PF_OutFlag2_FLOAT_COLOR_AWARE as PF_OutFlags2 != 0
			}
		}
	}

	/// The masks on the effect's layer at the current time (see
	/// [`Self::set_mask_paths`] and [`Self::set_mask_tracks`]).
	pub fn mask_paths(&self) -> &[crate::MaskPath] {
		&self.mask_paths
	}

	/// Replace the masks on the effect's layer with ones that do not change
	/// over time. Plugins see them through the "PF Path Query Suite" / "PF
	/// Path Data Suite" (e.g. for `PF_Param_PATH` parameters, which reference
	/// a mask by [`MaskPath::id`](crate::MaskPath::id)), the "AEGP Layer Mask
	/// Suite" and mask streams.
	pub fn set_mask_paths(&mut self, paths: Vec<crate::MaskPath>) {
		self.set_mask_tracks(paths.into_iter().map(crate::Track::new).collect());
	}

	/// Replace the masks on the effect's layer with ones that may change over
	/// time, keyed in layer time as [`Self::set_param_track`] is. A path
	/// morphs between keyframes with as many vertices and holds otherwise.
	/// `PF_CheckoutPath`, mask streams, their keyframes and
	/// `PF_GetCurrentState` read a mask at the time asked for; the rest of a
	/// mask (its id, mode, inversion and name) is its value at the current
	/// time, so keep the id the same on every keyframe.
	pub fn set_mask_tracks(&mut self, tracks: Vec<crate::Track<crate::MaskPath>>) {
		self.masks = tracks;
		self.apply_mask_tracks();
	}

	/// The masks on the effect's layer over time.
	pub fn mask_tracks(&self) -> &[crate::Track<crate::MaskPath>] {
		&self.masks
	}

	/// Mask `index` at `time` in layer time.
	pub(crate) fn mask_at(&self, index: usize, time: crate::TimePoint) -> Option<crate::MaskPath> {
		self.masks.get(index).map(|track| track.at(time))
	}

	/// Hold `path` until [`Self::check_in_path`]: a mask read at a time of its
	/// own. The pointer stays valid while it is held.
	pub(crate) fn check_out_path(&self, path: crate::MaskPath) -> *const crate::MaskPath {
		let held = Box::new(path);
		let at: *const crate::MaskPath = &*held;
		self.checked_out_paths
			.lock()
			.unwrap_or_else(std::sync::PoisonError::into_inner)
			.push(held);
		at
	}

	/// Drop a path [`Self::check_out_path`] holds; `false` if it is not one.
	pub(crate) fn check_in_path(&self, path: *const crate::MaskPath) -> bool {
		let mut held = self
			.checked_out_paths
			.lock()
			.unwrap_or_else(std::sync::PoisonError::into_inner);
		let Some(at) = held.iter().position(|p| std::ptr::eq(&**p, path)) else {
			return false;
		};
		held.swap_remove(at);
		true
	}

	/// Evaluate the mask tracks at the current time.
	pub(super) fn apply_mask_tracks(&mut self) {
		let now: crate::TimePoint = self.time().into();
		self.mask_paths = self.masks.iter().map(|track| track.at(now)).collect();
	}

	/// The text on the effect's layer, when it is a text layer (see
	/// [`Self::set_text_layer`]).
	pub fn text_layer(&self) -> Option<&crate::TextLayer> {
		self.text_layer.as_ref()
	}

	/// Make the effect's layer a text layer showing `text`, or an AV layer
	/// again with `None`. `AEGP_GetLayerObjectType` then answers
	/// `AEGP_ObjectType_TEXT`, and the "AEGP Text Layer Suite" serves the
	/// glyph outlines.
	///
	/// The pixels the plugin renders from are still the input layer's; set it
	/// to [`TextLayer::rasterize`](crate::TextLayer::rasterize) to match what
	/// After Effects would hand the effect.
	pub fn set_text_layer(&mut self, text: Option<crate::TextLayer>) {
		self.text_layer = text;
	}

	/// The project around the effect (see [`Self::set_project`]).
	pub fn project(&self) -> Option<&crate::Project> {
		self.project.as_ref().map(|state| &state.project)
	}

	pub(crate) fn project_state(&self) -> Option<&crate::project::ProjectState> {
		self.project.as_ref()
	}

	/// Describe the project around the effect: its comp, every layer, and
	/// which one the effect is on. With `None` the effect is alone again, on a
	/// layer that fills its comp.
	///
	/// With a project, what the effect's layer is comes from the project, not
	/// from [`Self::set_text_layer`], and [`Self::set_time`]'s time is that
	/// layer's: comp time is `start + time * stretch`. The pixels are still
	/// [`Self::set_input_layer`]'s; [`ProjectLayer::rasterize`](crate::ProjectLayer::rasterize)
	/// makes them for a text or shape layer.
	///
	/// A layer keeps its handle for as long as its id stays in the project;
	/// the handle of one that left is refused from then on.
	pub fn set_project(&mut self, project: Option<crate::Project>) {
		if self.project() == project.as_ref() {
			return;
		}
		let duration = project.as_ref().map(|project| {
			let layer = project.effect_layer();
			layer
				.to_layer_time(layer.out_point)
				.since(layer.to_layer_time(layer.in_point))
		});
		self.render_context.set_layer_duration(duration);
		let owner = (&raw const *self.source_item_token) as usize;
		let previous = self.project.take();
		self.project = project.map(|project| crate::project::ProjectState::new(project, owner, previous));
	}

	/// Link `layer` to the plugin's own layer parameter at `index` (same index
	/// space as [`Self::set_param`]), or unlink it with `None` (the parameter's
	/// "None" choice). The plugin sees the layer through `checkout_param` and
	/// smart-render layer checkouts, on both the CPU and GPU paths.
	///
	/// # Errors
	/// [`AexloError::ParamIndexOutOfBounds`] for index 0 (the input layer, see
	/// [`Self::set_input_layer`]) or an out-of-range index, and
	/// [`AexloError::ParamTypeMismatch`] if the parameter is not a layer.
	pub fn set_layer_param<D: PixelDepth>(&mut self, index: usize, layer: Option<Layer<D>>) -> Result<()> {
		if index == 0 || index >= self.params.len() {
			return Err(AexloError::ParamIndexOutOfBounds {
				index,
				max: self.params.len().saturating_sub(1),
			});
		}
		let param_type = self.params[index].param_type;
		if param_type != PF_Param_LAYER as PF_ParamType {
			return Err(AexloError::ParamTypeMismatch {
				index,
				expected: crate::ParamKind::Layer,
				actual: crate::ParamKind::from_sys(param_type),
			});
		}

		// SAFETY: the parameter was verified to be a layer above.
		let current = unsafe { self.params[index].u.ld };
		let unlinked = self.linked_layers.remove(&index).map_or(current, |l| l.unlinked);
		let def = match layer {
			Some(layer) => {
				let linked = LinkedLayer::new(AnyLayer::new(layer), unlinked);
				let def = linked.param_def();
				self.linked_layers.insert(index, linked);
				def
			}
			None => unlinked,
		};
		self.params[index].u = PF_ParamDefUnion { ld: def };
		Ok(())
	}

	/// Unlink the layer parameter at `index` (its "None" choice); the same as
	/// [`Self::set_layer_param`] with `None`, without naming a depth.
	///
	/// # Errors
	/// As [`Self::set_layer_param`].
	pub fn clear_layer_param(&mut self, index: usize) -> Result<()> {
		self.set_layer_param::<Depth8>(index, None)
	}

	/// The layer linked to the layer parameter at `index`, if any.
	pub fn layer_param(&self, index: usize) -> Option<&AnyLayer> {
		self.linked_layers.get(&index).map(LinkedLayer::layer)
	}

	/// Record that smart-render `checkout_id` refers to the layer parameter at `index`.
	pub(crate) fn record_layer_checkout(&mut self, checkout_id: crate::sys::A_long, index: usize) {
		self.layer_checkouts.insert(checkout_id, index);
	}

	pub(crate) fn has_layer_checkout(&self, checkout_id: crate::sys::A_long) -> bool {
		self.layer_checkouts.contains_key(&checkout_id)
	}

	/// A hash of the pixels behind layer parameter `index` (the input layer for
	/// 0), `None` when nothing is linked.
	pub(crate) fn layer_digest(&self, index: usize) -> Option<u64> {
		match index {
			0 => Some(self.input_layer.digest()),
			_ => self.linked_layers.get(&index).map(LinkedLayer::digest),
		}
	}

	/// Size of the layer behind parameter `index`: the input layer for 0, the
	/// linked layer otherwise, `None` for an unlinked or non-layer parameter.
	pub(crate) fn layer_param_size(&self, index: usize) -> Option<(i32, i32)> {
		if index == 0 {
			let (w, h) = self.input_size();
			return Some((w as i32, h as i32));
		}
		self.linked_layers.get(&index).map(LinkedLayer::size)
	}

	/// The world to hand out for smart-render `checkout_id`: the input world for
	/// the input layer, the linked layer's CPU or
	/// GPU world for a layer parameter, `None` for an unlinked one.
	pub(crate) fn checked_out_world(
		&mut self,
		checkout_id: crate::sys::A_long,
	) -> Option<*mut crate::sys::PF_EffectWorld> {
		let gpu = self.smart_render_data.is_gpu();
		match self.layer_checkouts.get(&checkout_id).copied()? {
			0 => Some(self.input_world_ptr()),
			index => self
				.linked_layers
				.get_mut(&index)
				.map(|l| l.world_ptr(gpu) as *mut crate::sys::PF_EffectWorld),
		}
	}

	/// Build a `width` x `height` layer filled with `fill`.
	pub(super) fn build_layer(width: u32, height: u32, fill: Pixel<Depth8>) -> Layer<Depth8> {
		Layer::<Depth8>::new(width, height, vec![fill; (width * height) as usize]).unwrap()
	}

	/// Register the implicit `PF_Param_LAYER` parameter at index 0, backed by `input_layer`.
	pub(super) fn push_input_layer_param(&mut self) {
		self.params.push(PF_ParamDef {
			uu: crate::sys::PF_ParamDef__bindgen_ty_1 { id: 0 },
			ui_flags: 0,
			ui_width: 0,
			ui_height: 0,
			param_type: 0 as PF_ParamType,
			name_do_not_use_directly: [0; 32],
			flags: 0,
			unused: 0,
			u: PF_ParamDefUnion {
				ld: self.input_layer.as_sys(),
			},
		});
		self.param_labels.push(Vec::new());
		// The push above invalidates any (nonexistent yet) cached param pointers;
		// mark dirty so `call_plugin` builds the cache on its first invocation.
		self.params_dirty = true;
	}
}
