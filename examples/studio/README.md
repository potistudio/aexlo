# aexlo studio

English | [日本語](README.ja.md)

New to the repository? Try minimal, sdk_noise, and interactive in the [getting started guide](../../docs/getting-started.md) first. See the [examples index](../README.md) for an overview.

A small compositor for exercising aexlo end to end. `examples/interactive`
tweaks one plugin's parameters; the studio puts real After Effects plugins
into a timeline with other layers, so the features that need a host around
them get exercised too: layer parameters, path parameters, time and
keyframes, adjustment layers, buttons, and hidden/disabled parameter UI.

```bash
cargo run -p studio --release              # opens a demo project
cargo run -p studio --release -- my.json   # opens a saved project
```

Use `--release`: debug builds composite several times slower.

## What's in it

- **Layers**: solids (a transparent one makes a "null" for generators),
  images, shape layers (rectangle, ellipse, star/polygon and pen paths, with
  fill and stroke), text (any TTF/OTF/TTC font, falling back to a system
  Japanese font), and adjustment layers whose effects process everything
  below them. Each has a transform, opacity, blend mode, and in/out points.
- **Masks**: drawn with the pen or shape tools on any layer, with Add,
  Subtract, Intersect, Lighten, Darken, Difference and None modes, inversion,
  opacity and feather. They cut the layer before its effects run, as in AE,
  and plugins see them through the Path suites: a `PF_Param_PATH` parameter
  offers the layer's masks.
- **Effects**: any bundled fixture or any plugin on disk, stacked per layer.
  Parameters are grouped as the plugin declares them, follow its
  `PF_UpdateParamUI` hiding/disabling, and buttons send
  `PF_Cmd_USER_CHANGED_PARAM`. Layer parameters pick any layer in the comp,
  rendered with its masks and effects, even when hidden; picking the
  effect's own layer gives its source. Point parameters get handles in the
  viewer.
- **Time**: every property and effect parameter has a stopwatch. Keyframes
  are Linear, Easy Ease or Hold, can be dragged in the timeline, and
  right-clicked to change. Effects see layer time (`frame - start`), so
  sliding a layer slides its effects' clocks and keyframes with it.
- **Viewer**: zoom (scroll or pinch), pan (Alt-drag or middle-drag), select
  and move layers, scale from corners, rotate from the knob, edit mask and
  path points and tangents (hold Alt mid-drag to move one tangent alone),
  drag shape items and effect points.
- **Files**: projects save as JSON; export the current frame as PNG, the
  comp as a PNG sequence, or as MP4 when `ffmpeg` is on `PATH`. Undo/redo
  covers every edit; a drag is one step.

## Keys

| Key | Action |
| --- | --- |
| Space | Play / pause |
| ← → (Shift: ×10), Home, End | Step / jump |
| J / K | Previous / next keyframe on the selected layer |
| V, G, Q | Selection, pen, shape tools (Q cycles) |
| Enter / Esc | End the current pen path |
| Delete | Delete the selected keyframe, mask, shape, effect, or layer |
| ⌘Z / ⇧⌘Z | Undo / redo |
| ⌘S / ⇧⌘S / ⌘O | Save / save as / open |
| ⌘D | Duplicate layer |
| ⌘] / ⌘[ | Move layer up / down |

## How it renders

`engine.rs` runs on its own thread and owns every `PluginInstance`; the UI
sends it snapshots of the project and only the newest pending one renders.
Per layer: source → masks → effects (each one `set_input_layer`,
`set_time`, parameters, `set_mask_paths`, `set_layer_param`,
`render_frame`) → transform and blend onto the comp with tiny-skia. Layer
sources whose contents and masks don't change are cached across frames.

For scripted checks, `AEXLO_STUDIO_SCREENSHOT=out.png` saves the window once
the first frame is shown and quits; `AEXLO_STUDIO_SELECT=<n>` selects the
n-th layer (0 = top) first.
