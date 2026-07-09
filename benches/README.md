# aexlo-bench

A benchmark platform for driving real After Effects plugins through
[`aexlo`](../aexlo) and measuring them. It is deliberately plugin-agnostic:
point it at any `.plugin`/`.aex` artifact and it will benchmark it — no code
changes required.

## Bench targets

| Target          | What it measures                                                        |
| --------------- | ----------------------------------------------------------------------- |
| `render_matrix` | Render throughput (pixels/sec) across a **plugin × mode × resolution × parameter** matrix. |
| `load`          | Plugin **load / initialization** cost: `try_load` = open binary + resolve entry point + `GLOBAL_SETUP` + `PARAMS_SETUP`. |
| `summary`       | Cross-plugin **leaderboard** at one resolution, sorted by throughput, with **CSV/JSON export** and GPU-vs-CPU speedups. |

## Running

```sh
# Default: curated set (FillColor, SDK_Noise, DeepGlow2) across all resolutions
cargo bench -p aexlo-bench

# A single target
cargo bench -p aexlo-bench --bench render_matrix
cargo bench -p aexlo-bench --bench load
cargo bench -p aexlo-bench --bench summary
```

## Environment knobs

Everything is driven by environment variables — no need to touch the bench code.

| Variable                  | Meaning                                                                 | Example                              |
| ------------------------- | ----------------------------------------------------------------------- | ------------------------------------ |
| `AEXLO_BENCH_PLUGINS`     | Comma list of fixture names **or** absolute artifact paths. `all` = every bundled fixture. | `all` · `SDK_Noise,DeepGlow2` · `/abs/MyFx.plugin` |
| `AEXLO_BENCH_RESOLUTIONS` | Comma list of resolution names to restrict the sweep.                   | `1080p,4k`                           |
| `AEXLO_BENCH_PARAMS`      | Parameter sweep: `Name=v1,v2;Other=v3,v4`. Each combination is benchmarked. Names match declared param names (case-insensitive), values are numbers. | `Radius=100,500,1000` |
| `AEXLO_BENCH_INPUT`       | **Absolute** path to an image fed as the input frame (resized to each resolution). Defaults to a synthetic gradient. | `/abs/footage.png` |
| `AEXLO_BENCH_SAMPLES`     | `summary` only: render iterations timed per plugin (default 30).        | `50`                                 |
| `AEXLO_BENCH_OUT`         | `summary` only: output path prefix; writes `<prefix>.csv` and `.json` (default `target/aexlo-bench/summary`). | `/tmp/run1` |
| `AEXLO_DISABLE_GPU`       | Force the CPU render path even on GPU-capable effects (from `aexlo`).   | `1`                                  |

```sh
# External plugin at 1080p + 4K
AEXLO_BENCH_PLUGINS=/path/to/MyEffect.plugin \
AEXLO_BENCH_RESOLUTIONS=1080p,4k \
  cargo bench -p aexlo-bench --bench render_matrix

# Parameter sweep: see how render cost scales with a param
AEXLO_BENCH_PLUGINS=DeepGlow2 AEXLO_BENCH_PARAMS="Radius=100,500,1000" \
  cargo bench -p aexlo-bench --bench render_matrix

# Leaderboard of every fixture, exported to CSV/JSON
AEXLO_BENCH_PLUGINS=all AEXLO_BENCH_OUT=/tmp/aexlo cargo bench -p aexlo-bench --bench summary
```

Resolutions: `512` (512×512), `720p`, `1080p`, `4k`. See
[`ALL_RESOLUTIONS`](src/lib.rs).

## What gets reported

- **Capabilities** (`smart_render`, `gpu`, `param_count`) and the full
  **parameter settings** are dumped once per plugin, so every number is tied to a
  known, reproducible configuration. Plugins run with their default values unless
  `AEXLO_BENCH_PARAMS` overrides them.
- **CPU vs GPU:** GPU-capable effects are benchmarked on both the `cpu` and `gpu`
  paths so they can be compared directly; `summary` prints the GPU speedup.
- **Throughput** is pixels/sec, so effects compare regardless of frame size.

## Notes / limitations

- **8-bit only.** The public input API (`set_input`/`Layer<Depth8>`) accepts only
  8-bit pixels, so the platform sweeps *resolution* but not *bit depth*. When a
  16/32-bit input path lands, `synthetic_input` in [`src/lib.rs`](src/lib.rs) is
  the place to add a depth axis.
- **GPU path is untested against the bundled fixtures** — none of them declare
  `PF_OutFlag2_SUPPORTS_GPU_RENDER_F32`, so the `gpu` mode only activates for
  external plugins that do. The comparison code is in place; it just has nothing
  local to exercise.
- **`AEXLO_BENCH_INPUT` should be absolute.** Bench binaries run with their cwd
  set to the `benches/` crate dir, so relative paths resolve there (a load failure
  warns and falls back to synthetic input).
- **A crashing plugin can abort the whole run.** Plugins are driven in-process, so
  a plugin that triggers a non-unwinding panic (SIGABRT) takes the bench process
  with it — recoverable render/load *errors* are skipped with a warning, but hard
  crashes are not. Narrow `AEXLO_BENCH_PLUGINS` to avoid a known-bad one; full
  per-plugin process isolation (like the e2e `render_one` harness) is a possible
  future addition.
