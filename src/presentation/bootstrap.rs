//! GPU / pipeline / launch-world helpers used by `State::new`.
//! Presentation-only: these do not own world authority.

use crate::camera::WorldTime;
use crate::game_rules::WorldRules;
use crate::inventory::{GameMode, Inventory};
use crate::menu::{GameSettings, WorldLaunch};
use crate::physics::PlayerPhysics;
use crate::player::PlayerState;
use glam::Vec3;
use std::sync::Arc;
use winit::window::Window;

pub(crate) struct GpuContext {
    pub surface: wgpu::Surface<'static>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub config: wgpu::SurfaceConfiguration,
    pub size: winit::dpi::PhysicalSize<u32>,
    pub gpu_timestamps_supported: bool,
    pub gpu_timestamps_inside_passes: bool,
}

/// Create the desktop wgpu surface, device, and swapchain config.
///
/// The NVIDIA Vulkan ICD crashes during the menu-to-world transition on
/// this Windows setup. `PRIMARY` still chooses Vulkan first, so force
/// DX12 here to match the menu and keep other platforms unchanged.
pub(crate) async fn create_gpu_context(
    window: &Arc<Window>,
    settings: &GameSettings,
) -> GpuContext {
    let size = window.inner_size();
    let backends = if cfg!(target_os = "windows") {
        wgpu::Backends::DX12
    } else {
        wgpu::Backends::PRIMARY
    };
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends,
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

    let adapter_features = adapter.features();
    let gpu_timestamps_supported = adapter_features.contains(wgpu::Features::TIMESTAMP_QUERY);
    let gpu_timestamps_inside_passes =
        adapter_features.contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES);
    let mut required_features = wgpu::Features::empty();
    if gpu_timestamps_supported {
        required_features |= wgpu::Features::TIMESTAMP_QUERY;
        if gpu_timestamps_inside_passes {
            required_features |= wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES;
        }
    }

    let (device, queue) = adapter
        .request_device(
            &wgpu::DeviceDescriptor {
                required_features,
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
        width: size.width.max(1),
        height: size.height.max(1),
        present_mode: if settings.vsync {
            wgpu::PresentMode::Fifo
        } else if surface_caps
            .present_modes
            .contains(&wgpu::PresentMode::Mailbox)
        {
            wgpu::PresentMode::Mailbox
        } else if surface_caps
            .present_modes
            .contains(&wgpu::PresentMode::Immediate)
        {
            wgpu::PresentMode::Immediate
        } else {
            wgpu::PresentMode::Fifo
        },
        alpha_mode: surface_caps.alpha_modes[0],
        view_formats: vec![],
        desired_maximum_frame_latency: 2,
    };
    surface.configure(&device, &config);

    GpuContext {
        surface,
        device,
        queue,
        config,
        size,
        gpu_timestamps_supported,
        gpu_timestamps_inside_passes,
    }
}

pub(crate) struct LaunchWorldState {
    pub current_dimension: crate::dimension::Dimension,
    pub player_physics: PlayerPhysics,
    pub game_mode: GameMode,
    pub inventory: Inventory,
    pub player_state: PlayerState,
    pub camera_yaw: f32,
    pub camera_pitch: f32,
    pub world_time: WorldTime,
    pub world_seed: u32,
    pub world_spawn: (i32, i32, i32),
    pub world_rules: WorldRules,
    pub world_type: crate::game_rules::WorldType,
    pub generate_structures: bool,
    pub bonus_chest: bool,
    pub cheats_enabled: bool,
    pub advancement_progress: crate::advancements::AdvancementProgressData,
}

/// Load presentation launch defaults for `State::new`.
///
/// Embedded presentations start empty and wait for runtime projections.
/// Reading player.dat / materializing the spawn halo here races
/// `ServerRuntime::new_embedded` on the same `world_dir`.
pub(crate) fn load_launch_world_state(
    launch: &WorldLaunch,
    is_client: bool,
    in_process_authority: bool,
) -> LaunchWorldState {
    // Join clients never own world persistence. They apply revision-gated
    // projections only and must not create a local save tree.
    // Embedded Singleplayer / listen-host already own the world through
    // ServerRuntime. Presentation has no SaveManager.
    let current_dimension = if is_client {
        crate::dimension::Dimension::Overworld
    } else if in_process_authority {
        // Read-only sidecar. Do not construct a presentation SaveManager
        // just to learn which dimension the runtime will project first.
        crate::save::peek_current_dimension(&launch.world_dir)
    } else {
        crate::dimension::Dimension::Overworld
    };

    let player_physics = PlayerPhysics::new(Vec3::new(8.0, 80.0, 8.0));
    let creation_options = crate::save::load_world_creation_options(&launch.world_dir);
    let game_mode = launch.game_mode;
    let inventory = match launch.game_mode {
        GameMode::Creative => Inventory::new_creative(),
        GameMode::Survival | GameMode::Adventure | GameMode::Spectator => Inventory::new(),
    };
    let player_state = PlayerState::new();
    let camera_yaw = f32::to_radians(90.0);
    let camera_pitch = f32::to_radians(-20.0);
    let world_time = WorldTime::new();
    let world_seed = launch.seed;
    let world_spawn = if creation_options.world_type == crate::game_rules::WorldType::Superflat {
        (8, 65, 8)
    } else {
        (8, 80, 8)
    };
    let world_rules = WorldRules {
        hardcore: creation_options.hardcore,
        ..Default::default()
    };
    let world_type = creation_options.world_type;
    let generate_structures = creation_options.generate_structures;
    let bonus_chest = creation_options.bonus_chest;
    let cheats_enabled = creation_options.cheats_enabled || is_client;
    let advancement_progress = crate::advancements::AdvancementProgressData::default();

    LaunchWorldState {
        current_dimension,
        player_physics,
        game_mode,
        inventory,
        player_state,
        camera_yaw,
        camera_pitch,
        world_time,
        world_seed,
        world_spawn,
        world_rules,
        world_type,
        generate_structures,
        bonus_chest,
        cheats_enabled,
        advancement_progress,
    }
}

pub(crate) struct PipelineLayouts {
    pub shader: wgpu::ShaderModule,
    pub region_bind_group_layout: wgpu::BindGroupLayout,
    pub render_pipeline_layout: wgpu::PipelineLayout,
    pub terrain_pipeline_layout: wgpu::PipelineLayout,
}

/// wgpu vertex layout for [`crate::chunk_render::TerrainVertex`].
/// Kept next to pipeline creation so `chunk_render` stays wgpu-free (lib fence).
pub(crate) fn terrain_vertex_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<crate::chunk_render::TerrainVertex>()
            as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &[
            wgpu::VertexAttribute {
                offset: 0,
                shader_location: 0,
                format: wgpu::VertexFormat::Uint16x4,
            },
            wgpu::VertexAttribute {
                offset: 8,
                shader_location: 1,
                format: wgpu::VertexFormat::Uint16x2,
            },
            wgpu::VertexAttribute {
                offset: 12,
                shader_location: 2,
                format: wgpu::VertexFormat::Uint16x2,
            },
        ],
    }
}

/// Shared shader + pipeline layouts used by `State::new`.
/// Individual wgpu render pipelines stay in `new` because they are
/// interleaved with buffer allocation.
pub(crate) fn create_pipelines(
    device: &wgpu::Device,
    camera_bind_group_layout: &wgpu::BindGroupLayout,
) -> PipelineLayouts {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Shader"),
        source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
            "../shader.wgsl"
        ))),
    });
    let region_bind_group_layout =
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
            label: Some("region_bind_group_layout"),
        });
    let render_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Render Pipeline Layout"),
        bind_group_layouts: &[camera_bind_group_layout],
        push_constant_ranges: &[],
    });
    let terrain_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Terrain Pipeline Layout"),
        bind_group_layouts: &[camera_bind_group_layout, &region_bind_group_layout],
        push_constant_ranges: &[],
    });
    PipelineLayouts {
        shader,
        region_bind_group_layout,
        render_pipeline_layout,
        terrain_pipeline_layout,
    }
}
