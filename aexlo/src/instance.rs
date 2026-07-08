use crate::core::error::{AexloError, Result};
use crate::host::smart_render::SmartRenderData;
use crate::utils;

/// A parameter value for an After Effects plugin.
#[derive(Debug, Clone, PartialEq)]
pub enum ParamValue {
	Float(f64),
	Fixed(f32),
	Slider(i32),
	Checkbox(bool),
}

use after_effects::ParamType;
use after_effects_sys::{
	PF_Err_INVALID_CALLBACK, PF_Err_NONE, PF_ParamDef, PF_ParamDefUnion, PF_ParamType, PF_Pixel, PF_ProgPtr,
};
use colored::Colorize;
use dlopen2::raw::Library;
use std::{
	ffi::{CStr, CString},
	path::{Path, PathBuf},
	ptr::NonNull,
	ptr::null_mut,
};

const DEFAULT_ENTRY_POINT_NAME: &str = "EffectMain";
const WIDTH: u32 = 1920;
const HEIGHT: u32 = 1080;

/// Entry point names to try if the plugin doesn't implement `PluginDataEntryFunction2`.
const FALLBACK_ENTRY_POINT_CANDIDATES: &[&str] = &[DEFAULT_ENTRY_POINT_NAME, "EntryPointFunc"];
/// Fixed symbol name of the AE SDK's self-describing plugin data entry function.
const PLUGIN_DATA_ENTRY_SYMBOL: &str = "PluginDataEntryFunction2";
const HOST_NAME: &str = "AfterEffects";
const HOST_VERSION: &str = "25.2";

type PluginEntryPoint = unsafe extern "C" fn(
	cmd: after_effects::RawCommand,
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
	library: Option<Library>,
	entry_point: Option<PluginEntryPoint>,
	entry_point_name: Option<String>,
	path: PathBuf,
	cmd: after_effects::RawCommand,
	world: after_effects_sys::PF_LayerDef,

	utility_callbacks: Box<after_effects_sys::_PF_UtilCallbacks>,

	/// Basic Suite pointer.
	pub pica: Box<after_effects_sys::SPBasicSuite>,

	/// InData structure.
	pub in_data: after_effects_sys::PF_InData,
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
	/// `PF_Cmd_GLOBAL_SETUP` or `PF_Cmd_PARAMS_SETUP` commands.
	pub fn try_load(path: impl AsRef<Path>) -> Result<Self> {
		let mut instance = Self::new(path.as_ref());

		instance.load()?;
		instance.setup_global()?;
		instance.setup_params()?;

		Ok(instance)
	}

	/// Call the plugin with `PF_Cmd_ABOUT` command.
	pub fn about(&mut self) -> Result<String> {
		self.cmd = after_effects::RawCommand::About;
		self.call_plugin(null_mut())?;

		Ok(self.message())
	}

	/// Call the plugin with `PF_Cmd_RENDER` command.
	pub fn render(&mut self) -> Result<()> {
		self.cmd = after_effects::RawCommand::Render;
		self.call_plugin(null_mut())?;

		Ok(())
	}

	/// Call the plugin with `PF_Cmd_SMART_PRE_RENDER` command, letting it declare the
	/// input/output checkout regions it needs via [`Self::render_smart`].
	pub fn render_pre(&mut self) -> Result<()> {
		let mut extra = self.smart_render_data.pre_render_extra();

		self.cmd = after_effects::RawCommand::SmartPreRender;
		self.call_plugin((&mut extra as *mut after_effects_sys::PF_PreRenderExtra).cast())?;

		self.smart_render_data.sync();

		Ok(())
	}

	/// Call the plugin with `PF_Cmd_SMART_RENDER` command, using the checkout regions
	/// declared during the preceding [`Self::render_pre`] call.
	pub fn render_smart(&mut self) -> Result<()> {
		let mut extra = self.smart_render_data.smart_render_extra();

		self.cmd = after_effects::RawCommand::SmartRender;
		self.call_plugin((&mut extra as *mut after_effects_sys::PF_SmartRenderExtra).cast())?;

		self.smart_render_data.sync();

		Ok(())
	}

	/// Replace the input layer, keeping the `PF_Param_LAYER` parameter (index 0) in sync.
	pub fn set_input(&mut self, input: wrapper::Layer<wrapper::Depth8>) {
		self.input_layer = input;

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
		self.output_layer
			.write_rgba_bytes(buffer)
			.map_err(|e| AexloError::Unexpected("Failed to write RGBA bytes: ".to_string() + &e))
	}

	//==== Getter ==========================================
	/// Get output dimensions in pixel (width, height).
	pub fn output_size(&self) -> (u32, u32) {
		(self.output_layer.width(), self.output_layer.height())
	}

	/// Get a pointer to the instance's persistent output world (`PF_LayerDef`/`PF_EffectWorld`).
	///
	/// Used by smart-render callbacks to hand back a stable pointer instead of one
	/// pointing at a temporary value that would dangle after the callback returns.
	pub fn output_world_ptr(&mut self) -> *mut after_effects_sys::PF_EffectWorld {
		&mut self.world as *mut after_effects_sys::PF_LayerDef as *mut after_effects_sys::PF_EffectWorld
	}

	/// Get the number of parameters.
	pub fn param_count(&self) -> usize {
		self.params.len()
	}

	//==== Setter / Getter =================================

	/// Set a parameter value by index.
	/// `index` must be 1 or greater (index 0 is the input layer, not settable).
	pub fn set_param(&mut self, index: usize, value: ParamValue) -> Result<()> {
		if index == 0 || index >= self.params.len() {
			return Err(AexloError::ParamIndexOutOfBounds {
				index,
				max: self.params.len(),
			});
		}

		let target = &mut self.params[index];

		let expected = match &value {
			ParamValue::Float(_) if target.param_type == ParamType::FloatSlider as PF_ParamType => None,
			ParamValue::Fixed(_) if target.param_type == ParamType::FixSlider as PF_ParamType => None,
			ParamValue::Slider(_) if target.param_type == ParamType::Slider as PF_ParamType => None,
			ParamValue::Checkbox(_) if target.param_type == ParamType::CheckBox as PF_ParamType => None,
			ParamValue::Float(_) => Some("FloatSlider"),
			ParamValue::Fixed(_) => Some("FixSlider"),
			ParamValue::Slider(_) => Some("Slider"),
			ParamValue::Checkbox(_) => Some("Checkbox"),
		};

		if let Some(expected) = expected {
			return Err(AexloError::ParamTypeMismatch {
				index,
				expected,
				actual: target.param_type,
			});
		}

		// SAFETY: union variant was verified against param_type above
		match value {
			ParamValue::Float(v) => target.u.fs_d.value = v,
			ParamValue::Fixed(v) => target.u.fd.value = utils::f32_to_q31(v),
			ParamValue::Slider(v) => target.u.sd.value = v,
			ParamValue::Checkbox(v) => target.u.bd.value = v as i32,
		}

		Ok(())
	}

	/// Returns all parameter values as `(index, value)` pairs.
	/// Index 0 (input layer) and parameters with unknown types are excluded.
	pub fn param_values(&self) -> Vec<(usize, ParamValue)> {
		(1..self.params.len())
			.filter_map(|i| self.get_param(i).map(|v| (i, v)))
			.collect()
	}

	/// Get a parameter value by index.
	/// Returns `None` if the index is out of bounds or the param type is unknown.
	pub fn get_param(&self, index: usize) -> Option<ParamValue> {
		if index == 0 || index >= self.params.len() {
			return None;
		}

		let param = &self.params[index];

		// SAFETY: union variant is selected based on param_type
		unsafe {
			if param.param_type == ParamType::FloatSlider as PF_ParamType {
				Some(ParamValue::Float(param.u.fs_d.value))
			} else if param.param_type == ParamType::FixSlider as PF_ParamType {
				Some(ParamValue::Fixed(utils::q31_to_f32(param.u.fd.value)))
			} else if param.param_type == ParamType::Slider as PF_ParamType {
				Some(ParamValue::Slider(param.u.sd.value))
			} else if param.param_type == ParamType::CheckBox as PF_ParamType {
				Some(ParamValue::Checkbox(param.u.bd.value != 0))
			} else {
				None
			}
		}
	}

	/// Get a PluginInstance pointer from an effect reference pointer.
	///
	/// The returned pointer does not imply unique mutable access.
	/// Callers must uphold aliasing rules before dereferencing.
	pub fn get_instance_ptr(effect_ref: PF_ProgPtr) -> Option<NonNull<PluginInstance>> {
		if effect_ref.is_null() {
			return None;
		}

		NonNull::new(effect_ref as *mut PluginInstance)
	}

	//==== Instance Parameter Management ==========================================

	/// Add a parameter to this instance's parameter storage.
	pub fn add_instance_param(&mut self, param: PF_ParamDef) {
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
	pub fn params(&self) -> &[PF_ParamDef] {
		&self.params
	}

	/// Get a specific instance parameter by index.
	pub fn param_by_index(&self, index: usize) -> Option<&PF_ParamDef> {
		self.params.get(index)
	}

	/// Clear all instance parameters.
	pub fn clear_instance_params(&mut self) {
		self.params.clear();
		self.params_dirty = true;
		log::debug!("PluginInstance: cleared all instance params");
	}
}

//* ---- Internal Methods ------------------------------- */
impl PluginInstance {
	/// Create a new PluginInstance with default values.
	fn new(path: &Path) -> Self {
		let interact_callbacks = crate::host::interact::create_interact_callbacks();
		let utility_callbacks = crate::host::utility::create_utility_callbacks();
		let pica = Self::build_pica_suite();

		let input_layer = Self::build_layer(wrapper::Pixel::<wrapper::Depth8>::green());
		let output_layer = Self::build_layer(wrapper::Pixel::<wrapper::Depth8>::black());

		let mut instance = PluginInstance {
			library: None,
			entry_point: None,
			entry_point_name: None,
			path: path.to_path_buf(),
			cmd: after_effects::RawCommand::About,
			utility_callbacks,
			pica,
			in_data: crate::core::helpers::InDataBuilder::new()
				.with_size(1280, 720)
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

			smart_render_data: SmartRenderData::new(),
		};

		instance.wire_self_pointers();
		instance.push_input_layer_param();

		instance
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

	/// Build a `WIDTH` x `HEIGHT` layer filled with `fill`.
	fn build_layer(fill: wrapper::Pixel<wrapper::Depth8>) -> wrapper::Layer<wrapper::Depth8> {
		wrapper::Layer::<wrapper::Depth8>::new(WIDTH, HEIGHT, vec![fill; (WIDTH * HEIGHT) as usize]).unwrap()
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
		self.cmd = after_effects::RawCommand::GlobalSetup;
		self.call_plugin(null_mut())?;

		Ok(())
	}

	/// Call the plugin with `PF_Cmd_PARAMS_SETUP` command.
	fn setup_params(&mut self) -> Result<()> {
		self.cmd = after_effects::RawCommand::ParamsSetup;
		self.call_plugin(null_mut())?;

		Ok(())
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
			match unsafe { lib.symbol::<PluginEntryPoint>(*candidate) } {
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
		let module_path = Self::resolve_binary_path(&self.path)?;
		let module_path_str = module_path.display().to_string();

		log::info!("Loading plugin from '{}'.", module_path_str.blue());

		let lib = Library::open(&module_path)?;
		let (entry_point, resolved_name) = Self::resolve_entry_point(&lib, self.pica.as_ref())?;

		self.entry_point = Some(entry_point);
		self.entry_point_name = Some(resolved_name.clone());
		self.library = Some(lib);

		// Set plugin path for get_platform_data callback
		crate::host::utility::set_plugin_path(&module_path);

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
	fn call_plugin(&mut self, extra_data: *mut ::std::os::raw::c_void) -> Result<()> {
		let entry_point = self.entry_point.ok_or(AexloError::ContainerNotLoaded)?;

		// Update effect_ref to point to self before calling the plugin
		self.in_data.effect_ref = self as *mut _ as PF_ProgPtr;

		let entry_point_name = self.entry_point_name.as_deref().unwrap_or(DEFAULT_ENTRY_POINT_NAME);

		log::info!("Executing command: {}", format!("{:?}", self.cmd).blue());

		if self.params_dirty {
			self.params_ptr_cache = self.params.iter_mut().map(|p| p as *mut _).collect();
			self.params_dirty = false;
		}

		let result = unsafe {
			entry_point(
				self.cmd,
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

		log::info!(
			"Executed command '{}' {}.",
			format!("{:?}", self.cmd).blue(),
			"successfully".green()
		);
		log::debug!(
			"{} exited with code: {}.",
			entry_point_name.blue(),
			result.to_string().blue()
		);

		//* ---- Check for errors ---------------------- *//
		#[allow(non_upper_case_globals)]
		match result {
			PF_Err_NONE => {
				log::info!("Plugin executed {}.", "successfully".green());
			}
			code => {
				return Err(AexloError::PluginExecutionFailed { code: code.into() });
			}
		}
		//* -------------------------------------------- *//

		Ok(())
	}
}
