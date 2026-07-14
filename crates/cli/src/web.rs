//! `aexlo dev --bin --web` — live, interactive preview in the browser.
//!
//! Same build-on-save render loop as [`crate::watch`], but instead of blitting
//! into a minifb window it serves the latest frame from a tiny local HTTP
//! server and streams the raw RGBA straight into a `<canvas>` (no PNG encode).
//! On top of that it exposes the plugin's parameters as HTML controls: editing
//! one POSTs to `/set`, the instance is re-rendered *without* rebuilding, and
//! the new frame streams back. Useful on headless/remote hosts and as the
//! interactive surface a raw framebuffer can't offer.
//!
//! Threading: the main thread owns the single [`aexlo::PluginInstance`] and its
//! watch/build/render loop; a background thread runs the blocking HTTP server,
//! reads the shared latest-frame + parameter snapshot, and forwards parameter
//! edits back to the main thread over a channel (the instance is not `Send`).

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use aexlo::{ParamValue, PluginInstance};
use anyhow::{Context, Result, anyhow};
use notify::{RecursiveMode, Watcher};
use tiny_http::{Header, Response, Server};

use crate::watch::{DEBOUNCE, build_and_load, is_relevant, render_instance};

/// Status of the most recent build attempt, surfaced to the browser tab title.
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

/// Latest render + parameter snapshot, shared between the build loop and the
/// HTTP server.
///
/// `attempt` bumps on every rebuild (so the client can show "building…" even
/// when a build fails and the pixels don't change); `frame_seq` bumps on every
/// new frame — build *or* parameter edit — so the client refetches `/frame`
/// exactly when there's a new image; `params_gen` bumps only when a rebuild may
/// have changed the parameter *set*, so the client rebuilds its controls (and
/// doesn't fight the value the user is dragging).
struct State {
	rgba: Vec<u8>,
	w: u32,
	h: u32,
	attempt: u64,
	frame_seq: u64,
	params_gen: u64,
	params_json: String,
	status: Status,
}

/// A parameter edit from the browser, forwarded to the instance-owning thread.
struct SetParam {
	index: usize,
	raw: String,
}

pub fn run(manifest: &Path, port: u16) -> Result<()> {
	let crate_dir = manifest.parent().context("manifest path has no parent directory")?;
	let src_dir = crate_dir.join("src");

	let server = Server::http(("127.0.0.1", port)).map_err(|e| anyhow!("starting web server: {e}"))?;
	let url = match server.server_addr().to_ip() {
		Some(addr) => format!("http://{addr}/"),
		None => format!("http://127.0.0.1:{port}/"),
	};
	let server = Arc::new(server);

	let state = Arc::new(Mutex::new(State {
		rgba: Vec::new(),
		w: 0,
		h: 0,
		attempt: 0,
		frame_seq: 0,
		params_gen: 0,
		params_json: "[]".to_string(),
		status: Status::Building,
	}));

	// HTTP server on its own thread: recv() blocks, and it forwards parameter
	// edits to the main thread (which owns the non-Send instance).
	let (set_tx, set_rx) = mpsc::channel::<SetParam>();
	{
		let server = server.clone();
		let state = state.clone();
		std::thread::spawn(move || serve(&server, &state, &set_tx));
	}

	// File watcher: forward raw events; the main loop debounces them.
	let (tx, rx) = mpsc::channel();
	let mut watcher = notify::recommended_watcher(move |res| {
		let _ = tx.send(res);
	})
	.context("creating file watcher")?;
	watcher
		.watch(&src_dir, RecursiveMode::Recursive)
		.with_context(|| format!("watching {}", src_dir.display()))?;
	let _ = watcher.watch(manifest, RecursiveMode::NonRecursive);

	println!("aexlo dev --bin --web: serving {url} (Ctrl+C to quit)");
	println!("aexlo dev --bin --web: watching {}", src_dir.display());
	open_browser(&url);

	// The live instance and the staged cdylib it was loaded from. Kept alive
	// across param edits so re-rendering doesn't rebuild; replaced on each build.
	let mut instance: Option<PluginInstance> = None;
	let mut staged: Option<PathBuf> = None;

	let mut attempt: u64 = 0;
	let mut pending: Option<Instant> = Some(Instant::now()); // build once on startup

	loop {
		// Collapse any file-change events into a single pending rebuild.
		while let Ok(res) = rx.try_recv() {
			if let Ok(event) = res
				&& event.paths.iter().any(|p| is_relevant(p))
			{
				pending = Some(Instant::now());
			}
		}

		if let Some(since) = pending
			&& since.elapsed() >= DEBOUNCE
		{
			pending = None;
			attempt += 1;
			set_status(&state, attempt, Status::Building);

			match build_and_load(manifest, attempt) {
				Ok((mut fx, new_staged)) => match render_instance(&mut fx) {
					Ok((rgba, w, h)) => {
						publish_frame(&state, rgba, w, h);
						publish_params(&state, &fx);
						println!("aexlo dev --bin --web: build #{attempt} → rendered {w}×{h}");
						instance = Some(fx);
						// Drop the previous instance's staged copy now that the new
						// one is live.
						if let Some(old) = staged.replace(new_staged) {
							let _ = std::fs::remove_file(old);
						}
					}
					Err(err) => {
						set_status(&state, attempt, Status::Failed);
						eprintln!("\n─── render failed ───\n{err:#}\n");
						let _ = std::fs::remove_file(new_staged);
					}
				},
				Err(err) => {
					set_status(&state, attempt, Status::Failed);
					eprintln!("\n─── build failed ───\n{err:#}\n");
				}
			}
		}

		// Apply any parameter edits, then re-render once for the whole batch.
		let mut changed = false;
		while let Ok(SetParam { index, raw }) = set_rx.try_recv() {
			if let Some(fx) = &mut instance {
				match apply_param(fx, index, &raw) {
					Ok(()) => changed = true,
					Err(err) => eprintln!("aexlo dev --bin --web: set param #{index} failed: {err:#}"),
				}
			}
		}
		if changed && let Some(fx) = &mut instance {
			let _ = fx.update_params_ui();
			match render_instance(fx) {
				Ok((rgba, w, h)) => {
					publish_frame(&state, rgba, w, h);
					// Republish values (a plugin may derive one param from another)
					// without bumping params_gen, so controls aren't rebuilt.
					refresh_param_values(&state, fx);
				}
				Err(err) => eprintln!("aexlo dev --bin --web: re-render failed: {err:#}"),
			}
		}

		std::thread::sleep(std::time::Duration::from_millis(50));
	}
}

/// Parse `raw` against the parameter's current type and write it into the
/// instance.
fn apply_param(fx: &mut PluginInstance, index: usize, raw: &str) -> Result<()> {
	let value = crate::parse_param_value(fx, index, raw)?;
	fx.set_param(index, value).with_context(|| format!("setting parameter #{index}"))
}

/// Store a freshly rendered frame and bump the draw sequence.
fn publish_frame(state: &Mutex<State>, rgba: Vec<u8>, w: u32, h: u32) {
	if let Ok(mut s) = state.lock() {
		s.rgba = rgba;
		s.w = w;
		s.h = h;
		s.frame_seq += 1;
		s.status = Status::Ok;
	}
}

/// Publish the parameter snapshot after a build and bump `params_gen` so the
/// client rebuilds its controls.
fn publish_params(state: &Mutex<State>, fx: &PluginInstance) {
	if let Ok(mut s) = state.lock() {
		s.params_json = params_json(fx);
		s.params_gen += 1;
	}
}

/// Refresh the parameter values without bumping `params_gen` (control set is
/// unchanged, so the client keeps its widgets).
fn refresh_param_values(state: &Mutex<State>, fx: &PluginInstance) {
	if let Ok(mut s) = state.lock() {
		s.params_json = params_json(fx);
	}
}

/// Update just the attempt counter and status, leaving the last good frame in
/// place so the browser keeps showing it across failed builds.
fn set_status(state: &Mutex<State>, attempt: u64, status: Status) {
	if let Ok(mut s) = state.lock() {
		s.attempt = attempt;
		s.status = status;
	}
}

/// Serialize the instance's parameters to a small JSON array the viewer turns
/// into controls. Hand-rolled to avoid a serde dependency for a handful of
/// flat objects.
fn params_json(fx: &PluginInstance) -> String {
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

fn json_escape(s: &str) -> String {
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
/// client polls, the current parameter list, the raw RGBA of the latest frame,
/// and accepts parameter edits.
fn serve(server: &Server, state: &Mutex<State>, set_tx: &mpsc::Sender<SetParam>) {
	for request in server.incoming_requests() {
		let (path, query) = split_url(request.url());
		let _ = match path {
			"/" => request.respond(html_response()),
			"/status" => {
				let (attempt, frame_seq, params_gen, status) = {
					let s = state.lock().expect("state poisoned");
					(s.attempt, s.frame_seq, s.params_gen, s.status)
				};
				request.respond(Response::from_string(format!(
					"{attempt} {frame_seq} {params_gen} {}",
					status.as_str()
				)))
			}
			"/params" => {
				let json = state.lock().expect("state poisoned").params_json.clone();
				let mut resp = Response::from_string(json);
				resp.add_header(header("Content-Type", "application/json"));
				request.respond(resp)
			}
			"/frame" => {
				let (rgba, w, h, frame_seq) = {
					let s = state.lock().expect("state poisoned");
					(s.rgba.clone(), s.w, s.h, s.frame_seq)
				};
				let mut resp = Response::from_data(rgba);
				resp.add_header(header("X-Width", &w.to_string()));
				resp.add_header(header("X-Height", &h.to_string()));
				resp.add_header(header("X-Frame-Seq", &frame_seq.to_string()));
				resp.add_header(header("Content-Type", "application/octet-stream"));
				request.respond(resp)
			}
			"/set" => {
				match parse_set(query) {
					Some(set) => {
						let _ = set_tx.send(set);
						request.respond(Response::from_string("ok"))
					}
					None => request.respond(Response::from_string("bad set").with_status_code(400)),
				}
			}
			_ => request.respond(Response::from_string("not found").with_status_code(404)),
		};
	}
}

/// Split `/set?i=3&v=0.5` into (`/set`, `i=3&v=0.5`).
fn split_url(url: &str) -> (&str, &str) {
	match url.split_once('?') {
		Some((path, query)) => (path, query),
		None => (url, ""),
	}
}

/// Parse a `/set` query of the form `i=<index>&v=<url-encoded value>`.
fn parse_set(query: &str) -> Option<SetParam> {
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
	Some(SetParam {
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
fn open_browser(url: &str) {
	let (cmd, args): (&str, &[&str]) = if cfg!(target_os = "macos") {
		("open", &[])
	} else if cfg!(target_os = "windows") {
		("cmd", &["/C", "start", ""])
	} else {
		("xdg-open", &[])
	};
	let _ = std::process::Command::new(cmd).args(args).arg(url).spawn();
}

/// Single-file viewer: polls `/status`, redraws from `/frame` on a new frame,
/// rebuilds parameter controls from `/params` on a rebuild, and POSTs edits to
/// `/set`. Kept dependency-free (no external assets).
const VIEWER_HTML: &str = r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>aexlo dev — building…</title>
<style>
  html, body { margin: 0; height: 100%; background: #14161a; color: #c7ccd4;
    font: 13px ui-monospace, SFMono-Regular, Menlo, monospace; }
  body { display: flex; flex-direction: column; }
  header { padding: 8px 12px; display: flex; gap: 10px; align-items: center;
    border-bottom: 1px solid #262a31; }
  #dot { width: 9px; height: 9px; border-radius: 50%; background: #888; flex: 0 0 auto; }
  #dot.building { background: #d6a419; animation: pulse 1s ease-in-out infinite; }
  #dot.ok { background: #3fb950; }
  #dot.failed { background: #f85149; }
  @keyframes pulse { 50% { opacity: .35; } }
  .body { flex: 1; display: flex; min-height: 0; }
  aside { width: 260px; flex: 0 0 auto; padding: 10px 12px; overflow: auto;
    border-right: 1px solid #262a31; }
  aside:empty::before { content: "no parameters"; color: #6b7280; }
  .row { margin-bottom: 12px; }
  .row label { display: block; margin-bottom: 4px; color: #9aa2ad; }
  .row input[type=number], .row select { width: 100%; box-sizing: border-box; }
  .pt { display: flex; gap: 6px; align-items: center; }
  .pt input { min-width: 0; box-sizing: border-box; }
  .pt input[type=range] { flex: 1; }
  .pt input[type=number] { width: 76px; flex: 0 0 auto; }
  input, select { background: #1c1f26; color: #c7ccd4; border: 1px solid #2f343c;
    border-radius: 4px; padding: 3px 5px; font: inherit; }
  input[type=range] { padding: 0; accent-color: #3fb950; }
  main { flex: 1; display: grid; place-items: center; overflow: auto; padding: 12px; min-width: 0; }
  canvas { max-width: 100%; max-height: 100%; background: #0000; box-shadow: 0 0 0 1px #262a31; }
</style>
</head>
<body>
<header><span id="dot"></span><span id="label">connecting…</span></header>
<div class="body">
  <aside id="params"></aside>
  <main><canvas id="c" width="16" height="16"></canvas></main>
</div>
<script>
const cv = document.getElementById('c');
const ctx = cv.getContext('2d');
const dot = document.getElementById('dot');
const label = document.getElementById('label');
const panel = document.getElementById('params');
let drawnFrame = -1, builtParams = -1;

async function drawFrame() {
  const res = await fetch('/frame');
  const w = +res.headers.get('X-Width'), h = +res.headers.get('X-Height');
  const seq = +res.headers.get('X-Frame-Seq');
  if (!w || !h) return;
  const buf = new Uint8ClampedArray(await res.arrayBuffer());
  if (buf.length < w * h * 4) return;
  cv.width = w; cv.height = h;
  ctx.putImageData(new ImageData(buf, w, h), 0, 0);
  drawnFrame = seq;
}

// Debounce per parameter so dragging a control doesn't flood the server.
const timers = {};
function set(index, value) {
  clearTimeout(timers[index]);
  timers[index] = setTimeout(() => {
    fetch('/set?i=' + index + '&v=' + encodeURIComponent(value));
  }, 40);
}

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
      row = numberRow(p, (p.kind === 'slider' || p.kind === 'popup') ? '1' : 'any');
    }
    panel.append(row);
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

async function tick() {
  try {
    const [attempt, frameSeq, paramsGen, status] =
      (await (await fetch('/status')).text()).split(' ');
    dot.className = status;
    label.textContent = status === 'building' ? `build #${attempt} — building…`
      : status === 'failed' ? `build #${attempt} — failed (see terminal)`
      : `build #${attempt} — ${cv.width}×${cv.height}`;
    document.title = status === 'ok' ? `aexlo dev — ${cv.width}×${cv.height}` : `aexlo dev — ${status}`;
    if (+paramsGen > 0 && +paramsGen !== builtParams) {
      buildControls(await (await fetch('/params')).json());
      builtParams = +paramsGen;
    }
    if (+frameSeq > 0 && +frameSeq !== drawnFrame) await drawFrame();
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
