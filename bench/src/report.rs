//! Presentation for measured benchmark points: leaderboard, GPU speedups, and
//! machine-readable export.
//!
//! Criterion owns the statistics for the criterion targets; this module is for
//! the custom timing loop ([`measure`](crate::measure)), which needs results
//! that are comparable *across* plugins and durable across runs.

use crate::{Caps, Resolution, Timing};
use std::path::{Path, PathBuf};

/// One timed `(plugin, mode, resolution, params)` point, ready to report.
#[derive(Clone, Debug)]
pub struct Measurement {
	pub plugin: String,
	/// Render path label, from [`RenderMode::label`](crate::RenderMode::label).
	pub mode: &'static str,
	pub caps: Caps,
	/// Parameter config label, from
	/// [`param_config_label`](crate::param_config_label).
	pub config: String,
	pub resolution: Resolution,
	pub timing: Timing,
}

impl Measurement {
	/// Median throughput in megapixels/second -- the sort key for the
	/// leaderboard, and what makes different frame sizes comparable.
	pub fn mpx_per_s(&self) -> f64 {
		self.timing.mpx_per_s(self.resolution.pixels())
	}

	pub fn median_ms(&self) -> f64 {
		self.timing.median().as_secs_f64() * 1.0e3
	}

	pub fn min_ms(&self) -> f64 {
		self.timing.min().as_secs_f64() * 1.0e3
	}
}

/// Sort rows fastest-first by throughput, the order [`print_leaderboard`] expects.
pub fn sort_by_throughput(rows: &mut [Measurement]) {
	rows.sort_by(|a, b| {
		b.mpx_per_s()
			.partial_cmp(&a.mpx_per_s())
			.unwrap_or(std::cmp::Ordering::Equal)
	});
}

/// Print the ranked table. `rows` should already be sorted (see
/// [`sort_by_throughput`]); the printed rank is just the row position.
pub fn print_leaderboard(rows: &[Measurement]) {
	println!("\n== Leaderboard (sorted by throughput) ==");
	println!(
		"{:>2}  {:<26} {:<7} {:<9} {:>10} {:>11} {:>10}  {:<8} {:<6} {:>6}",
		"#", "plugin", "mode", "res", "Mpx/s", "median ms", "min ms", "smart", "gpu", "params"
	);
	for (i, row) in rows.iter().enumerate() {
		println!(
			"{:>2}  {:<26} {:<7} {:<9} {:>10.1} {:>11.3} {:>10.3}  {:<8} {:<6} {:>6}",
			i + 1,
			truncate(&row.plugin, 26),
			row.mode,
			row.resolution.name,
			row.mpx_per_s(),
			row.median_ms(),
			row.min_ms(),
			row.caps.smart_render,
			row.caps.gpu,
			row.caps.param_count,
		);
	}
}

/// For plugins measured on both CPU and GPU at the same resolution and
/// parameter config, report how much the GPU path won by.
pub fn print_speedups(rows: &[Measurement]) {
	let mut printed_header = false;
	for cpu in rows.iter().filter(|r| r.mode == "cpu") {
		let Some(gpu) = rows.iter().find(|r| {
			r.mode == "gpu"
				&& r.plugin == cpu.plugin
				&& r.resolution.name == cpu.resolution.name
				&& r.config == cpu.config
		}) else {
			continue;
		};
		if !printed_header {
			println!("\n== GPU speedup (gpu vs cpu) ==");
			printed_header = true;
		}
		println!(
			"  {:<26} {:<9} {:>5.2}x",
			truncate(&cpu.plugin, 26),
			cpu.resolution.name,
			gpu.mpx_per_s() / cpu.mpx_per_s(),
		);
	}
}

/// Serialize rows as CSV, header included.
pub fn to_csv(rows: &[Measurement]) -> String {
	let mut csv = String::from(
		"plugin,mode,resolution,width,height,params,smart_render,gpu,param_count,samples,median_ms,min_ms,mpx_per_s\n",
	);
	for r in rows {
		csv.push_str(&format!(
			"{},{},{},{},{},{},{},{},{},{},{:.6},{:.6},{:.3}\n",
			csv_field(&r.plugin),
			r.mode,
			r.resolution.name,
			r.resolution.width,
			r.resolution.height,
			csv_field(&r.config),
			r.caps.smart_render,
			r.caps.gpu,
			r.caps.param_count,
			r.timing.samples.len(),
			r.median_ms(),
			r.min_ms(),
			r.mpx_per_s(),
		));
	}
	csv
}

/// Serialize rows as a JSON array. Hand-rolled to keep the bench crate free of
/// a serialization dependency -- the shape is fixed and every field is escaped.
pub fn to_json(rows: &[Measurement]) -> String {
	let mut json = String::from("[\n");
	for (i, r) in rows.iter().enumerate() {
		json.push_str(&format!(
			"  {{\"plugin\": \"{}\", \"mode\": \"{}\", \"resolution\": \"{}\", \"width\": {}, \"height\": {}, \"params\": \"{}\", \"smart_render\": {}, \"gpu\": {}, \"param_count\": {}, \"samples\": {}, \"median_ms\": {:.6}, \"min_ms\": {:.6}, \"mpx_per_s\": {:.3}}}{}\n",
			json_escape(&r.plugin),
			r.mode,
			json_escape(r.resolution.name),
			r.resolution.width,
			r.resolution.height,
			json_escape(&r.config),
			r.caps.smart_render,
			r.caps.gpu,
			r.caps.param_count,
			r.timing.samples.len(),
			r.median_ms(),
			r.min_ms(),
			r.mpx_per_s(),
			if i + 1 < rows.len() { "," } else { "" },
		));
	}
	json.push_str("]\n");
	json
}

/// Write `contents` to `path`, creating the parent directory. Reports failures
/// on stderr and returns `false` rather than aborting: an export problem should
/// never throw away numbers already printed to the terminal.
pub fn write_file(path: &Path, contents: &str) -> bool {
	if let Some(dir) = path.parent()
		&& !dir.as_os_str().is_empty()
		&& let Err(err) = std::fs::create_dir_all(dir)
	{
		eprintln!("aexlo-bench: could not create {}: {err}", dir.display());
		return false;
	}
	match std::fs::write(path, contents) {
		Ok(()) => {
			println!("Wrote {}", path.display());
			true
		}
		Err(err) => {
			eprintln!("aexlo-bench: failed to write {}: {err}", path.display());
			false
		}
	}
}

/// Append `ext` to a path used as a prefix, e.g. `out/summary` -> `out/summary.csv`.
/// Unlike `Path::with_extension` this keeps dots already in the file name.
pub fn with_extension(prefix: &Path, ext: &str) -> PathBuf {
	let mut s = prefix.as_os_str().to_os_string();
	s.push(".");
	s.push(ext);
	PathBuf::from(s)
}

fn truncate(s: &str, max: usize) -> String {
	if s.chars().count() <= max {
		s.to_string()
	} else {
		let head: String = s.chars().take(max.saturating_sub(1)).collect();
		format!("{head}…")
	}
}

/// Wrap a CSV field in quotes when it contains a comma or quote, escaping quotes.
fn csv_field(s: &str) -> String {
	if s.contains([',', '"', '\n']) {
		format!("\"{}\"", s.replace('"', "\"\""))
	} else {
		s.to_string()
	}
}

fn json_escape(s: &str) -> String {
	s.replace('\\', "\\\\").replace('"', "\\\"")
}
