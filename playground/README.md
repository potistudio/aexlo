# playground — host-behavior verification for aexlo

Infrastructure for answering one question precisely: **does aexlo behave like
the real After Effects host?**

The idea: *the plugin is the measuring instrument; the host is the variable.*
`playground/probe` is a real, loadable AE effect plugin (written in Rust,
PiPL and all) that records everything the host does to a JSONL trace —
command order, `PF_InData` contents, which suites `AcquireSuite` actually
vends, callback results, parameter values at render time, and world layouts
with pixel hashes. Load the same binary into real AE and into aexlo, then
diff the two traces to see exactly where the emulation diverges.

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
| `diff <a> <b> [--all]` | Key-by-key comparison of two traces. Volatile keys (exe path, serial, command counts, timestamps) are ignored unless `--all`. |
| `package [--debug] [--to <dir>]` | Build release and drop `playground/dist/AexloProbe.aex`. |
| `pipl [--release]` | Parse the PiPL resource back out of the built DLL — preflight that real AE will accept it (Windows). |

## What the probe records

Every line in the trace is one JSON event with `seq` / `t_ms` / `tid` /
`event` plus event-specific fields:

- **`cmd` (begin/end)** — every `EffectMain` invocation: command name, a full
  `PF_InData` snapshot (times, extents, downsample factors, quality, flags,
  which callback pointers are non-null), then the error code and the
  `PF_OutData` fields the handler touched.
- **`suite`** — one entry per (name, version) pair from a fixed table of ~45
  SDK suites: `AcquireSuite` error code and whether a pointer came back.
  Mirrors the implementation-progress table in the root README.
- **`utils_presence` / `callback`** — which `PF_UtilCallbacks` entries the
  host filled in, plus live results from calling the safe ones
  (`ansi.sprintf`, `ansi.strcpy`, math entries, and a full
  new/lock/write/resize/dispose cycle through the host handle allocator).
- **`param`** — every parameter as received at render time (type + value),
  catching value-translation bugs between host and plugin.
- **`world`** — input/output world layouts (dimensions, rowbytes, flags,
  origin) with FNV-1a pixel hashes, so "same picture in, same picture out"
  is checkable bit-for-bit.
- **`sequence`** — whether sequence-data handles survive
  SETUP → RESETUP/FLATTEN → SETDOWN with contents intact.
- **`panic`** — probe bugs surface in the trace instead of crashing the host.

The probe renders a deterministic test pattern driven by its six parameters
(float slider, checkbox, popup, color, angle, point), so parameter plumbing
is also visible on screen: if Invert doesn't invert, the host lied.

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
