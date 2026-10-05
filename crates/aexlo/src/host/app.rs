//! Injectable host-application services.
//!
//! Several suites ask the *host* about things only a real application knows:
//! UI colors and fonts, the UI language, the mouse, a color picker, progress
//! dialogs ("PF AE App Suite"), the Info panel and project state ("PF AE Adv App
//! Suite"), the current time and comp viewer ("PF AE Adv Item Suite"), the time
//! display preferences ("PF AE Adv Time Suite"), the tool palette ("AE Plugin
//! Helper Suite"/"Suite2") and the custom-UI overlay theme ("PF Effect Custom UI
//! Overlay Theme Suite"). aexlo has no UI of its own, so by default every query
//! is answered by [`HeadlessAppHost`]. Embedders that do have a UI (or want to
//! pretend to be a render engine, a localized host, ...) implement [`AppHost`]
//! and install it with [`Host::install`].
//!
//! Most of these callbacks carry no effect reference, so the host is process-wide:
//! one [`AppHost`] serves every [`PluginInstance`](crate::PluginInstance). It is
//! fixed at most once, and every `PluginInstance` constructor takes a [`Host`]
//! token that only exists once it is fixed, so "install before loading" is
//! enforced by the type system: every plugin call (including a multi-call
//! sequence like a progress dialog) sees the same host.

use std::sync::OnceLock;

use crate::core::error::{AexloError, Result};

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

/// An 8-bit straight color, as passed to `PF_InfoDrawColor`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AppPixel8 {
	pub alpha: u8,
	pub red: u8,
	pub green: u8,
	pub blue: u8,
}

/// One line of the Info panel. `PF_InfoDrawText`/`PF_InfoDrawText3` only fill
/// `left`; `PF_InfoDrawText3Plus` splits lines 2 and 3 into a left-justified
/// and a right-justified part.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InfoLine {
	pub left: String,
	pub right: String,
}

/// How the host displays time (`PF_TimeDisplayFormat*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TimeDisplayMode {
	/// `H:MM:SS:FF` (`;` separators when drop-frame).
	#[default]
	Timecode,
	/// A plain frame count, or feet+frames when
	/// [`TimeDisplayPref::use_feet_frames`] is set.
	Frames,
}

/// Time display preferences reported by `PF_GetTimeDisplayPref` and used by
/// aexlo to format times for `PF_FormatTime*`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeDisplayPref {
	pub mode: TimeDisplayMode,
	/// Timecode base in frames per second (`framemax`).
	pub timebase: i32,
	/// Frames per foot for feet+frames display.
	pub frames_per_foot: i32,
	/// Frame numbers start at 0 or 1 (`frames_start`).
	pub frames_start: i32,
	/// Number of the composition's first frame (`starting_frame_num`).
	pub starting_frame: i32,
	/// Non-drop-frame timecode for 29.97/59.94 bases.
	pub non_drop: bool,
	pub honor_source_timecode: bool,
	pub use_feet_frames: bool,
}

impl Default for TimeDisplayPref {
	fn default() -> Self {
		Self {
			mode: TimeDisplayMode::Timecode,
			timebase: 30,
			frames_per_foot: 16,
			frames_start: 0,
			starting_frame: 0,
			non_drop: true,
			honor_source_timecode: true,
			use_feet_frames: false,
		}
	}
}

/// Colors and metrics custom UIs should draw overlays with, reported by the
/// "PF Effect Custom UI Overlay Theme Suite".
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OverlayTheme {
	pub foreground: AppPixelF,
	pub shadow: AppPixelF,
	pub stroke_width: f32,
	pub vertex_size: f32,
	pub shadow_offset: AppPoint,
}

impl Default for OverlayTheme {
	fn default() -> Self {
		Self {
			foreground: AppPixelF {
				alpha: 1.0,
				red: 1.0,
				green: 1.0,
				blue: 1.0,
			},
			shadow: AppPixelF {
				alpha: 0.5,
				red: 0.0,
				green: 0.0,
				blue: 0.0,
			},
			stroke_width: 1.0,
			vertex_size: 6.0,
			shadow_offset: AppPoint { h: 1, v: 1 },
		}
	}
}

/// Host-application services queried through the App, Adv App, Adv Item, Adv
/// Time, Plugin Helper and Custom UI Overlay Theme suites, and the
/// `PF_ABORT`/`PF_PROGRESS` interact callbacks.
///
/// Every method has a headless default, so implementors override only what
/// they support. Integer arguments (`color_type`, `sheet`, `mode`, `cursor`,
/// `tool`) are the raw SDK enum values (`PF_App_Color_*`, `PF_FontStyle_*`,
/// `PF_EyeDropperSampleMode_*`, `PF_Cursor_*`, `PF_ExtendedSuiteTool_*`).
/// Raw handles (`context`, `effect`) are the `PF_ContextH` / `PF_ProgPtr`
/// values the plugin passed, as integers.
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

	// ---- Interact callbacks: PF_ABORT / PF_PROGRESS --------------------------

	/// Whether the render of the effect identified by the raw `PF_ProgPtr`
	/// should stop (the user cancelled). Plugins poll this during long renders.
	fn abort_requested(&self, effect: usize) -> bool {
		let _ = effect;
		false
	}

	/// Progress of the effect's render, `current` out of `total`. Return
	/// `false` to cancel the render.
	fn render_progress(&self, effect: usize, current: i32, total: i32) -> bool {
		let _ = (effect, current, total);
		true
	}

	// ---- PF AE Adv App Suite ------------------------------------------------

	/// Mark the open project as modified.
	fn set_project_dirty(&self) {}

	/// Save the open project.
	fn save_project(&self) {}

	/// Remember the UI state before a plugin brings the app to the foreground.
	fn save_background_state(&self) {}

	/// Bring the application to the foreground.
	fn force_foreground(&self) {}

	/// Restore the UI state saved by [`save_background_state`](Self::save_background_state).
	fn restore_background_state(&self) {}

	/// Redraw every window.
	fn refresh_all_windows(&self) {}

	/// Show three lines of text in the Info panel.
	fn info_draw_text(&self, lines: &[InfoLine; 3]) {
		let _ = lines;
	}

	/// Show a color swatch in the Info panel.
	fn info_draw_color(&self, color: AppPixel8) {
		let _ = color;
	}

	/// Append text to the Info panel's top line.
	fn append_info_text(&self, text: &str) {
		let _ = text;
	}

	// ---- PF AE Adv Item Suite -----------------------------------------------

	/// Move the current time of the active item by `steps` frames (negative
	/// steps move backward). `effect` is the raw `PF_ProgPtr`, or `0` for
	/// `PF_MoveTimeStepActiveItem`.
	fn move_time_step(&self, effect: usize, steps: i32) {
		let _ = (effect, steps);
	}

	/// Mark the active item as changed so it is re-rendered.
	fn touch_active_item(&self) {}

	/// Force the effect identified by the raw `PF_ProgPtr` to re-render.
	fn force_rerender(&self, effect: usize) {
		let _ = effect;
	}

	/// Whether the effect drawing into `context` is enabled and its layer active.
	fn effect_is_active_or_enabled(&self, context: usize) -> bool {
		let _ = context;
		true
	}

	// ---- PF AE Adv Time Suite -----------------------------------------------

	/// Time display preferences. Also drives how aexlo formats times for
	/// `PF_FormatTime*`.
	fn time_display_pref(&self) -> TimeDisplayPref {
		TimeDisplayPref::default()
	}

	// ---- AE Plugin Helper Suite / Suite2 ------------------------------------

	/// The selected tool, as a `PF_ExtendedSuiteTool_*` value.
	fn current_tool(&self) -> i32 {
		after_effects_sys::PF_ExtendedSuiteTool_NONE as i32
	}

	/// Select a tool (`PF_ExtendedSuiteTool_*`).
	fn set_current_tool(&self, tool: i32) {
		let _ = tool;
	}

	/// Re-read the system clipboard.
	fn parse_clipboard(&self) {}

	// ---- PF Effect Custom UI Overlay Theme Suite ----------------------------

	/// Colors and metrics for custom-UI overlays.
	fn overlay_theme(&self) -> OverlayTheme {
		OverlayTheme::default()
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

static APP_HOST: OnceLock<Box<dyn AppHost>> = OnceLock::new();

/// Proof that the process-wide [`AppHost`] has been fixed.
///
/// Every [`PluginInstance`](crate::PluginInstance) constructor requires one,
/// and the only ways to obtain one ([`Host::install`], [`Host::get`]) fix the
/// host first. So a plugin can never run before the host is chosen, and the
/// host can never change once a plugin has run.
///
/// Zero-sized and `Copy`: obtain it once and pass it around freely.
#[derive(Debug, Clone, Copy)]
pub struct Host {
	_fixed: (),
}

impl Host {
	/// Install `app` as the process-wide [`AppHost`].
	///
	/// # Errors
	/// [`AexloError::AppHostAlreadySet`] if the host was already fixed (by an
	/// earlier `install` or [`Host::get`]); the existing host stays in effect.
	pub fn install(app: impl AppHost + 'static) -> Result<Self> {
		APP_HOST.set(Box::new(app)).map_err(|_| AexloError::AppHostAlreadySet)?;
		Ok(Self { _fixed: () })
	}

	/// The host token, fixing the host to [`HeadlessAppHost`] if nothing was
	/// installed yet. Idempotent; use this when you don't customize the host.
	pub fn get() -> Self {
		APP_HOST.get_or_init(|| Box::new(HeadlessAppHost));
		Self { _fixed: () }
	}
}

#[cfg(test)]
thread_local! {
	/// Per-thread override so unit tests can inject a host without touching
	/// the set-once global (tests run in parallel and cannot reset it).
	pub(crate) static TEST_APP_HOST: std::cell::RefCell<Option<Box<dyn AppHost>>> =
		const { std::cell::RefCell::new(None) };
}

/// Run `f` against the installed host.
///
/// Every plugin is constructed with a [`Host`] token, so the host is always
/// fixed by the time a plugin calls back; the headless fallback only guards
/// against a plugin calling in some unforeseen way.
pub(crate) fn with_app_host<R>(f: impl FnOnce(&dyn AppHost) -> R) -> R {
	#[cfg(test)]
	if TEST_APP_HOST.with_borrow(Option::is_some) {
		return TEST_APP_HOST.with_borrow(|h| f(h.as_deref().unwrap()));
	}
	f(APP_HOST.get_or_init(|| Box::new(HeadlessAppHost)).as_ref())
}

/// [`with_app_host`] for FFI callbacks: a panicking host is reported to the
/// plugin as `PF_Err_INTERNAL_STRUCT_DAMAGED` instead of unwinding across the
/// FFI boundary.
pub(crate) fn with_app_host_ffi(
	f: impl FnOnce(&dyn AppHost) -> after_effects_sys::PF_Err,
) -> after_effects_sys::PF_Err {
	std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| with_app_host(f))).unwrap_or_else(|_| {
		log::error!("AppHost panicked; reporting PF_Err_INTERNAL_STRUCT_DAMAGED");
		after_effects_sys::PF_Err_INTERNAL_STRUCT_DAMAGED as after_effects_sys::PF_Err
	})
}
