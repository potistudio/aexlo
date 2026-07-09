# aexlo-bench

A benchmark platform for driving real After Effects plugins through
[`aexlo`](../aexlo) and measuring them. It is deliberately plugin-agnostic:
point it at any `.plugin`/`.aex` artifact and it will benchmark it — no code
changes required.

## Bench targets

| Target          | What it measures                                                        |
| --------------- | ----------------------------------------------------------------------- |
| `render_matrix` | Render throughput (pixels/sec) across a **plugin × resolution** matrix, via `PluginInstance::render_frame` (auto GPU/smart/legacy dispatch). |
| `load`          | Plugin **load / initialization** cost: `try_load` = open binary + resolve entry point + `GLOBAL_SETUP` + `PARAMS_SETUP`. |

## Running

```sh
# Default: curated set (FillColor, SDK_Noise, DeepGlow2) across all resolutions
cargo bench -p aexlo-bench

# Just the render matrix, or just load cost
cargo bench -p aexlo-bench --bench render_matrix
cargo bench -p aexlo-bench --bench load
```

## Pointing it at your own plugins

Everything is driven by environment variables — no need to touch the bench code.

| Variable                  | Meaning                                                                 | Example                              |
| ------------------------- | ----------------------------------------------------------------------- | ------------------------------------ |
| `AEXLO_BENCH_PLUGINS`     | Comma list of fixture names **or** absolute artifact paths. `all` = every bundled fixture. | `all` · `SDK_Noise,DeepGlow2` · `/abs/MyFx.plugin` |
| `AEXLO_BENCH_RESOLUTIONS` | Comma list of resolution names to restrict the sweep.                   | `1080p,4k`                           |
| `AEXLO_DISABLE_GPU`       | Force the CPU render path even on GPU-capable effects (from `aexlo`).   | `1`                                  |

```sh
# Benchmark an external plugin at 1080p and 4K only
AEXLO_BENCH_PLUGINS=/path/to/MyEffect.plugin \
AEXLO_BENCH_RESOLUTIONS=1080p,4k \
  cargo bench -p aexlo-bench --bench render_matrix

# Sweep every bundled fixture
AEXLO_BENCH_PLUGINS=all cargo bench -p aexlo-bench
```

Resolutions: `512` (512×512), `720p`, `1080p`, `4k`. See
[`ALL_RESOLUTIONS`](src/lib.rs).

## Notes / limitations

- **8-bit only.** The public input API (`set_input_raw`) currently accepts only
  `Depth8` pixels, so the platform sweeps *resolution* but not *bit depth*. When
  a 16/32-bit input path lands, `synthetic_input` in [`src/lib.rs`](src/lib.rs)
  is the place to add a depth axis.
- Configs a plugin can't handle (load/setup/render failure) are **skipped with a
  warning**, never aborting the run — so `AEXLO_BENCH_PLUGINS=all` is safe even
  when some fixtures are finicky.
- Input frames are a deterministic synthetic gradient so runs are comparable.
- Both targets **dump the plugin's parameter settings** (name = value) once per
  plugin before benchmarking, so every result is tied to the exact, reproducible
  configuration that produced it. Plugins are driven with their default values.
