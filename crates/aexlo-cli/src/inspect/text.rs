//! The text `--exec` and `--script` print: each command's outcome, `focus`'s
//! diagnosis and the analysis at the end.
//!
//! Plain text keeps a fixed shape for logs and pipes. Rich text (a terminal,
//! or `--color always`) colors it the way the TUI does and adds what reads
//! better in color: bars for where the time and calls went, a swatch of the
//! output's mean color, and live memory as a hex dump with its ASCII.

use std::fmt::Write;
use std::time::Duration;

use anstyle::{AnsiColor, Color, RgbColor, Style};

use super::diagnose::{Diagnosis, Tone};
use super::engine::Snapshot;
use super::recorder::{self, Entry, Kind, Recorder, fmt_duration};
use super::{MemoryDump, role};

/// Whether text is plain or rich.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Look {
	pub rich: bool,
}

const DIM: Style = Style::new().dimmed();
const BOLD: Style = Style::new().bold();
/// How wide a heading's rule runs.
const WIDTH: usize = 72;
const TIME_BAR: usize = 16;
const CALL_BAR: usize = 10;
const DUMP_ROW: usize = 16;

const fn fg(color: AnsiColor) -> Style {
	Style::new().fg_color(Some(Color::Ansi(color)))
}

const GREEN: Style = fg(AnsiColor::Green);
const YELLOW: Style = fg(AnsiColor::Yellow);
const RED: Style = fg(AnsiColor::Red).bold();
const CYAN: Style = fg(AnsiColor::Cyan);
const STEP: Style = fg(AnsiColor::Magenta).bold();

impl Look {
	/// `text` in `style`, when rich.
	pub fn paint(self, style: Style, text: &str) -> String {
		if self.rich && !text.is_empty() {
			format!("{style}{text}{style:#}")
		} else {
			text.to_string()
		}
	}

	fn dim(self, text: &str) -> String {
		self.paint(DIM, text)
	}

	fn bold(self, text: &str) -> String {
		self.paint(BOLD, text)
	}

	/// `» line`, the start of every command's outcome.
	fn step(self, line: &str) -> String {
		format!("{} {}", self.paint(STEP, "»"), self.bold(line))
	}

	/// A command that worked, with what it said.
	pub fn done(self, line: &str, elapsed: Duration, message: &str) -> String {
		let mut out = format!(
			"{}  {} {}\n",
			self.step(line),
			self.paint(GREEN, "✓"),
			self.dim(&fmt_duration(elapsed))
		);
		for text in message.lines() {
			let _ = writeln!(out, "  {text}");
		}
		out
	}

	/// A command that failed.
	pub fn failed(self, line: &str, err: &anyhow::Error) -> String {
		format!("{}  {}\n", self.step(line), self.paint(RED, &format!("✗ {err:#}")))
	}

	/// A command the batch does not run.
	pub fn skipped(self, line: &str) -> String {
		format!(
			"{}  {}\n",
			self.step(line),
			self.dim("(skipped: only meaningful in the TUI)")
		)
	}

	/// `focus`'s diagnosis, under the line that asked for it.
	pub fn focus(self, line: &str, diagnosis: &Diagnosis) -> String {
		let mut out = format!("{}\n", self.step(line));
		let title = format!("── {} ──", diagnosis.title);
		let _ = writeln!(out, "{}", self.paint(CYAN.bold(), &title));
		for spans in &diagnosis.lines {
			for (tone, text) in spans {
				out += &self.paint(tone_style(*tone), text);
			}
			out.push('\n');
		}
		out
	}

	/// `== name (detail)` plain; the name in bold over a rule when rich.
	fn heading(self, out: &mut String, name: &str, detail: Option<&str>) {
		if !self.rich {
			let _ = match detail {
				Some(detail) => writeln!(out, "\n== {name} ({detail})"),
				None => writeln!(out, "\n== {name}"),
			};
			return;
		}
		let label = match detail {
			Some(detail) => format!("{name}  {detail}"),
			None => name.to_string(),
		};
		let rule = "─".repeat(WIDTH.saturating_sub(label.chars().count() + 1));
		let detail = detail.map_or(String::new(), |d| format!("  {}", self.dim(d)));
		let _ = writeln!(out, "\n{}{detail} {}", self.paint(CYAN.bold(), name), self.dim(&rule));
	}
}

fn tone_style(tone: Tone) -> Style {
	match tone {
		Tone::Heading => BOLD.underline(),
		Tone::Key | Tone::Dim => DIM,
		Tone::Plain => Style::new(),
		Tone::Good => GREEN,
		Tone::Warn => YELLOW,
		Tone::Bad => RED,
	}
}

/// How a trace entry is colored, as in the TUI.
fn entry_style(entry: &Entry) -> Style {
	match &entry.kind {
		Kind::Step(_) => STEP,
		Kind::Begin(_) => CYAN.bold(),
		Kind::End { error: Some(_), .. } => RED,
		Kind::End { .. } => CYAN,
		Kind::Alloc { allocated: true, .. } => GREEN,
		Kind::Alloc { .. } => YELLOW,
		Kind::Checkout { .. } => fg(AnsiColor::Blue),
		Kind::Call { .. } => Style::new(),
	}
}

/// `part` of `whole` as a bar `width` cells wide, the rest dimmed.
fn bar(look: Look, part: f64, whole: f64, width: usize) -> String {
	let filled = if whole > 0.0 {
		((part / whole * width as f64).round() as usize).clamp(usize::from(part > 0.0), width)
	} else {
		0
	};
	format!(
		"{}{}",
		look.paint(CYAN, &"━".repeat(filled)),
		look.dim(&"─".repeat(width - filled))
	)
}

/// `OutFlags(A | B)` -> `A | B`, and `(none)` for no flags.
fn flag_names(debug: &str) -> String {
	let inner = debug
		.split_once('(')
		.map_or(debug, |(_, rest)| rest.trim_end_matches(')'));
	match inner {
		"" | "0x0" => "(none)".into(),
		names => names.to_string(),
	}
}

/// The analysis `--exec` prints.
pub fn report(
	look: Look,
	recorder: &Recorder,
	snapshot: Option<&Snapshot>,
	memory: Option<&[MemoryDump]>,
	print_trace: bool,
) -> String {
	let rec = recorder.lock();
	let mut out = String::new();

	if print_trace {
		look.heading(
			&mut out,
			"trace",
			Some(&format!("{} entries, {} dropped", rec.trace.len(), rec.dropped)),
		);
		for e in &rec.trace {
			let time = format!("{:>10.3}ms", e.at.as_secs_f64() * 1e3);
			let indent = match look.rich {
				true => look.dim(&"│ ".repeat(e.depth)),
				false => "  ".repeat(e.depth),
			};
			let _ = writeln!(
				out,
				"{} {indent}{}",
				look.dim(&time),
				look.paint(entry_style(e), &e.describe())
			);
		}
	}

	let mut commands: Vec<_> = rec.commands.iter().collect();
	commands.sort_by_key(|(_, s)| std::cmp::Reverse(s.total));
	look.heading(&mut out, "commands", None);
	let header = format!(
		"{:<32} {:>6} {:>6} {:>10} {:>10}",
		"command", "count", "errors", "max", "total"
	);
	let _ = writeln!(out, "{}", look.dim(&header));
	let slowest = commands.first().map_or(0.0, |(_, s)| s.total.as_secs_f64());
	for (name, s) in &commands {
		let name = match look.rich {
			true => format!(
				"{}{}{}",
				look.dim("PF_Cmd_"),
				look.bold(name),
				" ".repeat(25usize.saturating_sub(name.len()))
			),
			false => format!("{:<32}", format!("PF_Cmd_{name}")),
		};
		let errors = format!("{:>6}", s.errors);
		let errors = match s.errors {
			0 => look.dim(&errors),
			_ => look.paint(RED, &errors),
		};
		let _ = write!(
			out,
			"{name} {:>6} {errors} {:>10} {:>10}",
			s.count,
			fmt_duration(s.max),
			fmt_duration(s.total)
		);
		if look.rich {
			let _ = write!(out, "  {}", bar(look, s.total.as_secs_f64(), slowest, TIME_BAR));
		}
		out.push('\n');
	}

	let mut calls: Vec<_> = rec.calls.iter().collect();
	calls.sort_by(|a, b| b.1.count.cmp(&a.1.count).then(a.0.cmp(b.0)));
	look.heading(&mut out, "suite calls", Some(&format!("{} functions", calls.len())));
	let most = calls.first().map_or(0, |(_, s)| s.count);
	for ((suite, function), s) in &calls {
		let commands = format!("[{}]", s.commands.join(" "));
		match look.rich {
			true => {
				let _ = writeln!(
					out,
					"{}  {}  {}{function}  {}",
					look.bold(&format!("{:>8}", s.count)),
					bar(look, s.count as f64, most as f64, CALL_BAR),
					look.dim(&format!("{suite}::")),
					look.dim(&commands)
				);
			}
			false => {
				let _ = writeln!(out, "{:>8}  {suite}::{function}  {commands}", s.count);
			}
		}
	}

	let errors: Vec<String> = rec
		.trace
		.iter()
		.filter_map(|e| match &e.kind {
			Kind::End {
				name,
				error: Some(code),
				..
			} => Some(format!("PF_Cmd_{name} returned {}", recorder::err_name(*code))),
			_ => None,
		})
		.collect();
	if !errors.is_empty() {
		look.heading(&mut out, "errors", None);
		for e in errors {
			let line = match look.rich {
				true => look.paint(RED, &format!("✗ {e}")),
				false => e,
			};
			let _ = writeln!(out, "{line}");
		}
	}
	drop(rec);

	let Some(s) = snapshot else {
		return out;
	};
	state(&mut out, look, s);
	params(&mut out, look, s);
	live_memory(&mut out, look, s, memory);

	if !s.findings.is_empty() {
		look.heading(&mut out, "strict findings", None);
		for f in &s.findings {
			let line = match look.rich {
				true => format!("{} {f}", look.paint(YELLOW, "⚠")),
				false => f.clone(),
			};
			let _ = writeln!(out, "{line}");
		}
	}
	out
}

fn state(out: &mut String, look: Look, s: &Snapshot) {
	let st = &s.state;
	let handle = |h: Option<usize>| match h {
		Some(h) => format!("{h:#x}"),
		None => look.dim("null"),
	};
	let flags = |debug: String| match look.rich {
		true => flag_names(&debug),
		false => debug,
	};
	look.heading(out, "state", None);
	let mut row = |key: &str, value: String| {
		let _ = writeln!(out, "{} {value}", look.dim(&format!("{key:<14}")));
	};
	row("out_flags", flags(format!("{:?}", s.out_flags)));
	row("out_flags2", flags(format!("{:?}", s.out_flags2)));
	row("global_data", handle(st.global_data));
	row("sequence_data", handle(st.sequence_data));
	row(
		"time",
		format!("{} / {} / {}", st.current_time, st.time_step, st.time_scale),
	);
	row(
		"input/output",
		format!(
			"{}x{} → {}x{} @{} bpc",
			s.input.0,
			s.input.1,
			s.output.0,
			s.output.1,
			s.output.2.bits()
		),
	);
	row("last out_flags", flags(format!("{:?}", st.last_out_flags)));
	let [r, g, b, a] = s.mean;
	let mut mean = format!("{r:.4} {g:.4} {b:.4} {a:.4}");
	if look.rich {
		// Over black, the way the output would show.
		let channel = |c: f32| (c.clamp(0.0, 1.0) * a.clamp(0.0, 1.0) * 255.0).round() as u8;
		let swatch = Style::new().fg_color(Some(RgbColor(channel(r), channel(g), channel(b)).into()));
		mean = format!("{}  {mean}", look.paint(swatch, "██"));
	}
	row("output mean", mean);
	if let Some(unwritten) = s.unwritten {
		let count = unwritten.to_string();
		row(
			"unwritten px",
			match unwritten {
				0 => count,
				_ => look.paint(YELLOW, &count),
			},
		);
	}
}

fn params(out: &mut String, look: Look, s: &Snapshot) {
	look.heading(out, "params", None);
	for p in &s.params {
		if !look.rich {
			let _ = writeln!(out, "{:>3}  {:<24} {:<12} {}", p.index, p.name, p.kind, p.value);
			continue;
		}
		let mut tags = String::new();
		for (on, tag) in [(p.hidden, "hidden"), (p.disabled, "disabled")] {
			if on {
				tags += &format!("  {}", look.paint(YELLOW, tag));
			}
		}
		let _ = writeln!(
			out,
			"{}  {} {} {}{tags}",
			look.dim(&format!("{:>3}", p.index)),
			look.bold(&format!("{:<24}", p.name)),
			look.dim(&format!("{:<12}", p.kind)),
			p.value
		);
	}
}

fn live_memory(out: &mut String, look: Look, s: &Snapshot, memory: Option<&[MemoryDump]>) {
	look.heading(out, "live memory", Some(&s.allocations.len().to_string()));
	for a in &s.allocations {
		let role = role(a.address, &s.state);
		let bytes = memory
			.and_then(|m| m.iter().find(|(addr, _)| *addr == a.address))
			.and_then(|(_, bytes)| bytes.as_deref())
			.unwrap_or_default();
		if !look.rich {
			let label = role.map_or(String::new(), |role| format!(" ({role})"));
			let hex = bytes.iter().map(|x| format!("{x:02x}")).collect::<Vec<_>>().join(" ");
			let _ = writeln!(
				out,
				"{:?} {:#x} {} B from {}{label}\n    {hex}",
				a.kind, a.address, a.bytes, a.allocated_in
			);
			continue;
		}
		let label = role.map_or(String::new(), |role| format!("  {}", look.paint(YELLOW, role)));
		let _ = writeln!(
			out,
			"{} {:#x}  {}  {}{label}",
			look.paint(GREEN, &format!("{:?}", a.kind)),
			a.address,
			look.bold(&format!("{} B", a.bytes)),
			look.dim(&format!("from {}", a.allocated_in)),
		);
		dump(out, look, bytes, a.bytes);
	}
}

/// `bytes` as rows of hex with their ASCII, zeros dimmed; `size` is how
/// many the allocation has, for the note on what is not shown.
fn dump(out: &mut String, look: Look, bytes: &[u8], size: usize) {
	if bytes.is_empty() {
		let _ = writeln!(out, "    {}", look.dim("(could not be read)"));
		return;
	}
	for (row, chunk) in bytes.chunks(DUMP_ROW).enumerate() {
		let mut hex = String::new();
		let mut ascii = String::new();
		for (i, &b) in chunk.iter().enumerate() {
			if i == DUMP_ROW / 2 {
				hex.push(' ');
			}
			let cell = format!("{b:02x} ");
			hex += &match b {
				0 => look.dim(&cell),
				_ => cell,
			};
			ascii += &match b {
				0x20..=0x7e => (b as char).to_string(),
				_ => look.dim("·"),
			};
		}
		// Pad a short last row so the ASCII lines up.
		let missing = DUMP_ROW - chunk.len();
		let pad = missing * 3 + usize::from(chunk.len() <= DUMP_ROW / 2);
		let _ = writeln!(
			out,
			"    {}  {hex}{}  {ascii}",
			look.dim(&format!("{:04x}", row * DUMP_ROW)),
			" ".repeat(pad)
		);
	}
	if size > bytes.len() {
		let _ = writeln!(out, "    {}", look.dim(&format!("… {} more B", size - bytes.len())));
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	const RICH: Look = Look { rich: true };
	const PLAIN: Look = Look { rich: false };

	fn strip(text: &str) -> String {
		let mut out = String::new();
		let mut chars = text.chars();
		while let Some(c) = chars.next() {
			if c == '\x1b' {
				for c in chars.by_ref() {
					if c.is_ascii_alphabetic() {
						break;
					}
				}
			} else {
				out.push(c);
			}
		}
		out
	}

	#[test]
	fn plain_outcomes_keep_their_shape() {
		let done = PLAIN.done("render", Duration::from_micros(75), "rendered\n16x8");
		assert_eq!(done, "» render  ✓ 75 µs\n  rendered\n  16x8\n");
		let failed = PLAIN.failed("render", &anyhow::anyhow!("boom"));
		assert_eq!(failed, "» render  ✗ boom\n");
		assert_eq!(
			PLAIN.skipped("mem 0"),
			"» mem 0  (skipped: only meaningful in the TUI)\n"
		);
	}

	#[test]
	fn rich_outcomes_read_the_same_without_color() {
		let done = RICH.done("render", Duration::from_micros(75), "rendered");
		assert!(done.contains('\x1b'), "{done:?}");
		assert_eq!(strip(&done), "» render  ✓ 75 µs\n  rendered\n");
	}

	#[test]
	fn headings() {
		let mut plain = String::new();
		PLAIN.heading(&mut plain, "suite calls", Some("13 functions"));
		assert_eq!(plain, "\n== suite calls (13 functions)\n");
		let mut rich = String::new();
		RICH.heading(&mut rich, "suite calls", Some("13 functions"));
		let rich = strip(&rich);
		assert!(rich.starts_with("\nsuite calls  13 functions ─"), "{rich:?}");
		assert_eq!(rich.trim().chars().count(), WIDTH, "{rich:?}");
	}

	#[test]
	fn bars_fill_by_share() {
		assert_eq!(strip(&bar(RICH, 1.0, 2.0, 10)), "━━━━━─────");
		// Anything at all shows.
		assert_eq!(strip(&bar(RICH, 0.001, 100.0, 10)), "━─────────");
		assert_eq!(strip(&bar(RICH, 0.0, 0.0, 4)), "────");
	}

	#[test]
	fn flag_names_drop_the_wrapper() {
		assert_eq!(flag_names("OutFlags(A | B)"), "A | B");
		assert_eq!(flag_names("OutFlags2(0x0)"), "(none)");
	}

	#[test]
	fn dumps_line_up_their_ascii() {
		let mut out = String::new();
		let bytes: Vec<u8> = b"Hello, world!\0\0\0ab".to_vec();
		dump(&mut out, RICH, &bytes, 64);
		let out = strip(&out);
		let lines: Vec<&str> = out.lines().collect();
		assert_eq!(
			lines[0],
			"    0000  48 65 6c 6c 6f 2c 20 77  6f 72 6c 64 21 00 00 00   Hello, world!···"
		);
		assert_eq!(
			lines[1],
			"    0010  61 62                                              ab"
		);
		assert_eq!(lines[2], "    … 46 more B");
	}
}
