# Changelog

All notable changes to the `aexlo` crate are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the crate follows
[Semantic Versioning](https://semver.org/): while it is `0.x`, a minor release
may break the API, and every breaking change is listed below.

## [Unreleased]

## [0.3.0] - 2026-11-23

### Added

- `Observer::unsupported` hears about what the plugin asked for and aexlo
  does not do (`UnsupportedEvent`, at every `ObserveLevel`): suites or
  versions it does not serve (`UnsupportedKind::MissingSuite`), host
  functions it only stubs (`Stub`) and calls it serves but declines for those
  arguments (`Declined`). They are still logged as warnings.
- `Loader::observer` installs an `Observer` before `PF_Cmd_GLOBAL_SETUP`, so
  setup commands, the suite calls they make and what they allocate are
  observed too.
- `PluginInstance::allocations` lists the handles and worlds the plugin holds
  (`LiveAllocation`) while an observer is installed or allocation tracking is
  on, and `PluginInstance::read_allocation` copies their bytes between
  commands. Memory the plugin freed on another thread, such as an iterate
  worker, is neither listed nor read.
- `PluginInstance::state` returns an `InstanceState`: the `global_data` and
  `sequence_data` handles, the next command's time, and the `PF_OutData` the
  last command left.
- `ObserveLevel::Arguments` hands each suite call's arguments and result to
  the observer (`SuiteCallEvent::args`, `SuiteCallEvent::result`); the lower
  levels leave them empty and pay nothing for them. The Handle and Iterate
  suites, `AcquireSuite` / `ReleaseSuite` and the utility callbacks are now
  reported as suite calls too.
- Text layers: `PluginInstance::set_text_layer` puts a `TextLayer` on the
  effect's layer. `AEGP_GetLayerObjectType` then answers
  `AEGP_ObjectType_TEXT`, and the new "AEGP Text Layer Suite" v1 hands out its
  glyph outlines, which the PF Path Data Suite reads with absolute tangents as
  After Effects gives them. `TextLayer::layout` sets a string in a font
  (`TextStyle`) and `TextLayer::rasterize` draws it as the input layer, so
  text effects such as TextGrad render. `AexloError::Font` reports a font that
  cannot be read.
- `Pipl::read` reads a plugin's PiPL resource without loading it: from a
  Windows binary's `PIPL` resource, a macOS bundle's `.rsrc` files (in their
  data or resource fork), a Mach-O binary's `__TEXT,__pipl` section or its
  resource fork; a `.rsrc` it cannot read is passed over for the next. Each
  `PiplProperty` decodes to a `PiplValue`, and `Pipl` has accessors for the
  name, category, match name, entry point, `eVER` (`EffectVersion`) and
  `eGLO` / `eGL2`. `AexloError::InvalidPipl` reports a malformed one.
- `PluginInstance::version` returns the `my_version` the plugin reported at
  `PF_Cmd_GLOBAL_SETUP`, to compare with `Pipl::effect_version`.
- `PluginInstance::param_button_label` returns the text on a button
  parameter (`PF_ButtonDef`), which plugins often fill in place of the
  parameter's name.
- "AEGP Stream Suite" v11 (`AEGP_GetUniqueStreamID`) and "AEGP Keyframe
  Suite" v5 (keyframe label colours). RXXR applies its Preset popup through
  them, and ten more fixtures that asked for Stream Suite v11 (the AOD
  effects, Circle-Repeater, RainyDay) no longer report it missing.
- The AEGP Utility, Persistent Data and PF Param Utils suites report their
  calls to the observer, so `aexlo inspect` and `aexlo compat` see them.
  `AEGP_ExecuteScript`, and `AEGP_GetPluginPlatformRef` on macOS, report
  themselves as stubs.
- "AEGP Layer Mask Suite" v6 and v7 and "AEGP Mask Outline Suite" v3 and v5
  read the masks `PluginInstance::set_mask_paths` puts on the layer, and
  `AEGP_GetNewMaskStream` gives a mask's outline, opacity, feather and
  expansion streams. Changing a mask is declined. BitonicPixelSorter, which
  reads its Path through them once it is registered with AEGP, renders with
  masks on the layer.
- `AEGP_SetStreamValue` sets an effect parameter's value, clamped to its valid
  range (a color keeps its alpha), and `AEGP_SetDynamicStreamFlag` hides and
  disables parameters (`PF_PUI_INVISIBLE`, `PF_PUI_DISABLED`), which
  `AEGP_GetDynamicStreamFlags` now reports. Deep Glow's and TextGrad's Reset
  and Swap buttons work through them, and Deep Glow's
  `PF_Cmd_UPDATE_PARAMS_UI` no longer fails.

### Fixed

- The published crate includes the MIT license text.
- The text a plugin reports (`PF_Cmd_ABOUT`, a dialog's message, an error)
  comes back whole. Line breaks are `\n` instead of After Effects' `\r`, which
  made each line overwrite the one before in a terminal, and text that is not
  UTF-8 is read as Latin-1, so a `©` stays one. The ANSI suite's `sprintf`
  formats with the C library's `vsnprintf` (at most 256 bytes) instead of
  knowing only `%d` and `%s`: a format that was not UTF-8 (Deep Glow 2,
  TextGrad, Displacer Pro) wrote nothing, and `%.120s` (Cosmic) was printed
  as is.
- On macOS the loader now finds the entry point the PiPL names, as it
  already did on Windows, before trying the usual symbol names.
- "AEGP Utility Suite" serves each version at its own layout: v3–7
  (`AEGP_UtilitySuite1`–`3`), v10–11 (`4`–`5`) and v13 (`6`). One table
  served every version from 1 to 18, so a v11 plugin calling
  `AEGP_RegisterWithAEGP` reached `AEGP_EndUndoGroup`, and a v13 plugin
  calling `AEGP_StartUndoGroup` reached `AEGP_RegisterWithAEGP`. Every
  other slot returned success without doing anything; the calls now do
  what the SDK says (versions, quiet errors, palette colours, HSF ratios,
  `AEGP_GetSuppressInteractiveUI` from `AppHost::is_render_engine`, the
  `APP` path of `AEGP_GetPluginPaths`). `AEGP_IsScriptingAvailable` answers
  false, since aexlo runs no scripts. Versions the SDK never defined are
  refused.
- `AEGP_GetDataHandle` and `AEGP_SetDataHandle` keep their data in the
  Persistent Data store instead of returning nothing and dropping it.
- An arbitrary-data parameter starts as a copy of its default
  (`PF_Arbitrary_COPY_FUNC` of `dephault`), as in After Effects, and falls back
  to `PF_Arbitrary_NEW_FUNC` only without a default or when the copy fails.
  Plugins whose `NEW_FUNC` makes an empty placeholder (TextGrad's gradient)
  rendered nothing.
- `AEGP_DisposeColorProfile` refuses a pointer it did not hand out instead of
  freeing it. A plugin that bailed out of a failed call disposed an
  uninitialised profile, which aborted the process.

## [0.2.0] - 2026-11-18

### Changed (breaking)

- `aexlo::sys` gives every SDK enum constant aexlo uses the type of its SDK
  typedef (`PF_Err_NONE` is a `PF_Err`, `PF_Cmd_RENDER` a `PF_Cmd`, ...), so a
  constant has the same type on Windows and macOS. The raw `after-effects-sys`
  types them `i32` on Windows and `u32` on macOS, so a cast that code needed on
  macOS only (`PF_Err_NONE as PF_Err`) can go, and code that used one as a `u32`
  (`let code: u32 = PF_Err_NONE`, or a comparison with a `u32`) now needs
  `as u32` on both. `PF_ExtendedSuiteTool_*`, `PF_SuiteTool_*`
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

[Unreleased]: https://github.com/potistudio/aexlo/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/potistudio/aexlo/releases/tag/v0.3.0
[0.2.0]: https://github.com/potistudio/aexlo/releases/tag/v0.2.0
[0.1.0]: https://github.com/potistudio/aexlo/releases/tag/v0.1.0
