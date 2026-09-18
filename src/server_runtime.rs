//! Headless authoritative server runtime.
//!
//! The desktop application remains a presentation client.  This module owns
//! the fixed tick, authenticated session state, world mutation gate, interest
//! sets, persistence and observability needed by `icraft-server`.  It is
//! deliberately synchronous at the authority boundary; the existing Tokio
//! network thread only transports packets into the bounded event channel.

use crate::authority::contract::{
    AuthoritySnapshot, SessionContract, SessionGameplayState, SessionInventorySlot,
    SESSION_INVENTORY_SLOTS,
};
use crate::authority::interest::{
    capped_spawn_residency, residency_hysteresis_chunks, union_simulation_chunks, ChunkCoord,
    InterestKind, InterestSet, RoutedInterestUpdate,
};
use crate::authority::{AuthorityConfig, AuthorityCore};
use crate::dimension::Dimension;
use crate::game_rules::{Difficulty, WorldRules};
use crate::inventory::{GameMode, Inventory};
use crate::network::protocol::{
    ContainerAction, EntityStateWire, GameplayOperation, GameplayOutcome, GameplayRequest,
    GameplayResponse, ItemWire, PlayerEffectWire, RejectReason, SessionGameplayWire,
    PROTOCOL_VERSION,
};
use crate::network::server::{
    HostToServer, MeteredHostEventSender, NetworkMetrics, NetworkServer, ServerConfig, ServerToHost,
};
use crate::save::{
    normalize_player_identity, EntitySaveData, LevelData, MutationRevisionIndex, PlayerData,
    SaveManager,
};
use crate::world::chunk_xz;
use glam::Vec3;
#[cfg(test)]
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::fmt;
use std::fs;
use std::io;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

mod ingress;
pub mod projection;
mod save_worker;
mod session_sync;
mod worldgen_worker;

pub(super) const TICK_INTERVAL: Duration = Duration::from_millis(50);
pub(super) const MAX_INBOUND_EVENTS_PER_TICK: usize = 512;
pub(super) const WORLD_BOUND: f32 = 30_000_000.0;
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
pub(crate) const MAX_INITIAL_CHUNK_PROJECTIONS_PER_TICK: usize = 16;
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

mod events;
mod properties;
mod session_state;

pub use events::*;
pub use properties::*;
pub use session_state::PlayerSessionState;

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
    pub autosave_failures: u64,
    pub evict_flush_failures: u64,
    pub tick_over_budget: u64,
    pub save_queue_full: u64,
    pub restore_chunk_skipped: u64,
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
    /// Reverse interest map: which sessions currently want each column.
    /// Updated from chunk enter/depart (and join/leave); mutation fanout
    /// looks up targets here instead of scanning every player.
    pub(super) chunk_interest_index: HashMap<(Dimension, ChunkCoord), BTreeSet<u64>>,
    pub(super) world_dir: PathBuf,
    pub(super) save_manager: SaveManager,
    pub(super) default_game_mode: GameMode,
    pub(super) host_tx: Option<tokio::sync::mpsc::Sender<HostToServer>>,
    pub(super) host_rx: Receiver<ServerToHost>,
    pub(super) network_thread: Option<JoinHandle<()>>,
    pub(super) network_metrics: NetworkMetrics,
    pub(super) transport_mode: TransportMode,
    pub(super) local_session_id: Option<u64>,
    pub(super) presentation_events: VecDeque<PresentationEvent>,
    pub(super) observed_transport_rejections: u64,
    pub(super) observed_transport_duplicates: u64,
    pub(super) stopped: bool,
    /// Successful `save_all` during shutdown. `request_shutdown` only sets
    /// `stopped`; a later `shutdown` must still flush if this is false.
    pub(super) save_flushed: bool,
    worldgen_worker: worldgen_worker::WorldgenWorker,
    save_worker: Option<save_worker::SaveWorker>,
    /// Cached per-dimension simulation union from interest HashSets.
    simulation_union_cache: BTreeMap<Dimension, CachedSimulationUnion>,
    /// Cached residency keep-sets; short-circuit eviction when covered.
    residency_keep_cache: BTreeMap<Dimension, CachedResidencyKeep>,
}

struct CachedSimulationUnion {
    fingerprint: Vec<(u64, Option<crate::authority::interest::InterestChunkAnchor>)>,
    chunks: BTreeSet<(i32, i32)>,
}

struct CachedResidencyKeep {
    fingerprint: Vec<(u64, Option<crate::authority::interest::InterestChunkAnchor>, u8)>,
    keep: BTreeSet<(i32, i32)>,
    /// `WorldColumns::load_generation` when every resident was inside `keep`.
    covered_generation: Option<u64>,
}

impl ServerRuntime {
    pub fn new(properties: ServerProperties) -> Result<Self, ServerConfigError> {
        let (runtime, _input) = Self::construct(
            properties,
            EmbeddedRuntimeOptions {
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
        let mut authority = AuthorityCore::new(AuthorityConfig {
            seed: level.seed,
            dimension: level.spawn_dimension,
            world_type: level.world_type,
            generate_structures: level.generate_structures,
            rules: level.rules,
            difficulty,
            simulation_distance: properties.simulation_distance as i32,
        });
        authority
            .world_mut_expect(level.spawn_dimension)
            .time = level.time;
        let mut runtime = Self {
            properties,
            level,
            metrics: ServerMetrics::default(),
            players: HashMap::new(),
            authority,
            world_dir: world_dir.clone(),
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
            chunk_interest_index: HashMap::new(),
            stopped: false,
            save_flushed: false,
            worldgen_worker: worldgen_worker::WorldgenWorker::new(),
            save_worker: Some(save_worker::SaveWorker::spawn(SaveManager::new(&world_dir))),
            simulation_union_cache: BTreeMap::new(),
            residency_keep_cache: BTreeMap::new(),
        };
        runtime.restore_authority_state()?;
        runtime.ensure_spawn_chunk();
        // After spawn materialization, offload further ensure_chunk calls.
        runtime
            .authority
            .set_worldgen_mode_all(crate::server_world::WorldgenMode::Async);
        if let Some(profile) = options.local_session {
            runtime.handle_join_with_storage_and_override(
                profile.id,
                profile.username,
                profile.storage,
                profile.view_distance_override,
            )?;
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
        self.drain_save_acks();
        self.schedule_pending_worldgen();
        self.collect_worldgen_results();
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
        let simulation_unions = self.cached_simulation_unions();
        let snapshot = self.authority.tick_with_simulation_unions(&simulation_unions);
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
        // Tick walk already counted residents; refresh after eviction so the
        // published counters match the post-evict map without a second
        // `dimensions()` Vec allocation.
        let (loaded_chunks, entities) = self.authority.resident_metrics();
        self.metrics.loaded_chunks = loaded_chunks;
        self.metrics.entities = entities;
        if self.metrics.ticks % AUTOSAVE_INTERVAL_TICKS == 0 {
            if let Err(_error) = self.save_all_async() {
                self.metrics.autosave_failures = self.metrics.autosave_failures.saturating_add(1);
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
            // Over-budget is recorded only in timing counters — no stderr I/O
            // on the tick thread when already late.
            self.metrics.tick_over_budget = self.metrics.tick_over_budget.saturating_add(1);
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
        if let Some(worker) = self.save_worker.take() {
            worker.shutdown();
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
                && dimension != self.level.spawn_dimension
            {
                continue;
            }
            self.authority.with_world(dimension, |world| {
                for chunk in &chunks {
                    if let Err(_error) = world.restore_saved_chunk(chunk) {
                        // Count and skip; failed_restore_chunks stays fail-closed.
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
        // World-level dimension.dat follows the local / first session contract,
        // not an ambient active-world pointer.
        let persisted_dimension = self
            .local_session_id
            .and_then(|id| self.authority.session(id))
            .or_else(|| self.authority.sessions.values().next())
            .and_then(|session| Dimension::from_wire(session.dimension))
            .unwrap_or(self.level.spawn_dimension);
        let mut merged_revisions = MutationRevisionIndex::default();
        let dimensions: Vec<_> = self.authority.dimensions().collect();
        for dimension in dimensions {
            let (chunks, entities_payload, revisions, entities_epoch) =
                self.authority.with_world(dimension, |world| {
                    let mut dirty = world.chunks.dirty_chunks.dirty_revisions();
                    dirty.sort_unstable_by_key(|(coord, _)| *coord);
                    let mut payloads = Vec::new();
                    for ((cx, cz), revision) in dirty {
                        let Some(data) = world.chunk_save_payload(cx, cz) else {
                            continue;
                        };
                        if world.chunks.dirty_chunks.begin_save(cx, cz, revision) {
                            payloads.push((cx, cz, revision, data));
                        }
                    }
                    let entities_dirty = world.entities_dirty_for_save();
                    let entities_epoch = world.entities.checksum_epoch();
                    let entities = if entities_dirty {
                        Some(
                            world
                                .entities
                                .entities
                                .iter()
                                .map(EntitySaveData::from)
                                .collect::<Vec<_>>(),
                        )
                    } else {
                        None
                    };
                    (
                        payloads,
                        entities,
                        world.mutation_revision_index(),
                        entities_epoch,
                    )
                });
            if !chunks.is_empty() {
                let rollback: Vec<(i32, i32, u64)> = chunks
                    .iter()
                    .map(|(cx, cz, revision, _)| (*cx, *cz, *revision))
                    .collect();
                if !self.enqueue_save_payload(save_worker::SavePayload::Chunks {
                    dimension,
                    entries: chunks,
                }) {
                    self.authority.with_world(dimension, |world| {
                        for (cx, cz, revision) in rollback {
                            world
                                .chunks
                                .dirty_chunks
                                .acknowledge_failed(cx, cz, revision);
                        }
                    });
                    return Err(io::Error::new(
                        io::ErrorKind::WouldBlock,
                        "save queue full while enqueueing chunks",
                    ));
                }
            }
            if let Some(entities) = entities_payload {
                let path = self.save_manager.entities_file_path(dimension);
                let bytes = bincode::serialize(&entities)
                    .map_err(|error| io::Error::new(io::ErrorKind::Other, error))?;
                if !self.enqueue_save_payload(save_worker::SavePayload::Entities {
                    dimension,
                    epoch: entities_epoch,
                    bytes,
                    path,
                }) {
                    return Err(io::Error::new(
                        io::ErrorKind::WouldBlock,
                        "save queue full while enqueueing entities",
                    ));
                }
            }
            for ((cx, cz), revision) in revisions.entries_in(dimension) {
                merged_revisions
                    .ensure_at_least(dimension, cx, cz, revision)
                    .map_err(|error| io::Error::new(io::ErrorKind::Other, error.to_string()))?;
            }
        }
        let revision_bytes = bincode::serialize(&merged_revisions)
            .map_err(|error| io::Error::new(io::ErrorKind::Other, error))?;
        let level_bytes = bincode::serialize(&self.level)
            .map_err(|error| io::Error::new(io::ErrorKind::Other, error))?;
        let mut whitelist: Vec<_> = self.properties.whitelist.iter().cloned().collect();
        whitelist.sort();
        let properties_text = format!(
            "# online-mode=false is LAN/offline: names are accounts until real credentials exist.\n\
             # Operators are granted only by the dedicated-server console `op` command (or this file),\n\
             # bound to the normalized identity of later connections. There is still no password.\n\
             bind={}\nport={}\nmotd={}\nmax-players={}\ndifficulty={}\nonline-mode={}\nwhitelist={}\noperators={}\nview-distance={}\nsimulation-distance={}\npvp={}\nlevel-name={}\nlevel-seed={}\n",
            self.properties.bind,
            self.properties.port,
            self.properties.motd,
            self.properties.max_players,
            self.properties.difficulty,
            self.properties.online_mode,
            whitelist.join(","),
            sorted_names(&self.properties.operators).join(","),
            self.properties.view_distance,
            self.properties.simulation_distance,
            self.properties.pvp,
            self.properties.world_dir.display(),
            self.properties.seed as i64,
        );
        let sidecars = vec![
            (
                self.world_dir.join("mutation_revisions.bin"),
                revision_bytes,
            ),
            (
                self.world_dir.join("dimension.dat"),
                vec![persisted_dimension as u8],
            ),
            (self.world_dir.join("level.dat"), level_bytes),
            (
                self.world_dir.join("server.properties"),
                properties_text.into_bytes(),
            ),
        ];
        if !self.enqueue_save_payload(save_worker::SavePayload::SidecarGroup {
            entries: sidecars,
        }) {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "save queue full while enqueueing sidecars",
            ));
        }
        Ok(())
    }

    pub fn save_all(&mut self) -> io::Result<()> {
        self.save_all_inner(true)
    }

    fn save_all_async(&mut self) -> io::Result<()> {
        self.save_all_inner(false)
    }

    fn save_all_inner(&mut self, wait_for_drain: bool) -> io::Result<()> {
        #[cfg(test)]
        if SAVE_ALL_FAILPOINT.with(|failpoint| failpoint.get()) {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "injected save_all failure",
            ));
        }
        let started = Instant::now();
        self.save_authority_state()?;
        let mut player_ids: Vec<_> = self.players.keys().copied().collect();
        player_ids.sort_unstable();
        for id in player_ids.iter().copied() {
            self.sync_gameplay_projection(id);
        }
        for id in player_ids {
            let Some(session) = self.players.get(&id) else {
                continue;
            };
            if !session.player_dirty {
                continue;
            }
            self.save_player(id, session)?;
            if let Some(session) = self.players.get_mut(&id) {
                session.player_dirty = false;
            }
        }
        if wait_for_drain {
            if let Some(worker) = self.save_worker.as_mut() {
                let job_id = worker.next_job_id();
                worker
                    .enqueue_blocking(save_worker::SavePayload::Barrier { job_id })
                    .map_err(|error| io::Error::new(io::ErrorKind::Other, error.to_string()))?;
                let acks = worker.wait_barrier(job_id);
                let failed = acks.iter().any(|ack| {
                    matches!(
                        ack,
                        save_worker::SaveAck::Chunks { ok: false, .. }
                            | save_worker::SaveAck::Entities { ok: false, .. }
                            | save_worker::SaveAck::SidecarGroup { ok: false }
                    )
                });
                self.apply_save_acks(acks);
                if failed {
                    return Err(io::Error::new(
                        io::ErrorKind::Other,
                        "save worker reported a failed write",
                    ));
                }
            }
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
        let dimension = self.level.spawn_dimension;
        let (cx, cz) = chunk_xz(self.level.spawn_x, self.level.spawn_z);
        self.authority.with_world(dimension, |world| {
            world.materialize_chunk(cx, cz);
        });
    }

    fn schedule_pending_worldgen(&mut self) {
        let dimensions: Vec<_> = self.authority.dimensions().collect();
        let mut jobs = Vec::new();
        for dimension in dimensions {
            let Some(world) = self.authority.world_ref(dimension) else {
                continue;
            };
            for &(cx, cz) in world.pending_chunk_generation() {
                if self.authority.is_worldgen_pending(dimension, cx, cz) {
                    continue;
                }
                jobs.push(worldgen_worker::WorldgenJob {
                    dimension,
                    chunk_x: cx,
                    chunk_z: cz,
                    seed: world.seed,
                    world_type: world.world_type,
                    generate_structures: world.generate_structures,
                });
            }
        }
        for job in jobs {
            let _ = self.worldgen_worker.schedule(job);
        }
    }

    fn collect_worldgen_results(&mut self) {
        let completed = self.worldgen_worker.poll_completed();
        let mut columns = Vec::new();
        for result in completed {
            columns.push(crate::authority::PendingWorldgenColumn {
                dimension: result.dimension,
                chunk_x: result.chunk_x,
                chunk_z: result.chunk_z,
                chunk: result.chunk,
            });
        }
        if !columns.is_empty() {
            self.authority.queue_worldgen_results(columns);
        }
    }

    fn drain_save_acks(&mut self) {
        let Some(worker) = self.save_worker.as_ref() else {
            return;
        };
        let acks = worker.poll_acks();
        self.apply_save_acks(acks);
    }

    fn apply_save_acks(&mut self, acks: Vec<save_worker::SaveAck>) {
        let mut evict_failures = 0u64;
        let mut autosave_failures = 0u64;
        for ack in acks {
            match ack {
                save_worker::SaveAck::Chunks {
                    dimension,
                    revisions,
                    ok,
                } => {
                    self.authority.with_world(dimension, |world| {
                        for (cx, cz, revision) in revisions {
                            if ok {
                                world
                                    .chunks
                                    .dirty_chunks
                                    .acknowledge_persisted(cx, cz, revision);
                            } else {
                                world
                                    .chunks
                                    .dirty_chunks
                                    .acknowledge_failed(cx, cz, revision);
                                evict_failures = evict_failures.saturating_add(1);
                            }
                        }
                    });
                }
                save_worker::SaveAck::Entities {
                    dimension,
                    epoch,
                    ok,
                } => {
                    if ok {
                        self.authority.with_world(dimension, |world| {
                            if world.entities.checksum_epoch() == epoch {
                                world.acknowledge_entities_persisted();
                            }
                        });
                    } else {
                        autosave_failures = autosave_failures.saturating_add(1);
                    }
                }
                save_worker::SaveAck::SidecarGroup { ok } => {
                    if !ok {
                        autosave_failures = autosave_failures.saturating_add(1);
                    }
                }
                save_worker::SaveAck::Barrier { .. } => {}
            }
        }
        self.metrics.evict_flush_failures = self
            .metrics
            .evict_flush_failures
            .saturating_add(evict_failures);
        self.metrics.autosave_failures = self
            .metrics
            .autosave_failures
            .saturating_add(autosave_failures);
    }

    fn enqueue_save_payload(&mut self, payload: save_worker::SavePayload) -> bool {
        let Some(worker) = self.save_worker.as_ref() else {
            return false;
        };
        match worker.try_enqueue(payload) {
            Ok(()) => true,
            Err(std::sync::mpsc::TrySendError::Full(_))
            | Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                self.metrics.save_queue_full = self.metrics.save_queue_full.saturating_add(1);
                false
            }
        }
    }

    pub fn valid_coordinate(&self, dimension: Dimension, x: i32, y: i32, z: i32) -> bool {
        self.authority
            .world_ref(dimension)
            .is_some_and(|world| world.valid_coordinate(x, y, z))
    }

    fn simulation_union_fingerprint(
        &self,
        dimension: Dimension,
    ) -> Vec<(u64, Option<crate::authority::interest::InterestChunkAnchor>)> {
        let mut fingerprint: Vec<_> = self
            .players
            .iter()
            .filter(|(_, session)| session.interest.dimension == dimension)
            .map(|(id, session)| (*id, session.interest.current_chunk_anchor()))
            .collect();
        fingerprint.sort_unstable();
        fingerprint
    }

    fn cached_simulation_unions(&mut self) -> BTreeMap<Dimension, BTreeSet<(i32, i32)>> {
        let dimensions: Vec<_> = self.authority.dimensions().collect();
        let mut unions = BTreeMap::new();
        for dimension in dimensions {
            let fingerprint = self.simulation_union_fingerprint(dimension);
            let reuse = self
                .simulation_union_cache
                .get(&dimension)
                .is_some_and(|cached| cached.fingerprint == fingerprint);
            if !reuse {
                let sets: Vec<&InterestSet> = self
                    .players
                    .values()
                    .filter(|session| session.interest.dimension == dimension)
                    .map(|session| &session.interest)
                    .collect();
                let chunks = union_simulation_chunks(sets);
                self.simulation_union_cache.insert(
                    dimension,
                    CachedSimulationUnion {
                        fingerprint,
                        chunks: chunks.clone(),
                    },
                );
                unions.insert(dimension, chunks);
            } else if let Some(cached) = self.simulation_union_cache.get(&dimension) {
                unions.insert(dimension, cached.chunks.clone());
            }
        }
        unions
    }

    fn residency_keep_fingerprint(
        &self,
        dimension: Dimension,
    ) -> Vec<(u64, Option<crate::authority::interest::InterestChunkAnchor>, u8)> {
        let mut fingerprint: Vec<_> = self
            .players
            .iter()
            .filter(|(_, session)| session.interest.dimension == dimension)
            .map(|(id, session)| {
                (
                    *id,
                    session.interest.current_chunk_anchor(),
                    session.interest.view_distance,
                )
            })
            .collect();
        fingerprint.sort_unstable();
        fingerprint
    }

    fn residency_keep_set(&mut self, dimension: Dimension) -> (BTreeSet<(i32, i32)>, bool) {
        let fingerprint = self.residency_keep_fingerprint(dimension);
        let load_generation = self
            .authority
            .world_ref(dimension)
            .map(|world| world.chunks.load_generation())
            .unwrap_or(0);
        if let Some(cached) = self.residency_keep_cache.get(&dimension) {
            if cached.fingerprint == fingerprint {
                let covered = cached
                    .covered_generation
                    .is_some_and(|generation| generation == load_generation);
                return (cached.keep.clone(), covered);
            }
        }

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
                session.last_pose_position,
                session.interest.view_distance,
            ));
        }
        if !any_session {
            keep.extend(capped_spawn_residency(
                self.level.spawn_x,
                self.level.spawn_z,
            ));
        }
        self.residency_keep_cache.insert(
            dimension,
            CachedResidencyKeep {
                fingerprint,
                keep: keep.clone(),
                covered_generation: None,
            },
        );
        (keep, false)
    }

    fn mark_residency_covered(&mut self, dimension: Dimension, generation: u64) {
        if let Some(cached) = self.residency_keep_cache.get_mut(&dimension) {
            cached.covered_generation = Some(generation);
        }
    }

    fn evict_uninteresting_chunks(&mut self) {
        let dimensions: Vec<_> = self.authority.dimensions().collect();
        for dimension in dimensions {
            let (keep, fully_covered) = self.residency_keep_set(dimension);
            self.authority.with_world(dimension, |world| {
                world.prune_unkept_demands(&keep);
            });
            if fully_covered {
                continue;
            }
            let mut pending = Vec::new();
            let centers: Vec<(i32, i32)> = self
                .players
                .values()
                .filter(|session| session.interest.dimension == dimension)
                .map(|session| {
                    crate::world::chunk_xz(
                        session.last_pose_position[0].floor() as i32,
                        session.last_pose_position[2].floor() as i32,
                    )
                })
                .collect();
            self.authority.with_world(dimension, |world| {
                // Multiplayer dense grid covers the session-center union.
                if centers.is_empty() {
                    world.chunks.cover_session_centers(&[chunk_xz(
                        self.level.spawn_x,
                        self.level.spawn_z,
                    )]);
                } else {
                    world.chunks.cover_session_centers(&centers);
                }
                let mut unkept_dirty: Vec<_> = world
                    .chunks
                    .chunks
                    .keys()
                    .filter(|key| {
                        !keep.contains(key)
                            && !world.failed_restore_chunks().contains(key)
                            && world.chunks.dirty_chunks.is_dirty(key.0, key.1)
                    })
                    .collect();
                unkept_dirty.sort_unstable();
                pending = unkept_dirty
                    .into_iter()
                    .filter_map(|(cx, cz)| {
                        world.chunk_save_payload(cx, cz).map(|data| (cx, cz, data))
                    })
                    .collect();
            });
            let mut flush_error = None;
            if !pending.is_empty() {
                let flushed: Vec<(i32, i32)> =
                    pending.iter().map(|(cx, cz, _)| (*cx, *cz)).collect();
                if let Err(error) = self.save_manager.save_chunks_in(dimension, pending) {
                    flush_error = Some(error.to_string());
                } else {
                    self.authority.with_world(dimension, |world| {
                        for (cx, cz) in flushed {
                            world.chunks.dirty_chunks.remove(cx, cz);
                        }
                    });
                }
            }
            let save_manager = &mut self.save_manager;
            let mut generation_after = 0u64;
            let mut any_unkept = false;
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
                any_unkept = world.chunks.chunks.keys().any(|key| !keep.contains(&key));
                generation_after = world.chunks.load_generation();
            });
            // Only short-circuit future ticks when every resident is inside keep.
            // Dirty columns that refused to flush must be retried next tick.
            if flush_error.is_none() && !any_unkept {
                self.mark_residency_covered(dimension, generation_after);
            }
            if let Some(error) = flush_error {
                self.metrics.evict_flush_failures =
                    self.metrics.evict_flush_failures.saturating_add(1);
                let _ = error;
            }
        }
    }

    fn save_player(&self, id: u64, session: &PlayerSessionState) -> io::Result<()> {
        #[cfg(test)]
        if SAVE_PLAYER_FAILPOINT.with(|failpoint| failpoint.get()) {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "injected save_player failure",
            ));
        }
        // `save_all` calls `sync_gameplay_projection` first so `session.data`
        // matches the contract; still overlay from authority onto the clone as
        // a safety net for single-player leave paths.
        let mut data = session.data.clone();
        let username = self
            .authority
            .session(id)
            .map(|session| session.username.clone());
        let current_dimension = self
            .authority
            .session(id)
            .and_then(|authority_session| {
                Dimension::from_wire(authority_session.dimension).map(|dimension| {
                    apply_gameplay_to_player_data(&mut data, authority_session.gameplay);
                    data.game_mode = authority_session.game_mode;
                    data.position = authority_session.position;
                    data.yaw = authority_session.yaw;
                    data.pitch = authority_session.pitch;
                    dimension
                })
            })
            .unwrap_or(session.interest.dimension);
        match session.storage {
            LocalSessionStorage::Named => {
                let Some(username) = username else {
                    return Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        "missing authority session for named player save",
                    ));
                };
                self.save_manager.save_dedicated_player(
                    &username,
                    current_dimension,
                    &data,
                    &session.effects,
                )
            }
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

    #[cfg(test)]
    pub fn is_worldgen_in_flight(&self, dimension: Dimension, chunk_x: i32, chunk_z: i32) -> bool {
        self.worldgen_worker.is_in_flight(dimension, chunk_x, chunk_z)
    }

    #[cfg(test)]
    pub fn is_worldgen_pending_in_backlog(&self, dimension: Dimension, chunk_x: i32, chunk_z: i32) -> bool {
        self.authority.is_worldgen_pending(dimension, chunk_x, chunk_z)
    }

    #[cfg(test)]
    pub fn in_flight_worldgen_count(&self) -> usize {
        self.worldgen_worker.in_flight_count()
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
    crate::authority::contract::scalar_to_milli(value)
}

fn milli_to_scalar(value: u32) -> f32 {
    crate::authority::contract::milli_to_scalar(value)
}

fn session_slot_from_stack(
    stack: Option<&crate::inventory::ItemStack>,
) -> Option<SessionInventorySlot> {
    SessionInventorySlot::from_stack_opt(stack)
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
#[path = "server_runtime/tests.rs"]
mod tests;

