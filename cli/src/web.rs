//! `aexlo dev --bin --web` — live preview in the browser.
//!
//! Same build-on-save render loop as [`crate::watch`], but instead of blitting
//! into a minifb window it serves the latest frame from a tiny local HTTP
//! server and streams the raw RGBA straight into a `<canvas>` (no PNG encode).
//! Useful on headless/remote hosts where a native window can't open, and a
//! stepping stone toward richer HTML-driven controls (params, scrubbing) that a
//! raw framebuffer can't offer.
//!
//! Threading: the main thread owns the watch+build loop (reusing
//! [`crate::watch::build_and_render`]); a background thread runs the blocking
//! HTTP server and reads the shared latest-frame state.

use std::path::Path;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::{Context, Result, anyhow};
use notify::{RecursiveMode, Watcher};
use tiny_http::{Header, Response, Server};

use crate::watch::{DEBOUNCE, build_and_render, is_relevant};

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

/// Latest render, shared between the build loop and the HTTP server.
///
/// `attempt` bumps on every rebuild (so the client can show "building…" even
/// when a build fails and the pixels don't change); `frame_gen` tags the build
/// that produced the RGBA currently held, so the client refetches `/frame` only
/// when there's actually a new image to draw.
struct Frame {
	rgba: Vec<u8>,
	w: u32,
	h: u32,
	attempt: u64,
	frame_gen: u64,
	status: Status,
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

	let state = Arc::new(Mutex::new(Frame {
		rgba: Vec::new(),
		w: 0,
		h: 0,
		attempt: 0,
		frame_gen: 0,
		status: Status::Building,
	}));

	// HTTP server on its own thread: recv() blocks, so it can't share the main
	// thread with the watch loop's polling.
	{
		let server = server.clone();
		let state = state.clone();
		std::thread::spawn(move || serve(&server, &state));
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

			match build_and_render(manifest, attempt) {
				Ok((rgba, w, h)) => {
					if let Ok(mut s) = state.lock() {
						s.rgba = rgba;
						s.w = w;
						s.h = h;
						s.frame_gen = attempt;
						s.status = Status::Ok;
					}
					println!("aexlo dev --bin --web: build #{attempt} → rendered {w}×{h}");
				}
				Err(err) => {
					set_status(&state, attempt, Status::Failed);
					eprintln!("\n─── build/render failed ───\n{err:#}\n");
				}
			}
		}

		std::thread::sleep(std::time::Duration::from_millis(50));
	}
}

/// Update just the attempt counter and status, leaving the last good frame in
/// place so the browser keeps showing it across failed builds.
fn set_status(state: &Mutex<Frame>, attempt: u64, status: Status) {
	if let Ok(mut s) = state.lock() {
		s.attempt = attempt;
		s.status = status;
	}
}

/// Blocking request loop: serves the viewer page, a cheap status endpoint the
/// client polls, and the raw RGBA of the latest frame.
fn serve(server: &Server, state: &Mutex<Frame>) {
	for request in server.incoming_requests() {
		let path = request.url().split('?').next().unwrap_or("/");
		let _ = match path {
			"/" => request.respond(html_response()),
			"/status" => {
				let (attempt, frame_gen, status) = {
					let s = state.lock().expect("frame state poisoned");
					(s.attempt, s.frame_gen, s.status)
				};
				request.respond(Response::from_string(format!("{attempt} {frame_gen} {}", status.as_str())))
			}
			"/frame" => {
				let (rgba, w, h, frame_gen) = {
					let s = state.lock().expect("frame state poisoned");
					(s.rgba.clone(), s.w, s.h, s.frame_gen)
				};
				let mut resp = Response::from_data(rgba);
				resp.add_header(header("X-Width", &w.to_string()));
				resp.add_header(header("X-Height", &h.to_string()));
				resp.add_header(header("X-Frame-Gen", &frame_gen.to_string()));
				resp.add_header(header("Content-Type", "application/octet-stream"));
				request.respond(resp)
			}
			_ => request.respond(Response::from_string("not found").with_status_code(404)),
		};
	}
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

/// Single-file viewer: polls `/status`, and redraws from `/frame` only when a
/// new build produced a new image. Kept dependency-free (no external assets).
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
  #dot { width: 9px; height: 9px; border-radius: 50%; background: #888; }
  #dot.building { background: #d6a419; animation: pulse 1s ease-in-out infinite; }
  #dot.ok { background: #3fb950; }
  #dot.failed { background: #f85149; }
  @keyframes pulse { 50% { opacity: .35; } }
  main { flex: 1; display: grid; place-items: center; overflow: auto; padding: 12px; }
  canvas { max-width: 100%; max-height: 100%; image-rendering: auto;
    background: #0000; box-shadow: 0 0 0 1px #262a31; }
</style>
</head>
<body>
<header><span id="dot"></span><span id="label">connecting…</span></header>
<main><canvas id="c" width="16" height="16"></canvas></main>
<script>
const cv = document.getElementById('c');
const ctx = cv.getContext('2d');
const dot = document.getElementById('dot');
const label = document.getElementById('label');
let drawn = -1;

async function drawFrame() {
  const res = await fetch('/frame');
  const w = +res.headers.get('X-Width');
  const h = +res.headers.get('X-Height');
  const gen = +res.headers.get('X-Frame-Gen');
  if (!w || !h) return;
  const buf = new Uint8ClampedArray(await res.arrayBuffer());
  if (buf.length < w * h * 4) return;
  cv.width = w; cv.height = h;
  ctx.putImageData(new ImageData(buf, w, h), 0, 0);
  drawn = gen;
}

async function tick() {
  try {
    const [attempt, frameGen, status] = (await (await fetch('/status')).text()).split(' ');
    dot.className = status;
    label.textContent = status === 'building' ? `build #${attempt} — building…`
      : status === 'failed' ? `build #${attempt} — failed (see terminal)`
      : `build #${attempt} — ${cv.width}×${cv.height}`;
    document.title = status === 'ok' ? `aexlo dev — ${cv.width}×${cv.height}`
      : `aexlo dev — ${status}`;
    if (+frameGen > 0 && +frameGen !== drawn) await drawFrame();
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
