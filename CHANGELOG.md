# Changelog

All notable changes to the `aexlo` crate are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the crate follows
[Semantic Versioning](https://semver.org/): while it is `0.x`, a minor release
may break the API, and every breaking change is listed below.

## [0.1.0-beta.1] - 2026-11-09

The first public beta. It loads `.aex` / `.plugin` effects outside After
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

[0.1.0-beta.1]: https://github.com/potistudio/aexlo/releases/tag/v0.1.0-beta.1
