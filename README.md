# aexlo — After Effects Plugin Runtime

English | [日本語](README.ja.md)

New to the repository? Start with the [getting started guide](docs/getting-started.md), then explore the [examples](examples/README.md), GUI, and [verification playground](playground/README.md).

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

- Load and render After Effects plugins outside of After Effects.
- Selectively override or re-implement SDK commands, suites, and callbacks
  to fit your specific workflow.

## Status

> [!WARNING]
> This project is **currently under heavy development**.
> The latest progress of this project can be seen on the [master branch](https://github.com/potistudio/aexlo-rs/tree/master).

## Quick Start

### Requirements

- Rust toolchain specified by `rust-toolchain.toml` (currently 1.99.0)

- Windows x64 or macOS arm64

> [!NOTE]
> After Effects plugins target Windows and macOS only, so Linux is not currently supported.

### Build

```bash
cargo build
```

## Toolkit

`aexlo.toml` describes "render this plugin under these conditions" once, as
presets; the `aexlo` CLI (`cargo install --path crates/aexlo-cli`) then tests,
benchmarks and previews them. The full specification is
[docs/toolkit.md](docs/toolkit.md).

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
in-process `#[aexlo::test]`s with frame assertions.

## Implementation Progress

⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣶⣀ 89% (32/36)

> ○ fully implemented / △ only partially implemented / × not implemented

| CB Suites          | Effect Suites                    | Adv Effect Suites | Others            |
| ------------------ | -------------------------------- | ----------------- | ----------------- |
| ○ ANSI             | ○ AE App                         | ○ AE Adv App      | ○ Cache On Load   |
| ○ Batch Sampling   | ○ AngleParam                     | ○ AE Adv Item     | △ Channel         |
| ○ Color            | ○ ColorParam                     | ○ AE Adv Time     | ○ GPU Device      |
| ○ Color16          | △ Effect Custom UI Overlay Theme |                   | ○ Plugin Helper   |
| ○ ColorFloat       | △ Effect Custom UI               |                   | ○ Plugin Helper 2 |
| ○ Fill Matte       | ○ Effect UI                      |                   |                   |
| ○ Handle           | ○ Param Utils                    |                   |                   |
| ○ Iterate8         | ○ Path Data                      |                   |                   |
| ○ Iterate16        | ○ Path Query                     |                   |                   |
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
△ Channel: layers are plain 2D images and report no auxiliary channels.

### AEGP

⣿⣶⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀ 11% (4/36)

| Project        | Streams           | Pixels                 | Other                  |
| -------------- | ----------------- | ---------------------- | ---------------------- |
| △ Layer        | △ Stream          | ○ World                | △ Color Settings       |
| △ Comp         | △ Dynamic Stream  | ○ Memory               | △ Register             |
| △ Item         | △ Keyframe        | ○ Iterate              | △ Utility              |
| △ Light        | ○ Persistent Data | △ Render               | × Camera               |
| △ Effect       | × Mask            | △ Render Options       | × QueryXform           |
| △ PF Interface | × Mask Outline    | △ Layer Render Options | × Math                 |
| × Proj         | × Text Document   | × Composite            | × Compute Cache        |
| × Footage      | × Text Layer      | × Canvas               | × Hash                 |
| × Collection   |                   | × Sound Data           | × Render Async Manager |
| × Marker       |                   |                        |                        |

△ Layer / Comp / Item / Light / Effect / PF Interface / Stream / Dynamic Stream / Keyframe: the project is aexlo's fixed one-layer comp, with no keyframes.
△ Render / Render Options / Layer Render Options: checkout returns the effect's own input.
△ Color Settings: the working space is sRGB, with no colour management.
△ Register: idle hooks are recorded and never called.
△ Utility: `AEGP_ExecuteScript` does not run the script.

### Drawbot

⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿ 100% (6/6)

| ○ Draw | ○ Supplier | ○ Surface | ○ Path |
| ------ | ---------- | --------- | ------ |
| ○ Pen  | ○ Image    |           |        |

Text uses a system sans-serif. `DrawString` draws nothing when none is installed.

## License

This project is licensed under the [MIT License](LICENSE).
