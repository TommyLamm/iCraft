//! GPU-independent authoritative simulation.

pub mod contract;

use crate::dimension::Dimension;
use crate::game_rules::{WorldRules, WorldType};
use crate::network::protocol::{
    GameplayOutcome, GameplayRequest, GameplayResponse, PlayerId, RejectReason,
};
use crate::server_world::ServerWorld;
use contract::{AuthoritySnapshot, AuthorityTopology, SessionContract, WorldMutation};
use std::collections::BTreeMap;

pub use contract::{
    common_gameplay_vectors, RevisionClock, AUTHORITY_CONTRACT_VERSION, FIXED_TICK_HZ,
    RESPONSE_CACHE_CAPACITY,
};

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
    sessions: BTreeMap<PlayerId, SessionContract>,
    last_snapshot: AuthoritySnapshot,
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

    pub fn set_game_mode(&mut self, game_mode: crate::inventory::GameMode) {
        if let Some(session) = self.core.session_mut(self.session_id) {
            session.game_mode = game_mode;
        }
    }

    pub fn set_rules(&mut self, rules: WorldRules) {
        self.core.world.rules = rules.normalized();
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
            world: ServerWorld::new(
                config.seed,
                config.dimension,
                config.world_type,
                config.generate_structures,
                config.rules,
                config.render_distance,
            ),
            sessions: BTreeMap::new(),
            last_snapshot: AuthoritySnapshot::empty(),
            pending_mutations: Vec::new(),
        }
    }

    pub fn register_session(&mut self, session: SessionContract) -> Result<(), RejectReason> {
        if self.sessions.contains_key(&session.id) {
            return Err(RejectReason::Duplicate);
        }
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

    pub fn current_revision(&self) -> u64 {
        self.world.revisions.current()
    }

    /// Execute one fixed tick.  Sessions are sorted by their BTreeMap key so
    /// entity AI and automation do not depend on transport arrival order.
    pub fn tick(&mut self) -> AuthoritySnapshot {
        let players: Vec<(PlayerId, [f32; 3])> = self
            .sessions
            .values()
            .map(|session| (session.id, session.position))
            .collect();
        let mut snapshot = self.world.tick(&players);
        if !self.pending_mutations.is_empty() {
            let mut pending = std::mem::take(&mut self.pending_mutations);
            pending.append(&mut snapshot.mutations);
            pending.sort_by_key(|mutation| mutation.revision);
            snapshot.mutations = pending;
            snapshot.revision = self.world.revisions.current();
            snapshot.checksum = self.world.checksum(&snapshot.mutations);
        }
        self.last_snapshot = snapshot.clone();
        snapshot
    }

    pub fn submit_request(&mut self, request: GameplayRequest) -> GameplayResponse {
        let request_id = request.request_id;
        let id = request.session_id;
        let Some(session) = self.sessions.get(&id) else {
            return self.rejected(request_id, RejectReason::Unauthorized);
        };
        if let Some(cached) = session.cached_response(request_id) {
            return cached;
        }
        let Some(session_dimension) = Dimension::from_wire(request.dimension) else {
            return self.reject_for_session(id, request_id, RejectReason::InvalidDimension, None);
        };
        if request.validate_bounds().is_err() {
            return self.reject_for_session(id, request_id, RejectReason::Malformed, None);
        }
        if session.dimension != request.dimension {
            return self.reject_for_session(id, request_id, RejectReason::InvalidDimension, None);
        }
        if let Err(reason) = session.validate_sequence(&request) {
            return self.reject_for_session(id, request_id, reason, None);
        }
        if request.client_revision > self.current_revision() {
            return self.reject_for_session(id, request_id, RejectReason::InvalidRevision, None);
        }
        if session.game_mode == crate::inventory::GameMode::Spectator
            && matches!(
                &request.operation,
                crate::network::protocol::GameplayOperation::BlockUse { .. }
                    | crate::network::protocol::GameplayOperation::Container { .. }
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
                }
                session.cache_response(response.clone());
            }
        }
        response
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::protocol::{GameplayOperation, GameplayOutcome};
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
}
