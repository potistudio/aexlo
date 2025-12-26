use after_effects_sys::*;
use std::alloc::{Layout, alloc, dealloc, realloc};
use std::os::raw::c_void;
use std::ptr;

// ============================================================================
// Handle Implementation
//
// A PF_Handle is a pointer to a pointer (*mut *mut c_void).
// We implement this by allocating:
//   [size: usize][...user_data...]
// The handle points to the user_data start, and we retrieve size from prefix.
// ============================================================================

const SIZE_PREFIX: usize = std::mem::size_of::<usize>();

/// Allocates a new handle with the given size.
/// Returns a pointer to a pointer (handle indirection level).
unsafe extern "C" fn host_new_handle_impl(size: A_HandleSize) -> PF_Handle {
	log::debug!("host_new_handle called, size: {}", size);

	let total_size = SIZE_PREFIX + size as usize;
	let layout = match Layout::from_size_align(total_size, std::mem::align_of::<usize>()) {
		Ok(l) => l,
		Err(_) => {
			log::error!("host_new_handle: invalid layout for size {}", size);
			return ptr::null_mut();
		}
	};

	let ptr = alloc(layout);
	if ptr.is_null() {
		log::error!("host_new_handle: allocation failed for size {}", size);
		return ptr::null_mut();
	}

	// Store size in prefix
	*(ptr as *mut usize) = size as usize;

	// User data pointer
	let user_ptr = ptr.add(SIZE_PREFIX);

	// Alloc a handle (pointer to pointer)
	let handle_storage = Box::new(user_ptr as *mut c_void);
	Box::into_raw(handle_storage) as PF_Handle
}

/// Locks the handle and returns the data pointer.
unsafe extern "C" fn host_lock_handle_impl(pf_handle: PF_Handle) -> *mut c_void {
	log::debug!("host_lock_handle called");
	if pf_handle.is_null() {
		return ptr::null_mut();
	}
	// Dereference handle to get user data pointer
	*(pf_handle as *mut *mut c_void)
}

/// Unlocks the handle. (No-op in this simple implementation)
unsafe extern "C" fn host_unlock_handle_impl(pf_handle: PF_Handle) {
	log::debug!("host_unlock_handle called");
	// No-op - we don't have locked/unlocked state tracking
	let _ = pf_handle;
}

/// Disposes the handle and frees memory.
unsafe extern "C" fn host_dispose_handle_impl(pf_handle: PF_Handle) {
	log::debug!("host_dispose_handle called");
	if pf_handle.is_null() {
		return;
	}

	// Get user data pointer
	let user_ptr = *(pf_handle as *mut *mut u8);
	if user_ptr.is_null() {
		// Free handles storage
		let _ = Box::from_raw(pf_handle as *mut *mut c_void);
		return;
	}

	// Get base pointer (before size prefix)
	let base_ptr = user_ptr.sub(SIZE_PREFIX);
	let size = *(base_ptr as *mut usize);

	let total_size = SIZE_PREFIX + size;
	let layout = Layout::from_size_align_unchecked(total_size, std::mem::align_of::<usize>());
	dealloc(base_ptr, layout);

	// Free the handle storage itself
	let _ = Box::from_raw(pf_handle as *mut *mut c_void);
}

/// Returns the size of the allocated data.
unsafe extern "C" fn host_get_handle_size_impl(pf_handle: PF_Handle) -> A_HandleSize {
	log::debug!("host_get_handle_size called");
	if pf_handle.is_null() {
		return 0;
	}

	let user_ptr = *(pf_handle as *mut *mut u8);
	if user_ptr.is_null() {
		return 0;
	}

	let base_ptr = user_ptr.sub(SIZE_PREFIX);
	*(base_ptr as *mut usize) as A_HandleSize
}

/// Resizes the handle to the new size.
unsafe extern "C" fn host_resize_handle_impl(
	new_sizeL: A_HandleSize,
	handlePH: *mut PF_Handle,
) -> PF_Err {
	log::debug!("host_resize_handle called, new_size: {}", new_sizeL);
	if handlePH.is_null() || (*handlePH).is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	let pf_handle = *handlePH;
	let user_ptr = *(pf_handle as *mut *mut u8);
	if user_ptr.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	let base_ptr = user_ptr.sub(SIZE_PREFIX);
	let old_size = *(base_ptr as *mut usize);

	let old_total = SIZE_PREFIX + old_size;
	let new_total = SIZE_PREFIX + new_sizeL as usize;

	let old_layout = Layout::from_size_align_unchecked(old_total, std::mem::align_of::<usize>());
	let new_ptr = realloc(base_ptr, old_layout, new_total);

	if new_ptr.is_null() {
		log::error!("host_resize_handle: realloc failed");
		return PF_Err_OUT_OF_MEMORY as PF_Err;
	}

	// Update size in prefix
	*(new_ptr as *mut usize) = new_sizeL as usize;

	// Update handle to point to new user data
	let new_user_ptr = new_ptr.add(SIZE_PREFIX);
	*(pf_handle as *mut *mut u8) = new_user_ptr;

	PF_Err_NONE as PF_Err
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
