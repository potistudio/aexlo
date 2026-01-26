//! Parameter Manager
//!
//! Stores parameters registered by plugins via `add_param`.
//! Emulates the C++ `ParamManager` class from aexlo.js.

use after_effects_sys::*;
use std::collections::HashMap;
use std::sync::Mutex;

/// Wrapper for parameter storage that implements Send/Sync.
/// This is safe because we only access this from a controlled context.
struct ParamStorage {
	params: HashMap<usize, Vec<PF_ParamDef>>,
}

// SAFETY: We ensure exclusive access via Mutex, and the raw pointers within
// PF_ParamDef are only accessed on the same thread that created them.
unsafe impl Send for ParamStorage {}
unsafe impl Sync for ParamStorage {}

/// Global parameter storage, keyed by effect_ref (as usize for hashing).
static PARAMS: Mutex<Option<ParamStorage>> = Mutex::new(None);

/// Initializes the parameter manager (called once at startup).
pub fn init() {
	let mut params = PARAMS.lock().unwrap();
	if params.is_none() {
		*params = Some(ParamStorage {
			params: HashMap::new(),
		});
	}
}

/// Adds a parameter definition for the given effect_ref.
pub fn add_param(effect_ref: PF_ProgPtr, param: PF_ParamDef) {
	let mut guard = PARAMS.lock().unwrap();
	let storage = guard.get_or_insert_with(|| ParamStorage {
		params: HashMap::new(),
	});

	let key = effect_ref as usize;
	storage
		.params
		.entry(key)
		.or_insert_with(Vec::new)
		.push(param);

	log::debug!(
		"ParamManager: Added param for effect_ref={:#x}, total={}",
		key,
		storage.params.get(&key).map(|v| v.len()).unwrap_or(0)
	);
}

/// Gets all parameters for the given effect_ref.
pub fn get_params(effect_ref: PF_ProgPtr) -> Vec<PF_ParamDef> {
	let guard = PARAMS.lock().unwrap();
	if let Some(storage) = guard.as_ref() {
		storage
			.params
			.get(&(effect_ref as usize))
			.cloned()
			.unwrap_or_default()
	} else {
		Vec::new()
	}
}

/// Gets the number of parameters for the given effect_ref.
pub fn get_params_count(effect_ref: PF_ProgPtr) -> usize {
	let guard = PARAMS.lock().unwrap();
	if let Some(storage) = guard.as_ref() {
		storage
			.params
			.get(&(effect_ref as usize))
			.map(|v| v.len())
			.unwrap_or(0)
	} else {
		0
	}
}

/// Clears all parameters for the given effect_ref.
pub fn clear_params(effect_ref: PF_ProgPtr) {
	let mut guard = PARAMS.lock().unwrap();
	if let Some(storage) = guard.as_mut() {
		storage.params.remove(&(effect_ref as usize));
	}
}

/// Clears all parameters.
pub fn clear_all() {
	let mut guard = PARAMS.lock().unwrap();
	if let Some(storage) = guard.as_mut() {
		storage.params.clear();
	}
}
