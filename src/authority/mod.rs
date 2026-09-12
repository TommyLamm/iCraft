//! GPU-independent authoritative simulation.

pub mod combat;
pub mod contract;
pub(crate) mod dispatch;
pub mod fishing;
pub mod interest;
pub mod mining;
pub(crate) mod portals;
pub(crate) mod tick;
pub mod transactions;

use crate::dimension::Dimension;
use crate::game_rules::{Difficulty, WorldRules, WorldType};
use crate::network::protocol::{GameplayRequest, GameplayResponse, PlayerId, RejectReason};
use crate::server_world::{ServerWorld, WorldgenMode};
use crate::world::Chunk;
use contract::{
    AuthoritySnapshot, SessionContract, SessionGameplayState, SessionGameplayUpdate, WorldMutation,
};
pub(crate) use dispatch::stack_from_slot;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) const AUTHORITY_ENTITY_ID_START: u64 = 1 << 63;
pub(crate) const ATTACK_COOLDOWN_TICKS: u16 = 5;

pub use contract::{
    common_gameplay_vectors, milli_to_scalar, milli_to_vec3, milli_within_abs_limit,
    position_to_milli, position_to_milli_opt, quantize_health, scalar_to_milli, RevisionClock,
    SessionActionView, FIXED_TICK_HZ, POSITION_ABS_LIMIT, POSITION_MILLI_ABS_LIMIT,
    RESPONSE_CACHE_CAPACITY,
};

// Dimension is a wire-ordered enum and is used as the deterministic key for
// parked authoritative worlds.  Keep this local to the authority module so
// the renderer-facing dimension type does not need a broader API change.
impl Ord for Dimension {
    fn cmp(&self, other: &Self) -> Ordering {
        (*self as u8).cmp(&(*other as u8))
    }
}

impl PartialOrd for Dimension {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Clone, Copy)]
pub struct AuthorityConfig {
    pub seed: u32,
    pub dimension: Dimension,
    pub world_type: WorldType,
    pub generate_structures: bool,
    pub rules: WorldRules,
    pub difficulty: Difficulty,
    pub simulation_distance: i32,
}

impl Default for AuthorityConfig {
    fn default() -> Self {
        Self {
            seed: 0,
            dimension: Dimension::Overworld,
            world_type: WorldType::Default,
            // Structure generation remains an explicit world-creation option;
            // the shared headless contract defaults to terrain-only so a
            // malformed legacy structure seed cannot abort authority startup.
            generate_structures: false,
            rules: WorldRules::default(),
            difficulty: Difficulty::default(),
            simulation_distance: 8,
        }
    }
}

/// Owns sessions, request sequencing and the headless world.  Transport code
/// only registers sessions, submits envelopes and consumes snapshots.
pub struct AuthorityCore {
    /// Immutable world-creation inputs used when a dimension has not been
    /// visited yet.  Each dimension then owns an independent parked world so
    /// switching cannot reinterpret one dimension's chunks as another's.
    pub(crate) config: AuthorityConfig,
    /// Every loaded dimension. Iteration is `BTreeMap` order (`Dimension` as
    /// `u8`) so tick/save/checksum stay stable. Callers pass an explicit
    /// `Dimension` (or `&mut ServerWorld`) — there is no active-world pointer.
    pub(crate) worlds: BTreeMap<Dimension, ServerWorld>,
    pub(crate) sessions: BTreeMap<PlayerId, SessionContract>,
    /// Player ids grouped by session dimension. Tick phases look this up instead
    /// of filtering the full session table once per loaded dimension.
    pub(crate) sessions_by_dimension: BTreeMap<u8, Vec<PlayerId>>,
    /// Sessions whose `gameplay.revision` (or join/dimension identity) changed
    /// since the last published snapshot. `tick` drains this into `session_updates`.
    pub(crate) dirty_session_ids: BTreeSet<PlayerId>,
    pub(crate) last_snapshot: AuthoritySnapshot,
    pub(crate) fixed_tick: u64,
    /// Mutations emitted between fixed ticks (for example an authenticated
    /// player request).  Presentation roots drain these through the same
    /// snapshot projection as tick-driven automation.
    pub(crate) pending_mutations: Vec<WorldMutation>,
    /// Session ids changed as a side effect of another player's request. They
    /// receive the request's single authoritative revision at publication.
    pub(crate) pending_session_revisions: BTreeSet<PlayerId>,
    pub(crate) pending_dimension_transfers: Vec<DimensionTransferIntent>,
    /// High-bit ids are reserved for authority-created hooks, drops and XP.
    /// The allocator is shared by every loaded dimension, unlike each world's
    /// legacy EntityManager allocator.
    pub(crate) next_authority_entity_id: u64,
    /// Completed worldgen columns waiting for deterministic apply at tick start.
    /// Runtime polls the worker and pushes here before `tick`.
    pub(crate) pending_worldgen: Vec<PendingWorldgenColumn>,
    /// Mode applied to newly created dimensions.
    pub(crate) worldgen_mode: WorldgenMode,
    /// Chunk / entity counts accumulated while walking worlds in the last tick.
    pub(crate) last_tick_loaded_chunks: usize,
    pub(crate) last_tick_entities: usize,
}

pub struct PendingWorldgenColumn {
    pub dimension: Dimension,
    pub chunk_x: i32,
    pub chunk_z: i32,
    pub chunk: Chunk,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DimensionTransferIntent {
    pub player_id: PlayerId,
    pub from: Dimension,
    pub to: Dimension,
    pub position: [f32; 3],
}

impl AuthorityCore {
    pub fn new(config: AuthorityConfig) -> Self {
        let mut worlds = BTreeMap::new();
        worlds.insert(
            config.dimension,
            Self::new_world(config, config.dimension, WorldgenMode::Sync),
        );
        Self {
            config,
            worlds,
            sessions: BTreeMap::new(),
            sessions_by_dimension: BTreeMap::new(),
            dirty_session_ids: BTreeSet::new(),
            last_snapshot: AuthoritySnapshot::empty(),
            fixed_tick: 0,
            pending_mutations: Vec::new(),
            pending_session_revisions: BTreeSet::new(),
            pending_dimension_transfers: Vec::new(),
            next_authority_entity_id: AUTHORITY_ENTITY_ID_START,
            pending_worldgen: Vec::new(),
            worldgen_mode: WorldgenMode::Sync,
            last_tick_loaded_chunks: 0,
            last_tick_entities: 0,
        }
    }

    pub fn set_worldgen_mode_all(&mut self, mode: WorldgenMode) {
        self.worldgen_mode = mode;
        for world in self.worlds.values_mut() {
            world.set_worldgen_mode(mode);
        }
    }

    pub fn queue_worldgen_results(&mut self, columns: Vec<PendingWorldgenColumn>) {
        self.pending_worldgen.extend(columns);
    }

    /// Apply completed worldgen at tick start. Sort is session-id then
    /// nearest-first to the earliest session in that dimension; capped by
    /// `apply_limit` so projection budgets stay bounded.
    pub(crate) fn apply_pending_worldgen(&mut self, apply_limit: usize) {
        if self.pending_worldgen.is_empty() || apply_limit == 0 {
            return;
        }
        let pending = std::mem::take(&mut self.pending_worldgen);
        let mut order: Vec<usize> = (0..pending.len()).collect();
        order.sort_by_key(|&index| {
            let column = &pending[index];
            let session_key = self
                .session_ids_in_dimension(column.dimension)
                .first()
                .copied()
                .unwrap_or(PlayerId::MAX);
            let distance = self
                .sessions
                .get(&session_key)
                .map(|session| {
                    let px = (session.position[0] / 16.0).floor() as i32;
                    let pz = (session.position[2] / 16.0).floor() as i32;
                    let dx = column.chunk_x.saturating_sub(px) as i64;
                    let dz = column.chunk_z.saturating_sub(pz) as i64;
                    dx * dx + dz * dz
                })
                .unwrap_or(i64::MAX);
            (
                column.dimension as u8,
                session_key,
                distance,
                column.chunk_x,
                column.chunk_z,
                index,
            )
        });
        let mut slots: Vec<Option<PendingWorldgenColumn>> =
            pending.into_iter().map(Some).collect();
        let mut applied = 0usize;
        let mut deferred = Vec::new();
        for index in order {
            let Some(column) = slots[index].take() else {
                continue;
            };
            if applied >= apply_limit {
                deferred.push(column);
                continue;
            }
            if let Some(world) = self.worlds.get_mut(&column.dimension) {
                world.apply_generated_chunk(column.chunk_x, column.chunk_z, column.chunk);
                applied += 1;
            } else {
                deferred.push(column);
            }
        }
        for slot in slots.into_iter().flatten() {
            deferred.push(slot);
        }
        self.pending_worldgen = deferred;
    }

    fn new_world(config: AuthorityConfig, dimension: Dimension, mode: WorldgenMode) -> ServerWorld {
        let mut world = ServerWorld::new_with_difficulty(
            config.seed,
            dimension,
            config.world_type,
            config.generate_structures,
            config.rules,
            config.simulation_distance,
            config.difficulty,
        );
        world.set_worldgen_mode(mode);
        world
    }

    pub(crate) fn ensure_dimension(&mut self, target: Dimension) {
        if self.worlds.contains_key(&target) {
            return;
        }
        let mode = self.worldgen_mode;
        self.worlds
            .insert(target, Self::new_world(self.config, target, mode));
    }

    /// Resident column / entity totals without allocating a dimension Vec.
    pub fn resident_metrics(&self) -> (usize, usize) {
        self.worlds.values().fold((0, 0), |(chunks, entities), world| {
            (
                chunks.saturating_add(world.chunks.chunks.len()),
                entities.saturating_add(world.entities.entities.len()),
            )
        })
    }

    /// Read a loaded dimension. Prefer this over any ambient "active world".
    pub fn world_ref(&self, dimension: Dimension) -> Option<&ServerWorld> {
        self.worlds.get(&dimension)
    }

    /// Required lookup of a loaded dimension. Panics if the dimension was never
    /// ensured; request/tick paths call `ensure_dimension` / `with_world` first.
    pub fn world(&self, dimension: Dimension) -> &ServerWorld {
        self.worlds
            .get(&dimension)
            .unwrap_or_else(|| panic!("dimension {dimension:?} missing from world map"))
    }

    /// Mutably access a loaded dimension. Callers that need to create a missing
    /// dimension should use `with_world`.
    pub fn world_mut(&mut self, dimension: Dimension) -> Option<&mut ServerWorld> {
        self.worlds.get_mut(&dimension)
    }

    /// Required mutable lookup after the dimension is known to be loaded.
    pub(crate) fn world_mut_expect(&mut self, dimension: Dimension) -> &mut ServerWorld {
        self.worlds
            .get_mut(&dimension)
            .unwrap_or_else(|| panic!("dimension {dimension:?} missing from world map"))
    }

    /// Execute a bounded operation against one dimension, creating it if needed.
    pub fn with_world<R>(
        &mut self,
        dimension: Dimension,
        operation: impl FnOnce(&mut ServerWorld) -> R,
    ) -> R {
        self.ensure_dimension(dimension);
        let world = self
            .worlds
            .get_mut(&dimension)
            .expect("ensure_dimension inserts the target world");
        operation(world)
    }

    /// Every dimension with an authoritative world, in `BTreeMap` key order.
    /// Zero-allocation view over the map keys.
    pub fn dimensions(&self) -> impl Iterator<Item = Dimension> + '_ {
        self.worlds.keys().copied()
    }

    fn cleanup_session_lifecycle(&mut self, id: PlayerId, dimension: Dimension) {
        let hook = self
            .sessions
            .get(&id)
            .and_then(|session| session.gameplay.fishing_hook)
            .map(|hook| hook.entity_id);
        if let Some(session) = self.sessions.get_mut(&id) {
            // Brew reservations have not debited inventory yet. Clearing the
            // reservation is therefore the lossless logout/transition path.
            session.gameplay.fishing_hook = None;
            session.gameplay.brew = None;
            session.gameplay.mining = None;
            session.gameplay.mounted_entity = None;
            session.gameplay.shield_active = false;
            session.portal_contact_time = 0.0;
            session.portal_requested = false;
        }
        self.with_world(dimension, |world| {
            world.close_container_viewers_forced(id);
            world.remove_passenger(id);
            if let Some(hook) = hook {
                world.remove_authority_entity(hook);
            }
        });
    }

    /// Allocate the next global authority entity id. Trusts the monotonic
    /// counter; only probes the target dimension and optional session hook.
    /// Full multi-world / multi-session scans are debug-only asserts.
    pub(crate) fn next_unique_entity_id(
        &self,
        dimension: Dimension,
        owner: Option<PlayerId>,
    ) -> u64 {
        let mut candidate = self.next_authority_entity_id.max(AUTHORITY_ENTITY_ID_START);
        loop {
            if candidate == 0 {
                candidate = AUTHORITY_ENTITY_ID_START;
                continue;
            }
            let entity_exists = self
                .worlds
                .get(&dimension)
                .is_some_and(|world| world.entities.get_by_id(candidate).is_some());
            let hook_exists = owner
                .and_then(|id| self.sessions.get(&id))
                .and_then(|session| session.gameplay.fishing_hook)
                .is_some_and(|hook| hook.entity_id == candidate);
            debug_assert!(
                {
                    let full_entity = self
                        .worlds
                        .values()
                        .any(|world| world.entities.get_by_id(candidate).is_some());
                    let full_hook = self.sessions.values().any(|session| {
                        session
                            .gameplay
                            .fishing_hook
                            .is_some_and(|hook| hook.entity_id == candidate)
                    });
                    (entity_exists || hook_exists) == (full_entity || full_hook)
                },
                "monotonic entity id collided outside target world/session"
            );
            if !entity_exists && !hook_exists {
                return candidate;
            }
            candidate = candidate.wrapping_add(1).max(AUTHORITY_ENTITY_ID_START);
        }
    }

    pub(crate) fn claim_entity_id(&mut self, id: u64) {
        debug_assert_ne!(id, 0);
        self.next_authority_entity_id = id.wrapping_add(1).max(AUTHORITY_ENTITY_ID_START);
    }

    /// Revision for one dimension's independent namespace. Request gates and
    /// ACKs use this value for the session's dimension; the aggregate snapshot
    /// revision is only a compatibility summary and must not be used for a
    /// cross-dimension stale check.
    pub fn revision_for_dimension(&self, dimension: Dimension) -> u64 {
        self.worlds
            .get(&dimension)
            .map(|world| world.revisions.current())
            .unwrap_or(0)
    }

    /// Explicit dimension-scoped revision (same as [`Self::revision_for_dimension`]).
    pub fn current_revision(&self, dimension: Dimension) -> u64 {
        self.revision_for_dimension(dimension)
    }

    /// Move a session into `target`, ensuring that dimension exists. Does not
    /// maintain an ambient active-world pointer.
    pub fn set_session_dimension(&mut self, id: PlayerId, target: Dimension) -> bool {
        let Some(current_dimension) = self
            .sessions
            .get(&id)
            .and_then(|session| Dimension::from_wire(session.dimension))
        else {
            return false;
        };
        self.cleanup_session_lifecycle(id, current_dimension);
        self.ensure_dimension(target);
        let revision = self.current_revision(target);
        let previous_dimension = {
            let Some(session) = self.sessions.get_mut(&id) else {
                return false;
            };
            let previous_dimension = session.dimension;
            session.dimension = target as u8;
            session.last_revision = revision;
            session.gameplay.revision = revision;
            previous_dimension
        };
        self.reindex_session_dimension(id, previous_dimension, target as u8);
        self.mark_session_update(id);
        true
    }

    pub fn set_rules(&mut self, rules: WorldRules) {
        let rules = rules.normalized();
        self.config.rules = rules;
        for world in self.worlds.values_mut() {
            world.rules = rules;
        }
    }

    pub fn register_session(&mut self, session: SessionContract) -> Result<(), RejectReason> {
        self.register_session_with_limit(session, usize::MAX)
    }

    /// Atomically reserve an authenticated identity and player slot. The
    /// caller performs persistence/network setup only after this succeeds;
    /// `remove_session` is the rollback operation for a later setup failure.
    pub fn register_session_with_limit(
        &mut self,
        session: SessionContract,
        max_players: usize,
    ) -> Result<(), RejectReason> {
        if self.sessions.len() >= max_players.max(1) {
            return Err(RejectReason::QueueFull);
        }
        if self.sessions.contains_key(&session.id) {
            return Err(RejectReason::Duplicate);
        }
        if self
            .sessions
            .values()
            .any(|existing| existing.username.eq_ignore_ascii_case(&session.username))
        {
            return Err(RejectReason::Duplicate);
        }
        let Some(dimension) = Dimension::from_wire(session.dimension) else {
            return Err(RejectReason::InvalidDimension);
        };
        self.ensure_dimension(dimension);
        let id = session.id;
        let dimension_wire = session.dimension;
        self.sessions.insert(id, session);
        self.index_insert_session(dimension_wire, id);
        self.mark_session_update(id);
        Ok(())
    }

    pub fn remove_session(&mut self, id: PlayerId) -> Option<SessionContract> {
        let dimension = self
            .sessions
            .get(&id)
            .and_then(|session| Dimension::from_wire(session.dimension));
        if let Some(dimension) = dimension {
            self.cleanup_session_lifecycle(id, dimension);
        }
        let session = self.sessions.remove(&id)?;
        self.index_remove_session(session.dimension, id);
        self.dirty_session_ids.remove(&id);
        Some(session)
    }

    pub fn take_pending_dimension_transfers(&mut self) -> Vec<DimensionTransferIntent> {
        std::mem::take(&mut self.pending_dimension_transfers)
    }

    pub fn session(&self, id: PlayerId) -> Option<&SessionContract> {
        self.sessions.get(&id)
    }

    pub fn session_mut(&mut self, id: PlayerId) -> Option<&mut SessionContract> {
        self.sessions.get_mut(&id)
    }

    pub fn sessions(&self) -> impl Iterator<Item = &SessionContract> {
        self.sessions.values()
    }

    fn index_insert_session(&mut self, dimension: u8, id: PlayerId) {
        let ids = self.sessions_by_dimension.entry(dimension).or_default();
        match ids.binary_search(&id) {
            Ok(_) => {}
            Err(index) => ids.insert(index, id),
        }
    }

    fn index_remove_session(&mut self, dimension: u8, id: PlayerId) {
        let Some(ids) = self.sessions_by_dimension.get_mut(&dimension) else {
            return;
        };
        if let Ok(index) = ids.binary_search(&id) {
            ids.remove(index);
        }
        if ids.is_empty() {
            self.sessions_by_dimension.remove(&dimension);
        }
    }

    fn reindex_session_dimension(&mut self, id: PlayerId, from: u8, to: u8) {
        if from == to {
            return;
        }
        self.index_remove_session(from, id);
        self.index_insert_session(to, id);
    }

    pub(crate) fn session_ids_in_dimension(&self, dimension: Dimension) -> &[PlayerId] {
        self.sessions_by_dimension
            .get(&(dimension as u8))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub(crate) fn mark_session_update(&mut self, id: PlayerId) {
        self.dirty_session_ids.insert(id);
    }

    fn take_dirty_session_updates(&mut self) -> Vec<SessionGameplayUpdate> {
        std::mem::take(&mut self.dirty_session_ids)
            .into_iter()
            .filter_map(|id| {
                self.sessions.get(&id).map(|session| SessionGameplayUpdate {
                    player_id: session.id,
                    dimension: session.dimension,
                    state: session.gameplay,
                })
            })
            .collect()
    }

    pub fn last_snapshot(&self) -> &AuthoritySnapshot {
        &self.last_snapshot
    }

    pub fn set_session_gameplay(&mut self, id: PlayerId, gameplay: SessionGameplayState) -> bool {
        {
            let Some(session) = self.sessions.get_mut(&id) else {
                return false;
            };
            session.gameplay = gameplay;
        }
        self.mark_session_update(id);
        true
    }

    /// Reset a dead session through the same authority semantics used by the
    /// `/respawn` command.  Runtime transport adapters call this lifecycle seam
    /// directly because a client must not need operator permission to respawn.
    pub fn respawn_session(&mut self, id: PlayerId) -> bool {
        let Some(dimension) = self
            .sessions
            .get(&id)
            .and_then(|session| Dimension::from_wire(session.dimension))
        else {
            return false;
        };
        if !self
            .sessions
            .get(&id)
            .is_some_and(|session| session.gameplay.is_dead)
        {
            return false;
        }
        self.cleanup_session_lifecycle(id, dimension);
        self.ensure_dimension(dimension);
        let hardcore = self.world(dimension).rules.hardcore;
        let revision = self.world_mut_expect(dimension).revisions.allocate();
        {
            let Some(session) = self.sessions.get_mut(&id) else {
                return false;
            };
            session.gameplay.is_dead = false;
            session.gameplay.death_source = None;
            session.gameplay.health_milli = session.gameplay.max_health_milli;
            session.gameplay.hunger_milli = 20_000;
            session.gameplay.saturation_milli = 5_000;
            session.gameplay.velocity_milli = [0; 3];
            session.gameplay.mounted_entity = None;
            if hardcore {
                session.game_mode = crate::inventory::GameMode::Spectator;
            }
            session.last_revision = revision;
            session.gameplay.revision = revision;
        }
        self.mark_session_update(id);
        true
    }

    /// Drain request mutations without advancing the simulation clock.
    pub fn take_pending_mutations(&mut self) -> Vec<WorldMutation> {
        std::mem::take(&mut self.pending_mutations)
    }

    /// Drain exact container invalidations emitted by all loaded dimensions.
    /// The runtime consumes these after the fixed tick so a block break can
    /// close only the viewers that were actually registered on that block.
    pub fn take_container_closures(&mut self) -> Vec<crate::server_world::ContainerClosure> {
        let mut closures = Vec::new();
        for dimension in self.dimensions().collect::<Vec<_>>() {
            if let Some(world) = self.world_mut(dimension) {
                closures.extend(world.take_container_closures());
            }
        }
        closures
    }
}

#[cfg(test)]
mod tests;

