use std::error::Error;
use std::ffi::{CStr, CString, c_void};
use std::path::{Path, PathBuf};
use std::ptr::null_mut;

use after_effects_sys::*;
use colored::Colorize;
use dlopen::wrapper::{Container, WrapperApi};

use crate::diagnostics::DiagnosticBuilder;

unsafe extern "C" {
	fn Iterate8(a: i32, b: i32) -> i32;
}

static SUITE_CONTAINER: SuiteContainer = SuiteContainer {
	iterate_8_suite: PF_Iterate8Suite2 {
		iterate: Some(rusty_iterate_8),
		iterate_origin: None,
		iterate_lut: None,
		iterate_origin_non_clip_src: None,
		iterate_generic: None,
	},
	world_transform_suite: PF_WorldTransformSuite1 {
		composite_rect: None,
		blend: None,
		convolve: None,
		copy: Some(rusty_copy),
		copy_hq: None,
		transfer_rect: None,
		transform_world: None,
	},
};

pub struct SuiteContainer {
	iterate_8_suite: PF_Iterate8Suite2,
	world_transform_suite: PF_WorldTransformSuite1,
}

/// Emulates `PF_WorldTransformSuite1::copy` function
/// # Safety
/// This function is unsafe because it handles raw pointers.
pub unsafe extern "C" fn rusty_copy(
	effect_ref: PF_ProgPtr,
	src: *mut PF_EffectWorld,
	dst: *mut PF_EffectWorld,
	src_r: *mut PF_Rect,
	dst_r: *mut PF_Rect,
) -> PF_Err {
	#[cfg(feature = "diagnostics")]
	DiagnosticBuilder::new()
		.set_name("PF World Transform Suite/Copy")
		.add_arg("effect_ref", effect_ref as usize)
		.add_arg("src", src as usize)
		.add_arg("dst", dst as usize)
		.add_arg(
			"src_r",
			if !src_r.is_null() {
				format!("{:?}", src_r)
			} else {
				"(null)".to_string()
			},
		)
		.add_arg(
			"dst_r",
			if !dst_r.is_null() {
				format!("{:?}", dst_r)
			} else {
				"(null)".to_string()
			},
		)
		.set_result(0)
		.emit();

	PF_Err_NONE as PF_Err
}

/// Emulates `SPBasicSuite::AcquireSuite` function
/// # Safety
/// This function is unsafe because it handles raw pointers.
pub unsafe extern "C" fn rusty_acquire_suite(
	name: *const i8,
	version: i32,
	suite: *mut *const c_void,
) -> i32 {
	if suite.is_null() || name.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	unsafe {
		let suite_name = match CStr::from_ptr(name).to_str() {
			Ok(s) => s,
			Err(_) => return PF_Err_INTERNAL_STRUCT_DAMAGED as PF_Err,
		};

		#[cfg(feature = "diagnostics")]
		DiagnosticBuilder::new()
			.set_name("SPBasicSuite/AcquireSuite")
			.add_arg("name", format!("{:?}", CStr::from_ptr(name)))
			.add_arg("version", version)
			.add_arg("suite", format!("{:?}", suite))
			.emit();

		match (suite_name, version) {
			("PF World Transform Suite", 1) => {
				*suite = &SUITE_CONTAINER.world_transform_suite as *const _ as *mut c_void;

				log::info!("Acquired PF World Transform Suite v1");
				PF_Err_NONE as PF_Err
			}
			("PF Iterate8 Suite", 2) => {
				*suite = &SUITE_CONTAINER.iterate_8_suite as *const _ as *mut c_void;

				log::info!("Acquired PF Iterate8 Suite v2");
				PF_Err_NONE as PF_Err
			}
			_ => {
				log::warn!("Requested unknown suite: {} v{}", suite_name, version);
				PF_Err_OUT_OF_MEMORY as PF_Err
			}
		}
	}
}

/// Emulates `SPBasicSuite::ReleaseSuite` function
/// # Safety
/// This function is unsafe because it handles raw pointers.
pub unsafe extern "C" fn rusty_release_suite(
	name: *const ::std::os::raw::c_char,
	version: int32,
) -> PF_Err {
	#[cfg(feature = "diagnostics")]
	DiagnosticBuilder::new()
		.set_name("SPBasicSuite/ReleaseSuite")
		.add_arg("name", format!("{:?}", unsafe { CStr::from_ptr(name) }))
		.add_arg("version", version)
		.emit();

	if name.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn rusty_iterate_8(
	in_data: *mut PF_InData,
	progress_base: A_long,
	progress_final: A_long,
	src: *mut PF_EffectWorld,
	area: *const PF_Rect,
	refcon: *mut ::std::os::raw::c_void,
	pix_fn: ::std::option::Option<
		unsafe extern "C" fn(
			refcon: *mut ::std::os::raw::c_void,
			x: A_long,
			y: A_long,
			in_: *mut PF_Pixel,
			out: *mut PF_Pixel,
		) -> PF_Err,
	>,
	dst: *mut PF_EffectWorld,
) -> PF_Err {
	#[cfg(feature = "diagnostics")]
	DiagnosticBuilder::new()
		.set_name("PF Iterate8 Suite/iterate")
		.add_arg("in_data", format!("{:?}", in_data))
		.add_arg("progress_base", progress_base)
		.add_arg("progress_final", progress_final)
		.add_arg("src", format!("{:?}", src))
		.add_arg(
			"area",
			if !area.is_null() {
				format!("{:?}", area)
			} else {
				"(null)".to_string()
			},
		)
		.add_arg("refcon", format!("{:?}", refcon))
		.add_arg("pix_fn", if pix_fn.is_some() { "Some" } else { "None" })
		.add_arg("dst", format!("{:?}", dst))
		.set_result(0)
		.emit();

	let func = match pix_fn {
		Some(f) => f,
		None => return PF_Err_NONE as PF_Err,
	};

	let destination_layer = unsafe { &mut *dst };
	let width = destination_layer.width;
	let height = destination_layer.height;
	let pixels = width * height;

	let pixel_slice =
		unsafe { std::slice::from_raw_parts_mut(destination_layer.data, pixels as usize) };

	let mut in_pixel = wrapper::Pixel::<wrapper::Depth8>::black();
	let mut out_pixel = wrapper::Pixel::<wrapper::Depth8>::black();

	let in_ptr = &mut in_pixel as *mut _ as *mut PF_Pixel8;
	let out_ptr = &mut out_pixel as *mut _ as *mut PF_Pixel8;

	for (i, pixel) in pixel_slice.iter_mut().enumerate() {
		let x = i as i32 % width;
		let y = i as i32 / width;

		unsafe { func(refcon, x, y, in_ptr, out_ptr) };

		*pixel = out_pixel.into()
	}

	println!("{}", unsafe { Iterate8(1, 2) });

	PF_Err_NONE as PF_Err
}

/// Wrapper for After Effects plugin entry point
/// Note: EffectMain naming is required by the C API and cannot be changed
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
	ansi: after_effects_sys::PF_ANSICallbacks,
	utility_callbacks: after_effects_sys::_PF_UtilCallbacks,

	/// Basic Suite pointer
	pub pica: Box<after_effects_sys::SPBasicSuite>,

	/// InData structure
	pub in_data: after_effects_sys::PF_InData,
	out_data: after_effects_sys::PF_OutData,
	params: Vec<after_effects_sys::PF_ParamDef>,
	pub layer: after_effects_sys::PF_LayerDef,
	lllllayer: wrapper::Layer<wrapper::Depth8>,
}

impl PluginInstance {
	/// Create a new PluginInstance with default values
	pub fn new(path: &Path) -> Self {
		let width = 1920;
		let height = 1080;
		// Initialize Interact Callbacks
		let interact_callbacks = after_effects_sys::PF_InteractCallbacks {
			checkout_param: None,
			checkin_param: None,
			add_param: None,
			abort: None,
			progress: None,
			register_ui: None,
			checkout_layer_audio: None,
			checkin_layer_audio: None,
			get_audio_data: None,
			reserved_str: [std::ptr::null_mut(); 3],
			reserved: [std::ptr::null_mut(); 10],
		};

		let ansi = after_effects_sys::PF_ANSICallbacks {
			atan: Some(crate::ansi::atan_sys),
			atan2: Some(crate::ansi::atan2_sys),
			ceil: Some(crate::ansi::ceil_sys),
			cos: Some(crate::ansi::cos_sys),
			exp: None,
			fabs: None,
			floor: None,
			fmod: None,
			hypot: None,
			log: None,
			log10: None,
			pow: None,
			sin: Some(crate::ansi::sin_sys),
			sqrt: None,
			tan: None,
			sprintf: Some(crate::ansi::sprintf_sys),
			strcpy: None,
			asin: None,
			acos: None,
			ansi_procs: [0; 1],
		};

		let color = after_effects_sys::PF_ColorCallbacks {
			RGBtoHLS: None,
			HLStoRGB: None,
			RGBtoYIQ: None,
			YIQtoRGB: None,
			Luminance: None,
			Hue: None,
			Lightness: None,
			Saturation: None,
		};

		let utility_callbacks = after_effects_sys::_PF_UtilCallbacks {
			begin_sampling: None,
			subpixel_sample: None,
			area_sample: None,
			get_batch_func_is_deprecated: std::ptr::null_mut(),
			end_sampling: None,
			composite_rect: None,
			blend: None,
			convolve: None,
			copy: None,
			fill: None,
			gaussian_kernel: None,
			iterate: None,
			premultiply: None,
			premultiply_color: None,
			new_world: None,
			dispose_world: None,
			iterate_origin: None,
			iterate_lut: None,
			transfer_rect: None,
			transform_world: None,
			host_new_handle: None,
			host_lock_handle: None,
			host_unlock_handle: None,
			host_dispose_handle: None,
			get_callback_addr: None,
			app: None,
			ansi,
			colorCB: color,
			get_platform_data: None,
			host_get_handle_size: None,
			iterate_origin_non_clip_src: None,
			iterate_generic: None,
			host_resize_handle: None,
			subpixel_sample16: None,
			area_sample16: None,
			fill16: None,
			premultiply_color16: None,
			iterate16: None,
			iterate_origin16: None,
			iterate_origin_non_clip_src16: None,
			get_pixel_data8: None,
			get_pixel_data16: None,
			reserved: [0; 1],
		};

		let ld = after_effects_sys::PF_LayerDef {
			reserved0: null_mut(),
			reserved1: null_mut(),
			world_flags: 0 as after_effects_sys::PF_WorldFlags,
			data: null_mut(),
			rowbytes: 0,
			width: 0,
			height: 0,
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
		};

		let fs_d = after_effects_sys::PF_FloatSliderDef {
			//* Parameter Value */
			value: 50.0,
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
				param_type: 10 as after_effects_sys::PF_ParamType, // Float Slider,
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
			AcquireSuite: Some(rusty_acquire_suite),
			ReleaseSuite: Some(rusty_release_suite),
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
			ansi,
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
				width: 1920,
				height: 1080,
				extent_hint: after_effects_sys::PF_UnionableRect {
					left: 0,
					top: 0,
					right: 1920,
					bottom: 1080,
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
			lllllayer: wrapper::Layer::<wrapper::Depth8>::blank(width, height),
			layer: after_effects_sys::PF_LayerDef {
				reserved0: null_mut(),
				reserved1: null_mut(),
				world_flags: 0 as after_effects_sys::PF_WorldFlags,
				data: null_mut(),
				rowbytes: width as i32,
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
		instance.in_data.utils = &mut instance.utility_callbacks;
		instance.in_data.pica_basicP = instance.pica.as_mut() as *mut _;
		instance.layer.data = instance.lllllayer.pixels.as_mut_ptr() as *mut PF_Pixel;

		instance
	}

	pub fn load(&mut self) -> Result<(), Box<dyn Error>> {
		let dir = self
			.path
			.parent()
			.and_then(|s| s.to_str())
			.ok_or("Invalid module path")?;
		let name = self
			.path
			.file_name()
			.and_then(|s| s.to_str())
			.ok_or("Invalid module name")?;

		//* ---- Detect OS ------------------------------ */
		log::info!("Detecting OS...");
		let os = std::env::consts::OS;
		let module_path = match os {
			"windows" => format!("{}/{}.aex", dir, name),
			"macos" => format!("{}/{}.plugin/Contents/MacOS/{}", dir, name, name),
			_ => {
				return Err(format!(
					"Unsupported OS: {}. Supported platforms are Windows and macOS.",
					os
				)
				.into());
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
			return Err(format!("Plugin not found: {}", module_path).into());
		}

		self.container = Some(
			unsafe { Container::load(&module_path) }
				.map_err(|e| format!("Failed to load plugin {}: {}", module_path, e))?,
		);

		log::info!("Loaded plugin {}.", "successfully".green());
		//* -------------------------------------------- *//

		Ok(())
	}

	/// Call the plugin entry point
	fn call_plugin(&mut self) -> Result<(), Box<dyn Error>> {
		log::info!(
			"Calling EffectMain with command: {}...",
			format!("{:?}", self.cmd).blue()
		);

		let mut params_ptr: Vec<*mut PF_ParamDef> =
			self.params.iter_mut().map(|p| p as *mut _).collect();

		let container = self
			.container
			.as_ref()
			.ok_or("Plugin container is not loaded. Call load() before calling the plugin.")?;

		let result = unsafe {
			container.EffectMain(
				self.cmd,
				&mut self.in_data,
				&mut self.out_data,
				params_ptr.as_mut_ptr(),
				&mut self.layer,
				std::ptr::null_mut(),
			)
		};

		log::info!("Called EffectMain {}.", "successfully".green());
		log::debug!(
			"EffectMain exited with code: {}.",
			result.to_string().blue()
		);

		//* ---- Check for errors ---------------------- *//
		match result as PF_Err {
			PF_Err_NONE => {
				log::info!("Plugin executed {}.", "successfully".green());
			}
			_ => {
				return Err(format!("Plugin has failed with error: {}.", result).into());
			}
		}
		//* -------------------------------------------- *//

		Ok(())
	}

	/// Call the plugin with PF_Cmd_RENDER command
	pub fn about(&mut self) -> Result<(), Box<dyn Error>> {
		self.cmd = after_effects::RawCommand::About;
		self.call_plugin()?;

		Ok(())
	}

	/// Call the plugin with PF_Cmd_RENDER command
	pub fn setup_global(&mut self) -> Result<(), Box<dyn Error>> {
		self.cmd = after_effects::RawCommand::GlobalSetup;
		self.call_plugin()?;

		Ok(())
	}

	/// Call the plugin with PF_Cmd_RENDER command
	pub fn setup_params(&mut self) -> Result<(), Box<dyn Error>> {
		self.cmd = after_effects::RawCommand::ParamsSetup;
		self.call_plugin()?;

		Ok(())
	}

	/// Call the plugin with PF_Cmd_RENDER command
	pub fn render(&mut self) -> Result<(), Box<dyn Error>> {
		self.cmd = after_effects::RawCommand::Render;
		self.call_plugin()?;

		Ok(())
	}

	pub fn output_layer(&self) -> wrapper::Layer<wrapper::Depth8> {
		let width = self.layer.width;
		let height = self.layer.height;
		let pixels = self.lllllayer.pixels.clone();

		wrapper::Layer::new(
			width as u32,
			height as u32,
			pixels.iter().map(|p| (*p)).collect(),
		)
	}
}
