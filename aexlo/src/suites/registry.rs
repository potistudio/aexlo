//! Suite Registry for managing Suite lifecycle with reference counting

use after_effects_sys::*;
use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::{
	RwLock,
	atomic::{AtomicUsize, Ordering},
};
use std::mem;

/// Sendable wrapper for raw pointers (Suite pointers)
#[derive(Debug)]
struct SendablePtr(usize);

unsafe impl Send for SendablePtr {}
unsafe impl Sync for SendablePtr {}

impl SendablePtr {
	unsafe fn as_ptr<T>(&self) -> *const T {
		self.0 as *const T
	}

	unsafe fn from_ptr<T>(ptr: *const T) -> Self {
		Self(ptr as usize)
	}
}

/// Suite registry entry with reference counting
struct SuiteEntry {
	// Raw pointer to Suite (owned by the registry, will be Box::from_raw on drop)
	suite_ptr: SendablePtr,
	// Size of the Suite type (needed for correct deallocation)
	suite_size: usize,
	ref_count: AtomicUsize,
}

/// Global Suite registry with lazy initialization
pub static SUITE_REGISTRY: OnceLock<RwLock<HashMap<(String, i32), SuiteEntry>>> = OnceLock::new();

/// Acquire a Suite, creating it if necessary (lazy initialization)
///
/// # Safety
/// The returned pointer is valid as long as registry entry exists.
/// The creator function must return a valid Box containing a Suite.
#[allow(non_snake_case)]
pub fn acquire<T>(
	name: &str,
	version: i32,
	creator: fn() -> Box<T>,
) -> Result<*const (), PF_Err> {
	let key = (name.to_string(), version);
	let registry = SUITE_REGISTRY.get_or_init(|| RwLock::new(HashMap::new()));

	let mut guard = registry.write().expect("SuiteRegistry lock poisoned");

	// Check if Suite already exists (while holding write lock to prevent TOCTOU race)
	if let Some(entry) = guard.get_mut(&key) {
		// Increment ref count for existing entry
		entry.ref_count.fetch_add(1, Ordering::SeqCst);
		return Ok(unsafe { entry.suite_ptr.as_ptr() });
	}

	// Suite doesn't exist - create new one
	// Convert Box to raw pointer and store it
	let suite_ptr: *const T = Box::into_raw(creator());
	let suite_ptr_sendable = unsafe { SendablePtr::from_ptr(suite_ptr) };

	let entry = SuiteEntry {
		suite_ptr: suite_ptr_sendable,
		suite_size: mem::size_of::<T>(),
		ref_count: AtomicUsize::new(1),
	};

	// Insert new entry (still holding write lock, ensuring no duplicate)
	guard.insert(key, entry);

	Ok(suite_ptr as *const ())
}

/// Release a Suite (decrement reference count)
///
/// When reference count reaches 0, the Suite is removed from the registry
/// and the memory is freed using Box::from_raw with the correct size.
#[allow(non_snake_case)]
pub fn release(name: &str, version: i32) -> PF_Err {
	let key = (name.to_string(), version);
	let registry = SUITE_REGISTRY.get_or_init(|| RwLock::new(HashMap::new()));

	let mut guard = registry.write().expect("SuiteRegistry lock poisoned");
	if let Some(entry) = guard.get_mut(&key) {
		// Atomically decrement ref_count only if it's greater than 0
		let result = entry
			.ref_count
			.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |current| {
				if current > 0 {
					Some(current - 1)
				} else {
					None // Already 0, don't decrement (would underflow)
				}
			});

		match result {
			Ok(previous) => {
				// fetch_update returns the value BEFORE the update
				// If previous was 1, after decrement it becomes 0
				if previous == 1 {
					// Reference count reached 0, remove from registry
					// Convert the raw pointer back to Box and drop it to free memory
					// SAFETY: We own this pointer from when it was created with Box::into_raw
					// and we're the only one who will call drop on it
					let suite_ptr = unsafe { entry.suite_ptr.as_ptr::<u8>() };
					guard.remove(&key);
					let _ = unsafe { Box::from_raw(suite_ptr as *mut u8) };
				}
			}
			Err(_) => {
				// Ref count was already 0 - invalid operation
				log::warn!(
					"Attempted to release a suite with ref_count already 0: {} v{}",
					name,
					version
				);
				return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
			}
		}
	}
	PF_Err_NONE as PF_Err
}
