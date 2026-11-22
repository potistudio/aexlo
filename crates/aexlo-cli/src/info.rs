//! `about`, `params` and `dialog`: ask a plugin about itself.

use aexlo::{ParamValue, PluginInstance};
use anyhow::{Context, Result};

use crate::args::load;

pub const ABOUT_HELP: &str = "\
Usage: aexlo about <plugin>

Send PF_Cmd_ABOUT and print the plugin's text.
";

pub const PARAMS_HELP: &str = "\
Usage: aexlo params <plugin>

List the plugin's parameters: index, name, kind and value, whether it is
hidden, and its UI flags.
";

pub const DIALOG_HELP: &str = "\
Usage: aexlo dialog <plugin>

Send PF_Cmd_DO_DIALOG, what the effect's Options button does, after printing
the plugin's flags and parameters, and keep the process alive while a plugin
window is visible.
";

pub fn about(mut args: impl Iterator<Item = String>) -> Result<()> {
	let plugin = args.next().context("about: missing <plugin>")?;

	let mut instance = load(&plugin)?;

	let text = instance.about().context("plugin rejected PF_Cmd_ABOUT")?;
	println!("{}", text.trim());

	Ok(())
}

pub fn params(mut args: impl Iterator<Item = String>) -> Result<()> {
	let plugin = args.next().context("params: missing <plugin>")?;
	let instance = load(&plugin)?;
	print_params(&instance);
	Ok(())
}

pub fn dialog(mut args: impl Iterator<Item = String>) -> Result<()> {
	let plugin = args.next().context("dialog: missing <plugin>")?;
	let mut instance = load(&plugin)?;
	println!(
		"out_flags={:#x} out_flags2={:#x} options_button={}",
		instance.out_flags(),
		instance.out_flags2(),
		instance.options_button_name().unwrap_or("(unset)")
	);
	print_params(&instance);
	let message = instance.do_dialog().context("plugin rejected PF_Cmd_DO_DIALOG")?;
	if !message.trim().is_empty() {
		println!("{}", message.trim());
	}
	pump_plugin_windows();
	Ok(())
}

fn print_params(instance: &PluginInstance) {
	if instance.param_count() <= 1 {
		println!("(plugin declares no parameters)");
		return;
	}
	for index in 1..instance.param_count() {
		let kind = instance
			.param_kind(index)
			.map(|kind| format!("{kind:?}"))
			.unwrap_or_default();
		let value = instance
			.get_param(index)
			.map(|value| describe(&value))
			.unwrap_or_else(|| kind);
		let hidden = if instance.param_hidden(index) { " hidden" } else { "" };
		let flags = instance.param_ui_flags(index).unwrap_or_default();
		println!(
			"{index:>3}  {:<24}  {value}{hidden}  ui={flags:#x}",
			param_name(instance, index)
		);
	}
}

/// Dispatch this thread's window messages while the plugin still has a window
/// on screen. A modal Options dialog returns from `PF_Cmd_DO_DIALOG` itself.
#[cfg(windows)]
fn pump_plugin_windows() {
	use std::time::Duration;

	while process_has_visible_window() {
		while peek_message() {}
		std::thread::sleep(Duration::from_millis(16));
	}
}

#[cfg(not(windows))]
fn pump_plugin_windows() {}

#[cfg(windows)]
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
	let pending = unsafe { PeekMessageW(msg.as_mut_ptr(), std::ptr::null_mut(), 0, 0, 1) } != 0;
	if pending {
		let msg = unsafe { msg.assume_init() };
		unsafe {
			TranslateMessage(&msg);
			DispatchMessageW(&msg);
		}
	}
	pending
}

#[cfg(windows)]
fn process_has_visible_window() -> bool {
	#[link(name = "user32")]
	unsafe extern "system" {
		fn EnumWindows(cb: unsafe extern "system" fn(*mut std::ffi::c_void, isize) -> i32, lparam: isize) -> i32;
		fn IsWindowVisible(hwnd: *mut std::ffi::c_void) -> i32;
		fn GetWindowThreadProcessId(hwnd: *mut std::ffi::c_void, process: *mut u32) -> u32;
	}
	#[link(name = "kernel32")]
	unsafe extern "system" {
		fn GetCurrentProcessId() -> u32;
	}
	unsafe extern "system" fn visit(hwnd: *mut std::ffi::c_void, lparam: isize) -> i32 {
		let mut pid = 0u32;
		// SAFETY: `EnumWindows` passes a live top-level `hwnd`, and `pid` is a valid out-pointer.
		let ours = unsafe { GetWindowThreadProcessId(hwnd, &mut pid) != 0 && pid == GetCurrentProcessId() };
		// SAFETY: `hwnd` is the live window `EnumWindows` is visiting.
		if ours && unsafe { IsWindowVisible(hwnd) } != 0 {
			// SAFETY: `lparam` is the `&mut found` that `process_has_visible_window` passed to `EnumWindows`, alive for the whole enumeration.
			unsafe { *(lparam as *mut bool) = true };
			return 0;
		}
		1
	}
	let mut found = false;
	unsafe { EnumWindows(visit, &mut found as *mut bool as isize) };
	found
}

/// The plugin-declared name for a parameter, falling back to a positional label.
fn param_name(instance: &PluginInstance, index: usize) -> String {
	instance
		.param_name(index)
		.filter(|name| !name.is_empty())
		.unwrap_or_else(|| format!("Param {index}"))
}

/// Render a `ParamValue` for the `params` listing.
fn describe(value: &ParamValue) -> String {
	match value {
		ParamValue::Float(v) => format!("float    {v}"),
		ParamValue::Fixed(v) => format!("fixed    {v}"),
		ParamValue::Slider(v) => format!("slider   {v}"),
		ParamValue::Popup(v) => format!("popup    {v}"),
		ParamValue::Angle(v) => format!("angle    {v}"),
		ParamValue::Checkbox(v) => format!("checkbox {v}"),
		ParamValue::Point { x, y } => format!("point    {x},{y}"),
		ParamValue::Color {
			red,
			green,
			blue,
			alpha,
		} => format!("color    {red},{green},{blue},{alpha}"),
		ParamValue::Path(id) => format!("path     {id}"),
		ParamValue::Point3D { x, y, z } => format!("point3d  {x},{y},{z}"),
		other => format!("{other:?}"),
	}
}
