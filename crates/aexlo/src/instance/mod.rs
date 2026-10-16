//! [`PluginInstance`]: one loaded effect and the host state it runs against.
//! Loading, rendering, layers and parameters live in the submodules.

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
use crate::pixel::{AnyLayer, Depth8, Layer, Pixel, PixelDepth, PixelDepthKind};
use crate::time::Time;
use after_effects::{ParamType, RawCommand};
use after_effects_sys::{
	PF_ArbParamsExtra, PF_Arbitrary_COPY_FUNC, PF_Arbitrary_DISPOSE_FUNC, PF_Arbitrary_NEW_FUNC, PF_ArbitraryH,
	PF_Err_INVALID_CALLBACK, PF_Err_NONE, PF_GPU_Framework, PF_GPU_Framework_NONE, PF_GPUDeviceSetdownExtra,
	PF_GPUDeviceSetdownInput, PF_GPUDeviceSetupExtra, PF_GPUDeviceSetupInput, PF_GPUDeviceSetupOutput,
	PF_OutFlag2_SUPPORTS_GPU_RENDER_F32, PF_OutFlags, PF_OutFlags2, PF_ParamDef, PF_ParamDefUnion, PF_ParamType,
	PF_ProgPtr,
};
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

use crate::core::constants::{DEFAULT_HEIGHT as HEIGHT, DEFAULT_WIDTH as WIDTH};

mod layers;
mod load;
mod params;
mod render;
#[cfg(test)]
mod tests;

pub use render::RenderPath;

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

impl PluginInstance {
	/// Call the plugin with `PF_Cmd_ABOUT` command.
	pub fn about(&mut self) -> Result<String> {
		self.call_plugin(RawCommand::About, null_mut())?;

		Ok(self.message())
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

	/// Tell `observer` about every command sent to the plugin from now on and,
	/// at [`ObserveLevel::Calls`], every suite function it calls back on the
	/// dispatching thread. `None` removes it.
	pub fn set_observer(&mut self, observer: Option<Arc<dyn Observer>>, level: ObserveLevel) {
		if let Ok(mut tracker) = self.tracker.lock() {
			tracker.observer = observer.clone();
		}
		self.observer = observer.map(|observer| Installed { observer, level });
	}

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
}
