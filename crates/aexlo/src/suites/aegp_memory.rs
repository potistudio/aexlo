//! `AEGP Memory Suite` (wire version 1).
//!
//! Looks allocates the scratch buffers for spatial tools (Edge Softness,
//! Swing-Tilt) through this suite. A failed acquire skips those tools and
//! leaves the colour-only tools running.
//!
//! `AEGP_MemHandle` is a pointer to the data pointer, as `PF_Handle` is. Lock
//! returns that data pointer; resize updates it, so a lock must not be held
//! across a resize.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use after_effects_sys::{
	A_Boolean, A_Err, A_char, A_long, AEGP_MemHandle, AEGP_MemSize, AEGP_MemorySuite1, AEGP_PluginID,
	PF_Err_BAD_CALLBACK_PARAM, PF_Err_NONE, PF_Err_OUT_OF_MEMORY,
};

struct Block {
	bytes: Vec<u8>,
}

/// Live blocks keyed by the handle (`*mut *mut c_void`).
static LIVE: LazyLock<Mutex<HashMap<usize, Block>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

fn live() -> std::sync::MutexGuard<'static, HashMap<usize, Block>> {
	LIVE.lock().unwrap_or_else(|err| err.into_inner())
}

/// A memory-suite handle owning `bytes`. The plugin frees it with
/// `AEGP_FreeMemHandle`.
pub(crate) fn handle_from_bytes(bytes: Vec<u8>) -> AEGP_MemHandle {
	publish(Block { bytes })
}

fn publish(block: Block) -> AEGP_MemHandle {
	let data = block.bytes.as_ptr() as *mut std::ffi::c_void;
	let handle = Box::into_raw(Box::new(data)) as AEGP_MemHandle;
	live().insert(handle as usize, block);
	handle
}

unsafe extern "C" fn new_mem(
	_plugin_id: AEGP_PluginID,
	_what: *const A_char,
	size: AEGP_MemSize,
	_flags: A_long,
	mem_out: *mut AEGP_MemHandle,
) -> A_Err {
	crate::core::ffi::guard("new_mem", || {
		let Some(out) = (unsafe { mem_out.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM as A_Err;
		};
		let Ok(size) = usize::try_from(size) else {
			return PF_Err_OUT_OF_MEMORY as A_Err;
		};
		// Uninitialised memory is not observable, so `CLEAR` and the default both
		// hand back zeroes.
		*out = publish(Block { bytes: vec![0u8; size] });
		PF_Err_NONE as A_Err
	})
}

unsafe extern "C" fn free_mem(handle: AEGP_MemHandle) -> A_Err {
	crate::core::ffi::guard("free_mem", || {
		if live().remove(&(handle as usize)).is_none() {
			return PF_Err_BAD_CALLBACK_PARAM as A_Err;
		}
		drop(unsafe { Box::from_raw(handle as *mut *mut std::ffi::c_void) });
		PF_Err_NONE as A_Err
	})
}

unsafe extern "C" fn lock_mem(handle: AEGP_MemHandle, ptr_to_ptr: *mut *mut std::ffi::c_void) -> A_Err {
	crate::core::ffi::guard("lock_mem", || {
		let Some(out) = (unsafe { ptr_to_ptr.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM as A_Err;
		};
		let mut live = live();
		let Some(block) = live.get_mut(&(handle as usize)) else {
			return PF_Err_BAD_CALLBACK_PARAM as A_Err;
		};
		let data = block.bytes.as_mut_ptr().cast();
		unsafe { *(handle as *mut *mut std::ffi::c_void) = data };
		*out = data;
		PF_Err_NONE as A_Err
	})
}

unsafe extern "C" fn unlock_mem(handle: AEGP_MemHandle) -> A_Err {
	crate::core::ffi::guard("unlock_mem", || {
		if !live().contains_key(&(handle as usize)) {
			return PF_Err_BAD_CALLBACK_PARAM as A_Err;
		}
		PF_Err_NONE as A_Err
	})
}

unsafe extern "C" fn get_size(handle: AEGP_MemHandle, size_out: *mut AEGP_MemSize) -> A_Err {
	crate::core::ffi::guard("get_size", || {
		let live = live();
		let (Some(block), Some(out)) = (live.get(&(handle as usize)), unsafe { size_out.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM as A_Err;
		};
		*out = block.bytes.len() as AEGP_MemSize;
		PF_Err_NONE as A_Err
	})
}

unsafe extern "C" fn resize(_what: *const A_char, new_size: AEGP_MemSize, handle: AEGP_MemHandle) -> A_Err {
	crate::core::ffi::guard("resize", || {
		let Ok(new_size) = usize::try_from(new_size) else {
			return PF_Err_OUT_OF_MEMORY as A_Err;
		};
		let mut live = live();
		let Some(block) = live.get_mut(&(handle as usize)) else {
			return PF_Err_BAD_CALLBACK_PARAM as A_Err;
		};
		block.bytes.resize(new_size, 0);
		let data = block.bytes.as_mut_ptr().cast();
		unsafe { *(handle as *mut *mut std::ffi::c_void) = data };
		PF_Err_NONE as A_Err
	})
}

unsafe extern "C" fn set_reporting(_turn_on: A_Boolean) -> A_Err {
	crate::core::ffi::guard("set_reporting", || PF_Err_NONE as A_Err)
}

unsafe extern "C" fn get_stats(_plugin_id: AEGP_PluginID, count_out: *mut A_long, size_out: *mut A_long) -> A_Err {
	crate::core::ffi::guard("get_stats", || {
		let live = live();
		let bytes = live
			.values()
			.fold(0usize, |sum, block| sum.saturating_add(block.bytes.len()));
		if let Some(count) = unsafe { count_out.as_mut() } {
			*count = live.len() as A_long;
		}
		if let Some(size) = unsafe { size_out.as_mut() } {
			*size = bytes.min(i32::MAX as usize) as A_long;
		}
		PF_Err_NONE as A_Err
	})
}

pub const fn suite_1() -> AEGP_MemorySuite1 {
	AEGP_MemorySuite1 {
		AEGP_NewMemHandle: Some(new_mem),
		AEGP_FreeMemHandle: Some(free_mem),
		AEGP_LockMemHandle: Some(lock_mem),
		AEGP_UnlockMemHandle: Some(unlock_mem),
		AEGP_GetMemHandleSize: Some(get_size),
		AEGP_ResizeMemHandle: Some(resize),
		AEGP_SetMemReportingOn: Some(set_reporting),
		AEGP_GetMemStats: Some(get_stats),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn lock_sees_bytes_written_through_the_pointer() {
		let suite = suite_1();
		let mut handle: AEGP_MemHandle = std::ptr::null_mut();
		unsafe {
			assert_eq!(
				suite.AEGP_NewMemHandle.unwrap()(0, std::ptr::null(), 4, 1, &mut handle),
				PF_Err_NONE as A_Err
			);
			let mut ptr = std::ptr::null_mut();
			assert_eq!(
				suite.AEGP_LockMemHandle.unwrap()(handle, &mut ptr),
				PF_Err_NONE as A_Err
			);
			assert_eq!(*handle as usize, ptr as usize);
			std::ptr::write_bytes(ptr.cast::<u8>(), 0x5a, 4);
			suite.AEGP_UnlockMemHandle.unwrap()(handle);
			assert_eq!(suite.AEGP_FreeMemHandle.unwrap()(handle), PF_Err_NONE as A_Err);
			assert_ne!(suite.AEGP_FreeMemHandle.unwrap()(handle), PF_Err_NONE as A_Err);
		}
	}
}
