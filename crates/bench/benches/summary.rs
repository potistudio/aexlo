//! Cross-plugin render leaderboard with CSV/JSON export.
//!
//! Criterion times each benchmark in isolation and never ranks them against each
//! other. This target fills that gap: it drives every selected plugin through
//! its own lightweight timing loop at a single resolution, then prints a table
//! sorted by throughput and writes machine-readable `CSV`/`JSON` for tracking
//! results over time.
//!
//! Honors the same `AEXLO_BENCH_*` knobs as the criterion benches, plus:
//! * `AEXLO_BENCH_SAMPLES` -- render iterations timed per plugin (default 30).
//! * `AEXLO_BENCH_OUT` -- output path prefix (default `target/aexlo-bench/summary`);
//!   `<prefix>.csv` and `<prefix>.json` are written.
//!
//! Being `harness = false`, it owns `main` and ignores criterion CLI flags.

use aexlo::PluginInstance;
use aexlo_bench::{
	Caps, RenderMode, Resolution, apply_param_config, bench_modes, bench_plugins, bench_resolutions, capabilities,
	param_config_label, param_configs, set_bench_input,
};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// One timed (plugin, mode) measurement.
struct Measurement {
	plugin: String,
	mode: &'static str,
	caps: Caps,
	config: String,
	median: Duration,
	mpx_per_s: f64,
}

fn main() {
	let plugins = bench_plugins();
	if plugins.is_empty() {
		eprintln!("aexlo-bench: no plugins resolved; nothing to summarize (see AEXLO_BENCH_PLUGINS).");
		return;
	}

	let resolution = summary_resolution();
	let configs = param_configs();
	let config = configs.first().cloned().unwrap_or_default();
	let config_label = param_config_label(&config);
	let samples = std::env::var("AEXLO_BENCH_SAMPLES")
		.ok()
		.and_then(|v| v.parse::<usize>().ok())
		.filter(|&n| n > 0)
		.unwrap_or(30);

	println!(
		"aexlo-bench summary: {} plugin(s) @ {} ({}x{}), params={config_label}, {samples} samples/plugin\n",
		plugins.len(),
		resolution.name,
		resolution.width,
		resolution.height,
	);

	let mut measurements = Vec::new();
	for (label, path) in &plugins {
		let modes = match PluginInstance::try_load(path) {
			Ok(probe) => bench_modes(&probe),
			Err(err) => {
				eprintln!("aexlo-bench: {label}: load failed, skipping: {err:?}");
				continue;
			}
		};

		for mode in modes {
			match measure(path, mode, resolution, &config, samples) {
				Ok((median, caps)) => {
					let mpx_per_s = resolution.pixels() as f64 / median.as_secs_f64() / 1.0e6;
					eprintln!("  measured {label} [{}] -> {mpx_per_s:.1} Mpx/s", mode.label());
					measurements.push(Measurement {
						plugin: label.clone(),
						mode: mode.label(),
						caps,
						config: config_label.clone(),
						median,
						mpx_per_s,
					});
				}
				Err(err) => eprintln!("aexlo-bench: {label} [{}]: skipped ({err})", mode.label()),
			}
		}
	}

	if measurements.is_empty() {
		eprintln!("aexlo-bench: no measurements collected.");
		return;
	}

	measurements.sort_by(|a, b| {
		b.mpx_per_s
			.partial_cmp(&a.mpx_per_s)
			.unwrap_or(std::cmp::Ordering::Equal)
	});
	print_leaderboard(&measurements, resolution);
	print_speedups(&measurements);
	write_outputs(&measurements, resolution);
}

/// Load a fresh instance, apply the parameter config, feed the input, warm up,
/// then return the median render time over `samples` iterations plus the plugin's
/// capabilities.
fn measure(
	path: &PathBuf,
	mode: RenderMode,
	resolution: Resolution,
	config: &aexlo_bench::ParamConfig,
	samples: usize,
) -> Result<(Duration, Caps), String> {
	let mut instance = PluginInstance::try_load(path).map_err(|e| format!("load failed: {e:?}"))?;
	let _ = instance.about();
	let caps = capabilities(&instance);

	if !apply_param_config(&mut instance, config) {
		return Err("parameter setup failed".to_string());
	}

	set_bench_input(&mut instance, resolution.width, resolution.height)
		.map_err(|e| format!("set_input failed: {e}"))?;

	// Warmup (also validates the mode works and pays first-frame setup costs).
	for _ in 0..5 {
		mode.render(&mut instance)
			.map_err(|e| format!("render failed: {e:?}"))?;
	}

	let mut times = Vec::with_capacity(samples);
	for _ in 0..samples {
		let start = Instant::now();
		mode.render(&mut instance)
			.map_err(|e| format!("render failed: {e:?}"))?;
		times.push(start.elapsed());
	}
	times.sort();
	Ok((times[times.len() / 2], caps))
}

/// The resolution to summarize at: 1080p when available, else the first selected.
fn summary_resolution() -> Resolution {
	let selected = bench_resolutions();
	selected
		.iter()
		.find(|r| r.name == "1080p")
		.copied()
		.or_else(|| selected.first().copied())
		.unwrap_or(Resolution {
			name: "1080p",
			width: 1920,
			height: 1080,
		})
}

fn print_leaderboard(rows: &[Measurement], resolution: Resolution) {
	println!("\n== Leaderboard @ {} (sorted by throughput) ==", resolution.name);
	println!(
		"{:>2}  {:<26} {:<7} {:>10} {:>11}  {:<8} {:<6} {:>6}",
		"#", "plugin", "mode", "Mpx/s", "median ms", "smart", "gpu", "params"
	);
	for (i, row) in rows.iter().enumerate() {
		println!(
			"{:>2}  {:<26} {:<7} {:>10.1} {:>11.3}  {:<8} {:<6} {:>6}",
			i + 1,
			truncate(&row.plugin, 26),
			row.mode,
			row.mpx_per_s,
			row.median.as_secs_f64() * 1.0e3,
			row.caps.smart_render,
			row.caps.gpu,
			row.caps.param_count,
		);
	}
}

/// For plugins measured on both CPU and GPU, report the GPU speedup.
fn print_speedups(rows: &[Measurement]) {
	let mut printed_header = false;
	let plugins: Vec<&String> = {
		let mut seen = Vec::new();
		for row in rows {
			if !seen.contains(&&row.plugin) {
				seen.push(&row.plugin);
			}
		}
		seen
	};
	for plugin in plugins {
		let cpu = rows.iter().find(|r| &r.plugin == plugin && r.mode == "cpu");
		let gpu = rows.iter().find(|r| &r.plugin == plugin && r.mode == "gpu");
		if let (Some(cpu), Some(gpu)) = (cpu, gpu) {
			if !printed_header {
				println!("\n== GPU speedup (gpu vs cpu) ==");
				printed_header = true;
			}
			let speedup = gpu.mpx_per_s / cpu.mpx_per_s;
			println!("  {:<26} {:>5.2}x", truncate(plugin, 26), speedup);
		}
	}
}

fn write_outputs(rows: &[Measurement], resolution: Resolution) {
	let prefix = std::env::var("AEXLO_BENCH_OUT").unwrap_or_else(|_| "target/aexlo-bench/summary".to_string());
	let prefix = PathBuf::from(prefix);
	if let Some(dir) = prefix.parent()
		&& let Err(err) = std::fs::create_dir_all(dir)
	{
		eprintln!("aexlo-bench: could not create {}: {err}", dir.display());
		return;
	}

	let csv_path = with_extension(&prefix, "csv");
	let json_path = with_extension(&prefix, "json");

	let mut csv = String::from("plugin,mode,resolution,params,smart_render,gpu,param_count,median_ms,mpx_per_s\n");
	for r in rows {
		csv.push_str(&format!(
			"{},{},{},{},{},{},{},{:.6},{:.3}\n",
			csv_field(&r.plugin),
			r.mode,
			resolution.name,
			csv_field(&r.config),
			r.caps.smart_render,
			r.caps.gpu,
			r.caps.param_count,
			r.median.as_secs_f64() * 1.0e3,
			r.mpx_per_s,
		));
	}

	let mut json = String::from("[\n");
	for (i, r) in rows.iter().enumerate() {
		json.push_str(&format!(
			"  {{\"plugin\": \"{}\", \"mode\": \"{}\", \"resolution\": \"{}\", \"params\": \"{}\", \"smart_render\": {}, \"gpu\": {}, \"param_count\": {}, \"median_ms\": {:.6}, \"mpx_per_s\": {:.3}}}{}\n",
			json_escape(&r.plugin),
			r.mode,
			resolution.name,
			json_escape(&r.config),
			r.caps.smart_render,
			r.caps.gpu,
			r.caps.param_count,
			r.median.as_secs_f64() * 1.0e3,
			r.mpx_per_s,
			if i + 1 < rows.len() { "," } else { "" },
		));
	}
	json.push_str("]\n");

	match (std::fs::write(&csv_path, csv), std::fs::write(&json_path, json)) {
		(Ok(()), Ok(())) => println!("\nWrote {} and {}", csv_path.display(), json_path.display()),
		(csv_res, json_res) => {
			if let Err(err) = csv_res {
				eprintln!("aexlo-bench: failed to write {}: {err}", csv_path.display());
			}
			if let Err(err) = json_res {
				eprintln!("aexlo-bench: failed to write {}: {err}", json_path.display());
			}
		}
	}
}

fn with_extension(prefix: &std::path::Path, ext: &str) -> PathBuf {
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
