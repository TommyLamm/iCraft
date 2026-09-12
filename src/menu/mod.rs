use crate::game_rules::{WorldCreationOptions, WorldType};

pub use crate::game_rules::Difficulty;
use crate::inventory::GameMode;
pub use crate::presentation_inventory_policy::MultiplayerRole;
use crate::{
    accessibility::AccessibilitySettings,
    localization::TranslationCatalog,
    resources::{FontSource, ResourcePackManager},
};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use wgpu::util::DeviceExt;
use winit::event::ElementState;
use winit::keyboard::{Key, KeyCode, NamedKey, PhysicalKey};
use winit::window::{Fullscreen, Window};

#[path = "../server_address_book.rs"]
mod server_address_book;
pub use server_address_book::{AddressBookError, ServerAddressBook, ServerPingResult};

mod controls;
mod settings;
mod widgets;
use controls::{ControlAction, CONTROL_BINDINGS};
use widgets::*;


fn controls_static_focus_count() -> usize {
    1 + CONTROLS_VISIBLE_ROWS + 1
}

fn controls_button_rects() -> Vec<MenuRect> {
    let mut rects = Vec::with_capacity(controls_static_focus_count());
    rects.push(CONTROLS_SENSITIVITY.rect);
    for i in 0..CONTROLS_VISIBLE_ROWS {
        rects.push(control_binding_rect(i));
    }
    rects.push(CONTROLS_DONE.rect);
    rects
}

fn options_button_rects() -> [MenuRect; 15] {
    std::array::from_fn(|i| OPTIONS_SCREEN.widgets[i].rect)
}

fn accessibility_button_rects() -> [MenuRect; 11] {
    std::array::from_fn(|i| ACCESSIBILITY_SCREEN.widgets[i].rect)
}

const UI_VERTEX_CAPACITY: usize = 65_536;
const SETTINGS_FILE: &str = "settings.txt";
const CONTROLS_FILE: &str = "controls.config";
const SAVES_DIR: &str = "saves";
const META_FILE: &str = "world.meta";
const CURRENT_WORLD_FORMAT_VERSION: u32 = 3;
const OPTIONS_ROW_TOPS: [f32; 6] = [0.58, 0.38, 0.18, -0.02, -0.22, -0.42];

pub use settings::{ControlBindings, GameSettings, Language};
use settings::{cycle_fps_cap, fps_cap_label, key_name, parse_bool, parse_key};

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
    world_type: WorldType,
    generate_structures: bool,
    bonus_chest: bool,
    cheats_enabled: bool,
    hardcore: bool,
    version: u32,
    needs_upgrade: bool,
}

impl WorldMetadata {
    fn load(world_dir: &Path) -> Option<Self> {
        let contents = fs::read_to_string(world_dir.join(META_FILE)).ok()?;
        let mut name = None;
        let mut seed = 12345;
        let mut game_mode = GameMode::Survival;
        let mut difficulty = Difficulty::Normal;
        let mut last_played = 0;
        let mut world_type = WorldType::Default;
        let mut generate_structures = true;
        let mut bonus_chest = false;
        let mut cheats_enabled = false;
        let mut hardcore = false;
        let mut version = 0;
        let mut needs_upgrade = false;
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
                "world_type" => world_type = WorldType::parse(value),
                "generate_structures" => {
                    generate_structures = parse_bool(value, generate_structures)
                }
                "bonus_chest" => bonus_chest = parse_bool(value, bonus_chest),
                "cheats" | "cheats_enabled" => cheats_enabled = parse_bool(value, cheats_enabled),
                "hardcore" => hardcore = parse_bool(value, hardcore),
                "version" | "format_version" => version = value.trim().parse().unwrap_or(version),
                "needs_upgrade" => needs_upgrade = parse_bool(value, needs_upgrade),
                _ => {}
            }
        }
        needs_upgrade |= version < CURRENT_WORLD_FORMAT_VERSION;
        Some(Self {
            name: name?,
            seed,
            game_mode,
            difficulty,
            last_played,
            world_type,
            generate_structures,
            bonus_chest,
            cheats_enabled,
            hardcore,
            version,
            needs_upgrade,
        })
    }

    fn save(&self, world_dir: &Path) -> std::io::Result<()> {
        fs::create_dir_all(world_dir.join("regions"))?;
        crate::save::atomic_write(
            world_dir.join(META_FILE),
            format!(
                "name:{}\nseed:{}\ngame_mode:{}\ndifficulty:{}\nlast_played:{}\nworld_type:{}\ngenerate_structures:{}\nbonus_chest:{}\ncheats_enabled:{}\nhardcore:{}\nversion:{}\nneeds_upgrade:{}\n",
                self.name,
                self.seed,
                game_mode_name(self.game_mode),
                self.difficulty.as_str(),
                self.last_played,
                self.world_type.as_str(),
                self.generate_structures,
                self.bonus_chest,
                self.cheats_enabled,
                self.hardcore,
                self.version,
                self.needs_upgrade,
            )
            .as_bytes(),
        )
    }
}

/// Load creation-only options without exposing the menu's text metadata type.
/// Legacy worlds use the documented defaults until their next authoritative
/// level save writes the richer binary fields.
pub fn load_world_creation_options(world_dir: &Path) -> WorldCreationOptions {
    crate::save::load_world_creation_options(world_dir)
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
        GameMode::Adventure => "ADVENTURE",
        GameMode::Spectator => "SPECTATOR",
    }
}

fn parse_game_mode(value: &str) -> GameMode {
    match value.trim().to_ascii_lowercase().as_str() {
        "creative" => GameMode::Creative,
        "adventure" => GameMode::Adventure,
        "spectator" => GameMode::Spectator,
        _ => GameMode::Survival,
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
        let Ok(directory) = validated_world_path(&entry.path()) else {
            continue;
        };
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

fn world_index_by_directory(worlds: &[WorldEntry], directory: &Path) -> Option<usize> {
    worlds.iter().position(|world| world.directory == directory)
}

pub fn update_world_metadata(
    world_dir: &Path,
    seed: u32,
    game_mode: GameMode,
    difficulty: Difficulty,
) -> std::io::Result<()> {
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
            world_type: WorldType::Default,
            generate_structures: true,
            bonus_chest: false,
            cheats_enabled: false,
            hardcore: false,
            version: CURRENT_WORLD_FORMAT_VERSION,
            needs_upgrade: false,
        });
    if metadata.needs_upgrade {
        let base = world_dir
            .file_name()
            .and_then(|name| name.to_str())
            .map(slugify)
            .unwrap_or_else(|| "world".to_string());
        let mut backup_path = Path::new(SAVES_DIR).join(format!("{base}_backup_{}", unix_now()));
        let mut suffix = 2;
        while backup_path.exists() {
            backup_path =
                Path::new(SAVES_DIR).join(format!("{base}_backup_{}_{}", unix_now(), suffix));
            suffix += 1;
        }
        backup_world(world_dir, &backup_path)?;
    }
    metadata.seed = seed;
    metadata.game_mode = game_mode;
    metadata.difficulty = difficulty;
    metadata.last_played = unix_now();
    metadata.version = CURRENT_WORLD_FORMAT_VERSION;
    metadata.needs_upgrade = false;
    metadata.save(world_dir)
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
    let hardcore = level.hardcore || level.rules.hardcore;
    Some(WorldMetadata {
        name,
        seed: level.seed,
        game_mode: player.game_mode,
        difficulty: if hardcore {
            Difficulty::Hard
        } else {
            Difficulty::Normal
        },
        last_played: modified,
        world_type: level.world_type,
        generate_structures: level.generate_structures,
        bonus_chest: level.bonus_chest,
        cheats_enabled: level.cheats_enabled,
        hardcore,
        version: level.version,
        needs_upgrade: level.version < CURRENT_WORLD_FORMAT_VERSION,
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

fn canonical_saves_root() -> std::io::Result<PathBuf> {
    let root = Path::new(SAVES_DIR);
    if !root.exists() {
        fs::create_dir_all(root)?;
    }
    fs::canonicalize(root)
}

fn path_is_within(root: &Path, candidate: &Path) -> bool {
    candidate != root && candidate.starts_with(root)
}

fn is_symlink_or_junction(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
        return metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0;
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Resolve a world directory while rejecting the saves root itself, traversal,
/// and symlink/junction roots. Launch, upgrade, discover, and destructive
/// operations all use this guard.
pub fn validated_world_path(path: &Path) -> std::io::Result<PathBuf> {
    let root = canonical_saves_root()?;
    let metadata = fs::symlink_metadata(path)?;
    if is_symlink_or_junction(&metadata) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "world path must not be a symlink or junction",
        ));
    }
    if !metadata.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "world path must be a directory",
        ));
    }
    let candidate = fs::canonicalize(path)?;
    if path_is_within(&root, &candidate) {
        Ok(candidate)
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "world path must remain inside the saves directory",
        ))
    }
}

pub fn delete_world(path: &Path) -> std::io::Result<()> {
    let directory = validated_world_path(path)?;
    fs::remove_dir_all(directory)
}

fn copy_tree(source: &Path, destination: &Path) -> std::io::Result<()> {
    let metadata = fs::symlink_metadata(source)?;
    if metadata.file_type().is_symlink() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "symlink entries are not valid world data",
        ));
    }
    if metadata.is_dir() {
        fs::create_dir_all(destination)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            copy_tree(&entry.path(), &destination.join(entry.file_name()))?;
        }
    } else {
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(source, destination)?;
    }
    Ok(())
}

pub fn copy_world(source: &Path, destination: &Path) -> std::io::Result<()> {
    let source = validated_world_path(source)?;
    let root = canonical_saves_root()?;
    let destination = if destination.exists() {
        fs::canonicalize(destination)?
    } else {
        let parent = destination.parent().unwrap_or(Path::new(SAVES_DIR));
        let parent = fs::canonicalize(parent)?;
        parent.join(destination.file_name().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "destination has no name")
        })?)
    };
    if !path_is_within(&root, &destination) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "destination must remain inside the saves directory",
        ));
    }
    if destination.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "destination world already exists",
        ));
    }
    copy_tree(&source, &destination)
}

pub fn backup_world(source: &Path, destination: &Path) -> std::io::Result<()> {
    copy_world(source, destination)
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
    position: [f32; 3],
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
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: 12,
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
    Accessibility,
    ResourcePacks,
    ConfirmDelete,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct MenuRect {
    pub x0: f32,
    pub x1: f32,
    pub y0: f32,
    pub y1: f32,
}

impl MenuRect {
    pub const fn new(x0: f32, x1: f32, y0: f32, y1: f32) -> Self {
        Self { x0, x1, y0, y1 }
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x0 && x <= self.x1 && y >= self.y0 && y <= self.y1
    }

    pub fn as_array(&self) -> [f32; 4] {
        [self.x0, self.x1, self.y0, self.y1]
    }
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

fn back_transition(
    screen: MenuScreen,
    _active_field: Option<TextField>,
    _rebinding: Option<ControlAction>,
) -> (MenuScreen, Option<TextField>, Option<ControlAction>) {
    let screen = match screen {
        MenuScreen::Main => MenuScreen::Main,
        MenuScreen::Multiplayer | MenuScreen::Worlds | MenuScreen::Options => MenuScreen::Main,
        MenuScreen::CreateWorld | MenuScreen::ConfirmDelete => MenuScreen::Worlds,
        MenuScreen::Controls | MenuScreen::Accessibility | MenuScreen::ResourcePacks => {
            MenuScreen::Options
        }
    };
    (screen, None, None)
}

fn multiplayer_focus_count(mode: MultiplayerMode, recent_count: usize) -> usize {
    if mode == MultiplayerMode::Join {
        MULTIPLAYER_MODE_RECTS.len()
            + MULTIPLAYER_JOIN_FIELD_RECTS.len()
            + recent_count.min(3)
            + 1
            + MULTIPLAYER_BOTTOM_RECTS.len()
    } else {
        MULTIPLAYER_MODE_RECTS.len() + 1 + MULTIPLAYER_BOTTOM_RECTS.len()
    }
}

fn multiplayer_focus_rects(mode: MultiplayerMode, recent_count: usize) -> Vec<[f32; 4]> {
    let mut rects = vec![
        MULTIPLAYER_MODE_RECTS[0].as_array(),
        MULTIPLAYER_MODE_RECTS[1].as_array(),
    ];
    if mode == MultiplayerMode::Host {
        rects.push(MULTIPLAYER_HOST_PORT_RECT.as_array());
    } else {
        rects.extend(MULTIPLAYER_JOIN_FIELD_RECTS.iter().map(|r| r.as_array()));
        for index in 0..recent_count.min(3) {
            rects.push(recent_server_item_rect(index).as_array());
        }
        rects.push(MULTIPLAYER_PING_RECT.as_array());
    }
    rects.extend(MULTIPLAYER_BOTTOM_RECTS.iter().map(|r| r.as_array()));
    rects
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
    /// Store the selected save identity, not its current list index. The list
    /// is sorted by last-played time, so an index can refer to another world
    /// after a refresh.
    selected_world: Option<PathBuf>,
    world_scroll: usize,
    resource_pack_scroll: usize,
    control_scroll: usize,
    create_name: String,
    create_seed: String,
    create_mode: GameMode,
    create_difficulty: Difficulty,
    create_world_type: WorldType,
    create_generate_structures: bool,
    create_bonus_chest: bool,
    create_cheats: bool,
    create_hardcore: bool,
    multiplayer_mode: MultiplayerMode,
    selected_role: MultiplayerRole,
    host_port: String,
    server_address: String,
    join_port: String,
    username: String,
    active_field: Option<TextField>,
    rebinding: Option<ControlAction>,
    message: Option<String>,
    server_address_book: ServerAddressBook,
    pub settings: GameSettings,
    resource_packs: ResourcePackManager,
    catalog: TranslationCatalog,
    font_source: FontSource,
    focus_index: usize,
    supported_present_modes: Vec<wgpu::PresentMode>,
    gpu_timestamps_supported: bool,
    gpu_timestamps_inside_passes: bool,
}

impl Menu {
    fn refresh_catalog(&mut self) {
        self.catalog = TranslationCatalog::from_resource_packs_mut(
            &mut self.resource_packs,
            self.settings.language,
        );
        self.font_source = self.resource_packs.resolve_font_source("font/ui.json");
    }

    fn activate_field(&mut self, field: TextField) {
        self.active_field = Some(field);
        // Keep keyboard focus and the text caret on the same logical control
        // when a field is entered with the mouse.  This makes Tab/Shift+Tab
        // continue from the clicked field instead of from a stale ring.
        self.focus_index = match field {
            TextField::WorldName => 0,
            TextField::Seed => 1,
            TextField::HostPort | TextField::ServerAddress => 2,
            TextField::JoinPort => 3,
            TextField::Username => 4,
        };
    }

    fn tr<'a>(&'a self, key: &'a str) -> &'a str {
        self.catalog.lookup(key)
    }

    fn on_off_label(&self, value: bool) -> &str {
        self.tr(if value { "menu.on" } else { "menu.off" })
    }

    pub fn into_gpu_context(self) -> crate::presentation::bootstrap::GpuContext {
        crate::presentation::bootstrap::GpuContext {
            surface: self.surface,
            device: self.device,
            queue: self.queue,
            config: self.config,
            size: self.size,
            supported_present_modes: self.supported_present_modes,
            gpu_timestamps_supported: self.gpu_timestamps_supported,
            gpu_timestamps_inside_passes: self.gpu_timestamps_inside_passes,
        }
    }

    pub async fn new(window: Arc<Window>, settings: GameSettings) -> Self {
        let gpu = crate::presentation::bootstrap::create_gpu_context(&window, &settings).await;
        Self::from_gpu(window, settings, gpu).await
    }

    pub async fn from_gpu(
        window: Arc<Window>,
        settings: GameSettings,
        gpu: crate::presentation::bootstrap::GpuContext,
    ) -> Self {
        window.set_cursor_visible(true);
        let _ = window.set_cursor_grab(winit::window::CursorGrabMode::None);
        apply_fullscreen(&window, settings.fullscreen);
        let crate::presentation::bootstrap::GpuContext {
            surface,
            device,
            queue,
            config,
            size,
            supported_present_modes,
            gpu_timestamps_supported,
            gpu_timestamps_inside_passes,
        } = gpu;
        let format = config.format;
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
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
                "../shader.wgsl"
            ))),
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
                entry_point: "vs_ui",
                buffers: &[UiVertex::desc()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &ui_shader,
                entry_point: "fs_ui",
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

        let mut resource_packs = ResourcePackManager::discover_default();
        if !settings.resource_packs.is_empty() {
            let _ = resource_packs.apply_enabled_order(&settings.resource_packs);
        }
        let catalog =
            TranslationCatalog::from_resource_packs_mut(&mut resource_packs, settings.language);
        let font_source = resource_packs.resolve_font_source("font/ui.json");
        let server_address_book = match ServerAddressBook::load_default() {
            Ok(book) => book,
            Err(error) => {
                eprintln!("[Menu] failed to load server address book: {error}");
                ServerAddressBook::default()
            }
        };
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
            resource_pack_scroll: 0,
            control_scroll: 0,
            create_name: "NEW WORLD".to_string(),
            create_seed: String::new(),
            create_mode: GameMode::Survival,
            create_difficulty: settings.difficulty,
            create_world_type: WorldType::Default,
            create_generate_structures: true,
            create_bonus_chest: false,
            create_cheats: false,
            create_hardcore: false,
            multiplayer_mode: MultiplayerMode::Host,
            selected_role: MultiplayerRole::Singleplayer,
            host_port: settings.mp_host_port.clone(),
            server_address: settings.mp_server_address.clone(),
            join_port: settings.mp_join_port.clone(),
            username: settings.mp_username.clone(),
            active_field: None,
            rebinding: None,
            message: None,
            server_address_book,
            settings,
            resource_packs,
            catalog,
            font_source,
            focus_index: 0,
            supported_present_modes,
            gpu_timestamps_supported,
            gpu_timestamps_inside_passes,
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
        let ui_scale = self.settings.accessibility.ui_scale.max(0.5) as f64;
        self.mouse_ndc = [
            (x as f32 / self.size.width.max(1) as f32 * 2.0 - 1.0) / ui_scale as f32,
            (1.0 - y as f32 / self.size.height.max(1) as f32 * 2.0) / ui_scale as f32,
        ];
    }

    pub fn handle_scroll(&mut self, direction: i32) {
        match self.screen {
            MenuScreen::Worlds if self.worlds.len() > 5 => {
                let max_scroll = self.worlds.len() - 5;
                self.world_scroll =
                    (self.world_scroll as i32 + direction).clamp(0, max_scroll as i32) as usize;
            }
            MenuScreen::Controls => {
                let max_scroll = CONTROL_BINDINGS.len().saturating_sub(CONTROLS_VISIBLE_ROWS);
                if direction < 0 {
                    self.control_scroll = self.control_scroll.saturating_sub(1);
                } else {
                    self.control_scroll = (self.control_scroll + 1).min(max_scroll);
                }
            }
            MenuScreen::ResourcePacks => {
                let count = self.resource_packs.available().len();
                if count > 5 {
                    let max_scroll = count - 5;
                    self.resource_pack_scroll = (self.resource_pack_scroll as i32 + direction)
                        .clamp(0, max_scroll as i32)
                        as usize;
                    self.ensure_focus_visible();
                }
            }
            _ => {}
        }
    }

    pub fn handle_key(
        &mut self,
        state: ElementState,
        physical_key: PhysicalKey,
        logical_key: &Key,
        repeat: bool,
        shift_held: bool,
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
                Key::Named(NamedKey::Tab) => {
                    // Commit the current text field before moving focus.  Tab
                    // must remain usable from text input rather than being
                    // swallowed by the character-edit branch; the entered
                    // text stays in its backing string for the next field.
                    self.active_field = None;
                    let direction = if shift_held {
                        crate::accessibility::FocusDirection::Backward
                    } else {
                        crate::accessibility::FocusDirection::Forward
                    };
                    self.move_focus(direction);
                }
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
        if matches!(logical_key, Key::Named(NamedKey::Tab)) {
            let direction = if shift_held {
                crate::accessibility::FocusDirection::Backward
            } else {
                crate::accessibility::FocusDirection::Forward
            };
            self.move_focus(direction);
            return MenuAction::None;
        }
        if matches!(logical_key, Key::Named(NamedKey::ArrowDown)) {
            self.move_focus(crate::accessibility::FocusDirection::Forward);
            return MenuAction::None;
        }
        if matches!(logical_key, Key::Named(NamedKey::ArrowUp)) {
            self.move_focus(crate::accessibility::FocusDirection::Backward);
            return MenuAction::None;
        }
        if matches!(logical_key, Key::Named(NamedKey::Enter)) {
            return self.activate_focused();
        }
        if matches!(logical_key, Key::Named(NamedKey::Escape)) {
            self.back();
        }
        MenuAction::None
    }

    fn move_focus(&mut self, direction: crate::accessibility::FocusDirection) {
        let mut focus = crate::accessibility::FocusNavigator::new(self.focus_count());
        focus.set_count(self.focus_count());
        for _ in 0..self.focus_index {
            focus.move_by(crate::accessibility::FocusDirection::Forward);
        }
        focus.move_by(direction);
        self.focus_index = focus.index();
        self.ensure_focus_visible();
    }

    fn ensure_focus_visible(&mut self) {
        match self.screen {
            MenuScreen::Worlds => {
                let count = self.worlds.len();
                if count == 0 || self.focus_index >= count {
                    return;
                }
                if self.focus_index < self.world_scroll {
                    self.world_scroll = self.focus_index;
                } else if self.focus_index >= self.world_scroll + 5 {
                    self.world_scroll = self.focus_index.saturating_sub(4);
                }
                self.world_scroll = self.world_scroll.min(count.saturating_sub(5));
            }
            MenuScreen::ResourcePacks => {
                let count = self.resource_packs.available().len();
                if count == 0 || self.focus_index >= count {
                    return;
                }
                if self.focus_index < self.resource_pack_scroll {
                    self.resource_pack_scroll = self.focus_index;
                } else if self.focus_index >= self.resource_pack_scroll + 5 {
                    self.resource_pack_scroll = self.focus_index.saturating_sub(4);
                }
                self.resource_pack_scroll = self.resource_pack_scroll.min(count.saturating_sub(5));
            }
            _ => {}
        }
    }

    fn activate_focused(&mut self) -> MenuAction {
        let Some([x0, x1, y0, y1]) = self.focus_rect() else {
            return MenuAction::None;
        };
        self.mouse_ndc = [(x0 + x1) * 0.5, (y0 + y1) * 0.5];
        self.handle_click()
    }

    fn focus_count(&self) -> usize {
        match self.screen {
            MenuScreen::Main => MAIN_SCREEN_RECTS.len(),
            MenuScreen::Options => options_button_rects().len(),
            MenuScreen::Controls => controls_static_focus_count(),
            MenuScreen::Accessibility => accessibility_button_rects().len(),
            MenuScreen::ResourcePacks => {
                self.resource_packs.available().len() + RESOURCE_PACKS_BOTTOM_RECTS.len()
            }
            MenuScreen::Multiplayer => multiplayer_focus_count(
                self.multiplayer_mode,
                self.server_address_book.addresses().len(),
            ),
            MenuScreen::Worlds => self.worlds.len() + WORLDS_BOTTOM_RECTS.len(),
            MenuScreen::CreateWorld => CREATE_WORLD_SCREEN_RECTS.len(),
            MenuScreen::ConfirmDelete => CONFIRM_DELETE_RECTS.len(),
        }
    }

    fn focus_rect(&self) -> Option<[f32; 4]> {
        let rects = match self.screen {
            MenuScreen::Main => MAIN_SCREEN_RECTS.iter().map(|r| r.as_array()).collect(),
            MenuScreen::Options => options_button_rects()
                .iter()
                .map(|r| r.as_array())
                .collect(),
            MenuScreen::Controls => controls_button_rects()
                .iter()
                .map(|r| r.as_array())
                .collect(),
            MenuScreen::Accessibility => accessibility_button_rects()
                .iter()
                .map(|r| r.as_array())
                .collect(),
            MenuScreen::ResourcePacks => {
                let mut rects = (0..self.resource_packs.available().len())
                    .map(|index| {
                        let visible_index = index as isize - self.resource_pack_scroll as isize;
                        resource_pack_item_rect(visible_index).as_array()
                    })
                    .collect::<Vec<_>>();
                rects.extend(RESOURCE_PACKS_BOTTOM_RECTS.iter().map(|r| r.as_array()));
                rects
            }
            MenuScreen::Multiplayer => multiplayer_focus_rects(
                self.multiplayer_mode,
                self.server_address_book.addresses().len(),
            ),
            MenuScreen::Worlds => {
                let mut rects = (0..self.worlds.len())
                    .map(|index| {
                        let visible_index = index as isize - self.world_scroll as isize;
                        world_item_rect(visible_index).as_array()
                    })
                    .collect::<Vec<_>>();
                rects.extend(WORLDS_BOTTOM_RECTS.iter().map(|r| r.as_array()));
                rects
            }
            MenuScreen::CreateWorld => CREATE_WORLD_SCREEN_RECTS.iter().map(|r| r.as_array()).collect(),
            MenuScreen::ConfirmDelete => CONFIRM_DELETE_RECTS
                .iter()
                .map(|r| r.as_array())
                .collect(),
        };
        rects
            .get(self.focus_index.min(rects.len().saturating_sub(1)))
            .copied()
    }

    pub fn handle_click(&mut self) -> MenuAction {
        let [x, y] = self.mouse_ndc;
        self.message = None;
        match self.screen {
            MenuScreen::Main => {
                if MAIN_SCREEN_RECTS[0].contains(x, y) {
                    self.selected_role = MultiplayerRole::Singleplayer;
                    self.worlds = discover_worlds();
                    self.selected_world = self.worlds.first().map(|world| world.directory.clone());
                    self.world_scroll = 0;
                    self.screen = MenuScreen::Worlds;
                } else if MAIN_SCREEN_RECTS[1].contains(x, y) {
                    self.active_field = None;
                    self.screen = MenuScreen::Multiplayer;
                } else if MAIN_SCREEN_RECTS[2].contains(x, y) {
                    self.screen = MenuScreen::Options;
                } else if MAIN_SCREEN_RECTS[3].contains(x, y) {
                    return MenuAction::Quit;
                }
            }
            MenuScreen::Multiplayer => {
                if MULTIPLAYER_MODE_RECTS[0].contains(x, y) {
                    self.multiplayer_mode = MultiplayerMode::Host;
                    self.active_field = None;
                } else if MULTIPLAYER_MODE_RECTS[1].contains(x, y) {
                    self.multiplayer_mode = MultiplayerMode::Join;
                    self.active_field = None;
                } else if self.multiplayer_mode == MultiplayerMode::Host
                    && MULTIPLAYER_HOST_PORT_RECT.contains(x, y)
                {
                    self.activate_field(TextField::HostPort);
                } else if self.multiplayer_mode == MultiplayerMode::Join
                    && MULTIPLAYER_JOIN_FIELD_RECTS[0].contains(x, y)
                {
                    self.activate_field(TextField::ServerAddress);
                } else if self.multiplayer_mode == MultiplayerMode::Join
                    && MULTIPLAYER_JOIN_FIELD_RECTS[1].contains(x, y)
                {
                    self.activate_field(TextField::JoinPort);
                } else if self.multiplayer_mode == MultiplayerMode::Join
                    && MULTIPLAYER_JOIN_FIELD_RECTS[2].contains(x, y)
                {
                    self.activate_field(TextField::Username);
                } else if self.multiplayer_mode == MultiplayerMode::Join
                    && self.select_recent_server(x, y)
                {
                    self.active_field = None;
                } else if self.multiplayer_mode == MultiplayerMode::Join
                    && MULTIPLAYER_PING_RECT.contains(x, y)
                {
                    self.ping_selected_server();
                } else if MULTIPLAYER_BOTTOM_RECTS[0].contains(x, y) {
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
                        self.message = Some(self.tr("menu.enter_valid_multiplayer").to_string());
                        return MenuAction::None;
                    };
                    let is_client = matches!(role, MultiplayerRole::Client { .. });
                    self.selected_role = role;
                    self.sync_and_save_multiplayer_settings();
                    self.active_field = None;
                    if is_client {
                        return self.launch_client();
                    }
                    self.worlds = discover_worlds();
                    self.selected_world = self.worlds.first().map(|world| world.directory.clone());
                    self.world_scroll = 0;
                    self.screen = MenuScreen::Worlds;
                } else if MULTIPLAYER_BOTTOM_RECTS[1].contains(x, y) {
                    self.sync_and_save_multiplayer_settings();
                    self.active_field = None;
                    self.screen = MenuScreen::Main;
                }
            }
            MenuScreen::Worlds => {
                for visible_index in 0..(self.worlds.len() - self.world_scroll).min(5) {
                    let rect = world_item_rect(visible_index as isize);
                    if rect.contains(x, y) {
                        let index = self.world_scroll + visible_index;
                        self.selected_world = Some(self.worlds[index].directory.clone());
                        return MenuAction::None;
                    }
                }
                if WORLDS_BOTTOM_RECTS[0].contains(x, y) {
                    if let Some(directory) = self.selected_world.clone() {
                        return self.launch_existing(&directory);
                    }
                } else if WORLDS_BOTTOM_RECTS[1].contains(x, y) {
                    self.create_name = "NEW WORLD".to_string();
                    self.create_seed.clear();
                    self.create_mode = GameMode::Survival;
                    self.create_difficulty = self.settings.difficulty;
                    self.create_world_type = WorldType::Default;
                    self.create_generate_structures = true;
                    self.create_bonus_chest = false;
                    self.create_cheats = false;
                    self.create_hardcore = false;
                    self.screen = MenuScreen::CreateWorld;
                } else if WORLDS_BOTTOM_RECTS[2].contains(x, y) {
                    if self.selected_world.is_some() {
                        self.screen = MenuScreen::ConfirmDelete;
                    }
                } else if WORLDS_BOTTOM_RECTS[3].contains(x, y) {
                    if let Some(directory) = self.selected_world.clone() {
                        let base = directory
                            .file_name()
                            .and_then(|name| name.to_str())
                            .unwrap_or("world");
                        let destination = unique_world_dir(&format!("{base}_copy"));
                        match copy_world(&directory, &destination) {
                            Ok(()) => {
                                self.worlds = discover_worlds();
                                self.message = Some(self.tr("menu.world_copied").to_string());
                            }
                            Err(error) => self.message = Some(format!("COPY FAILED: {error}")),
                        }
                    }
                } else if WORLDS_BOTTOM_RECTS[4].contains(x, y) {
                    if let Some(directory) = self.selected_world.clone() {
                        let base = directory
                            .file_name()
                            .and_then(|name| name.to_str())
                            .unwrap_or("world");
                        let destination = unique_world_dir(&format!("{base}_backup"));
                        match backup_world(&directory, &destination) {
                            Ok(()) => {
                                self.worlds = discover_worlds();
                                self.message = Some(self.tr("menu.world_backed_up").to_string());
                            }
                            Err(error) => self.message = Some(format!("BACKUP FAILED: {error}")),
                        }
                    }
                } else if WORLDS_BOTTOM_RECTS[5].contains(x, y) {
                    self.screen = MenuScreen::Main;
                }
            }
            MenuScreen::CreateWorld => {
                if CREATE_WORLD_SCREEN_RECTS[0].contains(x, y) {
                    self.activate_field(TextField::WorldName);
                } else if CREATE_WORLD_SCREEN_RECTS[1].contains(x, y) {
                    self.activate_field(TextField::Seed);
                } else if CREATE_WORLD_SCREEN_RECTS[2].contains(x, y) {
                    self.create_mode = match self.create_mode {
                        GameMode::Survival => GameMode::Creative,
                        GameMode::Creative => GameMode::Adventure,
                        GameMode::Adventure => GameMode::Spectator,
                        GameMode::Spectator => GameMode::Survival,
                    };
                } else if CREATE_WORLD_SCREEN_RECTS[3].contains(x, y) {
                    self.create_difficulty =
                        self.create_difficulty.step(if x < 0.0 { -1 } else { 1 });
                } else if CREATE_WORLD_SCREEN_RECTS[4].contains(x, y) {
                    self.create_world_type = match self.create_world_type {
                        WorldType::Default => WorldType::Superflat,
                        WorldType::Superflat => WorldType::Default,
                    };
                } else if CREATE_WORLD_SCREEN_RECTS[5].contains(x, y) {
                    self.create_generate_structures = !self.create_generate_structures;
                } else if CREATE_WORLD_SCREEN_RECTS[6].contains(x, y) {
                    self.create_hardcore = !self.create_hardcore;
                    if self.create_hardcore {
                        self.create_mode = GameMode::Survival;
                        self.create_difficulty = Difficulty::Hard;
                    }
                } else if CREATE_WORLD_SCREEN_RECTS[7].contains(x, y) {
                    self.create_bonus_chest = !self.create_bonus_chest;
                } else if CREATE_WORLD_SCREEN_RECTS[8].contains(x, y) {
                    self.create_cheats = !self.create_cheats;
                } else if CREATE_WORLD_SCREEN_RECTS[9].contains(x, y) {
                    return self.create_world();
                } else if CREATE_WORLD_SCREEN_RECTS[10].contains(x, y) {
                    self.active_field = None;
                    self.screen = MenuScreen::Worlds;
                }
            }
            MenuScreen::Options => self.handle_options_click(x, y),
            MenuScreen::Controls => {
                let rects = controls_button_rects();
                if rects[0].contains(x, y) {
                    let delta = if x < 0.0 { -0.0002 } else { 0.0002 };
                    self.settings.sensitivity =
                        (self.settings.sensitivity + delta).clamp(0.0002, 0.006);
                    self.settings.save();
                    return MenuAction::None;
                }
                let visible = CONTROL_BINDINGS
                    .iter()
                    .skip(self.control_scroll)
                    .take(CONTROLS_VISIBLE_ROWS);
                for (index, meta) in visible.enumerate() {
                    if rects[1 + index].contains(x, y) {
                        self.rebinding = Some(meta.action);
                    }
                }
                if rects.last().is_some_and(|rect| rect.contains(x, y)) {
                    self.back();
                }
            }
            MenuScreen::Accessibility => self.handle_accessibility_click(x, y),
            MenuScreen::ResourcePacks => self.handle_resource_pack_click(x, y),
            MenuScreen::ConfirmDelete => {
                if CONFIRM_DELETE_RECTS[0].contains(x, y) {
                    if let Some(directory) = self.selected_world.as_deref() {
                        if let Some(world) = world_index_by_directory(&self.worlds, directory)
                            .and_then(|index| self.worlds.get(index))
                        {
                            if let Err(error) = delete_world(&world.directory) {
                                self.message = Some(format!("DELETE FAILED: {error}"));
                                self.screen = MenuScreen::Worlds;
                                return MenuAction::None;
                            }
                        }
                    }
                    self.worlds = discover_worlds();
                    self.selected_world = self.worlds.first().map(|world| world.directory.clone());
                    self.world_scroll = self.world_scroll.min(self.worlds.len().saturating_sub(5));
                    self.screen = MenuScreen::Worlds;
                } else if CONFIRM_DELETE_RECTS[1].contains(x, y) {
                    self.screen = MenuScreen::Worlds;
                }
            }
        }
        self.focus_index = self.focus_index.min(self.focus_count().saturating_sub(1));
        MenuAction::None
    }

    fn sync_and_save_multiplayer_settings(&mut self) {
        self.settings.mp_host_port = self.host_port.clone();
        self.settings.mp_server_address = self.server_address.clone();
        self.settings.mp_join_port = self.join_port.clone();
        self.settings.mp_username = self.username.clone();
        self.settings.save();
        if let Err(error) = self.server_address_book.save_default() {
            eprintln!("[Menu] failed to save server address book: {error}");
        }
    }

    fn join_target(&self) -> Option<String> {
        let address = self.server_address.trim();
        let port = self.join_port.trim().parse::<u16>().ok()?;
        if address.is_empty() || port == 0 {
            return None;
        }
        if address.starts_with('[') {
            Some(format!("{address}:{port}"))
        } else if address.matches(':').count() > 1 {
            Some(format!("[{address}]:{port}"))
        } else {
            Some(format!("{address}:{port}"))
        }
    }

    fn select_recent_server(&mut self, x: f32, y: f32) -> bool {
        for (index, target) in self
            .server_address_book
            .addresses()
            .iter()
            .take(3)
            .enumerate()
        {
            if recent_server_item_rect(index).contains(x, y) {
                let target = target.clone();
                if let Some((host, port)) = split_host_port(&target) {
                    self.server_address = host;
                    self.join_port = port;
                    self.sync_and_save_multiplayer_settings();
                    self.message = Some(format!("SELECTED {target}"));
                    return true;
                } else {
                    self.message = Some("INVALID SAVED SERVER ADDRESS".to_string());
                    return true;
                }
            }
        }
        false
    }

    fn ping_selected_server(&mut self) {
        let Some(target) = self.join_target() else {
            self.message = Some(self.tr("menu.enter_valid_multiplayer").to_string());
            return;
        };
        let result = self
            .server_address_book
            .ping(target.clone(), std::time::Duration::from_secs(2));
        if let Err(error) = self.server_address_book.save_default() {
            eprintln!("[Menu] failed to save server address book after ping: {error}");
        }
        self.message = Some(match result.error {
            Some(error) => format!("PING FAILED: {error}"),
            None => format!(
                "PING {} {}/{}",
                result.version, result.online_players, result.max_players
            ),
        });
    }

    fn back(&mut self) {
        if self.screen == MenuScreen::Multiplayer {
            self.sync_and_save_multiplayer_settings();
        }
        let (screen, active_field, rebinding) =
            back_transition(self.screen, self.active_field, self.rebinding);
        self.screen = screen;
        self.active_field = active_field;
        self.rebinding = rebinding;
    }

    fn launch_existing(&mut self, directory: &Path) -> MenuAction {
        let world_dir = match validated_world_path(directory) {
            Ok(path) => path,
            Err(error) => {
                self.message = Some(format!("LAUNCH FAILED: {error}"));
                return MenuAction::None;
            }
        };
        let Some(index) = world_index_by_directory(&self.worlds, &world_dir)
            .or_else(|| world_index_by_directory(&self.worlds, directory))
        else {
            return MenuAction::None;
        };
        let world = &mut self.worlds[index];
        world.metadata.last_played = unix_now();
        let _ = world.metadata.save(&world_dir);
        MenuAction::Launch(
            WorldLaunch {
                world_dir,
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
            world_type: self.create_world_type,
            generate_structures: self.create_generate_structures,
            bonus_chest: self.create_bonus_chest,
            cheats_enabled: self.create_cheats,
            hardcore: self.create_hardcore,
            version: CURRENT_WORLD_FORMAT_VERSION,
            needs_upgrade: false,
        };
        if let Err(error) = metadata.save(&world_dir) {
            self.message = Some(format!("CREATE FAILED: {error}"));
            return MenuAction::None;
        }
        let world_dir = match validated_world_path(&world_dir) {
            Ok(path) => path,
            Err(error) => {
                self.message = Some(format!("CREATE FAILED: {error}"));
                return MenuAction::None;
            }
        };
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

    fn launch_client(&mut self) -> MenuAction {
        // Placeholder path only. `State::new` must not construct a world
        // SaveManager, chunk save worker, or NetworkSnapshotWorker for a join
        // client, so this directory is never used as a persist root.
        let world_dir = std::env::temp_dir().join("icraft_multiplayer_client");
        MenuAction::Launch(
            WorldLaunch {
                world_dir,
                seed: 0,
                game_mode: GameMode::Survival,
                difficulty: self.settings.difficulty,
                role: self.selected_role.clone(),
            },
            self.settings.clone(),
        )
    }

    fn handle_options_click(&mut self, x: f32, y: f32) {
        let rects = options_button_rects();
        let delta = if x < -0.43 || (x > 0.05 && x < 0.43) {
            -1.0
        } else {
            1.0
        };
        if rects[0].contains(x, y) {
            self.settings.fov = (self.settings.fov + delta * 5.0).clamp(30.0, 120.0);
        } else if rects[1].contains(x, y) {
            self.settings.render_distance =
                (self.settings.render_distance + delta as i32).clamp(2, 16);
        } else if rects[2].contains(x, y) {
            self.settings.fullscreen = !self.settings.fullscreen;
            apply_fullscreen(&self.window, self.settings.fullscreen);
        } else if rects[3].contains(x, y) {
            if self.settings.vsync {
                let has_uncapped = self
                    .supported_present_modes
                    .contains(&wgpu::PresentMode::Mailbox)
                    || self
                        .supported_present_modes
                        .contains(&wgpu::PresentMode::Immediate);
                if !has_uncapped {
                    self.message = Some("VSYNC REQUIRED ON THIS DISPLAY".to_string());
                    return;
                }
                self.settings.vsync = false;
                self.config.present_mode = present_mode(false, &self.supported_present_modes);
            } else {
                self.settings.vsync = true;
                self.config.present_mode = wgpu::PresentMode::Fifo;
            }
            self.surface.configure(&self.device, &self.config);
        } else if rects[4].contains(x, y) {
            self.settings.difficulty = self.settings.difficulty.step(delta as i32);
        } else if rects[5].contains(x, y) {
            self.settings.fps_cap = cycle_fps_cap(self.settings.fps_cap, delta as i32);
        } else if rects[6].contains(x, y) {
            self.settings.master_volume =
                (self.settings.master_volume + delta * 0.1).clamp(0.0, 1.0);
        } else if rects[7].contains(x, y) {
            self.settings.music_volume = (self.settings.music_volume + delta * 0.1).clamp(0.0, 1.0);
        } else if rects[8].contains(x, y) {
            self.settings.sound_volume = (self.settings.sound_volume + delta * 0.1).clamp(0.0, 1.0);
        } else if rects[9].contains(x, y) {
            self.settings.weather_volume =
                (self.settings.weather_volume + delta * 0.1).clamp(0.0, 1.0);
        } else if rects[10].contains(x, y) {
            self.settings.language = self.settings.language.toggle();
            self.refresh_catalog();
        } else if rects[11].contains(x, y) {
            self.screen = MenuScreen::Accessibility;
            return;
        } else if rects[12].contains(x, y) {
            self.screen = MenuScreen::ResourcePacks;
            self.resource_pack_scroll = self
                .resource_pack_scroll
                .min(self.resource_packs.available().len().saturating_sub(5));
            self.focus_index = 0;
            return;
        } else if rects[13].contains(x, y) {
            self.screen = MenuScreen::Controls;
            self.focus_index = 0;
            return;
        } else if rects[14].contains(x, y) {
            self.screen = MenuScreen::Main;
            self.focus_index = 0;
            return;
        } else {
            return;
        }
        self.settings.save();
    }

    fn control_mut(&mut self, action: ControlAction) -> &mut KeyCode {
        let _ = CONTROL_BINDINGS
            .iter()
            .find(|meta| meta.action == action)
            .expect("control action must be in CONTROL_BINDINGS");
        match action {
            ControlAction::Forward => &mut self.settings.controls.forward,
            ControlAction::Backward => &mut self.settings.controls.backward,
            ControlAction::Left => &mut self.settings.controls.left,
            ControlAction::Right => &mut self.settings.controls.right,
            ControlAction::Jump => &mut self.settings.controls.jump,
            ControlAction::Sprint => &mut self.settings.controls.sprint,
            ControlAction::Sneak => &mut self.settings.controls.sneak,
            ControlAction::Inventory => &mut self.settings.controls.inventory,
            ControlAction::Chat => &mut self.settings.controls.chat,
            ControlAction::TimeSpeed => &mut self.settings.controls.time_speed,
            ControlAction::Advancements => &mut self.settings.controls.advancements,
            ControlAction::Debug => &mut self.settings.controls.debug,
            ControlAction::Perspective => &mut self.settings.controls.perspective,
            ControlAction::Gamemode => &mut self.settings.controls.gamemode,
            ControlAction::Pause => &mut self.settings.controls.pause,
            ControlAction::Hotbar1 => &mut self.settings.controls.hotbar_1,
            ControlAction::Hotbar2 => &mut self.settings.controls.hotbar_2,
            ControlAction::Hotbar3 => &mut self.settings.controls.hotbar_3,
            ControlAction::Hotbar4 => &mut self.settings.controls.hotbar_4,
            ControlAction::Hotbar5 => &mut self.settings.controls.hotbar_5,
            ControlAction::Hotbar6 => &mut self.settings.controls.hotbar_6,
            ControlAction::Hotbar7 => &mut self.settings.controls.hotbar_7,
            ControlAction::Hotbar8 => &mut self.settings.controls.hotbar_8,
            ControlAction::Hotbar9 => &mut self.settings.controls.hotbar_9,
        }
    }

    pub fn update(&mut self, dt: f32) {
        let motion_scale = if self.settings.accessibility.reduce_flashing {
            0.25
        } else {
            1.0
        };
        self.elapsed += dt.min(0.1) * motion_scale;
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
        draw_rect(vertices, -1.0, 1.0, -1.0, 1.0, [0.02, 0.03, 0.04, 0.30]);
        let ui_start = vertices.len();
        match self.screen {
            MenuScreen::Main => {
                draw_logo(vertices, aspect, &self.font_source);
                let [x, y] = self.mouse_ndc;
                for rect in MAIN_SCREEN_RECTS {
                    draw_button(
                        vertices,
                        rect.x0,
                        rect.x1,
                        rect.y0,
                        rect.y1,
                        rect.contains(x, y),
                    );
                }
                draw_centered_text(
                    vertices,
                    &self.tr("menu.singleplayer"),
                    0.248,
                    0.010,
                    aspect,
                    [1.0; 4],
                    &self.font_source,
                );
                draw_centered_text(
                    vertices,
                    &self.tr("menu.multiplayer"),
                    0.068,
                    0.010,
                    aspect,
                    [1.0; 4],
                    &self.font_source,
                );
                draw_centered_text(
                    vertices,
                    &self.tr("menu.options"),
                    -0.112,
                    0.010,
                    aspect,
                    [1.0; 4],
                    &self.font_source,
                );
                draw_centered_text(
                    vertices,
                    &self.tr("menu.quit_game"),
                    -0.292,
                    0.010,
                    aspect,
                    [1.0; 4],
                    &self.font_source,
                );
                draw_text(
                    vertices,
                    "JAVA-FREE EDITION",
                    -0.96,
                    -0.94,
                    0.006,
                    aspect,
                    [0.8, 0.84, 0.86, 1.0],
                    &self.font_source,
                );
            }
            MenuScreen::Multiplayer => self.draw_multiplayer(vertices, aspect),
            MenuScreen::Worlds => self.draw_worlds(vertices, aspect),
            MenuScreen::CreateWorld => self.draw_create(vertices, aspect),
            MenuScreen::Options => self.draw_options(vertices, aspect),
            MenuScreen::Controls => self.draw_controls(vertices, aspect),
            MenuScreen::Accessibility => self.draw_accessibility(vertices, aspect),
            MenuScreen::ResourcePacks => self.draw_resource_packs(vertices, aspect),
            MenuScreen::ConfirmDelete => self.draw_delete_confirmation(vertices, aspect),
        }
        if let Some([x0, x1, y0, y1]) = self.focus_rect() {
            draw_focus_ring(vertices, x0, x1, y0, y1);
        }
        if let Some(message) = &self.message {
            draw_centered_text(
                vertices,
                message,
                -0.94,
                0.007,
                aspect,
                [1.0, 0.35, 0.25, 1.0],
                &self.font_source,
            );
        }
        let requested_scale = self.settings.accessibility.ui_scale.clamp(0.75, 2.0);
        let max_abs = vertices
            .iter()
            .skip(ui_start)
            .flat_map(|vertex| [vertex.position[0].abs(), vertex.position[1].abs()])
            .fold(0.0, f32::max);
        let ui_scale = crate::accessibility::fit_ui_scale(requested_scale, max_abs);
        for vertex in vertices.iter_mut().skip(ui_start) {
            vertex.position[0] *= ui_scale;
            vertex.position[1] *= ui_scale;
            if self.settings.accessibility.high_contrast {
                let is_focus =
                    vertex.color[0] > 0.9 && vertex.color[1] > 0.6 && vertex.color[2] < 0.35;
                if !is_focus {
                    let luminance = vertex.color[0] * 0.2126
                        + vertex.color[1] * 0.7152
                        + vertex.color[2] * 0.0722;
                    let value = if luminance > 0.45 { 1.0 } else { 0.02 };
                    vertex.color[0] = value;
                    vertex.color[1] = value;
                    vertex.color[2] = value;
                }
            }
        }
    }

    fn draw_multiplayer(&self, vertices: &mut Vec<UiVertex>, aspect: f32) {
        panel(vertices, -0.64, 0.64, -0.72, 0.78);
        draw_centered_text(
            vertices,
            &self.tr("menu.multiplayer"),
            0.67,
            0.012,
            aspect,
            [1.0; 4],
            &self.font_source,
        );

        let [x, y] = self.mouse_ndc;
        for (rect, label, selected) in [
            (
                MULTIPLAYER_MODE_RECTS[0],
                self.tr("menu.host_game"),
                self.multiplayer_mode == MultiplayerMode::Host,
            ),
            (
                MULTIPLAYER_MODE_RECTS[1],
                self.tr("menu.join_game"),
                self.multiplayer_mode == MultiplayerMode::Join,
            ),
        ] {
            let hover = rect.contains(x, y);
            draw_button_state(
                vertices, rect.x0, rect.x1, rect.y0, rect.y1, hover, selected,
            );
            draw_centered_text_in(
                vertices,
                &label,
                rect.x0,
                rect.x1,
                0.488,
                0.007,
                aspect,
                [1.0; 4],
                &self.font_source,
            );
        }

        match self.multiplayer_mode {
            MultiplayerMode::Host => draw_field(
                vertices,
                &self.tr("menu.port"),
                &self.host_port,
                MULTIPLAYER_HOST_PORT_RECT.x0,
                MULTIPLAYER_HOST_PORT_RECT.x1,
                MULTIPLAYER_HOST_PORT_RECT.y0,
                MULTIPLAYER_HOST_PORT_RECT.y1,
                self.active_field == Some(TextField::HostPort),
                aspect,
                &self.font_source,
            ),
            MultiplayerMode::Join => {
                let field_rects = MULTIPLAYER_JOIN_FIELD_RECTS;
                draw_field(
                    vertices,
                    &self.tr("menu.server_address"),
                    &self.server_address,
                    field_rects[0].x0,
                    field_rects[0].x1,
                    field_rects[0].y0,
                    field_rects[0].y1,
                    self.active_field == Some(TextField::ServerAddress),
                    aspect,
                    &self.font_source,
                );
                draw_field(
                    vertices,
                    &self.tr("menu.port"),
                    &self.join_port,
                    field_rects[1].x0,
                    field_rects[1].x1,
                    field_rects[1].y0,
                    field_rects[1].y1,
                    self.active_field == Some(TextField::JoinPort),
                    aspect,
                    &self.font_source,
                );
                draw_field(
                    vertices,
                    &self.tr("menu.username"),
                    &self.username,
                    field_rects[2].x0,
                    field_rects[2].x1,
                    field_rects[2].y0,
                    field_rects[2].y1,
                    self.active_field == Some(TextField::Username),
                    aspect,
                    &self.font_source,
                );
                for (index, address) in self
                    .server_address_book
                    .addresses()
                    .iter()
                    .take(3)
                    .enumerate()
                {
                    let rect = recent_server_item_rect(index);
                    let hover = rect.contains(x, y);
                    let label = self
                        .server_address_book
                        .result_for(address)
                        .map(|result| {
                            let state = result
                                .error
                                .as_deref()
                                .map(|error| format!("ERR {error}"))
                                .unwrap_or_else(|| {
                                    format!(
                                        "{} {}/{}",
                                        result.version, result.online_players, result.max_players
                                    )
                                });
                            format!("{} {state}", address)
                        })
                        .unwrap_or_else(|| address.clone());
                    draw_button(vertices, rect.x0, rect.x1, rect.y0, rect.y1, hover);
                    draw_centered_text_in(
                        vertices,
                        &label.chars().take(34).collect::<String>(),
                        rect.x0,
                        rect.x1,
                        rect.y1 - 0.054,
                        0.0043,
                        aspect,
                        [1.0; 4],
                        &self.font_source,
                    );
                }
                let ping_rect = MULTIPLAYER_PING_RECT;
                draw_button(
                    vertices,
                    ping_rect.x0,
                    ping_rect.x1,
                    ping_rect.y0,
                    ping_rect.y1,
                    ping_rect.contains(x, y),
                );
                draw_centered_text_in(
                    vertices,
                    &self.tr("menu.ping_server"),
                    ping_rect.x0,
                    ping_rect.x1,
                    -0.204,
                    0.0058,
                    aspect,
                    [1.0; 4],
                    &self.font_source,
                );
            }
        }

        let confirm_label = match self.multiplayer_mode {
            MultiplayerMode::Host => self.tr("menu.select_world").to_string(),
            MultiplayerMode::Join => self.tr("menu.connect").to_string(),
        };
        for (rect, label) in MULTIPLAYER_BOTTOM_RECTS
            .iter()
            .zip([confirm_label, self.tr("menu.back").to_string()])
        {
            let hover = rect.contains(x, y);
            draw_button(vertices, rect.x0, rect.x1, rect.y0, rect.y1, hover);
            draw_centered_text_in(
                vertices,
                &label,
                rect.x0,
                rect.x1,
                -0.542,
                0.007,
                aspect,
                [1.0; 4],
                &self.font_source,
            );
        }
    }

    fn draw_worlds(&self, vertices: &mut Vec<UiVertex>, aspect: f32) {
        panel(vertices, -0.82, 0.82, -0.9, 0.82);
        draw_centered_text(
            vertices,
            &self.tr("menu.select_world"),
            0.72,
            0.012,
            aspect,
            [1.0; 4],
            &self.font_source,
        );
        if self.worlds.is_empty() {
            draw_centered_text(
                vertices,
                &self.tr("menu.no_worlds"),
                0.14,
                0.010,
                aspect,
                [0.8, 0.8, 0.8, 1.0],
                &self.font_source,
            );
        }
        let [x, y] = self.mouse_ndc;
        for (visible_index, world) in self
            .worlds
            .iter()
            .skip(self.world_scroll)
            .take(5)
            .enumerate()
        {
            let rect = world_item_rect(visible_index as isize);
            let selected = self.selected_world.as_deref() == Some(world.directory.as_path());
            let hover = rect.contains(x, y);
            draw_button_state(
                vertices, rect.x0, rect.x1, rect.y0, rect.y1, hover, selected,
            );
            draw_text(
                vertices,
                &world.metadata.name,
                -0.68,
                rect.y1 - 0.055,
                0.008,
                aspect,
                [1.0; 4],
                &self.font_source,
            );
            let detail = format!(
                "{} / {} / {} / {}{} / v{}{}",
                relative_time(world.metadata.last_played),
                game_mode_name(world.metadata.game_mode),
                world.metadata.difficulty.as_str(),
                world.metadata.world_type.as_str(),
                if world.metadata.hardcore {
                    " / HARDCORE"
                } else {
                    ""
                },
                world.metadata.version,
                if world.metadata.needs_upgrade {
                    " / UPGRADE"
                } else {
                    ""
                },
            );
            draw_text(
                vertices,
                &detail,
                -0.68,
                rect.y1 - 0.125,
                0.0055,
                aspect,
                [0.72, 0.76, 0.78, 1.0],
                &self.font_source,
            );
        }
        if self.worlds.len() > 5 {
            draw_text(
                vertices,
                &self.tr("menu.scroll_more_worlds"),
                0.38,
                -0.44,
                0.0048,
                aspect,
                [0.72, 0.76, 0.78, 1.0],
                &self.font_source,
            );
        }
        for (rect, label) in WORLDS_BOTTOM_RECTS[0..3].iter().zip([
            self.tr("menu.play_selected").to_string(),
            self.tr("menu.create_new_world").to_string(),
            self.tr("menu.delete").to_string(),
        ]) {
            draw_button(
                vertices,
                rect.x0,
                rect.x1,
                rect.y0,
                rect.y1,
                rect.contains(x, y),
            );
            draw_centered_text_in(
                vertices,
                &label,
                rect.x0,
                rect.x1,
                -0.602,
                0.006,
                aspect,
                [1.0; 4],
                &self.font_source,
            );
        }
        for (rect, label) in WORLDS_BOTTOM_RECTS[3..6].iter().zip([
            self.tr("menu.copy").to_string(),
            self.tr("menu.backup").to_string(),
            self.tr("menu.back").to_string(),
        ]) {
            draw_button(
                vertices,
                rect.x0,
                rect.x1,
                rect.y0,
                rect.y1,
                rect.contains(x, y),
            );
            draw_centered_text_in(
                vertices,
                &label,
                rect.x0,
                rect.x1,
                -0.805,
                0.006,
                aspect,
                [1.0; 4],
                &self.font_source,
            );
        }
    }

    fn draw_create(&self, vertices: &mut Vec<UiVertex>, aspect: f32) {
        panel(vertices, -0.64, 0.64, -0.92, 0.78);
        draw_centered_text(
            vertices,
            &self.tr("menu.create_new_world"),
            0.67,
            0.012,
            aspect,
            [1.0; 4],
            &self.font_source,
        );
        let rects = CREATE_WORLD_SCREEN_RECTS;
        let [x, y] = self.mouse_ndc;
        draw_field(
            vertices,
            &self.tr("menu.world_name"),
            &self.create_name,
            rects[0].x0,
            rects[0].x1,
            rects[0].y0,
            rects[0].y1,
            self.active_field == Some(TextField::WorldName),
            aspect,
            &self.font_source,
        );
        let seed = if self.create_seed.is_empty() {
            self.tr("menu.random").to_string()
        } else {
            self.create_seed.clone()
        };
        draw_field(
            vertices,
            &self.tr("menu.seed"),
            &seed,
            rects[1].x0,
            rects[1].x1,
            rects[1].y0,
            rects[1].y1,
            self.active_field == Some(TextField::Seed),
            aspect,
            &self.font_source,
        );
        draw_button(
            vertices,
            rects[2].x0,
            rects[2].x1,
            rects[2].y0,
            rects[2].y1,
            rects[2].contains(x, y),
        );
        draw_centered_text(
            vertices,
            &self.catalog.format_lookup(
                "menu.game_mode",
                &[("value", game_mode_name(self.create_mode))],
            ),
            -0.042,
            0.007,
            aspect,
            [1.0; 4],
            &self.font_source,
        );
        draw_button(
            vertices,
            rects[3].x0,
            rects[3].x1,
            rects[3].y0,
            rects[3].y1,
            rects[3].contains(x, y),
        );
        draw_centered_text(
            vertices,
            &self.catalog.format_lookup(
                "menu.difficulty",
                &[("value", self.create_difficulty.as_str())],
            ),
            -0.252,
            0.007,
            aspect,
            [1.0; 4],
            &self.font_source,
        );
        draw_button(
            vertices,
            rects[4].x0,
            rects[4].x1,
            rects[4].y0,
            rects[4].y1,
            rects[4].contains(x, y),
        );
        draw_centered_text(
            vertices,
            &self.catalog.format_lookup(
                "menu.world_type",
                &[("value", self.create_world_type.as_str())],
            ),
            -0.372,
            0.007,
            aspect,
            [1.0; 4],
            &self.font_source,
        );
        draw_button(
            vertices,
            rects[5].x0,
            rects[5].x1,
            rects[5].y0,
            rects[5].y1,
            rects[5].contains(x, y),
        );
        draw_button(
            vertices,
            rects[6].x0,
            rects[6].x1,
            rects[6].y0,
            rects[6].y1,
            rects[6].contains(x, y),
        );
        let structures_value = self.on_off_label(self.create_generate_structures);
        let structures_text = self
            .catalog
            .format_lookup("menu.structures", &[("value", &structures_value)]);
        draw_centered_text_in(
            vertices,
            &structures_text,
            rects[5].x0,
            rects[5].x1,
            -0.505,
            0.005,
            aspect,
            [1.0; 4],
            &self.font_source,
        );
        let hardcore_value = self.on_off_label(self.create_hardcore);
        let hardcore_text = self
            .catalog
            .format_lookup("menu.hardcore", &[("value", &hardcore_value)]);
        draw_centered_text_in(
            vertices,
            &hardcore_text,
            rects[6].x0,
            rects[6].x1,
            -0.505,
            0.005,
            aspect,
            [1.0; 4],
            &self.font_source,
        );
        draw_button(
            vertices,
            rects[7].x0,
            rects[7].x1,
            rects[7].y0,
            rects[7].y1,
            rects[7].contains(x, y),
        );
        draw_button(
            vertices,
            rects[8].x0,
            rects[8].x1,
            rects[8].y0,
            rects[8].y1,
            rects[8].contains(x, y),
        );
        let bonus_value = self.on_off_label(self.create_bonus_chest);
        let bonus_text = self
            .catalog
            .format_lookup("menu.bonus_chest", &[("value", &bonus_value)]);
        draw_centered_text_in(
            vertices,
            &bonus_text,
            rects[7].x0,
            rects[7].x1,
            -0.657,
            0.0048,
            aspect,
            [1.0; 4],
            &self.font_source,
        );
        let cheats_value = self.on_off_label(self.create_cheats);
        let cheats_text = self
            .catalog
            .format_lookup("menu.cheats", &[("value", &cheats_value)]);
        draw_centered_text_in(
            vertices,
            &cheats_text,
            rects[8].x0,
            rects[8].x1,
            -0.657,
            0.0048,
            aspect,
            [1.0; 4],
            &self.font_source,
        );
        draw_button(
            vertices,
            rects[9].x0,
            rects[9].x1,
            rects[9].y0,
            rects[9].y1,
            rects[9].contains(x, y),
        );
        draw_button(
            vertices,
            rects[10].x0,
            rects[10].x1,
            rects[10].y0,
            rects[10].y1,
            rects[10].contains(x, y),
        );
        draw_centered_text_in(
            vertices,
            &self.tr("menu.create_world"),
            rects[9].x0,
            rects[9].x1,
            -0.798,
            0.007,
            aspect,
            [1.0; 4],
            &self.font_source,
        );
        draw_centered_text_in(
            vertices,
            &self.tr("menu.cancel"),
            rects[10].x0,
            rects[10].x1,
            -0.798,
            0.007,
            aspect,
            [1.0; 4],
            &self.font_source,
        );
    }

    fn draw_options(&self, vertices: &mut Vec<UiVertex>, aspect: f32) {
        panel(vertices, -0.9, 0.9, -0.88, 0.82);
        draw_centered_text(
            vertices,
            &self.tr("menu.options"),
            0.72,
            0.012,
            aspect,
            [1.0; 4],
            &self.font_source,
        );
        let fov_value = format!("{:.0}", self.settings.fov);
        let render_distance_value = self.settings.render_distance.to_string();
        let fullscreen_value = self.on_off_label(self.settings.fullscreen);
        let vsync_value = self.on_off_label(self.settings.vsync);
        let fps_cap_value = fps_cap_label(self.settings.fps_cap);
        let master_volume_value = percent(self.settings.master_volume).to_string();
        let music_volume_value = percent(self.settings.music_volume).to_string();
        let sound_volume_value = percent(self.settings.sound_volume).to_string();
        let weather_volume_value = percent(self.settings.weather_volume).to_string();
        let left = [
            self.catalog
                .format_lookup("menu.fov", &[("value", &fov_value)]),
            self.catalog
                .format_lookup("menu.render_distance", &[("value", &render_distance_value)]),
            self.catalog
                .format_lookup("menu.fullscreen", &[("value", &fullscreen_value)]),
            self.catalog
                .format_lookup("menu.vsync", &[("value", &vsync_value)]),
            self.catalog.format_lookup(
                "menu.difficulty",
                &[("value", self.settings.difficulty.as_str())],
            ),
            self.catalog
                .format_lookup("menu.fps_cap", &[("value", &fps_cap_value)]),
        ];
        let right = [
            self.catalog
                .format_lookup("menu.master_volume", &[("value", &master_volume_value)]),
            self.catalog
                .format_lookup("menu.music_volume", &[("value", &music_volume_value)]),
            self.catalog
                .format_lookup("menu.sound_volume", &[("value", &sound_volume_value)]),
            self.catalog
                .format_lookup("menu.weather_volume", &[("value", &weather_volume_value)]),
            self.catalog.format_lookup(
                "menu.language",
                &[("value", self.settings.language.as_str())],
            ),
            self.tr("menu.accessibility").to_string(),
        ];
        let rects = options_button_rects();
        let [x, y] = self.mouse_ndc;
        for (row, label) in left.iter().enumerate() {
            let rect = rects[row];
            draw_button(
                vertices,
                rect.x0,
                rect.x1,
                rect.y0,
                rect.y1,
                rect.contains(x, y),
            );
            draw_centered_text_in(
                vertices,
                label,
                rect.x0,
                rect.x1,
                rect.y1 - 0.092,
                0.0058,
                aspect,
                [1.0; 4],
                &self.font_source,
            );
        }
        for (row, label) in right.iter().enumerate() {
            let rect = rects[6 + row];
            draw_button(
                vertices,
                rect.x0,
                rect.x1,
                rect.y0,
                rect.y1,
                rect.contains(x, y),
            );
            draw_centered_text_in(
                vertices,
                label,
                rect.x0,
                rect.x1,
                rect.y1 - 0.092,
                0.0058,
                aspect,
                [1.0; 4],
                &self.font_source,
            );
        }
        for (rect, label) in OPTIONS_BOTTOM_SCREEN_RECTS.iter().zip([
            self.tr("menu.resource_packs").to_string(),
            self.tr("menu.controls").to_string(),
            self.tr("menu.done").to_string(),
        ]) {
            draw_button(
                vertices,
                rect.x0,
                rect.x1,
                rect.y0,
                rect.y1,
                rect.contains(x, y),
            );
            draw_centered_text_in(
                vertices,
                &label,
                rect.x0,
                rect.x1,
                -0.738,
                0.006,
                aspect,
                [1.0; 4],
                &self.font_source,
            );
        }
    }

    fn handle_accessibility_click(&mut self, x: f32, y: f32) {
        let rects = accessibility_button_rects();
        for (index, rect) in rects[..10].iter().enumerate() {
            if rect.contains(x, y) {
                let setting = crate::accessibility::AccessibilityRow::ALL[index];
                let delta = if x < -0.42 || (x > 0.05 && x < 0.42) {
                    -1
                } else {
                    1
                };
                match setting {
                    crate::accessibility::AccessibilityRow::UiScale => {
                        self.settings.accessibility.cycle_ui_scale(delta)
                    }
                    crate::accessibility::AccessibilityRow::ChatScale => {
                        self.settings.accessibility.cycle_chat_scale(delta)
                    }
                    crate::accessibility::AccessibilityRow::ChatOpacity => {
                        self.settings.accessibility.cycle_chat_opacity(delta)
                    }
                    _ => self.settings.accessibility.toggle(setting),
                }
                self.settings.save();
                return;
            }
        }
        if rects[10].contains(x, y) {
            self.screen = MenuScreen::Options;
            self.focus_index = 0;
        }
    }

    fn handle_resource_pack_click(&mut self, x: f32, y: f32) {
        let available = self.resource_packs.available();
        let visible_count = available
            .len()
            .saturating_sub(self.resource_pack_scroll)
            .min(5);
        for row in 0..visible_count {
            let rect = resource_pack_item_rect(row as isize);
            if rect.contains(x, y) {
                let index = self.resource_pack_scroll + row;
                if let Some(summary) = available.get(index) {
                    let mut selected = self.resource_packs.enabled_order().to_vec();
                    if let Some(position) =
                        selected.iter().position(|id| id == &summary.manifest.id)
                    {
                        selected.remove(position);
                    } else {
                        selected.push(summary.manifest.id.clone());
                    }
                    if let Err(error) = self.resource_packs.apply_enabled_order(&selected) {
                        self.message = Some(format!("PACK REJECTED: {error}"));
                    } else {
                        self.refresh_catalog();
                    }
                }
                return;
            }
        }
        if RESOURCE_PACKS_BOTTOM_RECTS[0].contains(x, y) {
            self.settings.resource_packs = self.resource_packs.enabled_order().to_vec();
            self.settings.save();
            self.message = Some("PACKS APPLIED FOR NEXT WORLD".to_string());
        } else if RESOURCE_PACKS_BOTTOM_RECTS[1].contains(x, y) {
            if let Err(error) = self.resource_packs.reload() {
                self.message = Some(format!("PACK RELOAD FAILED: {error}"));
            } else if !self.settings.resource_packs.is_empty() {
                let _ = self
                    .resource_packs
                    .apply_enabled_order(&self.settings.resource_packs);
                self.refresh_catalog();
            } else {
                self.refresh_catalog();
            }
        } else if RESOURCE_PACKS_BOTTOM_RECTS[2].contains(x, y) {
            self.screen = MenuScreen::Options;
            self.focus_index = 0;
        }
    }

    fn draw_accessibility(&self, vertices: &mut Vec<UiVertex>, aspect: f32) {
        panel(vertices, -0.90, 0.90, -0.88, 0.82);
        draw_centered_text(
            vertices,
            &self.tr("menu.accessibility"),
            0.72,
            0.012,
            aspect,
            [1.0; 4],
            &self.font_source,
        );
        let rows = crate::accessibility::AccessibilityRow::ALL;
        let rects = accessibility_button_rects();
        let [x, y] = self.mouse_ndc;
        for (index, setting) in rows.into_iter().enumerate() {
            let rect = rects[index];
            let value = match setting {
                crate::accessibility::AccessibilityRow::UiScale => self.catalog.format_lookup(
                    "menu.ui_scale_value",
                    &[(
                        "value",
                        &format!("{:.2}x", self.settings.accessibility.ui_scale),
                    )],
                ),
                crate::accessibility::AccessibilityRow::ChatScale => self.catalog.format_lookup(
                    "menu.chat_scale_value",
                    &[(
                        "value",
                        &format!("{:.2}x", self.settings.accessibility.chat_scale),
                    )],
                ),
                crate::accessibility::AccessibilityRow::ChatOpacity => self.catalog.format_lookup(
                    "menu.chat_opacity_value",
                    &[(
                        "value",
                        &format!("{}%", percent(self.settings.accessibility.chat_opacity)),
                    )],
                ),
                _ => {
                    let label = accessibility_label(&self.catalog, setting);
                    let toggle = self.on_off_label(self.settings.accessibility.bool_value(setting));
                    self.catalog.format_lookup(
                        "menu.setting_value",
                        &[("label", &label), ("value", &toggle)],
                    )
                }
            };
            draw_button(
                vertices,
                rect.x0,
                rect.x1,
                rect.y0,
                rect.y1,
                rect.contains(x, y),
            );
            draw_centered_text_in(
                vertices,
                &value,
                rect.x0,
                rect.x1,
                rect.y1 - 0.092,
                0.0055,
                aspect,
                [1.0; 4],
                &self.font_source,
            );
        }
        let done_rect = rects[10];
        draw_button(
            vertices,
            done_rect.x0,
            done_rect.x1,
            done_rect.y0,
            done_rect.y1,
            done_rect.contains(x, y),
        );
        draw_centered_text(
            vertices,
            &self.tr("menu.done"),
            -0.738,
            0.008,
            aspect,
            [1.0; 4],
            &self.font_source,
        );
    }

    fn draw_resource_packs(&self, vertices: &mut Vec<UiVertex>, aspect: f32) {
        panel(vertices, -0.86, 0.86, -0.88, 0.82);
        draw_centered_text(
            vertices,
            &self.tr("menu.resource_packs"),
            0.72,
            0.012,
            aspect,
            [1.0; 4],
            &self.font_source,
        );
        let available = self.resource_packs.available();
        if available.is_empty() {
            draw_centered_text(
                vertices,
                &self.tr("menu.no_user_packs"),
                0.22,
                0.007,
                aspect,
                [0.8; 4],
                &self.font_source,
            );
        }
        let start = self.resource_pack_scroll.min(available.len());
        let [x, y] = self.mouse_ndc;
        for (visible_index, summary) in available.iter().skip(start).take(5).enumerate() {
            let rect = resource_pack_item_rect(visible_index as isize);
            let selected = summary.enabled;
            draw_button_state(
                vertices,
                rect.x0,
                rect.x1,
                rect.y0,
                rect.y1,
                rect.contains(x, y),
                selected,
            );
            let marker = if selected { "[X]" } else { "[ ]" };
            let label = format!(
                "{marker} {}  {}",
                summary.manifest.name, summary.manifest.version
            );
            draw_text_with_font(
                vertices,
                &label,
                -0.72,
                rect.y1 - 0.082,
                0.0058,
                aspect,
                [1.0; 4],
                &self.font_source,
            );
        }
        for (rect, label) in RESOURCE_PACKS_BOTTOM_RECTS.iter().zip([
            self.tr("menu.apply").to_string(),
            self.tr("menu.reload").to_string(),
            self.tr("menu.back").to_string(),
        ]) {
            draw_button(
                vertices,
                rect.x0,
                rect.x1,
                rect.y0,
                rect.y1,
                rect.contains(x, y),
            );
            draw_centered_text_in(
                vertices,
                &label,
                rect.x0,
                rect.x1,
                -0.738,
                0.006,
                aspect,
                [1.0; 4],
                &self.font_source,
            );
        }
        for (index, diagnostic) in self.resource_packs.diagnostics().iter().take(3).enumerate() {
            let detail = format!("PACK: {} — {}", diagnostic.source, diagnostic.message);
            let detail = detail.chars().take(108).collect::<String>();
            draw_text_with_font(
                vertices,
                &detail,
                -0.82,
                -0.56 - index as f32 * 0.045,
                0.0035,
                aspect,
                [1.0, 0.65, 0.25, 1.0],
                &self.font_source,
            );
        }
    }

    fn draw_controls(&self, vertices: &mut Vec<UiVertex>, aspect: f32) {
        panel(vertices, -0.86, 0.86, -0.88, 0.82);
        draw_centered_text(
            vertices,
            &self.tr("menu.controls"),
            0.72,
            0.012,
            aspect,
            [1.0; 4],
            &self.font_source,
        );
        let rects = controls_button_rects();
        let [x, y] = self.mouse_ndc;
        draw_button(
            vertices,
            rects[0].x0,
            rects[0].x1,
            rects[0].y0,
            rects[0].y1,
            rects[0].contains(x, y),
        );
        draw_centered_text(
            vertices,
            &self.catalog.format_lookup(
                "menu.mouse_sensitivity",
                &[(
                    "value",
                    &format!("{:.1}", self.settings.sensitivity * 1000.0),
                )],
            ),
            0.528,
            0.0065,
            aspect,
            [1.0; 4],
            &self.font_source,
        );
        let visible = CONTROL_BINDINGS
            .iter()
            .skip(self.control_scroll)
            .take(CONTROLS_VISIBLE_ROWS);
        for (index, meta) in visible.enumerate() {
            let rect = rects[1 + index];
            let active = self.rebinding == Some(meta.action);
            draw_button_state(
                vertices,
                rect.x0,
                rect.x1,
                rect.y0,
                rect.y1,
                rect.contains(x, y),
                active,
            );
            let value = if active {
                self.tr("menu.press_a_key").to_string()
            } else {
                key_name(self.control(meta.action)).to_string()
            };
            let action_label = control_label(&self.catalog, meta.action);
            draw_centered_text_in(
                vertices,
                &self.catalog.format_lookup(
                    "menu.control_value",
                    &[
                        ("action", action_label),
                        ("value", &value),
                    ],
                ),
                rect.x0,
                rect.x1,
                rect.y1 - 0.098,
                0.0065,
                aspect,
                [1.0; 4],
                &self.font_source,
            );
        }
        let done = rects.last().copied().unwrap_or(CONTROLS_DONE.rect);
        draw_button(
            vertices,
            done.x0,
            done.x1,
            done.y0,
            done.y1,
            done.contains(x, y),
        );
        draw_centered_text(
            vertices,
            &self.tr("menu.done"),
            -0.738,
            0.008,
            aspect,
            [1.0; 4],
            &self.font_source,
        );
    }

    fn control(&self, action: ControlAction) -> KeyCode {
        let meta = CONTROL_BINDINGS
            .iter()
            .find(|meta| meta.action == action)
            .expect("control action must be in CONTROL_BINDINGS");
        (meta.getter)(&self.settings.controls)
    }

    fn draw_delete_confirmation(&self, vertices: &mut Vec<UiVertex>, aspect: f32) {
        draw_rect(vertices, -1.0, 1.0, -1.0, 1.0, [0.0, 0.0, 0.0, 0.62]);
        panel(vertices, -0.58, 0.58, -0.32, 0.34);
        draw_centered_text(
            vertices,
            &self.tr("menu.delete_world"),
            0.20,
            0.011,
            aspect,
            [1.0, 0.45, 0.35, 1.0],
            &self.font_source,
        );
        draw_centered_text(
            vertices,
            &self.tr("menu.delete_warning"),
            0.08,
            0.0065,
            aspect,
            [0.85, 0.85, 0.85, 1.0],
            &self.font_source,
        );
        let [x, y] = self.mouse_ndc;
        let [del_rect, cancel_rect] = CONFIRM_DELETE_RECTS;
        draw_button(
            vertices,
            del_rect.x0,
            del_rect.x1,
            del_rect.y0,
            del_rect.y1,
            del_rect.contains(x, y),
        );
        draw_button(
            vertices,
            cancel_rect.x0,
            cancel_rect.x1,
            cancel_rect.y0,
            cancel_rect.y1,
            cancel_rect.contains(x, y),
        );
        draw_centered_text_in(
            vertices,
            &self.tr("menu.delete"),
            del_rect.x0,
            del_rect.x1,
            -0.118,
            0.007,
            aspect,
            [1.0; 4],
            &self.font_source,
        );
        draw_centered_text_in(
            vertices,
            &self.tr("menu.cancel"),
            cancel_rect.x0,
            cancel_rect.x1,
            -0.118,
            0.007,
            aspect,
            [1.0; 4],
            &self.font_source,
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
    crate::presentation::bootstrap::choose_present_mode(vsync, modes)
}

fn hash_seed(value: &str) -> u32 {
    value.bytes().fold(2_166_136_261, |hash, byte| {
        (hash ^ byte as u32).wrapping_mul(16_777_619)
    })
}

fn split_host_port(target: &str) -> Option<(String, String)> {
    let target = target.trim();
    if let Some(rest) = target.strip_prefix('[') {
        let (host, port) = rest.split_once("]:")?;
        if host.is_empty() || port.parse::<u16>().ok().filter(|port| *port > 0).is_none() {
            return None;
        }
        return Some((host.to_string(), port.to_string()));
    }
    let (host, port) = target.rsplit_once(':')?;
    if host.is_empty()
        || host.contains(':')
        || port.parse::<u16>().ok().filter(|port| *port > 0).is_none()
    {
        return None;
    }
    Some((host.to_string(), port.to_string()))
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
    (value.clamp(0.0, 1.0) * 100.0).round() as u32
}

fn options_row_at(y: f32) -> Option<usize> {
    OPTIONS_ROW_TOPS
        .iter()
        .position(|top| y <= *top && y >= *top - 0.13)
}

fn accessibility_label(
    catalog: &TranslationCatalog,
    row: crate::accessibility::AccessibilityRow,
) -> String {
    let key = match row {
        crate::accessibility::AccessibilityRow::UiScale => "menu.ui_scale",
        crate::accessibility::AccessibilityRow::ChatScale => "menu.chat_scale",
        crate::accessibility::AccessibilityRow::ChatOpacity => "menu.chat_opacity",
        crate::accessibility::AccessibilityRow::Subtitles => "menu.subtitles",
        crate::accessibility::AccessibilityRow::HighContrast => "menu.high_contrast",
        crate::accessibility::AccessibilityRow::ReduceFlashing => "menu.reduce_flashing",
        crate::accessibility::AccessibilityRow::ToggleSprint => "menu.toggle_sprint",
        crate::accessibility::AccessibilityRow::ToggleSneak => "menu.toggle_sneak",
        crate::accessibility::AccessibilityRow::CameraBobbing => "menu.camera_bobbing",
        crate::accessibility::AccessibilityRow::DamageTilt => "menu.damage_tilt",
    };
    catalog.lookup(key).to_string()
}

fn control_label(catalog: &TranslationCatalog, action: ControlAction) -> &str {
    let meta = CONTROL_BINDINGS
        .iter()
        .find(|meta| meta.action == action)
        .expect("control action must be in CONTROL_BINDINGS");
    catalog.lookup(meta.label_key)
}

fn hit(x: f32, y: f32, x0: f32, x1: f32, y0: f32, y1: f32) -> bool {
    MenuRect::new(x0, x1, y0, y1).contains(x, y)
}

fn draw_rect(vertices: &mut Vec<UiVertex>, x0: f32, x1: f32, y0: f32, y1: f32, color: [f32; 4]) {
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

fn draw_focus_ring(vertices: &mut Vec<UiVertex>, x0: f32, x1: f32, y0: f32, y1: f32) {
    let color = [1.0, 0.78, 0.18, 1.0];
    let thickness = 0.008;
    draw_rect(
        vertices,
        x0 - thickness,
        x1 + thickness,
        y1,
        y1 + thickness,
        color,
    );
    draw_rect(
        vertices,
        x0 - thickness,
        x1 + thickness,
        y0 - thickness,
        y0,
        color,
    );
    draw_rect(vertices, x0 - thickness, x0, y0, y1, color);
    draw_rect(vertices, x1, x1 + thickness, y0, y1, color);
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
    font: &FontSource,
) {
    draw_text(
        vertices,
        label,
        x0,
        y1 + 0.035,
        0.006,
        aspect,
        [0.8, 0.82, 0.84, 1.0],
        font,
    );
    draw_button_state(vertices, x0, x1, y0, y1, false, active);
    draw_centered_text_in(
        vertices,
        value,
        x0,
        x1,
        y0 + 0.038,
        0.008,
        aspect,
        [1.0; 4],
        font,
    );
}

fn draw_logo(vertices: &mut Vec<UiVertex>, aspect: f32, font: &FontSource) {
    draw_centered_text(
        vertices,
        "ICRAFT",
        0.505,
        0.026,
        aspect,
        [0.04, 0.045, 0.04, 1.0],
        font,
    );
    draw_centered_text(
        vertices,
        "ICRAFT",
        0.53,
        0.026,
        aspect,
        [0.72, 0.75, 0.70, 1.0],
        font,
    );
    draw_centered_text(
        vertices,
        "RUST EDITION",
        0.43,
        0.007,
        aspect,
        [1.0, 0.83, 0.18, 1.0],
        font,
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
    font: &FontSource,
) {
    let x = -text_width(text, pixel, aspect) * 0.5;
    draw_text(vertices, text, x, y, pixel, aspect, color, font);
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
    font: &FontSource,
) {
    let x = (x0 + x1 - text_width(text, pixel, aspect)) * 0.5;
    draw_text(vertices, text, x, y, pixel, aspect, color, font);
}

fn draw_text(
    vertices: &mut Vec<UiVertex>,
    text: &str,
    x: f32,
    y: f32,
    pixel: f32,
    aspect: f32,
    color: [f32; 4],
    font: &FontSource,
) {
    draw_text_with_font(vertices, text, x, y, pixel, aspect, color, font);
}

pub(crate) use crate::glyph_atlas::glyph;

fn draw_text_with_font(
    vertices: &mut Vec<UiVertex>,
    text: &str,
    x: f32,
    y: f32,
    pixel: f32,
    aspect: f32,
    color: [f32; 4],
    font: &FontSource,
) {
    // Solid-color menu path expands glyphs, honoring pack bitmap overrides.
    let pixel_x = pixel * aspect;
    let mut cursor = x;
    for ch in text.to_ascii_uppercase().chars() {
        let rows = font.glyph_override(ch).unwrap_or_else(|| glyph(ch));
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

fn draw_text_with_font_textured(
    vertices: &mut Vec<crate::state::TexturedUiVertex>,
    text: &str,
    x: f32,
    y: f32,
    pixel: f32,
    aspect: f32,
    color: [f32; 4],
    font: &FontSource,
) {
    let _ = font;
    let pixel_x = pixel * aspect;
    let char_w = pixel_x * 5.0;
    let char_h = pixel * 7.0;
    let mut cursor = x;
    for ch in text.to_ascii_uppercase().chars() {
        crate::glyph_atlas::push_glyph_quad(
            vertices,
            cursor,
            y,
            cursor + char_w * 0.88,
            y + char_h * 0.88,
            ch,
            color,
            |position, tex_coords, color| crate::state::TexturedUiVertex {
                position,
                tex_coords,
                color,
            },
        );
        cursor += pixel_x * 6.0;
    }
}


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
#[path = "tests.rs"]
mod tests;
