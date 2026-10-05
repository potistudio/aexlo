# aexlo toolkit — specification

Status: draft · Target: aexlo 0.1

## 1. Purpose

aexlo already hosts real After Effects plugins outside AE. This spec turns that
runtime into a **development toolkit for plugin authors**: one declarative
description of "render this plugin under these conditions" that drives
**tests**, **benchmarks** and **previews** alike, plus host-side checks that
After Effects itself cannot offer.

Today the pieces exist but do not share an input format:

| Use     | Existing                                                         | Gap                                                     |
| ------- | ---------------------------------------------------------------- | ------------------------------------------------------- |
| Test    | `tests/e2e`, `#[aexlo::preview]`, `render_one` crash isolation    | hand-written pixel indexing, no goldens, isolation is e2e-only |
| Bench   | `aexlo-bench`, `aexlo bench`                                      | env vars vs flags, no baseline comparison, 8-bit only   |
| Preview | `aexlo dev`, `dev --bin`, `preview --web`, `studio`              | four ways to say "input, params, size, time"            |

### Goals

- A plugin author (Rust **or** C++) can write a manifest, then run
  `aexlo test`, `aexlo bench`, `aexlo dev`/`preview` and `aexlo check`
  against the same presets.
- A crashing plugin fails one preset, never the whole run.
- Host-side instrumentation (strict mode) reports bugs AE would hide:
  out-of-bounds writes, unwritten output pixels, handle leaks, unbalanced
  checkouts.
- 8, 16 and 32 bpc are first-class.

### Non-goals

- Emulating the AE application (projects, `.ffx` presets, UI panels).
  aexlo stays a host for plugin instances.
- Pixel-exact parity with AE by default. Parity is opt-in through
  AE-sourced goldens (§8.4).
- Linux plugin artifacts (AE plugins target Windows and macOS).

## 2. Terms

| Term         | Meaning                                                                                  |
| ------------ | ---------------------------------------------------------------------------------------- |
| **manifest** | `aexlo.toml`: the plugin to load and its presets.                                        |
| **preset**   | A named set of run conditions: input, size, time, params, render path, depth, expectations. |
| **variant**  | One concrete combination after a preset's matrix axes are expanded.                       |
| **run**      | One execution of a variant; produces a `Frame`, a `Trace` and a `Timing`.                |
| **golden**   | A stored reference frame a variant is compared against.                                 |
| **check**    | A host-side invariant evaluated on a run (§7).                                           |
| **baseline** | A stored set of bench timings that later runs are compared against.                     |

## 3. Architecture

```text
                    aexlo.toml (presets)
          ┌──────────────┬──────────────┬──────────────┐
          ▼              ▼              ▼              ▼
     aexlo test     aexlo bench    aexlo dev/preview  aexlo check
          └──────────────┴──────┬───────┴──────────────┘
                                ▼
                         aexlo-harness
             preset · run (in-process | worker) · frame · checks · golden
                                ▼
                    aexlo (host) + Observer / Strict hooks
```

### 3.1 Crates

| Crate           | Role                                                                                       | Status   |
| --------------- | ------------------------------------------------------------------------------------------ | -------- |
| `aexlo`         | The host. Gains only observation and configuration surfaces (§5). No tooling logic.        | existing |
| `aexlo-macros`  | `#[aexlo::preview]`, new `#[aexlo::test]`. Expand to `aexlo-harness` paths.                | existing |
| `aexlo-harness` | `preset` (manifest model, `inherits`, matrix expansion, application), `run` (in-process and worker), `frame`, `check`, `golden`, `bench` timing loop. | new |
| `aexlo-test`    | The single dev-dependency for Rust plugin crates: re-exports the macros plus `Frame` assertions. | new |
| `aexlo-bench`   | criterion benches; becomes a thin layer over `aexlo_harness::bench`.                        | existing |
| `aexlo-cli`     | `aexlo` binary: `test`, `bench`, `dev`, `preview`, `check`, `presets`, `worker`, plus the existing `render`/`about`/`params`/`view`. | existing |

Dependency direction is strictly downward: `cli → harness → aexlo`,
`aexlo-test → harness`, `aexlo-bench → harness`. Nothing below `harness`
knows about manifests.

### 3.2 Core boundary rule

`aexlo` exposes **mechanisms** (events, allocation policy, depth I/O); the
harness implements **policy** (what counts as a failure, tolerances, files).
`crates/aexlo/src/preview.rs` (viewer lock, PNG writing, `AEXLO_PREVIEW`)
violates this and moves to `aexlo_harness::preview` (§13).

## 4. Manifest: `aexlo.toml`

Discovery: `--manifest <path>`, else the nearest `aexlo.toml` walking up from
the current directory. Relative paths inside the manifest resolve against the
manifest's directory.

### 4.1 Example

```toml
[plugin]
# One of `artifact` or `crate`.
artifact = { macos = "build/MyGlow.plugin", windows = "build/MyGlow.aex" }
# crate = "."            # cargo build the cdylib, as `aexlo dev --bin` does
# entry = "EffectMain"   # in-process entry symbol (Rust plugins only)

[defaults]
size   = [1920, 1080]
depth  = [8, 16, 32]
render = ["smart", "gpu"]
checks = ["finite", "coverage", "deterministic"]

[[preset]]
name   = "base"
hidden = true
input  = { generate = "gradient" }

[[preset]]
name     = "dot_glow"
inherits = "base"
input    = { generate = "dot", at = [320, 240], radius = 2 }
size     = [640, 480]
time     = { frame = 12, fps = 24 }
params   = { Radius = 100.0, Mode = "Screen", Center = [320, 240] }
golden   = { tolerance = { max_abs = 0.004 } }

[[preset]]
name     = "radius_sweep"
inherits = "dot_glow"
params   = { Radius = { sweep = [10, 100, 1000] } }
golden   = false
bench    = { samples = 30, warmup = 5 }
```

### 4.2 `[plugin]`

| Key        | Type                               | Meaning                                                            |
| ---------- | ---------------------------------- | ------------------------------------------------------------------ |
| `artifact` | string \| `{ macos, windows }`     | Prebuilt plugin. A bare string applies to the current platform.     |
| `crate`    | string                             | Crate directory; built with `cargo build` before loading.           |
| `entry`    | string, default `"EffectMain"`     | Entry symbol for in-process runs (§6.1).                            |

Exactly one of `artifact` / `crate` is required.

### 4.3 Preset fields

Every field may appear in `[defaults]` or a preset.

| Key        | Type                                 | Default                 | Meaning |
| ---------- | ------------------------------------ | ----------------------- | ------- |
| `name`     | string, `[a-z0-9_]+`                 | required (preset only)  | Unique id. |
| `hidden`   | bool                                 | `false`                 | Base for `inherits`; never run. |
| `inherits` | string \| [string]                   | none                    | §4.4. |
| `input`    | input spec                           | `{ generate = "gradient" }` | §4.5. |
| `size`     | [w, h]                               | input size              | Render size (`set_render_size`). The input is scaled to it unless `input.fit = "none"`. |
| `time`     | `{ frame, fps }`                     | `{ frame = 0, fps = 24 }` | §4.7. |
| `params`   | table                                | none                    | §4.6. |
| `layers`   | table: param → input spec            | none                    | Layer parameters (`set_layer_param`). |
| `render`   | mode \| [mode]                       | `"auto"`                | `auto` (`render_frame`), `legacy`, `smart`, `gpu`. Matrix axis. |
| `depth`    | 8 \| 16 \| 32 \| [..]                | `8`                     | Matrix axis. |
| `iterate`  | `"parallel"` \| `"serial"`           | `"parallel"`            | `set_parallel_iterate`. |
| `checks`   | [check id]                           | §7 defaults             | Replaces, not merges. `"+id"` / `"-id"` adjust the inherited list. |
| `strict`   | bool \| [strict feature]             | `false`                 | §5.3. |
| `golden`   | golden spec \| `false`               | `{}` (enabled)          | §8. |
| `bench`    | `{ samples, warmup }` \| `false`     | `{ samples = 30, warmup = 5 }` | §10. |
| `timeout`  | seconds                              | `30`                    | Per run (§6.3). |

### 4.4 Inheritance

Semantics follow CMakePresets:

- A preset's own keys override inherited ones. Tables (`params`, `layers`,
  `input`, `golden`, `bench`, `time`) merge key by key; arrays and scalars
  replace.
- With several parents, **earlier parents win** over later ones.
- `[defaults]` is the implicit root of every chain.
- Cycles, unknown parents and inheriting from a preset in another manifest
  are errors.

### 4.5 Input spec

```toml
input = { path = "plates/street.png" }                        # PNG (8/16) or EXR (32)
input = { generate = "gradient" }
input = { generate = "solid", color = [0.2, 0.4, 0.8, 1.0] }
input = { generate = "checker", cell = 32 }
input = { generate = "dot", at = [x, y], radius = 2 }
input = { generate = "noise", seed = 7 }
```

`fit = "stretch" | "none"` (default `stretch`). Generated inputs are produced
directly at the variant's depth; files are converted to it. Generator colors
are normalized `[0, 1]` regardless of depth.

### 4.6 Params

Keys name a parameter by its declared name (case-insensitive, matching
`aexlo-bench` today) or by index as `"#24"`. A name that matches more than one
parameter is an error that lists the candidate indices.

Values are TOML-native and interpreted by the parameter's `ParamKind`:

| Kind                         | TOML value                                   |
| ---------------------------- | -------------------------------------------- |
| Float / Fixed / Angle        | number                                       |
| Slider                       | integer                                      |
| Checkbox                     | bool                                         |
| Popup                        | integer (1-based, as AE) or choice label     |
| Point                        | `[x, y]` pixels                              |
| Point3D                      | `[x, y, z]`                                  |
| Color                        | `[r, g, b]` or `[r, g, b, a]`, normalized    |
| Path                         | mask id (integer)                            |

`{ sweep = [v1, v2, ...] }` makes the parameter a matrix axis. After setting
parameters the harness sends `PF_Cmd_UPDATE_PARAMS_UI` once, as
`aexlo render --set` does.

### 4.7 Time

`fps` is a number or a rational string (`"30000/1001"`). The harness calls
`set_time(frame * step, step, scale)` with `scale/step = fps` in lowest
integer terms (`24` → `24/1`, `"30000/1001"` → `30000/1001`).

### 4.8 Matrix expansion and variant ids

Axes: `depth`, `render`, and every swept parameter. The variant set is their
cartesian product. Variant id:

```text
display:   dot_glow[depth=16,render=gpu,Radius=100]
file-safe: dot_glow.d16.gpu.radius-100
```

Axes whose list has one value are omitted from the id. Variants the plugin
cannot run are **skipped**, not failed: `gpu` when `supports_gpu()` is false,
`smart` without `PF_OutFlag2_SUPPORTS_SMART_RENDER`, a depth the plugin does
not declare (16 without `PF_OutFlag_DEEP_COLOR_AWARE`, 32 absent from
`supported_pixel_formats()`).

## 5. Core additions (`aexlo`)

### 5.1 Parameter lookup

```rust
impl PluginInstance {
    /// Indices whose declared name matches, case-insensitively.
    pub fn param_indices(&self, name: &str) -> Vec<usize>;
}
```

`aexlo-bench`'s `resolve_param_index` and the CLI's index-only `--set` move
onto this.

### 5.2 Bit-depth I/O

```rust
impl PluginInstance {
    pub fn set_input_layer<D: PixelDepth>(&mut self, input: Layer<D>);
    pub fn output_depth(&self) -> PixelDepthKind;
    pub fn read_output<D: PixelDepth>(&self) -> Result<Layer<D>>;
}
```

- The output depth follows the input depth, as AE's project depth does.
- `set_input_layer` stays source-compatible for existing `Layer<Depth8>` callers.
- `read_output::<D>` errors when `D` differs from the output depth; converting
  is the harness's job.
- Layer parameters gain the same generic signature.

### 5.3 Strict mode

```rust
#[derive(Clone, Copy, Default)]
pub struct Strict {
    /// Fill output worlds with a sentinel before each render.
    pub poison_output: bool,
    /// Rows/bytes of guard band around every host-allocated world, verified on dispose.
    pub guard_bands: bool,
    /// Count handle and world allocations against disposals.
    pub track_allocations: bool,
    /// Record checkout/checkin of layers and params in smart render.
    pub track_checkouts: bool,
}

impl Strict { pub fn all() -> Self; }

impl PluginInstance {
    pub fn set_strict(&mut self, strict: Strict);
    pub fn strict_report(&self) -> StrictReport;   // violations since the last call
}
```

Per instance, like `set_parallel_iterate`. With everything off, the host's
allocation paths and costs are unchanged.

| Feature             | Mechanism                                                              | Detects                                     |
| ------------------- | ---------------------------------------------------------------------- | ------------------------------------------- |
| `poison_output`     | 8/16 bpc: byte pattern `0xCD`; 32 bpc: a signalling-NaN payload.        | Output pixels the plugin never wrote.       |
| `guard_bands`       | Extra rows above/below and a rowbytes tail, filled with a pattern.      | Writes outside the world (rowbytes misuse). |
| `track_allocations` | Counters on Handle Suite / World Suite / `new_world` paths.             | Leaks at `SEQUENCE_SETDOWN`/`GLOBAL_SETDOWN`. |
| `track_checkouts`   | Records from `PreRender` declarations and `SmartRender` checkouts.      | Checkouts not declared in PreRender, missing checkins. |

### 5.4 Observer

```rust
pub trait Observer: Send + Sync {
    fn command(&self, _event: &CommandEvent) {}      // cmd, Begin/End, duration, PF_Err
    fn suite_call(&self, _event: &SuiteCallEvent) {} // suite, version, function, duration
    fn allocation(&self, _event: &AllocationEvent) {}
    fn checkout(&self, _event: &CheckoutEvent) {}
}

pub enum ObserveLevel { Commands, Calls }

impl PluginInstance {
    pub fn set_observer(&mut self, observer: Option<Arc<dyn Observer>>, level: ObserveLevel);
}
```

- `Commands` only adds per-command events, which is cheap enough to leave
  on during benches.
- `Calls` adds per-suite-call events. It is costly and opt-in; benches never
  use it.
- Events are attributed through the instance reachable from `effect_ref`.
  Calls with no reachable instance (ANSI callbacks, calls from iterate worker
  threads) are attributed through a thread-local set during command dispatch,
  and propagated to the iterate pool.
- The existing `diagnostics` feature becomes one `Observer` implementation
  that logs.

## 6. Harness execution

### 6.1 Modes

| Mode         | How                                           | For                                                           | Trade-off |
| ------------ | --------------------------------------------- | ------------------------------------------------------------- | --------- |
| `in-process` | `Host::from_entry_raw(entry)`                 | Rust plugins: `#[aexlo::test]`, `aexlo dev`                   | Debugger and `dbg!` work; a crash kills the process. |
| `worker`     | `aexlo worker` subprocess, plugin `dlopen`ed   | Any plugin: `aexlo test/check/bench/preview` default          | Crash isolation; no in-process breakpoints. |

The CLI defaults to `worker`. `--isolate preset` (default) reuses one worker
for all variants of a preset; `--isolate variant` spawns one per variant;
`--isolate none` runs in-process in the CLI (debugging only).

### 6.2 Worker protocol

`aexlo worker` (hidden subcommand) reads one JSON request per line on stdin
and writes one JSON response per line on stdout; logs go to stderr and are
captured into the run report.

```json
→ {"id":1,"op":"load","artifact":"/abs/MyGlow.plugin"}
← {"id":1,"ok":true,"info":{"params":[...],"smart":true,"gpu":false,"formats":[...]}}
→ {"id":2,"op":"run","variant":{...resolved variant...},"frame_out":"/tmp/aexlo-.../2.raw","observe":"commands","strict":["poison_output"]}
← {"id":2,"ok":true,"frame":{"path":"...","w":640,"h":480,"depth":16},"timing":{...},"trace":{...},"strict":{...}}
```

Frames travel as raw files in a per-session temp directory, never through
the pipe. The `variant` payload is fully resolved (no `inherits`, no
names), so the worker needs no manifest logic. `render_one` in `tests/e2e` is
replaced by this.

### 6.3 Outcomes

Every run ends in exactly one outcome:

| Outcome    | Cause                                                                       |
| ---------- | --------------------------------------------------------------------------- |
| `pass`     | Rendered; all checks and golden comparison passed.                          |
| `fail`     | Rendered; a check or golden comparison failed.                              |
| `error`    | The plugin returned a `PF_Err`, or loading failed.                          |
| `crash`    | The worker died: the report gives the signal or exception code and the last command from the trace. |
| `timeout`  | Exceeded `timeout`. The harness first requests cancellation through the `AppHost` abort path, then kills the worker after 2 s. |
| `skipped`  | The variant is unsupported by the plugin (§4.8).                            |

After `crash` or `timeout` the next variant gets a fresh worker.

### 6.4 Results

```rust
pub struct Run {
    pub variant: VariantId,
    pub outcome: Outcome,
    pub frame: Option<Frame>,
    pub timing: Timing,         // wall time per command (Observer, Commands level)
    pub trace: Trace,           // command sequence, PF_Err values, captured logs
    pub strict: StrictReport,
    pub checks: Vec<CheckResult>,
    pub golden: Option<Comparison>,
}

pub struct Frame { /* width, height, depth, pixels */ }
impl Frame {
    pub fn pixel(&self, x: u32, y: u32) -> [f32; 4];   // normalized, any depth
    pub fn compare(&self, other: &Frame, tol: &Tolerance) -> Comparison;
    pub fn save(&self, path: &Path) -> Result<()>;      // PNG 8/16, EXR 32
}
```

## 7. Checks

Checks take a run (sometimes a second run) and return pass, fail with a
message, or not-applicable.

| Id                 | Default | Extra cost      | Fails when |
| ------------------ | ------- | --------------- | ---------- |
| `finite`           | on      | none            | Output contains NaN/Inf (32 bpc only). |
| `coverage`         | on      | none            | Poisoned pixels remain inside the output rect. Implies `strict.poison_output`. |
| `deterministic`    | on      | 1 extra render  | Two renders of the same variant differ. |
| `iterate-parallel` | off     | 1 extra render  | Parallel and serial iterate differ beyond tolerance (thread-safety). |
| `depth-consistency`| off     | per depth       | 8/16/32 outputs of the same variant differ beyond `max_abs = 2/255`. Needs ≥2 depths. |
| `bounds`           | strict  | none            | A guard band was overwritten. |
| `allocations`      | strict  | none            | Handles or worlds outlive their scope. |
| `checkouts`        | strict  | none            | Undeclared or unbalanced smart-render checkouts. |
| `flags`            | strict  | none            | Declared out_flags contradict behavior, e.g. claims 16 bpc but errors at 16 bpc. |

`strict = true` enables all strict features and their checks; a list enables
a subset.

## 8. Goldens

### 8.1 Storage

Default path: `golden/<variant file-safe id>.<ext>` next to the manifest,
where the id **excludes** `render` and `iterate` (all render paths must
match one reference, which is itself a check). Override:

```toml
golden = { path = "golden/custom.png", split_by = ["render", "platform"] }
```

Formats: 8 bpc → PNG8, 16 bpc → PNG16, 32 bpc → EXR (`image` crate `exr`
feature).

### 8.2 Tolerance

All values are normalized to `[0, 1]` so one tolerance fits every depth.

| Key           | Default  | Meaning                                         |
| ------------- | -------- | ----------------------------------------------- |
| `max_abs`     | `1/255`  | Largest allowed per-channel difference.         |
| `max_bad`     | `0`      | Fraction of pixels allowed to exceed `max_abs`. |
| `min_psnr`    | none     | Optional floor on PSNR (dB).                    |

### 8.3 Bless and failure artifacts

- `aexlo test --bless [filter]` writes or overwrites goldens for passing
  (non-crashing) variants and prints which files changed.
- A missing golden is a `fail` unless `--bless` is given. CI never creates
  goldens implicitly.
- On mismatch the harness writes
  `target/aexlo/<variant>/{actual,expected,diff}.png`; `diff` is a heatmap
  scaled to the tolerance.

### 8.4 AE-sourced goldens

`golden = { source = "ae", path = "golden/ae/dot_glow.png" }` marks a
reference rendered by real After Effects. These are never overwritten by
`--bless`, and their failures are reported as **parity** failures, separately
from regressions. This extends the `playground` idea (AE as ground truth) to
user plugins.

## 9. Parameter fuzzing

`aexlo test --fuzz <n> [--seed s] [preset]` renders `n` variants with
parameters drawn from each parameter's declared range (`param_slider_range`,
`param_choices`, checkbox, and points within the frame). Only `finite`,
`coverage`, `bounds`, crash and timeout apply; there are no goldens. A failing
case is printed as a ready-to-paste `[[preset]]` block with its seed.

## 10. Benchmarks

### 10.1 Protocol

Per variant: load once, `warmup` untimed renders, then `samples` timed
renders. Reported: median, min, mean, p95 and megapixels/s. Benches always
run serially, one worker, `ObserveLevel::Commands`, strict off.

### 10.2 Phase breakdown

From Observer command events: `PRE_RENDER`, `SMART_RENDER` / `RENDER`,
GPU dispatch, and **host overhead** (wall time minus time inside the plugin's
entry point: input conversion, world setup, readback). The breakdown is what
separates "the plugin got slower" from "aexlo got slower".

### 10.3 Baselines

```sh
aexlo bench --save-baseline main
aexlo bench --baseline main --threshold 5%
```

Stored in `.aexlo/baselines/<name>.json` next to the manifest (commit them or
not, as the project prefers). Each record carries a machine fingerprint (OS,
CPU model, GPU, aexlo version); comparing across fingerprints warns.

A variant **regresses** when both its median and its min are slower than
the baseline by more than the threshold. Requiring the min as well filters
out one-off noise. Regressions exit non-zero.

### 10.4 Compatibility

`AEXLO_BENCH_*` variables keep working for `cargo bench -p aexlo-bench`.
They synthesize an implicit preset, so both front-ends share one timing loop
(`aexlo_harness::bench`), as `aexlo bench` and `aexlo-bench` already do today.

## 11. Preview

`aexlo dev` (source watch) and `aexlo preview` (artifact watch) share the
browser viewer in `crates/cli/src/viewer.rs`. It gains:

- a **preset picker**: renders the selected variant exactly as `test` would;
- parameter controls (existing), initialized from the preset;
- a **compare** mode against the golden: wipe, side-by-side, diff heatmap;
  plus depth-vs-depth and render-path-vs-render-path;
- a time scrubber driving `time.frame`;
- the last render's phase timings and strict report;
- **"Save as preset"**: appends the current state to `aexlo.toml` with
  `toml_edit`, preserving formatting, so a bad frame found by eye becomes a
  regression test in one click.

`studio` gains "Export layer effect as preset" in a later milestone.

## 12. CLI

```text
aexlo presets   [filter]                 List expanded variants
aexlo test      [filter] [--bless] [--fuzz n] [--seed s]
aexlo bench     [filter] [--save-baseline n | --baseline n --threshold p]
aexlo check     [filter]                 test + strict + baseline (if configured), for CI
aexlo dev       [--preset name] ...      existing flags kept
aexlo preview   <plugin> [--preset name] ...
aexlo render    <plugin> [--preset name] ...   existing flags override the preset
aexlo worker                              hidden; §6.2
```

Common flags: `--manifest <path>`, `--depth 8,16`, `--render smart` (narrow
the matrix), `--isolate preset|variant|none`, `--jobs <n>` (parallel workers
for test/check; bench is always 1), `--strict`, `--format human|json`,
`--junit <path>`.

`filter` matches variant display ids by substring, or glob when it contains
`*`.

Exit codes:

| Code | Meaning | Cases |
| ---- | ------- | ----- |
| `0`  | Every variant passed or was skipped. | |
| `1`  | A plugin ran and was judged bad. | `fail`, `error`, `crash`, `timeout` (§6.3); a bench regression (§10.3). |
| `2`  | The manifest or the command line is invalid. | TOML syntax; unknown key, preset or parent; `inherits` cycle; ambiguous parameter name; bad flag value. |
| `3`  | **Harness error**: the run never reached a verdict. | Worker cannot be spawned or found; temp or output directory not writable; malformed worker response (an aexlo bug); `crate` mode `cargo build` fails; a baseline or golden file exists but cannot be read or decoded. |

The split tells CI what to do: `1` means fix the plugin; `3` means the
environment or aexlo itself is at fault, and a retry may succeed. A build
failure is `3` rather than `1` because no variant was judged. When a run has
both judged failures and a harness error, the exit code is `3`.

## 13. Rust API

```rust
// Cargo.toml: [dev-dependencies] aexlo-test = "0.1"

#[aexlo::test(depth = [8, 16, 32], render = [smart, gpu], preset = "dot_glow")]
fn glow_is_symmetric(fx: &mut aexlo::PluginInstance) -> aexlo_test::Result {
    fx.set_param_named("Radius", 100.0)?;
    let frame = aexlo_test::render(fx)?;
    frame.assert_symmetric_about((320, 240), 0..20, 4.0 / 255.0)?;
    frame.assert_golden("dot_glow")?;
    Ok(())
}
```

- Runs in-process. Expands to one `#[test]` per variant, named
  `glow_is_symmetric__d16_gpu`. Unsupported variants print "skipped" and pass.
- `preset = "..."` starts from that preset's resolved state, read from the
  nearest `aexlo.toml` at test time.
- Unlike `#[aexlo::preview]`, tests are not `#[ignore]`d.
- Assertions on `Frame`: `assert_golden`, `assert_close`, `assert_pixel`,
  `assert_region`, `assert_symmetric_about`, `assert_opaque`,
  `assert_finite`, all with normalized tolerances. A failure writes the same
  artifacts as §8.3.
- `aexlo_test::check(fx, &["coverage", "deterministic"])` runs §7 checks on
  an in-process instance.

## 14. Migration

| Existing                                     | Becomes                                                     |
| -------------------------------------------- | ----------------------------------------------------------- |
| `aexlo::{save_preview, preview_mode, ...}`   | `aexlo_harness::preview`; `#[aexlo::preview]` expands to it. Users add `aexlo-test` as a dev-dependency. |
| `tests/e2e/src/bin/render_one.rs`            | `aexlo worker`                                              |
| `aexlo-bench` param/resolution/input helpers | `aexlo_harness::preset` + `bench`                           |
| CLI `--set <index>=<value>`                  | Also accepts names via `param_indices`                      |
| `tests/e2e` hand-written fixtures            | May adopt a manifest per fixture. Not required.             |

## 15. Milestones

Each milestone is done when its acceptance criteria pass on macOS arm64, and
on Windows x64 where a fixture exists.

| #  | Scope | Acceptance |
| -- | ----- | ---------- |
| M1 | Core: `param_indices`, generic depth I/O (16, then 32), `Observer` at `Commands` level. | A bundled fixture declaring deep color renders at 16 bpc and reads back `Layer<Depth16>`; command timings are observable. |
| M2 | Harness: manifest, `inherits`, matrix, worker protocol, outcomes; `aexlo presets`, `aexlo render --preset`. | A manifest over two fixtures where one crashes: `aexlo test` reports one `crash` and the remaining variants still run. `render_one` removed. |
| M3 | Test: goldens, `--bless`, `finite`/`coverage`/`deterministic`; JSON and JUnit output; `aexlo-test` with `#[aexlo::test]`. | `demos/noise` has a manifest and an `#[aexlo::test]`; changing its hash constant fails both with diff artifacts. |
| M4 | Strict: guard bands, allocation and checkout tracking; `bounds`/`allocations`/`checkouts`/`flags`; `iterate-parallel`, `depth-consistency`; fuzzing. | A test plugin that writes one row past its world fails `bounds`; one that leaks a handle fails `allocations`. |
| M5 | Bench on presets: phase breakdown, baselines, regression exit code. `aexlo-bench` migrated. | Artificially slowing a fixture's render beyond the threshold makes `aexlo bench --baseline` exit 1. |
| M6 | Viewer: preset picker, compare modes, time scrubber, "Save as preset". | A saved preset round-trips: the viewer shows it, then `aexlo test` runs it. |
| M7 | `aexlo check`, CI template (GitHub Actions, macOS + Windows), AE-sourced goldens. | The template runs green on this repo's fixtures. |

## 16. Open questions

1. **Observer attribution in iterate threads.** Propagating the thread-local
   into the rayon pool may cost more than `Commands`-level tracing allows. If
   it does, suite-call attribution could be limited to the dispatching thread.
2. **GPU tolerance.** GPU and CPU paths, and different GPUs, rarely agree
   bit-for-bit. Should `render = gpu` get a looser default `max_abs`, or
   always `split_by = ["render"]`?
3. **Masks in the manifest.** `set_mask_paths` exists, but a textual mask
   format (vertices, tangents, mode, feather) is a format of its own. It is
   deferred until studio export (M6+) shows what people actually write.
4. **32 bpc poison value.** A signalling NaN is unambiguous but trips
   `finite`. `coverage` must run first and claim those pixels so they are not
   reported twice.
