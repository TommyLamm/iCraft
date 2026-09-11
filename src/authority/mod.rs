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
use crate::server_world::ServerWorld;
use contract::{
    AuthoritySnapshot, SessionContract, SessionGameplayState, SessionGameplayUpdate, WorldMutation,
};
pub(crate) use dispatch::stack_from_slot;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) const AUTHORITY_ENTITY_ID_START: u64 = 1 << 63;
pub(crate) const ATTACK_COOLDOWN_TICKS: u16 = 5;

pub use contract::{
    common_gameplay_vectors, milli_within_abs_limit, position_to_milli, position_to_milli_opt,
    RevisionClock, AUTHORITY_CONTRACT_VERSION, FIXED_TICK_HZ, POSITION_ABS_LIMIT,
    POSITION_MILLI_ABS_LIMIT, RESPONSE_CACHE_CAPACITY,
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
    pub render_distance: i32,
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
            render_distance: 8,
        }
    }
}

/// Owns sessions, request sequencing and the headless world.  Transport code
/// only registers sessions, submits envelopes and consumes snapshots.
pub struct AuthorityCore {
    /// Key of the currently selected world in `worlds`.  Never a moved value:
    /// every loaded dimension stays in the map for its lifetime.
    pub(crate) active_dimension: Dimension,
    /// Immutable world-creation inputs used when a dimension has not been
    /// visited yet.  Each dimension then owns an independent parked world so
    /// switching cannot reinterpret one dimension's chunks as another's.
    pub(crate) config: AuthorityConfig,
    /// Every loaded dimension, including the active one.  Iteration is
    /// `BTreeMap` order (`Dimension` as `u8`) so tick/save/checksum stay stable.
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
        worlds.insert(config.dimension, Self::new_world(config, config.dimension));
        Self {
            active_dimension: config.dimension,
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
        }
    }

    fn new_world(config: AuthorityConfig, dimension: Dimension) -> ServerWorld {
        ServerWorld::new_with_difficulty(
            config.seed,
            dimension,
            config.world_type,
            config.generate_structures,
            config.rules,
            config.render_distance,
            config.difficulty,
        )
    }

    pub(crate) fn ensure_dimension(&mut self, target: Dimension) {
        if self.worlds.contains_key(&target) {
            return;
        }
        self.worlds
            .insert(target, Self::new_world(self.config, target));
    }

    /// Select `target` as the active-dimension key.  The world stays in the
    /// map; this never moves a `ServerWorld` value.
    pub fn activate_dimension(&mut self, target: Dimension) {
        if self.active_dimension == target {
            return;
        }
        self.ensure_dimension(target);
        self.active_dimension = target;
    }

    pub fn active_dimension(&self) -> Dimension {
        self.active_dimension
    }

    /// Shared map lookup of the currently active dimension.
    pub fn world(&self) -> &ServerWorld {
        self.worlds
            .get(&self.active_dimension)
            .expect("active dimension missing from world map")
    }

    /// Mutable map lookup of the currently active dimension.
    pub fn world_mut_active(&mut self) -> &mut ServerWorld {
        let dim = self.active_dimension;
        self.worlds
            .get_mut(&dim)
            .expect("active dimension missing from world map")
    }

    /// Read a loaded dimension without changing the active key.
    pub fn world_ref(&self, dimension: Dimension) -> Option<&ServerWorld> {
        self.worlds.get(&dimension)
    }

    /// Mutably access a loaded dimension without changing the active key.
    /// Callers that need to create a missing dimension should use `with_world`.
    pub fn world_mut(&mut self, dimension: Dimension) -> Option<&mut ServerWorld> {
        self.worlds.get_mut(&dimension)
    }

    /// Execute a bounded operation against one dimension.  Does not swap
    /// worlds or change `active_dimension`.
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

    /// Return every dimension with an authoritative world.  Ordering is the
    /// `BTreeMap` key order (`Dimension` as `u8`) for deterministic tick
    /// and persistence traversal.
    pub fn dimensions(&self) -> Vec<Dimension> {
        self.worlds.keys().copied().collect()
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

    pub(crate) fn next_unique_entity_id(&self) -> u64 {
        let mut candidate = self.next_authority_entity_id.max(AUTHORITY_ENTITY_ID_START);
        loop {
            let entity_exists = self
                .worlds
                .values()
                .any(|world| world.entities.get_by_id(candidate).is_some());
            let hook_exists = self.sessions.values().any(|session| {
                session
                    .gameplay
                    .fishing_hook
                    .is_some_and(|hook| hook.entity_id == candidate)
            });
            if candidate != 0 && !entity_exists && !hook_exists {
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

    /// Activate the session's target dimension while preserving the boundary
    /// compatibility contract.  Gameplay requests use `activate_dimension`
    /// directly and therefore do not need to switch another session's world.
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
        self.activate_dimension(target);
        let revision = self.current_revision();
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

    /// Active-world compatibility revision. Use `revision_for_dimension` for
    /// request/client gates when a session may be in another dimension.
    pub fn current_revision(&self) -> u64 {
        self.world().revisions.current()
    }

    pub fn session_gameplay(&self, id: PlayerId) -> Option<SessionGameplayUpdate> {
        let session = self.sessions.get(&id)?;
        Some(SessionGameplayUpdate {
            player_id: id,
            dimension: session.dimension,
            state: session.gameplay,
        })
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
        self.activate_dimension(dimension);
        let hardcore = self.world().rules.hardcore;
        let revision = self.world_mut_active().revisions.allocate();
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

    pub fn common_vector_snapshot(&mut self) -> Vec<(GameplayResponse, AuthoritySnapshot)> {
        let mut responses = Vec::new();
        for request in common_gameplay_vectors() {
            let response = self.submit_request(request);
            responses.push((response, self.last_snapshot().clone()));
        }
        responses
    }

    /// Drain request mutations without advancing the simulation clock.
    pub fn take_pending_mutations(&mut self) -> Vec<WorldMutation> {
        std::mem::take(&mut self.pending_mutations)
    }

    /// Drain exact container invalidations emitted by all loaded dimensions.
    /// The runtime consumes these after the fixed tick so a block break can
    /// close only the viewers that were actually registered on that block.
    pub fn take_container_closures(&mut self) -> Vec<crate::server_world::ContainerClosure> {
        let dimensions = self.dimensions();
        let mut closures = Vec::new();
        for dimension in dimensions {
            if let Some(world) = self.world_mut(dimension) {
                closures.extend(world.take_container_closures());
            }
        }
        closures
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authority::contract::{
        SessionBrewState, SessionFishingHookState, SessionGameplayState, SessionInventorySlot,
    };
    use crate::entity::EntityType;
    use crate::inventory::Item;
    use crate::network::protocol::{
        BlockActionKind, GameplayOperation, GameplayOutcome, SlotRefWire,
    };
    use crate::world::BlockType;
    use contract::SessionContract;

    fn core() -> AuthorityCore {
        let mut core = AuthorityCore::new(AuthorityConfig::default());
        core.register_session(SessionContract::new(
            7,
            "alex",
            0,
            [8.0, 80.0, 8.0],
            true,
            true,
        ))
        .unwrap();
        core
    }

    fn block_request(
        request_id: u128,
        client_sequence: u64,
        action: BlockActionKind,
        target: (i32, i32, i32),
        held: Option<crate::network::protocol::SessionSlotWire>,
        block: BlockType,
        face: [i8; 3],
        look_milli: [i16; 3],
        client_revision: u64,
    ) -> GameplayRequest {
        GameplayRequest {
            request_id,
            client_sequence,
            session_id: 7,
            dimension: 0,
            client_revision,
            operation: GameplayOperation::BlockAction {
                action,
                x: target.0,
                y: target.1,
                z: target.2,
                face,
                hand: 0,
                held,
                block: if matches!(action, BlockActionKind::Place) {
                    block.to_wire()
                } else {
                    BlockType::Air.to_wire()
                },
                look_milli,
            },
        }
    }

    #[test]
    fn duplicate_and_stale_revision_are_authoritative() {
        let mut core = core();
        let mut request = GameplayRequest {
            request_id: 1,
            client_sequence: 1,
            session_id: 7,
            dimension: 0,
            client_revision: 0,
            operation: GameplayOperation::ItemUse {
                item: Item::Bread as u32,
                count: 1,
            },
        };
        let first = core.submit_request(request.clone());
        let duplicate = core.submit_request(request.clone());
        assert_eq!(first, duplicate);
        request.request_id = 2;
        request.client_sequence = 2;
        request.client_revision = core.current_revision() + 1;
        let revision_before_reject = core.current_revision();
        let rejected = core.submit_request(request);
        assert!(matches!(
            rejected.outcome,
            GameplayOutcome::Rejected {
                reason: RejectReason::InvalidRevision
            }
        ));
        assert_eq!(core.current_revision(), revision_before_reject);
        assert_eq!(rejected.server_sequence, revision_before_reject);
    }

    #[test]
    fn authenticated_rejections_are_cached_without_consuming_sequence() {
        let mut core = core();
        let request = GameplayRequest {
            request_id: 9,
            client_sequence: 1,
            session_id: 7,
            dimension: 0,
            client_revision: 0,
            operation: GameplayOperation::Command {
                command: "/gamerule doDaylightCycle ".to_string() + &"x".repeat(3000),
            },
        };
        let first = core.submit_request(request.clone());
        let duplicate = core.submit_request(request);
        assert_eq!(first, duplicate);
        assert_eq!(core.session(7).unwrap().last_client_sequence, 0);
        assert_eq!(core.session(7).unwrap().cache_len(), 1);
    }

    #[test]
    fn rejected_block_action_does_not_drain_a_mutation() {
        let mut core = core();
        let before = core.world().get_block(8, 80, 8);
        let request = GameplayRequest {
            request_id: 21,
            client_sequence: 1,
            session_id: 7,
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
                block: 3,
                look_milli: [0, 0, 1000],
            },
        };
        let response = core.submit_request(request);
        assert!(matches!(
            response.outcome,
            GameplayOutcome::Rejected {
                reason: RejectReason::InvalidState
            }
        ));
        assert!(core.take_pending_mutations().is_empty());
        assert_eq!(core.world().get_block(8, 80, 8), before);
    }

    #[test]
    fn typed_mining_fixed_tick_breaks_once_and_cancel_is_idempotent() {
        let mut core = core();
        let target = (8, 81, 9);
        core.world_mut_active()
            .set_block(target.0, target.1, target.2, BlockType::Stone, 0)
            .unwrap();

        let held_stack = crate::inventory::ItemStack::new(Item::StonePickaxe, 1);
        let held = crate::network::protocol::SessionSlotWire::new(
            crate::network::protocol::ItemWire::from_stack(&held_stack),
            0,
            0,
        );
        let mut gameplay = SessionGameplayState::default();
        gameplay.inventory[0] = Some(SessionInventorySlot::from(held));
        core.set_session_gameplay(7, gameplay);

        let start = GameplayRequest {
            request_id: 100,
            client_sequence: 1,
            session_id: 7,
            dimension: 0,
            client_revision: 0,
            operation: GameplayOperation::BlockAction {
                action: BlockActionKind::StartBreak,
                x: target.0,
                y: target.1,
                z: target.2,
                face: [0, 0, -1],
                hand: 0,
                held: Some(held),
                block: BlockType::Air.to_wire(),
                look_milli: [0, -100, 995],
            },
        };
        assert!(matches!(
            core.submit_request(start).outcome,
            GameplayOutcome::Accepted { .. }
        ));
        assert!(core.session(7).unwrap().gameplay.mining.is_some());

        let mut target_mutations = 0;
        for _ in 0..80 {
            let snapshot = core.tick();
            target_mutations += snapshot
                .mutations
                .iter()
                .filter(|mutation| mutation.position == target)
                .count();
        }
        assert_eq!(
            core.world().get_block(target.0, target.1, target.2),
            BlockType::Air
        );
        assert_eq!(target_mutations, 1);
        assert!(core.session(7).unwrap().gameplay.mining.is_none());
        assert_eq!(
            core.world_mut_active()
                .entities
                .entities
                .iter()
                .filter(|entity| entity.entity_type == EntityType::DroppedItem)
                .count(),
            1
        );

        let cancel = GameplayRequest {
            request_id: 101,
            client_sequence: 2,
            session_id: 7,
            dimension: 0,
            client_revision: core.current_revision(),
            operation: GameplayOperation::BlockAction {
                action: BlockActionKind::CancelBreak,
                x: target.0,
                y: target.1,
                z: target.2,
                face: [0, 0, 0],
                hand: 0,
                held: None,
                block: BlockType::Air.to_wire(),
                look_milli: [0, -100, 995],
            },
        };
        let first_cancel = core.submit_request(cancel.clone());
        assert!(matches!(
            first_cancel.outcome,
            GameplayOutcome::Accepted { .. }
        ));
        assert_eq!(core.submit_request(cancel), first_cancel);
        assert_eq!(
            core.world_mut_active()
                .entities
                .entities
                .iter()
                .filter(|entity| entity.entity_type == EntityType::DroppedItem)
                .count(),
            1
        );
    }

    #[test]
    fn typed_mining_rejects_unloaded_target_without_progress_or_mutation() {
        let mut core = core();
        core.session_mut(7).unwrap().position = [15.0, 80.0, 8.0];
        let held_stack = crate::inventory::ItemStack::new(Item::StonePickaxe, 1);
        let held = crate::network::protocol::SessionSlotWire::new(
            crate::network::protocol::ItemWire::from_stack(&held_stack),
            0,
            0,
        );
        let mut gameplay = SessionGameplayState::default();
        gameplay.inventory[0] = Some(SessionInventorySlot::from(held));
        core.set_session_gameplay(7, gameplay);
        let target = (16, 81, 8);
        let response = core.submit_request(GameplayRequest {
            request_id: 102,
            client_sequence: 1,
            session_id: 7,
            dimension: 0,
            client_revision: 0,
            operation: GameplayOperation::BlockAction {
                action: BlockActionKind::StartBreak,
                x: target.0,
                y: target.1,
                z: target.2,
                face: [0, 0, -1],
                hand: 0,
                held: Some(held),
                block: BlockType::Air.to_wire(),
                look_milli: [995, -100, 0],
            },
        });
        assert!(
            matches!(
                response.outcome,
                GameplayOutcome::Rejected {
                    reason: RejectReason::InvalidState
                }
            ),
            "unexpected unloaded-target response: {:?}",
            response.outcome
        );
        assert!(core.session(7).unwrap().gameplay.mining.is_none());
        assert!(core.take_pending_mutations().is_empty());
    }

    #[test]
    fn typed_mining_game_modes_and_empty_hand_are_authoritative() {
        let target = (8, 81, 9);
        let pick = crate::inventory::ItemStack::new(Item::StonePickaxe, 1);
        let pick_wire = crate::network::protocol::SessionSlotWire::new(
            crate::network::protocol::ItemWire::from_stack(&pick),
            0,
            0,
        );

        let mut creative = core();
        creative.session_mut(7).unwrap().game_mode = crate::inventory::GameMode::Creative;
        creative
            .world_mut_active()
            .set_block(target.0, target.1, target.2, BlockType::Stone, 0)
            .unwrap();
        let mut creative_gameplay = SessionGameplayState::default();
        creative_gameplay.inventory[0] = Some(SessionInventorySlot::from(pick_wire));
        creative.set_session_gameplay(7, creative_gameplay);
        assert!(matches!(
            creative
                .submit_request(block_request(
                    110,
                    1,
                    BlockActionKind::StartBreak,
                    target,
                    Some(pick_wire),
                    BlockType::Air,
                    [0, 0, -1],
                    [0, -100, 995],
                    0,
                ))
                .outcome,
            GameplayOutcome::Accepted { .. }
        ));
        assert_eq!(
            creative.world().get_block(target.0, target.1, target.2),
            BlockType::Air
        );
        assert_eq!(
            creative.session(7).unwrap().gameplay.inventory[0],
            Some(SessionInventorySlot::from(pick_wire))
        );
        assert!(creative
            .world()
            .entities
            .entities
            .iter()
            .all(|entity| entity.entity_type != EntityType::ExperienceOrb));

        let mut adventure = core();
        adventure.session_mut(7).unwrap().game_mode = crate::inventory::GameMode::Adventure;
        adventure
            .world_mut_active()
            .set_block(target.0, target.1, target.2, BlockType::Stone, 0)
            .unwrap();
        let mut allowed = SessionGameplayState::default();
        let tagged_pick = crate::inventory::ItemStack::new(Item::StonePickaxe, 1)
            .with_can_break(BlockType::Stone);
        let tagged_wire = crate::network::protocol::SessionSlotWire::new(
            crate::network::protocol::ItemWire::from_stack(&tagged_pick),
            tagged_pick.can_break,
            tagged_pick.can_place_on,
        );
        allowed.inventory[0] = Some(SessionInventorySlot::from(tagged_wire));
        adventure.set_session_gameplay(7, allowed);
        assert!(matches!(
            adventure
                .submit_request(block_request(
                    111,
                    1,
                    BlockActionKind::StartBreak,
                    target,
                    Some(tagged_wire),
                    BlockType::Air,
                    [0, 0, -1],
                    [0, -100, 995],
                    0,
                ))
                .outcome,
            GameplayOutcome::Accepted { .. }
        ));
        for _ in 0..80 {
            let _ = adventure.tick();
        }
        assert_eq!(
            adventure.world().get_block(target.0, target.1, target.2),
            BlockType::Air
        );

        let mut denied = core();
        denied.session_mut(7).unwrap().game_mode = crate::inventory::GameMode::Adventure;
        denied
            .world_mut_active()
            .set_block(target.0, target.1, target.2, BlockType::Stone, 0)
            .unwrap();
        let mut untagged = SessionGameplayState::default();
        untagged.inventory[0] = Some(SessionInventorySlot::from(pick_wire));
        denied.set_session_gameplay(7, untagged);
        assert!(matches!(
            denied
                .submit_request(block_request(
                    112,
                    1,
                    BlockActionKind::StartBreak,
                    target,
                    Some(pick_wire),
                    BlockType::Air,
                    [0, 0, -1],
                    [0, -100, 995],
                    0,
                ))
                .outcome,
            GameplayOutcome::Rejected {
                reason: RejectReason::PermissionDenied
            }
        ));
        assert_eq!(
            denied.world().get_block(target.0, target.1, target.2),
            BlockType::Stone
        );

        let mut empty_hand = core();
        empty_hand
            .world_mut_active()
            .set_block(target.0, target.1, target.2, BlockType::Dirt, 0)
            .unwrap();
        assert!(matches!(
            empty_hand
                .submit_request(block_request(
                    113,
                    1,
                    BlockActionKind::StartBreak,
                    target,
                    None,
                    BlockType::Air,
                    [0, 0, -1],
                    [0, -100, 995],
                    0,
                ))
                .outcome,
            GameplayOutcome::Accepted { .. }
        ));
    }

    #[test]
    fn typed_place_maps_item_debits_once_and_rejects_cheat_block() {
        let support = (8, 80, 9);
        let target = (8, 81, 9);
        let stone_stack = crate::inventory::ItemStack::new(Item::Stone, 2);
        let stone_wire = crate::network::protocol::SessionSlotWire::new(
            crate::network::protocol::ItemWire::from_stack(&stone_stack),
            0,
            0,
        );
        let mut core = core();
        core.world_mut_active()
            .set_block(support.0, support.1, support.2, BlockType::Stone, 0)
            .unwrap();
        core.world_mut_active()
            .set_block(target.0, target.1, target.2, BlockType::Air, 0)
            .unwrap();
        let mut gameplay = SessionGameplayState::default();
        gameplay.inventory[0] = Some(SessionInventorySlot::from(stone_wire));
        core.set_session_gameplay(7, gameplay);
        let place_request = block_request(
            114,
            1,
            BlockActionKind::Place,
            target,
            Some(stone_wire),
            BlockType::Stone,
            [0, 1, 0],
            [250, -550, 750],
            0,
        );
        let accepted = core.submit_request(place_request);
        assert!(
            matches!(accepted.outcome, GameplayOutcome::Accepted { .. }),
            "unexpected place response: {:?}",
            accepted.outcome
        );
        assert_eq!(
            core.world().get_block(target.0, target.1, target.2),
            BlockType::Stone
        );
        assert_eq!(
            core.session(7).unwrap().gameplay.inventory[0]
                .unwrap()
                .item
                .count,
            1
        );

        let stale = core.submit_request(block_request(
            115,
            2,
            BlockActionKind::Place,
            (8, 81, 10),
            Some(stone_wire),
            BlockType::Chest,
            [0, 1, 0],
            [0, -100, 995],
            core.current_revision(),
        ));
        assert!(matches!(
            stale.outcome,
            GameplayOutcome::Rejected {
                reason: RejectReason::InvalidState
            }
        ));
        assert_eq!(
            core.session(7).unwrap().gameplay.inventory[0]
                .unwrap()
                .item
                .count,
            1
        );
        assert_eq!(core.world().get_block(8, 81, 10), BlockType::Air);

        // The same typed path is valid across an explicitly loaded chunk
        // boundary; unloaded front/support chunks are never synthesized by
        // the action itself.
        core.world_mut_active().ensure_chunk(1, 0);
        core.session_mut(7).unwrap().position = [15.0, 80.0, 8.0];
        let support_cross = (16, 80, 8);
        let target_cross = (16, 81, 8);
        core.world_mut_active()
            .set_block(
                support_cross.0,
                support_cross.1,
                support_cross.2,
                BlockType::Stone,
                0,
            )
            .unwrap();
        core.world_mut_active()
            .set_block(
                target_cross.0,
                target_cross.1,
                target_cross.2,
                BlockType::Air,
                0,
            )
            .unwrap();
        let stone_one = crate::inventory::ItemStack::new(Item::Stone, 1);
        let stone_one_wire = crate::network::protocol::SessionSlotWire::new(
            crate::network::protocol::ItemWire::from_stack(&stone_one),
            0,
            0,
        );
        let mut cross_gameplay = core.session(7).unwrap().gameplay;
        cross_gameplay.inventory[0] = Some(SessionInventorySlot::from(stone_one_wire));
        core.set_session_gameplay(7, cross_gameplay);
        let cross = core.submit_request(block_request(
            116,
            3,
            BlockActionKind::Place,
            target_cross,
            Some(stone_one_wire),
            BlockType::Stone,
            [0, 1, 0],
            [750, -550, 250],
            core.current_revision(),
        ));
        assert!(matches!(cross.outcome, GameplayOutcome::Accepted { .. }));
        assert_eq!(
            core.world()
                .get_block(target_cross.0, target_cross.1, target_cross.2),
            BlockType::Stone
        );
        assert!(core.session(7).unwrap().gameplay.inventory[0].is_none());
    }

    #[test]
    fn typed_mining_cancels_on_cancel_held_change_range_or_block_replacement() {
        let target = (8, 81, 9);
        let pick = crate::inventory::ItemStack::new(Item::StonePickaxe, 1);
        let pick_wire = crate::network::protocol::SessionSlotWire::new(
            crate::network::protocol::ItemWire::from_stack(&pick),
            0,
            0,
        );
        let start = |core: &mut AuthorityCore, request_id: u128| {
            core.world_mut_active()
                .set_block(target.0, target.1, target.2, BlockType::Stone, 0)
                .unwrap();
            let mut gameplay = SessionGameplayState::default();
            gameplay.inventory[0] = Some(SessionInventorySlot::from(pick_wire));
            core.set_session_gameplay(7, gameplay);
            let response = core.submit_request(block_request(
                request_id,
                1,
                BlockActionKind::StartBreak,
                target,
                Some(pick_wire),
                BlockType::Air,
                [0, 0, -1],
                [0, -100, 995],
                0,
            ));
            assert!(matches!(response.outcome, GameplayOutcome::Accepted { .. }));
        };

        let mut cancelled = core();
        start(&mut cancelled, 120);
        let response = cancelled.submit_request(block_request(
            121,
            2,
            BlockActionKind::CancelBreak,
            target,
            None,
            BlockType::Air,
            [0, 0, 0],
            [0, -100, 995],
            cancelled.current_revision(),
        ));
        assert!(matches!(response.outcome, GameplayOutcome::Accepted { .. }));
        let _ = cancelled.tick();
        assert_eq!(
            cancelled.world().get_block(target.0, target.1, target.2),
            BlockType::Stone
        );

        let mut held_changed = core();
        start(&mut held_changed, 122);
        let mut changed = held_changed.session(7).unwrap().gameplay;
        let other = crate::inventory::ItemStack::new(Item::WoodenPickaxe, 1);
        changed.inventory[0] = Some(SessionInventorySlot::from(
            crate::network::protocol::SessionSlotWire::new(
                crate::network::protocol::ItemWire::from_stack(&other),
                0,
                0,
            ),
        ));
        held_changed.set_session_gameplay(7, changed);
        let _ = held_changed.tick();
        assert!(held_changed.session(7).unwrap().gameplay.mining.is_none());
        assert_eq!(
            held_changed.world().get_block(target.0, target.1, target.2),
            BlockType::Stone
        );

        let mut slot_switched = core();
        start(&mut slot_switched, 125);
        let mut switched = slot_switched.session(7).unwrap().gameplay;
        switched.selected_hotbar_slot = 1;
        switched.inventory[0] = Some(SessionInventorySlot::from(pick_wire));
        switched.inventory[1] = Some(SessionInventorySlot::from(pick_wire));
        slot_switched.set_session_gameplay(7, switched);
        let _ = slot_switched.tick();
        assert!(slot_switched.session(7).unwrap().gameplay.mining.is_none());
        assert_eq!(
            slot_switched
                .world()
                .get_block(target.0, target.1, target.2),
            BlockType::Stone
        );

        let mut moved = core();
        start(&mut moved, 123);
        moved.session_mut(7).unwrap().position = [30.0, 80.0, 30.0];
        let _ = moved.tick();
        assert!(moved.session(7).unwrap().gameplay.mining.is_none());
        assert_eq!(
            moved.world().get_block(target.0, target.1, target.2),
            BlockType::Stone
        );

        let mut replaced = core();
        start(&mut replaced, 124);
        replaced
            .world_mut_active()
            .set_block(target.0, target.1, target.2, BlockType::Dirt, 0)
            .unwrap();
        let _ = replaced.tick();
        assert!(replaced.session(7).unwrap().gameplay.mining.is_none());
        assert_eq!(
            replaced.world().get_block(target.0, target.1, target.2),
            BlockType::Dirt
        );
    }

    #[test]
    fn typed_adventure_place_requires_can_place_on_and_block_entity_projection() {
        let support = (8, 80, 9);
        let target = (8, 81, 9);
        let chest =
            crate::inventory::ItemStack::new(Item::Chest, 1).with_can_place_on(BlockType::Stone);
        let chest_wire = crate::network::protocol::SessionSlotWire::new(
            crate::network::protocol::ItemWire::from_stack(&chest),
            chest.can_break,
            chest.can_place_on,
        );
        let mut core = core();
        core.session_mut(7).unwrap().game_mode = crate::inventory::GameMode::Adventure;
        core.world_mut_active()
            .set_block(support.0, support.1, support.2, BlockType::Stone, 0)
            .unwrap();
        core.world_mut_active()
            .set_block(target.0, target.1, target.2, BlockType::Air, 0)
            .unwrap();
        let mut gameplay = SessionGameplayState::default();
        gameplay.inventory[0] = Some(SessionInventorySlot::from(chest_wire));
        core.set_session_gameplay(7, gameplay);
        let placed = core.submit_request(block_request(
            130,
            1,
            BlockActionKind::Place,
            target,
            Some(chest_wire),
            BlockType::Chest,
            [0, 1, 0],
            [250, -550, 750],
            0,
        ));
        assert!(matches!(placed.outcome, GameplayOutcome::Accepted { .. }));
        assert!(core
            .world()
            .get_block_entity(target.0, target.1, target.2)
            .is_some());
        assert!(core.session(7).unwrap().gameplay.inventory[0].is_none());

        let mut break_gameplay = SessionGameplayState::default();
        let break_chest =
            crate::inventory::ItemStack::new(Item::Chest, 1).with_can_break(BlockType::Chest);
        let break_wire = crate::network::protocol::SessionSlotWire::new(
            crate::network::protocol::ItemWire::from_stack(&break_chest),
            break_chest.can_break,
            break_chest.can_place_on,
        );
        break_gameplay.inventory[0] = Some(SessionInventorySlot::from(break_wire));
        core.set_session_gameplay(7, break_gameplay);
        let broken = core.submit_request(block_request(
            131,
            2,
            BlockActionKind::StartBreak,
            target,
            Some(break_wire),
            BlockType::Air,
            [0, 0, -1],
            [0, -100, 995],
            core.current_revision(),
        ));
        assert!(matches!(broken.outcome, GameplayOutcome::Accepted { .. }));
        for _ in 0..300 {
            let _ = core.tick();
        }
        assert_eq!(
            core.world().get_block(target.0, target.1, target.2),
            BlockType::Air
        );
        assert!(core
            .world()
            .get_block_entity(target.0, target.1, target.2)
            .is_none());
    }

    #[test]
    fn reconnect_resets_owner_private_mining_progress() {
        let target = (8, 81, 9);
        let pick = crate::inventory::ItemStack::new(Item::StonePickaxe, 1);
        let wire = crate::network::protocol::SessionSlotWire::new(
            crate::network::protocol::ItemWire::from_stack(&pick),
            0,
            0,
        );
        let mut core = core();
        core.world_mut_active()
            .set_block(target.0, target.1, target.2, BlockType::Stone, 0)
            .unwrap();
        let mut gameplay = SessionGameplayState::default();
        gameplay.inventory[0] = Some(SessionInventorySlot::from(wire));
        core.set_session_gameplay(7, gameplay);
        assert!(matches!(
            core.submit_request(block_request(
                140,
                1,
                BlockActionKind::StartBreak,
                target,
                Some(wire),
                BlockType::Air,
                [0, 0, -1],
                [0, -100, 995],
                0,
            ))
            .outcome,
            GameplayOutcome::Accepted { .. }
        ));
        assert!(core.session(7).unwrap().gameplay.mining.is_some());
        let _ = core.remove_session(7);
        core.register_session(SessionContract::new(
            7,
            "alex",
            0,
            [8.0, 80.0, 8.0],
            true,
            true,
        ))
        .unwrap();
        assert!(core.session(7).unwrap().gameplay.mining.is_none());
    }

    #[test]
    fn typed_mining_grants_xp_once_and_removes_broken_tool_without_orb() {
        let target = (8, 81, 9);
        let mut pick = crate::inventory::ItemStack::new(Item::DiamondPickaxe, 1);
        pick.durability = 1;
        let wire = crate::network::protocol::SessionSlotWire::new(
            crate::network::protocol::ItemWire::from_stack(&pick),
            0x55,
            0xaa,
        );
        let mut core = core();
        core.world_mut_active()
            .set_block(target.0, target.1, target.2, BlockType::DiamondOre, 0)
            .unwrap();
        let mut gameplay = SessionGameplayState::default();
        gameplay.experience = 6;
        gameplay.inventory[0] = Some(SessionInventorySlot::from(wire));
        core.set_session_gameplay(7, gameplay);
        assert!(matches!(
            core.submit_request(block_request(
                150,
                1,
                BlockActionKind::StartBreak,
                target,
                Some(wire),
                BlockType::Air,
                [0, 0, -1],
                [0, -100, 995],
                0,
            ))
            .outcome,
            GameplayOutcome::Accepted { .. }
        ));
        for _ in 0..80 {
            let _ = core.tick();
        }
        assert_eq!(
            core.world().get_block(target.0, target.1, target.2),
            BlockType::Air
        );
        assert_eq!(core.session(7).unwrap().gameplay.experience, 4);
        assert_eq!(core.session(7).unwrap().gameplay.experience_level, 1);
        assert!(core.session(7).unwrap().gameplay.inventory[0].is_none());
        assert!(core
            .world()
            .entities
            .entities
            .iter()
            .all(|entity| entity.entity_type != EntityType::ExperienceOrb));
        let dropped = core
            .world()
            .entities
            .entities
            .iter()
            .find(|entity| entity.entity_type == EntityType::DroppedItem)
            .expect("diamond ore must produce one authoritative drop");
        assert_eq!(dropped.dropped_item, Some(Item::Diamond));
        assert_eq!(dropped.dropped_count, 1);
    }

    #[test]
    fn authoritative_dispenser_edge_executes_once_with_global_entity_id() {
        let mut core = core();
        let lever = (7, 80, 8);
        let source = (8, 80, 8);
        core.world_mut_active()
            .set_block(lever.0, lever.1, lever.2, BlockType::LeverOn, 0)
            .unwrap();
        core.world_mut_active()
            .set_block(source.0, source.1, source.2, BlockType::Dispenser, 0)
            .unwrap();
        {
            let world = core.world_mut_active();
            world
                .redstone
                .on_block_changed(&world.chunks, lever, crate::redstone::Direction::East);
        }
        {
            let world = core.world_mut_active();
            world.redstone.on_block_changed(
                &world.chunks,
                source,
                crate::redstone::Direction::South,
            );
        }
        if let Some(entity) = core
            .world_mut_active()
            .chunks
            .get_block_entity_mut(source.0, source.1, source.2)
        {
            entity.set_stack(0, Some(crate::inventory::ItemStack::new(Item::Arrow, 2)));
        }

        let first = core.tick();
        assert_eq!(
            core.world_mut_active()
                .entities
                .entities
                .iter()
                .filter(|entity| entity.entity_type == EntityType::Arrow)
                .count(),
            1
        );
        let arrow_id = core
            .world()
            .entities
            .entities
            .iter()
            .find(|entity| entity.entity_type == EntityType::Arrow)
            .unwrap()
            .id;
        assert!(arrow_id >= AUTHORITY_ENTITY_ID_START);
        assert!(first
            .mutations
            .iter()
            .any(|mutation| mutation.position == source));

        let sustained = core.tick();
        assert!(sustained
            .mutations
            .iter()
            .all(|mutation| mutation.position != source));
        assert_eq!(
            core.world_mut_active()
                .entities
                .entities
                .iter()
                .filter(|entity| entity.entity_type == EntityType::Arrow)
                .count(),
            1
        );

        core.world_mut_active()
            .set_block(lever.0, lever.1, lever.2, BlockType::Lever, 0)
            .unwrap();
        {
            let world = core.world_mut_active();
            world
                .redstone
                .on_block_changed(&world.chunks, lever, crate::redstone::Direction::East);
        }
        let _ = core.tick();
        core.world_mut_active()
            .set_block(lever.0, lever.1, lever.2, BlockType::LeverOn, 0)
            .unwrap();
        {
            let world = core.world_mut_active();
            world
                .redstone
                .on_block_changed(&world.chunks, lever, crate::redstone::Direction::East);
        }
        let _ = core.tick();
        assert_eq!(
            core.world_mut_active()
                .entities
                .entities
                .iter()
                .filter(|entity| entity.entity_type == EntityType::Arrow)
                .count(),
            2
        );
    }

    #[test]
    fn item_use_mutates_session_inventory_and_revision() {
        let mut core = core();
        let mut gameplay = SessionGameplayState::default();
        gameplay.hunger_milli = 10_000;
        let mut wire = crate::network::protocol::ItemWire::empty();
        wire.item = Item::Bread as u32;
        wire.count = 2;
        gameplay.inventory[0] = Some(SessionInventorySlot::from_wire(wire, 0, 0));
        core.set_session_gameplay(7, gameplay);
        let response = core.submit_request(GameplayRequest {
            request_id: 30,
            client_sequence: 1,
            session_id: 7,
            dimension: 0,
            client_revision: 0,
            operation: GameplayOperation::ItemUse {
                item: Item::Bread as u32,
                count: 1,
            },
        });
        assert!(matches!(response.outcome, GameplayOutcome::Accepted { .. }));
        let state = core.session(7).unwrap().gameplay;
        assert_eq!(state.count_item(Item::Bread as u32), 1);
        assert!(state.hunger_milli > 10_000);
        assert!(state.revision > 0);
    }

    #[test]
    fn unsupported_tool_item_use_does_not_consume_inventory() {
        let mut core = core();
        let mut gameplay = SessionGameplayState::default();
        let mut wire = crate::network::protocol::ItemWire::empty();
        wire.item = Item::DiamondSword as u32;
        wire.count = 1;
        gameplay.inventory[0] = Some(SessionInventorySlot::from_wire(wire, 0, 0));
        core.set_session_gameplay(7, gameplay);
        let response = core.submit_request(GameplayRequest {
            request_id: 34,
            client_sequence: 1,
            session_id: 7,
            dimension: 0,
            client_revision: 0,
            operation: GameplayOperation::ItemUse {
                item: Item::DiamondSword as u32,
                count: 1,
            },
        });
        assert!(matches!(
            response.outcome,
            GameplayOutcome::Rejected {
                reason: RejectReason::InvalidState
            }
        ));
        assert_eq!(
            core.session(7)
                .unwrap()
                .gameplay
                .count_item(Item::DiamondSword as u32),
            1
        );
        assert_eq!(
            core.session(7).unwrap().last_client_sequence,
            0,
            "non-food ItemUse must fail in validate_bounds before sequencing"
        );
    }

    #[test]
    fn console_only_command_rejects_before_sequencing() {
        let mut core = core();
        let response = core.submit_request(GameplayRequest {
            request_id: 41,
            client_sequence: 1,
            session_id: 7,
            dimension: 0,
            client_revision: 0,
            operation: GameplayOperation::Command {
                command: "/help".to_string(),
            },
        });
        assert!(matches!(
            response.outcome,
            GameplayOutcome::Rejected {
                reason: RejectReason::Unsupported
            }
        ));
        assert_eq!(core.session(7).unwrap().last_client_sequence, 0);
    }

    #[test]
    fn client_cannot_submit_self_damage() {
        let mut core = core();
        let before = core.session(7).unwrap().gameplay;
        let response = core.submit_request(GameplayRequest {
            request_id: 35,
            client_sequence: 1,
            session_id: 7,
            dimension: 0,
            client_revision: 0,
            operation: GameplayOperation::Combat {
                target: 0,
                action: 0x80 | 127,
            },
        });
        assert!(matches!(
            response.outcome,
            GameplayOutcome::Rejected {
                reason: RejectReason::InvalidState
            }
        ));
        assert_eq!(core.session(7).unwrap().gameplay, before);
    }

    #[test]
    fn respawn_command_restores_authority_health_after_death() {
        let mut core = core();
        let mut dead = core.session(7).unwrap().gameplay;
        dead.health_milli = 0;
        dead.is_dead = true;
        dead.death_source = Some(crate::player::DamageSource::Mob.to_wire());
        assert!(core.set_session_gameplay(7, dead));
        assert!(core.session(7).unwrap().gameplay.is_dead);
        let response = core.submit_request(GameplayRequest {
            request_id: 40,
            client_sequence: 1,
            session_id: 7,
            dimension: 0,
            client_revision: core.current_revision(),
            operation: GameplayOperation::Command {
                command: "/respawn".to_string(),
            },
        });
        assert!(matches!(response.outcome, GameplayOutcome::Accepted { .. }));
        let state = core.session(7).unwrap().gameplay;
        assert!(!state.is_dead);
        assert_eq!(state.health_milli, state.max_health_milli);
    }

    #[test]
    fn respawn_session_rejects_living_player() {
        let mut core = core();
        let before = core.session(7).unwrap().clone();
        assert!(!before.gameplay.is_dead);
        assert!(!core.respawn_session(7));
        let after = core.session(7).unwrap();
        assert_eq!(after.position, before.position);
        assert_eq!(after.dimension, before.dimension);
        assert_eq!(after.gameplay, before.gameplay);
    }

    #[test]
    fn dimension_transfer_updates_session_and_world_contract() {
        let mut core = AuthorityCore::new(AuthorityConfig::default());
        let _ = core.register_session(SessionContract::new(
            7,
            "alex",
            0,
            [8.0, 80.0, 8.0],
            true,
            true,
        ));
        assert!(core.set_session_dimension(7, crate::dimension::Dimension::Nether));
        assert_eq!(core.session(7).unwrap().dimension, 1);
        assert_eq!(core.active_dimension(), crate::dimension::Dimension::Nether);
        let nether = core
            .world_ref(crate::dimension::Dimension::Nether)
            .expect("nether world stays in the map");
        assert_eq!(nether.dimension, crate::dimension::Dimension::Nether);
        assert_eq!(nether.chunks.dimension, crate::dimension::Dimension::Nether);
        assert_eq!(
            core.world_mut_active().dimension,
            crate::dimension::Dimension::Nether
        );
    }

    #[test]
    fn session_dimension_index_tracks_register_move_and_remove() {
        let mut core = AuthorityCore::new(AuthorityConfig::default());
        core.register_session(SessionContract::new(
            7,
            "alex",
            Dimension::Overworld as u8,
            [8.0, 80.0, 8.0],
            true,
            true,
        ))
        .unwrap();
        core.register_session(SessionContract::new(
            8,
            "sam",
            Dimension::Nether as u8,
            [8.0, 80.0, 8.0],
            true,
            true,
        ))
        .unwrap();
        assert_eq!(
            core.session_ids_in_dimension(Dimension::Overworld),
            &[7]
        );
        assert_eq!(core.session_ids_in_dimension(Dimension::Nether), &[8]);

        assert!(core.set_session_dimension(7, Dimension::Nether));
        assert!(core.session_ids_in_dimension(Dimension::Overworld).is_empty());
        assert_eq!(core.session_ids_in_dimension(Dimension::Nether), &[7, 8]);

        assert!(core.set_session_dimension(7, Dimension::Nether));
        assert_eq!(core.session_ids_in_dimension(Dimension::Nether), &[7, 8]);

        assert!(core.remove_session(8).is_some());
        assert_eq!(core.session_ids_in_dimension(Dimension::Nether), &[7]);
        assert!(core.session_ids_in_dimension(Dimension::End).is_empty());
    }

    #[test]
    fn session_updates_publish_join_and_dimension_change_but_not_idle_ticks() {
        let mut core = AuthorityCore::new(AuthorityConfig::default());
        core.register_session(SessionContract::new(
            7,
            "alex",
            Dimension::Overworld as u8,
            [8.0, 80.0, 8.0],
            true,
            true,
        ))
        .unwrap();

        let joined = core.tick();
        assert!(joined.session_updates.iter().any(|update| {
            update.player_id == 7 && update.dimension == Dimension::Overworld as u8
        }));
        assert_eq!(joined.session_updates.len(), core.last_snapshot.session_updates.len());

        let idle = core.tick();
        assert!(idle.session_updates.is_empty());
        assert!(core.last_snapshot.session_updates.is_empty());

        assert!(core.set_session_dimension(7, Dimension::Nether));
        let transferred = core.tick();
        assert!(transferred.session_updates.iter().any(|update| {
            update.player_id == 7 && update.dimension == Dimension::Nether as u8
        }));
        let idle_after_transfer = core.tick();
        assert!(idle_after_transfer.session_updates.is_empty());
    }

    #[test]
    fn mining_brew_and_fishing_revision_bumps_publish_dirty_session_updates() {
        let mut core = core();
        let _ = core.tick();
        assert!(core.tick().session_updates.is_empty());

        let target = (8, 81, 9);
        core.world_mut_active()
            .set_block(target.0, target.1, target.2, BlockType::Stone, 0)
            .unwrap();
        let held_stack = crate::inventory::ItemStack::new(Item::StonePickaxe, 1);
        let held = crate::network::protocol::SessionSlotWire::new(
            crate::network::protocol::ItemWire::from_stack(&held_stack),
            0,
            0,
        );
        let mut mining_gameplay = SessionGameplayState::default();
        mining_gameplay.inventory[0] = Some(SessionInventorySlot::from(held));
        core.set_session_gameplay(7, mining_gameplay);
        assert!(matches!(
            core.submit_request(GameplayRequest {
                request_id: 200,
                client_sequence: 1,
                session_id: 7,
                dimension: 0,
                client_revision: 0,
                operation: GameplayOperation::BlockAction {
                    action: BlockActionKind::StartBreak,
                    x: target.0,
                    y: target.1,
                    z: target.2,
                    face: [0, 0, -1],
                    hand: 0,
                    held: Some(held),
                    block: BlockType::Air.to_wire(),
                    look_milli: [0, -100, 995],
                },
            })
            .outcome,
            GameplayOutcome::Accepted { .. }
        ));
        let mining_tick = core.tick();
        let mining_update = mining_tick
            .session_updates
            .iter()
            .find(|update| update.player_id == 7)
            .expect("mining revision bump must publish");
        assert!(mining_update.state.mining.is_some());
        assert!(mining_update.state.revision > 0);

        core.world_mut_active()
            .set_block(8, 80, 8, BlockType::BrewingStand, 0)
            .unwrap();
        let mut brew_gameplay = core.session(7).unwrap().gameplay;
        brew_gameplay.mining = None;
        brew_gameplay.brew = Some(SessionBrewState {
            station: [8, 80, 8],
            ingredient: SlotRefWire {
                index: 0,
                count: 1,
                expected: held,
            },
            bottles: [None; 3],
            remaining_ticks: 4,
        });
        assert!(core.set_session_gameplay(7, brew_gameplay));
        let brew_tick = core.tick();
        let brew_update = brew_tick
            .session_updates
            .iter()
            .find(|update| update.player_id == 7)
            .expect("brew revision bump must publish");
        assert_eq!(
            brew_update.state.brew.map(|brew| brew.remaining_ticks),
            Some(3)
        );

        let _ = core.tick();
        let mut fishing_gameplay = core.session(7).unwrap().gameplay;
        fishing_gameplay.brew = None;
        fishing_gameplay.fishing_hook = Some(SessionFishingHookState {
            entity_id: super::AUTHORITY_ENTITY_ID_START,
            position_milli: [8_000, 80_000, 8_000],
            velocity_milli: [0, 0, 0],
            stage: 1,
            wait_ticks_remaining: 8,
            bite_ticks_remaining: 0,
        });
        assert!(core.set_session_gameplay(7, fishing_gameplay));
        let fishing_tick = core.tick();
        assert!(
            fishing_tick
                .session_updates
                .iter()
                .any(|update| update.player_id == 7),
            "fishing tick must publish a dirty session update"
        );
    }

    #[test]
    fn dimension_worlds_are_parked_without_chunk_aliasing() {
        let mut core = AuthorityCore::new(AuthorityConfig::default());
        let _ = core.register_session(SessionContract::new(
            7,
            "alex",
            0,
            [8.0, 80.0, 8.0],
            true,
            true,
        ));
        let marker = BlockType::Glass;
        core.world_mut(crate::dimension::Dimension::Overworld)
            .expect("overworld world")
            .set_block(1_234, 100, -2_345, marker, 0)
            .unwrap();
        assert_eq!(
            core.world_ref(crate::dimension::Dimension::Overworld)
                .expect("overworld world")
                .get_block(1_234, 100, -2_345),
            marker
        );

        assert!(core.set_session_dimension(7, crate::dimension::Dimension::Nether));
        assert_eq!(core.active_dimension(), crate::dimension::Dimension::Nether);
        assert_ne!(core.world().get_block(1_234, 100, -2_345), marker);
        assert!(core.world().valid_coordinate(1_234, 127, -2_345));
        assert!(!core.world().valid_coordinate(1_234, 128, -2_345));
        assert_eq!(
            core.world_ref(crate::dimension::Dimension::Overworld)
                .expect("overworld remains in the map")
                .get_block(1_234, 100, -2_345),
            marker
        );
        if let Some(session) = core.session_mut(7) {
            session.position = [154.25, 67.0, -293.5];
        }
        assert_eq!(core.session(7).unwrap().position, [154.25, 67.0, -293.5]);

        assert!(core.set_session_dimension(7, crate::dimension::Dimension::Overworld));
        assert_eq!(
            core.active_dimension(),
            crate::dimension::Dimension::Overworld
        );
        assert_eq!(
            core.world_ref(crate::dimension::Dimension::Overworld)
                .expect("overworld world")
                .get_block(1_234, 100, -2_345),
            marker
        );
        assert_eq!(core.session(7).unwrap().dimension, 0);
    }

    #[test]
    fn sessions_in_multiple_dimensions_tick_and_dispatch_independently() {
        let mut core = AuthorityCore::new(AuthorityConfig::default());
        core.register_session(SessionContract::new(
            7,
            "alex",
            Dimension::Overworld as u8,
            [8.0, 80.0, 8.0],
            true,
            true,
        ))
        .unwrap();
        core.register_session(SessionContract::new(
            8,
            "sam",
            Dimension::Nether as u8,
            [8.0, 80.0, 8.0],
            true,
            true,
        ))
        .unwrap();

        core.with_world(Dimension::Overworld, |world| {
            world
                .set_block(8, 80, 8, BlockType::Glass, 0)
                .expect("seed overworld glass");
        });
        core.with_world(Dimension::Nether, |world| {
            world
                .set_block(8, 80, 8, BlockType::Obsidian, 0)
                .expect("seed nether obsidian");
        });
        core.activate_dimension(Dimension::Overworld);
        let snapshot = core.tick();
        assert_eq!(snapshot.tick, 1);
        assert_eq!(core.world_mut_active().dimension, Dimension::Overworld);
        assert!(snapshot
            .session_updates
            .iter()
            .any(|update| update.player_id == 7 && update.dimension == Dimension::Overworld as u8));
        assert!(snapshot
            .session_updates
            .iter()
            .any(|update| update.player_id == 8 && update.dimension == Dimension::Nether as u8));

        core.activate_dimension(Dimension::Overworld);
        assert_eq!(core.world_mut_active().time, 1);
        assert_eq!(core.world().get_block(8, 80, 8), BlockType::Glass);
        let overworld_revision = core.revision_for_dimension(Dimension::Overworld);
        core.activate_dimension(Dimension::Nether);
        assert_eq!(core.world_mut_active().time, 1);
        assert_eq!(core.world().get_block(8, 80, 8), BlockType::Obsidian);
        let nether_revision = core.revision_for_dimension(Dimension::Nether);
        assert_eq!(overworld_revision, nether_revision);

        let leftover = core.submit_request(GameplayRequest {
            request_id: 101,
            client_sequence: 1,
            session_id: 7,
            dimension: Dimension::Overworld as u8,
            client_revision: overworld_revision,
            operation: GameplayOperation::BlockAction {
                action: BlockActionKind::Place,
                x: 8,
                y: 80,
                z: 8,
                face: [0, 1, 0],
                hand: 0,
                held: None,
                block: BlockType::DiamondOre.to_wire(),
                look_milli: [0, 0, 1000],
            },
        });
        assert!(matches!(
            leftover.outcome,
            GameplayOutcome::Rejected {
                reason: RejectReason::InvalidState
            }
        ));
        core.activate_dimension(Dimension::Overworld);
        assert_eq!(core.world().get_block(8, 80, 8), BlockType::Glass);

        // An active Nether compatibility view must not make a rejected
        // Overworld BlockAction mutate; routing still selects the session world.
        let routed_again = core.submit_request(GameplayRequest {
            request_id: 103,
            client_sequence: 2,
            session_id: 7,
            dimension: Dimension::Overworld as u8,
            client_revision: overworld_revision,
            operation: GameplayOperation::BlockAction {
                action: BlockActionKind::Place,
                x: 9,
                y: 80,
                z: 8,
                face: [0, 1, 0],
                hand: 0,
                held: None,
                block: BlockType::Glass.to_wire(),
                look_milli: [0, 0, 1000],
            },
        });
        assert!(matches!(
            routed_again.outcome,
            GameplayOutcome::Rejected {
                reason: RejectReason::InvalidState
            }
        ));
        core.activate_dimension(Dimension::Overworld);
        assert_eq!(core.world().get_block(8, 80, 8), BlockType::Glass);
        assert_ne!(core.world().get_block(9, 80, 8), BlockType::Glass);
    }

    #[test]
    fn authority_boundary_does_not_reingest_presentation_inventory() {
        let mut core = core();
        let mut gameplay = SessionGameplayState::default();
        let mut wire = crate::network::protocol::ItemWire::empty();
        wire.item = Item::Bread as u32;
        wire.count = 2;
        gameplay.hunger_milli = 10_000;
        gameplay.inventory[0] = Some(SessionInventorySlot::from_wire(wire, 0, 0));
        core.set_session_gameplay(7, gameplay);
        let response = core.submit_request(GameplayRequest {
            request_id: 37,
            client_sequence: 1,
            session_id: 7,
            dimension: 0,
            client_revision: 0,
            operation: GameplayOperation::ItemUse {
                item: Item::Bread as u32,
                count: 1,
            },
        });
        assert!(matches!(response.outcome, GameplayOutcome::Accepted { .. }));
        assert_eq!(
            core.session(7)
                .unwrap()
                .gameplay
                .count_item(Item::Bread as u32),
            1
        );
    }

    #[test]
    fn combat_mutates_headless_entity_without_state_fallback() {
        let mut core = core();
        let mut attacker = core.session(7).unwrap().gameplay;
        attacker.attack_cooldown_ticks = ATTACK_COOLDOWN_TICKS;
        assert!(core.set_session_gameplay(7, attacker));
        let target = core
            .world_mut_active()
            .entities
            .spawn(EntityType::Zombie, glam::Vec3::new(8.0, 80.0, 9.0));
        let before = core
            .world_mut_active()
            .entities
            .get_by_id(target)
            .unwrap()
            .health;
        let response = core.submit_request(GameplayRequest {
            request_id: 31,
            client_sequence: 1,
            session_id: 7,
            dimension: 0,
            client_revision: 0,
            operation: GameplayOperation::Combat { target, action: 0 },
        });
        assert!(
            matches!(response.outcome, GameplayOutcome::Accepted { .. }),
            "unexpected combat response: {response:?}"
        );
        assert!(
            core.world_mut_active()
                .entities
                .get_by_id(target)
                .unwrap()
                .health
                < before
        );
    }

    #[test]
    fn trade_conserves_items_and_mount_projects_session_state() {
        let mut core = core();
        let villager = 900;
        let mut sell = crate::inventory::ItemStack::new(Item::Emerald, 1);
        sell.durability = 9;
        sell.enchantments
            .add_or_upgrade(crate::enchantment::Enchantment::Fortune(2));
        sell.custom_name.set("trade emerald");
        sell.can_break = 0x11;
        sell.can_place_on = 0x22;
        let offers = vec![crate::village::trade::TradeOffer::new(
            crate::inventory::ItemStack::new(Item::Wheat, 2),
            Some(crate::inventory::ItemStack::new(Item::Carrot, 1)),
            sell,
            4,
            1,
        )];
        assert!(core.world_mut_active().ensure_villager(
            villager,
            [9.0, 80.0, 8.0],
            crate::village::poi::VillagerProfession::Farmer,
            crate::village::trade::VillagerLevel::Novice,
            offers,
        ));
        let mut gameplay = SessionGameplayState::default();
        let mut wheat = crate::network::protocol::ItemWire::empty();
        wheat.item = Item::Wheat as u32;
        wheat.count = 2;
        gameplay.inventory[0] = Some(SessionInventorySlot::from_wire(wheat, 0, 0));
        let mut carrot = crate::network::protocol::ItemWire::empty();
        carrot.item = Item::Carrot as u32;
        carrot.count = 1;
        gameplay.inventory[1] = Some(SessionInventorySlot::from_wire(carrot, 0, 0));
        core.set_session_gameplay(7, gameplay);
        let response = core.submit_request(GameplayRequest {
            request_id: 32,
            client_sequence: 1,
            session_id: 7,
            dimension: 0,
            client_revision: 0,
            operation: GameplayOperation::Trade {
                villager_id: villager,
                offer_index: 0,
            },
        });
        assert!(matches!(response.outcome, GameplayOutcome::Accepted { .. }));
        let state = core.session(7).unwrap().gameplay;
        assert_eq!(state.count_item(Item::Wheat as u32), 0);
        assert_eq!(state.count_item(Item::Carrot as u32), 0);
        assert_eq!(state.count_item(Item::Emerald as u32), 1);
        let emerald = state
            .inventory
            .iter()
            .flatten()
            .find(|slot| slot.item.item == Item::Emerald as u32)
            .unwrap();
        assert_eq!(emerald.item.durability, 9);
        let expected_name = sell.custom_name.as_str().as_bytes();
        assert_eq!(
            &emerald.item.custom_name[..expected_name.len()],
            expected_name
        );
        assert_eq!(emerald.can_break, 0x11);
        assert_eq!(emerald.can_place_on, 0x22);

        let vehicle = 901;
        assert!(core.world_mut_active().ensure_vehicle(
            vehicle,
            EntityType::Boat,
            [9.0, 80.0, 8.0],
        ));
        let response = core.submit_request(GameplayRequest {
            request_id: 33,
            client_sequence: 2,
            session_id: 7,
            dimension: 0,
            client_revision: core.current_revision(),
            operation: GameplayOperation::Mount { entity_id: vehicle },
        });
        assert!(matches!(response.outcome, GameplayOutcome::Accepted { .. }));
        assert_eq!(
            core.session(7).unwrap().gameplay.mounted_entity,
            Some(vehicle)
        );
        assert!(core
            .world_mut_active()
            .entities
            .get_by_id(vehicle)
            .unwrap()
            .passengers
            .contains(&7));
    }
}
