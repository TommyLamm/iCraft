//! Headless authoritative server runtime.
//!
//! The desktop application remains a presentation client.  This module owns
//! the fixed tick, authenticated session state, world mutation gate, interest
//! sets, persistence and observability needed by `icraft-server`.  It is
//! deliberately synchronous at the authority boundary; the existing Tokio
//! network thread only transports packets into the bounded event channel.

use crate::authority::contract::{
    AuthoritySnapshot, AuthorityTopology, SessionContract, SessionGameplayState,
    SessionInventorySlot, SESSION_INVENTORY_SLOTS,
};
use crate::authority::interest::{
    capped_spawn_residency, residency_hysteresis_chunks, InterestKind, InterestSet,
    RoutedInterestUpdate,
};
use crate::authority::{AuthorityConfig, AuthorityCore};
use crate::dimension::Dimension;
use crate::game_rules::{Difficulty, WorldRules};
use crate::inventory::{GameMode, Inventory};
use crate::network::protocol::{
    ContainerAction, EntityStateWire, GameplayOperation, GameplayOutcome, GameplayRequest,
    GameplayResponse, ItemWire, PlayerEffectWire, RejectReason, SessionGameplayWire,
};
use crate::network::server::{
    HostToServer, MeteredHostEventSender, NetworkMetrics, NetworkServer, ServerConfig, ServerToHost,
};
use crate::save::{
    normalize_player_identity, ChunkSaveData, EntitySaveData, LevelData, MutationRevisionIndex,
    PlayerData, SaveManager,
};
use glam::Vec3;
#[cfg(test)]
use std::cell::Cell;
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::fmt;
use std::fs;
use std::io;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

mod ingress;
mod projection;
mod session_sync;

pub(super) const TICK_INTERVAL: Duration = Duration::from_millis(50);
pub(super) const MAX_INBOUND_EVENTS_PER_TICK: usize = 512;
pub(super) const WORLD_BOUND: f32 = 30_000_000.0;
pub(super) const PLAYER_REACH: f32 = 8.0;
pub(super) const AUTOSAVE_INTERVAL_TICKS: u64 = 6_000;
pub(super) const HOST_COMMAND_QUEUE_CAPACITY: usize = 1_024;
pub(super) const HOST_EVENT_QUEUE_CAPACITY: usize = 1_024;
pub(super) const MAX_PRESENTATION_EVENTS_PER_TICK: usize = 1_024;
// The normal presentation budget is kept small enough to drain every frame.
// If it consists entirely of reliable events, retain at most one additional
// slot for every inbound command the fixed tick is allowed to process.  This
// gives the reliability lane a hard, input-budget-derived cap instead of
// evicting an already accepted response when transient state floods the queue.
pub(super) const MAX_PRESENTATION_CRITICAL_OVERFLOW: usize = MAX_INBOUND_EVENTS_PER_TICK;
pub(super) const MAX_PRESENTATION_QUEUE_LEN: usize =
    MAX_PRESENTATION_EVENTS_PER_TICK + MAX_PRESENTATION_CRITICAL_OVERFLOW;
pub(super) const MAX_INITIAL_CHUNK_PROJECTIONS_PER_TICK: usize = 16;
// A validated maximum view distance of 32 covers a 65x65 chunk square.
pub(super) const MAX_PENDING_INITIAL_CHUNKS_PER_SESSION: usize = 65 * 65;
pub(super) const MAX_POSE_SPEED_BLOCKS_PER_SECOND: f32 = 100.0;
pub(super) const POSE_DISTANCE_SLACK_BLOCKS: f32 = 4.0;
pub(super) const MAX_POSE_DELTA_MILLIS: u64 = 250;
pub(super) const TELEPORT_ALLOWANCE_RADIUS: f32 = 8.0;

#[cfg(test)]
thread_local! {
    static SAVE_ALL_FAILPOINT: Cell<bool> = const { Cell::new(false) };
    static SAVE_PLAYER_FAILPOINT: Cell<bool> = const { Cell::new(false) };
}

/// Socket ownership for an embedded authority runtime. `Disabled` creates no
/// host-command channel or network thread; local inputs still use the same
/// bounded FIFO and fixed-tick budget as a listen server's remote inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportMode {
    Disabled,
    Listen,
}

/// Persistent identity used to bootstrap an in-process presentation client.
/// Callers should reserve an ID that cannot collide with their listen
/// transport's remotely allocated IDs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalSessionProfile {
    pub id: u64,
    pub username: String,
    pub storage: LocalSessionStorage,
}

/// Player persistence policy is explicit because an existing singleplayer
/// world stores its player in `player.dat`, while authenticated remote players
/// are isolated under `players/<name>.dat`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalSessionStorage {
    WorldPlayer,
    Named,
}

impl LocalSessionProfile {
    /// Existing singleplayer/listen-host worlds default to the legacy world
    /// player payload and `dimension.dat`.
    pub fn new(id: u64, username: impl Into<String>) -> Self {
        Self {
            id,
            username: username.into(),
            storage: LocalSessionStorage::WorldPlayer,
        }
    }

    pub fn named(id: u64, username: impl Into<String>) -> Self {
        Self {
            id,
            username: username.into(),
            storage: LocalSessionStorage::Named,
        }
    }
}

/// Composition choices for a headless runtime embedded by singleplayer or a
/// listen host. This does not imply that the desktop `State` has been cut over
/// to consume the runtime output yet.
#[derive(Debug, Clone)]
pub struct EmbeddedRuntimeOptions {
    pub topology: AuthorityTopology,
    pub transport: TransportMode,
    pub local_session: Option<LocalSessionProfile>,
}

impl EmbeddedRuntimeOptions {
    pub fn singleplayer(local_session: LocalSessionProfile) -> Self {
        Self {
            topology: AuthorityTopology::Singleplayer,
            transport: TransportMode::Disabled,
            local_session: Some(local_session),
        }
    }

    pub fn listen(local_session: LocalSessionProfile) -> Self {
        Self {
            topology: AuthorityTopology::ListenServer,
            transport: TransportMode::Listen,
            local_session: Some(local_session),
        }
    }
}

/// Cloneable producer for the runtime's single bounded input FIFO. In listen
/// mode this sender and the socket transport publish into the same receiver,
/// so neither source can synchronously overtake events already in the queue.
#[derive(Clone)]
pub struct RuntimeInput {
    sender: SyncSender<ServerToHost>,
    metrics: NetworkMetrics,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeInputError {
    Full,
    Disconnected,
}

impl fmt::Display for RuntimeInputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Full => f.write_str("runtime input queue is full"),
            Self::Disconnected => f.write_str("runtime input queue is disconnected"),
        }
    }
}

impl std::error::Error for RuntimeInputError {}

impl RuntimeInput {
    pub fn try_send(&self, event: ServerToHost) -> Result<(), RuntimeInputError> {
        self.metrics.enqueue();
        match self.sender.try_send(event) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => {
                self.metrics.dequeue();
                self.metrics.record_queue_full();
                Err(RuntimeInputError::Full)
            }
            Err(TrySendError::Disconnected(_)) => {
                self.metrics.dequeue();
                Err(RuntimeInputError::Disconnected)
            }
        }
    }

    pub fn submit_request(
        &self,
        session_id: u64,
        mut request: GameplayRequest,
    ) -> Result<(), RuntimeInputError> {
        request.session_id = session_id;
        self.try_send(ServerToHost::GameplayRequest {
            id: session_id,
            request,
        })
    }
}

/// Target-aware events intended for an in-process presentation consumer.
/// World and per-session gameplay changes remain in `snapshot`, including its
/// bounded `session_updates`; this lane only diverts responses that would
/// otherwise be addressed to a nonexistent socket session.
#[derive(Debug, Clone, PartialEq)]
pub enum RuntimePresentationEvent {
    GameplayResponse {
        target: u64,
        response: GameplayResponse,
    },
    BlockChange {
        target: u64,
        dimension: u8,
        revision: u64,
        x: i32,
        y: i32,
        z: i32,
        block: u32,
        state: u8,
        raw_fluid: u8,
    },
    ChunkData {
        target: u64,
        dimension: u8,
        cx: i32,
        cz: i32,
        revision: u64,
        min_section_y: i8,
        section_count: u16,
        blocks: Vec<u8>,
        block_states: Vec<u8>,
        fluid_levels: Vec<u8>,
        block_entities: Vec<u8>,
    },
    BlockEntityDelta {
        target: u64,
        dimension: u8,
        revision: u64,
        x: i32,
        y: i32,
        z: i32,
        entity: Option<crate::block_entity::BlockEntity>,
    },
    EntitySpawn {
        target: u64,
        dimension: u8,
        sequence: u64,
        state: EntityStateWire,
    },
    EntityState {
        target: u64,
        dimension: u8,
        sequence: u64,
        state: EntityStateWire,
    },
    EntityDespawn {
        target: u64,
        dimension: u8,
        sequence: u64,
        entity_id: u64,
    },
    PlayerSessionUpdate {
        target: u64,
        sequence: u64,
        player_id: u64,
        dimension: u8,
        state: SessionGameplayWire,
    },
    PlayerEffect {
        target: u64,
        sequence: u64,
        player_id: u64,
        effects: Vec<PlayerEffectWire>,
    },
    PlayerPosition {
        target: u64,
        id: u64,
        sequence: u32,
        sender_time_millis: u64,
        position: [f32; 3],
        yaw: f32,
        pitch: f32,
    },
    ContainerOpenResult {
        target: u64,
        dimension: u8,
        success: bool,
        position: (i32, i32, i32),
        slots: Vec<Option<ItemWire>>,
        revision: u64,
    },
    ContainerClickResult {
        target: u64,
        dimension: u8,
        success: bool,
        slot_index: u16,
        slot: Option<ItemWire>,
        dragged: Option<ItemWire>,
    },
    ContainerSlotUpdate {
        target: u64,
        dimension: u8,
        revision: u64,
        position: (i32, i32, i32),
        slot_index: u16,
        slot: Option<ItemWire>,
    },
    /// Targeted invalidation for a container session that can no longer
    /// remain open (block break, transfer, interest departure, or logout).
    /// This uses the existing v16 close wire shape at the transport boundary.
    ContainerClose {
        target: u64,
        dimension: u8,
        position: (i32, i32, i32),
    },
    PlayerRespawnResult {
        target: u64,
        position: [f32; 3],
        dimension: u8,
    },
    DimensionTransfer {
        target: u64,
        dimension: u8,
        position: [f32; 3],
    },
    WorldRules {
        target: u64,
        rules: WorldRules,
    },
    TimeSync {
        target: u64,
        ticks: u64,
        weather: u8,
        weather_remaining_ticks: f32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReplaceablePresentationKey {
    Chunk {
        target: u64,
        dimension: u8,
        cx: i32,
        cz: i32,
    },
    EntityState {
        target: u64,
        dimension: u8,
        entity_id: u64,
    },
    PlayerPosition {
        target: u64,
        id: u64,
    },
    TimeSync {
        target: u64,
    },
}

impl RuntimePresentationEvent {
    fn replaceable_key(&self) -> Option<ReplaceablePresentationKey> {
        match self {
            Self::ChunkData {
                target,
                dimension,
                cx,
                cz,
                ..
            } => Some(ReplaceablePresentationKey::Chunk {
                target: *target,
                dimension: *dimension,
                cx: *cx,
                cz: *cz,
            }),
            Self::EntityState {
                target,
                dimension,
                state,
                ..
            } => Some(ReplaceablePresentationKey::EntityState {
                target: *target,
                dimension: *dimension,
                entity_id: state.entity_id,
            }),
            Self::PlayerPosition { target, id, .. } => {
                Some(ReplaceablePresentationKey::PlayerPosition {
                    target: *target,
                    id: *id,
                })
            }
            Self::TimeSync { target, .. } => {
                Some(ReplaceablePresentationKey::TimeSync { target: *target })
            }
            Self::GameplayResponse { .. }
            | Self::BlockChange { .. }
            | Self::BlockEntityDelta { .. }
            | Self::EntitySpawn { .. }
            | Self::EntityDespawn { .. }
            | Self::PlayerSessionUpdate { .. }
            | Self::PlayerEffect { .. }
            | Self::ContainerOpenResult { .. }
            | Self::ContainerClickResult { .. }
            | Self::ContainerSlotUpdate { .. }
            | Self::ContainerClose { .. }
            | Self::PlayerRespawnResult { .. }
            | Self::DimensionTransfer { .. }
            | Self::WorldRules { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeTickOutput {
    pub snapshot: AuthoritySnapshot,
    pub presentation_events: Vec<RuntimePresentationEvent>,
}

#[derive(Debug)]
pub enum ServerConfigError {
    Io(io::Error),
    Invalid {
        key: String,
        value: String,
        reason: String,
    },
}

impl fmt::Display for ServerConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "server.properties: {error}"),
            Self::Invalid { key, value, reason } => {
                write!(f, "invalid server.properties {key}={value:?}: {reason}")
            }
        }
    }
}

impl std::error::Error for ServerConfigError {}

impl From<io::Error> for ServerConfigError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// The supported `server.properties` surface.  Unknown keys are ignored for
/// forward compatibility; known keys are parsed strictly and validated before
/// a world directory is created or opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerProperties {
    pub bind: String,
    pub port: u16,
    pub motd: String,
    pub max_players: usize,
    pub difficulty: String,
    /// LAN/offline account switch. `true` is rejected until challenge /
    /// shared-secret auth exists; names are accounts and operators come only
    /// from the dedicated-server console `op` command (or this file).
    pub online_mode: bool,
    pub whitelist: HashSet<String>,
    pub operators: HashSet<String>,
    pub view_distance: u8,
    pub simulation_distance: u8,
    pub pvp: bool,
    pub world_dir: PathBuf,
    pub seed: u64,
}

impl Default for ServerProperties {
    fn default() -> Self {
        Self {
            bind: "0.0.0.0".into(),
            port: 25565,
            motd: "iCraft server".into(),
            max_players: 20,
            difficulty: "normal".into(),
            online_mode: false,
            whitelist: HashSet::new(),
            operators: HashSet::new(),
            view_distance: 10,
            simulation_distance: 8,
            pvp: true,
            world_dir: PathBuf::from("world"),
            seed: 0,
        }
    }
}

impl ServerProperties {
    pub fn difficulty_kind(&self) -> Result<Difficulty, ServerConfigError> {
        Difficulty::parse_strict(&self.difficulty).ok_or_else(|| {
            invalid(
                "difficulty",
                &self.difficulty,
                "expected peaceful, easy, normal, or hard",
            )
        })
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Self, ServerConfigError> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(path)?;
        let mut properties = Self::default();
        for (line_number, raw_line) in content.lines().enumerate() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((raw_key, raw_value)) = line.split_once('=') else {
                return Err(ServerConfigError::Invalid {
                    key: format!("line {}", line_number + 1),
                    value: line.into(),
                    reason: "expected key=value".into(),
                });
            };
            let key = raw_key.trim();
            let value = raw_value.trim();
            match key {
                "bind" | "server-ip" => properties.bind = value.to_string(),
                "port" | "server-port" => {
                    properties.port = parse_range(key, value, 1..=u16::MAX)?;
                }
                "motd" => properties.motd = value.to_string(),
                "max-players" => {
                    properties.max_players = parse_range(key, value, 1..=64)? as usize;
                }
                "difficulty" => {
                    let normalized = value.to_ascii_lowercase();
                    if !matches!(normalized.as_str(), "peaceful" | "easy" | "normal" | "hard") {
                        return Err(invalid(
                            key,
                            value,
                            "expected peaceful, easy, normal, or hard",
                        ));
                    }
                    properties.difficulty = normalized;
                }
                "online-mode" => properties.online_mode = parse_bool(key, value)?,
                "whitelist" => {
                    properties.whitelist = parse_identity_set(key, value)?;
                }
                "operators" | "ops" => {
                    properties.operators = parse_identity_set(key, value)?;
                }
                "view-distance" => {
                    properties.view_distance = parse_range(key, value, 2..=32)?;
                }
                "simulation-distance" => {
                    properties.simulation_distance = parse_range(key, value, 2..=32)?;
                }
                "pvp" => properties.pvp = parse_bool(key, value)?,
                "level-name" | "world" | "world-dir" => properties.world_dir = PathBuf::from(value),
                "level-seed" | "seed" => {
                    properties.seed = value
                        .parse::<i64>()
                        .map_err(|_| invalid(key, value, "expected a signed 64-bit integer"))?
                        as u64;
                }
                _ => {}
            }
        }
        properties.validate()?;
        Ok(properties)
    }

    pub fn validate(&self) -> Result<(), ServerConfigError> {
        self.difficulty_kind()?;
        if self.bind.trim().is_empty() {
            return Err(invalid("bind", &self.bind, "must not be empty"));
        }
        if self.bind.parse::<IpAddr>().is_err() && self.bind != "localhost" {
            return Err(invalid(
                "bind",
                &self.bind,
                "expected an IP address or localhost",
            ));
        }
        if self.port == 0 {
            return Err(invalid("port", self.port, "must be between 1 and 65535"));
        }
        if !(1..=64).contains(&self.max_players) {
            return Err(invalid(
                "max-players",
                self.max_players,
                "must be between 1 and 64",
            ));
        }
        if !(2..=32).contains(&self.view_distance) {
            return Err(invalid(
                "view-distance",
                self.view_distance,
                "must be between 2 and 32",
            ));
        }
        if !(2..=32).contains(&self.simulation_distance) {
            return Err(invalid(
                "simulation-distance",
                self.simulation_distance,
                "must be between 2 and 32",
            ));
        }
        if self.online_mode {
            return Err(invalid(
                "online-mode",
                true,
                "online authentication is not implemented; refuse to treat this as a credential switch (尚未實作驗證，拒絕當憑證開關)",
            ));
        }
        validate_identity_set("whitelist", &self.whitelist)?;
        validate_identity_set("operators", &self.operators)?;
        if self.motd.trim().is_empty() || self.motd.len() > 256 {
            return Err(invalid("motd", &self.motd, "must contain 1..=256 bytes"));
        }
        Ok(())
    }

    pub fn write(&self, path: impl AsRef<Path>) -> Result<(), ServerConfigError> {
        self.validate()?;
        let mut whitelist: Vec<_> = self.whitelist.iter().cloned().collect();
        whitelist.sort();
        let content = format!(
            "# online-mode=false is LAN/offline: names are accounts until real credentials exist.\n\
             # Operators are granted only by the dedicated-server console `op` command (or this file),\n\
             # bound to the normalized identity of later connections. There is still no password.\n\
             bind={}\nport={}\nmotd={}\nmax-players={}\ndifficulty={}\nonline-mode={}\nwhitelist={}\noperators={}\nview-distance={}\nsimulation-distance={}\npvp={}\nlevel-name={}\nlevel-seed={}\n",
            self.bind,
            self.port,
            self.motd,
            self.max_players,
            self.difficulty,
            self.online_mode,
            whitelist.join(","),
            sorted_names(&self.operators).join(","),
            self.view_distance,
            self.simulation_distance,
            self.pvp,
            self.world_dir.display(),
            self.seed as i64,
        );
        atomic_write(path.as_ref(), content.as_bytes())?;
        Ok(())
    }
}

fn invalid(
    key: impl Into<String>,
    value: impl ToString,
    reason: impl Into<String>,
) -> ServerConfigError {
    ServerConfigError::Invalid {
        key: key.into(),
        value: value.to_string(),
        reason: reason.into(),
    }
}

fn sorted_names(names: &HashSet<String>) -> Vec<String> {
    let mut values: Vec<_> = names.iter().cloned().collect();
    values.sort();
    values
}

fn parse_identity_set(key: &str, value: &str) -> Result<HashSet<String>, ServerConfigError> {
    let mut names = HashSet::new();
    for raw in value.split(',') {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        match normalize_player_identity(raw) {
            Ok(identity) => {
                names.insert(identity);
            }
            Err(error) => return Err(invalid(key, raw, error.to_string())),
        }
    }
    Ok(names)
}

fn validate_identity_set(key: &str, names: &HashSet<String>) -> Result<(), ServerConfigError> {
    for name in names {
        match normalize_player_identity(name) {
            Ok(normalized) if normalized == *name => {}
            Ok(_) | Err(_) => {
                return Err(invalid(
                    key,
                    name,
                    "must already be a normalized player identity",
                ));
            }
        }
    }
    Ok(())
}

fn parse_bool(key: &str, value: &str) -> Result<bool, ServerConfigError> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" => Ok(true),
        "false" | "0" | "no" => Ok(false),
        _ => Err(invalid(key, value, "expected true or false")),
    }
}

fn parse_range<T>(
    key: &str,
    value: &str,
    range: std::ops::RangeInclusive<T>,
) -> Result<T, ServerConfigError>
where
    T: std::str::FromStr + PartialOrd + Copy + fmt::Display + fmt::Debug,
{
    let parsed = value
        .parse::<T>()
        .map_err(|_| invalid(key, value, "expected an integer"))?;
    if range.contains(&parsed) {
        Ok(parsed)
    } else {
        Err(invalid(key, value, format!("must be in {range:?}")))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServerMetrics {
    pub ticks: u64,
    pub last_tick_time_us: u64,
    pub max_tick_time_us: u64,
    pub inbound_packets: u64,
    pub outbound_packets: u64,
    pub inbound_bytes: u64,
    pub outbound_bytes: u64,
    pub queue_depth: usize,
    pub queue_full: u64,
    pub requests_accepted: u64,
    pub requests_rejected: u64,
    pub duplicate_requests: u64,
    pub loaded_chunks: usize,
    pub entities: usize,
    pub players_online: usize,
    pub saves: u64,
    pub last_save_latency_ms: u64,
}

/// Runtime session record kept separate from authority `SessionContract`.
/// Interest, the save codec, pose clocks, and teleport allowance live here.
/// Authoritative pose / dimension / accepted sequence live on the contract.
#[derive(Debug, Clone)]
pub struct PlayerSessionState {
    pub id: u64,
    pub username: String,
    pub storage: LocalSessionStorage,
    pub data: PlayerData,
    pub interest: InterestSet,
    pub effects: Vec<PlayerEffectWire>,
    pub(super) pending_initial_chunks: VecDeque<(Dimension, i32, i32)>,
    pub(super) last_projected_session_revision: Option<(Dimension, u64)>,
    /// Last pose/health/anim fingerprint sent as `EntityState` to this session.
    /// Cleared when an entity leaves the simulation set so re-entry is full.
    pub(super) last_projected_entity_states: HashMap<u64, projection::EntityBroadcastFingerprint>,
    pub(super) last_pose_sequence: u32,
    pub(super) last_pose_sender_time_millis: u64,
    pub(super) last_pose_received_at: Option<Instant>,
    pub(super) teleport_allowance: Option<[f32; 3]>,
}

impl PlayerSessionState {
    fn new(
        id: u64,
        username: String,
        storage: LocalSessionStorage,
        data: PlayerData,
        dimension: Dimension,
        view_distance: u8,
        simulation_distance: u8,
    ) -> Self {
        Self {
            id,
            username,
            storage,
            data,
            interest: InterestSet::new(dimension, view_distance, simulation_distance),
            effects: Vec::new(),
            pending_initial_chunks: VecDeque::new(),
            last_projected_session_revision: None,
            last_projected_entity_states: HashMap::new(),
            last_pose_sequence: 0,
            last_pose_sender_time_millis: 0,
            last_pose_received_at: None,
            teleport_allowance: None,
        }
    }

    pub(super) fn queue_initial_chunks(
        &mut self,
        dimension: Dimension,
        chunks: impl IntoIterator<Item = (i32, i32)>,
    ) {
        for (cx, cz) in chunks {
            if self.pending_initial_chunks.len() >= MAX_PENDING_INITIAL_CHUNKS_PER_SESSION {
                break;
            }
            let item = (dimension, cx, cz);
            if !self.pending_initial_chunks.contains(&item) {
                self.pending_initial_chunks.push_back(item);
            }
        }
    }

    pub(super) fn prune_projected_entity_states(&mut self) {
        let stale: Vec<u64> = self
            .last_projected_entity_states
            .keys()
            .copied()
            .filter(|id| !self.interest.simulation_entities.contains(id))
            .collect();
        for id in stale {
            self.last_projected_entity_states.remove(&id);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn accept_pose(
        &mut self,
        sequence: u32,
        sender_time_millis: u64,
        position: [f32; 3],
        yaw: f32,
        pitch: f32,
        now: Instant,
    ) -> bool {
        if sequence == 0
            || !position
                .iter()
                .chain([yaw, pitch].iter())
                .all(|value| value.is_finite())
            || position.iter().any(|value| value.abs() > WORLD_BOUND)
        {
            return false;
        }
        if self.last_pose_received_at.is_some() {
            let sequence_delta = sequence.wrapping_sub(self.last_pose_sequence);
            if sequence_delta == 0
                || sequence_delta >= (1 << 31)
                || sender_time_millis <= self.last_pose_sender_time_millis
            {
                return false;
            }
            let target = Vec3::from_array(position);
            let previous = Vec3::from_array(self.data.position);
            let teleport_allowed = self.teleport_allowance.is_some_and(|allowance| {
                target.distance_squared(Vec3::from_array(allowance))
                    <= TELEPORT_ALLOWANCE_RADIUS * TELEPORT_ALLOWANCE_RADIUS
            });
            if !teleport_allowed {
                let sender_delta = sender_time_millis
                    .saturating_sub(self.last_pose_sender_time_millis)
                    .min(MAX_POSE_DELTA_MILLIS);
                let received_delta = self
                    .last_pose_received_at
                    .map(|last| now.saturating_duration_since(last).as_millis() as u64)
                    .unwrap_or_default()
                    .min(MAX_POSE_DELTA_MILLIS);
                let elapsed_seconds = sender_delta.max(received_delta) as f32 / 1_000.0;
                let allowed_distance =
                    POSE_DISTANCE_SLACK_BLOCKS + MAX_POSE_SPEED_BLOCKS_PER_SECOND * elapsed_seconds;
                if previous.distance_squared(target) > allowed_distance * allowed_distance {
                    return false;
                }
            }
        }
        // Pose fields are written by `ServerRuntime::write_pose` after this
        // clock/allowance update so the speed gate stays on this type.
        self.last_pose_sequence = sequence;
        self.last_pose_sender_time_millis = sender_time_millis;
        self.last_pose_received_at = Some(now);
        self.teleport_allowance = None;
        true
    }
}

pub struct ServerRuntime {
    pub properties: ServerProperties,
    pub level: LevelData,
    pub metrics: ServerMetrics,
    pub players: HashMap<u64, PlayerSessionState>,
    /// Shared headless authority used by dedicated, listen and in-process
    /// callers.  Runtime transport/session/save code never mirrors world
    /// mutation in a second map.
    pub authority: AuthorityCore,
    /// Interest-routed deltas are retained until the transport owner drains
    /// them. This keeps routing deterministic even when a network queue is
    /// backpressured, without mirroring world state in the renderer.
    pub(super) routed_updates: Vec<RoutedInterestUpdate>,
    /// Revisions already projected by an immediate request ACK.  The next
    /// fixed snapshot contains those pending mutations as well; this bounded
    /// set prevents duplicate block/container deltas without dropping later
    /// automation mutations.
    /// Immediate-ACK deduplication is dimension-scoped; two worlds may both
    /// legitimately emit revision 1 in the same fixed tick.
    pub(super) routed_mutations: BTreeSet<(Dimension, u64)>,
    pub(super) world_dir: PathBuf,
    pub(super) save_manager: SaveManager,
    pub(super) default_game_mode: GameMode,
    pub(super) host_tx: Option<tokio::sync::mpsc::Sender<HostToServer>>,
    pub(super) host_rx: Receiver<ServerToHost>,
    pub(super) network_thread: Option<JoinHandle<()>>,
    pub(super) network_metrics: NetworkMetrics,
    pub(super) transport_mode: TransportMode,
    pub(super) local_session_id: Option<u64>,
    pub(super) presentation_events: VecDeque<RuntimePresentationEvent>,
    pub(super) observed_transport_rejections: u64,
    pub(super) observed_transport_duplicates: u64,
    pub(super) stopped: bool,
    /// Successful `save_all` during shutdown. `request_shutdown` only sets
    /// `stopped`; a later `shutdown` must still flush if this is false.
    pub(super) save_flushed: bool,
}

impl ServerRuntime {
    pub fn new(properties: ServerProperties) -> Result<Self, ServerConfigError> {
        let (runtime, _input) = Self::construct(
            properties,
            EmbeddedRuntimeOptions {
                topology: AuthorityTopology::Dedicated,
                transport: TransportMode::Listen,
                local_session: None,
            },
        )?;
        Ok(runtime)
    }

    /// Construct a runtime for an in-process presentation root. The returned
    /// input is the only local producer; in listen mode the network transport
    /// clones the same bounded receiver-facing channel.
    pub fn new_embedded(
        properties: ServerProperties,
        options: EmbeddedRuntimeOptions,
    ) -> Result<(Self, RuntimeInput), ServerConfigError> {
        Self::construct(properties, options)
    }

    fn construct(
        properties: ServerProperties,
        options: EmbeddedRuntimeOptions,
    ) -> Result<(Self, RuntimeInput), ServerConfigError> {
        properties.validate()?;
        let difficulty = properties.difficulty_kind()?;
        let world_dir = properties.world_dir.clone();
        if world_dir.exists() && !world_dir.is_dir() {
            return Err(ServerConfigError::Io(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("world path {} is not a directory", world_dir.display()),
            )));
        }
        let save_manager = SaveManager::new(&world_dir);
        let creation = crate::save::load_world_creation_options(&world_dir);
        let existing_level = save_manager.load_level().map_err(ServerConfigError::Io)?;
        let mut level = existing_level.unwrap_or_else(|| LevelData {
            seed: properties.seed as u32,
            rules: WorldRules {
                pvp: properties.pvp,
                hardcore: creation.hardcore,
                ..WorldRules::default()
            },
            world_type: creation.world_type,
            generate_structures: creation.generate_structures,
            bonus_chest: creation.bonus_chest,
            cheats_enabled: creation.cheats_enabled,
            hardcore: creation.hardcore,
            ..LevelData::default()
        });
        // `world.meta` is the creation-time source of truth for cheats. The
        // first binary level save used to drop that flag because LevelData
        // defaulted cheats off, which then disabled `/gamemode` on reload.
        if creation.cheats_enabled {
            level.cheats_enabled = true;
        }
        // `server.properties` is the live authority configuration.  A saved
        // level may carry an older rule snapshot, but connection/runtime
        // policy must still apply the operator's pvp setting before
        // constructing the shared headless core. Difficulty is carried as a
        // separate server-owned value so adding it does not invalidate old
        // binary level payloads.
        level.rules.pvp = properties.pvp;
        if creation.hardcore {
            level.hardcore = true;
            level.rules.hardcore = true;
        }
        level.rules = level.rules.normalized();
        let (server_to_host_tx, host_rx) = mpsc::sync_channel(HOST_EVENT_QUEUE_CAPACITY);
        let network_metrics = NetworkMetrics::default();
        let input = RuntimeInput {
            sender: server_to_host_tx.clone(),
            metrics: network_metrics.clone(),
        };
        let (host_tx, host_rx_network) = match options.transport {
            TransportMode::Disabled => (None, None),
            TransportMode::Listen => {
                let (sender, receiver) = tokio::sync::mpsc::channel(HOST_COMMAND_QUEUE_CAPACITY);
                (Some(sender), Some(receiver))
            }
        };
        let mut authority = AuthorityCore::new(
            AuthorityConfig {
                seed: level.seed,
                dimension: level.spawn_dimension,
                world_type: level.world_type,
                generate_structures: level.generate_structures,
                rules: level.rules,
                difficulty,
                render_distance: properties.simulation_distance as i32,
            },
            options.topology,
        );
        authority.world_mut_active().time = level.time;
        let mut runtime = Self {
            properties,
            level,
            metrics: ServerMetrics::default(),
            players: HashMap::new(),
            authority,
            world_dir,
            save_manager,
            default_game_mode: creation.game_mode,
            host_tx,
            host_rx,
            network_thread: None,
            network_metrics,
            transport_mode: options.transport,
            local_session_id: options.local_session.as_ref().map(|profile| profile.id),
            presentation_events: VecDeque::with_capacity(MAX_PRESENTATION_EVENTS_PER_TICK),
            observed_transport_rejections: 0,
            observed_transport_duplicates: 0,
            routed_updates: Vec::new(),
            routed_mutations: BTreeSet::new(),
            stopped: false,
            save_flushed: false,
        };
        runtime.restore_authority_state()?;
        runtime.ensure_spawn_chunk();
        if let Some(profile) = options.local_session {
            runtime.handle_join_with_storage(profile.id, profile.username, profile.storage)?;
        }
        if let Some(host_rx_network) = host_rx_network {
            let server_to_host =
                MeteredHostEventSender::new(server_to_host_tx, runtime.network_metrics.clone());
            let mut network_config = ServerConfig::default();
            network_config.max_players = runtime.properties.max_players;
            network_config.motd = runtime.properties.motd.clone();
            network_config.whitelist = runtime.properties.whitelist.clone();
            let bind_addr = format!("{}:{}", runtime.properties.bind, runtime.properties.port);
            runtime.network_thread = Some(NetworkServer::spawn_with_config_and_metrics(
                bind_addr,
                runtime.properties.seed,
                0,
                host_rx_network,
                server_to_host,
                network_config,
                runtime.network_metrics.clone(),
            ));
        }
        Ok((runtime, input))
    }

    /// Process one fixed simulation tick.  No wgpu/winit/audio state is
    /// touched, making this safe for dedicated servers and headless tests.
    pub fn tick(&mut self) -> io::Result<()> {
        self.tick_with_output().map(|_| ())
    }

    /// Process the same fixed tick as `tick`, returning an owned projection for
    /// an embedded presentation client. Remote socket events are still handled
    /// exclusively by `handle_event`; no transport consumer is introduced.
    pub fn tick_with_output(&mut self) -> io::Result<RuntimeTickOutput> {
        if self.stopped {
            return Ok(RuntimeTickOutput {
                snapshot: self.authority.last_snapshot().clone(),
                presentation_events: self.presentation_events.drain(..).collect(),
            });
        }
        let started = Instant::now();
        let mut processed = 0;
        while processed < MAX_INBOUND_EVENTS_PER_TICK {
            let event = match self.host_rx.try_recv() {
                Ok(event) => {
                    self.network_metrics.dequeue();
                    event
                }
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
            };
            processed += 1;
            self.handle_event(event)?;
        }
        let snapshot = self.authority.tick();
        for transfer in self.authority.take_pending_dimension_transfers() {
            self.apply_authority_dimension_transfer(transfer);
        }
        self.route_authority_snapshot(&snapshot);
        self.evict_uninteresting_chunks();
        for closure in self.authority.take_container_closures() {
            self.close_runtime_container(closure.player_id, closure.dimension, closure.position);
        }
        self.level.time = snapshot.tick;
        self.metrics.ticks = self.metrics.ticks.wrapping_add(1);
        self.metrics.players_online = self.players.len();
        self.metrics.loaded_chunks = self
            .authority
            .dimensions()
            .into_iter()
            .filter_map(|dimension| self.authority.world_ref(dimension))
            .map(|world| world.chunks.chunks.len())
            .sum();
        self.metrics.entities = self
            .authority
            .dimensions()
            .into_iter()
            .filter_map(|dimension| self.authority.world_ref(dimension))
            .map(|world| world.entities.entities.len())
            .sum();
        if self.metrics.ticks % AUTOSAVE_INTERVAL_TICKS == 0 {
            if let Err(error) = self.save_all() {
                eprintln!("[ServerRuntime] autosave failed: {error}");
            }
        }
        let elapsed = started.elapsed();
        let elapsed_us = elapsed.as_micros().min(u64::MAX as u128) as u64;
        self.metrics.last_tick_time_us = elapsed_us.max(1);
        self.metrics.max_tick_time_us = self
            .metrics
            .max_tick_time_us
            .max(self.metrics.last_tick_time_us);
        self.sync_network_metrics();
        if elapsed > TICK_INTERVAL {
            eprintln!("[ServerRuntime] tick over budget: {elapsed:?}");
        }
        Ok(RuntimeTickOutput {
            snapshot,
            presentation_events: self.presentation_events.drain(..).collect(),
        })
    }

    pub fn run_for_ticks(&mut self, ticks: u64) -> io::Result<()> {
        for _ in 0..ticks {
            self.tick()?;
        }
        Ok(())
    }

    pub fn run_until_shutdown(&mut self) -> io::Result<()> {
        while !self.stopped {
            let started = Instant::now();
            self.tick()?;
            let elapsed = started.elapsed();
            if elapsed < TICK_INTERVAL {
                std::thread::sleep(TICK_INTERVAL - elapsed);
            }
        }
        Ok(())
    }

    pub fn shutdown(&mut self) -> io::Result<()> {
        if self.stopped && self.save_flushed {
            return Ok(());
        }
        let save_result = self.save_all();
        if save_result.is_ok() {
            self.save_flushed = true;
        }
        if self.host_tx.is_some() {
            self.enqueue_stop();
        }
        self.stopped = true;
        if let Some(handle) = self.network_thread.take() {
            let _ = handle.join();
        }
        self.sync_network_metrics();
        save_result
    }

    fn restore_authority_state(&mut self) -> io::Result<()> {
        let revision_index = self.save_manager.load_mutation_revision_index();
        let dimensions = [Dimension::Overworld, Dimension::Nether, Dimension::End];
        for dimension in dimensions {
            let chunks = self.save_manager.load_saved_chunks_in(dimension)?;
            let revisions: Vec<_> = revision_index.entries_in(dimension).collect();
            let entities = self.save_manager.load_entities_in_checked(dimension)?;
            // Do not materialize every possible dimension during restore just
            // because the save layout has no data for it.  The active spawn
            // world must remain initialized, while an untouched non-active
            // dimension is created lazily on its first session/request.
            if chunks.is_empty()
                && revisions.is_empty()
                && entities.is_empty()
                && dimension != self.authority.active_dimension()
            {
                continue;
            }
            self.authority.with_world(dimension, |world| {
                for chunk in &chunks {
                    if let Err(error) = world.restore_saved_chunk(chunk) {
                        eprintln!(
                            "[ServerRuntime] skipping corrupt saved chunk ({}, {}) in {:?}: {error}",
                            chunk.chunk_x, chunk.chunk_z, dimension
                        );
                    }
                }
                for ((_cx, _cz), revision) in revisions {
                    world.revisions.observe(revision);
                }
                if !entities.is_empty() {
                    world.restore_saved_entities(&entities);
                }
            });
        }
        Ok(())
    }

    fn save_authority_state(&mut self) -> io::Result<()> {
        let active_dimension = self.authority.active_dimension();
        let mut merged_revisions = MutationRevisionIndex::default();
        for dimension in self.authority.dimensions() {
            let (chunks, entities, revisions) = self.authority.with_world(dimension, |world| {
                let mut coordinates: Vec<_> = world.chunks.chunks.keys().copied().collect();
                coordinates.sort_unstable();
                let chunks = coordinates
                    .into_iter()
                    .filter_map(|(cx, cz)| {
                        if world.failed_restore_chunks().contains(&(cx, cz)) {
                            return None;
                        }
                        world.chunks.chunks.get(&(cx, cz)).and_then(|chunk| {
                            let metadata =
                                world.redstone.collect_chunk_metadata(&world.chunks, cx, cz);
                            let mut data =
                                ChunkSaveData::from_chunk_with_redstone(chunk, &metadata).ok()?;
                            data.mutation_revision = world.chunk_revision(cx, cz);
                            Some((cx, cz, data))
                        })
                    })
                    .collect::<Vec<_>>();
                let entities = world
                    .entities
                    .entities
                    .iter()
                    .map(EntitySaveData::from)
                    .collect::<Vec<_>>();
                (chunks, entities, world.mutation_revision_index())
            });
            for (cx, cz, data) in chunks {
                self.save_manager
                    .save_chunk_in(dimension, cx, cz, data)
                    .map_err(|error| io::Error::new(io::ErrorKind::Other, error.to_string()))?;
            }
            self.save_manager.save_entities_in(dimension, &entities)?;
            for ((cx, cz), revision) in revisions.entries_in(dimension) {
                merged_revisions
                    .ensure_at_least(dimension, cx, cz, revision)
                    .map_err(|error| io::Error::new(io::ErrorKind::Other, error.to_string()))?;
            }
        }
        self.save_manager
            .save_mutation_revision_index(&merged_revisions)?;
        self.save_manager.save_current_dimension(active_dimension)?;
        Ok(())
    }

    pub fn save_all(&mut self) -> io::Result<()> {
        #[cfg(test)]
        if SAVE_ALL_FAILPOINT.with(|failpoint| failpoint.get()) {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "injected save_all failure",
            ));
        }
        let started = Instant::now();
        self.save_manager.save_level(&self.level)?;
        self.save_authority_state()?;
        // Keep operator-owned difficulty (and the rest of server policy) in
        // the same durable world directory as level/player state.  Runtime
        // construction validates this file before an authority world exists.
        self.persist_properties()?;
        let mut names: Vec<_> = self.players.values().collect();
        names.sort_by_key(|session| session.id);
        for session in names {
            self.save_player(session)?;
        }
        self.metrics.saves = self.metrics.saves.saturating_add(1);
        let latency_us = started.elapsed().as_micros().min(u64::MAX as u128) as u64;
        self.metrics.last_save_latency_ms = (latency_us.saturating_add(999) / 1_000).max(1);
        Ok(())
    }

    fn persist_properties(&self) -> io::Result<()> {
        self.properties
            .write(self.world_dir.join("server.properties"))
            .map_err(|error| io::Error::new(io::ErrorKind::Other, error))
    }

    pub fn request_shutdown(&mut self) {
        self.stopped = true;
    }

    pub fn metrics(&self) -> &ServerMetrics {
        &self.metrics
    }

    pub fn transport_mode(&self) -> TransportMode {
        self.transport_mode
    }

    pub fn is_stopped(&self) -> bool {
        self.stopped
    }

    /// Execute a bounded console command.  The binary feeds stdin into this
    /// method, while tests and embedding applications can call it directly.
    pub fn execute_console_command(&mut self, line: &str) -> Result<String, String> {
        let mut words = line.split_whitespace();
        let command = words.next().unwrap_or_default().to_ascii_lowercase();
        match command.as_str() {
            "" => Ok(String::new()),
            "stop" | "shutdown" => {
                self.shutdown().map_err(|error| error.to_string())?;
                Ok("server stopped".into())
            }
            "save" | "save-all" => {
                self.save_all().map_err(|error| error.to_string())?;
                Ok("saved".into())
            }
            "list" => Ok(format!("{} player(s) online", self.players.len())),
            "whitelist" => match words.next().unwrap_or_default() {
                "add" => {
                    let name = words.next().ok_or("usage: whitelist add <name>")?;
                    let normalized =
                        normalize_player_identity(name).map_err(|error| error.to_string())?;
                    self.properties.whitelist.insert(normalized);
                    self.persist_properties()
                        .map_err(|error| error.to_string())?;
                    Ok(format!("added {name} to whitelist"))
                }
                "remove" => {
                    let name = words.next().ok_or("usage: whitelist remove <name>")?;
                    let normalized =
                        normalize_player_identity(name).map_err(|error| error.to_string())?;
                    self.properties.whitelist.remove(&normalized);
                    self.persist_properties()
                        .map_err(|error| error.to_string())?;
                    Ok(format!("removed {name} from whitelist"))
                }
                "on" => Ok("whitelist is enabled when at least one entry exists".into()),
                "off" => {
                    self.properties.whitelist.clear();
                    self.persist_properties()
                        .map_err(|error| error.to_string())?;
                    Ok("whitelist cleared".into())
                }
                _ => Err("usage: whitelist <add|remove|on|off> <name>".into()),
            },
            "op" | "deop" => {
                let name = words.next().ok_or("usage: op|deop <name>")?;
                let normalized =
                    normalize_player_identity(name).map_err(|error| error.to_string())?;
                if command == "op" {
                    self.properties.operators.insert(normalized);
                } else {
                    self.properties.operators.remove(&normalized);
                }
                self.persist_properties()
                    .map_err(|error| error.to_string())?;
                Ok(format!("{command} {name}"))
            }
            _ => Err(format!("unknown command: {command}")),
        }
    }

    pub(super) fn ensure_spawn_chunk(&mut self) {
        self.authority.world_mut_active().ensure_chunk(
            self.level.spawn_x.div_euclid(16),
            self.level.spawn_z.div_euclid(16),
        );
    }

    pub fn valid_coordinate(&self, dimension: Dimension, x: i32, y: i32, z: i32) -> bool {
        self.authority
            .world_ref(dimension)
            .is_some_and(|world| world.valid_coordinate(x, y, z))
    }

    fn residency_keep_set(&self, dimension: Dimension) -> BTreeSet<(i32, i32)> {
        let mut keep = BTreeSet::new();
        let mut any_session = false;
        for session in self.players.values() {
            if session.interest.dimension != dimension {
                continue;
            }
            any_session = true;
            keep.extend(session.interest.chunks.iter().copied());
            keep.extend(session.interest.simulation_chunks.iter().copied());
            keep.extend(residency_hysteresis_chunks(
                session.data.position,
                session.interest.view_distance,
            ));
        }
        if !any_session {
            keep.extend(capped_spawn_residency(
                self.level.spawn_x,
                self.level.spawn_z,
            ));
        }
        keep
    }

    fn evict_uninteresting_chunks(&mut self) {
        let dimensions = self.authority.dimensions();
        for dimension in dimensions {
            let keep = self.residency_keep_set(dimension);
            let mut flush_error = None;
            let save_manager = &mut self.save_manager;
            self.authority.with_world(dimension, |world| {
                world.evict_unkept_chunks(&keep, |cx, cz, data| {
                    save_manager
                        .save_chunk_in(dimension, cx, cz, data)
                        .map_err(|error| {
                            let io_error = io::Error::new(io::ErrorKind::Other, error.to_string());
                            flush_error = Some(format!("({cx}, {cz}): {io_error}"));
                            io_error
                        })
                });
            });
            if let Some(error) = flush_error {
                eprintln!("[ServerRuntime] evict flush failed in {dimension:?} {error}");
            }
        }
    }

    fn save_player(&self, session: &PlayerSessionState) -> io::Result<()> {
        #[cfg(test)]
        if SAVE_PLAYER_FAILPOINT.with(|failpoint| failpoint.get()) {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "injected save_player failure",
            ));
        }
        let mut data = session.data.clone();
        let current_dimension = self
            .authority
            .session(session.id)
            .and_then(|authority_session| {
                Dimension::from_wire(authority_session.dimension).map(|dimension| {
                    apply_gameplay_to_player_data(&mut data, authority_session.gameplay);
                    data.game_mode = authority_session.game_mode;
                    dimension
                })
            })
            .unwrap_or(session.interest.dimension);
        match session.storage {
            LocalSessionStorage::Named => self.save_manager.save_dedicated_player(
                &session.username,
                current_dimension,
                &data,
                &session.effects,
            ),
            LocalSessionStorage::WorldPlayer => {
                // `player.dat` predates the dedicated effect vector. Preserve
                // the established file format instead of inventing a silent,
                // incompatible sidecar during the runtime composition cutover.
                self.save_manager
                    .save_player_and_level(&self.level, &data)?;
                self.save_manager.save_current_dimension(current_dimension)
            }
        }
    }
}

pub(super) fn default_player_data(game_mode: GameMode) -> PlayerData {
    let state = crate::player::PlayerState::new();
    let inventory = match game_mode {
        GameMode::Creative => Inventory::new_creative(),
        GameMode::Survival | GameMode::Adventure | GameMode::Spectator => Inventory::new(),
    };
    PlayerData::from_state(
        Vec3::new(8.0, 80.0, 8.0),
        Vec3::ZERO,
        0.0,
        0.0,
        &state,
        game_mode,
        &inventory,
        Default::default(),
    )
}

fn scalar_to_milli(value: f32) -> u32 {
    if !value.is_finite() {
        return 0;
    }
    (value.max(0.0) * 1_000.0).round() as u32
}

fn milli_to_scalar(value: u32) -> f32 {
    value as f32 / 1_000.0
}

fn session_slot_from_stack(
    stack: Option<&crate::inventory::ItemStack>,
) -> Option<SessionInventorySlot> {
    let stack = stack?;
    if stack.count == 0 {
        return None;
    }
    Some(SessionInventorySlot::from_wire(
        ItemWire::from_stack(stack),
        stack.can_break,
        stack.can_place_on,
    ))
}

/// Convert the persisted player payload into the compact authority gameplay
/// contract.  The 41 slots retain ItemWire metadata and Adventure masks; the
/// real dragged cursor is restored so container-click conservation survives
/// save/join. Catalog-only creative cursors are already stripped by the save codec.
pub(super) fn gameplay_from_player_data(data: &PlayerData) -> SessionGameplayState {
    let inventory = data.inventory.to_inventory();
    let mut slots = [None; SESSION_INVENTORY_SLOTS];
    for (index, stack) in inventory.hotbar.iter().enumerate() {
        slots[index] = session_slot_from_stack(stack.as_ref());
    }
    for (index, stack) in inventory.main.iter().enumerate() {
        slots[9 + index] = session_slot_from_stack(stack.as_ref());
    }
    for (index, stack) in inventory.armor.iter().enumerate() {
        slots[36 + index] = session_slot_from_stack(stack.as_ref());
    }
    slots[40] = session_slot_from_stack(inventory.offhand.as_ref());

    let mut gameplay = SessionGameplayState::default();
    gameplay.health_milli = scalar_to_milli(data.health);
    gameplay.hunger_milli = scalar_to_milli(data.hunger);
    gameplay.saturation_milli = scalar_to_milli(data.saturation);
    gameplay.is_dead = data.is_dead;
    gameplay.experience = data.experience;
    gameplay.experience_level = data.experience_level;
    gameplay.selected_hotbar_slot = inventory.selected.min(8) as u8;
    gameplay.inventory = slots;
    gameplay.cursor = session_slot_from_stack(inventory.dragged.as_ref());
    gameplay
}

/// Overlay authoritative gameplay onto a persisted payload before saving.
/// Fields with no compact authority equivalent (experience, effects, spawn,
/// advancements and movement orientation) remain from the runtime payload.
pub(super) fn apply_gameplay_to_player_data(data: &mut PlayerData, gameplay: SessionGameplayState) {
    data.health = milli_to_scalar(gameplay.health_milli);
    data.hunger = milli_to_scalar(gameplay.hunger_milli);
    data.saturation = milli_to_scalar(gameplay.saturation_milli);
    data.is_dead = gameplay.is_dead;
    data.experience = gameplay.experience;
    data.experience_level = gameplay.experience_level;

    let mut inventory = data.inventory.to_inventory();
    for (index, slot) in gameplay.inventory[..9].iter().copied().enumerate() {
        inventory.hotbar[index] = slot.and_then(|s| s.to_stack());
    }
    for (index, slot) in gameplay.inventory[9..36].iter().copied().enumerate() {
        inventory.main[index] = slot.and_then(|s| s.to_stack());
    }
    for (index, slot) in gameplay.inventory[36..40].iter().copied().enumerate() {
        inventory.armor[index] = slot.and_then(|s| s.to_stack());
    }
    inventory.offhand = gameplay.inventory[40].and_then(|s| s.to_stack());
    inventory.dragged = gameplay.cursor.and_then(|s| s.to_stack());
    inventory.selected = usize::from(gameplay.selected_hotbar_slot.min(8));
    data.inventory = crate::save::InventoryData::from(&inventory);
}

fn within_reach(session: &PlayerSessionState, x: i32, y: i32, z: i32) -> bool {
    let position = Vec3::from_array(session.data.position);
    position.distance_squared(Vec3::new(x as f32, y as f32, z as f32))
        <= PLAYER_REACH * PLAYER_REACH
}

fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let tmp = path.with_extension(format!("tmp-{}-{unique}", std::process::id()));
    fs::write(&tmp, bytes)?;
    match fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = fs::remove_file(&tmp);
            Err(error)
        }
    }
}

impl Drop for ServerRuntime {
    fn drop(&mut self) {
        if !self.save_flushed {
            let _ = self.shutdown();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::protocol::{BlockActionKind, RejectReason};
    use crate::world::BlockType;

    fn temp_dir(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!("icraft_plan16_{label}_{unique}"))
    }

    fn embedded_runtime(label: &str) -> (ServerRuntime, RuntimeInput) {
        let mut properties = ServerProperties::default();
        properties.bind = "127.0.0.1".into();
        properties.port = 25580;
        properties.view_distance = 2;
        properties.simulation_distance = 2;
        properties.world_dir = temp_dir(label);
        ServerRuntime::new_embedded(
            properties,
            EmbeddedRuntimeOptions::singleplayer(LocalSessionProfile::new(99, "local")),
        )
        .unwrap()
    }

    fn write_world_meta(
        world_dir: &Path,
        game_mode: GameMode,
        cheats_enabled: bool,
    ) -> io::Result<()> {
        fs::create_dir_all(world_dir)?;
        fs::write(
            world_dir.join("world.meta"),
            format!(
                "name:TEST\nseed:1\ngame_mode:{}\ndifficulty:NORMAL\nlast_played:0\nworld_type:DEFAULT\ngenerate_structures:true\nbonus_chest:false\ncheats_enabled:{cheats_enabled}\nhardcore:false\nversion:3\nneeds_upgrade:false\n",
                match game_mode {
                    GameMode::Creative => "CREATIVE",
                    GameMode::Survival => "SURVIVAL",
                    GameMode::Adventure => "ADVENTURE",
                    GameMode::Spectator => "SPECTATOR",
                }
            ),
        )
    }

    fn embedded_runtime_in(world_dir: PathBuf) -> (ServerRuntime, RuntimeInput) {
        let mut properties = ServerProperties::default();
        properties.bind = "127.0.0.1".into();
        properties.port = 25580;
        properties.view_distance = 2;
        properties.simulation_distance = 2;
        properties.world_dir = world_dir;
        ServerRuntime::new_embedded(
            properties,
            EmbeddedRuntimeOptions::singleplayer(LocalSessionProfile::new(99, "local")),
        )
        .unwrap()
    }

    #[test]
    fn creative_world_meta_seeds_new_player_and_survives_reload() {
        let world_dir = temp_dir("creative_persist");
        write_world_meta(&world_dir, GameMode::Creative, false).unwrap();

        let (mut runtime, _input) = embedded_runtime_in(world_dir.clone());
        assert_eq!(
            runtime.authority.session(99).unwrap().game_mode,
            GameMode::Creative
        );
        assert!(!runtime.level.cheats_enabled);
        runtime.shutdown().unwrap();
        drop(runtime);

        let (mut restored, _input) = embedded_runtime_in(world_dir.clone());
        assert_eq!(
            restored.authority.session(99).unwrap().game_mode,
            GameMode::Creative
        );
        assert!(!restored.level.cheats_enabled);
        restored.shutdown().unwrap();
        let _ = fs::remove_dir_all(world_dir);
    }

    #[test]
    fn creative_world_recovers_player_dat_forced_to_survival() {
        let world_dir = temp_dir("creative_recover");
        write_world_meta(&world_dir, GameMode::Creative, false).unwrap();
        let manager = SaveManager::new(&world_dir);
        manager
            .save_player_and_level(
                &LevelData::default(),
                &default_player_data(GameMode::Survival),
            )
            .unwrap();

        let (mut runtime, _input) = embedded_runtime_in(world_dir.clone());
        assert_eq!(
            runtime.authority.session(99).unwrap().game_mode,
            GameMode::Creative
        );
        runtime.shutdown().unwrap();
        let _ = fs::remove_dir_all(world_dir);
    }

    #[test]
    fn world_meta_cheats_survive_first_level_save() {
        let world_dir = temp_dir("cheats_persist");
        write_world_meta(&world_dir, GameMode::Creative, true).unwrap();

        let (mut runtime, _input) = embedded_runtime_in(world_dir.clone());
        assert!(runtime.level.cheats_enabled);
        assert!(runtime.authority.session(99).unwrap().cheats_enabled);
        runtime.shutdown().unwrap();
        drop(runtime);

        let (mut restored, _input) = embedded_runtime_in(world_dir.clone());
        assert!(restored.level.cheats_enabled);
        assert!(restored.authority.session(99).unwrap().cheats_enabled);
        restored.shutdown().unwrap();
        let _ = fs::remove_dir_all(world_dir);
    }

    #[test]
    fn cheats_world_keeps_saved_survival_after_mode_change() {
        let world_dir = temp_dir("cheats_keep_survival");
        write_world_meta(&world_dir, GameMode::Creative, true).unwrap();
        let manager = SaveManager::new(&world_dir);
        let mut level = LevelData::default();
        level.cheats_enabled = true;
        manager
            .save_player_and_level(&level, &default_player_data(GameMode::Survival))
            .unwrap();

        let (mut runtime, _input) = embedded_runtime_in(world_dir.clone());
        assert_eq!(
            runtime.authority.session(99).unwrap().game_mode,
            GameMode::Survival
        );
        runtime.shutdown().unwrap();
        let _ = fs::remove_dir_all(world_dir);
    }

    #[test]
    fn invalid_properties_fail_before_world_creation() {
        let path = temp_dir("invalid").join("server.properties");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "port=0\n").unwrap();
        let error = ServerProperties::load(&path).unwrap_err();
        assert!(error.to_string().contains("port"));
        assert!(!path.parent().unwrap().join("world").exists());
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn runtime_pose_validation_rejects_regression_and_speed_but_allows_server_teleport() {
        let (mut runtime, _input) = embedded_runtime("pose_validation");
        let initial = runtime.players[&99].data.position;
        runtime
            .handle_position(
                99,
                1,
                100,
                initial[0] + 1.0,
                initial[1],
                initial[2],
                0.5,
                0.1,
            )
            .unwrap();
        let accepted = runtime.players[&99].data.position;

        runtime
            .handle_position(
                99,
                2,
                90,
                accepted[0] + 1.0,
                accepted[1],
                accepted[2],
                0.5,
                0.1,
            )
            .unwrap();
        assert_eq!(runtime.players[&99].data.position, accepted);
        runtime
            .handle_position(99, 2, 150, 5_000.0, accepted[1], 5_000.0, 0.5, 0.1)
            .unwrap();
        assert_eq!(runtime.players[&99].data.position, accepted);

        let teleport = [5_000.0, accepted[1], 5_000.0];
        runtime.players.get_mut(&99).unwrap().teleport_allowance = Some(teleport);
        runtime
            .handle_position(99, 2, 150, teleport[0], teleport[1], teleport[2], 0.5, 0.1)
            .unwrap();
        assert_eq!(runtime.players[&99].data.position, teleport);

        let world_dir = runtime.world_dir.clone();
        runtime.shutdown().unwrap();
        let _ = fs::remove_dir_all(world_dir);
    }

    #[test]
    fn presentation_saturation_preserves_ack_and_session_and_stays_bounded() {
        let (mut runtime, _input) = embedded_runtime("presentation_saturation");
        runtime.presentation_events.clear();
        assert!(
            runtime.push_presentation_event(RuntimePresentationEvent::GameplayResponse {
                target: 99,
                response: GameplayResponse {
                    request_id: 700,
                    server_sequence: 1,
                    outcome: GameplayOutcome::Accepted { revision: 1 },
                },
            })
        );
        let mut session_state = SessionGameplayWire::default();
        session_state.revision = 77;
        assert!(
            runtime.push_presentation_event(RuntimePresentationEvent::PlayerSessionUpdate {
                target: 99,
                sequence: 1,
                player_id: 99,
                dimension: Dimension::Overworld as u8,
                state: session_state,
            },)
        );

        for index in 0..(MAX_PRESENTATION_EVENTS_PER_TICK * 2) {
            assert!(
                runtime.push_presentation_event(RuntimePresentationEvent::PlayerPosition {
                    target: 99,
                    id: 10_000 + index as u64,
                    sequence: index as u32 + 1,
                    sender_time_millis: index as u64 + 1,
                    position: [index as f32, 80.0, 0.0],
                    yaw: 0.0,
                    pitch: 0.0,
                },)
            );
        }
        assert_eq!(
            runtime.presentation_events.len(),
            MAX_PRESENTATION_EVENTS_PER_TICK
        );
        assert!(runtime.presentation_events.iter().any(|event| matches!(
            event,
            RuntimePresentationEvent::GameplayResponse { response, .. }
                if response.request_id == 700
        )));
        assert!(runtime.presentation_events.iter().any(|event| matches!(
            event,
            RuntimePresentationEvent::PlayerSessionUpdate { state, .. }
                if state.revision == 77
        )));
        assert!(runtime.network_metrics.snapshot().queue_full > 0);

        runtime.presentation_events.clear();
        for index in 0..(MAX_PRESENTATION_QUEUE_LEN + 8) {
            let accepted =
                runtime.push_presentation_event(RuntimePresentationEvent::GameplayResponse {
                    target: 99,
                    response: GameplayResponse {
                        request_id: index as u128,
                        server_sequence: index as u64 + 1,
                        outcome: GameplayOutcome::Accepted {
                            revision: index as u64 + 1,
                        },
                    },
                });
            assert_eq!(accepted, index < MAX_PRESENTATION_QUEUE_LEN);
        }
        assert_eq!(
            runtime.presentation_events.len(),
            MAX_PRESENTATION_QUEUE_LEN
        );
        assert!(runtime.presentation_events.iter().any(|event| matches!(
            event,
            RuntimePresentationEvent::GameplayResponse { response, .. }
                if response.request_id == 0
        )));

        let world_dir = runtime.world_dir.clone();
        runtime.shutdown().unwrap();
        let _ = fs::remove_dir_all(world_dir);
    }

    #[test]
    fn embedded_interest_fanout_is_private_dimension_safe_and_exactly_once() {
        let (mut runtime, _input) = embedded_runtime("interest_fanout");
        runtime.login_session(2, "remote").unwrap();
        let baseline = runtime.tick_with_output().unwrap();
        assert!(baseline.presentation_events.iter().any(|event| matches!(
            event,
            RuntimePresentationEvent::ChunkData { target: 99, .. }
        )));
        runtime.drain_routed_updates();

        runtime.authority.with_world(Dimension::Overworld, |world| {
            world
                .set_block(8, 80, 8, crate::world::BlockType::Glass, 0)
                .unwrap();
        });
        let revision = runtime.session_revision(2).unwrap();
        let response = runtime
            .submit_request(
                2,
                GameplayRequest {
                    request_id: 700,
                    client_sequence: 1,
                    session_id: 2,
                    dimension: Dimension::Overworld as u8,
                    client_revision: revision,
                    operation: GameplayOperation::BlockAction {
                        action: BlockActionKind::Place,
                        x: 8,
                        y: 80,
                        z: 8,
                        face: [0, 1, 0],
                        hand: 0,
                        held: None,
                        block: crate::world::BlockType::DiamondOre.to_wire(),
                        look_milli: [0, 0, 1000],
                    },
                },
            )
            .unwrap();
        assert!(matches!(
            response.outcome,
            GameplayOutcome::Rejected {
                reason: RejectReason::InvalidState
            }
        ));
        let output = runtime.tick_with_output().unwrap();
        assert_eq!(
            output
                .presentation_events
                .iter()
                .filter(|event| matches!(
                    event,
                    RuntimePresentationEvent::BlockChange {
                        target: 99,
                        x: 8,
                        y: 80,
                        z: 8,
                        ..
                    }
                ))
                .count(),
            0,
            "rejected BlockAction must not project a BlockChange"
        );
        assert_eq!(
            runtime
                .authority
                .world_ref(Dimension::Overworld)
                .map(|world| world.get_block(8, 80, 8)),
            Some(crate::world::BlockType::Glass)
        );

        assert!(runtime.set_session_dimension(2, Dimension::Nether));
        runtime.drain_routed_updates();
        runtime.authority.with_world(Dimension::Overworld, |world| {
            world
                .set_block(9, 80, 8, crate::world::BlockType::Stone, 0)
                .unwrap();
        });
        let revision = runtime.session_revision(99).unwrap();
        let response = runtime
            .submit_request(
                99,
                GameplayRequest {
                    request_id: 701,
                    client_sequence: 1,
                    session_id: 99,
                    dimension: Dimension::Overworld as u8,
                    client_revision: revision,
                    operation: GameplayOperation::BlockAction {
                        action: BlockActionKind::Place,
                        x: 9,
                        y: 80,
                        z: 8,
                        face: [0, 1, 0],
                        hand: 0,
                        held: None,
                        block: crate::world::BlockType::DiamondOre.to_wire(),
                        look_milli: [0, 0, 1000],
                    },
                },
            )
            .unwrap();
        assert!(matches!(
            response.outcome,
            GameplayOutcome::Rejected {
                reason: RejectReason::InvalidState
            }
        ));
        assert!(!runtime
            .drain_routed_updates()
            .iter()
            .any(|update| update.target == 2 && update.dimension == Dimension::Overworld));
        assert_eq!(
            runtime
                .authority
                .world_ref(Dimension::Overworld)
                .map(|world| world.get_block(9, 80, 8)),
            Some(crate::world::BlockType::Stone)
        );

        let chest = (8, 80, 8);
        runtime
            .players
            .get_mut(&99)
            .unwrap()
            .interest
            .open_containers
            .clear();
        assert!(runtime.set_session_dimension(2, Dimension::Overworld));
        runtime
            .players
            .get_mut(&2)
            .unwrap()
            .interest
            .open_containers
            .insert(chest);
        assert_eq!(
            runtime.queue_interest_update(
                Dimension::Overworld,
                runtime
                    .authority
                    .revision_for_dimension(Dimension::Overworld),
                InterestKind::Container(chest),
            ),
            vec![2]
        );

        let world_dir = runtime.world_dir.clone();
        runtime.shutdown().unwrap();
        let _ = fs::remove_dir_all(world_dir);
    }

    #[test]
    fn embedded_block_action_loads_an_interested_boundary_chunk_on_demand() {
        let (mut runtime, _input) = embedded_runtime("boundary_block_action");
        let seed = runtime.level.seed;
        let generated = crate::dimension::generate_chunk(Dimension::Overworld, 1, 0, seed);
        let local_z = 8usize;
        let mut target_y = i32::from(generated.heightmap[0][local_z]);
        while !generated
            .get_block_local(0, target_y, local_z)
            .properties()
            .is_solid
        {
            target_y -= 1;
        }
        let target = (16, target_y, local_z as i32);
        assert_ne!(
            generated.get_block_local(0, target_y, local_z),
            BlockType::Air
        );

        let player_position = [15.25, target_y as f32 + 1.0, local_z as f32 + 0.5];
        assert!(runtime.teleport_session(99, player_position));
        assert!(runtime
            .authority
            .world_ref(Dimension::Overworld)
            .is_some_and(|world| !world.chunks.chunks.contains_key(&(1, 0))));
        runtime.authority.session_mut(99).unwrap().game_mode = GameMode::Creative;
        let held = runtime
            .authority
            .session(99)
            .and_then(|session| session.gameplay.slot(session.gameplay.selected_hotbar_slot))
            .flatten()
            .map(Into::into);

        let eye = Vec3::from_array(player_position) + Vec3::new(0.0, 1.62, 0.0);
        let look = (Vec3::new(16.5, target_y as f32 + 0.5, local_z as f32 + 0.5) - eye).normalize();
        let look_milli = [
            (look.x * 1_000.0).round() as i16,
            (look.y * 1_000.0).round() as i16,
            (look.z * 1_000.0).round() as i16,
        ];
        let response = runtime
            .submit_request(
                99,
                GameplayRequest {
                    request_id: 702,
                    client_sequence: 1,
                    session_id: 99,
                    dimension: Dimension::Overworld as u8,
                    client_revision: runtime.session_revision(99).unwrap(),
                    operation: GameplayOperation::BlockAction {
                        action: crate::network::protocol::BlockActionKind::StartBreak,
                        x: target.0,
                        y: target.1,
                        z: target.2,
                        face: [-1, 0, 0],
                        hand: 0,
                        held,
                        block: BlockType::Air.to_wire(),
                        look_milli,
                    },
                },
            )
            .unwrap();
        let loaded_world = runtime
            .authority
            .world_ref(Dimension::Overworld)
            .expect("overworld remains loaded");
        let loaded_block = loaded_world.get_block(target.0, target.1, target.2);
        let has_line_of_sight =
            loaded_world.has_block_line_of_sight(player_position, look_milli, target);
        assert!(
            matches!(response.outcome, GameplayOutcome::Accepted { .. }),
            "boundary action was rejected: {:?} (target={target:?}, block={loaded_block:?}, player={player_position:?}, look={look_milli:?}, los={has_line_of_sight})",
            response.outcome,
        );
        assert_eq!(
            runtime
                .authority
                .world_ref(Dimension::Overworld)
                .unwrap()
                .get_block(target.0, target.1, target.2),
            BlockType::Air
        );

        let world_dir = runtime.world_dir.clone();
        runtime.shutdown().unwrap();
        let _ = fs::remove_dir_all(world_dir);
    }

    #[test]
    fn opening_new_container_replaces_old_session_and_preserves_other_viewers() {
        fn request(
            id: u64,
            sequence: u64,
            revision: u64,
            position: (i32, i32, i32),
        ) -> GameplayRequest {
            GameplayRequest {
                request_id: sequence as u128,
                client_sequence: sequence,
                session_id: id,
                dimension: Dimension::Overworld as u8,
                client_revision: revision,
                operation: GameplayOperation::Container {
                    action: ContainerAction::Open.to_wire(),
                    x: position.0,
                    y: position.1,
                    z: position.2,
                    slot: 0,
                },
            }
        }

        let prepare =
            |runtime: &mut ServerRuntime, first: (i32, i32, i32), second: (i32, i32, i32)| {
                runtime.authority.with_world(Dimension::Overworld, |world| {
                    world
                        .set_block(first.0, first.1, first.2, BlockType::Chest, 0)
                        .unwrap();
                    world
                        .set_block(second.0, second.1, second.2, BlockType::Chest, 0)
                        .unwrap();
                });
            };

        let first = (10, 80, 8);
        let second = (11, 80, 8);
        let (mut runtime, _input) = embedded_runtime("container_open_replace");
        prepare(&mut runtime, first, second);
        let revision = runtime.session_revision(99).unwrap();
        assert!(matches!(
            runtime
                .submit_request(99, request(99, 1, revision, first))
                .unwrap()
                .outcome,
            GameplayOutcome::Accepted { .. }
        ));
        let revision = runtime.session_revision(99).unwrap();
        assert!(matches!(
            runtime
                .submit_request(99, request(99, 2, revision, second))
                .unwrap()
                .outcome,
            GameplayOutcome::Accepted { .. }
        ));
        let world = runtime.authority.world_ref(Dimension::Overworld).unwrap();
        assert!(world.container_viewers_at(first).next().is_none());
        assert!(
            !crate::world::BlockState::decode(world.get_block_state(first.0, first.1, first.2))
                .is_open
        );
        assert_eq!(
            world
                .container_viewers_at(second)
                .copied()
                .collect::<Vec<_>>(),
            vec![99]
        );
        assert!(
            crate::world::BlockState::decode(world.get_block_state(second.0, second.1, second.2))
                .is_open
        );
        let world_dir = runtime.world_dir.clone();
        runtime.shutdown().unwrap();
        let _ = fs::remove_dir_all(world_dir);

        let (mut runtime, _input) = embedded_runtime("container_open_replace_observer");
        prepare(&mut runtime, first, second);
        runtime.login_session(2, "observer").unwrap();
        let revision = runtime.session_revision(99).unwrap();
        assert!(matches!(
            runtime
                .submit_request(99, request(99, 1, revision, first))
                .unwrap()
                .outcome,
            GameplayOutcome::Accepted { .. }
        ));
        let revision = runtime.session_revision(2).unwrap();
        assert!(matches!(
            runtime
                .submit_request(2, request(2, 1, revision, first))
                .unwrap()
                .outcome,
            GameplayOutcome::Accepted { .. }
        ));
        let revision = runtime.session_revision(99).unwrap();
        assert!(matches!(
            runtime
                .submit_request(99, request(99, 2, revision, second))
                .unwrap()
                .outcome,
            GameplayOutcome::Accepted { .. }
        ));
        let world = runtime.authority.world_ref(Dimension::Overworld).unwrap();
        assert_eq!(
            world
                .container_viewers_at(first)
                .copied()
                .collect::<Vec<_>>(),
            vec![2]
        );
        assert!(
            crate::world::BlockState::decode(world.get_block_state(first.0, first.1, first.2))
                .is_open
        );
        assert_eq!(
            world
                .container_viewers_at(second)
                .copied()
                .collect::<Vec<_>>(),
            vec![99]
        );
        assert!(
            crate::world::BlockState::decode(world.get_block_state(second.0, second.1, second.2))
                .is_open
        );
        let world_dir = runtime.world_dir.clone();
        runtime.shutdown().unwrap();
        let _ = fs::remove_dir_all(world_dir);
    }

    #[test]
    fn complete_session_health_death_inventory_and_xp_reach_local_projection_once() {
        let (mut runtime, _input) = embedded_runtime("session_projection");
        let _ = runtime.tick_with_output().unwrap();
        let mut item = ItemWire::empty();
        item.item = crate::inventory::Item::Diamond as u32;
        item.count = 3;
        let gameplay = &mut runtime.authority.session_mut(99).unwrap().gameplay;
        gameplay.health_milli = 0;
        gameplay.is_dead = true;
        gameplay.death_source = Some(6);
        gameplay.experience = 77;
        gameplay.experience_level = 4;
        gameplay.selected_hotbar_slot = 5;
        gameplay.inventory[0] = Some(SessionInventorySlot::from_wire(item, 1, 2));
        gameplay.revision = 1;

        let output = runtime.tick_with_output().unwrap();
        let updates: Vec<_> = output
            .presentation_events
            .iter()
            .filter_map(|event| match event {
                RuntimePresentationEvent::PlayerSessionUpdate { state, .. }
                    if state.revision == 1 =>
                {
                    Some(*state)
                }
                _ => None,
            })
            .collect();
        assert_eq!(updates.len(), 1);
        assert!(updates[0].is_dead);
        assert_eq!(updates[0].health_milli, 0);
        assert_eq!(updates[0].experience, 77);
        assert_eq!(updates[0].hotbar[0].unwrap().item, item);
        let player_data = &runtime.players[&99].data;
        assert!(player_data.is_dead);
        assert_eq!(player_data.experience, 77);
        assert_eq!(player_data.experience_level, 4);
        assert_eq!(player_data.inventory.selected, 5);
        assert!(runtime
            .tick_with_output()
            .unwrap()
            .presentation_events
            .iter()
            .all(|event| !matches!(
                event,
                RuntimePresentationEvent::PlayerSessionUpdate { state, .. }
                    if state.revision == 1
            )));

        let world_dir = runtime.world_dir.clone();
        runtime.shutdown().unwrap();
        let _ = fs::remove_dir_all(world_dir);
    }

    #[test]
    fn properties_roundtrip_and_whitelist_are_deterministic() {
        let dir = temp_dir("properties");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("server.properties");
        let mut expected = ServerProperties::default();
        expected.port = 25570;
        expected.whitelist = ["Alex".to_ascii_lowercase(), "Steve".to_ascii_lowercase()]
            .into_iter()
            .collect();
        expected.write(&path).unwrap();
        let loaded = ServerProperties::load(&path).unwrap();
        assert_eq!(loaded.port, expected.port);
        assert_eq!(loaded.whitelist, expected.whitelist);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn request_deduplication_and_out_of_order_are_authoritative() {
        let mut properties = ServerProperties::default();
        properties.bind = "127.0.0.1".into();
        properties.port = 0;
        // `new` validates a real port, so exercise the protocol core without
        // opening a listener by constructing a temporary runtime through the
        // normal path and replacing the ephemeral bind port.
        properties.port = 25565;
        properties.world_dir = temp_dir("dedupe");
        let mut runtime = ServerRuntime::new(properties).unwrap();
        runtime
            .handle_join(1, "steve".into())
            .expect("join should be local and deterministic");
        let request = GameplayRequest {
            request_id: 17,
            client_sequence: 1,
            session_id: 1,
            dimension: 0,
            client_revision: 0,
            operation: GameplayOperation::ItemUse { item: 1, count: 1 },
        };
        let first = runtime.submit_request(1, request.clone()).unwrap();
        let duplicate = runtime.submit_request(1, request).unwrap();
        assert_eq!(first, duplicate);
        let stale = runtime
            .submit_request(
                1,
                GameplayRequest {
                    request_id: 18,
                    client_sequence: 1,
                    session_id: 1,
                    dimension: 0,
                    client_revision: 0,
                    operation: GameplayOperation::ItemUse { item: 1, count: 1 },
                },
            )
            .unwrap();
        assert!(matches!(
            stale.outcome,
            GameplayOutcome::Rejected {
                reason: RejectReason::OutOfOrder
            }
        ));
        let _ = runtime.shutdown();
        let _ = fs::remove_dir_all(&runtime.world_dir);
    }

    #[test]
    fn headless_two_sessions_share_one_authoritative_sequence() {
        let mut properties = ServerProperties::default();
        properties.bind = "127.0.0.1".into();
        properties.port = 25566;
        properties.world_dir = temp_dir("competition");
        let mut runtime = ServerRuntime::new(properties).unwrap();
        runtime.handle_join(1, "alex".into()).unwrap();
        runtime.handle_join(2, "steve".into()).unwrap();
        let before = runtime.authority.world().get_block(8, 80, 8);
        let first = runtime
            .submit_request(
                1,
                GameplayRequest {
                    request_id: 1,
                    client_sequence: 1,
                    session_id: 1,
                    dimension: 0,
                    client_revision: 0,
                    operation: GameplayOperation::BlockAction {
                        action: BlockActionKind::Place,
                        x: 8,
                        y: 80,
                        z: 8,
                        face: [0, 1, 0],
                        hand: 0,
                        held: None,
                        block: 1,
                        look_milli: [0, 0, 1000],
                    },
                },
            )
            .unwrap();
        let second = runtime
            .submit_request(
                2,
                GameplayRequest {
                    request_id: 2,
                    client_sequence: 1,
                    session_id: 2,
                    dimension: 0,
                    client_revision: 0,
                    operation: GameplayOperation::BlockAction {
                        action: BlockActionKind::Place,
                        x: 8,
                        y: 80,
                        z: 8,
                        face: [0, 1, 0],
                        hand: 0,
                        held: None,
                        block: 2,
                        look_milli: [0, 0, 1000],
                    },
                },
            )
            .unwrap();
        assert!(matches!(
            first.outcome,
            GameplayOutcome::Rejected {
                reason: RejectReason::InvalidState
            }
        ));
        assert!(matches!(
            second.outcome,
            GameplayOutcome::Rejected {
                reason: RejectReason::InvalidState
            }
        ));
        assert!(second.server_sequence > first.server_sequence);
        assert_eq!(runtime.authority.world().get_block(8, 80, 8), before);
        let _ = runtime.shutdown();
        let _ = fs::remove_dir_all(&runtime.world_dir);
    }

    #[test]
    fn dimension_interest_and_session_transfer_are_isolated() {
        let mut properties = ServerProperties::default();
        properties.bind = "127.0.0.1".into();
        properties.port = 25567;
        properties.world_dir = temp_dir("dimension_interest");
        let mut runtime = ServerRuntime::new(properties).unwrap();
        runtime.handle_join(1, "alex".into()).unwrap();
        runtime.handle_join(2, "steve".into()).unwrap();

        runtime.authority.with_world(Dimension::Overworld, |world| {
            assert!(world.ensure_entity(
                101,
                crate::entity::EntityType::Cow,
                [8.0, 80.0, 8.0],
                10.0,
            ));
        });
        runtime.authority.with_world(Dimension::Nether, |world| {
            assert!(world.ensure_entity(
                202,
                crate::entity::EntityType::Piglin,
                [8.0, 80.0, 8.0],
                10.0,
            ));
        });
        assert!(runtime.set_session_dimension(2, Dimension::Nether));
        runtime.update_interest_for(1, Dimension::Overworld, [8.0, 80.0, 8.0]);
        runtime.update_interest_for(2, Dimension::Nether, [8.0, 80.0, 8.0]);
        assert!(runtime.players[&1].interest.entities.contains(&101));
        assert!(!runtime.players[&1].interest.entities.contains(&202));
        assert!(runtime.players[&2].interest.entities.contains(&202));
        assert!(!runtime.players[&2].interest.entities.contains(&101));

        let _ = runtime.shutdown();
        let _ = fs::remove_dir_all(&runtime.world_dir);
    }

    fn entity_lifecycle_counts(
        events: &[RuntimePresentationEvent],
        entity_id: u64,
    ) -> (usize, usize, usize) {
        let mut spawns = 0;
        let mut states = 0;
        let mut despawns = 0;
        for event in events {
            match event {
                RuntimePresentationEvent::EntitySpawn { state, .. }
                    if state.entity_id == entity_id =>
                {
                    spawns += 1;
                }
                RuntimePresentationEvent::EntityState { state, .. }
                    if state.entity_id == entity_id =>
                {
                    states += 1;
                }
                RuntimePresentationEvent::EntityDespawn {
                    entity_id: id, ..
                } if *id == entity_id => {
                    despawns += 1;
                }
                _ => {}
            }
        }
        (spawns, states, despawns)
    }

    #[test]
    fn entity_state_broadcasts_dirty_or_entered_only() {
        let (mut runtime, _input) = embedded_runtime("entity_dirty");
        let _ = runtime.tick_with_output().unwrap();
        let player_pos = runtime.players[&99].data.position;
        const ENTITY_ID: u64 = 101;
        runtime.authority.with_world(Dimension::Overworld, |world| {
            assert!(world.ensure_entity(
                ENTITY_ID,
                crate::entity::EntityType::EndCrystal,
                player_pos,
                5.0,
            ));
        });

        let entered = runtime.tick_with_output().unwrap();
        let (spawns, states, despawns) =
            entity_lifecycle_counts(&entered.presentation_events, ENTITY_ID);
        assert!(
            spawns + states >= 1,
            "entering the interest set must project a full entity payload"
        );
        assert_eq!(despawns, 0);
        assert!(runtime.players[&99]
            .interest
            .simulation_entities
            .contains(&ENTITY_ID));

        let quiet = runtime.tick_with_output().unwrap();
        let (spawns, states, _) = entity_lifecycle_counts(&quiet.presentation_events, ENTITY_ID);
        assert_eq!(spawns, 0);
        assert_eq!(
            states, 0,
            "stationary pose/health/anim must not re-encode every tick"
        );

        runtime.authority.with_world(Dimension::Overworld, |world| {
            let entity = world.entities.get_by_id_mut(ENTITY_ID).unwrap();
            entity.health = 4.0;
        });
        let dirty = runtime.tick_with_output().unwrap();
        let (_, states, _) = entity_lifecycle_counts(&dirty.presentation_events, ENTITY_ID);
        assert_eq!(states, 1);
        let quiet_after_dirty = runtime.tick_with_output().unwrap();
        let (_, states, _) =
            entity_lifecycle_counts(&quiet_after_dirty.presentation_events, ENTITY_ID);
        assert_eq!(states, 0);

        let far = [player_pos[0] + 10_000.0, player_pos[1], player_pos[2]];
        assert!(runtime.teleport_session(99, far));
        let left: Vec<_> = runtime.presentation_events.drain(..).collect();
        let (_, _, despawns) = entity_lifecycle_counts(&left, ENTITY_ID);
        assert!(despawns >= 1);
        assert!(!runtime.players[&99]
            .interest
            .simulation_entities
            .contains(&ENTITY_ID));

        assert!(runtime.teleport_session(99, player_pos));
        let reentered: Vec<_> = runtime.presentation_events.drain(..).collect();
        let (spawns, _, _) = entity_lifecycle_counts(&reentered, ENTITY_ID);
        assert!(
            spawns >= 1,
            "re-entering view distance must send a full EntitySpawn"
        );
        assert!(runtime.players[&99]
            .last_projected_entity_states
            .get(&ENTITY_ID)
            .is_none());
        let reentered_tick = runtime.tick_with_output().unwrap();
        let (spawns, states, _) =
            entity_lifecycle_counts(&reentered_tick.presentation_events, ENTITY_ID);
        assert_eq!(spawns, 0);
        assert_eq!(
            states, 1,
            "re-entering the simulation set must send a full EntityState once"
        );
        let quiet_reentered = runtime.tick_with_output().unwrap();
        let (_, states, _) =
            entity_lifecycle_counts(&quiet_reentered.presentation_events, ENTITY_ID);
        assert_eq!(states, 0);

        let world_dir = runtime.world_dir.clone();
        runtime.shutdown().unwrap();
        let _ = fs::remove_dir_all(world_dir);
    }

    #[test]
    fn respawn_updates_authority_dimension_and_position() {
        let mut properties = ServerProperties::default();
        properties.bind = "127.0.0.1".into();
        properties.port = 25571;
        properties.world_dir = temp_dir("respawn_dimension");
        let mut runtime = ServerRuntime::new(properties).unwrap();
        runtime.handle_join(1, "alex".into()).unwrap();
        assert!(runtime.set_session_dimension(1, Dimension::Nether));
        runtime.players.get_mut(&1).unwrap().data.is_dead = true;
        runtime.authority.session_mut(1).unwrap().gameplay.is_dead = true;
        runtime
            .authority
            .session_mut(1)
            .unwrap()
            .gameplay
            .health_milli = 0;
        runtime
            .handle_event(ServerToHost::ClientRespawnRequest { id: 1 })
            .unwrap();
        let player = runtime.players.get(&1).unwrap();
        assert_eq!(player.interest.dimension, runtime.level.spawn_dimension);
        let authority_session = runtime.authority.session(1).unwrap();
        assert_eq!(
            authority_session.dimension,
            player.interest.dimension as u8
        );
        assert_eq!(authority_session.position, player.data.position);
        assert!(!authority_session.gameplay.is_dead);
        assert_eq!(
            authority_session.gameplay.health_milli,
            authority_session.gameplay.max_health_milli
        );
        assert_eq!(authority_session.gameplay.hunger_milli, 20_000);

        let _ = runtime.shutdown();
        let _ = fs::remove_dir_all(&runtime.world_dir);
    }

    #[test]
    fn authority_gameplay_round_trips_through_dedicated_player_save() {
        let mut properties = ServerProperties::default();
        properties.bind = "127.0.0.1".into();
        properties.port = 25572;
        properties.world_dir = temp_dir("gameplay_roundtrip");
        let world_dir = properties.world_dir.clone();
        let mut runtime = ServerRuntime::new(properties.clone()).unwrap();
        runtime.handle_join(1, "alex".into()).unwrap();
        assert!(runtime.set_session_dimension(1, Dimension::Nether));

        let mut wire = ItemWire::empty();
        wire.item = crate::inventory::Item::DiamondSword as u32;
        wire.count = 1;
        wire.durability = 37;
        // ItemWire stores kind/level in the protocol's packed representation.
        // Keep the fixture canonical and avoid the Silk Touch/Fortune
        // incompatibility enforced by EnchantmentSet::add_or_upgrade, so the
        // save round-trip can assert both wire and semantic metadata.
        wire.enchantments = [0x15, 0x23, 0x31, 0x55, 0x62, 0];
        wire.custom_name = [b'R'; 24];
        wire.can_break = 0x11;
        wire.can_place_on = 0x22;
        let mut gameplay = SessionGameplayState::default();
        gameplay.health_milli = 12_345;
        gameplay.hunger_milli = 8_765;
        gameplay.saturation_milli = 1_250;
        gameplay.inventory[0] = Some(SessionInventorySlot::from_wire(wire, 0x11, 0x22));
        let authority_session = runtime.authority.session_mut(1).unwrap();
        authority_session.game_mode = GameMode::Adventure;
        authority_session.gameplay = gameplay;
        runtime.authority.with_world(Dimension::Overworld, |world| {
            world
                .set_block(12, 80, 12, crate::world::BlockType::Glass, 0)
                .unwrap();
        });
        runtime.authority.with_world(Dimension::Nether, |world| {
            world
                .set_block(12, 80, 12, crate::world::BlockType::Obsidian, 0)
                .unwrap();
        });
        runtime.save_all().unwrap();
        runtime.shutdown().unwrap();

        let mut restored = ServerRuntime::new(properties).unwrap();
        restored.handle_join(2, "alex".into()).unwrap();
        let authority_session = restored.authority.session(2).unwrap();
        assert_eq!(authority_session.dimension, Dimension::Nether as u8);
        assert_eq!(authority_session.game_mode, GameMode::Adventure);
        assert_eq!(authority_session.gameplay.health_milli, 12_345);
        assert_eq!(authority_session.gameplay.hunger_milli, 8_765);
        assert_eq!(authority_session.gameplay.saturation_milli, 1_250);
        let saved_slot = authority_session.gameplay.inventory[0].unwrap();
        assert_eq!(saved_slot.item, wire);
        assert_eq!(saved_slot.can_break, 0x11);
        assert_eq!(saved_slot.can_place_on, 0x22);
        let roundtrip_stack = saved_slot.item.to_stack().unwrap();
        assert_eq!(
            roundtrip_stack
                .enchantments
                .level_of(crate::enchantment::Enchantment::Efficiency(1)),
            5
        );
        assert_eq!(
            roundtrip_stack
                .enchantments
                .level_of(crate::enchantment::Enchantment::Unbreaking(1)),
            3
        );
        assert_eq!(
            roundtrip_stack
                .enchantments
                .level_of(crate::enchantment::Enchantment::SilkTouch),
            1
        );
        assert_eq!(
            roundtrip_stack
                .enchantments
                .level_of(crate::enchantment::Enchantment::Sharpness(1)),
            5
        );
        assert_eq!(
            roundtrip_stack
                .enchantments
                .level_of(crate::enchantment::Enchantment::Knockback(1)),
            2
        );
        assert_eq!(
            restored
                .authority
                .world_ref(Dimension::Overworld)
                .unwrap()
                .get_block(12, 80, 12),
            crate::world::BlockType::Glass
        );
        assert_eq!(
            restored
                .authority
                .world_ref(Dimension::Nether)
                .unwrap()
                .get_block(12, 80, 12),
            crate::world::BlockType::Obsidian
        );

        let _ = restored.shutdown();
        let _ = fs::remove_dir_all(world_dir);
    }

    #[test]
    fn disconnect_reconnect_loads_atomic_player_state() {
        let mut properties = ServerProperties::default();
        properties.bind = "127.0.0.1".into();
        properties.port = 25568;
        properties.world_dir = temp_dir("reconnect");
        let mut runtime = ServerRuntime::new(properties).unwrap();
        runtime.handle_join(1, "alex".into()).unwrap();
        runtime.players.get_mut(&1).unwrap().data.position = [12.0, 70.0, -4.0];
        runtime.players.get_mut(&1).unwrap().data.health = 7.5;
        // Gameplay is authority-owned after join; keep the fixture's health
        // mutation on the authoritative session rather than the projection.
        runtime
            .authority
            .session_mut(1)
            .unwrap()
            .gameplay
            .health_milli = 7_500;
        runtime.handle_leave(1).unwrap();
        runtime.handle_join(2, "alex".into()).unwrap();
        let restored = &runtime.players.get(&2).unwrap().data;
        assert_eq!(restored.position, [12.0, 70.0, -4.0]);
        assert_eq!(restored.health, 7.5);
        let _ = runtime.shutdown();
        let _ = fs::remove_dir_all(&runtime.world_dir);
    }

    #[test]
    fn save_failure_is_reported_and_retry_keeps_original_path() {
        let mut properties = ServerProperties::default();
        properties.bind = "127.0.0.1".into();
        properties.port = 25569;
        properties.world_dir = temp_dir("save_failure");
        let mut runtime = ServerRuntime::new(properties).unwrap();
        let world_dir = runtime.world_dir.clone();
        let blocking_file = world_dir.with_extension("blocked");
        fs::write(&blocking_file, b"not a directory").unwrap();
        runtime.world_dir = blocking_file.clone();
        runtime.save_manager.world_dir = blocking_file.clone();
        let saves_before_failure = runtime.metrics.saves;
        assert!(runtime.save_all().is_err());
        assert_eq!(runtime.metrics.saves, saves_before_failure);
        runtime.world_dir = world_dir.clone();
        runtime.save_manager.world_dir = world_dir.clone();
        assert!(runtime.save_all().is_ok());
        assert_eq!(runtime.metrics.saves, saves_before_failure + 1);
        assert!(runtime.metrics.last_save_latency_ms >= 1);
        let _ = runtime.shutdown();
        let _ = fs::remove_dir_all(&world_dir);
        let _ = fs::remove_file(blocking_file);
    }

    #[test]
    fn motd_validate_applies_cli_256_byte_cap() {
        let mut properties = ServerProperties::default();
        properties.motd = "x".repeat(256);
        properties.validate().unwrap();
        properties.motd = "x".repeat(257);
        let error = properties.validate().unwrap_err();
        assert!(error.to_string().contains("motd"));
        assert!(error.to_string().contains("1..=256"));
        properties.motd = "   ".into();
        assert!(properties
            .validate()
            .unwrap_err()
            .to_string()
            .contains("motd"));
    }

    #[test]
    fn online_mode_true_fails_closed_at_validate_and_startup() {
        let mut properties = ServerProperties::default();
        properties.online_mode = true;
        let error = properties.validate().unwrap_err();
        let message = error.to_string();
        assert!(message.contains("online-mode"));
        assert!(
            message.contains("尚未實作驗證，拒絕當憑證開關"),
            "{message}"
        );
        assert!(message.contains("not implemented"), "{message}");

        let dir = temp_dir("online_mode");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("server.properties");
        fs::write(&path, "online-mode=true\n").unwrap();
        let loaded = ServerProperties::load(&path).unwrap_err();
        assert!(loaded.to_string().contains("online-mode"));
        properties.world_dir = dir.join("world");
        properties.bind = "127.0.0.1".into();
        assert!(ServerRuntime::new(properties).is_err());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn login_grants_operator_only_from_console_op_set() {
        let mut properties = ServerProperties::default();
        properties.bind = "127.0.0.1".into();
        properties.port = 25573;
        properties.world_dir = temp_dir("op_policy");
        let mut runtime = ServerRuntime::new(properties).unwrap();

        runtime.login_session(1, "Alice").unwrap();
        assert!(!runtime.authority.session(1).unwrap().operator);
        assert_eq!(runtime.players.get(&1).unwrap().username, "alice");
        runtime.logout_session(1).unwrap();

        runtime.execute_console_command("op Alice").unwrap();
        assert!(runtime.properties.operators.contains("alice"));
        assert!(runtime.execute_console_command("op foo.bar").is_err());
        assert!(runtime.execute_console_command("op CON").is_err());
        assert!(!runtime.properties.operators.contains("foo_bar"));

        runtime.login_session(2, "ALICE").unwrap();
        assert!(runtime.authority.session(2).unwrap().operator);
        runtime.login_session(3, "bob").unwrap();
        assert!(!runtime.authority.session(3).unwrap().operator);
        assert!(runtime.login_session(4, "foo.bar").is_err());
        assert!(runtime.login_session(5, "CON").is_err());

        let _ = runtime.shutdown();
        let _ = fs::remove_dir_all(&runtime.world_dir);
    }

    #[test]
    fn autosave_error_does_not_skip_shutdown_flush() {
        let mut properties = ServerProperties::default();
        properties.bind = "127.0.0.1".into();
        properties.port = 25574;
        properties.world_dir = temp_dir("autosave_flush");
        let mut runtime = ServerRuntime::new(properties).unwrap();
        runtime.login_session(1, "alex").unwrap();
        let saves_before = runtime.metrics.saves;

        SAVE_ALL_FAILPOINT.with(|failpoint| failpoint.set(true));
        runtime.metrics.ticks = AUTOSAVE_INTERVAL_TICKS - 1;
        assert!(runtime.tick().is_ok());
        assert_eq!(runtime.metrics.saves, saves_before);
        SAVE_ALL_FAILPOINT.with(|failpoint| failpoint.set(false));

        runtime.request_shutdown();
        assert!(runtime.is_stopped());
        runtime.shutdown().unwrap();
        assert!(runtime.metrics.saves > saves_before);
        assert!(runtime.save_flushed);

        let world_dir = runtime.world_dir.clone();
        let _ = fs::remove_dir_all(world_dir);
    }

    #[test]
    fn leave_save_failure_still_releases_identity() {
        let mut properties = ServerProperties::default();
        properties.bind = "127.0.0.1".into();
        properties.port = 25575;
        properties.world_dir = temp_dir("leave_save_fail");
        let mut runtime = ServerRuntime::new(properties).unwrap();
        runtime.login_session(1, "alex").unwrap();

        SAVE_PLAYER_FAILPOINT.with(|failpoint| failpoint.set(true));
        runtime.logout_session(1).unwrap();
        SAVE_PLAYER_FAILPOINT.with(|failpoint| failpoint.set(false));

        assert!(runtime.authority.session(1).is_none());
        assert!(!runtime.players.contains_key(&1));
        runtime.login_session(2, "ALEX").unwrap();
        assert_eq!(runtime.players.get(&2).unwrap().username, "alex");

        let _ = runtime.shutdown();
        let _ = fs::remove_dir_all(&runtime.world_dir);
    }

    #[test]
    fn interest_evict_drops_origin_after_long_walk_and_metrics_match() {
        let (mut runtime, _input) = embedded_runtime("residency_walk");
        runtime.run_for_ticks(8).unwrap();
        assert!(runtime
            .authority
            .world_mut_active()
            .chunks
            .chunks
            .contains_key(&(0, 0)));
        assert!(!runtime
            .authority
            .world_mut_active()
            .chunks
            .chunks
            .contains_key(&(8, 0)));
        assert!(runtime.teleport_session(99, [32.0 * 16.0 + 8.0, 80.0, 8.0]));
        runtime.tick().unwrap();
        assert!(!runtime
            .authority
            .world_mut_active()
            .chunks
            .chunks
            .contains_key(&(0, 0)));
        let loaded: usize = runtime
            .authority
            .dimensions()
            .into_iter()
            .filter_map(|dimension| runtime.authority.world_ref(dimension))
            .map(|world| world.chunks.chunks.len())
            .sum();
        assert_eq!(runtime.metrics.loaded_chunks, loaded);
        let _ = runtime.shutdown();
        let _ = fs::remove_dir_all(&runtime.world_dir);
    }

    #[test]
    fn empty_dimension_keeps_only_capped_spawn_ring() {
        let mut properties = ServerProperties::default();
        properties.bind = "127.0.0.1".into();
        properties.port = 25581;
        properties.view_distance = 2;
        properties.simulation_distance = 2;
        properties.world_dir = temp_dir("spawn_ring");
        let (mut runtime, _input) = ServerRuntime::new_embedded(
            properties,
            EmbeddedRuntimeOptions {
                topology: AuthorityTopology::Dedicated,
                transport: TransportMode::Disabled,
                local_session: None,
            },
        )
        .unwrap();
        runtime.authority.with_world(Dimension::Overworld, |world| {
            world.ensure_chunk(0, 0);
            world.ensure_chunk(8, 0);
            world.ensure_chunk(-3, 2);
        });
        runtime.tick().unwrap();
        let world = runtime.authority.world_ref(Dimension::Overworld).unwrap();
        assert!(world.chunks.chunks.contains_key(&(0, 0)));
        assert!(!world.chunks.chunks.contains_key(&(8, 0)));
        assert!(world.chunks.chunks.len() <= crate::authority::interest::SPAWN_RESIDENCY_CAP);
        let keep = crate::authority::interest::capped_spawn_residency(
            runtime.level.spawn_x,
            runtime.level.spawn_z,
        );
        for key in world.chunks.chunks.keys() {
            assert!(keep.contains(key));
        }
        let _ = runtime.shutdown();
        let _ = fs::remove_dir_all(&runtime.world_dir);
    }
}
