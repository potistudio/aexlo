use crate::suites::SuiteContainer;
use after_effects_sys::*;
use anyhow::{Context, Result, bail};
use colored::Colorize;
use dlopen::wrapper::{Container, WrapperApi};
use std::path::{Path, PathBuf};
use std::ptr::null_mut;

use crate::diagnostics::DiagnosticBuilder;

unsafe extern "C" fn rusty_add_param(
	effect_ref: PF_ProgPtr,
	index: PF_ParamIndex,
	def: PF_ParamDefPtr,
) -> PF_Err {
	#[cfg(feature = "diagnostics")]
	DiagnosticBuilder::new()
		.set_name("PF Interact Callbacks/AddParam")
		.add_arg("effect_ref", effect_ref as usize)
		.add_arg("index", index)
		.add_arg("def", format!("{:?}", def))
		.set_result(0)
		.emit();

	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn CheckoutParam_sys(
	effect_ref: PF_ProgPtr,
	index: PF_ParamIndex,
	what_time: A_long,
	time_step: A_long,
	time_scale: A_u_long,
	param: *mut PF_ParamDef,
) -> PF_Err {
	#[cfg(feature = "diagnostics")]
	DiagnosticBuilder::new()
		.set_name("InteractCallbacks/RegisterUI")
		.add_arg("effect_ref", effect_ref as usize)
		.add_arg("index", index)
		.add_arg("what_time", what_time)
		.add_arg("time_step", time_step)
		.add_arg("time_scale", time_scale)
		.add_arg("param", format! {"{:?}", param})
		.set_result(0)
		.emit();

	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn checkin_param_sys(effect_ref: PF_ProgPtr, param: *mut PF_ParamDef) -> PF_Err {
	#[cfg(feature = "diagnostics")]
	DiagnosticBuilder::new()
		.set_name("InteractCallbacks/CheckinParam")
		.add_arg("effect_ref", effect_ref as usize)
		.add_arg("param", format! {"{:?}", param})
		.set_result(0)
		.emit();

	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn RegisterUI_sys(
	effect_ref: PF_ProgPtr,
	cust_info: *mut PF_CustomUIInfo,
) -> PF_Err {
	#[cfg(feature = "diagnostics")]
	DiagnosticBuilder::new()
		.set_name("InteractCallbacks/RegisterUI")
		.add_arg("effect_ref", effect_ref as usize)
		.add_arg("cust_info", format!("{:?}", cust_info))
		.set_result(0)
		.emit();

	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn GetPlatformData_sys(
	effect_ref: PF_ProgPtr,
	which: PF_PlatDataID,
	data: *mut ::std::os::raw::c_void,
) -> PF_Err {
	#[cfg(feature = "diagnostics")]
	DiagnosticBuilder::new()
		.set_name("UtilityCallbacks/GetPlatformData")
		.add_arg("effect_ref", effect_ref as usize)
		.add_arg("which", which)
		.add_arg("data", format!("{:?}", data))
		.set_result(0)
		.emit();

	PF_Err_NONE as PF_Err
}

/// Wrapper for After Effects plugin entry point \
/// Note: EffectMain naming is required by the AE API and cannot be changed
#[derive(WrapperApi)]
#[repr(C)]
pub struct EffectMain {
	#[allow(non_snake_case)]
	EffectMain: unsafe extern "C" fn(
		cmd: after_effects::RawCommand,
		in_data: *mut after_effects_sys::PF_InData,
		out_data: *mut after_effects_sys::PF_OutData,
		params: after_effects_sys::PF_ParamList,
		output: *mut after_effects_sys::PF_LayerDef,
		extra: *mut ::std::os::raw::c_void,
	) -> after_effects_sys::PF_Err,
}

/// Represents an instance of an After Effects plugin
pub struct PluginInstance {
	container: Option<Container<EffectMain>>,
	path: PathBuf,
	cmd: after_effects::RawCommand,
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
	/// Create a new PluginInstance with default values
	pub fn new(path: &Path) -> Self {
		let width = 1920;
		let height = 1080;
		// Initialize Interact Callbacks using factory
		let interact_callbacks = crate::suites::factories::create_interact_callbacks();

		// Initialize Utility Callbacks using factory
		let utility_callbacks = crate::suites::factories::create_utility_callbacks();

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
			container: None,
			path: path.to_path_buf(),
			cmd: after_effects::RawCommand::About,
			utility_callbacks,
			pica,
			in_data: after_effects_sys::PF_InData {
				inter: interact_callbacks,
				utils: null_mut(), // Will be set later
				effect_ref: std::ptr::null_mut(),
				quality: after_effects_sys::PF_Quality_HI,
				version: after_effects_sys::PF_SpecVersion {
					major: 13,
					minor: 28,
				},
				serial_num: -2147483648,
				appl_id: 1180193859,
				num_params: 0,
				reserved: 0,
				what_cpu: 3,
				what_fpu: 0,
				current_time: 0,
				time_step: 1024,
				total_time: 0,
				local_time_step: 0,
				time_scale: 0,
				field: PF_Field_UPPER as PF_Field,
				shutter_angle: 0,
				width: 1280,
				height: 720,
				extent_hint: after_effects_sys::PF_UnionableRect {
					left: 0,
					top: 0,
					right: 1280,
					bottom: 720,
				},
				output_origin_x: 0,
				output_origin_y: 0,
				downsample_x: after_effects_sys::PF_RationalScale { num: 1, den: 1 }, // Fixed: den should not be 0
				downsample_y: after_effects_sys::PF_RationalScale { num: 1, den: 1 }, // Fixed: den should not be 0
				pixel_aspect_ratio: after_effects_sys::PF_RationalScale { num: 1, den: 1 }, // Fixed: den should not be 0
				in_flags: PF_InFlag_NONE as PF_InFlags,
				global_data: null_mut(),
				sequence_data: null_mut(),
				frame_data: null_mut(),
				start_sampL: 0,
				dur_sampL: 0,
				total_sampL: 0,
				src_snd: after_effects_sys::PF_SoundWorld {
					fi: after_effects_sys::PF_SoundFormatInfo {
						rateF: 1.0,
						num_channels: 2,
						format: 16,
						sample_size: 1024,
					},
					num_samples: 1024,
					dataP: null_mut(),
				},
				pica_basicP: null_mut(), // Will be set to &mut instance.pica later
				pre_effect_source_origin_x: 0,
				pre_effect_source_origin_y: 0,
				shutter_phase: 0,
			},
			out_data: after_effects_sys::PF_OutData {
				my_version: 0,
				name: [0; 32],
				global_data: null_mut(),
				num_params: 0,
				sequence_data: null_mut(),
				flat_sdata_size: 0,
				frame_data: null_mut(),
				width: 0,
				height: 0,
				origin: after_effects_sys::PF_Point { h: 0, v: 0 },
				out_flags: after_effects_sys::PF_OutFlag_NONE as after_effects_sys::PF_OutFlags,
				return_msg: [0; 256],
				start_sampL: 0,
				dur_sampL: 0,
				dest_snd: after_effects_sys::PF_SoundWorld {
					fi: after_effects_sys::PF_SoundFormatInfo {
						rateF: 44100.0,
						num_channels: 2,
						format: 16,
						sample_size: 1024,
					},
					num_samples: 1024,
					dataP: null_mut(),
				}, // Fixed: more realistic sample rate
				out_flags2: after_effects_sys::PF_OutFlag2_NONE as after_effects_sys::PF_OutFlags2,
			},
			params: param_list,
			input_layer,
			lllllayer: wrapper::Layer::<wrapper::Depth8>::new(
				width,
				height,
				vec![wrapper::Pixel::<wrapper::Depth8>::black(); (width * height) as usize],
			)
			.unwrap(),
			world: after_effects_sys::PF_LayerDef {
				reserved0: null_mut(),
				reserved1: null_mut(),
				world_flags: 0 as after_effects_sys::PF_WorldFlags,
				data: null_mut(),
				rowbytes: (width * 4) as i32,
				width: width as i32,
				height: height as i32,
				extent_hint: after_effects_sys::PF_UnionableRect {
					left: 0,
					top: 0,
					right: 0,
					bottom: 0,
				},
				platform_ref: null_mut(),
				reserved_long1: 0,
				reserved_long4: null_mut(),
				pix_aspect_ratio: after_effects_sys::PF_RationalScale { num: 1, den: 1 }, // Fixed: den should not be 0
				reserved_long2: null_mut(),
				origin_x: 0,
				origin_y: 0,
				reserved_long3: 0,
				dephault: 0,
			},
		};

		// Now set the utils pointer to reference our owned utility_callbacks
		instance.in_data.utils = instance.utility_callbacks.as_mut() as *mut _;
		instance.in_data.pica_basicP = instance.pica.as_mut() as *mut _;
		instance.world.data = instance.lllllayer.buffer_mut().as_mut_ptr() as *mut PF_Pixel;

		instance
	}

	pub fn load(&mut self) -> Result<()> {
		let dir = self
			.path
			.parent()
			.and_then(|s| s.to_str())
			.context("Invalid module directory")?;
		let name = self
			.path
			.file_name()
			.and_then(|s| s.to_str())
			.context("Invalid module name")?;

		//* ---- Detect OS ------------------------------ */
		log::info!("Detecting OS...");
		let os = std::env::consts::OS;
		let module_path = match os {
			"windows" => format!("{}/{}.aex", dir, name),
			"macos" => format!("{}/{}.plugin/Contents/MacOS/{}", dir, name, name),
			_ => {
				bail!(
					"Unsupported OS: {}. Supported platforms are Windows and macOS.",
					os
				);
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
			bail!("Plugin not found: {}", module_path);
		}

		self.container =
			Some(unsafe { Container::load(&module_path) }.context("Failed to load plugin")?);

		log::info!("Loaded plugin {}.", "successfully".green());
		//* -------------------------------------------- *//

		Ok(())
	}

	/// Call the plugin entry point
	fn call_plugin(&mut self) -> Result<()> {
		log::info!(
			"Calling EffectMain with command: {}...",
			format!("{:?}", self.cmd).blue()
		);

		let mut params_ptr: Vec<*mut PF_ParamDef> =
			self.params.iter_mut().map(|p| p as *mut _).collect();

		let container = self
			.container
			.as_ref()
			.context("Plugin container is not loaded. Call load() before calling the plugin.")?;

		let result = unsafe {
			container.EffectMain(
				self.cmd,
				&mut self.in_data,
				&mut self.out_data,
				params_ptr.as_mut_ptr(),
				&mut self.world,
				std::ptr::null_mut(),
			)
		};

		log::info!("Called EffectMain {}.", "successfully".green());
		log::debug!(
			"EffectMain exited with code: {}.",
			result.to_string().blue()
		);

		//* ---- Check for errors ---------------------- *//
		match result as i32 {
			PF_Err_NONE => {
				log::info!("Plugin executed {}.", "successfully".green());
			}
			_ => {
				bail!("Plugin has failed with error: {}.", result);
			}
		}
		//* -------------------------------------------- *//

		Ok(())
	}

	/// Call the plugin with `PF_Cmd_ABOUT` command
	pub fn about(&mut self) -> Result<()> {
		self.cmd = after_effects::RawCommand::About;
		self.call_plugin()?;

		Ok(())
	}

	/// Call the plugin with `PF_Cmd_GLOBAL_SETUP` command
	pub fn setup_global(&mut self) -> Result<()> {
		self.cmd = after_effects::RawCommand::GlobalSetup;
		self.call_plugin()?;

		Ok(())
	}

	/// Call the plugin with `PF_Cmd_PARAMS_SETUP` command
	pub fn setup_params(&mut self) -> Result<()> {
		self.cmd = after_effects::RawCommand::ParamsSetup;
		self.call_plugin()?;

		Ok(())
	}

	/// Call the plugin with `PF_Cmd_RENDER` command
	pub fn render(&mut self) -> Result<()> {
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

	pub(crate) fn add_param(&mut self, param: after_effects_sys::PF_ParamDef) {
		self.params.push(param);
	}

	/// Get the number of parameters
	pub fn param_count(&self) -> usize {
		self.params.len()
	}

	/// Set a float parameter value by index.
	/// Returns `true` if successful, `false` if index out of bounds or not a float param.
	pub fn set_param_float(&mut self, index: usize, value: f64) -> bool {
		if index >= self.params.len() {
			return false;
		}

		// Check if this is a float slider type (param_type == 10)
		let param = &mut self.params[index];
		if param.param_type != 10 {
			log::warn!("set_param_float: param {} is not a float slider (type={})", index, param.param_type);
			return false;
		}

		// SAFETY: We verified param_type is 10 (float slider), so fs_d is the active union variant
		unsafe {
			param.u.fs_d.value = value;
		}
		true
	}

	/// Get a float parameter value by index.
	/// Returns `None` if index out of bounds or not a float param.
	pub fn get_param_float(&self, index: usize) -> Option<f64> {
		if index >= self.params.len() {
			return None;
		}

		let param = &self.params[index];
		if param.param_type != 10 {
			return None;
		}

		// SAFETY: We verified param_type is 10 (float slider), so fs_d is the active union variant
		unsafe { Some(param.u.fs_d.value) }
	}
}
