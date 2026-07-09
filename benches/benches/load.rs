//! Plugin load / initialization cost.
//!
//! Measures a full [`PluginInstance::try_load`], i.e. opening the binary,
//! resolving the entry point, and running `PF_Cmd_GLOBAL_SETUP` +
//! `PF_Cmd_PARAMS_SETUP`. This is the fixed price paid before the first frame
//! can render, and it varies widely between a trivial effect and one that
//! allocates lookup tables or compiles pipelines at setup time.

use aexlo::PluginInstance;
use aexlo_bench::{bench_plugins, capabilities, print_params};
use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;

fn load(criterion: &mut Criterion) {
	let plugins = bench_plugins();

	if plugins.is_empty() {
		eprintln!("aexlo-bench: no plugins resolved; nothing to benchmark (see AEXLO_BENCH_PLUGINS).");
		return;
	}

	let mut group = criterion.benchmark_group("load");

	for (label, path) in &plugins {
		// Validate once so a broken artifact is skipped instead of panicking
		// inside the timed loop, and dump its parameter configuration.
		match PluginInstance::try_load(path) {
			Ok(instance) => {
				println!("aexlo-bench: {label}: {}", capabilities(&instance));
				print_params(label, &instance);
			}
			Err(err) => {
				eprintln!("aexlo-bench: {label}: load failed, skipping: {err:?}");
				continue;
			}
		}

		group.bench_function(label, |bencher| {
			bencher.iter(|| {
				let instance = PluginInstance::try_load(black_box(path.as_path())).unwrap();
				black_box(instance);
			});
		});
	}

	group.finish();
}

criterion_group!(benches, load);
criterion_main!(benches);
