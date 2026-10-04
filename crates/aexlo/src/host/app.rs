//! Injectable host-application services behind the "PF AE App Suite".
//!
//! The App Suite asks the *host* about things only a real application knows:
//! UI colors and fonts, the UI language, the mouse, a color picker, progress
//! dialogs. aexlo has no UI of its own, so by default every query is answered by
//! [`HeadlessAppHost`]. Embedders that do have a UI (or want to pretend to be a
//! render engine, a localized host, ...) implement [`AppHost`] and install it
//! with [`set_app_host`].
//!
//! App Suite callbacks carry no effect reference, so the host is process-wide:
//! one [`AppHost`] serves every [`PluginInstance`](crate::PluginInstance).

use std::sync::{Arc, LazyLock, RwLock};

/// A 16-bit-per-channel UI color, as reported by `PF_AppGetBgColor` / `PF_AppGetColor`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppColor {
	pub red: u16,
	pub green: u16,
	pub blue: u16,
}

/// A straight float color, as used by the color picker and eyedropper.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct AppPixelF {
	pub alpha: f32,
	pub red: f32,
	pub green: f32,
	pub blue: f32,
}

/// A point in panel-local or screen coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AppPoint {
	pub h: i32,
	pub v: i32,
}

/// Registered-user strings returned by `PF_GetPersonalInfo`.
/// Each string is truncated to the SDK's 63-byte limit.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PersonalInfo {
	pub name: String,
	pub org: String,
	pub serial: String,
}

/// A UI font returned by `PF_GetFontStyleSheet`.
/// `name` is truncated to the SDK's 255-byte limit.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FontInfo {
	pub name: String,
	pub font_num: i16,
	pub size: i16,
	pub style: i16,
}

/// Opaque id of a progress dialog opened through [`AppHost::progress_begin`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProgressId(pub u64);

/// Host-application services queried through the "PF AE App Suite".
///
/// Every method has a headless default, so implementors override only what
/// they support. Integer arguments (`color_type`, `sheet`, `mode`, `cursor`)
/// are the raw SDK enum values (`PF_App_Color_*`, `PF_FontStyle_*`,
/// `PF_EyeDropperSampleMode_*`, `PF_Cursor_*`).
///
/// Methods are called from plugin threads through FFI; a panic is caught and
/// reported to the plugin as an error, but implementations should not rely on it.
pub trait AppHost: Send + Sync {
	/// UI panel background color.
	fn bg_color(&self) -> AppColor {
		HeadlessAppHost::GRAY
	}

	/// Themed UI color for a `PF_App_Color_*` value.
	fn color(&self, color_type: i16) -> AppColor {
		let _ = color_type;
		self.bg_color()
	}

	/// UI language tag such as `"en_US"` (at most 5 bytes). Empty means
	/// "no specific language", which makes plugins use their base strings.
	fn language(&self) -> String {
		String::new()
	}

	/// Registered user / organization / serial.
	fn personal_info(&self) -> PersonalInfo {
		PersonalInfo::default()
	}

	/// UI font for a `PF_FontStyle_*` sheet.
	fn font_style_sheet(&self, sheet: i32) -> FontInfo {
		let _ = sheet;
		FontInfo::default()
	}

	/// Change the mouse cursor to a `PF_Cursor_*` value.
	fn set_cursor(&self, cursor: i32) {
		let _ = cursor;
	}

	/// Whether the host is a UI-less render engine (`aerender`, watch folder, ...).
	fn is_render_engine(&self) -> bool {
		false
	}

	/// Run a color picker. `None` means the user cancelled.
	fn pick_color(&self, title: Option<&str>, sample: AppPixelF, use_ws_to_monitor_xform: bool) -> Option<AppPixelF> {
		let _ = (title, use_ws_to_monitor_xform);
		Some(sample)
	}

	/// Current mouse position in panel-local coordinates.
	fn mouse(&self) -> AppPoint {
		AppPoint::default()
	}

	/// Request a redraw of the UI context (`rect` of `None` means everything).
	/// `context` is the raw `PF_ContextH`.
	fn invalidate_rect(&self, context: usize, rect: Option<(i32, i32, i32, i32)>) {
		let _ = (context, rect);
	}

	/// Convert panel-local to screen coordinates.
	fn local_to_global(&self, local: AppPoint) -> AppPoint {
		local
	}

	/// Eyedropper: sample the screen color at `global`.
	fn color_at_global_point(&self, global: AppPoint, eye_size: i16, mode: i16) -> AppPixelF {
		let _ = (global, eye_size, mode);
		AppPixelF::default()
	}

	/// Open a progress dialog. The returned id is passed back to
	/// [`progress_update`](Self::progress_update) and [`progress_end`](Self::progress_end).
	fn progress_begin(&self, title: &str, cancel_label: Option<&str>, indeterminate: bool) -> ProgressId {
		let _ = (title, cancel_label, indeterminate);
		ProgressId(0)
	}

	/// Advance a progress dialog. Return `false` if the user cancelled.
	fn progress_update(&self, id: ProgressId, count: i32, total: i32) -> bool {
		let _ = (id, count, total);
		true
	}

	/// Close a progress dialog.
	fn progress_end(&self, id: ProgressId) {
		let _ = id;
	}
}

/// The default [`AppHost`]: no UI, every query answers a neutral value and
/// every UI action is a no-op.
#[derive(Debug, Clone, Copy, Default)]
pub struct HeadlessAppHost;

impl HeadlessAppHost {
	/// Neutral mid-gray reported for every UI color.
	pub const GRAY: AppColor = AppColor { red: 0x8000, green: 0x8000, blue: 0x8000 };
}

impl AppHost for HeadlessAppHost {}

static APP_HOST: LazyLock<RwLock<Arc<dyn AppHost>>> = LazyLock::new(|| RwLock::new(Arc::new(HeadlessAppHost)));

/// Install the process-wide [`AppHost`], replacing the previous one.
/// Takes effect for every subsequent App Suite call from any plugin.
pub fn set_app_host(host: impl AppHost + 'static) {
	*APP_HOST.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(host);
}

/// Restore the default [`HeadlessAppHost`].
pub fn reset_app_host() {
	set_app_host(HeadlessAppHost);
}

/// The currently installed host. The lock is released before the caller
/// invokes it, so a host may itself call [`set_app_host`].
pub(crate) fn app_host() -> Arc<dyn AppHost> {
	APP_HOST.read().unwrap_or_else(|e| e.into_inner()).clone()
}
