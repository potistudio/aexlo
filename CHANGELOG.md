# Changelog

All notable changes to the `aexlo` crate are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the crate follows
[Semantic Versioning](https://semver.org/): while it is `0.x`, a minor release
may break the API, and every breaking change is listed below.

## [Unreleased]

### Changed (breaking)

No public item was removed, but these can break a build or change what a
plugin renders or returns.

- "PF Batch Sampling Suite" v1 is served as After Effects serves it, with
  every entry null, where `get_batch_func` and `get_batch_func16` returned
  `PF_Err_BAD_CALLBACK_PARAM` and the sampling calls succeeded. A trace of
  the probe in After Effects 2026 shows the four entries null.
- `PF_PathVertexInfo` gives a mask's tangents as points in layer space, as
  the SDK guide describes them, not as offsets from the vertex. `MaskPath`
  still takes offsets. Every effect that reads mask paths sees different
  tangents, so its reference frames may need updating: Extrude, which pulled
  masks towards the origin, now follows them.
- Host callbacks aexlo does not implement return `PF_Err_INVALID_CALLBACK`
  where they returned `PF_Err_NONE` without doing the work: the stubbed
  utility callbacks, layer audio, unknown `get_platform_data` selectors and
  `AEGP_ExecuteScript`. A plugin that relied on the false success can now
  fail.
- `checkout_layer` in a pre-render refuses an index that is not a layer
  parameter, clips `result_rect` to the request and no longer claims that the
  input is solid. `checkout_layer_pixels` refuses an id that was never
  checked out, where it gave the input layer.
- `PF_AreStatesIdentical` refuses a null state instead of calling it
  identical, and `PF_GetCurrentState` refuses an index that stands for
  nothing and an effect it does not know.
- PF World Transform composites as After Effects 2026 does, checked against
  a trace of the probe (`playground/probe/src/xform.rs`) in After Effects:
  at 32 bpc exactly, at 8 and 16 bpc to within a few levels, but for
  Dissolve's random pattern and Hue, Saturation and Luminosity where both
  layers are partly transparent. Renders that use it can change:
  - `COPY` replaces the destination instead of compositing over it, and
    `rgb_only` keeps the destination's alpha.
  - Blend results are not clamped at 32 bpc (only Add was), and are at 8
    and 16 bpc. The classic Difference, Color Dodge and Color Burn work on
    premultiplied colours, the stencil and silhouette modes change only the
    destination's alpha, and Hue, Saturation, Color and Luminosity use Rec.
    601 luma and blend against the destination over the source.
  - Dissolve gives each pixel the source, opaque, or the destination, by
    `rand_seed`; it was Normal.
  - `m_flags` no longer changes anything: After Effects reads colours as
    straight either way.
  - `transform_world` takes the nearest pixel at `PF_Quality_LO` and filters
    premultiplied pixels at `PF_Quality_HI`, so edges no longer darken;
    `copy` picks the pixels After Effects picks when it stretches, and
    `copy_hq` averages the area each pixel covers.
  - `composite_rect` ignores its transfer mode and puts the destination over
    the source, and reads every world as 8 bpc, as After Effects does.
- PF World Transform and the legacy callbacks of the same names but
  `composite_rect` refuse worlds of two depths in one call, and worlds whose
  depth cannot be told, and report it. `copy` used to move raw bytes between them: AEPixelSorter2's
  legacy path at 16 and 32 bpc now fails where it drew a scrambled picture
  (its smart path is unaffected).

### Added

- Masks that move over time: `PluginInstance::set_mask_tracks` takes a
  `Track<MaskPath>` per mask, keyed in layer time, and `mask_tracks` gives
  them back; `mask_paths` is the masks at the current time. `PF_CheckoutPath`
  reads a mask at its `what_time` (a path checked out at another time is held
  until `PF_CheckinPath`), as do mask outline streams and their keyframes,
  `PF_GetCurrentState` over a range and `PF_IsIdenticalCheckout` for path
  parameters and the input layer.
- A read-only project around the effect: `PluginInstance::set_project` takes
  a `Project` of a `Comp` and `ProjectLayer`s (footage, null, text, shape),
  with ids, names, spans, stretch, parents and transforms. The Layer, Comp,
  Item, PF Interface, Stream and Dynamic Stream suites answer from it, and the
  handle of a layer that left is refused. Without a project the effect is
  still alone on a layer that fills its comp. `ProjectLayer::rasterize`
  draws a text or shape layer as an input, `Project::link_layer_param` says
  which layer a layer parameter shows, and `AexloError::InvalidProject`
  reports a project that does not hold together.
- Shape layers' paths and text layers' source text are streams: "AEGP Mask
  Outline Suite" (now also v2) reads a shape path as it reads a layer mask's
  outline, "AEGP Text Document Suite" v1 reads source text, and
  `AEGP_GetLayerStreamValue` reads the transform. Extrude draws shape
  layers and Discotext text layers through them.
- Time: `TimePoint` is exact, and `Track`s of `Keyframe`s interpolate linearly
  or hold. `PluginInstance::set_param_track` keyframes a parameter; the
  `params` array, `PF_CHECKOUT_PARAM`, streams, the Keyframe Suite and PF Param
  Utils all read that track, at the time each is asked about. With a project,
  `set_time` is the effect layer's time.
- Suites and versions: "AEGP Comp Suite" v19, v25 and v26, "AEGP Item Suite"
  v14, "AEGP Layer Render Options Suite" v1, "AEGP Stream Suite" v4, "PF World
  Suite" v1, and the new "AEGP Proj Suite" v8 (read-only), "AEGP Math Suite"
  v1, "AEGP Compute Cache" v1 and "AEGP Hash Suite" v1. The angle convention
  of `AEGP_MatrixDecompose4` and the hash values have not been checked
  against After Effects.
- "AEGP Layer Suite" v5 and "AEGP Camera Suite" v2, for comps without a
  camera: none is found, and the default camera's distance is given. Shine's
  three 3D Light choices ask for them, and corrupted the heap while unwinding
  from the failed acquire.
- The input layer is a footage item and the hosted effect an effect on its
  layer: the Layer, Item and Effect suites report both, and the Effect Suite
  gives the identities the loaded plugins declare.
- The legacy utility callbacks blend, convolve, generate Gaussian kernels
  (approximate coefficients) and resolve `get_callback_addr`.
- `PF Effect Sequence Data Suite` v1. `PF_GetConstSequenceData` gives the
  sequence data handle the plugin returned from `PF_Cmd_SEQUENCE_SETUP`, or
  null before it. Effects built with the `after-effects` crate, such as
  RainyDay, read their sequence data through it whenever
  `in_data->sequence_data` is null, `PF_Cmd_PARAMS_SETUP` included.
- `PluginInstance::param_flags` reads the `PF_ParamFlag_*` bits a parameter
  declares, as the new `ParamFlags`. A host checks
  `ParamFlags::SUPERVISE` to tell when to send
  `PF_Cmd_USER_CHANGED_PARAM` (`PluginInstance::user_changed_param`).
- `AEGP Effect Suite` v5. It is v4 plus `AEGP_GetIsInternalEffect`, which
  takes an installed-effect key and is refused like the rest of the
  installed-effect registry. Falloff needs v5 to free its effect handle in
  `PF_Cmd_SMART_PRE_RENDER`.
- "AEGP Render Suite" at wire version 5 (`AEGP_RenderSuite4`). It serves what
  wire version 8 does; its `AEGP_RenderAndCheckoutLayerFrame` checks out the
  effect's input and declines `render_plain_layer_frameB = true`. AutoFill2
  acquires it in `PF_Cmd_SMART_PRE_RENDER` and crashed when the acquire
  failed.

### Changed

- `aexlo compat` counts a `render:legacy@*` scenario as `n/a`, not `error`,
  when the plugin declares smart render and returns a `PF_Err` from
  `PF_Cmd_RENDER`. After Effects never sends that command to a smart-render
  plugin, and Furikake refuses it outright.

### Fixed

- Every instance shares one `in_data->pica_basicP`, which lives as long as
  the process, as in After Effects. Before, each instance had its own,
  freed with it, so a plugin such as AutoFill2 that keeps the first one in a
  static called through freed memory once that instance was dropped.
- `PF_GetCurrentState` covers arbitrary data as the plugin flattens it
  (`PF_Arbitrary_FLAT_SIZE_FUNC` and `PF_Arbitrary_FLATTEN_FUNC`), so a change
  in what a value points to is no longer called unchanged. The host flattens
  between commands: after setup, and after `PF_Cmd_USER_CHANGED_PARAM`,
  `PF_Cmd_UPDATE_PARAMS_UI`, `PF_Cmd_EVENT` and `PF_Cmd_DO_DIALOG`; a value the
  plugin cannot flatten is covered by its handle's bytes as before. For an
  `AUTOMATIC_WIDE_TIME_INPUT` effect the range widens by how far its checkouts
  have reached before and after their time, and takes in every time one
  reached, instead of covering the whole track once any checkout is seen.
- `PF_GetCurrentState` changes when what it covers does, so that
  `PF_AreStatesIdentical` no longer calls a changed input unchanged: a
  value over the time range asked about, the pixels of the input and of
  layer parameters, masks, text and shapes on the layer, and the bytes of
  arbitrary data. The `CHECK_ALL` indices mean what the SDK says.
  `PF_IsIdenticalCheckout` compares all the parameters those stand for.
- PF World Transform works at 16 and 32 bpc. `transfer_rect` and
  `transform_world` refused those worlds, which failed
  every render of Shine at those depths and DeepGlow2's smart render at
  32 bpc; `blend` and `convolve` read them as
  8-bit; `copy` and `copy_hq` succeeded after copying a half or a quarter of
  each row. Deep opacity is read from `opacitySu`, or from `opacity` when
  `opacitySu` is 0 as DeepGlow2 sends it, and 32 bpc keeps values
  out of range through copying, filtering, normal and additive compositing.
  At 8 bpc only `copy` differs: it stretches into a destination rectangle of
  another size and no longer reads past the source.
- The legacy `copy` honours its rectangles, and `fill` and the colourless
  `premultiply` cover the whole of a deep world. Discotext and DisplacerPro
  left the alpha of three quarters of a 32 bpc frame wrong.
- `PF_GetPixelFormat` knows the depth of a world that a plugin made over part
  of one of the host's.
- `PF_CHECKOUT_PARAM` answers the two indices past a plugin's declared
  parameters with empty parameters. Red Giant Universe effects probe them,
  and corrupted the heap on smart renders when they failed.
- `PF_InData::total_time` is the layer's duration (the project's, or 30
  seconds without one) instead of 0, which made effects that divide by it
  render with NaNs or crash.
- The pre-render delete callback now runs for GPU frames too, so their
  pre-render data no longer leaks.
- Teardown sends `PF_Cmd_SEQUENCE_SETDOWN` and `PF_Cmd_GLOBAL_SETDOWN` only
  after the matching setup succeeded.
- `PF_DisposeWorld` frees the allocation the host made, whatever the plugin
  wrote into the world since, and refuses pixels it did not allocate.
  Disposing a handle the host did not make is ignored instead of freed.
- New handles are zeroed.
- Pre-render data that is a host handle without a delete callback is freed
  instead of leaked.
- `PF_KernelFlag_USE_CHAR` kernels are read as unsigned bytes.
- A world with no pixels (a width or height of 0 or less) is refused with
  `PF_Err_BAD_CALLBACK_PARAM` and a null `data`, by `PF_UtilCallbacks`'
  `new_world`, `PF_WorldSuite2::PF_NewWorld` and
  `PF_PixelFormatSuite1::NewWorldOfPixelFormat`. Its `data` was the dangling
  pointer of an empty buffer. FillColor and AnimatedNoise ask for a
  `height / 4` x 8 world, so 0 x 8 on a layer under 4 pixels tall, and wrote
  through that pointer unless it was null: they crashed in
  `PF_Cmd_SMART_RENDER`.
- The stream value of a layer parameter (`AEGP_GetNewStreamValue`) names the
  layer when one is linked: without a project, the id `AEGP_GetLayerID` gives
  the effect's layer, and 0 only for "None". It was always 0, so Furikake, which looks
  a linked Z buffer or occlusion layer up by that id for its transform,
  failed its smart pre-render.
- `PF_Event_DRAG` hands the plugin back the `continue_refcon` that its click
  (or the drag before it) left, as After Effects does, instead of zeros.
  Curve editors that keep the point being dragged there, such as Falloff's
  and Furikake's, ignored every drag.
- `PF_Cmd_SMART_PRE_RENDER` hands the plugin a `GuidMixInPtr` callback in
  `PF_PreRenderCallbacks` instead of a null pointer. Plugins that mix state
  into the frame GUID without checking the pointer, such as Cosmic and
  Falloff, crashed there. aexlo keeps no frame cache, so the callback checks
  its arguments, records the call and returns success.

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
  resource fork; a `.rsrc` or `__pipl` section it cannot read is passed over
  for the next place. Each
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
