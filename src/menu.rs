use crate::inventory::GameMode;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use wgpu::util::DeviceExt;
use winit::event::ElementState;
use winit::keyboard::{Key, KeyCode, NamedKey, PhysicalKey};
use winit::window::{Fullscreen, Window};

const UI_VERTEX_CAPACITY: usize = 65_536;
const SETTINGS_FILE: &str = "settings.txt";
const SAVES_DIR: &str = "saves";
const META_FILE: &str = "world.meta";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Difficulty {
    Peaceful,
    Easy,
    Normal,
    Hard,
}

impl Difficulty {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Peaceful => "PEACEFUL",
            Self::Easy => "EASY",
            Self::Normal => "NORMAL",
            Self::Hard => "HARD",
        }
    }

    fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "peaceful" => Self::Peaceful,
            "easy" => Self::Easy,
            "hard" => Self::Hard,
            _ => Self::Normal,
        }
    }

    fn step(self, delta: i32) -> Self {
        let values = [Self::Peaceful, Self::Easy, Self::Normal, Self::Hard];
        let index = values.iter().position(|value| *value == self).unwrap_or(2) as i32;
        values[(index + delta).rem_euclid(values.len() as i32) as usize]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    English,
    German,
}

impl Language {
    fn as_str(self) -> &'static str {
        match self {
            Self::English => "ENGLISH",
            Self::German => "DEUTSCH",
        }
    }

    fn parse(value: &str) -> Self {
        if value.trim().eq_ignore_ascii_case("deutsch")
            || value.trim().eq_ignore_ascii_case("german")
        {
            Self::German
        } else {
            Self::English
        }
    }

    fn toggle(self) -> Self {
        match self {
            Self::English => Self::German,
            Self::German => Self::English,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ControlBindings {
    pub forward: KeyCode,
    pub backward: KeyCode,
    pub left: KeyCode,
    pub right: KeyCode,
    pub jump: KeyCode,
    pub sprint: KeyCode,
    pub sneak: KeyCode,
    pub inventory: KeyCode,
}

impl Default for ControlBindings {
    fn default() -> Self {
        Self {
            forward: KeyCode::KeyW,
            backward: KeyCode::KeyS,
            left: KeyCode::KeyA,
            right: KeyCode::KeyD,
            jump: KeyCode::Space,
            sprint: KeyCode::ControlLeft,
            sneak: KeyCode::ShiftLeft,
            inventory: KeyCode::KeyE,
        }
    }
}

#[derive(Debug, Clone)]
pub struct GameSettings {
    pub fov: f32,
    pub sensitivity: f32,
    pub render_distance: i32,
    pub fullscreen: bool,
    pub vsync: bool,
    pub master_volume: f32,
    pub music_volume: f32,
    pub sound_volume: f32,
    pub difficulty: Difficulty,
    pub language: Language,
    pub controls: ControlBindings,
}

impl Default for GameSettings {
    fn default() -> Self {
        Self {
            fov: 70.0,
            sensitivity: 0.002,
            render_distance: 8,
            fullscreen: false,
            vsync: true,
            master_volume: 1.0,
            music_volume: 0.7,
            sound_volume: 1.0,
            difficulty: Difficulty::Normal,
            language: Language::English,
            controls: ControlBindings::default(),
        }
    }
}

impl GameSettings {
    pub fn load() -> Self {
        let mut settings = Self::default();
        let Ok(contents) = fs::read_to_string(SETTINGS_FILE) else {
            return settings;
        };
        for line in contents.lines() {
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let value = value.trim();
            match key.trim() {
                "fov" => settings.fov = value.parse().unwrap_or(settings.fov),
                "sensitivity" => {
                    settings.sensitivity = value.parse().unwrap_or(settings.sensitivity)
                }
                "render_distance" => {
                    settings.render_distance = value.parse().unwrap_or(settings.render_distance)
                }
                "fullscreen" => settings.fullscreen = parse_bool(value, settings.fullscreen),
                "vsync" => settings.vsync = parse_bool(value, settings.vsync),
                "volume" | "master_volume" => {
                    settings.master_volume = value.parse().unwrap_or(settings.master_volume)
                }
                "music_volume" => {
                    settings.music_volume = value.parse().unwrap_or(settings.music_volume)
                }
                "sound_volume" => {
                    settings.sound_volume = value.parse().unwrap_or(settings.sound_volume)
                }
                "difficulty" => settings.difficulty = Difficulty::parse(value),
                "language" => settings.language = Language::parse(value),
                "key_forward" => set_key(&mut settings.controls.forward, value),
                "key_backward" => set_key(&mut settings.controls.backward, value),
                "key_left" => set_key(&mut settings.controls.left, value),
                "key_right" => set_key(&mut settings.controls.right, value),
                "key_jump" => set_key(&mut settings.controls.jump, value),
                "key_sprint" => set_key(&mut settings.controls.sprint, value),
                "key_sneak" => set_key(&mut settings.controls.sneak, value),
                "key_inventory" => set_key(&mut settings.controls.inventory, value),
                _ => {}
            }
        }
        settings.fov = settings.fov.clamp(30.0, 120.0);
        settings.sensitivity = settings.sensitivity.clamp(0.0002, 0.006);
        settings.render_distance = settings.render_distance.clamp(2, 16);
        settings.master_volume = settings.master_volume.clamp(0.0, 1.0);
        settings.music_volume = settings.music_volume.clamp(0.0, 1.0);
        settings.sound_volume = settings.sound_volume.clamp(0.0, 1.0);
        settings
    }

    pub fn save(&self) {
        let contents = format!(
            concat!(
                "fov:{}\n",
                "sensitivity:{}\n",
                "render_distance:{}\n",
                "fullscreen:{}\n",
                "vsync:{}\n",
                "master_volume:{}\n",
                "music_volume:{}\n",
                "sound_volume:{}\n",
                "difficulty:{}\n",
                "language:{}\n",
                "key_forward:{}\n",
                "key_backward:{}\n",
                "key_left:{}\n",
                "key_right:{}\n",
                "key_jump:{}\n",
                "key_sprint:{}\n",
                "key_sneak:{}\n",
                "key_inventory:{}\n"
            ),
            self.fov,
            self.sensitivity,
            self.render_distance,
            self.fullscreen,
            self.vsync,
            self.master_volume,
            self.music_volume,
            self.sound_volume,
            self.difficulty.as_str(),
            self.language.as_str(),
            key_name(self.controls.forward),
            key_name(self.controls.backward),
            key_name(self.controls.left),
            key_name(self.controls.right),
            key_name(self.controls.jump),
            key_name(self.controls.sprint),
            key_name(self.controls.sneak),
            key_name(self.controls.inventory),
        );
        if let Err(error) = fs::write(SETTINGS_FILE, contents) {
            eprintln!("[Settings] Could not save settings: {error}");
        }
    }

    pub fn effective_sound_volume(&self) -> f32 {
        self.master_volume * self.sound_volume
    }
}

fn parse_bool(value: &str, fallback: bool) -> bool {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "on" => true,
        "false" | "0" | "off" => false,
        _ => fallback,
    }
}

fn set_key(target: &mut KeyCode, value: &str) {
    if let Some(code) = parse_key(value) {
        *target = code;
    }
}

fn key_name(code: KeyCode) -> &'static str {
    match code {
        KeyCode::KeyA => "A",
        KeyCode::KeyB => "B",
        KeyCode::KeyC => "C",
        KeyCode::KeyD => "D",
        KeyCode::KeyE => "E",
        KeyCode::KeyF => "F",
        KeyCode::KeyG => "G",
        KeyCode::KeyH => "H",
        KeyCode::KeyI => "I",
        KeyCode::KeyJ => "J",
        KeyCode::KeyK => "K",
        KeyCode::KeyL => "L",
        KeyCode::KeyM => "M",
        KeyCode::KeyN => "N",
        KeyCode::KeyO => "O",
        KeyCode::KeyP => "P",
        KeyCode::KeyQ => "Q",
        KeyCode::KeyR => "R",
        KeyCode::KeyS => "S",
        KeyCode::KeyT => "T",
        KeyCode::KeyU => "U",
        KeyCode::KeyV => "V",
        KeyCode::KeyW => "W",
        KeyCode::KeyX => "X",
        KeyCode::KeyY => "Y",
        KeyCode::KeyZ => "Z",
        KeyCode::Space => "SPACE",
        KeyCode::ControlLeft => "LCTRL",
        KeyCode::ControlRight => "RCTRL",
        KeyCode::ShiftLeft => "LSHIFT",
        KeyCode::ShiftRight => "RSHIFT",
        KeyCode::ArrowUp => "UP",
        KeyCode::ArrowDown => "DOWN",
        KeyCode::ArrowLeft => "LEFT",
        KeyCode::ArrowRight => "RIGHT",
        _ => "KEY",
    }
}

fn parse_key(value: &str) -> Option<KeyCode> {
    let value = value.trim().to_ascii_uppercase();
    if value.len() == 1 {
        let ch = value.as_bytes()[0];
        if ch.is_ascii_alphabetic() {
            return Some(match ch {
                b'A' => KeyCode::KeyA,
                b'B' => KeyCode::KeyB,
                b'C' => KeyCode::KeyC,
                b'D' => KeyCode::KeyD,
                b'E' => KeyCode::KeyE,
                b'F' => KeyCode::KeyF,
                b'G' => KeyCode::KeyG,
                b'H' => KeyCode::KeyH,
                b'I' => KeyCode::KeyI,
                b'J' => KeyCode::KeyJ,
                b'K' => KeyCode::KeyK,
                b'L' => KeyCode::KeyL,
                b'M' => KeyCode::KeyM,
                b'N' => KeyCode::KeyN,
                b'O' => KeyCode::KeyO,
                b'P' => KeyCode::KeyP,
                b'Q' => KeyCode::KeyQ,
                b'R' => KeyCode::KeyR,
                b'S' => KeyCode::KeyS,
                b'T' => KeyCode::KeyT,
                b'U' => KeyCode::KeyU,
                b'V' => KeyCode::KeyV,
                b'W' => KeyCode::KeyW,
                b'X' => KeyCode::KeyX,
                b'Y' => KeyCode::KeyY,
                b'Z' => KeyCode::KeyZ,
                _ => return None,
            });
        }
    }
    match value.as_str() {
        "SPACE" => Some(KeyCode::Space),
        "LCTRL" => Some(KeyCode::ControlLeft),
        "RCTRL" => Some(KeyCode::ControlRight),
        "LSHIFT" => Some(KeyCode::ShiftLeft),
        "RSHIFT" => Some(KeyCode::ShiftRight),
        "UP" => Some(KeyCode::ArrowUp),
        "DOWN" => Some(KeyCode::ArrowDown),
        "LEFT" => Some(KeyCode::ArrowLeft),
        "RIGHT" => Some(KeyCode::ArrowRight),
        _ => None,
    }
}

#[derive(Debug, Clone)]
pub enum MultiplayerRole {
    Singleplayer,
    Host {
        port: u16,
    },
    Client {
        server_addr: String,
        port: u16,
        username: String,
    },
}

#[derive(Debug, Clone)]
pub struct WorldLaunch {
    pub world_dir: PathBuf,
    pub seed: u32,
    pub game_mode: GameMode,
    pub difficulty: Difficulty,
    pub role: MultiplayerRole,
}

#[derive(Debug, Clone)]
struct WorldMetadata {
    name: String,
    seed: u32,
    game_mode: GameMode,
    difficulty: Difficulty,
    last_played: u64,
}

impl WorldMetadata {
    fn load(world_dir: &Path) -> Option<Self> {
        let contents = fs::read_to_string(world_dir.join(META_FILE)).ok()?;
        let mut name = None;
        let mut seed = 12345;
        let mut game_mode = GameMode::Survival;
        let mut difficulty = Difficulty::Normal;
        let mut last_played = 0;
        for line in contents.lines() {
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            match key.trim() {
                "name" => name = Some(value.trim().to_string()),
                "seed" => seed = value.trim().parse().unwrap_or(seed),
                "game_mode" => game_mode = parse_game_mode(value),
                "difficulty" => difficulty = Difficulty::parse(value),
                "last_played" => last_played = value.trim().parse().unwrap_or(0),
                _ => {}
            }
        }
        Some(Self {
            name: name?,
            seed,
            game_mode,
            difficulty,
            last_played,
        })
    }

    fn save(&self, world_dir: &Path) -> std::io::Result<()> {
        fs::create_dir_all(world_dir.join("regions"))?;
        fs::write(
            world_dir.join(META_FILE),
            format!(
                "name:{}\nseed:{}\ngame_mode:{}\ndifficulty:{}\nlast_played:{}\n",
                self.name,
                self.seed,
                game_mode_name(self.game_mode),
                self.difficulty.as_str(),
                self.last_played
            ),
        )
    }
}

#[derive(Debug, Clone)]
struct WorldEntry {
    directory: PathBuf,
    metadata: WorldMetadata,
}

fn game_mode_name(mode: GameMode) -> &'static str {
    match mode {
        GameMode::Survival => "SURVIVAL",
        GameMode::Creative => "CREATIVE",
    }
}

fn parse_game_mode(value: &str) -> GameMode {
    if value.trim().eq_ignore_ascii_case("creative") {
        GameMode::Creative
    } else {
        GameMode::Survival
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn discover_worlds() -> Vec<WorldEntry> {
    let mut worlds = Vec::new();
    let Ok(entries) = fs::read_dir(SAVES_DIR) else {
        return worlds;
    };
    for entry in entries.flatten() {
        let directory = entry.path();
        if !directory.is_dir() {
            continue;
        }
        let metadata = WorldMetadata::load(&directory).or_else(|| legacy_metadata(&directory));
        if let Some(metadata) = metadata {
            worlds.push(WorldEntry {
                directory,
                metadata,
            });
        }
    }
    worlds.sort_by_key(|world| std::cmp::Reverse(world.metadata.last_played));
    worlds
}

pub fn update_world_metadata(
    world_dir: &Path,
    seed: u32,
    game_mode: GameMode,
    difficulty: Difficulty,
) {
    let mut metadata = WorldMetadata::load(world_dir)
        .or_else(|| legacy_metadata(world_dir))
        .unwrap_or_else(|| WorldMetadata {
            name: world_dir
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("WORLD")
                .replace('_', " ")
                .to_ascii_uppercase(),
            seed,
            game_mode,
            difficulty,
            last_played: 0,
        });
    metadata.seed = seed;
    metadata.game_mode = game_mode;
    metadata.difficulty = difficulty;
    metadata.last_played = unix_now();
    let _ = metadata.save(world_dir);
}

fn legacy_metadata(directory: &Path) -> Option<WorldMetadata> {
    if !directory.join("level.dat").is_file() || !directory.join("player.dat").is_file() {
        return None;
    }
    let manager = crate::save::SaveManager::new(directory);
    let (level, player) = manager.load_player_and_level().ok()?;
    let modified = fs::metadata(directory.join("player.dat"))
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let name = directory
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("WORLD")
        .replace('_', " ")
        .to_ascii_uppercase();
    Some(WorldMetadata {
        name,
        seed: level.seed,
        game_mode: player.game_mode,
        difficulty: Difficulty::Normal,
        last_played: modified,
    })
}

fn sanitize_name(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, ' ' | '-' | '_'))
        .take(24)
        .collect::<String>()
        .trim()
        .to_string()
}

fn slugify(value: &str) -> String {
    let mut slug = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
        } else if !slug.ends_with('_') {
            slug.push('_');
        }
    }
    let slug = slug.trim_matches('_');
    if slug.is_empty() {
        "new_world".to_string()
    } else {
        slug.to_string()
    }
}

fn unique_world_dir(name: &str) -> PathBuf {
    let base = slugify(name);
    let mut candidate = Path::new(SAVES_DIR).join(&base);
    let mut suffix = 2;
    while candidate.exists() {
        candidate = Path::new(SAVES_DIR).join(format!("{base}_{suffix}"));
        suffix += 1;
    }
    candidate
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct PanoramaUniform {
    time: f32,
    width: f32,
    height: f32,
    _padding: f32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct UiVertex {
    position: [f32; 2],
    color: [f32; 4],
}

impl UiVertex {
    fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x2,
                },
                wgpu::VertexAttribute {
                    offset: 8,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x4,
                },
            ],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuScreen {
    Main,
    Multiplayer,
    Worlds,
    CreateWorld,
    Options,
    Controls,
    ConfirmDelete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TextField {
    WorldName,
    Seed,
    HostPort,
    ServerAddress,
    JoinPort,
    Username,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MultiplayerMode {
    Host,
    Join,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ControlAction {
    Forward,
    Backward,
    Left,
    Right,
    Jump,
    Sprint,
    Sneak,
    Inventory,
}

impl ControlAction {
    fn label(self) -> &'static str {
        match self {
            Self::Forward => "FORWARD",
            Self::Backward => "BACKWARD",
            Self::Left => "LEFT",
            Self::Right => "RIGHT",
            Self::Jump => "JUMP",
            Self::Sprint => "SPRINT",
            Self::Sneak => "SNEAK",
            Self::Inventory => "INVENTORY",
        }
    }
}

pub enum MenuAction {
    None,
    Launch(WorldLaunch, GameSettings),
    Quit,
}

pub struct Menu {
    pub window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    size: winit::dpi::PhysicalSize<u32>,
    panorama_pipeline: wgpu::RenderPipeline,
    panorama_buffer: wgpu::Buffer,
    panorama_bind_group: wgpu::BindGroup,
    ui_pipeline: wgpu::RenderPipeline,
    ui_buffer: wgpu::Buffer,
    elapsed: f32,
    mouse_ndc: [f32; 2],
    screen: MenuScreen,
    worlds: Vec<WorldEntry>,
    selected_world: Option<usize>,
    world_scroll: usize,
    create_name: String,
    create_seed: String,
    create_mode: GameMode,
    create_difficulty: Difficulty,
    multiplayer_mode: MultiplayerMode,
    selected_role: MultiplayerRole,
    host_port: String,
    server_address: String,
    join_port: String,
    username: String,
    active_field: Option<TextField>,
    rebinding: Option<ControlAction>,
    message: Option<String>,
    pub settings: GameSettings,
    immediate_present_supported: bool,
}

impl Menu {
    pub async fn new(window: Arc<Window>, settings: GameSettings) -> Self {
        window.set_cursor_visible(true);
        let _ = window.set_cursor_grab(winit::window::CursorGrabMode::None);
        apply_fullscreen(&window, settings.fullscreen);
        let size = window.inner_size();
        // On this Windows/NVIDIA setup the Vulkan ICD crashes while the game
        // surface is created. `PRIMARY` still prefers Vulkan, so explicitly
        // select DX12 on Windows and use the normal primary backends elsewhere.
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
            .expect("No compatible graphics adapter found");
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default(), None)
            .await
            .expect("Could not create graphics device");
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|format| format.is_srgb())
            .unwrap_or(caps.formats[0]);
        let immediate_present_supported =
            caps.present_modes.contains(&wgpu::PresentMode::Immediate);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: present_mode(settings.vsync, &caps.present_modes),
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let panorama_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Menu Panorama Shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(PANORAMA_SHADER)),
        });
        let panorama_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Menu Panorama Uniform"),
            contents: bytemuck::bytes_of(&PanoramaUniform {
                time: 0.0,
                width: size.width as f32,
                height: size.height as f32,
                _padding: 0.0,
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let panorama_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Menu Panorama Layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let panorama_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Menu Panorama Bind Group"),
            layout: &panorama_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: panorama_buffer.as_entire_binding(),
            }],
        });
        let panorama_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Menu Panorama Pipeline Layout"),
                bind_group_layouts: &[&panorama_layout],
                push_constant_ranges: &[],
            });
        let panorama_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Menu Panorama Pipeline"),
            layout: Some(&panorama_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &panorama_shader,
                entry_point: "vs_main",
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &panorama_shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        let ui_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Menu UI Shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(UI_SHADER)),
        });
        let ui_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Menu UI Pipeline Layout"),
            bind_group_layouts: &[],
            push_constant_ranges: &[],
        });
        let ui_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Menu UI Pipeline"),
            layout: Some(&ui_layout),
            vertex: wgpu::VertexState {
                module: &ui_shader,
                entry_point: "vs_main",
                buffers: &[UiVertex::desc()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &ui_shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });
        let ui_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Menu UI Vertex Buffer"),
            size: (UI_VERTEX_CAPACITY * std::mem::size_of::<UiVertex>()) as u64,
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
            panorama_pipeline,
            panorama_buffer,
            panorama_bind_group,
            ui_pipeline,
            ui_buffer,
            elapsed: 0.0,
            mouse_ndc: [0.0, 0.0],
            screen: MenuScreen::Main,
            worlds: discover_worlds(),
            selected_world: None,
            world_scroll: 0,
            create_name: "NEW WORLD".to_string(),
            create_seed: String::new(),
            create_mode: GameMode::Survival,
            create_difficulty: settings.difficulty,
            multiplayer_mode: MultiplayerMode::Host,
            selected_role: MultiplayerRole::Singleplayer,
            host_port: "25565".to_string(),
            server_address: "127.0.0.1".to_string(),
            join_port: "25565".to_string(),
            username: "PLAYER".to_string(),
            active_field: None,
            rebinding: None,
            message: None,
            settings,
            immediate_present_supported,
        }
    }

    pub fn resize(&mut self, size: winit::dpi::PhysicalSize<u32>) {
        if size.width == 0 || size.height == 0 {
            return;
        }
        self.size = size;
        self.config.width = size.width;
        self.config.height = size.height;
        self.surface.configure(&self.device, &self.config);
    }

    pub fn handle_mouse_move(&mut self, x: f64, y: f64) {
        self.mouse_ndc = [
            x as f32 / self.size.width.max(1) as f32 * 2.0 - 1.0,
            1.0 - y as f32 / self.size.height.max(1) as f32 * 2.0,
        ];
    }

    pub fn handle_scroll(&mut self, direction: i32) {
        if self.screen != MenuScreen::Worlds || self.worlds.len() <= 5 {
            return;
        }
        let max_scroll = self.worlds.len() - 5;
        self.world_scroll =
            (self.world_scroll as i32 + direction).clamp(0, max_scroll as i32) as usize;
    }

    pub fn handle_key(
        &mut self,
        state: ElementState,
        physical_key: PhysicalKey,
        logical_key: &Key,
        repeat: bool,
    ) -> MenuAction {
        if state != ElementState::Pressed {
            return MenuAction::None;
        }
        if let Some(action) = self.rebinding.take() {
            if let PhysicalKey::Code(code) = physical_key {
                if code != KeyCode::Escape {
                    *self.control_mut(action) = code;
                    self.settings.save();
                }
            }
            return MenuAction::None;
        }
        if let Some(field) = self.active_field {
            match logical_key {
                Key::Named(NamedKey::Escape) => self.active_field = None,
                Key::Named(NamedKey::Backspace) => match field {
                    TextField::WorldName => {
                        self.create_name.pop();
                    }
                    TextField::Seed => {
                        self.create_seed.pop();
                    }
                    TextField::HostPort => {
                        self.host_port.pop();
                    }
                    TextField::ServerAddress => {
                        self.server_address.pop();
                    }
                    TextField::JoinPort => {
                        self.join_port.pop();
                    }
                    TextField::Username => {
                        self.username.pop();
                    }
                },
                Key::Named(NamedKey::Enter) => self.active_field = None,
                Key::Character(text) if !repeat => {
                    for ch in text.chars() {
                        match field {
                            TextField::WorldName
                                if self.create_name.len() < 24
                                    && (ch.is_ascii_alphanumeric()
                                        || matches!(ch, ' ' | '-' | '_')) =>
                            {
                                self.create_name.push(ch.to_ascii_uppercase())
                            }
                            TextField::Seed
                                if self.create_seed.len() < 10
                                    && (ch.is_ascii_digit()
                                        || (ch == '-' && self.create_seed.is_empty())) =>
                            {
                                self.create_seed.push(ch)
                            }
                            TextField::HostPort
                                if self.host_port.len() < 5 && ch.is_ascii_digit() =>
                            {
                                self.host_port.push(ch)
                            }
                            TextField::ServerAddress
                                if self.server_address.len() < 64
                                    && (ch.is_ascii_alphanumeric()
                                        || matches!(ch, '.' | '-' | ':' | '_')) =>
                            {
                                self.server_address.push(ch)
                            }
                            TextField::JoinPort
                                if self.join_port.len() < 5 && ch.is_ascii_digit() =>
                            {
                                self.join_port.push(ch)
                            }
                            TextField::Username
                                if self.username.len() < 16
                                    && (ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_')) =>
                            {
                                self.username.push(ch.to_ascii_uppercase())
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
            return MenuAction::None;
        }
        if matches!(logical_key, Key::Named(NamedKey::Escape)) {
            self.back();
        }
        MenuAction::None
    }

    pub fn handle_click(&mut self) -> MenuAction {
        let [x, y] = self.mouse_ndc;
        self.message = None;
        match self.screen {
            MenuScreen::Main => {
                if hit(x, y, -0.34, 0.34, 0.21, 0.34) {
                    self.selected_role = MultiplayerRole::Singleplayer;
                    self.worlds = discover_worlds();
                    self.selected_world = (!self.worlds.is_empty()).then_some(0);
                    self.world_scroll = 0;
                    self.screen = MenuScreen::Worlds;
                } else if hit(x, y, -0.34, 0.34, 0.03, 0.16) {
                    self.active_field = None;
                    self.screen = MenuScreen::Multiplayer;
                } else if hit(x, y, -0.34, 0.34, -0.15, -0.02) {
                    self.screen = MenuScreen::Options;
                } else if hit(x, y, -0.34, 0.34, -0.33, -0.20) {
                    return MenuAction::Quit;
                }
            }
            MenuScreen::Multiplayer => {
                if hit(x, y, -0.52, -0.02, 0.45, 0.58) {
                    self.multiplayer_mode = MultiplayerMode::Host;
                    self.active_field = None;
                } else if hit(x, y, 0.02, 0.52, 0.45, 0.58) {
                    self.multiplayer_mode = MultiplayerMode::Join;
                    self.active_field = None;
                } else if self.multiplayer_mode == MultiplayerMode::Host
                    && hit(x, y, -0.52, 0.52, 0.17, 0.30)
                {
                    self.active_field = Some(TextField::HostPort);
                } else if self.multiplayer_mode == MultiplayerMode::Join
                    && hit(x, y, -0.52, 0.52, 0.17, 0.30)
                {
                    self.active_field = Some(TextField::ServerAddress);
                } else if self.multiplayer_mode == MultiplayerMode::Join
                    && hit(x, y, -0.52, 0.52, -0.04, 0.09)
                {
                    self.active_field = Some(TextField::JoinPort);
                } else if self.multiplayer_mode == MultiplayerMode::Join
                    && hit(x, y, -0.52, 0.52, -0.25, -0.12)
                {
                    self.active_field = Some(TextField::Username);
                } else if hit(x, y, -0.52, -0.02, -0.58, -0.45) {
                    let role = match self.multiplayer_mode {
                        MultiplayerMode::Host => self
                            .host_port
                            .parse::<u16>()
                            .ok()
                            .filter(|p| *p > 0)
                            .map(|port| MultiplayerRole::Host { port }),
                        MultiplayerMode::Join => {
                            let address = self.server_address.trim();
                            let username = self.username.trim();
                            self.join_port
                                .parse::<u16>()
                                .ok()
                                .filter(|port| {
                                    *port > 0 && !address.is_empty() && !username.is_empty()
                                })
                                .map(|port| MultiplayerRole::Client {
                                    server_addr: address.to_string(),
                                    port,
                                    username: username.to_string(),
                                })
                        }
                    };
                    let Some(role) = role else {
                        self.message = Some("ENTER VALID MULTIPLAYER SETTINGS".to_string());
                        return MenuAction::None;
                    };
                    self.selected_role = role;
                    self.worlds = discover_worlds();
                    self.selected_world = (!self.worlds.is_empty()).then_some(0);
                    self.world_scroll = 0;
                    self.active_field = None;
                    self.screen = MenuScreen::Worlds;
                } else if hit(x, y, 0.02, 0.52, -0.58, -0.45) {
                    self.active_field = None;
                    self.screen = MenuScreen::Main;
                }
            }
            MenuScreen::Worlds => {
                for visible_index in 0..(self.worlds.len() - self.world_scroll).min(5) {
                    let index = self.world_scroll + visible_index;
                    let top = 0.58 - visible_index as f32 * 0.19;
                    if hit(x, y, -0.72, 0.72, top - 0.15, top) {
                        self.selected_world = Some(index);
                        return MenuAction::None;
                    }
                }
                if hit(x, y, -0.72, -0.27, -0.64, -0.51) {
                    if let Some(index) = self.selected_world {
                        return self.launch_existing(index);
                    }
                } else if hit(x, y, -0.23, 0.23, -0.64, -0.51) {
                    self.create_name = "NEW WORLD".to_string();
                    self.create_seed.clear();
                    self.create_mode = GameMode::Survival;
                    self.create_difficulty = self.settings.difficulty;
                    self.screen = MenuScreen::CreateWorld;
                } else if hit(x, y, 0.27, 0.72, -0.64, -0.51) {
                    if self.selected_world.is_some() {
                        self.screen = MenuScreen::ConfirmDelete;
                    }
                } else if hit(x, y, -0.2, 0.2, -0.84, -0.72) {
                    self.screen = MenuScreen::Main;
                }
            }
            MenuScreen::CreateWorld => {
                if hit(x, y, -0.52, 0.52, 0.34, 0.47) {
                    self.active_field = Some(TextField::WorldName);
                } else if hit(x, y, -0.52, 0.52, 0.13, 0.26) {
                    self.active_field = Some(TextField::Seed);
                } else if hit(x, y, -0.52, 0.52, -0.08, 0.05) {
                    self.create_mode = match self.create_mode {
                        GameMode::Survival => GameMode::Creative,
                        GameMode::Creative => GameMode::Survival,
                    };
                } else if hit(x, y, -0.52, 0.52, -0.29, -0.16) {
                    self.create_difficulty =
                        self.create_difficulty.step(if x < 0.0 { -1 } else { 1 });
                } else if hit(x, y, -0.52, -0.02, -0.58, -0.45) {
                    return self.create_world();
                } else if hit(x, y, 0.02, 0.52, -0.58, -0.45) {
                    self.active_field = None;
                    self.screen = MenuScreen::Worlds;
                }
            }
            MenuScreen::Options => self.handle_options_click(x, y),
            MenuScreen::Controls => {
                if hit(x, y, -0.48, 0.48, 0.49, 0.62) {
                    let delta = if x < 0.0 { -0.0002 } else { 0.0002 };
                    self.settings.sensitivity =
                        (self.settings.sensitivity + delta).clamp(0.0002, 0.006);
                    self.settings.save();
                    return MenuAction::None;
                }
                let actions = [
                    ControlAction::Forward,
                    ControlAction::Backward,
                    ControlAction::Left,
                    ControlAction::Right,
                    ControlAction::Jump,
                    ControlAction::Sprint,
                    ControlAction::Sneak,
                    ControlAction::Inventory,
                ];
                for (index, action) in actions.into_iter().enumerate() {
                    let column = index / 4;
                    let row = index % 4;
                    let (x0, x1) = if column == 0 {
                        (-0.78, -0.04)
                    } else {
                        (0.04, 0.78)
                    };
                    let top = 0.38 - row as f32 * 0.19;
                    if hit(x, y, x0, x1, top - 0.14, top) {
                        self.rebinding = Some(action);
                    }
                }
                if hit(x, y, -0.25, 0.25, -0.78, -0.64) {
                    self.screen = MenuScreen::Options;
                }
            }
            MenuScreen::ConfirmDelete => {
                if hit(x, y, -0.48, -0.02, -0.16, -0.02) {
                    if let Some(index) = self.selected_world {
                        if let Some(world) = self.worlds.get(index) {
                            if let Err(error) = fs::remove_dir_all(&world.directory) {
                                self.message = Some(format!("DELETE FAILED: {error}"));
                                self.screen = MenuScreen::Worlds;
                                return MenuAction::None;
                            }
                        }
                    }
                    self.worlds = discover_worlds();
                    self.selected_world = (!self.worlds.is_empty()).then_some(0);
                    self.world_scroll = self.world_scroll.min(self.worlds.len().saturating_sub(5));
                    self.screen = MenuScreen::Worlds;
                } else if hit(x, y, 0.02, 0.48, -0.16, -0.02) {
                    self.screen = MenuScreen::Worlds;
                }
            }
        }
        MenuAction::None
    }

    fn back(&mut self) {
        self.active_field = None;
        self.rebinding = None;
        self.screen = match self.screen {
            MenuScreen::Main => MenuScreen::Main,
            MenuScreen::Multiplayer | MenuScreen::Worlds | MenuScreen::Options => MenuScreen::Main,
            MenuScreen::CreateWorld | MenuScreen::ConfirmDelete => MenuScreen::Worlds,
            MenuScreen::Controls => MenuScreen::Options,
        };
    }

    fn launch_existing(&mut self, index: usize) -> MenuAction {
        let Some(world) = self.worlds.get_mut(index) else {
            return MenuAction::None;
        };
        world.metadata.last_played = unix_now();
        let _ = world.metadata.save(&world.directory);
        MenuAction::Launch(
            WorldLaunch {
                world_dir: world.directory.clone(),
                seed: world.metadata.seed,
                game_mode: world.metadata.game_mode,
                difficulty: world.metadata.difficulty,
                role: self.selected_role.clone(),
            },
            self.settings.clone(),
        )
    }

    fn create_world(&mut self) -> MenuAction {
        let name = sanitize_name(&self.create_name);
        if name.is_empty() {
            self.message = Some("ENTER A WORLD NAME".to_string());
            return MenuAction::None;
        }
        let seed = if self.create_seed.trim().is_empty() {
            (unix_now() as u32)
                .wrapping_mul(747_796_405)
                .wrapping_add(2_891_336_453)
        } else {
            self.create_seed
                .trim()
                .parse::<i64>()
                .map(|seed| seed as u32)
                .unwrap_or_else(|_| hash_seed(&self.create_seed))
        };
        let world_dir = unique_world_dir(&name);
        let metadata = WorldMetadata {
            name,
            seed,
            game_mode: self.create_mode,
            difficulty: self.create_difficulty,
            last_played: unix_now(),
        };
        if let Err(error) = metadata.save(&world_dir) {
            self.message = Some(format!("CREATE FAILED: {error}"));
            return MenuAction::None;
        }
        MenuAction::Launch(
            WorldLaunch {
                world_dir,
                seed,
                game_mode: self.create_mode,
                difficulty: self.create_difficulty,
                role: self.selected_role.clone(),
            },
            self.settings.clone(),
        )
    }

    fn handle_options_click(&mut self, x: f32, y: f32) {
        let left = x >= -0.82 && x <= -0.05;
        let right = x >= 0.05 && x <= 0.82;
        let row = [0.58, 0.38, 0.18, -0.02, -0.22]
            .iter()
            .position(|top| y <= *top && y >= *top - 0.13);
        let delta = if x < -0.43 || (x > 0.05 && x < 0.43) {
            -1.0
        } else {
            1.0
        };
        match (left, right, row) {
            (true, _, Some(0)) => {
                self.settings.fov = (self.settings.fov + delta * 5.0).clamp(30.0, 120.0)
            }
            (true, _, Some(1)) => {
                self.settings.render_distance =
                    (self.settings.render_distance + delta as i32).clamp(2, 16)
            }
            (true, _, Some(2)) => {
                self.settings.fullscreen = !self.settings.fullscreen;
                apply_fullscreen(&self.window, self.settings.fullscreen);
            }
            (true, _, Some(3)) => {
                if self.settings.vsync {
                    if !self.immediate_present_supported {
                        self.message = Some("VSYNC REQUIRED ON THIS DISPLAY".to_string());
                        return;
                    }
                    self.settings.vsync = false;
                    self.config.present_mode = wgpu::PresentMode::Immediate;
                } else {
                    self.settings.vsync = true;
                    self.config.present_mode = wgpu::PresentMode::Fifo;
                }
                self.surface.configure(&self.device, &self.config);
            }
            (true, _, Some(4)) => {
                self.settings.difficulty = self.settings.difficulty.step(delta as i32)
            }
            (_, true, Some(0)) => {
                self.settings.master_volume =
                    (self.settings.master_volume + delta * 0.1).clamp(0.0, 1.0)
            }
            (_, true, Some(1)) => {
                self.settings.music_volume =
                    (self.settings.music_volume + delta * 0.1).clamp(0.0, 1.0)
            }
            (_, true, Some(2)) => {
                self.settings.sound_volume =
                    (self.settings.sound_volume + delta * 0.1).clamp(0.0, 1.0)
            }
            (_, true, Some(3)) => self.settings.language = self.settings.language.toggle(),
            (_, true, Some(4)) => {
                self.screen = MenuScreen::Controls;
                return;
            }
            _ if hit(x, y, -0.25, 0.25, -0.78, -0.64) => {
                self.screen = MenuScreen::Main;
                return;
            }
            _ => return,
        }
        self.settings.save();
    }

    fn control_mut(&mut self, action: ControlAction) -> &mut KeyCode {
        match action {
            ControlAction::Forward => &mut self.settings.controls.forward,
            ControlAction::Backward => &mut self.settings.controls.backward,
            ControlAction::Left => &mut self.settings.controls.left,
            ControlAction::Right => &mut self.settings.controls.right,
            ControlAction::Jump => &mut self.settings.controls.jump,
            ControlAction::Sprint => &mut self.settings.controls.sprint,
            ControlAction::Sneak => &mut self.settings.controls.sneak,
            ControlAction::Inventory => &mut self.settings.controls.inventory,
        }
    }

    pub fn update(&mut self, dt: f32) {
        self.elapsed += dt.min(0.1);
    }

    pub fn render(&mut self) -> Result<(), wgpu::SurfaceError> {
        let output = self.surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let uniform = PanoramaUniform {
            time: self.elapsed,
            width: self.size.width as f32,
            height: self.size.height as f32,
            _padding: 0.0,
        };
        self.queue
            .write_buffer(&self.panorama_buffer, 0, bytemuck::bytes_of(&uniform));

        let mut vertices = Vec::with_capacity(8192);
        self.build_ui(&mut vertices);
        let count = vertices.len().min(UI_VERTEX_CAPACITY);
        self.queue
            .write_buffer(&self.ui_buffer, 0, bytemuck::cast_slice(&vertices[..count]));

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Menu Render Encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Menu Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.panorama_pipeline);
            pass.set_bind_group(0, &self.panorama_bind_group, &[]);
            pass.draw(0..3, 0..1);
            pass.set_pipeline(&self.ui_pipeline);
            pass.set_vertex_buffer(0, self.ui_buffer.slice(..));
            pass.draw(0..count as u32, 0..1);
        }
        self.queue.submit(Some(encoder.finish()));
        output.present();
        Ok(())
    }

    fn build_ui(&self, vertices: &mut Vec<UiVertex>) {
        let aspect = self.size.height.max(1) as f32 / self.size.width.max(1) as f32;
        let hovered = |x0, x1, y0, y1| hit(self.mouse_ndc[0], self.mouse_ndc[1], x0, x1, y0, y1);
        draw_rect(vertices, -1.0, 1.0, -1.0, 1.0, [0.02, 0.03, 0.04, 0.30]);
        match self.screen {
            MenuScreen::Main => {
                draw_logo(vertices, aspect);
                draw_button(
                    vertices,
                    -0.34,
                    0.34,
                    0.21,
                    0.34,
                    hovered(-0.34, 0.34, 0.21, 0.34),
                );
                draw_button(
                    vertices,
                    -0.34,
                    0.34,
                    0.03,
                    0.16,
                    hovered(-0.34, 0.34, 0.03, 0.16),
                );
                draw_button(
                    vertices,
                    -0.34,
                    0.34,
                    -0.15,
                    -0.02,
                    hovered(-0.34, 0.34, -0.15, -0.02),
                );
                draw_button(
                    vertices,
                    -0.34,
                    0.34,
                    -0.33,
                    -0.20,
                    hovered(-0.34, 0.34, -0.33, -0.20),
                );
                draw_centered_text(
                    vertices,
                    tr(self.settings.language, "SINGLEPLAYER"),
                    0.248,
                    0.010,
                    aspect,
                    [1.0; 4],
                );
                draw_centered_text(vertices, "MULTIPLAYER", 0.068, 0.010, aspect, [1.0; 4]);
                draw_centered_text(
                    vertices,
                    tr(self.settings.language, "OPTIONS"),
                    -0.112,
                    0.010,
                    aspect,
                    [1.0; 4],
                );
                draw_centered_text(
                    vertices,
                    tr(self.settings.language, "QUIT GAME"),
                    -0.292,
                    0.010,
                    aspect,
                    [1.0; 4],
                );
                draw_text(
                    vertices,
                    "JAVA-FREE EDITION",
                    -0.96,
                    -0.94,
                    0.006,
                    aspect,
                    [0.8, 0.84, 0.86, 1.0],
                );
            }
            MenuScreen::Multiplayer => self.draw_multiplayer(vertices, aspect),
            MenuScreen::Worlds => self.draw_worlds(vertices, aspect),
            MenuScreen::CreateWorld => self.draw_create(vertices, aspect),
            MenuScreen::Options => self.draw_options(vertices, aspect),
            MenuScreen::Controls => self.draw_controls(vertices, aspect),
            MenuScreen::ConfirmDelete => self.draw_delete_confirmation(vertices, aspect),
        }
        if let Some(message) = &self.message {
            draw_centered_text(
                vertices,
                message,
                -0.94,
                0.007,
                aspect,
                [1.0, 0.35, 0.25, 1.0],
            );
        }
    }

    fn draw_multiplayer(&self, vertices: &mut Vec<UiVertex>, aspect: f32) {
        panel(vertices, -0.64, 0.64, -0.72, 0.78);
        draw_centered_text(vertices, "MULTIPLAYER", 0.67, 0.012, aspect, [1.0; 4]);

        for (x0, x1, label, selected) in [
            (
                -0.52,
                -0.02,
                "HOST GAME",
                self.multiplayer_mode == MultiplayerMode::Host,
            ),
            (
                0.02,
                0.52,
                "JOIN GAME",
                self.multiplayer_mode == MultiplayerMode::Join,
            ),
        ] {
            let hover = hit(self.mouse_ndc[0], self.mouse_ndc[1], x0, x1, 0.45, 0.58);
            draw_button_state(vertices, x0, x1, 0.45, 0.58, hover, selected);
            draw_centered_text_in(vertices, label, x0, x1, 0.488, 0.007, aspect, [1.0; 4]);
        }

        match self.multiplayer_mode {
            MultiplayerMode::Host => draw_field(
                vertices,
                "PORT",
                &self.host_port,
                -0.52,
                0.52,
                0.17,
                0.30,
                self.active_field == Some(TextField::HostPort),
                aspect,
            ),
            MultiplayerMode::Join => {
                draw_field(
                    vertices,
                    "SERVER ADDRESS",
                    &self.server_address,
                    -0.52,
                    0.52,
                    0.17,
                    0.30,
                    self.active_field == Some(TextField::ServerAddress),
                    aspect,
                );
                draw_field(
                    vertices,
                    "PORT",
                    &self.join_port,
                    -0.52,
                    0.52,
                    -0.04,
                    0.09,
                    self.active_field == Some(TextField::JoinPort),
                    aspect,
                );
                draw_field(
                    vertices,
                    "USERNAME",
                    &self.username,
                    -0.52,
                    0.52,
                    -0.25,
                    -0.12,
                    self.active_field == Some(TextField::Username),
                    aspect,
                );
            }
        }

        for (x0, x1, label) in [(-0.52, -0.02, "SELECT WORLD"), (0.02, 0.52, "BACK")] {
            let hover = hit(self.mouse_ndc[0], self.mouse_ndc[1], x0, x1, -0.58, -0.45);
            draw_button(vertices, x0, x1, -0.58, -0.45, hover);
            draw_centered_text_in(vertices, label, x0, x1, -0.542, 0.007, aspect, [1.0; 4]);
        }
    }

    fn draw_worlds(&self, vertices: &mut Vec<UiVertex>, aspect: f32) {
        panel(vertices, -0.82, 0.82, -0.9, 0.82);
        draw_centered_text(
            vertices,
            tr(self.settings.language, "SELECT WORLD"),
            0.72,
            0.012,
            aspect,
            [1.0; 4],
        );
        if self.worlds.is_empty() {
            draw_centered_text(
                vertices,
                "NO WORLDS YET",
                0.14,
                0.010,
                aspect,
                [0.8, 0.8, 0.8, 1.0],
            );
        }
        for (visible_index, world) in self
            .worlds
            .iter()
            .skip(self.world_scroll)
            .take(5)
            .enumerate()
        {
            let index = self.world_scroll + visible_index;
            let top = 0.58 - visible_index as f32 * 0.19;
            let selected = self.selected_world == Some(index);
            let hover = hit(
                self.mouse_ndc[0],
                self.mouse_ndc[1],
                -0.72,
                0.72,
                top - 0.15,
                top,
            );
            draw_button_state(vertices, -0.72, 0.72, top - 0.15, top, hover, selected);
            draw_text(
                vertices,
                &world.metadata.name,
                -0.68,
                top - 0.055,
                0.008,
                aspect,
                [1.0; 4],
            );
            let detail = format!(
                "{} / {} / {}",
                relative_time(world.metadata.last_played),
                game_mode_name(world.metadata.game_mode),
                world.metadata.difficulty.as_str()
            );
            draw_text(
                vertices,
                &detail,
                -0.68,
                top - 0.125,
                0.0055,
                aspect,
                [0.72, 0.76, 0.78, 1.0],
            );
        }
        if self.worlds.len() > 5 {
            draw_text(
                vertices,
                "SCROLL FOR MORE WORLDS",
                0.38,
                -0.44,
                0.0048,
                aspect,
                [0.72, 0.76, 0.78, 1.0],
            );
        }
        for (x0, x1, label) in [
            (-0.72, -0.27, "PLAY SELECTED"),
            (-0.23, 0.23, "CREATE NEW WORLD"),
            (0.27, 0.72, "DELETE"),
        ] {
            draw_button(
                vertices,
                x0,
                x1,
                -0.64,
                -0.51,
                hit(self.mouse_ndc[0], self.mouse_ndc[1], x0, x1, -0.64, -0.51),
            );
            draw_centered_text_in(vertices, label, x0, x1, -0.602, 0.006, aspect, [1.0; 4]);
        }
        draw_button(
            vertices,
            -0.2,
            0.2,
            -0.84,
            -0.72,
            hit(
                self.mouse_ndc[0],
                self.mouse_ndc[1],
                -0.2,
                0.2,
                -0.84,
                -0.72,
            ),
        );
        draw_centered_text(vertices, "BACK", -0.805, 0.008, aspect, [1.0; 4]);
    }

    fn draw_create(&self, vertices: &mut Vec<UiVertex>, aspect: f32) {
        panel(vertices, -0.64, 0.64, -0.72, 0.78);
        draw_centered_text(vertices, "CREATE NEW WORLD", 0.67, 0.012, aspect, [1.0; 4]);
        draw_field(
            vertices,
            "WORLD NAME",
            &self.create_name,
            -0.52,
            0.52,
            0.34,
            0.47,
            self.active_field == Some(TextField::WorldName),
            aspect,
        );
        let seed = if self.create_seed.is_empty() {
            "RANDOM"
        } else {
            &self.create_seed
        };
        draw_field(
            vertices,
            "SEED",
            seed,
            -0.52,
            0.52,
            0.13,
            0.26,
            self.active_field == Some(TextField::Seed),
            aspect,
        );
        draw_button(
            vertices,
            -0.52,
            0.52,
            -0.08,
            0.05,
            hit(
                self.mouse_ndc[0],
                self.mouse_ndc[1],
                -0.52,
                0.52,
                -0.08,
                0.05,
            ),
        );
        draw_centered_text(
            vertices,
            &format!("GAME MODE: < {} >", game_mode_name(self.create_mode)),
            -0.042,
            0.007,
            aspect,
            [1.0; 4],
        );
        draw_button(
            vertices,
            -0.52,
            0.52,
            -0.29,
            -0.16,
            hit(
                self.mouse_ndc[0],
                self.mouse_ndc[1],
                -0.52,
                0.52,
                -0.29,
                -0.16,
            ),
        );
        draw_centered_text(
            vertices,
            &format!("DIFFICULTY: < {} >", self.create_difficulty.as_str()),
            -0.252,
            0.007,
            aspect,
            [1.0; 4],
        );
        draw_button(
            vertices,
            -0.52,
            -0.02,
            -0.58,
            -0.45,
            hit(
                self.mouse_ndc[0],
                self.mouse_ndc[1],
                -0.52,
                -0.02,
                -0.58,
                -0.45,
            ),
        );
        draw_button(
            vertices,
            0.02,
            0.52,
            -0.58,
            -0.45,
            hit(
                self.mouse_ndc[0],
                self.mouse_ndc[1],
                0.02,
                0.52,
                -0.58,
                -0.45,
            ),
        );
        draw_centered_text_in(
            vertices,
            "CREATE WORLD",
            -0.52,
            -0.02,
            -0.542,
            0.007,
            aspect,
            [1.0; 4],
        );
        draw_centered_text_in(
            vertices, "CANCEL", 0.02, 0.52, -0.542, 0.007, aspect, [1.0; 4],
        );
    }

    fn draw_options(&self, vertices: &mut Vec<UiVertex>, aspect: f32) {
        panel(vertices, -0.9, 0.9, -0.88, 0.82);
        draw_centered_text(
            vertices,
            tr(self.settings.language, "OPTIONS"),
            0.72,
            0.012,
            aspect,
            [1.0; 4],
        );
        let left = [
            format!("FOV: < {:.0} >", self.settings.fov),
            format!("RENDER DISTANCE: < {} >", self.settings.render_distance),
            format!("FULLSCREEN: < {} >", on_off(self.settings.fullscreen)),
            format!("VSYNC: < {} >", on_off(self.settings.vsync)),
            format!("DIFFICULTY: < {} >", self.settings.difficulty.as_str()),
        ];
        let right = [
            format!(
                "MASTER VOLUME: < {}% >",
                percent(self.settings.master_volume)
            ),
            format!("MUSIC VOLUME: < {}% >", percent(self.settings.music_volume)),
            format!("SOUND VOLUME: < {}% >", percent(self.settings.sound_volume)),
            format!("LANGUAGE: < {} >", self.settings.language.as_str()),
            "CONTROLS...".to_string(),
        ];
        for row in 0..5 {
            let top = 0.58 - row as f32 * 0.20;
            for (x0, x1, label) in [(-0.82, -0.05, &left[row]), (0.05, 0.82, &right[row])] {
                draw_button(
                    vertices,
                    x0,
                    x1,
                    top - 0.13,
                    top,
                    hit(
                        self.mouse_ndc[0],
                        self.mouse_ndc[1],
                        x0,
                        x1,
                        top - 0.13,
                        top,
                    ),
                );
                draw_centered_text_in(
                    vertices,
                    label,
                    x0,
                    x1,
                    top - 0.092,
                    0.0058,
                    aspect,
                    [1.0; 4],
                );
            }
        }
        draw_button(
            vertices,
            -0.25,
            0.25,
            -0.78,
            -0.64,
            hit(
                self.mouse_ndc[0],
                self.mouse_ndc[1],
                -0.25,
                0.25,
                -0.78,
                -0.64,
            ),
        );
        draw_centered_text(vertices, "DONE", -0.738, 0.008, aspect, [1.0; 4]);
    }

    fn draw_controls(&self, vertices: &mut Vec<UiVertex>, aspect: f32) {
        panel(vertices, -0.86, 0.86, -0.88, 0.82);
        draw_centered_text(vertices, "CONTROLS", 0.72, 0.012, aspect, [1.0; 4]);
        draw_button(
            vertices,
            -0.48,
            0.48,
            0.49,
            0.62,
            hit(
                self.mouse_ndc[0],
                self.mouse_ndc[1],
                -0.48,
                0.48,
                0.49,
                0.62,
            ),
        );
        draw_centered_text(
            vertices,
            &format!(
                "MOUSE SENSITIVITY: < {:.1} >",
                self.settings.sensitivity * 1000.0
            ),
            0.528,
            0.0065,
            aspect,
            [1.0; 4],
        );
        let actions = [
            ControlAction::Forward,
            ControlAction::Backward,
            ControlAction::Left,
            ControlAction::Right,
            ControlAction::Jump,
            ControlAction::Sprint,
            ControlAction::Sneak,
            ControlAction::Inventory,
        ];
        for (index, action) in actions.into_iter().enumerate() {
            let column = index / 4;
            let row = index % 4;
            let (x0, x1) = if column == 0 {
                (-0.78, -0.04)
            } else {
                (0.04, 0.78)
            };
            let top = 0.38 - row as f32 * 0.19;
            let active = self.rebinding == Some(action);
            draw_button_state(
                vertices,
                x0,
                x1,
                top - 0.14,
                top,
                hit(
                    self.mouse_ndc[0],
                    self.mouse_ndc[1],
                    x0,
                    x1,
                    top - 0.14,
                    top,
                ),
                active,
            );
            let value = if active {
                "PRESS A KEY"
            } else {
                key_name(self.control(action))
            };
            draw_centered_text_in(
                vertices,
                &format!("{}: {}", action.label(), value),
                x0,
                x1,
                top - 0.098,
                0.0065,
                aspect,
                [1.0; 4],
            );
        }
        draw_button(
            vertices,
            -0.25,
            0.25,
            -0.78,
            -0.64,
            hit(
                self.mouse_ndc[0],
                self.mouse_ndc[1],
                -0.25,
                0.25,
                -0.78,
                -0.64,
            ),
        );
        draw_centered_text(vertices, "DONE", -0.738, 0.008, aspect, [1.0; 4]);
    }

    fn control(&self, action: ControlAction) -> KeyCode {
        match action {
            ControlAction::Forward => self.settings.controls.forward,
            ControlAction::Backward => self.settings.controls.backward,
            ControlAction::Left => self.settings.controls.left,
            ControlAction::Right => self.settings.controls.right,
            ControlAction::Jump => self.settings.controls.jump,
            ControlAction::Sprint => self.settings.controls.sprint,
            ControlAction::Sneak => self.settings.controls.sneak,
            ControlAction::Inventory => self.settings.controls.inventory,
        }
    }

    fn draw_delete_confirmation(&self, vertices: &mut Vec<UiVertex>, aspect: f32) {
        draw_rect(vertices, -1.0, 1.0, -1.0, 1.0, [0.0, 0.0, 0.0, 0.62]);
        panel(vertices, -0.58, 0.58, -0.32, 0.34);
        draw_centered_text(
            vertices,
            "DELETE THIS WORLD?",
            0.20,
            0.011,
            aspect,
            [1.0, 0.45, 0.35, 1.0],
        );
        draw_centered_text(
            vertices,
            "THIS CANNOT BE UNDONE",
            0.08,
            0.0065,
            aspect,
            [0.85, 0.85, 0.85, 1.0],
        );
        draw_button(
            vertices,
            -0.48,
            -0.02,
            -0.16,
            -0.02,
            hit(
                self.mouse_ndc[0],
                self.mouse_ndc[1],
                -0.48,
                -0.02,
                -0.16,
                -0.02,
            ),
        );
        draw_button(
            vertices,
            0.02,
            0.48,
            -0.16,
            -0.02,
            hit(
                self.mouse_ndc[0],
                self.mouse_ndc[1],
                0.02,
                0.48,
                -0.16,
                -0.02,
            ),
        );
        draw_centered_text_in(
            vertices, "DELETE", -0.48, -0.02, -0.118, 0.007, aspect, [1.0; 4],
        );
        draw_centered_text_in(
            vertices, "CANCEL", 0.02, 0.48, -0.118, 0.007, aspect, [1.0; 4],
        );
    }
}

fn apply_fullscreen(window: &Window, enabled: bool) {
    window.set_fullscreen(if enabled {
        Some(Fullscreen::Borderless(window.current_monitor()))
    } else {
        None
    });
}

fn present_mode(vsync: bool, modes: &[wgpu::PresentMode]) -> wgpu::PresentMode {
    if vsync || !modes.contains(&wgpu::PresentMode::Immediate) {
        wgpu::PresentMode::Fifo
    } else {
        wgpu::PresentMode::Immediate
    }
}

fn hash_seed(value: &str) -> u32 {
    value.bytes().fold(2_166_136_261, |hash, byte| {
        (hash ^ byte as u32).wrapping_mul(16_777_619)
    })
}

fn relative_time(timestamp: u64) -> String {
    let days = unix_now().saturating_sub(timestamp) / 86_400;
    match days {
        0 => "PLAYED TODAY".to_string(),
        1 => "PLAYED YESTERDAY".to_string(),
        days => format!("PLAYED {days} DAYS AGO"),
    }
}

fn percent(value: f32) -> u32 {
    (value * 100.0).round() as u32
}

fn on_off(value: bool) -> &'static str {
    if value {
        "ON"
    } else {
        "OFF"
    }
}

fn tr(language: Language, english: &'static str) -> &'static str {
    if language == Language::English {
        return english;
    }
    match english {
        "SINGLEPLAYER" => "EINZELSPIELER",
        "OPTIONS" => "OPTIONEN",
        "QUIT GAME" => "SPIEL BEENDEN",
        "SELECT WORLD" => "WELT AUSWAHLEN",
        _ => english,
    }
}

fn hit(x: f32, y: f32, x0: f32, x1: f32, y0: f32, y1: f32) -> bool {
    x >= x0 && x <= x1 && y >= y0 && y <= y1
}

fn draw_rect(vertices: &mut Vec<UiVertex>, x0: f32, x1: f32, y0: f32, y1: f32, color: [f32; 4]) {
    for position in [[x0, y1], [x0, y0], [x1, y0], [x0, y1], [x1, y0], [x1, y1]] {
        vertices.push(UiVertex { position, color });
    }
}

fn panel(vertices: &mut Vec<UiVertex>, x0: f32, x1: f32, y0: f32, y1: f32) {
    draw_rect(vertices, x0, x1, y0, y1, [0.055, 0.06, 0.065, 0.93]);
    draw_rect(vertices, x0, x1, y1 - 0.012, y1, [0.42, 0.45, 0.46, 1.0]);
    draw_rect(vertices, x0, x1, y0, y0 + 0.012, [0.015, 0.018, 0.02, 1.0]);
}

fn draw_button(vertices: &mut Vec<UiVertex>, x0: f32, x1: f32, y0: f32, y1: f32, hover: bool) {
    draw_button_state(vertices, x0, x1, y0, y1, hover, false);
}

fn draw_button_state(
    vertices: &mut Vec<UiVertex>,
    x0: f32,
    x1: f32,
    y0: f32,
    y1: f32,
    hover: bool,
    selected: bool,
) {
    let fill = if selected {
        [0.23, 0.38, 0.18, 0.98]
    } else if hover {
        [0.30, 0.31, 0.32, 0.98]
    } else {
        [0.16, 0.17, 0.18, 0.98]
    };
    let light = if hover || selected {
        [0.92, 0.94, 0.90, 1.0]
    } else {
        [0.48, 0.50, 0.51, 1.0]
    };
    draw_rect(vertices, x0, x1, y0, y1, fill);
    draw_rect(vertices, x0, x1, y1 - 0.008, y1, light);
    draw_rect(vertices, x0, x0 + 0.006, y0, y1, light);
    draw_rect(vertices, x0, x1, y0, y0 + 0.008, [0.035, 0.04, 0.04, 1.0]);
    draw_rect(vertices, x1 - 0.006, x1, y0, y1, [0.035, 0.04, 0.04, 1.0]);
}

fn draw_field(
    vertices: &mut Vec<UiVertex>,
    label: &str,
    value: &str,
    x0: f32,
    x1: f32,
    y0: f32,
    y1: f32,
    active: bool,
    aspect: f32,
) {
    draw_text(
        vertices,
        label,
        x0,
        y1 + 0.035,
        0.006,
        aspect,
        [0.8, 0.82, 0.84, 1.0],
    );
    draw_button_state(vertices, x0, x1, y0, y1, false, active);
    draw_centered_text_in(vertices, value, x0, x1, y0 + 0.038, 0.008, aspect, [1.0; 4]);
}

fn draw_logo(vertices: &mut Vec<UiVertex>, aspect: f32) {
    draw_centered_text(
        vertices,
        "ICRAFT",
        0.505,
        0.026,
        aspect,
        [0.04, 0.045, 0.04, 1.0],
    );
    draw_centered_text(
        vertices,
        "ICRAFT",
        0.53,
        0.026,
        aspect,
        [0.72, 0.75, 0.70, 1.0],
    );
    draw_centered_text(
        vertices,
        "RUST EDITION",
        0.43,
        0.007,
        aspect,
        [1.0, 0.83, 0.18, 1.0],
    );
}

fn text_width(text: &str, pixel: f32, aspect: f32) -> f32 {
    let char_width = pixel * aspect * 6.0;
    text.chars().count() as f32 * char_width - pixel * aspect
}

fn draw_centered_text(
    vertices: &mut Vec<UiVertex>,
    text: &str,
    y: f32,
    pixel: f32,
    aspect: f32,
    color: [f32; 4],
) {
    let x = -text_width(text, pixel, aspect) * 0.5;
    draw_text(vertices, text, x, y, pixel, aspect, color);
}

fn draw_centered_text_in(
    vertices: &mut Vec<UiVertex>,
    text: &str,
    x0: f32,
    x1: f32,
    y: f32,
    pixel: f32,
    aspect: f32,
    color: [f32; 4],
) {
    let x = (x0 + x1 - text_width(text, pixel, aspect)) * 0.5;
    draw_text(vertices, text, x, y, pixel, aspect, color);
}

fn draw_text(
    vertices: &mut Vec<UiVertex>,
    text: &str,
    x: f32,
    y: f32,
    pixel: f32,
    aspect: f32,
    color: [f32; 4],
) {
    let pixel_x = pixel * aspect;
    let mut cursor = x;
    for ch in text.to_ascii_uppercase().chars() {
        let rows = glyph(ch);
        for (row, bits) in rows.into_iter().enumerate() {
            for column in 0..5 {
                if bits & (1 << (4 - column)) != 0 {
                    let px = cursor + column as f32 * pixel_x;
                    let py = y + (6 - row) as f32 * pixel;
                    draw_rect(
                        vertices,
                        px,
                        px + pixel_x * 0.88,
                        py,
                        py + pixel * 0.88,
                        color,
                    );
                }
            }
        }
        cursor += pixel_x * 6.0;
    }
}

fn glyph(ch: char) -> [u8; 7] {
    match ch {
        'A' => [14, 17, 17, 31, 17, 17, 17],
        'B' => [30, 17, 17, 30, 17, 17, 30],
        'C' => [14, 17, 16, 16, 16, 17, 14],
        'D' => [30, 17, 17, 17, 17, 17, 30],
        'E' => [31, 16, 16, 30, 16, 16, 31],
        'F' => [31, 16, 16, 30, 16, 16, 16],
        'G' => [14, 17, 16, 23, 17, 17, 14],
        'H' => [17, 17, 17, 31, 17, 17, 17],
        'I' => [31, 4, 4, 4, 4, 4, 31],
        'J' => [7, 2, 2, 2, 18, 18, 12],
        'K' => [17, 18, 20, 24, 20, 18, 17],
        'L' => [16, 16, 16, 16, 16, 16, 31],
        'M' => [17, 27, 21, 21, 17, 17, 17],
        'N' => [17, 25, 21, 19, 17, 17, 17],
        'O' => [14, 17, 17, 17, 17, 17, 14],
        'P' => [30, 17, 17, 30, 16, 16, 16],
        'Q' => [14, 17, 17, 17, 21, 18, 13],
        'R' => [30, 17, 17, 30, 20, 18, 17],
        'S' => [15, 16, 16, 14, 1, 1, 30],
        'T' => [31, 4, 4, 4, 4, 4, 4],
        'U' => [17, 17, 17, 17, 17, 17, 14],
        'V' => [17, 17, 17, 17, 17, 10, 4],
        'W' => [17, 17, 17, 21, 21, 21, 10],
        'X' => [17, 17, 10, 4, 10, 17, 17],
        'Y' => [17, 17, 10, 4, 4, 4, 4],
        'Z' => [31, 1, 2, 4, 8, 16, 31],
        '0' => [14, 17, 19, 21, 25, 17, 14],
        '1' => [4, 12, 4, 4, 4, 4, 14],
        '2' => [14, 17, 1, 2, 4, 8, 31],
        '3' => [30, 1, 1, 14, 1, 1, 30],
        '4' => [2, 6, 10, 18, 31, 2, 2],
        '5' => [31, 16, 16, 30, 1, 1, 30],
        '6' => [14, 16, 16, 30, 17, 17, 14],
        '7' => [31, 1, 2, 4, 8, 8, 8],
        '8' => [14, 17, 17, 14, 17, 17, 14],
        '9' => [14, 17, 17, 15, 1, 1, 14],
        ':' => [0, 4, 4, 0, 4, 4, 0],
        '.' => [0, 0, 0, 0, 0, 4, 4],
        '-' => [0, 0, 0, 31, 0, 0, 0],
        '_' => [0, 0, 0, 0, 0, 0, 31],
        '<' => [2, 4, 8, 16, 8, 4, 2],
        '>' => [8, 4, 2, 1, 2, 4, 8],
        '/' => [1, 2, 2, 4, 8, 8, 16],
        '%' => [17, 2, 4, 8, 17, 0, 0],
        '?' => [14, 17, 1, 2, 4, 0, 4],
        _ => [0; 7],
    }
}

const UI_SHADER: &str = r#"
struct In { @location(0) position: vec2<f32>, @location(1) color: vec4<f32> };
struct Out { @builtin(position) position: vec4<f32>, @location(0) color: vec4<f32> };
@vertex fn vs_main(input: In) -> Out {
    var out: Out;
    out.position = vec4<f32>(input.position, 0.0, 1.0);
    out.color = input.color;
    return out;
}
@fragment fn fs_main(input: Out) -> @location(0) vec4<f32> { return input.color; }
"#;

const PANORAMA_SHADER: &str = r#"
struct Panorama { time: f32, width: f32, height: f32, padding: f32 };
@group(0) @binding(0) var<uniform> panorama: Panorama;
@vertex fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let x = f32((index << 1u) & 2u);
    let y = f32(index & 2u);
    return vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
}
fn hash(p: vec2<f32>) -> f32 { return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453); }
@fragment fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let resolution = vec2<f32>(max(panorama.width, 1.0), max(panorama.height, 1.0));
    var uv = position.xy / resolution;
    let horizon = 0.58;
    let travel = panorama.time * 0.018;
    let sky = mix(vec3<f32>(0.07, 0.19, 0.34), vec3<f32>(0.45, 0.67, 0.78), clamp(uv.y / horizon, 0.0, 1.0));
    var color = sky;
    let sun = smoothstep(0.055, 0.045, distance(uv, vec2<f32>(0.78, 0.20)));
    color = mix(color, vec3<f32>(1.0, 0.82, 0.38), sun * 0.75);
    let far_h = 0.48 + floor((sin((uv.x + travel) * 18.0) * 0.035 + sin((uv.x + travel) * 7.0) * 0.05) * 40.0) / 40.0;
    if uv.y > far_h { color = vec3<f32>(0.18, 0.28, 0.25); }
    let near_h = 0.62 + floor((sin((uv.x + travel * 1.7) * 24.0) * 0.045 + sin((uv.x + travel) * 9.0) * 0.07) * 32.0) / 32.0;
    if uv.y > near_h { color = vec3<f32>(0.12, 0.26, 0.12); }
    if uv.y > near_h + 0.035 { color = vec3<f32>(0.25, 0.20, 0.12); }
    let cell = floor(vec2<f32>((uv.x + travel * 1.7) * 80.0, uv.y * 80.0));
    color *= 0.88 + hash(cell) * 0.18;
    let vignette = 1.0 - 0.50 * dot(uv - vec2<f32>(0.5), uv - vec2<f32>(0.5));
    return vec4<f32>(color * vignette, 1.0);
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_world_names_and_generates_stable_slugs() {
        assert_eq!(sanitize_name("  My <World>!  "), "My World");
        assert_eq!(slugify("My World"), "my_world");
    }

    #[test]
    fn settings_key_names_round_trip() {
        for code in [
            KeyCode::KeyW,
            KeyCode::Space,
            KeyCode::ControlLeft,
            KeyCode::ArrowUp,
        ] {
            assert_eq!(parse_key(key_name(code)), Some(code));
        }
    }

    #[test]
    fn difficulty_steps_both_directions() {
        assert_eq!(Difficulty::Peaceful.step(-1), Difficulty::Hard);
        assert_eq!(Difficulty::Normal.step(1), Difficulty::Hard);
    }
}
