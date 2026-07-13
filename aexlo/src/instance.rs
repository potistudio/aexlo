use crate::core::error::{AexloError, Result};
use crate::host::smart_render::SmartRenderData;
use crate::utils;

/// A parameter value for an After Effects plugin.
#[derive(Debug, Clone, PartialEq)]
pub enum ParamValue {
	/// `PF_Param_FLOAT_SLIDER` — a floating-point slider value.
	Float(f64),
	/// `PF_Param_FIX_SLIDER` — a fixed-point slider value (surfaced as `f32`).
	Fixed(f32),
	/// `PF_Param_SLIDER` — an integer slider value.
	Slider(i32),
	/// `PF_Param_CHECKBOX` — a boolean toggle.
	Checkbox(bool),
	/// `PF_Param_POPUP` — the selected 1-based choice index.
	Popup(i32),
	/// `PF_Param_ANGLE` — an angle in degrees.
	Angle(f32),
	/// `PF_Param_POINT` — a 2D point, in pixels.
	Point { x: f32, y: f32 },
	/// `PF_Param_COLOR` — an 8-bit RGBA color.
	Color { red: u8, green: u8, blue: u8, alpha: u8 },
}

impl ParamValue {
	/// The `PF_ParamType` this value must be written to, plus a human-readable name
	/// for error reporting. Used to guard union writes in
	/// [`PluginInstance::set_param`].
	fn expected_param_type(&self) -> (ParamType, &'static str) {
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

use crate::gpu::GPU_BYTES_PER_PIXEL;
use after_effects::{ParamType, RawCommand};
use after_effects_sys::{
	PF_Err_INVALID_CALLBACK, PF_Err_NONE, PF_GPU_Framework, PF_GPU_Framework_NONE, PF_GPUDeviceSetdownExtra,
	PF_GPUDeviceSetdownInput, PF_GPUDeviceSetupExtra, PF_GPUDeviceSetupInput, PF_GPUDeviceSetupOutput,
	PF_OutFlag2_SUPPORTS_GPU_RENDER_F32, PF_OutFlags2, PF_ParamDef, PF_ParamDefUnion, PF_ParamType, PF_Pixel,
	PF_ProgPtr,
};
use colored::Colorize;
use dlopen2::raw::Library;
use std::{
	ffi::{CStr, CString},
	path::{Path, PathBuf},
	ptr::NonNull,
	ptr::null_mut,
};

use crate::core::constants::{DEFAULT_HEIGHT as HEIGHT, DEFAULT_WIDTH as WIDTH};

const DEFAULT_ENTRY_POINT_NAME: &str = "EffectMain";

/// Entry point names to try if the plugin doesn't implement `PluginDataEntryFunction2`.
const FALLBACK_ENTRY_POINT_CANDIDATES: &[&str] = &[DEFAULT_ENTRY_POINT_NAME, "EntryPointFunc"];
/// Fixed symbol name of the AE SDK's self-describing plugin data entry function.
const PLUGIN_DATA_ENTRY_SYMBOL: &str = "PluginDataEntryFunction2";
const HOST_NAME: &str = "AfterEffects";
const HOST_VERSION: &str = "25.2";

/// ABI of an After Effects effect entry point (`EffectMain`): the fixed
/// `(cmd, in_data, out_data, params, output, extra)` signature every effect
/// exports. Handed to [`PluginInstance::from_entry`] to drive an in-process
/// effect without `dlopen`.
pub type PluginEntryPoint = unsafe extern "C" fn(
	cmd: RawCommand,
	in_data: *mut after_effects_sys::PF_InData,
	out_data: *mut after_effects_sys::PF_OutData,
	params: after_effects_sys::PF_ParamList,
	output: *mut after_effects_sys::PF_LayerDef,
	extra: *mut ::std::os::raw::c_void,
) -> after_effects_sys::PF_Err;

/// Signature of `PluginDataEntryFunction2`: the AE SDK's cross-platform replacement
/// for a binary PiPL resource. Plugins export this under a fixed symbol name and,
/// when called, report their real entry point name (and other PiPL metadata) back
/// through the `PF_PluginDataCB2` callback instead of the host parsing a resource.
type PluginDataEntryFn = unsafe extern "C" fn(
	after_effects_sys::PF_PluginDataPtr,
	after_effects_sys::PF_PluginDataCB2,
	*const after_effects_sys::SPBasicSuite,
	*const std::os::raw::c_char,
	*const std::os::raw::c_char,
) -> after_effects_sys::PF_Err;

#[derive(Default)]
struct PluginDataInfo {
	entry_point_name: Option<String>,
}

unsafe extern "C" fn receive_plugin_data(
	in_ptr: after_effects_sys::PF_PluginDataPtr,
	_in_name: *const after_effects_sys::A_u_char,
	_in_match_name: *const after_effects_sys::A_u_char,
	_in_category: *const after_effects_sys::A_u_char,
	in_entry_point_name: *const after_effects_sys::A_u_char,
	_in_kind: after_effects_sys::A_long,
	_in_api_version_major: after_effects_sys::A_long,
	_in_api_version_minor: after_effects_sys::A_long,
	_in_reserved_info: after_effects_sys::A_long,
	_in_support_url: *const after_effects_sys::A_u_char,
) -> after_effects_sys::A_Err {
	if in_ptr.is_null() || in_entry_point_name.is_null() {
		return PF_Err_INVALID_CALLBACK as after_effects_sys::A_Err;
	}

	let info = unsafe { &mut *(in_ptr as *mut PluginDataInfo) };
	let name = unsafe { CStr::from_ptr(in_entry_point_name as *const std::os::raw::c_char) };
	info.entry_point_name = Some(name.to_string_lossy().into_owned());

	PF_Err_NONE as after_effects_sys::A_Err
}

/// Represents a loaded After Effects plugin instance, managing its library, entry point, parameters, and execution state.
pub struct PluginInstance {
	raw_library: Option<Library>,
	entry_point: Option<PluginEntryPoint>,
	entry_point_name: Option<String>,
	binary_file_path: PathBuf,

	/// The dynamic library actually `dlopen`ed (resolved from
	/// `binary_file_path`, e.g. the binary inside a `.plugin` bundle), or `None`
	/// for in-process entry points. Reported to the plugin through
	/// `get_platform_data` (`PF_PlatData_EXE_FILE_PATH_W`, ...) — per instance,
	/// so loading a second plugin doesn't clobber the first one's path.
	resolved_binary_path: Option<PathBuf>,

	/// Persistent output world handed back by `checkout_output` during smart render.
	world: after_effects_sys::PF_LayerDef,

	/// Persistent input world handed back by `checkout_layer_pixels` during smart
	/// render. Kept in sync with `input_layer` so the pointer stays valid across calls.
	input_world: after_effects_sys::PF_LayerDef,

	utility_callbacks: Box<after_effects_sys::_PF_UtilCallbacks>,

	/// Basic Suite pointer.
	pica: Box<after_effects_sys::SPBasicSuite>,

	/// InData structure.
	pub(crate) in_data: after_effects_sys::PF_InData,
	out_data: after_effects_sys::PF_OutData,

	/// Instance-specific parameters from the host (non-global storage).
	params: Vec<after_effects_sys::PF_ParamDef>,

	/// Track if instance params need synchronization.
	params_dirty: bool,

	/// Raw pointers into `params`, passed to the plugin entry point on each call.
	/// Rebuilt lazily in `call_plugin` whenever `params_dirty` is set, since pushing
	/// to `params` may reallocate its backing buffer and invalidate old pointers.
	params_ptr_cache: Vec<*mut PF_ParamDef>,

	pub(crate) input_layer: wrapper::Layer<wrapper::Depth8>,
	pub(crate) output_layer: wrapper::Layer<wrapper::Depth8>,

	smart_render_data: SmartRenderData,

	/// Metal device/queue plus the `MTLBuffer`s backing this instance's GPU worlds,
	/// created lazily when driving a plugin that declared GPU render support.
	gpu_context: Option<crate::gpu::GpuContext>,

	/// Reusable staging buffers for the GPU upload/readback paths, so a preview
	/// loop doesn't reallocate ~66 MB per 1080p frame.
	gpu_upload_staging: Vec<f32>,
	gpu_readback_staging: Vec<f32>,

	/// Whether the GPU input buffer currently holds `input_layer`'s pixels.
	/// Cleared by [`Self::set_input`] (and on context creation) so the next
	/// [`Self::render_gpu`] re-packs and re-uploads; left set otherwise, since a
	/// parameter-only change doesn't touch the input pixels.
	gpu_input_uploaded: bool,

	/// Opaque, plugin-owned GPU state returned by `PF_Cmd_GPU_DEVICE_SETUP`
	/// (compiled pipelines, etc.), handed back to the plugin during GPU render and
	/// released by `PF_Cmd_GPU_DEVICE_SETDOWN`.
	gpu_data: *mut ::std::os::raw::c_void,
}

impl PluginInstance {
	/// Load a plugin from `path`, then run it through global and params setup.
	///
	/// `path` is the plugin artifact exactly as it exists on disk: a bare
	/// `.aex`/`.dll` file on Windows, or a `.plugin` bundle directory on macOS.
	/// Callers never need to branch on platform -- if `path` is a directory it's
	/// treated as a bundle and the actual binary under `Contents/MacOS/` is
	/// resolved automatically; if it's a file, it's loaded as-is.
	///
	/// # Errors
	/// Returns an error if the binary can't be located or opened, if no entry
	/// point symbol can be resolved, or if the plugin rejects the
	/// `PF_Cmd_GLOBAL_SETUP`, `PF_Cmd_PARAMS_SETUP`, or `PF_Cmd_SEQUENCE_SETUP`
	/// commands.
	pub fn try_load(path: impl AsRef<Path>) -> Result<Self> {
		let mut instance = Self::new(path.as_ref());

		instance.load()?;
		instance.finalize()?;

		Ok(instance)
	}

	/// Drive an entry point that already lives in this process, skipping the
	/// whole `dlopen` → cdylib → bundle path.
	///
	/// The intended caller is a plugin's own `#[test]`/example: the
	/// `after-effects` `define_effect!` macro exports `EffectMain` as a plain
	/// `#[no_mangle] extern "C"` function, so it can be handed here directly for
	/// a fast, in-process preview that also happens to be debuggable (breakpoints
	/// and real backtraces, unlike a `dlopen`ed cdylib).
	///
	/// Use this when the plugin crate is built against the *same*
	/// `after-effects-sys` version as aexlo, so `EffectMain as PluginEntryPoint`
	/// coerces. Otherwise reach for [`Self::from_entry_raw`].
	///
	/// # Safety
	/// `entry` must be a valid AE effect entry point that is ABI-compatible with
	/// [`PluginEntryPoint`] and stays callable for the lifetime of the instance.
	pub unsafe fn from_entry(entry: PluginEntryPoint) -> Result<Self> {
		let mut instance = Self::new(Path::new("<in-process>"));

		instance.entry_point = Some(entry);
		instance.entry_point_name = Some(DEFAULT_ENTRY_POINT_NAME.to_string());
		// No `raw_library`: the entry point is already resident in this process.
		instance.finalize()?;

		Ok(instance)
	}

	/// Like [`Self::from_entry`], but takes the entry point's raw address and
	/// transmutes it to the expected ABI.
	///
	/// This exists for the common case where the plugin crate pulls a *different*
	/// `after-effects-sys` version than aexlo: the two `PluginEntryPoint` types
	/// are then nominally distinct and won't coerce, even though the C ABI is
	/// identical. Passing the address (`EffectMain as usize`) sidesteps the type
	/// mismatch, making ABI compatibility the caller's explicit promise.
	///
	/// # Safety
	/// `entry_addr` must be the address of a function that is ABI-compatible with
	/// [`PluginEntryPoint`] and stays callable for the lifetime of the instance.
	pub unsafe fn from_entry_raw(entry_addr: usize) -> Result<Self> {
		let entry: PluginEntryPoint = unsafe { std::mem::transmute(entry_addr) };
		unsafe { Self::from_entry(entry) }
	}

	/// Run the post-load setup shared by every constructor: `GLOBAL_SETUP`,
	/// `PARAMS_SETUP`, then `SEQUENCE_SETUP`. Independent of where the entry point
	/// came from (`dlopen`ed or handed in directly).
	fn finalize(&mut self) -> Result<()> {
		self.setup_global()?;
		self.setup_params()?;
		self.setup_sequence()?;

		Ok(())
	}

	/// Call the plugin with `PF_Cmd_ABOUT` command.
	pub fn about(&mut self) -> Result<String> {
		self.call_plugin(RawCommand::About, null_mut())?;

		Ok(self.message())
	}

	/// Call the plugin with `PF_Cmd_RENDER` command.
	pub fn render(&mut self) -> Result<()> {
		self.call_plugin(RawCommand::Render, null_mut())?;

		Ok(())
	}

	/// Call the plugin with `PF_Cmd_SMART_PRE_RENDER` command, letting it declare the
	/// input/output checkout regions it needs via [`Self::render_smart`].
	pub fn render_pre(&mut self) -> Result<()> {
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
	pub fn render_smart(&mut self) -> Result<()> {
		let mut extra = self.smart_render_data.smart_render_extra();

		self.call_plugin(
			after_effects::RawCommand::SmartRender,
			(&mut extra as *mut after_effects_sys::PF_SmartRenderExtra).cast(),
		)?;

		self.smart_render_data.sync();

		Ok(())
	}

	/// Whether the plugin declared `PF_OutFlag2_SUPPORTS_GPU_RENDER_F32` during
	/// `PF_Cmd_GLOBAL_SETUP`, i.e. it can render on the GPU into 32-bit-float
	/// `PF_PixelFormat_GPU_BGRA128` worlds via [`Self::render_gpu`].
	///
	/// Only meaningful once global setup has run (i.e. after [`Self::try_load`]).
	pub fn supports_gpu(&self) -> bool {
		// Escape hatch for benchmarking / debugging the CPU path even on GPU-capable
		// effects: setting AEXLO_DISABLE_GPU forces the CPU render path.
		if std::env::var_os("AEXLO_DISABLE_GPU").is_some() {
			return false;
		}
		let flag = PF_OutFlag2_SUPPORTS_GPU_RENDER_F32 as PF_OutFlags2;
		self.out_data.out_flags2 & flag != 0
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
			return Err(AexloError::Unexpected(
				"No GPU device available for GPU render".to_string(),
			));
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
	pub fn gpu_device_setdown(&mut self) -> Result<()> {
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

		self.call_plugin(
			RawCommand::SmartRenderGpu,
			(&mut extra as *mut after_effects_sys::PF_SmartRenderExtra).cast(),
		)?;

		self.smart_render_data.sync();
		Ok(())
	}

	/// Render one frame on the GPU: set up the device (once), back the input and
	/// output worlds with device buffers, upload the input as BGRA float, run the
	/// smart pre-render/GPU-render pair, wait for the GPU, and read the result back.
	///
	/// # Errors
	/// Propagates any failure from device setup or the render commands. On error the
	/// caller should fall back to CPU rendering (see [`Self::render_frame`]).
	pub fn render_gpu(&mut self) -> Result<()> {
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

			// The plugin derives its GPU dispatch size from `in_data`; keep it aligned
			// with the output frame so the whole image is rendered, not a stale
			// sub-rectangle.
			self.in_data.width = out_w as i32;
			self.in_data.height = out_h as i32;

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
				.ok_or_else(|| AexloError::Unexpected("GPU context missing".to_string()))?;
			framework = ctx.framework();
			ctx.make_current();

			if !ctx.ensure_buffer(input_key, in_w * in_h * GPU_BYTES_PER_PIXEL)
				|| !ctx.ensure_buffer(output_key, out_len)
			{
				return Err(AexloError::Unexpected(
					"Failed to allocate GPU world buffers".to_string(),
				));
			}

			// Pack + upload only when the input pixels changed since the last upload
			// (an input size change goes through `set_input`, which clears the flag,
			// so a reallocated buffer always gets fresh pixels).
			if !self.gpu_input_uploaded {
				Self::pack_layer_to_bgra_f32(&self.input_layer, &mut self.gpu_upload_staging);
				if !ctx.write_buffer(input_key, bytemuck::cast_slice(&self.gpu_upload_staging)) {
					return Err(AexloError::Unexpected(
						"Failed to upload input pixels to the GPU".to_string(),
					));
				}
				self.gpu_input_uploaded = true;
			}
		}

		self.smart_render_data.configure_gpu(self.gpu_data, 0, framework);

		// Phase B: pre-render declares regions, then the GPU render runs.
		self.render_pre()?;
		self.render_smart_gpu()?;

		// The plugin queues its work but does not wait; flush before reading back.
		if let Some(ctx) = &self.gpu_context {
			ctx.wait_for_completion();
		}

		// Phase C: read the rendered BGRA float output back into the 8-bit layer,
		// reusing the instance's staging buffer.
		if let Some(ctx) = self.gpu_context.as_ref() {
			self.gpu_readback_staging.resize(out_len / std::mem::size_of::<f32>(), 0.0);
			if ctx.read_buffer(output_key, bytemuck::cast_slice_mut(&mut self.gpu_readback_staging)) {
				Self::unpack_bgra_f32_to_layer(&self.gpu_readback_staging, &mut self.output_layer);
			}
		}

		Ok(())
	}

	/// Pack an 8-bit ARGB layer into `PF_PixelFormat_GPU_BGRA128` staging data:
	/// BGRA channel order, each channel normalised to `[0, 1]` float.
	///
	/// Writes into the caller-provided `staging` buffer (cleared first) so a
	/// render loop can reuse one allocation across frames.
	fn pack_layer_to_bgra_f32(layer: &wrapper::Layer<wrapper::Depth8>, staging: &mut Vec<f32>) {
		let pixels = layer.pixels();
		staging.clear();
		staging.reserve(pixels.len() * 4);
		for pixel in pixels {
			staging.push(pixel.blue as f32 / 255.0);
			staging.push(pixel.green as f32 / 255.0);
			staging.push(pixel.red as f32 / 255.0);
			staging.push(pixel.alpha as f32 / 255.0);
		}
	}

	/// Unpack `PF_PixelFormat_GPU_BGRA128` staging data (BGRA float) back into an
	/// 8-bit ARGB layer, clamping and rounding each channel.
	fn unpack_bgra_f32_to_layer(staging: &[f32], layer: &mut wrapper::Layer<wrapper::Depth8>) {
		for (pixel, bgra) in layer.pixels_mut().iter_mut().zip(staging.chunks_exact(4)) {
			pixel.blue = (bgra[0].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
			pixel.green = (bgra[1].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
			pixel.red = (bgra[2].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
			pixel.alpha = (bgra[3].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
		}
	}

	/// Whether the plugin declared `PF_OutFlag2_SUPPORTS_SMART_RENDER` during
	/// `PF_Cmd_GLOBAL_SETUP`, i.e. it expects the smart pre-render/render command
	/// pair ([`Self::render_pre`] + [`Self::render_smart`]) rather than the legacy
	/// [`Self::render`] path.
	///
	/// Only meaningful once global setup has run (i.e. after [`Self::try_load`]).
	pub fn supports_smart_render(&self) -> bool {
		let flag = after_effects_sys::PF_OutFlag2_SUPPORTS_SMART_RENDER as after_effects_sys::PF_OutFlags2;
		self.out_data.out_flags2 & flag != 0
	}

	/// Render one frame, preferring the smart pre-render/render pair when the plugin
	/// declared [`Self::supports_smart_render`] and falling back to the legacy
	/// [`Self::render`] command if the smart path fails (or was never declared).
	///
	/// Prefer this over calling [`Self::render`] directly in a general-purpose host:
	/// many modern effects implement `PF_Cmd_SMART_RENDER`, while others only handle
	/// the legacy command -- and some declare smart support but still render correctly
	/// through the legacy path when our smart emulation can't satisfy them.
	pub fn render_frame(&mut self) -> Result<()> {
		if self.supports_gpu() {
			match self.render_gpu() {
				Ok(()) => return Ok(()),
				Err(err) => {
					log::warn!("GPU render failed ({err:?}); falling back to CPU render.");
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
				Err(err) => {
					log::warn!("Smart render failed ({err:?}); falling back to legacy render.");
				}
			}
		}

		self.render()
	}

	/// Set the output frame size, resizing the output world and updating every
	/// place the plugin sees the frame dimensions (`in_data`, the smart-render
	/// output request rects).
	///
	/// Call this before rendering. Global/params/sequence setup runs at the
	/// default size ([`Self::output_size`] after load), which plugins tolerate --
	/// After Effects itself resizes freely between renders.
	pub fn set_render_size(&mut self, width: u32, height: u32) {
		self.output_layer = Self::build_layer(width, height, wrapper::Pixel::<wrapper::Depth8>::black());

		self.world = crate::core::helpers::LayerDefBuilder::new()
			.with_size(width as i32, height as i32)
			.build();
		self.world.data = self.output_layer.pixels_mut().as_mut_ptr() as *mut PF_Pixel;

		self.in_data.width = width as i32;
		self.in_data.height = height as i32;
		self.in_data.extent_hint = after_effects_sys::PF_UnionableRect {
			left: 0,
			top: 0,
			right: width as i32,
			bottom: height as i32,
		};

		self.smart_render_data.set_output_rect(width as i32, height as i32);
	}

	/// Get input layer dimensions in pixels (width, height).
	pub fn input_size(&self) -> (u32, u32) {
		(self.input_layer.width(), self.input_layer.height())
	}

	/// Replace the input layer, keeping the `PF_Param_LAYER` parameter (index 0) in sync.
	pub fn set_input(&mut self, input: wrapper::Layer<wrapper::Depth8>) {
		self.input_layer = input;

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
	}

	/// Write output pixels directly to an RGBA buffer (zero-allocation).
	/// The buffer must have exactly `width * height * 4` bytes.
	pub fn write_output_rgba(&self, buffer: &mut [u8]) -> Result<()> {
		self.output_layer.write_rgba_bytes(buffer)?;
		Ok(())
	}

	/// Encode the current output frame to an 8-bit RGBA PNG at `path`.
	///
	/// Full fidelity, no terminal/protocol dependence -- the point of a preview
	/// is to *see* the render, so this never degrades quality. The frame must be
	/// rendered first (call a `render_*` method before this).
	pub fn save_preview(&self, path: impl AsRef<Path>) -> Result<()> {
		let path = path.as_ref();
		let (w, h) = self.output_size();

		let mut pixels = vec![0u8; w as usize * h as usize * 4];
		self.write_output_rgba(&mut pixels)?;

		let file = std::fs::File::create(path)
			.map_err(|e| AexloError::Unexpected(format!("creating preview {}: {e}", path.display())))?;

		let options = mtpng::encoder::Options::new();
		let mut encoder = mtpng::encoder::Encoder::new(file, &options);
		let mut header = mtpng::Header::new();

		header
			.set_size(w, h)
			.and_then(|()| header.set_color(mtpng::ColorType::TruecolorAlpha, 8))
			.and_then(|()| encoder.write_header(&header))
			.and_then(|()| encoder.write_image_rows(&pixels))
			.and_then(|()| encoder.finish().map(drop))
			.map_err(|e| AexloError::Unexpected(format!("encoding preview PNG {}: {e}", path.display())))?;

		Ok(())
	}

	/// Save the current frame to a temp PNG and open it in the OS image viewer,
	/// returning the path written.
	///
	/// The dev-loop one-liner: render, then eyeball the result in a real viewer
	/// at full quality. Spawns the viewer without waiting, so it never blocks a
	/// test or `--watch` cycle.
	pub fn open_preview(&self) -> Result<PathBuf> {
		let path = std::env::temp_dir().join(format!("aexlo-preview-{}.png", std::process::id()));
		self.save_preview(&path)?;
		crate::preview::open_in_viewer(&path)?;

		Ok(path)
	}

	//---- Setter / Getter =================================

	/// Get a pointer to the instance's persistent output world (`PF_LayerDef`/`PF_EffectWorld`).
	///
	/// Used by smart-render callbacks to hand back a stable pointer instead of one
	/// pointing at a temporary value that would dangle after the callback returns.
	pub(crate) fn output_world_ptr(&mut self) -> *mut after_effects_sys::PF_EffectWorld {
		&mut self.world as *mut after_effects_sys::PF_LayerDef as *mut after_effects_sys::PF_EffectWorld
	}

	/// Get a pointer to the instance's persistent input world (`PF_LayerDef`/`PF_EffectWorld`).
	///
	/// Used by smart-render `checkout_layer_pixels` to hand back a stable pointer to
	/// the input layer, kept in sync with `input_layer` by [`Self::set_input`].
	pub(crate) fn input_world_ptr(&mut self) -> *mut after_effects_sys::PF_EffectWorld {
		&mut self.input_world as *mut after_effects_sys::PF_LayerDef as *mut after_effects_sys::PF_EffectWorld
	}

	/// Get the number of parameters.
	pub fn param_count(&self) -> usize {
		self.params.len()
	}

	/// Borrow the instance's GPU context (Metal or CUDA), if GPU rendering is active.
	///
	/// Used by [`PF_GPUDeviceSuite1`](crate::suites) callbacks (recovered via
	/// `effect_ref`) to answer `GetDeviceInfo`/`GetGPUWorldData`.
	pub(crate) fn gpu_context(&self) -> Option<&crate::gpu::GpuContext> {
		self.gpu_context.as_ref()
	}

	/// Mutably borrow the instance's GPU context, if GPU rendering is active.
	///
	/// Used by [`PF_GPUDeviceSuite1`](crate::suites) callbacks that allocate or
	/// release device memory (`AllocateDeviceMemory`/`FreeDeviceMemory`).
	pub(crate) fn gpu_context_mut(&mut self) -> Option<&mut crate::gpu::GpuContext> {
		self.gpu_context.as_mut()
	}

	/// The dynamic library this instance `dlopen`ed, if any. Used by the
	/// `get_platform_data` callback to answer plugin path queries.
	pub(crate) fn resolved_binary_path(&self) -> Option<&Path> {
		self.resolved_binary_path.as_deref()
	}

	//==== Setter / Getter =================================

	/// Set a parameter value by index.
	///
	/// Indices follow the plugin's own parameter order — the same space used by
	/// [`Self::get_param`], [`Self::param_values`], and [`Self::param_by_index`]:
	/// index 0 is the implicit input layer (not settable), real parameters start
	/// at 1.
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
		let (expected_type, expected_name) = value.expected_param_type();
		if target.param_type != expected_type as PF_ParamType {
			return Err(AexloError::ParamTypeMismatch {
				index,
				expected: expected_name,
				actual: target.param_type,
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
		}

		Ok(())
	}

	/// Notify the plugin that the user changed the parameter at `index`
	/// (`PF_Cmd_USER_CHANGED_PARAM`).
	///
	/// This is where an effect reacts to an edit — adjusting dependent parameters
	/// or requesting a UI refresh (typically by raising `PF_OutFlag_SEND_UPDATE_PARAMS_UI`,
	/// after which the host follows up with [`Self::update_params_ui`]).
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
	/// [`PF_ParamUtilsSuite3::PF_UpdateParamUI`](crate::suites). Cosmetic only — the
	/// plugin must not change parameter values in response to this command.
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
				_ => None,
			}
		}
	}

	/// Get a PluginInstance pointer from an effect reference pointer.
	///
	/// The returned pointer does not imply unique mutable access.
	/// Callers must uphold aliasing rules before dereferencing.
	pub(crate) fn get_instance_ptr(effect_ref: PF_ProgPtr) -> Option<NonNull<PluginInstance>> {
		if effect_ref.is_null() {
			return None;
		}

		NonNull::new(effect_ref as *mut PluginInstance)
	}
}

// ==== External Methods ===================================
impl PluginInstance {
	// ---- Getter ------------------------------------------
	/// Get output dimensions in pixel (width, height).
	pub fn output_size(&self) -> (u32, u32) {
		(self.output_layer.width(), self.output_layer.height())
	}
	// -----------------------------------------------------

	/// Add a parameter to this instance's parameter storage.
	///
	/// Crate-internal: this exists only to bridge `PF_Cmd_PARAMS_SETUP` -- the
	/// plugin owns its parameter list, the host never appends to it.
	pub(crate) fn add_instance_param(&mut self, param: PF_ParamDef) {
		self.params.push(param);
		self.params_dirty = true;
		self.in_data.num_params = self.params.len() as i32;
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
			target.name = def.name;
		}
	}

	/// Clear all instance parameters.
	///
	/// Crate-internal: see [`Self::add_instance_param`]. Currently unused --
	/// kept for the (re-)PARAMS_SETUP bridge described in AGENTS.md.
	#[allow(dead_code)]
	pub(crate) fn clear_instance_params(&mut self) {
		self.params.clear();
		self.params_dirty = true;
		log::debug!("PluginInstance: cleared all instance params");
	}
	// -----------------------------------------------------
}

//* ---- Internal Methods ------------------------------- */
impl PluginInstance {
	/// Create a new PluginInstance with default values.
	fn new(path: &Path) -> Self {
		let interact_callbacks = crate::host::interact::create_interact_callbacks();
		let utility_callbacks = crate::host::utility::create_utility_callbacks();
		let pica = Self::build_pica_suite();

		let input_layer = Self::build_layer(WIDTH, HEIGHT, wrapper::Pixel::<wrapper::Depth8>::green());
		let output_layer = Self::build_layer(WIDTH, HEIGHT, wrapper::Pixel::<wrapper::Depth8>::black());

		{
			let mut instance_placeholder = PluginInstance {
				raw_library: None,
				entry_point: None,
				entry_point_name: None,
				binary_file_path: path.to_path_buf(),
				resolved_binary_path: None,
				utility_callbacks,
				pica,
				in_data: crate::core::helpers::InDataBuilder::new()
					// Match the output world / default layers so the frame size the plugin
					// sees is consistent across in_data, the checkout rects, and the worlds.
					.with_size(WIDTH as i32, HEIGHT as i32)
					.with_callbacks(interact_callbacks)
					// .with_global_data(unsafe { crate::suites::handle::host_new_handle_impl(0x498) })
					.build(),
				out_data: crate::core::helpers::OutDataBuilder::new().build(),
				params: Vec::new(),
				params_dirty: false,
				params_ptr_cache: Vec::new(),
				input_layer,
				output_layer,
				world: crate::core::helpers::LayerDefBuilder::new()
					.with_size(WIDTH as i32, HEIGHT as i32)
					.build(),
				// Placeholder; wired to `input_layer`'s pixels in `wire_self_pointers`.
				input_world: crate::core::helpers::LayerDefBuilder::new()
					.with_size(WIDTH as i32, HEIGHT as i32)
					.build(),

				smart_render_data: SmartRenderData::new(),
				gpu_context: None,
				gpu_upload_staging: Vec::new(),
				gpu_readback_staging: Vec::new(),
				gpu_input_uploaded: false,
				gpu_data: null_mut(),
			};

			instance_placeholder.wire_self_pointers();
			instance_placeholder.push_input_layer_param();

			instance_placeholder
		}
	}

	/// Build the `SPBasicSuite` vtable handed to the plugin for acquiring host suites.
	fn build_pica_suite() -> Box<after_effects_sys::SPBasicSuite> {
		Box::new(after_effects_sys::SPBasicSuite {
			AcquireSuite: Some(crate::suites::rusty_acquire_suite),
			ReleaseSuite: Some(crate::suites::rusty_release_suite),
			IsEqual: None,
			AllocateBlock: None,
			FreeBlock: None,
			ReallocateBlock: None,
			Undefined: None,
		})
	}

	/// Build a `width` x `height` layer filled with `fill`.
	fn build_layer(width: u32, height: u32, fill: wrapper::Pixel<wrapper::Depth8>) -> wrapper::Layer<wrapper::Depth8> {
		wrapper::Layer::<wrapper::Depth8>::new(width, height, vec![fill; (width * height) as usize]).unwrap()
	}

	/// Point `in_data`/`world` raw pointers at this instance's own owned buffers, now
	/// that `self` has a stable address to reference.
	fn wire_self_pointers(&mut self) {
		self.in_data.utils = self.utility_callbacks.as_mut() as *mut _;
		self.in_data.pica_basicP = self.pica.as_mut() as *mut _;
		// effect_ref will be set dynamically before each plugin call
		self.in_data.effect_ref = std::ptr::null_mut();
		self.in_data.num_params = self.params.len() as i32;
		self.world.data = self.output_layer.pixels_mut().as_mut_ptr() as *mut PF_Pixel;
		self.input_world = self.input_layer.as_sys();
	}

	/// Register the implicit `PF_Param_LAYER` parameter at index 0, backed by `input_layer`.
	fn push_input_layer_param(&mut self) {
		self.params.push(PF_ParamDef {
			uu: after_effects_sys::PF_ParamDef__bindgen_ty_1 { id: 0 },
			ui_flags: 0,
			ui_width: 0,
			ui_height: 0,
			param_type: 0 as PF_ParamType,
			name: [0; 32],
			flags: 0,
			unused: 0,
			u: PF_ParamDefUnion {
				ld: self.input_layer.as_sys(),
			},
		});
		// The push above invalidates any (nonexistent yet) cached param pointers;
		// mark dirty so `call_plugin` builds the cache on its first invocation.
		self.params_dirty = true;
	}

	/// Get the output message from the instance (set during `PF_Cmd_ABOUT` command).
	///
	/// The message may contain line breaks and special characters (e.g. \r, \n).
	/// Invalid UTF-8 sequences are replaced with the Unicode replacement character (�).
	fn message(&self) -> String {
		let bytes = &self.out_data.return_msg;

		// Cramp the buffer at the first null byte (if any) to avoid trailing garbage
		let cramped_length = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());

		// SAFETY: `c_char` and `u8` share size and alignment; only the sign interpretation differs, which is irrelevant when reading raw bytes.
		let utf8: &[u8] = unsafe { std::slice::from_raw_parts(bytes.as_ptr() as *const u8, cramped_length) };

		String::from_utf8_lossy(utf8).into_owned()
	}

	/// Call the plugin with `PF_Cmd_GLOBAL_SETUP` command.
	fn setup_global(&mut self) -> Result<()> {
		self.call_plugin(RawCommand::GlobalSetup, null_mut())
	}

	/// Call the plugin with `PF_Cmd_PARAMS_SETUP` command.
	fn setup_params(&mut self) -> Result<()> {
		self.call_plugin(RawCommand::ParamsSetup, null_mut())
	}

	/// Call the plugin with `PF_Cmd_SEQUENCE_SETUP` command.
	///
	/// This gives the plugin a chance to allocate its per-instance
	/// `sequence_data` (which [`Self::call_plugin`] then propagates back into
	/// `in_data` for subsequent commands). Some effects also perform work here
	/// that later render commands depend on -- e.g. plugins built on the
	/// aescripts licensing library run their license check during
	/// `SEQUENCE_SETUP`/`SEQUENCE_RESETUP`, and render a watermark if it never
	/// runs -- so we always issue it before rendering.
	///
	/// A fresh setup expects a null incoming `sequence_data`; make sure any
	/// stale pointer is cleared so the plugin allocates from scratch rather than
	/// treating garbage as a flattened blob to resurrect.
	fn setup_sequence(&mut self) -> Result<()> {
		self.in_data.sequence_data = null_mut();
		self.out_data.sequence_data = null_mut();
		self.call_plugin(RawCommand::SequenceSetup, null_mut())
	}

	/// Ask the plugin what its real entry point symbol is via the modern
	/// `PluginDataEntryFunction2` protocol -- the same self-description mechanism
	/// After Effects itself uses instead of parsing a binary PiPL resource.
	fn query_declared_entry_point(lib: &Library, pica: &after_effects_sys::SPBasicSuite) -> Option<String> {
		let entry_fn = unsafe { lib.symbol::<PluginDataEntryFn>(PLUGIN_DATA_ENTRY_SYMBOL) }.ok()?;

		let mut info = PluginDataInfo::default();
		let host_name = CString::new(HOST_NAME).ok()?;
		let host_version = CString::new(HOST_VERSION).ok()?;

		let result = unsafe {
			entry_fn(
				&mut info as *mut PluginDataInfo as after_effects_sys::PF_PluginDataPtr,
				Some(receive_plugin_data),
				pica as *const after_effects_sys::SPBasicSuite,
				host_name.as_ptr(),
				host_version.as_ptr(),
			)
		};

		if result != PF_Err_NONE as after_effects_sys::PF_Err {
			log::debug!("{} reported error code {}.", PLUGIN_DATA_ENTRY_SYMBOL, result);
			return None;
		}

		info.entry_point_name
	}

	/// Resolve the plugin's entry point, preferring the name declared via
	/// [`Self::query_declared_entry_point`] and falling back to
	/// [`FALLBACK_ENTRY_POINT_CANDIDATES`] if that's unavailable or unresolvable.
	fn resolve_entry_point(
		lib: &Library,
		pica: &after_effects_sys::SPBasicSuite,
	) -> Result<(PluginEntryPoint, String)> {
		if let Some(name) = Self::query_declared_entry_point(lib, pica) {
			match unsafe { lib.symbol::<PluginEntryPoint>(name.as_str()) } {
				Ok(symbol) => {
					log::info!(
						"Resolved entry point '{}' via {}.",
						name.blue(),
						PLUGIN_DATA_ENTRY_SYMBOL
					);
					return Ok((symbol, name));
				}
				Err(err) => log::debug!("Declared entry point '{}' not resolvable: {}", name, err),
			}
		}

		let mut last_error = None;

		for candidate in FALLBACK_ENTRY_POINT_CANDIDATES {
			match unsafe { lib.symbol::<PluginEntryPoint>(candidate) } {
				Ok(symbol) => return Ok((symbol, candidate.to_string())),
				Err(err) => {
					log::debug!("Entry point symbol '{}' not resolved: {}", candidate, err);
					last_error = Some(err);
				}
			}
		}

		if let Some(err) = last_error {
			Err(err.into())
		} else {
			Err(AexloError::InvalidPath {
				message: "No entry point candidates configured".to_string(),
			})
		}
	}

	/// Resolve `artifact_path` -- the plugin as it exists on disk -- to the concrete
	/// dynamic library that should be `dlopen`'d.
	///
	/// Callers hand us whatever they'd double-click to install the plugin: a bare
	/// `.aex`/`.dll` file on Windows, or a `.plugin` bundle directory on macOS. Rather
	/// than branching on the compiled/runtime OS (which breaks the moment a flat test
	/// `.dylib` is loaded on macOS, or a bundle is inspected from another host), we
	/// dispatch on the shape of `artifact_path` itself: a directory is a bundle to dig
	/// into, a file is already the binary to load.
	fn resolve_binary_path(artifact_path: &Path) -> Result<PathBuf> {
		if artifact_path.is_dir() {
			return Self::resolve_bundle_binary(artifact_path);
		}

		if artifact_path.is_file() {
			return Ok(artifact_path.to_path_buf());
		}

		Err(AexloError::PluginNotFound {
			path: artifact_path.display().to_string(),
		})
	}

	/// Find the executable inside a macOS `.plugin` bundle's `Contents/MacOS/`.
	///
	/// AE plugin bundles are required to contain exactly one binary there, so we
	/// use that instead of assuming the binary is named after the bundle.
	fn resolve_bundle_binary(bundle_path: &Path) -> Result<PathBuf> {
		let macos_dir = bundle_path.join("Contents").join("MacOS");

		let mut binaries = std::fs::read_dir(&macos_dir)
			.map_err(|_| AexloError::PluginNotFound {
				path: macos_dir.display().to_string(),
			})?
			.filter_map(|entry| entry.ok())
			.map(|entry| entry.path())
			.filter(|path| path.is_file());

		match (binaries.next(), binaries.next()) {
			(Some(binary), None) => Ok(binary),
			(None, _) => Err(AexloError::PluginNotFound {
				path: macos_dir.display().to_string(),
			}),
			(Some(_), Some(_)) => Err(AexloError::InvalidPath {
				message: format!(
					"Ambiguous bundle '{}': expected exactly one executable in Contents/MacOS",
					bundle_path.display()
				),
			}),
		}
	}

	/// Resolve `self.path` to a binary, `dlopen` it, and resolve its entry point,
	/// storing the results on `self`.
	fn load(&mut self) -> Result<()> {
		let module_path = Self::resolve_binary_path(&self.binary_file_path)?;
		let module_path_str = module_path.display().to_string();

		log::info!("Loading plugin from '{}'.", module_path_str.blue());

		let lib = Library::open(&module_path)?;
		let (entry_point, resolved_name) = Self::resolve_entry_point(&lib, self.pica.as_ref())?;

		self.entry_point = Some(entry_point);
		self.entry_point_name = Some(resolved_name.clone());
		self.raw_library = Some(lib);

		// Remembered for the get_platform_data callback (plugin path queries).
		self.resolved_binary_path = Some(module_path.clone());

		log::info!("Resolved entry point symbol: {}.", resolved_name.blue());
		log::info!("Loaded plugin '{}' {}.", module_path_str.blue(), "successfully".green());

		Ok(())
	}

	/// Invoke the resolved entry point with `self.cmd`, updating `self` before and
	/// after the call so the next invocation sees a consistent state.
	///
	/// Before calling: points `in_data.effect_ref` at `self` (so suite callbacks
	/// can recover the instance via [`Self::get_instance_ptr`]), and rebuilds the
	/// cached param pointer list if `params` was mutated since the last call.
	///
	/// After calling: copies a non-null `out_data.global_data`/`sequence_data` back
	/// into `in_data`, so plugin-allocated state persists across subsequent commands.
	///
	/// `extra_data` is the command-specific extra struct (e.g. `PF_PreRenderExtra`
	/// for `SmartPreRender`), or null for commands that don't take one.
	///
	/// # Errors
	/// Returns [`AexloError::ContainerNotLoaded`] if no entry point has been
	/// resolved yet (see [`Self::load`]), before any other state is touched.
	/// Returns [`AexloError::PluginExecutionFailed`] if the plugin returns a
	/// non-`PF_Err_NONE` code.
	fn call_plugin(&mut self, command: RawCommand, extra_data: *mut ::std::os::raw::c_void) -> Result<()> {
		let entry_point = self.entry_point.ok_or(AexloError::ContainerNotLoaded)?;

		// Update effect_ref to point to self before calling the plugin
		self.in_data.effect_ref = self as *mut _ as PF_ProgPtr;

		let entry_point_name = self.entry_point_name.as_deref().unwrap_or(DEFAULT_ENTRY_POINT_NAME);

		// debug, not info: this runs for every render command, so at info level a
		// preview loop drowns the log.
		log::debug!("Executing command: {}", format!("{:?}", command).blue());

		if self.params_dirty {
			self.params_ptr_cache = self.params.iter_mut().map(|p| p as *mut _).collect();
			self.params_dirty = false;
		}

		let result = unsafe {
			entry_point(
				command,
				&mut self.in_data,
				&mut self.out_data,
				self.params_ptr_cache.as_mut_ptr(),
				&mut self.world,
				extra_data,
			)
		};

		#[cfg(target_os = "macos")]
		let result = result as u32;

		if !self.out_data.global_data.is_null() {
			self.in_data.global_data = self.out_data.global_data;
		}

		if !self.out_data.sequence_data.is_null() {
			self.in_data.sequence_data = self.out_data.sequence_data;
		}

		//* ---- Check for errors ---------------------- *//
		#[allow(non_upper_case_globals)]
		match result {
			PF_Err_NONE => {
				log::debug!(
					"Executed command '{}' {} ({} exited with code {}).",
					format!("{:?}", command).blue(),
					"successfully".green(),
					entry_point_name.blue(),
					result.to_string().blue()
				);
			}
			code => {
				// The format! only runs on the failure path, never per frame.
				return Err(AexloError::PluginExecutionFailed {
					command: format!("{command:?}"),
					code: code.into(),
				});
			}
		}
		//* -------------------------------------------- *//

		Ok(())
	}
}

impl Drop for PluginInstance {
	/// Best-effort teardown mirroring After Effects' shutdown order: release the
	/// plugin's GPU data, then `PF_Cmd_SEQUENCE_SETDOWN`, then
	/// `PF_Cmd_GLOBAL_SETDOWN`, so plugin-allocated state (sequence/global
	/// handles, license threads, GPU pipelines) is freed instead of leaking.
	///
	/// Failures are logged, never propagated — panicking in `drop` would abort,
	/// and a half-torn-down plugin is about to be unloaded anyway.
	fn drop(&mut self) {
		if let Err(err) = self.gpu_device_setdown() {
			log::warn!("PF_Cmd_GPU_DEVICE_SETDOWN failed during drop: {err:?}");
		}

		// Without an entry point nothing was ever set up, so there is nothing to
		// tear down (and call_plugin would just error).
		if self.entry_point.is_none() {
			return;
		}

		if let Err(err) = self.call_plugin(RawCommand::SequenceSetdown, null_mut()) {
			log::warn!("PF_Cmd_SEQUENCE_SETDOWN failed during drop: {err:?}");
		}
		if let Err(err) = self.call_plugin(RawCommand::GlobalSetdown, null_mut()) {
			log::warn!("PF_Cmd_GLOBAL_SETDOWN failed during drop: {err:?}");
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	/// A zeroed `PF_ParamDef` of the given type, bypassing `add_instance_param`'s
	/// default-normalisation concerns (all defaults are zero anyway).
	fn param_of_type(param_type: ParamType) -> PF_ParamDef {
		let mut def: PF_ParamDef = unsafe { std::mem::zeroed() };
		def.param_type = param_type as PF_ParamType;
		def
	}

	/// An instance with no plugin loaded: params[0] is the implicit input layer.
	fn bare_instance() -> PluginInstance {
		PluginInstance::new(Path::new("<test>"))
	}

	#[test]
	fn param_index_space_is_shared_across_accessors() {
		let mut fx = bare_instance();
		let mut float_def = param_of_type(ParamType::FloatSlider);
		float_def.u.fs_d.value = 1.5;
		fx.add_instance_param(float_def);

		// Index 0 is the input layer: unreadable, unsettable.
		assert!(fx.get_param(0).is_none());
		assert!(fx.set_param(0, ParamValue::Float(0.0)).is_err());

		// The first real parameter lives at index 1 in every accessor.
		assert_eq!(fx.get_param(1), Some(ParamValue::Float(1.5)));
		assert_eq!(fx.param_values(), vec![(1, ParamValue::Float(1.5))]);
		assert_eq!(
			fx.param_by_index(1).map(|def| def.param_type),
			Some(ParamType::FloatSlider as PF_ParamType)
		);

		fx.set_param(1, ParamValue::Float(2.0)).unwrap();
		assert_eq!(fx.get_param(1), Some(ParamValue::Float(2.0)));

		// Out of bounds and type mismatches are rejected.
		assert!(fx.set_param(2, ParamValue::Float(0.0)).is_err());
		assert!(fx.set_param(1, ParamValue::Checkbox(true)).is_err());
	}

	#[test]
	fn set_render_size_updates_every_dimension_view() {
		let mut fx = bare_instance();
		fx.set_render_size(640, 360);

		assert_eq!(fx.output_size(), (640, 360));
		assert_eq!((fx.in_data.width, fx.in_data.height), (640, 360));
		assert_eq!((fx.world.width, fx.world.height), (640, 360));
		// The world must point at the freshly sized output layer's pixels.
		assert_eq!(fx.world.data as *const _, fx.output_layer.pixels().as_ptr());
		assert_eq!(fx.world.rowbytes, 640 * 4);
	}

	#[test]
	fn fixed_slider_uses_16_16_fixed_point() {
		let mut fx = bare_instance();
		fx.add_instance_param(param_of_type(ParamType::FixSlider));

		fx.set_param(1, ParamValue::Fixed(2.5)).unwrap();

		// The raw union value must be PF_Fixed (16.16), the encoding plugins read.
		let raw = unsafe { fx.param_by_index(1).unwrap().u.fd.value };
		assert_eq!(raw, 2 * 65536 + 32768);
		assert_eq!(fx.get_param(1), Some(ParamValue::Fixed(2.5)));
	}
}
