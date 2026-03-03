use crate::core::error::{AexloError, Result};
// use crate::suites::SuiteContainer; // Not needed

use after_effects::ParamType;
use after_effects_sys::{PF_Boolean, PF_Err_NONE, PF_ParamDef, PF_ParamType, PF_Pixel};
use colored::Colorize;
use dlopen2::raw::Library;
use std::path::{Path, PathBuf};

const DEFAULT_ENTRY_POINT_NAME: &str = "EffectMain";

type EntryPointFunc = unsafe extern "C" fn(
	cmd: after_effects::RawCommand,
	in_data: *mut after_effects_sys::PF_InData,
	out_data: *mut after_effects_sys::PF_OutData,
	params: after_effects_sys::PF_ParamList,
	output: *mut after_effects_sys::PF_LayerDef,
	extra: *mut ::std::os::raw::c_void,
) -> after_effects_sys::PF_Err;

/// Represents an instance of an After Effects plugin
pub struct PluginInstance {
	library: Option<Library>,
	entry_point: Option<EntryPointFunc>,
	entry_point_name: Option<String>,
	entry_point_candidates: Vec<String>,
	path: PathBuf,
	cmd: after_effects::RawCommand,
	global_setup_done: bool,
	params_setup_done: bool,
	world: after_effects_sys::PF_LayerDef,

	utility_callbacks: Box<after_effects_sys::_PF_UtilCallbacks>,

	/// Basic Suite pointer
	pub pica: Box<after_effects_sys::SPBasicSuite>,

	/// InData structure
	pub in_data: after_effects_sys::PF_InData,
	out_data: after_effects_sys::PF_OutData,
	params: Vec<after_effects_sys::PF_ParamDef>,
	input_layer: wrapper::Layer<wrapper::Depth8>,
	lllllayer: wrapper::Layer<wrapper::Depth8>,
}

impl PluginInstance {
	fn make_input_layer_param(&self) -> PF_ParamDef {
		let layer = self.input_layer.as_sys();

		PF_ParamDef {
			ui_flags: 0,
			flags: 0,
			param_type: ParamType::Layer as PF_ParamType,
			name: [0; 32],
			ui_height: 0,
			ui_width: 0,
			unused: 0,
			u: after_effects_sys::PF_ParamDefUnion { ld: layer },
			uu: after_effects_sys::PF_ParamDef__bindgen_ty_1 { id: 0 },
		}
	}

	fn sync_render_params_from_host(&mut self) {
		let effect_ref = self.in_data.effect_ref;
		if effect_ref.is_null() {
			return;
		}

		let host_params = crate::host::params::get_params(effect_ref);
		if host_params.is_empty() {
			return;
		}

		let mut params = Vec::with_capacity(host_params.len() + 1);
		params.push(self.make_input_layer_param());
		params.extend(host_params);

		self.params = params;
		self.in_data.num_params = self.params.len() as i32;
	}

	/// Create a new PluginInstance with default values
	pub fn new(path: &Path) -> Self {
		let width = 1920;
		let height = 1080;
		// Initialize Interact Callbacks using factory
		let interact_callbacks = crate::host::interact::create_interact_callbacks();

		// Initialize Utility Callbacks using factory
		let utility_callbacks = crate::host::utility::create_utility_callbacks();

		let input_layer = wrapper::Layer::<wrapper::Depth8>::new(
			width,
			height,
			vec![wrapper::Pixel::<wrapper::Depth8>::black(); (width * height) as usize],
		)
		.unwrap();

		let ld = input_layer.as_sys();

		let fs_d = after_effects_sys::PF_FloatSliderDef {
			//* Parameter Value */
			value: 83.56,
			phase: 0.0,
			value_desc: [0; 32],

			//* Parameter Description */
			valid_min: 0.0,
			valid_max: 1000.0,
			slider_min: 0.0,
			slider_max: 100.0,
			dephault: 100.0,
			precision: 2,
			display_flags: 0,
			fs_flags: 0,
			curve_tolerance: 0.0,
			useExponent: false as PF_Boolean,
			exponent: 1.0,
		};

		let param_list = vec![
			after_effects_sys::PF_ParamDef {
				ui_flags: 0,
				flags: 0,
				param_type: 0 as after_effects_sys::PF_ParamType, // Layer
				name: [0; 32],
				ui_height: 0,
				ui_width: 0,
				unused: 0,
				u: after_effects_sys::PF_ParamDefUnion { ld },
				uu: after_effects_sys::PF_ParamDef__bindgen_ty_1 { id: 0 },
			},
			after_effects_sys::PF_ParamDef {
				ui_flags: 0,
				flags: 0,
				param_type: 10 as after_effects_sys::PF_ParamType, // Float Slider,
				name: [0; 32],
				ui_height: 0,
				ui_width: 0,
				unused: 0,
				u: after_effects_sys::PF_ParamDefUnion { fs_d },
				uu: after_effects_sys::PF_ParamDef__bindgen_ty_1 { id: 0 },
			},
		];

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
			entry_point_candidates: vec![DEFAULT_ENTRY_POINT_NAME.to_string()],
			path: path.to_path_buf(),
			cmd: after_effects::RawCommand::About,
			global_setup_done: false,
			params_setup_done: false,
			utility_callbacks,
			pica,
			in_data: crate::core::helpers::InDataBuilder::new()
				.with_size(1280, 720)
				.with_callbacks(interact_callbacks)
				// .with_global_data(unsafe { crate::suites::handle::host_new_handle_impl(0) })
				.build(),
			out_data: crate::core::helpers::OutDataBuilder::new().build(),
			params: param_list,
			input_layer,
			lllllayer: wrapper::Layer::<wrapper::Depth8>::new(
				width,
				height,
				vec![wrapper::Pixel::<wrapper::Depth8>::black(); (width * height) as usize],
			)
			.unwrap(),
			world: crate::core::helpers::LayerDefBuilder::new()
				.with_size(width as i32, height as i32)
				.build(),
		};

		// Now set the utils pointer to reference our owned utility_callbacks
		instance.in_data.utils = instance.utility_callbacks.as_mut() as *mut _;
		instance.in_data.pica_basicP = instance.pica.as_mut() as *mut _;
		instance.in_data.effect_ref = instance.in_data.global_data as _;
		instance.in_data.num_params = instance.params.len() as i32;
		instance.world.data = instance.lllllayer.buffer_mut().as_mut_ptr() as *mut PF_Pixel;

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

	fn resolve_entry_point(
		lib: &Library,
		candidates: &[String],
	) -> Result<(EntryPointFunc, String)> {
		let mut last_error = None;

		for candidate in candidates {
			match unsafe { lib.symbol::<EntryPointFunc>(candidate) } {
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

	pub fn load(&mut self) -> Result<()> {
		let dir =
			self.path
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
			"windows" => format!("{}/{}.aex", dir, name),
			"macos" => format!("{}/{}.plugin/Contents/MacOS/{}", dir, name, name),
			_ => {
				return Err(AexloError::UnsupportedOS { os: os.to_string() });
			}
		};

		log::info!("Detected OS: {}.", os.blue());
		//* --------------------------------------------- */
		//* ---- Load Plugin --------------------------- *//
		log::info!(
			"Loading plugin: {} from {}.",
			name.blue(),
			module_path.blue()
		);

		// Check if the plugin file exists
		if !std::path::Path::new(&module_path).exists() {
			return Err(AexloError::PluginNotFound { path: module_path });
		}

		let lib = Library::open(&module_path)?;
		let (entry_point, resolved_name) =
			Self::resolve_entry_point(&lib, &self.entry_point_candidates)?;

		self.entry_point = Some(entry_point);
		self.entry_point_name = Some(resolved_name.clone());
		self.library = Some(lib);

		// Set plugin path for get_platform_data callback
		crate::host::utility::set_plugin_path(std::path::Path::new(&module_path));

		log::info!("Resolved entry point symbol: {}.", resolved_name.blue());

		log::info!("Loaded plugin {}.", "successfully".green());
		//* -------------------------------------------- *//

		Ok(())
	}

	/// Call the plugin entry point
	fn call_plugin(&mut self) -> Result<()> {
		let entry_point_name = self
			.entry_point_name
			.as_deref()
			.unwrap_or(DEFAULT_ENTRY_POINT_NAME);

		log::info!(
			"Calling {} with command: {}...",
			entry_point_name.blue(),
			format!("{:?}", self.cmd).blue()
		);

		let mut params_ptr: Vec<*mut PF_ParamDef> =
			self.params.iter_mut().map(|p| p as *mut _).collect();

		let entry_point = self.entry_point.ok_or(AexloError::ContainerNotLoaded)?;

		let result = unsafe {
			entry_point(
				self.cmd,
				&mut self.in_data,
				&mut self.out_data,
				params_ptr.as_mut_ptr(),
				&mut self.world,
				std::ptr::null_mut(),
			)
		};

		if !self.out_data.global_data.is_null() {
			self.in_data.global_data = self.out_data.global_data;
			self.in_data.effect_ref = self.in_data.global_data as _;
		}

		if !self.out_data.sequence_data.is_null() {
			self.in_data.sequence_data = self.out_data.sequence_data;
		}

		log::info!(
			"Called {} {}.",
			entry_point_name.blue(),
			"successfully".green()
		);
		log::debug!(
			"{} exited with code: {}.",
			entry_point_name.blue(),
			result.to_string().blue()
		);

		//* ---- Check for errors ---------------------- *//
		match result as u32 {
			PF_Err_NONE => {
				log::info!("Plugin executed {}.", "successfully".green());
			}
			code => {
				return Err(AexloError::PluginExecutionFailed {
					code: code.try_into().unwrap(),
				});
			}
		}
		//* -------------------------------------------- *//

		Ok(())
	}

	/// Call the plugin with `PF_Cmd_ABOUT` command
	pub fn about(&mut self) -> Result<String> {
		self.cmd = after_effects::RawCommand::About;
		self.call_plugin()?;

		Ok(self.message())
	}

	/// Call the plugin with `PF_Cmd_GLOBAL_SETUP` command
	pub fn setup_global(&mut self) -> Result<()> {
		self.cmd = after_effects::RawCommand::GlobalSetup;
		self.call_plugin()?;
		self.global_setup_done = true;
		self.params_setup_done = false;

		Ok(())
	}

	/// Call the plugin with `PF_Cmd_PARAMS_SETUP` command
	pub fn setup_params(&mut self) -> Result<()> {
		if !self.global_setup_done {
			log::debug!("GlobalSetup not executed yet; running it before ParamsSetup");
			self.setup_global()?;
		}

		if self.params_setup_done {
			log::debug!("Skipping duplicate ParamsSetup for plugin instance");
			return Ok(());
		}

		self.cmd = after_effects::RawCommand::ParamsSetup;
		self.call_plugin()?;
		self.sync_render_params_from_host();
		self.params_setup_done = true;

		Ok(())
	}

	/// Call the plugin with `PF_Cmd_RENDER` command
	pub fn render(&mut self) -> Result<()> {
		self.sync_render_params_from_host();
		self.cmd = after_effects::RawCommand::Render;
		self.call_plugin()?;

		Ok(())
	}

	/// Get a reference to the internal output layer (zero-copy).
	/// Use `write_output_rgba()` for best performance.
	pub fn output_layer_ref(&self) -> &wrapper::Layer<wrapper::Depth8> {
		&self.lllllayer
	}

	/// Write output pixels directly to an RGBA buffer (zero-allocation).
	/// The buffer must have exactly `width * height * 4` bytes.
	/// Returns `true` on success, `false` if buffer size mismatches.
	pub fn write_output_rgba(&self, buffer: &mut [u8]) -> bool {
		self.lllllayer.write_rgba_bytes(buffer)
	}

	/// Get output dimensions (width, height).
	pub fn output_size(&self) -> (u32, u32) {
		(self.lllllayer.width(), self.lllllayer.height())
	}

	/// [Deprecated] Creates a copy of the output layer.
	/// Prefer `write_output_rgba()` for zero-copy performance.
	#[deprecated(since = "0.1.0", note = "Use write_output_rgba() for zero-copy")]
	pub fn output_layer(&self) -> wrapper::Layer<wrapper::Depth8> {
		let width = self.world.width;
		let height = self.world.height;
		let pixels = self.lllllayer.buffer().to_vec();

		wrapper::Layer::new(width as u32, height as u32, pixels).unwrap()
	}

	/// Add a parameter definition dynamically
	pub(crate) fn add_param(&mut self, param: after_effects_sys::PF_ParamDef) {
		self.params.push(param);
	}

	/// Get the number of parameters
	pub fn param_count(&self) -> usize {
		self.params.len()
	}

	/// Set a float parameter value by index.
	pub fn set_param_float(&mut self, index: usize, value: f64) -> Result<()> {
		// Check index bounds
		if index >= self.params.len() {
			return Err(AexloError::ParamIndexOutOfBounds {
				index,
				max: self.params.len(),
			});
		}

		// Check if this is a float slider type (param_type == 10)
		let target_param = &mut self.params[index];
		if target_param.param_type != ParamType::FloatSlider as PF_ParamType {
			return Err(AexloError::ParamTypeMismatch {
				index,
				expected: "FloatSlider",
				actual: target_param.param_type,
			});
		}

		// SAFETY: We verified param type, so fs_d is the active union variant
		target_param.u.fs_d.value = value;
		Ok(())
	}

	/// Get a float parameter value by index.
	/// Returns `None` if index out of bounds or not a float param.
	pub fn get_param_float(&self, index: usize) -> Option<f64> {
		if index >= self.params.len() {
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
	/// ```no_run
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
}
