use after_effects_sys::*;
use std::os::raw::c_void;

// ============================================================================
// Stub Implementations (Logging Only)
// ============================================================================

unsafe extern "C" fn begin_sampling_stub(
	_effect_ref: PF_ProgPtr,
	_qual: PF_Quality,
	_mf: PF_ModeFlags,
	_params: *mut PF_SampPB,
) -> PF_Err {
	log::warn!("STUB: begin_sampling called");
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn subpixel_sample_stub(
	_effect_ref: PF_ProgPtr,
	_x: PF_Fixed,
	_y: PF_Fixed,
	_params: *const PF_SampPB,
	_dst_pixel: *mut PF_Pixel,
) -> PF_Err {
	log::warn!("STUB: subpixel_sample called");
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn area_sample_stub(
	_effect_ref: PF_ProgPtr,
	_x: PF_Fixed,
	_y: PF_Fixed,
	_params: *const PF_SampPB,
	_dst_pixel: *mut PF_Pixel,
) -> PF_Err {
	log::warn!("STUB: area_sample called");
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn end_sampling_stub(
	_effect_ref: PF_ProgPtr,
	_qual: PF_Quality,
	_mf: PF_ModeFlags,
	_params: *mut PF_SampPB,
) -> PF_Err {
	log::warn!("STUB: end_sampling called");
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn composite_rect_stub(
	_effect_ref: PF_ProgPtr,
	_src_rect: *mut PF_Rect,
	_src_opacity: A_long,
	_source_wld: *mut PF_EffectWorld,
	_dest_x: A_long,
	_dest_y: A_long,
	_field_rdr: PF_Field,
	_xfer_mode: PF_XferMode,
	_dest_wld: *mut PF_EffectWorld,
) -> PF_Err {
	log::warn!("STUB: composite_rect called");
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn blend_stub(
	_effect_ref: PF_ProgPtr,
	_src1: *const PF_EffectWorld,
	_src2: *const PF_EffectWorld,
	_ratio: PF_Fixed,
	_dst: *mut PF_EffectWorld,
) -> PF_Err {
	log::warn!("STUB: blend called");
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn convolve_stub(
	_effect_ref: PF_ProgPtr,
	_src: *mut PF_EffectWorld,
	_area: *const PF_Rect,
	_flags: PF_KernelFlags,
	_kernel_size: A_long,
	_a_kernel: *mut c_void,
	_r_kernel: *mut c_void,
	_g_kernel: *mut c_void,
	_b_kernel: *mut c_void,
	_dst: *mut PF_EffectWorld,
) -> PF_Err {
	log::warn!("STUB: convolve called");
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn copy_stub(
	_effect_ref: PF_ProgPtr,
	_src: *mut PF_EffectWorld,
	_dst: *mut PF_EffectWorld,
	_src_rect: *mut PF_Rect,
	_dst_rect: *mut PF_Rect,
) -> PF_Err {
	log::warn!("STUB: copy called");
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn fill_stub(
	_effect_ref: PF_ProgPtr,
	_color: *const PF_Pixel,
	_dst_rect: *const PF_Rect,
	_world: *mut PF_EffectWorld,
) -> PF_Err {
	log::warn!("STUB: fill called");
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn gaussian_kernel_stub(
	_effect_ref: PF_ProgPtr,
	_kRadius: A_FpLong,
	_flags: PF_KernelFlags,
	_multiplier: A_FpLong,
	_diameter: *mut A_long,
	_kernel: *mut c_void,
) -> PF_Err {
	log::warn!("STUB: gaussian_kernel called");
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn iterate_stub(
	_in_data: *mut PF_InData,
	_progress_base: A_long,
	_progress_final: A_long,
	_src: *mut PF_EffectWorld,
	_area: *const PF_Rect,
	_refcon: *mut c_void,
	_pix_fn: ::std::option::Option<
		unsafe extern "C" fn(
			refcon: *mut c_void,
			x: A_long,
			y: A_long,
			in_: *mut PF_Pixel,
			out: *mut PF_Pixel,
		) -> PF_Err,
	>,
	_dst: *mut PF_EffectWorld,
) -> PF_Err {
	log::warn!("STUB: iterate called");
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn premultiply_stub(
	_effect_ref: PF_ProgPtr,
	_forward: A_long,
	_dst: *mut PF_EffectWorld,
) -> PF_Err {
	log::warn!("STUB: premultiply called");
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn premultiply_color_stub(
	_effect_ref: PF_ProgPtr,
	_src: *mut PF_EffectWorld,
	_color: *const PF_Pixel,
	_forward: A_long,
	_dst: *mut PF_EffectWorld,
) -> PF_Err {
	log::warn!("STUB: premultiply_color called");
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn new_world_stub(
	_effect_ref: PF_ProgPtr,
	_width: A_long,
	_height: A_long,
	_flags: PF_NewWorldFlags,
	_world: *mut PF_EffectWorld,
) -> PF_Err {
	log::warn!("STUB: new_world called");
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn dispose_world_stub(
	_effect_ref: PF_ProgPtr,
	_world: *mut PF_EffectWorld,
) -> PF_Err {
	log::warn!("STUB: dispose_world called");
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn iterate_origin_stub(
	_in_data: *mut PF_InData,
	_progress_base: A_long,
	_progress_final: A_long,
	_src: *mut PF_EffectWorld,
	_area: *const PF_Rect,
	_origin: *const PF_Point,
	_refcon: *mut c_void,
	_pix_fn: ::std::option::Option<
		unsafe extern "C" fn(
			refcon: *mut c_void,
			x: A_long,
			y: A_long,
			in_: *mut PF_Pixel,
			out: *mut PF_Pixel,
		) -> PF_Err,
	>,
	_dst: *mut PF_EffectWorld,
) -> PF_Err {
	log::warn!("STUB: iterate_origin called");
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn iterate_lut_stub(
	_in_data: *mut PF_InData,
	_progress_base: A_long,
	_progress_final: A_long,
	_src: *mut PF_EffectWorld,
	_area: *const PF_Rect,
	_a_lut0: *mut A_u_char,
	_r_lut0: *mut A_u_char,
	_g_lut0: *mut A_u_char,
	_b_lut0: *mut A_u_char,
	_dst: *mut PF_EffectWorld,
) -> PF_Err {
	log::warn!("STUB: iterate_lut called");
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn transfer_rect_stub(
	_effect_ref: PF_ProgPtr,
	_quality: PF_Quality,
	_m_flags: PF_ModeFlags,
	_field: PF_Field,
	_src_rec: *const PF_Rect,
	_src_world: *const PF_EffectWorld,
	_comp_mode: *const PF_CompositeMode,
	_mask_world0: *const PF_MaskWorld,
	_dest_x: A_long,
	_dest_y: A_long,
	_dst_world: *mut PF_EffectWorld,
) -> PF_Err {
	log::warn!("STUB: transfer_rect called");
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn transform_world_stub(
	_effect_ref: PF_ProgPtr,
	_quality: PF_Quality,
	_m_flags: PF_ModeFlags,
	_field: PF_Field,
	_src_world: *const PF_EffectWorld,
	_comp_mode: *const PF_CompositeMode,
	_mask_world0: *const PF_MaskWorld,
	_matrices: *const PF_FloatMatrix,
	_num_matrices: A_long,
	_src2dst_matrix: PF_Boolean,
	_dest_rect: *const PF_Rect,
	_dst_world: *mut PF_EffectWorld,
) -> PF_Err {
	log::warn!("STUB: transform_world called");
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn host_new_handle_stub(_size: A_HandleSize) -> PF_Handle {
	log::warn!("STUB: host_new_handle called");
	std::ptr::null_mut()
}

unsafe extern "C" fn host_lock_handle_stub(_handle: PF_Handle) -> *mut c_void {
	log::warn!("STUB: host_lock_handle called");
	std::ptr::null_mut()
}

unsafe extern "C" fn host_unlock_handle_stub(_handle: PF_Handle) {
	log::warn!("STUB: host_unlock_handle called");
}

unsafe extern "C" fn host_dispose_handle_stub(_handle: PF_Handle) {
	log::warn!("STUB: host_dispose_handle called");
}

unsafe extern "C" fn get_callback_addr_stub(
	_effect_ref: PF_ProgPtr,
	_quality: PF_Quality,
	_mode_flags: PF_ModeFlags,
	_which_callback: PF_CallbackID,
	_fn_ptr: *mut PF_CallbackFunc,
) -> PF_Err {
	log::warn!("STUB: get_callback_addr called");
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn app_stub(_effect_ref: PF_ProgPtr, _selector: A_long, ...) -> PF_Err {
	log::warn!("STUB: app called");
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn get_platform_data_stub(
	_effect_ref: PF_ProgPtr,
	_which: PF_PlatDataID,
	_data: *mut c_void,
) -> PF_Err {
	log::warn!("STUB: get_platform_data called");
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn host_get_handle_size_stub(_handle: PF_Handle) -> A_HandleSize {
	log::warn!("STUB: host_get_handle_size called");
	0
}

unsafe extern "C" fn iterate_origin_non_clip_src_stub(
	_in_data: *mut PF_InData,
	_progress_base: A_long,
	_progress_final: A_long,
	_src: *mut PF_EffectWorld,
	_area: *const PF_Rect,
	_origin: *const PF_Point,
	_refcon: *mut c_void,
	_pix_fn: ::std::option::Option<
		unsafe extern "C" fn(
			refcon: *mut c_void,
			x: A_long,
			y: A_long,
			in_: *mut PF_Pixel,
			out: *mut PF_Pixel,
		) -> PF_Err,
	>,
	_dst: *mut PF_EffectWorld,
) -> PF_Err {
	log::warn!("STUB: iterate_origin_non_clip_src called");
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn iterate_generic_stub(
	_iterationsL: A_long,
	_refconPV: *mut c_void,
	_fn_func: ::std::option::Option<
		unsafe extern "C" fn(
			refconPV: *mut c_void,
			thread_indexL: A_long,
			i: A_long,
			iterationsL: A_long,
		) -> PF_Err,
	>,
) -> PF_Err {
	log::warn!("STUB: iterate_generic called");
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn host_resize_handle_stub(
	_new_sizeL: A_HandleSize,
	_handlePH: *mut PF_Handle,
) -> PF_Err {
	log::warn!("STUB: host_resize_handle called");
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn subpixel_sample16_stub(
	_effect_ref: PF_ProgPtr,
	_x: PF_Fixed,
	_y: PF_Fixed,
	_params: *const PF_SampPB,
	_dst_pixel: *mut PF_Pixel16,
) -> PF_Err {
	log::warn!("STUB: subpixel_sample16 called");
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn area_sample16_stub(
	_effect_ref: PF_ProgPtr,
	_x: PF_Fixed,
	_y: PF_Fixed,
	_params: *const PF_SampPB,
	_dst_pixel: *mut PF_Pixel16,
) -> PF_Err {
	log::warn!("STUB: area_sample16 called");
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn fill16_stub(
	_effect_ref: PF_ProgPtr,
	_color: *const PF_Pixel16,
	_dst_rect: *const PF_Rect,
	_world: *mut PF_EffectWorld,
) -> PF_Err {
	log::warn!("STUB: fill16 called");
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn premultiply_color16_stub(
	_effect_ref: PF_ProgPtr,
	_src: *mut PF_EffectWorld,
	_color: *const PF_Pixel16,
	_forward: A_long,
	_dst: *mut PF_EffectWorld,
) -> PF_Err {
	log::warn!("STUB: premultiply_color16 called");
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn iterate16_stub(
	_in_data: *mut PF_InData,
	_progress_base: A_long,
	_progress_final: A_long,
	_src: *mut PF_EffectWorld,
	_area: *const PF_Rect,
	_refcon: *mut c_void,
	_pix_fn: ::std::option::Option<
		unsafe extern "C" fn(
			refcon: *mut c_void,
			x: A_long,
			y: A_long,
			in_: *mut PF_Pixel16,
			out: *mut PF_Pixel16,
		) -> PF_Err,
	>,
	_dst: *mut PF_EffectWorld,
) -> PF_Err {
	log::warn!("STUB: iterate16 called");
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn iterate_origin16_stub(
	_in_data: *mut PF_InData,
	_progress_base: A_long,
	_progress_final: A_long,
	_src: *mut PF_EffectWorld,
	_area: *const PF_Rect,
	_origin: *const PF_Point,
	_refcon: *mut c_void,
	_pix_fn: ::std::option::Option<
		unsafe extern "C" fn(
			refcon: *mut c_void,
			x: A_long,
			y: A_long,
			in_: *mut PF_Pixel16,
			out: *mut PF_Pixel16,
		) -> PF_Err,
	>,
	_dst: *mut PF_EffectWorld,
) -> PF_Err {
	log::warn!("STUB: iterate_origin16 called");
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn iterate_origin_non_clip_src16_stub(
	_in_data: *mut PF_InData,
	_progress_base: A_long,
	_progress_final: A_long,
	_src: *mut PF_EffectWorld,
	_area: *const PF_Rect,
	_origin: *const PF_Point,
	_refcon: *mut c_void,
	_pix_fn: ::std::option::Option<
		unsafe extern "C" fn(
			refcon: *mut c_void,
			x: A_long,
			y: A_long,
			in_: *mut PF_Pixel16,
			out: *mut PF_Pixel16,
		) -> PF_Err,
	>,
	_dst: *mut PF_EffectWorld,
) -> PF_Err {
	log::warn!("STUB: iterate_origin_non_clip_src16 called");
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn get_pixel_data8_stub(
	_worldP: *mut PF_EffectWorld,
	_pixelsP0: PF_PixelPtr,
	_pixPP: *mut *mut PF_Pixel8,
) -> PF_Err {
	log::warn!("STUB: get_pixel_data8 called");
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn get_pixel_data16_stub(
	_worldP: *mut PF_EffectWorld,
	_pixelsP0: PF_PixelPtr,
	_pixPP: *mut *mut PF_Pixel16,
) -> PF_Err {
	log::warn!("STUB: get_pixel_data16 called");
	PF_Err_NONE as PF_Err
}

// ============================================================================
// Factory Function
// ============================================================================

/// Creates a `_PF_UtilCallbacks` instance with all callbacks populated.
pub fn create_utility_callbacks() -> Box<_PF_UtilCallbacks> {
	Box::new(_PF_UtilCallbacks {
		begin_sampling: Some(begin_sampling_stub),
		subpixel_sample: Some(subpixel_sample_stub),
		area_sample: Some(area_sample_stub),
		get_batch_func_is_deprecated: std::ptr::null_mut(),
		end_sampling: Some(end_sampling_stub),
		composite_rect: Some(composite_rect_stub),
		blend: Some(blend_stub),
		convolve: Some(convolve_stub),
		copy: Some(copy_stub),
		fill: Some(fill_stub),
		gaussian_kernel: Some(gaussian_kernel_stub),
		iterate: Some(iterate_stub),
		premultiply: Some(premultiply_stub),
		premultiply_color: Some(premultiply_color_stub),
		new_world: Some(new_world_stub),
		dispose_world: Some(dispose_world_stub),
		iterate_origin: Some(iterate_origin_stub),
		iterate_lut: Some(iterate_lut_stub),
		transfer_rect: Some(transfer_rect_stub),
		transform_world: Some(transform_world_stub),
		host_new_handle: Some(host_new_handle_stub),
		host_lock_handle: Some(host_lock_handle_stub),
		host_unlock_handle: Some(host_unlock_handle_stub),
		host_dispose_handle: Some(host_dispose_handle_stub),
		get_callback_addr: Some(get_callback_addr_stub),
		app: Some(app_stub),
		ansi: super::super::SUITE_CONTAINER.ansi, // Use existing ANSI callbacks
		colorCB: PF_ColorCallbacks {
			RGBtoHLS: None,
			HLStoRGB: None,
			RGBtoYIQ: None,
			YIQtoRGB: None,
			Luminance: None,
			Hue: None,
			Lightness: None,
			Saturation: None,
		},
		get_platform_data: Some(get_platform_data_stub),
		host_get_handle_size: Some(host_get_handle_size_stub),
		iterate_origin_non_clip_src: Some(iterate_origin_non_clip_src_stub),
		iterate_generic: Some(iterate_generic_stub),
		host_resize_handle: Some(host_resize_handle_stub),
		subpixel_sample16: Some(subpixel_sample16_stub),
		area_sample16: Some(area_sample16_stub),
		fill16: Some(fill16_stub),
		premultiply_color16: Some(premultiply_color16_stub),
		iterate16: Some(iterate16_stub),
		iterate_origin16: Some(iterate_origin16_stub),
		iterate_origin_non_clip_src16: Some(iterate_origin_non_clip_src16_stub),
		get_pixel_data8: Some(get_pixel_data8_stub),
		get_pixel_data16: Some(get_pixel_data16_stub),
		reserved: [0; 1],
	})
}
