//! How the `aexlo` process ends.
//!
//! Dropping a `PluginInstance` sends the setdown commands and frees the
//! host's reference to the plugin, but a plugin with background threads stays
//! loaded: each thread holds a reference of its own. A normal Windows exit
//! then stops those threads where they stand and runs the DLL's detach, which
//! waits for them for ever (Red Giant's job manager) or calls into a driver
//! that is already gone (wgpu on Vulkan). The CLI has nothing left to do by
//! then, so it flushes its output and ends without running those detaches.
//!
//! This is the process's decision, so it is not in the `aexlo` library.

/// End the process with `code`, once every plugin has been torn down.
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
