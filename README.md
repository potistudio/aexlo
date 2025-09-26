# After Effects Plugin (.aex) Adapter Project

> Adapter for rendering AE Effects **without Adobe After Effects**.

## Status

Work in progress. Currently, Re-implementation on Rust has just begun. If you want to see the current result, [the C++ implementation is available here](https://github.com/potistudio/aexlo.js/releases/tag/v0.1a).

## Warnings

- is tested only on my environment (win_x64 & macOS_arm64).
- has no GUI, no CLI, and no Tests.

## Behavior

By default, some features that require interaction with client software are not implemented.
However, the minimum necessary implementation is required for core functions that provide functionality to plugins.

All functions, commands, & suites are redefinable.
These behavior can be implemented by yourself.

## Implemented Commands

⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀ 0% (0/29)

| Core               | Extra                         | Audio           |
| ------------------ | ----------------------------- | --------------- |
| □ About            | □ Completely General          | □ Audio Render  |
| □ Do Dialog        | □ Event                       | □ Audio Setdown |
| □ Frame Setdown    | □ Get External Dependencies   | □ Audio Setup   |
| □ Frame Setup      | □ Get Flattened Sequence Data |                 |
| □ Global Setdown   | □ GPU Device Setup            |                 |
| □ Global Setup     | □ GPU Device Setdown          |                 |
| □ Params Setup     | □ Num Cmds                    |                 |
| □ Render           | □ Query Dynamic Flags         |                 |
| □ Sequence Flatten | □ Smart Pre Render            |                 |
| □ Sequence Setdown | □ Smart Render                |                 |
| □ Sequence Setup   | □ Smart Render GPU            |                 |
| □ Sequence Resetup | □ Translate Params to Prefs   |                 |
|                    | □ User Changed Param          |                 |
|                    | □ Update Params UI            |                 |

## Implemented Suites

⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀ 0% (0/93)

### PF Suites

⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀ 0% (0/35)

| CB Suite          | Effect Suite                     | Adv Effect Suite | Others            |
| ----------------- | -------------------------------- | ---------------- | ----------------- |
| □ ANSI            | □ AE App                         | □ AE Adv App     | □ Cache On Load   |
| □ Batch Sampling  | □ AngleParam                     | □ AE Adv Item    | □ Channel         |
| □ Color           | □ ColorParam                     | □ AE Adv Time    | □ GPU Device      |
| □ Color16         | □ Effect Custom UI Overlay Theme |                  | □ Plugin Helper   |
| □ ColorFloat      | □ Effect Custom UI               |                  | □ Plugin Helper 2 |
| □ Fill Matte      | □ Effect UI                      |                  |                   |
| □ Handle          | □ Param Utils                    |                  |                   |
| □ Iterate8        | □ Path Data                      |                  |                   |
| □ iterate16       | □ Path Query                     |                  |                   |
| □ iterateFloat    | □ PointParam                     |                  |                   |
| □ Pixel Data      |                                  |                  |                   |
| □ Pixel Format    |                                  |                  |                   |
| □ Sampling8       |                                  |                  |                   |
| □ Sampling16      |                                  |                  |                   |
| □ SamplingFloat   |                                  |                  |                   |
| □ World           |                                  |                  |                   |
| □ World Transform |                                  |                  |                   |

### AEGP Suites

⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀ 0% (0/51)

| General                | Others          |
| ---------------------- | --------------- |
| □ Artisan Util         | □ Compute Cache |
| □ Canvas               | □ Duck          |
| □ Camera               | □ Hash          |
| □ Collection           | □ Mask          |
| □ Color Settings       |                 |
| □ Command              |                 |
| □ Comp                 |                 |
| □ Composite            |                 |
| □ Dynamic Stream       |                 |
| □ Effect               |                 |
| □ Effect Sequence Data |                 |
| □ File Import Manager  |                 |
| □ Footage              |                 |
| □ Layer                |                 |
| □ Layer Mask           |                 |
| □ Layer Render Options |                 |
| □ Light                |                 |
| □ IO In                |                 |
| □ IO Out               |                 |
| □ Item                 |                 |
| □ Item View            |                 |
| □ Iterate              |                 |
| □ Keyframe             |                 |
| □ Marker               |                 |
| □ Mask Outline         |                 |
| □ Math                 |                 |
| □ Memory               |                 |
| □ Output Module        |                 |
| □ Persistent Data      |                 |
| □ PF Interface         |                 |
| □ Proj                 |                 |
| □ QueryXform           |                 |
| □ Register             |                 |
| □ Render               |                 |
| □ Render Async Manager |                 |
| □ Render Options       |                 |
| □ Render Queue         |                 |
| □ Render Queue Item    |                 |
| □ RenderQueue Monitor  |                 |
| □ Sound Data           |                 |
| □ Stream               |                 |
| □ Text Document        |                 |
| □ Text Layer           |                 |
| □ Tracker              |                 |
| □ Tracker Utility      |                 |
| □ Utility              |                 |
| □ World                |                 |

### Drawbot

⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀ 0% (0/7)

| Drawbot    |
| ---------- |
| □ Draw     |
| □ Image    |
| □ Path     |
| □ Pen      |
| □ Supplier |
| □ Surface  |

## License

[MIT License](LICENSE)
