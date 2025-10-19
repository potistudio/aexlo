# Rust Interactive Demo

A real-time interactive demo application built with Rust, featuring:
- **winit** for cross-platform windowing
- **imgui-rs** for immediate-mode GUI
- **wgpu** for GPU-accelerated rendering
- Real-time background effects rendered from ARGB pixel arrays

## Features

- GPU-accelerated rendering of pixel buffer backgrounds
- Interactive ImGui overlay with real-time controls
- Animated plasma wave effect (demo)
- Easy to extend with your own effects

## Building

```bash
cargo build --release
```

## Running

```bash
cargo run --release
```

## Controls

The demo includes an interactive control panel where you can adjust:
- **Frequency**: Controls the wave pattern density
- **Speed**: Animation speed
- **Color Shift**: Color cycling speed

## Architecture

- `main.rs`: Application entry point and event loop
- `renderer.rs`: GPU rendering pipeline and background effect generation
- `ui.rs`: ImGui interface and state management
- `shader.wgsl`: WGSL shader for rendering the texture

## Extending

To add your own effects, modify the `update()` method in `renderer.rs`. The pixel buffer is a simple `Vec<u32>` in ARGB format that gets uploaded to the GPU each frame.

```rust
// In renderer.rs update() method
for y in 0..self.texture_height {
    for x in 0..self.texture_width {
        let idx = (y * self.texture_width + x) as usize;
        self.pixel_buffer[idx] = your_argb_color;
    }
}
```

## License

MIT
