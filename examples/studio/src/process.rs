//! How the studio's process ends.
//!
//! The render thread has torn the plugins down by now (`Engine::shutdown`),
//! but a plugin with background threads stays loaded, and a normal Windows
//! exit would run its DLL detach after killing those threads: that hangs (Red
//! Giant's job manager) or crashes (wgpu on Vulkan). So flush and end without
//! the detach, as the `aexlo` command does.

/// End the process with `code`, once the render thread has been shut down.
pub fn exit(code: u8) -> ! {
	use std::io::Write;
	let _ = std::io::stdout().flush();
	let _ = std::io::stderr().flush();
	end(code)
}

#[cfg(windows)]
fn end(code: u8) -> ! {
	use std::ffi::{c_int, c_void};

	#[link(name = "kernel32")]
	unsafe extern "system" {
		fn GetCurrentProcess() -> *mut c_void;
		fn TerminateProcess(process: *mut c_void, code: u32) -> c_int;
	}
	unsafe extern "C" {
		fn fflush(stream: *mut c_void) -> c_int;
	}

	// SAFETY: `fflush(NULL)` flushes every output stream of the C runtime this
	// process shares with plugins; terminating takes the current process's
	// pseudo-handle and does not return.
	unsafe {
		fflush(std::ptr::null_mut());
		TerminateProcess(GetCurrentProcess(), u32::from(code));
	}
	// Only reached if Windows refused to terminate.
	std::process::exit(i32::from(code))
}

#[cfg(not(windows))]
fn end(code: u8) -> ! {
	std::process::exit(i32::from(code))
}
