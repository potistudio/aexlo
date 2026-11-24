//! Deterministic 128-bit hashes for plugin-defined cache keys.
use crate::sys::*;
use sha2::{Digest, Sha256};
use std::ffi::c_void;

unsafe fn hash(size: A_u_longlong, data: *const c_void, out: *mut AEGP_GUID, mix: bool) -> A_Err {
	if out.is_null() || (size != 0 && data.is_null()) || size > isize::MAX as u64 {
		return PF_Err_BAD_CALLBACK_PARAM;
	}
	let mut digest = Sha256::new();
	if mix {
		for word in unsafe { (*out).bytes } {
			digest.update(word.to_le_bytes());
		}
	}
	if size != 0 {
		// SDK callers supply a readable buffer of the stated size.
		digest.update(unsafe { std::slice::from_raw_parts(data.cast::<u8>(), size as usize) });
	}
	let bytes = digest.finalize();
	let mut words = [0; 4];
	let (chunks, _) = bytes[..16].as_chunks::<4>();
	for (word, chunk) in words.iter_mut().zip(chunks) {
		*word = i32::from_le_bytes(*chunk);
	}
	unsafe {
		(*out).bytes = words;
	}
	PF_Err_NONE
}
unsafe extern "C" fn create(size: A_u_longlong, data: *const c_void, out: *mut AEGP_GUID) -> A_Err {
	crate::core::ffi::guard("create_hash_from_ptr", || unsafe { hash(size, data, out, false) })
}
unsafe extern "C" fn mix(size: A_u_longlong, data: *const c_void, out: *mut AEGP_GUID) -> A_Err {
	crate::core::ffi::guard("hash_mix_in_ptr", || unsafe { hash(size, data, out, true) })
}
pub(crate) static SUITE: AEGP_HashSuite1 = AEGP_HashSuite1 {
	AEGP_CreateHashFromPtr: Some(create),
	AEGP_HashMixInPtr: Some(mix),
};

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn stable_hash_and_mix_depend_on_bytes() {
		let mut first = AEGP_GUID { bytes: [0; 4] };
		let mut second = first;
		let data = b"abc";
		unsafe {
			assert_eq!(create(3, data.as_ptr().cast(), &mut first), 0);
			assert_eq!(create(3, data.as_ptr().cast(), &mut second), 0);
			assert_eq!(first.bytes, second.bytes);
			assert_eq!(mix(1, data.as_ptr().cast(), &mut second), 0);
			assert_ne!(first.bytes, second.bytes);
			assert_ne!(create(1, std::ptr::null(), &mut first), 0);
		}
	}
}
