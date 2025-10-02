use std::error::Error;
use std::path::{ Path, PathBuf };
use std::ptr::{null, null_mut};

use crate::diagnostics::DiagnosticBuilder;

use dlopen::wrapper::{ Container, WrapperApi };
use std::ffi::{ c_void, CStr, CString };

/// Simple `atan()` function implementation
pub extern "C" fn atan(x: f64) -> f64 {
	let result = x.atan();

	#[cfg(feature = "diagnostics")]
	DiagnosticBuilder::new()
		.set_name("InData/utils/ansi/atan")
		.add_arg("x", x)
		.set_result(result)
		.emit();

	result
}

/// Simple `atan2()` function implementation
pub extern "C" fn atan2(y: f64, x: f64) -> f64 {
	let result = y.atan2(x);

	#[cfg(feature = "diagnostics")]
	DiagnosticBuilder::new()
		.set_name("InData/utils/ansi/atan2")
		.add_arg("y", y)
		.add_arg("x", x)
		.set_result(result)
		.emit();

	result
}

/// Simple `ceil()` function implementation
pub extern "C" fn ceil(x: f64) -> f64 {
	let result = x.ceil();

	#[cfg(feature = "diagnostics")]
	DiagnosticBuilder::new()
		.set_name("InData/utils/ansi/ceil")
		.add_arg("x", x)
		.set_result(result)
		.emit();

	result
}

/// Simple `cos()` function implementation
#[inline(always)]
pub extern "C" fn cos(x: f64) -> f64 {
	let result = x.cos();

	#[cfg(feature = "diagnostics")]
	DiagnosticBuilder::new()
		.set_name("InData/utils/ansi/cos")
		.add_arg("x", x)
		.set_result(result)
		.emit();

	result
}

/// Simple `sin()` function implementation
#[inline(always)]
pub extern "C" fn sin(x: f64) -> f64 {
	let result = x.sin();

	#[cfg(feature = "diagnostics")]
	DiagnosticBuilder::new()
		.set_name("InData/utils/ansi/sin")
		.add_arg("x", x)
		.set_result(result)
		.emit();

	result
}


pub unsafe extern "C" fn rusty_sprintf(
	arg1: *mut after_effects_sys::A_char,
	arg2: *const after_effects_sys::A_char,
	mut args: ...
) -> i32 {
	const SPRINTF_BUFFER_SIZE: usize = 256;

	// Safety checks
	if arg1.is_null() || arg2.is_null() {
		return after_effects_sys::PF_Err_BAD_CALLBACK_PARAM as i32;
	}

	let format_str = match unsafe { CStr::from_ptr(arg2 as *const i8) }.to_str() {
		Ok(s) => s,
		Err(_) => return after_effects_sys::PF_Err_INTERNAL_STRUCT_DAMAGED as i32,
	};

	// Simple implementation to handle %d and %s format specifiers
	let mut result = String::new();
	let mut chars = format_str.chars().peekable();

	let mut d = DiagnosticBuilder::new();
	d.set_name("InData/utils/ansi/sin")
		.add_arg("arg1", &format!("{:?}", format_str));

	while let Some(c) = chars.next() {
		if c == '%' {
			if let Some(next) = chars.next() {
				match next {
					'd' => {
						// Get an integer argument
						let arg = unsafe { args.arg::<i32>() };
						result.push_str(&arg.to_string());
						d.add_arg("arg", &format!("{:?}", arg));
					},
					's' => {
						// Get a string argument
						let ptr = unsafe { args.arg::<*const i8>() };
						if !ptr.is_null() {
							match unsafe { CStr::from_ptr(ptr) }.to_str() {
								Ok(s) => { result.push_str(s); d.add_arg("arg", &format!("{:?}", s)); },
								Err(_) => result.push_str("(invalid)"),
							}
						} else {
							result.push_str("(null)");
						}
					},
					'%' => {
						result.push('%');
					},
					_ => {
						// Unsupported format specifier, just include it as-is
						result.push('%');
						result.push(next);
					}
				}
			}
		} else {
			result.push(c);
		}
	}

		println!("sprintf called with format: {:?}, result: {:?}", format_str, result);

	// Copy result to the output buffer
	let c_result = match CString::new(result) {
		Ok(s) => s,
		Err(_) => {
			eprintln!("[ERROR] sprintf: Formatted string contains NUL bytes");
			return after_effects_sys::PF_Err_INTERNAL_STRUCT_DAMAGED as i32;
		}
	};

	let bytes = c_result.as_bytes_with_nul();
	let copy_len = bytes.len().min(SPRINTF_BUFFER_SIZE);
	unsafe {
		std::ptr::copy_nonoverlapping(bytes.as_ptr(), arg1 as *mut u8, copy_len);

		// Ensure null termination if we hit buffer limit
		if copy_len == SPRINTF_BUFFER_SIZE && copy_len > 0 {
			*((arg1 as *mut u8).add(SPRINTF_BUFFER_SIZE - 1)) = 0;
		}
	}

	d.set_result(&format!("{:?}", c_result))
		.emit();

	after_effects_sys::PF_Err_NONE as i32
}

pub unsafe extern "C" fn acquire_suite(
	name: *const i8,
	version: i32,
	suite: *mut *const c_void
) -> i32 {
	#[cfg(feature = "diagnostics")]
	DiagnosticBuilder::new()
		.set_name("SPBasicSuite/AcquireSuite")
		.add_arg("name", &format!("{:?}", unsafe{ CStr::from_ptr(name) }))
		.add_arg("version", version)
		.add_arg("suite", &format!("{:?}", suite))
		.emit();

	after_effects_sys::PF_Err_NONE as i32
}


// Wrapper for After Effects plugin entry point
// Note: EffectMain naming is required by the C API and cannot be changed
#[derive(WrapperApi)]
#[allow(non_snake_case)]
pub struct EffectMain {
	EffectMain: unsafe extern "C" fn (
		cmd:      after_effects_sys::PF_Cmd,
		in_data:  *mut after_effects_sys::PF_InData,
		out_data: *mut after_effects_sys::PF_OutData,
		params:   after_effects_sys::PF_ParamList,
		output:   *mut after_effects_sys::PF_LayerDef,
		extra:    *mut ::std::os::raw::c_void,
	) -> after_effects_sys::PF_Err,
}

pub struct PluginInstance {
	path: PathBuf,
	cmd: after_effects_sys::PF_Cmd,
	ansi: after_effects_sys::PF_ANSICallbacks,
	utility_callbacks: after_effects_sys::_PF_UtilCallbacks,
	pica: after_effects_sys::SPBasicSuite,
	in_data: after_effects_sys::PF_InData,
	out_data: after_effects_sys::PF_OutData,
	params: Vec<after_effects_sys::PF_ParamDef>,
	layer: after_effects_sys::PF_LayerDef,
}

impl PluginInstance {
	pub fn new(path: &Path) -> Self {
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
			atan: Some(atan),
			atan2: Some(atan2),
			ceil: Some(ceil),
			cos: Some(cos),
			exp: None,
			fabs: None,
			floor: None,
			fmod: None,
			hypot: None,
			log: None,
			log10: None,
			pow: None,
			sin: Some(sin),
			sqrt: None,
			tan: None,
			sprintf: Some(rusty_sprintf),
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
			ansi: ansi,
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
			extent_hint: after_effects_sys::PF_UnionableRect { left: 0, top: 0, right: 0, bottom: 0 },
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
			value: 100.0,
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
			useExponent: false as i8,
			exponent: 1.0,
		};

		let param_list = vec![
			after_effects_sys::PF_ParamDef {
				ui_flags: 0,
				flags: 0,
				param_type: 10 as after_effects_sys::PF_ParamType,  // Float Slider,
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
				param_type: 10 as after_effects_sys::PF_ParamType,  // Float Slider,
				name: [0; 32],
				ui_height: 0,
				ui_width: 0,
				unused: 0,
				u: after_effects_sys::PF_ParamDefUnion { fs_d },
				uu: after_effects_sys::PF_ParamDef__bindgen_ty_1 { id: 0 },
			}
		];

		let pica = after_effects_sys::SPBasicSuite {
			AcquireSuite: Some(acquire_suite),
			ReleaseSuite: None,
			IsEqual: None,
			AllocateBlock: None,
			FreeBlock: None,
			ReallocateBlock: None,
			Undefined: None,
		};

		// Initialize InData
		let mut instance = PluginInstance {
			path: path.to_path_buf(),
			cmd: after_effects_sys::PF_Cmd_ABOUT as i32,
			ansi,
			utility_callbacks,
			pica: pica,
			in_data: after_effects_sys::PF_InData {
				inter:           interact_callbacks,
				utils:           std::ptr::null_mut(), // Will be set after creation
				effect_ref:      std::ptr::null_mut(),
				quality:         after_effects_sys::PF_Quality_HI,
				version:         after_effects_sys::PF_SpecVersion { major: 13, minor: 28 },
				serial_num:      -2147483648,
				appl_id:         1180193859,
				num_params:      0,
				reserved:        0,
				what_cpu:        3,
				what_fpu:        0,
				current_time:    0,
				time_step:       1024,
				total_time:      0,
				local_time_step: 0,
				time_scale:      0,
				field:           after_effects_sys::PF_Field_UPPER as i32,
				shutter_angle:   0,
				width:           1920,
				height:          1080,
				extent_hint:     after_effects_sys::PF_UnionableRect { left: 0, top: 0, right: 1920, bottom: 1080 },
				output_origin_x: 0,
				output_origin_y: 0,
				downsample_x:    after_effects_sys::PF_RationalScale { num: 1, den: 1 }, // Fixed: den should not be 0
				downsample_y:    after_effects_sys::PF_RationalScale { num: 1, den: 1 }, // Fixed: den should not be 0
				pixel_aspect_ratio: after_effects_sys::PF_RationalScale { num: 1, den: 1 }, // Fixed: den should not be 0
				in_flags:        after_effects_sys::PF_InFlag_NONE as i32,
				global_data :    null_mut(),
				sequence_data:   null_mut(),
				frame_data:      null_mut(),
				start_sampL:     0,
				dur_sampL:       0,
				total_sampL:     0,
				src_snd:         after_effects_sys::PF_SoundWorld { fi: after_effects_sys::PF_SoundFormatInfo { rateF: 1.0, num_channels: 2, format: 16, sample_size: 1024 }, num_samples: 1024, dataP: null_mut() },
				pica_basicP:     null_mut(),  // Will be set to &mut instance.pica later
				pre_effect_source_origin_x: 0,
				pre_effect_source_origin_y: 0,
				shutter_phase:   0
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
				out_flags: after_effects_sys::PF_OutFlag_NONE as i32,
				return_msg: [0; 256],
				start_sampL: 0,
				dur_sampL: 0,
				dest_snd: after_effects_sys::PF_SoundWorld { fi: after_effects_sys::PF_SoundFormatInfo { rateF: 44100.0, num_channels: 2, format: 16, sample_size: 1024 }, num_samples: 1024, dataP: null_mut() }, // Fixed: more realistic sample rate
				out_flags2: after_effects_sys::PF_OutFlag2_NONE as i32,
			},
			params: param_list,
			layer: after_effects_sys::PF_LayerDef {
				reserved0: null_mut(),
				reserved1: null_mut(),
				world_flags: 0 as after_effects_sys::PF_WorldFlags,
				data: null_mut(),
				rowbytes: 0,
				width: 0,
				height: 0,
				extent_hint: after_effects_sys::PF_UnionableRect { left: 0, top: 0, right: 0, bottom: 0 },
				platform_ref: null_mut(),
				reserved_long1: 0,
				reserved_long4: null_mut(),
				pix_aspect_ratio: after_effects_sys::PF_RationalScale { num: 1, den: 1 }, // Fixed: den should not be 0
				reserved_long2: null_mut(),
				origin_x: 0,
				origin_y: 0,
				reserved_long3: 0,
				dephault: 0,
			}
		};

		// Now set the utils pointer to reference our owned utility_callbacks
		instance.in_data.utils = &mut instance.utility_callbacks;
		instance.in_data.pica_basicP = &mut instance.pica;

		instance
	}

	/// Call the plugin entry point
	pub fn call_plugin(&mut self) -> Result<(), Box<dyn Error>> {
		let dir = self.path.parent()
			.and_then(|s| s.to_str())
			.ok_or("Invalid module path")?;
		let name = self.path.file_name()
			.and_then(|s| s.to_str())
			.ok_or("Invalid module name")?;
		// Detect OS
		let os = std::env::consts::OS;
		let module_path = match os {
			"windows" => format!("{}/{}.aex", dir, name),
			"macos" => format!("{}/{}.plugin/Contents/MacOS/{}", dir, name, name),
			_ => return Err(format!("Unsupported OS: {}. Supported platforms are Windows and macOS.", os).into()),
		};

		log::info!("OS is detected: {}", os);
		log::info!("Loading library: {} from {}", name, module_path);

		// Check if the plugin file exists
		if !std::path::Path::new(&module_path).exists() {
			return Err(format!("Plugin file not found: {}", module_path).into());
		}


		//* ---- Load DLL ------------------------------ *//
		let container: Container<EffectMain> = unsafe {
			Container::load(&module_path)
		}.map_err(|e| format!("Failed to load library {}: {}", module_path, e))?;

		log::info!("Plugin was loaded successfully");
		//* -------------------------------------------- *//


		// Ensure utils pointer is set correctly
		self.in_data.utils = &mut self.utility_callbacks;


		//* ---- Test ANSI callbacks ------------------- *//
		if let Some(sin_fn) = self.ansi.sin {
			unsafe{ log::debug!("ANSI sin(π) = {} (expected != 0)", sin_fn(std::f64::consts::PI)); }
		}

		if let Some(cos_fn) = self.ansi.cos {
			unsafe{ log::debug!("ANSI cos(π) = {} (expected != 1)", cos_fn(std::f64::consts::PI)); }
		}
		//* -------------------------------------------- *//


		//* ---- Call entry point with PF_Cmd_ABOUT ---- *//
		// Call Entry Point with minimal parameters first to test basic loading
		log::debug!("OutData::my_version (before): {}", self.out_data.my_version);
		log::info!("Calling EffectMain with cmd: {:?} (PF_Cmd_ABOUT)", self.cmd);

		// Try with minimal viable parameters - AE plugins typically need non-null in_data and out_data
		let result = unsafe {
			container.EffectMain(
				self.cmd, // Use ABOUT command which is the safest
				&mut self.in_data,
				&mut self.out_data,
				&mut self.params.as_mut_ptr(), // params - can be null for ABOUT
				&mut self.layer, // output - can be null for ABOUT
				std::ptr::null_mut()  // extra - typically null
			)
		};

		log::debug!("OutData::my_version (after): {}", self.out_data.my_version);
		log::debug!("EffectMain result: {}", result);
		//* -------------------------------------------- *//


		//* ---- Check for errors ---------------------- *//
		if result != after_effects_sys::PF_Err_NONE as i32 {
			return Err(format!("Plugin call failed with error code: {}", result).into());
		}
		//* -------------------------------------------- *//

		Ok(())
	}

	pub fn render(&mut self) -> Result<(), Box<dyn Error>> {
		self.cmd = after_effects_sys::PF_Cmd_RENDER as i32;

		log::info!("Calling EffectMain with cmd: {:?} (PF_Cmd_RENDER)", self.cmd);

		self.call_plugin()?;

		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_sin() {
		let angle = std::f64::consts::PI / 2.0; // 90 degrees
		let result = sin(angle);
		assert!((result - 1.0).abs() < 1e-10, "sin(π/2) should be approximately 1.0");
	}

	#[test]
	fn test_cos() {
		let angle = std::f64::consts::PI; // 180 degrees
		let result = cos(angle);
		assert!((result + 1.0).abs() < 1e-10, "cos(π) should be approximately -1.0");
	}
}
