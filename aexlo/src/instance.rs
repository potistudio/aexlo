use crate::core::error::{AexloError, Result};
use crate::host::smart_render::SmartRenderData;
use crate::utils;

use after_effects::ParamType;
use after_effects_sys::{PF_Err_NONE, PF_ParamDef, PF_ParamDefUnion, PF_ParamType, PF_Pixel, PF_ProgPtr};
use colored::Colorize;
use dlopen2::raw::Library;
use std::{
	path::{Path, PathBuf},
	ptr::NonNull,
	ptr::null_mut,
};

const DEFAULT_ENTRY_POINT_NAME: &str = "EffectMain";
const WIDTH: u32 = 1920;
const HEIGHT: u32 = 1080;

type PluginEntryPoint = unsafe extern "C" fn(
	cmd: after_effects::RawCommand,
	in_data: *mut after_effects_sys::PF_InData,
	out_data: *mut after_effects_sys::PF_OutData,
	params: after_effects_sys::PF_ParamList,
	output: *mut after_effects_sys::PF_LayerDef,
	extra: *mut ::std::os::raw::c_void,
) -> after_effects_sys::PF_Err;

/// Represents a loaded After Effects plugin instance, managing its library, entry point, parameters, and execution state.
pub struct PluginInstance {
	library: Option<Library>,
	entry_point: Option<PluginEntryPoint>,
	entry_point_name: Option<String>,
	entry_point_candidates: Vec<String>,
	path: PathBuf,
	cmd: after_effects::RawCommand,
	world: after_effects_sys::PF_LayerDef,

	utility_callbacks: Box<after_effects_sys::_PF_UtilCallbacks>,

	/// Basic Suite pointer
	pub pica: Box<after_effects_sys::SPBasicSuite>,

	/// InData structure
	pub in_data: after_effects_sys::PF_InData,
	out_data: after_effects_sys::PF_OutData,

	/// Instance-specific parameters from the host (non-global storage)
	params: Vec<after_effects_sys::PF_ParamDef>,

	/// Track if instance params need synchronization
	params_dirty: bool,

	pub(crate) input_layer: wrapper::Layer<wrapper::Depth8>,
	pub(crate) output_layer: wrapper::Layer<wrapper::Depth8>,

	smart_render_data: SmartRenderData,
}

impl PluginInstance {
	pub fn try_load(path: &Path) -> Result<Self> {
		let mut instance = Self::new(path);
		instance.load()?;
		instance.setup_global()?;
		instance.setup_params()?;
		Ok(instance)
	}

	/// Create a new PluginInstance with default values
	fn new(path: &Path) -> Self {
		let width = WIDTH;
		let height = HEIGHT;

		// Initialize Interact Callbacks using factory
		let interact_callbacks = crate::host::interact::create_interact_callbacks();

		// Initialize Utility Callbacks using factory
		let utility_callbacks = crate::host::utility::create_utility_callbacks();

		let input_layer = wrapper::Layer::<wrapper::Depth8>::new(
			width,
			height,
			vec![wrapper::Pixel::<wrapper::Depth8>::green(); (width * height) as usize],
		)
		.unwrap();

		let pica = Box::new(after_effects_sys::SPBasicSuite {
			AcquireSuite: Some(crate::suites::rusty_acquire_suite),
			ReleaseSuite: Some(crate::suites::rusty_release_suite),
			IsEqual: None,
			AllocateBlock: None,
			FreeBlock: None,
			ReallocateBlock: None,
			Undefined: None,
		});

		// Initialize InData
		let mut instance = PluginInstance {
			library: None,
			entry_point: None,
			entry_point_name: None,
			entry_point_candidates: vec![
				"EffectMain".to_string(),
				"EntryPointFunc".to_string(),
				"entryPointFunc".to_string(),
			],
			path: path.to_path_buf(),
			cmd: after_effects::RawCommand::About,
			utility_callbacks,
			pica,
			in_data: crate::core::helpers::InDataBuilder::new()
				.with_size(1280, 720)
				.with_callbacks(interact_callbacks)
				.with_global_data(unsafe { crate::suites::handle::host_new_handle_impl(0x498) })
				.build(),
			out_data: crate::core::helpers::OutDataBuilder::new().build(),
			params: Vec::new(),
			params_dirty: false,
			input_layer,
			output_layer: wrapper::Layer::<wrapper::Depth8>::new(
				width,
				height,
				vec![wrapper::Pixel::<wrapper::Depth8>::black(); (width * height) as usize],
			)
			.unwrap(),
			world: crate::core::helpers::LayerDefBuilder::new()
				.with_size(width as i32, height as i32)
				.build(),

			smart_render_data: SmartRenderData::new(),
		};

		// Now set the utils pointer to reference our owned utility_callbacks
		instance.in_data.utils = instance.utility_callbacks.as_mut() as *mut _;
		instance.in_data.pica_basicP = instance.pica.as_mut() as *mut _;

		// effect_ref will be set dynamically before each plugin call
		instance.in_data.effect_ref = std::ptr::null_mut();

		instance.in_data.num_params = instance.params.len() as i32;
		instance.world.data = instance.output_layer.pixels_mut().as_mut_ptr() as *mut PF_Pixel;

		instance.params.push(PF_ParamDef {
			uu: after_effects_sys::PF_ParamDef__bindgen_ty_1 { id: 0 },
			ui_flags: 0,
			ui_width: 0,
			ui_height: 0,
			param_type: 0 as PF_ParamType,
			name: [0; 32],
			flags: 0,
			unused: 0,
			u: PF_ParamDefUnion {
				ld: instance.input_layer.as_sys(),
			},
		});

		instance
	}

	pub fn with_entry_point_candidates<I, S>(mut self, names: I) -> Self
	where
		I: IntoIterator<Item = S>,
		S: AsRef<str>,
	{
		self.set_entry_point_candidates(names);
		self
	}

	pub fn set_entry_point_candidates<I, S>(&mut self, names: I)
	where
		I: IntoIterator<Item = S>,
		S: AsRef<str>,
	{
		let mut candidates = Vec::new();
		for name in names {
			let trimmed = name.as_ref().trim();
			if !trimmed.is_empty() && !candidates.iter().any(|s: &String| s == trimmed) {
				candidates.push(trimmed.to_string());
			}
		}

		if candidates.is_empty() {
			candidates.push(DEFAULT_ENTRY_POINT_NAME.to_string());
		}

		self.entry_point_candidates = candidates;
		self.entry_point = None;
		self.entry_point_name = None;
	}

	fn resolve_entry_point(lib: &Library, candidates: &[String]) -> Result<(PluginEntryPoint, String)> {
		let mut last_error = None;

		for candidate in candidates {
			match unsafe { lib.symbol::<PluginEntryPoint>(candidate) } {
				Ok(symbol) => return Ok((symbol, candidate.clone())),
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

	fn load(&mut self) -> Result<()> {
		let dir = self
			.path
			.parent()
			.and_then(|s| s.to_str())
			.ok_or_else(|| AexloError::InvalidPath {
				message: "Invalid module directory".to_string(),
			})?;
		let name = self
			.path
			.file_name()
			.and_then(|s| s.to_str())
			.ok_or_else(|| AexloError::InvalidPath {
				message: "Invalid module name".to_string(),
			})?;

		//* ---- Detect OS ------------------------------ */
		log::info!("Detecting OS...");
		let os = std::env::consts::OS;
		let module_path = match os {
			"windows" => format!("{}/{}", dir, name),
			"macos" => format!("{}/{}.plugin/Contents/MacOS/{}", dir, name, name),
			_ => {
				return Err(AexloError::UnsupportedOS { os: os.to_string() });
			}
		};

		log::info!("Detected OS: {}.", os.blue());
		//* --------------------------------------------- */
		//* ---- Load Plugin --------------------------- *//
		log::info!("Loading plugin: {} from {}.", name.blue(), module_path.blue());

		// Check if the plugin file exists
		if !std::path::Path::new(&module_path).exists() {
			return Err(AexloError::PluginNotFound { path: module_path });
		}

		let lib = Library::open(&module_path)?;
		let (entry_point, resolved_name) = Self::resolve_entry_point(&lib, &self.entry_point_candidates)?;

		self.entry_point = Some(entry_point);
		self.entry_point_name = Some(resolved_name.clone());
		self.library = Some(lib);

		// Set plugin path for get_platform_data callback
		crate::host::utility::set_plugin_path(std::path::Path::new(&module_path));

		log::info!("Resolved entry point symbol: {}.", resolved_name.blue());

		log::info!("Loaded plugin '{}' {}.", name.blue(), "successfully".green());
		//* -------------------------------------------- *//

		Ok(())
	}

	/// Call the plugin entry point
	fn call_plugin(&mut self, extra_data: *mut ::std::os::raw::c_void) -> Result<()> {
		self.sync_input_layer_param();

		let entry_point_name = self
			.entry_point_name
			.as_deref()
			.unwrap_or(DEFAULT_ENTRY_POINT_NAME)
			.to_string();

		log::info!("Executing command: {}", format!("{:?}", self.cmd).blue());

		// Update effect_ref to point to self before calling the plugin
		self.in_data.effect_ref = self as *mut _ as PF_ProgPtr;

		let mut params_ptr: Vec<*mut PF_ParamDef> = self.params.iter_mut().map(|p| p as *mut _).collect();

		let entry_point = self.entry_point.ok_or(AexloError::ContainerNotLoaded)?;

		let result = unsafe {
			entry_point(
				self.cmd,
				&mut self.in_data,
				&mut self.out_data,
				params_ptr.as_mut_ptr(),
				&mut self.world,
				extra_data,
			)
		};

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
				return Err(AexloError::PluginExecutionFailed { code });
			}
		}
		//* -------------------------------------------- *//

		Ok(())
	}

	/// Keep params[0] (`PF_Param_LAYER`) synchronized with the current input layer.
	fn sync_input_layer_param(&mut self) {
		if let Some(input_param) = self.params.get_mut(0) {
			input_param.u = PF_ParamDefUnion {
				ld: self.input_layer.as_sys(),
			};
		}
	}

	/// Call the plugin with `PF_Cmd_ABOUT` command
	pub fn about(&mut self) -> Result<String> {
		self.cmd = after_effects::RawCommand::About;
		self.call_plugin(null_mut())?;

		Ok(self.message())
	}

	/// Call the plugin with `PF_Cmd_GLOBAL_SETUP` command
	pub fn setup_global(&mut self) -> Result<()> {
		self.cmd = after_effects::RawCommand::GlobalSetup;
		self.call_plugin(null_mut())?;

		Ok(())
	}

	/// Call the plugin with `PF_Cmd_PARAMS_SETUP` command
	fn setup_params(&mut self) -> Result<()> {
		self.cmd = after_effects::RawCommand::ParamsSetup;
		self.call_plugin(null_mut())?;

		Ok(())
	}

	/// Call the plugin with `PF_Cmd_RENDER` command
	pub fn render(&mut self) -> Result<()> {
		self.cmd = after_effects::RawCommand::Render;
		self.call_plugin(null_mut())?;

		Ok(())
	}

	pub fn render_pre(&mut self) -> Result<()> {
		let mut extra = self.smart_render_data.pre_render_extra();

		self.cmd = after_effects::RawCommand::SmartPreRender;
		self.call_plugin((&mut extra as *mut after_effects_sys::PF_PreRenderExtra).cast())?;

		self.smart_render_data.sync();

		Ok(())
	}

	pub fn render_smart(&mut self) -> Result<()> {
		let mut extra = self.smart_render_data.smart_render_extra();

		self.cmd = after_effects::RawCommand::SmartRender;
		self.call_plugin((&mut extra as *mut after_effects_sys::PF_SmartRenderExtra).cast())?;

		self.smart_render_data.sync();

		Ok(())
	}

	pub fn set_input(&mut self, input: wrapper::Layer<wrapper::Depth8>) {
		self.input_layer = input;
		self.sync_input_layer_param();
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

	/// Get the number of parameters
	pub fn param_count(&self) -> usize {
		self.params.len()
	}

	//==== Setter ==========================================
	/// Set a float parameter value by index.
	pub fn set_param_float(&mut self, index: usize, value: f64) -> Result<()> {
		// Check index bounds for instance_params (offset by 1 for input layer param)
		if index == 0 || index >= self.params.len() {
			return Err(AexloError::ParamIndexOutOfBounds {
				index,
				max: self.params.len(),
			});
		}

		// Check if this is a float slider type
		let target_param = &mut self.params[index];
		if target_param.param_type != ParamType::FloatSlider as PF_ParamType {
			return Err(AexloError::ParamTypeMismatch {
				index,
				expected: "FloatSlider",
				actual: target_param.param_type,
			});
		}

		// SAFETY: We verified param type is float slider, so fs_d is the active union variant
		target_param.u.fs_d.value = value;

		Ok(())
	}

	pub fn set_param_fixed(&mut self, index: usize, value: f32) -> Result<()> {
		// Check index bounds for instance_params (offset by 1 for input layer param)
		if index == 0 || index >= self.params.len() {
			return Err(AexloError::ParamIndexOutOfBounds {
				index,
				max: self.params.len(),
			});
		}

		// Check if this is a fixed type
		let target_param = &mut self.params[index];
		if target_param.param_type != ParamType::FixSlider as PF_ParamType {
			return Err(AexloError::ParamTypeMismatch {
				index,
				expected: "FixSlider",
				actual: target_param.param_type,
			});
		}

		target_param.u.fd.value = utils::f32_to_q31(value);
		Ok(())
	}

	pub fn set_param_slider(&mut self, index: usize, value: i32) -> Result<()> {
		// Check index bounds for instance_params (offset by 1 for input layer param)
		if index == 0 || index >= self.params.len() {
			return Err(AexloError::ParamIndexOutOfBounds {
				index,
				max: self.params.len(),
			});
		}

		// Check if this is a slider type
		let target_param = &mut self.params[index];
		if target_param.param_type != ParamType::Slider as PF_ParamType {
			return Err(AexloError::ParamTypeMismatch {
				index,
				expected: "Slider",
				actual: target_param.param_type,
			});
		}

		target_param.u.sd.value = value;
		Ok(())
	}

	pub fn set_param_checkbox(&mut self, index: usize, value: bool) -> Result<()> {
		// Check index bounds for instance_params (offset by 1 for input layer param)
		if index == 0 || index >= self.params.len() {
			return Err(AexloError::ParamIndexOutOfBounds {
				index,
				max: self.params.len(),
			});
		}

		// Check if this is a checkbox type
		let target_param = &mut self.params[index];
		if target_param.param_type != ParamType::CheckBox as PF_ParamType {
			return Err(AexloError::ParamTypeMismatch {
				index,
				expected: "Checkbox",
				actual: target_param.param_type,
			});
		}

		target_param.u.bd.value = if value { 1 } else { 0 };
		Ok(())
	}

	/// Get a float parameter value by index.
	/// Returns `None` if index out of bounds or not a float param.
	pub fn get_param_float(&self, index: usize) -> Option<f64> {
		// Check index bounds for instance_params (offset by 1 for input layer param)
		if index == 0 || index >= self.params.len() {
			return None;
		}

		let target_param = &self.params[index];

		if target_param.param_type != ParamType::FloatSlider as PF_ParamType {
			return None;
		}

		// SAFETY: We verified param type is float slider, so fs_d is the active union variant
		unsafe { Some(target_param.u.fs_d.value) }
	}

	/// Get the output message from the plugin (set during About command).
	///
	/// # Note
	/// The message may contain line breaks and special characters (e.g. \r, \n).
	///
	/// # Returns
	///
	/// A `String` containing the message. Invalid UTF-8 sequences are replaced with the
	/// Unicode replacement character (�).
	///
	/// # Example
	/// ```ignore
	/// let mut instance = PluginInstance::new("SDK_Noise");
	/// instance.load()?;
	///
	/// instance.about()?;
	/// println!("Plugin Message: {}", instance.message());
	/// ```
	fn message(&self) -> String {
		let bytes = &self.out_data.return_msg;

		// Cramp the buffer at the first null byte (if any) to avoid trailing garbage
		let cramped_length = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());

		let utf8: Vec<u8> = bytes[..cramped_length].iter().map(|&b| b as u8).collect();
		String::from_utf8_lossy(&utf8).into_owned()
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

	/// Add a parameter to this instance's parameter storage
	pub fn add_instance_param(&mut self, param: PF_ParamDef) {
		self.params.push(param);
		self.params_dirty = true;
		log::debug!(
			"PluginInstance: added param #{} (type: {:?})",
			self.params.len(),
			param.param_type
		);
	}

	/// Get all instance parameters
	pub fn params(&self) -> &[PF_ParamDef] {
		&self.params
	}

	/// Get a specific instance parameter by index
	pub fn param_by_index(&self, index: usize) -> Option<&PF_ParamDef> {
		self.params.get(index)
	}

	/// Clear all instance parameters
	pub fn clear_instance_params(&mut self) {
		self.params.clear();
		self.params_dirty = true;
		log::debug!("PluginInstance: cleared all instance params");
	}
}
