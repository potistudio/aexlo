use after_effects_sys::*;

use super::super::world_transform::Copy_sys;

// ============================================================================
// Stub Implementations (Logging Only)
// ============================================================================

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
	_a_kernel: *mut ::std::os::raw::c_void,
	_r_kernel: *mut ::std::os::raw::c_void,
	_g_kernel: *mut ::std::os::raw::c_void,
	_b_kernel: *mut ::std::os::raw::c_void,
	_dst: *mut PF_EffectWorld,
) -> PF_Err {
	log::warn!("STUB: convolve called");
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn copy_hq_stub(
	_effect_ref: PF_ProgPtr,
	_src: *mut PF_EffectWorld,
	_dst: *mut PF_EffectWorld,
	_src_r: *mut PF_Rect,
	_dst_r: *mut PF_Rect,
) -> PF_Err {
	log::warn!("STUB: copy_hq called");
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

// ============================================================================
// Factory Function
// ============================================================================

/// Creates a dynamically allocated `PF_WorldTransformSuite1` instance.
/// All function pointers are populated with either real implementations or logging stubs.
pub fn create_world_transform_suite_1() -> Box<PF_WorldTransformSuite1> {
	Box::new(PF_WorldTransformSuite1 {
		composite_rect: Some(composite_rect_stub),
		blend: Some(blend_stub),
		convolve: Some(convolve_stub),
		copy: Some(Copy_sys),
		copy_hq: Some(copy_hq_stub),
		transfer_rect: Some(transfer_rect_stub),
		transform_world: Some(transform_world_stub),
	})
}
