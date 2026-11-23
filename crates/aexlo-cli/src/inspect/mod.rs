//! `aexlo inspect`: drive a plugin with commands and watch what it does.
//!
//! The plugin runs on a thread of its own with a [`Recorder`] observing it
//! from `GLOBAL_SETUP` on: every `PF_Cmd_*`, every host suite function it
//! calls back, every handle and world it allocates or frees. The TUI shows
//! the trace, per-function totals, parameters, host state, the plugin's live
//! memory as a hex dump (with what the last command changed) and the output.
//!
//! `--exec` and `--script` run commands without the TUI and print the same
//! analysis as text, or with `--json` as one JSON object per line for
//! agents and scripts (see [`json`]). `--script -` reads the commands from
//! stdin as they arrive, answering each before reading the next.

mod command;
mod diagnose;
mod engine;
mod json;
mod recorder;
mod term;
pub(crate) use term::json_output;
mod ui;
#[cfg(windows)]
mod win32;

use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use aexlo::{ObserveLevel, PixelDepthKind, Strict};
use anyhow::{Context, Result, bail};
use ratatui::crossterm::event::{self, Event, KeyEventKind};

use command::{Command, InputSource};
use engine::{Engine, Setup, Snapshot};
use recorder::{Kind, Recorder, fmt_duration};
use ui::{Action, App, MEMORY_WINDOW};

/// What `--exec`, `--script` and the TUI prompt take.
pub const COMMANDS_HELP: &str = command::HELP;

#[derive(clap::Args)]
pub struct InspectArgs {
	/// The plugin, or a crate directory to build
	plugin: String,
	/// Input layer [default: a gradient]
	#[arg(short, long, value_name = "PNG", conflicts_with = "text")]
	input: Option<PathBuf>,
	/// Make the layer a text layer showing <STRING> (\n breaks lines), for
	/// plugins that need one
	#[arg(long, value_name = "STRING", allow_hyphen_values = true)]
	text: Option<String>,
	/// Font for --text and `input text` [default: a system sans-serif]
	#[arg(long, value_name = "FILE")]
	font: Option<PathBuf>,
	/// Size of the default input and of the output
	#[arg(long, value_name = "WxH", value_parser = parse_size)]
	size: Option<(u32, u32)>,
	/// Project depth
	#[arg(long, value_name = "8|16|32", default_value = "8", value_parser = parse_depth)]
	depth: PixelDepthKind,
	/// args (every suite call with its arguments), calls (without them) or
	/// commands
	#[arg(long, value_name = "L", default_value = "args", value_parser = parse_level)]
	level: ObserveLevel,
	/// Every strict check from the first command
	#[arg(long)]
	strict: bool,
	/// Iterate callbacks on the plugin's thread, so their suite calls are
	/// traced too
	#[arg(long)]
	serial: bool,
	/// No TUI: run `;`-separated commands, then print the analysis (e.g.
	/// --exec "render; set 1 2; render")
	#[arg(long, value_name = "CMDS", group = "script_source", allow_hyphen_values = true)]
	exec: Option<String>,
	/// Like --exec, one command per line; `-` reads them from stdin as they
	/// come, answering each before the next, so a pipe drives one session
	#[arg(long, value_name = "FILE", group = "script_source")]
	script: Option<String>,
	/// With --exec/--script, print one JSON object per line (loaded,
	/// command, focus, report, error), for agents
	#[arg(long, requires = "script_source")]
	json: bool,
	/// The old spelling of --json: human or json
	#[arg(long, value_enum, value_name = "F", hide = true, requires = "script_source")]
	format: Option<Format>,
	/// With --exec/--script, print the whole trace
	#[arg(long, requires = "script_source")]
	trace: bool,
}

fn parse_size(raw: &str) -> Result<(u32, u32), String> {
	let parse = || {
		let (w, h) = raw.split_once('x')?;
		Some((w.parse().ok()?, h.parse().ok()?))
	};
	parse().ok_or_else(|| format!("'{raw}' is not <W>x<H>"))
}

fn parse_depth(raw: &str) -> Result<PixelDepthKind, String> {
	match raw {
		"8" => Ok(PixelDepthKind::U8),
		"16" => Ok(PixelDepthKind::U16),
		"32" => Ok(PixelDepthKind::F32),
		other => Err(format!("'{other}' is not 8, 16 or 32")),
	}
}

fn parse_level(raw: &str) -> Result<ObserveLevel, String> {
	match raw {
		"args" => Ok(ObserveLevel::Arguments),
		"calls" => Ok(ObserveLevel::Calls),
		"commands" => Ok(ObserveLevel::Commands),
		other => Err(format!("'{other}' is not args, calls or commands")),
	}
}

/// Default input size when no `--input` or `--size` is given.
const DEFAULT_SIZE: (u32, u32) = (640, 360);

pub fn run(args: InspectArgs) -> Result<()> {
	let InspectArgs {
		plugin,
		input,
		text,
		font,
		size,
		depth,
		level,
		strict,
		serial,
		exec,
		script,
		json,
		format,
		trace: print_trace,
	} = args;
	let format = if json { Format::Json } else { format.unwrap_or_default() };
	let input = InputArgs {
		png: input,
		text: text.map(|text| text.replace("\\n", "\n")),
		font,
	};
	let strict = if strict { Strict::all() } else { Strict::default() };
	let script = match (exec, script) {
		(Some(exec), _) => Some(Script::Parsed(command::parse_script(&exec)?)),
		(None, Some(path)) if path == "-" => Some(Script::Stdin),
		(None, Some(path)) => {
			let text = std::fs::read_to_string(&path).with_context(|| format!("reading {path}"))?;
			Some(Script::Parsed(command::parse_script(&text)?))
		}
		(None, None) => None,
	};
	let Some(script) = script else {
		return tui(prepare(&plugin, &input, size, depth, level, strict, serial)?);
	};
	let setup = match prepare(&plugin, &input, size, depth, level, strict, serial) {
		Ok(setup) => setup,
		Err(err) => {
			if format == Format::Json {
				emit(&mut std::io::stdout(), &error_event(&err));
			}
			return Err(err);
		}
	};
	batch(setup, script, print_trace, format)
}

/// `--input`, `--text` and `--font`.
#[derive(Default)]
struct InputArgs {
	png: Option<PathBuf>,
	text: Option<String>,
	font: Option<PathBuf>,
}

/// Find (or build) the plugin and its input.
fn prepare(
	plugin: &str,
	input: &InputArgs,
	size: Option<(u32, u32)>,
	depth: PixelDepthKind,
	level: ObserveLevel,
	strict: Strict,
	serial: bool,
) -> Result<Setup> {
	let resolved = crate::args::resolve_plugin(plugin);
	let manifest = Some(resolved.join("Cargo.toml")).filter(|m| resolved.is_dir() && m.exists());
	let path = match &manifest {
		Some(manifest) => crate::watch::build_cdylib(manifest)?,
		None => resolved,
	};
	if !path.exists() {
		bail!("no plugin at {}", path.display());
	}
	let font = input.font.clone();
	let (input, text) = match (&input.png, &input.text) {
		(Some(_), Some(_)) => bail!("give either --input or --text"),
		(Some(png), None) => (engine::load_png(png)?.convert(depth), None),
		(None, Some(text)) => engine::build_input(
			&InputSource::Text(text.clone()),
			depth,
			size.unwrap_or(DEFAULT_SIZE),
			font.as_deref(),
		)?,
		(None, None) => engine::build_input(&InputSource::Gradient(None), depth, size.unwrap_or(DEFAULT_SIZE), None)?,
	};
	Ok(Setup {
		path,
		manifest,
		input,
		text,
		project: None,
		masks: Vec::new(),
		font,
		size,
		level,
		strict,
		serial,
	})
}

fn error_event(err: &anyhow::Error) -> serde_json::Value {
	serde_json::json!({ "event": "error", "error": format!("{err:#}") })
}

//==== Batch ============================================================

/// Where `--exec` and `--script` take their commands from.
enum Script {
	/// Parsed up front, so a typo fails before the plugin loads.
	Parsed(Vec<(String, Command)>),
	/// `--script -`: one line at a time as it arrives, answered before the
	/// next is read, so an agent can drive one loaded plugin over a pipe.
	Stdin,
}

fn batch(setup: Setup, script: Script, print_trace: bool, format: Format) -> Result<()> {
	let json = (format == Format::Json).then(term::json_output);
	let recorder = Recorder::default();
	let plugin = setup.path.display().to_string();
	let engine = match Engine::load(setup, recorder.clone()) {
		Ok(engine) => engine,
		Err(err) => {
			if let Some(mut out) = json {
				emit(
					&mut out,
					&serde_json::json!({ "event": "error", "error": format!("{err:#}") }),
				);
			}
			return Err(err);
		}
	};
	let mut batch = Batch {
		engine,
		recorder,
		json,
		print_trace,
		failed: 0,
	};
	batch.loaded(&plugin);
	match script {
		Script::Parsed(commands) => {
			for (line, command) in commands {
				if !batch.run(&line, Ok(command)) {
					break;
				}
			}
		}
		Script::Stdin => {
			'lines: for text in std::io::stdin().lock().lines() {
				let text = text.context("reading commands from stdin")?;
				for line in text.split(';').map(str::trim) {
					if line.is_empty() || line.starts_with('#') {
						continue;
					}
					if !batch.run(line, command::parse(line)) {
						break 'lines;
					}
				}
			}
		}
	}
	batch.finish();
	let failed = batch.failed;
	drop(batch);
	if failed > 0 {
		bail!("{failed} command(s) failed");
	}
	Ok(())
}

/// The text `--exec` prints, or NDJSON with `--json` (see [`json`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
enum Format {
	#[default]
	Human,
	Json,
}

/// One JSON object on a line of its own, flushed so a reader sees it now.
fn emit(out: &mut dyn Write, value: &serde_json::Value) {
	let mut line = value.to_string();
	line.push('\n');
	let _ = out.write_all(line.as_bytes());
	let _ = out.flush();
}

struct Batch {
	engine: Engine,
	recorder: Recorder,
	/// `Some` for `--json`: where the events go.
	json: Option<Box<dyn Write>>,
	print_trace: bool,
	failed: usize,
}

impl Batch {
	fn loaded(&mut self, plugin: &str) {
		let Some(out) = self.json.as_mut() else { return };
		let params = self.engine.snapshot().map_or(0, |s| s.params.len());
		emit(
			out,
			&serde_json::json!({ "event": "loaded", "plugin": plugin, "params": params }),
		);
	}

	/// Run one command line; false when it was `quit`.
	fn run(&mut self, line: &str, command: Result<Command>) -> bool {
		use serde_json::json;
		let command = match command {
			Ok(command) => command,
			Err(err) => {
				self.failed += 1;
				match self.json.as_mut() {
					Some(out) => emit(
						out,
						&json!({ "event": "command", "line": line, "ok": false, "error": format!("{err:#}") }),
					),
					None => println!("» {line}  ✗ {err:#}"),
				}
				return true;
			}
		};
		match &command {
			Command::Focus(query) => self.focus(line, query),
			Command::Report => self.report(),
			Command::Quit => return false,
			Command::Clear => {
				self.recorder.lock().clear();
				self.done(line, Ok(("trace cleared".into(), Duration::ZERO)));
			}
			Command::Help => match self.json.as_mut() {
				Some(out) => emit(out, &json!({ "event": "help", "text": command::HELP })),
				None => print!("{}", command::HELP),
			},
			command if command.is_engine() => {
				let result = self
					.engine
					.run(line, command)
					.map(|outcome| (outcome.message, outcome.elapsed));
				self.done(line, result);
			}
			// `mem`, `find`: what the TUI shows.
			_ => match self.json.as_mut() {
				Some(out) => emit(
					out,
					&json!({ "event": "command", "line": line, "ok": true, "skipped": "only meaningful in the TUI" }),
				),
				None => println!("» {line}  (skipped: only meaningful in the TUI)"),
			},
		}
		true
	}

	/// Report a command's outcome.
	fn done(&mut self, line: &str, result: Result<(String, Duration)>) {
		if result.is_err() {
			self.failed += 1;
		}
		let Some(out) = self.json.as_mut() else {
			match result {
				Ok((message, elapsed)) => {
					println!("» {line}  ✓ {}", fmt_duration(elapsed));
					for text in message.lines() {
						println!("  {text}");
					}
				}
				Err(err) => println!("» {line}  ✗ {err:#}"),
			}
			return;
		};
		let rec = self.recorder.lock();
		let step = rec
			.trace
			.iter()
			.rev()
			.find(|e| matches!(&e.kind, Kind::Step(text) if text == line))
			.map(|e| e.seq);
		let mut event = serde_json::json!({ "event": "command", "line": line });
		match result {
			Ok((message, elapsed)) => {
				event["ok"] = true.into();
				event["elapsed_us"] = u64::try_from(elapsed.as_micros()).unwrap_or(u64::MAX).into();
				event["message"] = message.into();
			}
			Err(err) => {
				event["ok"] = false.into();
				event["error"] = format!("{err:#}").into();
			}
		}
		if let (Some(event), serde_json::Value::Object(caused)) = (event.as_object_mut(), json::caused(&rec, step)) {
			event.extend(caused);
		}
		drop(rec);
		emit(out, &event);
	}

	fn focus(&mut self, line: &str, query: &str) {
		let snapshot = self.engine.snapshot();
		let rec = self.recorder.lock();
		let found = diagnose::find(&rec, query);
		if found.is_none() {
			self.failed += 1;
		}
		match (self.json.as_mut(), found) {
			(None, Some(index)) => print!(
				"» {line}\n{}",
				diagnose::diagnose(&rec, index, snapshot.as_ref()).to_text()
			),
			(None, None) => println!("» {line}  ✗ nothing in the trace matches '{query}'"),
			(Some(out), Some(index)) => {
				let mut event = serde_json::json!({
					"event": "focus",
					"line": line,
					"query": query,
					"ok": true,
				});
				if let (Some(event), serde_json::Value::Object(fields)) =
					(event.as_object_mut(), json::focus(&rec, index, snapshot.as_ref()))
				{
					event.extend(fields);
				}
				emit(out, &event);
			}
			(Some(out), None) => emit(
				out,
				&serde_json::json!({
					"event": "focus",
					"line": line,
					"query": query,
					"ok": false,
					"error": format!("nothing in the trace matches '{query}'"),
				}),
			),
		}
	}

	/// The analysis of everything so far.
	fn report(&mut self) {
		let snapshot = self.engine.snapshot();
		let memory = self.memory(snapshot.as_ref());
		self.print_report(snapshot, memory);
	}

	/// The final report. The plugin is unloaded first, so it covers
	/// `GLOBAL_SETDOWN` and the handles still held then (strict leaks); the
	/// state, params and memory are as they were just before.
	fn finish(&mut self) {
		let mut snapshot = self.engine.snapshot();
		let memory = self.memory(snapshot.as_ref());
		self.engine.shutdown();
		if let Some(s) = snapshot.as_mut() {
			s.findings = self.engine.findings().to_vec();
		}
		self.print_report(snapshot, memory);
	}

	/// The first bytes of each live allocation.
	fn memory(&self, snapshot: Option<&Snapshot>) -> Option<Vec<MemoryDump>> {
		snapshot.map(|s| {
			s.allocations
				.iter()
				.map(|a| (a.address, self.engine.read(a.address, 0, 64)))
				.collect()
		})
	}

	fn print_report(&mut self, snapshot: Option<Snapshot>, memory: Option<Vec<MemoryDump>>) {
		match self.json.as_mut() {
			Some(out) => {
				let rec = self.recorder.lock();
				let value = json::report(
					&rec,
					snapshot.as_ref(),
					memory.as_deref(),
					self.print_trace,
					self.failed,
				);
				drop(rec);
				emit(out, &value);
			}
			None => print!(
				"{}",
				report(&self.recorder, snapshot.as_ref(), memory.as_deref(), self.print_trace)
			),
		}
	}
}

/// What a live allocation is to the plugin, if `global_data` or
/// `sequence_data`.
fn role(address: usize, state: &aexlo::InstanceState) -> Option<&'static str> {
	if Some(address) == state.global_data {
		Some("global_data")
	} else if Some(address) == state.sequence_data {
		Some("sequence_data")
	} else {
		None
	}
}

/// The first bytes of a live allocation, by address.
type MemoryDump = (usize, Option<Vec<u8>>);

/// The analysis `--exec` prints.
fn report(
	recorder: &Recorder,
	snapshot: Option<&Snapshot>,
	memory: Option<&[MemoryDump]>,
	print_trace: bool,
) -> String {
	use std::fmt::Write;
	let rec = recorder.lock();
	let mut out = String::new();

	if print_trace {
		let _ = writeln!(out, "\n== trace ({} entries, {} dropped)", rec.trace.len(), rec.dropped);
		for e in &rec.trace {
			let _ = writeln!(
				out,
				"{:>10.3}ms {}{}",
				e.at.as_secs_f64() * 1e3,
				"  ".repeat(e.depth),
				e.describe()
			);
		}
	}

	let mut commands: Vec<_> = rec.commands.iter().collect();
	commands.sort_by_key(|(_, s)| std::cmp::Reverse(s.total));
	let _ = writeln!(out, "\n== commands");
	let _ = writeln!(
		out,
		"{:<32} {:>6} {:>6} {:>10} {:>10}",
		"command", "count", "errors", "max", "total"
	);
	for (name, s) in commands {
		let _ = writeln!(
			out,
			"{:<32} {:>6} {:>6} {:>10} {:>10}",
			format!("PF_Cmd_{name}"),
			s.count,
			s.errors,
			fmt_duration(s.max),
			fmt_duration(s.total)
		);
	}

	let mut calls: Vec<_> = rec.calls.iter().collect();
	calls.sort_by(|a, b| b.1.count.cmp(&a.1.count).then(a.0.cmp(b.0)));
	let _ = writeln!(out, "\n== suite calls ({} functions)", calls.len());
	for ((suite, function), s) in calls {
		let _ = writeln!(out, "{:>8}  {suite}::{function}  [{}]", s.count, s.commands.join(" "));
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
		let _ = writeln!(out, "\n== errors");
		for e in errors {
			let _ = writeln!(out, "{e}");
		}
	}
	drop(rec);

	let Some(s) = snapshot else {
		return out;
	};
	let st = &s.state;
	let handle = |h: Option<usize>| h.map_or("null".into(), |h| format!("{h:#x}"));
	let _ = writeln!(out, "\n== state");
	let _ = writeln!(out, "out_flags      {:?}", s.out_flags);
	let _ = writeln!(out, "out_flags2     {:?}", s.out_flags2);
	let _ = writeln!(out, "global_data    {}", handle(st.global_data));
	let _ = writeln!(out, "sequence_data  {}", handle(st.sequence_data));
	let _ = writeln!(
		out,
		"time           {} / {} / {}",
		st.current_time, st.time_step, st.time_scale
	);
	let _ = writeln!(
		out,
		"input/output   {}x{} → {}x{} @{} bpc",
		s.input.0,
		s.input.1,
		s.output.0,
		s.output.1,
		s.output.2.bits()
	);
	let _ = writeln!(out, "last out_flags {:?}", st.last_out_flags);
	let _ = writeln!(
		out,
		"output mean    {:.4} {:.4} {:.4} {:.4}",
		s.mean[0], s.mean[1], s.mean[2], s.mean[3]
	);
	if let Some(unwritten) = s.unwritten {
		let _ = writeln!(out, "unwritten px   {unwritten}");
	}

	let _ = writeln!(out, "\n== params");
	for p in &s.params {
		let _ = writeln!(out, "{:>3}  {:<24} {:<12} {}", p.index, p.name, p.kind, p.value);
	}

	let _ = writeln!(out, "\n== live memory ({})", s.allocations.len());
	for a in &s.allocations {
		let label = role(a.address, st).map_or(String::new(), |role| format!(" ({role})"));
		let bytes = memory
			.and_then(|m| m.iter().find(|(addr, _)| *addr == a.address))
			.and_then(|(_, bytes)| bytes.as_ref())
			.map(|b| b.iter().map(|x| format!("{x:02x}")).collect::<Vec<_>>().join(" "))
			.unwrap_or_default();
		let _ = writeln!(
			out,
			"{:?} {:#x} {} B from {}{label}\n    {bytes}",
			a.kind, a.address, a.bytes, a.allocated_in
		);
	}

	if !s.findings.is_empty() {
		let _ = writeln!(out, "\n== strict findings");
		for f in &s.findings {
			let _ = writeln!(out, "{f}");
		}
	}
	out
}

//==== TUI ==============================================================

enum Request {
	Run(String, Command),
	Watch { address: usize, base: usize },
	Quit,
}

enum Reply {
	Loaded(Result<Option<Snapshot>, String>),
	Done(Result<(String, Duration), String>, Option<Snapshot>),
	Memory {
		address: usize,
		base: usize,
		bytes: Option<Vec<u8>>,
		fresh: bool,
	},
}

/// The plugin's thread: load, then serve requests until told to quit.
fn serve(setup: Setup, recorder: Recorder, requests: mpsc::Receiver<Request>, replies: mpsc::Sender<Reply>) {
	let mut engine = match Engine::load(setup, recorder) {
		Ok(engine) => engine,
		Err(err) => {
			let _ = replies.send(Reply::Loaded(Err(format!("{err:#}"))));
			return;
		}
	};
	let _ = replies.send(Reply::Loaded(Ok(engine.snapshot())));
	let mut watch: Option<(usize, usize)> = None;
	let read = |engine: &Engine, (address, base): (usize, usize), fresh: bool| Reply::Memory {
		address,
		base,
		bytes: engine.read(address, base, MEMORY_WINDOW),
		fresh,
	};
	while let Some(request) = next_request(&requests) {
		match request {
			Request::Run(line, command) => {
				let result = engine
					.run(&line, &command)
					.map(|o| (o.message, o.elapsed))
					.map_err(|e| format!("{e:#}"));
				if replies.send(Reply::Done(result, engine.snapshot())).is_err() {
					break;
				}
				if let Some(w) = watch {
					let _ = replies.send(read(&engine, w, true));
				}
			}
			Request::Watch { address, base } => {
				watch = Some((address, base));
				let _ = replies.send(read(&engine, (address, base), false));
			}
			Request::Quit => break,
		}
	}
}

/// The next request, or `None` once the UI hangs up. On Windows the
/// plugin's windows (an Options dialog `dialog` opened) live on this thread,
/// so their messages are dispatched while it waits.
fn next_request(requests: &mpsc::Receiver<Request>) -> Option<Request> {
	#[cfg(windows)]
	loop {
		match requests.recv_timeout(Duration::from_millis(16)) {
			Ok(request) => return Some(request),
			Err(mpsc::RecvTimeoutError::Timeout) => win32::pump_messages(),
			Err(mpsc::RecvTimeoutError::Disconnected) => return None,
		}
	}
	#[cfg(not(windows))]
	requests.recv().ok()
}

fn tui(setup: Setup) -> Result<()> {
	let recorder = Recorder::default();
	let (request_tx, request_rx) = mpsc::channel();
	let (reply_tx, reply_rx) = mpsc::channel();
	let engine = {
		let recorder = recorder.clone();
		std::thread::Builder::new()
			.name("aexlo-inspect-plugin".into())
			.spawn(move || serve(setup, recorder, request_rx, reply_tx))?
	};

	let mut session = term::Session::start()?;
	let mut app = App::new(recorder);
	let mut loading = true;
	let mut engine_gone = false;

	while !app.quit {
		app.output(session.captured.drain());
		loop {
			let reply = match reply_rx.try_recv() {
				Ok(reply) => reply,
				Err(mpsc::TryRecvError::Empty) => break,
				Err(mpsc::TryRecvError::Disconnected) => {
					if !engine_gone {
						engine_gone = true;
						app.error("the plugin thread stopped");
					}
					break;
				}
			};
			match reply {
				Reply::Loaded(Ok(snapshot)) => {
					loading = false;
					app.snapshot = snapshot;
					app.info("loaded: `:render` (or r) to render, `:help` for commands");
				}
				Reply::Loaded(Err(err)) => {
					loading = false;
					app.error(format!("load failed: {err}"));
				}
				Reply::Done(result, snapshot) => {
					app.finished(result, snapshot);
					if let Some(Action::Watch { address, base }) = app.refresh_watch() {
						let _ = request_tx.send(Request::Watch { address, base });
					}
				}
				Reply::Memory {
					address,
					base,
					bytes,
					fresh,
				} => app.memory(address, base, bytes, fresh),
			}
		}

		session.terminal.draw(|frame| app.draw(frame))?;

		if !event::poll(Duration::from_millis(50))? {
			continue;
		}
		let Event::Key(key) = event::read()? else { continue };
		if key.kind != KeyEventKind::Press {
			continue;
		}
		match app.key(key) {
			Some(Action::Run(line, command)) => {
				if loading || engine_gone {
					app.finished(Err("the plugin is not loaded".into()), None);
				} else if request_tx.send(Request::Run(line, command)).is_err() {
					app.finished(Err("the plugin thread stopped".into()), None);
				}
			}
			Some(Action::Watch { address, base }) => {
				let _ = request_tx.send(Request::Watch { address, base });
			}
			Some(Action::Quit) => app.quit = true,
			None => {}
		}
	}

	if app.is_busy() || loading {
		// The plugin is stuck in a command; it cannot be torn down.
		drop(session);
		eprintln!("aexlo inspect: left while the plugin was still running");
		crate::process::exit(130);
	}
	let _ = request_tx.send(Request::Quit);
	drop(session);
	let _ = engine.join();
	Ok(())
}
