//! Factory for After Effects PF_HandleSuite1
//! Handle Implementation
//!
//! A PF_Handle is a pointer to a pointer (*mut *mut c_void).
//! We implement this by allocating:
//!   [size: usize][PADDING][...user_data...]
//! The `user_data` MUST be 16-byte aligned to support SIMD operations safely.
//! The handle points to the user_data start.

#[cfg(feature = "diagnostics")]
use crate::diagnostics::*;
use after_effects_sys::*;
use std::alloc::{Layout, alloc, dealloc, realloc};
use std::os::raw::c_void;
use std::ptr;

/// Alignment for handle memory allocations.
/// AE plugins often use SIMD, so we align to 16 bytes (max_align_t equivalent for x64/SIMD).
const HANDLE_ALIGNMENT: usize = 16;
// const SIZE_PREFIX: usize = std::mem::size_of::<usize>(); // Not strictly needed if we just cast

/// Allocates a new handle with the given size.
/// Returns a pointer to a pointer (handle indirection level).
///
/// # Safety
/// This function is unsafe because it deals with raw pointers and manual memory management.
pub(crate) unsafe extern "C" fn host_new_handle_impl(size: A_HandleSize) -> PF_Handle {
	#[cfg(feature = "diagnostics")]
	{
		DiagnosticBuilder::new()
			.set_name("PF_HandleSuite1/host_new_handle")
			.add_arg("size", size)
			.set_result(0)
			.emit();
	}

	// Calculate layout: size prefix + padding to ensure next byte is aligned + user size
	// We want the user data pointer (returned address) to be aligned to HANDLE_ALIGNMENT.
	// Simple strategy:
	// Allocation: [ Size(usize) | Padding | User Data ... ]
	// To ensure User Data is 16-byte aligned, we can allocate (size + 16) and adjust,
	// or validly calculating offset.
	//
	// Better approach for strict alignment:
	// We need 8 bytes for size.
	// If we align the *allocation* to 16 bytes:
	// Addr: 0x...0  -> Size (8 bytes)
	// Addr: 0x...8  -> Padding (8 bytes)
	// Addr: 0x...10 -> User Data (Aligned 16)
	// Output: Pointer to 0x...10
	// Total allocation size = 16 (header) + size

	let header_size = HANDLE_ALIGNMENT; // Space for usize size + padding
	let total_size = header_size + size as usize;

	let layout = match Layout::from_size_align(total_size, HANDLE_ALIGNMENT) {
		Ok(l) => l,
		Err(_) => {
			log::error!("host_new_handle: layout error for size {}", size);
			return ptr::null_mut();
		}
	};

	let ptr = unsafe { alloc(layout) };
	if ptr.is_null() {
		log::error!("host_new_handle: allocation failed for size {}", size);
		return ptr::null_mut();
	}

	// Store size at the beginning of allocation
	unsafe { *(ptr as *mut usize) = size as usize };

	// User data starts at offset 16 (HANDLE_ALIGNMENT)
	let user_ptr = unsafe { ptr.add(header_size) };

	// Alloc a handle (pointer to pointer)
	// NOTE: We also use `alloc` for the handle itself to avoid panic on OOM from Box::new
	// The handle itself is small (one pointer), align of usize is sufficient.
	let handle_layout = Layout::new::<*mut c_void>();

	// Safe to use alloc for small layout, but still unsafe fn
	let handle_ptr = unsafe { alloc(handle_layout) as *mut *mut c_void };

	if handle_ptr.is_null() {
		log::error!("host_new_handle: handle storage allocation failed");
		// Cleanup the data buffer we just allocated
		unsafe { dealloc(ptr, layout) };
		return ptr::null_mut();
	}

	unsafe { *handle_ptr = user_ptr as *mut c_void };

	handle_ptr as PF_Handle
}

/// Locks the handle and returns the data pointer.
pub(crate) unsafe extern "C" fn host_lock_handle_impl(pf_handle: PF_Handle) -> *mut c_void {
	#[cfg(feature = "diagnostics")]
	{
		DiagnosticBuilder::new()
			.set_name("PF_HandleSuite1/host_lock_handle")
			.add_arg("pf_handle", format!("{:?}", pf_handle))
			.set_result(0)
			.emit();
	}

	if pf_handle.is_null() {
		return ptr::null_mut();
	}

	// Dereference handle to get user data pointer
	unsafe { *pf_handle }
}

/// Unlocks the handle. (No-op in this simple implementation)
pub(crate) unsafe extern "C" fn host_unlock_handle_impl(pf_handle: PF_Handle) {
	#[cfg(feature = "diagnostics")]
	log::trace!("host_unlock_handle called");

	let _ = pf_handle;
}

/// Disposes the handle and frees memory.
pub(crate) unsafe extern "C" fn host_dispose_handle_impl(pf_handle: PF_Handle) {
	#[cfg(feature = "diagnostics")]
	log::trace!("host_dispose_handle called");

	if pf_handle.is_null() {
		return;
	}

	// 1. Get the pointer to user data
	let user_ptr = unsafe { *(pf_handle as *mut *mut u8) };

	// 2. Free the user data buffer if it exists
	if !user_ptr.is_null() {
		// Calculate base pointer (subtract header size)
		let header_size = HANDLE_ALIGNMENT;

		// Unsafe sub
		let base_ptr = unsafe { user_ptr.sub(header_size) };

		// Read size
		let size = unsafe { *(base_ptr as *mut usize) };
		let total_size = header_size + size;

		// Reconstruct layout
		if let Ok(layout) = Layout::from_size_align(total_size, HANDLE_ALIGNMENT) {
			unsafe { dealloc(base_ptr, layout) };
		} else {
			log::error!("host_dispose_handle: failed to recreate layout during free");
		}
	}

	// 3. Free the handle storage itself
	// We allocated this with `alloc(Layout::new::<*mut c_void>())`
	let handle_layout = Layout::new::<*mut c_void>();
	unsafe { dealloc(pf_handle as *mut u8, handle_layout) };
}

/// Returns the size of the allocated data.
pub(crate) unsafe extern "C" fn host_get_handle_size_impl(pf_handle: PF_Handle) -> A_HandleSize {
	#[cfg(feature = "diagnostics")]
	log::trace!("host_get_handle_size called");

	if pf_handle.is_null() {
		return 0;
	}

	let user_ptr = unsafe { *(pf_handle as *mut *mut u8) };
	if user_ptr.is_null() {
		return 0;
	}

	// Back up to read size
	let header_size = HANDLE_ALIGNMENT;
	let base_ptr = unsafe { user_ptr.sub(header_size) };
	unsafe { *(base_ptr as *mut usize) as A_HandleSize }
}

/// Resizes the handle to the new size.
pub(crate) unsafe extern "C" fn host_resize_handle_impl(
	new_sizeL: A_HandleSize,
	handlePH: *mut PF_Handle,
) -> PF_Err {
	#[cfg(feature = "diagnostics")]
	log::trace!("host_resize_handle called, new_size: {}", new_sizeL);

	// Deref handlePH to check if it points to a handle
	if handlePH.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}
	let pf_handle = unsafe { *handlePH };

	if pf_handle.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	let user_ptr = unsafe { *(pf_handle as *mut *mut u8) };

	if user_ptr.is_null() {
		// If the handle exists but points to NULL, treat as new alloc?
		// Standard behavior usually implies a valid handle has valid data or strict rules.
		// For safety, let's fail.
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	let header_size = HANDLE_ALIGNMENT;
	let base_ptr = unsafe { user_ptr.sub(header_size) };
	let old_size = unsafe { *(base_ptr as *mut usize) };

	let old_total = header_size + old_size;
	let new_total = header_size + new_sizeL as usize;

	// Realloc
	// We trusted layout was created with HANDLE_ALIGNMENT
	if let Ok(old_layout) = Layout::from_size_align(old_total, HANDLE_ALIGNMENT) {
		let new_ptr = unsafe { realloc(base_ptr, old_layout, new_total) };

		if new_ptr.is_null() {
			log::error!("host_resize_handle: realloc failed");
			return PF_Err_OUT_OF_MEMORY as PF_Err;
		}

		// Update size in prefix
		unsafe { *(new_ptr as *mut usize) = new_sizeL as usize };

		// Update handle to point to new user data
		let new_user_ptr = unsafe { new_ptr.add(header_size) };

		// Update the handle to point to the new user pointer
		unsafe { *(pf_handle as *mut *mut u8) = new_user_ptr };

		PF_Err_NONE as PF_Err
	} else {
		log::error!("host_resize_handle: invalid old layout logic");
		PF_Err_INTERNAL_STRUCT_DAMAGED as PF_Err
	}
}

// ============================================================================
// Factory Function
// ============================================================================

/// Creates a dynamically allocated `PF_HandleSuite1` instance with working implementations.
pub fn create_handle_suite_1() -> Box<PF_HandleSuite1> {
	Box::new(PF_HandleSuite1 {
		host_new_handle: Some(host_new_handle_impl),
		host_lock_handle: Some(host_lock_handle_impl),
		host_unlock_handle: Some(host_unlock_handle_impl),
		host_dispose_handle: Some(host_dispose_handle_impl),
		host_get_handle_size: Some(host_get_handle_size_impl),
		host_resize_handle: Some(host_resize_handle_impl),
	})
}
