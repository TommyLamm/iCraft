use std::sync::Arc;
use winit::window::Window;
use crate::camera::{Camera, CameraUniform};
use crate::world::{Chunk, BlockType};
use crate::inventory::{Inventory, GameMode, ItemStack, Item, ToolType};
use crate::crafting::RecipeManager;
use crate::chunk_manager::ChunkManager;
use crate::physics::PlayerPhysics;
use crate::interaction::raycast;
use crate::player::{PlayerState, DamageSource};
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

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct TexturedUiVertex {
    pub position: [f32; 3],
    pub tex_coords: [f32; 2],
    pub color: [f32; 4],
}

impl TexturedUiVertex {
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<TexturedUiVertex>() as wgpu::BufferAddress,
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
    sky_pipeline: wgpu::RenderPipeline,
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
    ui_textured_pipeline: wgpu::RenderPipeline,
    ui_textured_vertex_buffer: wgpu::Buffer,
    num_ui_vertices: u32,
    num_ui_line_vertices: u32,
    num_ui_textured_vertices: u32,
    pub game_mode: GameMode,
    pub inventory: Inventory,
    pub recipe_manager: RecipeManager,
    pub left_mouse_pressed: bool,
    pub mining_target: Option<glam::Vec3>,
    pub mining_progress: f32,
    crack_vertex_buffer: wgpu::Buffer,
    crack_index_buffer: wgpu::Buffer,
    pub player_state: PlayerState,
    pub void_damage_timer: f32,
    pub world_time: crate::camera::WorldTime,
    pub show_debug: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotType {
    Hotbar(usize),
    Backpack(usize),
    Armor(usize),
    CraftInput(usize),
    CraftOutput,
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
        let world_time = crate::camera::WorldTime::new();
        let show_debug = false;
        let mut camera_uniform = CameraUniform::new();
        camera_uniform.update_view_proj(&camera, config.width as f32 / config.height as f32, settings.render_distance as u32, &world_time);

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
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
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

        let sky_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Sky Render Pipeline"),
            layout: Some(&render_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_sky",
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_sky",
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

        let ui_textured_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("UI Textured Render Pipeline"),
            layout: Some(&render_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_textured_ui",
                buffers: &[TexturedUiVertex::desc()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_textured_ui",
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

        // Initialize UI Buffers
        let ui_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("UI Vertex Buffer"),
            size: (std::mem::size_of::<UiVertex>() * 4096) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let ui_line_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("UI Line Vertex Buffer"),
            size: (std::mem::size_of::<UiVertex>() * 4096) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let ui_textured_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("UI Textured Vertex Buffer"),
            size: (std::mem::size_of::<TexturedUiVertex>() * 4096) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let crack_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Crack Vertex Buffer"),
            size: (24 * std::mem::size_of::<Vertex>()) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let crack_index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Crack Index Buffer"),
            size: (36 * std::mem::size_of::<u32>()) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
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
            sky_pipeline,
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
            ui_textured_pipeline,
            ui_textured_vertex_buffer,
            num_ui_vertices: 0,
            num_ui_line_vertices: 0,
            num_ui_textured_vertices: 0,
            game_mode: GameMode::Creative,
            inventory: Inventory::new_creative(),
            recipe_manager: RecipeManager::new(),
            left_mouse_pressed: false,
            mining_target: None,
            mining_progress: 0.0,
            crack_vertex_buffer,
            crack_index_buffer,
            player_state: PlayerState::new(),
            void_damage_timer: 0.0,
            world_time,
            show_debug,
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
                self.camera_uniform.update_view_proj(&self.camera, self.config.width as f32 / self.config.height as f32, self.chunk_manager.render_distance as u32, &self.world_time);
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
        if self.player_state.is_dead {
            return;
        }
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

        // Jump exhaustion check
        let jumped = self.keys.space && self.player_physics.on_ground;
        if jumped && self.game_mode == GameMode::Survival {
            self.player_state.add_exhaustion(0.05);
        }

        let old_pos = self.player_physics.position;

        let fall_damage = self.player_physics.update(dt, &self.chunk_manager, movement);
        self.update_chunks();

        // Apply fall damage
        if self.game_mode == GameMode::Survival && fall_damage > 0.0 {
            self.take_damage(fall_damage, DamageSource::Fall);
        }

        // Movement exhaustion check
        let horizontal_dist = glam::Vec2::new(self.player_physics.position.x - old_pos.x, self.player_physics.position.z - old_pos.z).length();
        if self.game_mode == GameMode::Survival {
            self.player_state.add_exhaustion(0.02 * horizontal_dist);
        }

        // Void damage check
        if self.player_physics.position.y < -64.0 {
            self.void_damage_timer += dt;
            if self.void_damage_timer >= 0.5 {
                self.void_damage_timer = 0.0;
                self.take_damage(2.0, DamageSource::Void);
            }
        } else {
            self.void_damage_timer = 0.0;
        }

        // Update player state timers & starvation
        if let Some((dmg, src)) = self.player_state.update(dt) {
            self.take_damage(dmg, src);
        }

        // Sync camera position to player position at eye height
        self.camera.position = self.player_physics.position + Vec3::new(0.0, 1.6, 0.0);
        self.camera_uniform.update_view_proj(&self.camera, self.config.width as f32 / self.config.height as f32, self.chunk_manager.render_distance as u32, &self.world_time);
        self.queue.write_buffer(&self.camera_buffer, 0, bytemuck::cast_slice(&[self.camera_uniform]));

        // Continuous mining logic
        if self.left_mouse_pressed && self.game_mode == GameMode::Survival {
            let dir = Vec3::new(
                self.camera.yaw.cos() * self.camera.pitch.cos(),
                self.camera.pitch.sin(),
                self.camera.yaw.sin() * self.camera.pitch.cos(),
            ).normalize_or_zero();

            if let Some(hit) = raycast(self.camera.position, dir, 5.0, &self.chunk_manager) {
                let target = hit.block_pos;
                let block = self.chunk_manager.get_block(target.x as i32, target.y as i32, target.z as i32);
                
                if block != BlockType::Air && block.properties().hardness >= 0.0 {
                    if self.mining_target != Some(target) {
                        self.mining_target = Some(target);
                        self.mining_progress = 0.0;
                    } else {
                        let mining_time = self.calculate_mining_time(block);
                        self.mining_progress += dt / mining_time;
                        if self.mining_progress >= 1.0 {
                            let pos = target;
                            self.break_block(pos);
                            self.mining_target = None;
                            self.mining_progress = 0.0;
                        }
                    }
                } else {
                    self.mining_target = None;
                    self.mining_progress = 0.0;
                }
            } else {
                self.mining_target = None;
                self.mining_progress = 0.0;
            }
        } else if !self.left_mouse_pressed {
            self.mining_target = None;
            self.mining_progress = 0.0;
        }
    }

    pub fn update_crack_buffers(&self, target_pos: Vec3, progress: f32) -> Option<(u32, u32)> {
        let stage = (progress * 10.0).floor().clamp(0.0, 9.0) as u32;
        let wx = target_pos.x;
        let wy = target_pos.y;
        let wz = target_pos.z;

        // Cube corner scale (slightly expanded to 1.002 to avoid z-fighting)
        let s = 1.002f32;
        let offset_min = 0.5 - 0.5 * s;
        let offset_max = 0.5 + 0.5 * s;

        let faces = [
            // South
            ([0.0, 0.0, 1.0], [
                ([offset_min, offset_min, offset_max], [0.0, 1.0]),
                ([offset_max, offset_min, offset_max], [1.0, 1.0]),
                ([offset_max, offset_max, offset_max], [1.0, 0.0]),
                ([offset_min, offset_max, offset_max], [0.0, 0.0]),
            ]),
            // North
            ([0.0, 0.0, -1.0], [
                ([offset_max, offset_min, offset_min], [0.0, 1.0]),
                ([offset_min, offset_min, offset_min], [1.0, 1.0]),
                ([offset_min, offset_max, offset_min], [1.0, 0.0]),
                ([offset_max, offset_max, offset_min], [0.0, 0.0]),
            ]),
            // West
            ([-1.0, 0.0, 0.0], [
                ([offset_min, offset_min, offset_min], [0.0, 1.0]),
                ([offset_min, offset_min, offset_max], [1.0, 1.0]),
                ([offset_min, offset_max, offset_max], [1.0, 0.0]),
                ([offset_min, offset_max, offset_min], [0.0, 0.0]),
            ]),
            // East
            ([1.0, 0.0, 0.0], [
                ([offset_max, offset_min, offset_max], [0.0, 1.0]),
                ([offset_max, offset_min, offset_min], [1.0, 1.0]),
                ([offset_max, offset_max, offset_min], [1.0, 0.0]),
                ([offset_max, offset_max, offset_max], [0.0, 0.0]),
            ]),
            // Up
            ([0.0, 1.0, 0.0], [
                ([offset_min, offset_max, offset_max], [0.0, 1.0]),
                ([offset_max, offset_max, offset_max], [1.0, 1.0]),
                ([offset_max, offset_max, offset_min], [1.0, 0.0]),
                ([offset_min, offset_max, offset_min], [0.0, 0.0]),
            ]),
            // Down
            ([0.0, -1.0, 0.0], [
                ([offset_min, offset_min, offset_min], [0.0, 1.0]),
                ([offset_max, offset_min, offset_min], [1.0, 1.0]),
                ([offset_max, offset_min, offset_max], [1.0, 0.0]),
                ([offset_min, offset_min, offset_max], [0.0, 0.0]),
            ]),
        ];

        let mut vertices = Vec::new();
        let mut indices = Vec::new();

        let max_light = self.chunk_manager.get_sky_light(wx as i32, wy as i32, wz as i32)
            .max(self.chunk_manager.get_block_light(wx as i32, wy as i32, wz as i32));
        
        for (face_idx, (_normal, corners)) in faces.iter().enumerate() {
            let start_idx = vertices.len() as u32;
            let multiplier = match face_idx {
                4 => 1.0,
                5 => 0.5,
                _ => 0.8,
            };
            let light_val = (max_light as f32 / 15.0) * multiplier;

            for &(corner, uv) in corners {
                // UV points to Row 15, Col "stage"
                let u = (uv[0] + stage as f32) * 0.0625;
                let v = (uv[1] + 15.0) * 0.0625;
                vertices.push(Vertex {
                    position: [wx + corner[0], wy + corner[1], wz + corner[2]],
                    tex_coords: [u, v],
                    light_level: light_val,
                });
            }

            indices.push(start_idx + 0);
            indices.push(start_idx + 1);
            indices.push(start_idx + 2);
            indices.push(start_idx + 0);
            indices.push(start_idx + 2);
            indices.push(start_idx + 3);
        }

        self.queue.write_buffer(&self.crack_vertex_buffer, 0, bytemuck::cast_slice(&vertices));
        self.queue.write_buffer(&self.crack_index_buffer, 0, bytemuck::cast_slice(&indices));

        Some((vertices.len() as u32, indices.len() as u32))
    }

    pub fn calculate_mining_time(&self, block: BlockType) -> f32 {
        let hardness = block.properties().hardness;
        if hardness < 0.0 {
            return f32::MAX; // Unbreakable (e.g. bedrock)
        }
        
        let held_item = self.inventory.hotbar[self.inventory.selected].map(|s| s.item).unwrap_or(Item::Air);
        let preferred = block.preferred_tool();
        
        let mut speed_multiplier = 1.0;
        let mut matching_tool = false;
        
        if let Some(tool_prop) = held_item.tool_properties() {
            if tool_prop.tool_type == preferred && preferred != ToolType::None {
                speed_multiplier = tool_prop.mining_speed;
                matching_tool = true;
            }
        }
        
        let base_time = if matching_tool || preferred == ToolType::None {
            hardness * 1.5
        } else {
            hardness * 5.0
        };
        
        base_time / speed_multiplier
    }

    pub fn break_block(&mut self, pos: glam::Vec3) {
        let wx = pos.x as i32;
        let wy = pos.y as i32;
        let wz = pos.z as i32;
        let old_block = self.chunk_manager.get_block(wx, wy, wz);
        if old_block == BlockType::Air { return; }

        self.chunk_manager.set_block(wx, wy, wz, BlockType::Air);
        println!("[Debug] Block mined at ({}, {}, {})", wx, wy, wz);

        // Survival drops check
        if self.game_mode == GameMode::Survival {
            let mut eligible_to_harvest = true;
            if let Some(min_material) = old_block.min_harvest_material() {
                let held_item = self.inventory.hotbar[self.inventory.selected].map(|s| s.item).unwrap_or(Item::Air);
                if let Some(tool_prop) = held_item.tool_properties() {
                    eligible_to_harvest = tool_prop.tool_type == old_block.preferred_tool() && tool_prop.material >= min_material;
                } else {
                    eligible_to_harvest = false;
                }
            }

            if eligible_to_harvest {
                if old_block == BlockType::OakLeaves {
                    let mut rng_seed = (wx as u32).wrapping_mul(31).wrapping_add(wy as u32).wrapping_mul(17).wrapping_add(wz as u32);
                    let mut next_rand = || {
                        rng_seed = rng_seed.wrapping_mul(1103515245).wrapping_add(12345);
                        (rng_seed / 65536) % 32768
                    };
                    if next_rand() % 10 == 0 {
                        self.inventory.add_item(crate::inventory::Item::Apple);
                    } else {
                        self.inventory.add_item(crate::inventory::Item::from_block(old_block));
                    }
                } else {
                    self.inventory.add_item(crate::inventory::Item::from_block(old_block));
                }
            }

            self.player_state.add_exhaustion(0.005);

            // Deduct tool durability
            if let Some(stack) = &mut self.inventory.hotbar[self.inventory.selected] {
                if stack.item.tool_properties().is_some() {
                    if stack.durability > 1 {
                        stack.durability -= 1;
                    } else {
                        // Destroy tool
                        println!("[Debug] Tool broke: {:?}", stack.item);
                        self.inventory.hotbar[self.inventory.selected] = None;
                    }
                }
            }
        }

        // recalculate lighting and redraw chunk
        let mut dirty_chunks = std::collections::HashSet::new();
        crate::lighting::update_sky_light_after_removed(&mut self.chunk_manager, wx, wy, wz, &mut dirty_chunks);
        crate::lighting::update_block_light_after_removed(&mut self.chunk_manager, wx, wy, wz, old_block.properties().light_emission, &mut dirty_chunks);

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

    pub fn take_damage(&mut self, amount: f32, source: DamageSource) {
        if self.game_mode == GameMode::Creative {
            return;
        }

        let died = self.player_state.take_damage(amount, source);
        if died {
            println!("[Debug] Player died due to: {:?}", source);
            self.inventory.clear();
            
            // Release cursor grab immediately on death so player can click Respawn
            let _ = self.window.set_cursor_grab(winit::window::CursorGrabMode::None);
            self.window.set_cursor_visible(true);
            self.keys = KeyState::default();
        }
    }

    pub fn respawn(&mut self) {
        // Reset player physics position to spawn point: (8.0, 80.0, 8.0)
        self.player_physics.position = glam::Vec3::new(8.0, 80.0, 8.0);
        self.player_physics.velocity = glam::Vec3::ZERO;
        self.player_physics.on_ground = false;
        self.player_physics.highest_y = 80.0;
        
        // Reset player state
        self.player_state.health = self.player_state.max_health;
        self.player_state.hunger = 20.0;
        self.player_state.saturation = 5.0;
        self.player_state.exhaustion = 0.0;
        self.player_state.is_dead = false;
        self.player_state.death_reason = None;
        self.player_state.invulnerable_time = 1.0; // Give 1.0s invulnerability on respawn
        self.player_state.damaged_flash_time = 0.0;
        self.void_damage_timer = 0.0;
        
        // Grab cursor
        let _ = self.window.set_cursor_grab(winit::window::CursorGrabMode::Locked)
            .or_else(|_| self.window.set_cursor_grab(winit::window::CursorGrabMode::Confined));
        self.window.set_cursor_visible(false);
        
        println!("[Debug] Player respawned at spawn point");
    }

    pub fn handle_death_click(&mut self) {
        let mouse_x = self.mouse_ndc[0];
        let mouse_y = self.mouse_ndc[1];
        
        // Respawn button: bounds X: [-0.3, 0.3], Y: [-0.1, 0.0]
        if mouse_x >= -0.3 && mouse_x <= 0.3 && mouse_y >= -0.1 && mouse_y <= 0.0 {
            self.respawn();
        }
    }

    pub fn handle_click(&mut self, is_left_click: bool) {
        if !is_left_click {
            let held_item = self.inventory.hotbar[self.inventory.selected].map(|s| s.item).unwrap_or(crate::inventory::Item::Air);
            if held_item == crate::inventory::Item::Apple || held_item == crate::inventory::Item::Bread {
                if self.player_state.hunger < 20.0 || self.game_mode == GameMode::Creative {
                    let (heal_hunger, heal_saturation) = match held_item {
                        crate::inventory::Item::Apple => (4.0, 2.4),
                        crate::inventory::Item::Bread => (5.0, 6.0),
                        _ => (0.0, 0.0),
                    };
                    self.player_state.hunger = (self.player_state.hunger + heal_hunger).min(20.0);
                    self.player_state.saturation = (self.player_state.saturation + heal_saturation).min(self.player_state.hunger);
                    
                    let is_creative = self.game_mode == GameMode::Creative;
                    self.inventory.use_selected_item(is_creative);
                    
                    println!("[Debug] Ate {:?}, hunger={:.1}, saturation={:.1}", held_item, self.player_state.hunger, self.player_state.saturation);
                    return;
                }
            }
        }

        let dir = Vec3::new(
            self.camera.yaw.cos() * self.camera.pitch.cos(),
            self.camera.pitch.sin(),
            self.camera.yaw.sin() * self.camera.pitch.cos(),
        ).normalize_or_zero();

        if let Some(hit) = raycast(self.camera.position, dir, 5.0, &self.chunk_manager) {
            let target = if is_left_click {
                hit.block_pos
            } else {
                let clicked_block = self.chunk_manager.get_block(hit.block_pos.x as i32, hit.block_pos.y as i32, hit.block_pos.z as i32);
                if clicked_block == BlockType::CraftingTable {
                    self.inventory.is_table_open = true;
                    self.inventory.craft_input = vec![None; 9];
                    self.open_inventory();
                    return;
                }
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

                    if self.game_mode == GameMode::Survival {
                        self.inventory.add_item(crate::inventory::Item::from_block(old_block));
                    }

                    // Update lighting for removal
                    crate::lighting::update_sky_light_after_removed(&mut self.chunk_manager, wx, wy, wz, &mut dirty_chunks);
                    crate::lighting::update_block_light_after_removed(&mut self.chunk_manager, wx, wy, wz, old_block.properties().light_emission, &mut dirty_chunks);
                }
            } else {
                if let Some(placed_block) = self.inventory.get_selected_block() {
                    self.chunk_manager.set_block(wx, wy, wz, placed_block);

                    let is_creative = self.game_mode == GameMode::Creative;
                    self.inventory.use_selected_item(is_creative);

                    // Update lighting for placement
                    crate::lighting::update_sky_light_after_placed(&mut self.chunk_manager, wx, wy, wz, &mut dirty_chunks);
                    crate::lighting::update_block_light_after_placed(&mut self.chunk_manager, wx, wy, wz, placed_block.properties().light_emission, &mut dirty_chunks);
                } else {
                    return; // No block selected to place
                }
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

    pub fn get_inventory_slots(&self) -> Vec<(SlotType, f32, f32, f32, f32)> {
        let aspect = self.size.width as f32 / self.size.height as f32;
        let slot_w = 0.08;
        let slot_h = 0.08 * aspect;
        let gap = 0.01;
        let mut slots = Vec::new();

        // 1. Hotbar (0..9)
        for i in 0..9 {
            let x0 = -0.40 + i as f32 * (slot_w + gap);
            let y0 = -0.85;
            slots.push((SlotType::Hotbar(i), x0, x0 + slot_w, y0, y0 + slot_h));
        }

        // 2. Backpack (0..27)
        for r in 0..3 {
            for c in 0..9 {
                let i = r * 9 + c;
                let x0 = -0.40 + c as f32 * (slot_w + gap);
                let y0 = -0.70 + r as f32 * (slot_h + gap);
                slots.push((SlotType::Backpack(i), x0, x0 + slot_w, y0, y0 + slot_h));
            }
        }

        // 3. Armor (0..4)
        for i in 0..4 {
            let x0 = -0.40;
            let y0 = -0.15 + i as f32 * (slot_h + gap);
            slots.push((SlotType::Armor(i), x0, x0 + slot_w, y0, y0 + slot_h));
        }

        // 4. Crafting Grid & Output
        if self.inventory.is_table_open {
            // 3x3 table
            let x_start = -0.05;
            for r in 0..3 {
                for c in 0..3 {
                    let i = r * 3 + c;
                    let x0 = x_start + c as f32 * (slot_w + gap);
                    let y0 = -0.10 + r as f32 * (slot_h + gap);
                    slots.push((SlotType::CraftInput(i), x0, x0 + slot_w, y0, y0 + slot_h));
                }
            }
            // Output
            let x0 = x_start + 3.0 * (slot_w + gap) + 0.06;
            let y0 = -0.10 + 1.0 * (slot_h + gap);
            slots.push((SlotType::CraftOutput, x0, x0 + slot_w, y0, y0 + slot_h));
        } else {
            // 2x2 player craft
            let x_start = 0.05;
            for r in 0..2 {
                for c in 0..2 {
                    let i = r * 2 + c;
                    let x0 = x_start + c as f32 * (slot_w + gap);
                    let y0 = -0.05 + r as f32 * (slot_h + gap);
                    slots.push((SlotType::CraftInput(i), x0, x0 + slot_w, y0, y0 + slot_h));
                }
            }
            // Output
            let x0 = x_start + 2.0 * (slot_w + gap) + 0.06;
            let y0 = -0.05 + 0.5 * (slot_h + gap);
            slots.push((SlotType::CraftOutput, x0, x0 + slot_w, y0, y0 + slot_h));
        }

        slots
    }

    pub fn get_item_at_slot(&self, slot: SlotType) -> Option<ItemStack> {
        match slot {
            SlotType::Hotbar(i) => self.inventory.hotbar[i],
            SlotType::Backpack(i) => self.inventory.main[i],
            SlotType::Armor(i) => self.inventory.armor[i],
            SlotType::CraftInput(i) => self.inventory.craft_input.get(i).copied().flatten(),
            SlotType::CraftOutput => self.inventory.craft_output,
        }
    }

    pub fn set_item_at_slot(&mut self, slot: SlotType, stack: Option<ItemStack>) {
        match slot {
            SlotType::Hotbar(i) => self.inventory.hotbar[i] = stack,
            SlotType::Backpack(i) => self.inventory.main[i] = stack,
            SlotType::Armor(i) => self.inventory.armor[i] = stack,
            SlotType::CraftInput(i) => {
                if i < self.inventory.craft_input.len() {
                    self.inventory.craft_input[i] = stack;
                }
            }
            SlotType::CraftOutput => self.inventory.craft_output = stack,
        }
    }

    pub fn handle_inventory_click(&mut self, is_left: bool) {
        let mouse_x = self.mouse_ndc[0];
        let mouse_y = self.mouse_ndc[1];
        let slots = self.get_inventory_slots();
        
        let clicked_slot = slots.into_iter().find(|&(_, x0, x1, y0, y1)| {
            mouse_x >= x0 && mouse_x <= x1 && mouse_y >= y0 && mouse_y <= y1
        });

        if let Some((slot_type, _, _, _, _)) = clicked_slot {
            let slot_item = self.get_item_at_slot(slot_type);

            match slot_type {
                SlotType::CraftOutput => {
                    if let Some(output) = slot_item {
                        // Can only take from output slot
                        let max_stack = output.item.properties().max_stack;
                        if self.inventory.dragged.is_none() {
                            self.inventory.dragged = Some(output);
                            // Consume craft input ingredients
                            for slot in self.inventory.craft_input.iter_mut() {
                                if let Some(stack) = slot {
                                    if stack.count > 1 {
                                        stack.count -= 1;
                                    } else {
                                        *slot = None;
                                    }
                                }
                            }
                            let grid_size = if self.inventory.is_table_open { 3 } else { 2 };
                            self.inventory.craft_output = self.recipe_manager.match_recipe(&self.inventory.craft_input, grid_size);
                        } else if let Some(ref mut dragged) = self.inventory.dragged {
                            if dragged.item == output.item && dragged.count + output.count <= max_stack {
                                dragged.count += output.count;
                                // Consume craft input ingredients
                                for slot in self.inventory.craft_input.iter_mut() {
                                    if let Some(stack) = slot {
                                        if stack.count > 1 {
                                            stack.count -= 1;
                                        } else {
                                            *slot = None;
                                        }
                                    }
                                }
                                let grid_size = if self.inventory.is_table_open { 3 } else { 2 };
                                self.inventory.craft_output = self.recipe_manager.match_recipe(&self.inventory.craft_input, grid_size);
                            }
                        }
                    }
                }
                _ => {
                    // Normal slots (Backpack, Hotbar, Armor, CraftInput)
                    let max_stack = slot_item.map(|s| s.item.properties().max_stack).unwrap_or(64);

                    if is_left {
                        // Left Click interaction
                        if let Some(dragged) = self.inventory.dragged {
                            if let Some(slot) = slot_item {
                                if slot.item == dragged.item {
                                    // Stack them
                                    let space = max_stack.saturating_sub(slot.count);
                                    let transfer = space.min(dragged.count);
                                    let new_slot_count = slot.count + transfer;
                                    let new_drag_count = dragged.count - transfer;

                                    self.set_item_at_slot(slot_type, Some(ItemStack { item: slot.item, count: new_slot_count, durability: slot.durability }));
                                    if new_drag_count > 0 {
                                        self.inventory.dragged = Some(ItemStack { item: dragged.item, count: new_drag_count, durability: dragged.durability });
                                    } else {
                                        self.inventory.dragged = None;
                                    }
                                } else {
                                    // Swap slot and dragged
                                    self.set_item_at_slot(slot_type, Some(dragged));
                                    self.inventory.dragged = Some(slot);
                                }
                            } else {
                                // Put dragged in empty slot
                                self.set_item_at_slot(slot_type, Some(dragged));
                                self.inventory.dragged = None;
                            }
                        } else {
                            // Pickup entire slot
                            if let Some(slot) = slot_item {
                                self.inventory.dragged = Some(slot);
                                self.set_item_at_slot(slot_type, None);
                            }
                        }
                    } else {
                        // Right Click interaction
                        if let Some(dragged) = self.inventory.dragged {
                            if let Some(slot) = slot_item {
                                if slot.item == dragged.item && slot.count < max_stack {
                                    // Drop 1
                                    self.set_item_at_slot(slot_type, Some(ItemStack { item: slot.item, count: slot.count + 1, durability: slot.durability }));
                                    if dragged.count > 1 {
                                        self.inventory.dragged = Some(ItemStack { item: dragged.item, count: dragged.count - 1, durability: dragged.durability });
                                    } else {
                                        self.inventory.dragged = None;
                                    }
                                } else if slot.item != dragged.item {
                                    // Swap (like left click swap)
                                    self.set_item_at_slot(slot_type, Some(dragged));
                                    self.inventory.dragged = Some(slot);
                                }
                            } else {
                                // Drop 1 in empty slot
                                self.set_item_at_slot(slot_type, Some(ItemStack { item: dragged.item, count: 1, durability: dragged.durability }));
                                if dragged.count > 1 {
                                    self.inventory.dragged = Some(ItemStack { item: dragged.item, count: dragged.count - 1, durability: dragged.durability });
                                } else {
                                    self.inventory.dragged = None;
                                }
                            }
                        } else {
                            // Split stack in slot
                            if let Some(slot) = slot_item {
                                let take = (slot.count + 1) / 2;
                                let keep = slot.count - take;
                                self.inventory.dragged = Some(ItemStack { item: slot.item, count: take, durability: slot.durability });
                                if keep > 0 {
                                    self.set_item_at_slot(slot_type, Some(ItemStack { item: slot.item, count: keep, durability: slot.durability }));
                                } else {
                                    self.set_item_at_slot(slot_type, None);
                                }
                            }
                        }
                    }

                    // If we clicked a craft input slot, recalculate craft output
                    if let SlotType::CraftInput(_) = slot_type {
                        let grid_size = if self.inventory.is_table_open { 3 } else { 2 };
                        self.inventory.craft_output = self.recipe_manager.match_recipe(&self.inventory.craft_input, grid_size);
                    }
                }
            }
        }
    }

    pub fn open_inventory(&mut self) {
        self.inventory.is_open = true;
        // Release cursor grab
        let _ = self.window.set_cursor_grab(winit::window::CursorGrabMode::None);
        self.window.set_cursor_visible(true);
        self.keys = KeyState::default();
    }

    pub fn close_inventory(&mut self) {
        self.inventory.is_open = false;
        // Return craft input items
        let inputs: Vec<ItemStack> = self.inventory.craft_input.iter_mut()
            .filter_map(|slot| slot.take())
            .collect();
        for stack in inputs {
            for _ in 0..stack.count {
                self.inventory.add_item(stack.item);
            }
        }
        // Also return dragged item if any
        if let Some(dragged) = self.inventory.dragged.take() {
            for _ in 0..dragged.count {
                self.inventory.add_item(dragged.item);
            }
        }
        
        self.inventory.is_table_open = false;
        self.inventory.craft_input = vec![None; 4];
        self.inventory.craft_output = None;
        
        // Re-lock cursor
        let _ = self.window.set_cursor_grab(winit::window::CursorGrabMode::Locked)
            .or_else(|_| self.window.set_cursor_grab(winit::window::CursorGrabMode::Confined));
        self.window.set_cursor_visible(false);
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

        if self.player_state.is_dead {
            let mut ui_vertices = Vec::new();
            let mut ui_line_vertices = Vec::new();

            let mouse_x = self.mouse_ndc[0];
            let mouse_y = self.mouse_ndc[1];

            // Respawn button hover (X: [-0.3, 0.3], Y: [-0.1, 0.0])
            let respawn_hover = mouse_x >= -0.3 && mouse_x <= 0.3 && mouse_y >= -0.1 && mouse_y <= 0.0;

            // Reddish overlay
            let bg_color = [0.4, 0.0, 0.0, 0.6];
            ui_vertices.push(UiVertex { position: [-1.0, 1.0, 0.0], color: bg_color });
            ui_vertices.push(UiVertex { position: [-1.0, -1.0, 0.0], color: bg_color });
            ui_vertices.push(UiVertex { position: [1.0, -1.0, 0.0], color: bg_color });
            ui_vertices.push(UiVertex { position: [-1.0, 1.0, 0.0], color: bg_color });
            ui_vertices.push(UiVertex { position: [1.0, -1.0, 0.0], color: bg_color });
            ui_vertices.push(UiVertex { position: [1.0, 1.0, 0.0], color: bg_color });

            // Button background
            let btn_bg = if respawn_hover { [0.4, 0.1, 0.1, 1.0] } else { [0.2, 0.0, 0.0, 1.0] };
            let btn_border = if respawn_hover { [1.0, 1.0, 1.0, 1.0] } else { [0.6, 0.2, 0.2, 1.0] };
            let btn_y_min = -0.10;
            let btn_y_max = 0.00;

            ui_vertices.push(UiVertex { position: [-0.3, btn_y_max, 0.0], color: btn_bg });
            ui_vertices.push(UiVertex { position: [-0.3, btn_y_min, 0.0], color: btn_bg });
            ui_vertices.push(UiVertex { position: [0.3, btn_y_min, 0.0], color: btn_bg });
            ui_vertices.push(UiVertex { position: [-0.3, btn_y_max, 0.0], color: btn_bg });
            ui_vertices.push(UiVertex { position: [0.3, btn_y_min, 0.0], color: btn_bg });
            ui_vertices.push(UiVertex { position: [0.3, btn_y_max, 0.0], color: btn_bg });

            // Button border
            ui_line_vertices.push(UiVertex { position: [-0.3, btn_y_max, 0.0], color: btn_border });
            ui_line_vertices.push(UiVertex { position: [0.3, btn_y_max, 0.0], color: btn_border });
            ui_line_vertices.push(UiVertex { position: [0.3, btn_y_max, 0.0], color: btn_border });
            ui_line_vertices.push(UiVertex { position: [0.3, btn_y_min, 0.0], color: btn_border });
            ui_line_vertices.push(UiVertex { position: [0.3, btn_y_min, 0.0], color: btn_border });
            ui_line_vertices.push(UiVertex { position: [-0.3, btn_y_min, 0.0], color: btn_border });
            ui_line_vertices.push(UiVertex { position: [-0.3, btn_y_min, 0.0], color: btn_border });
            ui_line_vertices.push(UiVertex { position: [-0.3, btn_y_max, 0.0], color: btn_border });

            let draw_centered_text = |s: &str, y: f32, char_w: f32, char_h: f32, spacing: f32, color: [f32; 4], vertices: &mut Vec<UiVertex>| {
                let upper = s.to_uppercase();
                let n = upper.len() as f32;
                let width = n * char_w + (n - 1.0) * spacing;
                let start_x = -width / 2.0;
                add_string_lines(&upper, start_x, y, char_w, char_h, spacing, color, vertices);
            };

            draw_centered_text("YOU DIED!", 0.30, 0.04, 0.08, 0.015, [1.0, 0.2, 0.2, 1.0], &mut ui_line_vertices);

            let msg = match self.player_state.death_reason {
                Some(DamageSource::Fall) => "FELL FROM A HIGH PLACE",
                Some(DamageSource::Void) => "FELL INTO THE VOID",
                Some(DamageSource::Hunger) => "STARVED TO DEATH",
                None => "DIED",
            };
            draw_centered_text(msg, 0.15, 0.015, 0.03, 0.006, [1.0, 1.0, 1.0, 1.0], &mut ui_line_vertices);
            draw_centered_text("RESPAWN", -0.06, 0.02, 0.04, 0.008, [1.0, 1.0, 1.0, 1.0], &mut ui_line_vertices);

            let ui_vert_len = ui_vertices.len().min(4096);
            let ui_line_vert_len = ui_line_vertices.len().min(4096);

            self.queue.write_buffer(&self.ui_vertex_buffer, 0, bytemuck::cast_slice(&ui_vertices[..ui_vert_len]));
            self.queue.write_buffer(&self.ui_line_vertex_buffer, 0, bytemuck::cast_slice(&ui_line_vertices[..ui_line_vert_len]));

            self.num_ui_vertices = ui_vert_len as u32;
            self.num_ui_line_vertices = ui_line_vert_len as u32;
            self.num_ui_textured_vertices = 0;
        } else if self.is_paused {
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

            // Cap the sizes to the preallocated buffers (4096 vertices)
            let ui_vert_len = ui_vertices.len().min(4096);
            let ui_line_vert_len = ui_line_vertices.len().min(4096);

            self.queue.write_buffer(&self.ui_vertex_buffer, 0, bytemuck::cast_slice(&ui_vertices[..ui_vert_len]));
            self.queue.write_buffer(&self.ui_line_vertex_buffer, 0, bytemuck::cast_slice(&ui_line_vertices[..ui_line_vert_len]));

            self.num_ui_vertices = ui_vert_len as u32;
            self.num_ui_line_vertices = ui_line_vert_len as u32;
        } else {
            let mut ui_vertices = Vec::new();
            let mut ui_line_vertices = Vec::new();
            let mut ui_textured_vertices = Vec::new();

            let aspect = self.size.width as f32 / self.size.height as f32;
            let slot_w = 0.08;
            let slot_h = 0.08 * aspect;
            let gap = 0.01;
            let start_x = -0.40;

            let draw_durability_bar = |stack: &ItemStack, x0: f32, x1: f32, y0: f32, y1: f32, _aspect: f32, ui_vertices: &mut Vec<UiVertex>| {
                if let Some(tool_prop) = stack.item.tool_properties() {
                    let max_dur = tool_prop.durability;
                    if stack.durability < max_dur {
                        let ratio = (stack.durability as f32 / max_dur as f32).clamp(0.0, 1.0);
                        
                        // Define bar bounds relative to slot size
                        let slot_w = x1 - x0;
                        let slot_h = y1 - y0;
                        
                        let bar_x0 = x0 + slot_w * 0.15;
                        let bar_x1 = x1 - slot_w * 0.15;
                        let bar_y0 = y0 + slot_h * 0.10;
                        let bar_y1 = y0 + slot_h * 0.16;
                        
                        // 1. Black background bar
                        let bg_color = [0.0, 0.0, 0.0, 1.0];
                        ui_vertices.push(UiVertex { position: [bar_x0, bar_y1, 0.0], color: bg_color });
                        ui_vertices.push(UiVertex { position: [bar_x0, bar_y0, 0.0], color: bg_color });
                        ui_vertices.push(UiVertex { position: [bar_x1, bar_y0, 0.0], color: bg_color });
                        ui_vertices.push(UiVertex { position: [bar_x0, bar_y1, 0.0], color: bg_color });
                        ui_vertices.push(UiVertex { position: [bar_x1, bar_y0, 0.0], color: bg_color });
                        ui_vertices.push(UiVertex { position: [bar_x1, bar_y1, 0.0], color: bg_color });
                        
                        // 2. Colored foreground bar
                        let fg_x1 = bar_x0 + (bar_x1 - bar_x0) * ratio;
                        let (r, g) = if ratio > 0.5 {
                            ((1.0 - ratio) * 2.0, 1.0)
                        } else {
                            (1.0, ratio * 2.0)
                        };
                        let fg_color = [r, g, 0.0, 1.0];
                        
                        ui_vertices.push(UiVertex { position: [bar_x0, bar_y1, 0.0], color: fg_color });
                        ui_vertices.push(UiVertex { position: [bar_x0, bar_y0, 0.0], color: fg_color });
                        ui_vertices.push(UiVertex { position: [fg_x1, bar_y0, 0.0], color: fg_color });
                        ui_vertices.push(UiVertex { position: [bar_x0, bar_y1, 0.0], color: fg_color });
                        ui_vertices.push(UiVertex { position: [fg_x1, bar_y0, 0.0], color: fg_color });
                        ui_vertices.push(UiVertex { position: [fg_x1, bar_y1, 0.0], color: fg_color });
                    }
                }
            };

            if self.inventory.is_open {
                // 1. Dark overlay (screen covers from -1.0 to 1.0)
                let bg_color = [0.08, 0.08, 0.08, 0.6];
                ui_vertices.push(UiVertex { position: [-1.0, 1.0, 0.0], color: bg_color });
                ui_vertices.push(UiVertex { position: [-1.0, -1.0, 0.0], color: bg_color });
                ui_vertices.push(UiVertex { position: [1.0, -1.0, 0.0], color: bg_color });
                ui_vertices.push(UiVertex { position: [-1.0, 1.0, 0.0], color: bg_color });
                ui_vertices.push(UiVertex { position: [1.0, -1.0, 0.0], color: bg_color });
                ui_vertices.push(UiVertex { position: [1.0, 1.0, 0.0], color: bg_color });

                // 2. Draw slots
                let slots = self.get_inventory_slots();
                let mouse_x = self.mouse_ndc[0];
                let mouse_y = self.mouse_ndc[1];
                let mut hovered_slot = None;

                for &(slot_type, x0, x1, y0, y1) in &slots {
                    let is_hovered = mouse_x >= x0 && mouse_x <= x1 && mouse_y >= y0 && mouse_y <= y1;
                    if is_hovered {
                        hovered_slot = Some((slot_type, x0, x1, y0, y1));
                    }

                    // Background Quad
                    let slot_bg_color = if is_hovered {
                        [0.35, 0.35, 0.35, 0.8]
                    } else {
                        [0.15, 0.15, 0.15, 0.8]
                    };
                    ui_vertices.push(UiVertex { position: [x0, y1, 0.0], color: slot_bg_color });
                    ui_vertices.push(UiVertex { position: [x0, y0, 0.0], color: slot_bg_color });
                    ui_vertices.push(UiVertex { position: [x1, y0, 0.0], color: slot_bg_color });
                    ui_vertices.push(UiVertex { position: [x0, y1, 0.0], color: slot_bg_color });
                    ui_vertices.push(UiVertex { position: [x1, y0, 0.0], color: slot_bg_color });
                    ui_vertices.push(UiVertex { position: [x1, y1, 0.0], color: slot_bg_color });

                    // Borders
                    let border_color = match slot_type {
                        SlotType::Hotbar(idx) if idx == self.inventory.selected => [1.0, 1.0, 1.0, 1.0],
                        _ => [0.3, 0.3, 0.3, 0.8],
                    };
                    ui_line_vertices.push(UiVertex { position: [x0, y1, 0.0], color: border_color });
                    ui_line_vertices.push(UiVertex { position: [x1, y1, 0.0], color: border_color });
                    ui_line_vertices.push(UiVertex { position: [x1, y1, 0.0], color: border_color });
                    ui_line_vertices.push(UiVertex { position: [x1, y0, 0.0], color: border_color });
                    ui_line_vertices.push(UiVertex { position: [x1, y0, 0.0], color: border_color });
                    ui_line_vertices.push(UiVertex { position: [x0, y0, 0.0], color: border_color });
                    ui_line_vertices.push(UiVertex { position: [x0, y0, 0.0], color: border_color });
                    ui_line_vertices.push(UiVertex { position: [x0, y1, 0.0], color: border_color });

                    // Slot Item
                    if let Some(stack) = self.get_item_at_slot(slot_type) {
                        let (col, row) = stack.item.properties().tex_coords;
                        let u0 = col as f32 * 0.0625;
                        let u1 = (col + 1) as f32 * 0.0625;
                        let v0 = row as f32 * 0.0625;
                        let v1 = (row + 1) as f32 * 0.0625;

                        let margin_x = 0.015;
                        let margin_y = 0.015 * aspect;
                        let tx0 = x0 + margin_x;
                        let tx1 = x1 - margin_x;
                        let ty0 = y0 + margin_y;
                        let ty1 = y1 - margin_y;

                        let c = [1.0, 1.0, 1.0, 1.0];
                        ui_textured_vertices.push(TexturedUiVertex { position: [tx0, ty1, 0.0], tex_coords: [u0, v0], color: c });
                        ui_textured_vertices.push(TexturedUiVertex { position: [tx0, ty0, 0.0], tex_coords: [u0, v1], color: c });
                        ui_textured_vertices.push(TexturedUiVertex { position: [tx1, ty0, 0.0], tex_coords: [u1, v1], color: c });
                        ui_textured_vertices.push(TexturedUiVertex { position: [tx0, ty1, 0.0], tex_coords: [u0, v0], color: c });
                        ui_textured_vertices.push(TexturedUiVertex { position: [tx1, ty0, 0.0], tex_coords: [u1, v1], color: c });
                        ui_textured_vertices.push(TexturedUiVertex { position: [tx1, ty1, 0.0], tex_coords: [u1, v0], color: c });

                        if stack.count > 1 {
                            let count_str = format!("{}", stack.count);
                            let cw = 0.008;
                            let ch = 0.016;
                            let cs = 0.003;
                            let n_chars = count_str.len() as f32;
                            let count_w = n_chars * cw + (n_chars - 1.0) * cs;
                            let count_x = x1 - count_w - 0.008;
                            let count_y = y0 + 0.01 * aspect;
                            add_string_lines(&count_str, count_x, count_y, cw, ch, cs, [1.0, 1.0, 1.0, 1.0], &mut ui_line_vertices);
                        }

                        // Draw durability bar
                        draw_durability_bar(&stack, x0, x1, y0, y1, aspect, &mut ui_vertices);
                    }
                }

                // 3. Draw crafting arrow symbol
                let arrow_y = if self.inventory.is_table_open {
                    -0.10 + 1.0 * (slot_h + gap) + slot_h / 2.0
                } else {
                    -0.05 + 0.5 * (slot_h + gap) + slot_h / 2.0
                };
                let arrow_x = if self.inventory.is_table_open {
                    -0.05 + 3.0 * (slot_w + gap) + 0.015
                } else {
                    0.05 + 2.0 * (slot_w + gap) + 0.015
                };
                let ac = [0.8, 0.8, 0.8, 1.0];
                ui_line_vertices.push(UiVertex { position: [arrow_x, arrow_y, 0.0], color: ac });
                ui_line_vertices.push(UiVertex { position: [arrow_x + 0.03, arrow_y, 0.0], color: ac });
                ui_line_vertices.push(UiVertex { position: [arrow_x + 0.03, arrow_y, 0.0], color: ac });
                ui_line_vertices.push(UiVertex { position: [arrow_x + 0.02, arrow_y + 0.01 * aspect, 0.0], color: ac });
                ui_line_vertices.push(UiVertex { position: [arrow_x + 0.03, arrow_y, 0.0], color: ac });
                ui_line_vertices.push(UiVertex { position: [arrow_x + 0.02, arrow_y - 0.01 * aspect, 0.0], color: ac });

                // 4. Draw texts (Labels)
                add_string_lines("INVENTORY", -0.40, -0.70 + 3.0 * (slot_h + gap) + 0.02, 0.008, 0.016, 0.003, [1.0, 1.0, 1.0, 1.0], &mut ui_line_vertices);
                let craft_lbl_x = if self.inventory.is_table_open { -0.05 } else { 0.05 };
                let craft_lbl_y = if self.inventory.is_table_open {
                    -0.10 + 3.0 * (slot_h + gap) + 0.02
                } else {
                    -0.05 + 2.0 * (slot_h + gap) + 0.02
                };
                add_string_lines("CRAFTING", craft_lbl_x, craft_lbl_y, 0.008, 0.016, 0.003, [1.0, 1.0, 1.0, 1.0], &mut ui_line_vertices);

                // 5. Draw dragged item at cursor position
                if let Some(dragged) = self.inventory.dragged {
                    let (col, row) = dragged.item.properties().tex_coords;
                    let u0 = col as f32 * 0.0625;
                    let u1 = (col + 1) as f32 * 0.0625;
                    let v0 = row as f32 * 0.0625;
                    let v1 = (row + 1) as f32 * 0.0625;

                    let dx0 = mouse_x - slot_w / 2.0 + 0.015;
                    let dx1 = mouse_x + slot_w / 2.0 - 0.015;
                    let dy0 = mouse_y - slot_h / 2.0 + 0.015 * aspect;
                    let dy1 = mouse_y + slot_h / 2.0 - 0.015 * aspect;

                    let c = [1.0, 1.0, 1.0, 1.0];
                    ui_textured_vertices.push(TexturedUiVertex { position: [dx0, dy1, 0.0], tex_coords: [u0, v0], color: c });
                    ui_textured_vertices.push(TexturedUiVertex { position: [dx0, dy0, 0.0], tex_coords: [u0, v1], color: c });
                    ui_textured_vertices.push(TexturedUiVertex { position: [dx1, dy0, 0.0], tex_coords: [u1, v1], color: c });
                    ui_textured_vertices.push(TexturedUiVertex { position: [dx0, dy1, 0.0], tex_coords: [u0, v0], color: c });
                    ui_textured_vertices.push(TexturedUiVertex { position: [dx1, dy0, 0.0], tex_coords: [u1, v1], color: c });
                    ui_textured_vertices.push(TexturedUiVertex { position: [dx1, dy1, 0.0], tex_coords: [u1, v0], color: c });

                    if dragged.count > 1 {
                        let count_str = format!("{}", dragged.count);
                        let cw = 0.008;
                        let ch = 0.016;
                        let cs = 0.003;
                        let n_chars = count_str.len() as f32;
                        let count_w = n_chars * cw + (n_chars - 1.0) * cs;
                        let count_x = mouse_x + slot_w / 2.0 - count_w - 0.008;
                        let count_y = mouse_y - slot_h / 2.0 + 0.01 * aspect;
                        add_string_lines(&count_str, count_x, count_y, cw, ch, cs, [1.0, 1.0, 1.0, 1.0], &mut ui_line_vertices);
                    }
                }

                // 6. Draw tooltip for hovered slot
                if self.inventory.dragged.is_none() {
                    if let Some((slot_type, _, _, _, _)) = hovered_slot {
                        if let Some(stack) = self.get_item_at_slot(slot_type) {
                            let name = stack.item.properties().name;
                            let tw = name.len() as f32 * 0.014 + 0.02;
                            let th = 0.035 * aspect;
                            let tx = mouse_x + 0.02;
                            let ty = mouse_y + 0.02;

                            let tt_bg = [0.05, 0.05, 0.1, 0.95];
                            ui_vertices.push(UiVertex { position: [tx, ty + th, 0.0], color: tt_bg });
                            ui_vertices.push(UiVertex { position: [tx, ty, 0.0], color: tt_bg });
                            ui_vertices.push(UiVertex { position: [tx + tw, ty, 0.0], color: tt_bg });
                            ui_vertices.push(UiVertex { position: [tx, ty + th, 0.0], color: tt_bg });
                            ui_vertices.push(UiVertex { position: [tx + tw, ty, 0.0], color: tt_bg });
                            ui_vertices.push(UiVertex { position: [tx + tw, ty + th, 0.0], color: tt_bg });

                            let tt_border = [0.3, 0.3, 0.7, 1.0];
                            ui_line_vertices.push(UiVertex { position: [tx, ty + th, 0.0], color: tt_border });
                            ui_line_vertices.push(UiVertex { position: [tx + tw, ty + th, 0.0], color: tt_border });
                            ui_line_vertices.push(UiVertex { position: [tx + tw, ty + th, 0.0], color: tt_border });
                            ui_line_vertices.push(UiVertex { position: [tx + tw, ty, 0.0], color: tt_border });
                            ui_line_vertices.push(UiVertex { position: [tx + tw, ty, 0.0], color: tt_border });
                            ui_line_vertices.push(UiVertex { position: [tx, ty, 0.0], color: tt_border });
                            ui_line_vertices.push(UiVertex { position: [tx, ty, 0.0], color: tt_border });
                            ui_line_vertices.push(UiVertex { position: [tx, ty + th, 0.0], color: tt_border });

                            add_string_lines(name, tx + 0.01, ty + 0.01 * aspect, 0.008, 0.016, 0.003, [1.0, 1.0, 1.0, 1.0], &mut ui_line_vertices);
                        }
                    }
                }
            } else {
                // Background Bar
                let bg_color = [0.05, 0.05, 0.05, 0.6];
                let bg_x0 = -0.415;
                let bg_x1 = 0.415;
                let bg_y0 = -0.96;
                let bg_y1 = -0.94 + slot_h;
                ui_vertices.push(UiVertex { position: [bg_x0, bg_y1, 0.0], color: bg_color });
                ui_vertices.push(UiVertex { position: [bg_x0, bg_y0, 0.0], color: bg_color });
                ui_vertices.push(UiVertex { position: [bg_x1, bg_y0, 0.0], color: bg_color });
                ui_vertices.push(UiVertex { position: [bg_x0, bg_y1, 0.0], color: bg_color });
                ui_vertices.push(UiVertex { position: [bg_x1, bg_y0, 0.0], color: bg_color });
                ui_vertices.push(UiVertex { position: [bg_x1, bg_y1, 0.0], color: bg_color });

                // Slots
                for i in 0..9 {
                    let x0 = start_x + i as f32 * (slot_w + gap);
                    let x1 = x0 + slot_w;
                    let y0 = -0.95;
                    let y1 = y0 + slot_h;

                    let border_color = if i == self.inventory.selected {
                        [1.0, 1.0, 1.0, 1.0] // White for active
                    } else {
                        [0.3, 0.3, 0.3, 0.8] // Gray for inactive
                    };

                    // Push lines to ui_line_vertices (forms border box)
                    ui_line_vertices.push(UiVertex { position: [x0, y1, 0.0], color: border_color });
                    ui_line_vertices.push(UiVertex { position: [x1, y1, 0.0], color: border_color });
                    ui_line_vertices.push(UiVertex { position: [x1, y1, 0.0], color: border_color });
                    ui_line_vertices.push(UiVertex { position: [x1, y0, 0.0], color: border_color });
                    ui_line_vertices.push(UiVertex { position: [x1, y0, 0.0], color: border_color });
                    ui_line_vertices.push(UiVertex { position: [x0, y0, 0.0], color: border_color });
                    ui_line_vertices.push(UiVertex { position: [x0, y0, 0.0], color: border_color });
                    ui_line_vertices.push(UiVertex { position: [x0, y1, 0.0], color: border_color });

                    if let Some(stack) = &self.inventory.hotbar[i] {
                        let (col, row) = stack.item.properties().tex_coords;
                        let u0 = col as f32 * 0.0625;
                        let u1 = (col + 1) as f32 * 0.0625;
                        let v0 = row as f32 * 0.0625;
                        let v1 = (row + 1) as f32 * 0.0625;

                        let margin_x = 0.015;
                        let margin_y = 0.015 * aspect;
                        let tx0 = x0 + margin_x;
                        let tx1 = x1 - margin_x;
                        let ty0 = y0 + margin_y;
                        let ty1 = y1 - margin_y;

                        let c = [1.0, 1.0, 1.0, 1.0];
                        ui_textured_vertices.push(TexturedUiVertex { position: [tx0, ty1, 0.0], tex_coords: [u0, v0], color: c });
                        ui_textured_vertices.push(TexturedUiVertex { position: [tx0, ty0, 0.0], tex_coords: [u0, v1], color: c });
                        ui_textured_vertices.push(TexturedUiVertex { position: [tx1, ty0, 0.0], tex_coords: [u1, v1], color: c });
                        ui_textured_vertices.push(TexturedUiVertex { position: [tx0, ty1, 0.0], tex_coords: [u0, v0], color: c });
                        ui_textured_vertices.push(TexturedUiVertex { position: [tx1, ty0, 0.0], tex_coords: [u1, v1], color: c });
                        ui_textured_vertices.push(TexturedUiVertex { position: [tx1, ty1, 0.0], tex_coords: [u1, v0], color: c });

                        if stack.count > 1 {
                            let count_str = format!("{}", stack.count);
                            let cw = 0.008;
                            let ch = 0.016;
                            let cs = 0.003;
                            let n_chars = count_str.len() as f32;
                            let count_w = n_chars * cw + (n_chars - 1.0) * cs;
                            let count_x = x1 - count_w - 0.01;
                            let count_y = y0 + 0.012 * aspect;
                            add_string_lines(&count_str, count_x, count_y, cw, ch, cs, [1.0, 1.0, 1.0, 1.0], &mut ui_line_vertices);
                        }

                        // Draw durability bar
                        draw_durability_bar(stack, x0, x1, y0, y1, aspect, &mut ui_vertices);
                    }
                }

                if self.game_mode == GameMode::Survival {
                    // Draw Health HUD
                    let hud_w = 0.03;
                    let hud_h = 0.03 * aspect;
                    let hud_gap = 0.005;
                    let x_hearts_start = -0.38;
                    let y_hud = -0.76;
                    
                    for i in 0..10 {
                        let h_val = self.player_state.health;
                        let (col, row) = if h_val >= 2.0 * (i + 1) as f32 {
                            (0, 8) // Full
                        } else if h_val >= 2.0 * i as f32 + 1.0 {
                            (1, 8) // Half
                        } else {
                            (2, 8) // Empty
                        };
                        
                        let u0 = col as f32 * 0.0625;
                        let u1 = (col + 1) as f32 * 0.0625;
                        let v0 = row as f32 * 0.0625;
                        let v1 = (row + 1) as f32 * 0.0625;
                        
                        let hx0 = x_hearts_start + i as f32 * (hud_w + hud_gap);
                        let hx1 = hx0 + hud_w;
                        let hy0 = y_hud;
                        let hy1 = hy0 + hud_h;
                        
                        let c = [1.0, 1.0, 1.0, 1.0];
                        ui_textured_vertices.push(TexturedUiVertex { position: [hx0, hy1, 0.0], tex_coords: [u0, v0], color: c });
                        ui_textured_vertices.push(TexturedUiVertex { position: [hx0, hy0, 0.0], tex_coords: [u0, v1], color: c });
                        ui_textured_vertices.push(TexturedUiVertex { position: [hx1, hy0, 0.0], tex_coords: [u1, v1], color: c });
                        ui_textured_vertices.push(TexturedUiVertex { position: [hx0, hy1, 0.0], tex_coords: [u0, v0], color: c });
                        ui_textured_vertices.push(TexturedUiVertex { position: [hx1, hy0, 0.0], tex_coords: [u1, v1], color: c });
                        ui_textured_vertices.push(TexturedUiVertex { position: [hx1, hy1, 0.0], tex_coords: [u1, v0], color: c });
                    }
                    
                    // Draw Hunger HUD
                    let x_hunger_start = 0.38 - 10.0 * hud_w - 9.0 * hud_gap;
                    for i in 0..10 {
                        let hung_val = self.player_state.hunger;
                        let (col, row) = if hung_val >= 2.0 * (i + 1) as f32 {
                            (3, 8) // Full
                        } else if hung_val >= 2.0 * i as f32 + 1.0 {
                            (4, 8) // Half
                        } else {
                            (5, 8) // Empty
                        };
                        
                        let u0 = col as f32 * 0.0625;
                        let u1 = (col + 1) as f32 * 0.0625;
                        let v0 = row as f32 * 0.0625;
                        let v1 = (row + 1) as f32 * 0.0625;
                        
                        let hx0 = x_hunger_start + i as f32 * (hud_w + hud_gap);
                        let hx1 = hx0 + hud_w;
                        let hy0 = y_hud;
                        let hy1 = hy0 + hud_h;
                        
                        let c = [1.0, 1.0, 1.0, 1.0];
                        ui_textured_vertices.push(TexturedUiVertex { position: [hx0, hy1, 0.0], tex_coords: [u0, v0], color: c });
                        ui_textured_vertices.push(TexturedUiVertex { position: [hx0, hy0, 0.0], tex_coords: [u0, v1], color: c });
                        ui_textured_vertices.push(TexturedUiVertex { position: [hx1, hy0, 0.0], tex_coords: [u1, v1], color: c });
                        ui_textured_vertices.push(TexturedUiVertex { position: [hx0, hy1, 0.0], tex_coords: [u0, v0], color: c });
                        ui_textured_vertices.push(TexturedUiVertex { position: [hx1, hy0, 0.0], tex_coords: [u1, v1], color: c });
                        ui_textured_vertices.push(TexturedUiVertex { position: [hx1, hy1, 0.0], tex_coords: [u1, v0], color: c });
                    }
                }

                // Selected Block/Item Text
                let selected_item = self.inventory.hotbar[self.inventory.selected].map(|s| s.item).unwrap_or(crate::inventory::Item::Air);
                let selected_text = format!("{:?}", selected_item).to_uppercase();
                let char_w = 0.010;
                let char_h = 0.020;
                let spacing = 0.004;
                let n = selected_text.len() as f32;
                let width = n * char_w + (n - 1.0) * spacing;
                let text_x = -width / 2.0;
                add_string_lines(&selected_text, text_x, -0.78, char_w, char_h, spacing, [1.0, 1.0, 1.0, 1.0], &mut ui_line_vertices);

                // Game Mode Status Text
                let mode_text = match self.game_mode {
                    GameMode::Creative => "CREATIVE MODE",
                    GameMode::Survival => "SURVIVAL MODE",
                };
                let mode_w = 0.009;
                let mode_h = 0.018;
                let mode_s = 0.003;
                let n_mode = mode_text.len() as f32;
                let width_mode = n_mode * mode_w + (n_mode - 1.0) * mode_s;
                let mode_x = -width_mode / 2.0;
                add_string_lines(mode_text, mode_x, -0.71, mode_w, mode_h, mode_s, [1.0, 0.9, 0.4, 1.0], &mut ui_line_vertices);

                // Damaged screen red flash overlay
                if self.player_state.damaged_flash_time > 0.0 {
                    let alpha = (self.player_state.damaged_flash_time / 0.5).min(1.0) * 0.25;
                    let flash_color = [1.0, 0.0, 0.0, alpha];
                    ui_vertices.push(UiVertex { position: [-1.0, 1.0, 0.0], color: flash_color });
                    ui_vertices.push(UiVertex { position: [-1.0, -1.0, 0.0], color: flash_color });
                    ui_vertices.push(UiVertex { position: [1.0, -1.0, 0.0], color: flash_color });
                    ui_vertices.push(UiVertex { position: [-1.0, 1.0, 0.0], color: flash_color });
                    ui_vertices.push(UiVertex { position: [1.0, -1.0, 0.0], color: flash_color });
                    ui_vertices.push(UiVertex { position: [1.0, 1.0, 0.0], color: flash_color });
                }
            }

            // Write Buffers
            let ui_vert_len = ui_vertices.len().min(4096);
            let ui_line_vert_len = ui_line_vertices.len().min(4096);
            let ui_textured_vert_len = ui_textured_vertices.len().min(4096);

            self.queue.write_buffer(&self.ui_vertex_buffer, 0, bytemuck::cast_slice(&ui_vertices[..ui_vert_len]));
            self.queue.write_buffer(&self.ui_line_vertex_buffer, 0, bytemuck::cast_slice(&ui_line_vertices[..ui_line_vert_len]));
            self.queue.write_buffer(&self.ui_textured_vertex_buffer, 0, bytemuck::cast_slice(&ui_textured_vertices[..ui_textured_vert_len]));

            self.num_ui_vertices = ui_vert_len as u32;
            self.num_ui_line_vertices = ui_line_vert_len as u32;
            self.num_ui_textured_vertices = ui_textured_vert_len as u32;
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

            // Draw Skybox first
            render_pass.set_pipeline(&self.sky_pipeline);
            render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
            render_pass.draw(0..6, 0..1);

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

            // Draw Block cracking animation overlay
            if let Some(target) = self.mining_target {
                if self.mining_progress > 0.0 {
                    if let Some((_num_vertices, num_indices)) = self.update_crack_buffers(target, self.mining_progress) {
                        render_pass.set_vertex_buffer(0, self.crack_vertex_buffer.slice(..));
                        render_pass.set_index_buffer(self.crack_index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                        render_pass.draw_indexed(0..num_indices, 0, 0..1);
                    }
                }
            }

            if !self.is_paused {
                // 1. Draw Textured UI (block thumbnails)
                if self.num_ui_textured_vertices > 0 {
                    render_pass.set_pipeline(&self.ui_textured_pipeline);
                    render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
                    render_pass.set_vertex_buffer(0, self.ui_textured_vertex_buffer.slice(..));
                    render_pass.draw(0..self.num_ui_textured_vertices, 0..1);
                }

                // 2. Draw Colored UI (hotbar background)
                if self.num_ui_vertices > 0 {
                    render_pass.set_pipeline(&self.ui_pipeline);
                    render_pass.set_vertex_buffer(0, self.ui_vertex_buffer.slice(..));
                    render_pass.draw(0..self.num_ui_vertices, 0..1);
                }

                // 3. Draw Crosshair
                render_pass.set_pipeline(&self.crosshair_pipeline);
                render_pass.set_vertex_buffer(0, self.crosshair_buffer.slice(..));
                render_pass.draw(0..4, 0..1);

                // 4. Draw Line/Text UI (slot borders & texts)
                if self.num_ui_line_vertices > 0 {
                    render_pass.set_pipeline(&self.ui_line_pipeline);
                    render_pass.set_vertex_buffer(0, self.ui_line_vertex_buffer.slice(..));
                    render_pass.draw(0..self.num_ui_line_vertices, 0..1);
                }
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
