//! `aexlo inspect`: drive a plugin with commands and watch what it does.
//!
//! The plugin runs on a thread of its own with a [`Recorder`] observing it
//! from `GLOBAL_SETUP` on: every `PF_Cmd_*`, every host suite function it
//! calls back, every handle and world it allocates or frees. The TUI shows
//! the trace, per-function totals, parameters, host state, the plugin's live
//! memory as a hex dump (with what the last command changed) and the output.
//!
//! `--exec` and `--script` run commands without the TUI and print the same
//! analysis as text, or with `--format json` as one JSON object per line for
//! agents and scripts (see [`json`]). `--script -` reads the commands from
//! stdin as they arrive, answering each before reading the next.

mod command;
mod diagnose;
mod engine;
mod json;
mod recorder;
mod term;
mod ui;

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

pub const USAGE: &str = "\
    inspect <plugin>   Drive the plugin from a prompt and analyze it in a TUI:
                       the commands and suite calls it makes (trace and
                       totals), its parameters, host state, live memory
                       (hex dump, changes per command) and the output
        -i, --input <png>    Input layer  [default: a gradient]
        --size <W>x<H>       Size of the default input and of the output
        --depth <8|16|32>    Project depth  [default: 8]
        --level <l>          args (every suite call with its arguments),
                             calls (without them) or commands  [default: args]
        --strict             Every strict check from the first command
        --serial             Iterate callbacks on the plugin's thread, so
                             their suite calls are traced too
        --exec <cmds>        No TUI: run `;`-separated commands, then print
                             the analysis (e.g. --exec \"render; set 1 2; render\")
        --script <file>      Like --exec, one command per line; `-` reads
                             them from stdin as they come, answering each
                             before the next, so a pipe drives one session
        --format <f>         With --exec/--script: human, or json for one
                             JSON object per line (loaded, command, focus,
                             report, error), for agents  [default: human]
        --trace              With --exec/--script, print the whole trace
                       `focus <text>` in --exec prints the diagnosis of the
                       newest matching call (e.g. --exec \"render; focus checkout\");
                       `report` prints the analysis then, `clear` forgets the
                       trace so far, `quit` stops reading.
                       `:help` in the TUI lists the commands; docs/inspect.md
                       describes them and every JSON event.
";

/// Default input size when no `--input` or `--size` is given.
const DEFAULT_SIZE: (u32, u32) = (640, 360);

pub fn run(args: impl Iterator<Item = String>) -> Result<()> {
	let mut plugin: Option<String> = None;
	let mut input: Option<PathBuf> = None;
	let mut size: Option<(u32, u32)> = None;
	let mut depth = PixelDepthKind::U8;
	let mut level = ObserveLevel::Arguments;
	let mut strict = Strict::default();
	let mut serial = false;
	let mut script: Option<Script> = None;
	let mut print_trace = false;
	let mut format = Format::Human;

	let mut args = args.peekable();
	while let Some(arg) = args.next() {
		match arg.as_str() {
			"-i" | "--input" => input = Some(PathBuf::from(crate::next_value(&mut args, &arg)?)),
			"--size" => {
				let raw = crate::next_value(&mut args, &arg)?;
				let (w, h) = raw.split_once('x').context("--size expects <W>x<H>")?;
				size = Some((
					w.parse().context("--size expects <W>x<H>")?,
					h.parse().context("--size expects <W>x<H>")?,
				));
			}
			"--depth" => {
				depth = match crate::next_value(&mut args, &arg)?.as_str() {
					"8" => PixelDepthKind::U8,
					"16" => PixelDepthKind::U16,
					"32" => PixelDepthKind::F32,
					other => bail!("--depth expects 8, 16 or 32, got '{other}'"),
				}
			}
			"--level" => {
				level = match crate::next_value(&mut args, &arg)?.as_str() {
					"args" => ObserveLevel::Arguments,
					"calls" => ObserveLevel::Calls,
					"commands" => ObserveLevel::Commands,
					other => bail!("--level expects args, calls or commands, got '{other}'"),
				}
			}
			"--strict" => strict = Strict::all(),
			"--serial" => serial = true,
			"--exec" => {
				script = Some(Script::Parsed(command::parse_script(&crate::next_value(
					&mut args, &arg,
				)?)?))
			}
			"--script" => {
				let path = crate::next_value(&mut args, &arg)?;
				script = Some(if path == "-" {
					Script::Stdin
				} else {
					let text = std::fs::read_to_string(&path).with_context(|| format!("reading {path}"))?;
					Script::Parsed(command::parse_script(&text)?)
				});
			}
			"--trace" => print_trace = true,
			"--format" => {
				format = match crate::next_value(&mut args, &arg)?.as_str() {
					"human" => Format::Human,
					"json" => Format::Json,
					other => bail!("--format expects human or json, got '{other}'"),
				}
			}
			other if other.starts_with('-') => bail!("unknown option '{other}'"),
			_ => {
				if plugin.replace(arg).is_some() {
					bail!("inspect: expected a single <plugin>");
				}
			}
		}
	}
	let plugin = plugin.context("inspect: missing <plugin>")?;
	let Some(script) = script else {
		if print_trace {
			bail!("--trace only applies to --exec and --script");
		}
		if format != Format::Human {
			bail!("--format only applies to --exec and --script");
		}
		return tui(prepare(&plugin, input.as_deref(), size, depth, level, strict, serial)?);
	};
	let setup = match prepare(&plugin, input.as_deref(), size, depth, level, strict, serial) {
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

/// Find (or build) the plugin and its input.
fn prepare(
	plugin: &str,
	input: Option<&std::path::Path>,
	size: Option<(u32, u32)>,
	depth: PixelDepthKind,
	level: ObserveLevel,
	strict: Strict,
	serial: bool,
) -> Result<Setup> {
	let resolved = crate::resolve_plugin(plugin);
	let manifest = Some(resolved.join("Cargo.toml")).filter(|m| resolved.is_dir() && m.exists());
	let path = match &manifest {
		Some(manifest) => crate::watch::build_cdylib(manifest)?,
		None => resolved,
	};
	if !path.exists() {
		bail!("no plugin at {}", path.display());
	}
	let input = match input {
		Some(png) => engine::load_png(png)?.convert(depth),
		None => engine::build_input(&InputSource::Gradient(None), depth, size.unwrap_or(DEFAULT_SIZE))?,
	};
	Ok(Setup {
		path,
		manifest,
		input,
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
	batch.report();
	let failed = batch.failed;
	drop(batch);
	if failed > 0 {
		bail!("{failed} command(s) failed");
	}
	Ok(())
}

/// `--format`: the text `--exec` prints, or NDJSON (see [`json`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
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
	/// `Some` for `--format json`: where the events go.
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
			(Some(out), Some(index)) => emit(
				out,
				&serde_json::json!({
					"event": "focus",
					"line": line,
					"query": query,
					"ok": true,
					"entry": json::entry(&rec.trace[index]),
					"diagnosis": json::diagnosis(&diagnose::diagnose(&rec, index, snapshot.as_ref())),
				}),
			),
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
		let memory = snapshot.as_ref().map(|s| {
			s.allocations
				.iter()
				.map(|a| (a.address, self.engine.read(a.address, 0, 64)))
				.collect::<Vec<_>>()
		});
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
	for request in requests {
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
		std::process::exit(130);
	}
	let _ = request_tx.send(Request::Quit);
	drop(session);
	let _ = engine.join();
	Ok(())
}
