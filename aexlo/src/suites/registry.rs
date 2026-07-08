//! Suite Registry for managing Suite lifecycle with reference counting.
//!
//! # Ownership model
//!
//! The registry is **process-global** ([`SUITE_REGISTRY`]): every
//! [`PluginInstance`](crate::PluginInstance) in the process shares
//! the same suite instances. This is sound because the suites we hand out are
//! stateless vtables (tables of `extern "C"` function pointers) — any mutable
//! state lives behind the plugin-provided pointers those callbacks receive, not
//! in the suite struct itself — so sharing one instance across plugins and
//! threads is safe.
//!
//! Lifetime is driven entirely by the plugin's own acquire/release calls: an
//! entry is created on first [`acquire`] and freed only when its ref count
//! returns to 0 via [`release`]. A plugin that acquires a suite but never
//! releases it (common, and permitted by the SDK contract) simply keeps that
//! suite's `Box` alive until the process exits. There is deliberately no
//! per-instance teardown that force-releases suites — see the decision recorded
//! in the git history if per-instance lifetimes ever become necessary (e.g. a
//! long-lived host loading and unloading many distinct plugins).

use after_effects_sys::*;
use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::RwLock;

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

/// Suite registry entry with reference counting.
///
/// `ref_count` is a plain `usize` rather than an atomic: every access happens
/// while holding the registry's `RwLock` write guard, which already provides
/// mutual exclusion, so atomics would only add redundant fences.
struct SuiteEntry {
	// Raw pointer to Suite (owned by the registry, will be Box::from_raw on drop)
	suite_ptr: SendablePtr,
	drop_fn: unsafe fn(SendablePtr),
	ref_count: usize,
}

/// Global Suite registry with lazy initialization
static SUITE_REGISTRY: OnceLock<RwLock<HashMap<(String, i32), SuiteEntry>>> = OnceLock::new();

unsafe fn drop_suite<T>(suite_ptr: SendablePtr) {
	let typed = unsafe { suite_ptr.as_ptr::<T>() as *mut T };
	let _ = unsafe { Box::from_raw(typed) };
}

/// Acquire a Suite, creating it if necessary (lazy initialization)
///
/// # Safety
/// The returned pointer is valid as long as registry entry exists.
/// The creator function must return a valid Box containing a Suite.
pub fn acquire<T>(name: &str, version: i32, creator: fn() -> Box<T>) -> Result<*const (), PF_Err> {
	let key = (name.to_string(), version);
	let registry = SUITE_REGISTRY.get_or_init(|| RwLock::new(HashMap::new()));

	let mut guard = registry.write().expect("SuiteRegistry lock poisoned");

	// Check if Suite already exists (while holding write lock to prevent TOCTOU race)
	if let Some(entry) = guard.get_mut(&key) {
		// Increment ref count for existing entry
		entry.ref_count += 1;
		return Ok(unsafe { entry.suite_ptr.as_ptr() });
	}

	// Suite doesn't exist - create new one
	// Convert Box to raw pointer and store it
	let suite_ptr: *const T = Box::into_raw(creator());
	let suite_ptr_sendable = unsafe { SendablePtr::from_ptr(suite_ptr) };

	let entry = SuiteEntry {
		suite_ptr: suite_ptr_sendable,
		drop_fn: drop_suite::<T>,
		ref_count: 1,
	};

	// Insert new entry (still holding write lock, ensuring no duplicate)
	guard.insert(key, entry);

	Ok(suite_ptr as *const ())
}

/// Release a Suite (decrement reference count)
///
/// When reference count reaches 0, the Suite is removed from the registry
/// and the memory is freed using Box::from_raw with the correct size.
pub fn release(name: &str, version: i32) -> PF_Err {
	let key = (name.to_string(), version);
	let registry = SUITE_REGISTRY.get_or_init(|| RwLock::new(HashMap::new()));

	let mut guard = registry.write().expect("SuiteRegistry lock poisoned");
	let mut should_remove = false;
	if let Some(entry) = guard.get_mut(&key) {
		// Decrement ref_count, guarding against underflow on a stray release.
		if entry.ref_count == 0 {
			log::warn!(
				"Attempted to release a suite with ref_count already 0: {} v{}",
				name,
				version
			);
			return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
		}

		entry.ref_count -= 1;
		if entry.ref_count == 0 {
			should_remove = true;
		}
	}

	if should_remove && let Some(entry) = guard.remove(&key) {
		unsafe { (entry.drop_fn)(entry.suite_ptr) };
	}

	PF_Err_NONE as PF_Err
}
