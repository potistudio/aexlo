//! Suite Registry for managing Suite lifecycle with reference counting

use std::collections::HashMap;
use std::sync::{Arc, RwLock, atomic::{AtomicUsize, Ordering}};
use after_effects_sys::*;
use std::sync::OnceLock;

/// Suite registry entry with reference counting
struct SuiteEntry {
	suite: Arc<()>,
	ref_count: AtomicUsize,
}

/// Global Suite registry with lazy initialization
pub static SUITE_REGISTRY: OnceLock<RwLock<HashMap<(String, i32), SuiteEntry>>> = OnceLock::new();

/// Acquire a Suite, creating it if necessary (lazy initialization)
///
/// # Safety
/// The returned pointer is valid as long as the Arc in the registry is alive.
/// The creator function must return a valid Box containing a Suite.
#[allow(non_snake_case)]
pub fn acquire<T>(
	name: &str,
	version: i32,
	creator: fn() -> Box<T>,
) -> Result<*const (), PF_Err> {
	let key = (name.to_string(), version);
	let registry = SUITE_REGISTRY.get_or_init(|| RwLock::new(HashMap::new()));

	// Check if Suite already exists
	{
		let guard = registry.read().expect("SuiteRegistry lock poisoned");
		if let Some(entry) = guard.get(&key) {
			entry.ref_count.fetch_add(1, Ordering::SeqCst);
			let ptr = Arc::as_ptr(&entry.suite);
			return Ok(ptr);
		}
	}

	// Create new Suite
	// We convert Box to Arc here, so Arc owns the Suite
	let suite: Arc<()> = Arc::new(*unsafe { Box::from_raw(Box::into_raw(creator()) as *mut ()) });
	let ptr = Arc::as_ptr(&suite);

	let entry = SuiteEntry {
		suite,
		ref_count: AtomicUsize::new(1),
	};

	{
		let mut guard = registry.write().expect("SuiteRegistry lock poisoned");
		guard.insert(key, entry);
	}

	Ok(ptr)
}

/// Release a Suite (decrement reference count)
///
/// When reference count reaches 0, the Suite is removed from the registry
/// and the Arc is dropped, freeing the memory.
#[allow(non_snake_case)]
pub fn release(name: &str, version: i32) -> PF_Err {
	let key = (name.to_string(), version);
	let registry = SUITE_REGISTRY.get_or_init(|| RwLock::new(HashMap::new()));

	let mut guard = registry.write().expect("SuiteRegistry lock poisoned");
	if let Some(entry) = guard.get_mut(&key) {
		// Atomically decrement ref_count only if it's greater than 0
		let result = entry.ref_count.fetch_update(
			Ordering::SeqCst,
			Ordering::SeqCst,
			|current| {
				if current > 0 {
					Some(current - 1)
				} else {
					None // Already 0, don't decrement (would underflow)
				}
			}
		);

		match result {
			Ok(new_count) => {
				if new_count == 0 {
					// Reference count reached 0, remove from registry
					// The Arc in SuiteEntry will be dropped, freeing memory
					guard.remove(&key);
				}
			}
			Err(_) => {
				// Ref count was already 0 - invalid operation
				log::warn!("Attempted to release a suite with ref_count already 0: {} v{}", name, version);
				return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
			}
		}
	}
	PF_Err_NONE as PF_Err
}
