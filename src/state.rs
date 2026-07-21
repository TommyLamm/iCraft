use crate::camera::{Camera, CameraUniform};
use crate::chunk_manager::{mark_block_mesh_dependencies, surrounding_chunk_coords, ChunkManager};
use crate::crafting::RecipeManager;
use crate::interaction::raycast;
use crate::inventory::{GameMode, Inventory, Item, ItemStack, ToolType};
use crate::menu::{Difficulty, GameSettings, WorldLaunch};
use crate::physics::{PlayerPhysics, AABB};
use crate::player::{DamageSource, PlayerState};
use crate::world::{Biome, BlockType, Chunk, CHUNK_DEPTH, CHUNK_HEIGHT, CHUNK_WIDTH};
use glam::Vec3;
use std::sync::Arc;
use wgpu::util::DeviceExt;
use winit::window::Window;

const UI_VERTEX_CAPACITY: usize = 4096;
const UI_LINE_VERTEX_CAPACITY: usize = 16384;
const DEBUG_STATS_INTERVAL: f32 = 0.5;
const RAIN_LOOP_ID: u64 = u64::MAX - 1;
// Creating an entire render distance while handling a menu click blocks the
// window event loop and can allocate hundreds of chunk meshes at once.  Start
// with a safe area around the player; `update_chunks` streams the rest in over
// subsequent frames.
const INITIAL_WORLD_CHUNK_RADIUS: i32 = 1;

fn initial_chunk_radius(render_distance: i32) -> i32 {
    render_distance.clamp(0, INITIAL_WORLD_CHUNK_RADIUS)
}

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
    pub ao: f32,
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
                    offset: (std::mem::size_of::<[f32; 3]>() + std::mem::size_of::<[f32; 2]>())
                        as wgpu::BufferAddress,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32,
                },
                wgpu::VertexAttribute {
                    offset: (std::mem::size_of::<[f32; 3]>()
                        + std::mem::size_of::<[f32; 2]>()
                        + std::mem::size_of::<f32>())
                        as wgpu::BufferAddress,
                    shader_location: 3,
                    format: wgpu::VertexFormat::Float32,
                },
            ],
        }
    }
}

impl State {
    fn apply_block_changes(&mut self, changes: &[((i32, i32, i32), BlockType)]) {
        let mut dirty_chunks = std::collections::HashSet::new();
        for &((x, y, z), new_block) in changes {
            let old_block = self.chunk_manager.get_block(x, y, z);
            if old_block == new_block {
                continue;
            }
            if old_block != BlockType::Air {
                self.chunk_manager.set_block(x, y, z, BlockType::Air);
                crate::lighting::update_sky_light_after_removed(
                    &mut self.chunk_manager,
                    x,
                    y,
                    z,
                    &mut dirty_chunks,
                );
                crate::lighting::update_block_light_after_removed(
                    &mut self.chunk_manager,
                    x,
                    y,
                    z,
                    old_block.properties().light_emission,
                    &mut dirty_chunks,
                );
            }
            self.chunk_manager.set_block(x, y, z, new_block);
            crate::lighting::update_sky_light_after_placed(
                &mut self.chunk_manager,
                x,
                y,
                z,
                &mut dirty_chunks,
            );
            crate::lighting::update_block_light_after_placed(
                &mut self.chunk_manager,
                x,
                y,
                z,
                new_block.properties().light_emission,
                &mut dirty_chunks,
            );
            mark_block_mesh_dependencies(&mut dirty_chunks, x, z);
            self.redstone.on_block_changed(
                &self.chunk_manager,
                (x, y, z),
                crate::redstone::Direction::North,
            );
            self.check_and_break_unsupported_above(x, y, z, &mut dirty_chunks);
        }
        for coord in dirty_chunks {
            if let Some(mesh) = self.chunk_meshes.get_mut(&coord) {
                mesh.dirty = true;
            }
        }
    }

    pub fn check_and_break_unsupported_above(
        &mut self,
        wx: i32,
        wy: i32,
        wz: i32,
        dirty_chunks: &mut std::collections::HashSet<(i32, i32)>,
    ) {
        let game_mode = self.game_mode;
        let mut drops = Vec::new();
        self.chunk_manager.check_and_break_unsupported_above(
            wx,
            wy,
            wz,
            dirty_chunks,
            |(x, y, z), block| {
                if game_mode == GameMode::Survival {
                    let drop_item = match block {
                        BlockType::TallGrass => {
                            let rng = (x as u32)
                                .wrapping_mul(31)
                                .wrapping_add(y as u32 * 17)
                                .wrapping_add(z as u32);
                            if rng % 8 == 0 {
                                Some(crate::inventory::Item::Seeds)
                            } else {
                                None
                            }
                        }
                        BlockType::SnowLayer => None,
                        _ => Some(crate::inventory::Item::from_block(block)),
                    };
                    if let Some(item) = drop_item {
                        drops.push((
                            item,
                            glam::Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5),
                        ));
                    }
                }
            },
        );
        for (item, pos) in drops {
            self.spawn_dropped_item(item, pos);
        }
    }


    fn safe_dimension_spawn_y(&mut self, x: i32, z: i32) -> f32 {
        let top = if self.current_dimension == crate::dimension::Dimension::Nether {
            120
        } else {
            180
        };
        for y in (2..=top).rev() {
            if self
                .chunk_manager
                .get_block(x, y - 1, z)
                .properties()
                .is_solid
                && self
                    .chunk_manager
                    .get_block(x, y, z)
                    .properties()
                    .is_passable
                && self
                    .chunk_manager
                    .get_block(x, y + 1, z)
                    .properties()
                    .is_passable
            {
                return y as f32;
            }
        }
        let floor = match self.current_dimension {
            crate::dimension::Dimension::Nether => BlockType::Netherrack,
            crate::dimension::Dimension::End => BlockType::EndStone,
            crate::dimension::Dimension::Overworld => BlockType::Stone,
        };
        self.apply_block_changes(&[
            ((x, 63, z), floor),
            ((x, 64, z), BlockType::Air),
            ((x, 65, z), BlockType::Air),
        ]);
        64.0
    }

    fn build_linked_nether_portal(&mut self, chunk_x: i32, chunk_z: i32, spawn_y: i32) -> Vec3 {
        let base_x = chunk_x * CHUNK_WIDTH as i32 + 6;
        let base_z = chunk_z * CHUNK_DEPTH as i32 + 8;
        let base_y = (spawn_y - 1).clamp(5, 116);
        let mut changes = Vec::new();
        for x in base_x..=base_x + 3 {
            changes.push(((x, base_y, base_z), BlockType::Obsidian));
            changes.push(((x, base_y + 4, base_z), BlockType::Obsidian));
        }
        for y in base_y + 1..=base_y + 3 {
            changes.push(((base_x, y, base_z), BlockType::Obsidian));
            changes.push(((base_x + 3, y, base_z), BlockType::Obsidian));
            changes.push(((base_x + 1, y, base_z), BlockType::NetherPortal));
            changes.push(((base_x + 2, y, base_z), BlockType::NetherPortal));
        }
        self.apply_block_changes(&changes);
        Vec3::new(
            base_x as f32 + 1.5,
            base_y as f32 + 1.0,
            base_z as f32 + 0.5,
        )
    }

    fn switch_dimension(&mut self, target: crate::dimension::Dimension) {
        if target == self.current_dimension {
            return;
        }
        let source = self.current_dimension;
        for chunk in self.chunk_manager.chunks.values() {
            let data = crate::save::ChunkSaveData::from_chunk(chunk);
            let _ = self.save_tx.send(crate::save::SaveCommand::SaveChunk {
                dimension: source,
                data,
            });
        }

        let mut destination =
            crate::dimension::transform_position(source, target, self.player_physics.position);
        if target == crate::dimension::Dimension::End {
            destination = Vec3::new(0.5, 80.0, 0.5);
        } else if source == crate::dimension::Dimension::End {
            destination = Vec3::new(8.5, 80.0, 8.5);
        }

        self.current_dimension = target;
        let render_distance = self.chunk_manager.render_distance;
        self.chunk_manager = ChunkManager::new_in_dimension(render_distance, target);
        self.chunk_meshes.clear();
        self.entity_manager = crate::entity::EntityManager::new();
        self.particles = crate::particles::ParticleSystem::new();
        self.redstone = crate::redstone::RedstoneSystem::new();
        self.redstone_tick_timer = 0.0;
        self.mining_target = None;
        self.mining_progress = 0.0;
        self.left_mouse_pressed = false;
        self.water_tick_timer = 0.0;
        self.lava_tick_timer = 0.0;
        self.lava_damage_timer = 0.0;
        self.cactus_damage_timer = 0.0;
        self.audio_manager.stop_looping_sound(RAIN_LOOP_ID);

        let cx = (destination.x / CHUNK_WIDTH as f32).floor() as i32;
        let cz = (destination.z / CHUNK_DEPTH as f32).floor() as i32;
        let mut chunk = crate::dimension::generate_chunk(target, cx, cz, self.world_seed);
        if let Some(saved) = self
            .save_manager
            .lock()
            .unwrap()
            .load_chunk_in(target, cx, cz)
        {
            saved.restore_to_chunk(&mut chunk);
        }
        self.chunk_manager.chunks.insert((cx, cz), chunk);
        let mut dirty = std::collections::HashSet::new();
        crate::lighting::propagate_chunk_lighting(&mut self.chunk_manager, cx, cz, &mut dirty);

        let wx = destination.x.floor() as i32;
        let wz = destination.z.floor() as i32;
        destination.y = self.safe_dimension_spawn_y(wx, wz);
        if matches!(
            target,
            crate::dimension::Dimension::Overworld | crate::dimension::Dimension::Nether
        ) && matches!(
            source,
            crate::dimension::Dimension::Overworld | crate::dimension::Dimension::Nether
        ) {
            destination = self.build_linked_nether_portal(cx, cz, destination.y as i32);
        }
        self.player_physics.position = destination;
        self.player_physics.velocity = Vec3::ZERO;
        self.player_physics.on_ground = false;
        self.player_physics.highest_y = destination.y;
        self.camera.position = destination + Vec3::new(0.0, 1.6, 0.0);
        self.portal_contact_time = 0.0;
        self.portal_cooldown = 3.0;
        let _ = self
            .save_manager
            .lock()
            .unwrap()
            .save_current_dimension(target);
        println!("[Dimension] {} -> {}", source.name(), target.name());
    }

    fn update_portal_travel(&mut self, dt: f32) {
        self.portal_cooldown = (self.portal_cooldown - dt).max(0.0);
        if self.portal_cooldown > 0.0 {
            self.portal_contact_time = 0.0;
            return;
        }
        let pos = self.player_physics.position;
        let x = pos.x.floor() as i32;
        let y = pos.y.floor() as i32;
        let z = pos.z.floor() as i32;
        let feet = self.chunk_manager.get_block(x, y, z);
        let body = self.chunk_manager.get_block(x, y + 1, z);
        if feet == BlockType::EndPortal || body == BlockType::EndPortal {
            let target = if self.current_dimension == crate::dimension::Dimension::End {
                crate::dimension::Dimension::Overworld
            } else {
                crate::dimension::Dimension::End
            };
            self.switch_dimension(target);
            return;
        }
        if feet == BlockType::NetherPortal || body == BlockType::NetherPortal {
            self.portal_contact_time += dt;
            if self.portal_contact_time >= 1.0 {
                let target = if self.current_dimension == crate::dimension::Dimension::Nether {
                    crate::dimension::Dimension::Overworld
                } else {
                    crate::dimension::Dimension::Nether
                };
                self.switch_dimension(target);
            }
        } else {
            self.portal_contact_time = 0.0;
        }
    }

    fn apply_boss_events(&mut self, events: crate::boss::BossEvents) {
        for hit in events.player_damage {
            self.take_damage(hit.amount, DamageSource::Mob);
        }
        for effect in events.apply_wither {
            self.wither_effect_timer = self.wither_effect_timer.max(effect.duration);
        }
        for explosion in events.explosions {
            if explosion.break_blocks {
                crate::mob::explode(
                    explosion.position,
                    explosion.radius,
                    &mut self.chunk_manager,
                    &mut self.chunk_meshes,
                    &mut self.player_physics,
                    &mut self.player_state,
                    GameMode::Creative,
                    0.0,
                );
            }
            self.audio_manager
                .play_sound(crate::audio::SoundId::Explosion);
        }
        for drop in events.drops {
            for _ in 0..drop.count {
                self.spawn_dropped_item(drop.item, drop.position);
            }
        }
        let changes: Vec<_> = events
            .block_placements
            .into_iter()
            .map(|placement| (placement.position, placement.block))
            .collect();
        self.apply_block_changes(&changes);
        if events.dragon_completion.is_some() {
            self.player_state.add_experience(120);
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
                    offset: (std::mem::size_of::<[f32; 3]>() + std::mem::size_of::<[f32; 2]>())
                        as wgpu::BufferAddress,
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
    pub t: bool,
    pub ctrl: bool,
    pub shift: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StationKind {
    Enchanting,
    Brewing,
    Anvil,
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
    crack_pipeline: wgpu::RenderPipeline,
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
    pub entity_manager: crate::entity::EntityManager,
    mob_vertex_buffer: wgpu::Buffer,
    mob_index_buffer: wgpu::Buffer,
    mob_num_indices: u32,
    pub particles: crate::particles::ParticleSystem,
    particle_vertex_buffer: wgpu::Buffer,
    particle_index_buffer: wgpu::Buffer,
    particle_num_indices: u32,
    torch_smoke_timer: f32,
    total_time: f32,
    pub audio_manager: crate::audio::AudioManager,
    pub footstep_accumulator: f32,
    pub was_on_ground: bool,
    pub water_tick_timer: f32,
    pub lava_tick_timer: f32,
    pub lava_damage_timer: f32,
    pub cactus_damage_timer: f32,
    pub save_manager: std::sync::Arc<std::sync::Mutex<crate::save::SaveManager>>,
    pub save_tx: std::sync::mpsc::Sender<crate::save::SaveCommand>,
    pub autosave_timer: f32,
    pub is_saving: bool,
    pub is_sprinting: bool,
    pub base_fov: f32,
    pub w_click_timer: f32,
    pub last_w_pressed: bool,
    debug_frame_time_accumulator: f32,
    debug_frame_samples: u32,
    debug_fps: f32,
    debug_frame_ms: f32,
    pub active_station: Option<StationKind>,
    pub enchanting: crate::enchantment::EnchantingState,
    pub brewing: crate::brewing::BrewingStandState,
    pub anvil: crate::enchantment::AnvilState,
    pub potion_effects: crate::brewing::EffectManager,
    pub redstone: crate::redstone::RedstoneSystem,
    redstone_tick_timer: f32,
    pub weather: crate::weather::WeatherSystem,
    pub settings: GameSettings,
    pub world_seed: u32,
    pub difficulty: Difficulty,
    pub current_dimension: crate::dimension::Dimension,
    portal_contact_time: f32,
    portal_cooldown: f32,
    wither_effect_timer: f32,
    wither_damage_timer: f32,
    pub advancement_manager: crate::advancements::AdvancementManager,
    pub advancement_gui: crate::advancements::AdvancementGui,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotType {
    Hotbar(usize),
    Backpack(usize),
    Armor(usize),
    CraftInput(usize),
    CraftOutput,
    EnchantInput,
    EnchantLapis,
    BrewBottle(usize),
    BrewIngredient,
    AnvilLeft,
    AnvilRight,
    AnvilOutput,
}

impl State {
    fn create_depth_texture(
        device: &wgpu::Device,
        config: &wgpu::SurfaceConfiguration,
    ) -> wgpu::TextureView {
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

    pub async fn new(window: Arc<Window>, launch: WorldLaunch, settings: GameSettings) -> Self {
        let size = window.inner_size();
        // The NVIDIA Vulkan ICD crashes during the menu-to-world transition on
        // this Windows setup. `PRIMARY` still chooses Vulkan first, so force
        // DX12 here to match the menu and keep other platforms unchanged.
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
            present_mode: if settings.vsync
                || !surface_caps
                    .present_modes
                    .contains(&wgpu::PresentMode::Immediate)
            {
                wgpu::PresentMode::Fifo
            } else {
                wgpu::PresentMode::Immediate
            },
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        // Setup Depth Buffer
        let depth_view = Self::create_depth_texture(&device, &config);

        // Initialize SaveManager
        let save_manager = std::sync::Arc::new(std::sync::Mutex::new(
            crate::save::SaveManager::new(&launch.world_dir),
        ));
        let current_dimension = save_manager.lock().unwrap().load_current_dimension();

        // Spawn background worker thread
        let (save_tx, save_rx) = std::sync::mpsc::channel::<crate::save::SaveCommand>();
        let save_manager_clone = std::sync::Arc::clone(&save_manager);
        std::thread::spawn(move || {
            while let Ok(cmd) = save_rx.recv() {
                match cmd {
                    crate::save::SaveCommand::SaveChunk { dimension, data } => {
                        let mut mgr = save_manager_clone.lock().unwrap();
                        let _ = mgr.save_chunk_in(dimension, data.chunk_x, data.chunk_z, data);
                    }
                    crate::save::SaveCommand::SaveLevelAndPlayer(level, player) => {
                        let mgr = save_manager_clone.lock().unwrap();
                        let _ = mgr.save_player_and_level(&level, &player);
                    }
                }
            }
        });

        // Initialize physics and keyboard input
        let mut player_physics = PlayerPhysics::new(Vec3::new(8.0, 80.0, 8.0));
        let keys = KeyState::default();

        let mut audio_manager = crate::audio::AudioManager::new();
        audio_manager.set_volume(settings.effective_sound_volume());

        // Load save data if exists
        let mut game_mode = launch.game_mode;
        let mut inventory = match launch.game_mode {
            GameMode::Creative => Inventory::new_creative(),
            GameMode::Survival => Inventory::new(),
        };
        let mut player_state = PlayerState::new();
        let mut camera_yaw = f32::to_radians(90.0);
        let mut camera_pitch = f32::to_radians(-20.0);
        let mut world_time = crate::camera::WorldTime::new();
        let mut world_seed = launch.seed;

        let mut advancement_progress = crate::advancements::AdvancementProgressData::default();
        let has_save = {
            let mgr = save_manager.lock().unwrap();
            mgr.load_player_and_level().is_ok()
        };

        if has_save {
            let (level, player) = {
                let mgr = save_manager.lock().unwrap();
                mgr.load_player_and_level().unwrap()
            };
            world_seed = level.seed;
            world_time.ticks = level.time;
            player_physics.position = Vec3::from_slice(&player.position);
            player_physics.velocity = Vec3::from_slice(&player.velocity);
            camera_yaw = player.yaw;
            camera_pitch = player.pitch;
            player_state.health = player.health;
            player_state.hunger = player.hunger;
            player_state.saturation = player.saturation;
            player_state.exhaustion = player.exhaustion;
            player_state.oxygen = player.oxygen;
            player_state.experience = player.experience;
            player_state.experience_level = player.experience_level;
            game_mode = player.game_mode;
            inventory = player.inventory.to_inventory();
            advancement_progress = player.advancements;
        }

        let advancement_manager =
            crate::advancements::AdvancementManager::new(advancement_progress);
        let advancement_gui = crate::advancements::AdvancementGui::new();

        // Setup Camera
        let camera = Camera::new(
            player_physics.position + Vec3::new(0.0, 1.6, 0.0), // Spawn at player eye height
            camera_yaw,
            camera_pitch,
            settings.fov,
        );
        let base_fov = camera.fov;
        let show_debug = false;
        let mut camera_uniform = CameraUniform::new();
        camera_uniform.update_view_proj(
            &camera,
            config.width as f32 / config.height as f32,
            settings.render_distance as u32,
            &world_time,
            0.0,
            false,
        );

        let camera_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Camera Buffer"),
            contents: bytemuck::cast_slice(&[camera_uniform]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let texture_atlas = crate::texture::TextureAtlas::new_procedural(&device, &queue);

        let camera_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
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
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
                "shader.wgsl"
            ))),
        });

        let render_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
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

        let crack_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Crack Overlay Render Pipeline"),
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
                            src_factor: wgpu::BlendFactor::Dst,
                            dst_factor: wgpu::BlendFactor::Zero,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::Zero,
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
        let crosshair_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
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
            Vertex {
                position: [-crosshair_size, 0.0, 0.0],
                tex_coords: [0.0, 0.0],
                light_level: 1.0,
                ao: 1.0,
            },
            Vertex {
                position: [crosshair_size, 0.0, 0.0],
                tex_coords: [0.0, 0.0],
                light_level: 1.0,
                ao: 1.0,
            },
            Vertex {
                position: [0.0, -crosshair_size * aspect, 0.0],
                tex_coords: [0.0, 0.0],
                light_level: 1.0,
                ao: 1.0,
            },
            Vertex {
                position: [0.0, crosshair_size * aspect, 0.0],
                tex_coords: [0.0, 0.0],
                light_level: 1.0,
                ao: 1.0,
            },
        ];

        let crosshair_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Crosshair Vertex Buffer"),
            contents: bytemuck::cast_slice(&crosshair_vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        // Initialize ChunkManager and load spawn area chunks
        let render_distance = settings.render_distance;
        let mut chunk_manager = ChunkManager::new_in_dimension(render_distance, current_dimension);
        let mut chunk_meshes = std::collections::HashMap::new();

        // Load only the immediate spawn area synchronously.  Loading every
        // chunk in a large render distance here used to create all CPU/GPU
        // meshes in one window event (625 chunks at distance 12), freezing the
        // app and often causing the graphics driver to reset.  `update_chunks`
        // loads the remaining requested chunks one at a time after the first
        // frame is visible.
        let player_chunk_x = (player_physics.position.x / CHUNK_WIDTH as f32).floor() as i32;
        let player_chunk_z = (player_physics.position.z / CHUNK_DEPTH as f32).floor() as i32;
        let initial_radius = initial_chunk_radius(render_distance);
        for cx in player_chunk_x - initial_radius..=player_chunk_x + initial_radius {
            for cz in player_chunk_z - initial_radius..=player_chunk_z + initial_radius {
                let mut chunk =
                    crate::dimension::generate_chunk(current_dimension, cx, cz, world_seed);
                let saved_chunk = {
                    let mut manager = save_manager.lock().unwrap();
                    manager.load_chunk_in(current_dimension, cx, cz)
                };
                if let Some(data) = saved_chunk {
                    data.restore_to_chunk(&mut chunk);
                }
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
        for (&(cx, cz), chunk) in chunks_ref {
            let (o_verts, o_inds, t_verts, t_inds) = chunk.generate_mesh(|wx, wy, wz| {
                if wy < 0 {
                    return (BlockType::Air, 0, 0, 0, false);
                }
                if wy >= crate::world::CHUNK_HEIGHT as i32 {
                    return (
                        BlockType::Air,
                        if current_dimension.has_sky_light() {
                            15
                        } else {
                            0
                        },
                        0,
                        0,
                        false,
                    );
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
                        c.fluid_levels[bx_neighbor][wy as usize][bz_neighbor] & 0x07,
                        (c.fluid_levels[bx_neighbor][wy as usize][bz_neighbor] & 0x08) != 0,
                    )
                } else {
                    (
                        BlockType::Air,
                        if current_dimension.has_sky_light() {
                            15
                        } else {
                            0
                        },
                        0,
                        0,
                        false,
                    )
                }
            });

            let opaque_vertex_buffer =
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Chunk Opaque Vertex Buffer"),
                    contents: bytemuck::cast_slice(&o_verts),
                    usage: wgpu::BufferUsages::VERTEX,
                });
            let opaque_index_buffer =
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Chunk Opaque Index Buffer"),
                    contents: bytemuck::cast_slice(&o_inds),
                    usage: wgpu::BufferUsages::INDEX,
                });
            let transparent_vertex_buffer =
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Chunk Translucent Vertex Buffer"),
                    contents: bytemuck::cast_slice(&t_verts),
                    usage: wgpu::BufferUsages::VERTEX,
                });
            let transparent_index_buffer =
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Chunk Translucent Index Buffer"),
                    contents: bytemuck::cast_slice(&t_inds),
                    usage: wgpu::BufferUsages::INDEX,
                });

            chunk_meshes.insert(
                (cx, cz),
                ChunkMesh {
                    opaque_vertex_buffer,
                    opaque_index_buffer,
                    opaque_num_indices: o_inds.len() as u32,
                    transparent_vertex_buffer,
                    transparent_index_buffer,
                    transparent_num_indices: t_inds.len() as u32,
                    dirty: false,
                },
            );
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
            size: (std::mem::size_of::<UiVertex>() * UI_VERTEX_CAPACITY) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let ui_line_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("UI Line Vertex Buffer"),
            size: (std::mem::size_of::<UiVertex>() * UI_LINE_VERTEX_CAPACITY)
                as wgpu::BufferAddress,
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

        let mob_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Mob Vertex Buffer"),
            size: (std::mem::size_of::<Vertex>() * 8192) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let mob_index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Mob Index Buffer"),
            size: (std::mem::size_of::<u32>() * 12288) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let particle_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Particle Vertex Buffer"),
            size: (std::mem::size_of::<Vertex>() * crate::particles::MAX_PARTICLES * 4)
                as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let particle_index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Particle Index Buffer"),
            size: (std::mem::size_of::<u32>() * crate::particles::MAX_PARTICLES * 6)
                as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let particles = crate::particles::ParticleSystem::new();
        let weather = crate::weather::WeatherSystem::new(world_seed);

        Self {
            window,
            surface,
            device,
            queue,
            config,
            size,
            render_pipeline,
            trans_pipeline,
            crack_pipeline,
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
            game_mode,
            inventory,
            recipe_manager: RecipeManager::new(),
            left_mouse_pressed: false,
            mining_target: None,
            mining_progress: 0.0,
            crack_vertex_buffer,
            crack_index_buffer,
            player_state,
            void_damage_timer: 0.0,
            world_time,
            show_debug,
            entity_manager: crate::entity::EntityManager::new(),
            mob_vertex_buffer,
            mob_index_buffer,
            mob_num_indices: 0,
            particles,
            particle_vertex_buffer,
            particle_index_buffer,
            particle_num_indices: 0,
            torch_smoke_timer: 0.0,
            total_time: 0.0,
            audio_manager,
            footstep_accumulator: 0.0,
            was_on_ground: false,
            water_tick_timer: 0.0,
            lava_tick_timer: 0.0,
            lava_damage_timer: 0.0,
            cactus_damage_timer: 0.0,
            save_manager,
            save_tx,
            autosave_timer: 0.0,
            is_saving: false,
            is_sprinting: false,
            base_fov,
            w_click_timer: 0.0,
            last_w_pressed: false,
            debug_frame_time_accumulator: 0.0,
            debug_frame_samples: 0,
            debug_fps: 0.0,
            debug_frame_ms: 0.0,
            active_station: None,
            enchanting: crate::enchantment::EnchantingState::default(),
            brewing: crate::brewing::BrewingStandState::default(),
            anvil: crate::enchantment::AnvilState::default(),
            potion_effects: crate::brewing::EffectManager::default(),
            redstone: crate::redstone::RedstoneSystem::new(),
            redstone_tick_timer: 0.0,
            weather,
            difficulty: launch.difficulty,
            world_seed,
            settings,
            current_dimension,
            portal_contact_time: 0.0,
            portal_cooldown: 0.0,
            wither_effect_timer: 0.0,
            wither_damage_timer: 0.0,
            advancement_manager,
            advancement_gui,
        }
    }

    pub fn save_settings(&self) {
        let mut settings = self.settings.clone();
        settings.fov = self.camera.fov;
        settings.sensitivity = self.sensitivity;
        settings.render_distance = self.chunk_manager.render_distance;
        settings.master_volume = if settings.sound_volume > 0.0 {
            (self.audio_manager.volume / settings.sound_volume).clamp(0.0, 1.0)
        } else {
            settings.master_volume
        };
        settings.save();
    }

    pub fn trigger_background_save(&self) {
        let world_dir = self.save_manager.lock().unwrap().world_dir.clone();
        crate::menu::update_world_metadata(
            &world_dir,
            self.world_seed,
            self.game_mode,
            self.difficulty,
        );
        let level = crate::save::LevelData {
            seed: self.world_seed,
            time: self.world_time.ticks,
        };
        let player = crate::save::PlayerData::from_state(
            self.player_physics.position,
            self.player_physics.velocity,
            self.camera.yaw,
            self.camera.pitch,
            &self.player_state,
            self.game_mode,
            &self.inventory,
            self.advancement_manager.progress.clone(),
        );
        let _ = self
            .save_tx
            .send(crate::save::SaveCommand::SaveLevelAndPlayer(level, player));

        for chunk in self.chunk_manager.chunks.values() {
            let chunk_data = crate::save::ChunkSaveData::from_chunk(chunk);
            let _ = self.save_tx.send(crate::save::SaveCommand::SaveChunk {
                dimension: self.current_dimension,
                data: chunk_data,
            });
        }
        let _ = self
            .save_manager
            .lock()
            .unwrap()
            .save_current_dimension(self.current_dimension);
    }

    pub fn save_synchronously(&self) {
        let level = crate::save::LevelData {
            seed: self.world_seed,
            time: self.world_time.ticks,
        };
        let player = crate::save::PlayerData::from_state(
            self.player_physics.position,
            self.player_physics.velocity,
            self.camera.yaw,
            self.camera.pitch,
            &self.player_state,
            self.game_mode,
            &self.inventory,
            self.advancement_manager.progress.clone(),
        );

        let mut mgr = self.save_manager.lock().unwrap();
        let _ = mgr.save_player_and_level(&level, &player);

        for chunk in self.chunk_manager.chunks.values() {
            let chunk_data = crate::save::ChunkSaveData::from_chunk(chunk);
            let _ = mgr.save_chunk_in(
                self.current_dimension,
                chunk.chunk_x,
                chunk.chunk_z,
                chunk_data,
            );
        }
        let _ = mgr.save_current_dimension(self.current_dimension);
        crate::menu::update_world_metadata(
            &mgr.world_dir,
            self.world_seed,
            self.game_mode,
            self.difficulty,
        );
        println!("[Save] Synchronously saved world state.");
    }

    pub fn trigger_advancement(&mut self, trigger: crate::advancements::AdvancementTrigger) {
        let newly_completed = self.advancement_manager.check_trigger(&trigger);
        for id in newly_completed {
            if let Some(adv) = self.advancement_manager.tree.get(&id) {
                if adv.xp_reward > 0 {
                    self.player_state.add_experience(adv.xp_reward);
                }
                self.audio_manager
                    .play_sound(crate::audio::SoundId::UiClick);
            }
        }
    }

    pub fn open_advancements_ui(&mut self) {
        if self.inventory.is_open {
            self.close_inventory();
        }
        self.advancement_gui.open();
        let _ = self
            .window
            .set_cursor_grab(winit::window::CursorGrabMode::None);
        self.window.set_cursor_visible(true);
    }

    pub fn close_advancements_ui(&mut self) {
        self.advancement_gui.close();
        if !self.is_paused && !self.inventory.is_open {
            let _ = self
                .window
                .set_cursor_grab(winit::window::CursorGrabMode::Confined)
                .or_else(|_| {
                    self.window
                        .set_cursor_grab(winit::window::CursorGrabMode::Locked)
                });
            self.window.set_cursor_visible(false);
        }
    }

    pub fn handle_advancements_click(&mut self, pressed: bool) {
        if !self.advancement_gui.is_open {
            return;
        }
        let (screen_w, screen_h) = (self.config.width as f32, self.config.height as f32);
        let mouse_x = (self.mouse_ndc[0] + 1.0) * 0.5 * screen_w;
        let mouse_y = (1.0 - self.mouse_ndc[1]) * 0.5 * screen_h;

        let wy0 = screen_h * 0.1;
        let wy1 = screen_h * 0.9;
        let wx0 = screen_w * 0.1;
        let wx1 = screen_w * 0.9;

        if pressed {
            if mouse_y >= wy0 && mouse_y <= wy0 + 40.0 && mouse_x >= wx0 && mouse_x <= wx1 {
                let tab_w = (wx1 - wx0) / 5.0;
                let tab_idx = ((mouse_x - wx0) / tab_w).floor() as usize;
                let categories = [
                    crate::advancements::AdvancementCategory::Minecraft,
                    crate::advancements::AdvancementCategory::Nether,
                    crate::advancements::AdvancementCategory::TheEnd,
                    crate::advancements::AdvancementCategory::Adventure,
                    crate::advancements::AdvancementCategory::Husbandry,
                ];
                if tab_idx < categories.len() {
                    self.advancement_gui.selected_category = categories[tab_idx];
                }
            } else if mouse_x >= wx0 && mouse_x <= wx1 && mouse_y >= wy0 + 40.0 && mouse_y <= wy1 {
                self.advancement_gui.is_dragging = true;
                self.advancement_gui.drag_start_x = mouse_x - self.advancement_gui.scroll_x;
                self.advancement_gui.drag_start_y = mouse_y - self.advancement_gui.scroll_y;
            }
        } else {
            self.advancement_gui.is_dragging = false;
        }
    }

    pub fn update_chunks(&mut self) {
        let player_pos = self.player_physics.position;
        let px = (player_pos.x / 16.0).floor() as i32;
        let pz = (player_pos.z / 16.0).floor() as i32;
        let r = self.chunk_manager.render_distance;

        // 1. Unload out-of-bounds chunks
        let mut to_unload = Vec::new();
        for &(cx, cz) in self.chunk_manager.chunks.keys() {
            if (cx - px).abs() > r || (cz - pz).abs() > r {
                to_unload.push((cx, cz));
            }
        }
        for &(cx, cz) in &to_unload {
            if let Some(chunk) = self.chunk_manager.chunks.remove(&(cx, cz)) {
                let chunk_data = crate::save::ChunkSaveData::from_chunk(&chunk);
                let _ = self.save_tx.send(crate::save::SaveCommand::SaveChunk {
                    dimension: self.current_dimension,
                    data: chunk_data,
                });
            }
        }
        for &(cx, cz) in &to_unload {
            for neighbor in surrounding_chunk_coords(cx, cz) {
                if self.chunk_manager.chunks.contains_key(&neighbor) {
                    if let Some(mesh) = self.chunk_meshes.get_mut(&neighbor) {
                        mesh.dirty = true;
                    }
                }
            }
        }
        self.chunk_meshes
            .retain(|&(cx, cz), _| (cx - px).abs() <= r && (cz - pz).abs() <= r);

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
            let mut chunk =
                crate::dimension::generate_chunk(self.current_dimension, cx, cz, self.world_seed);
            let chunk_data = {
                let mut mgr = self.save_manager.lock().unwrap();
                mgr.load_chunk_in(self.current_dimension, cx, cz)
            };
            if let Some(data) = chunk_data {
                data.restore_to_chunk(&mut chunk);
            }
            self.chunk_manager.chunks.insert((cx, cz), chunk);

            let mut dirty = std::collections::HashSet::new();
            // Re-seed both sides of every newly available boundary. This lets
            // light from an already-loaded neighboring cave enter the new
            // chunk, as well as light from the new chunk flow outward.
            for (lighting_cx, lighting_cz) in [
                (cx, cz),
                (cx - 1, cz),
                (cx + 1, cz),
                (cx, cz - 1),
                (cx, cz + 1),
            ] {
                if self
                    .chunk_manager
                    .chunks
                    .contains_key(&(lighting_cx, lighting_cz))
                {
                    crate::lighting::propagate_chunk_lighting(
                        &mut self.chunk_manager,
                        lighting_cx,
                        lighting_cz,
                        &mut dirty,
                    );
                }
            }

            // AO corner samples and face culling depend on all eight neighbors.
            for neighbor in surrounding_chunk_coords(cx, cz) {
                if let Some(mesh) = self.chunk_meshes.get_mut(&neighbor) {
                    mesh.dirty = true;
                }
            }

            // Only mark dirty chunks within ±2 of the loaded chunk to limit cascade
            for (dcx, dcz) in dirty {
                if (dcx - cx).abs() <= 2 && (dcz - cz).abs() <= 2 {
                    if let Some(mesh) = self.chunk_meshes.get_mut(&(dcx, dcz)) {
                        mesh.dirty = true;
                    }
                }
            }
        }

        // 4. Rebuild at most 4 dirty meshes per frame, prioritize nearest
        let mut to_rebuild = Vec::new();
        for (&(cx, cz), _) in &self.chunk_manager.chunks {
            let needs_mesh = !self.chunk_meshes.contains_key(&(cx, cz));
            let is_dirty = self
                .chunk_meshes
                .get(&(cx, cz))
                .map(|m| m.dirty)
                .unwrap_or(false);
            if needs_mesh || is_dirty {
                let dx = cx - px;
                let dz = cz - pz;
                to_rebuild.push((cx, cz, dx * dx + dz * dz));
            }
        }

        // Sort by distance — rebuild closest chunks first
        to_rebuild.sort_by_key(|&(_, _, dist)| dist);

        let chunks_ref = &self.chunk_manager.chunks;
        let default_sky_light = if self.current_dimension.has_sky_light() {
            15
        } else {
            0
        };
        for (cx, cz, _) in to_rebuild.into_iter().take(4) {
            let chunk = chunks_ref.get(&(cx, cz)).unwrap();
            let (o_verts, o_inds, t_verts, t_inds) = chunk.generate_mesh(|wx, wy, wz| {
                if wy < 0 {
                    return (BlockType::Air, 0, 0, 0, false);
                }
                if wy >= crate::world::CHUNK_HEIGHT as i32 {
                    return (BlockType::Air, default_sky_light, 0, 0, false);
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
                        c.fluid_levels[bx_neighbor][wy as usize][bz_neighbor] & 0x07,
                        (c.fluid_levels[bx_neighbor][wy as usize][bz_neighbor] & 0x08) != 0,
                    )
                } else {
                    (BlockType::Air, default_sky_light, 0, 0, false)
                }
            });

            let opaque_vertex_buffer =
                self.device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("Chunk Opaque Vertex Buffer"),
                        contents: bytemuck::cast_slice(&o_verts),
                        usage: wgpu::BufferUsages::VERTEX,
                    });
            let opaque_index_buffer =
                self.device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("Chunk Opaque Index Buffer"),
                        contents: bytemuck::cast_slice(&o_inds),
                        usage: wgpu::BufferUsages::INDEX,
                    });
            let transparent_vertex_buffer =
                self.device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("Chunk Translucent Vertex Buffer"),
                        contents: bytemuck::cast_slice(&t_verts),
                        usage: wgpu::BufferUsages::VERTEX,
                    });
            let transparent_index_buffer =
                self.device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("Chunk Translucent Index Buffer"),
                        contents: bytemuck::cast_slice(&t_inds),
                        usage: wgpu::BufferUsages::INDEX,
                    });

            self.chunk_meshes.insert(
                (cx, cz),
                ChunkMesh {
                    opaque_vertex_buffer,
                    opaque_index_buffer,
                    opaque_num_indices: o_inds.len() as u32,
                    transparent_vertex_buffer,
                    transparent_index_buffer,
                    transparent_num_indices: t_inds.len() as u32,
                    dirty: false,
                },
            );
        }
    }

    pub fn set_paused(&mut self, paused: bool) {
        self.is_paused = paused;
        println!("[Debug] set_paused called with: {}", paused);
        if paused {
            let res = self
                .window
                .set_cursor_grab(winit::window::CursorGrabMode::None);
            println!("[Debug] Release grab result: {:?}", res);
            self.window.set_cursor_visible(true);
            self.keys = KeyState::default();
        } else {
            let res = self
                .window
                .set_cursor_grab(winit::window::CursorGrabMode::Locked)
                .or_else(|_| {
                    self.window
                        .set_cursor_grab(winit::window::CursorGrabMode::Confined)
                });
            println!("[Debug] Grab cursor result: {:?}", res);
            self.window.set_cursor_visible(false);
        }
    }

    pub fn handle_mouse_move(&mut self, x: f64, y: f64) {
        let ndc_x = (x as f32 / self.size.width as f32) * 2.0 - 1.0;
        let ndc_y = 1.0 - (y as f32 / self.size.height as f32) * 2.0;
        self.mouse_ndc = [ndc_x, ndc_y];
    }

    pub fn handle_menu_click(&mut self) -> bool {
        if self.is_paused {
            let [x, y] = self.mouse_ndc;

            // Resume Button bounds: X: [-0.3, 0.3], Y: [0.24, 0.34]
            if x >= -0.3 && x <= 0.3 && y >= 0.24 && y <= 0.34 {
                self.audio_manager
                    .play_sound(crate::audio::SoundId::UiClick);
                self.set_paused(false);
            }
            // FOV Button bounds: X: [-0.3, 0.3], Y: [0.10, 0.20]
            else if x >= -0.3 && x <= 0.3 && y >= 0.10 && y <= 0.20 {
                self.audio_manager
                    .play_sound(crate::audio::SoundId::UiClick);
                if x < 0.0 {
                    self.camera.fov = (self.camera.fov - 5.0).max(30.0);
                } else {
                    self.camera.fov = (self.camera.fov + 5.0).min(120.0);
                }
                // Update camera projection buffer immediately for visual feedback in paused state
                let is_underwater = self.chunk_manager.get_block(
                    self.camera.position.x.floor() as i32,
                    self.camera.position.y.floor() as i32,
                    self.camera.position.z.floor() as i32,
                ) == BlockType::Water;
                self.camera_uniform.update_view_proj(
                    &self.camera,
                    self.config.width as f32 / self.config.height as f32,
                    self.chunk_manager.render_distance as u32,
                    &self.world_time,
                    self.total_time,
                    is_underwater,
                );
                self.queue.write_buffer(
                    &self.camera_buffer,
                    0,
                    bytemuck::cast_slice(&[self.camera_uniform]),
                );
                self.save_settings();
            }
            // Sensitivity Button bounds: X: [-0.3, 0.3], Y: [-0.04, 0.06]
            else if x >= -0.3 && x <= 0.3 && y >= -0.04 && y <= 0.06 {
                self.audio_manager
                    .play_sound(crate::audio::SoundId::UiClick);
                if x < 0.0 {
                    self.sensitivity = (self.sensitivity - 0.0002).max(0.0002);
                } else {
                    self.sensitivity = (self.sensitivity + 0.0002).min(0.0060);
                }
                self.save_settings();
            }
            // Render Distance Button bounds: X: [-0.3, 0.3], Y: [-0.18, -0.08]
            else if x >= -0.3 && x <= 0.3 && y >= -0.18 && y <= -0.08 {
                self.audio_manager
                    .play_sound(crate::audio::SoundId::UiClick);
                if x < 0.0 {
                    self.chunk_manager.render_distance =
                        (self.chunk_manager.render_distance - 1).max(2);
                } else {
                    self.chunk_manager.render_distance =
                        (self.chunk_manager.render_distance + 1).min(16);
                }
                self.save_settings();
            }
            // Volume Button: X: [-0.3, 0.3], Y: [-0.32, -0.22]
            else if x >= -0.3 && x <= 0.3 && y >= -0.32 && y <= -0.22 {
                self.audio_manager
                    .play_sound(crate::audio::SoundId::UiClick);
                let mut new_vol = self.audio_manager.volume;
                if x < 0.0 {
                    new_vol = (new_vol - 0.1).max(0.0);
                } else {
                    new_vol = (new_vol + 0.1).min(1.0);
                }
                self.audio_manager.set_volume(new_vol);
                self.save_settings();
            }
            // Quit Button bounds (Shifted): X: [-0.3, 0.3], Y: [-0.46, -0.36]
            else if x >= -0.3 && x <= 0.3 && y >= -0.46 && y <= -0.36 {
                self.audio_manager
                    .play_sound(crate::audio::SoundId::UiClick);
                self.is_saving = true;
                let _ = self.render();
                self.save_synchronously();
                return true;
            }
        }
        false
    }

    pub fn update(&mut self, dt: f32) {
        self.debug_frame_time_accumulator += dt;
        self.debug_frame_samples += 1;
        if self.debug_frame_time_accumulator >= DEBUG_STATS_INTERVAL {
            let average_frame_time =
                self.debug_frame_time_accumulator / self.debug_frame_samples as f32;
            self.debug_frame_ms = average_frame_time * 1000.0;
            self.debug_fps = if average_frame_time > f32::EPSILON {
                1.0 / average_frame_time
            } else {
                0.0
            };
            self.debug_frame_time_accumulator = 0.0;
            self.debug_frame_samples = 0;
        }

        self.autosave_timer += dt;
        if self.autosave_timer >= 300.0 {
            self.autosave_timer = 0.0;
            self.trigger_background_save();
        }

        self.water_tick_timer += dt;
        if self.water_tick_timer >= 0.25 {
            self.water_tick_timer = 0.0;
            let dirty = crate::fluid::tick_fluids(&mut self.chunk_manager, false, 2048);
            for (cx, cz) in dirty {
                if let Some(mesh) = self.chunk_meshes.get_mut(&(cx, cz)) {
                    mesh.dirty = true;
                }
            }
        }

        self.lava_tick_timer += dt;
        if self.lava_tick_timer >= 1.5 {
            self.lava_tick_timer = 0.0;
            let dirty = crate::fluid::tick_fluids(&mut self.chunk_manager, true, 512);
            for (cx, cz) in dirty {
                if let Some(mesh) = self.chunk_meshes.get_mut(&(cx, cz)) {
                    mesh.dirty = true;
                }
            }
        }
        if self.player_state.is_dead {
            self.audio_manager.stop_looping_sound(RAIN_LOOP_ID);
            return;
        }
        if self.is_paused {
            self.audio_manager.stop_looping_sound(RAIN_LOOP_ID);
            return;
        }
        self.update_portal_travel(dt);

        self.redstone_tick_timer += dt;
        let mut redstone_steps = 0;
        while self.redstone_tick_timer >= 0.05 && redstone_steps < 4 {
            self.redstone_tick_timer -= 0.05;
            redstone_steps += 1;
            let mut occupants = Vec::with_capacity(self.entity_manager.entities.len() + 1);
            occupants.push((
                self.player_physics.position.x.floor() as i32,
                self.player_physics.position.y.floor() as i32,
                self.player_physics.position.z.floor() as i32,
            ));
            occupants.extend(self.entity_manager.entities.iter().map(|entity| {
                (
                    entity.position.x.floor() as i32,
                    entity.position.y.floor() as i32,
                    entity.position.z.floor() as i32,
                )
            }));
            let update = self.redstone.tick(&mut self.chunk_manager, &occupants);
            self.apply_redstone_update(update);
        }
        if redstone_steps == 4 {
            self.redstone_tick_timer = self.redstone_tick_timer.min(0.05);
        }

        self.brewing.update(dt);
        let effect_health = self.potion_effects.update(dt);
        if effect_health > 0.0 {
            self.player_state.health =
                (self.player_state.health + effect_health).min(self.player_state.max_health);
        } else if effect_health < 0.0 && self.player_state.health > 1.0 {
            self.take_damage(
                (-effect_health).min(self.player_state.health - 1.0),
                DamageSource::Mob,
            );
        }
        if self.wither_effect_timer > 0.0 {
            self.wither_effect_timer = (self.wither_effect_timer - dt).max(0.0);
            self.wither_damage_timer += dt;
            if self.wither_damage_timer >= 1.0 {
                self.wither_damage_timer -= 1.0;
                self.take_damage(1.0, DamageSource::Mob);
            }
        } else {
            self.wither_damage_timer = 0.0;
        }

        self.advancement_manager.update_toasts(dt);
        if self.advancement_gui.is_open && self.advancement_gui.is_dragging {
            let (screen_w, screen_h) = (self.config.width as f32, self.config.height as f32);
            let mouse_x = (self.mouse_ndc[0] + 1.0) * 0.5 * screen_w;
            let mouse_y = (1.0 - self.mouse_ndc[1]) * 0.5 * screen_h;
            self.advancement_gui.scroll_x = mouse_x - self.advancement_gui.drag_start_x;
            self.advancement_gui.scroll_y = mouse_y - self.advancement_gui.drag_start_y;
        }

        // Advance lightweight particle simulation every frame.
        self.particles.update(dt);

        // Double click W logic
        if self.keys.w && !self.last_w_pressed {
            if self.w_click_timer > 0.0 && self.player_state.hunger > 6.0 {
                self.is_sprinting = true;
            }
            self.w_click_timer = 0.3; // 0.3 seconds window
        }
        self.last_w_pressed = self.keys.w;
        if self.w_click_timer > 0.0 {
            self.w_click_timer -= dt;
        }

        // Ctrl key sprint check
        if self.keys.ctrl && self.keys.w && self.player_state.hunger > 6.0 {
            self.is_sprinting = true;
        }

        // Cancel sprinting conditions
        if !self.keys.w || self.keys.shift || self.player_state.hunger <= 6.0 {
            self.is_sprinting = false;
        }

        // Cancel if player collides with a wall but has movement inputs
        if self.is_sprinting
            && (self.player_physics.velocity.x.abs() < 0.01
                && self.player_physics.velocity.z.abs() < 0.01)
            && (self.keys.w || self.keys.a || self.keys.s || self.keys.d)
        {
            self.is_sprinting = false;
        }

        // Interpolate FOV smoothly
        let target_fov = if self.is_sprinting {
            self.base_fov * 1.12
        } else {
            self.base_fov
        };
        self.camera.fov = self.camera.fov + (target_fov - self.camera.fov) * dt * 10.0;

        // Consume more hunger when sprinting
        if self.is_sprinting && (self.keys.w || self.keys.a || self.keys.s || self.keys.d) {
            self.player_state.add_exhaustion(dt * 0.15);
        }

        // Update game time
        let speed_multiplier = if self.keys.t { 200.0 } else { 1.0 };
        let elapsed_world_ticks = dt * 20.0 * speed_multiplier;
        self.world_time.tick_accumulator += elapsed_world_ticks;
        let new_ticks = self.world_time.tick_accumulator.floor() as u64;
        self.world_time.ticks += new_ticks;
        self.world_time.tick_accumulator -= new_ticks as f32;
        if self.current_dimension == crate::dimension::Dimension::Overworld {
            let weather_update = self.weather.update(elapsed_world_ticks, dt);
            self.update_weather_effects(dt, weather_update.lightning_due);
        } else {
            self.audio_manager.stop_looping_sound(RAIN_LOOP_ID);
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
        let mut movement = move_dir.normalize_or_zero() * self.potion_effects.speed_multiplier();
        if self.keys.space {
            movement.y = 1.0;
        }

        // Jump exhaustion check
        let jumped = self.keys.space && self.player_physics.on_ground;
        if jumped && self.game_mode == GameMode::Survival {
            self.player_state.add_exhaustion(0.05);
        }
        if jumped {
            self.audio_manager.play_sound(crate::audio::SoundId::Jump);
        }

        let old_pos = self.player_physics.position;

        let fall_damage = self.player_physics.update(
            dt,
            &self.chunk_manager,
            movement,
            self.keys.shift,
            self.is_sprinting,
        );
        self.update_chunks();

        // Landing sound
        let px = self.player_physics.position.x.floor() as i32;
        let py = (self.player_physics.position.y - 0.1).floor() as i32;
        let pz = self.player_physics.position.z.floor() as i32;
        let under_block = self.chunk_manager.get_block(px, py, pz);

        if self.player_physics.on_ground && !self.was_on_ground {
            if let Some(mat) = under_block.sound_material() {
                self.audio_manager
                    .play_sound(crate::audio::SoundId::Land(mat));
            }
        }

        // Apply fall damage
        if self.game_mode == GameMode::Survival && fall_damage > 0.0 {
            self.take_damage(fall_damage, DamageSource::Fall);
        }

        // Movement exhaustion check
        let horizontal_dist = glam::Vec2::new(
            self.player_physics.position.x - old_pos.x,
            self.player_physics.position.z - old_pos.z,
        )
        .length();
        if self.game_mode == GameMode::Survival {
            self.player_state.add_exhaustion(0.02 * horizontal_dist);
        }

        // Footstep sound update
        if self.player_physics.on_ground {
            if horizontal_dist > 0.0001 {
                let vel_h = glam::Vec2::new(
                    self.player_physics.velocity.x,
                    self.player_physics.velocity.z,
                )
                .length();
                let step_interval = if vel_h > 5.0 { 1.5 } else { 2.0 };
                self.footstep_accumulator += horizontal_dist;
                if self.footstep_accumulator >= step_interval {
                    self.footstep_accumulator = 0.0;
                    if let Some(mat) = under_block.sound_material() {
                        self.audio_manager
                            .play_sound(crate::audio::SoundId::Footstep(mat));
                    }

                    // Spawn footstep dust particles at the player's feet.
                    if under_block != BlockType::Air {
                        let feet_pos = glam::Vec3::new(
                            self.player_physics.position.x,
                            (self.player_physics.position.y - 0.05).max(0.0),
                            self.player_physics.position.z,
                        );
                        let mut rng = self
                            .total_time
                            .to_bits()
                            .wrapping_add(self.player_physics.position.x.to_bits());
                        crate::particles::spawn_footstep_dust(
                            &mut self.particles,
                            feet_pos,
                            under_block,
                            &mut rng,
                        );
                    }
                }
            }
        } else {
            self.footstep_accumulator = 0.0;
        }

        self.was_on_ground = self.player_physics.on_ground;

        // Torch smoke: periodically scan loaded chunks for torch blocks and
        // spawn a slowly rising smoke particle above each one.
        self.torch_smoke_timer += dt;
        if self.torch_smoke_timer >= 0.4 {
            self.torch_smoke_timer = 0.0;
            let mut rng = self.total_time.to_bits().wrapping_add(0x9E3779B9);
            let chunks: Vec<(i32, i32)> = self.chunk_manager.chunks.keys().copied().collect();
            for (cx, cz) in chunks {
                let chunk = match self.chunk_manager.chunks.get(&(cx, cz)) {
                    Some(c) => c,
                    None => continue,
                };
                // Scan a downsampled subset of columns for torches to keep the
                // cost bounded per frame.
                for bx in 0..16 {
                    for bz in 0..16 {
                        for by in (0..crate::world::CHUNK_HEIGHT).step_by(2) {
                            if chunk.blocks[bx][by][bz] == BlockType::Torch {
                                let wx = cx * crate::world::CHUNK_WIDTH as i32 + bx as i32;
                                let wy = by as i32;
                                let wz = cz * crate::world::CHUNK_DEPTH as i32 + bz as i32;
                                let torch_pos = glam::Vec3::new(
                                    wx as f32 + 0.5,
                                    wy as f32 + 0.6,
                                    wz as f32 + 0.5,
                                );
                                crate::particles::spawn_torch_smoke(
                                    &mut self.particles,
                                    torch_pos,
                                    &mut rng,
                                );
                                rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
                            }
                        }
                    }
                }
            }
        }

        // Dropped item collection: collect any DroppedItem entity within 1.5
        // meters of the player whose pickup cooldown has expired.
        {
            let player_pos = self.player_physics.position;
            let mut to_collect: Vec<usize> = Vec::new();
            for (i, entity) in self.entity_manager.entities.iter().enumerate() {
                if entity.entity_type != crate::entity::EntityType::DroppedItem {
                    continue;
                }
                if entity.pickup_cooldown > 0.0 {
                    continue;
                }
                if entity.dropped_item.is_none() {
                    continue;
                }
                let d = entity.position.distance(player_pos);
                if d < 1.5 {
                    to_collect.push(i);
                }
            }
            // Collect in reverse so indices stay valid as we remove.
            for &i in to_collect.iter().rev() {
                let item = self.entity_manager.entities[i].dropped_item;
                if let Some(item) = item {
                    let added = self.inventory.add_item(item);
                    if added {
                        self.entity_manager.entities.remove(i);
                    }
                }
            }
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

        // Lava damage check
        let px = self.player_physics.position.x.floor() as i32;
        let py = self.player_physics.position.y.floor() as i32;
        let pz = self.player_physics.position.z.floor() as i32;
        let block_at_feet = self.chunk_manager.get_block(px, py, pz);
        let block_at_eyes = self.chunk_manager.get_block(
            px,
            (self.player_physics.position.y + 1.62).floor() as i32,
            pz,
        );
        let player_in_lava = block_at_feet == BlockType::Lava || block_at_eyes == BlockType::Lava;

        if player_in_lava && !self.potion_effects.has_fire_resistance() {
            self.lava_damage_timer += dt;
            if self.lava_damage_timer >= 0.5 {
                self.lava_damage_timer = 0.0;
                self.take_damage(4.0, DamageSource::Mob); // Deal 4.0 damage (2 hearts) every 0.5s
            }
        } else {
            self.lava_damage_timer = 0.0;
        }

        // Leaf Decay Random Ticks
        let chunk_keys: Vec<(i32, i32)> = self.chunk_manager.chunks.keys().cloned().collect();
        if !chunk_keys.is_empty() {
            // Run 30 random ticks per frame
            let mut rng_seed = (self.total_time * 1000.0) as u32;
            let mut next_rand = |max: u32| -> u32 {
                rng_seed = rng_seed.wrapping_mul(1103515245).wrapping_add(12345);
                ((rng_seed / 65536) % 32768) % max
            };

            for _ in 0..30 {
                let chunk_idx = next_rand(chunk_keys.len() as u32) as usize;
                let (cx, cz) = chunk_keys[chunk_idx];

                let rx = next_rand(16) as i32;
                let rz = next_rand(16) as i32;
                let ry = next_rand(120) as i32 + 40; // Leaves usually spawn between Y=40..160

                let wx = cx * 16 + rx;
                let wz = cz * 16 + rz;

                let block = self.chunk_manager.get_block(wx, ry, wz);
                if block == BlockType::OakLeaves
                    || block == BlockType::BirchLeaves
                    || block == BlockType::SpruceLeaves
                {
                    // Run BFS check for log in radius 4
                    let mut queue = std::collections::VecDeque::new();
                    let mut visited = std::collections::HashSet::new();
                    queue.push_back((wx, ry, wz, 0));
                    visited.insert((wx, ry, wz));

                    let mut found_log = false;
                    while let Some((bx, by, bz, dist)) = queue.pop_front() {
                        let b = self.chunk_manager.get_block(bx, by, bz);
                        if b == BlockType::OakLog
                            || b == BlockType::BirchLog
                            || b == BlockType::SpruceLog
                        {
                            found_log = true;
                            break;
                        }
                        if dist < 4 {
                            for (dx, dy, dz) in &[
                                (1, 0, 0),
                                (-1, 0, 0),
                                (0, 1, 0),
                                (0, -1, 0),
                                (0, 0, 1),
                                (0, 0, -1),
                            ] {
                                let nx = bx + dx;
                                let ny = by + dy;
                                let nz = bz + dz;
                                let neighbor_b = self.chunk_manager.get_block(nx, ny, nz);
                                let is_leaf = neighbor_b == BlockType::OakLeaves
                                    || neighbor_b == BlockType::BirchLeaves
                                    || neighbor_b == BlockType::SpruceLeaves;
                                if (is_leaf
                                    || neighbor_b == BlockType::OakLog
                                    || neighbor_b == BlockType::BirchLog
                                    || neighbor_b == BlockType::SpruceLog)
                                    && visited.insert((nx, ny, nz))
                                {
                                    queue.push_back((nx, ny, nz, dist + 1));
                                }
                            }
                        }
                    }

                    if !found_log {
                        self.chunk_manager.set_block(wx, ry, wz, BlockType::Air);
                        // Recalculate lighting & mark dirty meshes
                        let mut dirty_chunks = std::collections::HashSet::new();
                        crate::lighting::update_sky_light_after_removed(
                            &mut self.chunk_manager,
                            wx,
                            ry,
                            wz,
                            &mut dirty_chunks,
                        );
                        mark_block_mesh_dependencies(&mut dirty_chunks, wx, wz);
                        for (dcx, dcz) in dirty_chunks {
                            if let Some(mesh) = self.chunk_meshes.get_mut(&(dcx, dcz)) {
                                mesh.dirty = true;
                            }
                        }
                    }
                }
            }
        }

        // Cactus damage check
        let player_aabb = self.player_physics.get_aabb();
        let min_x = player_aabb.min.x.floor() as i32;
        let max_x = player_aabb.max.x.floor() as i32;
        let min_y =
            (player_aabb.min.y.floor() as i32).clamp(0, crate::world::CHUNK_HEIGHT as i32 - 1);
        let max_y =
            (player_aabb.max.y.floor() as i32).clamp(0, crate::world::CHUNK_HEIGHT as i32 - 1);
        let min_z = player_aabb.min.z.floor() as i32;
        let max_z = player_aabb.max.z.floor() as i32;

        let mut touching_cactus = false;
        for x in min_x..=max_x {
            for y in min_y..=max_y {
                for z in min_z..=max_z {
                    if self.chunk_manager.get_block(x, y, z) == BlockType::Cactus {
                        let block_aabb = AABB::new(
                            Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5),
                            Vec3::ONE,
                        );
                        if player_aabb.intersects(&block_aabb) {
                            touching_cactus = true;
                        }
                    }
                }
            }
        }

        if touching_cactus {
            self.cactus_damage_timer += dt;
            if self.cactus_damage_timer >= 0.5 {
                self.cactus_damage_timer = 0.0;
                self.take_damage(1.0, DamageSource::Mob); // Deal 1.0 contact damage (0.5 heart)
            }
        } else {
            self.cactus_damage_timer = 0.0;
        }

        // Update player state timers & starvation
        let is_underwater = block_at_eyes == BlockType::Water;
        let respiration_level: u8 = self
            .inventory
            .armor
            .iter()
            .flatten()
            .map(|stack| {
                stack
                    .enchantments
                    .level_of(crate::enchantment::Enchantment::Respiration(1))
            })
            .sum();
        let water_breathing = self.potion_effects.has_water_breathing();
        let oxygen_rate = 1.0 / (1.0 + respiration_level as f32);
        if let Some((dmg, src)) = self.player_state.update_with_oxygen_rate(
            dt,
            is_underwater && !water_breathing,
            oxygen_rate,
        ) {
            self.take_damage(dmg, src);
        }

        self.total_time += dt;

        // Peaceful worlds keep passive creatures and dropped items, but remove
        // hostile actors immediately and do not schedule new hostile spawns.
        if self.difficulty == Difficulty::Peaceful {
            self.entity_manager
                .entities
                .retain(|entity| !entity.entity_type.is_hostile());
        } else if self.current_dimension == crate::dimension::Dimension::Overworld {
            crate::mob::spawn_mobs(
                &mut self.entity_manager,
                &self.chunk_manager,
                self.player_physics.position,
                self.world_time.sky_light_level(),
                self.total_time,
            );
        }

        if self.difficulty != Difficulty::Peaceful {
            crate::boss::ensure_dimension_entities(
                self.current_dimension,
                &mut self.entity_manager,
                &self.chunk_manager,
                self.player_physics.position,
                self.total_time,
            );
            let boss_events = crate::boss::update_dimension_entities(
                self.current_dimension,
                &mut self.entity_manager,
                &self.chunk_manager,
                self.player_physics.position,
                dt,
                self.game_mode,
            );
            self.apply_boss_events(boss_events);
        }

        // Update mobs
        self.update_player_projectiles(dt);
        crate::mob::update_mobs(
            &mut self.entity_manager,
            &mut self.chunk_manager,
            &mut self.chunk_meshes,
            &mut self.player_physics,
            &mut self.player_state,
            self.game_mode,
            self.world_time.sky_light_level(),
            dt,
            &mut self.audio_manager,
            right,
            self.potion_effects.has_invisibility(),
            crate::enchantment::protection_multiplier(&self.inventory.armor, false),
        );

        // Update passive mobs
        crate::passive_mob::update_passive_mobs(
            &mut self.entity_manager,
            &mut self.chunk_manager,
            &mut self.chunk_meshes,
            &self.player_physics,
            &mut self.inventory,
            self.game_mode,
            dt,
            self.total_time,
        );

        // Spawn passive mobs (daytime spawn)
        if self.current_dimension == crate::dimension::Dimension::Overworld {
            crate::passive_mob::spawn_passive_mobs(
                &mut self.entity_manager,
                &self.chunk_manager,
                self.player_physics.position,
                self.world_time.sky_light_level(),
                self.total_time,
            );
        }

        // Sync camera position to player position at eye height
        let eye_height = if self.keys.shift { 1.4 } else { 1.6 };
        self.camera.position = self.player_physics.position + Vec3::new(0.0, eye_height, 0.0);
        let is_underwater = self.chunk_manager.get_block(
            self.camera.position.x.floor() as i32,
            self.camera.position.y.floor() as i32,
            self.camera.position.z.floor() as i32,
        ) == BlockType::Water;
        self.camera_uniform.update_view_proj(
            &self.camera,
            self.config.width as f32 / self.config.height as f32,
            self.chunk_manager.render_distance as u32,
            &self.world_time,
            self.total_time,
            is_underwater,
        );
        self.camera_uniform.camera_pos[3] = self.current_dimension as u8 as f32;
        if self.current_dimension == crate::dimension::Dimension::Overworld {
            let weather_brightness = self.weather.sky_brightness();
            for channel in 0..3 {
                self.camera_uniform.sky_color_top[channel] *= weather_brightness;
                self.camera_uniform.sky_color_horizon[channel] *= weather_brightness;
            }
            self.camera_uniform.sun_dir[3] *= weather_brightness;
        } else if self.current_dimension == crate::dimension::Dimension::Nether {
            self.camera_uniform.sky_color_top = [0.16, 0.018, 0.012, 1.0];
            self.camera_uniform.sky_color_horizon = [0.36, 0.055, 0.025, 1.0];
            self.camera_uniform.sun_dir[3] = 0.55;
        } else {
            self.camera_uniform.sky_color_top = [0.003, 0.002, 0.009, 1.0];
            self.camera_uniform.sky_color_horizon = [0.025, 0.006, 0.04, 1.0];
            self.camera_uniform.sun_dir[3] = 0.35;
        }
        if self.potion_effects.has_night_vision() {
            self.camera_uniform.sun_dir[3] = 1.0;
        }
        self.queue.write_buffer(
            &self.camera_buffer,
            0,
            bytemuck::cast_slice(&[self.camera_uniform]),
        );

        // Continuous mining logic
        if self.left_mouse_pressed && self.game_mode == GameMode::Survival {
            let dir = Vec3::new(
                self.camera.yaw.cos() * self.camera.pitch.cos(),
                self.camera.pitch.sin(),
                self.camera.yaw.sin() * self.camera.pitch.cos(),
            )
            .normalize_or_zero();

            if let Some(hit) = raycast(self.camera.position, dir, 5.0, &self.chunk_manager) {
                let target = hit.block_pos;
                let block =
                    self.chunk_manager
                        .get_block(target.x as i32, target.y as i32, target.z as i32);

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

    fn update_weather_effects(&mut self, dt: f32, lightning_due: bool) {
        use crate::weather::Precipitation;

        let player_x = self.player_physics.position.x.floor() as i32;
        let player_z = self.player_physics.position.z.floor() as i32;
        if self.weather.precipitation_at(player_x, player_z) == Precipitation::Rain {
            self.audio_manager.start_looping_sound(
                RAIN_LOOP_ID,
                crate::audio::SoundId::Rain,
                self.player_physics.position,
            );
        } else {
            self.audio_manager.stop_looping_sound(RAIN_LOOP_ID);
        }

        let spawn_count = self.weather.take_precipitation_spawn_count(dt);
        let rain_uv = weather_tile_uv(10, 0);
        let snow_uv = weather_tile_uv(3, 1);
        for _ in 0..spawn_count {
            let wx = player_x + self.weather.random_offset(14);
            let wz = player_z + self.weather.random_offset(14);
            let precipitation = self.weather.precipitation_at(wx, wz);
            if precipitation == Precipitation::None {
                continue;
            }
            let Some(surface_y) = self.surface_height(wx, wz) else {
                continue;
            };
            if surface_y >= CHUNK_HEIGHT as i32 - 2 {
                continue;
            }

            // Start above both the camera and the highest block in this column.
            // Lifetime ends at that height, so precipitation never passes through
            // leaves, terrain, or a player-built roof.
            let spawn_y = (self.camera.position.y + 14.0).max(surface_y as f32 + 10.0);
            let stop_y = surface_y as f32 + 1.05;
            match precipitation {
                Precipitation::Rain => {
                    let speed = 26.0 + self.weather.random_unit() * 8.0;
                    let lifetime = ((spawn_y - stop_y) / speed).clamp(0.08, 2.5);
                    self.particles.spawn_stretched(
                        Vec3::new(wx as f32 + 0.5, spawn_y, wz as f32 + 0.5),
                        Vec3::new(0.0, -speed, 0.0),
                        0.075,
                        lifetime,
                        rain_uv,
                        0.0,
                        7.0,
                    );
                }
                Precipitation::Snow => {
                    let drift_x = (self.weather.random_unit() - 0.5) * 0.8;
                    let drift_z = (self.weather.random_unit() - 0.5) * 0.8;
                    let speed = 2.2 + self.weather.random_unit();
                    let lifetime = ((spawn_y - stop_y) / speed).clamp(0.2, 8.0);
                    self.particles.spawn(
                        Vec3::new(wx as f32 + 0.5, spawn_y, wz as f32 + 0.5),
                        Vec3::new(drift_x, -speed, drift_z),
                        0.16,
                        lifetime,
                        snow_uv,
                        0.0,
                    );
                }
                Precipitation::None => {}
            }
        }

        let accumulation_steps = self.weather.take_snow_accumulation_steps(dt);
        for _ in 0..accumulation_steps * 6 {
            let wx = player_x + self.weather.random_offset(24);
            let wz = player_z + self.weather.random_offset(24);
            if self.weather.precipitation_at(wx, wz) != Precipitation::Snow {
                continue;
            }
            let Some(surface_y) = self.surface_height(wx, wz) else {
                continue;
            };
            let target_y = surface_y + 1;
            if target_y >= CHUNK_HEIGHT as i32
                || self.chunk_manager.get_block(wx, target_y, wz) != BlockType::Air
            {
                continue;
            }
            let support = self.chunk_manager.get_block(wx, surface_y, wz);
            if support.properties().is_solid
                && !matches!(support, BlockType::Water | BlockType::Lava | BlockType::Ice)
            {
                self.apply_weather_block_change(wx, target_y, wz, BlockType::SnowLayer);
            }
        }

        if lightning_due {
            self.strike_lightning();
        }
    }

    fn surface_height(&self, wx: i32, wz: i32) -> Option<i32> {
        let ((cx, cz), (bx, _, bz)) = self.chunk_manager.world_to_local(wx, 0, wz)?;
        self.chunk_manager
            .chunks
            .get(&(cx, cz))
            .map(|chunk| chunk.heightmap[bx][bz] as i32)
    }

    fn strike_lightning(&mut self) {
        use crate::entity::EntityType;

        let player_pos = self.player_physics.position;
        let living_target = self
            .entity_manager
            .entities
            .iter()
            .filter(|entity| {
                entity.health > 0.0
                    && matches!(
                        entity.entity_type,
                        EntityType::Zombie
                            | EntityType::Skeleton
                            | EntityType::Creeper
                            | EntityType::Pig
                            | EntityType::Cow
                            | EntityType::Sheep
                            | EntityType::Chicken
                    )
                    && entity.position.distance_squared(player_pos) <= 32.0 * 32.0
            })
            .min_by(|a, b| {
                a.position
                    .distance_squared(player_pos)
                    .total_cmp(&b.position.distance_squared(player_pos))
            })
            .map(|entity| entity.position);

        let (strike_x, strike_z) = if let Some(target) = living_target {
            (target.x.floor() as i32, target.z.floor() as i32)
        } else {
            (
                player_pos.x.floor() as i32 + self.weather.random_offset(30),
                player_pos.z.floor() as i32 + self.weather.random_offset(30),
            )
        };
        let Some(surface_y) = self.surface_height(strike_x, strike_z) else {
            return;
        };
        let strike_pos = Vec3::new(
            strike_x as f32 + 0.5,
            surface_y as f32 + 1.0,
            strike_z as f32 + 0.5,
        );

        let listener_right =
            Vec3::new(-self.camera.yaw.sin(), 0.0, self.camera.yaw.cos()).normalize_or_zero();
        self.audio_manager.play_sound_3d(
            crate::audio::SoundId::Thunder,
            strike_pos,
            self.camera.position,
            listener_right,
        );

        for entity in &mut self.entity_manager.entities {
            let horizontal = glam::Vec2::new(
                entity.position.x - strike_pos.x,
                entity.position.z - strike_pos.z,
            )
            .length();
            if entity.health > 0.0 && horizontal <= 3.5 {
                entity.health -= 10.0;
                entity.fire_aspect_timer = entity.fire_aspect_timer.max(5.0);
            }
        }
        let player_horizontal =
            glam::Vec2::new(player_pos.x - strike_pos.x, player_pos.z - strike_pos.z).length();
        if player_horizontal <= 3.5 {
            self.take_damage(10.0, DamageSource::Lightning);
        }

        // A short chain of bright, vertically stretched billboards forms the
        // visible bolt and persists just long enough to accompany the flash.
        let bolt_uv = weather_tile_uv(3, 1);
        for segment in 0..12 {
            let jitter_x = (self.weather.random_unit() - 0.5) * 0.55;
            let jitter_z = (self.weather.random_unit() - 0.5) * 0.55;
            self.particles.spawn_stretched(
                strike_pos + Vec3::new(jitter_x, segment as f32 * 3.0 + 1.5, jitter_z),
                Vec3::ZERO,
                0.28,
                0.32,
                bolt_uv,
                0.0,
                12.0,
            );
        }

        let fire_y = surface_y + 1;
        let support = self.chunk_manager.get_block(strike_x, surface_y, strike_z);
        if fire_y < CHUNK_HEIGHT as i32
            && support.properties().is_solid
            && !matches!(
                support,
                BlockType::Water | BlockType::Lava | BlockType::Ice | BlockType::Snow
            )
            && self.chunk_manager.get_block(strike_x, fire_y, strike_z) == BlockType::Air
        {
            self.apply_weather_block_change(strike_x, fire_y, strike_z, BlockType::Fire);
        }
    }

    fn apply_weather_block_change(&mut self, wx: i32, wy: i32, wz: i32, block: BlockType) {
        let old = self.chunk_manager.get_block(wx, wy, wz);
        if old == block {
            return;
        }
        self.chunk_manager.set_block(wx, wy, wz, block);
        self.redstone.on_block_changed(
            &self.chunk_manager,
            (wx, wy, wz),
            crate::redstone::Direction::North,
        );

        let old_properties = old.properties();
        let new_properties = block.properties();
        let mut dirty_chunks = std::collections::HashSet::new();
        if old_properties.is_solid != new_properties.is_solid {
            if new_properties.is_solid {
                crate::lighting::update_sky_light_after_placed(
                    &mut self.chunk_manager,
                    wx,
                    wy,
                    wz,
                    &mut dirty_chunks,
                );
            } else {
                crate::lighting::update_sky_light_after_removed(
                    &mut self.chunk_manager,
                    wx,
                    wy,
                    wz,
                    &mut dirty_chunks,
                );
            }
        }
        if old_properties.light_emission != new_properties.light_emission {
            crate::lighting::update_block_light_after_removed(
                &mut self.chunk_manager,
                wx,
                wy,
                wz,
                old_properties.light_emission,
                &mut dirty_chunks,
            );
            if new_properties.light_emission > 0 {
                crate::lighting::update_block_light_after_placed(
                    &mut self.chunk_manager,
                    wx,
                    wy,
                    wz,
                    new_properties.light_emission,
                    &mut dirty_chunks,
                );
            }
        }
        mark_block_mesh_dependencies(&mut dirty_chunks, wx, wz);
        for chunk_pos in dirty_chunks {
            if let Some(mesh) = self.chunk_meshes.get_mut(&chunk_pos) {
                mesh.dirty = true;
            }
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
            (
                [0.0, 0.0, 1.0],
                [
                    ([offset_min, offset_min, offset_max], [0.0, 1.0]),
                    ([offset_max, offset_min, offset_max], [1.0, 1.0]),
                    ([offset_max, offset_max, offset_max], [1.0, 0.0]),
                    ([offset_min, offset_max, offset_max], [0.0, 0.0]),
                ],
            ),
            // North
            (
                [0.0, 0.0, -1.0],
                [
                    ([offset_max, offset_min, offset_min], [0.0, 1.0]),
                    ([offset_min, offset_min, offset_min], [1.0, 1.0]),
                    ([offset_min, offset_max, offset_min], [1.0, 0.0]),
                    ([offset_max, offset_max, offset_min], [0.0, 0.0]),
                ],
            ),
            // West
            (
                [-1.0, 0.0, 0.0],
                [
                    ([offset_min, offset_min, offset_min], [0.0, 1.0]),
                    ([offset_min, offset_min, offset_max], [1.0, 1.0]),
                    ([offset_min, offset_max, offset_max], [1.0, 0.0]),
                    ([offset_min, offset_max, offset_min], [0.0, 0.0]),
                ],
            ),
            // East
            (
                [1.0, 0.0, 0.0],
                [
                    ([offset_max, offset_min, offset_max], [0.0, 1.0]),
                    ([offset_max, offset_min, offset_min], [1.0, 1.0]),
                    ([offset_max, offset_max, offset_min], [1.0, 0.0]),
                    ([offset_max, offset_max, offset_max], [0.0, 0.0]),
                ],
            ),
            // Up
            (
                [0.0, 1.0, 0.0],
                [
                    ([offset_min, offset_max, offset_max], [0.0, 1.0]),
                    ([offset_max, offset_max, offset_max], [1.0, 1.0]),
                    ([offset_max, offset_max, offset_min], [1.0, 0.0]),
                    ([offset_min, offset_max, offset_min], [0.0, 0.0]),
                ],
            ),
            // Down
            (
                [0.0, -1.0, 0.0],
                [
                    ([offset_min, offset_min, offset_min], [0.0, 1.0]),
                    ([offset_max, offset_min, offset_min], [1.0, 1.0]),
                    ([offset_max, offset_min, offset_max], [1.0, 0.0]),
                    ([offset_min, offset_min, offset_max], [0.0, 0.0]),
                ],
            ),
        ];

        let mut vertices = Vec::new();
        let mut indices = Vec::new();

        let sky_light = self
            .chunk_manager
            .get_sky_light(wx as i32, wy as i32, wz as i32);
        let block_light = self
            .chunk_manager
            .get_block_light(wx as i32, wy as i32, wz as i32);

        for (face_idx, (_normal, corners)) in faces.iter().enumerate() {
            let start_idx = vertices.len() as u32;
            let multiplier_code = match face_idx {
                4 => 0.0, // Top
                5 => 2.0, // Bottom
                _ => 1.0, // Sides
            };
            let light_val =
                (sky_light as f32) + (block_light as f32) * 16.0 + multiplier_code * 256.0;

            for &(corner, uv) in corners {
                // UV points to Row 15, Col "stage"
                let u = (uv[0] + stage as f32) * 0.0625;
                let v = (uv[1] + 15.0) * 0.0625;
                vertices.push(Vertex {
                    position: [wx + corner[0], wy + corner[1], wz + corner[2]],
                    tex_coords: [u, v],
                    light_level: light_val,
                    ao: 1.0,
                });
            }

            indices.push(start_idx + 0);
            indices.push(start_idx + 1);
            indices.push(start_idx + 2);
            indices.push(start_idx + 0);
            indices.push(start_idx + 2);
            indices.push(start_idx + 3);
        }

        self.queue.write_buffer(
            &self.crack_vertex_buffer,
            0,
            bytemuck::cast_slice(&vertices),
        );
        self.queue
            .write_buffer(&self.crack_index_buffer, 0, bytemuck::cast_slice(&indices));

        Some((vertices.len() as u32, indices.len() as u32))
    }

    pub fn calculate_mining_time(&self, block: BlockType) -> f32 {
        let hardness = block.properties().hardness;
        if hardness < 0.0 {
            return f32::MAX; // Unbreakable (e.g. bedrock)
        }

        let held_stack = self.inventory.hotbar[self.inventory.selected];
        let held_item = held_stack.map(|s| s.item).unwrap_or(Item::Air);
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

        let enchantment_multiplier = held_stack
            .map(|stack| crate::enchantment::mining_speed_multiplier(&stack.enchantments))
            .unwrap_or(1.0);
        base_time / (speed_multiplier * enchantment_multiplier)
    }

    fn damage_selected_tool(&mut self, salt: u32) {
        if self.game_mode == GameMode::Creative {
            return;
        }
        let selected = self.inventory.selected;
        let should_damage = self.inventory.hotbar[selected]
            .filter(|stack| stack.item.tool_properties().is_some())
            .is_some_and(|stack| {
                crate::enchantment::should_consume_durability(&stack.enchantments, salt)
            });
        if !should_damage {
            return;
        }
        if let Some(stack) = &mut self.inventory.hotbar[selected] {
            if stack.durability > 1 {
                stack.durability -= 1;
            } else {
                println!("[Debug] Tool broke: {:?}", stack.item);
                self.inventory.hotbar[selected] = None;
            }
        }
    }

    fn apply_redstone_update(&mut self, update: crate::redstone::RedstoneUpdate) {
        let mut dirty_chunks = std::collections::HashSet::new();
        for mutation in update.mutations {
            let (wx, wy, wz) = mutation.pos;
            let old_properties = mutation.old_block.properties();
            let new_properties = mutation.new_block.properties();

            if old_properties.is_solid != new_properties.is_solid {
                if new_properties.is_solid {
                    crate::lighting::update_sky_light_after_placed(
                        &mut self.chunk_manager,
                        wx,
                        wy,
                        wz,
                        &mut dirty_chunks,
                    );
                } else {
                    crate::lighting::update_sky_light_after_removed(
                        &mut self.chunk_manager,
                        wx,
                        wy,
                        wz,
                        &mut dirty_chunks,
                    );
                }
            }
            if old_properties.light_emission != new_properties.light_emission {
                crate::lighting::update_block_light_after_removed(
                    &mut self.chunk_manager,
                    wx,
                    wy,
                    wz,
                    old_properties.light_emission,
                    &mut dirty_chunks,
                );
                if new_properties.light_emission > 0 {
                    crate::lighting::update_block_light_after_placed(
                        &mut self.chunk_manager,
                        wx,
                        wy,
                        wz,
                        new_properties.light_emission,
                        &mut dirty_chunks,
                    );
                }
            }
            mark_block_mesh_dependencies(&mut dirty_chunks, wx, wz);
        }

        for (dcx, dcz) in dirty_chunks {
            if let Some(mesh) = self.chunk_meshes.get_mut(&(dcx, dcz)) {
                mesh.dirty = true;
            }
        }

        for action in update.actions {
            match action {
                crate::redstone::RedstoneAction::Explode { pos } => {
                    let center =
                        Vec3::new(pos.0 as f32 + 0.5, pos.1 as f32 + 0.5, pos.2 as f32 + 0.5);
                    crate::mob::explode(
                        center,
                        4.0,
                        &mut self.chunk_manager,
                        &mut self.chunk_meshes,
                        &mut self.player_physics,
                        &mut self.player_state,
                        self.game_mode,
                        1.0,
                    );
                    self.audio_manager
                        .play_sound(crate::audio::SoundId::Explosion);
                }
                crate::redstone::RedstoneAction::Dispense {
                    pos,
                    facing,
                    dropper,
                } => {
                    let delta = facing.delta();
                    let spawn_pos = Vec3::new(
                        pos.0 as f32 + 0.5 + delta.0 as f32 * 0.7,
                        pos.1 as f32 + 0.5,
                        pos.2 as f32 + 0.5 + delta.2 as f32 * 0.7,
                    );
                    if dropper {
                        self.spawn_dropped_item(Item::Redstone, spawn_pos);
                    } else {
                        let id = self
                            .entity_manager
                            .spawn(crate::entity::EntityType::Arrow, spawn_pos);
                        if let Some(arrow) = self
                            .entity_manager
                            .entities
                            .iter_mut()
                            .find(|entity| entity.id == id)
                        {
                            arrow.velocity = Vec3::new(delta.0 as f32, 0.0, delta.2 as f32) * 18.0;
                            arrow.friendly_projectile = true;
                            arrow.projectile_damage = 4.0;
                        }
                        self.audio_manager
                            .play_sound(crate::audio::SoundId::ArrowShoot);
                    }
                }
                crate::redstone::RedstoneAction::PlayNote { pos, note } => {
                    let sound_pos =
                        Vec3::new(pos.0 as f32 + 0.5, pos.1 as f32 + 0.5, pos.2 as f32 + 0.5);
                    let listener_right =
                        Vec3::new(-self.camera.yaw.sin(), 0.0, self.camera.yaw.cos())
                            .normalize_or_zero();
                    self.audio_manager.play_sound_3d(
                        crate::audio::SoundId::Note(note),
                        sound_pos,
                        self.camera.position,
                        listener_right,
                    );
                }
            }
        }

        if update.propagation_overflowed {
            eprintln!("[Redstone] propagation pass limit reached; continuing next tick");
        }
    }

    pub fn break_block(&mut self, pos: glam::Vec3) {
        let wx = pos.x as i32;
        let wy = pos.y as i32;
        let wz = pos.z as i32;
        let old_block = self.chunk_manager.get_block(wx, wy, wz);
        if old_block == BlockType::Air {
            return;
        }

        self.chunk_manager.set_block(wx, wy, wz, BlockType::Air);
        self.redstone.on_block_changed(
            &self.chunk_manager,
            (wx, wy, wz),
            crate::redstone::Direction::North,
        );
        println!("[Debug] Block mined at ({}, {}, {})", wx, wy, wz);

        let sound_pos = glam::Vec3::new(wx as f32 + 0.5, wy as f32 + 0.5, wz as f32 + 0.5);
        let listener_right =
            glam::Vec3::new(-self.camera.yaw.sin(), 0.0, self.camera.yaw.cos()).normalize_or_zero();
        if let Some(mat) = old_block.sound_material() {
            self.audio_manager.play_sound_3d(
                crate::audio::SoundId::BlockBreak(mat),
                sound_pos,
                self.camera.position,
                listener_right,
            );
        }

        // Spawn block-break debris particles (15-25 small quads textured from
        // the broken block's atlas tile).
        {
            let mut rng = (wx as u32)
                .wrapping_mul(2654435761)
                .wrapping_add(wy as u32)
                .wrapping_mul(40503)
                .wrapping_add(wz as u32)
                .wrapping_add(self.total_time.to_bits());
            let count = 15 + (rng % 11) as usize;
            crate::particles::spawn_block_debris(
                &mut self.particles,
                sound_pos,
                old_block,
                count,
                &mut rng,
            );
        }

        // Survival drops check
        if self.game_mode == GameMode::Survival {
            let mut eligible_to_harvest = true;
            if let Some(min_material) = old_block.min_harvest_material() {
                let held_item = self.inventory.hotbar[self.inventory.selected]
                    .map(|s| s.item)
                    .unwrap_or(Item::Air);
                if let Some(tool_prop) = held_item.tool_properties() {
                    eligible_to_harvest = tool_prop.tool_type == old_block.preferred_tool()
                        && tool_prop.material >= min_material;
                } else {
                    eligible_to_harvest = false;
                }
            }

            if eligible_to_harvest {
                let held_enchantments = self.inventory.hotbar[self.inventory.selected]
                    .map(|stack| stack.enchantments)
                    .unwrap_or_default();
                let silk_touch =
                    held_enchantments.level_of(crate::enchantment::Enchantment::SilkTouch) > 0;
                let fortune =
                    held_enchantments.level_of(crate::enchantment::Enchantment::Fortune(1)) as u32;
                let is_any_leaves = old_block == BlockType::OakLeaves
                    || old_block == BlockType::BirchLeaves
                    || old_block == BlockType::SpruceLeaves;
                if silk_touch {
                    self.spawn_dropped_item(Item::from_block(old_block), sound_pos);
                } else if is_any_leaves {
                    let mut rng_seed = (wx as u32)
                        .wrapping_mul(31)
                        .wrapping_add(wy as u32)
                        .wrapping_mul(17)
                        .wrapping_add(wz as u32);
                    let mut next_rand = || {
                        rng_seed = rng_seed.wrapping_mul(1103515245).wrapping_add(12345);
                        (rng_seed / 65536) % 32768
                    };
                    if next_rand() % 10 == 0 {
                        self.spawn_dropped_item(crate::inventory::Item::Apple, sound_pos);
                    } else {
                        self.spawn_dropped_item(
                            crate::inventory::Item::from_block(old_block),
                            sound_pos,
                        );
                    }
                } else if old_block == BlockType::TallGrass {
                    let mut rng_seed = (wx as u32)
                        .wrapping_mul(31)
                        .wrapping_add(wy as u32)
                        .wrapping_mul(17)
                        .wrapping_add(wz as u32);
                    let mut next_rand = || {
                        rng_seed = rng_seed.wrapping_mul(1103515245).wrapping_add(12345);
                        (rng_seed / 65536) % 32768
                    };
                    if next_rand() % 8 == 0 {
                        // 12.5% chance to drop seed
                        self.spawn_dropped_item(crate::inventory::Item::Seeds, sound_pos);
                    }
                } else {
                    let base_drop = match old_block {
                        BlockType::CoalOre => Item::Coal,
                        BlockType::DiamondOre => Item::Diamond,
                        BlockType::RedstoneOre => Item::Redstone,
                        _ => Item::from_block(old_block),
                    };
                    let fortune_eligible = matches!(
                        old_block,
                        BlockType::CoalOre | BlockType::DiamondOre | BlockType::RedstoneOre
                    );
                    let bonus = if fortune_eligible && fortune > 0 {
                        ((wx as u32)
                            .wrapping_mul(31)
                            .wrapping_add(wy as u32 * 17)
                            .wrapping_add(wz as u32 * 13)
                            % (fortune + 1))
                            + fortune / 2
                    } else {
                        0
                    };
                    for _ in 0..(1 + bonus) {
                        self.spawn_dropped_item(base_drop, sound_pos);
                    }
                }
            }

            if matches!(
                old_block,
                BlockType::CoalOre
                    | BlockType::IronOre
                    | BlockType::GoldOre
                    | BlockType::DiamondOre
                    | BlockType::RedstoneOre
            ) {
                let xp = if old_block == BlockType::DiamondOre {
                    5
                } else {
                    2
                };
                self.player_state.add_experience(xp);
                if old_block == BlockType::RedstoneOre && ((wx ^ wy ^ wz) & 1) == 0 {
                    self.spawn_dropped_item(Item::LapisLazuli, sound_pos);
                }
            }

            self.player_state.add_exhaustion(0.005);

            self.damage_selected_tool(
                (wx as u32) ^ (wy as u32).rotate_left(11) ^ (wz as u32).rotate_left(22),
            );
        }

        // recalculate lighting and redraw chunk
        let mut dirty_chunks = std::collections::HashSet::new();
        crate::lighting::update_sky_light_after_removed(
            &mut self.chunk_manager,
            wx,
            wy,
            wz,
            &mut dirty_chunks,
        );
        crate::lighting::update_block_light_after_removed(
            &mut self.chunk_manager,
            wx,
            wy,
            wz,
            old_block.properties().light_emission,
            &mut dirty_chunks,
        );

        mark_block_mesh_dependencies(&mut dirty_chunks, wx, wz);
        self.check_and_break_unsupported_above(wx, wy, wz, &mut dirty_chunks);

        for (dcx, dcz) in dirty_chunks {
            if let Some(mesh) = self.chunk_meshes.get_mut(&(dcx, dcz)) {
                mesh.dirty = true;
            }
        }
    }

    /// Spawn a `DroppedItem` entity in the world carrying the given `Item`.
    /// The item is launched with a small random upward velocity and given a
    /// brief pickup cooldown so it can't be instantly re-collected.
    pub fn spawn_dropped_item(&mut self, item: crate::inventory::Item, pos: glam::Vec3) {
        if item == Item::Air {
            return;
        }
        let id = self
            .entity_manager
            .spawn(crate::entity::EntityType::DroppedItem, pos);
        if let Some(entity) = self.entity_manager.entities.last_mut() {
            entity.dropped_item = Some(item);
            // Small random initial upward velocity plus a little horizontal
            // scatter so stacks don't overlap perfectly.
            let mut rng = self
                .total_time
                .to_bits()
                .wrapping_add((id.wrapping_mul(2654435761)) as u32);
            rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
            let vx = ((rng / 65536) as f32 / 32768.0 - 0.5) * 1.5;
            rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
            let vz = ((rng / 65536) as f32 / 32768.0 - 0.5) * 1.5;
            rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
            let vy = 2.0 + ((rng / 65536) as f32 / 32768.0) * 1.0;
            entity.velocity = glam::Vec3::new(vx, vy, vz);
            entity.pickup_cooldown = 0.5;
        }
    }

    fn update_player_projectiles(&mut self, dt: f32) {
        let mut splashes = Vec::new();
        for projectile in &mut self.entity_manager.entities {
            if projectile.entity_type != crate::entity::EntityType::SplashPotion {
                continue;
            }
            projectile.update_physics(dt, &self.chunk_manager);
            projectile.life_time -= dt;
            let pos = projectile.position;
            let hit_block = self
                .chunk_manager
                .get_block(
                    pos.x.floor() as i32,
                    pos.y.floor() as i32,
                    pos.z.floor() as i32,
                )
                .properties()
                .is_solid;
            if hit_block || projectile.life_time <= 0.0 {
                if let Some(potion) = projectile.potion {
                    splashes.push((pos, potion));
                }
                projectile.health = -1.0;
            }
        }

        for (position, potion) in splashes {
            if position.distance(self.player_physics.position) <= 4.0 {
                let healing = self.potion_effects.apply(potion);
                self.player_state.health =
                    (self.player_state.health + healing).min(self.player_state.max_health);
            }
            for entity in &mut self.entity_manager.entities {
                if entity.position.distance(position) > 4.0 {
                    continue;
                }
                match potion.kind {
                    crate::brewing::PotionKind::Healing
                    | crate::brewing::PotionKind::Regeneration => {
                        entity.health =
                            (entity.health + 4.0 * potion.level as f32).min(entity.max_health);
                    }
                    crate::brewing::PotionKind::Poison => {
                        entity.health -= 2.0 * potion.level as f32
                    }
                    crate::brewing::PotionKind::Slowness => entity.velocity *= 0.4,
                    _ => {}
                }
            }
        }

        let mut hits = Vec::new();
        for projectile in &self.entity_manager.entities {
            if projectile.entity_type != crate::entity::EntityType::Arrow
                || !projectile.friendly_projectile
            {
                continue;
            }
            for target in &self.entity_manager.entities {
                if target.id != projectile.id
                    && !matches!(
                        target.entity_type,
                        crate::entity::EntityType::Arrow
                            | crate::entity::EntityType::SplashPotion
                            | crate::entity::EntityType::DroppedItem
                            | crate::entity::EntityType::HeartParticle
                    )
                    && projectile.get_aabb().intersects(&target.get_aabb())
                {
                    hits.push((projectile.id, target.id, projectile.projectile_damage));
                    break;
                }
            }
        }
        for (projectile_id, target_id, damage) in hits {
            if let Some(target) = self
                .entity_manager
                .entities
                .iter_mut()
                .find(|entity| entity.id == target_id)
            {
                target.health -= damage;
            }
            if let Some(projectile) = self
                .entity_manager
                .entities
                .iter_mut()
                .find(|entity| entity.id == projectile_id)
            {
                projectile.health = -1.0;
            }
        }
        self.entity_manager.entities.retain(|entity| {
            entity.health >= 0.0
                || matches!(
                    entity.entity_type,
                    crate::entity::EntityType::Blaze
                        | crate::entity::EntityType::Piglin
                        | crate::entity::EntityType::Husk
                        | crate::entity::EntityType::Shulker
                        | crate::entity::EntityType::EnderDragon
                        | crate::entity::EntityType::Wither
                        | crate::entity::EntityType::EndCrystal
                )
        });
    }

    pub fn take_damage(&mut self, amount: f32, source: DamageSource) {
        if self.game_mode == GameMode::Creative {
            return;
        }

        let can_damage = !self.player_state.is_dead && self.player_state.invulnerable_time <= 0.0;
        let reduced = amount
            * crate::enchantment::protection_multiplier(
                &self.inventory.armor,
                source == DamageSource::Fall,
            );
        let died = self.player_state.take_damage(reduced, source);

        if can_damage {
            if died {
                self.audio_manager
                    .play_sound(crate::audio::SoundId::PlayerDeath);
                println!("[Debug] Player died due to: {:?}", source);
                self.inventory.clear();

                // Release cursor grab immediately on death so player can click Respawn
                let _ = self
                    .window
                    .set_cursor_grab(winit::window::CursorGrabMode::None);
                self.window.set_cursor_visible(true);
                self.keys = KeyState::default();
            } else {
                self.audio_manager
                    .play_sound(crate::audio::SoundId::PlayerHurt);
            }
        }
    }

    pub fn respawn(&mut self) {
        if self.current_dimension != crate::dimension::Dimension::Overworld {
            self.switch_dimension(crate::dimension::Dimension::Overworld);
        }
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
        let _ = self
            .window
            .set_cursor_grab(winit::window::CursorGrabMode::Locked)
            .or_else(|_| {
                self.window
                    .set_cursor_grab(winit::window::CursorGrabMode::Confined)
            });
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
            let held_stack = self.inventory.hotbar[self.inventory.selected];
            let held_item = held_stack
                .map(|s| s.item)
                .unwrap_or(crate::inventory::Item::Air);
            if let Some(potion) = held_stack.and_then(|stack| stack.potion) {
                if potion.splash || held_item == Item::SplashPotion {
                    let dir = Vec3::new(
                        self.camera.yaw.cos() * self.camera.pitch.cos(),
                        self.camera.pitch.sin(),
                        self.camera.yaw.sin() * self.camera.pitch.cos(),
                    )
                    .normalize_or_zero();
                    let id = self.entity_manager.spawn(
                        crate::entity::EntityType::SplashPotion,
                        self.camera.position + dir * 0.5,
                    );
                    if let Some(projectile) = self
                        .entity_manager
                        .entities
                        .iter_mut()
                        .find(|entity| entity.id == id)
                    {
                        projectile.velocity = dir * 12.0;
                        projectile.potion = Some(potion);
                        projectile.life_time = 3.0;
                    }
                } else {
                    let healing = self.potion_effects.apply(potion);
                    self.player_state.health =
                        (self.player_state.health + healing).min(self.player_state.max_health);
                }
                self.inventory
                    .use_selected_item(self.game_mode == GameMode::Creative);
                return;
            }
            if held_item == Item::MilkBucket {
                self.potion_effects.active.clear();
                if self.game_mode == GameMode::Survival {
                    self.inventory.replace_selected_item(Item::Bucket);
                }
                return;
            }
            if held_item == Item::Bow {
                let enchantments = held_stack
                    .map(|stack| stack.enchantments)
                    .unwrap_or_default();
                let infinity = enchantments.level_of(crate::enchantment::Enchantment::Infinity) > 0;
                if self.game_mode == GameMode::Creative
                    || infinity
                    || self.inventory.remove_one(Item::Arrow)
                {
                    let dir = Vec3::new(
                        self.camera.yaw.cos() * self.camera.pitch.cos(),
                        self.camera.pitch.sin(),
                        self.camera.yaw.sin() * self.camera.pitch.cos(),
                    )
                    .normalize_or_zero();
                    let id = self.entity_manager.spawn(
                        crate::entity::EntityType::Arrow,
                        self.camera.position + dir * 0.6,
                    );
                    if let Some(arrow) = self
                        .entity_manager
                        .entities
                        .iter_mut()
                        .find(|entity| entity.id == id)
                    {
                        arrow.velocity = dir * 22.0;
                        arrow.friendly_projectile = true;
                        arrow.projectile_damage = 4.0
                            + enchantments.level_of(crate::enchantment::Enchantment::Power(1))
                                as f32
                                * 1.25;
                    }
                }
                return;
            }
            if held_item == crate::inventory::Item::Apple
                || held_item == crate::inventory::Item::Bread
            {
                if self.player_state.hunger < 20.0 || self.game_mode == GameMode::Creative {
                    let (heal_hunger, heal_saturation) = match held_item {
                        crate::inventory::Item::Apple => (4.0, 2.4),
                        crate::inventory::Item::Bread => (5.0, 6.0),
                        _ => (0.0, 0.0),
                    };
                    self.player_state.hunger = (self.player_state.hunger + heal_hunger).min(20.0);
                    self.player_state.saturation = (self.player_state.saturation + heal_saturation)
                        .min(self.player_state.hunger);

                    let is_creative = self.game_mode == GameMode::Creative;
                    self.inventory.use_selected_item(is_creative);

                    println!(
                        "[Debug] Ate {:?}, hunger={:.1}, saturation={:.1}",
                        held_item, self.player_state.hunger, self.player_state.saturation
                    );
                    return;
                }
            }
        }

        let dir = Vec3::new(
            self.camera.yaw.cos() * self.camera.pitch.cos(),
            self.camera.pitch.sin(),
            self.camera.yaw.sin() * self.camera.pitch.cos(),
        )
        .normalize_or_zero();

        // 1. Raycast against entities first for left-clicks
        if is_left_click {
            let mut closest_entity: Option<(u64, f32)> = None;
            for entity in &self.entity_manager.entities {
                if entity.entity_type == crate::entity::EntityType::Arrow {
                    continue;
                }
                let aabb = entity.get_aabb();
                if let Some(dist) =
                    crate::entity::ray_intersects_aabb(self.camera.position, dir, &aabb)
                {
                    if dist <= 4.0 {
                        if let Some((_, closest_dist)) = closest_entity {
                            if dist < closest_dist {
                                closest_entity = Some((entity.id, dist));
                            }
                        } else {
                            closest_entity = Some((entity.id, dist));
                        }
                    }
                }
            }

            if let Some((entity_id, _)) = closest_entity {
                if let Some(entity) = self
                    .entity_manager
                    .entities
                    .iter_mut()
                    .find(|e| e.id == entity_id)
                {
                    if entity.invulnerable_time <= 0.0 {
                        let held_stack = self.inventory.hotbar[self.inventory.selected];
                        let held_item = held_stack
                            .map(|s| s.item)
                            .unwrap_or(crate::inventory::Item::Air);
                        let enchantments = held_stack.map(|s| s.enchantments).unwrap_or_default();
                        let damage = held_item.tool_properties().map(|t| t.damage).unwrap_or(1.0)
                            + crate::enchantment::attack_damage_bonus(&enchantments)
                            + self.potion_effects.strength_bonus();
                        let knockback = 8.0
                            + enchantments.level_of(crate::enchantment::Enchantment::Knockback(1))
                                as f32
                                * 3.0;

                        entity.health -= damage;
                        entity.invulnerable_time = 0.4;
                        entity.velocity += dir * knockback + Vec3::new(0.0, 3.0, 0.0);
                        let fire_level =
                            enchantments.level_of(crate::enchantment::Enchantment::FireAspect(1));
                        if fire_level > 0 {
                            entity.fire_aspect_timer =
                                entity.fire_aspect_timer.max(fire_level as f32 * 4.0);
                        }

                        println!(
                            "[Debug] Hit {:?}, health={:.1}",
                            entity.entity_type, entity.health
                        );

                        if entity.health <= 0.0 {
                            println!("[Debug] Killed {:?}", entity.entity_type);
                            if self.game_mode == GameMode::Survival {
                                let looting = enchantments
                                    .level_of(crate::enchantment::Enchantment::Looting(1));
                                for _ in 0..=(looting / 2) {
                                    match entity.entity_type {
                                        crate::entity::EntityType::Zombie => {
                                            self.inventory
                                                .add_item(crate::inventory::Item::RottenFlesh);
                                        }
                                        crate::entity::EntityType::Skeleton => {
                                            self.inventory.add_item(crate::inventory::Item::Bone);
                                            self.inventory.add_item(crate::inventory::Item::Arrow);
                                            let mut rng_seed = (entity.position.x as u32)
                                                .wrapping_mul(31)
                                                .wrapping_add(entity.position.z as u32);
                                            let mut next_rand = || {
                                                rng_seed = rng_seed
                                                    .wrapping_mul(1103515245)
                                                    .wrapping_add(12345);
                                                (rng_seed / 65536) % 32768
                                            };
                                            if next_rand() % 10 == 0 {
                                                self.inventory
                                                    .add_item(crate::inventory::Item::Bow);
                                            }
                                        }
                                        crate::entity::EntityType::Creeper => {
                                            self.inventory
                                                .add_item(crate::inventory::Item::Gunpowder);
                                        }
                                        crate::entity::EntityType::Pig => {
                                            let is_on_fire = entity.burn_timer > 0.0
                                                || entity.fire_aspect_timer > 0.0;
                                            let drop = if is_on_fire {
                                                crate::inventory::Item::CookedPorkchop
                                            } else {
                                                crate::inventory::Item::RawPorkchop
                                            };
                                            self.inventory.add_item(drop);
                                        }
                                        crate::entity::EntityType::Cow => {
                                            self.inventory
                                                .add_item(crate::inventory::Item::RawBeef);
                                            let rng = (entity.position.x as u32).wrapping_mul(31);
                                            if rng % 2 == 0 {
                                                self.inventory
                                                    .add_item(crate::inventory::Item::Leather);
                                            }
                                        }
                                        crate::entity::EntityType::Sheep => {
                                            self.inventory
                                                .add_item(crate::inventory::Item::RawMutton);
                                            if entity.has_wool {
                                                self.inventory
                                                    .add_item(crate::inventory::Item::Wool);
                                            }
                                        }
                                        crate::entity::EntityType::Chicken => {
                                            self.inventory
                                                .add_item(crate::inventory::Item::RawChicken);
                                            self.inventory
                                                .add_item(crate::inventory::Item::Feather);
                                        }
                                        _ => {}
                                    }
                                }
                                self.player_state.add_experience(match entity.entity_type {
                                    crate::entity::EntityType::Zombie
                                    | crate::entity::EntityType::Skeleton
                                    | crate::entity::EntityType::Creeper => 5,
                                    _ => 2,
                                });
                            }
                        }

                        self.damage_selected_tool(entity_id as u32 ^ self.total_time.to_bits());

                        return;
                    }
                }
            }
        }

        if !is_left_click {
            let mut closest_entity: Option<(u64, f32)> = None;
            for entity in &self.entity_manager.entities {
                if entity.entity_type == crate::entity::EntityType::Arrow
                    || entity.entity_type == crate::entity::EntityType::HeartParticle
                {
                    continue;
                }
                let aabb = entity.get_aabb();
                if let Some(dist) =
                    crate::entity::ray_intersects_aabb(self.camera.position, dir, &aabb)
                {
                    if dist <= 4.0 {
                        if let Some((_, closest_dist)) = closest_entity {
                            if dist < closest_dist {
                                closest_entity = Some((entity.id, dist));
                            }
                        } else {
                            closest_entity = Some((entity.id, dist));
                        }
                    }
                }
            }

            if let Some((entity_id, _)) = closest_entity {
                if let Some(entity) = self
                    .entity_manager
                    .entities
                    .iter_mut()
                    .find(|e| e.id == entity_id)
                {
                    let held_stack = self.inventory.hotbar[self.inventory.selected].clone();
                    let held_item = held_stack
                        .map(|s| s.item)
                        .unwrap_or(crate::inventory::Item::Air);

                    match entity.entity_type {
                        crate::entity::EntityType::Pig => {
                            if held_item == crate::inventory::Item::Carrot
                                && entity.age >= 0.0
                                && entity.breeding_timer <= 0.0
                                && entity.breed_cooldown <= 0.0
                            {
                                entity.breeding_timer = 20.0;
                                self.inventory.remove_selected_item(1);
                                println!("[Debug] Pig entered love mode!");
                                return;
                            }
                        }
                        crate::entity::EntityType::Cow => {
                            if held_item == crate::inventory::Item::Wheat
                                && entity.age >= 0.0
                                && entity.breeding_timer <= 0.0
                                && entity.breed_cooldown <= 0.0
                            {
                                entity.breeding_timer = 20.0;
                                self.inventory.remove_selected_item(1);
                                println!("[Debug] Cow entered love mode!");
                                return;
                            }
                            if held_item == crate::inventory::Item::Bucket {
                                self.inventory
                                    .replace_selected_item(crate::inventory::Item::MilkBucket);
                                println!("[Debug] Milked a Cow!");
                                return;
                            }
                        }
                        crate::entity::EntityType::Sheep => {
                            if held_item == crate::inventory::Item::Wheat
                                && entity.age >= 0.0
                                && entity.breeding_timer <= 0.0
                                && entity.breed_cooldown <= 0.0
                            {
                                entity.breeding_timer = 20.0;
                                self.inventory.remove_selected_item(1);
                                println!("[Debug] Sheep entered love mode!");
                                return;
                            }
                            if held_item == crate::inventory::Item::Shears && entity.has_wool {
                                entity.has_wool = false;
                                self.inventory.add_item(crate::inventory::Item::Wool);
                                println!("[Debug] Sheared a Sheep!");
                                if let Some(stack) =
                                    &mut self.inventory.hotbar[self.inventory.selected]
                                {
                                    if stack.durability > 1 {
                                        stack.durability -= 1;
                                    } else {
                                        self.inventory.hotbar[self.inventory.selected] = None;
                                    }
                                }
                                return;
                            }
                        }
                        crate::entity::EntityType::Chicken => {
                            if held_item == crate::inventory::Item::Seeds
                                && entity.age >= 0.0
                                && entity.breeding_timer <= 0.0
                                && entity.breed_cooldown <= 0.0
                            {
                                entity.breeding_timer = 20.0;
                                self.inventory.remove_selected_item(1);
                                println!("[Debug] Chicken entered love mode!");
                                return;
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        if let Some(hit) = raycast(self.camera.position, dir, 5.0, &self.chunk_manager) {
            let target = if is_left_click {
                hit.block_pos
            } else {
                let clicked_block = self.chunk_manager.get_block(
                    hit.block_pos.x as i32,
                    hit.block_pos.y as i32,
                    hit.block_pos.z as i32,
                );
                let held = self.inventory.hotbar[self.inventory.selected];
                let clicked_pos = (
                    hit.block_pos.x as i32,
                    hit.block_pos.y as i32,
                    hit.block_pos.z as i32,
                );
                let held_item = held.map(|stack| stack.item).unwrap_or(Item::Air);
                if clicked_block == BlockType::Obsidian && held_item == Item::FlintAndSteel {
                    if let Some(interior) =
                        crate::dimension::detect_nether_frame(clicked_pos, |x, y, z| {
                            self.chunk_manager.get_block(x, y, z)
                        })
                    {
                        let changes: Vec<_> = interior
                            .into_iter()
                            .map(|position| (position, BlockType::NetherPortal))
                            .collect();
                        self.apply_block_changes(&changes);
                        self.inventory
                            .use_selected_item(self.game_mode == GameMode::Creative);
                        return;
                    }
                }
                if clicked_block == BlockType::EndPortalFrame && held_item == Item::EyeOfEnder {
                    self.apply_block_changes(&[(clicked_pos, BlockType::EndPortalFrameFilled)]);
                    self.inventory
                        .use_selected_item(self.game_mode == GameMode::Creative);
                    if let Some(interior) =
                        crate::dimension::detect_completed_end_portal(clicked_pos, |x, y, z| {
                            self.chunk_manager.get_block(x, y, z)
                        })
                    {
                        let changes: Vec<_> = interior
                            .into_iter()
                            .map(|position| (position, BlockType::EndPortal))
                            .collect();
                        self.apply_block_changes(&changes);
                    }
                    return;
                }
                if matches!(clicked_block, BlockType::Obsidian | BlockType::Bedrock)
                    && held_item == Item::EndCrystal
                {
                    self.entity_manager.spawn(
                        crate::entity::EntityType::EndCrystal,
                        Vec3::new(
                            clicked_pos.0 as f32 + 0.5,
                            clicked_pos.1 as f32 + 1.0,
                            clicked_pos.2 as f32 + 0.5,
                        ),
                    );
                    self.inventory
                        .use_selected_item(self.game_mode == GameMode::Creative);
                    return;
                }
                if clicked_block == BlockType::EndCityChest {
                    self.spawn_dropped_item(Item::Elytra, hit.block_pos + Vec3::Y);
                    self.apply_block_changes(&[(clicked_pos, BlockType::Air)]);
                    return;
                }
                if clicked_block == BlockType::Water
                    && held.is_some_and(|stack| stack.item == Item::GlassBottle)
                {
                    self.inventory
                        .use_selected_item(self.game_mode == GameMode::Creative);
                    let mut water_bottle = ItemStack::new(Item::Potion, 1);
                    water_bottle.potion = Some(crate::brewing::PotionData::water());
                    self.inventory.add_stack(water_bottle);
                    return;
                }
                if clicked_block == BlockType::CraftingTable {
                    self.inventory.is_table_open = true;
                    self.inventory.craft_input = vec![None; 9];
                    self.open_inventory();
                    return;
                }
                if matches!(
                    clicked_block,
                    BlockType::EnchantingTable | BlockType::BrewingStand | BlockType::Anvil
                ) {
                    let kind = match clicked_block {
                        BlockType::EnchantingTable => StationKind::Enchanting,
                        BlockType::BrewingStand => StationKind::Brewing,
                        _ => StationKind::Anvil,
                    };
                    self.open_station(kind, hit.block_pos);
                    return;
                }
                if matches!(
                    clicked_block,
                    BlockType::Lever
                        | BlockType::LeverOn
                        | BlockType::StoneButton
                        | BlockType::StoneButtonPressed
                        | BlockType::Repeater
                        | BlockType::RepeaterPowered
                        | BlockType::Comparator
                        | BlockType::ComparatorPowered
                        | BlockType::NoteBlock
                ) {
                    let pos = (
                        hit.block_pos.x as i32,
                        hit.block_pos.y as i32,
                        hit.block_pos.z as i32,
                    );
                    let update = self.redstone.interact(&mut self.chunk_manager, pos);
                    self.apply_redstone_update(update);
                    self.audio_manager
                        .play_sound(crate::audio::SoundId::UiClick);
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
                    if old_block.properties().hardness < 0.0 {
                        return;
                    }
                    self.chunk_manager.set_block(wx, wy, wz, BlockType::Air);
                    self.trigger_advancement(crate::advancements::AdvancementTrigger::MineBlock(
                        old_block,
                    ));
                    self.redstone.on_block_changed(
                        &self.chunk_manager,
                        (wx, wy, wz),
                        crate::redstone::Direction::North,
                    );

                    let sound_pos =
                        glam::Vec3::new(wx as f32 + 0.5, wy as f32 + 0.5, wz as f32 + 0.5);
                    let listener_right =
                        glam::Vec3::new(-self.camera.yaw.sin(), 0.0, self.camera.yaw.cos())
                            .normalize_or_zero();
                    if let Some(mat) = old_block.sound_material() {
                        self.audio_manager.play_sound_3d(
                            crate::audio::SoundId::BlockBreak(mat),
                            sound_pos,
                            self.camera.position,
                            listener_right,
                        );
                    }

                    if self.game_mode == GameMode::Survival {
                        self.inventory
                            .add_item(crate::inventory::Item::from_block(old_block));

                        if old_block == BlockType::Grass {
                            let rng = (wx as u32).wrapping_mul(31).wrapping_add(wz as u32);
                            if rng % 20 == 0 {
                                let drop = match rng % 3 {
                                    0 => crate::inventory::Item::Seeds,
                                    1 => crate::inventory::Item::Wheat,
                                    _ => crate::inventory::Item::Carrot,
                                };
                                self.inventory.add_item(drop);
                            }
                        }
                    }

                    // Update lighting for removal
                    crate::lighting::update_sky_light_after_removed(
                        &mut self.chunk_manager,
                        wx,
                        wy,
                        wz,
                        &mut dirty_chunks,
                    );
                    crate::lighting::update_block_light_after_removed(
                        &mut self.chunk_manager,
                        wx,
                        wy,
                        wz,
                        old_block.properties().light_emission,
                        &mut dirty_chunks,
                    );
                    self.check_and_break_unsupported_above(wx, wy, wz, &mut dirty_chunks);
                }
            } else {
                if let Some(placed_block) = self.inventory.get_selected_block() {
                    let below_block = self.chunk_manager.get_block(wx, wy - 1, wz);
                    if !placed_block.can_stay_on(below_block) {
                        return;
                    }

                    self.chunk_manager.set_block(wx, wy, wz, placed_block);
                    self.redstone.on_block_changed(
                        &self.chunk_manager,
                        (wx, wy, wz),
                        crate::redstone::Direction::from_yaw(self.camera.yaw),
                    );

                    let sound_pos =
                        glam::Vec3::new(wx as f32 + 0.5, wy as f32 + 0.5, wz as f32 + 0.5);
                    let listener_right =
                        glam::Vec3::new(-self.camera.yaw.sin(), 0.0, self.camera.yaw.cos())
                            .normalize_or_zero();
                    if let Some(mat) = placed_block.sound_material() {
                        self.audio_manager.play_sound_3d(
                            crate::audio::SoundId::BlockPlace(mat),
                            sound_pos,
                            self.camera.position,
                            listener_right,
                        );
                    }

                    let is_creative = self.game_mode == GameMode::Creative;
                    self.inventory.use_selected_item(is_creative);

                    // Update lighting for placement
                    crate::lighting::update_sky_light_after_placed(
                        &mut self.chunk_manager,
                        wx,
                        wy,
                        wz,
                        &mut dirty_chunks,
                    );
                    crate::lighting::update_block_light_after_placed(
                        &mut self.chunk_manager,
                        wx,
                        wy,
                        wz,
                        placed_block.properties().light_emission,
                        &mut dirty_chunks,
                    );

                    self.check_and_break_unsupported_above(wx, wy, wz, &mut dirty_chunks);

                    if matches!(
                        placed_block,
                        BlockType::SoulSand | BlockType::WitherSkeletonSkull
                    ) {
                        if let Some(pattern) =
                            crate::boss::detect_wither_pattern((wx, wy, wz), |position| {
                                self.chunk_manager
                                    .get_block(position.0, position.1, position.2)
                            })
                        {
                            let spawn_pos = pattern.iter().fold(Vec3::ZERO, |sum, &(x, y, z)| {
                                sum + Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5)
                            }) / pattern.len() as f32;
                            let removals: Vec<_> = pattern
                                .into_iter()
                                .map(|position| (position, BlockType::Air))
                                .collect();
                            self.apply_block_changes(&removals);
                            self.entity_manager
                                .spawn(crate::entity::EntityType::Wither, spawn_pos);
                            return;
                        }
                    }
                } else {
                    return; // No block selected to place
                }
            }

            mark_block_mesh_dependencies(&mut dirty_chunks, wx, wz);

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
        if self.active_station.is_none() && self.inventory.is_table_open {
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
        } else if self.active_station.is_none() {
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

        match self.active_station {
            Some(StationKind::Enchanting) => {
                slots.push((
                    SlotType::EnchantInput,
                    -0.18,
                    -0.18 + slot_w,
                    0.12,
                    0.12 + slot_h,
                ));
                slots.push((
                    SlotType::EnchantLapis,
                    -0.18,
                    -0.18 + slot_w,
                    -0.02,
                    -0.02 + slot_h,
                ));
            }
            Some(StationKind::Brewing) => {
                for i in 0..3 {
                    let x0 = -0.18 + i as f32 * (slot_w + gap);
                    slots.push((
                        SlotType::BrewBottle(i),
                        x0,
                        x0 + slot_w,
                        -0.02,
                        -0.02 + slot_h,
                    ));
                }
                slots.push((
                    SlotType::BrewIngredient,
                    -0.09,
                    -0.09 + slot_w,
                    0.17,
                    0.17 + slot_h,
                ));
            }
            Some(StationKind::Anvil) => {
                slots.push((
                    SlotType::AnvilLeft,
                    -0.20,
                    -0.20 + slot_w,
                    0.10,
                    0.10 + slot_h,
                ));
                slots.push((
                    SlotType::AnvilRight,
                    -0.05,
                    -0.05 + slot_w,
                    0.10,
                    0.10 + slot_h,
                ));
                slots.push((
                    SlotType::AnvilOutput,
                    0.20,
                    0.20 + slot_w,
                    0.10,
                    0.10 + slot_h,
                ));
            }
            None => {}
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
            SlotType::EnchantInput => self.enchanting.input,
            SlotType::EnchantLapis => self.enchanting.lapis,
            SlotType::BrewBottle(i) => self.brewing.bottles[i],
            SlotType::BrewIngredient => self.brewing.ingredient,
            SlotType::AnvilLeft => self.anvil.left,
            SlotType::AnvilRight => self.anvil.right,
            SlotType::AnvilOutput => self.anvil.output,
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
            SlotType::EnchantInput => self.enchanting.input = stack,
            SlotType::EnchantLapis => self.enchanting.lapis = stack,
            SlotType::BrewBottle(i) => self.brewing.bottles[i] = stack,
            SlotType::BrewIngredient => self.brewing.ingredient = stack,
            SlotType::AnvilLeft => self.anvil.left = stack,
            SlotType::AnvilRight => self.anvil.right = stack,
            SlotType::AnvilOutput => {}
        }
    }

    fn slot_accepts(&self, slot: SlotType, stack: ItemStack) -> bool {
        match slot {
            SlotType::EnchantInput => crate::enchantment::can_enchant(stack.item),
            SlotType::EnchantLapis => stack.item == Item::LapisLazuli,
            SlotType::BrewBottle(_) => stack.potion.is_some(),
            SlotType::AnvilOutput | SlotType::CraftOutput => false,
            _ => true,
        }
    }

    fn refresh_workstations(&mut self) {
        self.enchanting.refresh();
        self.anvil.refresh();
    }

    pub fn handle_inventory_click(&mut self, is_left: bool) {
        let mouse_x = self.mouse_ndc[0];
        let mouse_y = self.mouse_ndc[1];
        let slots = self.get_inventory_slots();

        if self.active_station == Some(StationKind::Enchanting) && is_left {
            for index in 0..3 {
                let y1 = 0.28 - index as f32 * 0.12;
                let y0 = y1 - 0.09;
                if mouse_x >= 0.02 && mouse_x <= 0.62 && mouse_y >= y0 && mouse_y <= y1 {
                    self.perform_enchantment(index);
                    return;
                }
            }
        }

        let clicked_slot = slots.into_iter().find(|&(_, x0, x1, y0, y1)| {
            mouse_x >= x0 && mouse_x <= x1 && mouse_y >= y0 && mouse_y <= y1
        });

        if let Some((slot_type, _, _, _, _)) = clicked_slot {
            self.audio_manager
                .play_sound(crate::audio::SoundId::UiClick);
            let slot_item = self.get_item_at_slot(slot_type);

            if let Some(dragged) = self.inventory.dragged {
                if !self.slot_accepts(slot_type, dragged) {
                    return;
                }
            }

            match slot_type {
                SlotType::CraftOutput => {
                    if let Some(output) = slot_item {
                        self.trigger_advancement(
                            crate::advancements::AdvancementTrigger::CraftItem(output.item),
                        );
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
                            self.inventory.craft_output = self
                                .recipe_manager
                                .match_recipe(&self.inventory.craft_input, grid_size);
                        } else if let Some(ref mut dragged) = self.inventory.dragged {
                            if dragged.item == output.item
                                && dragged.count + output.count <= max_stack
                            {
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
                                self.inventory.craft_output = self
                                    .recipe_manager
                                    .match_recipe(&self.inventory.craft_input, grid_size);
                            }
                        }
                    }
                }
                SlotType::AnvilOutput => {
                    if let Some(output) = self.anvil.output {
                        let affordable = self.game_mode == GameMode::Creative
                            || self.player_state.experience_level >= self.anvil.cost as u32;
                        if affordable && self.inventory.dragged.is_none() {
                            if self.game_mode == GameMode::Survival {
                                self.player_state.spend_levels(self.anvil.cost as u32);
                            }
                            self.inventory.dragged = Some(output);
                            self.anvil.left = None;
                            self.anvil.right = None;
                            self.anvil.rename.clear();
                            self.anvil.refresh();
                        }
                    }
                }
                _ => {
                    // Normal slots (Backpack, Hotbar, Armor, CraftInput)
                    let max_stack = slot_item
                        .map(|s| s.item.properties().max_stack)
                        .unwrap_or(64);

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

                                    self.set_item_at_slot(
                                        slot_type,
                                        Some(ItemStack {
                                            count: new_slot_count,
                                            ..slot
                                        }),
                                    );
                                    if new_drag_count > 0 {
                                        self.inventory.dragged = Some(ItemStack {
                                            count: new_drag_count,
                                            ..dragged
                                        });
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
                                    self.set_item_at_slot(
                                        slot_type,
                                        Some(ItemStack {
                                            count: slot.count + 1,
                                            ..slot
                                        }),
                                    );
                                    if dragged.count > 1 {
                                        self.inventory.dragged = Some(ItemStack {
                                            count: dragged.count - 1,
                                            ..dragged
                                        });
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
                                self.set_item_at_slot(
                                    slot_type,
                                    Some(ItemStack {
                                        count: 1,
                                        ..dragged
                                    }),
                                );
                                if dragged.count > 1 {
                                    self.inventory.dragged = Some(ItemStack {
                                        count: dragged.count - 1,
                                        ..dragged
                                    });
                                } else {
                                    self.inventory.dragged = None;
                                }
                            }
                        } else {
                            // Split stack in slot
                            if let Some(slot) = slot_item {
                                let take = (slot.count + 1) / 2;
                                let keep = slot.count - take;
                                self.inventory.dragged = Some(ItemStack {
                                    count: take,
                                    ..slot
                                });
                                if keep > 0 {
                                    self.set_item_at_slot(
                                        slot_type,
                                        Some(ItemStack {
                                            count: keep,
                                            ..slot
                                        }),
                                    );
                                } else {
                                    self.set_item_at_slot(slot_type, None);
                                }
                            }
                        }
                    }

                    // If we clicked a craft input slot, recalculate craft output
                    if let SlotType::CraftInput(_) = slot_type {
                        let grid_size = if self.inventory.is_table_open { 3 } else { 2 };
                        self.inventory.craft_output = self
                            .recipe_manager
                            .match_recipe(&self.inventory.craft_input, grid_size);
                    }
                    self.refresh_workstations();
                }
            }
        }
    }

    fn perform_enchantment(&mut self, index: usize) {
        let Some(mut input) = self.enchanting.input else {
            return;
        };
        if !crate::enchantment::can_enchant(input.item) {
            return;
        }
        let option = self.enchanting.options[index];
        let lapis_available = self
            .enchanting
            .lapis
            .filter(|stack| stack.item == Item::LapisLazuli)
            .map(|stack| stack.count)
            .unwrap_or(0);
        let affordable = self.game_mode == GameMode::Creative
            || (lapis_available >= option.lapis_cost as u32
                && self.player_state.experience_level >= option.cost as u32);
        if !affordable {
            return;
        }
        input.enchantments.merge(&option.enchantments);
        self.enchanting.input = Some(input);
        self.trigger_advancement(crate::advancements::AdvancementTrigger::EnchantItem);
        if self.game_mode == GameMode::Survival {
            self.player_state.spend_levels(option.cost as u32);
            if let Some(lapis) = &mut self.enchanting.lapis {
                if lapis.count > option.lapis_cost as u32 {
                    lapis.count -= option.lapis_cost as u32;
                } else {
                    self.enchanting.lapis = None;
                }
            }
        }
        self.enchanting.seed = self.enchanting.seed.wrapping_add(0x9E37_79B9);
        self.enchanting.refresh();
    }

    pub fn open_inventory(&mut self) {
        self.inventory.is_open = true;
        // Release cursor grab
        let _ = self
            .window
            .set_cursor_grab(winit::window::CursorGrabMode::None);
        self.window.set_cursor_visible(true);
        self.keys = KeyState::default();
    }

    fn open_station(&mut self, kind: StationKind, position: Vec3) {
        self.active_station = Some(kind);
        if kind == StationKind::Enchanting {
            let wx = position.x as i32;
            let wy = position.y as i32;
            let wz = position.z as i32;
            let mut shelves = 0;
            for dx in -2i32..=2i32 {
                for dz in -2i32..=2i32 {
                    if dx.abs() != 2 && dz.abs() != 2 {
                        continue;
                    }
                    for dy in 0..=1 {
                        if self.chunk_manager.get_block(wx + dx, wy + dy, wz + dz)
                            == BlockType::Bookshelf
                        {
                            shelves += 1;
                        }
                    }
                }
            }
            self.enchanting.bookshelves = shelves.min(15);
            self.enchanting.seed =
                self.world_time.ticks as u32 ^ wx as u32 ^ (wz as u32).rotate_left(16);
            self.enchanting.refresh();
        }
        self.open_inventory();
    }

    pub fn close_inventory(&mut self) {
        self.inventory.is_open = false;
        // Return craft input items
        let inputs: Vec<ItemStack> = self
            .inventory
            .craft_input
            .iter_mut()
            .filter_map(|slot| slot.take())
            .collect();
        for stack in inputs {
            for _ in 0..stack.count {
                self.inventory.add_item(stack.item);
            }
        }
        let station_items: Vec<ItemStack> = match self.active_station {
            Some(StationKind::Enchanting) => {
                [self.enchanting.input.take(), self.enchanting.lapis.take()]
                    .into_iter()
                    .flatten()
                    .collect()
            }
            Some(StationKind::Brewing) => self
                .brewing
                .bottles
                .iter_mut()
                .map(Option::take)
                .chain(std::iter::once(self.brewing.ingredient.take()))
                .flatten()
                .collect(),
            Some(StationKind::Anvil) => [self.anvil.left.take(), self.anvil.right.take()]
                .into_iter()
                .flatten()
                .collect(),
            None => Vec::new(),
        };
        for stack in station_items {
            self.inventory.add_stack(stack);
        }

        // Also return dragged item if any
        if let Some(dragged) = self.inventory.dragged.take() {
            self.inventory.add_stack(dragged);
        }

        self.inventory.is_table_open = false;
        self.inventory.craft_input = vec![None; 4];
        self.inventory.craft_output = None;
        self.active_station = None;
        self.anvil.rename.clear();

        // Re-lock cursor
        let _ = self
            .window
            .set_cursor_grab(winit::window::CursorGrabMode::Locked)
            .or_else(|_| {
                self.window
                    .set_cursor_grab(winit::window::CursorGrabMode::Confined)
            });
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

    fn estimated_debug_memory_bytes(&self) -> usize {
        let chunk_volume = CHUNK_WIDTH * CHUNK_HEIGHT * CHUNK_DEPTH;
        let chunk_heap_bytes = chunk_volume
            * (std::mem::size_of::<BlockType>() + 3 * std::mem::size_of::<u8>())
            + CHUNK_WIDTH * CHUNK_DEPTH * std::mem::size_of::<u16>();
        let chunks_bytes = self
            .chunk_manager
            .chunks
            .len()
            .saturating_mul(std::mem::size_of::<Chunk>() + chunk_heap_bytes);

        let mesh_indices: usize = self
            .chunk_meshes
            .values()
            .map(|mesh| mesh.opaque_num_indices as usize + mesh.transparent_num_indices as usize)
            .sum();
        let mesh_vertices = mesh_indices.saturating_mul(2) / 3;
        let mesh_bytes = mesh_vertices
            .saturating_mul(std::mem::size_of::<Vertex>())
            .saturating_add(mesh_indices.saturating_mul(std::mem::size_of::<u32>()));

        let entities_bytes = self
            .entity_manager
            .entities
            .capacity()
            .saturating_mul(std::mem::size_of::<crate::entity::Entity>());
        let particles_bytes = self
            .particles
            .particles
            .capacity()
            .saturating_mul(std::mem::size_of::<crate::particles::Particle>());

        chunks_bytes
            .saturating_add(mesh_bytes)
            .saturating_add(entities_bytes)
            .saturating_add(particles_bytes)
    }

    pub fn render(&mut self) -> Result<(), wgpu::SurfaceError> {
        let output = self.surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        // Compile mob meshes
        let mut mob_vertices = Vec::new();
        let mut mob_indices = Vec::new();
        crate::mob_renderer::render_mobs(
            &self.entity_manager,
            &self.chunk_manager,
            &mut mob_vertices,
            &mut mob_indices,
            self.total_time,
        );
        let mob_indices_len = mob_indices.len();
        self.mob_num_indices = mob_indices_len as u32;

        if mob_indices_len > 0 {
            let vert_limit = mob_vertices.len().min(8192);
            let ind_limit = mob_indices_len.min(12288);
            self.mob_num_indices = ind_limit as u32;
            self.queue.write_buffer(
                &self.mob_vertex_buffer,
                0,
                bytemuck::cast_slice(&mob_vertices[..vert_limit]),
            );
            self.queue.write_buffer(
                &self.mob_index_buffer,
                0,
                bytemuck::cast_slice(&mob_indices[..ind_limit]),
            );
        }

        // Compile billboard particle quads into the dynamic particle buffers.
        // Camera right/up vectors are derived from yaw/pitch so billboards face
        // the viewer.
        let cam_right =
            glam::Vec3::new(-self.camera.yaw.sin(), 0.0, self.camera.yaw.cos()).normalize_or_zero();
        let cam_up = glam::Vec3::new(
            -self.camera.yaw.cos() * self.camera.pitch.sin(),
            self.camera.pitch.cos(),
            -self.camera.yaw.sin() * self.camera.pitch.sin(),
        )
        .normalize_or_zero();
        self.particle_num_indices = self
            .particles
            .compile_mesh(
                &self.device,
                &self.queue,
                cam_right,
                cam_up,
                &self.particle_vertex_buffer,
                &self.particle_index_buffer,
            )
            .unwrap_or(0);

        if self.is_saving {
            let mut ui_vertices = Vec::new();
            let mut ui_line_vertices = Vec::new();

            let bg_color = [0.1, 0.1, 0.1, 0.75];
            ui_vertices.push(UiVertex {
                position: [-1.0, 1.0, 0.0],
                color: bg_color,
            });
            ui_vertices.push(UiVertex {
                position: [-1.0, -1.0, 0.0],
                color: bg_color,
            });
            ui_vertices.push(UiVertex {
                position: [1.0, -1.0, 0.0],
                color: bg_color,
            });
            ui_vertices.push(UiVertex {
                position: [-1.0, 1.0, 0.0],
                color: bg_color,
            });
            ui_vertices.push(UiVertex {
                position: [1.0, -1.0, 0.0],
                color: bg_color,
            });
            ui_vertices.push(UiVertex {
                position: [1.0, 1.0, 0.0],
                color: bg_color,
            });

            let draw_centered_text =
                |s: &str,
                 y: f32,
                 char_w: f32,
                 char_h: f32,
                 spacing: f32,
                 color: [f32; 4],
                 vertices: &mut Vec<UiVertex>| {
                    let upper = s.to_uppercase();
                    let n = upper.len() as f32;
                    let width = n * char_w + (n - 1.0) * spacing;
                    let start_x = -width / 2.0;
                    add_string_lines(&upper, start_x, y, char_w, char_h, spacing, color, vertices);
                };

            draw_centered_text(
                "SAVING WORLD...",
                0.0,
                0.03,
                0.06,
                0.012,
                [1.0, 1.0, 1.0, 1.0],
                &mut ui_line_vertices,
            );

            let ui_vert_len = ui_vertices.len().min(4096);
            let ui_line_vert_len = ui_line_vertices.len().min(4096);

            self.queue.write_buffer(
                &self.ui_vertex_buffer,
                0,
                bytemuck::cast_slice(&ui_vertices[..ui_vert_len]),
            );
            self.queue.write_buffer(
                &self.ui_line_vertex_buffer,
                0,
                bytemuck::cast_slice(&ui_line_vertices[..ui_line_vert_len]),
            );

            self.num_ui_vertices = ui_vert_len as u32;
            self.num_ui_line_vertices = ui_line_vert_len as u32;
            self.num_ui_textured_vertices = 0;
        } else if self.player_state.is_dead {
            let mut ui_vertices = Vec::new();
            let mut ui_line_vertices = Vec::new();

            let mouse_x = self.mouse_ndc[0];
            let mouse_y = self.mouse_ndc[1];

            // Respawn button hover (X: [-0.3, 0.3], Y: [-0.1, 0.0])
            let respawn_hover =
                mouse_x >= -0.3 && mouse_x <= 0.3 && mouse_y >= -0.1 && mouse_y <= 0.0;

            // Reddish overlay
            let bg_color = [0.4, 0.0, 0.0, 0.6];
            ui_vertices.push(UiVertex {
                position: [-1.0, 1.0, 0.0],
                color: bg_color,
            });
            ui_vertices.push(UiVertex {
                position: [-1.0, -1.0, 0.0],
                color: bg_color,
            });
            ui_vertices.push(UiVertex {
                position: [1.0, -1.0, 0.0],
                color: bg_color,
            });
            ui_vertices.push(UiVertex {
                position: [-1.0, 1.0, 0.0],
                color: bg_color,
            });
            ui_vertices.push(UiVertex {
                position: [1.0, -1.0, 0.0],
                color: bg_color,
            });
            ui_vertices.push(UiVertex {
                position: [1.0, 1.0, 0.0],
                color: bg_color,
            });

            // Button background
            let btn_bg = if respawn_hover {
                [0.4, 0.1, 0.1, 1.0]
            } else {
                [0.2, 0.0, 0.0, 1.0]
            };
            let btn_border = if respawn_hover {
                [1.0, 1.0, 1.0, 1.0]
            } else {
                [0.6, 0.2, 0.2, 1.0]
            };
            let btn_y_min = -0.10;
            let btn_y_max = 0.00;

            ui_vertices.push(UiVertex {
                position: [-0.3, btn_y_max, 0.0],
                color: btn_bg,
            });
            ui_vertices.push(UiVertex {
                position: [-0.3, btn_y_min, 0.0],
                color: btn_bg,
            });
            ui_vertices.push(UiVertex {
                position: [0.3, btn_y_min, 0.0],
                color: btn_bg,
            });
            ui_vertices.push(UiVertex {
                position: [-0.3, btn_y_max, 0.0],
                color: btn_bg,
            });
            ui_vertices.push(UiVertex {
                position: [0.3, btn_y_min, 0.0],
                color: btn_bg,
            });
            ui_vertices.push(UiVertex {
                position: [0.3, btn_y_max, 0.0],
                color: btn_bg,
            });

            // Button border
            ui_line_vertices.push(UiVertex {
                position: [-0.3, btn_y_max, 0.0],
                color: btn_border,
            });
            ui_line_vertices.push(UiVertex {
                position: [0.3, btn_y_max, 0.0],
                color: btn_border,
            });
            ui_line_vertices.push(UiVertex {
                position: [0.3, btn_y_max, 0.0],
                color: btn_border,
            });
            ui_line_vertices.push(UiVertex {
                position: [0.3, btn_y_min, 0.0],
                color: btn_border,
            });
            ui_line_vertices.push(UiVertex {
                position: [0.3, btn_y_min, 0.0],
                color: btn_border,
            });
            ui_line_vertices.push(UiVertex {
                position: [-0.3, btn_y_min, 0.0],
                color: btn_border,
            });
            ui_line_vertices.push(UiVertex {
                position: [-0.3, btn_y_min, 0.0],
                color: btn_border,
            });
            ui_line_vertices.push(UiVertex {
                position: [-0.3, btn_y_max, 0.0],
                color: btn_border,
            });

            let draw_centered_text =
                |s: &str,
                 y: f32,
                 char_w: f32,
                 char_h: f32,
                 spacing: f32,
                 color: [f32; 4],
                 vertices: &mut Vec<UiVertex>| {
                    let upper = s.to_uppercase();
                    let n = upper.len() as f32;
                    let width = n * char_w + (n - 1.0) * spacing;
                    let start_x = -width / 2.0;
                    add_string_lines(&upper, start_x, y, char_w, char_h, spacing, color, vertices);
                };

            draw_centered_text(
                "YOU DIED!",
                0.30,
                0.04,
                0.08,
                0.015,
                [1.0, 0.2, 0.2, 1.0],
                &mut ui_line_vertices,
            );

            let msg = match self.player_state.death_reason {
                Some(DamageSource::Fall) => "FELL FROM A HIGH PLACE",
                Some(DamageSource::Void) => "FELL INTO THE VOID",
                Some(DamageSource::Hunger) => "STARVED TO DEATH",
                Some(DamageSource::Mob) => "WAS SLAIN BY ZOMBIE/SKELETON",
                Some(DamageSource::Explosion) => "WAS BLOWN UP BY CREEPER",
                Some(DamageSource::Drowning) => "DROWNED",
                Some(DamageSource::Lightning) => "WAS STRUCK BY LIGHTNING",
                None => "DIED",
            };
            draw_centered_text(
                msg,
                0.15,
                0.015,
                0.03,
                0.006,
                [1.0, 1.0, 1.0, 1.0],
                &mut ui_line_vertices,
            );
            draw_centered_text(
                "RESPAWN",
                -0.06,
                0.02,
                0.04,
                0.008,
                [1.0, 1.0, 1.0, 1.0],
                &mut ui_line_vertices,
            );

            let ui_vert_len = ui_vertices.len().min(4096);
            let ui_line_vert_len = ui_line_vertices.len().min(4096);

            self.queue.write_buffer(
                &self.ui_vertex_buffer,
                0,
                bytemuck::cast_slice(&ui_vertices[..ui_vert_len]),
            );
            self.queue.write_buffer(
                &self.ui_line_vertex_buffer,
                0,
                bytemuck::cast_slice(&ui_line_vertices[..ui_line_vert_len]),
            );

            self.num_ui_vertices = ui_vert_len as u32;
            self.num_ui_line_vertices = ui_line_vert_len as u32;
            self.num_ui_textured_vertices = 0;
        } else if self.is_paused {
            let mut ui_vertices = Vec::new();
            let mut ui_line_vertices = Vec::new();

            let mouse_x = self.mouse_ndc[0];
            let mouse_y = self.mouse_ndc[1];

            // Hover states
            let resume_hover =
                mouse_x >= -0.3 && mouse_x <= 0.3 && mouse_y >= 0.24 && mouse_y <= 0.34;
            let fov_hover = mouse_x >= -0.3 && mouse_x <= 0.3 && mouse_y >= 0.10 && mouse_y <= 0.20;
            let sens_hover =
                mouse_x >= -0.3 && mouse_x <= 0.3 && mouse_y >= -0.04 && mouse_y <= 0.06;
            let rd_hover =
                mouse_x >= -0.3 && mouse_x <= 0.3 && mouse_y >= -0.18 && mouse_y <= -0.08;
            let vol_hover =
                mouse_x >= -0.3 && mouse_x <= 0.3 && mouse_y >= -0.32 && mouse_y <= -0.22;
            let quit_hover =
                mouse_x >= -0.3 && mouse_x <= 0.3 && mouse_y >= -0.46 && mouse_y <= -0.36;

            // 1. Dark overlay (screen covers from -1.0 to 1.0)
            let bg_color = [0.1, 0.1, 0.1, 0.7];
            ui_vertices.push(UiVertex {
                position: [-1.0, 1.0, 0.0],
                color: bg_color,
            });
            ui_vertices.push(UiVertex {
                position: [-1.0, -1.0, 0.0],
                color: bg_color,
            });
            ui_vertices.push(UiVertex {
                position: [1.0, -1.0, 0.0],
                color: bg_color,
            });
            ui_vertices.push(UiVertex {
                position: [-1.0, 1.0, 0.0],
                color: bg_color,
            });
            ui_vertices.push(UiVertex {
                position: [1.0, -1.0, 0.0],
                color: bg_color,
            });
            ui_vertices.push(UiVertex {
                position: [1.0, 1.0, 0.0],
                color: bg_color,
            });

            // Button drawing helper
            let draw_button = |hover: bool,
                               y_min: f32,
                               y_max: f32,
                               ui_verts: &mut Vec<UiVertex>,
                               ui_line_verts: &mut Vec<UiVertex>| {
                let bg = if hover {
                    [0.4, 0.4, 0.4, 1.0]
                } else {
                    [0.2, 0.2, 0.2, 1.0]
                };
                let border = if hover {
                    [1.0, 1.0, 1.0, 1.0]
                } else {
                    [0.6, 0.6, 0.6, 1.0]
                };

                // Background (two triangles)
                ui_verts.push(UiVertex {
                    position: [-0.3, y_max, 0.0],
                    color: bg,
                });
                ui_verts.push(UiVertex {
                    position: [-0.3, y_min, 0.0],
                    color: bg,
                });
                ui_verts.push(UiVertex {
                    position: [0.3, y_min, 0.0],
                    color: bg,
                });
                ui_verts.push(UiVertex {
                    position: [-0.3, y_max, 0.0],
                    color: bg,
                });
                ui_verts.push(UiVertex {
                    position: [0.3, y_min, 0.0],
                    color: bg,
                });
                ui_verts.push(UiVertex {
                    position: [0.3, y_max, 0.0],
                    color: bg,
                });

                // Border (line loop)
                ui_line_verts.push(UiVertex {
                    position: [-0.3, y_max, 0.0],
                    color: border,
                });
                ui_line_verts.push(UiVertex {
                    position: [0.3, y_max, 0.0],
                    color: border,
                });
                ui_line_verts.push(UiVertex {
                    position: [0.3, y_max, 0.0],
                    color: border,
                });
                ui_line_verts.push(UiVertex {
                    position: [0.3, y_min, 0.0],
                    color: border,
                });
                ui_line_verts.push(UiVertex {
                    position: [0.3, y_min, 0.0],
                    color: border,
                });
                ui_line_verts.push(UiVertex {
                    position: [-0.3, y_min, 0.0],
                    color: border,
                });
                ui_line_verts.push(UiVertex {
                    position: [-0.3, y_min, 0.0],
                    color: border,
                });
                ui_line_verts.push(UiVertex {
                    position: [-0.3, y_max, 0.0],
                    color: border,
                });
            };

            // Draw Button backgrounds and borders
            draw_button(
                resume_hover,
                0.24,
                0.34,
                &mut ui_vertices,
                &mut ui_line_vertices,
            );
            draw_button(
                fov_hover,
                0.10,
                0.20,
                &mut ui_vertices,
                &mut ui_line_vertices,
            );
            draw_button(
                sens_hover,
                -0.04,
                0.06,
                &mut ui_vertices,
                &mut ui_line_vertices,
            );
            draw_button(
                rd_hover,
                -0.18,
                -0.08,
                &mut ui_vertices,
                &mut ui_line_vertices,
            );
            draw_button(
                vol_hover,
                -0.32,
                -0.22,
                &mut ui_vertices,
                &mut ui_line_vertices,
            );
            draw_button(
                quit_hover,
                -0.46,
                -0.36,
                &mut ui_vertices,
                &mut ui_line_vertices,
            );

            // Centered text drawing helper
            let draw_centered_text =
                |s: &str,
                 y: f32,
                 char_w: f32,
                 char_h: f32,
                 spacing: f32,
                 color: [f32; 4],
                 vertices: &mut Vec<UiVertex>| {
                    let upper = s.to_uppercase();
                    let n = upper.len() as f32;
                    let width = n * char_w + (n - 1.0) * spacing;
                    let start_x = -width / 2.0;
                    add_string_lines(&upper, start_x, y, char_w, char_h, spacing, color, vertices);
                };

            // Render Text Labels
            let text_color = [1.0, 1.0, 1.0, 1.0];
            // "GAME PAUSED"
            draw_centered_text(
                "GAME PAUSED",
                0.40,
                0.03,
                0.06,
                0.012,
                text_color,
                &mut ui_line_vertices,
            );
            // "RESUME"
            draw_centered_text(
                "RESUME",
                0.28,
                0.02,
                0.04,
                0.008,
                text_color,
                &mut ui_line_vertices,
            );

            // "FOV < value >"
            let fov_text = format!("FOV < {:.0} >", self.camera.fov);
            draw_centered_text(
                &fov_text,
                0.14,
                0.02,
                0.04,
                0.008,
                text_color,
                &mut ui_line_vertices,
            );

            // "SENS < value >"
            let sens_val = (self.sensitivity / 0.002 * 100.0).round();
            let sens_text = format!("SENS < {:.0} >", sens_val);
            draw_centered_text(
                &sens_text,
                0.00,
                0.02,
                0.04,
                0.008,
                text_color,
                &mut ui_line_vertices,
            );

            // "RENDER DISTANCE < value >"
            let rd_text = format!("RENDER DISTANCE < {} >", self.chunk_manager.render_distance);
            draw_centered_text(
                &rd_text,
                -0.14,
                0.02,
                0.04,
                0.008,
                text_color,
                &mut ui_line_vertices,
            );

            // "VOLUME < value >"
            let vol_text = format!("VOLUME < {:.0}% >", self.audio_manager.volume * 100.0);
            draw_centered_text(
                &vol_text,
                -0.28,
                0.02,
                0.04,
                0.008,
                text_color,
                &mut ui_line_vertices,
            );

            // "SAVE AND QUIT"
            draw_centered_text(
                "SAVE AND QUIT",
                -0.42,
                0.02,
                0.04,
                0.008,
                text_color,
                &mut ui_line_vertices,
            );

            // Cap the sizes to the preallocated buffers (4096 vertices)
            let ui_vert_len = ui_vertices.len().min(4096);
            let ui_line_vert_len = ui_line_vertices.len().min(4096);

            self.queue.write_buffer(
                &self.ui_vertex_buffer,
                0,
                bytemuck::cast_slice(&ui_vertices[..ui_vert_len]),
            );
            self.queue.write_buffer(
                &self.ui_line_vertex_buffer,
                0,
                bytemuck::cast_slice(&ui_line_vertices[..ui_line_vert_len]),
            );

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

            let draw_durability_bar =
                |stack: &ItemStack,
                 x0: f32,
                 x1: f32,
                 y0: f32,
                 y1: f32,
                 _aspect: f32,
                 ui_vertices: &mut Vec<UiVertex>| {
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
                            ui_vertices.push(UiVertex {
                                position: [bar_x0, bar_y1, 0.0],
                                color: bg_color,
                            });
                            ui_vertices.push(UiVertex {
                                position: [bar_x0, bar_y0, 0.0],
                                color: bg_color,
                            });
                            ui_vertices.push(UiVertex {
                                position: [bar_x1, bar_y0, 0.0],
                                color: bg_color,
                            });
                            ui_vertices.push(UiVertex {
                                position: [bar_x0, bar_y1, 0.0],
                                color: bg_color,
                            });
                            ui_vertices.push(UiVertex {
                                position: [bar_x1, bar_y0, 0.0],
                                color: bg_color,
                            });
                            ui_vertices.push(UiVertex {
                                position: [bar_x1, bar_y1, 0.0],
                                color: bg_color,
                            });

                            // 2. Colored foreground bar
                            let fg_x1 = bar_x0 + (bar_x1 - bar_x0) * ratio;
                            let (r, g) = if ratio > 0.5 {
                                ((1.0 - ratio) * 2.0, 1.0)
                            } else {
                                (1.0, ratio * 2.0)
                            };
                            let fg_color = [r, g, 0.0, 1.0];

                            ui_vertices.push(UiVertex {
                                position: [bar_x0, bar_y1, 0.0],
                                color: fg_color,
                            });
                            ui_vertices.push(UiVertex {
                                position: [bar_x0, bar_y0, 0.0],
                                color: fg_color,
                            });
                            ui_vertices.push(UiVertex {
                                position: [fg_x1, bar_y0, 0.0],
                                color: fg_color,
                            });
                            ui_vertices.push(UiVertex {
                                position: [bar_x0, bar_y1, 0.0],
                                color: fg_color,
                            });
                            ui_vertices.push(UiVertex {
                                position: [fg_x1, bar_y0, 0.0],
                                color: fg_color,
                            });
                            ui_vertices.push(UiVertex {
                                position: [fg_x1, bar_y1, 0.0],
                                color: fg_color,
                            });
                        }
                    }
                };

            if self.inventory.is_open {
                // 1. Dark overlay (screen covers from -1.0 to 1.0)
                let bg_color = [0.08, 0.08, 0.08, 0.6];
                ui_vertices.push(UiVertex {
                    position: [-1.0, 1.0, 0.0],
                    color: bg_color,
                });
                ui_vertices.push(UiVertex {
                    position: [-1.0, -1.0, 0.0],
                    color: bg_color,
                });
                ui_vertices.push(UiVertex {
                    position: [1.0, -1.0, 0.0],
                    color: bg_color,
                });
                ui_vertices.push(UiVertex {
                    position: [-1.0, 1.0, 0.0],
                    color: bg_color,
                });
                ui_vertices.push(UiVertex {
                    position: [1.0, -1.0, 0.0],
                    color: bg_color,
                });
                ui_vertices.push(UiVertex {
                    position: [1.0, 1.0, 0.0],
                    color: bg_color,
                });

                // 2. Draw slots
                let slots = self.get_inventory_slots();
                let mouse_x = self.mouse_ndc[0];
                let mouse_y = self.mouse_ndc[1];
                let mut hovered_slot = None;

                for &(slot_type, x0, x1, y0, y1) in &slots {
                    let is_hovered =
                        mouse_x >= x0 && mouse_x <= x1 && mouse_y >= y0 && mouse_y <= y1;
                    if is_hovered {
                        hovered_slot = Some((slot_type, x0, x1, y0, y1));
                    }

                    // Background Quad
                    let slot_bg_color = if is_hovered {
                        [0.35, 0.35, 0.35, 0.8]
                    } else {
                        [0.15, 0.15, 0.15, 0.8]
                    };
                    ui_vertices.push(UiVertex {
                        position: [x0, y1, 0.0],
                        color: slot_bg_color,
                    });
                    ui_vertices.push(UiVertex {
                        position: [x0, y0, 0.0],
                        color: slot_bg_color,
                    });
                    ui_vertices.push(UiVertex {
                        position: [x1, y0, 0.0],
                        color: slot_bg_color,
                    });
                    ui_vertices.push(UiVertex {
                        position: [x0, y1, 0.0],
                        color: slot_bg_color,
                    });
                    ui_vertices.push(UiVertex {
                        position: [x1, y0, 0.0],
                        color: slot_bg_color,
                    });
                    ui_vertices.push(UiVertex {
                        position: [x1, y1, 0.0],
                        color: slot_bg_color,
                    });

                    // Borders
                    let border_color = match slot_type {
                        SlotType::Hotbar(idx) if idx == self.inventory.selected => {
                            [1.0, 1.0, 1.0, 1.0]
                        }
                        _ => [0.3, 0.3, 0.3, 0.8],
                    };
                    ui_line_vertices.push(UiVertex {
                        position: [x0, y1, 0.0],
                        color: border_color,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [x1, y1, 0.0],
                        color: border_color,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [x1, y1, 0.0],
                        color: border_color,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [x1, y0, 0.0],
                        color: border_color,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [x1, y0, 0.0],
                        color: border_color,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [x0, y0, 0.0],
                        color: border_color,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [x0, y0, 0.0],
                        color: border_color,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [x0, y1, 0.0],
                        color: border_color,
                    });

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

                        let c = if stack.enchantments.is_empty() {
                            [1.0, 1.0, 1.0, 1.0]
                        } else {
                            let pulse = 0.72 + (self.total_time * 3.0).sin() * 0.18;
                            [0.82, pulse, 1.0, 1.0]
                        };
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [tx0, ty1, 0.0],
                            tex_coords: [u0, v0],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [tx0, ty0, 0.0],
                            tex_coords: [u0, v1],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [tx1, ty0, 0.0],
                            tex_coords: [u1, v1],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [tx0, ty1, 0.0],
                            tex_coords: [u0, v0],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [tx1, ty0, 0.0],
                            tex_coords: [u1, v1],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [tx1, ty1, 0.0],
                            tex_coords: [u1, v0],
                            color: c,
                        });

                        if stack.count > 1 {
                            let count_str = format!("{}", stack.count);
                            let cw = 0.008;
                            let ch = 0.016;
                            let cs = 0.003;
                            let n_chars = count_str.len() as f32;
                            let count_w = n_chars * cw + (n_chars - 1.0) * cs;
                            let count_x = x1 - count_w - 0.008;
                            let count_y = y0 + 0.01 * aspect;
                            add_string_lines(
                                &count_str,
                                count_x,
                                count_y,
                                cw,
                                ch,
                                cs,
                                [1.0, 1.0, 1.0, 1.0],
                                &mut ui_line_vertices,
                            );
                        }

                        // Draw durability bar
                        draw_durability_bar(&stack, x0, x1, y0, y1, aspect, &mut ui_vertices);
                    }
                }

                // 3. Draw crafting arrow symbol
                if self.active_station.is_none() {
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
                    ui_line_vertices.push(UiVertex {
                        position: [arrow_x, arrow_y, 0.0],
                        color: ac,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [arrow_x + 0.03, arrow_y, 0.0],
                        color: ac,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [arrow_x + 0.03, arrow_y, 0.0],
                        color: ac,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [arrow_x + 0.02, arrow_y + 0.01 * aspect, 0.0],
                        color: ac,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [arrow_x + 0.03, arrow_y, 0.0],
                        color: ac,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [arrow_x + 0.02, arrow_y - 0.01 * aspect, 0.0],
                        color: ac,
                    });
                }

                // 4. Draw texts (Labels)
                add_string_lines(
                    "INVENTORY",
                    -0.40,
                    -0.70 + 3.0 * (slot_h + gap) + 0.02,
                    0.008,
                    0.016,
                    0.003,
                    [1.0, 1.0, 1.0, 1.0],
                    &mut ui_line_vertices,
                );
                if self.active_station.is_none() {
                    let craft_lbl_x = if self.inventory.is_table_open {
                        -0.05
                    } else {
                        0.05
                    };
                    let craft_lbl_y = if self.inventory.is_table_open {
                        -0.10 + 3.0 * (slot_h + gap) + 0.02
                    } else {
                        -0.05 + 2.0 * (slot_h + gap) + 0.02
                    };
                    add_string_lines(
                        "CRAFTING",
                        craft_lbl_x,
                        craft_lbl_y,
                        0.008,
                        0.016,
                        0.003,
                        [1.0, 1.0, 1.0, 1.0],
                        &mut ui_line_vertices,
                    );
                }

                match self.active_station {
                    Some(StationKind::Enchanting) => {
                        add_string_lines(
                            "ENCHANTING",
                            -0.18,
                            0.37,
                            0.012,
                            0.024,
                            0.004,
                            [0.75, 0.45, 1.0, 1.0],
                            &mut ui_line_vertices,
                        );
                        let level_text = format!(
                            "LEVEL {}  BOOKSHELVES {}",
                            self.player_state.experience_level, self.enchanting.bookshelves
                        );
                        add_string_lines(
                            &level_text,
                            -0.18,
                            0.31,
                            0.008,
                            0.016,
                            0.003,
                            [0.5, 1.0, 0.5, 1.0],
                            &mut ui_line_vertices,
                        );
                        for (index, option) in self.enchanting.options.iter().enumerate() {
                            let y1 = 0.28 - index as f32 * 0.12;
                            let y0 = y1 - 0.09;
                            let hovered = mouse_x >= 0.02
                                && mouse_x <= 0.62
                                && mouse_y >= y0
                                && mouse_y <= y1;
                            add_ui_quad(
                                &mut ui_vertices,
                                0.02,
                                0.62,
                                y0,
                                y1,
                                if hovered {
                                    [0.30, 0.16, 0.42, 0.95]
                                } else {
                                    [0.14, 0.07, 0.20, 0.95]
                                },
                            );
                            let enchantment =
                                option.enchantments.entries.iter().flatten().next().copied();
                            let label = enchantment
                                .map(|e| {
                                    format!(
                                        "{} {}  COST {} + {} LAPIS",
                                        e.short_name(),
                                        e.level(),
                                        option.cost,
                                        option.lapis_cost
                                    )
                                })
                                .unwrap_or_else(|| "NO ENCHANTMENT".to_string());
                            add_string_lines(
                                &label,
                                0.04,
                                y0 + 0.032,
                                0.007,
                                0.014,
                                0.002,
                                [0.8, 0.65, 1.0, 1.0],
                                &mut ui_line_vertices,
                            );
                        }
                    }
                    Some(StationKind::Brewing) => {
                        add_string_lines(
                            "BREWING STAND",
                            -0.18,
                            0.37,
                            0.012,
                            0.024,
                            0.004,
                            [0.8, 0.6, 0.3, 1.0],
                            &mut ui_line_vertices,
                        );
                        let progress = (self.brewing.progress / 10.0).clamp(0.0, 1.0);
                        add_ui_quad(
                            &mut ui_vertices,
                            0.04,
                            0.54,
                            0.20,
                            0.24,
                            [0.05, 0.05, 0.05, 1.0],
                        );
                        add_ui_quad(
                            &mut ui_vertices,
                            0.04,
                            0.04 + 0.5 * progress,
                            0.20,
                            0.24,
                            [0.85, 0.45, 0.1, 1.0],
                        );
                        let status = if self.brewing.can_brew() {
                            format!("BREWING {:.0} PCT", progress * 100.0)
                        } else {
                            "ADD BOTTLES AND INGREDIENT".to_string()
                        };
                        add_string_lines(
                            &status,
                            0.04,
                            0.28,
                            0.008,
                            0.016,
                            0.003,
                            [1.0, 0.85, 0.55, 1.0],
                            &mut ui_line_vertices,
                        );
                    }
                    Some(StationKind::Anvil) => {
                        add_string_lines(
                            "ANVIL",
                            -0.20,
                            0.37,
                            0.012,
                            0.024,
                            0.004,
                            [0.8, 0.8, 0.8, 1.0],
                            &mut ui_line_vertices,
                        );
                        add_ui_quad(
                            &mut ui_vertices,
                            -0.20,
                            0.45,
                            0.25,
                            0.31,
                            [0.04, 0.04, 0.04, 0.95],
                        );
                        let rename = if self.anvil.rename.is_empty() {
                            "TYPE A NAME"
                        } else {
                            &self.anvil.rename
                        };
                        add_string_lines(
                            rename,
                            -0.18,
                            0.27,
                            0.009,
                            0.018,
                            0.003,
                            [1.0, 1.0, 1.0, 1.0],
                            &mut ui_line_vertices,
                        );
                        let cost = format!("COST {} LEVELS", self.anvil.cost);
                        add_string_lines(
                            &cost,
                            0.20,
                            0.05,
                            0.008,
                            0.016,
                            0.003,
                            [0.5, 1.0, 0.5, 1.0],
                            &mut ui_line_vertices,
                        );
                    }
                    None => {}
                }

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

                    let c = if dragged.enchantments.is_empty() {
                        [1.0, 1.0, 1.0, 1.0]
                    } else {
                        [0.82, 0.65 + (self.total_time * 3.0).sin() * 0.18, 1.0, 1.0]
                    };
                    ui_textured_vertices.push(TexturedUiVertex {
                        position: [dx0, dy1, 0.0],
                        tex_coords: [u0, v0],
                        color: c,
                    });
                    ui_textured_vertices.push(TexturedUiVertex {
                        position: [dx0, dy0, 0.0],
                        tex_coords: [u0, v1],
                        color: c,
                    });
                    ui_textured_vertices.push(TexturedUiVertex {
                        position: [dx1, dy0, 0.0],
                        tex_coords: [u1, v1],
                        color: c,
                    });
                    ui_textured_vertices.push(TexturedUiVertex {
                        position: [dx0, dy1, 0.0],
                        tex_coords: [u0, v0],
                        color: c,
                    });
                    ui_textured_vertices.push(TexturedUiVertex {
                        position: [dx1, dy0, 0.0],
                        tex_coords: [u1, v1],
                        color: c,
                    });
                    ui_textured_vertices.push(TexturedUiVertex {
                        position: [dx1, dy1, 0.0],
                        tex_coords: [u1, v0],
                        color: c,
                    });

                    if dragged.count > 1 {
                        let count_str = format!("{}", dragged.count);
                        let cw = 0.008;
                        let ch = 0.016;
                        let cs = 0.003;
                        let n_chars = count_str.len() as f32;
                        let count_w = n_chars * cw + (n_chars - 1.0) * cs;
                        let count_x = mouse_x + slot_w / 2.0 - count_w - 0.008;
                        let count_y = mouse_y - slot_h / 2.0 + 0.01 * aspect;
                        add_string_lines(
                            &count_str,
                            count_x,
                            count_y,
                            cw,
                            ch,
                            cs,
                            [1.0, 1.0, 1.0, 1.0],
                            &mut ui_line_vertices,
                        );
                    }
                }

                // 6. Draw tooltip for hovered slot
                if self.inventory.dragged.is_none() {
                    if let Some((slot_type, _, _, _, _)) = hovered_slot {
                        if let Some(stack) = self.get_item_at_slot(slot_type) {
                            let name = if !stack.custom_name.is_empty() {
                                stack.custom_name.as_str().to_string()
                            } else if let Some(potion) = stack.potion {
                                potion.display_name().to_string()
                            } else {
                                stack.item.properties().name.to_string()
                            };
                            let tw = name.len() as f32 * 0.014 + 0.02;
                            let th = 0.035 * aspect;
                            let tx = mouse_x + 0.02;
                            let ty = mouse_y + 0.02;

                            let tt_bg = [0.05, 0.05, 0.1, 0.95];
                            ui_vertices.push(UiVertex {
                                position: [tx, ty + th, 0.0],
                                color: tt_bg,
                            });
                            ui_vertices.push(UiVertex {
                                position: [tx, ty, 0.0],
                                color: tt_bg,
                            });
                            ui_vertices.push(UiVertex {
                                position: [tx + tw, ty, 0.0],
                                color: tt_bg,
                            });
                            ui_vertices.push(UiVertex {
                                position: [tx, ty + th, 0.0],
                                color: tt_bg,
                            });
                            ui_vertices.push(UiVertex {
                                position: [tx + tw, ty, 0.0],
                                color: tt_bg,
                            });
                            ui_vertices.push(UiVertex {
                                position: [tx + tw, ty + th, 0.0],
                                color: tt_bg,
                            });

                            let tt_border = [0.3, 0.3, 0.7, 1.0];
                            ui_line_vertices.push(UiVertex {
                                position: [tx, ty + th, 0.0],
                                color: tt_border,
                            });
                            ui_line_vertices.push(UiVertex {
                                position: [tx + tw, ty + th, 0.0],
                                color: tt_border,
                            });
                            ui_line_vertices.push(UiVertex {
                                position: [tx + tw, ty + th, 0.0],
                                color: tt_border,
                            });
                            ui_line_vertices.push(UiVertex {
                                position: [tx + tw, ty, 0.0],
                                color: tt_border,
                            });
                            ui_line_vertices.push(UiVertex {
                                position: [tx + tw, ty, 0.0],
                                color: tt_border,
                            });
                            ui_line_vertices.push(UiVertex {
                                position: [tx, ty, 0.0],
                                color: tt_border,
                            });
                            ui_line_vertices.push(UiVertex {
                                position: [tx, ty, 0.0],
                                color: tt_border,
                            });
                            ui_line_vertices.push(UiVertex {
                                position: [tx, ty + th, 0.0],
                                color: tt_border,
                            });

                            add_string_lines(
                                &name,
                                tx + 0.01,
                                ty + 0.01 * aspect,
                                0.008,
                                0.016,
                                0.003,
                                [1.0, 1.0, 1.0, 1.0],
                                &mut ui_line_vertices,
                            );
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
                ui_vertices.push(UiVertex {
                    position: [bg_x0, bg_y1, 0.0],
                    color: bg_color,
                });
                ui_vertices.push(UiVertex {
                    position: [bg_x0, bg_y0, 0.0],
                    color: bg_color,
                });
                ui_vertices.push(UiVertex {
                    position: [bg_x1, bg_y0, 0.0],
                    color: bg_color,
                });
                ui_vertices.push(UiVertex {
                    position: [bg_x0, bg_y1, 0.0],
                    color: bg_color,
                });
                ui_vertices.push(UiVertex {
                    position: [bg_x1, bg_y0, 0.0],
                    color: bg_color,
                });
                ui_vertices.push(UiVertex {
                    position: [bg_x1, bg_y1, 0.0],
                    color: bg_color,
                });

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
                    ui_line_vertices.push(UiVertex {
                        position: [x0, y1, 0.0],
                        color: border_color,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [x1, y1, 0.0],
                        color: border_color,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [x1, y1, 0.0],
                        color: border_color,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [x1, y0, 0.0],
                        color: border_color,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [x1, y0, 0.0],
                        color: border_color,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [x0, y0, 0.0],
                        color: border_color,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [x0, y0, 0.0],
                        color: border_color,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [x0, y1, 0.0],
                        color: border_color,
                    });

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

                        let c = if stack.enchantments.is_empty() {
                            [1.0, 1.0, 1.0, 1.0]
                        } else {
                            [0.82, 0.65 + (self.total_time * 3.0).sin() * 0.18, 1.0, 1.0]
                        };
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [tx0, ty1, 0.0],
                            tex_coords: [u0, v0],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [tx0, ty0, 0.0],
                            tex_coords: [u0, v1],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [tx1, ty0, 0.0],
                            tex_coords: [u1, v1],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [tx0, ty1, 0.0],
                            tex_coords: [u0, v0],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [tx1, ty0, 0.0],
                            tex_coords: [u1, v1],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [tx1, ty1, 0.0],
                            tex_coords: [u1, v0],
                            color: c,
                        });

                        if stack.count > 1 {
                            let count_str = format!("{}", stack.count);
                            let cw = 0.008;
                            let ch = 0.016;
                            let cs = 0.003;
                            let n_chars = count_str.len() as f32;
                            let count_w = n_chars * cw + (n_chars - 1.0) * cs;
                            let count_x = x1 - count_w - 0.01;
                            let count_y = y0 + 0.012 * aspect;
                            add_string_lines(
                                &count_str,
                                count_x,
                                count_y,
                                cw,
                                ch,
                                cs,
                                [1.0, 1.0, 1.0, 1.0],
                                &mut ui_line_vertices,
                            );
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
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [hx0, hy1, 0.0],
                            tex_coords: [u0, v0],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [hx0, hy0, 0.0],
                            tex_coords: [u0, v1],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [hx1, hy0, 0.0],
                            tex_coords: [u1, v1],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [hx0, hy1, 0.0],
                            tex_coords: [u0, v0],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [hx1, hy0, 0.0],
                            tex_coords: [u1, v1],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [hx1, hy1, 0.0],
                            tex_coords: [u1, v0],
                            color: c,
                        });
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
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [hx0, hy1, 0.0],
                            tex_coords: [u0, v0],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [hx0, hy0, 0.0],
                            tex_coords: [u0, v1],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [hx1, hy0, 0.0],
                            tex_coords: [u1, v1],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [hx0, hy1, 0.0],
                            tex_coords: [u0, v0],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [hx1, hy0, 0.0],
                            tex_coords: [u1, v1],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [hx1, hy1, 0.0],
                            tex_coords: [u1, v0],
                            color: c,
                        });
                    }

                    // Draw Oxygen HUD
                    if self.player_state.oxygen < 300.0 {
                        let oxygen = self.player_state.oxygen;
                        let bubble_count = (oxygen / 30.0).ceil() as i32;
                        let y_bubbles = y_hud + hud_h + 0.005;

                        for i in 0..bubble_count {
                            let col = 15;
                            let row = 3;
                            let u0 = col as f32 * 0.0625;
                            let u1 = (col + 1) as f32 * 0.0625;
                            let v0 = row as f32 * 0.0625;
                            let v1 = (row + 1) as f32 * 0.0625;

                            let slot_idx = 9 - i;
                            let hx0 = x_hunger_start + slot_idx as f32 * (hud_w + hud_gap);
                            let hx1 = hx0 + hud_w;
                            let hy0 = y_bubbles;
                            let hy1 = hy0 + hud_h;

                            let c = [1.0, 1.0, 1.0, 1.0];
                            ui_textured_vertices.push(TexturedUiVertex {
                                position: [hx0, hy1, 0.0],
                                tex_coords: [u0, v0],
                                color: c,
                            });
                            ui_textured_vertices.push(TexturedUiVertex {
                                position: [hx0, hy0, 0.0],
                                tex_coords: [u0, v1],
                                color: c,
                            });
                            ui_textured_vertices.push(TexturedUiVertex {
                                position: [hx1, hy0, 0.0],
                                tex_coords: [u1, v1],
                                color: c,
                            });
                            ui_textured_vertices.push(TexturedUiVertex {
                                position: [hx0, hy1, 0.0],
                                tex_coords: [u0, v0],
                                color: c,
                            });
                            ui_textured_vertices.push(TexturedUiVertex {
                                position: [hx1, hy0, 0.0],
                                tex_coords: [u1, v1],
                                color: c,
                            });
                            ui_textured_vertices.push(TexturedUiVertex {
                                position: [hx1, hy1, 0.0],
                                tex_coords: [u1, v0],
                                color: c,
                            });
                        }
                    }
                }

                // Selected Block/Item Text
                let selected_item = self.inventory.hotbar[self.inventory.selected]
                    .map(|s| s.item)
                    .unwrap_or(crate::inventory::Item::Air);
                let selected_text = format!("{:?}", selected_item).to_uppercase();
                let char_w = 0.010;
                let char_h = 0.020;
                let spacing = 0.004;
                let n = selected_text.len() as f32;
                let width = n * char_w + (n - 1.0) * spacing;
                let text_x = -width / 2.0;
                add_string_lines(
                    &selected_text,
                    text_x,
                    -0.78,
                    char_w,
                    char_h,
                    spacing,
                    [1.0, 1.0, 1.0, 1.0],
                    &mut ui_line_vertices,
                );

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
                add_string_lines(
                    mode_text,
                    mode_x,
                    -0.71,
                    mode_w,
                    mode_h,
                    mode_s,
                    [1.0, 0.9, 0.4, 1.0],
                    &mut ui_line_vertices,
                );

                if self.game_mode == GameMode::Survival {
                    let xp_text = format!("LEVEL {}", self.player_state.experience_level);
                    let width = xp_text.len() as f32 * 0.009;
                    add_string_lines(
                        &xp_text,
                        -width / 2.0,
                        -0.66,
                        0.009,
                        0.018,
                        0.003,
                        [0.35, 1.0, 0.25, 1.0],
                        &mut ui_line_vertices,
                    );
                }

                for (index, effect) in self.potion_effects.active.iter().enumerate() {
                    let seconds = effect.remaining().ceil() as u32;
                    let text = format!("{} {}:{:02}", effect.name(), seconds / 60, seconds % 60);
                    add_string_lines(
                        &text,
                        0.54,
                        0.86 - index as f32 * 0.05,
                        0.007,
                        0.014,
                        0.002,
                        [0.75, 0.55, 1.0, 1.0],
                        &mut ui_line_vertices,
                    );
                }

                // Damaged screen red flash overlay
                if self.player_state.damaged_flash_time > 0.0 {
                    let alpha = (self.player_state.damaged_flash_time / 0.5).min(1.0) * 0.25;
                    let flash_color = [1.0, 0.0, 0.0, alpha];
                    ui_vertices.push(UiVertex {
                        position: [-1.0, 1.0, 0.0],
                        color: flash_color,
                    });
                    ui_vertices.push(UiVertex {
                        position: [-1.0, -1.0, 0.0],
                        color: flash_color,
                    });
                    ui_vertices.push(UiVertex {
                        position: [1.0, -1.0, 0.0],
                        color: flash_color,
                    });
                    ui_vertices.push(UiVertex {
                        position: [-1.0, 1.0, 0.0],
                        color: flash_color,
                    });
                    ui_vertices.push(UiVertex {
                        position: [1.0, -1.0, 0.0],
                        color: flash_color,
                    });
                    ui_vertices.push(UiVertex {
                        position: [1.0, 1.0, 0.0],
                        color: flash_color,
                    });
                }

                let lightning_flash = self.weather.flash_intensity();
                if lightning_flash > 0.0 {
                    let flash_color = [1.0, 1.0, 1.0, lightning_flash * 0.82];
                    for position in [
                        [-1.0, 1.0, 0.0],
                        [-1.0, -1.0, 0.0],
                        [1.0, -1.0, 0.0],
                        [-1.0, 1.0, 0.0],
                        [1.0, -1.0, 0.0],
                        [1.0, 1.0, 0.0],
                    ] {
                        ui_vertices.push(UiVertex {
                            position,
                            color: flash_color,
                        });
                    }
                }

                // F3 Debug Screen
                if self.show_debug {
                    let frame_str = format!(
                        "FPS: {:.1} / FRAME: {:.2} MS",
                        self.debug_fps, self.debug_frame_ms
                    );

                    let time_of_day = self.world_time.time_of_day_smooth();
                    let hour = ((time_of_day * 24.0 + 6.0) % 24.0).floor() as u32;
                    let minute = (((time_of_day * 24.0 + 6.0) % 1.0) * 60.0).floor() as u32;
                    let day = self.world_time.ticks / self.world_time.day_length;
                    let time_str = format!(
                        "TIME: {:02}:{:02} / DAY: {} / TICKS: {}",
                        hour, minute, day, self.world_time.ticks
                    );

                    let pos = self.player_physics.position;
                    let pos_str = format!("XYZ: {:.3} / {:.3} / {:.3}", pos.x, pos.y, pos.z);
                    let facing_str = format!(
                        "FACING: YAW {:.2} / PITCH {:.2}",
                        self.camera.yaw.to_degrees().rem_euclid(360.0),
                        self.camera.pitch.to_degrees()
                    );
                    let chunk_x = debug_chunk_coordinate(pos.x, CHUNK_WIDTH);
                    let chunk_z = debug_chunk_coordinate(pos.z, CHUNK_DEPTH);
                    let chunk_str = format!("CHUNK: {} / {}", chunk_x, chunk_z);

                    let biome = self
                        .weather
                        .biome_at(pos.x.floor() as i32, pos.z.floor() as i32);
                    let biome_str = format!("BIOME: {}", biome_debug_name(biome));
                    let weather_str = format!("WEATHER: {:?}", self.weather.current).to_uppercase();
                    let chunks_str = format!("CHUNKS LOADED: {}", self.chunk_manager.chunks.len());
                    let entities_str = format!(
                        "ENTITIES: {} / PARTICLES: {}",
                        self.entity_manager.entities.len(),
                        self.particles.particles.len()
                    );

                    let terrain_indices: u64 = self
                        .chunk_meshes
                        .values()
                        .map(|mesh| {
                            u64::from(mesh.opaque_num_indices)
                                + u64::from(mesh.transparent_num_indices)
                        })
                        .sum();
                    let rendered_indices = terrain_indices
                        + u64::from(self.mob_num_indices)
                        + u64::from(self.particle_num_indices);
                    let rendered_triangles = rendered_indices / 3;
                    let rendered_vertices = rendered_indices * 2 / 3;
                    let render_str = format!(
                        "RENDER: {} VERTICES / {} TRIANGLES",
                        rendered_vertices, rendered_triangles
                    );
                    let memory_str = format!(
                        "MEMORY EST: {:.1} MB",
                        self.estimated_debug_memory_bytes() as f64 / (1024.0 * 1024.0)
                    );

                    let char_w = 0.007;
                    let char_h = 0.014;
                    let spacing = 0.002;

                    let start_x = -0.98;
                    let start_y = 0.95;
                    let line_gap = 0.025;

                    let debug_lines = [
                        frame_str,
                        pos_str,
                        facing_str,
                        chunk_str,
                        biome_str,
                        weather_str,
                        chunks_str,
                        entities_str,
                        render_str,
                        memory_str,
                        time_str,
                    ];
                    for (line_index, line) in debug_lines.iter().enumerate() {
                        add_string_lines(
                            line,
                            start_x,
                            start_y - line_gap * line_index as f32,
                            char_w,
                            char_h,
                            spacing,
                            [1.0, 1.0, 1.0, 1.0],
                            &mut ui_line_vertices,
                        );
                    }
                }
            }

            if let Some(boss) = crate::boss::active_boss_hud(&self.entity_manager) {
                let x0 = -0.42;
                let x1 = 0.42;
                let y0 = 0.82;
                let y1 = 0.875;
                add_ui_quad(&mut ui_vertices, x0, x1, y0, y1, [0.05, 0.01, 0.07, 0.92]);
                add_ui_quad(
                    &mut ui_vertices,
                    x0 + 0.008,
                    x0 + 0.008 + (x1 - x0 - 0.016) * boss.progress,
                    y0 + 0.009,
                    y1 - 0.009,
                    [0.55, 0.05, 0.65, 1.0],
                );
                let char_w = 0.010;
                let spacing = 0.003;
                let width = boss.title.len() as f32 * (char_w + spacing) - spacing;
                add_string_lines(
                    boss.title,
                    -width / 2.0,
                    0.895,
                    char_w,
                    0.02,
                    spacing,
                    [1.0, 1.0, 1.0, 1.0],
                    &mut ui_line_vertices,
                );
            }

            self.render_advancement_ui_and_toasts(
                &mut ui_vertices,
                &mut ui_line_vertices,
                &mut ui_textured_vertices,
            );

            // Write Buffers
            let ui_vert_len = ui_vertices.len().min(UI_VERTEX_CAPACITY);
            let ui_line_vert_len = ui_line_vertices.len().min(UI_LINE_VERTEX_CAPACITY);
            let ui_textured_vert_len = ui_textured_vertices.len().min(UI_VERTEX_CAPACITY);

            self.queue.write_buffer(
                &self.ui_vertex_buffer,
                0,
                bytemuck::cast_slice(&ui_vertices[..ui_vert_len]),
            );
            self.queue.write_buffer(
                &self.ui_line_vertex_buffer,
                0,
                bytemuck::cast_slice(&ui_line_vertices[..ui_line_vert_len]),
            );
            self.queue.write_buffer(
                &self.ui_textured_vertex_buffer,
                0,
                bytemuck::cast_slice(&ui_textured_vertices[..ui_textured_vert_len]),
            );

            self.num_ui_vertices = ui_vert_len as u32;
            self.num_ui_line_vertices = ui_line_vert_len as u32;
            self.num_ui_textured_vertices = ui_textured_vert_len as u32;
        }

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
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
                            r: self.camera_uniform.sky_color_horizon[0] as f64,
                            g: self.camera_uniform.sky_color_horizon[1] as f64,
                            b: self.camera_uniform.sky_color_horizon[2] as f64,
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
                    render_pass.set_index_buffer(
                        mesh.opaque_index_buffer.slice(..),
                        wgpu::IndexFormat::Uint32,
                    );
                    render_pass.draw_indexed(0..mesh.opaque_num_indices, 0, 0..1);
                }
            }

            // Draw Mobs
            if self.mob_num_indices > 0 {
                render_pass.set_vertex_buffer(0, self.mob_vertex_buffer.slice(..));
                render_pass
                    .set_index_buffer(self.mob_index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                render_pass.draw_indexed(0..self.mob_num_indices, 0, 0..1);
            }

            // Pass 2: Translucent (Water/Ice)
            render_pass.set_pipeline(&self.trans_pipeline);
            for mesh in self.chunk_meshes.values() {
                if mesh.transparent_num_indices > 0 {
                    render_pass.set_vertex_buffer(0, mesh.transparent_vertex_buffer.slice(..));
                    render_pass.set_index_buffer(
                        mesh.transparent_index_buffer.slice(..),
                        wgpu::IndexFormat::Uint32,
                    );
                    render_pass.draw_indexed(0..mesh.transparent_num_indices, 0, 0..1);
                }
            }

            // Draw billboard particles using the translucent (alpha-blend) pipeline.
            if self.particle_num_indices > 0 {
                render_pass.set_vertex_buffer(0, self.particle_vertex_buffer.slice(..));
                render_pass.set_index_buffer(
                    self.particle_index_buffer.slice(..),
                    wgpu::IndexFormat::Uint32,
                );
                render_pass.draw_indexed(0..self.particle_num_indices, 0, 0..1);
            }

            // Draw Block cracking animation overlay (multiply blend)
            if let Some(target) = self.mining_target {
                if self.mining_progress > 0.0 {
                    if let Some((_num_vertices, num_indices)) =
                        self.update_crack_buffers(target, self.mining_progress)
                    {
                        render_pass.set_pipeline(&self.crack_pipeline);
                        render_pass.set_vertex_buffer(0, self.crack_vertex_buffer.slice(..));
                        render_pass.set_index_buffer(
                            self.crack_index_buffer.slice(..),
                            wgpu::IndexFormat::Uint32,
                        );
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

    fn render_advancement_ui_and_toasts(
        &self,
        ui_vertices: &mut Vec<UiVertex>,
        ui_line_vertices: &mut Vec<UiVertex>,
        ui_textured_vertices: &mut Vec<TexturedUiVertex>,
    ) {
        let (screen_w, screen_h) = (self.config.width as f32, self.config.height as f32);
        let aspect = screen_w / screen_h.max(1.0);

        // 1. Render Toast Notifications (top-right overlay)
        for toast in &self.advancement_manager.active_toasts {
            let slide = if toast.timer < 0.4 {
                (1.0 - (toast.timer / 0.4)) * 0.4
            } else if toast.timer > 2.6 {
                ((toast.timer - 2.6) / 0.4) * 0.4
            } else {
                0.0
            };

            let x0 = 0.55 + slide;
            let x1 = 0.95 + slide;
            let y0 = 0.72;
            let y1 = 0.92;

            add_ui_quad(ui_vertices, x0, x1, y0, y1, [0.08, 0.08, 0.12, 0.88]);

            let border_col = match toast.frame {
                crate::advancements::AdvancementFrameType::Challenge => [1.0, 0.85, 0.2, 1.0],
                crate::advancements::AdvancementFrameType::Goal => [0.4, 0.8, 1.0, 1.0],
                crate::advancements::AdvancementFrameType::Task => [0.9, 0.9, 0.9, 1.0],
            };
            add_ui_border(ui_line_vertices, x0, x1, y0, y1, border_col);

            let (col, row) = toast.icon_item.properties().tex_coords;
            let u0 = col as f32 * 0.0625;
            let u1 = (col + 1) as f32 * 0.0625;
            let v0 = row as f32 * 0.0625;
            let v1 = (row + 1) as f32 * 0.0625;

            let ix0 = x0 + 0.02;
            let ix1 = x0 + 0.08;
            let iy0 = y0 + 0.03 * aspect;
            let iy1 = y1 - 0.03 * aspect;

            ui_textured_vertices.push(TexturedUiVertex {
                position: [ix0, iy1, 0.0],
                tex_coords: [u0, v0],
                color: [1.0, 1.0, 1.0, 1.0],
            });
            ui_textured_vertices.push(TexturedUiVertex {
                position: [ix0, iy0, 0.0],
                tex_coords: [u0, v1],
                color: [1.0, 1.0, 1.0, 1.0],
            });
            ui_textured_vertices.push(TexturedUiVertex {
                position: [ix1, iy0, 0.0],
                tex_coords: [u1, v1],
                color: [1.0, 1.0, 1.0, 1.0],
            });
            ui_textured_vertices.push(TexturedUiVertex {
                position: [ix0, iy1, 0.0],
                tex_coords: [u0, v0],
                color: [1.0, 1.0, 1.0, 1.0],
            });
            ui_textured_vertices.push(TexturedUiVertex {
                position: [ix1, iy0, 0.0],
                tex_coords: [u1, v1],
                color: [1.0, 1.0, 1.0, 1.0],
            });
            ui_textured_vertices.push(TexturedUiVertex {
                position: [ix1, iy1, 0.0],
                tex_coords: [u1, v0],
                color: [1.0, 1.0, 1.0, 1.0],
            });

            add_string_lines(
                "ADVANCEMENT MADE!",
                x0 + 0.09,
                y1 - 0.04 * aspect,
                0.007,
                0.014,
                0.002,
                border_col,
                ui_line_vertices,
            );
            add_string_lines(
                &toast.title.to_uppercase(),
                x0 + 0.09,
                y1 - 0.10 * aspect,
                0.008,
                0.016,
                0.002,
                [1.0, 1.0, 1.0, 1.0],
                ui_line_vertices,
            );
        }

        // 2. Render Advancements GUI screen when open
        if self.advancement_gui.is_open {
            add_ui_quad(ui_vertices, -1.0, 1.0, -1.0, 1.0, [0.0, 0.0, 0.0, 0.65]);

            let wx0 = -0.80;
            let wx1 = 0.80;
            let wy0 = -0.80;
            let wy1 = 0.80;

            add_ui_quad(ui_vertices, wx0, wx1, wy0, wy1, [0.12, 0.12, 0.15, 0.95]);
            add_ui_border(ui_line_vertices, wx0, wx1, wy0, wy1, [0.5, 0.5, 0.6, 1.0]);

            let tab_y0 = wy1 - 0.12;
            let tab_y1 = wy1;
            let tab_w = (wx1 - wx0) / 5.0;

            let categories = [
                (crate::advancements::AdvancementCategory::Minecraft, "STORY"),
                (crate::advancements::AdvancementCategory::Nether, "NETHER"),
                (crate::advancements::AdvancementCategory::TheEnd, "THE END"),
                (
                    crate::advancements::AdvancementCategory::Adventure,
                    "ADVENTURE",
                ),
                (
                    crate::advancements::AdvancementCategory::Husbandry,
                    "HUSBANDRY",
                ),
            ];

            for (i, (cat, name)) in categories.iter().enumerate() {
                let tx0 = wx0 + i as f32 * tab_w;
                let tx1 = tx0 + tab_w;
                let is_sel = *cat == self.advancement_gui.selected_category;
                let bg_col = if is_sel {
                    [0.25, 0.25, 0.32, 0.95]
                } else {
                    [0.16, 0.16, 0.20, 0.95]
                };
                let line_col = if is_sel {
                    [0.9, 0.8, 0.3, 1.0]
                } else {
                    [0.35, 0.35, 0.40, 1.0]
                };

                add_ui_quad(ui_vertices, tx0, tx1, tab_y0, tab_y1, bg_col);
                add_ui_border(ui_line_vertices, tx0, tx1, tab_y0, tab_y1, line_col);

                add_string_lines(
                    name,
                    tx0 + 0.015,
                    tab_y0 + 0.035,
                    0.007,
                    0.014,
                    0.002,
                    if is_sel {
                        [1.0, 0.9, 0.4, 1.0]
                    } else {
                        [0.7, 0.7, 0.7, 1.0]
                    },
                    ui_line_vertices,
                );
            }

            let view_x0 = wx0 + 0.02;
            let view_x1 = wx1 - 0.02;
            let view_y0 = wy0 + 0.02;
            let view_y1 = tab_y0 - 0.02;

            let center_x =
                (view_x0 + view_x1) * 0.5 + (self.advancement_gui.scroll_x / screen_w) * 2.0;
            let center_y =
                (view_y0 + view_y1) * 0.5 - (self.advancement_gui.scroll_y / screen_h) * 2.0;
            let zoom = self.advancement_gui.zoom;

            let advs = self
                .advancement_manager
                .tree
                .get_category_advancements(self.advancement_gui.selected_category);

            for adv in &advs {
                let nx = center_x + adv.x_pos * 0.15 * zoom;
                let ny = center_y + adv.y_pos * 0.15 * aspect * zoom;

                if let Some(parent_id) = adv.parent {
                    if let Some(parent_adv) = self.advancement_manager.tree.get(parent_id) {
                        let px = center_x + parent_adv.x_pos * 0.15 * zoom;
                        let py = center_y + parent_adv.y_pos * 0.15 * aspect * zoom;

                        let line_col = if self.advancement_manager.is_unlocked(adv.id) {
                            [0.9, 0.8, 0.3, 1.0]
                        } else {
                            [0.3, 0.3, 0.35, 1.0]
                        };

                        ui_line_vertices.push(UiVertex {
                            position: [px, py, 0.0],
                            color: line_col,
                        });
                        ui_line_vertices.push(UiVertex {
                            position: [nx, ny, 0.0],
                            color: line_col,
                        });
                    }
                }
            }

            let mouse_ndc_x = self.mouse_ndc[0];
            let mouse_ndc_y = self.mouse_ndc[1];
            let mut hovered = None;

            for adv in &advs {
                let nx = center_x + adv.x_pos * 0.15 * zoom;
                let ny = center_y + adv.y_pos * 0.15 * aspect * zoom;

                let nw = 0.04 * zoom;
                let nh = 0.04 * aspect * zoom;
                let bx0 = nx - nw;
                let bx1 = nx + nw;
                let by0 = ny - nh;
                let by1 = ny + nh;

                if mouse_ndc_x >= bx0
                    && mouse_ndc_x <= bx1
                    && mouse_ndc_y >= by0
                    && mouse_ndc_y <= by1
                {
                    hovered = Some(adv.id);
                }

                let is_unlocked = self.advancement_manager.is_unlocked(adv.id);
                let bg_col = if is_unlocked {
                    [0.18, 0.30, 0.18, 0.95]
                } else {
                    [0.10, 0.10, 0.12, 0.95]
                };
                let border_col = match adv.frame {
                    crate::advancements::AdvancementFrameType::Challenge => {
                        if is_unlocked {
                            [1.0, 0.85, 0.2, 1.0]
                        } else {
                            [0.5, 0.4, 0.1, 0.9]
                        }
                    }
                    crate::advancements::AdvancementFrameType::Goal => {
                        if is_unlocked {
                            [0.3, 0.75, 1.0, 1.0]
                        } else {
                            [0.15, 0.35, 0.5, 0.9]
                        }
                    }
                    crate::advancements::AdvancementFrameType::Task => {
                        if is_unlocked {
                            [0.9, 0.9, 0.9, 1.0]
                        } else {
                            [0.4, 0.4, 0.4, 0.9]
                        }
                    }
                };

                add_ui_quad(ui_vertices, bx0, bx1, by0, by1, bg_col);
                add_ui_border(ui_line_vertices, bx0, bx1, by0, by1, border_col);

                let (col, row) = adv.icon_item.properties().tex_coords;
                let u0 = col as f32 * 0.0625;
                let u1 = (col + 1) as f32 * 0.0625;
                let v0 = row as f32 * 0.0625;
                let v1 = (row + 1) as f32 * 0.0625;

                let icon_col = if is_unlocked {
                    [1.0, 1.0, 1.0, 1.0]
                } else {
                    [0.4, 0.4, 0.4, 0.6]
                };

                let ix0 = bx0 + 0.008 * zoom;
                let ix1 = bx1 - 0.008 * zoom;
                let iy0 = by0 + 0.008 * aspect * zoom;
                let iy1 = by1 - 0.008 * aspect * zoom;

                ui_textured_vertices.push(TexturedUiVertex {
                    position: [ix0, iy1, 0.0],
                    tex_coords: [u0, v0],
                    color: icon_col,
                });
                ui_textured_vertices.push(TexturedUiVertex {
                    position: [ix0, iy0, 0.0],
                    tex_coords: [u0, v1],
                    color: icon_col,
                });
                ui_textured_vertices.push(TexturedUiVertex {
                    position: [ix1, iy0, 0.0],
                    tex_coords: [u1, v1],
                    color: icon_col,
                });
                ui_textured_vertices.push(TexturedUiVertex {
                    position: [ix0, iy1, 0.0],
                    tex_coords: [u0, v0],
                    color: icon_col,
                });
                ui_textured_vertices.push(TexturedUiVertex {
                    position: [ix1, iy0, 0.0],
                    tex_coords: [u1, v1],
                    color: icon_col,
                });
                ui_textured_vertices.push(TexturedUiVertex {
                    position: [ix1, iy1, 0.0],
                    tex_coords: [u1, v0],
                    color: icon_col,
                });
            }

            if let Some(adv_id) = hovered {
                if let Some(adv) = self.advancement_manager.tree.get(adv_id) {
                    let tx0 = mouse_ndc_x + 0.02;
                    let tx1 = tx0 + 0.40;
                    let ty0 = mouse_ndc_y - 0.15;
                    let ty1 = mouse_ndc_y;

                    add_ui_quad(ui_vertices, tx0, tx1, ty0, ty1, [0.05, 0.05, 0.08, 0.95]);
                    add_ui_border(ui_line_vertices, tx0, tx1, ty0, ty1, [0.8, 0.8, 0.3, 1.0]);

                    add_string_lines(
                        &adv.title.to_uppercase(),
                        tx0 + 0.015,
                        ty1 - 0.04,
                        0.008,
                        0.016,
                        0.002,
                        [1.0, 1.0, 1.0, 1.0],
                        ui_line_vertices,
                    );

                    let status = if self.advancement_manager.is_unlocked(adv.id) {
                        "[COMPLETED]"
                    } else {
                        "[LOCKED]"
                    };
                    let status_col = if self.advancement_manager.is_unlocked(adv.id) {
                        [0.3, 1.0, 0.3, 1.0]
                    } else {
                        [0.8, 0.3, 0.3, 1.0]
                    };
                    add_string_lines(
                        status,
                        tx0 + 0.015,
                        ty1 - 0.08,
                        0.007,
                        0.014,
                        0.002,
                        status_col,
                        ui_line_vertices,
                    );
                }
            }
        }
    }
}

fn add_ui_quad(vertices: &mut Vec<UiVertex>, x0: f32, x1: f32, y0: f32, y1: f32, color: [f32; 4]) {
    for position in [
        [x0, y1, 0.0],
        [x0, y0, 0.0],
        [x1, y0, 0.0],
        [x0, y1, 0.0],
        [x1, y0, 0.0],
        [x1, y1, 0.0],
    ] {
        vertices.push(UiVertex { position, color });
    }
}

fn add_ui_border(
    vertices: &mut Vec<UiVertex>,
    x0: f32,
    x1: f32,
    y0: f32,
    y1: f32,
    color: [f32; 4],
) {
    for (p1, p2) in [
        ([x0, y1, 0.0], [x1, y1, 0.0]),
        ([x1, y1, 0.0], [x1, y0, 0.0]),
        ([x1, y0, 0.0], [x0, y0, 0.0]),
        ([x0, y0, 0.0], [x0, y1, 0.0]),
    ] {
        vertices.push(UiVertex {
            position: p1,
            color,
        });
        vertices.push(UiVertex {
            position: p2,
            color,
        });
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
        vertices.push(UiVertex {
            position: [x_start, y_start, 0.0],
            color,
        });
        vertices.push(UiVertex {
            position: [x_end, y_end, 0.0],
            color,
        });
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
        'B' => {
            add_line(x0, y0, x0, y1);
            add_line(x0, y1, x1, y1);
            add_line(x1, y1, x1, ym);
            add_line(x1, ym, x0, ym);
            add_line(x1, ym, x1, y0);
            add_line(x1, y0, x0, y0);
        }
        'K' => {
            add_line(x0, y0, x0, y1);
            add_line(x0, ym, x1, y1);
            add_line(x0, ym, x1, y0);
        }
        'W' => {
            add_line(x0, y1, x0 + w * 0.2, y0);
            add_line(x0 + w * 0.2, y0, xm, ym);
            add_line(xm, ym, x0 + w * 0.8, y0);
            add_line(x0 + w * 0.8, y0, x1, y1);
        }
        'X' => {
            add_line(x0, y0, x1, y1);
            add_line(x0, y1, x1, y0);
        }
        'Z' => {
            add_line(x0, y1, x1, y1);
            add_line(x1, y1, x0, y0);
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
        '/' => {
            add_line(x0, y0, x1, y1);
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

fn weather_tile_uv(column: u32, row: u32) -> [f32; 4] {
    let inset = 0.08;
    [
        (column as f32 + inset) / 16.0,
        (row as f32 + inset) / 16.0,
        (column as f32 + 1.0 - inset) / 16.0,
        (row as f32 + 1.0 - inset) / 16.0,
    ]
}

impl Drop for State {
    fn drop(&mut self) {
        let _ = self
            .window
            .set_cursor_grab(winit::window::CursorGrabMode::None);
        self.window.set_cursor_visible(true);
    }
}

fn biome_debug_name(biome: Biome) -> &'static str {
    match biome {
        Biome::Plains => "PLAINS",
        Biome::Forest => "FOREST",
        Biome::Desert => "DESERT",
        Biome::Taiga => "TAIGA",
        Biome::Swamp => "SWAMP",
        Biome::Mountains => "MOUNTAINS",
        Biome::Ocean => "OCEAN",
    }
}

fn debug_chunk_coordinate(position: f32, chunk_size: usize) -> i32 {
    (position.floor() as i32).div_euclid(chunk_size as i32)
}

#[cfg(test)]
mod debug_tests {
    use super::*;

    #[test]
    fn terrain_vertex_layout_exposes_ambient_occlusion() {
        let layout = Vertex::desc();
        assert_eq!(std::mem::size_of::<Vertex>(), 28);
        assert_eq!(layout.array_stride, 28);
        assert_eq!(layout.attributes.len(), 4);
        assert_eq!(layout.attributes[3].offset, 24);
        assert_eq!(layout.attributes[3].shader_location, 3);
        assert_eq!(layout.attributes[3].format, wgpu::VertexFormat::Float32);
    }

    #[test]
    fn debug_chunk_coordinates_handle_negative_world_positions() {
        assert_eq!(debug_chunk_coordinate(0.0, CHUNK_WIDTH), 0);
        assert_eq!(debug_chunk_coordinate(15.999, CHUNK_WIDTH), 0);
        assert_eq!(debug_chunk_coordinate(16.0, CHUNK_WIDTH), 1);
        assert_eq!(debug_chunk_coordinate(-0.001, CHUNK_WIDTH), -1);
        assert_eq!(debug_chunk_coordinate(-16.0, CHUNK_WIDTH), -1);
        assert_eq!(debug_chunk_coordinate(-16.001, CHUNK_WIDTH), -2);
    }

    #[test]
    fn initial_world_load_is_bounded_independently_of_render_distance() {
        assert_eq!(initial_chunk_radius(0), 0);
        assert_eq!(initial_chunk_radius(2), INITIAL_WORLD_CHUNK_RADIUS);
        assert_eq!(initial_chunk_radius(12), INITIAL_WORLD_CHUNK_RADIUS);
        assert_eq!(initial_chunk_radius(16), INITIAL_WORLD_CHUNK_RADIUS);
    }

    #[test]
    fn debug_overlay_font_supports_every_required_character() {
        let mut vertices = Vec::new();
        for character in ['B', 'K', 'W', 'X', 'Z', '/'] {
            let before = vertices.len();
            add_char_lines(character, 0.0, 0.0, 0.1, 0.2, [1.0; 4], &mut vertices);
            assert!(vertices.len() > before, "missing glyph for {character}");
        }
    }

    #[test]
    fn every_biome_has_a_debug_name() {
        let biomes = [
            Biome::Plains,
            Biome::Forest,
            Biome::Desert,
            Biome::Taiga,
            Biome::Swamp,
            Biome::Mountains,
            Biome::Ocean,
        ];
        assert!(biomes
            .into_iter()
            .all(|biome| !biome_debug_name(biome).is_empty()));
    }

    #[test]
    fn test_flower_breaks_and_pops_when_ground_is_destroyed() {
        let mut manager = ChunkManager::new(2);
        manager.chunks.insert((0, 0), Chunk::new(0, 0));
        manager.set_block(2, 10, 2, BlockType::Grass);
        manager.set_block(2, 11, 2, BlockType::Dandelion);

        let mut dirty = std::collections::HashSet::new();
        let mut drops = Vec::new();

        // Destroy the grass block
        manager.set_block(2, 10, 2, BlockType::Air);
        manager.check_and_break_unsupported_above(2, 10, 2, &mut dirty, |pos, block| {
            drops.push((pos, block));
        });

        // Ground is Air now, flower above must be destroyed
        assert_eq!(manager.get_block(2, 11, 2), BlockType::Air);
        assert_eq!(drops, vec![((2, 11, 2), BlockType::Dandelion)]);
    }
}

