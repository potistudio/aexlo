//! "PF AE Adv App Suite" callbacks: project state and the Info panel.
//!
//! Every callback delegates to the installed [`AppHost`](crate::host::app::AppHost)
//! (headless by default: all of them are no-ops).

use super::ffi::read_c_str;
use crate::core::diagnostics::diag;
use crate::host::app::{AppPixel8, InfoLine, with_app_host_ffi as with_host};
use after_effects_sys::{A_char, PF_AdvAppSuite1, PF_AdvAppSuite2, PF_Err, PF_Err_NONE, PF_Pixel};

/// Build an [`InfoLine`] from optional left/right-justified C strings.
///
/// # Safety
/// Each pointer must be null or point to a NUL-terminated string.
unsafe fn info_line(left: *const A_char, right: *const A_char) -> InfoLine {
	InfoLine {
		left: unsafe { read_c_str(left) }.unwrap_or_default(),
		right: unsafe { read_c_str(right) }.unwrap_or_default(),
	}
}

macro_rules! host_action {
	($name:ident, $diag:literal, $method:ident) => {
		unsafe extern "C" fn $name() -> PF_Err {
			diag!($diag);
			with_host(|host| {
				host.$method();
				PF_Err_NONE as PF_Err
			})
		}
	};
}

host_action!(
	set_project_dirty,
	"PF_AdvAppSuite/PF_SetProjectDirty",
	set_project_dirty
);
host_action!(save_project, "PF_AdvAppSuite/PF_SaveProject", save_project);
host_action!(
	save_background_state,
	"PF_AdvAppSuite/PF_SaveBackgroundState",
	save_background_state
);
host_action!(force_foreground, "PF_AdvAppSuite/PF_ForceForeground", force_foreground);
host_action!(
	restore_background_state,
	"PF_AdvAppSuite/PF_RestoreBackgroundState",
	restore_background_state
);
host_action!(
	refresh_all_windows,
	"PF_AdvAppSuite/PF_RefreshAllWindows",
	refresh_all_windows
);

unsafe extern "C" fn info_draw_text3(line1Z0: *const A_char, line2Z0: *const A_char, line3Z0: *const A_char) -> PF_Err {
	diag!("PF_AdvAppSuite/PF_InfoDrawText3",
		"line1Z0" => format!("{:#x}", line1Z0 as usize),
		"line2Z0" => format!("{:#x}", line2Z0 as usize),
		"line3Z0" => format!("{:#x}", line3Z0 as usize),
	);

	with_host(|host| {
		let null = std::ptr::null();
		let lines = unsafe {
			[
				info_line(line1Z0, null),
				info_line(line2Z0, null),
				info_line(line3Z0, null),
			]
		};
		host.info_draw_text(&lines);
		PF_Err_NONE as PF_Err
	})
}

/// Same as `PF_InfoDrawText3(line1, line2, NULL)`, per the SDK.
unsafe extern "C" fn info_draw_text(line1Z0: *const A_char, line2Z0: *const A_char) -> PF_Err {
	unsafe { info_draw_text3(line1Z0, line2Z0, std::ptr::null()) }
}

unsafe extern "C" fn info_draw_text3_plus(
	line1Z0: *const A_char,
	line2_jrZ0: *const A_char,
	line2_jlZ0: *const A_char,
	line3_jrZ0: *const A_char,
	line3_jlZ0: *const A_char,
) -> PF_Err {
	diag!("PF_AdvAppSuite/PF_InfoDrawText3Plus",
		"line1Z0" => format!("{:#x}", line1Z0 as usize),
		"line2_jrZ0" => format!("{:#x}", line2_jrZ0 as usize),
		"line2_jlZ0" => format!("{:#x}", line2_jlZ0 as usize),
		"line3_jrZ0" => format!("{:#x}", line3_jrZ0 as usize),
		"line3_jlZ0" => format!("{:#x}", line3_jlZ0 as usize),
	);

	with_host(|host| {
		let lines = unsafe {
			[
				info_line(line1Z0, std::ptr::null()),
				info_line(line2_jlZ0, line2_jrZ0),
				info_line(line3_jlZ0, line3_jrZ0),
			]
		};
		host.info_draw_text(&lines);
		PF_Err_NONE as PF_Err
	})
}

unsafe extern "C" fn info_draw_color(color: PF_Pixel) -> PF_Err {
	diag!("PF_AdvAppSuite/PF_InfoDrawColor",
		"color" => format!("{:?}", (color.alpha, color.red, color.green, color.blue)),
	);

	with_host(|host| {
		host.info_draw_color(AppPixel8 {
			alpha: color.alpha,
			red: color.red,
			green: color.green,
			blue: color.blue,
		});
		PF_Err_NONE as PF_Err
	})
}

unsafe extern "C" fn append_info_text(appendZ0: *const A_char) -> PF_Err {
	diag!("PF_AdvAppSuite/PF_AppendInfoText",
		"appendZ0" => format!("{:#x}", appendZ0 as usize),
	);

	with_host(|host| {
		host.append_info_text(&unsafe { read_c_str(appendZ0) }.unwrap_or_default());
		PF_Err_NONE as PF_Err
	})
}

/// Build the `PF_AdvAppSuite1` table (wire version 1).
pub(super) const fn create_adv_app_suite_1() -> PF_AdvAppSuite1 {
	PF_AdvAppSuite1 {
		PF_SetProjectDirty: Some(set_project_dirty),
		PF_SaveProject: Some(save_project),
		PF_SaveBackgroundState: Some(save_background_state),
		PF_ForceForeground: Some(force_foreground),
		PF_RestoreBackgroundState: Some(restore_background_state),
		PF_RefreshAllWindows: Some(refresh_all_windows),
		PF_InfoDrawText: Some(info_draw_text),
		PF_InfoDrawColor: Some(info_draw_color),
		PF_InfoDrawText3: Some(info_draw_text3),
		PF_InfoDrawText3Plus: Some(info_draw_text3_plus),
	}
}

/// Build the `PF_AdvAppSuite2` table (wire version 2; v1 plus `PF_AppendInfoText`).
pub(super) const fn create_adv_app_suite_2() -> PF_AdvAppSuite2 {
	PF_AdvAppSuite2 {
		PF_SetProjectDirty: Some(set_project_dirty),
		PF_SaveProject: Some(save_project),
		PF_SaveBackgroundState: Some(save_background_state),
		PF_ForceForeground: Some(force_foreground),
		PF_RestoreBackgroundState: Some(restore_background_state),
		PF_RefreshAllWindows: Some(refresh_all_windows),
		PF_InfoDrawText: Some(info_draw_text),
		PF_InfoDrawColor: Some(info_draw_color),
		PF_InfoDrawText3: Some(info_draw_text3),
		PF_InfoDrawText3Plus: Some(info_draw_text3_plus),
		PF_AppendInfoText: Some(append_info_text),
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::host::app::{AppHost, TEST_APP_HOST};
	use std::sync::{Arc, Mutex};

	#[derive(Clone, Default)]
	struct Recorder(Arc<Mutex<Vec<String>>>);
	impl AppHost for Recorder {
		fn info_draw_text(&self, lines: &[InfoLine; 3]) {
			let mut log = self.0.lock().unwrap();
			for l in lines {
				log.push(format!("{}|{}", l.left, l.right));
			}
		}
		fn set_project_dirty(&self) {
			self.0.lock().unwrap().push("dirty".into());
		}
	}

	#[test]
	fn info_text_reaches_host() {
		let rec = Recorder::default();
		TEST_APP_HOST.set(Some(Box::new(rec.clone())));
		let suite = create_adv_app_suite_2();
		unsafe {
			suite.PF_SetProjectDirty.unwrap()();
			suite.PF_InfoDrawText.unwrap()(c"a".as_ptr(), c"b".as_ptr());
			suite.PF_InfoDrawText3Plus.unwrap()(
				c"t".as_ptr(),
				c"r2".as_ptr(),
				c"l2".as_ptr(),
				std::ptr::null(),
				c"l3".as_ptr(),
			);
		}
		TEST_APP_HOST.set(None);
		assert_eq!(*rec.0.lock().unwrap(), ["dirty", "a|", "b|", "|", "t|", "l2|r2", "l3|"]);
	}
}
