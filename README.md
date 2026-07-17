# aexlo — After Effects Plugin Runtime

> Load, run, and render After Effects plugins (.aex) outside of After Effects.

## What

**aexlo** is a Rust crate that emulates the After Effects plugin runtime.
It loads and renders `.aex` plugins by re-implementing the AE Plugin SDK —
the same interface After Effects itself exposes to plugins.

By default, host-dependent features (such as UI and application callbacks)
are left unimplemented, giving you full control over what to override
and how to integrate plugins into your own application.

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

- Rust 1.80.0 or higher

> [!NOTE]
> This crate requires **Rust Nightly** due to its use of the C variadic arguments feature.

- Windows x64 or macOS arm64

> [!NOTE]
> After Effects plugins target Windows and macOS only, so Linux is not currently supported.

### Build

```bash
cargo build
```

## Implementation Progress

⣿⣿⣶⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀ 14% (5/35)

> ○ fully implemented / △ only partially implemented / × not implemented

| CB Suites          | Effect Suites                    | Adv Effect Suites | Others            |
| ------------------ | -------------------------------- | ----------------- | ----------------- |
| ○ ANSI             | △ AE App                         | × AE Adv App      | × Cache On Load   |
| × Batch Sampling   | △ AngleParam                     | × AE Adv Item     | × Channel         |
| × Color            | × ColorParam                     | × AE Adv Time     | △ GPU Device      |
| × Color16          | × Effect Custom UI Overlay Theme |                   | × Plugin Helper   |
| × ColorFloat       | × Effect Custom UI               |                   | × Plugin Helper 2 |
| × Fill Matte       | × Effect UI                      |                   |                   |
| ○ Handle           | ○ Param Utils                    |                   |                   |
| △ Iterate8         | × Path Data                      |                   |                   |
| × Iterate16        | × Path Query                     |                   |                   |
| × IterateFloat     | × PointParam                     |                   |                   |
| × Pixel Data       |                                  |                   |                   |
| × Pixel Format     |                                  |                   |                   |
| × Sampling8        |                                  |                   |                   |
| × Sampling16       |                                  |                   |                   |
| × SamplingFloat    |                                  |                   |                   |
| ○ World            |                                  |                   |                   |
| ○ World Transform  |                                  |                   |                   |

## License

This project is licensed under the [MIT License](LICENSE).
