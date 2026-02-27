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

/// Suite registry for lazy initialization and reference counting
pub struct SuiteRegistry {
	inner: OnceLock<RwLock<HashMap<(String, i32), SuiteEntry>>>,
}

impl SuiteRegistry {
	/// Create a new SuiteRegistry
	pub fn new() -> Self {
		Self {
			inner: OnceLock::new(RwLock::new(HashMap::new())),
		}
	}

	/// Acquire a Suite, creating it if necessary (lazy initialization)
	///
	/// # Safety
	/// The returned pointer is valid as long as the Arc in the registry is alive.
	/// The creator function must return a valid Box containing a Suite.
	#[allow(non_snake_case)]
	pub fn acquire(
		&self,
		name: &str,
		version: i32,
		creator: fn() -> Box<()>,
	) -> Result<*const (), PF_Err> {
		let key = (name.to_string(), version);

		// Check if Suite already exists
		{
			let guard = self.inner.read().expect("SuiteRegistry lock poisoned");
			if let Some(entry) = guard.get(&key) {
				entry.ref_count.fetch_add(1, Ordering::SeqCst);
				let ptr = Arc::as_ptr(&entry.suite);
				return Ok(ptr);
			}
		}

		// Create new Suite
		// We convert Box to Arc here, so Arc owns the Suite
		let suite = Arc::new(*creator());
		let ptr = Arc::as_ptr(&suite);

		let entry = SuiteEntry {
			suite,
			ref_count: AtomicUsize::new(1),
		};

		{
			let mut guard = self.inner.write().expect("SuiteRegistry lock poisoned");
			guard.insert(key, entry);
		}

		Ok(ptr)
	}

	/// Release a Suite (decrement reference count)
	///
	/// When reference count reaches 0, the Suite is removed from the registry
	/// and the Arc is dropped, freeing the memory.
	#[allow(non_snake_case)]
	pub fn release(&self, name: &str, version: i32) -> PF_Err {
		let key = (name.to_string(), version);

		let mut guard = self.inner.write().expect("SuiteRegistry lock poisoned");
		if let Some(entry) = guard.get_mut(&key) {
			let prev_count = entry.ref_count.fetch_sub(1, Ordering::SeqCst);

			if prev_count <= 1 {
				// Reference count reached 0, remove from registry
				// The Arc in SuiteEntry will be dropped, freeing memory
				guard.remove(&key);
			}
		}
		PF_Err_NONE as PF_Err
	}
}

	/// Global Suite registry
pub static SUITE_REGISTRY: SuiteRegistry = SuiteRegistry::new();
