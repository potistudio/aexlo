//! Dev-tooling helpers for `#[aexlo::preview]` and the `aexlo view` live window.
//!
//! Everything here is about *surfacing* a rendered frame during development --
//! env-var conventions, lock files, and viewer processes. None of it is part of
//! hosting a plugin; it lives in its own module so the core loader stays free
//! of process management, and may graduate to a separate crate later.

use std::path::{Path, PathBuf};

use crate::core::error::{AexloError, Result};

/// Whether a visual preview was requested, i.e. the `AEXLO_PREVIEW` env var is
/// set. Used by `#[aexlo::preview]` to decide whether to pop the OS viewer.
pub fn preview_requested() -> bool {
	std::env::var_os("AEXLO_PREVIEW").is_some()
}

/// How `#[aexlo::preview]` should surface the rendered frame, from
/// `AEXLO_PREVIEW`: unset = save only, `live` = keep a live `aexlo view` window
/// updated, anything else = open the OS viewer once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewMode {
	/// Just write the PNG.
	Off,
	/// Write the PNG and open it once in the OS image viewer.
	Once,
	/// Write the PNG and ensure a live `aexlo view` window is watching it.
	Live,
}

/// Read [`PreviewMode`] from the `AEXLO_PREVIEW` env var.
pub fn preview_mode() -> PreviewMode {
	match std::env::var("AEXLO_PREVIEW") {
		Err(_) => PreviewMode::Off,
		Ok(v) if v.eq_ignore_ascii_case("live") => PreviewMode::Live,
		Ok(_) => PreviewMode::Once,
	}
}

/// Sibling lock file recording the pid of the live `aexlo view` owning `png`.
fn viewer_lock_path(png: &Path) -> PathBuf {
	let name = png.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
	png.parent().unwrap_or_else(|| Path::new(".")).join(format!(".{name}.aexlo-view.lock"))
}

/// Best-effort "is this pid alive?" without pulling in platform FFI crates.
fn pid_alive(pid: u32) -> bool {
	#[cfg(unix)]
	{
		std::process::Command::new("kill")
			.arg("-0")
			.arg(pid.to_string())
			.stdout(std::process::Stdio::null())
			.stderr(std::process::Stdio::null())
			.status()
			.map(|s| s.success())
			.unwrap_or(false)
	}
	#[cfg(windows)]
	{
		// `tasklist /FI "PID eq N" /NH /FO CSV` prints a quoted row for a live
		// pid and a "no tasks" message otherwise. Unconditionally answering
		// `true` here (as before) meant a stale lock file blocked live viewers
		// from ever respawning on Windows.
		std::process::Command::new("tasklist")
			.args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
			.output()
			.map(|out| String::from_utf8_lossy(&out.stdout).contains(&format!("\"{pid}\"")))
			.unwrap_or(false)
	}
	#[cfg(not(any(unix, windows)))]
	{
		let _ = pid;
		// Unknown platform: claim dead so a stale lock never wedges the viewer;
		// worst case a duplicate window opens.
		false
	}
}

/// Whether a live `aexlo view` window already owns `png`.
pub fn viewer_is_running(png: impl AsRef<Path>) -> bool {
	match std::fs::read_to_string(viewer_lock_path(png.as_ref())) {
		Ok(contents) => contents.trim().parse::<u32>().map(pid_alive).unwrap_or(false),
		Err(_) => false,
	}
}

/// Ensure a live `aexlo view` window is watching `png`, spawning one (detached)
/// only if none is already running for it.
///
/// The viewer binary is `aexlo` by default; override with the `AEXLO_BIN` env
/// var (e.g. a `target/debug/aexlo` path in a workspace).
pub fn ensure_live_viewer(png: impl AsRef<Path>) -> Result<()> {
	let png = png.as_ref();
	if viewer_is_running(png) {
		return Ok(());
	}

	let bin = std::env::var("AEXLO_BIN").unwrap_or_else(|_| "aexlo".to_string());
	let mut cmd = std::process::Command::new(&bin);
	cmd.arg("view").arg(png);

	// Detach from our process group: a re-runner like `bacon` kills its job's
	// whole process group when the run finishes, which would otherwise take the
	// freshly spawned viewer window down with it.
	#[cfg(unix)]
	{
		use std::os::unix::process::CommandExt;
		cmd.process_group(0);
	}

	// The viewer outlives us, so inheriting our soon-closed stdio risks SIGPIPE,
	// while discarding it hides setup errors (e.g. a stale `aexlo` on PATH with
	// no `view` command). Send it to a log file instead.
	let log = std::env::temp_dir().join("aexlo-view.log");
	if let Ok(out) = std::fs::File::create(&log)
		&& let Ok(err) = out.try_clone()
	{
		cmd.stdout(std::process::Stdio::from(out)).stderr(std::process::Stdio::from(err));
	}

	cmd.spawn().map_err(|e| {
		AexloError::Unexpected(format!(
			"spawning live viewer `{bin} view` (set AEXLO_BIN to the aexlo binary, or install the CLI): {e}"
		))
	})?;
	Ok(())
}

/// Ownership of the live-viewer lock for a PNG; removes the lock on drop. Held
/// by `aexlo view` for as long as its window is open.
pub struct ViewerLock(PathBuf);

impl Drop for ViewerLock {
	fn drop(&mut self) {
		let _ = std::fs::remove_file(&self.0);
	}
}

/// Claim the live-viewer lock for `png`. Returns `None` if another live viewer
/// already owns it (the caller should exit); `Some` guard once we own it.
pub fn acquire_viewer_lock(png: impl AsRef<Path>) -> Option<ViewerLock> {
	let png = png.as_ref();
	if viewer_is_running(png) {
		return None;
	}
	let lock = viewer_lock_path(png);
	std::fs::write(&lock, std::process::id().to_string()).ok()?;
	Some(ViewerLock(lock))
}

/// Stable on-disk location for a preview PNG:
/// `<manifest_dir>/target/aexlo-preview/<module>_<name>.png`.
///
/// Lives under `target/` (git-ignored by default) so previews don't clutter the
/// source tree. The directory is created if missing. `manifest_dir` should be
/// the caller crate's `env!("CARGO_MANIFEST_DIR")`.
pub fn preview_path(manifest_dir: &str, module_path: &str, name: &str) -> PathBuf {
	let slug: String = module_path.chars().map(|c| if c.is_alphanumeric() { c } else { '_' }).collect();
	let dir = Path::new(manifest_dir).join("target").join("aexlo-preview");
	let _ = std::fs::create_dir_all(&dir);
	dir.join(format!("{slug}_{name}.png"))
}

/// Launch the OS image viewer on `path` without waiting (spawn, not status), so
/// it never blocks a test or `--watch` cycle.
pub fn open_in_viewer(path: impl AsRef<Path>) -> Result<()> {
	let path = path.as_ref();
	let program = if cfg!(target_os = "macos") {
		"open"
	} else if cfg!(target_os = "windows") {
		"explorer"
	} else {
		"xdg-open"
	};
	std::process::Command::new(program)
		.arg(path)
		.spawn()
		.map_err(|e| AexloError::Unexpected(format!("launching image viewer ({program}): {e}")))?;
	Ok(())
}
