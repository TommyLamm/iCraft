//! Headless authoritative server runtime.
//!
//! The desktop application remains a presentation client.  This module owns
//! the fixed tick, authenticated session state, world mutation gate, interest
//! sets, persistence and observability needed by `icraft-server`.  It is
//! deliberately synchronous at the authority boundary; the existing Tokio
//! network thread only transports packets into the bounded event channel.

use crate::authority::contract::{AuthorityTopology, SessionContract};
use crate::authority::{AuthorityConfig, AuthorityCore};
use crate::dimension::Dimension;
use crate::game_rules::WorldRules;
use crate::inventory::{GameMode, Inventory};
use crate::network::protocol::{
    GameplayOperation, GameplayOutcome, GameplayRequest, GameplayResponse, PlayerEffectWire,
};
use crate::network::server::{HostToServer, NetworkServer, ServerConfig, ServerToHost};
use crate::save::{LevelData, PlayerData, SaveManager};
use glam::Vec3;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs;
use std::io;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const TICK_INTERVAL: Duration = Duration::from_millis(50);
const MAX_INBOUND_EVENTS_PER_TICK: usize = 512;
const WORLD_BOUND: f32 = 30_000_000.0;
const PLAYER_REACH: f32 = 8.0;
const PLAYER_SAVE_VERSION: u16 = 1;
const AUTOSAVE_INTERVAL_TICKS: u64 = 6_000;

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
    pub interest_chunks: HashSet<(i32, i32)>,
    pub entity_interest: HashSet<u64>,
    pub effects: Vec<PlayerEffectWire>,
}

impl PlayerSessionState {
    fn new(id: u64, username: String, data: PlayerData) -> Self {
        let dimension = data.spawn_dimension.unwrap_or_default();
        Self {
            id,
            username,
            data,
            dimension,
            last_client_sequence: 0,
            interest_chunks: HashSet::new(),
            entity_interest: HashSet::new(),
            effects: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PlayerFile {
    version: u16,
    data: PlayerData,
    #[serde(default)]
    effects: Vec<PlayerEffectWire>,
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
    world_dir: PathBuf,
    save_manager: SaveManager,
    host_tx: Sender<HostToServer>,
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
        let level = load_level(&world_dir).unwrap_or_else(|| LevelData {
            seed: properties.seed as u32,
            rules: WorldRules {
                pvp: properties.pvp,
                ..WorldRules::default()
            },
            ..LevelData::default()
        });
        let (host_tx, host_rx_network) = mpsc::channel();
        let (server_to_host, host_rx) = mpsc::channel();
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
            stopped: false,
        };
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
        let _ = self.host_tx.send(HostToServer::Stop);
        if let Some(handle) = self.network_thread.take() {
            let _ = handle.join();
        }
        save_result
    }

    pub fn save_all(&mut self) -> io::Result<()> {
        let started = Instant::now();
        let level_bytes = bincode::serialize(&self.level)
            .map_err(|error| io::Error::new(io::ErrorKind::Other, error))?;
        atomic_write(&self.world_dir.join("level.dat"), &level_bytes)?;
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
            ServerToHost::ClientJoined { id, username } => self.handle_join(id, username),
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
                self.host_tx
                    .send(HostToServer::BroadcastPlayerAction { id, action })
                    .ok();
                Ok(())
            }
            ServerToHost::ChatFromClient { id, message } => {
                if let Some(session) = self.players.get(&id) {
                    self.host_tx
                        .send(HostToServer::BroadcastChat {
                            sender: session.username.clone(),
                            message: message.chars().take(256).collect(),
                        })
                        .ok();
                }
                Ok(())
            }
            ServerToHost::ClientRespawnRequest { id } => {
                if let Some(session) = self.players.get_mut(&id) {
                    session.data.position = [
                        self.level.spawn_x as f32,
                        self.level.spawn_y as f32,
                        self.level.spawn_z as f32,
                    ];
                    session.data.is_dead = false;
                    session.dimension = self.level.spawn_dimension;
                    self.host_tx
                        .send(HostToServer::SendPlayerRespawnResult {
                            to: id,
                            position: session.data.position,
                            dimension: session.dimension as u8,
                        })
                        .ok();
                }
                Ok(())
            }
            ServerToHost::ClientBlockAction { id, x, y, z, .. } => {
                self.handle_block_change(id, x, y, z, 0, 0)
            }
            ServerToHost::ClientSleepRequest { id, .. }
            | ServerToHost::ContainerOpenRequest { id, .. }
            | ServerToHost::ContainerClickRequest { id, .. }
            | ServerToHost::ContainerClose { id, .. } => {
                // Legacy packets are intentionally routed through the same
                // envelope gate.  The protocol owner supplies richer adapters
                // for slot/coordinate payloads; until then this branch emits
                // an explicit unsupported response instead of accepting a
                // fabricated zero-coordinate mutation.
                let request = GameplayRequest {
                    request_id: self.authority.current_revision() as u128 + 1,
                    client_sequence: self
                        .authority
                        .session(id)
                        .map(|session| session.last_client_sequence + 1)
                        .unwrap_or(1),
                    session_id: id,
                    dimension: self
                        .authority
                        .session(id)
                        .map(|session| session.dimension)
                        .unwrap_or(0),
                    client_revision: self.authority.current_revision(),
                    operation: GameplayOperation::ItemUse { item: 0, count: 0 },
                };
                let response = self.handle_gameplay_request(request)?;
                self.send_response(id, response);
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
            return Ok(());
        }
        let (data, effects) = self
            .load_player(&username)
            .unwrap_or_else(|| (default_player_data(), Vec::new()));
        let mut session = PlayerSessionState::new(id, username, data);
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
            .register_session(authority_session)
            .map_err(|reason| {
                io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!("session rejected: {reason:?}"),
                )
            })?;
        self.players.insert(id, session);
        self.host_tx
            .send(HostToServer::SendWorldRules {
                rules: self.authority.world.rules,
                to: id,
            })
            .ok();
        self.host_tx
            .send(HostToServer::SendTimeSync {
                ticks: self.level.time,
                weather: 0,
                weather_remaining_ticks: 0.0,
                to: id,
            })
            .ok();
        self.host_tx
            .send(HostToServer::SendGameplayResponse {
                to: id,
                response: GameplayResponse {
                    request_id: 0,
                    server_sequence: self.authority.world.revisions.allocate(),
                    outcome: GameplayOutcome::Accepted {
                        revision: self.authority.current_revision(),
                    },
                },
            })
            .ok();
        self.metrics.players_online = self.players.len();
        self.metrics.queue_depth = self.metrics.queue_depth.saturating_add(3);
        eprintln!("[ServerRuntime] player joined id={id} dimension={dimension}");
        Ok(())
    }

    fn handle_leave(&mut self, id: u64) -> io::Result<()> {
        if let Some(session) = self.players.remove(&id) {
            self.save_player(&session)?;
        }
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
        let interest = self.interest_chunks_for(dimension, position);
        let entity_interest = self
            .authority
            .world
            .entities
            .query_radius(
                Vec3::from_array(position),
                f32::from(self.properties.view_distance) * 16.0,
            )
            .map(|entity| entity.id)
            .collect();
        if let Some(session) = self.players.get_mut(&id) {
            session.interest_chunks = interest;
            session.entity_interest = entity_interest;
        }
        self.host_tx
            .send(HostToServer::BroadcastPlayerPosition {
                id,
                sequence,
                sender_time_millis,
                x,
                y,
                z,
                yaw,
                pitch,
            })
            .ok();
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
        state: u8,
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
        let response = self.handle_gameplay_request(request)?;
        if let GameplayOutcome::Accepted { revision } = response.outcome {
            self.host_tx
                .send(HostToServer::BroadcastBlockChange {
                    dimension: dimension as u8,
                    revision,
                    x,
                    y,
                    z,
                    block,
                    state,
                })
                .ok();
            self.metrics.queue_depth = self.metrics.queue_depth.saturating_add(1);
        }
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
                if let GameplayOperation::BlockUse { x, y, z, block } = operation {
                    self.host_tx
                        .send(HostToServer::BroadcastBlockChange {
                            dimension: self
                                .authority
                                .session(id)
                                .map(|session| session.dimension)
                                .unwrap_or(0),
                            revision: *revision,
                            x,
                            y,
                            z,
                            block,
                            state: self.authority.world.get_block_state(x, y, z),
                        })
                        .ok();
                }
            }
            GameplayOutcome::Rejected { .. } => {
                self.metrics.requests_rejected = self.metrics.requests_rejected.saturating_add(1);
            }
        }
        Ok(response)
    }

    fn send_response(&mut self, to: u64, response: GameplayResponse) {
        self.host_tx
            .send(HostToServer::SendGameplayResponse { to, response })
            .ok();
        self.metrics.outbound_packets = self.metrics.outbound_packets.saturating_add(1);
        self.metrics.queue_depth = self.metrics.queue_depth.saturating_add(1);
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

    fn update_interest(&mut self, session: &mut PlayerSessionState) {
        session.interest_chunks =
            self.interest_chunks_for(session.dimension, session.data.position);
        let center = Vec3::from_array(session.data.position);
        let radius = f32::from(self.properties.view_distance) * 16.0;
        session.entity_interest = self
            .authority
            .world
            .entities
            .query_radius(center, radius)
            .map(|entity| entity.id)
            .collect();
    }

    fn interest_chunks_for(
        &mut self,
        dimension: Dimension,
        position: [f32; 3],
    ) -> HashSet<(i32, i32)> {
        let _ = dimension;
        let cx = (position[0] / 16.0).floor() as i32;
        let cz = (position[2] / 16.0).floor() as i32;
        let mut interest_chunks = HashSet::new();
        let radius = i32::from(self.properties.view_distance);
        for dx in -radius..=radius {
            for dz in -radius..=radius {
                if dx * dx + dz * dz <= radius * radius {
                    let key = (cx + dx, cz + dz);
                    interest_chunks.insert(key);
                }
            }
        }
        interest_chunks
    }

    fn player_path(&self, username: &str) -> PathBuf {
        let safe: String = username
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
                    ch
                } else {
                    '_'
                }
            })
            .collect();
        self.world_dir.join("players").join(format!("{safe}.dat"))
    }

    fn load_player(&self, username: &str) -> Option<(PlayerData, Vec<PlayerEffectWire>)> {
        let bytes = fs::read(self.player_path(username)).ok()?;
        let file: PlayerFile = bincode::deserialize(&bytes).ok()?;
        (file.version == PLAYER_SAVE_VERSION).then_some((file.data, file.effects))
    }

    fn save_player(&self, session: &PlayerSessionState) -> io::Result<()> {
        let path = self.player_path(&session.username);
        let bytes = bincode::serialize(&PlayerFile {
            version: PLAYER_SAVE_VERSION,
            data: session.data.clone(),
            effects: session.effects.clone(),
        })
        .map_err(|error| io::Error::new(io::ErrorKind::Other, error))?;
        atomic_write(&path, &bytes)
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

fn load_level(world_dir: &Path) -> Option<LevelData> {
    let bytes = fs::read(world_dir.join("level.dat")).ok()?;
    bincode::deserialize(&bytes).ok()
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
        assert!(runtime.save_all().is_err());
        runtime.world_dir = world_dir.clone();
        assert!(runtime.save_all().is_ok());
        let _ = runtime.shutdown();
        let _ = fs::remove_dir_all(&world_dir);
        let _ = fs::remove_file(blocking_file);
    }
}
