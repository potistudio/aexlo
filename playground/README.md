# playground — host-behavior verification for aexlo

English | [日本語](README.ja.md)

## First run

This playground measures host API behavior. If you want to build an image-processing application, start with the [getting started guide](../docs/getting-started.md) and [examples](../examples/README.md).

Run the following from the repository root; After Effects is not required:

```sh
cargo run -p playground -- run --in-process
cargo run -p playground -- report target/probe/trace-aexlo.jsonl
```

`--in-process` runs the probe directly. To include dynamic library loading, use `cargo run -p playground -- run`. The trace is written to `target/probe/trace-aexlo.jsonl` and the preview to `target/probe/preview-aexlo.png`. Subsequent runs overwrite these files, so save traces under separate names before comparing them. In the report, `unavailable` identifies suites the probe could not acquire.

The After Effects `package` instructions below target Windows. Packaging an installable macOS bundle is not supported yet.

Infrastructure for answering one question precisely: **does aexlo behave like
the real After Effects host?**

The idea: *the plugin is the measuring instrument; the host is the variable.*
`playground/probe` is a real, loadable AE effect plugin (written in Rust,
PiPL and all) that verifies host behavior **one function, one suite, one
variable at a time**: every check feeds a host service fixed inputs and
records the exact output as a `fact` in a JSONL trace — `sin(0.5)` down to
the last bit, every `sprintf` conversion, rowbytes layout policy, `blend`
rounding, iterate invocation counts. Load the same binary into real AE and
into aexlo, then diff the two traces: facts are deterministic by
construction, so any difference is a real emulation divergence — never
scenario noise. A headless aexlo run and a GUI AE session drive the plugin
completely differently (command order, timing, UI chatter), so all of that
is recorded only as context and excluded from the default comparison.

```text
┌───────────────┐  load   ┌─────────────────┐  writes  trace-ae.jsonl ─┐
│ After Effects │ ──────► │                 │ ───────►                 │  playground diff
├───────────────┤         │  AexloProbe.aex │                          ├─────────────────►
│     aexlo     │ ──────► │   (the probe)   │ ───────►                 │   divergence list
└───────────────┘  load   └─────────────────┘  writes  trace-aexlo.jsonl
```

## Quick start

```bash
# 1. Trace the probe under aexlo (builds it, renders a frame, prints a report)
cargo run -p playground -- run
#    The SmartRender variant (see below) renders at 8, 16 and 32 bpc:
cargo run -p playground -- run --variant smart

# 2. Trace the same probe in real After Effects (Windows)
cargo run -p playground -- ae
#    → target/probe/trace-ae.jsonl

# 3. Compare
cargo run -p playground -- diff target/probe/trace-ae.jsonl target/probe/trace-aexlo.jsonl
```

To do step 2 by hand instead, run `cargo run -p playground -- package`, which
drops `playground/dist/AexloProbe.aex` and
`playground/dist/AexloProbeSmart.aex`. Copy one into a plugin folder such as
`C:\Program Files\Adobe\Common\Plug-ins\7.0\MediaCore\`, apply
"Effect > aexlo > Aexlo Probe" (or "Aexlo Probe Smart") to a layer, and render a
frame. The trace lands in `%TEMP%\aexlo-probe\`; the effect's About dialog shows
the exact path.

`diff` exits non-zero when behavior diverges, so it can gate CI once a
reference trace from real AE is checked in.

## Commands

| Command | What it does |
| --- | --- |
| `run [--release] [--in-process] [--variant legacy\|smart] [--trace <file>] [--input <png>]` | Build + load the probe under aexlo, nudge every param off its default, render `fixtures/input.png`, write `target/probe/trace-aexlo.jsonl` and a preview PNG, print the report. `--variant smart` runs the SmartRender effect instead, once at each of 8, 16 and 32 bpc, and writes `trace-aexlo-smart.jsonl` and `preview-aexlo-smart.png`. `--in-process` drives the entry point without `dlopen` — breakpoints inside the probe work; it runs the legacy build only. |
| `report <trace.jsonl>` | Human-readable summary of one trace (host identity, command/error table, suite availability map, callback results, world layouts, param values). |
| `diff <a> <b> [--all]` | Key-by-key comparison of two traces. Compares facts, suite availability, and callback presence by default; `--all` also compares context (command counts, render scenario, timing). |
| `package [--debug] [--to <dir>]` | Build both variants in release mode and write `playground/dist/AexloProbe.aex` and `playground/dist/AexloProbeSmart.aex`. |
| `pipl [--release] [--variant legacy\|smart]` | Parse the PiPL resource back out of the built DLL — preflight that real AE will accept it (Windows). The out flags are printed by name. |
| `ae [--ae <AfterFX.exe>] [--trace <file>] [--frames <n>] [--input <png>] [--plugin-dir <dir>] [--timeout <secs>] [--no-build] [--variant legacy\|smart]` | Package the probe, install it into After Effects, apply it and render through `AfterFX.exe -r`, and save the trace to `target/probe/trace-ae.jsonl` (Windows). `--variant smart` renders the SmartRender effect at project depths 8, 16 and 32 bpc and saves `trace-ae-smart.jsonl`. See [Recording in real After Effects](#recording-in-real-after-effects). |

## Recording in real After Effects

`playground ae` does what the manual steps do, without the GUI:

1. Builds the release probe and copies it to
   `<After Effects>\Support Files\Plug-ins\aexlo\AexloProbe.aex`, unless the
   same bytes are already there. `--plugin-dir` picks another folder.
2. Writes `target/probe/ae-run/trace.jsx` from
   [`harness/ae/trace.jsx`](harness/ae/trace.jsx) and hands it to
   `AfterFX.exe -r`. The script imports `fixtures/input.png` (or `--input`),
   applies the probe, sets the same parameter values `run` sets under aexlo,
   renders `--frames` frames with `saveFrameToPng`, and removes everything it
   added. It reports back through `target/probe/ae-run/result.json`.
3. Saves the trace and prints the report.

Which After Effects it uses: `--ae`, then the `AEXLO_AE_EXE` environment
variable, then the running instance, then the newest
`C:\Program Files\Adobe\Adobe After Effects <year>` (beta builds only when
named).

How it runs depends on whether After Effects is already open:

| After Effects | What happens | Trace |
| --- | --- | --- |
| Not running | `AfterFX.exe -r` starts a new instance with `AEXLO_PROBE_TRACE` set. The script quits it without saving. | One session, from plugin load to `GLOBAL_SETDOWN`. |
| Running | `AfterFX.exe -r` forwards the script to it. The script works in the open project and leaves it as it was. | A copy of that instance's `%TEMP%\aexlo-probe\trace-*-pid<pid>.jsonl`, which holds the whole session so far. |

After Effects loads plugins only at startup. If it is running and the probe
it loaded is older than the build, `ae` stops and asks you to close it. Run
it again and it starts a fresh instance.

If no result arrives within `--timeout` seconds (default 600), a dialog is
probably waiting in the After Effects window. Startup dialogs, such as crash
recovery, are not suppressed.

## Facts: the unit under test

`probe/src/checks.rs` runs at GLOBAL_SETUP, each check panic-guarded so one
broken host service can't silence the rest. Every fact is
`fixed input → exact output`:

- **Variables** — `appl_id`, spec version, quality, in_flags: static host
  identity read straight from `PF_InData`.
- **ANSI callbacks** — all 17 math entries at full f64 precision (a last-bit
  libm difference is a genuine finding), an `sprintf` formatting matrix
  (width, precision, alignment, zero-pad, `%d/%u/%x/%f/%e/%g/%s/%c/%%`), and
  `strcpy` semantics.
- **Color callbacks** — `RGBtoHLS` / `HLStoRGB` on fixed pixels.
- **Host handles** — new/lock/write/read/unlock/resize/dispose cycle: sizes
  reported at each step, whether resize preserves contents.
- **Worlds & pixel ops** — worlds allocated *through the host*
  (`utils.new_world`, falling back to World Suite 2): rowbytes/layout policy,
  clear-on-alloc, `fill` (full and sub-rect, edge exclusivity), `copy`,
  `blend` at ratio 0.5 (rounding direction), `premultiply` rounding,
  `iterate` invocation count + result hash.
- **Kernel** — `gaussian_kernel` diameter and normalized weights.
- **Suite functions** — one level below the availability map: the same fixed
  inputs pushed through `PF ANSI Suite`, `PF Handle Suite`,
  `PF World Suite v2` (per-pixel-format allocation + `PF_GetPixelFormat`
  echo), and `PF Iterate8 Suite v2`.

Alongside the facts, the trace also captures **context**: the suite
availability map (~45 suites × versions), the `PF_UtilCallbacks` presence
map (both compared by default too, since they're deterministic), plus
command flow, `PF_InData` snapshots, render params, and world hashes at
render time — informational only, excluded from the default diff.

The probe still renders a deterministic, parameter-driven test pattern
(float slider, checkbox, popup, color, angle, point), so parameter plumbing
is visible on screen inside AE as well.

## The SmartRender variant

Out flags must match the PiPL, so the SmartRender path is measured by a
second effect rather than a runtime switch: the `smart` Cargo feature of
`probe` builds "Aexlo Probe Smart" (match name "AEXLO Probe Smart",
`AexloProbeSmart.aex`). It declares `PF_OutFlag2_SUPPORTS_SMART_RENDER`,
`PF_OutFlag_DEEP_COLOR_AWARE`, `PF_OutFlag2_FLOAT_COLOR_AWARE` and
`PF_OutFlag2_SUPPORTS_GPU_RENDER_F32`, runs every GLOBAL_SETUP check the
legacy effect runs, and renders the same test pattern at 8, 16 and 32 bpc.
The harness builds it into `<target>/probe-smart/`, so the two builds never
overwrite each other, and both effects can be installed side by side.

`probe/src/smart.rs` records as facts the host's answers to requests the
probe builds itself, one set per bit depth (`smart.8.*`, `smart.16.*`,
`smart.32.*`). `cargo run -p playground -- ae --variant smart` switches the
project depth to 8, 16 and 32 bpc in one session, so a single trace carries
all three sets. A manual run installs `AexloProbeSmart.aex` from `package`
and renders a frame at each depth:

- `checkout_layer` with a fixed rectangle inside any layer of at least 10x9
  pixels, with an empty rectangle, and with parameter index 99;
- `checkout_layer` 4 pixels past every edge of the layer, reported relative
  to the layer's `max_result_rect` so that the layer size cancels out;
- the layout of the fixed-rectangle checkout, and `checkin_layer_pixels`
  on it;
- `GuidMixInPtr` with fixed bytes, and whether `pre_render_data` reaches
  SMART_RENDER intact;
- the `PF_GetPixelFormat` answer and world flags of the checked-out input
  and output worlds.

The probe makes only calls whose behavior the SDK defines. It does not check
in one id twice, check out pixels for an id that pre-render did not check out,
or pass `GuidMixInPtr` a null buffer, because a host may abort on those. It
writes and hashes pixels only when `PF_GetPixelFormat` returns a known pixel
format and every row fits in `rowbytes`.

The facts line up only when the effect sits on a plain footage or solid
layer of at least 10x9 pixels, with nothing that grows the layer.

Everything that depends on the scenario is context, excluded from the
default diff: the `PF_PreRenderExtra` input (output request, field,
channel mask, bit depth, GPU framework), the full checkout results
(`par`, `solid`, `ref_width`/`ref_height`), the `PF_PreRenderOutput` that
the probe returns, and the input and output world layouts and hashes at
render time. `report` lists them under `smart/` keys.

GPU: a host with a GPU sends `PF_Cmd_GPU_DEVICE_SETUP`. The probe records
the framework, the device index, and the GPU Device Suite's device count
and device info as context under `gpu/`. Device pointers are recorded only
as null or non-null and are never dereferenced. The pre-render never sets
`PF_RenderOutputFlag_GPU_RENDER_POSSIBLE`, so every frame renders on the
CPU; the probe does not render on the GPU.

## Trace destination

1. `AEXLO_PROBE_TRACE` — exact file path (the harness sets this),
2. `AEXLO_PROBE_DIR` — directory, auto-named file,
3. otherwise `%TEMP%/aexlo-probe/trace-<time>-pid<pid>.jsonl`
   (`trace-smart-<time>-pid<pid>.jsonl` for the smart variant).

The harness builds into `CARGO_TARGET_DIR` when it is set, or `target/`
otherwise.
Traces and previews stay in `<workspace>/target/probe/` either way.

Each trace starts with `trace_schema`, which changes whenever events or
fact names change. `diff` compares it, so traces from different schemas
show up as a difference instead of as missing facts.

## Layout

```text
playground/
  probe/     Rust cdylib → the .aex. build.rs embeds the PiPL resource via
             the `pipl` crate. Also an rlib for --in-process.
  harness/   `playground` CLI: run / report / diff / package / pipl.
  dist/      packaged AexloProbe.aex and AexloProbeSmart.aex (gitignored
             build output).
```

## Notes

- The probe depends on `after-effects 0.4.0` — the same crate as aexlo — but
  deliberately talks to its raw `sys` re-export instead of the high-level
  wrapper: the probe measures the host, and a wrapper in between would make
  it measure the wrapper. Sharing the crate version keeps struct layouts
  identical on both sides of the ABI.
- The legacy effect covers `PF_Cmd_RENDER`. The smart effect covers
  `PF_Cmd_SMART_PRE_RENDER`, `PF_Cmd_SMART_RENDER`, and GPU device setup,
  but not GPU rendering (`PF_Cmd_SMART_RENDER_GPU`).
- macOS packaging (`.plugin` bundle + rsrc PiPL) is not wired up yet; the
  probe itself is platform-independent.
