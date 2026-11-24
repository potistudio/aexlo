//! The project timestamp that `AEGP_GetCurrentTimestamp` returns.
//!
//! After Effects increases it whenever something in the project that affects
//! rendering changes. aexlo keeps one counter for the whole process and
//! increases it whenever an instance's parameters, layers, masks, text,
//! project or output size change, and after each command in which a plugin may
//! change its own values. Moving the current time does not increase it.
//!
//! One counter for every instance means that a change to one instance also
//! moves the timestamp the others see. A plugin that compares timestamps then
//! renders again when it did not need to, never less often than it should.

use std::sync::atomic::{AtomicU32, Ordering};

static STAMP: AtomicU32 = AtomicU32::new(1);

/// Record a change that affects rendering.
pub(crate) fn touch() {
	STAMP.fetch_add(1, Ordering::Relaxed);
	#[cfg(test)]
	TOUCHES.with(|count| count.set(count.get() + 1));
}

/// The current timestamp, as the four bytes of `AEGP_TimeStamp`.
pub(crate) fn current() -> [u8; 4] {
	STAMP.load(Ordering::Relaxed).to_le_bytes()
}

// Tests run in parallel and share `STAMP`, so they count the changes their own
// thread made instead.
#[cfg(test)]
thread_local! {
	static TOUCHES: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// How many times this thread has called [`touch`].
#[cfg(test)]
pub(crate) fn touches_on_this_thread() -> u32 {
	TOUCHES.with(std::cell::Cell::get)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn touch_moves_the_timestamp_forward() {
		let before = u32::from_le_bytes(current());
		touch();
		assert!(u32::from_le_bytes(current()) > before);
	}
}
