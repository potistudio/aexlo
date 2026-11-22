//! The plugin thread's Win32 message loop. A window a plugin opens (an
//! Options dialog that is not modal) belongs to the thread that opened it,
//! and stays responsive only while that thread dispatches its messages.

/// Dispatch every message pending for this thread's windows.
pub fn pump_messages() {
	while peek_message() {}
}

/// Dispatch one pending message for this thread's windows; whether there was
/// one.
fn peek_message() -> bool {
	#[repr(C)]
	struct Msg {
		hwnd: *mut std::ffi::c_void,
		message: u32,
		_pad: u32,
		wparam: usize,
		lparam: isize,
		time: u32,
		_pad2: u32,
		x: i32,
		y: i32,
		lprivate: u32,
		_pad3: u32,
	}
	#[link(name = "user32")]
	unsafe extern "system" {
		fn PeekMessageW(msg: *mut Msg, hwnd: *mut std::ffi::c_void, min: u32, max: u32, remove: u32) -> i32;
		fn TranslateMessage(msg: *const Msg) -> i32;
		fn DispatchMessageW(msg: *const Msg) -> isize;
	}
	let mut msg = std::mem::MaybeUninit::<Msg>::uninit();
	// SAFETY: `msg` is a valid out-pointer; a null hwnd asks for any of this
	// thread's messages, and 1 is PM_REMOVE.
	let pending = unsafe { PeekMessageW(msg.as_mut_ptr(), std::ptr::null_mut(), 0, 0, 1) } != 0;
	if pending {
		// SAFETY: `PeekMessageW` filled `msg` when it returned nonzero.
		let msg = unsafe { msg.assume_init() };
		// SAFETY: `msg` is the message `PeekMessageW` just removed.
		unsafe {
			TranslateMessage(&msg);
			DispatchMessageW(&msg);
		}
	}
	pending
}
