//! The `stub_log!` macro, which generates the logging stub functions that stand in for unimplemented suite callbacks.

/// Simple logging stub generator
///
/// Creates an FFI-compatible stub function that reports itself as an
/// [`UnsupportedKind::Stub`](crate::UnsupportedKind::Stub) (logged as a
/// warning, and handed to the observer) and returns PF_Err_NONE.
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
            $crate::observe::unsupported(
                $crate::observe::UnsupportedKind::Stub,
                stringify!($fn_name).trim_end_matches("_stub"),
                "",
            );
            PF_Err_NONE
        }
    };
}

// Make the macro available to other modules
pub(crate) use stub_log;
