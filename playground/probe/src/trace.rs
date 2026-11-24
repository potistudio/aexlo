//! JSONL trace writer - the probe's single output channel.
//!
//! Every host interaction is appended as one JSON object per line and flushed
//! immediately, so a host crash cannot swallow the tail of the trace.

use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

pub const PROBE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Bumped whenever events, fact names or fact values change, so `diff`
/// refuses to compare traces whose keys do not line up. 2: SmartRender
/// variant, per-depth `smart.<bpc>.*` facts. 3: drop
/// `smart.<bpc>.checkout_layer.invalid_index(99)` - real hosts throw on that
/// checkout instead of answering - and give `world.iterate` its own fixed
/// gray input instead of a world another check left behind.
pub const TRACE_SCHEMA: u32 = 3;

pub struct Trace {
	writer: Mutex<Option<BufWriter<File>>>,
	path: PathBuf,
	seq: AtomicU64,
	started: Instant,
}

static TRACE: OnceLock<Trace> = OnceLock::new();

/// `AEXLO_PROBE_VARIANT` names the variant a scripted run measures; a probe
/// whose variant does not match it writes no trace. `playground ae` sets it
/// so two installed probes cannot interleave their events into the one
/// `AEXLO_PROBE_TRACE` file. An unset variable keeps every other caller's
/// behavior: trace exactly as before.
fn writes_enabled() -> bool {
	match std::env::var("AEXLO_PROBE_VARIANT") {
		Ok(variant) => variant == crate::VARIANT,
		Err(_) => true,
	}
}

/// Global trace instance; the sink file is created on first access.
pub fn trace() -> &'static Trace {
	TRACE.get_or_init(|| {
		let path = resolve_path();
		if let Some(parent) = path.parent() {
			let _ = fs::create_dir_all(parent);
		}

		let writer = writes_enabled().then(|| File::create(&path).ok().map(BufWriter::new));
		let trace = Trace {
			writer: Mutex::new(writer.flatten()),
			path,
			seq: AtomicU64::new(0),
			started: Instant::now(),
		};

		trace.emit(
			"trace_open",
			serde_json::json!({
				"probe_version": PROBE_VERSION,
				"trace_schema": TRACE_SCHEMA,
				"variant": crate::VARIANT,
				"pid": std::process::id(),
				"exe": std::env::current_exe().ok().map(|p| p.display().to_string()),
				"os": std::env::consts::OS,
				"unix_time": unix_time(),
			}),
		);

		trace
	})
}

/// Trace destination: `AEXLO_PROBE_TRACE` (exact file path) wins, then
/// `AEXLO_PROBE_DIR`, then the OS temp dir. Inside After Effects neither env
/// var is usually set, so traces land in `%TEMP%/aexlo-probe/`. The effect's
/// About dialog echoes the resolved path. Auto-named files carry the variant,
/// so the legacy and smart effects in one After Effects process never share
/// a file.
fn resolve_path() -> PathBuf {
	if let Ok(path) = std::env::var("AEXLO_PROBE_TRACE") {
		return PathBuf::from(path);
	}

	let dir = std::env::var("AEXLO_PROBE_DIR")
		.map(PathBuf::from)
		.unwrap_or_else(|_| std::env::temp_dir().join("aexlo-probe"));

	let tag = if crate::VARIANT == "legacy" {
		String::new()
	} else {
		format!("{}-", crate::VARIANT)
	};
	dir.join(format!("trace-{tag}{}-pid{}.jsonl", unix_time(), std::process::id()))
}

fn unix_time() -> u64 {
	std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.map(|d| d.as_secs())
		.unwrap_or(0)
}

impl Trace {
	pub fn path(&self) -> &Path {
		&self.path
	}

	/// Append one event line. `fields` must be a JSON object; `seq`, `t_ms`,
	/// `tid` and `event` are injected before it.
	pub fn emit(&self, event: &str, fields: serde_json::Value) {
		let mut object = serde_json::Map::new();
		object.insert("seq".into(), self.seq.fetch_add(1, Ordering::Relaxed).into());
		object.insert(
			"t_ms".into(),
			((self.started.elapsed().as_secs_f64() * 1_000_000.0).round() / 1000.0).into(),
		);
		object.insert("tid".into(), format!("{:?}", std::thread::current().id()).into());
		object.insert("event".into(), event.into());

		if let serde_json::Value::Object(map) = fields {
			object.extend(map);
		}

		if let Ok(mut guard) = self.writer.lock()
			&& let Some(writer) = guard.as_mut()
		{
			let _ = serde_json::to_writer(&mut *writer, &serde_json::Value::Object(object));
			let _ = writer.write_all(b"\n");
			let _ = writer.flush();
		}
	}
}
