//! Macros for generating Suite stub functions

/// Simple logging stub generator
///
/// Creates an FFI-compatible stub that reports an unavailable callback.
///
/// # Example
/// ```ignore
/// stub_log!(get_clip_name_stub,
///     _effect_ref: PF_ProgPtr,
///     _get_master_clip_name: A_Boolean,
///     out_sdk_string: *mut PrSDKString
/// );
/// ```
macro_rules! stub_log {
    ( $fn_name:ident, $( $arg_name:ident : $arg_ty:ty ),* $(,)? ) => {
        #[allow(non_snake_case)]
        #[allow(unused_variables)]
        pub unsafe extern "C" fn $fn_name( $( $arg_name : $arg_ty ),* ) -> PF_Err {
			crate::core::ffi::guard(stringify!($fn_name), || {
				log::warn!("Unsupported callback: {}", stringify!($fn_name));
				crate::sys::PF_Err_INVALID_CALLBACK
			})
        }
    };
}

// Make the macro available to other modules
pub(crate) use stub_log;
