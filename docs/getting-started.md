# Getting started with aexlo

aexlo is a Rust host that loads existing After Effects plugins and processes images outside After Effects. Start with the bundled SDK_Noise fixture, then try your own plugin. Run the commands below from the repository root.

## 1. Load a plugin

- Install Rust and Cargo through rustup. The repository selects its toolchain in `rust-toolchain.toml` (currently 1.99.0).
- Use a plugin built for your platform. The primary targets are macOS ARM64 and Windows x64; Linux is not supported.
- The bundled examples do not require After Effects. Comparing host behavior against Adobe's host does require After Effects.

```sh
rustup show
cargo run -p minimal
```

The first run downloads dependencies and builds the example. A successful run prints `Loading ...SDK_Noise...`, followed by the plugin's ABOUT message.

## 2. Render an image

```sh
cargo run -p sdk_noise
```

This reads the bundled `input.png` and writes `target/sdk-noise.png`. The final `Saved ...` message includes the output dimensions. Read the [source](../examples/sdk_noise/src/main.rs) in order: load the plugin, supply an input layer, render, then save the PNG.

To use your own plugin and image, separate Cargo's arguments from the application's arguments with `--`. Quote paths containing spaces.

```sh
# macOS example; use an .aex path on Windows.
cargo run -p minimal -- "/path/to/MyEffect.plugin"
cargo run -p sdk_noise -- "/path/to/MyEffect.plugin" "input.png" "target/my-effect.png"
cargo run -p sdk_noise -- --help
```

Explicit relative paths resolve from the current working directory. Default fixture paths resolve from the repository regardless of the working directory. Third-party plugins may require additional libraries or licenses.

## 3. Explore parameters in the GUI

```sh
cargo run -p interactive --release
```

Select a bundled plugin using `Plugin` on the left. Change values under `Parameters` and observe the preview on the right. Turn off `Auto-render (animate)` and click `Render once` to inspect one frame at a time. Check the status for errors and the selected render path. See the [interactive guide](../examples/interactive/README.md) for details.

For multiple layers, run `cargo run -p studio --release` to open the compositor. The [studio guide](../examples/studio/README.md) covers saving projects and exporting frames.

## 4. Inspect host behavior with playground

```sh
cargo run -p playground -- run --in-process
cargo run -p playground -- report target/probe/trace-aexlo.jsonl
```

The playground probe measures how the host responds to API calls. It writes a trace to `target/probe/trace-aexlo.jsonl` and an image to `target/probe/preview-aexlo.png`. In the report, `unavailable` identifies suites the probe could not acquire. Read the reported facts and suite results; the exit code alone does not describe API coverage.

To include dynamic library loading, use `cargo run -p playground -- run`. The [playground guide](../playground/README.md) explains comparison with After Effects.

## 5. Repeat checks with presets

```sh
cargo run -p aexlo-cli -- presets --manifest fixtures/aexlo.toml
cargo run -p aexlo-cli -- test --manifest fixtures/aexlo.toml
```

`fixtures/aexlo.toml` defines inputs, dimensions, bit depths, parameters, and checks. The `passthrough` preset disables noise and compares results with the available reference images (goldens). The `noisy` preset uses random noise, so it disables golden comparisons and some checks, including determinism. Read the manifest comments for the reasons behind these choices.

`--bless` updates reference images. Use existing references for your first run, and update them only after confirming that the expected output has changed. See [toolkit](toolkit.md) for manifest configuration and development loops.

## Repository map

| Directory | Purpose | When to explore it |
| --- | --- | --- |
| `examples/` | Applications using the host | Start here: minimal → sdk_noise → interactive |
| `fixtures/` | Existing plugins and verification settings | Inspect the examples' inputs |
| `crates/aexlo/` | Host implementation and public API | Integrate the host into your application |
| `crates/cli/` | Verification and preview CLI | Repeat runs under consistent conditions |
| `demos/` | Demo plugins built from source | Explore plugin-side code |
| `playground/` | Host API probe and runner | Investigate compatibility and unsupported APIs |

## Troubleshooting

- **Plugin not found:** Check the path and platform-specific extension. On macOS, pass the entire `.plugin` bundle; on Windows, pass the `.aex` file. If a fixture is a Git LFS pointer, run `git lfs pull` to fetch its contents.
- **Build fails:** Check the selected toolchain with `rustup show` and read the first compiler error. Building demo plugins also requires the platform's linker.
- **Image cannot be opened:** Check the input path and use PNG. The image decoder in this example enables only PNG support.
- **Plugin loads but rendering fails:** Try the same workflow with SDK_Noise first, then inspect the target plugin's error. The host may not provide every feature the plugin requires.
- **GUI does not start:** Use a desktop environment with GPU access. minimal, sdk_noise, and the CLI can run from a terminal.
