use std::hint::black_box;
use criterion::{criterion_group, criterion_main, Criterion};
use aexlo::*;

const MODULE_NAME: &str = "SDK_Noise";

// 1. 40.527ms 1920x1080
fn benchmark_rendering_full(criterion: &mut Criterion) {
	let exe_dir = std::env::current_exe().expect("Failed to get current executable path");
	let plugin_path = exe_dir
		.parent()
		.expect("Failed to get parent directory of executable")
		.parent()
		.expect("Failed to get parent directory of executable")
		.join(MODULE_NAME);

	let mut instance = PluginInstance::new(plugin_path.as_path());
	instance.load().expect("Failed to load plugin");

	criterion.bench_function("rendering", |bencher| {
		bencher.iter(|| {
			instance.render().unwrap();
			black_box(instance.output_layer());
		})
	});
}

criterion_group!(
	benches,
	benchmark_rendering_full,
);
criterion_main!(benches);
