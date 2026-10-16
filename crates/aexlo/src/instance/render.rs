//! Rendering a frame: the legacy, smart and GPU command paths, plus the
//! frame size and time they render at.

use super::*;

/// The command path [`PluginInstance::render_with`] renders a frame through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum RenderPath {
	/// GPU when possible, else smart, else legacy, falling through on failure
	/// ([`PluginInstance::render_frame`]).
	#[default]
	Auto,
	/// `PF_Cmd_RENDER` only.
	Legacy,
	/// `PF_Cmd_SMART_PRE_RENDER` + `PF_Cmd_SMART_RENDER` only.
	Smart,
	/// `PF_Cmd_SMART_PRE_RENDER` + `PF_Cmd_SMART_RENDER_GPU` only, on a
	/// 32-bit float GPU world.
	Gpu,
}

impl PluginInstance {
	/// Call the plugin with `PF_Cmd_RENDER` command ([`RenderPath::Legacy`]).
	fn render_legacy(&mut self) -> Result<()> {
		self.reset_cpu_worlds();
		self.poison_output();
		self.begin_output_guard();
		let result = self.call_plugin(RawCommand::Render, null_mut());
		self.end_output_guard();

		result
	}

	/// Call the plugin with `PF_Cmd_SMART_PRE_RENDER` command, letting it declare the
	/// input/output checkout regions it needs via [`Self::render_smart`].
	fn render_pre(&mut self) -> Result<()> {
		self.layer_checkouts.clear();
		// Each pre-render gets a fresh output, as in After Effects; plugins OR
		// their flags into it and would otherwise inherit the last frame's.
		self.smart_render_data.begin_pre_render();
		if !self.smart_render_data.is_gpu() {
			self.reset_cpu_worlds();
			self.poison_output();
			self.begin_output_guard();
		}
		let mut extra = self.smart_render_data.pre_render_extra();

		self.call_plugin(
			RawCommand::SmartPreRender,
			(&mut extra as *mut after_effects_sys::PF_PreRenderExtra).cast(),
		)?;

		self.smart_render_data.sync();

		Ok(())
	}

	/// Call the plugin with `PF_Cmd_SMART_RENDER` command, using the checkout regions
	/// declared during the preceding [`Self::render_pre`] call.
	fn render_smart(&mut self) -> Result<()> {
		let mut extra = self.smart_render_data.smart_render_extra();

		let result = self.call_plugin(
			after_effects::RawCommand::SmartRender,
			(&mut extra as *mut after_effects_sys::PF_SmartRenderExtra).cast(),
		);
		self.end_output_guard();

		// The frame is done; release the plugin's pre-render data as AE does.
		self.dispose_pre_render();
		result
	}

	/// Whether the plugin declared `PF_OutFlag2_SUPPORTS_GPU_RENDER_F32` during
	/// `PF_Cmd_GLOBAL_SETUP`, i.e. it can render on the GPU into 32-bit-float
	/// `PF_PixelFormat_GPU_BGRA128` worlds ([`RenderPath::Gpu`]).
	///
	/// Only meaningful once global setup has run (i.e. after [`Self::try_load`]).
	pub fn supports_gpu(&self) -> bool {
		// Escape hatch for benchmarking / debugging the CPU path even on GPU-capable
		// effects: setting AEXLO_DISABLE_GPU forces the CPU render path.
		if std::env::var_os("AEXLO_DISABLE_GPU").is_some() {
			return false;
		}
		let flag = PF_OutFlag2_SUPPORTS_GPU_RENDER_F32 as PF_OutFlags2;
		self.global_out_flags2 & flag != 0
	}

	/// Run `PF_Cmd_GPU_DEVICE_SETUP`, creating aexlo's GPU context (Metal or CUDA)
	/// on first use and capturing the plugin-owned GPU data (compiled pipelines,
	/// etc.) it returns.
	///
	/// # Errors
	/// Returns an error if no GPU device is available, or if the plugin's setup
	/// command fails.
	fn gpu_device_setup(&mut self) -> Result<()> {
		if self.gpu_context.is_none() {
			self.gpu_context = crate::gpu::GpuContext::new();
			// A fresh context owns no buffers yet; force an input upload.
			self.gpu_input_uploaded = false;
		}

		let Some(ctx) = self.gpu_context.as_ref() else {
			return Err(AexloError::GpuUnavailable);
		};

		let what_gpu = ctx.framework();
		// The plugin's own GPU-API calls during setup (e.g. cudaMalloc) resolve
		// against the thread-current context.
		ctx.make_current();

		let mut input = PF_GPUDeviceSetupInput {
			what_gpu,
			device_index: 0,
		};
		let mut output = PF_GPUDeviceSetupOutput { gpu_data: null_mut() };
		let mut extra = PF_GPUDeviceSetupExtra {
			input: &mut input,
			output: &mut output,
		};

		self.call_plugin(
			RawCommand::GpuDeviceSetup,
			(&mut extra as *mut PF_GPUDeviceSetupExtra).cast(),
		)?;

		self.gpu_data = output.gpu_data;
		Ok(())
	}

	/// Run `PF_Cmd_GPU_DEVICE_SETDOWN` so the plugin releases its GPU data, then drop
	/// this instance's GPU-world registrations. Safe to call when no device was set up.
	pub(super) fn gpu_device_setdown(&mut self) -> Result<()> {
		if self.gpu_data.is_null() {
			return Ok(());
		}

		let what_gpu = self
			.gpu_context
			.as_ref()
			.map(|ctx| ctx.framework())
			.unwrap_or(PF_GPU_Framework_NONE as PF_GPU_Framework);
		let mut input = PF_GPUDeviceSetdownInput {
			gpu_data: self.gpu_data,
			what_gpu,
			device_index: 0,
		};
		let mut extra = PF_GPUDeviceSetdownExtra { input: &mut input };

		let result = self.call_plugin(
			RawCommand::GpuDeviceSetdown,
			(&mut extra as *mut PF_GPUDeviceSetdownExtra).cast(),
		);

		self.gpu_data = null_mut();
		if let Some(ctx) = &self.gpu_context {
			ctx.unregister_all_worlds();
		}
		result
	}

	/// Dispatch `PF_Cmd_SMART_RENDER_GPU` using the checkout regions declared during
	/// the preceding [`Self::render_pre`] call.
	fn render_smart_gpu(&mut self) -> Result<()> {
		let mut extra = self.smart_render_data.smart_render_extra();

		let result = self.call_plugin(
			RawCommand::SmartRenderGpu,
			(&mut extra as *mut after_effects_sys::PF_SmartRenderExtra).cast(),
		);

		self.dispose_pre_render();
		result
	}

	/// Render one frame on the GPU: set up the device (once), back the input and
	/// output worlds with device buffers, upload the input as BGRA float, run the
	/// smart pre-render/GPU-render pair, wait for the GPU, and read the result back.
	///
	/// # Errors
	/// Propagates any failure from device setup or the render commands. On error the
	/// caller should fall back to CPU rendering (see [`Self::render_frame`]).
	fn render_gpu(&mut self) -> Result<()> {
		if self.gpu_data.is_null() {
			self.gpu_device_setup()?;
		}

		// Phase A: geometry, buffers, and input upload. All borrows are of distinct
		// fields, so they coexist without borrowing `self` as a whole.
		let output_key = &self.world as *const _ as usize;
		let framework;
		let out_len;
		{
			let in_w = self.input_layer.width() as usize;
			let in_h = self.input_layer.height() as usize;
			let out_w = self.output_layer.width() as usize;
			let out_h = self.output_layer.height() as usize;
			out_len = out_w * out_h * GPU_BYTES_PER_PIXEL;

			// `in_data.width/height` describe the source layer, as in After Effects;
			// the output extent comes from the output world itself.
			self.render_context.set_size(in_w as i32, in_h as i32);

			// Present both worlds as f32 BGRA (16 bytes/pixel, no row padding).
			self.input_world.width = in_w as i32;
			self.input_world.height = in_h as i32;
			self.input_world.rowbytes = (in_w * GPU_BYTES_PER_PIXEL) as i32;
			self.world.width = out_w as i32;
			self.world.height = out_h as i32;
			self.world.rowbytes = (out_w * GPU_BYTES_PER_PIXEL) as i32;

			let input_key = &self.input_world as *const _ as usize;

			let ctx = self
				.gpu_context
				.as_mut()
				.ok_or_else(|| AexloError::Gpu("GPU context missing".to_string()))?;
			framework = ctx.framework();
			ctx.make_current();

			if !ctx.ensure_buffer(input_key, in_w * in_h * GPU_BYTES_PER_PIXEL)
				|| !ctx.ensure_buffer(output_key, out_len)
			{
				return Err(AexloError::Gpu("Failed to allocate GPU world buffers".to_string()));
			}

			// Pack + upload only when the input pixels changed since the last upload
			// (an input size change goes through `set_input`, which clears the flag,
			// so a reallocated buffer always gets fresh pixels).
			if !self.gpu_input_uploaded {
				Self::pack_layer_to_bgra_f32(self.input_layer.layer(), &mut self.gpu_upload_staging);
				if !ctx.write_buffer(input_key, bytemuck::cast_slice(&self.gpu_upload_staging)) {
					return Err(AexloError::Gpu("Failed to upload input pixels to the GPU".to_string()));
				}
				self.gpu_input_uploaded = true;
			}

			// Layers linked to layer parameters get their own device buffers.
			for linked in self.linked_layers.values_mut() {
				let key = linked.world_ptr(true) as usize;
				if !ctx.ensure_buffer(key, linked.gpu_len()) {
					return Err(AexloError::Gpu(
						"Failed to allocate GPU layer-parameter buffer".to_string(),
					));
				}
				if !linked.gpu_uploaded {
					Self::pack_layer_to_bgra_f32(linked.layer(), &mut self.gpu_upload_staging);
					if !ctx.write_buffer(key, bytemuck::cast_slice(&self.gpu_upload_staging)) {
						return Err(AexloError::Gpu(
							"Failed to upload layer-parameter pixels to the GPU".to_string(),
						));
					}
					linked.gpu_uploaded = true;
				}
			}
		}

		self.smart_render_data.configure_gpu(self.gpu_data, 0, framework);
		let result = self.run_gpu_frame(output_key, out_len);
		// Later renders default to the CPU; leaving the GPU configuration in
		// place would hand the next CPU `SMART_RENDER` a GPU frame description.
		self.smart_render_data.configure_cpu();
		result
	}

	/// Phases B and C of [`Self::render_gpu`], with the smart-render inputs
	/// configured for the GPU.
	fn run_gpu_frame(&mut self, output_key: usize, out_len: usize) -> Result<()> {
		// Phase B: pre-render declares regions, then the GPU render runs -- if the
		// plugin says this frame can render on the GPU. Otherwise After Effects
		// renders it on the CPU instead, which `render_frame` does on this error.
		self.render_pre()?;
		if !self.smart_render_data.gpu_render_possible() {
			self.dispose_pre_render();
			return Err(AexloError::GpuRenderDeclined);
		}
		self.render_smart_gpu()?;

		// The plugin queues its work but does not wait; flush before reading back.
		if let Some(ctx) = &self.gpu_context {
			ctx.wait_for_completion();
		}

		// Phase C: read the rendered BGRA float output back into the output layer,
		// reusing the instance's staging buffer.
		if let Some(ctx) = self.gpu_context.as_ref() {
			self.gpu_readback_staging
				.resize(out_len / std::mem::size_of::<f32>(), 0.0);
			if ctx.read_buffer(output_key, bytemuck::cast_slice_mut(&mut self.gpu_readback_staging)) {
				Self::unpack_bgra_f32_to_layer(&self.gpu_readback_staging, self.output_layer.layer_mut());
			}
		}

		Ok(())
	}

	/// Pack an ARGB layer of any depth into `PF_PixelFormat_GPU_BGRA128` staging
	/// data: BGRA channel order, each channel normalised to `[0, 1]` float.
	///
	/// Writes into the caller-provided `staging` buffer (cleared first) so a
	/// render loop can reuse one allocation across frames.
	fn pack_layer_to_bgra_f32(layer: &AnyLayer, staging: &mut Vec<f32>) {
		fn pack<D: PixelDepth>(layer: &Layer<D>, staging: &mut Vec<f32>) {
			staging.reserve(layer.len() * 4);
			for pixel in layer.pixels() {
				staging.push(D::to_unit(pixel.blue));
				staging.push(D::to_unit(pixel.green));
				staging.push(D::to_unit(pixel.red));
				staging.push(D::to_unit(pixel.alpha));
			}
		}
		staging.clear();
		match layer {
			AnyLayer::U8(l) => pack(l, staging),
			AnyLayer::U16(l) => pack(l, staging),
			AnyLayer::F32(l) => pack(l, staging),
		}
	}

	/// Unpack `PF_PixelFormat_GPU_BGRA128` staging data (BGRA float) back into an
	/// ARGB layer of any depth, clamping and rounding integer channels.
	fn unpack_bgra_f32_to_layer(staging: &[f32], layer: &mut AnyLayer) {
		fn unpack<D: PixelDepth>(staging: &[f32], layer: &mut Layer<D>) {
			for (pixel, bgra) in layer.pixels_mut().iter_mut().zip(staging.as_chunks::<4>().0) {
				pixel.blue = D::from_unit(bgra[0]);
				pixel.green = D::from_unit(bgra[1]);
				pixel.red = D::from_unit(bgra[2]);
				pixel.alpha = D::from_unit(bgra[3]);
			}
		}
		match layer {
			AnyLayer::U8(l) => unpack(staging, l),
			AnyLayer::U16(l) => unpack(staging, l),
			AnyLayer::F32(l) => unpack(staging, l),
		}
	}

	/// Whether the plugin declared `PF_OutFlag2_SUPPORTS_SMART_RENDER` during
	/// `PF_Cmd_GLOBAL_SETUP`, i.e. it expects the smart pre-render/render command
	/// pair ([`RenderPath::Smart`]) rather than the legacy `PF_Cmd_RENDER`
	/// ([`RenderPath::Legacy`]).
	///
	/// Only meaningful once global setup has run (i.e. after [`Self::try_load`]).
	pub fn supports_smart_render(&self) -> bool {
		let flag = after_effects_sys::PF_OutFlag2_SUPPORTS_SMART_RENDER as after_effects_sys::PF_OutFlags2;
		self.global_out_flags2 & flag != 0
	}

	/// Render one frame the way After Effects would pick the path: on the GPU
	/// when the plugin [supports it](Self::supports_gpu), else the smart
	/// pre-render/render pair when [declared](Self::supports_smart_render), else
	/// the legacy `PF_Cmd_RENDER`. A failed GPU or smart render falls through to
	/// the next path; a cancelled one does not.
	///
	/// This is the call for a general-purpose host: many modern effects implement
	/// `PF_Cmd_SMART_RENDER`, others only the legacy command, and some declare
	/// smart support but still render correctly through the legacy path when the
	/// smart emulation can't satisfy them. Same as
	/// [`render_with(RenderPath::Auto)`](Self::render_with).
	///
	/// # Errors
	/// The error of the last path tried, typically
	/// [`AexloError::PluginExecutionFailed`].
	pub fn render_frame(&mut self) -> Result<()> {
		self.render_with(RenderPath::Auto)
	}

	/// Render one frame through `path` only, without falling back to another
	/// path (except [`RenderPath::Auto`], which is [`Self::render_frame`]).
	///
	/// For tests and tools that exercise one command path of a plugin; a host
	/// should call [`Self::render_frame`].
	///
	/// # Errors
	/// [`AexloError::PluginExecutionFailed`] if the plugin rejects a command,
	/// [`AexloError::GpuUnavailable`] or [`AexloError::GpuRenderDeclined`] for a
	/// GPU render that cannot run.
	pub fn render_with(&mut self, path: RenderPath) -> Result<()> {
		match path {
			RenderPath::Auto => self.render_auto(),
			RenderPath::Legacy => self.render_legacy(),
			RenderPath::Smart => self.render_pre().and_then(|()| self.render_smart()),
			RenderPath::Gpu => self.render_gpu(),
		}
	}

	/// [`RenderPath::Auto`]: GPU, then smart, then legacy.
	fn render_auto(&mut self) -> Result<()> {
		if self.supports_gpu() {
			match self.render_gpu() {
				Ok(()) => return Ok(()),
				// A cancelled render is not retried on another path.
				Err(err) if err.is_cancelled() => return Err(err),
				Err(err) => {
					if matches!(err, AexloError::GpuRenderDeclined) {
						log::debug!("Plugin declined GPU render for this frame; rendering on the CPU.");
					} else {
						log::warn!("GPU render failed ({err:?}); falling back to CPU render.");
					}
					// Undo GPU state so the CPU fallback doesn't leave the plugin
					// expecting a GPU frame or reporting GPU worlds.
					self.smart_render_data.configure_cpu();
					if let Some(ctx) = &self.gpu_context {
						ctx.unregister_all_worlds();
					}
				}
			}
		}

		if self.supports_smart_render() {
			match self.render_pre().and_then(|()| self.render_smart()) {
				Ok(()) => return Ok(()),
				Err(err) if err.is_cancelled() => return Err(err),
				Err(err) => {
					log::warn!("Smart render failed ({err:?}); falling back to legacy render.");
				}
			}
		}

		self.render_legacy()
	}

	/// Set the output frame size in pixels, resizing the output world and the smart-render
	/// output request rects. `in_data.width/height` keep describing the input
	/// layer (see [`Self::set_input`]).
	///
	/// Call this before rendering. Global/params/sequence setup runs at the
	/// default size ([`Self::output_size`] after load), which plugins tolerate --
	/// After Effects itself resizes freely between renders.
	pub fn set_render_size(&mut self, width: u32, height: u32) {
		let kind = self.output_layer.kind();
		self.output_layer = HostLayer::new(AnyLayer::filled(kind, width, height, OPAQUE_BLACK));

		self.world = crate::core::helpers::LayerDefBuilder::new()
			.with_size(width as i32, height as i32)
			.build();
		self.sync_output_world();

		self.smart_render_data.set_output_rect(width as i32, height as i32);
	}

	/// Set the time the next commands operate on (e.g. `Time::frame(n, 30)` for
	/// frame `n` of a 30 fps comp).
	///
	/// The layer and comp the plugin sees through the AEGP suites run at the
	/// same frame rate. Plugins start at [`Time::default`], frame 0 at 30 fps.
	///
	/// # Panics
	/// If `time.step` or `time.scale` is not positive: plugins divide by both.
	pub fn set_time(&mut self, time: Time) {
		assert!(time.step > 0 && time.scale > 0, "time step and scale must be positive");
		self.render_context.set_time(time.value, time.step, time.scale);
	}

	/// The time set by [`Self::set_time`].
	pub fn time(&self) -> Time {
		let (value, step, scale) = self.render_context.time();
		Time { value, step, scale }
	}

	/// Release the plugin's pre-render data, attributing what it frees.
	pub(super) fn dispose_pre_render(&mut self) {
		let _tracking = self.tracking();
		self.smart_render_data.dispose_pre_render_data();
	}

	/// Strict mode: point the output world at a copy of the output with guard
	/// bands around it, for the plugin to render into.
	fn begin_output_guard(&mut self) {
		self.end_output_guard();
		if !self.strict.guard_bands {
			return;
		}
		let layer = self.output_layer.layer_mut();
		let bpp = layer.kind().bytes_per_pixel();
		let (w, h) = (layer.width() as usize, layer.height() as usize);
		let mut guarded = crate::strict::Guarded::new(w * bpp, h, bpp);
		// SAFETY: the layer's buffer is exactly `w * h * bpp` bytes.
		let bytes = unsafe { std::slice::from_raw_parts(layer.data_ptr(), w * h * bpp) };
		guarded.load_rows(bytes);
		self.world.data = guarded.data_ptr() as *mut after_effects_sys::PF_Pixel;
		self.world.rowbytes = guarded.rowbytes() as i32;
		self.output_guard = Some(guarded);
	}

	/// Copy the guarded output back and report overwritten guard bands.
	pub(super) fn end_output_guard(&mut self) {
		let Some(guarded) = self.output_guard.take() else {
			return;
		};
		let layer = self.output_layer.layer_mut();
		let len = layer.width() as usize * layer.height() as usize * layer.kind().bytes_per_pixel();
		// SAFETY: as in `begin_output_guard`.
		let bytes = unsafe { std::slice::from_raw_parts_mut(layer.data_mut_ptr(), len) };
		guarded.store_rows(bytes);
		let (w, h) = (layer.width(), layer.height());
		let broken = guarded.verify();
		if !broken.is_empty()
			&& let Ok(mut tracker) = self.tracker.lock()
		{
			for (band, bytes) in broken {
				tracker.report.guard_violations.push(crate::GuardViolation {
					world: format!("output {w}x{h}"),
					band,
					bytes,
				});
			}
		}
		self.sync_output_world();
	}

	/// Fill the output with the strict-mode poison, when asked to.
	fn poison_output(&mut self) {
		if self.strict.poison_output {
			crate::strict::poison(self.output_layer.layer_mut());
		}
	}

	/// Point `world` raw pointers at this instance's own owned buffers.
	/// Restore the 8-bit CPU layout of the input/output worlds and drop their
	/// GPU-world registration. [`Self::render_gpu`] reshapes both worlds to
	/// `GPU_BGRA128`; a later CPU render must not see that stride or have
	/// `PF_GetPixelFormat` report the float GPU format.
	fn reset_cpu_worlds(&mut self) {
		if let Some(ctx) = &self.gpu_context {
			ctx.unregister_all_worlds();
		}
		self.input_world = self.input_layer.as_sys();
		self.sync_output_world();
	}

	/// Describe `output_layer` in the output world: size, stride, depth flag
	/// and pixels. The rest of the world (extent hint, ...) is left alone.
	pub(super) fn sync_output_world(&mut self) {
		let layer = self.output_layer.as_sys();
		self.world.width = layer.width;
		self.world.height = layer.height;
		self.world.rowbytes = layer.rowbytes;
		self.world.world_flags = layer.world_flags;
		self.world.data = layer.data;
	}
}
