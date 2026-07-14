//! Render throughput across a plugin x mode x resolution x parameter matrix.
//!
//! For every selected plugin the bench sweeps:
//! * render **mode** -- `cpu` vs `gpu` for GPU-capable effects (so the two are
//!   directly comparable), otherwise a single auto-dispatched `render`;
//! * **resolution** -- see `AEXLO_BENCH_RESOLUTIONS`;
//! * **parameter** combinations -- see `AEXLO_BENCH_PARAMS`.
//!
//! Throughput is reported in pixels/second so effects compare regardless of
//! frame size. The plugin's capabilities and parameter settings are dumped once
//! up front. See the crate docs for all `AEXLO_BENCH_*` knobs.

use aexlo::PluginInstance;
use aexlo_bench::{
	apply_param_config, bench_modes, bench_plugins, bench_resolutions, capabilities, param_config_label, param_configs,
	print_params, set_bench_input,
};
use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use std::hint::black_box;

fn render_matrix(criterion: &mut Criterion) {
	let plugins = bench_plugins();
	let resolutions = bench_resolutions();
	let configs = param_configs();

	if plugins.is_empty() {
		eprintln!("aexlo-bench: no plugins resolved; nothing to benchmark (see AEXLO_BENCH_PLUGINS).");
		return;
	}

	for (label, path) in &plugins {
		// Probe the plugin once to report its capabilities / parameters and
		// decide which render modes to sweep.
		let modes = match PluginInstance::try_load(path) {
			Ok(mut probe) => {
				let _ = probe.about();
				println!("aexlo-bench: {label}: {}", capabilities(&probe));
				print_params(label, &probe);
				bench_modes(&probe)
			}
			Err(err) => {
				eprintln!("aexlo-bench: {label}: load failed, skipping: {err:?}");
				continue;
			}
		};
		let multi_mode = modes.len() > 1;
		let multi_config = configs.len() > 1;

		let mut group = criterion.benchmark_group(format!("render/{label}"));

		for &mode in &modes {
			for resolution in &resolutions {
				for config in &configs {
					// One-time setup, kept out of the timed section: fresh
					// instance, parameter overrides, input frame, warmup render.
					let mut instance = match PluginInstance::try_load(path) {
						Ok(instance) => instance,
						Err(err) => {
							eprintln!("aexlo-bench: {label}: load failed, skipping: {err:?}");
							break;
						}
					};
					let _ = instance.about();

					if !apply_param_config(&mut instance, config) {
						continue;
					}

					if let Err(err) = set_bench_input(&mut instance, resolution.width, resolution.height) {
						eprintln!("aexlo-bench: {label}: set_input failed, skipping: {err}");
						continue;
					}
					if let Err(err) = mode.render(&mut instance) {
						eprintln!(
							"aexlo-bench: {label} [{}] @ {}: render failed, skipping: {err:?}",
							mode.label(),
							resolution.name
						);
						continue;
					}

					// Compose a unique id from just the axes that actually vary.
					let mut parts: Vec<String> = Vec::new();
					if multi_mode {
						parts.push(mode.label().to_string());
					}
					parts.push(resolution.name.to_string());
					if multi_config {
						parts.push(param_config_label(config));
					}
					let id = parts.join("/");

					group.throughput(Throughput::Elements(resolution.pixels()));
					group.bench_function(id, |bencher| {
						bencher.iter(|| {
							mode.render(&mut instance).unwrap();
							black_box(instance.output_size());
						});
					});
				}
			}
		}

		group.finish();
	}
}

criterion_group!(benches, render_matrix);
criterion_main!(benches);
