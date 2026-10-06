use crate::core::error::{AexloError, Result};
use crate::core::in_data::{CallBindings, EffectState, HostInfo as InDataHost, RenderContext};
use crate::host::app::Host;
use crate::host::host_layer::HostLayer;
use crate::host::layer_param::LinkedLayer;
use crate::host::smart_render::SmartRenderData;
use crate::observe::{CommandEvent, CommandPhase, Installed, ObserveLevel, Observer};
use crate::param_value::ParamValue;
use crate::utils;

use crate::gpu::GPU_BYTES_PER_PIXEL;
use after_effects::{ParamType, RawCommand};
use after_effects_sys::{
	PF_ArbParamsExtra, PF_Arbitrary_COPY_FUNC, PF_Arbitrary_DISPOSE_FUNC, PF_Arbitrary_NEW_FUNC, PF_ArbitraryH,
	PF_Err_INVALID_CALLBACK, PF_Err_NONE, PF_GPU_Framework, PF_GPU_Framework_NONE, PF_GPUDeviceSetdownExtra,
	PF_GPUDeviceSetdownInput, PF_GPUDeviceSetupExtra, PF_GPUDeviceSetupInput, PF_GPUDeviceSetupOutput,
	PF_OutFlag2_SUPPORTS_GPU_RENDER_F32, PF_OutFlags, PF_OutFlags2, PF_ParamDef, PF_ParamDefUnion, PF_ParamType,
	PF_ProgPtr,
};
use colored::Colorize;
use dlopen2::raw::Library;
use std::{
	collections::HashMap,
	ffi::{CStr, CString, c_void},
	path::{Path, PathBuf},
	ptr::NonNull,
	ptr::null_mut,
	sync::Arc,
	time::Instant,
};
use wrapper::{AnyLayer, Layer, PixelDepth, PixelDepthKind};

use crate::core::constants::{DEFAULT_HEIGHT as HEIGHT, DEFAULT_WIDTH as WIDTH};

const DEFAULT_ENTRY_POINT_NAME: &str = "EffectMain";

/// Normalized `[r, g, b, a]` of the fresh output world AE hands a plugin.
const OPAQUE_BLACK: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

/// Entry point names to try if the plugin doesn't implement `PluginDataEntryFunction2`.
const FALLBACK_ENTRY_POINT_CANDIDATES: &[&str] = &[DEFAULT_ENTRY_POINT_NAME, "EntryPointFunc"];
/// Fixed symbol name of the AE SDK's self-describing plugin data entry function.
const PLUGIN_DATA_ENTRY_SYMBOL: &str = "PluginDataEntryFunction2";
const HOST_NAME: &str = "AfterEffects";
const HOST_VERSION: &str = "25.2";

/// ABI of an After Effects effect entry point (`EffectMain`): the fixed
/// `(cmd, in_data, out_data, params, output, extra)` signature every effect
/// exports. Handed to [`Host::from_entry`] to drive an in-process
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

/// Independent instance of an After Effects Plug-in.
/// It is created by loading a plug-in with [`Host::try_load`] or by driving an in-process entry point with [`Host::from_entry`].
/// Manages its library, entry point, parameters, and execution state.
/// All operations related to the plug-in should be invoked from this.
pub struct PluginInstance {
	raw_library: Option<Library>,
	entry_point: Option<PluginEntryPoint>,
	entry_point_name: Option<String>,
	binary_file_path: PathBuf,

	/// The dynamic library actually `dlopen`ed (resolved from
	/// `binary_file_path`, e.g. the binary inside a `.plugin` bundle), or `None`
	/// for in-process entry points. Reported to the plugin through
	/// `get_platform_data` (`PF_PlatData_EXE_FILE_PATH_W`, ...) - per instance,
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

	/// Host-wide constants composed into every `PF_InData`.
	host_info: InDataHost,
	/// Plugin-owned handles carried across commands.
	effect_state: EffectState,
	/// Frame the next command operates on.
	pub(crate) render_context: RenderContext,
	out_data: after_effects_sys::PF_OutData,
	/// `out_flags` as declared at `PF_Cmd_GLOBAL_SETUP`.
	global_out_flags: PF_OutFlags,
	/// `out_flags2` as declared at `PF_Cmd_GLOBAL_SETUP`.
	global_out_flags2: PF_OutFlags2,

	/// Instance-specific parameters from the host (non-global storage).
	params: Vec<after_effects_sys::PF_ParamDef>,

	/// Popup choice labels captured per parameter at `PF_Cmd_PARAMS_SETUP`,
	/// parallel to `params` (empty for non-popup params). Captured at add time
	/// because a popup's `namesptr` points into a plugin-owned string that only
	/// lives for the add-param callback; reading it later is a dangling read.
	popup_choices: Vec<Vec<String>>,

	/// Track if instance params need synchronization.
	params_dirty: bool,

	/// Raw pointers into `params`, passed to the plugin entry point on each call.
	/// Rebuilt lazily in `call_plugin` whenever `params_dirty` is set, since pushing
	/// to `params` may reallocate its backing buffer and invalidate old pointers.
	params_ptr_cache: Vec<*mut PF_ParamDef>,

	/// The effect's own layer. Its depth is the project depth: the output
	/// follows it (see [`Self::set_input_layer`]).
	pub(crate) input_layer: HostLayer,
	pub(crate) output_layer: HostLayer,

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

	/// Label the plugin gave its Options button via `PF_SetOptionsButtonName`.
	options_button_name: Option<String>,

	/// Pixel formats (FourCCs) registered through the "PF Pixel Format Suite".
	supported_pixel_formats: Vec<u32>,

	/// Masks on the effect's layer, served through the Path Query/Data suites.
	mask_paths: Vec<crate::MaskPath>,

	/// Layers linked to the plugin's own `PF_Param_LAYER` parameters, by index.
	linked_layers: HashMap<usize, LinkedLayer>,

	/// Smart-render checkout ids mapped to the layer parameter they checked
	/// out, recorded by `checkout_layer` during pre-render.
	layer_checkouts: HashMap<after_effects_sys::A_long, usize>,

	/// Whether iterate callbacks may run on worker threads (see
	/// [`Self::set_parallel_iterate`]).
	parallel_iterate: bool,

	/// Indices of the arbitrary-data parameters whose value handle the plugin
	/// created for us at setup (see [`Self::setup_arbitrary_params`]); handed
	/// back through `PF_Arbitrary_DISPOSE_FUNC` on drop.
	owned_arb_values: Vec<usize>,

	/// Who is told about commands and calls (see [`Self::set_observer`]).
	observer: Option<Installed>,

	/// Strict-mode instrumentation (see [`Self::set_strict`]).
	strict: crate::Strict,
	/// Strict-mode bookkeeping, shared with host callbacks during commands.
	tracker: crate::strict::SharedTracker,
	/// The guarded copy of the output a CPU render writes into (strict mode).
	output_guard: Option<crate::strict::Guarded>,
	/// Whether the plugin has been set down (see [`Self::finish`]).
	torn_down: bool,
}

/// Plugin constructors. Taking a [`Host`] guarantees the process-wide
/// [`AppHost`](crate::AppHost) is fixed before any plugin code runs.
impl Host {
	/// Load a plugin from `path` under this host, then run it through global and params setup.
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
	pub fn try_load(self, path: impl AsRef<Path>) -> Result<PluginInstance> {
		self.try_load_with(path, crate::Strict::default())
	}

	/// [`Self::try_load`] with strict mode on from the very first command, so
	/// what the plugin allocates during setup is tracked too (see
	/// [`PluginInstance::finish`]).
	pub fn try_load_with(self, path: impl AsRef<Path>, strict: crate::Strict) -> Result<PluginInstance> {
		let mut instance = PluginInstance::new(path.as_ref());
		instance.set_strict(strict);

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
	pub unsafe fn from_entry(self, entry: PluginEntryPoint) -> Result<PluginInstance> {
		let mut instance = PluginInstance::new(Path::new("<in-process>"));

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
	pub unsafe fn from_entry_raw(self, entry_addr: usize) -> Result<PluginInstance> {
		unsafe { self.from_entry_raw_with(entry_addr, crate::Strict::default()) }
	}

	/// [`Self::from_entry_raw`] with strict mode on from the first command.
	///
	/// # Safety
	/// As [`Self::from_entry_raw`].
	pub unsafe fn from_entry_raw_with(self, entry_addr: usize, strict: crate::Strict) -> Result<PluginInstance> {
		let entry: PluginEntryPoint = unsafe { std::mem::transmute(entry_addr) };
		let mut instance = PluginInstance::new(Path::new("<in-process>"));
		instance.set_strict(strict);

		instance.entry_point = Some(entry);
		instance.entry_point_name = Some(DEFAULT_ENTRY_POINT_NAME.to_string());
		instance.finalize()?;

		Ok(instance)
	}
}

impl PluginInstance {
	/// Run the post-load setup shared by every constructor: `GLOBAL_SETUP`,
	/// `PARAMS_SETUP`, then `SEQUENCE_SETUP`. Independent of where the entry point
	/// came from (`dlopen`ed or handed in directly).
	fn finalize(&mut self) -> Result<()> {
		self.setup_global()?;
		self.setup_params()?;
		self.setup_arbitrary_params();
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
		self.reset_cpu_worlds();
		self.poison_output();
		self.begin_output_guard();
		let result = self.call_plugin(RawCommand::Render, null_mut());
		self.end_output_guard();

		result
	}

	/// Call the plugin with `PF_Cmd_SMART_PRE_RENDER` command, letting it declare the
	/// input/output checkout regions it needs via [`Self::render_smart`].
	pub fn render_pre(&mut self) -> Result<()> {
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
	pub fn render_smart(&mut self) -> Result<()> {
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
				Self::pack_layer_to_bgra_f32(self.input_layer.layer(), &mut self.gpu_upload_staging);
				if !ctx.write_buffer(input_key, bytemuck::cast_slice(&self.gpu_upload_staging)) {
					return Err(AexloError::Unexpected(
						"Failed to upload input pixels to the GPU".to_string(),
					));
				}
				self.gpu_input_uploaded = true;
			}

			// Layers linked to layer parameters get their own device buffers.
			for linked in self.linked_layers.values_mut() {
				let key = linked.world_ptr(true) as usize;
				if !ctx.ensure_buffer(key, linked.gpu_len()) {
					return Err(AexloError::Unexpected(
						"Failed to allocate GPU layer-parameter buffer".to_string(),
					));
				}
				if !linked.gpu_uploaded {
					Self::pack_layer_to_bgra_f32(linked.layer(), &mut self.gpu_upload_staging);
					if !ctx.write_buffer(key, bytemuck::cast_slice(&self.gpu_upload_staging)) {
						return Err(AexloError::Unexpected(
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
	/// pair ([`Self::render_pre`] + [`Self::render_smart`]) rather than the legacy
	/// [`Self::render`] path.
	///
	/// Only meaningful once global setup has run (i.e. after [`Self::try_load`]).
	pub fn supports_smart_render(&self) -> bool {
		let flag = after_effects_sys::PF_OutFlag2_SUPPORTS_SMART_RENDER as after_effects_sys::PF_OutFlags2;
		self.global_out_flags2 & flag != 0
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

		self.render()
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

	/// Set the time the next commands operate on: `current_time / time_scale`
	/// seconds into the comp, one frame being `time_step / time_scale` seconds
	/// (so frame `n` of a 30 fps comp is `set_time(n, 1, 30)`).
	///
	/// The layer and comp the plugin sees through the AEGP suites run at the
	/// same frame rate. Plugins start at frame 0 of a 30 fps comp.
	///
	/// # Panics
	/// If `time_step` or `time_scale` is not positive: plugins divide by both.
	pub fn set_time(&mut self, current_time: i32, time_step: i32, time_scale: u32) {
		assert!(
			time_step > 0 && time_scale > 0,
			"time_step and time_scale must be positive"
		);
		self.render_context.set_time(current_time, time_step, time_scale);
	}

	/// The time set by [`Self::set_time`], as `(current_time, time_step, time_scale)`.
	pub fn time(&self) -> (i32, i32, u32) {
		self.render_context.time()
	}

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
	/// The buffer must have exactly `width * height * 4` bytes. Deeper output
	/// is quantized to 8 bits per channel.
	pub fn write_rendered_pixels(&self, buffer: &mut [u8]) -> Result<()> {
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
	/// Only meaningful once global setup has run (i.e. after [`Host::try_load`]).
	pub fn supports_depth(&self, depth: PixelDepthKind) -> bool {
		match depth {
			PixelDepthKind::U8 => true,
			PixelDepthKind::U16 => {
				self.global_out_flags & after_effects_sys::PF_OutFlag_DEEP_COLOR_AWARE as PF_OutFlags != 0
			}
			PixelDepthKind::F32 => {
				self.global_out_flags2 & after_effects_sys::PF_OutFlag2_FLOAT_COLOR_AWARE as PF_OutFlags2 != 0
			}
		}
	}

	/// `out_flags` as the plugin declared them at `PF_Cmd_GLOBAL_SETUP`.
	pub fn out_flags(&self) -> i32 {
		self.global_out_flags
	}

	/// `out_flags2` as the plugin declared them at `PF_Cmd_GLOBAL_SETUP`.
	pub fn out_flags2(&self) -> i32 {
		self.global_out_flags2
	}

	/// Turn strict-mode instrumentation on or off for this instance (see
	/// [`Strict`](crate::Strict)). With everything off, the host behaves and
	/// costs exactly as without strict mode.
	pub fn set_strict(&mut self, strict: crate::Strict) {
		self.strict = strict;
		if let Ok(mut tracker) = self.tracker.lock() {
			tracker.strict = strict;
		}
	}

	/// The strict-mode features in effect.
	pub fn strict(&self) -> crate::Strict {
		self.strict
	}

	/// What strict mode found since the last call (violations are reported
	/// once). Unwritten output pixels are not violations the host can judge;
	/// count them with [`unwritten_pixels`](crate::unwritten_pixels) on
	/// [`Self::output`] after a render.
	pub fn strict_report(&mut self) -> crate::StrictReport {
		self.tracker
			.lock()
			.map(|mut t| std::mem::take(&mut t.report))
			.unwrap_or_default()
	}

	/// Tear the plugin down (`SEQUENCE_SETDOWN`, `GLOBAL_SETDOWN`, as on drop)
	/// and return what strict mode found, including handles and worlds the
	/// plugin never released ([`Strict::track_allocations`](crate::Strict)).
	pub fn finish(mut self) -> crate::StrictReport {
		self.teardown();
		self.strict_report()
	}

	/// Route host callbacks on this thread to this instance's tracker while
	/// the guard lives, when strict mode or an observer wants them.
	fn tracking(&self) -> Option<crate::strict::TrackGuard> {
		let wanted = self.strict.any() || self.observer.is_some();
		wanted.then(|| crate::strict::enter(Some(self.tracker.clone())))
	}

	/// Release the plugin's pre-render data, attributing what it frees.
	fn dispose_pre_render(&mut self) {
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
	fn end_output_guard(&mut self) {
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

	/// Tell `observer` about every command sent to the plugin from now on and,
	/// at [`ObserveLevel::Calls`], every suite function it calls back on the
	/// dispatching thread. `None` removes it.
	pub fn set_observer(&mut self, observer: Option<Arc<dyn Observer>>, level: ObserveLevel) {
		if let Ok(mut tracker) = self.tracker.lock() {
			tracker.observer = observer.clone();
		}
		self.observer = observer.map(|observer| Installed { observer, level });
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
	/// index 0 is the implicit input layer (not settabl-), real parameters start
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
	/// after which the host follows up with [`Sel-::update_params_ui`]).
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

//* ---- External Methods --------------------------------------------------- */
impl PluginInstance {
	// ---- Getter ------------------------------------------
	/// Get input layer dimensions in pixels (width, height).
	pub fn input_size(&self) -> (u32, u32) {
		(self.input_layer.width(), self.input_layer.height())
	}

	/// Get output dimensions in pixel (width, height).
	pub fn output_size(&self) -> (u32, u32) {
		(self.output_layer.width(), self.output_layer.height())
	}

	/// The label the plugin set for its Options button
	/// (`PF_EffectUISuite1::PF_SetOptionsButtonName`), if any.
	pub fn options_button_name(&self) -> Option<&str> {
		self.options_button_name.as_deref()
	}

	pub(crate) fn set_options_button_name(&mut self, name: String) {
		self.options_button_name = Some(name);
	}

	/// Pixel formats (`PF_PixelFormat` / `PrPixelFormat` FourCCs) the plugin
	/// registered via the "PF Pixel Format Suite", in registration order. 8-bit
	/// ARGB is implied and only listed if registered explicitly.
	pub fn supported_pixel_formats(&self) -> &[u32] {
		&self.supported_pixel_formats
	}

	pub(crate) fn add_supported_pixel_format(&mut self, format: u32) {
		if !self.supported_pixel_formats.contains(&format) {
			self.supported_pixel_formats.push(format);
		}
	}

	pub(crate) fn clear_supported_pixel_formats(&mut self) {
		self.supported_pixel_formats.clear();
	}

	/// The masks on the effect's layer (see [`Self::set_mask_paths`]).
	pub fn mask_paths(&self) -> &[crate::MaskPath] {
		&self.mask_paths
	}

	/// Replace the masks on the effect's layer. Plugins see them through the
	/// "PF Path Query Suite" / "PF Path Data Suite" (e.g. for `PF_Param_PATH`
	/// parameters, which reference a mask by [`MaskPath::id`](crate::MaskPath::id)).
	pub fn set_mask_paths(&mut self, paths: Vec<crate::MaskPath>) {
		self.mask_paths = paths;
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
		if param_type != ParamType::Layer as PF_ParamType {
			return Err(AexloError::ParamTypeMismatch {
				index,
				expected: "Layer",
				actual: param_type,
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
	pub fn clear_layer_param(&mut self, index: usize) -> Result<()> {
		self.set_layer_param::<wrapper::Depth8>(index, None)
	}

	/// Whether the plugin's iterate callbacks (`PF_Iterate*Suite`, the legacy
	/// `utils->iterate*` and `iterate_generic`) may run on worker threads.
	///
	/// On by default, as in After Effects. Turn it off for plugins whose pixel
	/// callbacks rely on running on the thread that called iterate -- notably
	/// plugins built on the Rust `after-effects` crate that acquire suites
	/// inside the callback (its suite pointer lives in a thread-local set on
	/// entry to `EffectMain`, so such a call panics on a worker thread and
	/// aborts the process). Every callback then runs in order on the calling
	/// thread.
	pub fn set_parallel_iterate(&mut self, parallel: bool) {
		self.parallel_iterate = parallel;
	}

	/// See [`Self::set_parallel_iterate`].
	pub fn parallel_iterate(&self) -> bool {
		self.parallel_iterate
	}

	/// The layer linked to the layer parameter at `index`, if any.
	pub fn layer_param(&self, index: usize) -> Option<&AnyLayer> {
		self.linked_layers.get(&index).map(LinkedLayer::layer)
	}

	/// Record that smart-render `checkout_id` refers to the layer parameter at `index`.
	pub(crate) fn record_layer_checkout(&mut self, checkout_id: after_effects_sys::A_long, index: usize) {
		self.layer_checkouts.insert(checkout_id, index);
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
	/// the input layer (or an id never checked out), the linked layer's CPU or
	/// GPU world for a layer parameter, `None` for an unlinked one.
	pub(crate) fn checked_out_world(
		&mut self,
		checkout_id: after_effects_sys::A_long,
	) -> Option<*mut after_effects_sys::PF_EffectWorld> {
		let gpu = self.smart_render_data.is_gpu();
		match self.layer_checkouts.get(&checkout_id).copied().unwrap_or(0) {
			0 => Some(self.input_world_ptr()),
			index => self
				.linked_layers
				.get_mut(&index)
				.map(|l| l.world_ptr(gpu) as *mut after_effects_sys::PF_EffectWorld),
		}
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
	// -----------------------------------------------------
}

//* ---- Internal Methods --------------------------------------------------- */
impl PluginInstance {
	/// Create a new PluginInstance with default values.
	fn new(path: &Path) -> Self {
		let interact_callbacks = crate::host::interact::create_interact_callbacks();
		let utility_callbacks = crate::host::utility::create_utility_callbacks();
		let pica = Self::build_pica_suite();

		let input_layer = HostLayer::new(AnyLayer::new(Self::build_layer(
			WIDTH,
			HEIGHT,
			wrapper::Pixel::<wrapper::Depth8>::green(),
		)));
		let output_layer = HostLayer::new(AnyLayer::filled(PixelDepthKind::U8, WIDTH, HEIGHT, OPAQUE_BLACK));

		{
			let mut instance_placeholder = PluginInstance {
				raw_library: None,
				entry_point: None,
				entry_point_name: None,
				binary_file_path: path.to_path_buf(),
				resolved_binary_path: None,
				utility_callbacks,
				pica,
				host_info: InDataHost::new(interact_callbacks),
				effect_state: EffectState::default(),
				// Match the output world / default layers so the frame size the plugin
				// sees is consistent across in_data, the checkout rects, and the worlds.
				render_context: {
					let mut ctx = RenderContext::default();
					ctx.set_size(WIDTH as i32, HEIGHT as i32);
					ctx
				},
				out_data: crate::core::helpers::OutDataBuilder::new().build(),
				global_out_flags: 0,
				global_out_flags2: 0,
				params: Vec::new(),
				popup_choices: Vec::new(),
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
				options_button_name: None,
				supported_pixel_formats: Vec::new(),
				mask_paths: Vec::new(),
				linked_layers: HashMap::new(),
				layer_checkouts: HashMap::new(),
				parallel_iterate: true,
				owned_arb_values: Vec::new(),
				observer: None,
				strict: crate::Strict::default(),
				tracker: Default::default(),
				output_guard: None,
				torn_down: false,
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
	fn sync_output_world(&mut self) {
		let layer = self.output_layer.as_sys();
		self.world.width = layer.width;
		self.world.height = layer.height;
		self.world.rowbytes = layer.rowbytes;
		self.world.world_flags = layer.world_flags;
		self.world.data = layer.data;
	}

	fn wire_self_pointers(&mut self) {
		self.sync_output_world();
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
			name_do_not_use_directly: [0; 32],
			flags: 0,
			unused: 0,
			u: PF_ParamDefUnion {
				ld: self.input_layer.as_sys(),
			},
		});
		self.popup_choices.push(Vec::new());
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
		self.call_plugin(RawCommand::GlobalSetup, null_mut())?;
		self.global_out_flags = self.out_data.out_flags;
		// The capabilities declared here are what count. `out_data` is shared
		// by every later command, and plugins overwrite its flags in some of
		// them (the SDK samples set `out_flags2` in GPU device setup), so it
		// must not be consulted afterwards.
		self.global_out_flags2 = self.out_data.out_flags2;
		Ok(())
	}

	/// Call the plugin with `PF_Cmd_PARAMS_SETUP` command.
	fn setup_params(&mut self) -> Result<()> {
		self.call_plugin(RawCommand::ParamsSetup, null_mut())
	}

	/// Give every arbitrary-data parameter a value, as After Effects does when
	/// the effect is applied: the plugin allocates it through
	/// `PF_Cmd_ARBITRARY_CALLBACK` (`PF_Arbitrary_NEW_FUNC`, or a
	/// `PF_Arbitrary_COPY_FUNC` of the default). Plugins dereference the value
	/// handle they check out, so leaving it null crashes them.
	fn setup_arbitrary_params(&mut self) {
		for index in 0..self.params.len() {
			if self.params[index].param_type != ParamType::ArbitraryData as PF_ParamType {
				continue;
			}
			// SAFETY: the parameter was verified to be arbitrary data above.
			let arb = unsafe { self.params[index].u.arb_d };
			if !arb.value.is_null() {
				continue;
			}

			let mut value: PF_ArbitraryH = null_mut();
			let mut extra: PF_ArbParamsExtra = unsafe { std::mem::zeroed() };
			extra.which_function = PF_Arbitrary_NEW_FUNC as _;
			extra.id = arb.id;
			extra.u.new_func_params.refconPV = arb.refconPV;
			extra.u.new_func_params.arbPH = &mut value;
			let created = self.call_plugin(RawCommand::ArbitraryCallback, &mut extra as *mut _ as *mut c_void);

			if (created.is_err() || value.is_null()) && !arb.dephault.is_null() {
				let mut extra: PF_ArbParamsExtra = unsafe { std::mem::zeroed() };
				extra.which_function = PF_Arbitrary_COPY_FUNC as _;
				extra.id = arb.id;
				extra.u.copy_func_params.refconPV = arb.refconPV;
				extra.u.copy_func_params.src_arbH = arb.dephault;
				extra.u.copy_func_params.dst_arbPH = &mut value;
				if let Err(err) = self.call_plugin(RawCommand::ArbitraryCallback, &mut extra as *mut _ as *mut c_void) {
					log::warn!("arbitrary param #{index}: copying the default failed: {err:?}");
				}
			}

			if value.is_null() {
				// Legitimate for arbs that only carry a custom UI.
				log::debug!("arbitrary param #{index}: the plugin created no value");
				continue;
			}
			self.params[index].u.arb_d.value = value;
			self.owned_arb_values.push(index);
		}
	}

	/// Hand the arbitrary-data values created by [`Self::setup_arbitrary_params`]
	/// back to the plugin (`PF_Arbitrary_DISPOSE_FUNC`).
	fn dispose_arbitrary_params(&mut self) {
		for index in std::mem::take(&mut self.owned_arb_values) {
			// SAFETY: only arbitrary-data parameters are recorded as owned.
			let arb = unsafe { self.params[index].u.arb_d };
			let mut extra: PF_ArbParamsExtra = unsafe { std::mem::zeroed() };
			extra.which_function = PF_Arbitrary_DISPOSE_FUNC as _;
			extra.id = arb.id;
			extra.u.dispose_func_params.refconPV = arb.refconPV;
			extra.u.dispose_func_params.arbH = arb.value;
			if let Err(err) = self.call_plugin(RawCommand::ArbitraryCallback, &mut extra as *mut _ as *mut c_void) {
				log::warn!("arbitrary param #{index}: disposing the value failed: {err:?}");
			}
			self.params[index].u.arb_d.value = null_mut();
		}
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
		self.effect_state.reset_sequence();
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
		let declared = Self::query_declared_entry_point(lib, pica);
		let candidates = declared
			.as_deref()
			.into_iter()
			.chain(FALLBACK_ENTRY_POINT_CANDIDATES.iter().copied());

		let mut last_error = None;
		for candidate in candidates {
			match unsafe { lib.symbol::<PluginEntryPoint>(candidate) } {
				Ok(symbol) => {
					log::info!("Resolved entry point '{}'.", candidate.blue());
					return Ok((symbol, candidate.to_string()));
				}
				Err(err) => {
					log::debug!("Entry point symbol '{}' not resolved: {}", candidate, err);
					last_error = Some(err);
				}
			}
		}

		Err(last_error.expect("FALLBACK_ENTRY_POINT_CANDIDATES is non-empty").into())
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
	/// Before calling: composes a fresh `PF_InData` from the host info, effect
	/// state and render context, with `effect_ref` pointing at `self` (so suite
	/// callbacks can recover the instance via [`Self::get_instance_ptr`]), and
	/// rebuilds the cached param pointer list if `params` was mutated.
	///
	/// After calling: adopts a non-null `out_data.global_data`/`sequence_data`
	/// into the effect state, so plugin-allocated state persists across commands.
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
		let entry_point = self.entry_point.ok_or(AexloError::PluginNotLoaded)?;
		let effect_ref = self as *mut _ as PF_ProgPtr;

		let entry_point_name = self.entry_point_name.as_deref().unwrap_or(DEFAULT_ENTRY_POINT_NAME);

		// debug, not info: this runs for every render command, so at info level a
		// preview loop drowns the log.
		log::debug!("Executing command: {}", format!("{:?}", command).blue());

		if self.params_dirty {
			self.params_ptr_cache = self.params.iter_mut().map(|p| p as *mut _).collect();
			self.params_dirty = false;
		}

		let mut in_data = crate::core::in_data::compose(
			&self.host_info,
			&self.effect_state,
			&self.render_context,
			CallBindings {
				effect_ref,
				utils: self.utility_callbacks.as_mut() as *mut _,
				pica: self.pica.as_mut() as *mut _,
				num_params: self.params.len() as i32,
			},
		);

		let observer = self.observer.clone();
		let started = observer.as_ref().map(|installed| {
			installed.observer.command(&CommandEvent {
				command: command as i32,
				name: crate::observe::command_name(command as i32),
				phase: CommandPhase::Begin,
				duration: None,
				error: None,
			});
			Instant::now()
		});
		let calls = observer
			.as_ref()
			.filter(|installed| installed.level >= ObserveLevel::Calls)
			.map(|installed| installed.observer.clone());
		let calls_guard = crate::observe::enter_calls(calls);
		let name = crate::observe::command_name(command as i32);
		let tracking = self.tracking();
		let resumed = tracking
			.as_ref()
			.and_then(|_| self.tracker.lock().ok().map(|mut t| t.begin(name)));

		let result = crate::suites::iterate::with_parallel_iterate(self.parallel_iterate, || unsafe {
			entry_point(
				command,
				&mut in_data,
				&mut self.out_data,
				self.params_ptr_cache.as_mut_ptr(),
				&mut self.world,
				extra_data,
			)
		});

		if let Some(resumed) = resumed
			&& let Ok(mut tracker) = self.tracker.lock()
		{
			tracker.end(name, resumed);
		}
		drop(tracking);
		drop(calls_guard);
		if let (Some(installed), Some(started)) = (&observer, started) {
			let code = result as i64;
			installed.observer.command(&CommandEvent {
				command: command as i32,
				name: crate::observe::command_name(command as i32),
				phase: CommandPhase::End,
				duration: Some(started.elapsed()),
				error: (code != PF_Err_NONE as i64).then_some(code as i32),
			});
		}

		#[cfg(target_os = "macos")]
		let result = result as u32;

		self.effect_state.absorb(&self.out_data);

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
	/// Tears the plugin down in the same order as After Effects so that the
	/// state it allocated is released.
	///
	/// Failures are only logged, because panicking in `drop` would abort the
	/// process and the plugin is about to be unloaded anyway.
	fn drop(&mut self) {
		self.teardown();
	}
}

impl PluginInstance {
	/// Set the plugin down once, in After Effects' order.
	fn teardown(&mut self) {
		// The entry point is missing only when `try_load` failed to resolve it,
		// in which case the plugin was never set up.
		if self.entry_point.is_none() || self.torn_down {
			return;
		}
		self.torn_down = true;

		// Release pre-render data left by an interrupted render while the
		// plugin's code (which owns the delete callback) is still loaded.
		self.end_output_guard();
		self.dispose_pre_render();

		if let Err(err) = self.gpu_device_setdown() {
			log::warn!("PF_Cmd_GPU_DEVICE_SETDOWN failed during drop: {err:?}");
		}

		if let Err(err) = self.call_plugin(RawCommand::SequenceSetdown, null_mut()) {
			log::warn!("PF_Cmd_SEQUENCE_SETDOWN failed during drop: {err:?}");
		}

		self.dispose_arbitrary_params();

		if let Err(err) = self.call_plugin(RawCommand::GlobalSetdown, null_mut()) {
			log::warn!("PF_Cmd_GLOBAL_SETDOWN failed during drop: {err:?}");
		}
		if let Ok(mut tracker) = self.tracker.lock() {
			tracker.torn_down();
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
	fn param_kind_covers_value_less_params() {
		let mut fx = bare_instance();
		fx.add_instance_param(param_of_type(ParamType::GroupStart));
		let mut button = param_of_type(ParamType::Button);
		button.ui_flags = after_effects_sys::PF_PUI_INVISIBLE as after_effects_sys::PF_ParamUIFlags;
		fx.add_instance_param(button);

		assert_eq!(fx.param_kind(0), Some(crate::ParamKind::Layer));
		assert_eq!(fx.param_kind(1), Some(crate::ParamKind::GroupStart));
		assert_eq!(fx.param_kind(2), Some(crate::ParamKind::Button));
		assert_eq!(fx.param_kind(3), None);
		assert!(!fx.param_hidden(1));
		assert!(fx.param_hidden(2));
	}

	#[test]
	fn point3d_round_trips() {
		let mut fx = bare_instance();
		fx.add_instance_param(param_of_type(ParamType::Point3D));
		let value = ParamValue::Point3D {
			x: 1.5,
			y: -2.0,
			z: 300.25,
		};
		fx.set_param(1, value.clone()).unwrap();
		assert_eq!(fx.get_param(1), Some(value));
		assert!(fx.set_param(1, ParamValue::Point { x: 0.0, y: 0.0 }).is_err());
	}

	#[test]
	fn set_time_reaches_in_data() {
		let mut fx = bare_instance();
		assert_eq!(fx.time(), (0, 1, 30));
		fx.set_time(48, 1, 24);
		assert_eq!(fx.time(), (48, 1, 24));
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
		assert_eq!((fx.world.width, fx.world.height), (640, 360));
		// The world must point at the freshly sized output layer's pixels.
		assert_eq!(fx.world.data as *const u8, fx.output_layer.layer().data_ptr());
		assert_eq!(fx.world.rowbytes, 640 * 4);
	}

	#[test]
	fn in_data_size_follows_input_not_output() {
		let mut fx = bare_instance();
		let (in_w, in_h) = fx.input_size();
		fx.set_render_size(640, 360);
		assert_eq!(fx.render_context.size(), (in_w as i32, in_h as i32));

		fx.set_input_layer(PluginInstance::build_layer(
			320,
			240,
			wrapper::Pixel::<wrapper::Depth8>::black(),
		));
		assert_eq!(fx.render_context.size(), (320, 240));
		assert_eq!(fx.output_size(), (640, 360));
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
