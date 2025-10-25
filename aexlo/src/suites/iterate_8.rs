use crate::diagnostics::*;
use after_effects_sys::*;
use std::os::raw::c_void;

unsafe extern "C" {
	fn Iterate8(
		pixel_count: i32,
		in_layer: *mut PF_Pixel8,
		out_layer: *mut PF_Pixel8,
		controller: *const c_void,
		func: Option<
			unsafe extern "C" fn(
				refcon: *mut c_void,
				x: A_long,
				y: A_long,
				in_pixel: *mut PF_Pixel8,
				out_pixel: *mut PF_Pixel8,
			) -> PF_Err,
		>,
	);
}

pub(super) unsafe extern "C" fn iterate_8_sys(
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
		.set_name("Iterate8Suite/Iterate8")
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

	let destination_layer = unsafe { &mut *dst };
	let width = destination_layer.width as u32;
	let height = destination_layer.height as u32;
	let pixels = width * height;

	let pixel_slice =
		unsafe { std::slice::from_raw_parts_mut(destination_layer.data, pixels as usize) };

	let mut in_layer = wrapper::Layer::blank(width, height);

	let in_layer_sys = in_layer
		.pixels
		.iter()
		.map(|p| (*p).into())
		.collect::<Vec<PF_Pixel8>>();

	let in_ptr = in_layer_sys.as_ptr() as *mut PF_Pixel8;
	let out_ptr = pixel_slice.as_mut_ptr() as *mut PF_Pixel8;

	unsafe { Iterate8(pixels as i32, in_ptr, out_ptr, refcon, pix_fn) };

	PF_Err_NONE as PF_Err
}
