//! Headless authoritative server runtime.
//!
//! The desktop application remains a presentation client.  This module owns
//! the fixed tick, authenticated session state, world mutation gate, interest
//! sets, persistence and observability needed by `icraft-server`.  It is
//! deliberately synchronous at the authority boundary; the existing Tokio
//! network thread only transports packets into the bounded event channel.

use crate::authority::contract::{AuthorityTopology, SessionContract};
use crate::authority::interest::{
    InterestKind, InterestSet, RoutedInterestUpdate, MAX_INTEREST_UPDATES_PER_TICK,
};
use crate::authority::{AuthorityConfig, AuthorityCore};
use crate::dimension::Dimension;
use crate::game_rules::WorldRules;
use crate::inventory::{GameMode, Inventory};
use crate::network::protocol::{
    ContainerAction, GameplayOperation, GameplayOutcome, GameplayRequest, GameplayResponse,
    PlayerEffectWire, RejectReason,
};
use crate::network::server::{HostToServer, NetworkServer, ServerConfig, ServerToHost};
use crate::save::{
    ChunkSaveData, EntitySaveData, LevelData, MutationRevisionIndex, PlayerData, SaveManager,
};
use glam::Vec3;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt;
use std::fs;
use std::io;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const TICK_INTERVAL: Duration = Duration::from_millis(50);
const MAX_INBOUND_EVENTS_PER_TICK: usize = 512;
const WORLD_BOUND: f32 = 30_000_000.0;
const PLAYER_REACH: f32 = 8.0;
const AUTOSAVE_INTERVAL_TICKS: u64 = 6_000;
const HOST_COMMAND_QUEUE_CAPACITY: usize = 1_024;
const HOST_EVENT_QUEUE_CAPACITY: usize = 1_024;

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
                    properties.whitelist = value
                        .split(',')
                        .map(str::trim)
                        .filter(|entry| !entry.is_empty())
                        .map(str::to_ascii_lowercase)
                        .collect();
                }
                "operators" | "ops" => {
                    properties.operators = value
                        .split(',')
                        .map(str::trim)
                        .filter(|entry| !entry.is_empty())
                        .map(str::to_ascii_lowercase)
                        .collect();
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
        Ok(())
    }

    pub fn write(&self, path: impl AsRef<Path>) -> Result<(), ServerConfigError> {
        self.validate()?;
        let mut whitelist: Vec<_> = self.whitelist.iter().cloned().collect();
        whitelist.sort();
        let content = format!(
            "bind={}\nport={}\nmotd={}\nmax-players={}\ndifficulty={}\nonline-mode={}\nwhitelist={}\noperators={}\nview-distance={}\nsimulation-distance={}\npvp={}\nlevel-name={}\nlevel-seed={}\n",
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

#[derive(Debug, Clone)]
pub struct PlayerSessionState {
    pub id: u64,
    pub username: String,
    pub data: PlayerData,
    pub dimension: Dimension,
    pub last_client_sequence: u64,
    pub interest: InterestSet,
    /// Compatibility projections retained for existing presentation bridges;
    /// all routing decisions use `interest` as the source of truth.
    pub interest_chunks: HashSet<(i32, i32)>,
    pub simulation_chunks: HashSet<(i32, i32)>,
    pub entity_interest: HashSet<u64>,
    pub simulation_entity_interest: HashSet<u64>,
    pub container_viewers: BTreeSet<(i32, i32, i32)>,
    pub effects: Vec<PlayerEffectWire>,
}

impl PlayerSessionState {
    fn new(
        id: u64,
        username: String,
        data: PlayerData,
        dimension: Dimension,
        view_distance: u8,
        simulation_distance: u8,
    ) -> Self {
        Self {
            id,
            username,
            data,
            dimension,
            last_client_sequence: 0,
            interest: InterestSet::new(dimension, view_distance, simulation_distance),
            interest_chunks: HashSet::new(),
            simulation_chunks: HashSet::new(),
            entity_interest: HashSet::new(),
            simulation_entity_interest: HashSet::new(),
            container_viewers: BTreeSet::new(),
            effects: Vec::new(),
        }
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
    routed_updates: Vec<RoutedInterestUpdate>,
    /// Revisions already projected by an immediate request ACK.  The next
    /// fixed snapshot contains those pending mutations as well; this bounded
    /// set prevents duplicate block/container deltas without dropping later
    /// automation mutations.
    routed_mutations: BTreeSet<u64>,
    world_dir: PathBuf,
    save_manager: SaveManager,
    host_tx: SyncSender<HostToServer>,
    host_rx: Receiver<ServerToHost>,
    network_thread: Option<JoinHandle<()>>,
    stopped: bool,
}

impl ServerRuntime {
    pub fn new(properties: ServerProperties) -> Result<Self, ServerConfigError> {
        properties.validate()?;
        let world_dir = properties.world_dir.clone();
        if world_dir.exists() && !world_dir.is_dir() {
            return Err(ServerConfigError::Io(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("world path {} is not a directory", world_dir.display()),
            )));
        }
        let save_manager = SaveManager::new(&world_dir);
        let mut level = save_manager
            .load_level()
            .map_err(ServerConfigError::Io)?
            .unwrap_or_else(|| LevelData {
                seed: properties.seed as u32,
                rules: WorldRules {
                    pvp: properties.pvp,
                    ..WorldRules::default()
                },
                ..LevelData::default()
            });
        // `server.properties` is the live authority configuration.  A saved
        // level may carry an older rule snapshot, but connection/runtime
        // policy must still apply the operator's pvp and difficulty settings
        // before constructing the shared headless core.
        level.rules.pvp = properties.pvp;
        if properties.difficulty.eq_ignore_ascii_case("peaceful") {
            level.rules.do_mob_spawning = false;
            level.rules.pvp = false;
        }
        level.rules = level.rules.normalized();
        let (host_tx, host_rx_network) = mpsc::sync_channel(HOST_COMMAND_QUEUE_CAPACITY);
        let (server_to_host, host_rx) = mpsc::sync_channel(HOST_EVENT_QUEUE_CAPACITY);
        let mut network_config = ServerConfig::default();
        network_config.max_players = properties.max_players;
        network_config.motd = properties.motd.clone();
        network_config.whitelist = properties.whitelist.clone();
        let bind_addr = format!("{}:{}", properties.bind, properties.port);
        let network_thread = NetworkServer::spawn_with_config(
            bind_addr,
            properties.seed,
            gamemode_wire(&level),
            host_rx_network,
            server_to_host,
            network_config,
        );
        let mut authority = AuthorityCore::new(
            AuthorityConfig {
                seed: level.seed,
                dimension: level.spawn_dimension,
                world_type: crate::game_rules::WorldType::Default,
                generate_structures: false,
                rules: level.rules,
                render_distance: properties.simulation_distance as i32,
            },
            AuthorityTopology::Dedicated,
        );
        authority.world.time = level.time;
        let mut runtime = Self {
            properties,
            level,
            metrics: ServerMetrics::default(),
            players: HashMap::new(),
            authority,
            world_dir,
            save_manager,
            host_tx,
            host_rx,
            network_thread: Some(network_thread),
            routed_updates: Vec::new(),
            routed_mutations: BTreeSet::new(),
            stopped: false,
        };
        runtime.restore_authority_state()?;
        runtime.ensure_spawn_chunk();
        Ok(runtime)
    }

    /// Process one fixed simulation tick.  No wgpu/winit/audio state is
    /// touched, making this safe for dedicated servers and headless tests.
    pub fn tick(&mut self) -> io::Result<()> {
        if self.stopped {
            return Ok(());
        }
        let started = Instant::now();
        let mut processed = 0;
        while processed < MAX_INBOUND_EVENTS_PER_TICK {
            let event = match self.host_rx.try_recv() {
                Ok(event) => event,
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
            };
            processed += 1;
            self.metrics.inbound_packets = self.metrics.inbound_packets.saturating_add(1);
            self.metrics.queue_depth = self.metrics.queue_depth.saturating_sub(1);
            self.handle_event(event)?;
        }
        let snapshot = self.authority.tick();
        self.route_authority_snapshot(&snapshot);
        self.level.time = snapshot.tick;
        self.metrics.ticks = self.metrics.ticks.wrapping_add(1);
        self.metrics.players_online = self.players.len();
        self.metrics.loaded_chunks = self.authority.world.chunks.chunks.len();
        self.metrics.entities = self.authority.world.entities.entities.len();
        self.metrics.queue_depth = self.metrics.queue_depth.saturating_add(processed.max(0));
        if self.metrics.ticks % AUTOSAVE_INTERVAL_TICKS == 0 {
            self.save_all()?;
        }
        if started.elapsed() > TICK_INTERVAL {
            eprintln!("[ServerRuntime] tick over budget: {:?}", started.elapsed());
        }
        Ok(())
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
        if self.stopped {
            return Ok(());
        }
        let save_result = self.save_all();
        self.stopped = true;
        let _ = self.host_tx.try_send(HostToServer::Stop);
        if let Some(handle) = self.network_thread.take() {
            let _ = handle.join();
        }
        save_result
    }

    fn restore_authority_state(&mut self) -> io::Result<()> {
        let dimension = self.authority.world.dimension;
        let revision_index = self.save_manager.load_mutation_revision_index();
        for chunk in self.save_manager.load_saved_chunks_in(dimension)? {
            self.authority.world.restore_saved_chunk(&chunk);
        }
        for ((_cx, _cz), revision) in revision_index.entries_in(dimension) {
            self.authority.world.revisions.observe(revision);
        }
        let entities = self.save_manager.load_entities_in_checked(dimension)?;
        if !entities.is_empty() {
            self.authority.world.restore_saved_entities(&entities);
        }
        Ok(())
    }

    fn save_authority_state(&mut self) -> io::Result<()> {
        let dimension = self.authority.world.dimension;
        let mut coordinates: Vec<_> = self.authority.world.chunks.chunks.keys().copied().collect();
        coordinates.sort_unstable();
        for (cx, cz) in coordinates {
            let Some(chunk) = self.authority.world.chunks.chunks.get(&(cx, cz)) else {
                continue;
            };
            let revision = self.authority.world.chunk_revision(cx, cz);
            let mut data = ChunkSaveData::from_chunk(chunk);
            data.mutation_revision = revision;
            self.save_manager
                .save_chunk_in(dimension, cx, cz, data)
                .map_err(|error| io::Error::new(io::ErrorKind::Other, error.to_string()))?;
        }

        let entities: Vec<_> = self
            .authority
            .world
            .entities
            .entities
            .iter()
            .map(EntitySaveData::from)
            .collect();
        self.save_manager.save_entities_in(dimension, &entities)?;
        let revisions: MutationRevisionIndex = self.authority.world.mutation_revision_index();
        self.save_manager.save_mutation_revision_index(&revisions)?;
        self.save_manager.save_current_dimension(dimension)?;
        Ok(())
    }

    pub fn save_all(&mut self) -> io::Result<()> {
        let started = Instant::now();
        self.save_manager.save_level(&self.level)?;
        self.save_authority_state()?;
        let mut names: Vec<_> = self.players.values().collect();
        names.sort_by_key(|session| session.id);
        for session in names {
            self.save_player(session)?;
        }
        self.metrics.saves = self.metrics.saves.saturating_add(1);
        self.metrics.last_save_latency_ms =
            started.elapsed().as_millis().min(u64::MAX as u128) as u64;
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

    pub fn submit_request(
        &mut self,
        session_id: u64,
        mut request: GameplayRequest,
    ) -> Option<GameplayResponse> {
        request.session_id = session_id;
        self.handle_gameplay_request(request).ok()
    }

    /// Headless/in-process login seam. The network transport calls the same
    /// private handler, while tests and listen-server bridges can exercise the
    /// exact persistence and interest policy without a GPU or socket client.
    pub fn login_session(&mut self, id: u64, username: impl Into<String>) -> io::Result<()> {
        self.handle_join(id, username.into())
    }

    pub fn logout_session(&mut self, id: u64) -> io::Result<()> {
        self.handle_leave(id)
    }

    pub fn set_session_dimension(&mut self, id: u64, dimension: Dimension) -> bool {
        self.authority.world.close_container_viewers(id);
        let Some(session) = self.players.get_mut(&id) else {
            return false;
        };
        session.dimension = dimension;
        session.interest.open_containers.clear();
        session.container_viewers.clear();
        if let Some(authority_session) = self.authority.session_mut(id) {
            authority_session.dimension = dimension as u8;
        }
        let position = session.data.position;
        let _ = session;
        self.update_interest_for(id, dimension, position);
        true
    }

    pub fn metrics(&self) -> &ServerMetrics {
        &self.metrics
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
                    self.properties.whitelist.insert(name.to_ascii_lowercase());
                    self.persist_properties()
                        .map_err(|error| error.to_string())?;
                    Ok(format!("added {name} to whitelist"))
                }
                "remove" => {
                    let name = words.next().ok_or("usage: whitelist remove <name>")?;
                    self.properties.whitelist.remove(&name.to_ascii_lowercase());
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
                let normalized = name.to_ascii_lowercase();
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

    fn handle_event(&mut self, event: ServerToHost) -> io::Result<()> {
        match event {
            ServerToHost::ClientJoined { id, username } => {
                if let Err(error) = self.handle_join(id, username) {
                    let _ = self.enqueue_host(HostToServer::DisconnectClient {
                        to: id,
                        reason: format!("authority login rejected: {error}"),
                    });
                }
                Ok(())
            }
            ServerToHost::ClientLeft { id } => self.handle_leave(id),
            ServerToHost::GameplayRequest { id, mut request } => {
                request.session_id = id;
                let response = self.handle_gameplay_request(request)?;
                self.send_response(id, response);
                Ok(())
            }
            ServerToHost::ClientPosition {
                id,
                sequence,
                sender_time_millis,
                x,
                y,
                z,
                yaw,
                pitch,
            } => self.handle_position(id, sequence, sender_time_millis, x, y, z, yaw, pitch),
            ServerToHost::ClientBlockChange {
                id,
                x,
                y,
                z,
                block,
                state,
            } => self.handle_block_change(id, x, y, z, block, state),
            ServerToHost::ClientAction { id, action } => {
                self.enqueue_host(HostToServer::BroadcastPlayerAction { id, action });
                Ok(())
            }
            ServerToHost::ChatFromClient { id, message } => {
                if let Some(sender) = self
                    .players
                    .get(&id)
                    .map(|session| session.username.clone())
                {
                    self.enqueue_host(HostToServer::BroadcastChat {
                        sender,
                        message: message.chars().take(256).collect(),
                    });
                }
                Ok(())
            }
            ServerToHost::ClientRespawnRequest { id } => {
                self.authority.world.close_container_viewers(id);
                let respawn = if let Some(session) = self.players.get_mut(&id) {
                    session.data.position = [
                        self.level.spawn_x as f32,
                        self.level.spawn_y as f32,
                        self.level.spawn_z as f32,
                    ];
                    session.data.is_dead = false;
                    session.dimension = self.level.spawn_dimension;
                    session.interest.open_containers.clear();
                    session.container_viewers.clear();
                    Some((session.data.position, session.dimension))
                } else {
                    None
                };
                if let Some((respawn_position, dimension)) = respawn {
                    self.enqueue_host(HostToServer::SendPlayerRespawnResult {
                        to: id,
                        position: respawn_position,
                        dimension: dimension as u8,
                    });
                    self.update_interest_for(id, dimension, respawn_position);
                }
                Ok(())
            }
            ServerToHost::ClientBlockAction { id, x, y, z, .. } => {
                self.handle_block_change(id, x, y, z, 0, 0)
            }
            ServerToHost::ClientSleepRequest {
                id,
                bed_x,
                bed_y,
                bed_z,
            } => {
                let Some(request) = self.legacy_request(
                    id,
                    self.authority.current_revision(),
                    GameplayOperation::Sleep {
                        x: bed_x,
                        y: bed_y,
                        z: bed_z,
                    },
                ) else {
                    return Ok(());
                };
                let response = self.handle_gameplay_request(request)?;
                self.send_response(id, response);
                Ok(())
            }
            ServerToHost::ContainerOpenRequest {
                id,
                dimension,
                x,
                y,
                z,
            } => {
                let Some(request) = self.legacy_request(
                    id,
                    self.authority.current_revision(),
                    GameplayOperation::Container {
                        action: ContainerAction::Open.to_wire(),
                        x,
                        y,
                        z,
                        slot: 0,
                    },
                ) else {
                    return Ok(());
                };
                if request.dimension != dimension {
                    self.send_legacy_rejection(
                        id,
                        request.request_id,
                        RejectReason::InvalidDimension,
                    );
                } else {
                    let response = self.handle_gameplay_request(request)?;
                    self.send_response(id, response);
                }
                Ok(())
            }
            ServerToHost::ContainerClickRequest {
                id,
                dimension,
                revision,
                slot_index,
                is_left,
                dragged,
            } => {
                let Some((x, y, z)) = self
                    .players
                    .get(&id)
                    .and_then(|session| session.interest.open_containers.iter().next())
                    .copied()
                else {
                    self.send_legacy_rejection(
                        id,
                        self.authority.current_revision() as u128 + 1,
                        RejectReason::InvalidState,
                    );
                    return Ok(());
                };
                let request = GameplayRequest {
                    request_id: self.authority.current_revision() as u128 + 1,
                    client_sequence: self
                        .authority
                        .session(id)
                        .map(|session| session.last_client_sequence + 1)
                        .unwrap_or(1),
                    session_id: id,
                    dimension,
                    client_revision: revision,
                    operation: GameplayOperation::ContainerClick {
                        x,
                        y,
                        z,
                        slot: slot_index,
                        is_left,
                        dragged,
                    },
                };
                let response = self.handle_gameplay_request(request)?;
                self.send_response(id, response);
                Ok(())
            }
            ServerToHost::ContainerClose {
                id,
                dimension,
                x,
                y,
                z,
            } => {
                let Some(request) = self.legacy_request(
                    id,
                    self.authority.current_revision(),
                    GameplayOperation::Container {
                        action: ContainerAction::Close.to_wire(),
                        x,
                        y,
                        z,
                        slot: 0,
                    },
                ) else {
                    return Ok(());
                };
                if request.dimension != dimension {
                    self.send_legacy_rejection(
                        id,
                        request.request_id,
                        RejectReason::InvalidDimension,
                    );
                } else {
                    let response = self.handle_gameplay_request(request)?;
                    self.send_response(id, response);
                }
                Ok(())
            }
            ServerToHost::CatchupAccepted { .. }
            | ServerToHost::CatchupBackpressured { .. }
            | ServerToHost::CatchupAck { .. }
            | ServerToHost::Disconnected { .. } => Ok(()),
        }
    }

    fn handle_join(&mut self, id: u64, username: String) -> io::Result<()> {
        if self
            .players
            .values()
            .any(|session| session.username.eq_ignore_ascii_case(&username))
        {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("duplicate player identity: {username}"),
            ));
        }
        let saved_player = self.save_manager.load_dedicated_player(&username)?;
        let (data, current_dimension, effects) = saved_player
            .map(|file| (file.data, file.current_dimension, file.effects))
            .unwrap_or_else(|| {
                let data = default_player_data();
                (
                    data.clone(),
                    data.spawn_dimension.unwrap_or(self.level.spawn_dimension),
                    Vec::new(),
                )
            });
        let mut session = PlayerSessionState::new(
            id,
            username,
            data,
            current_dimension,
            self.properties.view_distance,
            self.properties.simulation_distance,
        );
        session.effects = effects;
        self.update_interest(&mut session);
        let dimension = session.dimension as u8;
        let authority_session = SessionContract::new(
            id,
            session.username.clone(),
            dimension,
            session.data.position,
            self.properties
                .operators
                .contains(&session.username.to_ascii_lowercase()),
            self.level.cheats_enabled,
        );
        self.authority
            .register_session_with_limit(authority_session, self.properties.max_players)
            .map_err(|reason| {
                io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!("session rejected: {reason:?}"),
                )
            })?;
        self.players.insert(id, session);
        let (chunks, entities) = self
            .players
            .get(&id)
            .map(|session| {
                (
                    session.interest.chunks.iter().copied().collect::<Vec<_>>(),
                    session
                        .interest
                        .simulation_entities
                        .iter()
                        .copied()
                        .collect::<Vec<_>>(),
                )
            })
            .unwrap_or_default();
        for chunk in chunks {
            self.queue_interest_update(
                current_dimension,
                self.authority.current_revision(),
                InterestKind::Chunk(chunk),
            );
        }
        for entity in entities {
            self.queue_interest_update(
                current_dimension,
                self.authority.current_revision(),
                InterestKind::Entity(entity),
            );
        }
        self.enqueue_host(HostToServer::SendWorldRules {
            rules: self.authority.world.rules,
            to: id,
        });
        self.enqueue_host(HostToServer::SendTimeSync {
            ticks: self.level.time,
            weather: 0,
            weather_remaining_ticks: 0.0,
            to: id,
        });
        let join_sequence = self.authority.world.revisions.allocate();
        self.enqueue_host(HostToServer::SendGameplayResponse {
            to: id,
            response: GameplayResponse {
                request_id: 0,
                server_sequence: join_sequence,
                outcome: GameplayOutcome::Accepted {
                    revision: self.authority.current_revision(),
                },
            },
        });
        self.metrics.players_online = self.players.len();
        self.metrics.queue_depth = self.metrics.queue_depth.saturating_add(3);
        eprintln!("[ServerRuntime] player joined id={id} dimension={dimension}");
        Ok(())
    }

    fn handle_leave(&mut self, id: u64) -> io::Result<()> {
        if let Some(session) = self.players.remove(&id) {
            self.save_player(&session)?;
        }
        self.authority.world.close_container_viewers(id);
        self.authority.remove_session(id);
        self.metrics.players_online = self.players.len();
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn handle_position(
        &mut self,
        id: u64,
        sequence: u32,
        sender_time_millis: u64,
        x: f32,
        y: f32,
        z: f32,
        yaw: f32,
        pitch: f32,
    ) -> io::Result<()> {
        let Some(session) = self.players.get_mut(&id) else {
            return Ok(());
        };
        if ![x, y, z, yaw, pitch].iter().all(|value| value.is_finite()) {
            return Ok(());
        }
        if x.abs() > WORLD_BOUND || z.abs() > WORLD_BOUND || y.abs() > WORLD_BOUND {
            return Ok(());
        }
        session.data.position = [x, y, z];
        session.data.yaw = yaw;
        session.data.pitch = pitch;
        let position = session.data.position;
        let dimension = session.dimension;
        let _ = session;
        if let Some(authority_session) = self.authority.session_mut(id) {
            authority_session.position = position;
            authority_session.dimension = dimension as u8;
        }
        self.update_interest_for(id, dimension, position);
        self.enqueue_host(HostToServer::BroadcastPlayerPosition {
            id,
            sequence,
            sender_time_millis,
            x,
            y,
            z,
            yaw,
            pitch,
        });
        self.metrics.queue_depth = self.metrics.queue_depth.saturating_add(1);
        Ok(())
    }

    fn handle_block_change(
        &mut self,
        id: u64,
        x: i32,
        y: i32,
        z: i32,
        block: u32,
        _state: u8,
    ) -> io::Result<()> {
        let Some(session) = self.players.get(&id) else {
            return Ok(());
        };
        if !within_reach(session, x, y, z) || !self.valid_coordinate(x, y, z) {
            return Ok(());
        }
        let dimension = session.dimension;
        let _ = session;
        let request = GameplayRequest {
            request_id: self.authority.current_revision() as u128 + 1,
            client_sequence: self
                .authority
                .session(id)
                .map(|authority_session| authority_session.last_client_sequence + 1)
                .unwrap_or(1),
            session_id: id,
            dimension: dimension as u8,
            client_revision: self.authority.current_revision(),
            operation: GameplayOperation::BlockUse { x, y, z, block },
        };
        let _response = self.handle_gameplay_request(request)?;
        Ok(())
    }

    fn handle_gameplay_request(
        &mut self,
        request: GameplayRequest,
    ) -> io::Result<GameplayResponse> {
        let request_id = request.request_id;
        let id = request.session_id;
        let duplicate = self
            .authority
            .session(id)
            .and_then(|session| session.cached_response(request_id))
            .is_some();
        let operation = request.operation.clone();
        let response = self.authority.submit_request(request.clone());
        if duplicate {
            self.metrics.duplicate_requests = self.metrics.duplicate_requests.saturating_add(1);
            return Ok(response);
        }
        match &response.outcome {
            GameplayOutcome::Accepted { revision } => {
                self.metrics.requests_accepted = self.metrics.requests_accepted.saturating_add(1);
                if let Some(session) = self.players.get_mut(&id) {
                    session.last_client_sequence = request.client_sequence;
                    session.dimension = self
                        .authority
                        .session(id)
                        .and_then(|authority_session| {
                            Dimension::from_wire(authority_session.dimension)
                        })
                        .unwrap_or(session.dimension);
                }
                match operation {
                    GameplayOperation::BlockUse { x, y, z, block } => {
                        let dimension = self
                            .authority
                            .session(id)
                            .and_then(|session| Dimension::from_wire(session.dimension))
                            .unwrap_or(self.authority.world.dimension);
                        let state = self.authority.world.get_block_state(x, y, z);
                        self.queue_block_change(dimension, *revision, x, y, z, block, state);
                        self.routed_mutations.insert(*revision);
                    }
                    GameplayOperation::Container {
                        x,
                        y,
                        z,
                        action,
                        slot,
                    } => {
                        let action = ContainerAction::from_wire(action)
                            .expect("authority accepted only a typed container action");
                        self.route_container_result(id, *revision, x, y, z, slot, action, None);
                        self.routed_mutations.insert(*revision);
                    }
                    GameplayOperation::ContainerClick {
                        x,
                        y,
                        z,
                        slot,
                        dragged,
                        is_left: _,
                    } => {
                        self.route_container_result(
                            id,
                            *revision,
                            x,
                            y,
                            z,
                            slot,
                            ContainerAction::Click,
                            dragged.as_ref(),
                        );
                        self.routed_mutations.insert(*revision);
                    }
                    _ => {}
                }
            }
            GameplayOutcome::Rejected { .. } => {
                self.metrics.requests_rejected = self.metrics.requests_rejected.saturating_add(1);
            }
        }
        Ok(response)
    }

    fn route_container_result(
        &mut self,
        id: u64,
        revision: u64,
        x: i32,
        y: i32,
        z: i32,
        slot: u16,
        action: ContainerAction,
        dragged: Option<&crate::network::protocol::ItemWire>,
    ) {
        let position = (x, y, z);
        if let Some(session) = self.players.get_mut(&id) {
            match action {
                ContainerAction::Close => {
                    session.interest.open_containers.remove(&position);
                    session.container_viewers.remove(&position);
                }
                ContainerAction::Open | ContainerAction::Click => {
                    session.interest.open_containers.insert(position);
                    session.container_viewers.insert(position);
                }
            }
        }
        let dimension = self
            .authority
            .session(id)
            .and_then(|session| Dimension::from_wire(session.dimension))
            .unwrap_or(self.authority.world.dimension);
        self.queue_interest_update(dimension, revision, InterestKind::BlockEntity(position));
        let container_targets =
            self.queue_interest_update(dimension, revision, InterestKind::Container(position));
        match action {
            ContainerAction::Open => {
                let slots = self
                    .authority
                    .world
                    .container_slots_wire(position)
                    .unwrap_or_default();
                self.enqueue_host(HostToServer::SendContainerOpenResult {
                    to: id,
                    dimension: dimension as u8,
                    success: true,
                    x,
                    y,
                    z,
                    slots,
                    revision,
                });
            }
            ContainerAction::Click => {
                let slot_value = self
                    .authority
                    .world
                    .container_slot_wire(position, slot)
                    .flatten();
                self.enqueue_host(HostToServer::SendContainerClickResult {
                    to: id,
                    dimension: dimension as u8,
                    success: true,
                    slot_index: slot,
                    slot: slot_value,
                    dragged: dragged.copied(),
                });
                for target in container_targets {
                    if target != id {
                        self.enqueue_host(HostToServer::SendContainerSlotUpdate {
                            to: target,
                            dimension: dimension as u8,
                            revision,
                            x,
                            y,
                            z,
                            slot_index: slot,
                            slot: slot_value,
                        });
                    }
                }
            }
            ContainerAction::Close => {}
        }
    }

    fn send_response(&mut self, to: u64, response: GameplayResponse) {
        if self.enqueue_host(HostToServer::SendGameplayResponse { to, response }) {
            self.metrics.outbound_packets = self.metrics.outbound_packets.saturating_add(1);
            self.metrics.queue_depth = self.metrics.queue_depth.saturating_add(1);
        }
    }

    fn enqueue_host(&mut self, event: HostToServer) -> bool {
        match self.host_tx.try_send(event) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => {
                self.metrics.queue_full = self.metrics.queue_full.saturating_add(1);
                false
            }
            Err(TrySendError::Disconnected(_)) => false,
        }
    }

    fn legacy_request(
        &self,
        id: u64,
        client_revision: u64,
        operation: GameplayOperation,
    ) -> Option<GameplayRequest> {
        let session = self.authority.session(id)?;
        Some(GameplayRequest {
            request_id: self.authority.current_revision() as u128 + 1,
            client_sequence: session.last_client_sequence.saturating_add(1).max(1),
            session_id: id,
            dimension: session.dimension,
            client_revision,
            operation,
        })
    }

    fn send_legacy_rejection(&mut self, to: u64, request_id: u128, reason: RejectReason) {
        let response = GameplayResponse {
            request_id,
            server_sequence: self.authority.world.revisions.allocate(),
            outcome: GameplayOutcome::Rejected { reason },
        };
        self.send_response(to, response);
    }

    pub fn drain_routed_updates(&mut self) -> Vec<RoutedInterestUpdate> {
        std::mem::take(&mut self.routed_updates)
    }

    fn queue_interest_update(
        &mut self,
        dimension: Dimension,
        revision: u64,
        kind: InterestKind,
    ) -> Vec<u64> {
        let mut targets: Vec<_> = self
            .players
            .values()
            .filter(|session| match kind {
                InterestKind::Container(position) => {
                    session.interest.wants_container(dimension, position)
                }
                _ => session.interest.wants(dimension, kind),
            })
            .map(|session| session.id)
            .collect();
        targets.sort_unstable();
        for target in &targets {
            if self.routed_updates.len() >= MAX_INTEREST_UPDATES_PER_TICK {
                break;
            }
            self.routed_updates.push(RoutedInterestUpdate {
                target: *target,
                dimension,
                revision,
                kind,
            });
        }
        targets
    }

    fn queue_block_change(
        &mut self,
        dimension: Dimension,
        revision: u64,
        x: i32,
        y: i32,
        z: i32,
        block: u32,
        state: u8,
    ) {
        let targets =
            self.queue_interest_update(dimension, revision, InterestKind::Block((x, y, z)));
        // The legacy network command is a broadcast. It is safe only when
        // every connected session is in the same dimension and has this block
        // in its view; otherwise the routed update remains queued for the
        // targeted transport adapter instead of leaking across dimensions.
        if !targets.is_empty() && targets.len() == self.players.len() {
            self.enqueue_host(HostToServer::BroadcastBlockChange {
                dimension: dimension as u8,
                revision,
                x,
                y,
                z,
                block,
                state,
            });
        }
    }

    /// Project every fixed-tick authority mutation through the authenticated
    /// interest sets.  This is the only runtime fanout path for automation,
    /// entity AI and block-entity/container deltas; presentation roots never
    /// replay these mutations locally.
    fn route_authority_snapshot(
        &mut self,
        snapshot: &crate::authority::contract::AuthoritySnapshot,
    ) {
        let dimension = self.authority.world.dimension;
        for mutation in &snapshot.mutations {
            if !self.routed_mutations.insert(mutation.revision) {
                continue;
            }
            let (x, y, z) = mutation.position;
            self.queue_block_change(
                dimension,
                mutation.revision,
                x,
                y,
                z,
                mutation.block,
                mutation.state,
            );

            let entity = self.authority.world.get_block_entity(x, y, z).cloned();
            let block_entity_targets = self.queue_interest_update(
                dimension,
                mutation.revision,
                InterestKind::BlockEntity(mutation.position),
            );
            for target in block_entity_targets {
                self.enqueue_host(HostToServer::SendBlockEntityDelta {
                    to: target,
                    dimension: dimension as u8,
                    revision: mutation.revision,
                    x,
                    y,
                    z,
                    entity: entity.clone(),
                });
            }

            // Container viewers receive concrete slot deltas, not merely an
            // opaque block-entity notification.  Sending the bounded slot
            // vector is deterministic for automation and avoids leaking a
            // private inventory to players who only have chunk interest.
            let container_targets = self.queue_interest_update(
                dimension,
                mutation.revision,
                InterestKind::Container(mutation.position),
            );
            if !container_targets.is_empty() {
                if let Some(slots) = self.authority.world.container_slots_wire(mutation.position) {
                    for (slot_index, slot) in slots.into_iter().enumerate() {
                        let slot_index = slot_index.min(u16::MAX as usize) as u16;
                        for target in &container_targets {
                            self.enqueue_host(HostToServer::SendContainerSlotUpdate {
                                to: *target,
                                dimension: dimension as u8,
                                revision: mutation.revision,
                                x,
                                y,
                                z,
                                slot_index,
                                slot,
                            });
                        }
                    }
                }
            }
        }
        while self.routed_mutations.len() > 2_048 {
            let Some(oldest) = self.routed_mutations.iter().next().copied() else {
                break;
            };
            self.routed_mutations.remove(&oldest);
        }

        // Entity AI runs inside AuthorityCore::tick.  Emit state only to
        // sessions whose simulation-distance set contains that entity.
        let mut entities: Vec<_> = self
            .authority
            .world
            .entities
            .entities
            .iter()
            .map(|entity| {
                let animation_state = u8::from(entity.on_ground)
                    | (u8::from(entity.target_player) << 1)
                    | (u8::from(entity.is_ignited) << 2)
                    | (u8::from(entity.fire_aspect_timer > 0.0) << 3);
                (
                    entity.id,
                    crate::network::protocol::EntityStateWire {
                        entity_id: entity.id,
                        entity_type: entity.entity_type.to_wire(),
                        position: entity.position.to_array(),
                        velocity: entity.velocity.to_array(),
                        yaw: entity.yaw,
                        pitch: entity.pitch,
                        health: entity.health,
                        animation_state,
                    },
                )
            })
            .collect();
        entities.sort_by_key(|(id, _)| *id);
        for (entity_id, state) in entities {
            let targets = self.queue_interest_update(
                dimension,
                snapshot.revision,
                InterestKind::Entity(entity_id),
            );
            for target in targets {
                self.enqueue_host(HostToServer::SendEntityState {
                    to: target,
                    dimension: dimension as u8,
                    sequence: snapshot.tick,
                    state,
                });
            }
        }
    }

    fn update_interest(&mut self, session: &mut PlayerSessionState) {
        session
            .interest
            .update_position(session.dimension, session.data.position);
        let center = Vec3::from_array(session.data.position);
        let radius = f32::from(session.interest.view_distance) * 16.0;
        let entities: Vec<_> = self
            .authority
            .world
            .entities
            .query_radius(center, radius)
            .map(|entity| entity.id)
            .collect();
        let simulation_entities: Vec<_> = self
            .authority
            .world
            .entities
            .query_radius(
                center,
                f32::from(session.interest.simulation_distance) * 16.0,
            )
            .map(|entity| entity.id)
            .collect();
        session.interest.update_entities(entities);
        session
            .interest
            .update_simulation_entities(simulation_entities);
        session.interest_chunks = session.interest.chunks.clone();
        session.simulation_chunks = session.interest.simulation_chunks.clone();
        session.entity_interest = session.interest.entities.clone();
        session.simulation_entity_interest = session.interest.simulation_entities.clone();
        session.container_viewers = session.interest.open_containers.clone();
    }

    fn update_interest_for(&mut self, id: u64, dimension: Dimension, position: [f32; 3]) {
        let entities: Vec<_> = self
            .authority
            .world
            .entities
            .query_radius(
                Vec3::from_array(position),
                f32::from(self.properties.view_distance) * 16.0,
            )
            .map(|entity| entity.id)
            .collect();
        let simulation_entities: Vec<_> = self
            .authority
            .world
            .entities
            .query_radius(
                Vec3::from_array(position),
                f32::from(self.properties.simulation_distance) * 16.0,
            )
            .map(|entity| entity.id)
            .collect();
        if let Some(session) = self.players.get_mut(&id) {
            session.dimension = dimension;
            session.interest.update_position(dimension, position);
            session.interest.update_entities(entities);
            session
                .interest
                .update_simulation_entities(simulation_entities);
            session.interest_chunks = session.interest.chunks.clone();
            session.simulation_chunks = session.interest.simulation_chunks.clone();
            session.entity_interest = session.interest.entities.clone();
            session.simulation_entity_interest = session.interest.simulation_entities.clone();
            session.container_viewers = session.interest.open_containers.clone();
        }
    }

    fn valid_coordinate(&self, x: i32, y: i32, z: i32) -> bool {
        self.authority.world.valid_coordinate(x, y, z)
    }

    fn ensure_spawn_chunk(&mut self) {
        self.authority.world.ensure_chunk(
            self.level.spawn_x.div_euclid(16),
            self.level.spawn_z.div_euclid(16),
        );
    }

    fn save_player(&self, session: &PlayerSessionState) -> io::Result<()> {
        self.save_manager.save_dedicated_player(
            &session.username,
            session.dimension,
            &session.data,
            &session.effects,
        )
    }
}

fn gamemode_wire(level: &LevelData) -> u8 {
    let _ = level;
    0
}

fn default_player_data() -> PlayerData {
    let state = crate::player::PlayerState::new();
    let inventory = Inventory::new();
    PlayerData::from_state(
        Vec3::new(8.0, 80.0, 8.0),
        Vec3::ZERO,
        0.0,
        0.0,
        &state,
        GameMode::Survival,
        &inventory,
        Default::default(),
    )
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
        if !self.stopped {
            let _ = self.shutdown();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::protocol::RejectReason;

    fn temp_dir(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!("icraft_plan16_{label}_{unique}"))
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
        let first = runtime
            .submit_request(
                1,
                GameplayRequest {
                    request_id: 1,
                    client_sequence: 1,
                    session_id: 1,
                    dimension: 0,
                    client_revision: 0,
                    operation: GameplayOperation::BlockUse {
                        x: 8,
                        y: 80,
                        z: 8,
                        block: 1,
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
                    operation: GameplayOperation::BlockUse {
                        x: 8,
                        y: 80,
                        z: 8,
                        block: 2,
                    },
                },
            )
            .unwrap();
        assert!(matches!(first.outcome, GameplayOutcome::Accepted { .. }));
        assert!(matches!(second.outcome, GameplayOutcome::Accepted { .. }));
        assert!(second.server_sequence > first.server_sequence);
        assert_eq!(runtime.authority.world.get_block(8, 80, 8).to_wire(), 2);
        let _ = runtime.shutdown();
        let _ = fs::remove_dir_all(&runtime.world_dir);
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
        assert!(runtime.save_all().is_err());
        runtime.world_dir = world_dir.clone();
        runtime.save_manager.world_dir = world_dir.clone();
        assert!(runtime.save_all().is_ok());
        let _ = runtime.shutdown();
        let _ = fs::remove_dir_all(&world_dir);
        let _ = fs::remove_file(blocking_file);
    }
}
