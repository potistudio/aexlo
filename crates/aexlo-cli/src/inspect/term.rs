//! The terminal `aexlo inspect` draws on, and keeping it usable around a
//! plugin that prints or crashes.
//!
//! On Unix the UI draws on `/dev/tty` while file descriptors 1 and 2 go to a
//! pipe, so whatever the plugin (or the `log` crate) prints lands in the Log
//! tab instead of over the UI. A plugin crash cannot be caught, but a signal
//! handler puts the terminal back before the process dies.

use std::collections::VecDeque;
use std::fs::File;
use std::io::Write;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::{
	cursor, execute,
	terminal::{Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};

/// Lines of captured output kept.
const CAPTURE_LINES: usize = 5_000;

/// Captured stdout/stderr lines, shared with the reader thread.
#[derive(Clone, Default)]
pub struct Captured(Arc<Mutex<VecDeque<String>>>);

impl Captured {
	/// Take the lines captured since the last call.
	pub fn drain(&self) -> Vec<String> {
		self.0
			.lock()
			.map(|mut lines| lines.drain(..).collect())
			.unwrap_or_default()
	}

	#[cfg_attr(not(unix), allow(dead_code))]
	fn push(&self, line: String) {
		if let Ok(mut lines) = self.0.lock() {
			if lines.len() >= CAPTURE_LINES {
				lines.pop_front();
			}
			lines.push_back(line);
		}
	}
}

pub type Tui = Terminal<CrosstermBackend<File>>;

/// The terminal in TUI mode; dropping it restores the terminal and stdio.
pub struct Session {
	pub terminal: Tui,
	pub captured: Captured,
	#[cfg(unix)]
	saved: unix::Saved,
}

impl Session {
	pub fn start() -> Result<Self> {
		let captured = Captured::default();
		#[cfg(unix)]
		let (tty, saved) = unix::start(&captured)?;
		#[cfg(not(unix))]
		let tty = other::tty()?;

		let terminal = (|| -> Result<Tui> {
			enable_raw_mode()?;
			let mut tty = tty;
			// The first frame only draws what differs from blank cells, so
			// start blank. (`Terminal::clear` would query the cursor, which
			// not every terminal answers.)
			execute!(tty, EnterAlternateScreen, cursor::Hide, Clear(ClearType::All))?;
			Ok(Terminal::new(CrosstermBackend::new(tty))?)
		})();
		let terminal = match terminal {
			Ok(terminal) => terminal,
			Err(err) => {
				restore_screen(&mut std::io::stdout());
				#[cfg(unix)]
				saved.restore();
				return Err(err);
			}
		};
		install_panic_hook();
		Ok(Self {
			terminal,
			captured,
			#[cfg(unix)]
			saved,
		})
	}
}

impl Drop for Session {
	fn drop(&mut self) {
		restore_screen(self.terminal.backend_mut());
		#[cfg(unix)]
		self.saved.restore();
	}
}

fn restore_screen(out: &mut impl Write) {
	let _ = disable_raw_mode();
	let _ = execute!(out, LeaveAlternateScreen, cursor::Show);
}

fn install_panic_hook() {
	let previous = std::panic::take_hook();
	std::panic::set_hook(Box::new(move |info| {
		let mut out = std::io::stdout();
		#[cfg(unix)]
		unix::restore_stdio();
		restore_screen(&mut out);
		previous(info);
	}));
}

#[cfg(not(unix))]
mod other {
	use super::*;

	/// The console, through a handle of its own (`CONOUT$`).
	pub fn tty() -> Result<File> {
		Ok(std::fs::OpenOptions::new().read(true).write(true).open("CONOUT$")?)
	}
}

#[cfg(unix)]
mod unix {
	use super::*;
	use std::io::{BufRead, BufReader};
	use std::os::fd::{AsRawFd, FromRawFd};
	use std::sync::OnceLock;
	use std::sync::atomic::{AtomicI32, Ordering};

	/// The terminal settings before raw mode, for the crash handler.
	static TERMIOS: OnceLock<libc::termios> = OnceLock::new();
	/// The terminal's descriptor, for the crash handler (-1: none).
	static TTY: AtomicI32 = AtomicI32::new(-1);
	/// The original stdout and stderr (-1: not redirected).
	static STDOUT: AtomicI32 = AtomicI32::new(-1);
	static STDERR: AtomicI32 = AtomicI32::new(-1);

	pub struct Saved;

	impl Saved {
		pub fn restore(&self) {
			restore_stdio();
		}
	}

	pub fn start(captured: &Captured) -> Result<(File, Saved)> {
		let tty = std::fs::OpenOptions::new().read(true).write(true).open("/dev/tty")?;
		let fd = tty.as_raw_fd();
		// SAFETY: `fd` is the open terminal; `termios` is plain data.
		unsafe {
			let mut termios = std::mem::zeroed::<libc::termios>();
			if libc::tcgetattr(fd, &mut termios) == 0 {
				let _ = TERMIOS.set(termios);
			}
		}
		TTY.store(fd, Ordering::SeqCst);
		install_crash_handler();
		redirect(captured)?;
		Ok((tty, Saved))
	}

	/// Point descriptors 1 and 2 at a pipe read into `captured`.
	fn redirect(captured: &Captured) -> Result<()> {
		let _ = std::io::stdout().flush();
		let mut fds = [0; 2];
		// SAFETY: plain descriptor calls on descriptors this process owns.
		unsafe {
			if libc::pipe(fds.as_mut_ptr()) != 0 {
				return Err(std::io::Error::last_os_error().into());
			}
			STDOUT.store(libc::dup(1), Ordering::SeqCst);
			STDERR.store(libc::dup(2), Ordering::SeqCst);
			libc::dup2(fds[1], 1);
			libc::dup2(fds[1], 2);
			libc::close(fds[1]);
		}
		// SAFETY: `fds[0]` is the pipe's read end, owned by nothing else.
		let reader = unsafe { File::from_raw_fd(fds[0]) };
		let captured = captured.clone();
		std::thread::Builder::new()
			.name("aexlo-inspect-capture".into())
			.spawn(move || {
				for line in BufReader::new(reader).split(b'\n') {
					let Ok(line) = line else { break };
					captured.push(String::from_utf8_lossy(&line).trim_end_matches('\r').to_string());
				}
			})?;
		Ok(())
	}

	/// Put stdout and stderr back where they were.
	pub fn restore_stdio() {
		let _ = std::io::stdout().flush();
		for (saved, target) in [(&STDOUT, 1), (&STDERR, 2)] {
			let fd = saved.swap(-1, Ordering::SeqCst);
			if fd >= 0 {
				// SAFETY: `fd` is the descriptor `redirect` duplicated.
				unsafe {
					libc::dup2(fd, target);
					libc::close(fd);
				}
			}
		}
	}

	fn install_crash_handler() {
		for signal in [libc::SIGSEGV, libc::SIGBUS, libc::SIGILL, libc::SIGFPE, libc::SIGABRT] {
			// SAFETY: installs `on_crash`, which only makes async-signal-safe
			// calls; SA_RESETHAND makes the next delivery the default action.
			unsafe {
				let mut action = std::mem::zeroed::<libc::sigaction>();
				action.sa_sigaction = on_crash as *const () as usize;
				action.sa_flags = libc::SA_RESETHAND | libc::SA_ONSTACK;
				libc::sigemptyset(&mut action.sa_mask);
				libc::sigaction(signal, &action, std::ptr::null_mut());
			}
		}
	}

	/// Restore the terminal and stderr, say what happened and let the
	/// default action (now reinstated) kill the process.
	extern "C" fn on_crash(signal: libc::c_int) {
		const RESTORE: &[u8] = b"\x1b[?1049l\x1b[?25h";
		const MESSAGE: &[u8] = b"\naexlo inspect: the plugin crashed (signal ";
		let tty = TTY.load(Ordering::SeqCst);
		// SAFETY: write, tcsetattr, dup2 and raise are async-signal-safe.
		unsafe {
			if tty >= 0 {
				libc::write(tty, RESTORE.as_ptr().cast(), RESTORE.len());
				if let Some(termios) = TERMIOS.get() {
					libc::tcsetattr(tty, libc::TCSANOW, termios);
				}
			}
			for (saved, target) in [(&STDOUT, 1), (&STDERR, 2)] {
				let fd = saved.load(Ordering::SeqCst);
				if fd >= 0 {
					libc::dup2(fd, target);
				}
			}
			libc::write(2, MESSAGE.as_ptr().cast(), MESSAGE.len());
			let mut digits = [0u8; 4];
			let mut n = signal.unsigned_abs();
			let mut i = digits.len();
			loop {
				i -= 1;
				digits[i] = b'0' + (n % 10) as u8;
				n /= 10;
				if n == 0 || i == 0 {
					break;
				}
			}
			libc::write(2, digits[i..].as_ptr().cast(), digits.len() - i);
			libc::write(2, b")".as_ptr().cast(), 1);
			use crate::inspect::recorder::last;
			let say = |text: &[u8]| {
				libc::write(2, text.as_ptr().cast(), text.len());
			};
			if let Some(command) = last::COMMAND.get() {
				say(b" in PF_Cmd_");
				say(command);
				if let (Some(suite), Some(function)) = (last::SUITE.get(), last::FUNCTION.get()) {
					say(b", after ");
					say(suite);
					say(b"::");
					say(function);
				}
			}
			say(b"\n");
			libc::raise(signal);
		}
	}
}
