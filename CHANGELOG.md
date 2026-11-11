# Changelog

All notable changes to the `aexlo` crate are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the crate follows
[Semantic Versioning](https://semver.org/): while it is `0.x`, a minor release
may break the API, and every breaking change is listed below.

## [Unreleased]

## [0.1.1] - 2026-11-11

### Changed (breaking)

- `aexlo::sys` gives every SDK enum constant aexlo uses the type of its SDK
  typedef (`PF_Err_NONE` is a `PF_Err`, `PF_Cmd_RENDER` a `PF_Cmd`, ...), so a
  constant has the same type on Windows and macOS. The raw `after-effects-sys`
  types them `i32` on Windows and `u32` on macOS, so code that cast a constant
  for macOS (`PF_Err_NONE as u32`) no longer needs it, and code that compared one
  with a `u32` needs a cast on both. `PF_ExtendedSuiteTool_*`, `PF_SuiteTool_*`
  and `PF_Step_*` are `A_long`. A constant aexlo does not use keeps the
  platform type; every other item is still `after-effects-sys`'s (#21).

### Fixed

- The crate's documentation builds on docs.rs: the macOS target is documented
  instead of the Windows one, which cannot cross-build the SEH shim there
  (#41).

## [0.1.0] - 2026-11-09

The first public release. It loads `.aex` / `.plugin` effects outside After
Effects on Windows x64 and macOS arm64 and renders them through the GPU, smart
and legacy paths. See the README for which SDK suites are implemented.

### Added

- `Host::loader` / `Host::entry_loader` / `Host::entry_loader_raw` and the
  `Loader` builder: strict mode, the input layer and the output size are set
  before the first command runs (#33).
- `OutFlags`, `OutFlags2`, `ParamUiFlags` and `PixelFormat`: typed flags and
  FourCCs with the SDK's names as constants (#32).
- `aexlo::sys`: a re-export of the `after-effects-sys` bindings the public API
  uses (#28).
- Crate documentation on the trust and process model: plugins run unsandboxed
  in this process, host state is process-wide, and `PluginInstance` is not
  `Send` (#29, #31, #35).

### Changed (breaking)

- `Host::try_load` is now `Host::load`. `try_load_with`, `try_load_with_initial`
  and `try_load_sized` are replaced by `Host::loader(path)` with `.strict()`,
  `.input()`, `.output_size()` and `.size()`, and `from_entry_raw_with` by
  `Host::entry_loader_raw(addr).strict(..)` (#33).
- `PluginInstance::out_flags`, `out_flags2`, `param_ui_flags` and
  `supported_pixel_formats` return the typed flags above instead of `i32` /
  `u32` (#32).
- `ParamValue`, `MaskMode` and `TimeDisplayMode` are `#[non_exhaustive]` (#25).
- `AexloError::PluginLoad` is `{ path, source }` with an opaque source, so it
  no longer exposes `dlopen2::Error` (#27).
- Renamed for consistency (#34):
  - `PluginInstance::write_rendered_pixels` → `write_output_rgba8`
  - `Layer<Depth8>::write_rgba_bytes` → `write_rgba8`
  - `Layer<Depth8>::from_raw` → `from_rgba8`
  - `AnyLayer::converted` → `convert`
- `set_main_window` is an `unsafe fn` (#30).
- `PixelDepth` is sealed, layer dimensions are validated (`LayerError`,
  `AexloError::InvalidSize`), and `Time` enforces its invariants.

### Fixed

- A panic in aexlo's own suite code while a plugin calls it is reported to the
  plugin as `PF_Err_INTERNAL_STRUCT_DAMAGED` instead of aborting the process
  (#31).
- On Windows, only a C++ exception leaving a plugin's entry point becomes
  `PF_Err_INTERNAL_STRUCT_DAMAGED`. An access violation crashes the process
  as on macOS instead of being reported as a failed command.

[Unreleased]: https://github.com/potistudio/aexlo/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/potistudio/aexlo/releases/tag/v0.1.1
[0.1.0]: https://github.com/potistudio/aexlo/releases/tag/v0.1.0
