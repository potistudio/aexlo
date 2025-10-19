use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct Vertex {
	position: [f32; 3],
	tex_coords: [f32; 2],
}

impl Vertex {
	const ATTRIBS: [wgpu::VertexAttribute; 2] =
		wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2];

	fn desc() -> wgpu::VertexBufferLayout<'static> {
		wgpu::VertexBufferLayout {
			array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
			step_mode: wgpu::VertexStepMode::Vertex,
			attributes: &Self::ATTRIBS,
		}
	}
}

const VERTICES: &[Vertex] = &[
	Vertex {
		position: [-1.0, -1.0, 0.0],
		tex_coords: [0.0, 1.0],
	},
	Vertex {
		position: [1.0, -1.0, 0.0],
		tex_coords: [1.0, 1.0],
	},
	Vertex {
		position: [1.0, 1.0, 0.0],
		tex_coords: [1.0, 0.0],
	},
	Vertex {
		position: [-1.0, 1.0, 0.0],
		tex_coords: [0.0, 0.0],
	},
];

const INDICES: &[u16] = &[0, 1, 2, 0, 2, 3];

pub struct Renderer {
	pub surface: wgpu::Surface,
	pub device: wgpu::Device,
	pub queue: wgpu::Queue,
	pub config: wgpu::SurfaceConfiguration,
	pub width: u32,
	pub height: u32,

	render_pipeline: wgpu::RenderPipeline,
	vertex_buffer: wgpu::Buffer,
	index_buffer: wgpu::Buffer,
	texture: wgpu::Texture,
	texture_bind_group: wgpu::BindGroup,

	pixel_buffer: Vec<u32>,
	texture_width: u32,
	texture_height: u32,

	pub imgui_renderer: imgui_wgpu::Renderer,
}

impl Renderer {
	pub async fn new(window: &winit::window::Window) -> Self {
		let size = window.inner_size();
		let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
			backends: wgpu::Backends::all(),
			..Default::default()
		});

		let surface = unsafe { instance.create_surface(window) }.unwrap();

		let adapter = instance
			.request_adapter(&wgpu::RequestAdapterOptions {
				power_preference: wgpu::PowerPreference::default(),
				compatible_surface: Some(&surface),
				force_fallback_adapter: false,
			})
			.await
			.unwrap();

		let (device, queue) = adapter
			.request_device(
				&wgpu::DeviceDescriptor {
					label: None,
					features: wgpu::Features::empty(),
					limits: wgpu::Limits::default(),
				},
				None,
			)
			.await
			.unwrap();

		let surface_caps = surface.get_capabilities(&adapter);
		let surface_format = surface_caps
			.formats
			.iter()
			.find(|f| f.is_srgb())
			.copied()
			.unwrap_or(surface_caps.formats[0]);

		let config = wgpu::SurfaceConfiguration {
			usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
			format: surface_format,
			width: size.width,
			height: size.height,
			present_mode: wgpu::PresentMode::Fifo,
			alpha_mode: surface_caps.alpha_modes[0],
			view_formats: vec![],
		};
		surface.configure(&device, &config);

		// Create texture for background
		let texture_width = 800;
		let texture_height = 600;
		let pixel_buffer = vec![0u32; (texture_width * texture_height) as usize];

		let texture_size = wgpu::Extent3d {
			width: texture_width,
			height: texture_height,
			depth_or_array_layers: 1,
		};

		let texture = device.create_texture(&wgpu::TextureDescriptor {
			label: Some("Background Texture"),
			size: texture_size,
			mip_level_count: 1,
			sample_count: 1,
			dimension: wgpu::TextureDimension::D2,
			format: wgpu::TextureFormat::Rgba8UnormSrgb,
			usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
			view_formats: &[],
		});

		let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
		let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
			address_mode_u: wgpu::AddressMode::ClampToEdge,
			address_mode_v: wgpu::AddressMode::ClampToEdge,
			address_mode_w: wgpu::AddressMode::ClampToEdge,
			mag_filter: wgpu::FilterMode::Linear,
			min_filter: wgpu::FilterMode::Linear,
			mipmap_filter: wgpu::FilterMode::Nearest,
			..Default::default()
		});

		let texture_bind_group_layout =
			device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
				entries: &[
					wgpu::BindGroupLayoutEntry {
						binding: 0,
						visibility: wgpu::ShaderStages::FRAGMENT,
						ty: wgpu::BindingType::Texture {
							multisampled: false,
							view_dimension: wgpu::TextureViewDimension::D2,
							sample_type: wgpu::TextureSampleType::Float { filterable: true },
						},
						count: None,
					},
					wgpu::BindGroupLayoutEntry {
						binding: 1,
						visibility: wgpu::ShaderStages::FRAGMENT,
						ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
						count: None,
					},
				],
				label: Some("texture_bind_group_layout"),
			});

		let texture_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
			layout: &texture_bind_group_layout,
			entries: &[
				wgpu::BindGroupEntry {
					binding: 0,
					resource: wgpu::BindingResource::TextureView(&texture_view),
				},
				wgpu::BindGroupEntry {
					binding: 1,
					resource: wgpu::BindingResource::Sampler(&sampler),
				},
			],
			label: Some("texture_bind_group"),
		});

		let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
			label: Some("Shader"),
			source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
		});

		let render_pipeline_layout =
			device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
				label: Some("Render Pipeline Layout"),
				bind_group_layouts: &[&texture_bind_group_layout],
				push_constant_ranges: &[],
			});

		let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
			label: Some("Render Pipeline"),
			layout: Some(&render_pipeline_layout),
			vertex: wgpu::VertexState {
				module: &shader,
				entry_point: "vs_main",
				buffers: &[Vertex::desc()],
			},
			fragment: Some(wgpu::FragmentState {
				module: &shader,
				entry_point: "fs_main",
				targets: &[Some(wgpu::ColorTargetState {
					format: config.format,
					blend: Some(wgpu::BlendState::REPLACE),
					write_mask: wgpu::ColorWrites::ALL,
				})],
			}),
			primitive: wgpu::PrimitiveState {
				topology: wgpu::PrimitiveTopology::TriangleList,
				strip_index_format: None,
				front_face: wgpu::FrontFace::Ccw,
				cull_mode: Some(wgpu::Face::Back),
				polygon_mode: wgpu::PolygonMode::Fill,
				unclipped_depth: false,
				conservative: false,
			},
			depth_stencil: None,
			multisample: wgpu::MultisampleState {
				count: 1,
				mask: !0,
				alpha_to_coverage_enabled: false,
			},
			multiview: None,
		});

		let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
			label: Some("Vertex Buffer"),
			contents: bytemuck::cast_slice(VERTICES),
			usage: wgpu::BufferUsages::VERTEX,
		});

		let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
			label: Some("Index Buffer"),
			contents: bytemuck::cast_slice(INDICES),
			usage: wgpu::BufferUsages::INDEX,
		});

		// Initialize ImGui renderer
		let mut imgui_context = imgui::Context::create();
		imgui_context.set_ini_filename(None);

		let mut imgui_renderer = imgui_wgpu::Renderer::new(
			&mut imgui_context,
			&device,
			&queue,
			imgui_wgpu::RendererConfig {
				texture_format: surface_format,
				..Default::default()
			},
		);

		imgui_renderer.reload_font_texture(&mut imgui_context, &device, &queue);

		Self {
			surface,
			device,
			queue,
			config,
			width: size.width,
			height: size.height,
			render_pipeline,
			vertex_buffer,
			index_buffer,
			texture,
			texture_bind_group,
			pixel_buffer,
			texture_width,
			texture_height,
			imgui_renderer,
		}
	}

	pub fn resize(&mut self, width: u32, height: u32) {
		if width > 0 && height > 0 {
			self.width = width;
			self.height = height;
			self.config.width = width;
			self.config.height = height;
			self.surface.configure(&self.device, &self.config);
		}
	}

	pub fn update(&mut self, time: f32, params: &crate::ui::EffectParams) {
		// Generate plasma effect
		for y in 0..self.texture_height {
			for x in 0..self.texture_width {
				let fx = x as f32 / self.texture_width as f32;
				let fy = y as f32 / self.texture_height as f32;

				let value1 = (fx * params.frequency + time * params.speed).sin();
				let value2 = (fy * params.frequency + time * params.speed).cos();
				let value3 = ((fx + fy) * params.frequency * 0.5 + time * params.speed).sin();

				let combined = (value1 + value2 + value3) / 3.0;

				let r = ((combined * params.color_shift).sin() * 0.5 + 0.5) * 255.0;
				let g = ((combined * params.color_shift + 2.0).sin() * 0.5 + 0.5) * 255.0;
				let b = ((combined * params.color_shift + 4.0).sin() * 0.5 + 0.5) * 255.0;

				let idx = (y * self.texture_width + x) as usize;
				self.pixel_buffer[idx] =
					0xFF000000 | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);
			}
		}

		// Upload to GPU
		let rgba_buffer: Vec<u8> = self
			.pixel_buffer
			.iter()
			.flat_map(|&argb| {
				let a = ((argb >> 24) & 0xFF) as u8;
				let r = ((argb >> 16) & 0xFF) as u8;
				let g = ((argb >> 8) & 0xFF) as u8;
				let b = (argb & 0xFF) as u8;
				[r, g, b, a]
			})
			.collect();

		self.queue.write_texture(
			wgpu::ImageCopyTexture {
				texture: &self.texture,
				mip_level: 0,
				origin: wgpu::Origin3d::ZERO,
				aspect: wgpu::TextureAspect::All,
			},
			&rgba_buffer,
			wgpu::ImageDataLayout {
				offset: 0,
				bytes_per_row: Some(4 * self.texture_width),
				rows_per_image: Some(self.texture_height),
			},
			wgpu::Extent3d {
				width: self.texture_width,
				height: self.texture_height,
				depth_or_array_layers: 1,
			},
		);
	}

	pub fn render(
		&mut self,
		ui_state: &mut crate::ui::UiState,
		delta_time: f32,
	) -> Result<(), wgpu::SurfaceError> {
		let output = self.surface.get_current_texture()?;
		let view = output
			.texture
			.create_view(&wgpu::TextureViewDescriptor::default());

		let mut encoder = self
			.device
			.create_command_encoder(&wgpu::CommandEncoderDescriptor {
				label: Some("Render Encoder"),
			});

		// Render background
		{
			let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
				label: Some("Background Render Pass"),
				color_attachments: &[Some(wgpu::RenderPassColorAttachment {
					view: &view,
					resolve_target: None,
					ops: wgpu::Operations {
						load: wgpu::LoadOp::Clear(wgpu::Color {
							r: 0.0,
							g: 0.0,
							b: 0.0,
							a: 1.0,
						}),
						store: true,
					},
				})],
				depth_stencil_attachment: None,
			});

			render_pass.set_pipeline(&self.render_pipeline);
			render_pass.set_bind_group(0, &self.texture_bind_group, &[]);
			render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
			render_pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
			render_pass.draw_indexed(0..INDICES.len() as u32, 0, 0..1);
		}

		// Render ImGui
		ui_state.prepare_frame(delta_time);
		let draw_data = ui_state.render();

		{
			let mut imgui_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
				label: Some("ImGui Render Pass"),
				color_attachments: &[Some(wgpu::RenderPassColorAttachment {
					view: &view,
					resolve_target: None,
					ops: wgpu::Operations {
						load: wgpu::LoadOp::Load,
						store: true,
					},
				})],
				depth_stencil_attachment: None,
			});

			self.imgui_renderer
				.render(draw_data, &self.queue, &self.device, &mut imgui_pass)
				.expect("ImGui rendering failed");
		}

		self.queue.submit(std::iter::once(encoder.finish()));
		output.present();

		Ok(())
	}
}
