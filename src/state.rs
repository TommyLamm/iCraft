use std::sync::Arc;
use winit::window::Window;
use crate::camera::{Camera, CameraUniform};
use crate::world::{Chunk, BlockType};
use crate::chunk_manager::ChunkManager;
use crate::physics::PlayerPhysics;
use crate::interaction::raycast;
use glam::Vec3;
use wgpu::util::DeviceExt;

pub struct ChunkMesh {
    pub opaque_vertex_buffer: wgpu::Buffer,
    pub opaque_index_buffer: wgpu::Buffer,
    pub opaque_num_indices: u32,
    pub transparent_vertex_buffer: wgpu::Buffer,
    pub transparent_index_buffer: wgpu::Buffer,
    pub transparent_num_indices: u32,
    pub dirty: bool,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub tex_coords: [f32; 2],
    pub light_level: f32,
}

impl Vertex {
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 3]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x2,
                },
                wgpu::VertexAttribute {
                    offset: (std::mem::size_of::<[f32; 3]>() + std::mem::size_of::<[f32; 2]>()) as wgpu::BufferAddress,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32,
                },
            ],
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct UiVertex {
    pub position: [f32; 3],
    pub color: [f32; 4],
}

impl UiVertex {
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<UiVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 3]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x4,
                },
            ],
        }
    }
}

#[derive(Default)]
pub struct KeyState {
    pub w: bool,
    pub a: bool,
    pub s: bool,
    pub d: bool,
    pub space: bool,
}

pub struct State {
    pub window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pub size: winit::dpi::PhysicalSize<u32>,
    render_pipeline: wgpu::RenderPipeline,
    trans_pipeline: wgpu::RenderPipeline,
    pub camera: Camera,
    camera_uniform: CameraUniform,
    camera_buffer: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,
    depth_view: wgpu::TextureView,
    pub chunk_manager: ChunkManager,
    pub chunk_meshes: std::collections::HashMap<(i32, i32), ChunkMesh>,
    pub player_physics: PlayerPhysics,
    pub keys: KeyState,
    #[allow(dead_code)]
    texture_atlas: crate::texture::TextureAtlas,
    crosshair_pipeline: wgpu::RenderPipeline,
    crosshair_buffer: wgpu::Buffer,
    pub is_paused: bool,
    mouse_ndc: [f32; 2],
    pub sensitivity: f32,
    ui_pipeline: wgpu::RenderPipeline,
    ui_line_pipeline: wgpu::RenderPipeline,
    ui_vertex_buffer: wgpu::Buffer,
    ui_line_vertex_buffer: wgpu::Buffer,
    num_ui_vertices: u32,
    num_ui_line_vertices: u32,
    pub selected_block: BlockType,
}

impl State {
    fn create_depth_texture(device: &wgpu::Device, config: &wgpu::SurfaceConfiguration) -> wgpu::TextureView {
        let depth_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Depth Texture"),
            size: wgpu::Extent3d {
                width: config.width,
                height: config.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        depth_texture.create_view(&wgpu::TextureViewDescriptor::default())
    }

    pub async fn new(window: Window) -> Self {
        let size = window.inner_size();
        let window = Arc::new(window);
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        let surface = instance.create_surface(window.clone()).unwrap();

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .unwrap();

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                    label: None,
                },
                None,
            )
            .await
            .unwrap();

        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(surface_caps.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width,
            height: size.height,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        // Setup Depth Buffer
        let depth_view = Self::create_depth_texture(&device, &config);

        // Initialize physics and keyboard input
        let player_physics = PlayerPhysics::new(Vec3::new(8.0, 80.0, 8.0));
        let keys = KeyState::default();

        // Load settings
        let settings = GameSettings::load();

        // Setup Camera
        let camera = Camera::new(
            player_physics.position + Vec3::new(0.0, 1.6, 0.0), // Spawn at player eye height
            f32::to_radians(90.0),
            f32::to_radians(-20.0),
            settings.fov,
        );
        let mut camera_uniform = CameraUniform::new();
        camera_uniform.update_view_proj(&camera, config.width as f32 / config.height as f32);

        let camera_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Camera Buffer"),
            contents: bytemuck::cast_slice(&[camera_uniform]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let texture_atlas = crate::texture::TextureAtlas::new_procedural(&device, &queue);

        let camera_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
            label: Some("camera_bind_group_layout"),
        });

        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &camera_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: camera_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&texture_atlas.view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&texture_atlas.sampler),
                },
            ],
            label: Some("camera_bind_group"),
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!("shader.wgsl"))),
        });

        let render_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Render Pipeline Layout"),
            bind_group_layouts: &[&camera_bind_group_layout],
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
                front_face: wgpu::FrontFace::Cw,
                cull_mode: Some(wgpu::Face::Back),
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        let trans_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Translucent Render Pipeline"),
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
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::SrcAlpha,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Cw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        // Initialize Crosshair Pipeline
        let crosshair_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Crosshair Pipeline Layout"),
            bind_group_layouts: &[],
            push_constant_ranges: &[],
        });

        let crosshair_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Crosshair Render Pipeline"),
            layout: Some(&crosshair_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_crosshair",
                buffers: &[Vertex::desc()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_crosshair",
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::LineList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Always,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        // Crosshair Vertices (Horizontal and Vertical Lines)
        let aspect = size.width as f32 / size.height as f32;
        let crosshair_size = 0.02;
        let crosshair_vertices = [
            Vertex { position: [-crosshair_size, 0.0, 0.0], tex_coords: [0.0, 0.0], light_level: 1.0 },
            Vertex { position: [crosshair_size, 0.0, 0.0], tex_coords: [0.0, 0.0], light_level: 1.0 },
            Vertex { position: [0.0, -crosshair_size * aspect, 0.0], tex_coords: [0.0, 0.0], light_level: 1.0 },
            Vertex { position: [0.0, crosshair_size * aspect, 0.0], tex_coords: [0.0, 0.0], light_level: 1.0 },
        ];

        let crosshair_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Crosshair Vertex Buffer"),
            contents: bytemuck::cast_slice(&crosshair_vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        // Initialize ChunkManager and load spawn area chunks
        let render_distance = settings.render_distance;
        let mut chunk_manager = ChunkManager::new(render_distance);
        let mut chunk_meshes = std::collections::HashMap::new();

        // Load spawn chunks synchronously
        for cx in -render_distance..=render_distance {
            for cz in -render_distance..=render_distance {
                let chunk = Chunk::new(cx, cz);
                chunk_manager.chunks.insert((cx, cz), chunk);
            }
        }

        // Propagate lighting for spawn chunks synchronously
        let mut spawn_dirty = std::collections::HashSet::new();
        let chunk_keys: Vec<(i32, i32)> = chunk_manager.chunks.keys().cloned().collect();
        for &(cx, cz) in &chunk_keys {
            crate::lighting::propagate_chunk_lighting(&mut chunk_manager, cx, cz, &mut spawn_dirty);
        }

        // Build meshes for spawn chunks synchronously
        let chunks_ref = &chunk_manager.chunks;
        for cx in -render_distance..=render_distance {
            for cz in -render_distance..=render_distance {
                let chunk = chunks_ref.get(&(cx, cz)).unwrap();
                let (o_verts, o_inds, t_verts, t_inds) = chunk.generate_mesh(|wx, wy, wz| {
                    if wy < 0 {
                        return (BlockType::Air, 0, 0);
                    }
                    if wy >= crate::world::CHUNK_HEIGHT as i32 {
                        return (BlockType::Air, 15, 0);
                    }
                    let cx_neighbor = wx.div_euclid(crate::world::CHUNK_WIDTH as i32);
                    let cz_neighbor = wz.div_euclid(crate::world::CHUNK_DEPTH as i32);
                    let bx_neighbor = wx.rem_euclid(crate::world::CHUNK_WIDTH as i32) as usize;
                    let bz_neighbor = wz.rem_euclid(crate::world::CHUNK_DEPTH as i32) as usize;
                    if let Some(c) = chunks_ref.get(&(cx_neighbor, cz_neighbor)) {
                        (
                            c.blocks[bx_neighbor][wy as usize][bz_neighbor],
                            c.sky_light[bx_neighbor][wy as usize][bz_neighbor],
                            c.block_light[bx_neighbor][wy as usize][bz_neighbor],
                        )
                    } else {
                        (BlockType::Air, 15, 0)
                    }
                });

                let opaque_vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Chunk Opaque Vertex Buffer"),
                    contents: bytemuck::cast_slice(&o_verts),
                    usage: wgpu::BufferUsages::VERTEX,
                });
                let opaque_index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Chunk Opaque Index Buffer"),
                    contents: bytemuck::cast_slice(&o_inds),
                    usage: wgpu::BufferUsages::INDEX,
                });
                let transparent_vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Chunk Translucent Vertex Buffer"),
                    contents: bytemuck::cast_slice(&t_verts),
                    usage: wgpu::BufferUsages::VERTEX,
                });
                let transparent_index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Chunk Translucent Index Buffer"),
                    contents: bytemuck::cast_slice(&t_inds),
                    usage: wgpu::BufferUsages::INDEX,
                });

                chunk_meshes.insert((cx, cz), ChunkMesh {
                    opaque_vertex_buffer,
                    opaque_index_buffer,
                    opaque_num_indices: o_inds.len() as u32,
                    transparent_vertex_buffer,
                    transparent_index_buffer,
                    transparent_num_indices: t_inds.len() as u32,
                    dirty: false,
                });
            }
        }

        // Initialize UI Pipelines
        let ui_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("UI Pipeline Layout"),
            bind_group_layouts: &[],
            push_constant_ranges: &[],
        });

        let ui_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("UI Render Pipeline"),
            layout: Some(&ui_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_ui",
                buffers: &[UiVertex::desc()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_ui",
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Cw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Always,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        let ui_line_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("UI Line Render Pipeline"),
            layout: Some(&ui_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_ui",
                buffers: &[UiVertex::desc()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_ui",
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::LineList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Cw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Always,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        // Initialize UI Buffers
        let ui_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("UI Vertex Buffer"),
            size: (std::mem::size_of::<UiVertex>() * 1024) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let ui_line_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("UI Line Vertex Buffer"),
            size: (std::mem::size_of::<UiVertex>() * 1024) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            window,
            surface,
            device,
            queue,
            config,
            size,
            render_pipeline,
            trans_pipeline,
            camera,
            camera_uniform,
            camera_buffer,
            camera_bind_group,
            depth_view,
            chunk_manager,
            chunk_meshes,
            player_physics,
            keys,
            texture_atlas,
            crosshair_pipeline,
            crosshair_buffer,
            is_paused: false,
            mouse_ndc: [0.0, 0.0],
            sensitivity: settings.sensitivity,
            ui_pipeline,
            ui_line_pipeline,
            ui_vertex_buffer,
            ui_line_vertex_buffer,
            num_ui_vertices: 0,
            num_ui_line_vertices: 0,
            selected_block: BlockType::Stone,
        }
    }

    pub fn save_settings(&self) {
        let settings = GameSettings {
            fov: self.camera.fov,
            sensitivity: self.sensitivity,
            render_distance: self.chunk_manager.render_distance,
        };
        settings.save();
    }

    pub fn update_chunks(&mut self) {
        let player_pos = self.player_physics.position;
        let px = (player_pos.x / 16.0).floor() as i32;
        let pz = (player_pos.z / 16.0).floor() as i32;
        let r = self.chunk_manager.render_distance;

        // 1. Unload out-of-bounds chunks
        self.chunk_manager.chunks.retain(|&(cx, cz), _| {
            (cx - px).abs() <= r && (cz - pz).abs() <= r
        });
        self.chunk_meshes.retain(|&(cx, cz), _| {
            (cx - px).abs() <= r && (cz - pz).abs() <= r
        });

        // 2. Queue missing chunks
        let mut load_queue = Vec::new();
        for dx in -r..=r {
            for dz in -r..=r {
                let cx = px + dx;
                let cz = pz + dz;
                if !self.chunk_manager.chunks.contains_key(&(cx, cz)) {
                    load_queue.push((cx, cz));
                }
            }
        }

        load_queue.sort_by_key(|&(cx, cz)| {
            let dx = cx - px;
            let dz = cz - pz;
            dx * dx + dz * dz
        });

        // 3. Load 1 chunk per frame
        if let Some(&(cx, cz)) = load_queue.first() {
            let chunk = Chunk::new(cx, cz);
            self.chunk_manager.chunks.insert((cx, cz), chunk);

            let mut dirty = std::collections::HashSet::new();
            crate::lighting::propagate_chunk_lighting(&mut self.chunk_manager, cx, cz, &mut dirty);

            // Mark neighbors dirty
            for &(ncx, ncz) in &[(cx - 1, cz), (cx + 1, cz), (cx, cz - 1), (cx, cz + 1)] {
                if let Some(mesh) = self.chunk_meshes.get_mut(&(ncx, ncz)) {
                    mesh.dirty = true;
                }
            }

            for (dcx, dcz) in dirty {
                if let Some(mesh) = self.chunk_meshes.get_mut(&(dcx, dcz)) {
                    mesh.dirty = true;
                }
            }
        }

        // 4. Rebuild at most 2 dirty meshes per frame
        let mut to_rebuild = Vec::new();
        for (&(cx, cz), _) in &self.chunk_manager.chunks {
            let needs_mesh = !self.chunk_meshes.contains_key(&(cx, cz));
            let is_dirty = self.chunk_meshes.get(&(cx, cz)).map(|m| m.dirty).unwrap_or(false);
            if needs_mesh || is_dirty {
                to_rebuild.push((cx, cz));
            }
        }

        let chunks_ref = &self.chunk_manager.chunks;
        for (cx, cz) in to_rebuild.into_iter().take(2) {
            let chunk = chunks_ref.get(&(cx, cz)).unwrap();
            let (o_verts, o_inds, t_verts, t_inds) = chunk.generate_mesh(|wx, wy, wz| {
                if wy < 0 {
                    return (BlockType::Air, 0, 0);
                }
                if wy >= crate::world::CHUNK_HEIGHT as i32 {
                    return (BlockType::Air, 15, 0);
                }
                let cx_neighbor = wx.div_euclid(crate::world::CHUNK_WIDTH as i32);
                let cz_neighbor = wz.div_euclid(crate::world::CHUNK_DEPTH as i32);
                let bx_neighbor = wx.rem_euclid(crate::world::CHUNK_WIDTH as i32) as usize;
                let bz_neighbor = wz.rem_euclid(crate::world::CHUNK_DEPTH as i32) as usize;
                if let Some(c) = chunks_ref.get(&(cx_neighbor, cz_neighbor)) {
                    (
                        c.blocks[bx_neighbor][wy as usize][bz_neighbor],
                        c.sky_light[bx_neighbor][wy as usize][bz_neighbor],
                        c.block_light[bx_neighbor][wy as usize][bz_neighbor],
                    )
                } else {
                    (BlockType::Air, 15, 0)
                }
            });

            let opaque_vertex_buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Chunk Opaque Vertex Buffer"),
                contents: bytemuck::cast_slice(&o_verts),
                usage: wgpu::BufferUsages::VERTEX,
            });
            let opaque_index_buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Chunk Opaque Index Buffer"),
                contents: bytemuck::cast_slice(&o_inds),
                usage: wgpu::BufferUsages::INDEX,
            });
            let transparent_vertex_buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Chunk Translucent Vertex Buffer"),
                contents: bytemuck::cast_slice(&t_verts),
                usage: wgpu::BufferUsages::VERTEX,
            });
            let transparent_index_buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Chunk Translucent Index Buffer"),
                contents: bytemuck::cast_slice(&t_inds),
                usage: wgpu::BufferUsages::INDEX,
            });

            self.chunk_meshes.insert((cx, cz), ChunkMesh {
                opaque_vertex_buffer,
                opaque_index_buffer,
                opaque_num_indices: o_inds.len() as u32,
                transparent_vertex_buffer,
                transparent_index_buffer,
                transparent_num_indices: t_inds.len() as u32,
                dirty: false,
            });
        }
    }

    pub fn set_paused(&mut self, paused: bool) {
        self.is_paused = paused;
        println!("[Debug] set_paused called with: {}", paused);
        if paused {
            let res = self.window.set_cursor_grab(winit::window::CursorGrabMode::None);
            println!("[Debug] Release grab result: {:?}", res);
            self.window.set_cursor_visible(true);
            self.keys = KeyState::default();
        } else {
            let res = self.window.set_cursor_grab(winit::window::CursorGrabMode::Locked)
                .or_else(|_| self.window.set_cursor_grab(winit::window::CursorGrabMode::Confined));
            println!("[Debug] Grab cursor result: {:?}", res);
            self.window.set_cursor_visible(false);
        }
    }

    pub fn handle_mouse_move(&mut self, x: f64, y: f64) {
        let ndc_x = (x as f32 / self.size.width as f32) * 2.0 - 1.0;
        let ndc_y = 1.0 - (y as f32 / self.size.height as f32) * 2.0;
        self.mouse_ndc = [ndc_x, ndc_y];
    }

    pub fn handle_menu_click(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if self.is_paused {
            let [x, y] = self.mouse_ndc;
            
            // Resume Button bounds: X: [-0.3, 0.3], Y: [0.24, 0.34]
            if x >= -0.3 && x <= 0.3 && y >= 0.24 && y <= 0.34 {
                self.set_paused(false);
            }
            // FOV Button bounds: X: [-0.3, 0.3], Y: [0.10, 0.20]
            else if x >= -0.3 && x <= 0.3 && y >= 0.10 && y <= 0.20 {
                if x < 0.0 {
                    self.camera.fov = (self.camera.fov - 5.0).max(30.0);
                } else {
                    self.camera.fov = (self.camera.fov + 5.0).min(120.0);
                }
                // Update camera projection buffer immediately for visual feedback in paused state
                self.camera_uniform.update_view_proj(&self.camera, self.config.width as f32 / self.config.height as f32);
                self.queue.write_buffer(&self.camera_buffer, 0, bytemuck::cast_slice(&[self.camera_uniform]));
                self.save_settings();
            }
            // Sensitivity Button bounds: X: [-0.3, 0.3], Y: [-0.04, 0.06]
            else if x >= -0.3 && x <= 0.3 && y >= -0.04 && y <= 0.06 {
                if x < 0.0 {
                    self.sensitivity = (self.sensitivity - 0.0002).max(0.0002);
                } else {
                    self.sensitivity = (self.sensitivity + 0.0002).min(0.0060);
                }
                self.save_settings();
            }
            // Render Distance Button bounds: X: [-0.3, 0.3], Y: [-0.18, -0.08]
            else if x >= -0.3 && x <= 0.3 && y >= -0.18 && y <= -0.08 {
                if x < 0.0 {
                    self.chunk_manager.render_distance = (self.chunk_manager.render_distance - 1).max(2);
                } else {
                    self.chunk_manager.render_distance = (self.chunk_manager.render_distance + 1).min(16);
                }
                self.save_settings();
            }
            // Quit Button bounds: X: [-0.3, 0.3], Y: [-0.32, -0.22]
            else if x >= -0.3 && x <= 0.3 && y >= -0.32 && y <= -0.22 {
                event_loop.exit();
            }
        }
    }

    pub fn update(&mut self, dt: f32) {
        if self.is_paused {
            return;
        }
        let mut move_dir = Vec3::ZERO;
        let yaw_cos = self.camera.yaw.cos();
        let yaw_sin = self.camera.yaw.sin();
        let forward = Vec3::new(yaw_cos, 0.0, yaw_sin).normalize_or_zero();
        let right = Vec3::new(-yaw_sin, 0.0, yaw_cos).normalize_or_zero();

        if self.keys.w {
            move_dir += forward;
        }
        if self.keys.s {
            move_dir -= forward;
        }
        if self.keys.a {
            move_dir += right;
        }
        if self.keys.d {
            move_dir -= right;
        }
        let mut movement = move_dir.normalize_or_zero();
        if self.keys.space {
            movement.y = 1.0;
        }

        self.player_physics.update(dt, &self.chunk_manager, movement);
        self.update_chunks();

        // Sync camera position to player position at eye height
        self.camera.position = self.player_physics.position + Vec3::new(0.0, 1.6, 0.0);
        self.camera_uniform.update_view_proj(&self.camera, self.config.width as f32 / self.config.height as f32);
        self.queue.write_buffer(&self.camera_buffer, 0, bytemuck::cast_slice(&[self.camera_uniform]));
    }

    pub fn handle_click(&mut self, is_left_click: bool) {
        let dir = Vec3::new(
            self.camera.yaw.cos() * self.camera.pitch.cos(),
            self.camera.pitch.sin(),
            self.camera.yaw.sin() * self.camera.pitch.cos(),
        ).normalize_or_zero();

        if let Some(hit) = raycast(self.camera.position, dir, 5.0, &self.chunk_manager) {
            let target = if is_left_click {
                hit.block_pos
            } else {
                hit.block_pos + hit.normal
            };

            let wx = target.x as i32;
            let wy = target.y as i32;
            let wz = target.z as i32;

            let mut dirty_chunks = std::collections::HashSet::new();

            if is_left_click {
                let old_block = self.chunk_manager.get_block(wx, wy, wz);
                if old_block != BlockType::Air {
                    self.chunk_manager.set_block(wx, wy, wz, BlockType::Air);

                    // Update lighting for removal
                    crate::lighting::update_sky_light_after_removed(&mut self.chunk_manager, wx, wy, wz, &mut dirty_chunks);
                    crate::lighting::update_block_light_after_removed(&mut self.chunk_manager, wx, wy, wz, old_block.properties().light_emission, &mut dirty_chunks);
                }
            } else {
                let placed_block = self.selected_block;
                self.chunk_manager.set_block(wx, wy, wz, placed_block);

                // Update lighting for placement
                crate::lighting::update_sky_light_after_placed(&mut self.chunk_manager, wx, wy, wz, &mut dirty_chunks);
                crate::lighting::update_block_light_after_placed(&mut self.chunk_manager, wx, wy, wz, placed_block.properties().light_emission, &mut dirty_chunks);
            }

            // Mark the modified chunk and boundary neighbors dirty
            let cx = wx.div_euclid(crate::world::CHUNK_WIDTH as i32);
            let cz = wz.div_euclid(crate::world::CHUNK_DEPTH as i32);
            let lx = wx.rem_euclid(crate::world::CHUNK_WIDTH as i32);
            let lz = wz.rem_euclid(crate::world::CHUNK_DEPTH as i32);

            dirty_chunks.insert((cx, cz));
            if lx == 0 { dirty_chunks.insert((cx - 1, cz)); }
            if lx == 15 { dirty_chunks.insert((cx + 1, cz)); }
            if lz == 0 { dirty_chunks.insert((cx, cz - 1)); }
            if lz == 15 { dirty_chunks.insert((cx, cz + 1)); }

            for (dcx, dcz) in dirty_chunks {
                if let Some(mesh) = self.chunk_meshes.get_mut(&(dcx, dcz)) {
                    mesh.dirty = true;
                }
            }
        }
    }

    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width > 0 && new_size.height > 0 {
            self.size = new_size;
            self.config.width = new_size.width;
            self.config.height = new_size.height;
            self.surface.configure(&self.device, &self.config);
            // Recreate depth texture on resize
            self.depth_view = Self::create_depth_texture(&self.device, &self.config);
        }
    }

    pub fn render(&mut self) -> Result<(), wgpu::SurfaceError> {
        let output = self.surface.get_current_texture()?;
        let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());

        if self.is_paused {
            let mut ui_vertices = Vec::new();
            let mut ui_line_vertices = Vec::new();

            let mouse_x = self.mouse_ndc[0];
            let mouse_y = self.mouse_ndc[1];

            // Hover states
            let resume_hover = mouse_x >= -0.3 && mouse_x <= 0.3 && mouse_y >= 0.24 && mouse_y <= 0.34;
            let fov_hover = mouse_x >= -0.3 && mouse_x <= 0.3 && mouse_y >= 0.10 && mouse_y <= 0.20;
            let sens_hover = mouse_x >= -0.3 && mouse_x <= 0.3 && mouse_y >= -0.04 && mouse_y <= 0.06;
            let rd_hover = mouse_x >= -0.3 && mouse_x <= 0.3 && mouse_y >= -0.18 && mouse_y <= -0.08;
            let quit_hover = mouse_x >= -0.3 && mouse_x <= 0.3 && mouse_y >= -0.32 && mouse_y <= -0.22;

            // 1. Dark overlay (screen covers from -1.0 to 1.0)
            let bg_color = [0.1, 0.1, 0.1, 0.7];
            ui_vertices.push(UiVertex { position: [-1.0, 1.0, 0.0], color: bg_color });
            ui_vertices.push(UiVertex { position: [-1.0, -1.0, 0.0], color: bg_color });
            ui_vertices.push(UiVertex { position: [1.0, -1.0, 0.0], color: bg_color });
            ui_vertices.push(UiVertex { position: [-1.0, 1.0, 0.0], color: bg_color });
            ui_vertices.push(UiVertex { position: [1.0, -1.0, 0.0], color: bg_color });
            ui_vertices.push(UiVertex { position: [1.0, 1.0, 0.0], color: bg_color });

            // Button drawing helper
            let draw_button = |hover: bool, y_min: f32, y_max: f32, ui_verts: &mut Vec<UiVertex>, ui_line_verts: &mut Vec<UiVertex>| {
                let bg = if hover { [0.4, 0.4, 0.4, 1.0] } else { [0.2, 0.2, 0.2, 1.0] };
                let border = if hover { [1.0, 1.0, 1.0, 1.0] } else { [0.6, 0.6, 0.6, 1.0] };
                
                // Background (two triangles)
                ui_verts.push(UiVertex { position: [-0.3, y_max, 0.0], color: bg });
                ui_verts.push(UiVertex { position: [-0.3, y_min, 0.0], color: bg });
                ui_verts.push(UiVertex { position: [0.3, y_min, 0.0], color: bg });
                ui_verts.push(UiVertex { position: [-0.3, y_max, 0.0], color: bg });
                ui_verts.push(UiVertex { position: [0.3, y_min, 0.0], color: bg });
                ui_verts.push(UiVertex { position: [0.3, y_max, 0.0], color: bg });

                // Border (line loop)
                ui_line_verts.push(UiVertex { position: [-0.3, y_max, 0.0], color: border });
                ui_line_verts.push(UiVertex { position: [0.3, y_max, 0.0], color: border });
                ui_line_verts.push(UiVertex { position: [0.3, y_max, 0.0], color: border });
                ui_line_verts.push(UiVertex { position: [0.3, y_min, 0.0], color: border });
                ui_line_verts.push(UiVertex { position: [0.3, y_min, 0.0], color: border });
                ui_line_verts.push(UiVertex { position: [-0.3, y_min, 0.0], color: border });
                ui_line_verts.push(UiVertex { position: [-0.3, y_min, 0.0], color: border });
                ui_line_verts.push(UiVertex { position: [-0.3, y_max, 0.0], color: border });
            };

            // Draw Button backgrounds and borders
            draw_button(resume_hover, 0.24, 0.34, &mut ui_vertices, &mut ui_line_vertices);
            draw_button(fov_hover, 0.10, 0.20, &mut ui_vertices, &mut ui_line_vertices);
            draw_button(sens_hover, -0.04, 0.06, &mut ui_vertices, &mut ui_line_vertices);
            draw_button(rd_hover, -0.18, -0.08, &mut ui_vertices, &mut ui_line_vertices);
            draw_button(quit_hover, -0.32, -0.22, &mut ui_vertices, &mut ui_line_vertices);

            // Centered text drawing helper
            let draw_centered_text = |s: &str, y: f32, char_w: f32, char_h: f32, spacing: f32, color: [f32; 4], vertices: &mut Vec<UiVertex>| {
                let upper = s.to_uppercase();
                let n = upper.len() as f32;
                let width = n * char_w + (n - 1.0) * spacing;
                let start_x = -width / 2.0;
                add_string_lines(&upper, start_x, y, char_w, char_h, spacing, color, vertices);
            };

            // Render Text Labels
            let text_color = [1.0, 1.0, 1.0, 1.0];
            // "GAME PAUSED"
            draw_centered_text("GAME PAUSED", 0.40, 0.03, 0.06, 0.012, text_color, &mut ui_line_vertices);
            // "RESUME"
            draw_centered_text("RESUME", 0.28, 0.02, 0.04, 0.008, text_color, &mut ui_line_vertices);
            
            // "FOV < value >"
            let fov_text = format!("FOV < {:.0} >", self.camera.fov);
            draw_centered_text(&fov_text, 0.14, 0.02, 0.04, 0.008, text_color, &mut ui_line_vertices);
            
            // "SENS < value >"
            let sens_val = (self.sensitivity / 0.002 * 100.0).round();
            let sens_text = format!("SENS < {:.0} >", sens_val);
            draw_centered_text(&sens_text, 0.00, 0.02, 0.04, 0.008, text_color, &mut ui_line_vertices);

            // "RENDER DISTANCE < value >"
            let rd_text = format!("RENDER DISTANCE < {} >", self.chunk_manager.render_distance);
            draw_centered_text(&rd_text, -0.14, 0.02, 0.04, 0.008, text_color, &mut ui_line_vertices);
            
            // "QUIT"
            draw_centered_text("QUIT", -0.28, 0.02, 0.04, 0.008, text_color, &mut ui_line_vertices);

            // Cap the sizes to the preallocated buffers (1024 vertices)
            let ui_vert_len = ui_vertices.len().min(1024);
            let ui_line_vert_len = ui_line_vertices.len().min(1024);

            self.queue.write_buffer(&self.ui_vertex_buffer, 0, bytemuck::cast_slice(&ui_vertices[..ui_vert_len]));
            self.queue.write_buffer(&self.ui_line_vertex_buffer, 0, bytemuck::cast_slice(&ui_line_vertices[..ui_line_vert_len]));

            self.num_ui_vertices = ui_vert_len as u32;
            self.num_ui_line_vertices = ui_line_vert_len as u32;
        } else {
            let mut hud_line_vertices = Vec::new();
            let text_color = [1.0, 1.0, 1.0, 1.0];
            let selected_text = format!("SELECTED: {:?}", self.selected_block);
            let upper = selected_text.to_uppercase();
            let char_w = 0.015;
            let char_h = 0.03;
            let spacing = 0.006;
            let n = upper.len() as f32;
            let width = n * char_w + (n - 1.0) * spacing;
            let start_x = -width / 2.0;
            add_string_lines(&upper, start_x, -0.90, char_w, char_h, spacing, text_color, &mut hud_line_vertices);

            let hud_line_vert_len = hud_line_vertices.len().min(1024);
            self.queue.write_buffer(&self.ui_line_vertex_buffer, 0, bytemuck::cast_slice(&hud_line_vertices[..hud_line_vert_len]));
            self.num_ui_line_vertices = hud_line_vert_len as u32;
        }

        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Render Encoder"),
        });

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.53,
                            g: 0.81,
                            b: 0.92,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                occlusion_query_set: None,
                timestamp_writes: None,
            });

            // Pass 1: Opaque & Cutout
            render_pass.set_pipeline(&self.render_pipeline);
            render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
            for mesh in self.chunk_meshes.values() {
                if mesh.opaque_num_indices > 0 {
                    render_pass.set_vertex_buffer(0, mesh.opaque_vertex_buffer.slice(..));
                    render_pass.set_index_buffer(mesh.opaque_index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                    render_pass.draw_indexed(0..mesh.opaque_num_indices, 0, 0..1);
                }
            }

            // Pass 2: Translucent (Water/Ice)
            render_pass.set_pipeline(&self.trans_pipeline);
            for mesh in self.chunk_meshes.values() {
                if mesh.transparent_num_indices > 0 {
                    render_pass.set_vertex_buffer(0, mesh.transparent_vertex_buffer.slice(..));
                    render_pass.set_index_buffer(mesh.transparent_index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                    render_pass.draw_indexed(0..mesh.transparent_num_indices, 0, 0..1);
                }
            }

            if !self.is_paused {
                // 2. Draw 2D UI Crosshair
                render_pass.set_pipeline(&self.crosshair_pipeline);
                render_pass.set_vertex_buffer(0, self.crosshair_buffer.slice(..));
                render_pass.draw(0..4, 0..1);

                // 2b. Draw 2D HUD text (using ui_line_pipeline)
                render_pass.set_pipeline(&self.ui_line_pipeline);
                render_pass.set_vertex_buffer(0, self.ui_line_vertex_buffer.slice(..));
                render_pass.draw(0..self.num_ui_line_vertices, 0..1);
            } else {
                // 3. Draw Pause Menu
                // Background overlay & buttons
                render_pass.set_pipeline(&self.ui_pipeline);
                render_pass.set_vertex_buffer(0, self.ui_vertex_buffer.slice(..));
                render_pass.draw(0..self.num_ui_vertices, 0..1);

                // Borders & Text
                render_pass.set_pipeline(&self.ui_line_pipeline);
                render_pass.set_vertex_buffer(0, self.ui_line_vertex_buffer.slice(..));
                render_pass.draw(0..self.num_ui_line_vertices, 0..1);
            }
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();
        Ok(())
    }
}

fn add_char_lines(
    c: char,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: [f32; 4],
    vertices: &mut Vec<UiVertex>,
) {
    let x0 = x;
    let x1 = x + w;
    let xm = x + w * 0.5;
    let y0 = y;
    let y1 = y + h;
    let ym = y + h * 0.5;

    let mut add_line = |x_start: f32, y_start: f32, x_end: f32, y_end: f32| {
        vertices.push(UiVertex { position: [x_start, y_start, 0.0], color });
        vertices.push(UiVertex { position: [x_end, y_end, 0.0], color });
    };

    match c {
        'R' => {
            add_line(x0, y0, x0, y1);
            add_line(x0, y1, x1, y1);
            add_line(x1, y1, x1, ym);
            add_line(x1, ym, x0, ym);
            add_line(x0, ym, x1, y0);
        }
        'E' => {
            add_line(x0, y0, x0, y1);
            add_line(x0, y1, x1, y1);
            add_line(x0, ym, x1, ym);
            add_line(x0, y0, x1, y0);
        }
        'S' => {
            add_line(x1, y1, x0, y1);
            add_line(x0, y1, x0, ym);
            add_line(x0, ym, x1, ym);
            add_line(x1, ym, x1, y0);
            add_line(x1, y0, x0, y0);
        }
        'U' => {
            add_line(x0, y1, x0, y0);
            add_line(x0, y0, x1, y0);
            add_line(x1, y0, x1, y1);
        }
        'M' => {
            add_line(x0, y0, x0, y1);
            add_line(x0, y1, xm, ym);
            add_line(xm, ym, x1, y1);
            add_line(x1, y1, x1, y0);
        }
        'G' => {
            add_line(x1, y1, x0, y1);
            add_line(x0, y1, x0, y0);
            add_line(x0, y0, x1, y0);
            add_line(x1, y0, x1, ym);
            add_line(x1, ym, xm, ym);
        }
        'A' => {
            add_line(x0, y0, x0, y1);
            add_line(x0, y1, x1, y1);
            add_line(x1, y1, x1, y0);
            add_line(x0, ym, x1, ym);
        }
        'Q' => {
            add_line(x0, y0, x0, y1);
            add_line(x0, y1, x1, y1);
            add_line(x1, y1, x1, y0);
            add_line(x1, y0, x0, y0);
            add_line(xm, ym, x1 + w * 0.2, y0 - h * 0.2);
        }
        'I' => {
            add_line(xm, y0, xm, y1);
            add_line(x0, y1, x1, y1);
            add_line(x0, y0, x1, y0);
        }
        'T' => {
            add_line(x0, y1, x1, y1);
            add_line(xm, y1, xm, y0);
        }
        'P' => {
            add_line(x0, y0, x0, y1);
            add_line(x0, y1, x1, y1);
            add_line(x1, y1, x1, ym);
            add_line(x1, ym, x0, ym);
        }
        'O' => {
            add_line(x0, y0, x0, y1);
            add_line(x0, y1, x1, y1);
            add_line(x1, y1, x1, y0);
            add_line(x1, y0, x0, y0);
        }
        'D' => {
            add_line(x0, y0, x0, y1);
            add_line(x0, y1, xm, y1);
            add_line(xm, y1, x1, ym);
            add_line(x1, ym, xm, y0);
            add_line(xm, y0, x0, y0);
        }
        'F' => {
            add_line(x0, y0, x0, y1);
            add_line(x0, y1, x1, y1);
            add_line(x0, ym, x1, ym);
        }
        'V' => {
            add_line(x0, y1, xm, y0);
            add_line(xm, y0, x1, y1);
        }
        'N' => {
            add_line(x0, y0, x0, y1);
            add_line(x0, y1, x1, y0);
            add_line(x1, y0, x1, y1);
        }
        'Y' => {
            add_line(x0, y1, xm, ym);
            add_line(x1, y1, xm, ym);
            add_line(xm, ym, xm, y0);
        }
        'C' => {
            add_line(x1, y1, x0, y1);
            add_line(x0, y1, x0, y0);
            add_line(x0, y0, x1, y0);
        }
        'H' => {
            add_line(x0, y0, x0, y1);
            add_line(x1, y0, x1, y1);
            add_line(x0, ym, x1, ym);
        }
        'L' => {
            add_line(x0, y1, x0, y0);
            add_line(x0, y0, x1, y0);
        }
        '<' => {
            add_line(x1, y1, x0, ym);
            add_line(x0, ym, x1, y0);
        }
        '>' => {
            add_line(x0, y1, x1, ym);
            add_line(x1, ym, x0, y0);
        }
        '-' => {
            add_line(x0, ym, x1, ym);
        }
        '+' => {
            add_line(x0, ym, x1, ym);
            add_line(xm, y0, xm, y1);
        }
        ':' => {
            add_line(xm - w * 0.05, y0 + h * 0.7, xm + w * 0.05, y0 + h * 0.7);
            add_line(xm - w * 0.05, y0 + h * 0.3, xm + w * 0.05, y0 + h * 0.3);
        }
        '0' => {
            add_line(x0, y0, x0, y1);
            add_line(x0, y1, x1, y1);
            add_line(x1, y1, x1, y0);
            add_line(x1, y0, x0, y0);
        }
        '1' => {
            add_line(xm, y0, xm, y1);
            add_line(x0, y0, x1, y0);
            add_line(xm - w * 0.2, y1 - h * 0.2, xm, y1);
        }
        '2' => {
            add_line(x0, y1, x1, y1);
            add_line(x1, y1, x1, ym);
            add_line(x1, ym, x0, ym);
            add_line(x0, ym, x0, y0);
            add_line(x0, y0, x1, y0);
        }
        '3' => {
            add_line(x0, y1, x1, y1);
            add_line(x1, y1, x1, y0);
            add_line(x1, y0, x0, y0);
            add_line(x0, ym, x1, ym);
        }
        '4' => {
            add_line(x0, y1, x0, ym);
            add_line(x0, ym, x1, ym);
            add_line(x1, y1, x1, y0);
        }
        '5' => {
            add_line(x1, y1, x0, y1);
            add_line(x0, y1, x0, ym);
            add_line(x0, ym, x1, ym);
            add_line(x1, ym, x1, y0);
            add_line(x1, y0, x0, y0);
        }
        '6' => {
            add_line(x1, y1, x0, y1);
            add_line(x0, y1, x0, y0);
            add_line(x0, y0, x1, y0);
            add_line(x1, y0, x1, ym);
            add_line(x1, ym, x0, ym);
        }
        '7' => {
            add_line(x0, y1, x1, y1);
            add_line(x1, y1, x1, y0);
        }
        '8' => {
            add_line(x0, y0, x0, y1);
            add_line(x0, y1, x1, y1);
            add_line(x1, y1, x1, y0);
            add_line(x1, y0, x0, y0);
            add_line(x0, ym, x1, ym);
        }
        '9' => {
            add_line(x0, ym, x1, ym);
            add_line(x0, ym, x0, y1);
            add_line(x0, y1, x1, y1);
            add_line(x1, y1, x1, y0);
            add_line(x1, y0, x0, y0);
        }
        '.' => {
            add_line(xm - w * 0.05, y0, xm + w * 0.05, y0);
        }
        ' ' => {}
        _ => {}
    }
}

fn add_string_lines(
    s: &str,
    start_x: f32,
    y: f32,
    char_w: f32,
    char_h: f32,
    spacing: f32,
    color: [f32; 4],
    vertices: &mut Vec<UiVertex>,
) {
    let mut current_x = start_x;
    for c in s.chars() {
        add_char_lines(c, current_x, y, char_w, char_h, color, vertices);
        current_x += char_w + spacing;
    }
}

impl Drop for State {
    fn drop(&mut self) {
        let _ = self.window.set_cursor_grab(winit::window::CursorGrabMode::None);
        self.window.set_cursor_visible(true);
    }
}

pub struct GameSettings {
    pub fov: f32,
    pub sensitivity: f32,
    pub render_distance: i32,
}

impl GameSettings {
    pub fn load() -> Self {
        let mut fov = 70.0;
        let mut sensitivity = 0.002;
        let mut render_distance = 8;
        if let Ok(mut file) = std::fs::File::open("settings.txt") {
            let mut contents = String::new();
            use std::io::Read;
            if file.read_to_string(&mut contents).is_ok() {
                for line in contents.lines() {
                    let parts: Vec<&str> = line.split(':').collect();
                    if parts.len() == 2 {
                        let key = parts[0].trim();
                        let value = parts[1].trim();
                        match key {
                            "fov" => {
                                if let Ok(parsed) = value.parse::<f32>() {
                                    fov = parsed;
                                }
                            }
                            "sensitivity" => {
                                if let Ok(parsed) = value.parse::<f32>() {
                                    sensitivity = parsed;
                                }
                            }
                            "render_distance" => {
                                if let Ok(parsed) = value.parse::<i32>() {
                                    render_distance = parsed;
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        Self { fov, sensitivity, render_distance }
    }

    pub fn save(&self) {
        if let Ok(mut file) = std::fs::File::create("settings.txt") {
            let contents = format!("fov:{}\nsensitivity:{}\nrender_distance:{}\n", self.fov, self.sensitivity, self.render_distance);
            use std::io::Write;
            let _ = file.write_all(contents.as_bytes());
        }
    }
}
