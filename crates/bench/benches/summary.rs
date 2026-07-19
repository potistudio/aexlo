//! Cross-plugin render leaderboard with CSV/JSON export.
//!
//! Criterion times each benchmark in isolation and never ranks them against each
//! other. This target fills that gap: it drives every selected plugin through
//! the shared timing loop ([`aexlo_bench::measure`]) at a single resolution,
//! then prints a table sorted by throughput and writes machine-readable
//! `CSV`/`JSON` for tracking results over time.
//!
//! Honors the same `AEXLO_BENCH_*` knobs as the criterion benches, plus:
//! * `AEXLO_BENCH_SAMPLES` -- render iterations timed per plugin (default 30).
//! * `AEXLO_BENCH_OUT` -- output path prefix (default `target/aexlo-bench/summary`);
//!   `<prefix>.csv` and `<prefix>.json` are written.
//!
//! `aexlo bench` is the same report driven by CLI flags instead of environment
//! variables; this target stays for `cargo bench` workflows.
//!
//! Being `harness = false`, it owns `main` and ignores criterion CLI flags.

use aexlo::PluginInstance;
use aexlo_bench::report::{
	Measurement, print_leaderboard, print_speedups, sort_by_throughput, to_csv, to_json, with_extension, write_file,
};
use aexlo_bench::{
	MeasureOptions, Resolution, bench_modes, bench_plugins, bench_resolutions, measure, param_config_label,
	param_configs,
};
use std::path::PathBuf;

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

	let options = MeasureOptions {
		resolution,
		config: &config,
		input: None,
		samples,
		warmup: 5,
	};

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
			match measure(path, mode, options) {
				Ok((timing, caps)) => {
					let row = Measurement {
						plugin: label.clone(),
						mode: mode.label(),
						caps,
						config: config_label.clone(),
						resolution,
						timing,
					};
					eprintln!("  measured {label} [{}] -> {:.1} Mpx/s", mode.label(), row.mpx_per_s());
					measurements.push(row);
				}
				Err(err) => eprintln!("aexlo-bench: {label} [{}]: skipped ({err})", mode.label()),
			}
		}
	}

	if measurements.is_empty() {
		eprintln!("aexlo-bench: no measurements collected.");
		return;
	}

	sort_by_throughput(&mut measurements);
	print_leaderboard(&measurements);
	print_speedups(&measurements);

	let prefix =
		PathBuf::from(std::env::var("AEXLO_BENCH_OUT").unwrap_or_else(|_| "target/aexlo-bench/summary".to_string()));
	println!();
	write_file(&with_extension(&prefix, "csv"), &to_csv(&measurements));
	write_file(&with_extension(&prefix, "json"), &to_json(&measurements));
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
