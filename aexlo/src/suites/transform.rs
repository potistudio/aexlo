use after_effects_sys::*;
use rayon::prelude::*;

/// Emulates `PF_WorldTransformSuite1::copy` function
/// # Safety
/// This function is unsafe because it handles raw pointers.
pub unsafe extern "C" fn Copy_sys(
	_effect_ref: PF_ProgPtr,
	src: *mut PF_EffectWorld,
	dst: *mut PF_EffectWorld,
	src_r: *mut PF_Rect,
	dst_r: *mut PF_Rect,
) -> PF_Err {
	// Handle null source/dest pointers gracefully
	if src.is_null() || dst.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	// Check if src and dst point to the same buffer
	if std::ptr::eq(src, dst) {
		// Same buffer - nothing to copy
		return PF_Err_NONE as PF_Err;
	}

	let src_world = &mut unsafe { *src };
	let dst_world = &mut unsafe { *dst };

	// Calculate buffer addresses for overlap detection
	// Buffers overlap if: src_addr <= dst_addr + dst_size && dst_addr <= src_addr + src_size
	let src_addr = src_world.data as usize;
	let dst_addr = dst_world.data as usize;
	let src_size = (src_world.height as usize) * (src_world.rowbytes as usize);
	let dst_size = (dst_world.height as usize) * (dst_world.rowbytes as usize);
	let src_end = src_addr.saturating_add(src_size);
	let dst_end = dst_addr.saturating_add(dst_size);
	let buffers_overlap = !(src_end <= dst_addr || dst_end <= src_addr);

	// Determine source rectangle
	let src_rect = if !src_r.is_null() {
		unsafe { *src_r }
	} else {
		PF_Rect {
			left: 0,
			top: 0,
			right: src_world.width,
			bottom: src_world.height,
		}
	};

	// Determine destination point (top-left)
	let (dst_x, dst_y) = if !dst_r.is_null() {
		((unsafe { *dst_r }).left, (unsafe { *dst_r }).top)
	} else {
		(src_rect.left, src_rect.top)
	};

	// Calculate copy dimensions
	let copy_width = (src_rect.right - src_rect.left).max(0);
	let copy_height = (src_rect.bottom - src_rect.top).max(0);

	if copy_width == 0 || copy_height == 0 {
		return PF_Err_NONE as PF_Err; // Nothing to copy
	}

	// Clipping: Ensure we don't read/write out of bounds
	// Source clipping
	let src_clamped_left = src_rect.left.max(0);
	let src_clamped_top = src_rect.top.max(0);
	// We also need to clip width/height based on src bounds
	let src_avail_width = src_world.width - src_clamped_left;
	let src_avail_height = src_world.height - src_clamped_top;

	// Dest clipping
	let dst_clamped_left = dst_x.max(0); // If negative dst, we must increment src start
	let dst_clamped_top = dst_y.max(0);

	let dst_avail_width = dst_world.width - dst_clamped_left;
	let dst_avail_height = dst_world.height - dst_clamped_top;

	// Adjust for negative destination offsets (if dst_x < 0, we skip pixels in src)
	let skip_x = if dst_x < 0 { -dst_x } else { 0 };
	let skip_y = if dst_y < 0 { -dst_y } else { 0 };

	// Final dimensions to copy
	let final_width = (copy_width - skip_x)
		.min(src_avail_width)
		.min(dst_avail_width);
	let final_height = (copy_height - skip_y)
		.min(src_avail_height)
		.min(dst_avail_height);

	if final_width <= 0 || final_height <= 0 {
		return PF_Err_NONE as PF_Err;
	}

	// Calculate starting offsets
	let actual_src_left = src_clamped_left + skip_x;
	let actual_src_top = src_clamped_top + skip_y;
	let actual_dst_left = dst_x + skip_x;
	let actual_dst_top = dst_y + skip_y;

	// Prepare data for parallel execution
	// We cast to usize to pass across threads safely (assuming buffers are accessible/pinned)
	let src_buffer_addr = src_world.data as usize;
	let src_rowbytes = src_world.rowbytes as isize;
	let dst_buffer_addr = dst_world.data as usize;
	let dst_rowbytes = dst_world.rowbytes as isize;

	// Size of a pixel in bytes. PF_EffectWorld usually PF_Pixel8 (4 bytes).
	// But strictly it depends on deep color.
	// For now assuming PF_Pixel8 (ARGB 8-bit).
	let pixel_size = std::mem::size_of::<PF_Pixel8>();

	// Parallel Copy
	(0..final_height).into_par_iter().for_each(|y| {
		let current_src_y = actual_src_top + y;
		let current_dst_y = actual_dst_top + y;

		// Calculate row start addresses
		// Note: data is *mut c_void, treating as *mut u8 for offset
		let src_row_ptr =
			(src_buffer_addr as *const u8).wrapping_offset((current_src_y as isize) * src_rowbytes);
		let dst_row_ptr =
			(dst_buffer_addr as *mut u8).wrapping_offset((current_dst_y as isize) * dst_rowbytes);

		// Calculate signal offsets within the row
		let src_pixel_ptr = src_row_ptr.wrapping_add((actual_src_left as usize) * pixel_size);
		let dst_pixel_ptr = dst_row_ptr.wrapping_add((actual_dst_left as usize) * pixel_size);

		// Use std::ptr::copy if buffers overlap (safe for overlapping regions),
		// otherwise use copy_nonoverlapping for better performance
		unsafe {
			if buffers_overlap {
				std::ptr::copy(
					src_pixel_ptr,
					dst_pixel_ptr,
					(final_width as usize) * pixel_size,
				);
			} else {
				std::ptr::copy_nonoverlapping(
					src_pixel_ptr,
					dst_pixel_ptr,
					(final_width as usize) * pixel_size,
				);
			}
		}
	});

	PF_Err_NONE as PF_Err
}

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
/// Returns a Box<> that will be converted to Arc by the registry.
pub fn create_world_transform_suite_1() -> Box<PF_WorldTransformSuite1> {
	let suite = Box::new(PF_WorldTransformSuite1 {
		composite_rect: Some(composite_rect_stub),
		blend: Some(blend_stub),
		convolve: Some(convolve_stub),
		copy: Some(Copy_sys),
		copy_hq: Some(copy_hq_stub),
		transfer_rect: Some(transfer_rect_stub),
		transform_world: Some(transform_world_stub),
	});
	suite
}
