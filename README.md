# aexlo — After Effects Plugin Runtime

English | [日本語](https://github.com/potistudio/aexlo/blob/master/README.ja.md)

New to the repository? Start with the [getting started guide](https://github.com/potistudio/aexlo/blob/master/docs/getting-started.md), then explore the [examples](https://github.com/potistudio/aexlo/blob/master/examples/README.md), GUI, and [verification playground](https://github.com/potistudio/aexlo/blob/master/playground/README.md).

> Load, run, and render After Effects plugins (.aex) outside of After Effects.

## What

**aexlo** is a Rust crate that emulates the After Effects plugin runtime.
It loads and renders `.aex` plugins by re-implementing the AE Plugin SDK —
the same interface After Effects itself exposes to plugins.

By default, host-dependent features (such as UI and application callbacks)
are answered by a headless host, giving you full control over what to override
and how to integrate plugins into your own application: implement `AppHost`
and install it with `Host::install` before loading plugins.

## Why

**Adobe After Effects** is the most widely used video editor in the world,  
and there is a rich ecosystem of powerful plugins built for it.  
However, all of these plugins are designed to run exclusively within After Effects.

**aexlo** emulates the After Effects plugin runtime, allowing AE plugins to run outside of After Effects entirely — much like Wine runs Windows applications on Linux.

## Who This Is For

- **Plugin developers** who want to test their plugins without spinning up a full After Effects instance.
- **Software developers** who want to make their software compatible with After Effects plugins.
- **Artists** who want to integrate After Effects plugins into their image processing workflows.

## Capabilities

- Load and render After Effects plugins outside of After Effects, through
  their GPU, smart and legacy paths, in 8, 16 and 32 bits per channel.
- Put an effect on a text or shape layer, so effects that work from glyph
  and path data (Discotext, Extrude) can read it, and give it masks.
- Describe the project around an effect: comp, layers, transforms and
  layer time.
- Keyframe parameters, with one value at any instant whichever API the
  plugin asks.
- Observe the commands and suite calls, and what a plugin asked for that
  aexlo does not do.
- Selectively override or re-implement SDK commands, suites, and callbacks
  to fit your specific workflow.

## Status

> [!WARNING]
> This project is **currently under heavy development**.
> Rendering without an error is not a promise of the same pixels: much of the
> host has not been compared with After Effects.
> The latest progress of this project can be seen on the [master branch](https://github.com/potistudio/aexlo/tree/master).

## Quick Start

### Requirements

- Rust toolchain specified by `rust-toolchain.toml` (currently 1.100.0)

- Windows x64 or macOS arm64

> [!NOTE]
> After Effects plugins target Windows and macOS only, so Linux is not currently supported.

### Use the library

```bash
cargo add aexlo
```

```rust,no_run
use aexlo::{Depth8, Host, Layer, ParamValue, Time};

fn main() -> aexlo::Result<()> {
    // On Windows pass the `.aex` file, on macOS the `.plugin` bundle.
    let mut fx = Host::get().load("MyEffect.aex")?;
    println!("{}", fx.about()?);

    fx.set_input_layer(Layer::<Depth8>::black(1920, 1080));
    fx.set_param(1, ParamValue::Float(0.5))?;
    fx.set_time(Time::frame(0, 30));
    fx.render_frame()?;
    let output: Layer<Depth8> = fx.read_output()?;
    Ok(())
}
```

A plugin is native code that runs inside your process; see the crate docs on
[docs.rs](https://docs.rs/aexlo) for the trust, process and threading model.

### Layers, paths and keyframes

> [!NOTE]
> These APIs are newer than 0.2.0; see
> [Unreleased](https://github.com/potistudio/aexlo/blob/master/CHANGELOG.md).

Without a project, an effect is alone on a layer that fills its comp.
`set_text_layer` makes that a text layer and `set_mask_paths` gives it masks.
`set_project` describes a whole comp instead, which is how an effect gets the
paths of a shape layer:

```rust,no_run
use aexlo::{
    Comp, Depth8, Host, Keyframe, Layer, LayerContent, MaskPath, MaskVertex, ParamValue, PixelDepthKind, Project,
    ProjectLayer, ShapeFill, ShapeGroup, ShapeItem, ShapePath, Time, TimePoint, Track,
};

fn main() -> aexlo::Result<()> {
    let comp = Comp::new("Comp 1", 640, 360, TimePoint::whole_seconds(10));
    let triangle = MaskPath {
        vertices: vec![
            MaskVertex::corner(260.0, 60.0),
            MaskVertex::corner(370.0, 250.0),
            MaskVertex::corner(150.0, 250.0),
        ],
        closed: true,
        ..Default::default()
    };
    let group = ShapeGroup::new(
        "Triangle",
        vec![
            ShapeItem::Path(ShapePath::new("Path 1", triangle)),
            ShapeItem::Fill(ShapeFill::new([1.0; 4])),
        ],
    );
    let content = LayerContent::Shape(vec![ShapeItem::Group(group)]);
    let layer = ProjectLayer::new(1, "Shape Layer 1", content, &comp);
    // The pixels are the host's to give, as After Effects would draw them.
    let input = layer
        .rasterize(PixelDepthKind::U8, TimePoint::ZERO, [1.0; 4])
        .expect("a shape layer");

    let mut fx = Host::get().loader("Extrude.aex").input(input).load()?;
    fx.set_project(Some(Project::new(comp, vec![layer], 1)?));

    // Extrude's "Extrusion Length Back".
    let length = Track::keyed(vec![
        Keyframe::linear(TimePoint::frame(0, 30), ParamValue::Float(0.0)),
        Keyframe::linear(TimePoint::frame(30, 30), ParamValue::Float(200.0)),
    ])
    .expect("two different times");
    fx.set_param_track(27, length)?;

    fx.set_time(Time::frame(15, 30)); // 100
    fx.render_frame()?;
    let output: Layer<Depth8> = fx.read_output()?;
    Ok(())
}
```

The project is read-only to plugins, and with one `set_time` is the time of
the effect's layer: comp time is `start + time * stretch`. Keyframes are
linear or held. [AEGP](#aegp) lists the other limits.

### Build from the repository

```bash
cargo build
```

## Toolkit

`aexlo.toml` describes "render this plugin under these conditions" once, as
presets; the `aexlo` CLI then tests, benchmarks and previews them. The full
specification is [docs/toolkit.md](https://github.com/potistudio/aexlo/blob/master/docs/toolkit.md).

The CLI and its toolkit crates are not published on crates.io; install the CLI
from the repository:

```bash
cargo install --git https://github.com/potistudio/aexlo aexlo-cli
```

```toml
[plugin]
artifact = { macos = "build/MyGlow.plugin", windows = "build/MyGlow.aex" }

[defaults]
depth = [8, 16, 32]

[[preset]]
name   = "dot_glow"
input  = { generate = "dot", at = [320, 240], radius = 2 }
size   = [640, 480]
params = { Radius = 100.0, Mode = "Screen" }
```

```bash
aexlo presets                      # the variants the presets expand to
aexlo test --bless                 # write goldens; later `aexlo test` compares
aexlo test --strict                # + guard bands, leak and checkout tracking
aexlo test --fuzz 100              # random parameters, failures as presets
aexlo bench --save-baseline main   # later: aexlo bench --baseline main
aexlo preview build/MyGlow.plugin --preset dot_glow
aexlo check                        # what CI runs (.github/workflows/aexlo-check.yml)
```

Every variant runs in a worker process, so a crashing plugin fails one run,
not the suite. Rust plugins add `aexlo-test` as a dev-dependency for
in-process `#[aexlo_test::test]`s with frame assertions.

To see what a plugin does, drive it from a prompt in a terminal UI:

```bash
aexlo inspect build/MyGlow.plugin            # TUI; `:help` lists the commands
aexlo inspect MyGlow --exec "render; set Radius 50; render smart"
```

`input text`, `input shape`, `mask` and `key` give the effect a text layer, a
shape layer, masks and keyframes to work on:

```bash
aexlo inspect Extrude --exec "input shape ring; render smart; save ring.png"
aexlo inspect Discotext --exec "input text HELLO; render smart; save text.png"
```

It traces every command and the host suite functions the plugin calls back,
totals them per function, and shows the parameters, the `PF_InData` /
`PF_OutData` state, the handles and worlds the plugin holds as a hex dump
(highlighting what the last command changed) and the output frame. Focus a
trace line (Enter, or `:focus <text|#seq>`) for a diagnosis of it: the call's
arguments and result, the command it ran in, the memory it allocated or
names and whether that is still held, or, for a failed command, its error
and the last call before it. `--exec` prints the same analysis as text. Given a crate directory, `:reload` rebuilds
it and keeps the parameter values.

For agents and scripts, `--format json` prints one JSON object per line
instead: `loaded`, then a `command` per command (its result, the `PF_Cmd_*`
it issued with their errors, and the `#seq` that `focus` takes), a `focus`
with the diagnosis in sections, and a final `report`. The plugin's own output
goes to stderr, so stdout stays parseable. `--script -` reads the commands
from stdin and answers each before reading the next, so one loaded plugin
can be driven over a pipe; `report` prints the analysis on demand, `clear`
forgets the trace so far and `quit` ends the session.

```bash
aexlo inspect MyGlow --format json --exec "render; focus checkout"
aexlo inspect MyGlow --format json --script -    # one command per line on stdin
```

Every option, command and JSON field is described in
[docs/inspect.md](https://github.com/potistudio/aexlo/blob/master/docs/inspect.md).

To see how far aexlo can drive plugins you did not write, run them through
`aexlo compat`. It loads each one and runs it on every render path and depth,
with other inputs, sizes and times, and with each parameter set to other
values, one scenario per worker. It reports `ok`, `partial`, `n/a`, `error`,
`crash` or `timeout` per scenario, then lists the missing suites, stubs and
declined calls the plugins asked for, with the most widespread first. A saved
baseline shows which scenarios a change to aexlo made worse:

```bash
aexlo compat fixtures/plugins/macos --save-baseline compat.json
aexlo compat fixtures/plugins/macos --baseline compat.json   # exit 1 if any got worse
```

See [docs/compat.md](https://github.com/potistudio/aexlo/blob/master/docs/compat.md).

## Implementation Progress

⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣶⣀⣀⣀ 78% (29/37)

> ○ fully implemented / △ only partially implemented / × not implemented

| CB Suites          | Effect Suites                    | Adv Effect Suites | Others            |
| ------------------ | -------------------------------- | ----------------- | ----------------- |
| ○ ANSI             | ○ AE App                         | ○ AE Adv App      | ○ Cache On Load   |
| △ Batch Sampling   | ○ AngleParam                     | ○ AE Adv Item     | △ Channel         |
| ○ Color            | ○ ColorParam                     | ○ AE Adv Time     | ○ GPU Device      |
| ○ Color16          | △ Effect Custom UI Overlay Theme |                   | ○ Plugin Helper   |
| ○ ColorFloat       | △ Effect Custom UI               |                   | ○ Plugin Helper 2 |
| ○ Fill Matte       | ○ Effect UI                      |                   | × PF Utility      |
| ○ Handle           | △ Param Utils                    |                   |                   |
| ○ Iterate8         | ○ Path Data                      |                   |                   |
| ○ Iterate16        | △ Path Query                     |                   |                   |
| ○ IterateFloat     | ○ PointParam                     |                   |                   |
| ○ Pixel Data       | × Effect Sequence Data           |                   |                   |
| ○ Pixel Format     |                                  |                   |                   |
| ○ Sampling8        |                                  |                   |                   |
| ○ Sampling16       |                                  |                   |                   |
| ○ SamplingFloat    |                                  |                   |                   |
| ○ World            |                                  |                   |                   |
| ○ World Transform  |                                  |                   |                   |

△ Effect Custom UI / Overlay Theme: theme values are served, and Drawbot draws
during `PF_Event_DRAW`. The CLI never sends `PF_Cmd_EVENT`; an embedder does.
△ Batch Sampling: no batch functions are provided (the SDK withdrew them in
After Effects 7.0); the per-pixel samplers work.
△ Param Utils: states follow values and keyframes over the whole timeline,
not the time range asked about or the pixels of layer parameters.
△ Path Query: masks are read-only and the same at every time.
× PF Utility: not to be confused with AEGP Utility. The suite can be acquired,
but each of its functions is a stub that returns an error.
△ Channel: layers are plain 2D images and report no auxiliary channels.
Callbacks that are not implemented return an error rather than a false success.
Legacy Gaussian-kernel generation uses approximate coefficients that have not been checked against After Effects. Legacy blend and convolution currently accept 8-bit worlds only.

### AEGP

⣿⣿⣶⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀ 14% (5/36)

5 fully and 21 partially implemented, 10 not implemented.

| Project        | Streams           | Pixels                 | Other                  |
| -------------- | ----------------- | ---------------------- | ---------------------- |
| △ Layer        | △ Stream          | ○ World                | △ Color Settings       |
| △ Comp         | △ Dynamic Stream  | ○ Memory               | △ Register             |
| △ Item         | △ Keyframe        | ○ Iterate              | △ Utility              |
| △ Light        | ○ Persistent Data | △ Render               | × Camera               |
| △ Effect       | × Mask            | △ Render Options       | × QueryXform           |
| △ PF Interface | △ Mask Outline    | △ Layer Render Options | △ Math                 |
| △ Proj         | △ Text Document   | × Composite            | △ Compute Cache        |
| × Footage      | ○ Text Layer      | × Canvas               | △ Hash                 |
| × Collection   |                   | × Sound Data           | × Render Async Manager |
| × Marker       |                   |                        |                        |

△ Layer / Comp / Item / Light / Effect / PF Interface: read-only, from the project given with `set_project`, or else one comp with the effect's layer and a synthetic footage item for its input.
The Effect Suite knows one effect, the hosted one, and the identities reported by loaded plugins; it does not enumerate the system's installed effects.
△ Stream / Dynamic Stream: transforms, effect parameters, source text and shape contents (paths, groups, fills and Merge Paths) at the time asked for.
Strokes, parametric shapes, layer masks and expressions are not streams. A plugin can hide or disable parameters, and set those without keyframes other than layers and arbitrary data; other writes are refused.
△ Keyframe: reads only, linear or held; no bezier eases or spatial tangents. A layer checked out at another time has the current pixels.
△ Mask Outline / Text Document: shape paths and source strings are read; nothing is written. Layer masks are read through Path Query.
△ Math: the angle convention of `AEGP_MatrixDecompose4` has not been checked against After Effects.
△ Proj: one unsaved synthetic project is exposed; project editing, persistence, root folders and time-display settings are not implemented.
△ Render / Render Options / Layer Render Options: checkout returns the effect's own input.
△ Color Settings: the working space is sRGB, with no colour management.
△ Register: idle hooks are recorded and never called.
△ Utility: there is no script engine (`AEGP_IsScriptingAvailable` says so and `AEGP_ExecuteScript` returns an error), no undo history and no palette UI. `AEGP_GetPluginPlatformRef` gives no bundle on macOS.
△ Compute Cache: entries persist until class unregistration or plugin teardown;
memory-pressure eviction is not implemented. △ Hash: deterministic host-local
hashes are provided, but their values are not guaranteed to match After Effects.

### Drawbot

⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿ 100% (6/6)

| ○ Draw | ○ Supplier | ○ Surface | ○ Path |
| ------ | ---------- | --------- | ------ |
| ○ Pen  | ○ Image    |           |        |

Text uses a system sans-serif. `DrawString` draws nothing when none is installed.

## License

This project is licensed under the [MIT License](https://github.com/potistudio/aexlo/blob/master/LICENSE).
