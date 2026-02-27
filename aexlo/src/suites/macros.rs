//! Macros for generating Suite stub functions

use after_effects_sys::*;

/// Simple logging stub generator
///
/// Creates an FFI-compatible stub function that logs a warning and returns PF_Err_NONE.
///
/// # Example
/// ```ignore
/// stub_log!(get_clip_name_stub,
///     _effect_ref: PF_ProgPtr,
///     _get_master_clip_name: A_Boolean,
///     out_sdk_string: *mut PrSDKString
/// );
/// ```
#[macro_export]
macro_rules! stub_log {
    ( $fn_name:ident, $( $arg_name:ident : $arg_ty:ty ),* $(,)? ) => {
        #[allow(non_snake_case)]
        #[allow(unused_variables)]
        pub unsafe extern "C" fn $fn_name( $( $arg_name : $arg_ty ),* ) -> PF_Err {
            log::warn!("STUB: {} called", stringify!($fn_name));
            PF_Err_NONE as PF_Err
        }
    };
}

// Make the macro available to other modules
pub(crate) use stub_log;
