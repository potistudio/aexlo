//! Shared interactive preview surface - a tiny local HTTP server that streams
//! the latest raw RGBA frame into a `<canvas>` and exposes a plugin's
//! parameters as live HTML controls.
//!
//! This is the "heart" both live surfaces share: `aexlo dev --bin --web` drives
//! it with a rebuild-on-save loop in front (compiler in the loop), while `aexlo
//! preview` drives it against a prebuilt artifact (no compiler). Editing a
//! control POSTs to `/set`; the owning thread re-renders the instance *without*
//! rebuilding and streams the new frame back.
//!
//! Threading: the caller's thread owns the single, non-`Send`
//! [`aexlo::PluginInstance`] and its render loop; a background thread runs the
//! blocking HTTP server, reads the shared latest-frame + parameter snapshot, and
//! forwards parameter edits back over a channel.

use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use aexlo::{ParamValue, PluginInstance};
use anyhow::{Result, anyhow};
use tiny_http::{Header, Response, Server};

/// Status of the most recent (re)load/build, surfaced to the browser tab title.
#[derive(Clone, Copy)]
enum Status {
	Building,
	Ok,
	Failed,
}

impl Status {
	fn as_str(self) -> &'static str {
		match self {
			Status::Building => "building",
			Status::Ok => "ok",
			Status::Failed => "failed",
		}
	}
}

/// An 8-bit RGBA image the browser draws.
#[derive(Clone, Default)]
pub(crate) struct Image {
	pub rgba: Vec<u8>,
	pub w: u32,
	pub h: u32,
}

/// Latest render + parameter snapshot, shared between the render loop and the
/// HTTP server.
///
/// `attempt` bumps on every (re)load (so the client can show "working…" even
/// when a load/build fails and the pixels don't change); `frame_seq` bumps on
/// every new frame - reload *or* parameter edit - so the client refetches
/// `/frame` exactly when there's a new image; `params_gen` bumps only when a
/// reload may have changed the parameter *set*, so the client rebuilds its
/// controls (and doesn't fight the value the user is dragging). The other
/// generations do the same for the preset list, the info panel and the
/// comparison images.
struct State {
	frame: Image,
	reference: Option<Image>,
	diff: Option<Image>,
	attempt: u64,
	frame_seq: u64,
	params_gen: u64,
	params_json: String,
	presets_gen: u64,
	presets_json: String,
	info_seq: u64,
	info_json: String,
	reference_seq: u64,
	status: Status,
}

/// What the browser asks the instance-owning thread to do.
pub(crate) enum Command {
	/// Set parameter `index` from its textual value.
	Set { index: usize, raw: String },
	/// Switch to the variant with this display id.
	Select(String),
	/// Render at this frame.
	Time(i64),
	/// Compare against `off`, `golden`, `depth:<bits>` or `render:<mode>`.
	Compare(String),
	/// Append the current state to the manifest as a preset with this name.
	Save(String),
}

/// A running interactive viewer: an HTTP server on a background thread plus the
/// shared state the owning thread publishes frames into.
pub(crate) struct Viewer {
	state: Arc<Mutex<State>>,
	commands: mpsc::Receiver<Command>,
	/// The URL the viewer is served at (e.g. `http://127.0.0.1:52143/`).
	pub url: String,
}

/// Start the viewer server on `port` (0 = OS-assigned) and return a handle the
/// owning thread publishes into.
pub(crate) fn start(port: u16) -> Result<Viewer> {
	let server = Server::http(("127.0.0.1", port)).map_err(|e| anyhow!("starting web server: {e}"))?;
	let url = match server.server_addr().to_ip() {
		Some(addr) => format!("http://{addr}/"),
		None => format!("http://127.0.0.1:{port}/"),
	};
	let server = Arc::new(server);

	let state = Arc::new(Mutex::new(State {
		frame: Image::default(),
		reference: None,
		diff: None,
		attempt: 0,
		frame_seq: 0,
		params_gen: 0,
		params_json: "[]".to_string(),
		presets_gen: 0,
		presets_json: "null".to_string(),
		info_seq: 0,
		info_json: "{}".to_string(),
		reference_seq: 0,
		status: Status::Building,
	}));

	// HTTP server on its own thread: recv() blocks, and it forwards commands
	// to the owning thread (which owns the non-Send instance).
	let (tx, commands) = mpsc::channel::<Command>();
	{
		let server = server.clone();
		let state = state.clone();
		std::thread::spawn(move || serve(&server, &state, &tx));
	}

	Ok(Viewer { state, commands, url })
}

impl Viewer {
	fn with(&self, f: impl FnOnce(&mut State)) {
		if let Ok(mut s) = self.state.lock() {
			f(&mut s);
		}
	}

	/// Mark attempt `attempt` as in progress (the browser shows a pulsing dot),
	/// leaving the last good frame on screen.
	pub fn begin_attempt(&self, attempt: u64) {
		self.with(|s| {
			s.attempt = attempt;
			s.status = Status::Building;
		});
	}

	/// Mark attempt `attempt` as failed, leaving the last good frame on screen.
	pub fn fail_attempt(&self, attempt: u64) {
		self.with(|s| {
			s.attempt = attempt;
			s.status = Status::Failed;
		});
	}

	/// Publish a freshly (re)loaded instance: a new frame *and* a fresh parameter
	/// set, so the client both redraws and rebuilds its controls.
	pub fn publish_reload(&self, fx: &PluginInstance, frame: Image) {
		let json = params_json(fx);
		self.with(|s| {
			publish(s, frame);
			s.params_json = json;
			s.params_gen += 1;
		});
	}

	/// Publish a re-render after parameter edits: a new frame plus refreshed
	/// values, *without* rebuilding controls (the control set is unchanged).
	pub fn publish_frame(&self, fx: &PluginInstance, frame: Image) {
		let json = params_json(fx);
		self.with(|s| {
			publish(s, frame);
			s.params_json = json;
		});
	}

	/// Publish the comparison images (`None` clears them).
	pub fn publish_reference(&self, reference: Option<Image>, diff: Option<Image>) {
		self.with(|s| {
			s.reference = reference;
			s.diff = diff;
			s.reference_seq += 1;
		});
	}

	/// Publish the info panel's JSON (variant, timings, strict findings, ...).
	pub fn publish_info(&self, json: String) {
		self.with(|s| {
			s.info_json = json;
			s.info_seq += 1;
		});
	}

	/// Publish the preset picker's JSON (`null` hides the picker).
	pub fn publish_presets(&self, json: String) {
		self.with(|s| {
			s.presets_json = json;
			s.presets_gen += 1;
		});
	}

	/// Every command queued since the last call.
	pub fn commands(&self) -> Vec<Command> {
		self.commands.try_iter().collect()
	}
}

/// Store a freshly rendered frame and bump the draw sequence.
fn publish(s: &mut State, frame: Image) {
	s.frame = frame;
	s.frame_seq += 1;
	s.status = Status::Ok;
}

/// Serialize the instance's parameters to a small JSON array the viewer turns
/// into controls. Hand-rolled to avoid a serde dependency for a handful of
/// flat objects.
pub(crate) fn params_json(fx: &PluginInstance) -> String {
	let mut out = String::from("[");
	for (i, (index, value)) in fx.param_values().into_iter().enumerate() {
		if i > 0 {
			out.push(',');
		}
		let name = json_escape(&fx.param_name(index).unwrap_or_default());
		let head = format!("{{\"index\":{index},\"name\":\"{name}\",");
		out.push_str(&head);
		match value {
			ParamValue::Float(v) => {
				out.push_str(&format!("\"kind\":\"float\",\"value\":{v}"));
				push_range(&mut out, fx.param_slider_range(index));
			}
			ParamValue::Fixed(v) => {
				out.push_str(&format!("\"kind\":\"fixed\",\"value\":{v}"));
				push_range(&mut out, fx.param_slider_range(index));
			}
			ParamValue::Slider(v) => {
				out.push_str(&format!("\"kind\":\"slider\",\"value\":{v}"));
				push_range(&mut out, fx.param_slider_range(index));
			}
			ParamValue::Popup(v) => {
				out.push_str(&format!("\"kind\":\"popup\",\"value\":{v}"));
				if let Some(choices) = fx.param_choices(index) {
					out.push_str(",\"choices\":[");
					for (i, c) in choices.iter().enumerate() {
						if i > 0 {
							out.push(',');
						}
						out.push('"');
						out.push_str(&json_escape(c));
						out.push('"');
					}
					out.push(']');
				}
			}
			ParamValue::Angle(v) => out.push_str(&format!("\"kind\":\"angle\",\"value\":{v}")),
			ParamValue::Checkbox(v) => out.push_str(&format!("\"kind\":\"checkbox\",\"value\":{v}")),
			ParamValue::Point { x, y } => out.push_str(&format!("\"kind\":\"point\",\"x\":{x},\"y\":{y}")),
			ParamValue::Path(id) => out.push_str(&format!("\"kind\":\"path\",\"value\":{id}")),
			ParamValue::Point3D { x, y, z } => {
				out.push_str(&format!("\"kind\":\"point3d\",\"x\":{x},\"y\":{y},\"z\":{z}"))
			}
			ParamValue::Color {
				red,
				green,
				blue,
				alpha,
			} => out.push_str(&format!(
				"\"kind\":\"color\",\"r\":{red},\"g\":{green},\"b\":{blue},\"a\":{alpha}"
			)),
		}
		out.push('}');
	}
	out.push(']');
	out
}

/// Append `,"min":..,"max":..` to a param object when the slider has a range.
fn push_range(out: &mut String, range: Option<(f64, f64)>) {
	if let Some((min, max)) = range {
		out.push_str(&format!(",\"min\":{min},\"max\":{max}"));
	}
}

pub(crate) fn json_escape(s: &str) -> String {
	let mut out = String::with_capacity(s.len());
	for c in s.chars() {
		match c {
			'"' => out.push_str("\\\""),
			'\\' => out.push_str("\\\\"),
			'\n' => out.push_str("\\n"),
			'\r' => out.push_str("\\r"),
			'\t' => out.push_str("\\t"),
			c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
			c => out.push(c),
		}
	}
	out
}

/// Blocking request loop: serves the viewer page, a cheap status endpoint the
/// client polls, the parameter list, presets and info as JSON, the raw RGBA of
/// the latest frame and of the comparison images, and accepts commands.
fn serve(server: &Server, state: &Mutex<State>, tx: &mpsc::Sender<Command>) {
	for request in server.incoming_requests() {
		let url = request.url().to_string();
		let (path, query) = split_url(&url);
		let json = |text: String| {
			let mut resp = Response::from_string(text);
			resp.add_header(header("Content-Type", "application/json"));
			resp
		};
		let image = |pick: fn(&State) -> Option<&Image>| {
			let s = state.lock().expect("state poisoned");
			let img = pick(&s).cloned().unwrap_or_default();
			let mut resp = Response::from_data(img.rgba);
			resp.add_header(header("X-Width", &img.w.to_string()));
			resp.add_header(header("X-Height", &img.h.to_string()));
			resp.add_header(header("X-Frame-Seq", &s.frame_seq.to_string()));
			resp.add_header(header("Content-Type", "application/octet-stream"));
			resp
		};
		let send = |command: Option<Command>| match command {
			Some(command) => {
				let _ = tx.send(command);
				Response::from_string("ok")
			}
			None => Response::from_string("bad request").with_status_code(400),
		};
		let arg = |name: &str| query_arg(query, name);
		let _ = match path {
			"/" => request.respond(html_response()),
			"/status" => {
				let line = {
					let s = state.lock().expect("state poisoned");
					format!(
						"{} {} {} {} {} {} {}",
						s.attempt,
						s.frame_seq,
						s.params_gen,
						s.status.as_str(),
						s.presets_gen,
						s.info_seq,
						s.reference_seq
					)
				};
				request.respond(Response::from_string(line))
			}
			"/params" => request.respond(json(state.lock().expect("state poisoned").params_json.clone())),
			"/presets" => request.respond(json(state.lock().expect("state poisoned").presets_json.clone())),
			"/info" => request.respond(json(state.lock().expect("state poisoned").info_json.clone())),
			"/frame" => request.respond(image(|s| Some(&s.frame))),
			"/reference" => request.respond(image(|s| s.reference.as_ref())),
			"/diff" => request.respond(image(|s| s.diff.as_ref())),
			"/set" => request.respond(send(parse_set(query))),
			"/select" => request.respond(send(arg("id").map(Command::Select))),
			"/time" => request.respond(send(arg("frame").and_then(|f| f.parse().ok()).map(Command::Time))),
			"/compare" => request.respond(send(arg("mode").map(Command::Compare))),
			"/save" => request.respond(send(arg("name").map(Command::Save))),
			_ => request.respond(Response::from_string("not found").with_status_code(404)),
		};
	}
}

/// The percent-decoded value of `name` in a query string.
fn query_arg(query: &str, name: &str) -> Option<String> {
	query
		.split('&')
		.filter_map(|pair| pair.split_once('='))
		.find(|(k, _)| *k == name)
		.map(|(_, v)| percent_decode(v))
}

/// Split `/set?i=3&v=0.5` into (`/set`, `i=3&v=0.5`).
fn split_url(url: &str) -> (&str, &str) {
	match url.split_once('?') {
		Some((path, query)) => (path, query),
		None => (url, ""),
	}
}

/// Parse a `/set` query of the form `i=<index>&v=<url-encoded value>`.
fn parse_set(query: &str) -> Option<Command> {
	let mut index = None;
	let mut raw = None;
	for pair in query.split('&') {
		let (k, v) = pair.split_once('=')?;
		match k {
			"i" => index = v.parse::<usize>().ok(),
			"v" => raw = Some(percent_decode(v)),
			_ => {}
		}
	}
	Some(Command::Set {
		index: index?,
		raw: raw?,
	})
}

/// Minimal percent-decode for `/set` values (numbers, commas, booleans).
fn percent_decode(s: &str) -> String {
	let bytes = s.as_bytes();
	let mut out = Vec::with_capacity(bytes.len());
	let mut i = 0;
	while i < bytes.len() {
		match bytes[i] {
			b'%' if i + 2 < bytes.len() => {
				let hex = |b: u8| (b as char).to_digit(16);
				if let (Some(hi), Some(lo)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
					out.push((hi * 16 + lo) as u8);
					i += 3;
					continue;
				}
				out.push(b'%');
				i += 1;
			}
			b'+' => {
				out.push(b' ');
				i += 1;
			}
			b => {
				out.push(b);
				i += 1;
			}
		}
	}
	String::from_utf8_lossy(&out).into_owned()
}

fn header(name: &str, value: &str) -> Header {
	Header::from_bytes(name.as_bytes(), value.as_bytes()).expect("valid header")
}

fn html_response() -> Response<std::io::Cursor<Vec<u8>>> {
	let mut resp = Response::from_string(VIEWER_HTML);
	resp.add_header(header("Content-Type", "text/html; charset=utf-8"));
	resp
}

/// Best-effort: open the preview URL in the default browser, ignoring failures
/// (headless hosts just use the printed URL).
pub(crate) fn open_browser(url: &str) {
	let (cmd, args): (&str, &[&str]) = if cfg!(target_os = "macos") {
		("open", &[])
	} else if cfg!(target_os = "windows") {
		("cmd", &["/C", "start", ""])
	} else {
		("xdg-open", &[])
	};
	let _ = std::process::Command::new(cmd).args(args).arg(url).spawn();
}

/// Single-file viewer: polls `/status`, redraws from `/frame` (and the
/// comparison images) when they change, rebuilds parameter controls and the
/// preset picker when their sets change, and sends edits as commands. Kept
/// dependency-free (no external assets).
const VIEWER_HTML: &str = r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>aexlo - working…</title>
<style>
  :root { --bg: #14161a; --panel: #1c1f26; --line: #262a31; --text: #c7ccd4; --muted: #8b939e; --ok: #3fb950;
    --warn: #d6a419; --bad: #f85149; }
  html, body { margin: 0; height: 100%; background: var(--bg); color: var(--text);
    font: 13px ui-monospace, SFMono-Regular, Menlo, monospace; }
  body { display: flex; flex-direction: column; }
  header { padding: 8px 12px; display: flex; flex-wrap: wrap; gap: 10px; align-items: center;
    border-bottom: 1px solid var(--line); }
  header .grow { flex: 1; }
  #dot { width: 9px; height: 9px; border-radius: 50%; background: #888; flex: 0 0 auto; }
  #dot.building { background: var(--warn); animation: pulse 1s ease-in-out infinite; }
  #dot.ok { background: var(--ok); }
  #dot.failed { background: var(--bad); }
  @keyframes pulse { 50% { opacity: .35; } }
  .group { display: flex; gap: 6px; align-items: center; }
  .group > span { color: var(--muted); }
  .hidden { display: none !important; }
  .body { flex: 1; display: flex; min-height: 0; }
  aside { width: 260px; flex: 0 0 auto; padding: 10px 12px; overflow: auto; border-right: 1px solid var(--line); }
  aside#info { border-right: 0; border-left: 1px solid var(--line); width: 280px; }
  aside#params:empty::before { content: "no parameters"; color: #6b7280; }
  .row { margin-bottom: 12px; }
  .row label { display: block; margin-bottom: 4px; color: #9aa2ad; }
  .row input[type=number], .row select { width: 100%; box-sizing: border-box; }
  .pt { display: flex; gap: 6px; align-items: center; }
  .pt input { min-width: 0; box-sizing: border-box; }
  .pt input[type=range] { flex: 1; }
  .pt input[type=number] { width: 76px; flex: 0 0 auto; }
  input, select, button { background: var(--panel); color: var(--text); border: 1px solid #2f343c;
    border-radius: 4px; padding: 3px 5px; font: inherit; }
  button { cursor: pointer; }
  input[type=range] { padding: 0; accent-color: var(--ok); }
  main { flex: 1; display: flex; flex-direction: column; align-items: center; justify-content: center;
    overflow: auto; padding: 12px; min-width: 0; gap: 8px; }
  canvas { max-width: 100%; max-height: 100%; box-shadow: 0 0 0 1px var(--line); image-rendering: pixelated; }
  #wipe { width: 60%; }
  h3 { margin: 0 0 6px; font-size: 12px; color: var(--muted); font-weight: normal; text-transform: uppercase; }
  table { border-collapse: collapse; width: 100%; margin-bottom: 14px; }
  td { padding: 2px 0; vertical-align: top; }
  td:last-child { text-align: right; }
  .bad { color: var(--bad); } .good { color: var(--ok); }
  #toast { color: var(--muted); }
</style>
</head>
<body>
<header>
  <span id="dot"></span><span id="label">connecting…</span>
  <span class="group hidden" id="presetGroup"><span>preset</span><select id="preset"></select></span>
  <span class="group"><span>frame</span>
    <input type="range" id="scrub" min="0" max="240" value="0">
    <input type="number" id="frameNum" min="0" value="0" style="width:64px"></span>
  <span class="group"><span>compare</span>
    <select id="compare">
      <option value="off">off</option>
      <option value="golden">golden</option>
      <option value="depth:8">8 bpc</option><option value="depth:16">16 bpc</option><option value="depth:32">32 bpc</option>
      <option value="render:auto">auto</option><option value="render:legacy">legacy</option>
      <option value="render:smart">smart</option><option value="render:gpu">gpu</option>
    </select>
    <select id="view"><option value="wipe">wipe</option><option value="side">side by side</option><option value="diff">diff</option></select>
  </span>
  <span class="grow"></span>
  <span class="group hidden" id="saveGroup"><input id="saveName" placeholder="preset_name" size="14">
    <button id="save">Save as preset</button></span>
  <span id="toast"></span>
</header>
<div class="body">
  <aside id="params"></aside>
  <main>
    <canvas id="c" width="16" height="16"></canvas>
    <input type="range" id="wipe" min="0" max="1000" value="500" class="hidden">
  </main>
  <aside id="info"></aside>
</div>
<script>
const $ = id => document.getElementById(id);
const cv = $('c'), ctx = cv.getContext('2d');
const dot = $('dot'), label = $('label'), panel = $('params');
let drawnFrame = -1, builtParams = -1, builtPresets = -1, shownInfo = -1, drawnRef = -1;
let frame = null, reference = null, diff = null;

async function fetchImage(path) {
  const res = await fetch(path);
  const w = +res.headers.get('X-Width'), h = +res.headers.get('X-Height');
  if (!w || !h) return null;
  const buf = new Uint8ClampedArray(await res.arrayBuffer());
  if (buf.length < w * h * 4) return null;
  return new ImageData(buf, w, h);
}

// Small frames are shown enlarged, pixel for pixel.
function zoom() {
  const z = Math.max(1, Math.floor(384 / Math.max(cv.width, cv.height)));
  cv.style.width = cv.width * z + 'px';
}

// Draw the frame, and the comparison the view asks for.
function redraw() {
  if (!frame) return;
  draw();
  zoom();
}
function draw() {
  const view = $('view').value, comparing = $('compare').value !== 'off' && reference;
  $('wipe').classList.toggle('hidden', !(comparing && view === 'wipe'));
  if (comparing && view === 'side') {
    cv.width = frame.width + reference.width + 8; cv.height = Math.max(frame.height, reference.height);
    ctx.clearRect(0, 0, cv.width, cv.height);
    ctx.putImageData(frame, 0, 0);
    ctx.putImageData(reference, frame.width + 8, 0);
    return;
  }
  cv.width = frame.width; cv.height = frame.height;
  if (comparing && view === 'diff' && diff) { ctx.putImageData(diff, 0, 0); return; }
  ctx.putImageData(frame, 0, 0);
  if (comparing && view === 'wipe') {
    const x = Math.round(frame.width * $('wipe').value / 1000);
    const w = Math.max(0, Math.min(reference.width, frame.width) - x);
    if (w > 0) ctx.putImageData(reference, 0, 0, x, 0, w, reference.height);
    ctx.fillStyle = '#ffffffaa'; ctx.fillRect(x, 0, 1, frame.height);
  }
}

// Debounce per key so dragging a control doesn't flood the server.
const timers = {};
function send(key, url) {
  clearTimeout(timers[key]);
  timers[key] = setTimeout(() => fetch(url), 40);
}
function set(index, value) { send('p' + index, '/set?i=' + index + '&v=' + encodeURIComponent(value)); }

function labelled(p) {
  const row = el('div', 'row');
  row.append(el('label', '', `#${p.index} ${p.name}`));
  return row;
}

// Plain number field, for ranges the plugin didn't bound (and for angles).
function numberRow(p, step) {
  const row = labelled(p);
  const inp = document.createElement('input');
  inp.type = 'number'; inp.step = step; inp.value = p.value;
  inp.oninput = () => set(p.index, inp.value);
  row.append(inp);
  return row;
}

// Range slider paired with a number box, kept in sync, for bounded sliders.
function sliderRow(p, step) {
  const row = labelled(p);
  const wrap = el('div', 'pt');
  const range = document.createElement('input');
  range.type = 'range'; range.min = p.min; range.max = p.max;
  range.step = step === '1' ? '1' : (p.max - p.min) / 1000 || 'any';
  range.value = p.value;
  const num = numInput(p.value); num.step = step;
  const push = v => { range.value = v; num.value = v; set(p.index, v); };
  range.oninput = () => push(range.value);
  num.oninput = () => push(num.value);
  wrap.append(range, num); row.append(wrap);
  return row;
}

// Dropdown for popups whose choice labels the plugin exposed.
function selectRow(p) {
  const row = labelled(p);
  const sel = document.createElement('select');
  p.choices.forEach((c, i) => {
    const o = document.createElement('option');
    o.value = i + 1; o.textContent = c; sel.append(o); // popup values are 1-based
  });
  sel.value = p.value;
  sel.oninput = () => set(p.index, sel.value);
  row.append(sel);
  return row;
}

function buildControls(params) {
  panel.innerHTML = '';
  for (const p of params) {
    let row;
    if (p.kind === 'checkbox') {
      row = labelled(p);
      const inp = document.createElement('input');
      inp.type = 'checkbox'; inp.checked = p.value;
      inp.oninput = () => set(p.index, inp.checked ? 'true' : 'false');
      row.querySelector('label').prepend(inp, ' ');
    } else if (p.kind === 'point') {
      row = labelled(p);
      const wrap = el('div', 'pt');
      const x = numInput(p.x), y = numInput(p.y);
      const push = () => set(p.index, x.value + ',' + y.value);
      x.oninput = push; y.oninput = push;
      wrap.append(x, y); row.append(wrap);
    } else if (p.kind === 'point3d') {
      row = labelled(p);
      const wrap = el('div', 'pt');
      const x = numInput(p.x), y = numInput(p.y), z = numInput(p.z);
      const push = () => set(p.index, x.value + ',' + y.value + ',' + z.value);
      x.oninput = push; y.oninput = push; z.oninput = push;
      wrap.append(x, y, z); row.append(wrap);
    } else if (p.kind === 'color') {
      row = labelled(p);
      const wrap = el('div', 'pt');
      const col = document.createElement('input');
      col.type = 'color'; col.value = rgbHex(p.r, p.g, p.b);
      const a = numInput(p.a); a.min = 0; a.max = 255;
      const push = () => { const [r, g, b] = hexRgb(col.value); set(p.index, `${r},${g},${b},${a.value}`); };
      col.oninput = push; a.oninput = push;
      wrap.append(col, a); row.append(wrap);
    } else if (p.kind === 'popup' && p.choices) {
      row = selectRow(p);
    } else if ((p.kind === 'float' || p.kind === 'fixed' || p.kind === 'slider') && p.min !== undefined) {
      row = sliderRow(p, p.kind === 'slider' ? '1' : 'any');
    } else {
      // angle, unbounded sliders, or a popup with no labels
      row = numberRow(p, (p.kind === 'slider' || p.kind === 'popup' || p.kind === 'path') ? '1' : 'any');
    }
    panel.append(row);
  }
}

function buildPresets(presets) {
  const has = presets && presets.variants && presets.variants.length > 0;
  $('presetGroup').classList.toggle('hidden', !has);
  $('saveGroup').classList.toggle('hidden', !(presets && presets.can_save));
  if (!has) return;
  const sel = $('preset');
  sel.innerHTML = '';
  if (!presets.current) {
    const o = document.createElement('option'); o.value = ''; o.textContent = '(none)'; sel.append(o);
  }
  for (const id of presets.variants) {
    const o = document.createElement('option'); o.value = id; o.textContent = id; sel.append(o);
  }
  sel.value = presets.current || '';
}

function showInfo(info) {
  const box = $('info');
  box.innerHTML = '';
  const table = (title, rows) => {
    if (!rows.length) return;
    box.append(el('h3', '', title));
    const t = el('table');
    for (const [k, v, cls] of rows) {
      const tr = el('tr'); tr.append(el('td', '', k)); const td = el('td', cls || '', v); tr.append(td); t.append(tr);
    }
    box.append(t);
  };
  const ms = s => (s * 1000).toFixed(3) + ' ms';
  table('variant', [
    ['id', info.variant || '(no preset)'],
    ['size', info.size || ''], ['depth', info.depth ? info.depth + ' bpc' : ''],
    ['render', info.render || ''], ['time', info.time || ''],
  ].filter(r => r[1]));
  if (info.timing) table('last render', [
    ['wall', ms(info.timing.wall)], ['pre-render', ms(info.timing.pre_render)],
    ['render', ms(info.timing.render)], ['gpu', ms(info.timing.gpu)], ['host overhead', ms(info.timing.host)],
  ]);
  if (info.strict) {
    const rows = [];
    if (info.strict.unwritten != null)
      rows.push(['unwritten pixels', String(info.strict.unwritten), info.strict.unwritten ? 'bad' : 'good']);
    for (const v of info.strict.violations) rows.push(['', v, 'bad']);
    if (!rows.length) rows.push(['', 'nothing found', 'good']);
    table('strict', rows);
  }
  if (info.compare) table('compare', [[info.compare.against, info.compare.summary, info.compare.passed ? 'good' : 'bad']]);
  if (info.error) table('error', [['', info.error, 'bad']]);
  if (info.toast) $('toast').textContent = info.toast;
  if (info.frame != null && document.activeElement !== $('scrub') && document.activeElement !== $('frameNum')) {
    $('scrub').value = info.frame; $('frameNum').value = info.frame;
  }
}

function el(tag, cls, text) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text != null) e.textContent = text;
  return e;
}
function numInput(v) { const i = document.createElement('input'); i.type = 'number'; i.step = 'any'; i.value = v; return i; }
function rgbHex(r, g, b) { return '#' + [r, g, b].map(c => c.toString(16).padStart(2, '0')).join(''); }
function hexRgb(h) { return [1, 3, 5].map(i => parseInt(h.slice(i, i + 2), 16)); }

$('preset').oninput = () => fetch('/select?id=' + encodeURIComponent($('preset').value));
const scrub = v => { $('scrub').value = v; $('frameNum').value = v; send('time', '/time?frame=' + v); };
$('scrub').oninput = () => scrub($('scrub').value);
$('frameNum').oninput = () => scrub($('frameNum').value);
$('compare').oninput = () => fetch('/compare?mode=' + encodeURIComponent($('compare').value));
$('view').oninput = redraw;
$('wipe').oninput = redraw;
$('save').onclick = () => {
  const name = $('saveName').value.trim();
  if (name) fetch('/save?name=' + encodeURIComponent(name));
};

async function tick() {
  try {
    const [attempt, frameSeq, paramsGen, status, presetsGen, infoSeq, refSeq] =
      (await (await fetch('/status')).text()).split(' ');
    dot.className = status;
    label.textContent = status === 'building' ? `#${attempt} - working…`
      : status === 'failed' ? `#${attempt} - failed (see terminal)`
      : `#${attempt} · ${frame ? frame.width + '×' + frame.height : ''}`;
    document.title = status === 'ok' && frame ? `aexlo - ${frame.width}×${frame.height}` : `aexlo - ${status}`;
    if (+paramsGen > 0 && +paramsGen !== builtParams) {
      buildControls(await (await fetch('/params')).json());
      builtParams = +paramsGen;
    }
    if (+presetsGen !== builtPresets) {
      buildPresets(await (await fetch('/presets')).json());
      builtPresets = +presetsGen;
    }
    if (+infoSeq !== shownInfo) {
      showInfo(await (await fetch('/info')).json());
      shownInfo = +infoSeq;
    }
    let dirty = false;
    if (+frameSeq > 0 && +frameSeq !== drawnFrame) {
      frame = await fetchImage('/frame'); drawnFrame = +frameSeq; dirty = true;
    }
    if (+refSeq !== drawnRef) {
      reference = await fetchImage('/reference'); diff = await fetchImage('/diff'); drawnRef = +refSeq; dirty = true;
    }
    if (dirty) redraw();
  } catch (_) {
    dot.className = ''; label.textContent = 'disconnected';
  }
}
setInterval(tick, 250);
tick();
</script>
</body>
</html>
"#;
