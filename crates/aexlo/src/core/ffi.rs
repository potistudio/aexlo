//! Keeping Rust panics from crossing the C ABI.
//!
//! A panic that reaches an `extern "C"` boundary aborts the whole process, and
//! aexlo runs inside its embedder's process. Every suite callback that returns
//! an error code runs its body through [`guard`], so a bug in aexlo surfaces to
//! the plugin as an error instead of killing the host application.

/// Run `body`, reporting a panic as `PF_Err_INTERNAL_STRUCT_DAMAGED`.
///
/// `PF_Err`, `A_Err` and `SPErr` are all `i32`, so one guard serves every
/// suite. `name` is the callback, for the log.
#[inline]
pub(crate) fn guard(name: &'static str, body: impl FnOnce() -> i32) -> i32 {
	std::panic::catch_unwind(std::panic::AssertUnwindSafe(body)).unwrap_or_else(|_| {
		log::error!("{name} panicked; reporting PF_Err_INTERNAL_STRUCT_DAMAGED to the plugin");
		crate::sys::PF_Err_INTERNAL_STRUCT_DAMAGED
	})
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn passes_results_through() {
		assert_eq!(guard("ok", || 7), 7);
	}

	#[test]
	fn reports_a_panic_as_an_error() {
		let previous = std::panic::take_hook();
		std::panic::set_hook(Box::new(|_| {}));
		let err = guard("boom", || panic!("boom"));
		std::panic::set_hook(previous);
		assert_eq!(err, crate::sys::PF_Err_INTERNAL_STRUCT_DAMAGED);
	}
}
