//! Cancelling a render through an injected `AppHost`. Kept in its own test
//! binary because the host is process-wide and set once.

use aexlo::{AppHost, Host};
use std::sync::atomic::{AtomicUsize, Ordering};

static ABORT_POLLS: AtomicUsize = AtomicUsize::new(0);

struct CancellingHost;

impl AppHost for CancellingHost {
	fn abort_requested(&self, _effect: usize) -> bool {
		ABORT_POLLS.fetch_add(1, Ordering::Relaxed);
		true
	}
}

/// DeepGlow2 polls `PF_ABORT` while rendering; a host reporting a cancel stops
/// the render with `PF_Interrupt_CANCEL`, which `render_frame` reports instead
/// of retrying on another render path.
#[test]
fn host_cancel_stops_the_render() {
	let Some(path) = test_e2e::fixture("DeepGlow2") else {
		eprintln!("skipping: fixture 'DeepGlow2' not present locally");
		return;
	};
	let host = Host::install(CancellingHost).expect("host already installed");
	let mut instance = host.try_load(&path).expect("failed to load plugin");

	let err = instance.render_frame().expect_err("a cancelled render must fail");
	assert!(err.is_cancelled(), "expected a cancellation, got {err:?}");
	assert!(ABORT_POLLS.load(Ordering::Relaxed) > 0);
}
