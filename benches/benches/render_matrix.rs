//! Render throughput across a plugin x resolution matrix.
//!
//! For every selected plugin and every selected resolution, drives
//! [`PluginInstance::render_frame`] (which auto-dispatches GPU / smart / legacy
//! render). Throughput is reported in pixels/second so effects are comparable
//! regardless of frame size. See the crate docs for the `AEXLO_BENCH_*` knobs.

use aexlo::PluginInstance;
use aexlo_bench::{bench_plugins, bench_resolutions, synthetic_input};
use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use std::hint::black_box;

fn render_matrix(criterion: &mut Criterion) {
	let plugins = bench_plugins();
	let resolutions = bench_resolutions();

	if plugins.is_empty() {
		eprintln!("aexlo-bench: no plugins resolved; nothing to benchmark (see AEXLO_BENCH_PLUGINS).");
		return;
	}

	for (label, path) in &plugins {
		let mut group = criterion.benchmark_group(format!("render/{label}"));

		for resolution in &resolutions {
			// Everything below the `bench_function` call is one-time setup, kept
			// out of the timed section: load the plugin, feed a synthetic frame,
			// and render once as a warmup + validity check. Configs the plugin
			// can't handle are skipped rather than aborting the whole run.
			let mut instance = match PluginInstance::try_load(path) {
				Ok(instance) => instance,
				Err(err) => {
					eprintln!("aexlo-bench: {label}: load failed, skipping: {err:?}");
					break;
				}
			};
			let _ = instance.about();

			let input = synthetic_input(resolution.width, resolution.height);
			if let Err(err) = instance.set_input_raw(input, resolution.width, resolution.height) {
				eprintln!("aexlo-bench: {label} @ {}: set_input failed, skipping: {err:?}", resolution.name);
				continue;
			}
			if let Err(err) = instance.render_frame() {
				eprintln!("aexlo-bench: {label} @ {}: render failed, skipping: {err:?}", resolution.name);
				continue;
			}

			group.throughput(Throughput::Elements(resolution.pixels()));
			group.bench_function(resolution.name, |bencher| {
				bencher.iter(|| {
					instance.render_frame().unwrap();
					black_box(instance.output_size());
				});
			});
		}

		group.finish();
	}
}

criterion_group!(benches, render_matrix);
criterion_main!(benches);
