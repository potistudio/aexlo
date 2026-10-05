//! "AE Plugin Helper Suite" / "AE Plugin Helper Suite2" callbacks: the tool
//! palette and the clipboard.
//!
//! Every callback delegates to the installed [`AppHost`](crate::host::app::AppHost)
//! (headless by default: no tool is selected and the clipboard is ignored).

use crate::core::diagnostics::diag;
use crate::host::app::with_app_host_ffi as with_host;
use after_effects_sys::*;

/// Map a `PF_ExtendedSuiteTool_*` to the legacy `PF_SuiteTool_*` reported by
/// `PF_HelperSuite1`; tools the legacy enum has no name for map to `NONE`.
#[allow(non_upper_case_globals, clippy::unnecessary_cast)] // the constants are `u32` on macOS only.
fn legacy_tool(tool: i32) -> PF_SuiteTool {
	let legacy = match tool as u32 {
		t if t == PF_ExtendedSuiteTool_ARROW as u32 => PF_SuiteTool_ARROW,
		t if t == PF_ExtendedSuiteTool_ROTATE as u32 => PF_SuiteTool_ROTATE,
		t if t == PF_ExtendedSuiteTool_PEN_NORMAL as u32
			|| t == PF_ExtendedSuiteTool_PEN_ADD_POINT as u32
			|| t == PF_ExtendedSuiteTool_PEN_DELETE_POINT as u32
			|| t == PF_ExtendedSuiteTool_PEN_CONVERT_POINT as u32 =>
		{
			PF_SuiteTool_PEN
		}
		t if t == PF_ExtendedSuiteTool_RECT as u32 || t == PF_ExtendedSuiteTool_OVAL as u32 => PF_SuiteTool_SHAPE,
		t if t == PF_ExtendedSuiteTool_PAN_BEHIND as u32 => PF_SuiteTool_PAN,
		t if t == PF_ExtendedSuiteTool_HAND as u32 => PF_SuiteTool_HAND,
		t if t == PF_ExtendedSuiteTool_MAGNIFY as u32 => PF_SuiteTool_MAGNIFY,
		t if t == PF_ExtendedSuiteTool_ROUNDED_RECT as u32 => PF_SuiteTool_ROUNDED_RECT,
		t if t == PF_ExtendedSuiteTool_POLYGON as u32 => PF_SuiteTool_POLYGON,
		t if t == PF_ExtendedSuiteTool_STAR as u32 => PF_SuiteTool_STAR,
		t if t == PF_ExtendedSuiteTool_PIN as u32 => PF_SuiteTool_PIN,
		t if t == PF_ExtendedSuiteTool_PIN_STARCH as u32 => PF_SuiteTool_PIN_STARCH,
		t if t == PF_ExtendedSuiteTool_PIN_DEPTH as u32 => PF_SuiteTool_PIN_DEPTH,
		_ => PF_SuiteTool_NONE,
	};
	legacy as PF_SuiteTool
}

unsafe extern "C" fn get_current_tool(toolP: *mut PF_SuiteTool) -> PF_Err {
	diag!("PF_HelperSuite1/PF_GetCurrentTool",
		"toolP" => format!("{:#x}", toolP as usize),
	);

	if toolP.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}
	with_host(|host| {
		unsafe { *toolP = legacy_tool(host.current_tool()) };
		PF_Err_NONE as PF_Err
	})
}

unsafe extern "C" fn parse_clipboard() -> PF_Err {
	diag!("PF_HelperSuite2/PF_ParseClipboard");

	with_host(|host| {
		host.parse_clipboard();
		PF_Err_NONE as PF_Err
	})
}

unsafe extern "C" fn set_current_extended_tool(tool: PF_ExtendedSuiteTool) -> PF_Err {
	diag!("PF_HelperSuite2/PF_SetCurrentExtendedTool",
		"tool" => tool,
	);

	with_host(|host| {
		host.set_current_tool(tool as i32);
		PF_Err_NONE as PF_Err
	})
}

unsafe extern "C" fn get_current_extended_tool(tool: *mut PF_ExtendedSuiteTool) -> PF_Err {
	diag!("PF_HelperSuite2/PF_GetCurrentExtendedTool",
		"tool" => format!("{:#x}", tool as usize),
	);

	if tool.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}
	with_host(|host| {
		unsafe { *tool = host.current_tool() as PF_ExtendedSuiteTool };
		PF_Err_NONE as PF_Err
	})
}

/// Build the `PF_HelperSuite1` table ("AE Plugin Helper Suite").
pub(super) const fn create_helper_suite_1() -> PF_HelperSuite1 {
	PF_HelperSuite1 {
		PF_GetCurrentTool: Some(get_current_tool),
	}
}

/// Build the `PF_HelperSuite2` table ("AE Plugin Helper Suite2").
pub(super) const fn create_helper_suite_2() -> PF_HelperSuite2 {
	PF_HelperSuite2 {
		PF_ParseClipboard: Some(parse_clipboard),
		PF_SetCurrentExtendedTool: Some(set_current_extended_tool),
		PF_GetCurrentExtendedTool: Some(get_current_extended_tool),
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::host::app::{AppHost, TEST_APP_HOST};

	struct PenHost;
	impl AppHost for PenHost {
		fn current_tool(&self) -> i32 {
			PF_ExtendedSuiteTool_PEN_ADD_POINT as i32
		}
	}

	#[test]
	fn tools_come_from_host() {
		TEST_APP_HOST.set(Some(Box::new(PenHost)));
		let (mut legacy, mut extended) = (0 as PF_SuiteTool, 0 as PF_ExtendedSuiteTool);
		unsafe {
			create_helper_suite_1().PF_GetCurrentTool.unwrap()(&mut legacy);
			create_helper_suite_2().PF_GetCurrentExtendedTool.unwrap()(&mut extended);
		}
		TEST_APP_HOST.set(None);
		assert_eq!(legacy, PF_SuiteTool_PEN as PF_SuiteTool);
		assert_eq!(extended, PF_ExtendedSuiteTool_PEN_ADD_POINT as PF_ExtendedSuiteTool);
	}
}
