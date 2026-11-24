# SDK layout check — the bindings against the SDK headers

English | [日本語](sdk-layout.ja.md)

aexlo and `playground/probe` use the same Rust bindings of the After Effects
SDK (`after-effects-sys`, re-exported as `after_effects::sys`). If a field of
those bindings is at the wrong offset, it is wrong on both sides, and a trace
diff between the probe and aexlo does not show it. This matters most for
structs a plugin writes and aexlo reads, such as `PF_OutData`.

`after-effects-sys` is generated without layout tests, and aexlo uses its
prebuilt bindings, so nothing else compares them with the real headers. The
`aexlo_sdk_layout` crate (`tests/sdk_layout`) does, in two halves:

1. The `snapshot` binary compiles a C++ program against the SDK headers. The
   program prints the `sizeof` and `alignof` of each struct and the
   `offsetof`, size and alignment of each field. The binary writes them to
   `fixtures/sdk-layout/<os>-<arch>.json`. It needs the SDK and a C++
   compiler, so a maintainer runs it and commits the result. The snapshot
   holds only numbers and names, no header text.
2. A test compares `size_of`, `align_of` and `offset_of!` of
   `after_effects_sys` with the snapshot for the target it runs on. It runs
   in CI, with no SDK.

```bash
cargo test -p aexlo_sdk_layout                         # compare (every machine)
cargo run -p aexlo_sdk_layout --bin snapshot           # regenerate (needs the SDK)
AE_SDK_DIR="/path/to/AE SDK" cargo run -p aexlo_sdk_layout --bin snapshot
```

- [1. What is checked](#1-what-is-checked)
- [2. Mismatches](#2-mismatches)
- [3. Regenerating the snapshot](#3-regenerating-the-snapshot)
- [4. Limits](#4-limits)

## 1. What is checked

The struct list comes from the sources, not from a hand-written list. The
generator collects every identifier in `crates/aexlo/src` and
`playground/probe/src` that names a struct, union or type alias of the
bindings. It then follows field types, function pointer signatures and
aliases from there. So a suite's function table brings in every struct its
functions take, and `PF_ParamDef` brings in every member of
`PF_ParamDefUnion`. `EXTRA_ROOTS` in the generator adds structs that the
sources do not name.

Every field of each struct is listed with its Rust path, and the C++
program measures the SDK field of the same name. bindgen's renames are
undone: `type_` is `type` in C, a field of an anonymous member
(`__bindgen_anon_1.x`) is the C field `x`, and an anonymous type held by a
named member (`PF_ParamDef__bindgen_ty_1`) is measured as the type of that
member. Structs that the bindings leave opaque (only declared in C) are
listed under `skipped` with the reason.

The test checks all structs in the snapshot; `build.rs` generates the Rust
side from the same list. Two more tests keep the snapshot current:

- `snapshot_covers_every_struct_the_sources_use` fails when the sources
  start to use a struct the snapshot does not cover. `unchecked` in the
  snapshot lists the names that lead to such structs.
- `snapshot_is_of_the_locked_bindings` fails when `Cargo.lock` has another
  version of `after-effects-sys` than the one measured.

Only Windows x64 has a snapshot so far. On any other target the three tests
are reported as ignored, with the reason; with `--ignored` they fail, and
never pass without comparing anything.

## 2. Mismatches

The test reports every mismatch in one failure, grouped by kind:

| Kind | Meaning |
| ---- | ------- |
| structs the SDK headers do not declare | The bindings have a type the headers lack |
| structs the SDK only declares | The headers declare the type but do not define it |
| named differently in the SDK headers | The headers have the struct or field under an older name. The layout is still compared, under that name |
| `size_of` / `align_of` differs | The whole struct |
| field offsets differ / field sizes differ | One field |
| Rust fields the SDK struct does not have | A field name the headers lack |
| SDK fields the Rust struct does not have | SDK bytes that no Rust field accounts for, beyond alignment padding |

The last kind is found from the numbers: each field starts at most at the
end of the previous one rounded up to its alignment, so bytes past that
belong to a field the bindings do not name.

A mismatch that is understood and harmless goes into
`tests/sdk_layout/src/allowlist.rs`, one entry per mismatch, with the
reason. An entry that matches no mismatch fails the test, so the list cannot
go stale. Do not loosen the comparison instead.

### Known mismatches

The bindings of `after-effects-sys` 0.4.0 come from SDK 25.6, and the
Windows snapshot was measured from the 23.4 SDK (`PF_AE_PLUG_IN_SUBVERS` 28).
The older headers spell three names differently. The generator measures each
under its old name (`RENAMED`), and the offsets and sizes are the same:

| Rust | SDK 23.4 |
| ---- | -------- |
| `PF_ParamDef.name_do_not_use_directly` | `PF_ParamDef.name` |
| `PF_ANSICallbacksBlock` | `PF_ANSICallbacks` |
| `PF_ANSICallbacksBlock.unused_longA` | `PF_ANSICallbacks.ansi_procs` |

No offset, size or alignment differs.

## 3. Regenerating the snapshot

Regenerate after upgrading `after-effects-sys`, when the coverage test asks
for it, or with newer SDK headers:

1. Put the SDK where `AE_SDK_DIR` points: the folder that holds `Headers/`
   and `Util/`. The default is `refs/AE SDK` in the workspace, which git
   ignores.
2. Unset `AESDK_ROOT`. With it, `after-effects-sys` generates its own
   bindings instead of the prebuilt ones aexlo uses, and the generator stops.
3. Run `cargo run -p aexlo_sdk_layout --bin snapshot`. It finds the C++
   compiler through the `cc` crate, as `crates/aexlo/build.rs` does (MSVC on
   Windows).
4. Run `cargo test -p aexlo_sdk_layout`, then commit the snapshot with the
   change that needed it.

The program includes the same headers as `after-effects-sys`'s
`wrapper.hpp`, with `_WINDOWS` defined on Windows, but not
`Mach-O_prefix.h`. That header only defines `__MACH__`, and a Windows plugin
never includes it.

## 4. Limits

- A bitfield has no byte offset. Its field is listed as `bitfield`, and that
  struct gets no hole check. The current Windows bindings have no bitfields.
- In a union, an SDK member smaller than the union that the bindings lack
  leaves no hole, so it goes unseen.
- The generator has only run with MSVC on Windows. A macOS snapshot needs a
  run on a Mac; the code takes the bindings file and compiler flags for the
  host, but that path is untested.
