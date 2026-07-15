//! JSONL trace writer — the probe's single output channel.
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

pub struct Trace {
	writer: Mutex<Option<BufWriter<File>>>,
	path: PathBuf,
	seq: AtomicU64,
	started: Instant,
}

static TRACE: OnceLock<Trace> = OnceLock::new();

/// Global trace instance; the sink file is created on first access.
pub fn trace() -> &'static Trace {
	TRACE.get_or_init(|| {
		let path = resolve_path();
		if let Some(parent) = path.parent() {
			let _ = fs::create_dir_all(parent);
		}

		let trace = Trace {
			writer: Mutex::new(File::create(&path).ok().map(BufWriter::new)),
			path,
			seq: AtomicU64::new(0),
			started: Instant::now(),
		};

		trace.emit(
			"trace_open",
			serde_json::json!({
				"probe_version": PROBE_VERSION,
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
/// About dialog echoes the resolved path.
fn resolve_path() -> PathBuf {
	if let Ok(path) = std::env::var("AEXLO_PROBE_TRACE") {
		return PathBuf::from(path);
	}

	let dir = std::env::var("AEXLO_PROBE_DIR")
		.map(PathBuf::from)
		.unwrap_or_else(|_| std::env::temp_dir().join("aexlo-probe"));

	dir.join(format!("trace-{}-pid{}.jsonl", unix_time(), std::process::id()))
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
