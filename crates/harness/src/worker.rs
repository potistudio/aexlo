//! `aexlo worker` (§6.2): a subprocess that loads a plugin and runs variants
//! sent to it, so a crashing plugin fails one run instead of the harness.
//!
//! The protocol is one JSON object per line. Requests go to the worker's
//! stdin; responses (with an `ok` field) and progress events (with an
//! `event` field) come back on its stdout. Frames travel as raw files
//! ([`Frame::write_raw`]), never through the pipe. The plugin's own stdout is
//! redirected to stderr, which the client captures as the run's logs.

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use aexlo::{AppHost, CommandPhase, PixelDepthKind, PluginInstance};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::exec::{CommandSink, Executor, RunFailure, RunOptions, RunOutput, Timing, Trace, run_on};
use crate::frame::Frame;
use crate::preset::{PluginRef, Variant};

/// How long a timed-out worker gets to honor a cancellation before it is
/// killed (§6.3).
pub const CANCEL_GRACE: Duration = Duration::from_secs(2);

/// Extra time a load gets on top of a run's timeout.
const LOAD_TIMEOUT: Duration = Duration::from_secs(60);

/// A request to the worker.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "op", rename_all = "lowercase")]
pub enum Request {
	/// Load (and validate) a plugin; later runs make fresh instances of it.
	Load {
		id: u64,
		artifact: PathBuf,
	},
	/// Run a fully resolved variant, writing frame `i` to `frame_out` with
	/// `.i` before its extension for `i > 0`.
	Run {
		id: u64,
		variant: Box<Variant>,
		frame_out: PathBuf,
		#[serde(default)]
		options: RunOptions,
	},
	/// Ask the plugin to stop the current render (`PF_ABORT`).
	Cancel {
		id: u64,
	},
	Shutdown {
		id: u64,
	},
}

/// A parameter as `load` reports it.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ParamInfo {
	pub index: usize,
	pub name: String,
	pub kind: String,
}

/// What a plugin declared, as `load` reports it.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct PluginInfo {
	pub params: Vec<ParamInfo>,
	pub smart: bool,
	pub gpu: bool,
	/// Bits per channel the plugin declares.
	pub depths: Vec<u32>,
	/// Pixel formats registered through the Pixel Format Suite.
	pub formats: Vec<u32>,
}

impl PluginInfo {
	pub fn of(fx: &PluginInstance) -> Self {
		Self {
			params: (1..fx.param_count())
				.map(|index| ParamInfo {
					index,
					name: fx.param_name(index).unwrap_or_default(),
					kind: fx.param_kind(index).map(|k| format!("{k:?}")).unwrap_or_default(),
				})
				.collect(),
			smart: fx.supports_smart_render(),
			gpu: fx.supports_gpu(),
			depths: [PixelDepthKind::U8, PixelDepthKind::U16, PixelDepthKind::F32]
				.into_iter()
				.filter(|d| fx.supports_depth(*d))
				.map(PixelDepthKind::bits)
				.collect(),
			formats: fx.supported_pixel_formats().to_vec(),
		}
	}
}

/// A frame file a run wrote.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct FrameRef {
	pub path: PathBuf,
	pub w: u32,
	pub h: u32,
	pub depth: u32,
}

/// Why a request failed, by who is at fault.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct WireError {
	/// `plugin`, `invalid` or `harness`.
	pub kind: String,
	pub message: String,
}

/// A response to a request.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct Response {
	pub id: u64,
	pub ok: bool,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub info: Option<PluginInfo>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub frames: Vec<FrameRef>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub skipped: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub timing: Option<Timing>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub trace: Option<Trace>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub error: Option<WireError>,
}

/// Progress, streamed while a request runs.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Event {
	pub id: u64,
	/// `command` (a `PF_Cmd` begins or ends) or `load`.
	pub event: String,
	#[serde(default)]
	pub command: String,
	/// `begin` or `end`.
	#[serde(default)]
	pub phase: String,
}

/// The path frame `index` of a run is written to.
pub fn frame_path(frame_out: &Path, index: usize) -> PathBuf {
	if index == 0 {
		return frame_out.to_path_buf();
	}
	let stem = frame_out
		.file_stem()
		.map(|s| s.to_string_lossy().into_owned())
		.unwrap_or_default();
	let ext = frame_out
		.extension()
		.map(|e| e.to_string_lossy().into_owned())
		.unwrap_or_default();
	frame_out.with_file_name(format!("{stem}.{index}.{ext}"))
}

//==== Server ===========================================================

/// The `AppHost` of a worker: headless, but its renders can be cancelled.
struct WorkerHost {
	cancel: Arc<AtomicBool>,
}

impl AppHost for WorkerHost {
	fn abort_requested(&self, _effect: usize) -> bool {
		self.cancel.load(Ordering::Relaxed)
	}
}

/// Take over this process's stdout for the protocol and point fd 1 at
/// stderr, so whatever the plugin prints lands in the logs.
fn protocol_stdout() -> Result<std::fs::File> {
	#[cfg(unix)]
	{
		use std::os::fd::FromRawFd;
		// SAFETY: plain fd juggling on the process's own standard streams.
		unsafe {
			let proto = libc::dup(1);
			if proto < 0 || libc::dup2(2, 1) < 0 {
				return Err(Error::harness("redirecting the worker's stdout"));
			}
			Ok(std::fs::File::from_raw_fd(proto))
		}
	}
	#[cfg(windows)]
	{
		use std::os::windows::io::FromRawHandle;
		// SAFETY: as above, through the CRT's descriptors.
		unsafe {
			let proto = libc::dup(1);
			if proto < 0 || libc::dup2(2, 1) < 0 {
				return Err(Error::harness("redirecting the worker's stdout"));
			}
			let handle = libc::get_osfhandle(proto);
			Ok(std::fs::File::from_raw_handle(handle as _))
		}
	}
}

/// Run the worker loop until stdin closes or a `shutdown` arrives.
pub fn serve() -> Result<()> {
	let out = Arc::new(Mutex::new(protocol_stdout()?));
	let cancel = Arc::new(AtomicBool::new(false));
	// The worker is its own process: the host is ours to fix.
	let _ = aexlo::Host::install(WorkerHost { cancel: cancel.clone() });

	let send = {
		let out = out.clone();
		move |line: String| {
			if let Ok(mut out) = out.lock() {
				let _ = writeln!(out, "{line}");
				let _ = out.flush();
			}
		}
	};

	// Read requests on their own thread, so `cancel` lands mid-render.
	let (tx, rx) = mpsc::channel::<Request>();
	{
		let cancel = cancel.clone();
		let send = send.clone();
		std::thread::spawn(move || {
			for line in std::io::stdin().lock().lines() {
				let Ok(line) = line else { break };
				if line.trim().is_empty() {
					continue;
				}
				match serde_json::from_str::<Request>(&line) {
					Ok(Request::Cancel { .. }) => cancel.store(true, Ordering::Relaxed),
					Ok(request) => {
						if tx.send(request).is_err() {
							break;
						}
					}
					Err(e) => send(
						serde_json::to_string(&Response {
							ok: false,
							error: Some(WireError {
								kind: "harness".into(),
								message: format!("malformed request: {e}"),
							}),
							..Response::default()
						})
						.unwrap_or_default(),
					),
				}
			}
		});
	}

	let mut artifact: Option<PathBuf> = None;
	while let Ok(request) = rx.recv() {
		let response = match request {
			Request::Shutdown { id } => {
				send(json(&Response {
					id,
					ok: true,
					..Response::default()
				}));
				break;
			}
			Request::Cancel { .. } => continue,
			Request::Load { id, artifact: path } => {
				send(json(&Event {
					id,
					event: "load".into(),
					command: String::new(),
					phase: "begin".into(),
				}));
				match aexlo::Host::get().try_load(&path) {
					Ok(fx) => {
						artifact = Some(path);
						Response {
							id,
							ok: true,
							info: Some(PluginInfo::of(&fx)),
							..Response::default()
						}
					}
					Err(e) => failed(id, "plugin", format!("loading {}: {e}", path.display()), None),
				}
			}
			Request::Run {
				id,
				variant,
				frame_out,
				options,
			} => {
				cancel.store(false, Ordering::Relaxed);
				match &artifact {
					None => failed(id, "harness", "run before load".into(), None),
					Some(path) => run_request(id, path, &variant, &frame_out, &options, &send),
				}
			}
		};
		send(json(&response));
	}
	Ok(())
}

fn json(value: &impl Serialize) -> String {
	serde_json::to_string(value).unwrap_or_default()
}

fn failed(id: u64, kind: &str, message: String, trace: Option<Trace>) -> Response {
	Response {
		id,
		ok: false,
		trace,
		error: Some(WireError {
			kind: kind.into(),
			message,
		}),
		..Response::default()
	}
}

fn run_request(
	id: u64,
	artifact: &Path,
	variant: &Variant,
	frame_out: &Path,
	options: &RunOptions,
	send: &(impl Fn(String) + Send + Sync + Clone + 'static),
) -> Response {
	let sink: CommandSink = {
		let send = send.clone();
		Arc::new(move |event: &aexlo::CommandEvent| {
			send(json(&Event {
				id,
				event: "command".into(),
				command: event.name.to_string(),
				phase: match event.phase {
					CommandPhase::Begin => "begin".into(),
					CommandPhase::End => "end".into(),
				},
			}))
		})
	};

	let mut fx = match aexlo::Host::get().try_load(artifact) {
		Ok(fx) => fx,
		Err(e) => return failed(id, "plugin", format!("loading {}: {e}", artifact.display()), None),
	};
	match run_on(&mut fx, variant, options, Some(sink)) {
		Ok(output) => {
			let mut frames = Vec::with_capacity(output.frames.len());
			for (i, frame) in output.frames.iter().enumerate() {
				let path = frame_path(frame_out, i);
				if let Err(e) = frame.write_raw(&path) {
					return failed(id, "harness", e.message, Some(output.trace));
				}
				frames.push(FrameRef {
					path,
					w: frame.width(),
					h: frame.height(),
					depth: frame.depth().bits(),
				});
			}
			Response {
				id,
				ok: true,
				frames,
				skipped: output.skipped,
				timing: Some(output.timing),
				trace: Some(output.trace),
				..Response::default()
			}
		}
		Err(failure) => {
			let kind = match &failure {
				RunFailure::Invalid { .. } => "invalid",
				RunFailure::Harness { .. } => "harness",
				_ => "plugin",
			};
			failed(id, kind, failure.message().to_string(), failure.trace().cloned())
		}
	}
}

//==== Client ===========================================================

/// Maps a variant's plugin to the artifact to load.
pub type ArtifactMap = Arc<dyn Fn(&PluginRef) -> Option<PathBuf> + Send + Sync>;

/// The command that starts a worker: by default this executable with
/// `worker` as its argument (what `aexlo` does).
#[derive(Debug, Clone)]
pub struct WorkerCommand {
	pub program: PathBuf,
	pub args: Vec<OsString>,
}

impl WorkerCommand {
	pub fn new(program: impl Into<PathBuf>, args: impl IntoIterator<Item = impl Into<OsString>>) -> Self {
		Self {
			program: program.into(),
			args: args.into_iter().map(Into::into).collect(),
		}
	}

	/// `<this executable> worker`.
	pub fn current_exe() -> Result<Self> {
		let exe = std::env::current_exe().map_err(|e| Error::harness(format!("locating the aexlo binary: {e}")))?;
		Ok(Self::new(exe, ["worker"]))
	}
}

/// A running worker process.
struct Live {
	child: Child,
	stdin: ChildStdin,
	lines: Receiver<String>,
	stderr: Arc<Mutex<String>>,
	stderr_thread: Option<std::thread::JoinHandle<()>>,
	/// The artifact the worker has loaded, and what it declared.
	loaded: Option<(PathBuf, PluginInfo)>,
}

impl Live {
	fn spawn(command: &WorkerCommand) -> Result<Self> {
		let mut child = Command::new(&command.program)
			.args(&command.args)
			.stdin(Stdio::piped())
			.stdout(Stdio::piped())
			.stderr(Stdio::piped())
			.spawn()
			.map_err(|e| Error::harness(format!("spawning worker {}: {e}", command.program.display())))?;
		let stdin = child.stdin.take().expect("piped");
		let stdout = child.stdout.take().expect("piped");
		let stderr_pipe = child.stderr.take().expect("piped");

		let (tx, lines) = mpsc::channel();
		std::thread::spawn(move || {
			for line in BufReader::new(stdout).lines() {
				let Ok(line) = line else { break };
				if tx.send(line).is_err() {
					break;
				}
			}
		});
		let stderr = Arc::new(Mutex::new(String::new()));
		let stderr_thread = {
			let stderr = stderr.clone();
			std::thread::spawn(move || {
				let mut reader = BufReader::new(stderr_pipe);
				let mut line = String::new();
				while reader.read_line(&mut line).is_ok_and(|n| n > 0) {
					if let Ok(mut buffer) = stderr.lock() {
						buffer.push_str(&line);
					}
					line.clear();
				}
			})
		};
		Ok(Self {
			child,
			stdin,
			lines,
			stderr,
			stderr_thread: Some(stderr_thread),
			loaded: None,
		})
	}

	fn send(&mut self, request: &Request) -> Result<()> {
		let line = json(request);
		writeln!(self.stdin, "{line}")
			.and_then(|()| self.stdin.flush())
			.map_err(|e| Error::harness(format!("writing to the worker: {e}")))
	}

	fn take_logs(&self) -> String {
		self.stderr
			.lock()
			.map(|mut s| std::mem::take(&mut *s))
			.unwrap_or_default()
	}

	/// Wait for the child to exit and every log line to arrive.
	fn reap(mut self) -> (String, String) {
		let status = self.child.wait();
		if let Some(thread) = self.stderr_thread.take() {
			let _ = thread.join();
		}
		let logs = self.take_logs();
		let description = match status {
			Ok(status) => describe_exit(status),
			Err(e) => format!("worker vanished: {e}"),
		};
		(description, logs)
	}

	fn kill(mut self) -> String {
		let _ = self.child.kill();
		self.reap().1
	}
}

/// How a request ended, from the client's point of view.
enum Waited {
	Response(Box<Response>),
	/// The worker exited: `(description, logs)`.
	Died(String, String),
	TimedOut,
}

/// What a worker's exit looks like: its signal or exception code.
fn describe_exit(status: std::process::ExitStatus) -> String {
	#[cfg(unix)]
	{
		use std::os::unix::process::ExitStatusExt;
		if let Some(signal) = status.signal() {
			let name = match signal {
				libc::SIGSEGV => "SIGSEGV",
				libc::SIGBUS => "SIGBUS",
				libc::SIGABRT => "SIGABRT",
				libc::SIGILL => "SIGILL",
				libc::SIGFPE => "SIGFPE",
				libc::SIGKILL => "SIGKILL",
				libc::SIGTRAP => "SIGTRAP",
				_ => "signal",
			};
			return format!("killed by signal {signal} ({name})");
		}
	}
	match status.code() {
		#[cfg(windows)]
		Some(code) if (code as u32) >= 0xC000_0000 => {
			let name = match code as u32 {
				0xC000_0005 => "access violation",
				0xC000_00FD => "stack overflow",
				0xC000_0409 => "stack buffer overrun / fail fast",
				0xC000_001D => "illegal instruction",
				0xC000_0094 => "integer divide by zero",
				_ => "exception",
			};
			format!("exception {:#010X} ({name})", code as u32)
		}
		Some(code) => format!("exited with code {code}"),
		None => "exited abnormally".to_string(),
	}
}

/// Runs variants in `aexlo worker` subprocesses: crash isolation, no
/// in-process breakpoints (§6.1). One worker is reused until it crashes or
/// times out; the next run then gets a fresh one.
pub struct WorkerClient {
	command: WorkerCommand,
	live: Option<Live>,
	/// Built artifacts for each plugin a variant may name.
	artifacts: ArtifactMap,
	next_id: u64,
	temp: PathBuf,
}

impl WorkerClient {
	/// `artifacts` maps a variant's plugin to the file to load (crates are
	/// built beforehand by the caller).
	pub fn new(command: WorkerCommand, artifacts: ArtifactMap) -> Result<Self> {
		static SESSION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
		let temp = std::env::temp_dir().join(format!(
			"aexlo-{}-{}",
			std::process::id(),
			SESSION.fetch_add(1, Ordering::Relaxed)
		));
		std::fs::create_dir_all(&temp).map_err(|e| Error::harness(format!("creating {}: {e}", temp.display())))?;
		Ok(Self {
			command,
			live: None,
			artifacts,
			next_id: 0,
			temp,
		})
	}

	fn id(&mut self) -> u64 {
		self.next_id += 1;
		self.next_id
	}

	/// Wait for the response to request `id`, following its events.
	fn wait(&mut self, id: u64, timeout: Duration, trace: &mut Trace) -> Waited {
		let Some(live) = self.live.as_ref() else {
			return Waited::Died("no worker".into(), String::new());
		};
		let deadline = Instant::now() + timeout;
		loop {
			let left = deadline.saturating_duration_since(Instant::now());
			match live.lines.recv_timeout(left) {
				Ok(line) => {
					let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
						continue;
					};
					if value.get("event").is_some() {
						if let Ok(event) = serde_json::from_value::<Event>(value)
							&& event.id == id
						{
							match (event.event.as_str(), event.phase.as_str()) {
								("command", "begin") | ("load", _) => {
									trace.unfinished = Some(if event.command.is_empty() {
										event.event.to_uppercase()
									} else {
										event.command
									});
								}
								("command", "end") => trace.unfinished = None,
								_ => {}
							}
						}
						continue;
					}
					match serde_json::from_value::<Response>(value) {
						Ok(response) if response.id == id || response.id == 0 => {
							return Waited::Response(Box::new(response));
						}
						Ok(_) => continue,
						Err(e) => {
							return Waited::Response(Box::new(Response {
								id,
								ok: false,
								error: Some(WireError {
									kind: "harness".into(),
									message: format!("malformed worker response ({e}): {line}"),
								}),
								..Response::default()
							}));
						}
					}
				}
				Err(RecvTimeoutError::Timeout) => return Waited::TimedOut,
				Err(RecvTimeoutError::Disconnected) => {
					let (description, logs) = self.live.take().map(Live::reap).unwrap_or_default();
					return Waited::Died(description, logs);
				}
			}
		}
	}

	/// Cancel the request in flight, give it [`CANCEL_GRACE`], then kill the
	/// worker. Returns whether the plugin honored the cancellation, and the logs.
	fn abandon(&mut self, id: u64) -> (bool, String) {
		if let Some(live) = self.live.as_mut() {
			let _ = live.send(&Request::Cancel { id });
		}
		let mut scratch = Trace::default();
		let honored = matches!(self.wait(id, CANCEL_GRACE, &mut scratch), Waited::Response(_));
		(honored, self.live.take().map(Live::kill).unwrap_or_default())
	}

	fn ensure_loaded(&mut self, artifact: &Path) -> std::result::Result<PluginInfo, RunFailure> {
		if self.live.is_none() {
			self.live = Some(Live::spawn(&self.command).map_err(|e| RunFailure::Harness { message: e.message })?);
		}
		if let Some((loaded, info)) = self.live.as_ref().and_then(|l| l.loaded.as_ref())
			&& loaded == artifact
		{
			return Ok(info.clone());
		}
		let id = self.id();
		let send = self.live.as_mut().map(|l| {
			l.send(&Request::Load {
				id,
				artifact: artifact.to_path_buf(),
			})
		});
		if let Some(Err(e)) = send {
			return Err(RunFailure::Harness { message: e.message });
		}
		let mut trace = Trace::default();
		match self.wait(id, LOAD_TIMEOUT, &mut trace) {
			Waited::Response(response) if response.ok => {
				let info = response.info.unwrap_or_default();
				if let Some(live) = self.live.as_mut() {
					live.loaded = Some((artifact.to_path_buf(), info.clone()));
					live.take_logs();
				}
				Ok(info)
			}
			Waited::Response(response) => {
				let logs = self.live.as_ref().map(Live::take_logs).unwrap_or_default();
				Err(response_failure(*response, Trace { logs, ..trace }))
			}
			Waited::Died(description, logs) => Err(RunFailure::Crash {
				message: format!("worker {description} while loading {}", artifact.display()),
				trace: Trace { logs, ..trace },
			}),
			Waited::TimedOut => {
				let (_, logs) = self.abandon(id);
				Err(RunFailure::Timeout {
					message: format!("loading {} took longer than {LOAD_TIMEOUT:?}", artifact.display()),
					trace: Trace { logs, ..trace },
				})
			}
		}
	}

	/// The plugin's declared capabilities, loading it if needed.
	pub fn info(&mut self, plugin: &PluginRef) -> std::result::Result<PluginInfo, RunFailure> {
		let artifact = (self.artifacts)(plugin).ok_or_else(|| RunFailure::Harness {
			message: format!("no artifact for plugin {}", plugin.label()),
		})?;
		self.ensure_loaded(&artifact)
	}
}

fn response_failure(response: Response, trace: Trace) -> RunFailure {
	let error = response.error.unwrap_or(WireError {
		kind: "harness".into(),
		message: "the worker reported a failure without saying why".into(),
	});
	let trace = match response.trace {
		Some(mut reported) => {
			reported.logs = trace.logs;
			reported
		}
		None => trace,
	};
	match error.kind.as_str() {
		"invalid" => RunFailure::Invalid { message: error.message },
		"harness" => RunFailure::Harness { message: error.message },
		_ => RunFailure::Plugin {
			message: error.message,
			trace,
		},
	}
}

impl Executor for WorkerClient {
	fn run(&mut self, variant: &Variant, options: &RunOptions) -> std::result::Result<RunOutput, RunFailure> {
		let artifact = (self.artifacts)(&variant.plugin).ok_or_else(|| RunFailure::Harness {
			message: format!("no artifact for plugin {}", variant.plugin.label()),
		})?;
		self.ensure_loaded(&artifact)?;

		let id = self.id();
		let frame_out = self.temp.join(format!("{id}.raw"));
		let request = Request::Run {
			id,
			variant: Box::new(variant.clone()),
			frame_out,
			options: options.clone(),
		};
		if let Some(Err(e)) = self.live.as_mut().map(|l| l.send(&request)) {
			return Err(RunFailure::Harness { message: e.message });
		}

		let timeout = Duration::from_secs_f64(variant.timeout * options.renders.max(1) as f64);
		let mut trace = Trace::default();
		match self.wait(id, timeout, &mut trace) {
			Waited::Response(response) => {
				let logs = self.live.as_ref().map(Live::take_logs).unwrap_or_default();
				if !response.ok {
					return Err(response_failure(*response, Trace { logs, ..trace }));
				}
				let mut frames = Vec::with_capacity(response.frames.len());
				for frame in &response.frames {
					let depth = PixelDepthKind::from_bits(frame.depth).ok_or_else(|| RunFailure::Harness {
						message: format!("worker reported a {}-bit frame", frame.depth),
					})?;
					let read = Frame::read_raw(&frame.path, frame.w, frame.h, depth);
					let _ = std::fs::remove_file(&frame.path);
					frames.push(read.map_err(|e| RunFailure::Harness { message: e.message })?);
				}
				let mut trace = response.trace.unwrap_or_default();
				trace.logs = logs;
				Ok(RunOutput {
					skipped: response.skipped,
					frames,
					trace,
					timing: response.timing.unwrap_or_default(),
				})
			}
			Waited::Died(description, logs) => {
				let last = trace.last_command().map(str::to_string);
				Err(RunFailure::Crash {
					message: match last {
						Some(command) => format!("worker {description} during PF_Cmd_{command}"),
						None => format!("worker {description}"),
					},
					trace: Trace { logs, ..trace },
				})
			}
			Waited::TimedOut => {
				let last = trace.last_command().map(str::to_string);
				let (honored, logs) = self.abandon(id);
				let ending = if honored {
					"cancelled".to_string()
				} else {
					format!("killed {:.0}s after ignoring cancellation", CANCEL_GRACE.as_secs_f64())
				};
				let place = last.map_or(String::new(), |command| format!(" in PF_Cmd_{command}"));
				Err(RunFailure::Timeout {
					message: format!("no result after {:.1}s{place}; {ending}", timeout.as_secs_f64()),
					trace: Trace { logs, ..trace },
				})
			}
		}
	}
}

impl Drop for WorkerClient {
	fn drop(&mut self) {
		if let Some(mut live) = self.live.take() {
			let _ = live.send(&Request::Shutdown { id: 0 });
			drop(live.stdin);
			let _ = live.child.wait();
		}
		let _ = std::fs::remove_dir_all(&self.temp);
	}
}
