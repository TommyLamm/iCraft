//! GPU-independent authoritative simulation.

pub mod contract;
pub mod interest;

use crate::dimension::Dimension;
use crate::game_rules::{WorldRules, WorldType};
use crate::network::protocol::{
    GameplayOutcome, GameplayRequest, GameplayResponse, PlayerId, RejectReason,
};
use crate::server_world::ServerWorld;
use contract::{
    AuthoritySnapshot, AuthorityTopology, SessionContract, SessionGameplayState,
    SessionGameplayUpdate, WorldMutation,
};
use std::cmp::Ordering;
use std::collections::BTreeMap;

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
        }
    }

    fn new_world(config: AuthorityConfig, dimension: Dimension) -> ServerWorld {
        ServerWorld::new(
            config.seed,
            dimension,
            config.world_type,
            config.generate_structures,
            config.rules,
            config.render_distance,
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
        if !self.sessions.contains_key(&id) {
            return false;
        }
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
        self.sessions.remove(&id)
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
            let players: Vec<(PlayerId, [f32; 3])> = self
                .sessions
                .values()
                .filter(|session| session.dimension == dimension as u8)
                .map(|session| (session.id, session.position))
                .collect();
            let world_snapshot = self.world.tick(&players);
            mutations_by_dimension.insert(dimension, world_snapshot.mutations);
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
                crate::network::protocol::GameplayOperation::BlockUse { .. }
                    | crate::network::protocol::GameplayOperation::Container { .. }
                    | crate::network::protocol::GameplayOperation::ContainerClick { .. }
                    | crate::network::protocol::GameplayOperation::ItemUse { .. }
                    | crate::network::protocol::GameplayOperation::Combat { .. }
                    | crate::network::protocol::GameplayOperation::Trade { .. }
                    | crate::network::protocol::GameplayOperation::Mount { .. }
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

        let result = self
            .dispatch_session_command(&request, id)
            .or_else(|| self.dispatch_session_gameplay(&request, id))
            .unwrap_or_else(|| {
                self.world
                    .dispatch(&request, id, operator)
                    .map_err(|error| error.reason())
            });
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
        use crate::network::protocol::GameplayOperation;

        match &request.operation {
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
                let Some(session) = self.sessions.get_mut(&session_id) else {
                    return Some(Err(RejectReason::Unauthorized));
                };
                let original_gameplay = session.gameplay;
                let hunger = session.gameplay.hunger_milli as f32 / 1000.0;
                if hunger >= 20.0 && !food.always_edible && session.game_mode != GameMode::Creative
                {
                    return Some(Err(RejectReason::InvalidState));
                }
                session.gameplay.hunger_milli =
                    ((hunger + food.hunger).min(20.0) * 1000.0).round() as u32;
                session.gameplay.saturation_milli =
                    ((session.gameplay.saturation_milli as f32 / 1000.0 + food.saturation)
                        .min(session.gameplay.hunger_milli as f32 / 1000.0)
                        * 1000.0)
                        .round() as u32;
                if session.game_mode != GameMode::Creative
                    && !session.gameplay.remove_item(*item, u32::from(*count))
                {
                    session.gameplay = original_gameplay;
                    return Some(Err(RejectReason::InvalidState));
                }
                Some(Ok(None))
            }
            GameplayOperation::Combat { target, action } => {
                if *target == 0 && (*action & 0x80) != 0 {
                    let amount_milli = u32::from(*action & 0x7f).saturating_mul(100);
                    let Some(session) = self.sessions.get_mut(&session_id) else {
                        return Some(Err(RejectReason::Unauthorized));
                    };
                    if session.gameplay.is_dead || amount_milli == 0 {
                        return Some(Err(RejectReason::InvalidState));
                    }
                    session.gameplay.health_milli =
                        session.gameplay.health_milli.saturating_sub(amount_milli);
                    if session.gameplay.health_milli == 0 {
                        session.gameplay.is_dead = true;
                    }
                    return Some(Ok(None));
                }
                if *action != 0 {
                    return Some(Err(RejectReason::Unsupported));
                }
                let Some(position) = self.sessions.get(&session_id).map(|s| s.position) else {
                    return Some(Err(RejectReason::Unauthorized));
                };
                Some(self.world.apply_combat(*target, position).map(|_| None))
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
                let result = self
                    .world
                    .apply_trade(&mut gameplay, *villager_id, *offer_index, position)
                    .map(|_| {
                        if let Some(session) = self.sessions.get_mut(&session_id) {
                            session.gameplay = gameplay;
                        }
                        None
                    });
                Some(result)
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
            _ => None,
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
        if !self.sessions.contains_key(&id) {
            return false;
        }
        let hardcore = self.world.rules.hardcore;
        let revision = self.world.revisions.allocate();
        let Some(session) = self.sessions.get_mut(&id) else {
            return false;
        };
        session.gameplay.is_dead = false;
        session.gameplay.health_milli = session.gameplay.max_health_milli;
        session.gameplay.hunger_milli = 20_000;
        session.gameplay.saturation_milli = 5_000;
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
    use crate::network::protocol::{GameplayOperation, GameplayOutcome};
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
    fn self_damage_combat_updates_session_health_and_death() {
        let mut core = core(AuthorityTopology::Singleplayer);
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
        assert!(matches!(response.outcome, GameplayOutcome::Accepted { .. }));
        let state = core.session(7).unwrap().gameplay;
        assert_eq!(state.health_milli, 7_300);
        let response = core.submit_request(GameplayRequest {
            request_id: 36,
            client_sequence: 2,
            session_id: 7,
            dimension: 0,
            client_revision: core.current_revision(),
            operation: GameplayOperation::Combat {
                target: 0,
                action: 0x80 | 127,
            },
        });
        assert!(matches!(response.outcome, GameplayOutcome::Accepted { .. }));
        let state = core.session(7).unwrap().gameplay;
        assert!(state.is_dead);
        assert_eq!(state.health_milli, 0);
    }

    #[test]
    fn respawn_command_restores_authority_health_after_death() {
        let mut core = core(AuthorityTopology::Singleplayer);
        for (request_id, sequence) in [(38, 1), (39, 2)] {
            let response = core.submit_request(GameplayRequest {
                request_id,
                client_sequence: sequence,
                session_id: 7,
                dimension: 0,
                client_revision: core.current_revision(),
                operation: GameplayOperation::Combat {
                    target: 0,
                    action: 0x80 | 127,
                },
            });
            assert!(matches!(response.outcome, GameplayOutcome::Accepted { .. }));
        }
        assert!(core.session(7).unwrap().gameplay.is_dead);
        let response = core.submit_request(GameplayRequest {
            request_id: 40,
            client_sequence: 3,
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
        let target = core
            .world
            .entities
            .spawn(EntityType::Zombie, glam::Vec3::new(9.0, 80.0, 8.0));
        let before = core.world.entities.get_by_id(target).unwrap().health;
        let response = core.submit_request(GameplayRequest {
            request_id: 31,
            client_sequence: 1,
            session_id: 7,
            dimension: 0,
            client_revision: 0,
            operation: GameplayOperation::Combat { target, action: 0 },
        });
        assert!(matches!(response.outcome, GameplayOutcome::Accepted { .. }));
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
