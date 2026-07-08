//! Factory for After Effects PF_HandleSuite1
//! Handle Implementation
//!
//! A PF_Handle is a pointer to a pointer (*mut *mut c_void).
//! We implement this by allocating:
//!   [size: usize][PADDING][...user_data...]
//! The `user_data` MUST be 16-byte aligned to support SIMD operations safely.
//! The handle points to the user_data start.

#[cfg(feature = "diagnostics")]
use crate::core::diagnostics::*; // Adjusted import path
use after_effects_sys::*;
use std::alloc::{Layout, alloc, dealloc, realloc};
use std::os::raw::c_void;
use std::ptr;

/// Alignment for handle memory allocations.
/// AE plugins often use SIMD, so we align to 16 bytes (max_align_t equivalent for x64/SIMD).
const HANDLE_ALIGNMENT: usize = 16;

/// Magic number to identify valid handles created by us
const HANDLE_MAGIC: u64 = 0x4145584C4F484E44; // "AEXLOHND" in ASCII

// Handle header layout: [magic: u64][size: usize][pad to 16 bytes][user data...]

/// Allocates anew handle with the given size.
/// Returns a pointer to a pointer (handle indirection level).
///
/// # Safety
/// This function is unsafe because it deals with raw pointers and manual memory management.
pub(crate) unsafe extern "C" fn host_new_handle_impl(size: A_HandleSize) -> PF_Handle {
	// Log immediately at function entry
	log::info!("host_new_handle: ENTRY with size={} (0x{:x})", size, size);

	#[cfg(feature = "diagnostics")]
	{
		DiagnosticBuilder::new()
			.set_name("PF_HandleSuite1/host_new_handle")
			.add_arg("size", size)
			.set_result(0)
			.emit();
	}

	// Sanity check BEFORE any conversion
	// A_HandleSize is u64 on Windows, so check for unreasonably large values
	const MAX_REASONABLE_SIZE: u64 = 1_000_000_000; // 1GB
	if size > MAX_REASONABLE_SIZE {
		log::error!(
			"host_new_handle: UNREASONABLE SIZE at entry! size={} (0x{:x})",
			size,
			size
		);
		return ptr::null_mut();
	}

	let requested_size = match usize::try_from(size) {
		Ok(value) => value,
		Err(_) => {
			log::error!("host_new_handle: try_from failed for size={}", size);
			return ptr::null_mut();
		}
	};

	log::debug!("host_new_handle: converted to requested_size={}", requested_size);

	let header_size = HANDLE_ALIGNMENT; // Space for usize size + padding
	let total_size = match header_size.checked_add(requested_size) {
		Some(value) => value,
		None => {
			log::error!("host_new_handle: size overflow for requested_size={}", requested_size);
			return ptr::null_mut();
		}
	};

	log::debug!("host_new_handle: total_size={}", total_size);

	// Double check total_size before creating layout
	if total_size > MAX_REASONABLE_SIZE as usize {
		log::error!(
			"host_new_handle: total_size too large! total_size={} (0x{:x})",
			total_size,
			total_size
		);
		return ptr::null_mut();
	}

	let layout = match Layout::from_size_align(total_size, HANDLE_ALIGNMENT) {
		Ok(l) => l,
		Err(_) => {
			log::error!("host_new_handle: layout error for total_size={}", total_size);
			return ptr::null_mut();
		}
	};

	log::debug!("host_new_handle: layout created successfully");

	let ptr = unsafe { alloc(layout) };
	if ptr.is_null() {
		log::error!("host_new_handle: allocation failed for size {}", size);
		return ptr::null_mut();
	}

	// Store magic number and size at the beginning of allocation
	// Layout: [magic: u64 @ +0][size: usize @ +8][user_data @ +16]
	unsafe { *(ptr as *mut u64) = HANDLE_MAGIC };
	unsafe { *(ptr.add(8) as *mut usize) = requested_size };

	// User data starts at offset 16 (HANDLE_ALIGNMENT)
	let user_ptr = unsafe { ptr.add(header_size) };

	// Alloc a handle (pointer to pointer)
	// NOTE: We also use `alloc` for the handle itself to avoid panic on OOM from Box::new
	// The handle itself is small (one pointer), align of usize is sufficient.
	let handle_layout = Layout::new::<*mut c_void>();

	// Safe to use alloc for small layout, but still unsafe fn
	let handle_ptr = unsafe { alloc(handle_layout) } as *mut *mut c_void;

	if handle_ptr.is_null() {
		log::error!("host_new_handle: handle storage allocation failed");
		// Cleanup the data buffer we just allocated
		unsafe { dealloc(ptr, layout) };
		return ptr::null_mut();
	}

	unsafe { *handle_ptr = user_ptr as *mut c_void };

	log::info!(
		"host_new_handle: SUCCESS handle={:p}, user_ptr={:p}, size={}",
		handle_ptr,
		user_ptr,
		requested_size
	);

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
	(unsafe { *pf_handle }) as *mut c_void
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

		// Verify magic number before freeing
		let magic = unsafe { *(base_ptr as *mut u64) };
		if magic != HANDLE_MAGIC {
			log::error!(
				"host_dispose_handle: INVALID HANDLE at dispose! magic=0x{:x} (expected 0x{:x}), pf_handle={:p}, user_ptr={:p}",
				magic,
				HANDLE_MAGIC,
				pf_handle,
				user_ptr
			);
			// Don't free corrupted memory
			return;
		}

		// Read size
		let size = unsafe { *(base_ptr.add(8) as *mut usize) };
		let total_size = match header_size.checked_add(size) {
			Some(value) => value,
			None => {
				log::error!("host_dispose_handle: size overflow during free");
				return;
			}
		};

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
		log::warn!("host_get_handle_size: NULL handle passed");
		return 0;
	}

	let user_ptr = unsafe { *(pf_handle as *mut *mut u8) };
	log::debug!(
		"host_get_handle_size: pf_handle={:p}, user_ptr={:p}",
		pf_handle,
		user_ptr
	);

	if user_ptr.is_null() {
		log::warn!("host_get_handle_size: handle points to NULL user data");
		return 0;
	}

	// Back up to read magic and size
	let header_size = HANDLE_ALIGNMENT;
	let base_ptr = unsafe { user_ptr.sub(header_size) };

	// Verify magic number
	let magic = unsafe { *(base_ptr as *mut u64) };
	if magic != HANDLE_MAGIC {
		log::error!(
			"host_get_handle_size: INVALID HANDLE! magic=0x{:x} (expected 0x{:x}), pf_handle={:p}, user_ptr={:p}, base_ptr={:p}",
			magic,
			HANDLE_MAGIC,
			pf_handle,
			user_ptr,
			base_ptr
		);
		return 0;
	}

	let size = unsafe { *(base_ptr.add(8) as *mut usize) };

	log::debug!("host_get_handle_size: base_ptr={:p}, raw_size={}", base_ptr, size);

	// Sanity check: if size is unreasonably large, it's likely corrupted
	const MAX_REASONABLE_SIZE: usize = 1_000_000_000; // 1GB
	if size > MAX_REASONABLE_SIZE {
		log::error!(
			"host_get_handle_size: CORRUPTED SIZE DETECTED! handle={:p}, user_ptr={:p}, base_ptr={:p}, size={}",
			pf_handle,
			user_ptr,
			base_ptr,
			size
		);
		return 0;
	}

	match A_HandleSize::try_from(size) {
		Ok(value) => {
			log::debug!("host_get_handle_size: returning size={}", value);
			value
		}
		Err(_) => {
			log::error!("host_get_handle_size: size does not fit A_HandleSize");
			0
		}
	}
}

/// Resizes the handle to the new size.
pub(crate) unsafe extern "C" fn host_resize_handle_impl(new_sizeL: A_HandleSize, handlePH: *mut PF_Handle) -> PF_Err {
	log::debug!("host_resize_handle: new_size={}, handlePH={:p}", new_sizeL, handlePH);

	#[cfg(feature = "diagnostics")]
	log::trace!("host_resize_handle called, new_size: {}", new_sizeL);

	// Deref handlePH to check if it points to a handle
	if handlePH.is_null() {
		log::error!("host_resize_handle: handlePH is NULL");
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}
	let pf_handle = unsafe { *handlePH };

	if pf_handle.is_null() {
		log::error!("host_resize_handle: pf_handle is NULL");
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	let user_ptr = unsafe { *(pf_handle as *mut *mut u8) };

	if user_ptr.is_null() {
		// If the handle exists but points to NULL, treat as new alloc?
		// Standard behavior usually implies a valid handle has valid data or strict rules.
		// For safety, let's fail.
		log::error!("host_resize_handle: user_ptr is NULL");
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	let header_size = HANDLE_ALIGNMENT;
	let base_ptr = unsafe { user_ptr.sub(header_size) };

	// Verify magic number
	let magic = unsafe { *(base_ptr as *mut u64) };
	if magic != HANDLE_MAGIC {
		log::error!(
			"host_resize_handle: INVALID HANDLE! magic=0x{:x} (expected 0x{:x}), pf_handle={:p}, user_ptr={:p}",
			magic,
			HANDLE_MAGIC,
			pf_handle,
			user_ptr
		);
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	let old_size = unsafe { *(base_ptr.add(8) as *mut usize) };

	log::debug!(
		"host_resize_handle: pf_handle={:p}, user_ptr={:p}, old_size={}",
		pf_handle,
		user_ptr,
		old_size
	);

	let new_size = match usize::try_from(new_sizeL) {
		Ok(value) => value,
		Err(_) => {
			log::error!("host_resize_handle: invalid negative new_size {}", new_sizeL);
			return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
		}
	};

	// Sanity check
	const MAX_REASONABLE_SIZE: usize = 1_000_000_000; // 1GB
	if new_size > MAX_REASONABLE_SIZE {
		log::error!(
			"host_resize_handle: UNREASONABLE SIZE REQUESTED! new_size={} (0x{:x}), old_size={}",
			new_size,
			new_size,
			old_size
		);
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}

	let old_total = match header_size.checked_add(old_size) {
		Some(value) => value,
		None => {
			log::error!("host_resize_handle: old size overflow");
			return PF_Err_INTERNAL_STRUCT_DAMAGED as PF_Err;
		}
	};
	let new_total = match header_size.checked_add(new_size) {
		Some(value) => value,
		None => {
			log::error!("host_resize_handle: new size overflow");
			return PF_Err_OUT_OF_MEMORY as PF_Err;
		}
	};

	// Realloc
	// We trusted layout was created with HANDLE_ALIGNMENT
	if let Ok(old_layout) = Layout::from_size_align(old_total, HANDLE_ALIGNMENT) {
		let new_ptr = unsafe { realloc(base_ptr, old_layout, new_total) };

		if new_ptr.is_null() {
			log::error!("host_resize_handle: realloc failed");
			return PF_Err_OUT_OF_MEMORY as PF_Err;
		}

		// Update size in prefix
		unsafe { *(new_ptr as *mut u64) = HANDLE_MAGIC };
		unsafe { *(new_ptr.add(8) as *mut usize) = new_size };

		// Update handle to point to new user data
		let new_user_ptr = unsafe { new_ptr.add(header_size) };

		// Update handle to point to the new user pointer
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

/// Builds the `PF_HandleSuite1` vtable of working implementations.
///
/// `const` so it can initialize the shared [`SUITE_CONTAINER`](crate::suites::SUITE_CONTAINER)
/// static: the suite is a stateless table of function pointers, so a single
/// process-wide instance is handed to every plugin.
pub const fn create_handle_suite_1() -> PF_HandleSuite1 {
	PF_HandleSuite1 {
		host_new_handle: Some(host_new_handle_impl),
		host_lock_handle: Some(host_lock_handle_impl),
		host_unlock_handle: Some(host_unlock_handle_impl),
		host_dispose_handle: Some(host_dispose_handle_impl),
		host_get_handle_size: Some(host_get_handle_size_impl),
		host_resize_handle: Some(host_resize_handle_impl),
	}
}
