mod renderer;
mod ui;

use std::time::Instant;
use winit::{
	event::{Event, WindowEvent},
	event_loop::{ControlFlow, EventLoop},
	window::WindowBuilder,
};

fn main() {
	env_logger::init();

	let event_loop = EventLoop::new();
	let window = WindowBuilder::new()
		.with_title("Rust Interactive Demo")
		.with_inner_size(winit::dpi::LogicalSize::new(1280, 720))
		.build(&event_loop)
		.unwrap();

	// Initialize renderer and UI
	let mut renderer = pollster::block_on(renderer::Renderer::new(&window));
	let mut ui_state = ui::UiState::new(&window, &renderer);

	let mut last_frame = Instant::now();
	let start_time = Instant::now();

	event_loop.run(move |event, _, control_flow| {
		*control_flow = ControlFlow::Poll;

		match event {
			Event::WindowEvent {
				event: WindowEvent::CloseRequested,
				..
			} => {
				*control_flow = ControlFlow::Exit;
			}
			Event::WindowEvent {
				event: WindowEvent::Resized(new_size),
				..
			} => {
				renderer.resize(new_size.width, new_size.height);
			}
			Event::RedrawRequested(_) => {
				let now = Instant::now();
				let delta_time = now.duration_since(last_frame).as_secs_f32();
				let elapsed = now.duration_since(start_time).as_secs_f32();
				last_frame = now;

				// Update background effect
				renderer.update(elapsed, &ui_state.params);

				// Render
				match renderer.render(&mut ui_state, delta_time) {
					Ok(_) => {}
					Err(wgpu::SurfaceError::Lost) => {
						renderer.resize(renderer.width, renderer.height)
					}
					Err(wgpu::SurfaceError::OutOfMemory) => *control_flow = ControlFlow::Exit,
					Err(e) => eprintln!("Render error: {:?}", e),
				}
			}
			Event::MainEventsCleared => {
				window.request_redraw();
			}
			_ => {
				ui_state.handle_event(&window, &event);
			}
		}
	});
}
