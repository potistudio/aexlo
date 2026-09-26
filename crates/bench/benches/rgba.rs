//! `Layer<Depth8>::write_rgba_bytes` throughput.
//!
//! Compares the current per-field copy against a `u32::rotate_right(8)`
//! candidate (ARGB -> RGBA in one op) and a plain `memcpy` lower bound, so it
//! is clear how much headroom the ARGB -> RGBA swizzle leaves.

use aexlo::{Depth8, Layer};
use aexlo_bench::ALL_RESOLUTIONS;
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use std::hint::black_box;

fn layer_bytes(layer: &Layer<Depth8>) -> &[u8] {
	let pixels = layer.pixels();
	// SAFETY: `Pixel<Depth8>` is `#[repr(C)]` with four `u8` fields.
	unsafe { std::slice::from_raw_parts(pixels.as_ptr() as *const u8, pixels.len() * 4) }
}

fn write_rotate(src: &[u8], buffer: &mut [u8]) {
	for (dst, s) in buffer.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
		let v = u32::from_le_bytes(s.try_into().unwrap()).rotate_right(8);
		dst.copy_from_slice(&v.to_le_bytes());
	}
}

fn rgba(criterion: &mut Criterion) {
	let mut group = criterion.benchmark_group("write_rgba_bytes");

	for resolution in ALL_RESOLUTIONS {
		let (width, height) = (resolution.width, resolution.height);
		let raw = (0..width * height * 4).map(|i| i as u8).collect();
		let layer = Layer::<Depth8>::from_raw(raw, width, height).expect("layer");
		let mut buffer = vec![0u8; layer.len() * 4];

		// Sanity: the candidate must produce identical output.
		let mut expected = vec![0u8; buffer.len()];
		layer.write_rgba_bytes(&mut expected).unwrap();
		write_rotate(layer_bytes(&layer), &mut buffer);
		assert_eq!(expected, buffer, "rotate variant diverges");

		group.throughput(Throughput::Elements(resolution.pixels()));

		group.bench_function(BenchmarkId::new("current", resolution.name), |b| {
			b.iter(|| layer.write_rgba_bytes(black_box(&mut buffer)).unwrap())
		});
		group.bench_function(BenchmarkId::new("rotate", resolution.name), |b| {
			b.iter(|| write_rotate(black_box(layer_bytes(&layer)), black_box(&mut buffer)))
		});
		group.bench_function(BenchmarkId::new("memcpy", resolution.name), |b| {
			b.iter(|| black_box(&mut buffer).copy_from_slice(black_box(layer_bytes(&layer))))
		});
	}

	group.finish();
}

criterion_group!(benches, rgba);
criterion_main!(benches);
