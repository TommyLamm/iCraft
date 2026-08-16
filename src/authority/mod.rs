//! GPU-independent authoritative simulation.

pub mod combat;
pub mod contract;
pub mod fishing;
pub mod interest;
pub mod mining;
pub mod transactions;

use crate::block_entity::BlockEntity;
use crate::dimension::Dimension;
use crate::game_rules::{ServerDifficulty, WorldRules, WorldType};
use crate::inventory::ItemStack;
use crate::network::protocol::{
    BlockActionKind, GameplayOperation, GameplayOutcome, GameplayRequest, GameplayResponse,
    ItemWire, PlayerId, RejectReason, SessionSlotWire,
};
use crate::server_world::{ServerWorld, FIXED_DT};
use contract::{
    AuthoritySnapshot, AuthorityTopology, MiningProgressState, SessionContract,
    SessionGameplayState, SessionGameplayUpdate, SessionInventorySlot, WorldMutation,
};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

const AUTHORITY_ENTITY_ID_START: u64 = 1 << 63;
const ATTACK_COOLDOWN_TICKS: u16 = 5;

pub use contract::{
    common_gameplay_vectors, RevisionClock, AUTHORITY_CONTRACT_VERSION, FIXED_TICK_HZ,
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
    pub difficulty: ServerDifficulty,
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
            difficulty: ServerDifficulty::default(),
            render_distance: 8,
        }
    }
}

/// Owns sessions, request sequencing and the headless world.  Transport code
/// only registers sessions, submits envelopes and consumes snapshots.
pub struct AuthorityCore {
    pub topology: AuthorityTopology,
    pub world: ServerWorld,
    /// Immutable world-creation inputs used when a dimension has not been
    /// visited yet.  Each dimension then owns an independent parked world so
    /// switching cannot reinterpret one dimension's chunks as another's.
    config: AuthorityConfig,
    /// All non-active dimensions.  The active compatibility world above is
    /// moved in and out of this map for each routed operation/tick, so no
    /// dimension has a shadow or aliased chunk/entity collection.
    worlds: BTreeMap<Dimension, ServerWorld>,
    sessions: BTreeMap<PlayerId, SessionContract>,
    last_snapshot: AuthoritySnapshot,
    fixed_tick: u64,
    /// Mutations emitted between fixed ticks (for example an authenticated
    /// player request).  Presentation roots drain these through the same
    /// snapshot projection as tick-driven automation.
    pending_mutations: Vec<WorldMutation>,
    /// Session ids changed as a side effect of another player's request. They
    /// receive the request's single authoritative revision at publication.
    pending_session_revisions: BTreeSet<PlayerId>,
    pending_dimension_transfers: Vec<DimensionTransferIntent>,
    /// High-bit ids are reserved for authority-created hooks, drops and XP.
    /// The allocator is shared by every loaded dimension, unlike each world's
    /// legacy EntityManager allocator.
    next_authority_entity_id: u64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DimensionTransferIntent {
    pub player_id: PlayerId,
    pub from: Dimension,
    pub to: Dimension,
    pub position: [f32; 3],
}

/// Presentation roots use this small in-process boundary for Singleplayer and
/// Host.  It keeps the exact same `AuthorityCore` request/tick path as the
/// dedicated binary while exposing no transport details to `State`.
pub struct AuthorityBoundary {
    pub topology: AuthorityTopology,
    pub core: AuthorityCore,
    pub session_id: PlayerId,
}

impl AuthorityBoundary {
    pub fn new(
        config: AuthorityConfig,
        topology: AuthorityTopology,
        session_id: PlayerId,
        username: impl Into<String>,
        position: [f32; 3],
        operator: bool,
        cheats_enabled: bool,
    ) -> Self {
        let mut core = AuthorityCore::new(config, topology);
        let _ = core.register_session(SessionContract::new(
            session_id,
            username,
            config.dimension as u8,
            position,
            operator,
            cheats_enabled,
        ));
        Self {
            topology,
            core,
            session_id,
        }
    }

    pub fn tick(&mut self) -> AuthoritySnapshot {
        self.core.tick()
    }

    pub fn submit(&mut self, mut request: GameplayRequest) -> GameplayResponse {
        request.session_id = self.session_id;
        self.core.submit_request(request)
    }

    pub fn submit_for_session(
        &mut self,
        session_id: PlayerId,
        mut request: GameplayRequest,
    ) -> GameplayResponse {
        request.session_id = session_id;
        self.core.submit_request(request)
    }

    pub fn register_session(
        &mut self,
        id: PlayerId,
        username: impl Into<String>,
        dimension: u8,
        position: [f32; 3],
        operator: bool,
        cheats_enabled: bool,
    ) {
        if let Some(session) = self.core.session_mut(id) {
            session.username = username.into();
            session.dimension = dimension;
            session.position = position;
            session.operator = operator;
            session.cheats_enabled = cheats_enabled;
            return;
        }
        let _ = self.core.register_session(SessionContract::new(
            id,
            username,
            dimension,
            position,
            operator,
            cheats_enabled,
        ));
    }

    pub fn set_session_position(&mut self, id: PlayerId, position: [f32; 3]) {
        if let Some(session) = self.core.session_mut(id) {
            session.position = position;
        }
    }

    pub fn set_position(&mut self, position: [f32; 3]) {
        if let Some(session) = self.core.session_mut(self.session_id) {
            session.position = position;
        }
    }

    /// Atomically update the in-process session's dimension.  Presentation
    /// roots use this seam before rebuilding their local render cache so a
    /// portal transfer cannot leave the authority session in the old world.
    pub fn set_dimension(&mut self, dimension: u8) -> bool {
        let Some(
            crate::dimension::Dimension::Overworld
            | crate::dimension::Dimension::Nether
            | crate::dimension::Dimension::End,
        ) = crate::dimension::Dimension::from_wire(dimension)
        else {
            return false;
        };
        let target = crate::dimension::Dimension::from_wire(dimension).expect("validated above");
        self.core.set_session_dimension(self.session_id, target)
    }

    pub fn set_session_dimension(&mut self, id: PlayerId, dimension: u8) -> bool {
        let Some(
            crate::dimension::Dimension::Overworld
            | crate::dimension::Dimension::Nether
            | crate::dimension::Dimension::End,
        ) = crate::dimension::Dimension::from_wire(dimension)
        else {
            return false;
        };
        let target = crate::dimension::Dimension::from_wire(dimension).expect("validated above");
        self.core.set_session_dimension(id, target)
    }

    pub fn session_gameplay(&self, id: PlayerId) -> Option<SessionGameplayUpdate> {
        self.core.session_gameplay(id)
    }

    pub fn set_session_gameplay(&mut self, id: PlayerId, gameplay: SessionGameplayState) -> bool {
        self.core.set_session_gameplay(id, gameplay)
    }

    pub fn sync_villager(
        &mut self,
        villager_id: u64,
        position: [f32; 3],
        profession: crate::village::poi::VillagerProfession,
        level: crate::village::trade::VillagerLevel,
        offers: Vec<crate::village::trade::TradeOffer>,
    ) -> bool {
        self.core
            .world
            .ensure_villager(villager_id, position, profession, level, offers)
    }

    pub fn sync_vehicle(
        &mut self,
        vehicle_id: u64,
        entity_type: crate::entity::EntityType,
        position: [f32; 3],
    ) -> bool {
        self.core
            .world
            .ensure_vehicle(vehicle_id, entity_type, position)
    }

    pub fn sync_entity(
        &mut self,
        entity_id: u64,
        entity_type: crate::entity::EntityType,
        position: [f32; 3],
        health: f32,
    ) -> bool {
        self.core
            .world
            .ensure_entity(entity_id, entity_type, position, health)
    }

    pub fn set_game_mode(&mut self, game_mode: crate::inventory::GameMode) {
        if let Some(session) = self.core.session_mut(self.session_id) {
            session.game_mode = game_mode;
        }
    }

    pub fn set_rules(&mut self, rules: WorldRules) {
        self.core.set_rules(rules);
    }

    pub fn take_pending_mutations(&mut self) -> Vec<WorldMutation> {
        self.core.take_pending_mutations()
    }

    pub fn block_entity_at(
        &self,
        position: (i32, i32, i32),
    ) -> Option<crate::block_entity::BlockEntity> {
        self.core
            .world
            .get_block_entity(position.0, position.1, position.2)
            .cloned()
    }

    pub fn seed_bonus_chest(&mut self, position: (i32, i32, i32)) {
        if let Some(mutation) = self.core.world.place_bonus_chest(position) {
            self.core.pending_mutations.push(mutation);
        }
    }
}

impl AuthorityCore {
    pub fn new(config: AuthorityConfig, topology: AuthorityTopology) -> Self {
        Self {
            topology,
            world: Self::new_world(config, config.dimension),
            config,
            worlds: BTreeMap::new(),
            sessions: BTreeMap::new(),
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

    fn ensure_dimension(&mut self, target: Dimension) {
        if self.world.dimension == target || self.worlds.contains_key(&target) {
            return;
        }
        self.worlds
            .insert(target, Self::new_world(self.config, target));
    }

    /// Move the compatibility `world` view to a dimension without aliasing
    /// chunk/entity state.  The previous active world is retained in the map.
    pub fn activate_dimension(&mut self, target: Dimension) {
        if self.world.dimension == target {
            return;
        }
        let current = self.world.dimension;
        let next = self
            .worlds
            .remove(&target)
            .unwrap_or_else(|| Self::new_world(self.config, target));
        let previous = std::mem::replace(&mut self.world, next);
        self.worlds.insert(current, previous);
    }

    pub fn active_dimension(&self) -> Dimension {
        self.world.dimension
    }

    /// Read a dimension without changing the active compatibility view.
    pub fn world_ref(&self, dimension: Dimension) -> Option<&ServerWorld> {
        if self.world.dimension == dimension {
            Some(&self.world)
        } else {
            self.worlds.get(&dimension)
        }
    }

    /// Mutably access a loaded dimension without changing the active view.
    /// Callers that need to create a missing dimension should use
    /// `with_world`, which restores the previous active view automatically.
    pub fn world_mut(&mut self, dimension: Dimension) -> Option<&mut ServerWorld> {
        if self.world.dimension == dimension {
            Some(&mut self.world)
        } else {
            self.worlds.get_mut(&dimension)
        }
    }

    /// Execute a bounded operation against one dimension and restore the
    /// caller's active compatibility view before returning.
    pub fn with_world<R>(
        &mut self,
        dimension: Dimension,
        operation: impl FnOnce(&mut ServerWorld) -> R,
    ) -> R {
        let active = self.world.dimension;
        self.ensure_dimension(dimension);
        self.activate_dimension(dimension);
        let result = operation(&mut self.world);
        self.activate_dimension(active);
        result
    }

    /// Return every dimension with an authoritative world, including the
    /// active compatibility world.  Ordering is stable for deterministic tick
    /// and persistence traversal.
    pub fn dimensions(&self) -> Vec<Dimension> {
        let mut dimensions: Vec<_> = self.worlds.keys().copied().collect();
        dimensions.push(self.world.dimension);
        dimensions.sort_unstable();
        dimensions.dedup();
        dimensions
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

    fn next_unique_entity_id(&self) -> u64 {
        let mut candidate = self.next_authority_entity_id.max(AUTHORITY_ENTITY_ID_START);
        loop {
            let entity_exists = self
                .world_ref(self.world.dimension)
                .is_some_and(|world| world.entities.get_by_id(candidate).is_some())
                || self
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

    fn claim_entity_id(&mut self, id: u64) {
        debug_assert_ne!(id, 0);
        self.next_authority_entity_id = id.wrapping_add(1).max(AUTHORITY_ENTITY_ID_START);
    }

    /// Revision for one dimension's independent namespace. Request gates and
    /// ACKs use this value for the session's dimension; the aggregate snapshot
    /// revision is only a compatibility summary and must not be used for a
    /// cross-dimension stale check.
    pub fn revision_for_dimension(&self, dimension: Dimension) -> u64 {
        if self.world.dimension == dimension {
            self.world.revisions.current()
        } else {
            self.worlds
                .get(&dimension)
                .map(|world| world.revisions.current())
                .unwrap_or(0)
        }
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
        let Some(session) = self.sessions.get_mut(&id) else {
            return false;
        };
        session.dimension = target as u8;
        session.last_revision = revision;
        session.gameplay.revision = revision;
        true
    }

    pub fn set_rules(&mut self, rules: WorldRules) {
        let rules = rules.normalized();
        self.config.rules = rules;
        self.world.rules = rules;
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
        self.sessions.insert(session.id, session);
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
        self.sessions.remove(&id)
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

    pub fn last_snapshot(&self) -> &AuthoritySnapshot {
        &self.last_snapshot
    }

    /// Active-world compatibility revision. Use `revision_for_dimension` for
    /// request/client gates when a session may be in another dimension.
    pub fn current_revision(&self) -> u64 {
        self.world.revisions.current()
    }

    /// Revision for one dimension's independent namespace. Request gates and
    /// ACKs use this value for the session's dimension; the aggregate snapshot
    /// revision is only a compatibility summary and must not be used for a
    /// cross-dimension stale check.
    /// Execute one fixed tick for every loaded dimension. Sessions are sorted
    /// by their BTreeMap key within each dimension, so AI/automation and
    /// mutation order do not depend on transport arrival order. Revisions are
    /// dimension-scoped; `(WorldMutation.dimension, revision)` is the stable
    /// routing/persistence identity.
    pub fn tick(&mut self) -> AuthoritySnapshot {
        self.fixed_tick = self.fixed_tick.wrapping_add(1).max(1);
        let active_before_tick = self.world.dimension;
        let dimensions = self.dimensions();
        let mut mutations_by_dimension: BTreeMap<Dimension, Vec<WorldMutation>> = BTreeMap::new();

        for dimension in dimensions.iter().copied() {
            self.activate_dimension(dimension);
            self.tick_session_domains(dimension);
            self.tick_mining(dimension);
            self.tick_portal_travel(dimension);
            let players: Vec<(PlayerId, [f32; 3])> = self
                .sessions
                .values()
                .filter(|session| session.dimension == dimension as u8)
                .map(|session| (session.id, session.position))
                .collect();
            let world_snapshot = self.world.tick(&players);
            self.tick_item_pickups(dimension);
            let mut world_mutations = world_snapshot.mutations;
            // Redstone emits dispenser/dropper edges from inside the world
            // tick, but entity ids belong to AuthorityCore's global namespace.
            // Drain and execute them here before collecting pending revisions
            // so source/target block entities and spawned entities share one
            // deterministic snapshot boundary.
            let actions = self.world.take_pending_redstone_actions();
            for action in actions {
                let candidate = self.next_unique_entity_id();
                let spawned = self.world.execute_redstone_dispense(action, candidate);
                if spawned {
                    self.claim_entity_id(candidate);
                }
            }
            world_mutations.extend(self.world.take_pending_mutations());
            mutations_by_dimension.insert(dimension, world_mutations);
        }

        for mutation in std::mem::take(&mut self.pending_mutations) {
            if let Some(dimension) = Dimension::from_wire(mutation.dimension) {
                mutations_by_dimension
                    .entry(dimension)
                    .or_default()
                    .push(mutation);
            }
        }

        let mut mutations = Vec::new();
        for dimension in dimensions.iter().copied() {
            let entries = mutations_by_dimension.entry(dimension).or_default();
            entries.sort_by_key(|mutation| (mutation.revision, mutation.position));
            mutations.extend(entries.iter().copied());
        }

        let mut checksums = Vec::with_capacity(dimensions.len());
        let mut revision = 0;
        for dimension in dimensions.iter().copied() {
            self.activate_dimension(dimension);
            let entries = mutations_by_dimension
                .get(&dimension)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            revision = revision.max(self.world.revisions.current());
            checksums.push((dimension, self.world.checksum(entries)));
        }
        // Keep the public active-world compatibility view stable for State,
        // ServerRuntime metrics, and save callers after the multi-world pass.
        self.activate_dimension(active_before_tick);

        let snapshot = AuthoritySnapshot {
            tick: self.fixed_tick,
            revision,
            checksum: aggregate_dimension_checksums(&checksums),
            mutations,
            session_updates: self
                .sessions
                .values()
                .map(|session| SessionGameplayUpdate {
                    player_id: session.id,
                    dimension: session.dimension,
                    state: session.gameplay,
                })
                .collect(),
        };
        self.last_snapshot = snapshot.clone();
        snapshot
    }

    /// Settle dropped-item collection in the same authority that owns both
    /// entities and inventories. Presentation roots must never delete a local
    /// replica and award an item independently of this transaction.
    fn tick_item_pickups(&mut self, dimension: Dimension) {
        let ids: Vec<_> = self
            .sessions
            .values()
            .filter(|session| session.dimension == dimension as u8)
            .map(|session| session.id)
            .collect();

        for id in ids {
            let Some((position, game_mode)) = self
                .sessions
                .get(&id)
                .map(|session| (session.position, session.game_mode))
            else {
                continue;
            };
            if !crate::game_rules::GameModePolicy::for_rules(game_mode, &self.world.rules)
                .can_pickup
            {
                continue;
            }

            let mut candidates: Vec<_> = self
                .world
                .entities
                .query_radius_types(
                    glam::Vec3::from_array(position),
                    1.5,
                    &[crate::entity::EntityType::DroppedItem],
                )
                .filter(|entity| {
                    entity.pickup_cooldown <= 0.0
                        && (entity.dropped_stack.is_some() || entity.dropped_item.is_some())
                })
                .map(|entity| entity.id)
                .collect();
            candidates.sort_unstable();

            for entity_id in candidates {
                let Some(stack) = self.world.entities.get_by_id(entity_id).and_then(|entity| {
                    entity.dropped_stack.or_else(|| {
                        entity
                            .dropped_item
                            .map(|item| ItemStack::new(item, entity.dropped_count.max(1)))
                    })
                }) else {
                    continue;
                };
                let slot = SessionInventorySlot::from_wire(
                    ItemWire::from_stack(&stack),
                    stack.can_break,
                    stack.can_place_on,
                );
                let Some(mut gameplay) = self.sessions.get(&id).map(|session| session.gameplay)
                else {
                    continue;
                };
                if !gameplay.add_slot(slot) {
                    continue;
                }
                gameplay.revision = self.world.revisions.allocate();
                if let Some(session) = self.sessions.get_mut(&id) {
                    session.gameplay = gameplay;
                }
                self.world.remove_authority_entity(entity_id);
            }

            let mut experience_orbs: Vec<_> = self
                .world
                .entities
                .query_radius_types(
                    glam::Vec3::from_array(position),
                    1.5,
                    &[crate::entity::EntityType::ExperienceOrb],
                )
                .filter(|entity| entity.pickup_cooldown <= 0.0 && entity.xp_value > 0)
                .map(|entity| (entity.id, entity.xp_value))
                .collect();
            experience_orbs.sort_unstable_by_key(|(entity_id, _)| *entity_id);
            if !experience_orbs.is_empty() {
                let amount = experience_orbs
                    .iter()
                    .fold(0u32, |total, (_, value)| total.saturating_add(*value));
                let Some(mut gameplay) = self.sessions.get(&id).map(|session| session.gameplay)
                else {
                    continue;
                };
                gameplay.experience = gameplay.experience.saturating_add(amount);
                loop {
                    let cost = 7u32.saturating_add(gameplay.experience_level.saturating_mul(2));
                    if gameplay.experience < cost || cost == u32::MAX {
                        break;
                    }
                    gameplay.experience -= cost;
                    gameplay.experience_level = gameplay.experience_level.saturating_add(1);
                }
                gameplay.revision = self.world.revisions.allocate();
                if let Some(session) = self.sessions.get_mut(&id) {
                    session.gameplay = gameplay;
                }
                for (entity_id, _) in experience_orbs {
                    self.world.remove_authority_entity(entity_id);
                }
            }
        }
    }

    fn tick_session_domains(&mut self, dimension: Dimension) {
        use crate::authority::transactions::{self, BrewTick, WorkstationContext};
        use crate::inventory::GameMode;

        let ids: Vec<_> = self
            .sessions
            .values()
            .filter(|session| session.dimension == dimension as u8)
            .map(|session| session.id)
            .collect();
        for id in ids {
            let Some((position, game_mode, original)) = self
                .sessions
                .get(&id)
                .map(|session| (session.position, session.game_mode, session.gameplay))
            else {
                continue;
            };
            let mut candidate = original;

            candidate.invulnerability_ticks = candidate.invulnerability_ticks.saturating_sub(1);
            candidate.shield_cooldown_ticks = candidate.shield_cooldown_ticks.saturating_sub(1);
            if candidate.shield_cooldown_ticks > 0 {
                candidate.shield_active = false;
            }
            candidate.attack_cooldown_ticks = candidate
                .attack_cooldown_ticks
                .saturating_add(1)
                .min(ATTACK_COOLDOWN_TICKS);

            if let Some(pending) = candidate.brew {
                let block = self.world.get_block(
                    pending.station[0],
                    pending.station[1],
                    pending.station[2],
                );
                let context = WorkstationContext::at(pending.station, block);
                match transactions::tick_brew(&mut candidate, context) {
                    Ok(BrewTick::Ready) => {}
                    Ok(BrewTick::Brewing { .. }) => {}
                    Err(_) => candidate.brew = None,
                }
            }

            let previous_hook = original.fishing_hook;
            if candidate.fishing_hook.is_some() {
                match self.world.fishing_context(
                    &candidate,
                    position,
                    game_mode != GameMode::Creative,
                ) {
                    Ok(context) => {
                        if crate::authority::fishing::tick(&mut candidate, context).is_err() {
                            candidate.fishing_hook = None;
                        }
                    }
                    Err(_) => candidate.fishing_hook = None,
                }
            }

            if candidate == original {
                continue;
            }
            self.world
                .sync_authority_hook(previous_hook, candidate.fishing_hook, id);
            let revision = self.world.revisions.allocate();
            candidate.revision = revision;
            if let Some(session) = self.sessions.get_mut(&id) {
                session.gameplay = candidate;
                // Autonomous cooldown, brew and hook ticks publish a newer
                // owner-private projection, but they are not a client-authored
                // transaction baseline. Advancing `last_revision` here makes
                // every in-flight reel/cancel stale before TCP ingress. This
                // matches the mining tick seam: accepted requests and durable
                // mutations advance the anti-stale baseline; fixed-tick
                // presentation progress advances only gameplay.revision.
            }
        }
    }

    fn tick_mining(&mut self, dimension: Dimension) {
        let ids: Vec<_> = self
            .sessions
            .values()
            .filter(|session| session.dimension == dimension as u8)
            .map(|session| session.id)
            .collect();
        for id in ids {
            let Some((position, game_mode, progress)) = self
                .sessions
                .get(&id)
                .map(|session| (session.position, session.game_mode, session.gameplay.mining))
            else {
                continue;
            };
            let Some(progress) = progress else {
                continue;
            };
            if progress.dimension != dimension as u8 {
                self.clear_mining_progress(id);
                continue;
            }
            let target = (progress.target[0], progress.target[1], progress.target[2]);
            let Some(block) = self
                .world
                .chunks
                .get_loaded_block(target.0, target.1, target.2)
            else {
                self.clear_mining_progress(id);
                continue;
            };
            let expected_block = crate::world::BlockType::from_wire(progress.block);
            let expected_state = self.world.get_block_state(target.0, target.1, target.2);
            if expected_block != Some(block) || expected_state != progress.state {
                self.clear_mining_progress(id);
                continue;
            }
            let eye = glam::Vec3::from_array(position) + glam::Vec3::new(0.0, 1.62, 0.0);
            let target_center = glam::Vec3::new(
                target.0 as f32 + 0.5,
                target.1 as f32 + 0.5,
                target.2 as f32 + 0.5,
            );
            if eye.distance(target_center) > 8.0 {
                self.clear_mining_progress(id);
                continue;
            }
            if block == crate::world::BlockType::Air
                || !self
                    .world
                    .has_block_line_of_sight(position, progress.look_milli, target)
            {
                self.clear_mining_progress(id);
                continue;
            }
            let selected_index = if progress.hand == 1 {
                40
            } else {
                self.sessions
                    .get(&id)
                    .map_or(0, |session| session.gameplay.selected_hotbar_slot)
            };
            if selected_index != progress.slot_index {
                self.clear_mining_progress(id);
                continue;
            }
            let held_matches = self
                .sessions
                .get(&id)
                .and_then(|session| session.gameplay.slot(selected_index).flatten())
                == progress.held.map(SessionInventorySlot::from);
            if !held_matches {
                self.clear_mining_progress(id);
                continue;
            }
            let held_stack = stack_from_slot(progress.held);
            let policy = crate::game_rules::GameModePolicy::for_rules(game_mode, &self.world.rules);
            if !policy.can_break_stack(held_stack.as_ref(), block) {
                self.clear_mining_progress(id);
                continue;
            }
            if game_mode == crate::inventory::GameMode::Creative {
                let _ = self.commit_mining_break(id, dimension, target, held_stack, game_mode);
                continue;
            }
            let duration =
                crate::authority::mining::mining_time_seconds(block, held_stack.as_ref());
            if !duration.is_finite() || duration <= 0.0 || duration == f32::MAX {
                self.clear_mining_progress(id);
                continue;
            }
            let step = ((1_000.0 / (duration * FIXED_TICK_HZ as f32)).ceil() as u16).max(1);
            let next = progress.progress_milli.saturating_add(step);
            if next >= 1_000 {
                let _ = self.commit_mining_break(id, dimension, target, held_stack, game_mode);
            } else if let Some(session) = self.sessions.get_mut(&id) {
                if let Some(active) = session.gameplay.mining.as_mut() {
                    active.progress_milli = next;
                }
                let revision = self.world.revisions.allocate();
                session.gameplay.revision = revision;
            }
        }
    }

    fn clear_mining_progress(&mut self, id: PlayerId) {
        let Some(session) = self.sessions.get_mut(&id) else {
            return;
        };
        if session.gameplay.mining.take().is_some() {
            let revision = self.world.revisions.allocate();
            session.gameplay.revision = revision;
        }
    }

    fn tick_portal_travel(&mut self, dimension: Dimension) {
        use crate::inventory::GameMode;
        use crate::world::BlockType;
        use glam::Vec3;

        let ids: Vec<_> = self
            .sessions
            .values()
            .filter(|session| session.dimension == dimension as u8)
            .map(|session| session.id)
            .collect();

        for id in ids {
            let Some((position, game_mode, cooldown, contact_time, requested)) =
                self.sessions.get(&id).map(|session| {
                    (
                        session.position,
                        session.game_mode,
                        session.portal_cooldown,
                        session.portal_contact_time,
                        session.portal_requested,
                    )
                })
            else {
                continue;
            };

            let new_cooldown = (cooldown - FIXED_DT).max(0.0);
            if new_cooldown > 0.0 {
                if let Some(session) = self.sessions.get_mut(&id) {
                    session.portal_cooldown = new_cooldown;
                    session.portal_contact_time = 0.0;
                    session.portal_requested = false;
                }
                continue;
            }

            if !requested {
                if let Some(session) = self.sessions.get_mut(&id) {
                    session.portal_contact_time = 0.0;
                }
                continue;
            }

            let px = position[0].floor() as i32;
            let py = position[1].floor() as i32;
            let pz = position[2].floor() as i32;

            let feet = self.world.get_block(px, py, pz);
            let body = self.world.get_block(px, py + 1, pz);

            if feet == BlockType::EndGateway || body == BlockType::EndGateway {
                if dimension == Dimension::End {
                    let pos_vec = Vec3::from_array(position);
                    let dist = pos_vec.length();
                    let target_pos = if dist < 300.0 {
                        Vec3::new(1035.5, 89.0, 11.5)
                    } else {
                        Vec3::new(0.5, 65.0, 0.5)
                    };
                    if let Some(session) = self.sessions.get_mut(&id) {
                        session.position = target_pos.to_array();
                        session.portal_cooldown = 2.0;
                        session.portal_contact_time = 0.0;
                    }
                    self.pending_session_revisions.insert(id);
                    continue;
                }
            }

            if feet == BlockType::EndPortal || body == BlockType::EndPortal {
                let target_dim = if dimension == Dimension::End {
                    Dimension::Overworld
                } else {
                    Dimension::End
                };

                let target_pos = if target_dim == Dimension::End {
                    Vec3::new(0.5, 65.0, 0.5)
                } else {
                    let spawn = self.sessions.get(&id).and_then(|s| s.spawn_point);
                    if let Some(sp) = spawn {
                        Vec3::new(sp[0] as f32 + 0.5, sp[1] as f32, sp[2] as f32 + 0.5)
                    } else {
                        Vec3::new(0.5, 65.0, 0.5)
                    }
                };

                self.execute_portal_dimension_transfer(id, target_dim, target_pos.to_array());
                continue;
            }

            if feet == BlockType::NetherPortal || body == BlockType::NetherPortal {
                let new_contact = contact_time + FIXED_DT;
                if new_contact >= 1.0 || game_mode == GameMode::Creative {
                    let target_dim = if dimension == Dimension::Nether {
                        Dimension::Overworld
                    } else {
                        Dimension::Nether
                    };

                    let scaled = crate::dimension::transform_position(
                        dimension,
                        target_dim,
                        Vec3::from_array(position),
                    );
                    let cx = scaled.x.floor() as i32 >> 4;
                    let cz = scaled.z.floor() as i32 >> 4;
                    let height = target_dim.height();

                    let target_pos = {
                        self.ensure_dimension(target_dim);
                        let target_world = self.world_mut(target_dim).unwrap();
                        let target_y = target_world
                            .safe_spawn_y(scaled.x.floor() as i32, scaled.z.floor() as i32);
                        let (portal_blocks, spawn_vec) =
                            crate::dimension::build_linked_nether_portal_blocks(
                                cx, cz, target_y, height,
                            );
                        let mut mutations = Vec::new();
                        for ((bx, by, bz), btype) in portal_blocks {
                            if target_world.get_block(bx, by, bz) != BlockType::NetherPortal {
                                if let Ok(Some(mutation)) =
                                    target_world.set_block(bx, by, bz, btype, 0)
                                {
                                    mutations.push(mutation);
                                }
                            }
                        }
                        (spawn_vec, mutations)
                    };
                    self.pending_mutations.extend(target_pos.1);
                    self.execute_portal_dimension_transfer(id, target_dim, target_pos.0.to_array());
                } else {
                    if let Some(session) = self.sessions.get_mut(&id) {
                        session.portal_contact_time = new_contact;
                    }
                }
            } else {
                if let Some(session) = self.sessions.get_mut(&id) {
                    session.portal_contact_time = 0.0;
                    session.portal_requested = false;
                }
            }
        }
    }

    pub fn execute_portal_dimension_transfer(
        &mut self,
        id: PlayerId,
        target_dim: Dimension,
        target_pos: [f32; 3],
    ) -> bool {
        let Some(from) = self
            .sessions
            .get(&id)
            .and_then(|session| Dimension::from_wire(session.dimension))
        else {
            return false;
        };
        if !self.set_session_dimension(id, target_dim) {
            return false;
        }
        if let Some(session) = self.sessions.get_mut(&id) {
            session.position = target_pos;
            session.portal_contact_time = 0.0;
            session.portal_cooldown = 3.0;
            session.portal_requested = false;
        }
        self.pending_session_revisions.insert(id);
        self.pending_dimension_transfers
            .push(DimensionTransferIntent {
                player_id: id,
                from,
                to: target_dim,
                position: target_pos,
            });
        true
    }

    fn commit_mining_break(
        &mut self,
        id: PlayerId,
        dimension: Dimension,
        target: (i32, i32, i32),
        held_stack: Option<crate::inventory::ItemStack>,
        game_mode: crate::inventory::GameMode,
    ) -> bool {
        let Some(session) = self.sessions.get(&id) else {
            return false;
        };
        let Some(progress) = session.gameplay.mining else {
            return false;
        };
        if session.dimension != dimension as u8 || progress.target != [target.0, target.1, target.2]
        {
            return false;
        }
        let Some(old_block) = self
            .world
            .chunks
            .get_loaded_block(target.0, target.1, target.2)
        else {
            self.clear_mining_progress(id);
            return false;
        };
        if old_block == crate::world::BlockType::Air
            || crate::world::BlockType::from_wire(progress.block) != Some(old_block)
            || self.world.get_block_state(target.0, target.1, target.2) != progress.state
        {
            self.clear_mining_progress(id);
            return false;
        }
        let rewards = crate::authority::mining::calculate_block_break_rewards(
            old_block,
            self.world.get_block_state(target.0, target.1, target.2),
            target,
            held_stack.as_ref(),
            game_mode,
        );
        // Preflight every session-side consequence on a copy. XP/level
        // overflow or a stale durability slot must abort before any entity ID
        // is claimed or world block is changed.
        let mut next_gameplay = session.gameplay;
        if !next_gameplay.grant_experience(rewards.xp) {
            return false;
        }
        next_gameplay.mining = None;
        if rewards.tool_damaged && game_mode != crate::inventory::GameMode::Creative {
            let salt = (target.0 as u32)
                ^ (target.1 as u32).rotate_left(11)
                ^ (target.2 as u32).rotate_left(22);
            if held_stack.as_ref().is_some_and(|stack| {
                crate::enchantment::should_consume_durability(&stack.enchantments, salt)
            }) {
                let slot = usize::from(progress.slot_index);
                let Some(Some(current)) = next_gameplay.inventory.get_mut(slot) else {
                    return false;
                };
                if current.item.durability > 1 {
                    current.item.durability -= 1;
                } else {
                    next_gameplay.inventory[slot] = None;
                }
            }
        }
        // A block-entity-backed container is removed by set_block(Air).  Copy
        // its complete non-empty stacks only after every session-side
        // preflight above has succeeded, and keep the source entity untouched
        // until the block mutation commits.
        let mut drops = rewards.drops;
        if let Some(block_entity) = self.world.get_block_entity(target.0, target.1, target.2) {
            let slots: Box<dyn Iterator<Item = &ItemStack> + '_> = match block_entity {
                BlockEntity::Chest(chest) => Box::new(chest.inventory.slots.iter().flatten()),
                BlockEntity::Furnace(furnace) => Box::new(furnace.slots.iter().flatten()),
                BlockEntity::Hopper(hopper) => Box::new(hopper.slots.iter().flatten()),
                BlockEntity::Dispenser(dispenser) => Box::new(dispenser.slots.iter().flatten()),
                BlockEntity::Dropper(dropper) => Box::new(dropper.slots.iter().flatten()),
                BlockEntity::Sign(_) | BlockEntity::Spawner(_) | BlockEntity::Observer(_) => {
                    Box::new(std::iter::empty())
                }
            };
            drops.extend(slots.filter(|stack| stack.count > 0).copied());
        }
        // Reserve every entity id before changing source or inventory. Gaps in
        // the global allocator are harmless; reusing an id after a failed
        // request would not be. Prepare all dropped entities first: no
        // presentation snapshot can interleave with this synchronous commit,
        // and rollback keeps a failed mutation from losing a prepared drop.
        let mut entity_ids = Vec::with_capacity(drops.len());
        for _ in &drops {
            let candidate = self.next_unique_entity_id();
            if candidate == 0 {
                return false;
            }
            self.claim_entity_id(candidate);
            entity_ids.push(candidate);
        }
        let drop_position = [
            target.0 as f32 + 0.5,
            target.1 as f32 + 0.5,
            target.2 as f32 + 0.5,
        ];
        let mut prepared_ids = Vec::with_capacity(entity_ids.len());
        for (entity_id, stack) in entity_ids.iter().copied().zip(drops.iter().copied()) {
            if !self
                .world
                .spawn_dropped_item(entity_id, drop_position, stack)
            {
                for prepared_id in prepared_ids {
                    self.world.remove_authority_entity(prepared_id);
                }
                return false;
            }
            prepared_ids.push(entity_id);
        }
        let Ok(Some(mutation)) = self.world.set_block(
            target.0,
            target.1,
            target.2,
            crate::world::BlockType::Air,
            0,
        ) else {
            for prepared_id in prepared_ids {
                self.world.remove_authority_entity(prepared_id);
            }
            return false;
        };
        self.pending_mutations.push(mutation);
        if let Some(session) = self.sessions.get_mut(&id) {
            session.gameplay = next_gameplay;
            session.gameplay.revision = mutation.revision;
            session.last_revision = mutation.revision;
        }
        true
    }

    pub fn submit_request(&mut self, request: GameplayRequest) -> GameplayResponse {
        let request_id = request.request_id;
        let id = request.session_id;
        let Some(session_dimension_wire) = self.sessions.get(&id).map(|session| session.dimension)
        else {
            return self.rejected(request_id, RejectReason::Unauthorized);
        };
        if let Some(cached) = self
            .sessions
            .get(&id)
            .and_then(|session| session.cached_response(request_id))
        {
            return cached;
        }
        let Some(session_dimension) = Dimension::from_wire(request.dimension) else {
            return self.reject_for_session(id, request_id, RejectReason::InvalidDimension, None);
        };
        if let Err(reason) = request.validate_bounds() {
            return self.reject_for_session(id, request_id, reason, None);
        }
        if session_dimension_wire != request.dimension {
            return self.reject_for_session(id, request_id, RejectReason::InvalidDimension, None);
        }
        self.ensure_dimension(session_dimension);
        self.activate_dimension(session_dimension);
        let Some(session) = self.sessions.get(&id) else {
            return self.rejected(request_id, RejectReason::Unauthorized);
        };
        if let Err(reason) = session.validate_sequence(&request) {
            return self.reject_for_session(id, request_id, reason, None);
        }
        if request.client_revision > self.current_revision() {
            return self.reject_for_session(id, request_id, RejectReason::InvalidRevision, None);
        }
        if request.client_revision < session.last_revision {
            return self.reject_for_session(id, request_id, RejectReason::InvalidRevision, None);
        }
        if session.game_mode == crate::inventory::GameMode::Spectator
            && matches!(
                &request.operation,
                crate::network::protocol::GameplayOperation::BlockAction { .. }
                    | crate::network::protocol::GameplayOperation::BlockUse { .. }
                    | crate::network::protocol::GameplayOperation::Container { .. }
                    | crate::network::protocol::GameplayOperation::ContainerClick { .. }
                    | crate::network::protocol::GameplayOperation::ItemUse { .. }
                    | crate::network::protocol::GameplayOperation::Combat { .. }
                    | crate::network::protocol::GameplayOperation::Trade { .. }
                    | crate::network::protocol::GameplayOperation::Mount { .. }
                    | crate::network::protocol::GameplayOperation::Fishing { .. }
                    | crate::network::protocol::GameplayOperation::FurnaceTakeOutput { .. }
                    | crate::network::protocol::GameplayOperation::Craft { .. }
                    | crate::network::protocol::GameplayOperation::Enchant { .. }
                    | crate::network::protocol::GameplayOperation::Brew { .. }
                    | crate::network::protocol::GameplayOperation::Anvil { .. }
                    | crate::network::protocol::GameplayOperation::UseState { .. }
                    | crate::network::protocol::GameplayOperation::FluidUse { .. }
            )
        {
            return self.reject_for_session(id, request_id, RejectReason::PermissionDenied, None);
        }
        let session_position = session.position;
        let operator = session.operator || session.cheats_enabled;
        if let Err(reason) =
            self.world
                .validate_request(&request, session_dimension, session_position, operator)
        {
            return self.reject_for_session(id, request_id, reason, None);
        }

        self.pending_session_revisions.clear();
        let result = self
            .dispatch_session_command(&request, id)
            .or_else(|| self.dispatch_session_gameplay(&request, id))
            .unwrap_or_else(|| {
                self.world
                    .dispatch(&request, id, operator)
                    .map_err(|error| error.reason())
            });
        self.pending_mutations
            .extend(self.world.take_pending_mutations());
        let response = match result {
            Ok(mutation) => {
                if let Some(mutation) = mutation {
                    self.pending_mutations.push(mutation);
                }
                let revision = mutation
                    .map(|mutation| mutation.revision)
                    .unwrap_or_else(|| self.world.revisions.allocate());
                GameplayResponse {
                    request_id,
                    server_sequence: revision,
                    outcome: GameplayOutcome::Accepted { revision },
                }
            }
            Err(reason) => {
                // A well-formed, authenticated request consumes its client
                // sequence even when the domain rejects it.  This prevents a
                // rejected operation from being replayed under a later ACK
                // and keeps the 128-entry cache idempotent.
                self.reject_for_session(id, request_id, reason, Some(request.client_sequence))
            }
        };
        if let Some(session) = self.sessions.get_mut(&id) {
            if matches!(response.outcome, GameplayOutcome::Accepted { .. }) {
                session.last_client_sequence = request.client_sequence;
                if let GameplayOutcome::Accepted { revision } = response.outcome {
                    session.last_revision = revision;
                    session.gameplay.revision = revision;
                }
                session.cache_response(response.clone());
            }
        }
        if let GameplayOutcome::Accepted { revision } = response.outcome {
            for changed_id in std::mem::take(&mut self.pending_session_revisions) {
                if changed_id == id {
                    continue;
                }
                if let Some(session) = self.sessions.get_mut(&changed_id) {
                    session.last_revision = revision;
                    session.gameplay.revision = revision;
                }
            }
        } else {
            self.pending_session_revisions.clear();
        }
        response
    }

    /// Dispatch player gameplay against the authenticated session and the
    /// headless world.  Renderer roots never perform these mutations after an
    /// authority boundary exists; an unsupported/invalid domain is rejected
    /// before it can fall back to local simulation.
    fn dispatch_session_gameplay(
        &mut self,
        request: &GameplayRequest,
        session_id: PlayerId,
    ) -> Option<Result<Option<WorldMutation>, RejectReason>> {
        use crate::inventory::GameMode;

        match &request.operation {
            GameplayOperation::BlockAction {
                action,
                x,
                y,
                z,
                face,
                hand,
                held,
                block,
                look_milli,
            } => Some(self.apply_block_action(
                session_id,
                *action,
                (*x, *y, *z),
                *face,
                *hand,
                *held,
                *block,
                *look_milli,
            )),
            GameplayOperation::ItemUse { item, count } => {
                let Some(item_kind) = crate::inventory::Item::from_u32(*item) else {
                    return Some(Err(RejectReason::InvalidState));
                };
                if *count == 0 {
                    return Some(Err(RejectReason::InvalidState));
                }
                let Some(food) = item_kind.food_properties() else {
                    return Some(Err(RejectReason::Unsupported));
                };
                let Some(session) = self.sessions.get(&session_id) else {
                    return Some(Err(RejectReason::Unauthorized));
                };
                let original_gameplay = session.gameplay;
                let mut gameplay = original_gameplay;
                let hunger = gameplay.hunger_milli as f32 / 1000.0;
                if hunger >= 20.0 && !food.always_edible && session.game_mode != GameMode::Creative
                {
                    return Some(Err(RejectReason::InvalidState));
                }
                gameplay.hunger_milli = ((hunger + food.hunger).min(20.0) * 1000.0).round() as u32;
                gameplay.saturation_milli = ((gameplay.saturation_milli as f32 / 1000.0
                    + food.saturation)
                    .min(gameplay.hunger_milli as f32 / 1000.0)
                    * 1000.0)
                    .round() as u32;
                if session.game_mode != GameMode::Creative
                    && !gameplay.remove_item(*item, u32::from(*count))
                {
                    return Some(Err(RejectReason::InvalidState));
                }
                if !preserves_brew_locks(&original_gameplay, &gameplay) {
                    return Some(Err(RejectReason::InvalidState));
                }
                if let Some(session) = self.sessions.get_mut(&session_id) {
                    session.gameplay = gameplay;
                }
                Some(Ok(None))
            }
            GameplayOperation::Combat { target, action } => {
                Some(self.apply_authoritative_combat(request, session_id, *target, *action))
            }
            GameplayOperation::Trade {
                villager_id,
                offer_index,
            } => {
                let Some(position) = self.sessions.get(&session_id).map(|s| s.position) else {
                    return Some(Err(RejectReason::Unauthorized));
                };
                let Some(mut gameplay) = self.sessions.get(&session_id).map(|s| s.gameplay) else {
                    return Some(Err(RejectReason::Unauthorized));
                };
                let original = gameplay;
                let result = self
                    .world
                    .apply_trade(&mut gameplay, *villager_id, *offer_index, position)
                    .map(|_| {
                        if !preserves_brew_locks(&original, &gameplay) {
                            return Err(RejectReason::InvalidState);
                        }
                        if let Some(session) = self.sessions.get_mut(&session_id) {
                            session.gameplay = gameplay;
                        }
                        Ok(None)
                    });
                Some(result.and_then(|result| result))
            }
            GameplayOperation::Mount { entity_id } => {
                let Some(position) = self.sessions.get(&session_id).map(|s| s.position) else {
                    return Some(Err(RejectReason::Unauthorized));
                };
                Some(
                    self.world
                        .apply_mount(session_id, *entity_id, position)
                        .map(|mounted| {
                            if let Some(session) = self.sessions.get_mut(&session_id) {
                                session.gameplay.mounted_entity = mounted;
                            }
                            None
                        }),
                )
            }
            GameplayOperation::Fishing {
                action,
                hand,
                look_milli,
            } => Some(self.apply_fishing(session_id, *action, *hand, *look_milli)),
            GameplayOperation::FluidUse {
                x,
                y,
                z,
                face,
                hand,
                source,
            } => Some(self.apply_fluid_use(session_id, (*x, *y, *z), *face, *hand, *source)),
            GameplayOperation::FurnaceTakeOutput { .. }
            | GameplayOperation::Craft { .. }
            | GameplayOperation::Enchant { .. }
            | GameplayOperation::Brew { .. }
            | GameplayOperation::Anvil { .. }
            | GameplayOperation::UseState { .. } => {
                Some(self.apply_transaction_operation(session_id, &request.operation))
            }
            _ => None,
        }
    }

    fn apply_block_action(
        &mut self,
        session_id: PlayerId,
        action: BlockActionKind,
        position: (i32, i32, i32),
        face: [i8; 3],
        hand: u8,
        held: Option<SessionSlotWire>,
        block_wire: u32,
        look_milli: [i16; 3],
    ) -> Result<Option<WorldMutation>, RejectReason> {
        let Some(session) = self.sessions.get(&session_id).cloned() else {
            return Err(RejectReason::Unauthorized);
        };
        let Some(dimension) = Dimension::from_wire(session.dimension) else {
            return Err(RejectReason::InvalidDimension);
        };
        if dimension != self.world.dimension {
            return Err(RejectReason::InvalidDimension);
        }
        if matches!(action, BlockActionKind::CancelBreak) {
            if let Some(session) = self.sessions.get_mut(&session_id) {
                session.gameplay.mining = None;
            }
            return Ok(None);
        }
        if matches!(action, BlockActionKind::EnterPortal) {
            let expected =
                crate::world::BlockType::from_wire(block_wire).ok_or(RejectReason::InvalidState)?;
            let actual = self
                .world
                .chunks
                .get_loaded_block(position.0, position.1, position.2)
                .ok_or(RejectReason::InvalidState)?;
            let feet = (
                session.position[0].floor() as i32,
                session.position[1].floor() as i32,
                session.position[2].floor() as i32,
            );
            let body = (feet.0, feet.1 + 1, feet.2);
            if actual != expected
                || !matches!(
                    actual,
                    crate::world::BlockType::NetherPortal
                        | crate::world::BlockType::EndPortal
                        | crate::world::BlockType::EndGateway
                )
                || (position != feet && position != body)
                || session.portal_cooldown > 0.0
            {
                return Err(RejectReason::InvalidState);
            }
            if let Some(session) = self.sessions.get_mut(&session_id) {
                session.portal_contact_time = 0.0;
                session.portal_requested = true;
                session.gameplay.mining = None;
            }
            return Ok(None);
        }
        let slot_index = if hand == 0 {
            session.gameplay.selected_hotbar_slot
        } else if hand == 1 {
            40
        } else {
            return Err(RejectReason::InvalidState);
        };
        let current = session.gameplay.slot(slot_index).flatten();
        let current_stack = match held {
            Some(expected) => {
                if current != Some(SessionInventorySlot::from(expected)) {
                    return Err(RejectReason::InvalidState);
                }
                stack_from_slot(Some(expected))
            }
            None => {
                if current.is_some() {
                    return Err(RejectReason::InvalidState);
                }
                None
            }
        };
        if !self
            .world
            .valid_coordinate(position.0, position.1, position.2)
        {
            return Err(RejectReason::InvalidCoordinate);
        }

        match action {
            BlockActionKind::StartBreak => {
                let Some(target_block) = self
                    .world
                    .chunks
                    .get_loaded_block(position.0, position.1, position.2)
                else {
                    return Err(RejectReason::InvalidState);
                };
                if target_block == crate::world::BlockType::Air
                    || !self
                        .world
                        .has_block_line_of_sight(session.position, look_milli, position)
                {
                    return Err(RejectReason::InvalidState);
                }
                let policy = crate::game_rules::GameModePolicy::for_rules(
                    session.game_mode,
                    &self.world.rules,
                );
                if !policy.can_break_stack(current_stack.as_ref(), target_block) {
                    return Err(RejectReason::PermissionDenied);
                }
                let target_state = self
                    .world
                    .get_block_state(position.0, position.1, position.2);
                let progress = MiningProgressState {
                    dimension: dimension as u8,
                    target: [position.0, position.1, position.2],
                    progress_milli: session
                        .gameplay
                        .mining
                        .filter(|active| {
                            active.dimension == dimension as u8
                                && active.target == [position.0, position.1, position.2]
                                && active.hand == hand
                                && active.slot_index == slot_index
                                && active.held == held
                                && active.block == target_block.to_wire()
                                && active.state == target_state
                        })
                        .map_or(0, |active| active.progress_milli),
                    hand,
                    slot_index,
                    held,
                    block: target_block.to_wire(),
                    state: target_state,
                    look_milli,
                };
                if let Some(session) = self.sessions.get_mut(&session_id) {
                    session.gameplay.mining = Some(progress);
                }
                if session.game_mode == crate::inventory::GameMode::Creative {
                    let _ = self.commit_mining_break(
                        session_id,
                        dimension,
                        position,
                        current_stack,
                        session.game_mode,
                    );
                }
                Ok(None)
            }
            BlockActionKind::CancelBreak => unreachable!("cancel handled before slot validation"),
            BlockActionKind::Place => {
                let block = crate::world::BlockType::from_wire(block_wire)
                    .ok_or(RejectReason::InvalidState)?;
                if !self.world.has_block_line_of_sight(
                    session.position,
                    look_milli,
                    (
                        position.0 - i32::from(face[0]),
                        position.1 - i32::from(face[1]),
                        position.2 - i32::from(face[2]),
                    ),
                ) {
                    return Err(RejectReason::InvalidState);
                }
                let support = (
                    position.0.saturating_sub(i32::from(face[0])),
                    position.1.saturating_sub(i32::from(face[1])),
                    position.2.saturating_sub(i32::from(face[2])),
                );
                let Some(support_block) = self
                    .world
                    .chunks
                    .get_loaded_block(support.0, support.1, support.2)
                else {
                    return Err(RejectReason::InvalidState);
                };
                let policy = crate::game_rules::GameModePolicy::for_rules(
                    session.game_mode,
                    &self.world.rules,
                );
                let Some(held_stack) = current_stack.as_ref() else {
                    return Err(RejectReason::PermissionDenied);
                };
                if held_stack.item.properties().block_type != Some(block) {
                    return Err(RejectReason::InvalidState);
                }
                if !policy.can_place_stack(Some(held_stack), support_block) {
                    return Err(RejectReason::PermissionDenied);
                }
                // Prepare the inventory debit before mutating the world. The
                // copied gameplay state makes the place transaction atomic if
                // a late slot check ever fails, and placing always cancels an
                // in-flight mining target for this owner.
                let mut next_gameplay = session.gameplay;
                next_gameplay.mining = None;
                if session.game_mode != crate::inventory::GameMode::Creative {
                    let index = usize::from(slot_index);
                    let Some(Some(slot)) = next_gameplay.inventory.get_mut(index) else {
                        return Err(RejectReason::PermissionDenied);
                    };
                    if slot.item.count == 0 {
                        return Err(RejectReason::InvalidState);
                    }
                    slot.item.count -= 1;
                    if slot.item.count == 0 {
                        next_gameplay.inventory[index] = None;
                    }
                }
                let mutation = self.world.apply_block_place(position, face, block)?;
                let Some(session) = self.sessions.get_mut(&session_id) else {
                    return Err(RejectReason::Unauthorized);
                };
                session.gameplay = next_gameplay;
                Ok(mutation)
            }
            BlockActionKind::IgnitePortal => {
                let Some(held_stack) = current_stack else {
                    return Err(RejectReason::PermissionDenied);
                };
                if held_stack.item != crate::inventory::Item::FlintAndSteel
                    || self.world.get_block(position.0, position.1, position.2)
                        != crate::world::BlockType::Air
                {
                    return Err(RejectReason::InvalidState);
                }
                let support = (
                    position.0.saturating_sub(i32::from(face[0])),
                    position.1.saturating_sub(i32::from(face[1])),
                    position.2.saturating_sub(i32::from(face[2])),
                );
                if self.world.get_block(support.0, support.1, support.2)
                    != crate::world::BlockType::Obsidian
                    || !self
                        .world
                        .has_block_line_of_sight(session.position, look_milli, support)
                {
                    return Err(RejectReason::InvalidState);
                }
                let mutation = self
                    .world
                    .set_block(
                        position.0,
                        position.1,
                        position.2,
                        crate::world::BlockType::Fire,
                        0,
                    )
                    .map_err(|error| error.reason())?;
                if mutation.is_none() {
                    return Err(RejectReason::InvalidState);
                }
                if session.game_mode != crate::inventory::GameMode::Creative {
                    let mut gameplay = session.gameplay;
                    let index = usize::from(slot_index);
                    let Some(slot) = gameplay.inventory[index].as_mut() else {
                        return Err(RejectReason::InvalidState);
                    };
                    slot.item.durability = slot.item.durability.saturating_sub(1);
                    if slot.item.durability == 0 {
                        gameplay.inventory[index] = None;
                    }
                    gameplay.mining = None;
                    if let Some(target) = self.sessions.get_mut(&session_id) {
                        target.gameplay = gameplay;
                    }
                }
                Ok(mutation)
            }
            BlockActionKind::InsertEnderEye => {
                let Some(held_stack) = current_stack else {
                    return Err(RejectReason::PermissionDenied);
                };
                if held_stack.item != crate::inventory::Item::EyeOfEnder
                    || self.world.get_block(position.0, position.1, position.2)
                        != crate::world::BlockType::EndPortalFrame
                    || !self
                        .world
                        .has_block_line_of_sight(session.position, look_milli, position)
                {
                    return Err(RejectReason::InvalidState);
                }
                let mutation = self
                    .world
                    .set_block(
                        position.0,
                        position.1,
                        position.2,
                        crate::world::BlockType::EndPortalFrameFilled,
                        0,
                    )
                    .map_err(|error| error.reason())?;
                if mutation.is_none() {
                    return Err(RejectReason::InvalidState);
                }
                if session.game_mode != crate::inventory::GameMode::Creative {
                    let mut gameplay = session.gameplay;
                    let index = usize::from(slot_index);
                    let Some(slot) = gameplay.inventory[index].as_mut() else {
                        return Err(RejectReason::InvalidState);
                    };
                    slot.item.count = slot.item.count.saturating_sub(1);
                    if slot.item.count == 0 {
                        gameplay.inventory[index] = None;
                    }
                    gameplay.mining = None;
                    if let Some(target) = self.sessions.get_mut(&session_id) {
                        target.gameplay = gameplay;
                    }
                }
                Ok(mutation)
            }
            BlockActionKind::EnterPortal => {
                unreachable!("portal entry handled before slot validation")
            }
        }
    }

    fn apply_fluid_use(
        &mut self,
        session_id: PlayerId,
        position: (i32, i32, i32),
        face: [i8; 3],
        hand: u8,
        source: crate::network::protocol::SlotRefWire,
    ) -> Result<Option<WorldMutation>, RejectReason> {
        use crate::inventory::Item;
        use crate::network::protocol::SessionSlotWire;

        let Some((dimension, original)) = self
            .sessions
            .get(&session_id)
            .map(|session| (session.dimension, session.gameplay))
        else {
            return Err(RejectReason::Unauthorized);
        };
        let selected_index = held_slot_index(&original, hand)?;
        if source.index != selected_index || source.count != 1 {
            return Err(RejectReason::InvalidState);
        }
        let source_item =
            Item::from_u32(source.expected.item.item).ok_or(RejectReason::InvalidState)?;
        let mut candidate = original;
        match source_item {
            Item::WaterBucket => {
                if source.expected.item.count != 1
                    || !candidate.slot_matches(source)
                    || !candidate.replace_slot_exact(
                        source,
                        Some(SessionSlotWire::new(
                            crate::network::protocol::ItemWire {
                                item: Item::Bucket.to_u32(),
                                ..source.expected.item
                            },
                            source.expected.can_break,
                            source.expected.can_place_on,
                        )),
                    )
                {
                    return Err(RejectReason::InvalidState);
                }
            }
            Item::Bucket => {
                let mut filled_wire = source.expected.item;
                filled_wire.item = Item::WaterBucket.to_u32();
                filled_wire.count = 1;
                if !candidate.consume_slot_exact(source)
                    || !candidate.add_slot(contract::SessionInventorySlot::from_wire(
                        filled_wire,
                        source.expected.can_break,
                        source.expected.can_place_on,
                    ))
                {
                    return Err(RejectReason::InvalidState);
                }
            }
            _ => return Err(RejectReason::InvalidState),
        }
        if !preserves_brew_locks(&original, &candidate) {
            return Err(RejectReason::InvalidState);
        }

        let Some(dimension) = Dimension::from_wire(dimension) else {
            return Err(RejectReason::InvalidDimension);
        };
        let mutation = self.with_world(dimension, |world| {
            world.apply_fluid_use(position, face, source_item)
        })?;
        let Some(mutation) = mutation else {
            return Err(RejectReason::InvalidState);
        };
        let Some(session) = self.sessions.get_mut(&session_id) else {
            return Err(RejectReason::Unauthorized);
        };
        session.gameplay = candidate;
        Ok(Some(mutation))
    }

    fn apply_fishing(
        &mut self,
        session_id: PlayerId,
        action: u8,
        hand: u8,
        look_milli: [i16; 3],
    ) -> Result<Option<WorldMutation>, RejectReason> {
        use crate::authority::fishing;
        use crate::inventory::GameMode;

        let Some((position, game_mode, original)) = self
            .sessions
            .get(&session_id)
            .map(|session| (session.position, session.game_mode, session.gameplay))
        else {
            return Err(RejectReason::Unauthorized);
        };
        let mut candidate = original;
        let previous_hook = candidate.fishing_hook;
        match action {
            0 => {
                let rod_slot = held_slot_index(&candidate, hand)?;
                if transactions::brew_locks_slot(&candidate, rod_slot) {
                    return Err(RejectReason::InvalidState);
                }
                let hook_id = self.next_unique_entity_id();
                let context = fishing::FishingDomainContext {
                    world_seed: self.world.seed as u64
                        ^ (u64::from(self.world.dimension as u8) << 32),
                    hook_entity_id: hook_id,
                    player_position_milli: position_to_milli(position)?,
                    open_water: false,
                    water_surface_y_milli: None,
                    consume_durability: game_mode != GameMode::Creative,
                };
                fishing::cast(&mut candidate, session_id, hand, look_milli, context)
                    .map_err(map_fishing_error)?;
                self.claim_entity_id(hook_id);
            }
            1 => {
                let rod_slot = held_slot_index(&candidate, hand)?;
                if transactions::brew_locks_slot(&candidate, rod_slot) {
                    return Err(RejectReason::InvalidState);
                }
                let context = self
                    .world
                    .fishing_context(&candidate, position, game_mode != GameMode::Creative)
                    .map_err(map_fishing_error)?;
                fishing::reel(&mut candidate, session_id, hand, context)
                    .map_err(map_fishing_error)?;
            }
            2 => {
                let context = self
                    .world
                    .fishing_context(&candidate, position, game_mode != GameMode::Creative)
                    .map_err(map_fishing_error)?;
                fishing::cancel(&mut candidate, hand, context).map_err(map_fishing_error)?;
            }
            _ => return Err(RejectReason::InvalidState),
        }
        self.world
            .sync_authority_hook(previous_hook, candidate.fishing_hook, session_id);
        let Some(session) = self.sessions.get_mut(&session_id) else {
            return Err(RejectReason::Unauthorized);
        };
        session.gameplay = candidate;
        Ok(None)
    }

    fn apply_transaction_operation(
        &mut self,
        session_id: PlayerId,
        operation: &GameplayOperation,
    ) -> Result<Option<WorldMutation>, RejectReason> {
        use crate::authority::transactions::{self, WorkstationContext};
        use crate::inventory::Item;

        let Some(original) = self
            .sessions
            .get(&session_id)
            .map(|session| session.gameplay)
        else {
            return Err(RejectReason::Unauthorized);
        };
        let mut candidate = original;
        let mut mutation = None;
        match operation {
            GameplayOperation::FurnaceTakeOutput { x, y, z, count } => {
                mutation = Some(self.world.take_furnace_output(
                    &mut candidate,
                    [*x, *y, *z],
                    *count,
                )?);
            }
            GameplayOperation::Craft {
                grid,
                sources,
                station,
            } => {
                if sources
                    .iter()
                    .flatten()
                    .any(|source| transactions::brew_locks_slot(&candidate, source.index))
                {
                    return Err(RejectReason::InvalidState);
                }
                let context = match (*grid, *station) {
                    (2, None) => WorkstationContext::personal_crafting(),
                    (3, Some(position)) => WorkstationContext::at(
                        position,
                        self.world.get_block(position[0], position[1], position[2]),
                    ),
                    _ => return Err(RejectReason::InvalidState),
                };
                transactions::execute_craft(
                    &mut candidate,
                    &self.world.recipe_manager,
                    context,
                    *grid,
                    *sources,
                )
                .map_err(map_transaction_error)?;
            }
            GameplayOperation::Enchant {
                x,
                y,
                z,
                source,
                option,
            } => {
                if transactions::brew_locks_slot(&candidate, source.index) {
                    return Err(RejectReason::InvalidState);
                }
                let position = [*x, *y, *z];
                let context = WorkstationContext::enchanting(
                    position,
                    self.world.get_block(*x, *y, *z),
                    self.world.bookshelf_power(position),
                );
                transactions::execute_enchant(&mut candidate, context, *source, *option)
                    .map_err(map_transaction_error)?;
                if !preserves_brew_locks(&original, &candidate) {
                    return Err(RejectReason::InvalidState);
                }
            }
            GameplayOperation::Brew {
                action,
                x,
                y,
                z,
                ingredient,
                bottles,
            } => {
                let position = [*x, *y, *z];
                let context = WorkstationContext::at(position, self.world.get_block(*x, *y, *z));
                match *action {
                    0 => {
                        let ingredient = ingredient.ok_or(RejectReason::InvalidState)?;
                        transactions::start_brew(&mut candidate, context, ingredient, *bottles)
                            .map_err(map_transaction_error)?;
                    }
                    1 if ingredient.is_none() && bottles.iter().all(Option::is_none) => {
                        transactions::cancel_brew(&mut candidate, context)
                            .map_err(map_transaction_error)?;
                    }
                    2 if ingredient.is_none() && bottles.iter().all(Option::is_none) => {
                        transactions::take_brew(&mut candidate, context)
                            .map_err(map_transaction_error)?;
                    }
                    _ => return Err(RejectReason::InvalidState),
                }
            }
            GameplayOperation::Anvil {
                x,
                y,
                z,
                left,
                right,
                rename,
            } => {
                if transactions::brew_locks_slot(&candidate, left.index)
                    || right.is_some_and(|source| {
                        transactions::brew_locks_slot(&candidate, source.index)
                    })
                {
                    return Err(RejectReason::InvalidState);
                }
                let position = [*x, *y, *z];
                let context = WorkstationContext::at(position, self.world.get_block(*x, *y, *z));
                transactions::execute_anvil(&mut candidate, context, *left, *right, rename)
                    .map_err(map_transaction_error)?;
            }
            GameplayOperation::UseState { hand, active } => {
                if *active {
                    let slot = held_slot_index(&candidate, *hand)?;
                    let held =
                        candidate.inventory[usize::from(slot)].ok_or(RejectReason::InvalidState)?;
                    if held.item.item != Item::Shield.to_u32()
                        || held.item.count != 1
                        || held.item.durability == 0
                        || candidate.shield_cooldown_ticks > 0
                    {
                        return Err(RejectReason::InvalidState);
                    }
                }
                candidate.shield_active = *active;
            }
            _ => return Err(RejectReason::Unsupported),
        }
        if !matches!(operation, GameplayOperation::Brew { .. })
            && !preserves_brew_locks(&original, &candidate)
        {
            return Err(RejectReason::InvalidState);
        }
        let Some(session) = self.sessions.get_mut(&session_id) else {
            return Err(RejectReason::Unauthorized);
        };
        session.gameplay = candidate;
        Ok(mutation)
    }

    fn apply_authoritative_combat(
        &mut self,
        request: &GameplayRequest,
        session_id: PlayerId,
        target: u64,
        action: u8,
    ) -> Result<Option<WorldMutation>, RejectReason> {
        use crate::authority::combat::{
            self, AuthorityDamageInput, CombatantId, DamageEvent, EntityCombatSnapshot,
            PlayerCombatSnapshot,
        };
        use crate::inventory::GameMode;
        use crate::player::DamageSource;

        if action != 0 || target == 0 || target == session_id {
            return Err(RejectReason::InvalidState);
        }
        let Some(attacker) = self.sessions.get(&session_id).cloned() else {
            return Err(RejectReason::Unauthorized);
        };
        if attacker.gameplay.is_dead {
            return Err(RejectReason::InvalidState);
        }
        let attacker_position_milli = position_to_milli(attacker.position)?;
        let attacker_look_milli = look_from_angles(attacker.yaw, attacker.pitch)?;
        let profile = combat_profile(&attacker.gameplay)?;
        let cooldown_ready = attacker.gameplay.attack_cooldown_ticks >= ATTACK_COOLDOWN_TICKS;

        if let Some(target_session) = self.sessions.get(&target).cloned() {
            if !self.world.rules.pvp
                || target_session.dimension != attacker.dimension
                || matches!(
                    target_session.game_mode,
                    GameMode::Creative | GameMode::Spectator
                )
            {
                return Err(RejectReason::PermissionDenied);
            }
            let target_position_milli = position_to_milli(target_session.position)?;
            let event = DamageEvent::from_authority(AuthorityDamageInput {
                event_id: request.request_id,
                attacker: CombatantId::Player(session_id),
                target: CombatantId::Player(target),
                source: DamageSource::Mob,
                base_damage_milli: profile.base_damage_milli,
                attacker_position_milli,
                target_position_milli,
                attacker_look_milli,
                target_look_milli: look_from_angles(target_session.yaw, target_session.pitch)?,
                cooldown_ready,
                has_line_of_sight: self
                    .world
                    .has_line_of_sight(attacker.position, target_session.position),
                attacker_used_axe: profile.used_axe,
                knockback_milli: profile.knockback_milli,
                fire_ticks: profile.fire_ticks,
                looting_level: profile.looting_level,
            })
            .map_err(map_combat_error)?;
            let mut target_snapshot = PlayerCombatSnapshot {
                player_id: target,
                gameplay: target_session.gameplay,
                velocity_milli: target_session.gameplay.velocity_milli,
                last_applied_event: None,
            };
            let outcome = combat::resolve_player_hit(&event, &mut target_snapshot)
                .map_err(map_combat_error)?;
            target_snapshot.gameplay.velocity_milli = target_snapshot.velocity_milli;
            let mut attacker_gameplay = attacker.gameplay;
            attacker_gameplay.attack_cooldown_ticks = 0;

            if outcome.death.is_some() && !self.world.rules.keep_inventory {
                target_snapshot.gameplay.inventory = [None; contract::SESSION_INVENTORY_SLOTS];
                target_snapshot.gameplay.experience = 0;
                target_snapshot.gameplay.experience_level = 0;
            }
            if outcome.death.is_some() {
                target_snapshot.gameplay.mounted_entity = None;
                self.world.remove_passenger(target);
            }
            self.sessions
                .get_mut(&session_id)
                .ok_or(RejectReason::Unauthorized)?
                .gameplay = attacker_gameplay;
            self.sessions
                .get_mut(&target)
                .ok_or(RejectReason::InvalidState)?
                .gameplay = target_snapshot.gameplay;
            self.pending_session_revisions.insert(target);
            if !self.world.rules.keep_inventory {
                if let Some(death) = outcome.death {
                    self.spawn_death_outcome(target_session.position, death);
                }
            }
            return Ok(None);
        }

        let Some(entity) = self.world.entities.get_by_id(target) else {
            return Err(RejectReason::InvalidState);
        };
        let target_entity_type = entity.entity_type;
        let target_position = entity.position.to_array();
        let target_bounds = entity.get_aabb();
        let attacker_position = glam::Vec3::from_array(attacker.position);
        let target_hit_position = attacker_position.clamp(target_bounds.min, target_bounds.max);
        let mut target_snapshot = EntityCombatSnapshot {
            entity_id: entity.id,
            entity_type: entity.entity_type,
            health_milli: quantize_health(entity.health),
            max_health_milli: quantize_health(entity.max_health),
            velocity_milli: position_to_milli(entity.velocity.to_array())?,
            armor_points_milli: 0,
            toughness_milli: 0,
            enchantment_protection_factor: 0,
            knockback_resistance_milli: 0,
            invulnerability_ticks: (entity.invulnerable_time.max(0.0) * 20.0)
                .round()
                .min(u16::MAX as f32) as u16,
            fire_ticks_remaining: (entity.fire_aspect_timer.max(0.0) * 20.0)
                .round()
                .min(u16::MAX as f32) as u16,
            has_wool: entity.has_wool,
            death_source: None,
            death_settled: entity.player_kill_rewarded,
            last_applied_event: None,
        };
        let event = DamageEvent::from_authority(AuthorityDamageInput {
            event_id: request.request_id,
            attacker: CombatantId::Player(session_id),
            target: CombatantId::Entity(target),
            source: DamageSource::Mob,
            base_damage_milli: profile.base_damage_milli,
            attacker_position_milli,
            target_position_milli: position_to_milli(target_hit_position.to_array())?,
            attacker_look_milli,
            target_look_milli: look_from_angles(entity.yaw, entity.pitch)?,
            cooldown_ready,
            has_line_of_sight: self
                .world
                .has_line_of_sight(attacker.position, target_hit_position.to_array()),
            attacker_used_axe: profile.used_axe,
            knockback_milli: profile.knockback_milli,
            fire_ticks: profile.fire_ticks,
            looting_level: profile.looting_level,
        })
        .map_err(map_combat_error)?;
        let outcome =
            combat::resolve_entity_hit(&event, &mut target_snapshot).map_err(map_combat_error)?;

        let mut attacker_gameplay = attacker.gameplay;
        attacker_gameplay.attack_cooldown_ticks = 0;
        self.sessions
            .get_mut(&session_id)
            .ok_or(RejectReason::Unauthorized)?
            .gameplay = attacker_gameplay;
        if target_snapshot.health_milli == 0 {
            let _ = self.world.entities.remove_by_id(target);
            if target_entity_type == crate::entity::EntityType::EnderDragon {
                self.world.handle_dragon_completion();
            }
        } else if let Some(entity) = self.world.entities.get_by_id_mut(target) {
            entity.health = target_snapshot.health_milli as f32 / 1_000.0;
            entity.velocity = glam::Vec3::new(
                target_snapshot.velocity_milli[0] as f32 / 1_000.0,
                target_snapshot.velocity_milli[1] as f32 / 1_000.0,
                target_snapshot.velocity_milli[2] as f32 / 1_000.0,
            );
            entity.invulnerable_time = f32::from(target_snapshot.invulnerability_ticks) / 20.0;
            entity.fire_aspect_timer = f32::from(target_snapshot.fire_ticks_remaining) / 20.0;
            entity.player_kill_rewarded = target_snapshot.death_settled;
        }
        if let Some(death) = outcome.death {
            self.spawn_death_outcome(target_position, death);
        }
        Ok(None)
    }

    fn spawn_death_outcome(&mut self, position: [f32; 3], death: combat::DeathOutcome) {
        for slot in death.drops {
            let id = self.next_unique_entity_id();
            self.claim_entity_id(id);
            let _ = self.world.spawn_authority_drop(id, slot, position);
        }
        if death.experience > 0 {
            let id = self.next_unique_entity_id();
            self.claim_entity_id(id);
            let _ = self
                .world
                .spawn_authority_experience(id, death.experience, position);
        }
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
        let Some(session) = self.sessions.get_mut(&id) else {
            return false;
        };
        session.gameplay = gameplay;
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
        self.cleanup_session_lifecycle(id, dimension);
        self.activate_dimension(dimension);
        let hardcore = self.world.rules.hardcore;
        let revision = self.world.revisions.allocate();
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
        true
    }

    /// Commands that mutate authenticated session state (rather than world
    /// voxels) still execute in the same core.  `State` only projects the
    /// resulting session snapshot and never edits its local player position or
    /// game mode as authority.
    fn dispatch_session_command(
        &mut self,
        request: &GameplayRequest,
        session_id: PlayerId,
    ) -> Option<Result<Option<WorldMutation>, RejectReason>> {
        let crate::network::protocol::GameplayOperation::Command { command } = &request.operation
        else {
            return None;
        };
        if command.trim().eq_ignore_ascii_case("/respawn") {
            return Some(if self.respawn_session(session_id) {
                Ok(None)
            } else {
                Err(RejectReason::Unauthorized)
            });
        }
        let parsed = match crate::commands::parse(command) {
            Ok(parsed) => parsed,
            Err(_) => return Some(Err(RejectReason::InvalidState)),
        };
        match parsed {
            crate::commands::Command::GameMode { mode, target } => {
                if target.is_some_and(|target| {
                    !matches!(
                        target,
                        crate::commands::CommandTarget::SelfPlayer
                            | crate::commands::CommandTarget::NearestPlayer
                    )
                }) {
                    return Some(Err(RejectReason::PermissionDenied));
                }
                let Some(session) = self.sessions.get_mut(&session_id) else {
                    return Some(Err(RejectReason::Unauthorized));
                };
                session.game_mode = mode;
                Some(Ok(None))
            }
            crate::commands::Command::Teleport { target, position } => {
                if !matches!(
                    target,
                    crate::commands::CommandTarget::SelfPlayer
                        | crate::commands::CommandTarget::NearestPlayer
                ) {
                    return Some(Err(RejectReason::PermissionDenied));
                }
                if !self.world.dimension.height().contains_y(position[1]) {
                    return Some(Err(RejectReason::InvalidCoordinate));
                }
                let Some(session) = self.sessions.get_mut(&session_id) else {
                    return Some(Err(RejectReason::Unauthorized));
                };
                session.position = [
                    position[0] as f32 + 0.5,
                    position[1] as f32,
                    position[2] as f32 + 0.5,
                ];
                Some(Ok(None))
            }
            crate::commands::Command::Give {
                target,
                item,
                count,
            } => {
                if !matches!(
                    target,
                    crate::commands::CommandTarget::SelfPlayer
                        | crate::commands::CommandTarget::NearestPlayer
                ) {
                    return Some(Err(RejectReason::PermissionDenied));
                }
                let Some(session) = self.sessions.get_mut(&session_id) else {
                    return Some(Err(RejectReason::Unauthorized));
                };
                let stack = crate::inventory::ItemStack::new(item, count);
                let slot = SessionInventorySlot::from_wire(
                    crate::network::protocol::ItemWire::from_stack(&stack),
                    stack.can_break,
                    stack.can_place_on,
                );
                if !session.gameplay.add_slot(slot) {
                    return Some(Err(RejectReason::InvalidState));
                }
                Some(Ok(None))
            }
            _ => None,
        }
    }

    fn rejected(&mut self, request_id: u128, reason: RejectReason) -> GameplayResponse {
        let revision = self.world.revisions.allocate();
        GameplayResponse {
            request_id,
            server_sequence: revision,
            outcome: GameplayOutcome::Rejected { reason },
        }
    }

    fn reject_for_session(
        &mut self,
        session_id: PlayerId,
        request_id: u128,
        reason: RejectReason,
        consumed_sequence: Option<u64>,
    ) -> GameplayResponse {
        let response = self.rejected(request_id, reason);
        if let Some(session) = self.sessions.get_mut(&session_id) {
            if let Some(sequence) = consumed_sequence {
                session.last_client_sequence = sequence;
            }
            session.cache_response(response.clone());
        }
        response
    }

    pub fn common_vector_snapshot(&mut self) -> Vec<(GameplayResponse, AuthoritySnapshot)> {
        let mut responses = Vec::new();
        for request in common_gameplay_vectors() {
            let response = self.submit_request(request);
            responses.push((response, self.last_snapshot().clone()));
        }
        responses
    }

    pub fn world_mutations(&self) -> &[WorldMutation] {
        &self.last_snapshot.mutations
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

fn stack_from_slot(slot: Option<SessionSlotWire>) -> Option<crate::inventory::ItemStack> {
    let slot = slot?;
    let mut stack = slot.item.to_stack()?;
    stack.can_break = slot.can_break;
    stack.can_place_on = slot.can_place_on;
    Some(stack)
}

fn held_slot_index(gameplay: &SessionGameplayState, hand: u8) -> Result<u8, RejectReason> {
    match hand {
        0 if gameplay.selected_hotbar_slot < 9 => Ok(gameplay.selected_hotbar_slot),
        1 => Ok((contract::SESSION_INVENTORY_SLOTS - 1) as u8),
        _ => Err(RejectReason::InvalidState),
    }
}

fn preserves_brew_locks(before: &SessionGameplayState, after: &SessionGameplayState) -> bool {
    (0..contract::SESSION_INVENTORY_SLOTS).all(|index| {
        !transactions::brew_locks_slot(before, index as u8)
            || before.inventory[index] == after.inventory[index]
    })
}

fn position_to_milli(position: [f32; 3]) -> Result<[i32; 3], RejectReason> {
    let mut result = [0; 3];
    for (index, value) in position.into_iter().enumerate() {
        if !value.is_finite() || value.abs() > 2_000_000.0 {
            return Err(RejectReason::InvalidState);
        }
        result[index] = (value * 1_000.0).round() as i32;
    }
    Ok(result)
}

fn map_fishing_error(error: fishing::FishingDomainError) -> RejectReason {
    match error {
        fishing::FishingDomainError::HookTooFar => RejectReason::TooFar,
        fishing::FishingDomainError::InvalidContext
        | fishing::FishingDomainError::InvalidHand
        | fishing::FishingDomainError::InvalidSelectedSlot
        | fishing::FishingDomainError::MissingRod
        | fishing::FishingDomainError::InvalidRod
        | fishing::FishingDomainError::HookAlreadyActive
        | fishing::FishingDomainError::NoActiveHook
        | fishing::FishingDomainError::StaleHook
        | fishing::FishingDomainError::CorruptHook
        | fishing::FishingDomainError::InventoryFull
        | fishing::FishingDomainError::ExperienceOverflow => RejectReason::InvalidState,
    }
}

fn map_transaction_error(_error: transactions::TransactionError) -> RejectReason {
    RejectReason::InvalidState
}

#[derive(Debug, Clone, Copy)]
struct CombatProfile {
    base_damage_milli: u32,
    used_axe: bool,
    knockback_milli: u32,
    fire_ticks: u16,
    looting_level: u8,
}

fn combat_profile(gameplay: &SessionGameplayState) -> Result<CombatProfile, RejectReason> {
    use crate::enchantment::{attack_damage_bonus, Enchantment};
    use crate::inventory::ToolType;

    let selected = usize::from(gameplay.selected_hotbar_slot);
    if selected >= 9 {
        return Err(RejectReason::InvalidState);
    }
    let stack = gameplay.inventory[selected]
        .map(|slot| slot.item.to_stack().ok_or(RejectReason::InvalidState))
        .transpose()?;
    let tool = stack
        .as_ref()
        .and_then(|stack| stack.item.tool_properties());
    let enchantments = stack
        .as_ref()
        .map(|stack| stack.enchantments)
        .unwrap_or_default();
    let base = tool.map(|tool| tool.damage).unwrap_or(1.0) + attack_damage_bonus(&enchantments);
    Ok(CombatProfile {
        base_damage_milli: (base.max(0.001) * 1_000.0).round().clamp(1.0, 100_000.0) as u32,
        used_axe: tool.is_some_and(|tool| tool.tool_type == ToolType::Axe),
        knockback_milli: 400 + u32::from(enchantments.level_of(Enchantment::Knockback(1))) * 500,
        fire_ticks: u16::from(enchantments.level_of(Enchantment::FireAspect(1))) * 80,
        looting_level: enchantments.level_of(Enchantment::Looting(1)).min(3),
    })
}

fn look_from_angles(yaw: f32, pitch: f32) -> Result<[i16; 3], RejectReason> {
    if !yaw.is_finite() || !pitch.is_finite() || pitch.abs() > 90.0 {
        return Err(RejectReason::InvalidState);
    }
    let yaw = yaw.to_radians();
    let pitch = pitch.to_radians();
    let horizontal = pitch.cos();
    let look = [
        (-yaw.sin() * horizontal * 1_000.0).round() as i16,
        (-pitch.sin() * 1_000.0).round() as i16,
        (yaw.cos() * horizontal * 1_000.0).round() as i16,
    ];
    Ok(look)
}

fn quantize_health(health: f32) -> u32 {
    if health.is_finite() {
        (health.max(0.0) * 1_000.0).round().min(u32::MAX as f32) as u32
    } else {
        u32::MAX
    }
}

fn map_combat_error(error: combat::CombatReject) -> RejectReason {
    match error {
        combat::CombatReject::OutOfRange => RejectReason::TooFar,
        combat::CombatReject::ReplayedEvent => RejectReason::Duplicate,
        combat::CombatReject::InvalidEvent
        | combat::CombatReject::IdentityMismatch
        | combat::CombatReject::Cooldown
        | combat::CombatReject::NoLineOfSight
        | combat::CombatReject::NotFacingTarget
        | combat::CombatReject::TargetDead
        | combat::CombatReject::TargetInvulnerable
        | combat::CombatReject::InvalidTargetState => RejectReason::InvalidState,
    }
}

fn aggregate_dimension_checksums(checksums: &[(Dimension, u64)]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for (dimension, checksum) in checksums {
        hash ^= u64::from(*dimension as u8);
        hash = hash.wrapping_mul(0x100000001b3);
        for byte in checksum.to_le_bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authority::contract::{SessionGameplayState, SessionInventorySlot};
    use crate::entity::EntityType;
    use crate::inventory::Item;
    use crate::network::protocol::{BlockActionKind, GameplayOperation, GameplayOutcome};
    use crate::world::BlockType;
    use contract::SessionContract;

    fn core(topology: AuthorityTopology) -> AuthorityCore {
        let mut core = AuthorityCore::new(AuthorityConfig::default(), topology);
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
                block: if matches!(
                    action,
                    BlockActionKind::Place
                        | BlockActionKind::IgnitePortal
                        | BlockActionKind::InsertEnderEye
                        | BlockActionKind::EnterPortal
                ) {
                    block.to_wire()
                } else {
                    BlockType::Air.to_wire()
                },
                look_milli,
            },
        }
    }

    #[test]
    fn authority_collects_dropped_items_and_projects_inventory_revision() {
        let mut core = core(AuthorityTopology::Singleplayer);
        core.world.rules.do_mob_spawning = false;
        let entity_id = 99_001;
        assert!(core.world.spawn_dropped_item(
            entity_id,
            [8.25, 80.0, 8.25],
            ItemStack::new(Item::Diamond, 3),
        ));
        assert!(core.world.spawn_experience_orb(99_002, [8.5, 80.0, 8.5], 9));

        let before_revision = core.session(7).unwrap().gameplay.revision;
        let snapshot = core.tick();

        assert!(core.world.entities.get_by_id(entity_id).is_none());
        let collected = core
            .session(7)
            .unwrap()
            .gameplay
            .inventory
            .iter()
            .flatten()
            .find(|slot| slot.item.item == Item::Diamond as u32)
            .copied()
            .expect("diamond drop should enter the authority inventory");
        assert_eq!(collected.item.count, 3);
        assert!(core.world.entities.get_by_id(99_002).is_none());
        assert_eq!(core.session(7).unwrap().gameplay.experience_level, 1);
        assert_eq!(core.session(7).unwrap().gameplay.experience, 2);
        assert!(core.session(7).unwrap().gameplay.revision > before_revision);
        assert!(snapshot
            .session_updates
            .iter()
            .any(|update| update.player_id == 7 && update.state.revision > before_revision));
    }

    #[test]
    fn duplicate_and_stale_revision_are_authoritative() {
        let mut core = core(AuthorityTopology::Dedicated);
        let mut request = GameplayRequest {
            request_id: 1,
            client_sequence: 1,
            session_id: 7,
            dimension: 0,
            client_revision: 0,
            operation: GameplayOperation::BlockUse {
                x: 8,
                y: 80,
                z: 8,
                block: 3,
            },
        };
        let first = core.submit_request(request.clone());
        let duplicate = core.submit_request(request.clone());
        assert_eq!(first, duplicate);
        request.request_id = 2;
        request.client_sequence = 2;
        request.client_revision = core.current_revision() + 1;
        assert!(matches!(
            core.submit_request(request).outcome,
            GameplayOutcome::Rejected {
                reason: RejectReason::InvalidRevision
            }
        ));
    }

    #[test]
    fn authenticated_rejections_are_cached_without_consuming_sequence() {
        let mut core = core(AuthorityTopology::Dedicated);
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
    fn request_mutation_is_drained_into_the_next_authority_snapshot() {
        let mut core = core(AuthorityTopology::Singleplayer);
        let request = GameplayRequest {
            request_id: 21,
            client_sequence: 1,
            session_id: 7,
            dimension: 0,
            client_revision: 0,
            operation: GameplayOperation::BlockUse {
                x: 8,
                y: 80,
                z: 8,
                block: 3,
            },
        };
        let response = core.submit_request(request);
        assert!(matches!(response.outcome, GameplayOutcome::Accepted { .. }));
        let pending = core.take_pending_mutations();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].position, (8, 80, 8));
    }

    #[test]
    fn typed_mining_fixed_tick_breaks_once_and_cancel_is_idempotent() {
        let mut core = core(AuthorityTopology::Dedicated);
        let target = (8, 81, 9);
        core.world
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
            core.world.get_block(target.0, target.1, target.2),
            BlockType::Air
        );
        assert_eq!(target_mutations, 1);
        assert!(core.session(7).unwrap().gameplay.mining.is_none());
        assert_eq!(
            core.world
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
            core.world
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
        let mut core = core(AuthorityTopology::Dedicated);
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

        let mut creative = core(AuthorityTopology::Singleplayer);
        creative.session_mut(7).unwrap().game_mode = crate::inventory::GameMode::Creative;
        creative
            .world
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
            creative.world.get_block(target.0, target.1, target.2),
            BlockType::Air
        );
        assert_eq!(
            creative.session(7).unwrap().gameplay.inventory[0],
            Some(SessionInventorySlot::from(pick_wire))
        );
        assert!(creative
            .world
            .entities
            .entities
            .iter()
            .all(|entity| entity.entity_type != EntityType::ExperienceOrb));

        let mut adventure = core(AuthorityTopology::Dedicated);
        adventure.session_mut(7).unwrap().game_mode = crate::inventory::GameMode::Adventure;
        adventure
            .world
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
            adventure.world.get_block(target.0, target.1, target.2),
            BlockType::Air
        );

        let mut denied = core(AuthorityTopology::Dedicated);
        denied.session_mut(7).unwrap().game_mode = crate::inventory::GameMode::Adventure;
        denied
            .world
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
            denied.world.get_block(target.0, target.1, target.2),
            BlockType::Stone
        );

        let mut empty_hand = core(AuthorityTopology::Dedicated);
        empty_hand
            .world
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
        let mut core = core(AuthorityTopology::Dedicated);
        core.world
            .set_block(support.0, support.1, support.2, BlockType::Stone, 0)
            .unwrap();
        core.world
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
            core.world.get_block(target.0, target.1, target.2),
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
        assert_eq!(core.world.get_block(8, 81, 10), BlockType::Air);

        // The same typed path is valid across an explicitly loaded chunk
        // boundary; unloaded front/support chunks are never synthesized by
        // the action itself.
        core.world.ensure_chunk(1, 0);
        core.session_mut(7).unwrap().position = [15.0, 80.0, 8.0];
        let support_cross = (16, 80, 8);
        let target_cross = (16, 81, 8);
        core.world
            .set_block(
                support_cross.0,
                support_cross.1,
                support_cross.2,
                BlockType::Stone,
                0,
            )
            .unwrap();
        core.world
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
            core.world
                .get_block(target_cross.0, target_cross.1, target_cross.2),
            BlockType::Stone
        );
        assert!(core.session(7).unwrap().gameplay.inventory[0].is_none());
    }

    #[test]
    fn typed_portal_items_reach_end_eye_and_nether_ignition_actions() {
        let target = (8, 81, 9);
        let eye_stack = ItemStack::new(Item::EyeOfEnder, 1);
        let eye_wire =
            crate::network::protocol::SessionSlotWire::new(ItemWire::from_stack(&eye_stack), 0, 0);
        let mut end = core(AuthorityTopology::Singleplayer);
        end.world
            .set_block(target.0, target.1, target.2, BlockType::EndPortalFrame, 0)
            .unwrap();
        let mut gameplay = SessionGameplayState::default();
        gameplay.inventory[0] = Some(SessionInventorySlot::from(eye_wire));
        end.set_session_gameplay(7, gameplay);
        assert_eq!(
            end.world.get_block(target.0, target.1, target.2),
            BlockType::EndPortalFrame
        );
        assert_eq!(
            end.session(7).unwrap().gameplay.inventory[0],
            Some(SessionInventorySlot::from(eye_wire))
        );
        assert!(end
            .world
            .has_block_line_of_sight([8.0, 80.0, 8.0], [315, -76, 946], target));
        let inserted = end.submit_request(block_request(
            117,
            1,
            BlockActionKind::InsertEnderEye,
            target,
            Some(eye_wire),
            BlockType::EndPortalFrameFilled,
            [0, 1, 0],
            [315, -76, 946],
            0,
        ));
        assert!(
            matches!(inserted.outcome, GameplayOutcome::Accepted { .. }),
            "unexpected end-eye response: {:?}",
            inserted.outcome
        );
        assert_eq!(
            end.world.get_block(target.0, target.1, target.2),
            BlockType::EndPortalFrameFilled
        );
        assert!(end.session(7).unwrap().gameplay.inventory[0].is_none());

        let mut nether = core(AuthorityTopology::Singleplayer);
        // Complete 4x5 X-axis frame at z=9; target is its lower interior.
        for x in 7..=10 {
            nether
                .world
                .set_block(x, 80, 9, BlockType::Obsidian, 0)
                .unwrap();
            nether
                .world
                .set_block(x, 84, 9, BlockType::Obsidian, 0)
                .unwrap();
        }
        for y in 81..=83 {
            nether
                .world
                .set_block(7, y, 9, BlockType::Obsidian, 0)
                .unwrap();
            nether
                .world
                .set_block(10, y, 9, BlockType::Obsidian, 0)
                .unwrap();
        }
        let flint_stack = ItemStack::new(Item::FlintAndSteel, 1);
        let flint_wire = crate::network::protocol::SessionSlotWire::new(
            ItemWire::from_stack(&flint_stack),
            0,
            0,
        );
        let mut gameplay = SessionGameplayState::default();
        gameplay.inventory[0] = Some(SessionInventorySlot::from(flint_wire));
        nether.set_session_gameplay(7, gameplay);
        let ignited = nether.submit_request(block_request(
            118,
            1,
            BlockActionKind::IgnitePortal,
            target,
            Some(flint_wire),
            BlockType::Fire,
            [0, 1, 0],
            [257, -575, 771],
            0,
        ));
        assert!(
            matches!(ignited.outcome, GameplayOutcome::Accepted { .. }),
            "unexpected nether ignition response: {:?}",
            ignited.outcome
        );
        for x in 8..=9 {
            for y in 81..=83 {
                assert_eq!(nether.world.get_block(x, y, 9), BlockType::NetherPortal);
            }
        }
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
            core.world
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

        let mut cancelled = core(AuthorityTopology::Dedicated);
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
            cancelled.world.get_block(target.0, target.1, target.2),
            BlockType::Stone
        );

        let mut held_changed = core(AuthorityTopology::Dedicated);
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
            held_changed.world.get_block(target.0, target.1, target.2),
            BlockType::Stone
        );

        let mut slot_switched = core(AuthorityTopology::Dedicated);
        start(&mut slot_switched, 125);
        let mut switched = slot_switched.session(7).unwrap().gameplay;
        switched.selected_hotbar_slot = 1;
        switched.inventory[0] = Some(SessionInventorySlot::from(pick_wire));
        switched.inventory[1] = Some(SessionInventorySlot::from(pick_wire));
        slot_switched.set_session_gameplay(7, switched);
        let _ = slot_switched.tick();
        assert!(slot_switched.session(7).unwrap().gameplay.mining.is_none());
        assert_eq!(
            slot_switched.world.get_block(target.0, target.1, target.2),
            BlockType::Stone
        );

        let mut moved = core(AuthorityTopology::Dedicated);
        start(&mut moved, 123);
        moved.session_mut(7).unwrap().position = [30.0, 80.0, 30.0];
        let _ = moved.tick();
        assert!(moved.session(7).unwrap().gameplay.mining.is_none());
        assert_eq!(
            moved.world.get_block(target.0, target.1, target.2),
            BlockType::Stone
        );

        let mut replaced = core(AuthorityTopology::Dedicated);
        start(&mut replaced, 124);
        replaced
            .world
            .set_block(target.0, target.1, target.2, BlockType::Dirt, 0)
            .unwrap();
        let _ = replaced.tick();
        assert!(replaced.session(7).unwrap().gameplay.mining.is_none());
        assert_eq!(
            replaced.world.get_block(target.0, target.1, target.2),
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
        let mut core = core(AuthorityTopology::Dedicated);
        core.session_mut(7).unwrap().game_mode = crate::inventory::GameMode::Adventure;
        core.world
            .set_block(support.0, support.1, support.2, BlockType::Stone, 0)
            .unwrap();
        core.world
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
            .world
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
            core.world.get_block(target.0, target.1, target.2),
            BlockType::Air
        );
        assert!(core
            .world
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
        let mut core = core(AuthorityTopology::Dedicated);
        core.world
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
        let mut core = core(AuthorityTopology::Dedicated);
        core.world
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
            core.world.get_block(target.0, target.1, target.2),
            BlockType::Air
        );
        assert_eq!(core.session(7).unwrap().gameplay.experience, 4);
        assert_eq!(core.session(7).unwrap().gameplay.experience_level, 1);
        assert!(core.session(7).unwrap().gameplay.inventory[0].is_none());
        assert!(core
            .world
            .entities
            .entities
            .iter()
            .all(|entity| entity.entity_type != EntityType::ExperienceOrb));
        let dropped = core
            .world
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
        let mut core = core(AuthorityTopology::Dedicated);
        let lever = (7, 80, 8);
        let source = (8, 80, 8);
        core.world
            .set_block(lever.0, lever.1, lever.2, BlockType::LeverOn, 0)
            .unwrap();
        core.world
            .set_block(source.0, source.1, source.2, BlockType::Dispenser, 0)
            .unwrap();
        core.world.redstone.on_block_changed(
            &core.world.chunks,
            lever,
            crate::redstone::Direction::East,
        );
        core.world.redstone.on_block_changed(
            &core.world.chunks,
            source,
            crate::redstone::Direction::South,
        );
        if let Some(entity) = core
            .world
            .chunks
            .get_block_entity_mut(source.0, source.1, source.2)
        {
            entity.set_stack(0, Some(crate::inventory::ItemStack::new(Item::Arrow, 2)));
        }

        let first = core.tick();
        assert_eq!(
            core.world
                .entities
                .entities
                .iter()
                .filter(|entity| entity.entity_type == EntityType::Arrow)
                .count(),
            1
        );
        let arrow_id = core
            .world
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
            core.world
                .entities
                .entities
                .iter()
                .filter(|entity| entity.entity_type == EntityType::Arrow)
                .count(),
            1
        );

        core.world
            .set_block(lever.0, lever.1, lever.2, BlockType::Lever, 0)
            .unwrap();
        core.world.redstone.on_block_changed(
            &core.world.chunks,
            lever,
            crate::redstone::Direction::East,
        );
        let _ = core.tick();
        core.world
            .set_block(lever.0, lever.1, lever.2, BlockType::LeverOn, 0)
            .unwrap();
        core.world.redstone.on_block_changed(
            &core.world.chunks,
            lever,
            crate::redstone::Direction::East,
        );
        let _ = core.tick();
        assert_eq!(
            core.world
                .entities
                .entities
                .iter()
                .filter(|entity| entity.entity_type == EntityType::Arrow)
                .count(),
            2
        );
    }

    #[test]
    fn same_vectors_have_same_revisions_for_each_topology() {
        let mut snapshots = Vec::new();
        for topology in [
            AuthorityTopology::Singleplayer,
            AuthorityTopology::ListenServer,
            AuthorityTopology::Dedicated,
        ] {
            let mut core = core(topology);
            snapshots.push(core.common_vector_snapshot());
        }
        assert_eq!(snapshots[0], snapshots[1]);
        assert_eq!(snapshots[1], snapshots[2]);
    }

    #[test]
    fn item_use_mutates_session_inventory_and_revision() {
        let mut core = core(AuthorityTopology::Singleplayer);
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
        let mut core = core(AuthorityTopology::Singleplayer);
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
                reason: RejectReason::Unsupported
            }
        ));
        assert_eq!(
            core.session(7)
                .unwrap()
                .gameplay
                .count_item(Item::DiamondSword as u32),
            1
        );
    }

    #[test]
    fn client_cannot_submit_self_damage() {
        let mut core = core(AuthorityTopology::Singleplayer);
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
        let mut core = core(AuthorityTopology::Singleplayer);
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
    fn dimension_transfer_updates_session_and_world_contract() {
        let mut boundary = AuthorityBoundary::new(
            AuthorityConfig::default(),
            AuthorityTopology::Singleplayer,
            7,
            "alex",
            [8.0, 80.0, 8.0],
            true,
            true,
        );
        assert!(boundary.set_dimension(crate::dimension::Dimension::Nether as u8));
        assert_eq!(boundary.core.session(7).unwrap().dimension, 1);
        assert_eq!(
            boundary.core.world.dimension,
            crate::dimension::Dimension::Nether
        );
        assert_eq!(
            boundary.core.world.chunks.dimension,
            crate::dimension::Dimension::Nether
        );
    }

    #[test]
    fn dimension_worlds_are_parked_without_chunk_aliasing() {
        let mut boundary = AuthorityBoundary::new(
            AuthorityConfig::default(),
            AuthorityTopology::Singleplayer,
            7,
            "alex",
            [8.0, 80.0, 8.0],
            true,
            true,
        );
        let marker = BlockType::Glass;
        boundary
            .core
            .world
            .set_block(1_234, 100, -2_345, marker, 0)
            .unwrap();
        assert_eq!(boundary.core.world.get_block(1_234, 100, -2_345), marker);

        assert!(boundary.set_dimension(crate::dimension::Dimension::Nether as u8));
        assert_ne!(boundary.core.world.get_block(1_234, 100, -2_345), marker);
        assert!(boundary.core.world.valid_coordinate(1_234, 127, -2_345));
        assert!(!boundary.core.world.valid_coordinate(1_234, 128, -2_345));
        boundary.set_position([154.25, 67.0, -293.5]);
        assert_eq!(
            boundary.core.session(7).unwrap().position,
            [154.25, 67.0, -293.5]
        );

        assert!(boundary.set_dimension(crate::dimension::Dimension::Overworld as u8));
        assert_eq!(boundary.core.world.get_block(1_234, 100, -2_345), marker);
        assert_eq!(boundary.core.session(7).unwrap().dimension, 0);
    }

    #[test]
    fn sessions_in_multiple_dimensions_tick_and_dispatch_independently() {
        let mut core = AuthorityCore::new(AuthorityConfig::default(), AuthorityTopology::Dedicated);
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

        let overworld = core.submit_request(GameplayRequest {
            request_id: 101,
            client_sequence: 1,
            session_id: 7,
            dimension: Dimension::Overworld as u8,
            client_revision: core.revision_for_dimension(Dimension::Overworld),
            operation: GameplayOperation::BlockUse {
                x: 8,
                y: 80,
                z: 8,
                block: BlockType::Glass.to_wire(),
            },
        });
        let nether = core.submit_request(GameplayRequest {
            request_id: 102,
            client_sequence: 1,
            session_id: 8,
            dimension: Dimension::Nether as u8,
            client_revision: core.revision_for_dimension(Dimension::Nether),
            operation: GameplayOperation::BlockUse {
                x: 8,
                y: 80,
                z: 8,
                block: BlockType::Obsidian.to_wire(),
            },
        });
        assert!(matches!(
            overworld.outcome,
            GameplayOutcome::Accepted { .. }
        ));
        assert!(matches!(nether.outcome, GameplayOutcome::Accepted { .. }));
        assert_eq!(overworld.server_sequence, nether.server_sequence);

        core.activate_dimension(Dimension::Overworld);
        let snapshot = core.tick();
        assert_eq!(snapshot.tick, 1);
        assert_eq!(core.world.dimension, Dimension::Overworld);
        assert!(snapshot
            .mutations
            .iter()
            .any(|mutation| mutation.dimension == Dimension::Overworld as u8
                && mutation.block == BlockType::Glass.to_wire()));
        assert!(snapshot
            .mutations
            .iter()
            .any(|mutation| mutation.dimension == Dimension::Nether as u8
                && mutation.block == BlockType::Obsidian.to_wire()));
        assert!(snapshot
            .session_updates
            .iter()
            .any(|update| update.player_id == 7 && update.dimension == Dimension::Overworld as u8));
        assert!(snapshot
            .session_updates
            .iter()
            .any(|update| update.player_id == 8 && update.dimension == Dimension::Nether as u8));

        core.activate_dimension(Dimension::Overworld);
        assert_eq!(core.world.time, 1);
        assert_eq!(core.world.get_block(8, 80, 8), BlockType::Glass);
        let overworld_revision = core.revision_for_dimension(Dimension::Overworld);
        core.activate_dimension(Dimension::Nether);
        assert_eq!(core.world.time, 1);
        assert_eq!(core.world.get_block(8, 80, 8), BlockType::Obsidian);
        let nether_revision = core.revision_for_dimension(Dimension::Nether);
        assert_eq!(overworld_revision, nether_revision);

        // An active Nether compatibility view must not make a valid Overworld
        // request fail its dimension gate; routing selects the session world.
        let routed_again = core.submit_request(GameplayRequest {
            request_id: 103,
            client_sequence: 2,
            session_id: 7,
            dimension: Dimension::Overworld as u8,
            client_revision: overworld_revision,
            operation: GameplayOperation::BlockUse {
                x: 9,
                y: 80,
                z: 8,
                block: BlockType::Glass.to_wire(),
            },
        });
        assert!(matches!(
            routed_again.outcome,
            GameplayOutcome::Accepted { .. }
        ));
    }

    #[test]
    fn authority_boundary_does_not_reingest_presentation_inventory() {
        let mut core = core(AuthorityTopology::Singleplayer);
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
        let mut core = core(AuthorityTopology::Dedicated);
        let mut attacker = core.session(7).unwrap().gameplay;
        attacker.attack_cooldown_ticks = ATTACK_COOLDOWN_TICKS;
        assert!(core.set_session_gameplay(7, attacker));
        let target = core
            .world
            .entities
            .spawn(EntityType::Zombie, glam::Vec3::new(8.0, 80.0, 9.0));
        let before = core.world.entities.get_by_id(target).unwrap().health;
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
        assert!(core.world.entities.get_by_id(target).unwrap().health < before);
    }

    #[test]
    fn trade_conserves_items_and_mount_projects_session_state() {
        let mut core = core(AuthorityTopology::Singleplayer);
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
        assert!(core.world.ensure_villager(
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
        assert!(core
            .world
            .ensure_vehicle(vehicle, EntityType::Boat, [9.0, 80.0, 8.0],));
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
            .world
            .entities
            .get_by_id(vehicle)
            .unwrap()
            .passengers
            .contains(&7));
    }
}
