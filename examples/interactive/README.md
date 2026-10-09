# Interactive playground

A desktop application that loads existing After Effects plugins through aexlo and previews their output while you edit parameters. The UI uses eframe / egui. New to the repository? Try minimal and sdk_noise in the [getting started guide](../../docs/getting-started.md) first.

## Run

From the repository root:

```sh
cargo run -p interactive --release
# Select a bundled plugin by name, without its extension (not a file path).
cargo run -p interactive --release -- SDK_Noise
```

The application lists plugins in `fixtures/plugins/macos/` or `fixtures/plugins/windows/`. SDK_Noise is the default selection. If no plugins are found, the UI displays an error. Running the application requires a desktop environment with GUI and GPU access.

## First steps

1. Check that `Plugin` on the left is set to SDK_Noise.
2. Change `Noise variation` under `Parameters` and inspect the preview on the right.
3. Turn off `Auto-render (animate)` and click `Render once` to process individual frames.
4. Check the status for loading and rendering errors, FPS, image dimensions, and the smart or legacy render path.

The UI exposes supported parameter types, including numeric and boolean controls. It may not expose every parameter of an arbitrary plugin. The implementation starts in [src/main.rs](src/main.rs).

For multiple layers and file export, try [studio](../studio/README.md). To inspect host API responses, use the [verification playground](../../playground/README.md).
