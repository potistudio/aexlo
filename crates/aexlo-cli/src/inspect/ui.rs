//! What `aexlo inspect` draws and how it reacts to keys. Holds no plugin
//! state of its own: everything comes from [`Snapshot`]s, the [`Recorder`]
//! and memory reads the engine sends back.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Cell, Clear, Paragraph, Row, Table, TableState, Tabs, Wrap};

use super::command::{self, Command, HELP, MemTarget};
use super::diagnose::{self, Tone};
use super::engine::Snapshot;
use super::recorder::{Kind, Recorder, fmt_duration};

/// Bytes the engine reads around the memory cursor at a time.
pub const MEMORY_WINDOW: usize = 64 * 1024;
const HEX_ROW: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
	Trace,
	Calls,
	Params,
	State,
	Memory,
	Output,
	Log,
}

const TABS: [(Tab, &str); 7] = [
	(Tab::Trace, "Trace"),
	(Tab::Calls, "Calls"),
	(Tab::Params, "Params"),
	(Tab::State, "State"),
	(Tab::Memory, "Memory"),
	(Tab::Output, "Output"),
	(Tab::Log, "Log"),
];

/// What the UI asks of the engine.
pub enum Action {
	Run(String, Command),
	Watch { address: usize, base: usize },
	Quit,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Level {
	Info,
	Ok,
	Error,
	Output,
}

struct Message {
	level: Level,
	text: String,
}

/// The bytes shown in the Memory tab.
struct MemoryView {
	address: usize,
	base: usize,
	bytes: Option<Vec<u8>>,
	/// Offsets (from `base`) the last command changed.
	changed: Vec<bool>,
}

pub struct App {
	pub recorder: Recorder,
	tab: Tab,
	prompt: Option<String>,
	history: Vec<String>,
	history_pos: Option<usize>,
	log: VecDeque<Message>,
	pub snapshot: Option<Snapshot>,
	busy: Option<(String, Instant)>,
	filter: String,
	/// Trace lines scrolled up from the bottom, for the view.
	trace_back: usize,
	/// The focused trace line, counted up from the bottom (0: the newest,
	/// following new entries).
	trace_sel: usize,
	/// Whether the diagnosis of the focused line is open.
	focus: bool,
	/// The diagnosis shown, for `(seq, generation)`, so it is not rebuilt
	/// every frame.
	diagnosis: Option<((u64, u64), String, Vec<Line<'static>>)>,
	diag_scroll: u16,
	calls: TableState,
	params: TableState,
	log_back: usize,
	mem_sel: usize,
	mem_cursor: usize,
	memory: Option<MemoryView>,
	help: bool,
	pub quit: bool,
}

impl App {
	pub fn new(recorder: Recorder) -> Self {
		let mut app = Self {
			recorder,
			tab: Tab::Trace,
			prompt: None,
			history: Vec::new(),
			history_pos: None,
			log: VecDeque::new(),
			snapshot: None,
			busy: None,
			filter: String::new(),
			trace_back: 0,
			trace_sel: 0,
			focus: false,
			diagnosis: None,
			diag_scroll: 0,
			calls: TableState::default().with_selected(0),
			params: TableState::default().with_selected(0),
			log_back: 0,
			mem_sel: 0,
			mem_cursor: 0,
			memory: None,
			help: false,
			quit: false,
		};
		app.info("loading the plugin... (`:` for a command, `?` for help)");
		app
	}

	fn push(&mut self, level: Level, text: impl Into<String>) {
		for line in text.into().lines() {
			if self.log.len() >= 10_000 {
				self.log.pop_front();
			}
			self.log.push_back(Message {
				level,
				text: line.to_string(),
			});
		}
	}

	pub fn info(&mut self, text: impl Into<String>) {
		self.push(Level::Info, text);
	}

	pub fn error(&mut self, text: impl Into<String>) {
		self.push(Level::Error, text);
	}

	pub fn output(&mut self, lines: Vec<String>) {
		for line in lines {
			self.push(Level::Output, line);
		}
	}

	pub fn is_busy(&self) -> bool {
		self.busy.is_some()
	}

	/// A command finished on the engine.
	pub fn finished(&mut self, result: Result<(String, Duration), String>, snapshot: Option<Snapshot>) {
		let line = self.busy.take().map(|(line, _)| line).unwrap_or_default();
		match result {
			Ok((message, elapsed)) => {
				let message = if message.is_empty() { "ok".to_string() } else { message };
				self.push(Level::Ok, format!("{line}  ✓ {}", fmt_duration(elapsed)));
				self.push(Level::Info, message);
			}
			Err(err) => self.push(Level::Error, format!("{line}  ✗ {err}")),
		}
		if let Some(snapshot) = snapshot {
			self.snapshot = Some(snapshot);
		}
	}

	/// The engine read memory for the Memory tab. `fresh` is set when a
	/// command ran since the last read, so differences are its doing.
	pub fn memory(&mut self, address: usize, base: usize, bytes: Option<Vec<u8>>, fresh: bool) {
		let changed = match (&self.memory, &bytes) {
			(Some(view), Some(new)) if view.address == address && view.base == base => match (&view.bytes, fresh) {
				(Some(old), true) => (0..new.len()).map(|i| old.get(i) != new.get(i)).collect(),
				_ => view.changed.clone(),
			},
			_ => Vec::new(),
		};
		self.memory = Some(MemoryView {
			address,
			base,
			bytes,
			changed,
		});
	}

	/// The allocation the Memory tab shows, and the window around the cursor.
	pub fn watched(&self) -> Option<(usize, usize)> {
		let allocation = self.snapshot.as_ref()?.allocations.get(self.mem_sel)?;
		let base = self.mem_cursor / MEMORY_WINDOW * MEMORY_WINDOW;
		Some((allocation.address, base))
	}

	fn watch_action(&self) -> Option<Action> {
		let (address, base) = self.watched()?;
		let current = self.memory.as_ref().map(|m| (m.address, m.base));
		(current != Some((address, base))).then_some(Action::Watch { address, base })
	}

	/// After a snapshot arrives: keep the Memory tab on a live allocation.
	pub fn refresh_watch(&mut self) -> Option<Action> {
		let count = self.snapshot.as_ref().map_or(0, |s| s.allocations.len());
		if self.mem_sel >= count {
			self.mem_sel = count.saturating_sub(1);
			self.mem_cursor = 0;
		}
		self.watch_action()
	}

	fn submit(&mut self, line: String) -> Option<Action> {
		let line = line.trim().to_string();
		if line.is_empty() {
			return None;
		}
		if self.history.last() != Some(&line) {
			self.history.push(line.clone());
		}
		let command = match command::parse(&line) {
			Ok(command) => command,
			Err(err) => {
				self.error(format!("{line}  ✗ {err:#}"));
				return None;
			}
		};
		match command {
			Command::Help => {
				self.help = true;
				None
			}
			Command::Quit => Some(Action::Quit),
			Command::Clear => {
				self.recorder.lock().clear();
				self.trace_back = 0;
				self.trace_sel = 0;
				self.info("trace cleared");
				None
			}
			Command::Find(text) => {
				self.filter = text;
				self.trace_back = 0;
				self.trace_sel = 0;
				self.calls.select(Some(0));
				None
			}
			Command::Mem(target) => self.select_memory(target),
			Command::Report => {
				self.info("report prints the analysis in --exec/--script; here the tabs show it");
				None
			}
			Command::Focus(query) => {
				self.focus_on(&query);
				None
			}
			command => {
				if let Some((running, _)) = &self.busy {
					self.error(format!("{line}  ✗ still running `{running}`"));
					return None;
				}
				self.busy = Some((line.clone(), Instant::now()));
				Some(Action::Run(line, command))
			}
		}
	}

	/// Focus the trace entry `query` names (see [`diagnose::find`]).
	fn focus_on(&mut self, query: &str) {
		let rec = self.recorder.lock();
		let found = diagnose::find(&rec, query).map(|pos| rec.trace.len() - 1 - pos);
		drop(rec);
		match found {
			Some(from_bottom) => {
				self.filter.clear();
				self.trace_sel = from_bottom;
				self.tab = Tab::Trace;
				self.focus = true;
			}
			None => self.error(format!("nothing in the trace matches '{query}'")),
		}
	}

	/// Focus the newest call of the function selected in the Calls tab.
	fn focus_selected_function(&mut self) {
		let rec = self.recorder.lock();
		let calls = sorted_calls(&rec, &self.filter);
		let picked = self.calls.selected().and_then(|i| calls.get(i).copied());
		drop(rec);
		if let Some((suite, function)) = picked {
			self.filter = format!("{suite}::{function}");
			self.trace_sel = 0;
			self.trace_back = 0;
			self.tab = Tab::Trace;
			self.focus = true;
		}
	}

	fn select_memory(&mut self, target: MemTarget) -> Option<Action> {
		let snapshot = self.snapshot.as_ref()?;
		let address = match target {
			MemTarget::Address(address) => Some(address),
			MemTarget::Global => snapshot.state.global_data,
			MemTarget::Sequence => snapshot.state.sequence_data,
		};
		let found = address.and_then(|a| snapshot.allocations.iter().position(|l| l.address == a));
		match found {
			Some(index) => {
				self.mem_sel = index;
				self.mem_cursor = 0;
				self.tab = Tab::Memory;
				self.watch_action()
			}
			None => {
				self.error("no live allocation there (see the Memory tab)");
				None
			}
		}
	}

	pub fn key(&mut self, key: KeyEvent) -> Option<Action> {
		if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
			return Some(Action::Quit);
		}
		if self.help {
			self.help = false;
			return None;
		}
		if let Some(prompt) = &mut self.prompt {
			match key.code {
				KeyCode::Enter => {
					let line = self.prompt.take().unwrap_or_default();
					self.history_pos = None;
					return self.submit(line);
				}
				KeyCode::Esc => {
					self.prompt = None;
					self.history_pos = None;
				}
				KeyCode::Backspace => {
					if prompt.pop().is_none() {
						self.prompt = None;
					}
				}
				KeyCode::Up | KeyCode::Down if !self.history.is_empty() => {
					let last = self.history.len() - 1;
					let pos = match (key.code, self.history_pos) {
						(KeyCode::Up, None) => Some(last),
						(KeyCode::Up, Some(p)) => Some(p.saturating_sub(1)),
						(KeyCode::Down, Some(p)) if p < last => Some(p + 1),
						_ => None,
					};
					self.history_pos = pos;
					*prompt = pos.map(|p| self.history[p].clone()).unwrap_or_default();
				}
				KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => prompt.clear(),
				KeyCode::Char(c) => prompt.push(c),
				_ => {}
			}
			return None;
		}

		let page = 20;
		match key.code {
			KeyCode::Enter => match self.tab {
				Tab::Params => return self.scroll_key(key.code, page),
				Tab::Trace => self.focus = !self.focus,
				Tab::Calls => self.focus_selected_function(),
				_ => self.prompt = Some(String::new()),
			},
			KeyCode::Char('d') if self.tab == Tab::Trace => self.focus = !self.focus,
			KeyCode::Esc if self.focus => self.focus = false,
			KeyCode::Char('J') if self.tab == Tab::Trace && self.focus => {
				self.diag_scroll = self.diag_scroll.saturating_add(3)
			}
			KeyCode::Char('K') if self.tab == Tab::Trace && self.focus => {
				self.diag_scroll = self.diag_scroll.saturating_sub(3)
			}
			KeyCode::Char(':') => self.prompt = Some(String::new()),
			KeyCode::Char('f') => self.prompt = Some("focus ".into()),
			KeyCode::Char('/') => self.prompt = Some("find ".into()),
			KeyCode::Char('?') => self.help = true,
			KeyCode::Char('q') => return Some(Action::Quit),
			KeyCode::Char('r') => return self.submit("render".into()),
			KeyCode::Char('a') => return self.submit("about".into()),
			KeyCode::Tab => self.cycle(1),
			KeyCode::BackTab => self.cycle(TABS.len() - 1),
			KeyCode::Char(c @ '1'..='7') => self.tab = TABS[c as usize - '1' as usize].0,
			_ => return self.scroll_key(key.code, page),
		}
		None
	}

	fn cycle(&mut self, by: usize) {
		let index = TABS.iter().position(|(t, _)| *t == self.tab).unwrap_or(0);
		self.tab = TABS[(index + by) % TABS.len()].0;
	}

	fn scroll_key(&mut self, code: KeyCode, page: usize) -> Option<Action> {
		// Positive: towards the end.
		let delta: isize = match code {
			KeyCode::Down | KeyCode::Char('j') => 1,
			KeyCode::Up | KeyCode::Char('k') => -1,
			KeyCode::PageDown | KeyCode::Char(' ') => page as isize,
			KeyCode::PageUp => -(page as isize),
			KeyCode::Char('G') | KeyCode::End => isize::MAX / 2,
			KeyCode::Char('g') | KeyCode::Home => isize::MIN / 2,
			_ => 0,
		};
		match self.tab {
			Tab::Trace => self.trace_sel = apply(self.trace_sel, -delta, usize::MAX / 4),
			Tab::Log => self.log_back = apply(self.log_back, -delta, self.log.len()),
			Tab::Calls => {
				let n = self.recorder.lock().calls.len();
				move_selection(&mut self.calls, delta, n);
			}
			Tab::Params => {
				let n = self.snapshot.as_ref().map_or(0, |s| s.params.len());
				if code == KeyCode::Enter {
					let row = self.snapshot.as_ref().and_then(|s| s.params.get(self.selected_param()));
					match row {
						Some(row) if row.kind == "Button" => return self.submit(format!("press {}", row.index)),
						Some(row) => self.prompt = Some(format!("set {} ", row.index)),
						None => {}
					}
					return None;
				}
				move_selection(&mut self.params, delta, n);
			}
			Tab::Memory => return self.memory_key(code),
			Tab::State | Tab::Output => {}
		}
		None
	}

	fn selected_param(&self) -> usize {
		self.params.selected().unwrap_or(0)
	}

	fn memory_key(&mut self, code: KeyCode) -> Option<Action> {
		let snapshot = self.snapshot.as_ref()?;
		let count = snapshot.allocations.len();
		let size = snapshot
			.allocations
			.get(self.mem_sel)
			.map_or(0, |a| self.memory_len(a.bytes));
		let last = size.saturating_sub(1) as isize;
		let cursor = self.mem_cursor as isize;
		let page = (HEX_ROW * 16) as isize;
		match code {
			KeyCode::Char('[') | KeyCode::Char('K') if self.mem_sel > 0 => {
				self.mem_sel -= 1;
				self.mem_cursor = 0;
			}
			KeyCode::Char(']') | KeyCode::Char('J') if self.mem_sel + 1 < count => {
				self.mem_sel += 1;
				self.mem_cursor = 0;
			}
			KeyCode::Left | KeyCode::Char('h') => self.mem_cursor = (cursor - 1).clamp(0, last) as usize,
			KeyCode::Right | KeyCode::Char('l') => self.mem_cursor = (cursor + 1).clamp(0, last) as usize,
			KeyCode::Up | KeyCode::Char('k') => self.mem_cursor = (cursor - HEX_ROW as isize).clamp(0, last) as usize,
			KeyCode::Down | KeyCode::Char('j') => self.mem_cursor = (cursor + HEX_ROW as isize).clamp(0, last) as usize,
			KeyCode::PageUp => self.mem_cursor = (cursor - page).clamp(0, last) as usize,
			KeyCode::PageDown | KeyCode::Char(' ') => self.mem_cursor = (cursor + page).clamp(0, last) as usize,
			KeyCode::Char('g') | KeyCode::Home => self.mem_cursor = 0,
			KeyCode::Char('G') | KeyCode::End => self.mem_cursor = last.max(0) as usize,
			_ => {}
		}
		self.watch_action()
	}

	/// The size of the allocation as last read (handles may have grown).
	fn memory_len(&self, recorded: usize) -> usize {
		match &self.memory {
			Some(view) if view.bytes.is_some() && self.watched().map(|w| w.0) == Some(view.address) => {
				let read = view.bytes.as_ref().map_or(0, Vec::len);
				if read < MEMORY_WINDOW {
					view.base + read
				} else {
					recorded.max(view.base + read)
				}
			}
			_ => recorded,
		}
	}

	//==== Drawing ======================================================

	pub fn draw(&mut self, frame: &mut Frame) {
		let [header, tabs, body, status, prompt] = Layout::vertical([
			Constraint::Length(1),
			Constraint::Length(1),
			Constraint::Min(3),
			Constraint::Length(1),
			Constraint::Length(1),
		])
		.areas(frame.area());

		frame.render_widget(Paragraph::new(self.header_line()), header);
		let counts = self.tab_counts();
		let titles = TABS.iter().enumerate().map(|(i, (tab, name))| {
			let mut spans = vec![Span::raw(format!("{} ", i + 1)).dark_gray(), Span::raw(*name)];
			for (_, count, style) in counts.iter().filter(|(t, ..)| t == tab) {
				spans.push(Span::styled(format!(" {count}"), *style));
			}
			Line::from(spans)
		});
		let selected = TABS.iter().position(|(t, _)| *t == self.tab).unwrap_or(0);
		frame.render_widget(
			Tabs::new(titles)
				.select(selected)
				.highlight_style(Style::new().reversed().bold())
				.divider(Span::raw("│").dark_gray()),
			tabs,
		);

		match self.tab {
			Tab::Trace => self.draw_trace(frame, body),
			Tab::Calls => self.draw_calls(frame, body),
			Tab::Params => self.draw_params(frame, body),
			Tab::State => self.draw_state(frame, body),
			Tab::Memory => self.draw_memory(frame, body),
			Tab::Output => self.draw_output(frame, body),
			Tab::Log => self.draw_log(frame, body),
		}

		frame.render_widget(Paragraph::new(self.status_line()), status);
		let prompt_line = match &self.prompt {
			Some(text) => Line::from(vec![Span::raw(":").bold(), Span::raw(text.clone()), Span::raw("█")]),
			None => hints(),
		};
		frame.render_widget(Paragraph::new(prompt_line), prompt);

		if self.help {
			let area = centered(frame.area(), 96, 36);
			frame.render_widget(Clear, area);
			frame.render_widget(
				Paragraph::new(format!("{HELP}\nKEYS\n  Memory: ←→↑↓/hjkl move  [ ] (K J) allocation  PgUp/PgDn  g/G\n  Trace: j/k pick a line, Enter (or d) diagnoses it, J/K scroll the diagnosis, Esc closes\n  Calls: Enter focuses the newest call of that function\n  Params: Enter presses a button, or fills `set <index> `\n\n(any key closes)"))
					.block(panel(" help ")),
				area,
			);
		}
	}

	fn header_line(&self) -> Line<'static> {
		let Some(s) = &self.snapshot else {
			return Line::from(" aexlo inspect".bold());
		};
		let name = s
			.path
			.file_name()
			.map(|n| n.to_string_lossy().into_owned())
			.unwrap_or_default();
		let (iw, ih, depth) = s.input;
		let (ow, oh, _) = s.output;
		let strict = super::engine::describe_strict(s.strict);
		let strict_style = if strict == "off" {
			Style::new()
		} else {
			Style::new().yellow()
		};
		let mut spans = vec![
			Span::raw(" aexlo inspect ").bold(),
			Span::raw(name).cyan().bold(),
			Span::raw("  "),
		];
		let fields = [
			("", format!("{iw}x{ih} → {ow}x{oh}"), Style::new()),
			("", format!("{} bpc", depth.bits()), Style::new()),
			("t ", format!("{}/{}", s.time.value(), s.time.scale()), Style::new()),
			("level ", level_name(s.level).to_string(), Style::new()),
			("strict ", strict, strict_style),
			(
				"iterate ",
				(if s.parallel_iterate { "parallel" } else { "serial" }).to_string(),
				Style::new(),
			),
		];
		for (i, (label, value, style)) in fields.into_iter().enumerate() {
			if i > 0 {
				spans.push(Span::raw(" · ").dark_gray());
			}
			spans.push(Span::raw(label).dark_gray());
			spans.push(Span::styled(value, style));
		}
		Line::from(spans)
	}

	/// The count each tab shows next to its name, and how.
	fn tab_counts(&self) -> Vec<(Tab, String, Style)> {
		let rec = self.recorder.lock();
		let errors: u64 = rec.commands.values().map(|c| c.errors).sum();
		let mut counts = vec![
			(
				Tab::Trace,
				compact(rec.trace.len() as u64 + rec.dropped),
				Style::new().dark_gray(),
			),
			(Tab::Calls, rec.calls.len().to_string(), Style::new().dark_gray()),
		];
		drop(rec);
		if let Some(s) = &self.snapshot {
			counts.push((Tab::Params, s.params.len().to_string(), Style::new().dark_gray()));
			if !s.findings.is_empty() {
				counts.push((Tab::State, format!("⚠{}", s.findings.len()), Style::new().yellow()));
			}
			counts.push((Tab::Memory, s.allocations.len().to_string(), Style::new().dark_gray()));
		}
		if errors > 0 {
			counts.push((Tab::Calls, format!("✗{errors}"), Style::new().red()));
		}
		let failed = self.log.iter().filter(|m| m.level == Level::Error).count();
		if failed > 0 {
			counts.push((Tab::Log, format!("✗{failed}"), Style::new().red()));
		}
		counts
	}

	fn status_line(&self) -> Line<'static> {
		if let Some((line, started)) = &self.busy {
			let running = self.recorder.lock().running().join(" › ");
			return Line::from(vec![
				Span::raw(format!(" {} running ", spinner(started.elapsed())))
					.yellow()
					.bold(),
				Span::raw(format!("{line}  {}  ", fmt_duration(started.elapsed()))),
				Span::raw(if running.is_empty() {
					String::new()
				} else {
					format!("PF_Cmd_{running}")
				})
				.yellow(),
			]);
		}
		match self.log.iter().rev().find(|m| m.level != Level::Output) {
			Some(m) => Line::from(Span::styled(format!(" {}", m.text), level_style(m.level))),
			None => Line::from(""),
		}
	}

	fn draw_trace(&mut self, frame: &mut Frame, area: Rect) {
		let (area, focus_area) = if self.focus {
			let [top, bottom] = Layout::vertical([Constraint::Percentage(40), Constraint::Percentage(60)]).areas(area);
			(top, Some(bottom))
		} else {
			(area, None)
		};
		let rec = self.recorder.lock();
		let height = (area.height.saturating_sub(2) as usize).max(1);
		let filter = self.filter.to_ascii_lowercase();
		let matches =
			|e: &super::recorder::Entry| filter.is_empty() || e.describe().to_ascii_lowercase().contains(&filter);

		// Walk back from the end, only as far as the view and the focus need.
		let want = self.trace_sel.max(self.trace_back + height - 1) + 1;
		let mut positions = Vec::with_capacity(want.min(4096));
		for (pos, entry) in rec.trace.iter().enumerate().rev() {
			if matches(entry) {
				positions.push(pos);
				if positions.len() == want {
					break;
				}
			}
		}
		self.trace_sel = self.trace_sel.min(positions.len().saturating_sub(1));
		if self.trace_sel < self.trace_back {
			self.trace_back = self.trace_sel;
		} else if self.trace_sel >= self.trace_back + height {
			self.trace_back = self.trace_sel + 1 - height;
		}
		let selected = positions.get(self.trace_sel).copied();
		let visible = &positions[self.trace_back.min(positions.len())..(self.trace_back + height).min(positions.len())];

		let lines: Vec<Line> = visible
			.iter()
			.rev()
			.map(|&pos| {
				let e = &rec.trace[pos];
				let mut style = match &e.kind {
					Kind::Step(_) => Style::new().magenta().bold(),
					Kind::Begin(_) => Style::new().cyan().bold(),
					Kind::End { error: Some(_), .. } => Style::new().red().bold(),
					Kind::End { .. } => Style::new().cyan(),
					Kind::Alloc { allocated: true, .. } => Style::new().green(),
					Kind::Alloc { .. } => Style::new().yellow(),
					Kind::Checkout { .. } => Style::new().blue(),
					Kind::Call { .. } => Style::new(),
				};
				let focused = Some(pos) == selected;
				if focused {
					style = style.reversed();
				}
				Line::from(vec![
					Span::raw(format!(
						"{}{:>7} {:>10.3}ms ",
						if focused { "▶" } else { " " },
						e.seq,
						e.at.as_secs_f64() * 1e3
					))
					.dark_gray(),
					Span::raw("│ ".repeat(e.depth)).dark_gray(),
					Span::styled(e.describe(), style),
				])
			})
			.collect();
		let follow = if self.trace_sel == 0 { "following" } else { "scrolled" };
		let mut title = format!(" trace: {} entries, {follow}", rec.trace.len());
		if rec.dropped > 0 {
			title += &format!(", {} oldest dropped", rec.dropped);
		}
		if !self.filter.is_empty() {
			title += &format!(", filter \"{}\"", self.filter);
		}
		if !self.focus {
			title += ", Enter: diagnose";
		}

		// The diagnosis of the focused line, rebuilt when it or the trace changes.
		if let (Some(focus_area), Some(pos)) = (focus_area, selected) {
			let key = (rec.trace[pos].seq, rec.generation);
			if self.diagnosis.as_ref().map(|(k, _, _)| *k) != Some(key) {
				if self.diagnosis.as_ref().map(|(k, _, _)| k.0) != Some(key.0) {
					self.diag_scroll = 0;
				}
				let d = diagnose::diagnose(&rec, pos, self.snapshot.as_ref());
				let lines = d
					.lines
					.iter()
					.map(|parts| {
						Line::from(
							parts
								.iter()
								.map(|(tone, text)| Span::styled(text.clone(), tone_style(*tone)))
								.collect::<Vec<_>>(),
						)
					})
					.collect();
				self.diagnosis = Some((key, d.title, lines));
			}
			drop(rec);
			if let Some((_, title, lines)) = &self.diagnosis {
				frame.render_widget(
					Paragraph::new(lines.clone())
						.scroll((self.diag_scroll, 0))
						.wrap(Wrap { trim: false })
						.block(
							panel(format!(" diagnosis: {title} "))
								.border_style(Style::new().yellow())
								.title_bottom(" J/K scroll · Esc close ".dark_gray()),
						),
					focus_area,
				);
			}
		} else {
			drop(rec);
			if let Some(focus_area) = focus_area {
				frame.render_widget(
					Paragraph::new("nothing to focus").block(panel(" diagnosis ")),
					focus_area,
				);
			}
		}
		frame.render_widget(Paragraph::new(lines).block(panel(title + " ")), area);
	}

	fn draw_calls(&mut self, frame: &mut Frame, area: Rect) {
		let rec = self.recorder.lock();
		let mut commands: Vec<_> = rec.commands.iter().collect();
		commands.sort_by_key(|(_, s)| std::cmp::Reverse(s.total));
		let filter = self.filter.to_ascii_lowercase();
		let calls: Vec<_> = sorted_calls(&rec, &self.filter)
			.into_iter()
			.map(|key| (key, &rec.calls[&key]))
			.collect();
		let step = rec.step;

		let command_rows = commands.len().min(10) as u16 + 3;
		let [top, bottom] = Layout::vertical([Constraint::Length(command_rows), Constraint::Min(3)]).areas(area);

		let slowest = commands.first().map_or(0.0, |(_, s)| s.total.as_secs_f64());
		let rows = commands.iter().map(|(name, s)| {
			let avg = s.total / s.count.max(1) as u32;
			let errors = if s.errors > 0 {
				s.errors.to_string().red().bold()
			} else {
				"0".dark_gray()
			};
			let name_style = if s.errors > 0 { Style::new().red() } else { Style::new() };
			Row::new(vec![
				Cell::from(Line::from(vec![
					Span::raw("PF_Cmd_").dark_gray(),
					Span::styled(name.to_string(), name_style.bold()),
				])),
				Cell::from(s.count.to_string()),
				Cell::from(errors),
				Cell::from(fmt_duration(s.last)),
				Cell::from(fmt_duration(avg)),
				Cell::from(fmt_duration(s.max)),
				Cell::from(fmt_duration(s.total)),
				Cell::from(bar(s.total.as_secs_f64(), slowest, 16)),
			])
		});
		frame.render_widget(
			Table::new(
				rows,
				[
					Constraint::Min(28),
					Constraint::Length(7),
					Constraint::Length(6),
					Constraint::Length(10),
					Constraint::Length(10),
					Constraint::Length(10),
					Constraint::Length(10),
					Constraint::Length(16),
				],
			)
			.header(
				Row::new([
					"command",
					"count",
					"errors",
					"last",
					"avg",
					"max",
					"total",
					"share of time",
				])
				.bold(),
			)
			.block(panel(" commands ")),
			top,
		);

		let total: u64 = calls.iter().map(|(_, s)| s.count).sum();
		let most = calls.first().map_or(0, |(_, s)| s.count);
		let rows = calls.iter().map(|((suite, function), s)| {
			let last = if s.step == step { s.in_step } else { 0 };
			// Bright when the latest command called it.
			let (function, last) = if last > 0 {
				(function.to_string().bold(), last.to_string().cyan())
			} else {
				(function.to_string().into(), "0".dark_gray())
			};
			Row::new(vec![
				Cell::from(suite.to_string()).dark_gray(),
				Cell::from(function),
				Cell::from(s.count.to_string()),
				Cell::from(bar(s.count as f64, most as f64, 10)),
				Cell::from(last),
				Cell::from(s.commands.join(" ")).dark_gray(),
			])
		});
		let title = format!(
			" suite calls: {} functions, {total} calls{} (last = during the latest command) ",
			calls.len(),
			if filter.is_empty() {
				String::new()
			} else {
				format!(", filter \"{}\"", self.filter)
			}
		);
		frame.render_stateful_widget(
			Table::new(
				rows,
				[
					Constraint::Percentage(22),
					Constraint::Percentage(26),
					Constraint::Length(8),
					Constraint::Length(10),
					Constraint::Length(7),
					Constraint::Min(10),
				],
			)
			.header(Row::new(["suite", "function", "total", "", "last", "from"]).bold())
			.row_highlight_style(Style::new().reversed())
			.block(panel(title)),
			bottom,
			&mut self.calls,
		);
	}

	fn draw_params(&mut self, frame: &mut Frame, area: Rect) {
		let Some(s) = &self.snapshot else { return };
		let rows = s.params.iter().map(|p| {
			let mut flags = String::new();
			if p.hidden {
				flags += "hidden ";
			}
			if p.disabled {
				flags += "disabled";
			}
			let style = if p.hidden || p.disabled {
				Style::new().dark_gray()
			} else {
				Style::new()
			};
			Row::new(vec![
				Cell::from(p.index.to_string()),
				Cell::from(p.name.clone()).bold(),
				Cell::from(p.kind.clone()).dark_gray(),
				Cell::from(p.value.clone()).cyan(),
				Cell::from(flags),
				Cell::from(p.detail.clone()).dark_gray(),
			])
			.style(style)
		});
		frame.render_stateful_widget(
			Table::new(
				rows,
				[
					Constraint::Length(4),
					Constraint::Percentage(22),
					Constraint::Length(13),
					Constraint::Percentage(20),
					Constraint::Length(16),
					Constraint::Min(10),
				],
			)
			.header(Row::new(["#", "name", "kind", "value", "state", "range / choices / flags"]).bold())
			.row_highlight_style(Style::new().reversed())
			.block(panel(format!(" {} parameters (Enter: set / press) ", s.params.len()))),
			area,
			&mut self.params,
		);
	}

	fn draw_state(&self, frame: &mut Frame, area: Rect) {
		let Some(s) = &self.snapshot else { return };
		let st = &s.state;
		let handle = |h: Option<usize>| -> Vec<Span<'static>> {
			match h {
				Some(h) => vec![Span::raw(format!("{h:#x}")).cyan()],
				None => vec![Span::raw("null").dark_gray()],
			}
		};
		let text = |t: String| vec![Span::raw(t)];
		let unset = |t: &str| vec![Span::raw(t.to_string()).dark_gray()];
		let kv = |k: &str, v: Vec<Span<'static>>| {
			let mut spans = vec![Span::raw(format!("  {k:<22}")).dark_gray()];
			spans.extend(v);
			Line::from(spans)
		};
		let heading = |t: &str| Line::from(Span::raw(t.to_string()).cyan().bold());
		let mut lines = vec![
			heading("plugin"),
			kv("path", text(s.path.display().to_string())),
			kv("out_flags", flag_spans(&format!("{:?}", s.out_flags))),
			kv("out_flags2", flag_spans(&format!("{:?}", s.out_flags2))),
			kv("render paths", {
				let mut paths = vec![Span::raw("legacy").green()];
				for name in [(s.smart, "smart"), (s.gpu, "gpu")]
					.iter()
					.filter(|(on, _)| *on)
					.map(|(_, name)| *name)
				{
					paths.push(Span::raw(", ").dark_gray());
					paths.push(Span::raw(name).green());
				}
				paths
			}),
			kv("custom UI", yes(s.custom_ui)),
			kv(
				"options button",
				s.options_button.clone().map_or_else(|| unset("(unset)"), text),
			),
			kv(
				"pixel formats",
				if s.pixel_formats.is_empty() {
					unset("(none registered)")
				} else {
					text(s.pixel_formats.join(", "))
				},
			),
			Line::from(""),
			heading("PF_InData (next command)"),
			kv("global_data", handle(st.global_data)),
			kv("sequence_data", handle(st.sequence_data)),
			kv(
				"current_time",
				text(format!(
					"{} / time_step {} / time_scale {}  (frame {:.2})",
					st.current_time,
					st.time_step,
					st.time_scale,
					st.current_time as f64 / st.time_step.max(1) as f64
				)),
			),
			kv(
				"input",
				text(format!("{}x{} {}-bit", s.input.0, s.input.1, s.input.2.bits())),
			),
			kv(
				"output",
				text(format!("{}x{} {}-bit", s.output.0, s.output.1, s.output.2.bits())),
			),
			Line::from(""),
			heading("PF_OutData (as the last command left it)"),
			kv("out_flags", flag_spans(&format!("{:?}", st.last_out_flags))),
			kv("out_flags2", flag_spans(&format!("{:?}", st.last_out_flags2))),
			kv(
				"width x height",
				text(format!("{} x {}", st.last_out_size.0, st.last_out_size.1)),
			),
			kv("origin", text(format!("{}, {}", st.last_origin.0, st.last_origin.1))),
			kv(
				"return_msg",
				if st.last_message.is_empty() {
					unset("(empty)")
				} else {
					text(st.last_message.replace(['\r', '\n'], " ⏎ "))
				},
			),
			Line::from(""),
			heading("host"),
			kv("GPU device set up", yes(st.gpu_device_ready)),
			kv("torn down", yes(st.torn_down)),
			kv("live allocations", text(s.allocations.len().to_string())),
			kv("allocated so far", text(self.memory_totals())),
			kv("strict", text(super::engine::describe_strict(s.strict).to_string())),
		];
		if let Some(unwritten) = s.unwritten {
			let count = Span::raw(unwritten.to_string());
			lines.push(kv(
				"unwritten pixels",
				vec![if unwritten > 0 { count.yellow() } else { count.green() }],
			));
		}
		lines.push(Line::from(""));
		lines.push(Line::from(vec![
			Span::raw("strict findings ").cyan().bold(),
			Span::raw(s.findings.len().to_string()).dark_gray(),
		]));
		if s.findings.is_empty() {
			lines.push(Line::from("  none".dark_gray()));
		}
		for finding in &s.findings {
			lines.push(Line::from(vec![
				Span::raw("  ⚠ ").yellow(),
				Span::raw(finding.clone()).red(),
			]));
		}
		frame.render_widget(
			Paragraph::new(lines).wrap(Wrap { trim: false }).block(panel(" state ")),
			area,
		);
	}

	fn memory_totals(&self) -> String {
		let rec = self.recorder.lock();
		let mut parts: Vec<String> = rec
			.memory
			.iter()
			.map(|(kind, (allocated, freed, bytes))| format!("{kind:?}: {allocated} allocs ({bytes} B), {freed} frees"))
			.collect();
		parts.sort();
		if parts.is_empty() {
			"nothing".into()
		} else {
			parts.join("; ")
		}
	}

	fn draw_memory(&self, frame: &mut Frame, area: Rect) {
		let Some(s) = &self.snapshot else { return };
		let [list, dump] = Layout::horizontal([Constraint::Length(58), Constraint::Min(40)]).areas(area);

		let rows = s.allocations.iter().enumerate().map(|(i, a)| {
			let label = if Some(a.address) == s.state.global_data {
				" global"
			} else if Some(a.address) == s.state.sequence_data {
				" sequence"
			} else {
				""
			};
			let kind = Line::from(vec![
				Span::raw(format!("{:?}", a.kind)).green(),
				Span::raw(label).yellow(),
			]);
			let style = if i == self.mem_sel {
				Style::new().reversed()
			} else {
				Style::new()
			};
			Row::new(vec![
				Cell::from(kind),
				Cell::from(format!("{:#x}", a.address)),
				Cell::from(a.bytes.to_string()).bold(),
				Cell::from(a.allocated_in).dark_gray(),
			])
			.style(style)
		});
		let title = if s.allocations.is_empty() {
			" no live allocations ".to_string()
		} else {
			format!(" {} live ([ ] to pick) ", s.allocations.len())
		};
		frame.render_widget(
			Table::new(
				rows,
				[
					Constraint::Length(15),
					Constraint::Length(14),
					Constraint::Length(8),
					Constraint::Min(16),
				],
			)
			.header(Row::new(["kind", "address", "bytes", "from"]).bold())
			.block(panel(title)),
			list,
		);

		let Some(view) = self
			.memory
			.as_ref()
			.filter(|v| Some((v.address, v.base)) == self.watched())
		else {
			frame.render_widget(Paragraph::new("").block(panel(" memory ")), dump);
			return;
		};
		let Some(bytes) = &view.bytes else {
			frame.render_widget(
				Paragraph::new("unreadable: no longer live").block(panel(" memory ")),
				dump,
			);
			return;
		};
		let [hex, inspector] = Layout::vertical([Constraint::Min(3), Constraint::Length(7)]).areas(dump);
		let height = hex.height.saturating_sub(2) as usize;
		let cursor = self.mem_cursor.saturating_sub(view.base);
		let cursor_row = cursor / HEX_ROW;
		let first_row = cursor_row.saturating_sub(height / 2);
		let rows = bytes.len().div_ceil(HEX_ROW);
		let mut lines = Vec::with_capacity(height);
		for row in first_row..rows.min(first_row + height) {
			let start = row * HEX_ROW;
			let chunk = &bytes[start..(start + HEX_ROW).min(bytes.len())];
			let mut spans = vec![Span::raw(format!("{:08x}  ", view.base + start)).dark_gray()];
			for (i, b) in chunk.iter().enumerate() {
				let offset = start + i;
				spans.push(Span::styled(format!("{b:02x}"), byte_style(view, offset, cursor, *b)));
				spans.push(Span::raw(if i == 7 { "  " } else { " " }));
			}
			spans.push(Span::raw("   ".repeat(HEX_ROW - chunk.len())));
			spans.push(Span::raw(" "));
			for (i, b) in chunk.iter().enumerate() {
				let c = if b.is_ascii_graphic() || *b == b' ' {
					*b as char
				} else {
					'·'
				};
				spans.push(Span::styled(c.to_string(), byte_style(view, start + i, cursor, *b)));
			}
			lines.push(Line::from(spans));
		}
		let changed = view.changed.iter().filter(|c| **c).count();
		let title = format!(
			" {:#x} +{:#x}: {} B read, {changed} changed by the last command ",
			view.address,
			view.base,
			bytes.len()
		);
		frame.render_widget(Paragraph::new(lines).block(panel(title)), hex);
		frame.render_widget(
			Paragraph::new(inspect_bytes(&bytes[cursor.min(bytes.len())..]))
				.block(panel(format!(" at +{:#x} ", self.mem_cursor))),
			inspector,
		);
	}

	fn draw_output(&self, frame: &mut Frame, area: Rect) {
		let Some(s) = &self.snapshot else { return };
		let [r, g, b, a] = s.mean;
		// The mean over black, as the output would show it.
		let channel = |c: f32| (c.clamp(0.0, 1.0) * a.clamp(0.0, 1.0) * 255.0).round() as u8;
		let mut title = vec![
			Span::raw(format!(" output {}x{}  mean ", s.output.0, s.output.1)),
			Span::raw("██").fg(Color::Rgb(channel(r), channel(g), channel(b))),
			Span::raw(format!(" rgba {r:.3} {g:.3} {b:.3} {a:.3} ")),
		];
		if let Some(u) = s.unwritten {
			let text = Span::raw(format!(" unwritten {u} "));
			title.push(if u > 0 { text.yellow() } else { text.dark_gray() });
		}
		let block = panel(Line::from(title));
		let inner = block.inner(area);
		frame.render_widget(block, area);
		let p = &s.preview;
		if p.width == 0 || inner.width == 0 || inner.height == 0 {
			return;
		}
		// Two pixels per cell, stacked with '▀'.
		let scale = (inner.width as f32 / p.width as f32).min(inner.height as f32 * 2.0 / p.height as f32);
		let cols = ((p.width as f32 * scale) as u16).clamp(1, inner.width);
		let rows = ((p.height as f32 * scale / 2.0) as u16).clamp(1, inner.height);
		let sample = |x: u16, y: u16| -> Color {
			let px = (x as u32 * p.width / cols as u32).min(p.width - 1);
			let py = (y as u32 * p.height / (rows as u32 * 2)).min(p.height - 1);
			let i = ((py * p.width + px) * 4) as usize;
			let a = p.rgba[i + 3] as u32;
			// Straight alpha over a checkerboard.
			let bg = if ((x / 2) + (y / 4)).is_multiple_of(2) { 60 } else { 90 };
			let mix = |c: u8| ((c as u32 * a + bg * (255 - a)) / 255) as u8;
			Color::Rgb(mix(p.rgba[i]), mix(p.rgba[i + 1]), mix(p.rgba[i + 2]))
		};
		let lines: Vec<Line> = (0..rows)
			.map(|row| {
				Line::from(
					(0..cols)
						.map(|col| {
							Span::styled("▀", Style::new().fg(sample(col, row * 2)).bg(sample(col, row * 2 + 1)))
						})
						.collect::<Vec<_>>(),
				)
			})
			.collect();
		frame.render_widget(Paragraph::new(lines), inner);
	}

	fn draw_log(&mut self, frame: &mut Frame, area: Rect) {
		let height = area.height.saturating_sub(2) as usize;
		self.log_back = self.log_back.min(self.log.len().saturating_sub(height));
		let end = self.log.len() - self.log_back;
		let start = end.saturating_sub(height);
		let lines: Vec<Line> = self
			.log
			.range(start..end)
			.map(|m| {
				let prefix = if m.level == Level::Output { "│ " } else { "" };
				Line::from(Span::styled(format!("{prefix}{}", m.text), level_style(m.level)))
			})
			.collect();
		frame.render_widget(
			Paragraph::new(lines).block(panel(" log: results, and what the plugin printed (│) ")),
			area,
		);
	}
}

/// The Calls tab's functions, busiest first, matching `filter`.
fn sorted_calls(rec: &super::recorder::Recording, filter: &str) -> Vec<(&'static str, &'static str)> {
	let filter = filter.to_ascii_lowercase();
	let mut calls: Vec<_> = rec
		.calls
		.iter()
		.filter(|((suite, function), _)| {
			filter.is_empty() || format!("{suite}::{function}").to_ascii_lowercase().contains(&filter)
		})
		.collect();
	calls.sort_by(|a, b| b.1.count.cmp(&a.1.count).then(a.0.cmp(b.0)));
	calls.into_iter().map(|(key, _)| *key).collect()
}

/// A bordered panel with rounded corners and a bold title.
fn panel<'a>(title: impl Into<Line<'a>>) -> Block<'a> {
	Block::bordered()
		.border_type(BorderType::Rounded)
		.border_style(Style::new().dark_gray())
		.title_style(Style::new().bold())
		.title(title)
}

/// `part` of `whole` as a bar `width` cells wide; anything at all shows.
fn bar(part: f64, whole: f64, width: usize) -> Line<'static> {
	let filled = if whole > 0.0 {
		((part / whole * width as f64).round() as usize).clamp(usize::from(part > 0.0), width)
	} else {
		0
	};
	Line::from(vec![
		Span::raw("━".repeat(filled)).cyan(),
		Span::raw("─".repeat(width - filled)).dark_gray(),
	])
}

/// The keys the prompt line offers when no command is being typed.
fn hints() -> Line<'static> {
	const KEYS: [(&str, &str); 10] = [
		(":", "command"),
		("/", "find"),
		("f", "focus"),
		("Enter", "diagnose"),
		("r", "render"),
		("Tab/1-7", "tabs"),
		("j/k", "move"),
		("?", "help"),
		("q", "quit"),
		("", ""),
	];
	let mut spans = vec![Span::raw(" ")];
	for (key, what) in KEYS.iter().filter(|(k, _)| !k.is_empty()) {
		spans.push(Span::raw(format!(" {key}")).cyan());
		spans.push(Span::raw(format!(" {what} ")).dark_gray());
	}
	Line::from(spans)
}

/// `1234567` -> `1.2M`, for the tab counts.
fn compact(n: u64) -> String {
	match n {
		0..1_000 => n.to_string(),
		1_000..1_000_000 => format!("{:.1}k", n as f64 / 1e3),
		_ => format!("{:.1}M", n as f64 / 1e6),
	}
}

/// A spinner frame for something that has run `elapsed`.
fn spinner(elapsed: Duration) -> char {
	const FRAMES: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
	FRAMES[(elapsed.as_millis() / 80) as usize % FRAMES.len()]
}

/// `--level`'s spelling of a level.
fn level_name(level: aexlo::ObserveLevel) -> &'static str {
	match level {
		aexlo::ObserveLevel::Arguments => "args",
		aexlo::ObserveLevel::Calls => "calls",
		aexlo::ObserveLevel::Commands => "commands",
		_ => "other",
	}
}

/// `OutFlags(A | B)` as the names in cyan between dim bars, or a dim
/// `(none)`.
fn flag_spans(debug: &str) -> Vec<Span<'static>> {
	let inner = debug
		.split_once('(')
		.map_or(debug, |(_, rest)| rest.trim_end_matches(')'));
	if inner.is_empty() || inner == "0x0" {
		return vec![Span::raw("(none)").dark_gray()];
	}
	let mut spans = Vec::new();
	for (i, name) in inner.split(" | ").enumerate() {
		if i > 0 {
			spans.push(Span::raw(" | ").dark_gray());
		}
		spans.push(Span::raw(name.to_string()).cyan());
	}
	spans
}

/// A flag's state: `true` in green, `false` dim.
fn yes(on: bool) -> Vec<Span<'static>> {
	vec![if on {
		Span::raw("true").green()
	} else {
		Span::raw("false").dark_gray()
	}]
}

fn tone_style(tone: Tone) -> Style {
	match tone {
		Tone::Heading => Style::new().bold().underlined(),
		Tone::Key => Style::new().dark_gray(),
		Tone::Plain => Style::new(),
		Tone::Good => Style::new().green(),
		Tone::Warn => Style::new().yellow(),
		Tone::Bad => Style::new().red().bold(),
		Tone::Dim => Style::new().dark_gray(),
	}
}

fn level_style(level: Level) -> Style {
	match level {
		Level::Info => Style::new(),
		Level::Ok => Style::new().green(),
		Level::Error => Style::new().red(),
		Level::Output => Style::new().dark_gray(),
	}
}

/// A byte in the hex dump: zeros dim, what the last command changed in
/// yellow, the cursor reversed.
fn byte_style(view: &MemoryView, offset: usize, cursor: usize, byte: u8) -> Style {
	let mut style = Style::new();
	if byte == 0 {
		style = style.fg(Color::DarkGray);
	}
	if view.changed.get(offset).copied().unwrap_or(false) {
		style = style.fg(Color::Yellow).add_modifier(Modifier::BOLD);
	}
	if offset == cursor {
		style = style.add_modifier(Modifier::REVERSED);
	}
	style
}

/// The bytes at the cursor read as little-endian numbers and a string.
fn inspect_bytes(bytes: &[u8]) -> Text<'static> {
	fn le<const N: usize>(bytes: &[u8]) -> Option<[u8; N]> {
		bytes.get(..N)?.try_into().ok()
	}
	let na = || "-".to_string();
	let u8v = bytes.first().map_or_else(na, |b| format!("{b} / {}", *b as i8));
	let u16v = le::<2>(bytes).map_or_else(na, |b| format!("{} / {}", u16::from_le_bytes(b), i16::from_le_bytes(b)));
	let u32v = le::<4>(bytes).map_or_else(na, |b| format!("{} / {}", u32::from_le_bytes(b), i32::from_le_bytes(b)));
	let f32v = le::<4>(bytes).map_or_else(na, |b| format!("{}", f32::from_le_bytes(b)));
	let fixed = le::<4>(bytes).map_or_else(na, |b| format!("{:.5}", i32::from_le_bytes(b) as f64 / 65536.0));
	let u64v = le::<8>(bytes).map_or_else(na, |b| {
		format!("{} / {:#x}", i64::from_le_bytes(b), u64::from_le_bytes(b))
	});
	let f64v = le::<8>(bytes).map_or_else(na, |b| format!("{}", f64::from_le_bytes(b)));
	let text: String = bytes
		.iter()
		.take(48)
		.take_while(|b| **b != 0)
		.map(|b| {
			if b.is_ascii_graphic() || *b == b' ' {
				*b as char
			} else {
				'·'
			}
		})
		.collect();
	let kv = |k: &str, v: String| Span::raw(format!("{k} {v:<26}"));
	Text::from(vec![
		Line::from(vec![kv("u8/i8  ", u8v), kv("u16/i16", u16v)]),
		Line::from(vec![kv("u32/i32", u32v), kv("f32    ", f32v)]),
		Line::from(vec![kv("i64/hex", u64v), kv("f64    ", f64v)]),
		Line::from(vec![kv("fixed  ", fixed), kv("str    ", format!("\"{text}\""))]),
	])
}

fn apply(value: usize, delta: isize, max: usize) -> usize {
	(value as isize).saturating_add(delta).clamp(0, max as isize) as usize
}

fn move_selection(state: &mut TableState, delta: isize, len: usize) {
	if len == 0 {
		state.select(None);
		return;
	}
	let current = state.selected().unwrap_or(0) as isize;
	state.select(Some(current.saturating_add(delta).clamp(0, len as isize - 1) as usize));
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
	let w = width.min(area.width);
	let h = height.min(area.height);
	Rect::new(area.x + (area.width - w) / 2, area.y + (area.height - h) / 2, w, h)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn marks_the_bytes_a_command_changed() {
		let mut app = App::new(Recorder::default());
		app.memory(0x10, 0, Some(vec![1, 2, 3]), false);
		app.memory(0x10, 0, Some(vec![1, 9, 3]), true);
		assert_eq!(app.memory.as_ref().unwrap().changed, [false, true, false]);
		// A re-read with no command in between keeps the marks.
		app.memory(0x10, 0, Some(vec![1, 9, 3]), false);
		assert_eq!(app.memory.as_ref().unwrap().changed, [false, true, false]);
		// Another allocation starts clean.
		app.memory(0x20, 0, Some(vec![1, 9, 3]), true);
		assert!(app.memory.as_ref().unwrap().changed.is_empty());
	}

	#[test]
	fn bars_counts_and_flags() {
		assert_eq!(bar(1.0, 2.0, 10).to_string(), "━━━━━─────");
		// Anything at all shows.
		assert_eq!(bar(0.001, 100.0, 4).to_string(), "━───");
		assert_eq!(bar(0.0, 0.0, 4).to_string(), "────");
		assert_eq!(compact(999), "999");
		assert_eq!(compact(12_345), "12.3k");
		assert_eq!(compact(2_000_000), "2.0M");
		let text = |spans: Vec<Span>| Line::from(spans).to_string();
		assert_eq!(text(flag_spans("OutFlags(A | B)")), "A | B");
		assert_eq!(text(flag_spans("OutFlags2(0x0)")), "(none)");
	}

	#[test]
	fn inspects_little_endian_values() {
		let text = inspect_bytes(&[0x00, 0x00, 0x80, 0x3f, b'h', b'i', 0, 0]);
		let rendered: String = text.lines.iter().map(|l| l.to_string()).collect::<Vec<_>>().join("\n");
		assert!(rendered.contains("f32     1 "), "{rendered}");
		assert!(rendered.contains("u32/i32 1065353216"), "{rendered}");
		assert!(rendered.contains("u16/i16 0 / 0"), "{rendered}");
	}
}
