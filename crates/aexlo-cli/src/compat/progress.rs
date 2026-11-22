//! The progress line `aexlo compat` keeps on stderr while it runs, when
//! stderr is a terminal.

use std::io::{IsTerminal, Write};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anstyle::{AnsiColor, Style};

/// Redraws at most this often.
const EVERY: Duration = Duration::from_millis(80);
const BAR: usize = 30;
const SPINNER: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

/// `⠹ AV layer  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━  2310/5100  14s`, redrawn in
/// place, and cleared when done (or dropped).
pub struct Progress {
	on: bool,
	/// Colors too, unless `NO_COLOR` or `--color never` says not.
	color: bool,
	start: Instant,
	state: Mutex<State>,
}

struct State {
	phase: String,
	done: usize,
	total: usize,
	drawn: Option<Instant>,
	frame: usize,
}

impl Progress {
	/// Shown only when stderr is a terminal.
	pub fn new() -> Self {
		Self {
			on: std::io::stderr().is_terminal(),
			color: anstream::AutoStream::choice(&std::io::stderr()) != anstream::ColorChoice::Never,
			start: Instant::now(),
			state: Mutex::new(State {
				phase: String::new(),
				done: 0,
				total: 0,
				drawn: None,
				frame: 0,
			}),
		}
	}

	/// Start counting `total` steps of `phase`.
	pub fn phase(&self, phase: &str, total: usize) {
		let mut state = self.state.lock().expect("progress");
		state.phase = phase.to_string();
		state.done = 0;
		state.total = total;
		self.draw(&mut state, true);
	}

	/// One more step done.
	pub fn tick(&self) {
		let mut state = self.state.lock().expect("progress");
		state.done += 1;
		let last = state.done >= state.total;
		self.draw(&mut state, last);
	}

	fn draw(&self, state: &mut State, now: bool) {
		if !self.on || (!now && state.drawn.is_some_and(|at| at.elapsed() < EVERY)) {
			return;
		}
		state.drawn = Some(Instant::now());
		state.frame = (state.frame + 1) % SPINNER.len();
		let filled = (state.done * BAR).checked_div(state.total).unwrap_or(0).min(BAR);
		let (cyan, dim) = match self.color {
			true => (
				Style::new().fg_color(Some(AnsiColor::Cyan.into())),
				Style::new().dimmed(),
			),
			false => (Style::new(), Style::new()),
		};
		let line = format!(
			"\r\x1b[2K{cyan}{}{cyan:#} {:<10} {cyan}{}{cyan:#}{dim}{}{dim:#}  {}/{}  {dim}{}s{dim:#}",
			SPINNER[state.frame],
			state.phase,
			"━".repeat(filled),
			(if self.color { "━" } else { "─" }).repeat(BAR - filled),
			state.done,
			state.total,
			self.start.elapsed().as_secs(),
		);
		// Straight to the terminal: the line is cleared with an escape
		// either way.
		let mut stderr = std::io::stderr().lock();
		let _ = stderr.write_all(line.as_bytes());
		let _ = stderr.flush();
	}

	/// Clear the line.
	pub fn finish(&self) {
		if self.on {
			let mut stderr = std::io::stderr().lock();
			let _ = stderr.write_all(b"\r\x1b[2K");
			let _ = stderr.flush();
		}
	}
}

impl Drop for Progress {
	fn drop(&mut self) {
		self.finish();
	}
}
