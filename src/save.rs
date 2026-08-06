use crate::inventory::{CreativeDragOrigin, GameMode, Inventory, Item, ItemStack};
use crate::world::{BlockType, Chunk};
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};

const PLAYER_SAVE_MAGIC: &[u8; 8] = b"ICRPLR01";
const PLAYER_SAVE_VERSION: u16 = 1;
pub const SAVE_QUEUE_CAPACITY: usize = 128;
pub const NETWORK_SNAPSHOT_QUEUE_CAPACITY: usize = 8;
/// Hard admission limit for distinct mutated chunk coordinates.
///
/// Existing coordinates remain updatable at the limit; new coordinates are
/// refused explicitly so an unloaded chunk's revision is never evicted.
pub const MUTATION_REVISION_INDEX_CAPACITY: usize = 65_536;

fn default_mutation_revision_index_capacity() -> usize {
    MUTATION_REVISION_INDEX_CAPACITY
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

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct MutationRevisionIndex {
    revisions: HashMap<(crate::dimension::Dimension, i32, i32), u64>,
    #[serde(skip, default = "default_mutation_revision_index_capacity")]
    capacity: usize,
}

impl Default for MutationRevisionIndex {
    fn default() -> Self {
        Self::with_capacity_limit(MUTATION_REVISION_INDEX_CAPACITY)
    }
}

impl MutationRevisionIndex {
    pub fn with_capacity_limit(capacity: usize) -> Self {
        Self {
            revisions: HashMap::new(),
            capacity,
        }
    }

    pub fn bump(
        &mut self,
        dimension: crate::dimension::Dimension,
        cx: i32,
        cz: i32,
    ) -> Result<u64, MutationRevisionIndexCapacityError> {
        self.require_admission_capacity(dimension, cx, cz)?;
        let revision = self
            .revisions
            .get(&(dimension, cx, cz))
            .copied()
            .unwrap_or(0)
            .saturating_add(1);
        self.revisions.insert((dimension, cx, cz), revision);
        Ok(revision)
    }

    pub fn ensure_at_least(
        &mut self,
        dimension: crate::dimension::Dimension,
        cx: i32,
        cz: i32,
        revision: u64,
    ) -> Result<bool, MutationRevisionIndexCapacityError> {
        self.require_admission_capacity(dimension, cx, cz)?;
        let entry = self.revisions.entry((dimension, cx, cz)).or_insert(0);
        if *entry >= revision {
            return Ok(false);
        }
        *entry = revision;
        Ok(true)
    }

    pub fn latest(&self, dimension: crate::dimension::Dimension, cx: i32, cz: i32) -> u64 {
        self.revisions
            .get(&(dimension, cx, cz))
            .copied()
            .unwrap_or(0)
    }

    pub fn entries_in(
        &self,
        dimension: crate::dimension::Dimension,
    ) -> impl Iterator<Item = ((i32, i32), u64)> + '_ {
        self.revisions
            .iter()
            .filter(move |((entry_dimension, _, _), _)| *entry_dimension == dimension)
            .map(|((_, cx, cz), revision)| ((*cx, *cz), *revision))
    }

    pub fn len(&self) -> usize {
        self.revisions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.revisions.is_empty()
    }

    pub fn capacity(&self) -> usize {
        self.capacity.max(self.revisions.len())
    }

    pub fn remove(
        &mut self,
        dimension: crate::dimension::Dimension,
        cx: i32,
        cz: i32,
    ) -> Option<u64> {
        self.revisions.remove(&(dimension, cx, cz))
    }

    pub fn reclaim_through(
        &mut self,
        dimension: crate::dimension::Dimension,
        cx: i32,
        cz: i32,
        acknowledged_revision: u64,
    ) -> bool {
        // A stale acknowledgement must not reclaim a newer mutation.
        let key = (dimension, cx, cz);
        match self.revisions.get(&key).copied() {
            Some(latest) if latest <= acknowledged_revision => {
                self.revisions.remove(&key);
                true
            }
            _ => false,
        }
    }

    fn require_admission_capacity(
        &self,
        dimension: crate::dimension::Dimension,
        cx: i32,
        cz: i32,
    ) -> Result<(), MutationRevisionIndexCapacityError> {
        if self.revisions.contains_key(&(dimension, cx, cz))
            || self.revisions.len() < self.capacity()
        {
            Ok(())
        } else {
            Err(MutationRevisionIndexCapacityError {
                capacity: self.capacity(),
            })
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NetworkSnapshotKey {
    pub player_id: crate::network::protocol::PlayerId,
    pub dimension: crate::dimension::Dimension,
    pub cx: i32,
    pub cz: i32,
    pub revision: u64,
}

pub struct NetworkSnapshotRequest {
    pub key: NetworkSnapshotKey,
    pub chunk: Option<Arc<Chunk>>,
}

pub struct NetworkSnapshotPayload {
    pub key: NetworkSnapshotKey,
    pub result: Result<(Vec<u8>, Vec<u8>, Vec<u8>, i8, u16), String>,
}

pub enum NetworkSnapshotWorkerResult {
    Snapshot(NetworkSnapshotPayload),
    IndexPersisted {
        generation: u64,
        result: Result<(), String>,
    },
}

enum NetworkSnapshotWorkerCommand {
    Snapshot(NetworkSnapshotRequest),
    PersistIndex {
        generation: u64,
        index: MutationRevisionIndex,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkSnapshotSubmitError {
    Full,
    Closed,
}

pub struct NetworkSnapshotWorker {
    tx: std::sync::mpsc::SyncSender<NetworkSnapshotWorkerCommand>,
    rx: std::sync::mpsc::Receiver<NetworkSnapshotWorkerResult>,
}

impl NetworkSnapshotWorker {
    pub fn try_submit(
        &self,
        request: NetworkSnapshotRequest,
    ) -> Result<(), NetworkSnapshotSubmitError> {
        self.tx
            .try_send(NetworkSnapshotWorkerCommand::Snapshot(request))
            .map_err(|error| match error {
                std::sync::mpsc::TrySendError::Full(_) => NetworkSnapshotSubmitError::Full,
                std::sync::mpsc::TrySendError::Disconnected(_) => {
                    NetworkSnapshotSubmitError::Closed
                }
            })
    }

    pub fn try_persist_index(
        &self,
        generation: u64,
        index: MutationRevisionIndex,
    ) -> Result<(), NetworkSnapshotSubmitError> {
        self.tx
            .try_send(NetworkSnapshotWorkerCommand::PersistIndex { generation, index })
            .map_err(|error| match error {
                std::sync::mpsc::TrySendError::Full(_) => NetworkSnapshotSubmitError::Full,
                std::sync::mpsc::TrySendError::Disconnected(_) => {
                    NetworkSnapshotSubmitError::Closed
                }
            })
    }

    pub fn try_iter(&self) -> std::sync::mpsc::TryIter<'_, NetworkSnapshotWorkerResult> {
        self.rx.try_iter()
    }
}

pub fn spawn_network_snapshot_worker(
    manager: Arc<Mutex<SaveManager>>,
    capacity: usize,
) -> NetworkSnapshotWorker {
    let (tx, worker_rx) = std::sync::mpsc::sync_channel(capacity.max(1));
    let (result_tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("icraft-network-snapshot".into())
        .spawn(move || {
            while let Ok(command) = worker_rx.recv() {
                let result = match command {
                    NetworkSnapshotWorkerCommand::Snapshot(request) => {
                        let data = if let Some(ref chunk) = request.chunk {
                            let mut data = ChunkSaveData::from_chunk(&chunk);
                            data.mutation_revision = request.key.revision;
                            Some(data)
                        } else {
                            manager
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .load_chunk_in(
                                    request.key.dimension,
                                    request.key.cx,
                                    request.key.cz,
                                )
                        };
                        let result = match data {
                            Some(data) if data.mutation_revision >= request.key.revision => {
                                let min_section_y = request
                                    .chunk
                                    .as_ref()
                                    .map(|c| c.min_section_y)
                                    .unwrap_or(0);
                                let section_count = request
                                    .chunk
                                    .as_ref()
                                    .map(|c| c.sections.len() as u16)
                                    .unwrap_or(0);
                                Ok((
                                    data.blocks,
                                    data.block_states,
                                    data.block_entities,
                                    min_section_y,
                                    section_count,
                                ))
                            }
                            Some(data) => Err(format!(
                                "persisted snapshot for {:?} chunk ({}, {}) is revision {}, waiting for {}",
                                request.key.dimension,
                                request.key.cx,
                                request.key.cz,
                                data.mutation_revision,
                                request.key.revision
                            )),
                            None => Err(format!(
                                "snapshot source unavailable for {:?} chunk ({}, {})",
                                request.key.dimension, request.key.cx, request.key.cz
                            )),
                        };
                        NetworkSnapshotWorkerResult::Snapshot(NetworkSnapshotPayload {
                            key: request.key,
                            result,
                        })
                    }
                    NetworkSnapshotWorkerCommand::PersistIndex { generation, index } => {
                        let result = manager
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .save_mutation_revision_index(&index)
                            .map_err(|error| error.to_string());
                        NetworkSnapshotWorkerResult::IndexPersisted { generation, result }
                    }
                };
                if result_tx.send(result).is_err() {
                    break;
                }
            }
        })
        .expect("failed to spawn network snapshot worker");
    NetworkSnapshotWorker { tx, rx }
}

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
    fn io(
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

fn default_spawn_x() -> i32 {
    8
}
fn default_spawn_y() -> i32 {
    80
}
fn default_spawn_z() -> i32 {
    8
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
    pub spawn_dimension: crate::dimension::Dimension,
    #[serde(default)]
    pub spawn_yaw: f32,
    #[serde(default)]
    pub version: u32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ItemStackData {
    pub item: Item,
    pub count: u32,
    pub durability: u32,
    pub enchantments: crate::enchantment::EnchantmentSet,
    pub potion: Option<crate::brewing::PotionData>,
    pub custom_name: crate::enchantment::ItemName,
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

fn default_collar_color() -> [f32; 3] {
    [0.8, 0.2, 0.2]
}

fn default_slime_size() -> u8 {
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
        entity
    }

    pub fn should_persist(&self) -> bool {
        self.entity_type.is_living()
            || self.entity_type.is_persistent()
            || self.entity_type == crate::entity::EntityType::DroppedItem
            || self.entity_type == crate::entity::EntityType::ExperienceOrb
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
    pub inventory: InventoryData,
    #[serde(default)]
    pub advancements: crate::advancements::AdvancementProgressData,
    #[serde(default)]
    pub spawn_point: Option<[i32; 3]>,
    #[serde(default)]
    pub spawn_dimension: Option<crate::dimension::Dimension>,
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

#[derive(Serialize, Deserialize)]
struct PlayerSaveEnvelope {
    version: u16,
    player: PlayerData,
}

fn serialize_player_data(player: &PlayerData) -> bincode::Result<Vec<u8>> {
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

fn deserialize_player_data(bytes: &[u8]) -> bincode::Result<PlayerData> {
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
    /// written before this sidecar existed deserialize as an empty vector via
    /// `#[serde(default)]`, preserving full backward compatibility.
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

impl ChunkSaveData {
    pub fn from_chunk(chunk: &Chunk) -> Self {
        Self::from_chunk_with_redstone(chunk, &[])
    }

    /// Builds a `ChunkSaveData` and attaches the provided redstone component
    /// metadata sidecar. Pass an empty slice for chunks with no persisted
    /// redstone state (the historical behavior of `from_chunk`).
    pub fn from_chunk_with_redstone(
        chunk: &Chunk,
        redstone_metadata: &[crate::redstone::RedstoneComponentMetadata],
    ) -> Self {
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
            bincode::serialize(redstone_metadata)
                .ok()
                .and_then(|bytes| compress_bytes(&bytes).ok())
                .unwrap_or_default()
        };

        let block_entities_list: Vec<((u8, i16, u8), crate::block_entity::BlockEntity)> = chunk
            .iter_block_entities()
            .map(|(pos, e)| (pos, e.clone()))
            .collect();
        let block_entities = if block_entities_list.is_empty() {
            Vec::new()
        } else {
            bincode::serialize(&block_entities_list)
                .ok()
                .and_then(|bytes| compress_bytes(&bytes).ok())
                .unwrap_or_default()
        };

        Self {
            chunk_x: chunk.chunk_x,
            chunk_z: chunk.chunk_z,
            blocks: compress_bytes(&blocks).unwrap_or_default(),
            sky_light: compress_bytes(&sky_light).unwrap_or_default(),
            block_light: compress_bytes(&block_light).unwrap_or_default(),
            fluid_levels: compress_bytes(&fluid_levels).unwrap_or_default(),
            redstone_metadata,
            block_states: compress_bytes(&block_states_raw).unwrap_or_default(),
            mutation_revision: 0,
            block_entities,
            data_version: 2,
        }
    }

    /// Decodes the redstone metadata sidecar into typed records. Returns an
    /// empty vector for older saves (no sidecar) or when decompression fails,
    /// so callers can always iterate the result without a separate error path.
    pub fn redstone_metadata(&self) -> Vec<crate::redstone::RedstoneComponentMetadata> {
        if self.redstone_metadata.is_empty() {
            return Vec::new();
        }
        decompress_bytes(&self.redstone_metadata)
            .ok()
            .and_then(|bytes| {
                bincode::deserialize::<Vec<crate::redstone::RedstoneComponentMetadata>>(&bytes).ok()
            })
            .unwrap_or_default()
    }

    pub fn block_entities(&self) -> Vec<((u8, i16, u8), crate::block_entity::BlockEntity)> {
        if self.block_entities.is_empty() {
            return Vec::new();
        }
        decompress_bytes(&self.block_entities)
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
        decompress_bytes(&self.block_states).unwrap_or_default()
    }

    pub fn restore_to_chunk(&self, chunk: &mut Chunk) {
        let blocks = decompress_bytes(&self.blocks).unwrap_or_default();
        let block_states = decompress_bytes(&self.block_states).unwrap_or_default();
        let sky_light = decompress_bytes(&self.sky_light).unwrap_or_default();
        let block_light = decompress_bytes(&self.block_light).unwrap_or_default();
        let fluid_levels = decompress_bytes(&self.fluid_levels).unwrap_or_default();

        let total_voxels = blocks.len();
        if total_voxels == 0 {
            return;
        }

        let is_legacy_256 = total_voxels == 16 * 256 * 16;
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
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveState {
    Dirty(u64),
    InFlight(u64),
    Persisted(u64),
}

#[derive(Debug)]
struct DirtyChunkSetInner {
    id: u64,
    states: Mutex<HashMap<(i32, i32), SaveState>>,
}

static NEXT_DIRTY_SET_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone)]
pub struct DirtyChunkSet {
    inner: Arc<DirtyChunkSetInner>,
}

impl Default for DirtyChunkSet {
    fn default() -> Self {
        Self::new()
    }
}

impl DirtyChunkSet {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(DirtyChunkSetInner {
                id: NEXT_DIRTY_SET_ID.fetch_add(1, Ordering::Relaxed),
                states: Mutex::new(HashMap::new()),
            }),
        }
    }

    pub fn id(&self) -> u64 {
        self.inner.id
    }

    pub fn mark_dirty(&self, cx: i32, cz: i32) -> u64 {
        let mut states = self.inner.states.lock().unwrap_or_else(|e| e.into_inner());
        let next_revision = match states.get(&(cx, cz)).copied() {
            Some(SaveState::Dirty(revision))
            | Some(SaveState::InFlight(revision))
            | Some(SaveState::Persisted(revision)) => revision.saturating_add(1),
            None => 1,
        };
        states.insert((cx, cz), SaveState::Dirty(next_revision));
        next_revision
    }

    pub fn is_dirty(&self, cx: i32, cz: i32) -> bool {
        matches!(
            self.inner
                .states
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(&(cx, cz)),
            Some(SaveState::Dirty(_))
        )
    }

    pub fn remove(&self, cx: i32, cz: i32) -> bool {
        self.inner
            .states
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&(cx, cz))
            .is_some()
    }

    pub fn clear(&self) {
        self.inner
            .states
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }

    pub fn dirty_revisions(&self) -> Vec<((i32, i32), u64)> {
        self.inner
            .states
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter_map(|(&coord, &state)| match state {
                SaveState::Dirty(revision) => Some((coord, revision)),
                SaveState::InFlight(_) | SaveState::Persisted(_) => None,
            })
            .collect()
    }

    pub fn dirty_revision(&self, cx: i32, cz: i32) -> Option<u64> {
        match self
            .inner
            .states
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&(cx, cz))
            .copied()
        {
            Some(SaveState::Dirty(revision)) => Some(revision),
            Some(SaveState::InFlight(_)) | Some(SaveState::Persisted(_)) | None => None,
        }
    }

    pub fn begin_save(&self, cx: i32, cz: i32, revision: u64) -> bool {
        let mut states = self.inner.states.lock().unwrap_or_else(|e| e.into_inner());
        if states.get(&(cx, cz)) == Some(&SaveState::Dirty(revision)) {
            states.insert((cx, cz), SaveState::InFlight(revision));
            true
        } else {
            false
        }
    }

    pub fn acknowledge_persisted(&self, cx: i32, cz: i32, revision: u64) {
        let mut states = self.inner.states.lock().unwrap_or_else(|e| e.into_inner());
        if states.get(&(cx, cz)) == Some(&SaveState::InFlight(revision)) {
            states.insert((cx, cz), SaveState::Persisted(revision));
        }
    }

    pub fn acknowledge_failed(&self, cx: i32, cz: i32, revision: u64) {
        let mut states = self.inner.states.lock().unwrap_or_else(|e| e.into_inner());
        if states.get(&(cx, cz)) == Some(&SaveState::InFlight(revision)) {
            states.insert((cx, cz), SaveState::Dirty(revision));
        }
    }

    pub fn state(&self, cx: i32, cz: i32) -> Option<SaveState> {
        self.inner
            .states
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&(cx, cz))
            .copied()
    }

    pub fn len(&self) -> usize {
        self.inner
            .states
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .filter(|state| matches!(state, SaveState::Dirty(_)))
            .count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[derive(Clone)]
pub struct UncompressedChunkSnapshot {
    pub dimension: crate::dimension::Dimension,
    pub chunk_x: i32,
    pub chunk_z: i32,
    pub blocks: Box<
        [[[BlockType; crate::world::CHUNK_DEPTH]; crate::world::CHUNK_HEIGHT];
            crate::world::CHUNK_WIDTH],
    >,
    pub block_states: Box<
        [[[u8; crate::world::CHUNK_DEPTH]; crate::world::CHUNK_HEIGHT]; crate::world::CHUNK_WIDTH],
    >,
    pub sky_light: Box<
        [[[u8; crate::world::CHUNK_DEPTH]; crate::world::CHUNK_HEIGHT]; crate::world::CHUNK_WIDTH],
    >,
    pub block_light: Box<
        [[[u8; crate::world::CHUNK_DEPTH]; crate::world::CHUNK_HEIGHT]; crate::world::CHUNK_WIDTH],
    >,
    pub fluid_levels: Box<
        [[[u8; crate::world::CHUNK_DEPTH]; crate::world::CHUNK_HEIGHT]; crate::world::CHUNK_WIDTH],
    >,
    pub redstone_metadata: Vec<crate::redstone::RedstoneComponentMetadata>,
    pub mutation_revision: u64,
}

impl UncompressedChunkSnapshot {
    pub fn from_chunk_with_redstone(
        dimension: crate::dimension::Dimension,
        chunk: &Chunk,
        redstone_metadata: Vec<crate::redstone::RedstoneComponentMetadata>,
    ) -> Self {
        let mut blocks: Box<
            [[[BlockType; crate::world::CHUNK_DEPTH]; crate::world::CHUNK_HEIGHT];
                crate::world::CHUNK_WIDTH],
        > = vec![
            [[BlockType::Air; crate::world::CHUNK_DEPTH]; crate::world::CHUNK_HEIGHT];
            crate::world::CHUNK_WIDTH
        ]
        .try_into()
        .unwrap();
        let mut block_states: Box<
            [[[u8; crate::world::CHUNK_DEPTH]; crate::world::CHUNK_HEIGHT];
                crate::world::CHUNK_WIDTH],
        > = vec![
            [[0u8; crate::world::CHUNK_DEPTH]; crate::world::CHUNK_HEIGHT];
            crate::world::CHUNK_WIDTH
        ]
        .try_into()
        .unwrap();
        let mut sky_light: Box<
            [[[u8; crate::world::CHUNK_DEPTH]; crate::world::CHUNK_HEIGHT];
                crate::world::CHUNK_WIDTH],
        > = vec![
            [[0u8; crate::world::CHUNK_DEPTH]; crate::world::CHUNK_HEIGHT];
            crate::world::CHUNK_WIDTH
        ]
        .try_into()
        .unwrap();
        let mut block_light: Box<
            [[[u8; crate::world::CHUNK_DEPTH]; crate::world::CHUNK_HEIGHT];
                crate::world::CHUNK_WIDTH],
        > = vec![
            [[0u8; crate::world::CHUNK_DEPTH]; crate::world::CHUNK_HEIGHT];
            crate::world::CHUNK_WIDTH
        ]
        .try_into()
        .unwrap();
        let mut fluid_levels: Box<
            [[[u8; crate::world::CHUNK_DEPTH]; crate::world::CHUNK_HEIGHT];
                crate::world::CHUNK_WIDTH],
        > = vec![
            [[0u8; crate::world::CHUNK_DEPTH]; crate::world::CHUNK_HEIGHT];
            crate::world::CHUNK_WIDTH
        ]
        .try_into()
        .unwrap();

        for x in 0..crate::world::CHUNK_WIDTH {
            for y in 0..crate::world::CHUNK_HEIGHT {
                for z in 0..crate::world::CHUNK_DEPTH {
                    blocks[x][y][z] = chunk.get_block_local(x, y as i32, z);
                    block_states[x][y][z] = chunk.get_block_state(x as i32, y as i32, z as i32);
                    sky_light[x][y][z] = chunk.get_sky_light(x, y as i32, z);
                    block_light[x][y][z] = chunk.get_block_light(x, y as i32, z);
                    fluid_levels[x][y][z] = chunk.get_fluid_level(x, y as i32, z);
                }
            }
        }

        Self {
            dimension,
            chunk_x: chunk.chunk_x,
            chunk_z: chunk.chunk_z,
            blocks,
            block_states,
            sky_light,
            block_light,
            fluid_levels,
            redstone_metadata,
            mutation_revision: 0,
        }
    }

    pub fn with_mutation_revision(mut self, revision: u64) -> Self {
        self.mutation_revision = revision;
        self
    }

    pub fn to_chunk_save_data(&self) -> ChunkSaveData {
        self.try_to_chunk_save_data()
            .unwrap_or_else(|_| ChunkSaveData {
                chunk_x: self.chunk_x,
                chunk_z: self.chunk_z,
                blocks: Vec::new(),
                sky_light: Vec::new(),
                block_light: Vec::new(),
                fluid_levels: Vec::new(),
                redstone_metadata: Vec::new(),
                block_states: Vec::new(),
                mutation_revision: self.mutation_revision,
                block_entities: Vec::new(),
                data_version: 0,
            })
    }

    pub fn try_to_chunk_save_data(&self) -> SaveResult<ChunkSaveData> {
        let mut blocks = Vec::with_capacity(16 * 256 * 16);
        let mut block_states_raw = Vec::with_capacity(16 * 256 * 16);
        let mut sky_light = Vec::with_capacity(16 * 256 * 16);
        let mut block_light = Vec::with_capacity(16 * 256 * 16);
        let mut fluid_levels = Vec::with_capacity(16 * 256 * 16);

        for x in 0..16 {
            for y in 0..256 {
                for z in 0..16 {
                    blocks.push(self.blocks[x][y][z] as u8);
                    block_states_raw.push(self.block_states[x][y][z]);
                    sky_light.push(self.sky_light[x][y][z]);
                    block_light.push(self.block_light[x][y][z]);
                    fluid_levels.push(self.fluid_levels[x][y][z]);
                }
            }
        }

        let redstone_metadata_bytes = if self.redstone_metadata.is_empty() {
            Vec::new()
        } else {
            bincode::serialize(&self.redstone_metadata)
                .map_err(|error| SaveError::Serialization(error.to_string()))
                .and_then(|bytes| {
                    compress_bytes(&bytes)
                        .map_err(|error| SaveError::Serialization(error.to_string()))
                })?
        };

        Ok(ChunkSaveData {
            chunk_x: self.chunk_x,
            chunk_z: self.chunk_z,
            blocks: compress_bytes(&blocks)
                .map_err(|error| SaveError::Serialization(error.to_string()))?,
            sky_light: compress_bytes(&sky_light)
                .map_err(|error| SaveError::Serialization(error.to_string()))?,
            block_light: compress_bytes(&block_light)
                .map_err(|error| SaveError::Serialization(error.to_string()))?,
            fluid_levels: compress_bytes(&fluid_levels)
                .map_err(|error| SaveError::Serialization(error.to_string()))?,
            redstone_metadata: redstone_metadata_bytes,
            block_states: compress_bytes(&block_states_raw)
                .map_err(|error| SaveError::Serialization(error.to_string()))?,
            mutation_revision: self.mutation_revision,
            block_entities: Vec::new(),
            data_version: 1,
        })
    }

    pub fn estimated_bytes(&self) -> u64 {
        let voxel_count =
            crate::world::CHUNK_WIDTH * crate::world::CHUNK_HEIGHT * crate::world::CHUNK_DEPTH;
        (voxel_count * (std::mem::size_of::<BlockType>() + 4 * std::mem::size_of::<u8>())
            + self.redstone_metadata.len()
                * std::mem::size_of::<crate::redstone::RedstoneComponentMetadata>()) as u64
    }
}

pub enum SaveCommand {
    SaveChunk {
        snapshot: UncompressedChunkSnapshot,
        revision: u64,
        tracker: DirtyChunkSet,
    },
    SaveLevelAndPlayer(LevelData, PlayerData),
    Flush(std::sync::mpsc::Sender<SaveResult<()>>),
}

type SaveKey = (crate::dimension::Dimension, i32, i32, u64);

#[derive(Clone)]
struct PendingChunkSave {
    snapshot: UncompressedChunkSnapshot,
    revision: u64,
    tracker: DirtyChunkSet,
    bytes: u64,
}

impl PendingChunkSave {
    fn key(&self) -> SaveKey {
        (
            self.snapshot.dimension,
            self.snapshot.chunk_x,
            self.snapshot.chunk_z,
            self.tracker.id(),
        )
    }

    fn acknowledge_persisted(&self) {
        self.tracker.acknowledge_persisted(
            self.snapshot.chunk_x,
            self.snapshot.chunk_z,
            self.revision,
        );
    }

    fn acknowledge_failed(&self) {
        self.tracker.acknowledge_failed(
            self.snapshot.chunk_x,
            self.snapshot.chunk_z,
            self.revision,
        );
    }
}

#[derive(Debug, Default)]
pub struct SaveQueueStats {
    queued_items: AtomicU64,
    queued_bytes: AtomicU64,
    in_flight: AtomicU64,
    in_flight_bytes: AtomicU64,
    dropped: AtomicU64,
    retries: AtomicU64,
    cancels: AtomicU64,
}

impl SaveQueueStats {
    pub fn depth(&self) -> u64 {
        self.queued_items.load(Ordering::Relaxed) + self.in_flight.load(Ordering::Relaxed)
    }

    pub fn queued_bytes(&self) -> u64 {
        self.queued_bytes.load(Ordering::Relaxed)
    }

    pub fn in_flight(&self) -> u64 {
        self.in_flight.load(Ordering::Relaxed)
    }

    pub fn in_flight_bytes(&self) -> u64 {
        self.in_flight_bytes.load(Ordering::Relaxed)
    }

    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
    pub fn retries(&self) -> u64 {
        self.retries.load(Ordering::Relaxed)
    }
    pub fn cancels(&self) -> u64 {
        self.cancels.load(Ordering::Relaxed)
    }
    pub(crate) fn record_retry(&self) {
        self.retries.fetch_add(1, Ordering::Relaxed);
    }
    pub(crate) fn record_cancel(&self) {
        self.cancels.fetch_add(1, Ordering::Relaxed);
    }
}

#[derive(Default)]
struct SaveQueueState {
    pending_chunks: HashMap<SaveKey, PendingChunkSave>,
    failed_chunks: HashMap<SaveKey, PendingChunkSave>,
    pending_level_player: Option<(LevelData, PlayerData)>,
    failed_level_player: Option<(LevelData, PlayerData)>,
    flush_waiters: VecDeque<std::sync::mpsc::Sender<SaveResult<()>>>,
    flush_error: Option<SaveError>,
    closed: bool,
}

impl SaveQueueState {
    fn work_items(&self) -> usize {
        self.pending_chunks.len()
            + self.failed_chunks.len()
            + usize::from(self.pending_level_player.is_some())
            + usize::from(self.failed_level_player.is_some())
    }
}

struct SaveQueueInner {
    state: Mutex<SaveQueueState>,
    work_available: Condvar,
    capacity_available: Condvar,
    capacity: usize,
    stats: Arc<SaveQueueStats>,
    last_error: Mutex<Option<SaveError>>,
    producers: AtomicU64,
}

pub struct SaveQueue {
    inner: Arc<SaveQueueInner>,
}

impl Clone for SaveQueue {
    fn clone(&self) -> Self {
        self.inner.producers.fetch_add(1, Ordering::Relaxed);
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl Drop for SaveQueue {
    fn drop(&mut self) {
        if self.inner.producers.fetch_sub(1, Ordering::AcqRel) != 1 {
            return;
        }
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        state.closed = true;
        for waiter in state.flush_waiters.drain(..) {
            let _ = waiter.send(Err(SaveError::QueueClosed));
        }
        self.inner.work_available.notify_all();
        self.inner.capacity_available.notify_all();
    }
}

impl SaveQueue {
    pub fn send(&self, command: SaveCommand) -> SaveResult<()> {
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if state.closed {
            drop(state);
            if let SaveCommand::SaveChunk {
                snapshot,
                revision,
                tracker,
            } = command
            {
                tracker.acknowledge_failed(snapshot.chunk_x, snapshot.chunk_z, revision);
            }
            return Err(SaveError::QueueClosed);
        }

        match command {
            SaveCommand::SaveChunk {
                snapshot,
                revision,
                tracker,
            } => {
                let task = PendingChunkSave {
                    bytes: snapshot.estimated_bytes(),
                    snapshot,
                    revision,
                    tracker,
                };
                let key = task.key();

                if let Some(failed) = state.failed_chunks.remove(&key) {
                    self.inner
                        .stats
                        .queued_bytes
                        .fetch_sub(failed.bytes, Ordering::Relaxed);
                    self.inner
                        .stats
                        .queued_items
                        .fetch_sub(1, Ordering::Relaxed);
                    self.inner.stats.dropped.fetch_add(1, Ordering::Relaxed);
                }

                if let Some(existing) = state.pending_chunks.get(&key) {
                    if existing.revision > revision {
                        self.inner.stats.dropped.fetch_add(1, Ordering::Relaxed);
                        return Ok(());
                    }
                } else {
                    while state.work_items()
                        + self.inner.stats.in_flight.load(Ordering::Relaxed) as usize
                        >= self.inner.capacity
                        && !state.closed
                    {
                        state = self
                            .inner
                            .capacity_available
                            .wait(state)
                            .unwrap_or_else(|error| error.into_inner());
                    }
                    if state.closed {
                        task.acknowledge_failed();
                        return Err(SaveError::QueueClosed);
                    }
                    self.inner
                        .stats
                        .queued_items
                        .fetch_add(1, Ordering::Relaxed);
                }

                if let Some(replaced) = state.pending_chunks.insert(key, task) {
                    self.inner
                        .stats
                        .queued_bytes
                        .fetch_sub(replaced.bytes, Ordering::Relaxed);
                    self.inner.stats.dropped.fetch_add(1, Ordering::Relaxed);
                }
                let bytes = state.pending_chunks.get(&key).unwrap().bytes;
                self.inner
                    .stats
                    .queued_bytes
                    .fetch_add(bytes, Ordering::Relaxed);
            }
            SaveCommand::SaveLevelAndPlayer(level, player) => {
                if state.pending_level_player.is_none() && state.failed_level_player.is_none() {
                    while state.work_items()
                        + self.inner.stats.in_flight.load(Ordering::Relaxed) as usize
                        >= self.inner.capacity
                        && !state.closed
                    {
                        state = self
                            .inner
                            .capacity_available
                            .wait(state)
                            .unwrap_or_else(|error| error.into_inner());
                    }
                    if state.closed {
                        return Err(SaveError::QueueClosed);
                    }
                    self.inner
                        .stats
                        .queued_items
                        .fetch_add(1, Ordering::Relaxed);
                } else {
                    self.inner.stats.dropped.fetch_add(1, Ordering::Relaxed);
                }
                state.failed_level_player = None;
                state.pending_level_player = Some((level, player));
            }
            SaveCommand::Flush(waiter) => {
                if state.pending_chunks.is_empty()
                    && state.failed_chunks.is_empty()
                    && state.pending_level_player.is_none()
                    && state.failed_level_player.is_none()
                {
                    state.flush_error = None;
                }
                if state.pending_chunks.is_empty() {
                    let failed = std::mem::take(&mut state.failed_chunks);
                    for (key, task) in failed {
                        if task.tracker.begin_save(
                            task.snapshot.chunk_x,
                            task.snapshot.chunk_z,
                            task.revision,
                        ) {
                            state.pending_chunks.insert(key, task);
                        } else {
                            self.inner
                                .stats
                                .queued_items
                                .fetch_sub(1, Ordering::Relaxed);
                            self.inner
                                .stats
                                .queued_bytes
                                .fetch_sub(task.bytes, Ordering::Relaxed);
                        }
                    }
                }
                if state.pending_level_player.is_none() {
                    state.pending_level_player = state.failed_level_player.take();
                }
                state.flush_waiters.push_back(waiter);
            }
        }
        self.inner.work_available.notify_one();
        Ok(())
    }

    pub fn stats(&self) -> Arc<SaveQueueStats> {
        Arc::clone(&self.inner.stats)
    }

    pub fn last_error(&self) -> Option<SaveError> {
        self.inner
            .last_error
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }
}

#[derive(Clone)]
struct SaveBatch {
    chunks: Vec<PendingChunkSave>,
    level_player: Option<(LevelData, PlayerData)>,
}

pub fn spawn_save_worker(manager: Arc<Mutex<SaveManager>>, capacity: usize) -> SaveQueue {
    let inner = Arc::new(SaveQueueInner {
        state: Mutex::new(SaveQueueState::default()),
        work_available: Condvar::new(),
        capacity_available: Condvar::new(),
        capacity: capacity.max(1),
        stats: Arc::new(SaveQueueStats::default()),
        last_error: Mutex::new(None),
        producers: AtomicU64::new(1),
    });
    let queue = SaveQueue {
        inner: Arc::clone(&inner),
    };

    std::thread::Builder::new()
        .name("icraft-save".to_string())
        .spawn(move || run_save_worker(inner, manager))
        .expect("failed to spawn save worker");
    queue
}

fn run_save_worker(inner: Arc<SaveQueueInner>, manager: Arc<Mutex<SaveManager>>) {
    loop {
        let batch = {
            let mut state = inner
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            while state.pending_chunks.is_empty()
                && state.pending_level_player.is_none()
                && state.flush_waiters.is_empty()
                && !state.closed
            {
                state = inner
                    .work_available
                    .wait(state)
                    .unwrap_or_else(|error| error.into_inner());
            }
            if state.closed {
                return;
            }

            let chunks: Vec<_> = state.pending_chunks.drain().map(|(_, task)| task).collect();
            let level_player = state.pending_level_player.take();
            let item_count = chunks.len() + usize::from(level_player.is_some());
            let byte_count = chunks.iter().map(|task| task.bytes).sum::<u64>();
            inner
                .stats
                .queued_items
                .fetch_sub(item_count as u64, Ordering::Relaxed);
            inner
                .stats
                .queued_bytes
                .fetch_sub(byte_count, Ordering::Relaxed);
            inner
                .stats
                .in_flight
                .fetch_add(item_count as u64, Ordering::Relaxed);
            inner
                .stats
                .in_flight_bytes
                .fetch_add(byte_count, Ordering::Relaxed);
            inner.capacity_available.notify_all();
            SaveBatch {
                chunks,
                level_player,
            }
        };

        let panic_backup = batch.clone();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            persist_save_batch(&manager, batch)
        }));
        let (failed_chunks, failed_level_player, error, completed_items, completed_bytes) =
            match result {
                Ok(outcome) => outcome,
                Err(payload) => {
                    let message = payload
                        .downcast_ref::<&str>()
                        .map(|message| (*message).to_string())
                        .or_else(|| payload.downcast_ref::<String>().cloned())
                        .unwrap_or_else(|| "unknown panic".to_string());
                    for task in &panic_backup.chunks {
                        task.acknowledge_failed();
                    }
                    let completed_items = panic_backup.chunks.len()
                        + usize::from(panic_backup.level_player.is_some());
                    let completed_bytes = panic_backup.chunks.iter().map(|task| task.bytes).sum();
                    (
                        panic_backup.chunks,
                        panic_backup.level_player,
                        Some(SaveError::WorkerPanic(message)),
                        completed_items,
                        completed_bytes,
                    )
                }
            };

        inner
            .stats
            .in_flight
            .fetch_sub(completed_items as u64, Ordering::Relaxed);
        inner
            .stats
            .in_flight_bytes
            .fetch_sub(completed_bytes, Ordering::Relaxed);
        inner.capacity_available.notify_all();

        let mut state = inner
            .state
            .lock()
            .unwrap_or_else(|lock_error| lock_error.into_inner());
        for task in failed_chunks {
            inner.stats.record_retry();
            let key = task.key();
            if state.failed_chunks.insert(key, task).is_none() {
                inner.stats.queued_items.fetch_add(1, Ordering::Relaxed);
            }
            let bytes = state.failed_chunks.get(&key).unwrap().bytes;
            inner.stats.queued_bytes.fetch_add(bytes, Ordering::Relaxed);
        }
        if let Some(level_player) = failed_level_player {
            if state.failed_level_player.replace(level_player).is_none() {
                inner.stats.queued_items.fetch_add(1, Ordering::Relaxed);
            }
        }

        if let Some(error) = &error {
            *inner
                .last_error
                .lock()
                .unwrap_or_else(|lock_error| lock_error.into_inner()) = Some(error.clone());
            eprintln!("[Save] {error}");
            if !state.flush_waiters.is_empty() && state.flush_error.is_none() {
                state.flush_error = Some(error.clone());
            }
        }

        if state.pending_chunks.is_empty() && state.pending_level_player.is_none() {
            let flush_result = match &state.flush_error {
                Some(err) => Err(err.clone()),
                None => Ok(()),
            };
            if flush_result.is_ok()
                && state.failed_chunks.is_empty()
                && state.failed_level_player.is_none()
            {
                *inner
                    .last_error
                    .lock()
                    .unwrap_or_else(|lock_error| lock_error.into_inner()) = None;
            }
            if !state.flush_waiters.is_empty() {
                if flush_result.is_err() {
                    state.flush_error = None;
                }
                for waiter in state.flush_waiters.drain(..) {
                    let _ = waiter.send(flush_result.clone());
                }
            }
        }
    }
}

fn persist_save_batch(
    manager: &Arc<Mutex<SaveManager>>,
    batch: SaveBatch,
) -> (
    Vec<PendingChunkSave>,
    Option<(LevelData, PlayerData)>,
    Option<SaveError>,
    usize,
    u64,
) {
    let completed_items = batch.chunks.len() + usize::from(batch.level_player.is_some());
    let completed_bytes = batch.chunks.iter().map(|task| task.bytes).sum();
    let mut first_error = None;
    let mut failed_chunks = Vec::new();
    let mut failed_level_player = None;
    let mut manager = manager.lock().unwrap_or_else(|error| error.into_inner());

    #[cfg(test)]
    if std::mem::take(&mut manager.panic_next_worker_save) {
        panic!("injected save worker panic");
    }

    if let Some((level, player)) = batch.level_player {
        if let Err(error) = manager.save_player_and_level(&level, &player) {
            first_error.get_or_insert_with(|| {
                SaveError::io("save level and player", manager.world_dir.clone(), &error)
            });
            failed_level_player = Some((level, player));
        }
    }

    let mut groups: HashMap<(crate::dimension::Dimension, i32, i32), Vec<PendingChunkSave>> =
        HashMap::new();
    for task in batch.chunks {
        groups
            .entry((
                task.snapshot.dimension,
                task.snapshot.chunk_x.div_euclid(32),
                task.snapshot.chunk_z.div_euclid(32),
            ))
            .or_default()
            .push(task);
    }

    for ((dimension, _, _), tasks) in groups {
        let snapshots: Vec<_> = tasks.iter().map(|task| task.snapshot.clone()).collect();
        match manager.save_chunks_batch_in(dimension, &snapshots) {
            Ok(()) => {
                for task in &tasks {
                    task.acknowledge_persisted();
                }
            }
            Err(error) => {
                first_error.get_or_insert_with(|| error.clone());
                for task in &tasks {
                    task.acknowledge_failed();
                }
                failed_chunks.extend(tasks);
            }
        }
    }

    (
        failed_chunks,
        failed_level_player,
        first_error,
        completed_items,
        completed_bytes,
    )
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RegionData {
    /// Maps local coordinate (0..32, 0..32) -> Bincode serialized ChunkSaveData bytes
    pub chunks: HashMap<(u8, u8), Vec<u8>>,
}

pub struct SaveManager {
    pub world_dir: PathBuf,
    region_cache: HashMap<(crate::dimension::Dimension, i32, i32), RegionData>,
    lru_order: VecDeque<(crate::dimension::Dimension, i32, i32)>,
    #[cfg(test)]
    panic_next_worker_save: bool,
    #[cfg(test)]
    fail_next_serialization: bool,
}

static NEXT_TEMP_FILE_ID: AtomicU64 = AtomicU64::new(1);

#[cfg(test)]
thread_local! {
    static ATOMIC_WRITE_FAILPOINT: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
fn atomic_write_should_fail(stage: u8) -> bool {
    ATOMIC_WRITE_FAILPOINT.with(|failpoint| failpoint.get() == stage)
}

#[cfg(not(test))]
fn atomic_write_should_fail(_stage: u8) -> bool {
    false
}

#[cfg(test)]
fn atomic_write_should_crash(stage: &str) -> bool {
    std::env::var("ICRAFT_TEST_ATOMIC_CRASH_STAGE").as_deref() == Ok(stage)
}

#[cfg(not(test))]
fn atomic_write_should_crash(_stage: &str) -> bool {
    false
}

pub fn atomic_write<P: AsRef<Path>>(path: P, bytes: &[u8]) -> io::Result<()> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("save");
    let tmp_path = path.with_file_name(format!(
        ".{file_name}.{}.{}.tmp",
        std::process::id(),
        NEXT_TEMP_FILE_ID.fetch_add(1, Ordering::Relaxed)
    ));
    {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp_path)?;
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()?;
    }

    if atomic_write_should_crash("before_replace") {
        std::process::abort();
    }

    if atomic_write_should_fail(1) {
        let _ = fs::remove_file(&tmp_path);
        return Err(io::Error::new(
            io::ErrorKind::Other,
            "injected failure before atomic replacement",
        ));
    }

    if let Err(error) = replace_file_atomically(&tmp_path, path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(error);
    }

    if atomic_write_should_crash("after_replace") {
        std::process::abort();
    }

    if atomic_write_should_fail(2) {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            "injected failure after atomic replacement",
        ));
    }

    #[cfg(unix)]
    if let Some(parent) = path.parent() {
        File::open(parent)?.sync_all()?;
    }

    Ok(())
}

#[cfg(not(windows))]
fn replace_file_atomically(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(windows)]
fn replace_file_atomically(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let source_wide: Vec<u16> = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let destination_wide: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let result = unsafe {
        MoveFileExW(
            source_wide.as_ptr(),
            destination_wide.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

pub fn compress_bytes(data: &[u8]) -> io::Result<Vec<u8>> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data)?;
    encoder.finish()
}

pub fn decompress_bytes(data: &[u8]) -> io::Result<Vec<u8>> {
    let mut decoder = ZlibDecoder::new(data);
    let mut result = Vec::new();
    decoder.read_to_end(&mut result)?;
    Ok(result)
}

/// Deserializes a `ChunkSaveData` from persisted chunk bytes with full backward
/// compatibility. Bincode 1.x does not synthesize newly-added fields from
/// `#[serde(default)]`, so every historical shape needs an explicit fallback.
fn deserialize_chunk_save_data(bytes: &[u8]) -> Option<ChunkSaveData> {
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

impl SaveManager {
    pub fn new<P: AsRef<Path>>(world_dir: P) -> Self {
        let world_dir = world_dir.as_ref().to_path_buf();
        let regions_dir = world_dir.join("regions");
        if !regions_dir.exists() {
            fs::create_dir_all(&regions_dir).unwrap();
        }
        for name in ["nether", "end"] {
            let path = world_dir.join("dimensions").join(name).join("regions");
            if !path.exists() {
                fs::create_dir_all(path).unwrap();
            }
        }
        Self {
            world_dir,
            region_cache: HashMap::new(),
            lru_order: VecDeque::new(),
            #[cfg(test)]
            panic_next_worker_save: false,
            #[cfg(test)]
            fail_next_serialization: false,
        }
    }

    #[cfg(test)]
    fn inject_worker_panic_once(&mut self) {
        self.panic_next_worker_save = true;
    }

    #[cfg(test)]
    fn inject_serialization_failure_once(&mut self) {
        self.fail_next_serialization = true;
    }

    fn touch_region(&mut self, key: (crate::dimension::Dimension, i32, i32)) {
        if !self.region_cache.contains_key(&key) {
            if let Some(pos) = self
                .lru_order
                .iter()
                .position(|candidate| candidate == &key)
            {
                self.lru_order.remove(pos);
            }
            return;
        }
        if let Some(pos) = self.lru_order.iter().position(|k| k == &key) {
            self.lru_order.remove(pos);
        }
        self.lru_order.push_back(key);
    }

    fn evict_lru_regions(&mut self) {
        const MAX_ENTRIES: usize = 16;
        const MAX_BYTES: u64 = 64 * 1024 * 1024;

        while self.region_cache.len() > MAX_ENTRIES || self.region_cache_bytes() > MAX_BYTES {
            if let Some(lru_key) = self.lru_order.pop_front() {
                self.region_cache.remove(&lru_key);
            } else {
                break;
            }
        }
    }

    pub fn region_cache_bytes(&self) -> u64 {
        self.region_cache
            .values()
            .map(|region| region.chunks.values().map(|v| v.len() as u64).sum::<u64>())
            .sum()
    }

    fn region_dir(&self, dimension: crate::dimension::Dimension) -> PathBuf {
        match dimension {
            crate::dimension::Dimension::Overworld => self.world_dir.join("regions"),
            crate::dimension::Dimension::Nether => self
                .world_dir
                .join("dimensions")
                .join("nether")
                .join("regions"),
            crate::dimension::Dimension::End => self
                .world_dir
                .join("dimensions")
                .join("end")
                .join("regions"),
        }
    }

    pub fn entities_file_path(&self, dimension: crate::dimension::Dimension) -> PathBuf {
        match dimension {
            crate::dimension::Dimension::Overworld => self.world_dir.join("entities.dat"),
            crate::dimension::Dimension::Nether => self
                .world_dir
                .join("dimensions")
                .join("nether")
                .join("entities.dat"),
            crate::dimension::Dimension::End => self
                .world_dir
                .join("dimensions")
                .join("end")
                .join("entities.dat"),
        }
    }

    pub fn save_entities_in(
        &self,
        dimension: crate::dimension::Dimension,
        entities: &[EntitySaveData],
    ) -> io::Result<()> {
        let path = self.entities_file_path(dimension);
        let bytes =
            bincode::serialize(entities).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        atomic_write(path, &bytes)
    }

    pub fn load_entities_in(&self, dimension: crate::dimension::Dimension) -> Vec<EntitySaveData> {
        let path = self.entities_file_path(dimension);
        if !path.exists() {
            return Vec::new();
        }
        let bytes = match fs::read(path) {
            Ok(b) => b,
            Err(_) => return Vec::new(),
        };
        bincode::deserialize(&bytes).unwrap_or_default()
    }

    pub fn load_chunk(&mut self, cx: i32, cz: i32) -> Option<ChunkSaveData> {
        self.load_chunk_in(crate::dimension::Dimension::Overworld, cx, cz)
    }

    pub fn load_chunk_in(
        &mut self,
        dimension: crate::dimension::Dimension,
        cx: i32,
        cz: i32,
    ) -> Option<ChunkSaveData> {
        let rx = cx.div_euclid(32);
        let rz = cz.div_euclid(32);
        let lx = cx.rem_euclid(32) as u8;
        let lz = cz.rem_euclid(32) as u8;
        let region_file = self
            .region_dir(dimension)
            .join(format!("r.{}.{}.bin", rx, rz));

        if !self.region_cache.contains_key(&(dimension, rx, rz)) {
            if region_file.exists() {
                if let Ok(mut file) = File::open(&region_file) {
                    let mut bytes = Vec::new();
                    if file.read_to_end(&mut bytes).is_ok() {
                        if let Ok(region_data) = bincode::deserialize::<RegionData>(&bytes) {
                            self.region_cache.insert((dimension, rx, rz), region_data);
                        }
                    }
                }
            }
        }

        if self.region_cache.contains_key(&(dimension, rx, rz)) {
            self.touch_region((dimension, rx, rz));
        }
        self.evict_lru_regions();

        let region = self.region_cache.get(&(dimension, rx, rz))?;

        if let Some(chunk_bytes) = region.chunks.get(&(lx, lz)) {
            deserialize_chunk_save_data(chunk_bytes)
        } else {
            None
        }
    }

    pub fn save_chunk(&mut self, cx: i32, cz: i32, data: ChunkSaveData) -> SaveResult<()> {
        self.save_chunk_in(crate::dimension::Dimension::Overworld, cx, cz, data)
    }

    pub fn save_chunks_batch_in(
        &mut self,
        dimension: crate::dimension::Dimension,
        snapshots: &[UncompressedChunkSnapshot],
    ) -> SaveResult<()> {
        #[cfg(test)]
        if std::mem::take(&mut self.fail_next_serialization) {
            return Err(SaveError::Serialization(
                "injected serialization failure".to_string(),
            ));
        }
        if snapshots.is_empty() {
            return Ok(());
        }
        let mut by_region: HashMap<(i32, i32), Vec<&UncompressedChunkSnapshot>> = HashMap::new();
        for snap in snapshots {
            let rx = snap.chunk_x.div_euclid(32);
            let rz = snap.chunk_z.div_euclid(32);
            by_region.entry((rx, rz)).or_default().push(snap);
        }

        for ((rx, rz), snaps) in by_region {
            let region_file = self
                .region_dir(dimension)
                .join(format!("r.{}.{}.bin", rx, rz));
            let representative = (snaps[0].chunk_x, snaps[0].chunk_z);
            let region =
                self.load_region_for_write(&region_file, representative.0, representative.1)?;
            self.region_cache.insert((dimension, rx, rz), region);

            let mut serialized_chunks = Vec::with_capacity(snaps.len());
            for snap in &snaps {
                let lx = snap.chunk_x.rem_euclid(32) as u8;
                let lz = snap.chunk_z.rem_euclid(32) as u8;
                let data = snap.try_to_chunk_save_data()?;
                let serialized_chunk = bincode::serialize(&data)
                    .map_err(|error| SaveError::Serialization(error.to_string()))?;
                serialized_chunks.push(((lx, lz), serialized_chunk));
            }

            let region = self
                .region_cache
                .entry((dimension, rx, rz))
                .or_insert_with(|| RegionData {
                    chunks: HashMap::new(),
                });

            for (coord, serialized_chunk) in serialized_chunks {
                region.chunks.insert(coord, serialized_chunk);
            }

            let serialized_region = bincode::serialize(region)
                .map_err(|error| SaveError::Serialization(error.to_string()))?;

            backup_region_file_if_needed(&region_file);
            atomic_write(&region_file, &serialized_region)
                .map_err(|error| SaveError::io("atomic region replacement", &region_file, error))?;
            self.touch_region((dimension, rx, rz));
            self.evict_lru_regions();
        }
        Ok(())
    }

    pub fn save_chunk_in(
        &mut self,
        dimension: crate::dimension::Dimension,
        cx: i32,
        cz: i32,
        data: ChunkSaveData,
    ) -> SaveResult<()> {
        let rx = cx.div_euclid(32);
        let rz = cz.div_euclid(32);
        let lx = cx.rem_euclid(32) as u8;
        let lz = cz.rem_euclid(32) as u8;
        let region_file = self
            .region_dir(dimension)
            .join(format!("r.{}.{}.bin", rx, rz));

        let region = self.load_region_for_write(&region_file, cx, cz)?;
        self.region_cache.insert((dimension, rx, rz), region);

        let region = self
            .region_cache
            .entry((dimension, rx, rz))
            .or_insert_with(|| RegionData {
                chunks: HashMap::new(),
            });

        let serialized_chunk = bincode::serialize(&data)
            .map_err(|error| SaveError::Serialization(error.to_string()))?;

        region.chunks.insert((lx, lz), serialized_chunk);

        let serialized_region = bincode::serialize(region)
            .map_err(|error| SaveError::Serialization(error.to_string()))?;

        backup_region_file_if_needed(&region_file);
        atomic_write(&region_file, &serialized_region)
            .map_err(|error| SaveError::io("atomic region replacement", &region_file, error))?;
        self.touch_region((dimension, rx, rz));
        self.evict_lru_regions();
        Ok(())
    }

    fn load_region_for_write(
        &self,
        region_file: &Path,
        chunk_x: i32,
        chunk_z: i32,
    ) -> SaveResult<RegionData> {
        if !region_file.exists() {
            return Ok(RegionData {
                chunks: HashMap::new(),
            });
        }

        let bytes = fs::read(region_file).map_err(|error| SaveError::RegionCorruption {
            path: region_file.to_path_buf(),
            chunk_x,
            chunk_z,
            message: format!("could not read existing region: {error}"),
        })?;
        bincode::deserialize(&bytes).map_err(|error| SaveError::RegionCorruption {
            path: region_file.to_path_buf(),
            chunk_x,
            chunk_z,
            message: format!("could not deserialize existing region: {error}"),
        })
    }

    pub fn salvage_readable_region(&self, source: &Path, destination: &Path) -> SaveResult<usize> {
        let bytes = fs::read(source)
            .map_err(|error| SaveError::io("read region for salvage", source, error))?;
        let region: RegionData =
            bincode::deserialize(&bytes).map_err(|error| SaveError::RegionCorruption {
                path: source.to_path_buf(),
                chunk_x: 0,
                chunk_z: 0,
                message: format!("region container is not readable: {error}"),
            })?;
        let readable_chunks: HashMap<_, _> = region
            .chunks
            .into_iter()
            .filter(|(_, bytes)| deserialize_chunk_save_data(bytes).is_some())
            .collect();
        let count = readable_chunks.len();
        let serialized = bincode::serialize(&RegionData {
            chunks: readable_chunks,
        })
        .map_err(|error| SaveError::Serialization(error.to_string()))?;
        atomic_write(destination, &serialized)
            .map_err(|error| SaveError::io("write salvaged region", destination, error))?;
        Ok(count)
    }

    pub fn save_current_dimension(&self, dimension: crate::dimension::Dimension) -> io::Result<()> {
        atomic_write(self.world_dir.join("dimension.dat"), &[dimension as u8])
    }

    pub fn save_mutation_revision_index(&self, index: &MutationRevisionIndex) -> io::Result<()> {
        let bytes = bincode::serialize(index)
            .map_err(|error| io::Error::new(io::ErrorKind::Other, error))?;
        atomic_write(self.world_dir.join("mutation_revisions.bin"), &bytes)
    }

    pub fn load_mutation_revision_index(&self) -> MutationRevisionIndex {
        fs::read(self.world_dir.join("mutation_revisions.bin"))
            .ok()
            .and_then(|bytes| bincode::deserialize(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn load_current_dimension(&self) -> crate::dimension::Dimension {
        match fs::read(self.world_dir.join("dimension.dat"))
            .ok()
            .and_then(|bytes| bytes.first().copied())
        {
            Some(1) => crate::dimension::Dimension::Nether,
            Some(2) => crate::dimension::Dimension::End,
            _ => crate::dimension::Dimension::Overworld,
        }
    }

    pub fn save_player_and_level(&self, level: &LevelData, player: &PlayerData) -> io::Result<()> {
        let level_file = self.world_dir.join("level.dat");
        let player_file = self.world_dir.join("player.dat");

        let serialized_level =
            bincode::serialize(level).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        let serialized_player =
            serialize_player_data(player).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        atomic_write(&level_file, &serialized_level)?;
        atomic_write(&player_file, &serialized_player)?;

        Ok(())
    }

    pub fn load_player_and_level(&self) -> io::Result<(LevelData, PlayerData)> {
        let level_file = self.world_dir.join("level.dat");
        let player_file = self.world_dir.join("player.dat");

        let mut lf = File::open(&level_file)?;
        let mut level_bytes = Vec::new();
        lf.read_to_end(&mut level_bytes)?;
        let level = bincode::deserialize::<LevelData>(&level_bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        let mut pf = File::open(&player_file)?;
        let mut player_bytes = Vec::new();
        pf.read_to_end(&mut player_bytes)?;
        let player = deserialize_player_data(&player_bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::Other, error))?;

        Ok((level, player))
    }
}

fn backup_region_file_if_needed(region_file: &Path) {
    if region_file.exists() {
        let backup_file = region_file.with_extension("bin.bak");
        if !backup_file.exists() {
            let _ = fs::copy(region_file, backup_file);
        }
    }
}

#[derive(Serialize, Deserialize)]
struct PreviousInventoryData {
    hotbar: Vec<Option<ItemStackData>>,
    main: Vec<Option<ItemStackData>>,
    armor: Vec<Option<ItemStackData>>,
    selected: usize,
}

#[derive(Serialize, Deserialize)]
struct PreviousPlayerData {
    position: [f32; 3],
    velocity: [f32; 3],
    yaw: f32,
    pitch: f32,
    health: f32,
    hunger: f32,
    saturation: f32,
    exhaustion: f32,
    oxygen: f32,
    experience: u32,
    experience_level: u32,
    game_mode: GameMode,
    inventory: PreviousInventoryData,
    advancements: crate::advancements::AdvancementProgressData,
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
struct LegacyItemStackData {
    item: Item,
    count: u32,
    durability: u32,
}

#[derive(Deserialize)]
struct LegacyInventoryData {
    hotbar: Vec<Option<LegacyItemStackData>>,
    main: Vec<Option<LegacyItemStackData>>,
    armor: Vec<Option<LegacyItemStackData>>,
    selected: usize,
}

#[derive(Deserialize)]
struct LegacyPlayerData {
    position: [f32; 3],
    velocity: [f32; 3],
    yaw: f32,
    pitch: f32,
    health: f32,
    hunger: f32,
    saturation: f32,
    exhaustion: f32,
    oxygen: f32,
    game_mode: GameMode,
    inventory: LegacyInventoryData,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dimension::Dimension;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn test_serialization_roundtrips() {
        let level = LevelData {
            seed: 12345,
            time: 6000,
            spawn_x: 8,
            spawn_y: 80,
            spawn_z: 8,
            spawn_dimension: Dimension::Overworld,
            spawn_yaw: 0.0,
            version: 2,
        };
        let encoded_level = bincode::serialize(&level).unwrap();
        let decoded_level: LevelData = bincode::deserialize(&encoded_level).unwrap();
        assert_eq!(level.seed, decoded_level.seed);
        assert_eq!(level.time, decoded_level.time);

        let player = PlayerData {
            position: [1.0, 2.0, 3.0],
            velocity: [0.1, 0.2, 0.3],
            yaw: 1.5,
            pitch: 0.5,
            health: 20.0,
            hunger: 20.0,
            saturation: 5.0,
            exhaustion: 0.0,
            oxygen: 300.0,
            experience: 120,
            experience_level: 12,
            game_mode: GameMode::Survival,
            spawn_point: None,
            spawn_dimension: None,
            inventory: InventoryData {
                hotbar: vec![Some(ItemStackData {
                    item: Item::Stone,
                    count: 64,
                    durability: 0,
                    enchantments: Default::default(),
                    potion: None,
                    custom_name: Default::default(),
                })],
                main: vec![None],
                armor: vec![None],
                offhand: None,
                selected: 0,
                dragged: None,
                creative_drag_origin: None,
            },
            advancements: Default::default(),
            unlocked_recipes: Default::default(),
        };
        let encoded_player = bincode::serialize(&player).unwrap();
        let decoded_player: PlayerData = bincode::deserialize(&encoded_player).unwrap();
        assert_eq!(player.position, decoded_player.position);
        assert_eq!(player.yaw, decoded_player.yaw);
        assert_eq!(player.health, decoded_player.health);
        assert_eq!(
            player.inventory.hotbar[0].as_ref().unwrap().item,
            Item::Stone
        );

        let mut original_blocks = vec![0u8; 16 * 256 * 16];
        original_blocks[0] = 1;
        original_blocks[100] = 3;
        let compressed_blocks = compress_bytes(&original_blocks).unwrap();
        let decompressed_blocks = decompress_bytes(&compressed_blocks).unwrap();
        assert_eq!(original_blocks, decompressed_blocks);
        println!(
            "Compressed size: {} bytes, Original: {} bytes",
            compressed_blocks.len(),
            original_blocks.len()
        );
    }

    #[test]
    fn enchanted_potion_stack_metadata_roundtrips() {
        let mut stack = ItemStack::new(Item::Potion, 1);
        stack
            .enchantments
            .add_or_upgrade(crate::enchantment::Enchantment::Unbreaking(3));
        stack.potion = Some(crate::brewing::PotionData {
            kind: crate::brewing::PotionKind::Speed,
            level: 2,
            duration_seconds: 90,
            splash: true,
        });
        stack.custom_name.set("Swift Brew");
        let encoded = bincode::serialize(&ItemStackData::from(&stack)).unwrap();
        let decoded: ItemStackData = bincode::deserialize(&encoded).unwrap();
        let decoded = decoded.to_item_stack();
        assert_eq!(decoded.enchantments, stack.enchantments);
        assert_eq!(decoded.potion, stack.potion);
        assert_eq!(decoded.custom_name.as_str(), "Swift Brew");
    }

    #[test]
    fn real_cursors_roundtrip_and_catalog_cursors_do_not_persist() {
        let mut stack = ItemStack::new(Item::Dirt, 17);
        stack.custom_name.set("Travel Stack");
        stack
            .enchantments
            .add_or_upgrade(crate::enchantment::Enchantment::Unbreaking(2));

        for origin in [None, Some(CreativeDragOrigin::Inventory)] {
            let mut inventory = Inventory::new();
            inventory.dragged = Some(stack);
            inventory.creative_drag_origin = origin;

            let restored = InventoryData::from(&inventory).to_inventory();
            assert_eq!(restored.dragged, Some(stack));
            assert_eq!(restored.creative_drag_origin, origin);
        }

        let mut catalog_inventory = Inventory::new();
        catalog_inventory.dragged = Some(stack);
        catalog_inventory.creative_drag_origin = Some(CreativeDragOrigin::Catalog);
        let saved = InventoryData::from(&catalog_inventory);
        assert!(saved.dragged.is_none());
        assert!(saved.creative_drag_origin.is_none());
        let restored = saved.to_inventory();
        assert!(restored.dragged.is_none());
        assert!(restored.creative_drag_origin.is_none());
    }

    #[test]
    fn versioned_player_codec_preserves_real_cursor_metadata_and_provenance() {
        let mut inventory = Inventory::new();
        let mut cursor = ItemStack::new(Item::Potion, 1);
        cursor.custom_name.set("Exit Safe");
        cursor.potion = Some(crate::brewing::PotionData {
            kind: crate::brewing::PotionKind::Strength,
            level: 2,
            duration_seconds: 90,
            splash: false,
        });
        inventory.dragged = Some(cursor);
        inventory.creative_drag_origin = Some(CreativeDragOrigin::Inventory);
        let player = PlayerData {
            position: [1.0, 2.0, 3.0],
            velocity: [0.0; 3],
            yaw: 0.5,
            pitch: -0.25,
            health: 20.0,
            hunger: 18.0,
            saturation: 4.0,
            exhaustion: 1.0,
            oxygen: 300.0,
            experience: 10,
            experience_level: 2,
            game_mode: GameMode::Creative,
            inventory: InventoryData::from(&inventory),
            advancements: Default::default(),
            spawn_point: None,
            spawn_dimension: None,
            unlocked_recipes: Default::default(),
            bad_omen_level: 0,
            hero_of_the_village_timer: 0.0,
        };

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let world_dir = std::env::temp_dir().join(format!(
            "icraft_cursor_player_save_{}_{}",
            std::process::id(),
            unique
        ));
        let manager = SaveManager::new(&world_dir);
        manager
            .save_player_and_level(
                &LevelData {
                    seed: 99,
                    time: 1234,
                    spawn_x: 8,
                    spawn_y: 80,
                    spawn_z: 8,
                    spawn_dimension: Dimension::Overworld,
                    spawn_yaw: 0.0,
                    version: 2,
                },
                &player,
            )
            .unwrap();
        let encoded = fs::read(world_dir.join("player.dat")).unwrap();
        assert!(encoded.starts_with(PLAYER_SAVE_MAGIC));

        let (_, decoded) = manager.load_player_and_level().unwrap();
        let restored = decoded.inventory.to_inventory();
        assert_eq!(restored.dragged, Some(cursor));
        assert_eq!(
            restored.creative_drag_origin,
            Some(CreativeDragOrigin::Inventory)
        );
        fs::remove_dir_all(world_dir).unwrap();
    }

    #[test]
    fn previous_bincode_player_fixture_migrates_without_a_cursor() {
        let previous = PreviousPlayerData {
            position: [4.0, 5.0, 6.0],
            velocity: [0.1, 0.2, 0.3],
            yaw: 1.0,
            pitch: 0.2,
            health: 17.0,
            hunger: 12.0,
            saturation: 3.0,
            exhaustion: 2.0,
            oxygen: 250.0,
            experience: 42,
            experience_level: 5,
            game_mode: GameMode::Survival,
            inventory: PreviousInventoryData {
                hotbar: vec![Some(ItemStackData::from(&ItemStack::new(Item::Stone, 32)))],
                main: vec![None],
                armor: vec![None],
                selected: 0,
            },
            advancements: Default::default(),
        };
        let legacy_fixture = bincode::serialize(&previous).unwrap();
        assert!(!legacy_fixture.starts_with(PLAYER_SAVE_MAGIC));

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let world_dir = std::env::temp_dir().join(format!(
            "icraft_previous_player_save_{}_{}",
            std::process::id(),
            unique
        ));
        fs::create_dir_all(&world_dir).unwrap();
        fs::write(
            world_dir.join("level.dat"),
            bincode::serialize(&LevelData {
                seed: 7,
                time: 9000,
                spawn_x: 8,
                spawn_y: 80,
                spawn_z: 8,
                spawn_dimension: Dimension::Overworld,
                spawn_yaw: 0.0,
                version: 2,
            })
            .unwrap(),
        )
        .unwrap();
        fs::write(world_dir.join("player.dat"), legacy_fixture).unwrap();

        let manager = SaveManager::new(&world_dir);
        let (_, migrated) = manager.load_player_and_level().unwrap();
        assert_eq!(migrated.experience, 42);
        assert_eq!(
            migrated.inventory.hotbar[0].as_ref().unwrap().item,
            Item::Stone
        );
        assert!(migrated.inventory.dragged.is_none());
        assert!(migrated.inventory.creative_drag_origin.is_none());
        fs::remove_dir_all(world_dir).unwrap();
    }

    #[test]
    fn saved_chunk_restores_player_placed_blocks() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let world_dir = std::env::temp_dir().join(format!(
            "icraft_chunk_save_{}_{}",
            std::process::id(),
            unique
        ));

        let mut original = Chunk::new(0, 0);
        original.set_block_local(8, 100, 8, BlockType::Brick);

        let mut manager = SaveManager::new(&world_dir);
        manager
            .save_chunk(0, 0, ChunkSaveData::from_chunk(&original))
            .unwrap();

        let saved = manager.load_chunk(0, 0).expect("saved chunk should load");
        let mut restored = Chunk::new(0, 0);
        saved.restore_to_chunk(&mut restored);

        assert_eq!(restored.get_block_local(8, 100, 8), BlockType::Brick);

        fs::remove_dir_all(world_dir).unwrap();
    }

    #[test]
    fn dimension_chunk_namespaces_are_independent() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let world_dir = std::env::temp_dir().join(format!(
            "icraft_dimension_save_{}_{}",
            std::process::id(),
            unique
        ));
        let mut manager = SaveManager::new(&world_dir);
        let cases = [
            (crate::dimension::Dimension::Overworld, BlockType::Brick),
            (crate::dimension::Dimension::Nether, BlockType::Netherrack),
            (crate::dimension::Dimension::End, BlockType::EndStone),
        ];

        for (dimension, marker) in cases {
            let mut chunk = Chunk::new(4, -3);
            chunk.set_block_local(7, 90, 11, marker);
            manager
                .save_chunk_in(dimension, 4, -3, ChunkSaveData::from_chunk(&chunk))
                .unwrap();
        }

        drop(manager);
        let mut manager = SaveManager::new(&world_dir);
        for (dimension, marker) in cases {
            let saved = manager
                .load_chunk_in(dimension, 4, -3)
                .expect("dimension chunk should load");
            let mut restored = Chunk::new(4, -3);
            saved.restore_to_chunk(&mut restored);
            assert_eq!(restored.get_block_local(7, 90, 11), marker);
        }

        assert!(world_dir.join("regions/r.0.-1.bin").exists());
        assert!(world_dir
            .join("dimensions/nether/regions/r.0.-1.bin")
            .exists());
        assert!(world_dir.join("dimensions/end/regions/r.0.-1.bin").exists());
        fs::remove_dir_all(world_dir).unwrap();
    }

    #[test]
    fn current_dimension_sidecar_roundtrips_and_defaults_to_overworld() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let world_dir = std::env::temp_dir().join(format!(
            "icraft_dimension_state_{}_{}",
            std::process::id(),
            unique
        ));
        let manager = SaveManager::new(&world_dir);
        assert_eq!(
            manager.load_current_dimension(),
            crate::dimension::Dimension::Overworld
        );
        manager
            .save_current_dimension(crate::dimension::Dimension::End)
            .unwrap();
        assert_eq!(
            manager.load_current_dimension(),
            crate::dimension::Dimension::End
        );
        fs::remove_dir_all(world_dir).unwrap();
    }

    #[test]
    fn redstone_metadata_sidecar_roundtrips_through_save_and_load() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let world_dir = std::env::temp_dir().join(format!(
            "icraft_redstone_sidecar_{}_{}",
            std::process::id(),
            unique
        ));

        let chunk = Chunk::new(-2, 5);
        let metadata = vec![crate::redstone::RedstoneComponentMetadata {
            local_x: 3,
            local_y: 100,
            local_z: 7,
            facing: crate::redstone::SavedDirection::East,
            repeater_delay: 4,
            comparator_mode: crate::redstone::SavedComparatorMode::Subtract,
            note: 12,
        }];
        let mut manager = SaveManager::new(&world_dir);
        manager
            .save_chunk_in(
                crate::dimension::Dimension::Overworld,
                -2,
                5,
                ChunkSaveData::from_chunk_with_redstone(&chunk, &metadata),
            )
            .unwrap();

        let saved = manager
            .load_chunk_in(crate::dimension::Dimension::Overworld, -2, 5)
            .expect("redstone sidecar chunk should load");
        assert_eq!(saved.redstone_metadata(), metadata);

        fs::remove_dir_all(world_dir).unwrap();
    }

    #[test]
    fn chunk_saved_without_redstone_sidecar_loads_as_empty_metadata() {
        // Older saves written before the redstone metadata sidecar existed
        // must deserialize cleanly and report an empty metadata vector. Build a
        // `ChunkSaveData` without the sidecar by serializing the historical
        // shape directly, then confirm `redstone_metadata()` degrades
        // gracefully.
        #[derive(serde::Serialize)]
        struct LegacyChunkSaveData {
            chunk_x: i32,
            chunk_z: i32,
            blocks: Vec<u8>,
            sky_light: Vec<u8>,
            block_light: Vec<u8>,
            fluid_levels: Vec<u8>,
        }

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let world_dir = std::env::temp_dir().join(format!(
            "icraft_redstone_legacy_{}_{}",
            std::process::id(),
            unique
        ));

        let chunk = Chunk::new(0, 0);
        let mut manager = SaveManager::new(&world_dir);
        let legacy = LegacyChunkSaveData {
            chunk_x: chunk.chunk_x,
            chunk_z: chunk.chunk_z,
            blocks: Vec::new(),
            sky_light: Vec::new(),
            block_light: Vec::new(),
            fluid_levels: Vec::new(),
        };
        let region = crate::save::RegionData {
            chunks: [((0u8, 0u8), bincode::serialize(&legacy).unwrap())]
                .into_iter()
                .collect(),
        };
        let region_bytes = bincode::serialize(&region).unwrap();
        std::fs::create_dir_all(world_dir.join("regions")).unwrap();
        std::fs::write(world_dir.join("regions/r.0.0.bin"), region_bytes).unwrap();

        let saved = manager
            .load_chunk_in(crate::dimension::Dimension::Overworld, 0, 0)
            .expect("legacy chunk should load");
        assert!(saved.redstone_metadata().is_empty());
        assert!(saved.block_states().is_empty());

        let mut restored = Chunk::new(0, 0);
        saved.restore_to_chunk(&mut restored);
        assert_eq!(restored.get_block_state(0, 64, 0), 0);

        fs::remove_dir_all(world_dir).unwrap();
    }

    #[test]
    fn block_states_roundtrip_and_restore() {
        let mut chunk = Chunk::new(1, 1);
        chunk.set_block_local(5, 64, 5, BlockType::OakDoor);
        chunk.set_block_local(7, 65, 7, BlockType::Torch);
        chunk.set_block_state(5, 64, 5, 0b0000_1101); // facing East, top, right hinge

        let save_data = ChunkSaveData::from_chunk(&chunk);
        assert!(!save_data.block_states().is_empty());

        let mut restored = Chunk::new(1, 1);
        save_data.restore_to_chunk(&mut restored);
        assert_eq!(restored.get_block_state(5, 64, 5), 0b0000_1101);
        assert_eq!(restored.get_block(5, 64, 5), BlockType::OakDoor);
        assert!(restored
            .torch_positions()
            .iter()
            .any(|&position| Chunk::decode_torch_position(position) == (7, 65, 7)));
    }

    #[test]
    fn test_entity_save_data_roundtrip() {
        use crate::entity::{Entity, EntityType};
        use glam::Vec3;

        let mut entity = Entity::new(42, EntityType::Pig, Vec3::new(10.5, 64.0, -15.2));
        entity.health = 7.5;
        entity.age = -120.0;
        entity.has_wool = true;

        let save_data = EntitySaveData::from(&entity);
        assert_eq!(save_data.entity_type, EntityType::Pig);
        assert_eq!(save_data.position, [10.5, 64.0, -15.2]);

        let restored = save_data.to_entity(100);
        assert_eq!(restored.id, 100);
        assert_eq!(restored.entity_type, EntityType::Pig);
        assert_eq!(restored.position, Vec3::new(10.5, 64.0, -15.2));
        assert_eq!(restored.health, 7.5);
        assert_eq!(restored.age, -120.0);
    }

    #[test]
    fn test_save_manager_entities_persistence() {
        let temp_dir =
            std::env::temp_dir().join(format!("icraft_test_entities_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        let manager = SaveManager::new(&temp_dir);

        let test_entity = crate::entity::Entity::new(
            1,
            crate::entity::EntityType::Zombie,
            glam::Vec3::new(1.0, 65.0, 2.0),
        );
        let test_data = vec![EntitySaveData::from(&test_entity)];

        manager
            .save_entities_in(crate::dimension::Dimension::Overworld, &test_data)
            .unwrap();

        let loaded = manager.load_entities_in(crate::dimension::Dimension::Overworld);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].entity_type, crate::entity::EntityType::Zombie);
        assert_eq!(loaded[0].position, [1.0, 65.0, 2.0]);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    fn unique_test_dir(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("icraft_{label}_{}_{}", std::process::id(), unique))
    }

    fn unstarted_save_queue(capacity: usize) -> SaveQueue {
        SaveQueue {
            inner: Arc::new(SaveQueueInner {
                state: Mutex::new(SaveQueueState::default()),
                work_available: Condvar::new(),
                capacity_available: Condvar::new(),
                capacity,
                stats: Arc::new(SaveQueueStats::default()),
                last_error: Mutex::new(None),
                producers: AtomicU64::new(1),
            }),
        }
    }

    #[test]
    fn stale_ack_cannot_clear_a_newer_dirty_revision() {
        let tracker = DirtyChunkSet::new();
        let first = tracker.mark_dirty(2, -4);
        assert!(tracker.begin_save(2, -4, first));
        let second = tracker.mark_dirty(2, -4);

        tracker.acknowledge_persisted(2, -4, first);
        assert_eq!(tracker.state(2, -4), Some(SaveState::Dirty(second)));

        assert!(tracker.begin_save(2, -4, second));
        tracker.acknowledge_persisted(2, -4, second);
        assert_eq!(tracker.state(2, -4), Some(SaveState::Persisted(second)));
    }

    #[test]
    fn latest_revision_replaces_older_pending_snapshot() {
        let queue = unstarted_save_queue(1);
        let tracker = DirtyChunkSet::new();
        let chunk = Chunk::new(1, 2);

        let first = tracker.mark_dirty(1, 2);
        assert!(tracker.begin_save(1, 2, first));
        queue
            .send(SaveCommand::SaveChunk {
                snapshot: UncompressedChunkSnapshot::from_chunk_with_redstone(
                    crate::dimension::Dimension::Overworld,
                    &chunk,
                    Vec::new(),
                ),
                revision: first,
                tracker: tracker.clone(),
            })
            .unwrap();

        let second = tracker.mark_dirty(1, 2);
        assert!(tracker.begin_save(1, 2, second));
        queue
            .send(SaveCommand::SaveChunk {
                snapshot: UncompressedChunkSnapshot::from_chunk_with_redstone(
                    crate::dimension::Dimension::Overworld,
                    &chunk,
                    Vec::new(),
                ),
                revision: second,
                tracker,
            })
            .unwrap();

        let state = queue
            .inner
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        assert_eq!(state.pending_chunks.len(), 1);
        assert_eq!(
            state.pending_chunks.values().next().unwrap().revision,
            second
        );
        assert_eq!(queue.stats().depth(), 1);
        assert_eq!(queue.stats().dropped(), 1);
    }

    #[test]
    fn paused_worker_sustained_latest_wins_stays_bounded_in_items_and_bytes() {
        let queue = unstarted_save_queue(2);
        let tracker = DirtyChunkSet::new();
        let chunk = Chunk::new(1, 2);
        let base = UncompressedChunkSnapshot::from_chunk_with_redstone(
            crate::dimension::Dimension::Overworld,
            &chunk,
            Vec::new(),
        );
        let metadata = crate::redstone::RedstoneComponentMetadata {
            local_x: 1,
            local_y: 64,
            local_z: 1,
            facing: crate::redstone::SavedDirection::East,
            repeater_delay: 2,
            comparator_mode: crate::redstone::SavedComparatorMode::Subtract,
            note: 7,
        };
        let mut larger = base.clone();
        larger.redstone_metadata = vec![metadata; 32];

        const MUTATIONS: u64 = 64;
        for _ in 1..=MUTATIONS {
            let revision = tracker.mark_dirty(1, 2);
            assert!(tracker.begin_save(1, 2, revision));
            let snapshot = if revision % 2 == 0 {
                larger.clone()
            } else {
                base.clone()
            }
            .with_mutation_revision(revision);
            let expected_bytes = snapshot.estimated_bytes();

            queue
                .send(SaveCommand::SaveChunk {
                    snapshot,
                    revision,
                    tracker: tracker.clone(),
                })
                .unwrap();

            let stats = queue.stats();
            assert_eq!(stats.depth(), 1, "latest-wins must retain one queued item");
            assert_eq!(stats.in_flight(), 0, "the worker remains paused");
            assert_eq!(stats.in_flight_bytes(), 0);
            assert_eq!(stats.queued_bytes(), expected_bytes);
            assert!(stats.queued_bytes() <= larger.estimated_bytes());

            let state = queue
                .inner
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            assert_eq!(state.pending_chunks.len(), 1);
            let pending = state.pending_chunks.values().next().unwrap();
            assert_eq!(pending.revision, revision);
            assert_eq!(pending.bytes, expected_bytes);
        }

        assert_eq!(queue.stats().dropped(), MUTATIONS - 1);
    }

    #[test]
    fn flush_propagates_io_failure_and_keeps_chunk_dirty_for_retry() {
        let world_dir = unique_test_dir("save_flush_failure");
        let manager = Arc::new(Mutex::new(SaveManager::new(&world_dir)));
        fs::remove_dir_all(world_dir.join("regions")).unwrap();
        File::create(world_dir.join("regions")).unwrap();

        let queue = spawn_save_worker(Arc::clone(&manager), 2);
        let tracker = DirtyChunkSet::new();
        let revision = tracker.mark_dirty(0, 0);
        assert!(tracker.begin_save(0, 0, revision));
        let chunk = Chunk::new(0, 0);
        queue
            .send(SaveCommand::SaveChunk {
                snapshot: UncompressedChunkSnapshot::from_chunk_with_redstone(
                    crate::dimension::Dimension::Overworld,
                    &chunk,
                    Vec::new(),
                ),
                revision,
                tracker: tracker.clone(),
            })
            .unwrap();
        let (ack_tx, ack_rx) = std::sync::mpsc::channel();
        queue.send(SaveCommand::Flush(ack_tx)).unwrap();

        let result = ack_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("save worker did not ACK flush");
        assert!(result.is_err());
        assert_eq!(tracker.state(0, 0), Some(SaveState::Dirty(revision)));
        assert_eq!(queue.stats().depth(), 1);
        assert_eq!(queue.stats().in_flight(), 0);

        drop(queue);
        drop(manager);
        fs::remove_file(world_dir.join("regions")).unwrap();
        fs::remove_dir_all(world_dir).unwrap();
    }

    #[test]
    fn corrupt_existing_region_is_never_overwritten() {
        let world_dir = unique_test_dir("region_corruption");
        let mut manager = SaveManager::new(&world_dir);
        let first = Chunk::new(0, 0);
        manager
            .save_chunk(0, 0, ChunkSaveData::from_chunk(&first))
            .unwrap();

        let region_path = world_dir.join("regions/r.0.0.bin");
        let corrupt_bytes = b"not a bincode region".to_vec();
        fs::write(&region_path, &corrupt_bytes).unwrap();

        let second = Chunk::new(1, 0);
        let error = manager
            .save_chunk(1, 0, ChunkSaveData::from_chunk(&second))
            .unwrap_err();
        assert!(matches!(error, SaveError::RegionCorruption { .. }));
        assert_eq!(fs::read(&region_path).unwrap(), corrupt_bytes);

        fs::remove_dir_all(world_dir).unwrap();
    }

    #[test]
    fn atomic_replace_faults_leave_a_complete_old_or_new_file() {
        let world_dir = unique_test_dir("atomic_replace");
        fs::create_dir_all(&world_dir).unwrap();
        let path = world_dir.join("level.dat");
        atomic_write(&path, b"old complete value").unwrap();

        ATOMIC_WRITE_FAILPOINT.with(|failpoint| failpoint.set(1));
        assert!(atomic_write(&path, b"new complete value").is_err());
        assert_eq!(fs::read(&path).unwrap(), b"old complete value");

        ATOMIC_WRITE_FAILPOINT.with(|failpoint| failpoint.set(2));
        assert!(atomic_write(&path, b"new complete value").is_err());
        assert_eq!(fs::read(&path).unwrap(), b"new complete value");
        ATOMIC_WRITE_FAILPOINT.with(|failpoint| failpoint.set(0));

        assert!(fs::read_dir(&world_dir).unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".tmp")));
        fs::remove_dir_all(world_dir).unwrap();
    }

    #[test]
    fn atomic_replace_survives_process_crash_before_and_after_replace() {
        let world_dir = unique_test_dir("atomic_process_crash");
        fs::create_dir_all(&world_dir).unwrap();
        let path = world_dir.join("level.dat");
        atomic_write(&path, b"old complete value").unwrap();
        let test_binary = std::env::current_exe().unwrap();

        let before = std::process::Command::new(&test_binary)
            .args([
                "--ignored",
                "--exact",
                "save::tests::atomic_replace_crash_child",
            ])
            .env("ICRAFT_TEST_ATOMIC_CRASH_STAGE", "before_replace")
            .env("ICRAFT_TEST_ATOMIC_CRASH_PATH", &path)
            .output()
            .unwrap();
        assert!(!before.status.success());
        assert_eq!(fs::read(&path).unwrap(), b"old complete value");

        let after = std::process::Command::new(&test_binary)
            .args([
                "--ignored",
                "--exact",
                "save::tests::atomic_replace_crash_child",
            ])
            .env("ICRAFT_TEST_ATOMIC_CRASH_STAGE", "after_replace")
            .env("ICRAFT_TEST_ATOMIC_CRASH_PATH", &path)
            .output()
            .unwrap();
        assert!(!after.status.success());
        assert_eq!(fs::read(&path).unwrap(), b"new complete value");

        fs::remove_dir_all(world_dir).unwrap();
    }

    fn same_region_snapshot(cx: i32, cz: i32, marker: BlockType) -> UncompressedChunkSnapshot {
        let mut chunk = Chunk::new(cx, cz);
        chunk.set_block_local(0, 64, 0, marker);
        UncompressedChunkSnapshot::from_chunk_with_redstone(
            crate::dimension::Dimension::Overworld,
            &chunk,
            Vec::new(),
        )
    }

    fn assert_saved_marker(manager: &mut SaveManager, cx: i32, cz: i32, marker: BlockType) {
        let saved = manager
            .load_chunk_in(crate::dimension::Dimension::Overworld, cx, cz)
            .expect("same-region chunk should load after restart");
        let mut restored = Chunk::new(cx, cz);
        saved.restore_to_chunk(&mut restored);
        assert_eq!(restored.get_block_local(0, 64, 0), marker);
    }

    fn save_same_region_new_snapshots(world_dir: &Path) {
        let mut manager = SaveManager::new(world_dir);
        let snapshots = [
            same_region_snapshot(0, 0, BlockType::Obsidian),
            same_region_snapshot(1, 0, BlockType::StoneBrick),
        ];
        manager
            .save_chunks_batch_in(crate::dimension::Dimension::Overworld, &snapshots)
            .unwrap();
    }

    #[test]
    fn same_region_batch_faults_replace_atomically_and_preserve_sibling_on_restart() {
        let world_dir = unique_test_dir("same_region_batch_faults");
        let old_snapshots = [
            same_region_snapshot(0, 0, BlockType::Brick),
            same_region_snapshot(1, 0, BlockType::Cobblestone),
        ];
        let new_snapshots = [
            same_region_snapshot(0, 0, BlockType::Obsidian),
            same_region_snapshot(1, 0, BlockType::StoneBrick),
        ];

        let mut manager = SaveManager::new(&world_dir);
        manager
            .save_chunks_batch_in(crate::dimension::Dimension::Overworld, &old_snapshots)
            .unwrap();
        drop(manager);

        let mut manager = SaveManager::new(&world_dir);
        ATOMIC_WRITE_FAILPOINT.with(|failpoint| failpoint.set(1));
        assert!(matches!(
            manager.save_chunks_batch_in(crate::dimension::Dimension::Overworld, &new_snapshots),
            Err(SaveError::Io { .. })
        ));
        ATOMIC_WRITE_FAILPOINT.with(|failpoint| failpoint.set(0));
        drop(manager);

        let mut restarted = SaveManager::new(&world_dir);
        assert_saved_marker(&mut restarted, 0, 0, BlockType::Brick);
        assert_saved_marker(&mut restarted, 1, 0, BlockType::Cobblestone);
        drop(restarted);

        let mut manager = SaveManager::new(&world_dir);
        ATOMIC_WRITE_FAILPOINT.with(|failpoint| failpoint.set(2));
        assert!(matches!(
            manager.save_chunks_batch_in(crate::dimension::Dimension::Overworld, &new_snapshots),
            Err(SaveError::Io { .. })
        ));
        ATOMIC_WRITE_FAILPOINT.with(|failpoint| failpoint.set(0));
        drop(manager);

        let mut restarted = SaveManager::new(&world_dir);
        assert_saved_marker(&mut restarted, 0, 0, BlockType::Obsidian);
        assert_saved_marker(&mut restarted, 1, 0, BlockType::StoneBrick);
        fs::remove_dir_all(world_dir).unwrap();
    }

    #[test]
    fn same_region_batch_survives_process_crash_before_and_after_replace() {
        let world_dir = unique_test_dir("same_region_batch_crash");
        let old_snapshots = [
            same_region_snapshot(0, 0, BlockType::Brick),
            same_region_snapshot(1, 0, BlockType::Cobblestone),
        ];
        let mut manager = SaveManager::new(&world_dir);
        manager
            .save_chunks_batch_in(crate::dimension::Dimension::Overworld, &old_snapshots)
            .unwrap();
        drop(manager);

        let test_binary = std::env::current_exe().unwrap();
        let before = std::process::Command::new(&test_binary)
            .args([
                "--ignored",
                "--exact",
                "save::tests::same_region_batch_crash_child",
            ])
            .env("ICRAFT_TEST_ATOMIC_CRASH_STAGE", "before_replace")
            .env("ICRAFT_TEST_ATOMIC_CRASH_WORLD", &world_dir)
            .output()
            .unwrap();
        assert!(!before.status.success());

        let mut restarted = SaveManager::new(&world_dir);
        assert_saved_marker(&mut restarted, 0, 0, BlockType::Brick);
        assert_saved_marker(&mut restarted, 1, 0, BlockType::Cobblestone);
        drop(restarted);

        let after = std::process::Command::new(&test_binary)
            .args([
                "--ignored",
                "--exact",
                "save::tests::same_region_batch_crash_child",
            ])
            .env("ICRAFT_TEST_ATOMIC_CRASH_STAGE", "after_replace")
            .env("ICRAFT_TEST_ATOMIC_CRASH_WORLD", &world_dir)
            .output()
            .unwrap();
        assert!(!after.status.success());

        let mut restarted = SaveManager::new(&world_dir);
        assert_saved_marker(&mut restarted, 0, 0, BlockType::Obsidian);
        assert_saved_marker(&mut restarted, 1, 0, BlockType::StoneBrick);
        fs::remove_dir_all(world_dir).unwrap();
    }

    #[test]
    #[ignore = "helper subprocess for atomic_replace_survives_process_crash_before_and_after_replace"]
    fn atomic_replace_crash_child() {
        let path = std::env::var_os("ICRAFT_TEST_ATOMIC_CRASH_PATH")
            .map(PathBuf::from)
            .expect("missing crash-test path");
        atomic_write(path, b"new complete value").unwrap();
        panic!("atomic crash failpoint did not abort");
    }

    #[test]
    #[ignore = "helper subprocess for same_region_batch_survives_process_crash_before_and_after_replace"]
    fn same_region_batch_crash_child() {
        let world_dir = std::env::var_os("ICRAFT_TEST_ATOMIC_CRASH_WORLD")
            .map(PathBuf::from)
            .expect("missing crash-test world path");
        save_same_region_new_snapshots(&world_dir);
        panic!("atomic crash failpoint did not abort");
    }

    #[test]
    fn missing_region_load_does_not_create_phantom_lru_keys() {
        let world_dir = unique_test_dir("region_lru");
        let mut manager = SaveManager::new(&world_dir);
        assert!(manager.load_chunk(1024, 1024).is_none());
        assert!(manager.lru_order.is_empty());
        fs::remove_dir_all(world_dir).unwrap();
    }

    #[test]
    fn enqueue_failure_restores_in_flight_revision_to_dirty() {
        let queue = unstarted_save_queue(1);
        queue
            .inner
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .closed = true;
        let tracker = DirtyChunkSet::new();
        let revision = tracker.mark_dirty(0, 0);
        assert!(tracker.begin_save(0, 0, revision));
        let chunk = Chunk::new(0, 0);
        let error = queue
            .send(SaveCommand::SaveChunk {
                snapshot: UncompressedChunkSnapshot::from_chunk_with_redstone(
                    crate::dimension::Dimension::Overworld,
                    &chunk,
                    Vec::new(),
                ),
                revision,
                tracker: tracker.clone(),
            })
            .unwrap_err();
        assert_eq!(error, SaveError::QueueClosed);
        assert_eq!(tracker.state(0, 0), Some(SaveState::Dirty(revision)));
    }

    #[test]
    fn worker_panic_is_acked_as_error_and_snapshot_is_retained() {
        let world_dir = unique_test_dir("save_worker_panic");
        let manager = Arc::new(Mutex::new(SaveManager::new(&world_dir)));
        manager
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .inject_worker_panic_once();
        let queue = spawn_save_worker(Arc::clone(&manager), 2);
        let tracker = DirtyChunkSet::new();
        let revision = tracker.mark_dirty(0, 0);
        assert!(tracker.begin_save(0, 0, revision));
        let chunk = Chunk::new(0, 0);
        queue
            .send(SaveCommand::SaveChunk {
                snapshot: UncompressedChunkSnapshot::from_chunk_with_redstone(
                    crate::dimension::Dimension::Overworld,
                    &chunk,
                    Vec::new(),
                ),
                revision,
                tracker: tracker.clone(),
            })
            .unwrap();
        let (ack_tx, ack_rx) = std::sync::mpsc::channel();
        queue.send(SaveCommand::Flush(ack_tx)).unwrap();
        let error = ack_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap()
            .unwrap_err();
        assert!(matches!(error, SaveError::WorkerPanic(_)));
        assert_eq!(tracker.state(0, 0), Some(SaveState::Dirty(revision)));
        assert_eq!(queue.stats().depth(), 1);
        drop(queue);
        drop(manager);
        fs::remove_dir_all(world_dir).unwrap();
    }

    #[test]
    fn serialization_failure_is_propagated_and_retryable() {
        let world_dir = unique_test_dir("save_serialize_failure");
        let manager = Arc::new(Mutex::new(SaveManager::new(&world_dir)));
        manager
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .inject_serialization_failure_once();
        let queue = spawn_save_worker(Arc::clone(&manager), 2);
        let tracker = DirtyChunkSet::new();
        let revision = tracker.mark_dirty(0, 0);
        assert!(tracker.begin_save(0, 0, revision));
        let chunk = Chunk::new(0, 0);
        queue
            .send(SaveCommand::SaveChunk {
                snapshot: UncompressedChunkSnapshot::from_chunk_with_redstone(
                    crate::dimension::Dimension::Overworld,
                    &chunk,
                    Vec::new(),
                ),
                revision,
                tracker: tracker.clone(),
            })
            .unwrap();
        let (ack_tx, ack_rx) = std::sync::mpsc::channel();
        queue.send(SaveCommand::Flush(ack_tx)).unwrap();
        let error = ack_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap()
            .unwrap_err();
        assert!(matches!(error, SaveError::Serialization(_)));
        assert_eq!(tracker.state(0, 0), Some(SaveState::Dirty(revision)));

        let (retry_tx, retry_rx) = std::sync::mpsc::channel();
        queue.send(SaveCommand::Flush(retry_tx)).unwrap();
        retry_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap()
            .unwrap();
        assert_eq!(tracker.state(0, 0), Some(SaveState::Persisted(revision)));
        drop(queue);
        drop(manager);
        fs::remove_dir_all(world_dir).unwrap();
    }

    #[test]
    fn salvage_copies_only_readable_chunks_without_mutating_source() {
        let world_dir = unique_test_dir("region_salvage");
        let manager = SaveManager::new(&world_dir);
        let source = world_dir.join("corrupt-region.bin");
        let destination = world_dir.join("salvaged-region.bin");
        let valid = bincode::serialize(&ChunkSaveData::from_chunk(&Chunk::new(0, 0))).unwrap();
        let region = RegionData {
            chunks: [((0, 0), valid), ((1, 0), b"broken chunk".to_vec())]
                .into_iter()
                .collect(),
        };
        let source_bytes = bincode::serialize(&region).unwrap();
        fs::write(&source, &source_bytes).unwrap();

        assert_eq!(
            manager
                .salvage_readable_region(&source, &destination)
                .unwrap(),
            1
        );
        assert_eq!(fs::read(&source).unwrap(), source_bytes);
        let salvaged: RegionData = bincode::deserialize(&fs::read(destination).unwrap()).unwrap();
        assert_eq!(salvaged.chunks.len(), 1);
        assert!(salvaged.chunks.contains_key(&(0, 0)));
        fs::remove_dir_all(world_dir).unwrap();
    }

    #[test]
    fn mutation_index_and_unloaded_snapshot_source_survive_reload() {
        fn wait_payload(worker: &NetworkSnapshotWorker) -> NetworkSnapshotPayload {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
            loop {
                if let Some(NetworkSnapshotWorkerResult::Snapshot(payload)) =
                    worker.try_iter().next()
                {
                    return payload;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "unloaded snapshot worker timed out"
                );
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }

        let world_dir = unique_test_dir("network_revision_index");
        let mut manager = SaveManager::new(&world_dir);
        let chunk = Chunk::new(7, -4);
        let mut saved = ChunkSaveData::from_chunk(&chunk);
        saved.mutation_revision = 1;
        let expected_blocks = saved.blocks.clone();
        manager
            .save_chunk_in(crate::dimension::Dimension::Overworld, 7, -4, saved)
            .unwrap();

        let mut index = MutationRevisionIndex::default();
        assert_eq!(
            index
                .bump(crate::dimension::Dimension::Overworld, 7, -4)
                .unwrap(),
            1
        );
        assert_eq!(
            index
                .bump(crate::dimension::Dimension::Overworld, 7, -4)
                .unwrap(),
            2
        );
        manager.save_mutation_revision_index(&index).unwrap();
        drop(manager);

        let manager = Arc::new(Mutex::new(SaveManager::new(&world_dir)));
        let reloaded = manager.lock().unwrap().load_mutation_revision_index();
        assert_eq!(
            reloaded.latest(crate::dimension::Dimension::Overworld, 7, -4),
            2
        );
        let worker = spawn_network_snapshot_worker(Arc::clone(&manager), 1);
        let key = NetworkSnapshotKey {
            player_id: 11,
            dimension: crate::dimension::Dimension::Overworld,
            cx: 7,
            cz: -4,
            revision: 2,
        };
        worker
            .try_submit(NetworkSnapshotRequest { key, chunk: None })
            .unwrap();
        let stale = wait_payload(&worker);
        assert_eq!(stale.key, key);
        assert!(
            stale.result.unwrap_err().contains("waiting for 2"),
            "persisted revision must not be mislabeled as current"
        );

        let mut current = ChunkSaveData::from_chunk(&chunk);
        current.mutation_revision = 2;
        manager
            .lock()
            .unwrap()
            .save_chunk_in(crate::dimension::Dimension::Overworld, 7, -4, current)
            .unwrap();
        worker
            .try_submit(NetworkSnapshotRequest { key, chunk: None })
            .unwrap();
        let payload = wait_payload(&worker);
        assert_eq!(payload.key, key);
        assert_eq!(payload.result.unwrap().0, expected_blocks);
        fs::remove_dir_all(world_dir).unwrap();
    }

    #[test]
    fn mutation_revision_index_refuses_new_coordinate_at_capacity() {
        let dimension = crate::dimension::Dimension::Overworld;
        let mut index = MutationRevisionIndex::with_capacity_limit(2);

        assert_eq!(index.bump(dimension, 1, 1).unwrap(), 1);
        assert!(index.ensure_at_least(dimension, 2, 2, 7).unwrap());
        assert_eq!(index.bump(dimension, 1, 1).unwrap(), 2);
        assert!(!index.ensure_at_least(dimension, 2, 2, 3).unwrap());

        assert_eq!(
            index.bump(dimension, 3, 3),
            Err(MutationRevisionIndexCapacityError { capacity: 2 })
        );
        assert_eq!(
            index.ensure_at_least(dimension, 4, 4, 9),
            Err(MutationRevisionIndexCapacityError { capacity: 2 })
        );
        assert_eq!(index.len(), 2);
        assert_eq!(index.latest(dimension, 1, 1), 2);
        assert_eq!(index.latest(dimension, 2, 2), 7);
        assert_eq!(index.latest(dimension, 3, 3), 0);
    }

    #[test]
    fn mutation_revision_index_reclaim_is_revision_safe_and_frees_capacity() {
        let dimension = crate::dimension::Dimension::Nether;
        let mut index = MutationRevisionIndex::with_capacity_limit(1);

        assert!(index.ensure_at_least(dimension, -5, 8, 12).unwrap());
        assert!(!index.reclaim_through(dimension, -5, 8, 11));
        assert_eq!(index.latest(dimension, -5, 8), 12);
        assert!(index.bump(dimension, 9, 9).is_err());

        assert!(index.reclaim_through(dimension, -5, 8, 12));
        assert!(index.is_empty());
        assert_eq!(index.bump(dimension, 9, 9).unwrap(), 1);
        assert_eq!(index.remove(dimension, 9, 9), Some(1));
        assert!(index.is_empty());
    }

    #[test]
    fn mutation_revision_index_roundtrip_preserves_highest_revision() {
        let dimension = crate::dimension::Dimension::End;
        let mut index = MutationRevisionIndex::default();

        assert!(index.ensure_at_least(dimension, 6, -3, 41).unwrap());
        assert!(!index.ensure_at_least(dimension, 6, -3, 17).unwrap());
        let encoded = bincode::serialize(&index).unwrap();
        let reloaded: MutationRevisionIndex = bincode::deserialize(&encoded).unwrap();

        assert_eq!(reloaded.latest(dimension, 6, -3), 41);
        assert_eq!(reloaded.len(), 1);
        assert_eq!(reloaded.capacity(), MUTATION_REVISION_INDEX_CAPACITY);
    }

    #[test]
    fn block_entity_save_and_restore_roundtrip() {
        use crate::block_entity::{BlockEntity, ChestBlockEntity};

        let mut chunk = Chunk::new(0, 0);
        chunk.set_block_local(1, 2, 3, BlockType::Chest);
        let chest_stub = BlockEntity::Chest(ChestBlockEntity {
            inventory: crate::inventory::ContainerInventory::new(),
            custom_name: Some("Secret Stash".to_string()),
            loot_table: None,
            loot_seed: None,
        });
        chunk
            .insert_block_entity(1, 2, 3, chest_stub.clone())
            .unwrap();

        // Roundtrip via ChunkSaveData
        let save_data = ChunkSaveData::from_chunk(&chunk);
        let mut restored = Chunk::new(0, 0);
        save_data.restore_to_chunk(&mut restored);

        assert_eq!(restored.get_block_local(1, 2, 3), BlockType::Chest);
        assert_eq!(restored.get_block_entity(1, 2, 3), Some(&chest_stub));

        // Roundtrip via bincode serialization of ChunkSaveData
        let bytes = bincode::serialize(&save_data).unwrap();
        let loaded_save_data = deserialize_chunk_save_data(&bytes).unwrap();
        let mut reloaded = Chunk::new(0, 0);
        loaded_save_data.restore_to_chunk(&mut reloaded);

        assert_eq!(reloaded.get_block_entity(1, 2, 3), Some(&chest_stub));
    }

    #[test]
    fn test_plan04_save_roundtrip_and_migration() {
        // 1. LevelData roundtrip
        let level = LevelData {
            seed: 12345,
            time: 6000,
            spawn_x: 100,
            spawn_y: 65,
            spawn_z: -200,
            spawn_dimension: crate::dimension::Dimension::Overworld,
            spawn_yaw: 90.0,
            version: 2,
        };
        let bytes = bincode::serialize(&level).unwrap();
        let restored_level: LevelData = bincode::deserialize(&bytes).unwrap();
        assert_eq!(restored_level.spawn_x, 100);
        assert_eq!(restored_level.spawn_y, 65);
        assert_eq!(restored_level.spawn_z, -200);

        // 2. EntitySaveData dropped_stack migration
        let mut stack = crate::inventory::ItemStack::new(crate::inventory::Item::DiamondSword, 1);
        stack.durability = 100;

        let mut entity = crate::entity::Entity::new(
            1,
            crate::entity::EntityType::DroppedItem,
            glam::Vec3::new(10.0, 64.0, 10.0),
        );
        entity.dropped_stack = Some(stack.clone());
        entity.dropped_item = Some(crate::inventory::Item::DiamondSword);
        entity.dropped_count = 1;

        let save_entity = EntitySaveData::from(&entity);
        let restored_entity = save_entity.to_entity(1);
        let restored_stack = restored_entity.dropped_stack.unwrap();
        assert_eq!(restored_stack.item, crate::inventory::Item::DiamondSword);
        assert_eq!(restored_stack.durability, 100);

        // Pet and entity save data test (Plan 11)
        let mut wolf = crate::entity::Entity::new(
            2,
            crate::entity::EntityType::Wolf,
            glam::Vec3::new(5.0, 64.0, 5.0),
        );
        wolf.is_tamed = true;
        wolf.is_sitting = true;
        wolf.owner_id = Some(42);
        wolf.collar_color = [0.0, 0.0, 1.0]; // Blue collar

        let wolf_save = EntitySaveData::from(&wolf);
        let wolf_bytes = bincode::serialize(&wolf_save).unwrap();
        let restored_wolf_save: EntitySaveData = bincode::deserialize(&wolf_bytes).unwrap();
        let restored_wolf = restored_wolf_save.to_entity(2);

        assert!(restored_wolf.is_tamed);
        assert!(restored_wolf.is_sitting);
        assert_eq!(restored_wolf.owner_id, Some(42));
        assert_eq!(restored_wolf.collar_color, [0.0, 0.0, 1.0]);

        // 3. PlayerData spawn_point
        let mut player_state = crate::player::PlayerState::new();
        player_state.spawn_point = Some([12, 64, -15]);
        player_state.spawn_dimension = Some(crate::dimension::Dimension::Overworld);

        let inv = crate::inventory::Inventory::new();
        let adv = crate::advancements::AdvancementProgressData::default();
        let player_data = PlayerData::from_state(
            glam::Vec3::ZERO,
            glam::Vec3::ZERO,
            0.0,
            0.0,
            &player_state,
            crate::save::GameMode::Survival,
            &inv,
            adv,
        );

        let bytes = bincode::serialize(&player_data).unwrap();
        let restored_player: PlayerData = bincode::deserialize(&bytes).unwrap();
        assert_eq!(restored_player.spawn_point, Some([12, 64, -15]));
        assert_eq!(
            restored_player.spawn_dimension,
            Some(crate::dimension::Dimension::Overworld)
        );
    }

    #[test]
    fn offhand_save_roundtrip_and_legacy_migration() {
        let mut inv = crate::inventory::Inventory::new();
        let shield = crate::inventory::ItemStack::new(crate::inventory::Item::Shield, 1);
        inv.offhand = Some(shield);

        let data = InventoryData::from(&inv);
        assert_eq!(
            data.offhand.as_ref().unwrap().item,
            crate::inventory::Item::Shield
        );

        let restored = data.to_inventory();
        assert_eq!(
            restored.offhand.unwrap().item,
            crate::inventory::Item::Shield
        );

        // Test LegacyInventoryData conversion backward compatibility (where legacy data has no offhand field)
        let legacy = LegacyInventoryData {
            hotbar: vec![],
            main: vec![],
            armor: vec![],
            selected: 0,
        };
        let migrated = InventoryData::from(legacy);
        assert!(migrated.offhand.is_none());
    }

    #[test]
    fn migration_fixture_legacy_0_to_255_preserves_data_and_creates_backup() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let world_dir = std::env::temp_dir().join(format!(
            "icraft_migration_test_{}_{}",
            std::process::id(),
            unique
        ));

        // 1. Create a legacy 256-height format ChunkSaveData fixture (data_version = 0)
        // representing a historical save at chunk (0, 0).
        let total_voxels_256 = 16 * 256 * 16;
        let mut legacy_blocks = vec![0u8; total_voxels_256];
        let mut legacy_states = vec![0u8; total_voxels_256];
        let mut legacy_sky = vec![15u8; total_voxels_256];
        let mut legacy_block_light = vec![0u8; total_voxels_256];
        let mut legacy_fluid = vec![0u8; total_voxels_256];

        // Place custom blocks at y = 10, y = 100, y = 255
        // Index formula in legacy flat array: (x * 256 + y) * 16 + z
        let idx_y10 = (8 * 256 + 10) * 16 + 8;
        let idx_y100 = (4 * 256 + 100) * 16 + 4;
        let idx_y255 = (2 * 256 + 255) * 16 + 2;

        legacy_blocks[idx_y10] = BlockType::DiamondOre as u8;
        legacy_blocks[idx_y100] = BlockType::Obsidian as u8;
        legacy_blocks[idx_y255] = BlockType::GoldOre as u8;
        legacy_states[idx_y100] = 0b00000001; // custom block state bit

        let legacy_save_data = ChunkSaveData {
            chunk_x: 0,
            chunk_z: 0,
            blocks: compress_bytes(&legacy_blocks).unwrap(),
            sky_light: compress_bytes(&legacy_sky).unwrap(),
            block_light: compress_bytes(&legacy_block_light).unwrap(),
            fluid_levels: compress_bytes(&legacy_fluid).unwrap(),
            redstone_metadata: Vec::new(),
            block_states: compress_bytes(&legacy_states).unwrap(),
            mutation_revision: 5,
            block_entities: Vec::new(),
            data_version: 0,
        };

        // 2. Write the legacy save data into region r.0.0.bin manually using SaveManager
        let mut manager = SaveManager::new(&world_dir);
        let region_path = world_dir.join("regions").join("r.0.0.bin");
        let backup_path = world_dir.join("regions").join("r.0.0.bin.bak");

        // Write initial legacy region file
        let legacy_bytes = bincode::serialize(&legacy_save_data).unwrap();
        let mut initial_region = RegionData {
            chunks: std::collections::HashMap::new(),
        };
        initial_region.chunks.insert((0, 0), legacy_bytes);
        let serialized_initial = bincode::serialize(&initial_region).unwrap();
        atomic_write(&region_path, &serialized_initial).unwrap();

        assert!(region_path.exists());
        assert!(!backup_path.exists());

        // 3. Load the chunk via SaveManager in modern Overworld (min_y = -64, height = 384)
        let loaded = manager.load_chunk(0, 0).expect("legacy chunk should load");
        assert_eq!(loaded.data_version, 0);

        let mut modern_chunk = Chunk::empty(0, 0);
        loaded.restore_to_chunk(&mut modern_chunk);

        // Verify Y=0..255 block mapping and states
        assert_eq!(
            modern_chunk.get_block_local(8, 10, 8),
            BlockType::DiamondOre
        );
        assert_eq!(modern_chunk.get_block_local(4, 100, 4), BlockType::Obsidian);
        assert_eq!(modern_chunk.get_block_state(4, 100, 4), 0b00000001);
        assert_eq!(modern_chunk.get_block_local(2, 255, 2), BlockType::GoldOre);

        // Verify sections Y < 0 (-64..-1) and Y >= 256 (256..319) remain unconfigured/empty Air
        for wy in -64..0 {
            assert_eq!(modern_chunk.get_block_local(8, wy, 8), BlockType::Air);
        }
        for wy in 256..384 {
            assert_eq!(modern_chunk.get_block_local(8, wy, 8), BlockType::Air);
        }

        // 4. Modify and re-save chunk to trigger region update and original file backup
        modern_chunk.set_block_local(8, -10, 8, BlockType::Bedrock);
        let updated_save_data = ChunkSaveData::from_chunk(&modern_chunk);
        assert_eq!(updated_save_data.data_version, 2);

        manager.save_chunk(0, 0, updated_save_data).unwrap();

        // Verify original file backup r.0.0.bin.bak was created during migration/save
        assert!(
            backup_path.exists(),
            "original region file backup should exist after migration save"
        );

        let backup_bytes = fs::read(&backup_path).unwrap();
        let backup_region: RegionData = bincode::deserialize(&backup_bytes).unwrap();
        assert!(backup_region.chunks.contains_key(&(0, 0)));

        fs::remove_dir_all(world_dir).unwrap();
    }

    #[test]
    fn test_corrupt_region_file_is_not_overwritten_on_save_failure() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let world_dir = std::env::temp_dir().join(format!(
            "icraft_corrupt_save_test_{}_{}",
            std::process::id(),
            unique
        ));

        let mut manager = SaveManager::new(&world_dir);
        let region_path = world_dir.join("regions").join("r.0.0.bin");
        fs::create_dir_all(region_path.parent().unwrap()).unwrap();

        // Write corrupt bytes to region file
        let corrupt_content = b"INVALID_BINCODE_REGION_CORRUPT_BYTES_123456789";
        fs::write(&region_path, corrupt_content).unwrap();

        // Attempt to save a chunk into the corrupt region
        let chunk = Chunk::new(0, 0);
        let save_data = ChunkSaveData::from_chunk(&chunk);
        let result = manager.save_chunk(0, 0, save_data);

        // Verify save_chunk fails with RegionCorruption error
        assert!(result.is_err());
        if let Err(SaveError::RegionCorruption { path, .. }) = result {
            assert_eq!(path, region_path);
        } else {
            panic!("expected SaveError::RegionCorruption");
        }

        // Verify the corrupt file on disk was NOT overwritten or wiped
        let on_disk_bytes = fs::read(&region_path).unwrap();
        assert_eq!(
            on_disk_bytes, corrupt_content,
            "corrupt region file must be preserved on disk without overwrite"
        );

        fs::remove_dir_all(world_dir).unwrap();
    }
}
