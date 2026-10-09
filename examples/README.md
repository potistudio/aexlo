# Examples

English | [日本語](README.ja.md)

Start with [Getting started with aexlo](../docs/getting-started.md). Run all commands from the repository root. The basic examples use the bundled SDK_Noise fixture and do not require After Effects.

| Example | What it demonstrates | Command |
| --- | --- | --- |
| [minimal](minimal/src/main.rs) | Load an existing plugin through Host and retrieve ABOUT | `cargo run -p minimal` |
| [sdk_noise](sdk_noise/src/main.rs) | PNG input → rendering → PNG output | `cargo run -p sdk_noise` |
| [interactive](interactive/README.md) | Select plugins and edit parameters in a GUI | `cargo run -p interactive --release` |
| [studio](studio/README.md) | Combine layers, effects, and time | `cargo run -p studio --release` |

minimal and sdk_noise accept `-- --help` to describe their arguments. sdk_noise writes to `target/sdk-noise.png` by default. Read these two examples first when integrating the API into your application.

[playground](../playground/README.md) verifies host behavior, while [demos](../demos/) contains plugin-side implementations. Start with these examples when building an application that hosts existing plugins.
