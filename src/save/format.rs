use crate::dimension::Dimension;
use crate::inventory::{CreativeDragOrigin, GameMode, Inventory, Item, ItemStack};
use crate::network::protocol::PlayerEffectWire;
use crate::world::{BlockType, Chunk};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::region::{compress_bytes, decompress_bytes_limited};

pub const PLAYER_SAVE_MAGIC: &[u8; 8] = b"ICRPLR01";
pub const PLAYER_SAVE_VERSION: u16 = 1;
/// Version of the dedicated-runtime per-player file. Version 2 adds the
/// current dimension alongside the existing spawn dimension in `PlayerData`.
pub const DEDICATED_PLAYER_SAVE_VERSION: u16 = 2;
/// Handshake, whitelist, operators, and `players/<id>.dat` all share this
/// bound. It is a strict subset of the historical 32-byte login cap.
pub const PLAYER_IDENTITY_MAX_LEN: usize = 16;
pub const WORLD_META_FILE: &str = "world.meta";

/// Version of the chunk payload emitted by current saves. Versions 0–2 are
/// still accepted by `restore_to_chunk`; the new version only records that
/// hopper/dispenser/dropper/observer state is included in every save path.
pub const CHUNK_SAVE_DATA_VERSION: u32 = 3;

/// Documented column height for `data_version == 0` (pre-signed-Y) saves.
pub const LEGACY_CHUNK_HEIGHT: usize = 256;
pub const LEGACY_VOXEL_COUNT: usize = 16 * LEGACY_CHUNK_HEIGHT * 16;

/// Sidecar streams (redstone / block entities) have no voxel length. Cap them
/// at the pack-entry budget so a hostile zlib stream cannot grow without limit.
pub const SAVE_SIDECAR_INFLATE_MAX: usize = 8 * 1024 * 1024;

/// Creation-time fields from `world.meta`, with a level.dat fallback for
/// legacy worlds. Lives here so the dedicated server can read them without
/// compiling the wgpu menu.
pub fn load_world_creation_options(world_dir: &Path) -> crate::game_rules::WorldCreationOptions {
    load_world_creation_options_from_meta(world_dir)
        .or_else(|| load_world_creation_options_from_level(world_dir))
        .unwrap_or_default()
}

pub(crate) fn parse_meta_bool(value: &str, fallback: bool) -> bool {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "on" => true,
        "false" | "0" | "off" => false,
        _ => fallback,
    }
}

pub(crate) fn parse_meta_game_mode(value: &str) -> GameMode {
    match value.trim().to_ascii_lowercase().as_str() {
        "creative" => GameMode::Creative,
        "adventure" => GameMode::Adventure,
        "spectator" => GameMode::Spectator,
        _ => GameMode::Survival,
    }
}

pub(crate) fn load_world_creation_options_from_meta(
    world_dir: &Path,
) -> Option<crate::game_rules::WorldCreationOptions> {
    let contents = fs::read_to_string(world_dir.join(WORLD_META_FILE)).ok()?;
    let mut options = crate::game_rules::WorldCreationOptions::default();
    let mut saw_name = false;
    for line in contents.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        match key.trim() {
            "name" if !value.trim().is_empty() => saw_name = true,
            "world_type" => options.world_type = crate::game_rules::WorldType::parse(value),
            "generate_structures" => {
                options.generate_structures = parse_meta_bool(value, options.generate_structures)
            }
            "bonus_chest" => options.bonus_chest = parse_meta_bool(value, options.bonus_chest),
            "cheats" | "cheats_enabled" => {
                options.cheats_enabled = parse_meta_bool(value, options.cheats_enabled)
            }
            "hardcore" => options.hardcore = parse_meta_bool(value, options.hardcore),
            "game_mode" => options.game_mode = parse_meta_game_mode(value),
            _ => {}
        }
    }
    saw_name.then_some(options)
}

pub(crate) fn load_world_creation_options_from_level(
    world_dir: &Path,
) -> Option<crate::game_rules::WorldCreationOptions> {
    if !world_dir.join("level.dat").is_file() || !world_dir.join("player.dat").is_file() {
        return None;
    }
    let (level, player) = super::player::load_player_and_level(world_dir).ok()?;
    Some(crate::game_rules::WorldCreationOptions {
        world_type: level.world_type,
        generate_structures: level.generate_structures,
        bonus_chest: level.bonus_chest,
        cheats_enabled: level.cheats_enabled,
        hardcore: level.hardcore || level.rules.hardcore,
        game_mode: player.game_mode,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MutationRevisionIndexCapacityError {
    pub capacity: usize,
}

impl std::fmt::Display for MutationRevisionIndexCapacityError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "mutation revision index capacity {} reached",
            self.capacity
        )
    }
}

impl std::error::Error for MutationRevisionIndexCapacityError {}

/// Why a raw login name was rejected instead of being rewritten.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentityError {
    Empty,
    TooLong,
    InvalidCharset,
    ReservedStem,
}

impl std::fmt::Display for IdentityError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(formatter, "player identity must not be empty"),
            Self::TooLong => write!(
                formatter,
                "player identity must be at most {PLAYER_IDENTITY_MAX_LEN} characters"
            ),
            Self::InvalidCharset => write!(
                formatter,
                "player identity must already be ASCII [A-Za-z0-9_-] after lowercasing"
            ),
            Self::ReservedStem => write!(
                formatter,
                "player identity must not use a Windows reserved device name"
            ),
        }
    }
}

impl std::error::Error for IdentityError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveError {
    Io {
        operation: &'static str,
        path: PathBuf,
        message: String,
    },
    Serialization(String),
    RegionCorruption {
        path: PathBuf,
        chunk_x: i32,
        chunk_z: i32,
        message: String,
    },
    QueueClosed,
    WorkerPanic(String),
}

impl SaveError {
    pub fn io(
        operation: &'static str,
        path: impl Into<PathBuf>,
        error: impl std::fmt::Display,
    ) -> Self {
        Self::Io {
            operation,
            path: path.into(),
            message: error.to_string(),
        }
    }
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io {
                operation,
                path,
                message,
            } => write!(f, "{operation} failed for {}: {message}", path.display()),
            Self::Serialization(message) => write!(f, "save serialization failed: {message}"),
            Self::RegionCorruption {
                path,
                chunk_x,
                chunk_z,
                message,
            } => write!(
                f,
                "region corruption at {} while saving chunk ({chunk_x}, {chunk_z}): {message}",
                path.display()
            ),
            Self::QueueClosed => write!(f, "save worker queue is closed"),
            Self::WorkerPanic(message) => write!(f, "save worker panicked: {message}"),
        }
    }
}

impl std::error::Error for SaveError {}

pub type SaveResult<T> = Result<T, SaveError>;

pub(crate) fn default_spawn_x() -> i32 {
    8
}
pub(crate) fn default_spawn_y() -> i32 {
    80
}
pub(crate) fn default_spawn_z() -> i32 {
    8
}

pub(crate) fn default_true_bool() -> bool {
    true
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LevelData {
    pub seed: u32,
    pub time: u64,
    #[serde(default = "default_spawn_x")]
    pub spawn_x: i32,
    #[serde(default = "default_spawn_y")]
    pub spawn_y: i32,
    #[serde(default = "default_spawn_z")]
    pub spawn_z: i32,
    #[serde(default)]
    pub spawn_dimension: Dimension,
    #[serde(default)]
    pub spawn_yaw: f32,
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub rules: crate::game_rules::WorldRules,
    #[serde(default)]
    pub world_type: crate::game_rules::WorldType,
    #[serde(default = "default_true_bool")]
    pub generate_structures: bool,
    #[serde(default)]
    pub bonus_chest: bool,
    #[serde(default)]
    pub cheats_enabled: bool,
    #[serde(default)]
    pub hardcore: bool,
}

/// The level payload written before Plan 15 added creation options and rules.
/// Bincode does not apply serde defaults to fields that are absent from the
/// byte stream, so old worlds need an explicit prefix migration on load.
#[derive(Serialize, Deserialize)]
pub(crate) struct LegacyLevelData {
    pub seed: u32,
    pub time: u64,
    #[serde(default = "default_spawn_x")]
    pub spawn_x: i32,
    #[serde(default = "default_spawn_y")]
    pub spawn_y: i32,
    #[serde(default = "default_spawn_z")]
    pub spawn_z: i32,
    #[serde(default)]
    pub spawn_dimension: Dimension,
    #[serde(default)]
    pub spawn_yaw: f32,
    #[serde(default)]
    pub version: u32,
}

impl From<LegacyLevelData> for LevelData {
    fn from(legacy: LegacyLevelData) -> Self {
        Self {
            seed: legacy.seed,
            time: legacy.time,
            spawn_x: legacy.spawn_x,
            spawn_y: legacy.spawn_y,
            spawn_z: legacy.spawn_z,
            spawn_dimension: legacy.spawn_dimension,
            spawn_yaw: legacy.spawn_yaw,
            version: legacy.version,
            ..Self::default()
        }
    }
}

impl Default for LevelData {
    fn default() -> Self {
        Self {
            seed: 0,
            time: 0,
            spawn_x: default_spawn_x(),
            spawn_y: default_spawn_y(),
            spawn_z: default_spawn_z(),
            spawn_dimension: Dimension::Overworld,
            spawn_yaw: 0.0,
            version: 0,
            rules: Default::default(),
            world_type: Default::default(),
            generate_structures: true,
            bonus_chest: false,
            cheats_enabled: false,
            hardcore: false,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ItemStackData {
    pub item: Item,
    pub count: u32,
    pub durability: u32,
    pub enchantments: crate::enchantment::EnchantmentSet,
    pub potion: Option<crate::brewing::PotionData>,
    pub custom_name: crate::enchantment::ItemName,
    #[serde(default)]
    pub can_break: u128,
    #[serde(default)]
    pub can_place_on: u128,
}

impl ItemStackData {
    pub fn to_item_stack(&self) -> ItemStack {
        ItemStack {
            item: self.item,
            count: self.count,
            durability: self.durability,
            enchantments: self.enchantments,
            potion: self.potion,
            custom_name: self.custom_name,
            can_break: self.can_break,
            can_place_on: self.can_place_on,
        }
    }
}

impl From<&ItemStack> for ItemStackData {
    fn from(stack: &ItemStack) -> Self {
        Self {
            item: stack.item,
            count: stack.count,
            durability: stack.durability,
            enchantments: stack.enchantments,
            potion: stack.potion,
            custom_name: stack.custom_name,
            can_break: stack.can_break,
            can_place_on: stack.can_place_on,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct InventoryData {
    pub hotbar: Vec<Option<ItemStackData>>,
    pub main: Vec<Option<ItemStackData>>,
    pub armor: Vec<Option<ItemStackData>>,
    #[serde(default)]
    pub offhand: Option<ItemStackData>,
    pub selected: usize,
    pub dragged: Option<ItemStackData>,
    pub creative_drag_origin: Option<CreativeDragOrigin>,
}

pub(crate) fn default_collar_color() -> [f32; 3] {
    [0.8, 0.2, 0.2]
}

pub(crate) fn default_slime_size() -> u8 {
    1
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EntitySaveData {
    pub entity_type: crate::entity::EntityType,
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub health: f32,
    pub max_health: f32,
    pub is_ignited: bool,
    pub burn_timer: f32,
    pub age: f32,
    pub breeding_timer: f32,
    pub breed_cooldown: f32,
    pub has_wool: bool,
    pub wool_color: [f32; 3],
    pub dropped_item: Option<crate::inventory::Item>,
    pub dropped_count: u32,
    #[serde(default)]
    pub dropped_stack: Option<ItemStackData>,
    #[serde(default)]
    pub item_age: f32,
    #[serde(default)]
    pub xp_value: u32,
    #[serde(default)]
    pub owner_id: Option<u64>,
    #[serde(default)]
    pub owner_uuid: Option<String>,
    #[serde(default)]
    pub is_tamed: bool,
    #[serde(default)]
    pub is_sitting: bool,
    #[serde(default = "default_collar_color")]
    pub collar_color: [f32; 3],
    #[serde(default = "default_slime_size")]
    pub slime_size: u8,
    #[serde(default)]
    pub is_persistent: bool,
    #[serde(default)]
    pub profession: Option<crate::village::poi::VillagerProfession>,
    #[serde(default)]
    pub villager_level: Option<crate::village::trade::VillagerLevel>,
    #[serde(default)]
    pub villager_xp: u32,
    #[serde(default)]
    pub offers: Vec<crate::village::trade::TradeOffer>,
    #[serde(default)]
    pub home_poi: Option<(i32, i32, i32)>,
    #[serde(default)]
    pub job_poi: Option<(i32, i32, i32)>,
    #[serde(default)]
    pub meeting_poi: Option<(i32, i32, i32)>,
    #[serde(default)]
    pub restock_count_today: u8,
    #[serde(default)]
    pub last_restock_tick: u64,
    #[serde(default)]
    pub food_count: u32,
    #[serde(default)]
    pub is_raid_captain: bool,
    #[serde(default)]
    pub has_saddle: bool,
}

impl From<&crate::entity::Entity> for EntitySaveData {
    fn from(entity: &crate::entity::Entity) -> Self {
        Self {
            entity_type: entity.entity_type,
            position: [entity.position.x, entity.position.y, entity.position.z],
            velocity: [entity.velocity.x, entity.velocity.y, entity.velocity.z],
            yaw: entity.yaw,
            pitch: entity.pitch,
            health: entity.health,
            max_health: entity.max_health,
            is_ignited: entity.is_ignited,
            burn_timer: entity.burn_timer,
            age: entity.age,
            breeding_timer: entity.breeding_timer,
            breed_cooldown: entity.breed_cooldown,
            has_wool: entity.has_wool,
            wool_color: entity.wool_color,
            dropped_item: entity.dropped_item,
            dropped_count: entity.dropped_count,
            dropped_stack: entity.dropped_stack.as_ref().map(ItemStackData::from),
            item_age: entity.item_age,
            xp_value: entity.xp_value,
            owner_id: entity.owner_id,
            owner_uuid: entity.owner_uuid.clone(),
            is_tamed: entity.is_tamed,
            is_sitting: entity.is_sitting,
            collar_color: entity.collar_color,
            slime_size: entity.slime_size,
            is_persistent: entity.is_persistent,
            profession: Some(entity.profession),
            villager_level: Some(entity.villager_level),
            villager_xp: entity.villager_xp,
            offers: entity.offers.clone(),
            home_poi: entity.home_poi,
            job_poi: entity.job_poi,
            meeting_poi: entity.meeting_poi,
            restock_count_today: entity.restock_count_today,
            last_restock_tick: entity.last_restock_tick,
            food_count: entity.food_count,
            is_raid_captain: entity.is_raid_captain,
            has_saddle: entity.has_saddle,
        }
    }
}

impl EntitySaveData {
    pub fn to_entity(&self, id: u64) -> crate::entity::Entity {
        let pos = glam::Vec3::new(self.position[0], self.position[1], self.position[2]);
        let mut entity = crate::entity::Entity::new(id, self.entity_type, pos);
        entity.velocity = glam::Vec3::new(self.velocity[0], self.velocity[1], self.velocity[2]);
        entity.yaw = self.yaw;
        entity.pitch = self.pitch;
        entity.health = self.health;
        entity.max_health = self.max_health;
        entity.is_ignited = self.is_ignited;
        entity.burn_timer = self.burn_timer;
        entity.age = self.age;
        entity.breeding_timer = self.breeding_timer;
        entity.breed_cooldown = self.breed_cooldown;
        entity.has_wool = self.has_wool;
        entity.wool_color = self.wool_color;
        entity.dropped_item = self.dropped_item;
        entity.dropped_count = self.dropped_count;
        entity.dropped_stack = self.dropped_stack.as_ref().map(|s| s.to_item_stack());
        if entity.dropped_stack.is_none() && entity.dropped_item.is_some() {
            let item = entity.dropped_item.unwrap();
            entity.dropped_stack = Some(crate::inventory::ItemStack::new(
                item,
                entity.dropped_count.max(1),
            ));
        }
        entity.item_age = self.item_age;
        entity.xp_value = self.xp_value;
        entity.owner_id = self.owner_id;
        entity.owner_uuid = self.owner_uuid.clone();
        entity.is_tamed = self.is_tamed;
        entity.is_sitting = self.is_sitting;
        entity.collar_color = self.collar_color;
        entity.slime_size = self.slime_size;
        entity.is_persistent = self.is_persistent;
        if let Some(prof) = self.profession {
            entity.profession = prof;
        }
        if let Some(lvl) = self.villager_level {
            entity.villager_level = lvl;
        }
        entity.villager_xp = self.villager_xp;
        entity.offers = self.offers.clone();
        entity.home_poi = self.home_poi;
        entity.job_poi = self.job_poi;
        entity.meeting_poi = self.meeting_poi;
        entity.restock_count_today = self.restock_count_today;
        entity.last_restock_tick = self.last_restock_tick;
        entity.food_count = self.food_count;
        entity.is_raid_captain = self.is_raid_captain;
        entity.has_saddle = self.has_saddle;
        entity
    }

    pub fn should_persist(&self) -> bool {
        self.entity_type.is_living()
            || self.entity_type.is_persistent()
            || self.entity_type == crate::entity::EntityType::DroppedItem
            || self.entity_type == crate::entity::EntityType::ExperienceOrb
            || self.entity_type == crate::entity::EntityType::Boat
            || self.entity_type == crate::entity::EntityType::Minecart
    }
}

impl From<&Inventory> for InventoryData {
    fn from(inv: &Inventory) -> Self {
        let (dragged, creative_drag_origin) = match inv.creative_drag_origin {
            Some(CreativeDragOrigin::Catalog) => (None, None),
            Some(CreativeDragOrigin::Inventory) => (
                inv.dragged.as_ref().map(ItemStackData::from),
                inv.dragged.map(|_| CreativeDragOrigin::Inventory),
            ),
            None => (inv.dragged.as_ref().map(ItemStackData::from), None),
        };
        Self {
            hotbar: inv
                .hotbar
                .iter()
                .map(|o| o.as_ref().map(|s| ItemStackData::from(s)))
                .collect(),
            main: inv
                .main
                .iter()
                .map(|o| o.as_ref().map(|s| ItemStackData::from(s)))
                .collect(),
            armor: inv
                .armor
                .iter()
                .map(|o| o.as_ref().map(|s| ItemStackData::from(s)))
                .collect(),
            offhand: inv.offhand.as_ref().map(ItemStackData::from),
            selected: inv.selected,
            dragged,
            creative_drag_origin,
        }
    }
}

impl InventoryData {
    pub fn to_inventory(&self) -> Inventory {
        let mut inv = Inventory::new();
        for (i, opt) in self.hotbar.iter().enumerate() {
            if i < inv.hotbar.len() {
                inv.hotbar[i] = opt.as_ref().map(|s| s.to_item_stack());
            }
        }
        for (i, opt) in self.main.iter().enumerate() {
            if i < inv.main.len() {
                inv.main[i] = opt.as_ref().map(|s| s.to_item_stack());
            }
        }
        for (i, opt) in self.armor.iter().enumerate() {
            if i < inv.armor.len() {
                inv.armor[i] = opt.as_ref().map(|s| s.to_item_stack());
            }
        }
        inv.offhand = self.offhand.as_ref().map(|s| s.to_item_stack());
        inv.selected = self.selected;
        match self.creative_drag_origin {
            Some(CreativeDragOrigin::Catalog) => {}
            Some(CreativeDragOrigin::Inventory) => {
                inv.dragged = self.dragged.as_ref().map(ItemStackData::to_item_stack);
                inv.creative_drag_origin = inv.dragged.map(|_| CreativeDragOrigin::Inventory);
            }
            None => {
                inv.dragged = self.dragged.as_ref().map(ItemStackData::to_item_stack);
            }
        }
        inv
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PlayerData {
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub health: f32,
    pub hunger: f32,
    pub saturation: f32,
    pub exhaustion: f32,
    pub oxygen: f32,
    pub experience: u32,
    pub experience_level: u32,
    pub game_mode: GameMode,
    #[serde(default)]
    pub is_dead: bool,
    pub inventory: InventoryData,
    #[serde(default)]
    pub advancements: crate::advancements::AdvancementProgressData,
    #[serde(default)]
    pub spawn_point: Option<[i32; 3]>,
    #[serde(default)]
    pub spawn_dimension: Option<Dimension>,
    #[serde(default)]
    pub unlocked_recipes: std::collections::HashSet<String>,
    #[serde(default)]
    pub bad_omen_level: u8,
    #[serde(default)]
    pub hero_of_the_village_timer: f32,
}

impl PlayerData {
    pub fn from_state(
        position: glam::Vec3,
        velocity: glam::Vec3,
        yaw: f32,
        pitch: f32,
        state: &crate::player::PlayerState,
        game_mode: GameMode,
        inventory: &Inventory,
        advancements: crate::advancements::AdvancementProgressData,
    ) -> Self {
        Self {
            position: [position.x, position.y, position.z],
            velocity: [velocity.x, velocity.y, velocity.z],
            yaw,
            pitch,
            health: state.health,
            hunger: state.hunger,
            saturation: state.saturation,
            exhaustion: state.exhaustion,
            oxygen: state.oxygen,
            experience: state.experience,
            experience_level: state.experience_level,
            game_mode,
            is_dead: state.is_dead,
            inventory: InventoryData::from(inventory),
            advancements,
            spawn_point: state.spawn_point,
            spawn_dimension: state.spawn_dimension,
            unlocked_recipes: state.unlocked_recipes.clone(),
            bad_omen_level: state.bad_omen_level,
            hero_of_the_village_timer: state.hero_of_the_village_timer,
        }
    }
}

/// Dedicated-runtime player payload. The current dimension is deliberately
/// outside `PlayerData`: `spawn_dimension` describes the respawn anchor and
/// must not be overwritten when a player logs out in another dimension.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DedicatedPlayerFile {
    pub version: u16,
    pub current_dimension: Dimension,
    pub data: PlayerData,
    #[serde(default)]
    pub effects: Vec<PlayerEffectWire>,
}

#[derive(Deserialize)]
pub(crate) struct LegacyDedicatedPlayerFile {
    pub version: u16,
    pub data: PlayerData,
    #[serde(default)]
    pub effects: Vec<PlayerEffectWire>,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct PlayerSaveEnvelope {
    pub version: u16,
    pub player: PlayerData,
}

pub fn serialize_player_data(player: &PlayerData) -> bincode::Result<Vec<u8>> {
    let envelope = PlayerSaveEnvelope {
        version: PLAYER_SAVE_VERSION,
        player: player.clone(),
    };
    let payload = bincode::serialize(&envelope)?;
    let mut encoded = Vec::with_capacity(PLAYER_SAVE_MAGIC.len() + payload.len());
    encoded.extend_from_slice(PLAYER_SAVE_MAGIC);
    encoded.extend_from_slice(&payload);
    Ok(encoded)
}

pub fn deserialize_player_data(bytes: &[u8]) -> bincode::Result<PlayerData> {
    if let Some(payload) = bytes.strip_prefix(PLAYER_SAVE_MAGIC) {
        let envelope: PlayerSaveEnvelope = bincode::deserialize(payload)?;
        if envelope.version != PLAYER_SAVE_VERSION {
            return Err(Box::new(bincode::ErrorKind::Custom(format!(
                "unsupported player save version {}",
                envelope.version
            ))));
        }
        return Ok(envelope.player);
    }

    match bincode::deserialize::<PreviousPlayerData>(bytes) {
        Ok(previous) => Ok(previous.into()),
        Err(previous_error) => bincode::deserialize::<LegacyPlayerData>(bytes)
            .map(PlayerData::from)
            .map_err(|_| previous_error),
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ChunkSaveData {
    pub chunk_x: i32,
    pub chunk_z: i32,
    pub blocks: Vec<u8>,       // Zlib compressed u8 array of BlockType
    pub sky_light: Vec<u8>,    // Zlib compressed u8 array of sky light
    pub block_light: Vec<u8>,  // Zlib compressed u8 array of block light
    pub fluid_levels: Vec<u8>, // Zlib compressed u8 array of fluid levels
    /// Zlib-compressed bincode of `Vec<RedstoneComponentMetadata>`. Older saves
    /// written before this sidecar existed deserialize as an empty vector; the
    /// immediately previous sidecar shape (without `last_powered`) is decoded
    /// explicitly below so bincode's fixed struct layout cannot discard it.
    #[serde(default)]
    pub redstone_metadata: Vec<u8>,
    #[serde(default)]
    pub block_states: Vec<u8>,
    #[serde(default)]
    pub mutation_revision: u64,
    #[serde(default)]
    pub block_entities: Vec<u8>,
    #[serde(default)]
    pub data_version: u32,
}

pub(crate) fn destination_voxel_count(chunk: &Chunk) -> usize {
    chunk.sections.len() * 16 * 16 * 16
}

/// Expected inflated voxel-column size from the save `data_version` and the
/// destination section span. Version 0 is the documented 256-high column.
pub(crate) fn expected_voxel_len(data_version: u32, section_count: usize) -> usize {
    if data_version == 0 {
        LEGACY_VOXEL_COUNT
    } else {
        section_count.saturating_mul(16 * 16 * 16)
    }
}

/// Tight inflate budget for a voxel stream: the versioned column, the
/// destination span, or the documented 256-high overlay — never unbounded.
pub(crate) fn voxel_inflate_limit(data_version: u32, chunk: &Chunk) -> usize {
    expected_voxel_len(data_version, chunk.sections.len())
        .max(destination_voxel_count(chunk))
        .max(LEGACY_VOXEL_COUNT)
}

pub(crate) fn voxel_count_matches_save(len: usize, _data_version: u32, chunk: &Chunk) -> bool {
    let dest = destination_voxel_count(chunk);
    // Destination dimension height, or the documented 256-high column used by
    // pre-signed-Y saves and `UncompressedChunkSnapshot` flattening.
    len == dest || len == LEGACY_VOXEL_COUNT
}

pub(crate) fn decode_required_voxel_stream(
    data: &[u8],
    name: &'static str,
    data_version: u32,
    chunk: &Chunk,
) -> io::Result<Vec<u8>> {
    if data.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{name} stream is empty"),
        ));
    }
    let bytes = decompress_bytes_limited(data, voxel_inflate_limit(data_version, chunk)).map_err(
        |error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{name} inflate failed: {error}"),
            )
        },
    )?;
    if bytes.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{name} stream inflated to empty"),
        ));
    }
    if !voxel_count_matches_save(bytes.len(), data_version, chunk) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "{name} length {} does not match data_version {data_version} (expected {} or 256-high {LEGACY_VOXEL_COUNT})",
                bytes.len(),
                destination_voxel_count(chunk)
            ),
        ));
    }
    Ok(bytes)
}

pub(crate) fn decode_optional_voxel_stream(
    data: &[u8],
    name: &'static str,
    expected_len: usize,
) -> io::Result<Vec<u8>> {
    if data.is_empty() {
        return Ok(Vec::new());
    }
    let bytes = decompress_bytes_limited(data, expected_len).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{name} inflate failed: {error}"),
        )
    })?;
    if bytes.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{name} stream inflated to empty"),
        ));
    }
    if bytes.len() != expected_len {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "{name} length {} does not match blocks length {expected_len}",
                bytes.len()
            ),
        ));
    }
    Ok(bytes)
}

/// The Plan14/Plan27 sidecar shape before the rising-edge latch was added.
/// `serde(default)` is not sufficient for bincode: unlike self-describing
/// formats, bincode will not synthesize a missing struct tail. Keep this
/// private compatibility carrier until all pre-latch worlds have migrated.
#[derive(Serialize, Deserialize)]
pub(crate) struct LegacyRedstoneComponentMetadata {
    pub local_x: u8,
    pub local_y: u8,
    pub local_z: u8,
    pub facing: crate::redstone::Direction,
    pub repeater_delay: u8,
    pub comparator_mode: crate::redstone::ComparatorMode,
    pub note: u8,
}

/// Sidecar shape after the latch was added but while Y was still `u8`.
/// Old payloads treated Y as 0..256 world Y; 236 stays 236, never -20.
#[derive(Serialize, Deserialize)]
pub(crate) struct LegacyU8YRedstoneComponentMetadata {
    pub local_x: u8,
    pub local_y: u8,
    pub local_z: u8,
    pub facing: crate::redstone::Direction,
    pub repeater_delay: u8,
    pub comparator_mode: crate::redstone::ComparatorMode,
    pub note: u8,
    pub last_powered: bool,
}

pub(crate) fn migrate_legacy_u8_redstone_y(local_y: u8) -> i16 {
    // Historical saves stored 0..256 world Y. Do not reinterpret high values
    // as wrapped signed Y (236 must stay 236, not -20).
    local_y as i16
}

impl ChunkSaveData {
    pub fn from_chunk(chunk: &Chunk) -> io::Result<Self> {
        Self::from_chunk_with_redstone(chunk, &[])
    }

    /// Builds a `ChunkSaveData` and attaches the provided redstone component
    /// metadata sidecar. Pass an empty slice for chunks with no persisted
    /// redstone state (the historical behavior of `from_chunk`).
    pub fn from_chunk_with_redstone(
        chunk: &Chunk,
        redstone_metadata: &[crate::redstone::RedstoneComponentMetadata],
    ) -> io::Result<Self> {
        let section_count = chunk.sections.len();
        let total_height = section_count * 16;
        let min_y = chunk.min_section_y as i32 * 16;

        let mut blocks = Vec::with_capacity(16 * total_height * 16);
        let mut block_states_raw = Vec::with_capacity(16 * total_height * 16);
        let mut sky_light = Vec::with_capacity(16 * total_height * 16);
        let mut block_light = Vec::with_capacity(16 * total_height * 16);
        let mut fluid_levels = Vec::with_capacity(16 * total_height * 16);

        for x in 0..16 {
            for h in 0..total_height {
                let wy = min_y + h as i32;
                for z in 0..16 {
                    blocks.push(chunk.get_block_local(x, wy, z) as u8);
                    block_states_raw.push(chunk.get_block_state(x as i32, wy, z as i32));
                    sky_light.push(chunk.get_sky_light(x, wy, z));
                    block_light.push(chunk.get_block_light(x, wy, z));
                    fluid_levels.push(chunk.get_fluid_level(x, wy, z));
                }
            }
        }

        let redstone_metadata = if redstone_metadata.is_empty() {
            Vec::new()
        } else {
            let bytes = bincode::serialize(redstone_metadata)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
            compress_bytes(&bytes)?
        };

        let block_entities_list: Vec<((u8, i16, u8), crate::block_entity::BlockEntity)> = chunk
            .iter_block_entities()
            .map(|(pos, e)| (pos, e.clone()))
            .collect();
        let block_entities = if block_entities_list.is_empty() {
            Vec::new()
        } else {
            let bytes = bincode::serialize(&block_entities_list)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
            compress_bytes(&bytes)?
        };

        let blocks = compress_bytes(&blocks)?;
        if blocks.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "compressed blocks payload is empty",
            ));
        }

        Ok(Self {
            chunk_x: chunk.chunk_x,
            chunk_z: chunk.chunk_z,
            blocks,
            sky_light: compress_bytes(&sky_light)?,
            block_light: compress_bytes(&block_light)?,
            fluid_levels: compress_bytes(&fluid_levels)?,
            redstone_metadata,
            block_states: compress_bytes(&block_states_raw)?,
            mutation_revision: 0,
            block_entities,
            data_version: CHUNK_SAVE_DATA_VERSION,
        })
    }

    /// Decodes the redstone metadata sidecar into typed records. Returns an
    /// empty vector for older saves (no sidecar) or when decompression fails,
    /// so callers can always iterate the result without a separate error path.
    pub fn redstone_metadata(&self) -> Vec<crate::redstone::RedstoneComponentMetadata> {
        if self.redstone_metadata.is_empty() {
            return Vec::new();
        }
        decompress_bytes_limited(&self.redstone_metadata, SAVE_SIDECAR_INFLATE_MAX)
            .ok()
            .and_then(|bytes| {
                // Decode the current shape first. A legacy vector has no
                // latch byte to satisfy this shape and falls through to the
                // explicit migration below. This ordering is important:
                // bincode's shorter struct decoder accepts trailing bytes.
                if let Ok(current) =
                    bincode::deserialize::<Vec<crate::redstone::RedstoneComponentMetadata>>(&bytes)
                {
                    // Require an exact re-encoding match so a legacy vector
                    // whose next local_x happens to be 0/1 cannot be accepted
                    // after being misaligned into the new bool tail.
                    if bincode::serialize(&current).ok().as_deref() == Some(bytes.as_slice()) {
                        return Some(current);
                    }
                }
                if let Ok(legacy) =
                    bincode::deserialize::<Vec<LegacyU8YRedstoneComponentMetadata>>(&bytes)
                {
                    if bincode::serialize(&legacy).ok().as_deref() == Some(bytes.as_slice()) {
                        return Some(
                            legacy
                                .into_iter()
                                .map(|entry| crate::redstone::RedstoneComponentMetadata {
                                    local_x: entry.local_x,
                                    local_y: migrate_legacy_u8_redstone_y(entry.local_y),
                                    local_z: entry.local_z,
                                    facing: entry.facing,
                                    repeater_delay: entry.repeater_delay,
                                    comparator_mode: entry.comparator_mode,
                                    note: entry.note,
                                    last_powered: entry.last_powered,
                                })
                                .collect(),
                        );
                    }
                }
                bincode::deserialize::<Vec<LegacyRedstoneComponentMetadata>>(&bytes)
                    .ok()
                    .map(|legacy| {
                        legacy
                            .into_iter()
                            .map(|entry| crate::redstone::RedstoneComponentMetadata {
                                local_x: entry.local_x,
                                local_y: migrate_legacy_u8_redstone_y(entry.local_y),
                                local_z: entry.local_z,
                                facing: entry.facing,
                                repeater_delay: entry.repeater_delay,
                                comparator_mode: entry.comparator_mode,
                                note: entry.note,
                                last_powered: false,
                            })
                            .collect()
                    })
            })
            .unwrap_or_default()
    }

    pub fn block_entities(&self) -> Vec<((u8, i16, u8), crate::block_entity::BlockEntity)> {
        if self.block_entities.is_empty() {
            return Vec::new();
        }
        decompress_bytes_limited(&self.block_entities, SAVE_SIDECAR_INFLATE_MAX)
            .ok()
            .and_then(|bytes| {
                bincode::deserialize::<Vec<((u8, i16, u8), crate::block_entity::BlockEntity)>>(
                    &bytes,
                )
                .ok()
                .or_else(|| {
                    bincode::deserialize::<
                        Vec<((u8, i16, u8), crate::block_entity::LegacyBlockEntity)>,
                    >(&bytes)
                    .ok()
                    .map(|legacy_list| {
                        legacy_list
                            .into_iter()
                            .map(|(pos, le)| (pos, le.into()))
                            .collect()
                    })
                })
            })
            .unwrap_or_default()
    }

    pub fn block_states(&self) -> Vec<u8> {
        if self.block_states.is_empty() {
            return Vec::new();
        }
        decompress_bytes_limited(&self.block_states, SAVE_SIDECAR_INFLATE_MAX).unwrap_or_default()
    }

    /// Decode a `ChunkSaveData`-style compressed network/save payload into
    /// `chunk`. Shared by disk restore and join-client `ChunkData` insert.
    pub fn restore_network_payload(
        chunk: &mut Chunk,
        blocks: &[u8],
        block_states: &[u8],
        fluid_levels: &[u8],
        block_entities: &[u8],
    ) -> io::Result<()> {
        let save_data = ChunkSaveData {
            chunk_x: chunk.chunk_x,
            chunk_z: chunk.chunk_z,
            blocks: blocks.to_vec(),
            sky_light: Vec::new(),
            block_light: Vec::new(),
            fluid_levels: fluid_levels.to_vec(),
            redstone_metadata: Vec::new(),
            block_states: block_states.to_vec(),
            mutation_revision: 0,
            block_entities: block_entities.to_vec(),
            data_version: CHUNK_SAVE_DATA_VERSION,
        };
        save_data.restore_to_chunk(chunk)?;
        chunk.recompute_direct_column_lighting();
        Ok(())
    }

    pub fn restore_to_chunk(&self, chunk: &mut Chunk) -> io::Result<()> {
        let blocks =
            decode_required_voxel_stream(&self.blocks, "blocks", self.data_version, chunk)?;
        let block_states =
            decode_optional_voxel_stream(&self.block_states, "block_states", blocks.len())?;
        let sky_light = decode_optional_voxel_stream(&self.sky_light, "sky_light", blocks.len())?;
        let block_light =
            decode_optional_voxel_stream(&self.block_light, "block_light", blocks.len())?;
        let fluid_levels =
            decode_optional_voxel_stream(&self.fluid_levels, "fluid_levels", blocks.len())?;

        let total_voxels = blocks.len();
        let is_legacy_256 = total_voxels == LEGACY_VOXEL_COUNT;
        let total_height = if is_legacy_256 {
            256
        } else {
            total_voxels / (16 * 16)
        };
        let source_sec_count = total_height / 16;

        for sec_i in 0..source_sec_count {
            let target_sec_y = if is_legacy_256 {
                sec_i as i8
            } else {
                chunk.min_section_y + sec_i as i8
            };
            let Some(target_sec_idx) = chunk.section_index(target_sec_y) else {
                continue;
            };

            let mut sec_b = [BlockType::Air; 4096];
            let mut sec_st = [0u8; 4096];
            let mut sec_sk = [0u8; 4096];
            let mut sec_bl = [0u8; 4096];
            let mut sec_fl = [0u8; 4096];

            for ly in 0..16 {
                let h = sec_i * 16 + ly;
                for z in 0..16 {
                    for x in 0..16 {
                        let flat_idx = (x * total_height + h) * 16 + z;
                        let sec_idx = (ly << 8) | (z << 4) | x;

                        if flat_idx < blocks.len() {
                            sec_b[sec_idx] = BlockType::from_u8(blocks[flat_idx]);
                        }
                        if flat_idx < block_states.len() {
                            sec_st[sec_idx] = block_states[flat_idx];
                        }
                        if flat_idx < sky_light.len() {
                            sec_sk[sec_idx] = sky_light[flat_idx];
                        }
                        if flat_idx < block_light.len() {
                            sec_bl[sec_idx] = block_light[flat_idx];
                        }
                        if flat_idx < fluid_levels.len() {
                            sec_fl[sec_idx] = fluid_levels[flat_idx];
                        }
                    }
                }
            }

            let sec = crate::world::ChunkSection::from_dense(
                &sec_b,
                &sec_sk,
                &sec_bl,
                if block_states.is_empty() {
                    None
                } else {
                    Some(&sec_st)
                },
                if fluid_levels.is_empty() {
                    None
                } else {
                    Some(&sec_fl)
                },
            );
            if sec.is_empty() && sec_sk.iter().all(|&l| l == 0) && sec_bl.iter().all(|&l| l == 0) {
                chunk.sections[target_sec_idx] = None;
            } else {
                chunk.sections[target_sec_idx] = Some(sec);
            }
        }

        chunk.rebuild_torch_index();
        chunk.rebuild_redstone_index();
        chunk.rebuild_furnace_index();

        for x in 0..16 {
            for z in 0..16 {
                chunk.update_heightmap(x, z);
            }
        }

        // Restore block entities with validation: limit check, bounds check, type matching check
        chunk.block_entities.clear();
        let entities = self.block_entities();
        if entities.len() <= 4096 {
            for ((x, y, z), entity) in entities {
                let _ = chunk.insert_block_entity(x, y, z, entity);
            }
        }
        Ok(())
    }
}

/// Deserializes a `ChunkSaveData` from persisted chunk bytes with full backward
/// compatibility. Bincode 1.x does not synthesize newly-added fields from
/// `#[serde(default)]`, so every historical shape needs an explicit fallback.
pub fn deserialize_chunk_save_data(bytes: &[u8]) -> Option<ChunkSaveData> {
    if let Ok(data) = bincode::deserialize::<ChunkSaveData>(bytes) {
        return Some(data);
    }
    #[derive(serde::Deserialize)]
    struct PreviousChunkSaveData {
        chunk_x: i32,
        chunk_z: i32,
        blocks: Vec<u8>,
        sky_light: Vec<u8>,
        block_light: Vec<u8>,
        fluid_levels: Vec<u8>,
        redstone_metadata: Vec<u8>,
        block_states: Vec<u8>,
    }
    if let Ok(previous) = bincode::deserialize::<PreviousChunkSaveData>(bytes) {
        return Some(ChunkSaveData {
            chunk_x: previous.chunk_x,
            chunk_z: previous.chunk_z,
            blocks: previous.blocks,
            sky_light: previous.sky_light,
            block_light: previous.block_light,
            fluid_levels: previous.fluid_levels,
            redstone_metadata: previous.redstone_metadata,
            block_states: previous.block_states,
            mutation_revision: 0,
            block_entities: Vec::new(),
            data_version: 0,
        });
    }
    #[derive(serde::Deserialize)]
    struct LegacyChunkSaveData {
        chunk_x: i32,
        chunk_z: i32,
        blocks: Vec<u8>,
        sky_light: Vec<u8>,
        block_light: Vec<u8>,
        fluid_levels: Vec<u8>,
    }
    bincode::deserialize::<LegacyChunkSaveData>(bytes)
        .ok()
        .map(|legacy| ChunkSaveData {
            chunk_x: legacy.chunk_x,
            chunk_z: legacy.chunk_z,
            blocks: legacy.blocks,
            sky_light: legacy.sky_light,
            block_light: legacy.block_light,
            fluid_levels: legacy.fluid_levels,
            redstone_metadata: Vec::new(),
            block_states: Vec::new(),
            mutation_revision: 0,
            block_entities: Vec::new(),
            data_version: 0,
        })
}

#[derive(Serialize, Deserialize)]
pub(crate) struct PreviousInventoryData {
    pub hotbar: Vec<Option<ItemStackData>>,
    pub main: Vec<Option<ItemStackData>>,
    pub armor: Vec<Option<ItemStackData>>,
    pub selected: usize,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct PreviousPlayerData {
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub health: f32,
    pub hunger: f32,
    pub saturation: f32,
    pub exhaustion: f32,
    pub oxygen: f32,
    pub experience: u32,
    pub experience_level: u32,
    pub game_mode: GameMode,
    pub inventory: PreviousInventoryData,
    pub advancements: crate::advancements::AdvancementProgressData,
}

impl From<PreviousInventoryData> for InventoryData {
    fn from(old: PreviousInventoryData) -> Self {
        Self {
            hotbar: old.hotbar,
            main: old.main,
            armor: old.armor,
            offhand: None,
            selected: old.selected,
            dragged: None,
            creative_drag_origin: None,
        }
    }
}

impl From<PreviousPlayerData> for PlayerData {
    fn from(old: PreviousPlayerData) -> Self {
        Self {
            position: old.position,
            velocity: old.velocity,
            yaw: old.yaw,
            pitch: old.pitch,
            health: old.health,
            hunger: old.hunger,
            saturation: old.saturation,
            exhaustion: old.exhaustion,
            oxygen: old.oxygen,
            experience: old.experience,
            experience_level: old.experience_level,
            game_mode: old.game_mode,
            is_dead: false,
            inventory: old.inventory.into(),
            advancements: old.advancements,
            spawn_point: None,
            spawn_dimension: None,
            unlocked_recipes: Default::default(),
            bad_omen_level: 0,
            hero_of_the_village_timer: 0.0,
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct LegacyItemStackData {
    pub item: Item,
    pub count: u32,
    pub durability: u32,
}

#[derive(Deserialize)]
pub(crate) struct LegacyInventoryData {
    pub hotbar: Vec<Option<LegacyItemStackData>>,
    pub main: Vec<Option<LegacyItemStackData>>,
    pub armor: Vec<Option<LegacyItemStackData>>,
    pub selected: usize,
}

#[derive(Deserialize)]
pub(crate) struct LegacyPlayerData {
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub health: f32,
    pub hunger: f32,
    pub saturation: f32,
    pub exhaustion: f32,
    pub oxygen: f32,
    pub game_mode: GameMode,
    pub inventory: LegacyInventoryData,
}

impl From<LegacyItemStackData> for ItemStackData {
    fn from(old: LegacyItemStackData) -> Self {
        Self {
            item: old.item,
            count: old.count,
            durability: old.durability,
            enchantments: Default::default(),
            potion: None,
            custom_name: Default::default(),
            can_break: 0,
            can_place_on: 0,
        }
    }
}

impl From<LegacyInventoryData> for InventoryData {
    fn from(old: LegacyInventoryData) -> Self {
        let upgrade = |items: Vec<Option<LegacyItemStackData>>| {
            items
                .into_iter()
                .map(|stack| stack.map(Into::into))
                .collect()
        };
        Self {
            hotbar: upgrade(old.hotbar),
            main: upgrade(old.main),
            armor: upgrade(old.armor),
            offhand: None,
            selected: old.selected,
            dragged: None,
            creative_drag_origin: None,
        }
    }
}

impl From<LegacyPlayerData> for PlayerData {
    fn from(old: LegacyPlayerData) -> Self {
        Self {
            position: old.position,
            velocity: old.velocity,
            yaw: old.yaw,
            pitch: old.pitch,
            health: old.health,
            hunger: old.hunger,
            saturation: old.saturation,
            exhaustion: old.exhaustion,
            oxygen: old.oxygen,
            experience: 0,
            experience_level: 0,
            game_mode: old.game_mode,
            is_dead: false,
            inventory: old.inventory.into(),
            advancements: crate::advancements::AdvancementProgressData::default(),
            spawn_point: None,
            spawn_dimension: None,
            unlocked_recipes: Default::default(),
            bad_omen_level: 0,
            hero_of_the_village_timer: 0.0,
        }
    }
}
