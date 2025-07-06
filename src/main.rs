#![feature(c_variadic)]

#[macro_use]
extern crate dlopen_derive;

use dlopen::wrapper::{ Container, WrapperApi };
use std::ffi::{CStr, CString};

const BASE_PATH: &str = "./test";
const MODULE_NAME: &str = "SDK_Noise";

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sprintf (arg1: *mut after_effects_sys::A_char, arg2: *const after_effects_sys::A_char, mut args: ...) -> i32 {
	unsafe {
		const BUFFER_SIZE: usize = 256;

		let format_str = CStr::from_ptr(arg2 as *const i8).to_string_lossy();

		// Simple implementation to handle %d and %s format specifiers
		let mut result = String::new();
		let mut chars = format_str.chars().peekable();

		while let Some(c) = chars.next() {
			if c == '%' {
				if let Some(next) = chars.next() {
					match next {
						'd' => {
							// Get an integer argument
							let arg = args.arg::<i32>();
							result.push_str(&arg.to_string());
						},
						's' => {
							// Get a string argument
							let ptr = args.arg::<*const i8>();
							if !ptr.is_null() {
								let s = CStr::from_ptr(ptr).to_string_lossy();
								result.push_str(&s);
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
		if !arg1.is_null() {
			let c_result = match CString::new(result) {
				Ok(s) => s,
				Err(_nul_error) => {
			        // Log the error, e.g., using eprintln! or a proper logging setup.
			        // eprintln!("[sprintf] Error: Formatted string contains NUL bytes: {}", _nul_error);
			        // Return an error code to indicate failure.
			        return after_effects_sys::PF_Err_INTERNAL_STRUCT_DAMAGED as i32; // Or another suitable PF_Err_ code
				}
			};
			let bytes = c_result.as_bytes_with_nul();
			std::ptr::copy_nonoverlapping(bytes.as_ptr(), arg1 as *mut u8, bytes.len().min(BUFFER_SIZE));
		}
	}

	after_effects_sys::PF_Err_NONE as i32
}


#[derive(WrapperApi)]
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
	cmd: after_effects_sys::PF_Cmd,
	out_data: after_effects_sys::PF_OutData,
}

impl PluginInstance {
	fn new() -> Self {
		let out_data = after_effects_sys::PF_OutData {
			my_version: 0,
			name: [0; 32],
			global_data: std::ptr::null_mut(),
			num_params: 0,
			sequence_data: std::ptr::null_mut(),
			flat_sdata_size: 0,
			frame_data: std::ptr::null_mut(),
			width: 0,
			height: 0,
			origin: after_effects_sys::PF_Point { h: 0, v: 0 },
			out_flags: after_effects_sys::PF_OutFlag_NONE as i32,
			return_msg: [0; 256],
			start_sampL: 0,
			dur_sampL: 0,
			dest_snd: after_effects_sys::PF_SoundWorld { fi: after_effects_sys::PF_SoundFormatInfo { rateF: 1024.0, num_channels: 0, format: 16, sample_size: 1024 }, num_samples: 1024, dataP: std::ptr::null_mut() },
			out_flags2: after_effects_sys::PF_OutFlag2_NONE as i32,
		};

		PluginInstance {
			cmd: after_effects_sys::PF_Cmd_GLOBAL_SETUP,  // Default command
			out_data,
		}
	}

	//* Call the plugin entry point *//
	fn call_plugin(&mut self) -> Result<(), dlopen::Error> {

		// Detect OS
		let os = std::env::consts::OS;
		let module_path = match os {
			"windows" => format! ("{}/{}.aex", BASE_PATH, MODULE_NAME),
			"macos" => format! ("{}/{}.plugin/Contents/MacOS/{}", BASE_PATH, MODULE_NAME, MODULE_NAME),
			_ => { panic!("Cannot detect OS. Supported OS is windows and macos.") },
		};

		println!("[INFO] - Detected OS: {}", os);
		println! ("[INFO] - Loading library: {} from {}", MODULE_NAME, module_path);

		// Initialize Cmd
		let cmd = after_effects_sys::PF_Cmd_ABOUT;

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
			atan: None,
			atan2: None,
			ceil: None,
			cos: None,
			exp: None,
			fabs: None,
			floor: None,
			fmod: None,
			hypot: None,
			log: None,
			log10: None,
			pow: None,
			sin: None,
			sqrt: None,
			tan: None,
			sprintf: Some(sprintf),
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

		let mut utility_callbacks = after_effects_sys::_PF_UtilCallbacks {
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

		// Initialize InData
		let mut in_data = after_effects_sys::PF_InData {
			inter:           interact_callbacks,
			utils:           &mut utility_callbacks,
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
			downsample_x:    after_effects_sys::PF_RationalScale { num: 1, den: 0 },
			downsample_y:    after_effects_sys::PF_RationalScale { num: 1, den: 0 },
			pixel_aspect_ratio: after_effects_sys::PF_RationalScale { num: 1, den: 0 },
			in_flags:        after_effects_sys::PF_InFlag_NONE as i32,
			global_data :    std::ptr::null_mut(),
			sequence_data:   std::ptr::null_mut(),
			frame_data:      std::ptr::null_mut(),
			start_sampL:     0,
			dur_sampL:       0,
			total_sampL:     0,
			src_snd:         after_effects_sys::PF_SoundWorld { fi: after_effects_sys::PF_SoundFormatInfo { rateF: 1.0, num_channels: 2, format: 16, sample_size: 1024 }, num_samples: 1024, dataP: std::ptr::null_mut() },
			pica_basicP:     std::ptr::null_mut(),
			pre_effect_source_origin_x: 0,
			pre_effect_source_origin_y: 0,
			shutter_phase:   0
		};

		// Initialize Params
		let raw_params: Vec<after_effects_sys::PF_ParamDef> = Vec::new();
		let params: after_effects_sys::PF_ParamList = raw_params.as_ptr() as after_effects_sys::PF_ParamList;

		// Initialize Layer
		let mut layer = after_effects_sys::PF_LayerDef {
			reserved0: std::ptr::null_mut(),
			reserved1: std::ptr::null_mut(),
			world_flags: after_effects_sys::PF_NewWorldFlag_NONE as i32,
			data: std::ptr::null_mut(),
			rowbytes: 0,
			width: 0,
			height: 0,
			extent_hint: after_effects_sys::PF_UnionableRect { left: 0, top: 0, right: 0, bottom: 0 },
			platform_ref: std::ptr::null_mut(),
			reserved_long1: 0,
			reserved_long4: std::ptr::null_mut(),
			pix_aspect_ratio: after_effects_sys::PF_RationalScale { num: 1, den: 0 },
			reserved_long2: std::ptr::null_mut(),
			origin_x: 0,
			origin_y: 0,
			reserved_long3: 0,
			dephault: 0,
		};
		println! ("[INFO] - Plugin parameters initialized");

		// Load DLL
		let container: Container<EffectMain> = unsafe {
			Container::load (module_path)
		}.expect ("Cannot load library");
		println! ("[INFO] - Plugin loaded successfully");

		// Call Entry Point
		println! ("[DEBUG] - OutData::my_version (before): {}", self.out_data.my_version);
		println! ("[INFO] - Calling EffectMain with cmd: {:?}", cmd);
		unsafe {
			let result: after_effects_sys::PF_Err = container.EffectMain(cmd as i32, &mut in_data, &mut self.out_data, params, &mut layer, std::ptr::null_mut());  // expect 0
			println! ("[DEBUG] - Result: {}", result);
		}
		println! ("[DEBUG] - OutData::my_version (after): {}", self.out_data.my_version);

		Ok(())
	}
}

fn main() {
	let mut instance = PluginInstance::new();
	instance.call_plugin().expect("Failed to call plugin");

	println! ("Hello, World!");
}
