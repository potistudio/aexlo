# playground — host-behavior verification for aexlo

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

# 2. Build the .aex for real After Effects
cargo run -p playground -- package
#    → playground/dist/AexloProbe.aex
#    Copy it into e.g. C:\Program Files\Adobe\Common\Plug-ins\7.0\MediaCore\
#    Apply "Effect > aexlo > Aexlo Probe" to a layer, scrub/render a frame.
#    The trace lands in %TEMP%\aexlo-probe\ — the effect's About dialog
#    shows the exact path.

# 3. Compare
cargo run -p playground -- diff "%TEMP%\aexlo-probe\trace-....jsonl" target/probe/trace-aexlo.jsonl
```

`diff` exits non-zero when behavior diverges, so it can gate CI once a
reference trace from real AE is checked in.

## Commands

| Command | What it does |
| --- | --- |
| `run [--release] [--in-process] [--trace <file>] [--input <png>]` | Build + load the probe under aexlo, nudge every param off its default, render `input.png`, write `target/probe/trace-aexlo.jsonl` and a preview PNG, print the report. `--in-process` drives the entry point without `dlopen` — breakpoints inside the probe work. |
| `report <trace.jsonl>` | Human-readable summary of one trace (host identity, command/error table, suite availability map, callback results, world layouts, param values). |
| `diff <a> <b> [--all]` | Key-by-key comparison of two traces. Compares facts, suite availability, and callback presence by default; `--all` also compares context (command counts, render scenario, timing). |
| `package [--debug] [--to <dir>]` | Build release and drop `playground/dist/AexloProbe.aex`. |
| `pipl [--release]` | Parse the PiPL resource back out of the built DLL — preflight that real AE will accept it (Windows). |

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

## Trace destination

1. `AEXLO_PROBE_TRACE` — exact file path (the harness sets this),
2. `AEXLO_PROBE_DIR` — directory, auto-named file,
3. otherwise `%TEMP%/aexlo-probe/trace-<time>-pid<pid>.jsonl`.

## Layout

```text
playground/
  probe/     Rust cdylib → the .aex. build.rs embeds the PiPL resource via
             the `pipl` crate. Also an rlib for --in-process.
  harness/   `playground` CLI: run / report / diff / package / pipl.
  dist/      packaged AexloProbe.aex (gitignored build output).
```

## Notes

- The probe depends on `after-effects 0.4.0` — the same crate as aexlo — but
  deliberately talks to its raw `sys` re-export instead of the high-level
  wrapper: the probe measures the host, and a wrapper in between would make
  it measure the wrapper. Sharing the crate version keeps struct layouts
  identical on both sides of the ABI.
- Legacy render path only for now (`PF_OutFlag2_SUPPORTS_SMART_RENDER` is not
  claimed); SmartRender/GPU probing is a natural next step.
- macOS packaging (`.plugin` bundle + rsrc PiPL) is not wired up yet; the
  probe itself is platform-independent.
