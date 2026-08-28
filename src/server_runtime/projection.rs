//! Authority snapshot routing, interest sets, and presentation/transport fanout.
//!
//! Owns chunk/entity/session deltas routed by player interest, initial chunk
//! stream queueing, presentation queue backpressure, and container event fanout.

use super::*;
use crate::authority::contract::{AuthoritySnapshot, SessionGameplayState};
use crate::authority::interest::{
    InterestKind, RoutedInterestUpdate, MAX_INTEREST_UPDATES_PER_TICK,
};
use crate::dimension::Dimension;
use crate::network::protocol::{
    ContainerAction, EntityStateWire, GameplayResponse, ItemWire, PlayerEffectWire,
    SessionGameplayWire,
};
use crate::network::server::HostToServer;
use crate::save::ChunkSaveData;
use glam::Vec3;

impl ServerRuntime {
    pub(super) fn route_container_result(
        &mut self,
        id: u64,
        revision: u64,
        x: i32,
        y: i32,
        z: i32,
        slot: u16,
        action: ContainerAction,
    ) {
        let position = (x, y, z);
        let dimension = self
            .authority
            .session(id)
            .and_then(|session| Dimension::from_wire(session.dimension))
            .unwrap_or_else(|| self.authority.active_dimension());
        if matches!(action, ContainerAction::Open) {
            let previous_positions = self
                .players
                .get(&id)
                .map(|session| {
                    session
                        .interest
                        .open_containers
                        .iter()
                        .copied()
                        .filter(|previous| *previous != position)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            for previous in previous_positions {
                self.close_runtime_container(id, dimension, previous);
            }
        }
        if let Some(session) = self.players.get_mut(&id) {
            match action {
                ContainerAction::Close => {
                    session.interest.open_containers.remove(&position);
                }
                ContainerAction::Open | ContainerAction::Click => {
                    session.interest.open_containers.insert(position);
                }
            }
        }
        self.queue_interest_update(dimension, revision, InterestKind::BlockEntity(position));
        let container_targets =
            self.queue_interest_update(dimension, revision, InterestKind::Container(position));
        match action {
            ContainerAction::Open => {
                let slots = self
                    .authority
                    .world_mut(dimension)
                    .and_then(|world| world.container_slots_wire(position))
                    .unwrap_or_default();
                self.send_targeted(
                    id,
                    slots,
                    |slots| RuntimePresentationEvent::ContainerOpenResult {
                        target: id,
                        dimension: dimension as u8,
                        success: true,
                        position,
                        slots,
                        revision,
                    },
                    |slots| HostToServer::SendContainerOpenResult {
                        to: id,
                        dimension: dimension as u8,
                        success: true,
                        x,
                        y,
                        z,
                        slots,
                        revision,
                    },
                );
            }
            ContainerAction::Click => {
                let slot_value = self
                    .authority
                    .world_mut(dimension)
                    .and_then(|world| world.container_slot_wire(position, slot))
                    .flatten();
                let session_state = self.authority.session(id).map(|session| session.gameplay);
                let dragged = session_state
                    .and_then(|state| state.cursor)
                    .map(|slot| slot.item);
                if let Some(state) = session_state {
                    if let Some(session) = self.players.get_mut(&id) {
                        session.last_projected_session_revision = Some((dimension, state.revision));
                    }
                    self.send_session_update(id, revision, dimension, state);
                }
                self.send_targeted(
                    id,
                    (),
                    |_| RuntimePresentationEvent::ContainerClickResult {
                        target: id,
                        dimension: dimension as u8,
                        success: true,
                        slot_index: slot,
                        slot: slot_value,
                        dragged,
                    },
                    |_| HostToServer::SendContainerClickResult {
                        to: id,
                        dimension: dimension as u8,
                        success: true,
                        slot_index: slot,
                        slot: slot_value,
                        dragged,
                    },
                );
                for target in container_targets {
                    if target != id {
                        self.send_container_slot_update(
                            target, dimension, revision, position, slot, slot_value,
                        );
                    }
                }
            }
            ContainerAction::Close => {}
        }
    }

    fn send_targeted<T>(
        &mut self,
        to: u64,
        payload: T,
        local: impl FnOnce(T) -> RuntimePresentationEvent,
        remote: impl FnOnce(T) -> HostToServer,
    ) {
        if self.local_session_id == Some(to) {
            self.push_presentation_event(local(payload));
        } else {
            self.enqueue_host(remote(payload));
        }
    }

    pub(super) fn send_response(&mut self, to: u64, response: GameplayResponse) {
        self.send_targeted(
            to,
            response,
            |response| RuntimePresentationEvent::GameplayResponse {
                target: to,
                response,
            },
            |response| HostToServer::SendGameplayResponse { to, response },
        );
    }

    pub(super) fn send_respawn_result(
        &mut self,
        to: u64,
        position: [f32; 3],
        dimension: Dimension,
    ) {
        self.send_targeted(
            to,
            (),
            |_| RuntimePresentationEvent::PlayerRespawnResult {
                target: to,
                position,
                dimension: dimension as u8,
            },
            |_| HostToServer::SendPlayerRespawnResult {
                to,
                position,
                dimension: dimension as u8,
            },
        );
    }

    pub(super) fn send_block_entity_delta(
        &mut self,
        to: u64,
        dimension: Dimension,
        revision: u64,
        position: (i32, i32, i32),
        entity: Option<crate::block_entity::BlockEntity>,
    ) {
        let (x, y, z) = position;
        self.send_targeted(
            to,
            entity,
            |entity| RuntimePresentationEvent::BlockEntityDelta {
                target: to,
                dimension: dimension as u8,
                revision,
                x,
                y,
                z,
                entity,
            },
            |entity| HostToServer::BlockEntityDelta {
                to: Some(to),
                dimension: dimension as u8,
                revision,
                x,
                y,
                z,
                entity,
            },
        );
    }

    pub(super) fn send_entity_spawn(
        &mut self,
        to: u64,
        dimension: Dimension,
        sequence: u64,
        state: EntityStateWire,
    ) {
        self.send_targeted(
            to,
            state,
            |state| RuntimePresentationEvent::EntitySpawn {
                target: to,
                dimension: dimension as u8,
                sequence,
                state,
            },
            |state| HostToServer::EntitySpawn {
                to: Some(to),
                dimension: dimension as u8,
                sequence,
                state,
            },
        );
    }

    pub(super) fn send_entity_state(
        &mut self,
        to: u64,
        dimension: Dimension,
        sequence: u64,
        state: EntityStateWire,
    ) {
        self.send_targeted(
            to,
            state,
            |state| RuntimePresentationEvent::EntityState {
                target: to,
                dimension: dimension as u8,
                sequence,
                state,
            },
            |state| HostToServer::EntityState {
                to: Some(to),
                dimension: dimension as u8,
                sequence,
                state,
            },
        );
    }

    pub(super) fn send_entity_despawn(
        &mut self,
        to: u64,
        dimension: Dimension,
        sequence: u64,
        entity_id: u64,
    ) {
        self.send_targeted(
            to,
            (),
            |_| RuntimePresentationEvent::EntityDespawn {
                target: to,
                dimension: dimension as u8,
                sequence,
                entity_id,
            },
            |_| HostToServer::EntityDespawn {
                to: Some(to),
                dimension: dimension as u8,
                sequence,
                entity_id,
            },
        );
    }

    pub(super) fn send_session_update(
        &mut self,
        to: u64,
        sequence: u64,
        dimension: Dimension,
        state: SessionGameplayState,
    ) {
        let state = SessionGameplayWire::from(state);
        self.send_targeted(
            to,
            state,
            |state| RuntimePresentationEvent::PlayerSessionUpdate {
                target: to,
                sequence,
                player_id: to,
                dimension: dimension as u8,
                state,
            },
            |state| HostToServer::SendPlayerSessionUpdate {
                to,
                sequence,
                player_id: to,
                dimension: dimension as u8,
                state,
            },
        );
    }

    pub(super) fn send_player_effects(
        &mut self,
        to: u64,
        sequence: u64,
        effects: Vec<PlayerEffectWire>,
    ) {
        self.send_targeted(
            to,
            effects,
            |effects| RuntimePresentationEvent::PlayerEffect {
                target: to,
                sequence,
                player_id: to,
                effects,
            },
            |effects| HostToServer::PlayerEffect {
                to: Some(to),
                sequence,
                player_id: to,
                effects,
            },
        );
    }

    pub(super) fn send_container_slot_update(
        &mut self,
        to: u64,
        dimension: Dimension,
        revision: u64,
        position: (i32, i32, i32),
        slot_index: u16,
        slot: Option<ItemWire>,
    ) {
        let (x, y, z) = position;
        self.send_targeted(
            to,
            (),
            |_| RuntimePresentationEvent::ContainerSlotUpdate {
                target: to,
                dimension: dimension as u8,
                revision,
                position,
                slot_index,
                slot,
            },
            |_| HostToServer::ContainerSlotUpdate {
                to: Some(to),
                dimension: dimension as u8,
                revision,
                x,
                y,
                z,
                slot_index,
                slot,
            },
        );
    }

    pub(super) fn send_container_close(
        &mut self,
        to: u64,
        dimension: Dimension,
        position: (i32, i32, i32),
    ) {
        let (x, y, z) = position;
        self.send_targeted(
            to,
            (),
            |_| RuntimePresentationEvent::ContainerClose {
                target: to,
                dimension: dimension as u8,
                position,
            },
            |_| HostToServer::SendContainerClose {
                to,
                dimension: dimension as u8,
                x,
                y,
                z,
            },
        );
    }

    /// Remove one exact lifecycle registration and emit at most one close for
    /// the current session.  Double-chest partner intents may arrive after the
    /// primary half; they still clean the world viewer but cannot close a new
    /// session or duplicate the packet.
    pub(super) fn close_runtime_container(
        &mut self,
        id: u64,
        dimension: Dimension,
        position: (i32, i32, i32),
    ) {
        let _ = self.authority.with_world(dimension, |world| {
            world.close_container_viewer_forced(id, position)
        });
        let was_open = self
            .players
            .get_mut(&id)
            .is_some_and(|session| session.interest.open_containers.remove(&position));
        if was_open {
            self.send_container_close(id, dimension, position);
        }
    }

    pub(super) fn force_close_player_containers(&mut self, id: u64) {
        let Some((dimension, positions)) = self.players.get(&id).map(|session| {
            (
                session.dimension,
                session
                    .interest
                    .open_containers
                    .iter()
                    .copied()
                    .collect::<Vec<_>>(),
            )
        }) else {
            return;
        };
        for position in positions {
            self.close_runtime_container(id, dimension, position);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn send_chunk_projection(
        &mut self,
        to: u64,
        dimension: Dimension,
        cx: i32,
        cz: i32,
        revision: u64,
        min_section_y: i8,
        section_count: u16,
        blocks: Vec<u8>,
        block_states: Vec<u8>,
        fluid_levels: Vec<u8>,
        block_entities: Vec<u8>,
    ) {
        self.send_targeted(
            to,
            (blocks, block_states, fluid_levels, block_entities),
            |(blocks, block_states, fluid_levels, block_entities)| {
                RuntimePresentationEvent::ChunkData {
                    target: to,
                    dimension: dimension as u8,
                    cx,
                    cz,
                    revision,
                    min_section_y,
                    section_count,
                    blocks,
                    block_states,
                    fluid_levels,
                    block_entities,
                }
            },
            |(blocks, block_states, fluid_levels, block_entities)| HostToServer::SendChunk {
                dimension: dimension as u8,
                cx,
                cz,
                revision,
                min_section_y,
                section_count,
                blocks,
                block_states,
                fluid_levels,
                block_entities,
                to,
            },
        );
    }

    /// Queue an embedded-client projection without allowing replaceable state
    /// floods to evict request acknowledgements or authoritative lifecycle
    /// changes. Returns `false` only when a replaceable update is discarded or
    /// the bounded critical overflow is exhausted; both paths emit QueueFull.
    pub(super) fn push_presentation_event(&mut self, event: RuntimePresentationEvent) -> bool {
        let replaceable_key = event.replaceable_key();
        if let Some(key) = replaceable_key {
            if let Some(index) = self
                .presentation_events
                .iter()
                .position(|queued| queued.replaceable_key() == Some(key))
            {
                self.presentation_events[index] = event;
                return true;
            }
        }

        if self.presentation_events.len() < MAX_PRESENTATION_EVENTS_PER_TICK {
            self.presentation_events.push_back(event);
            return true;
        }

        if let Some(index) = self
            .presentation_events
            .iter()
            .position(|queued| queued.replaceable_key().is_some())
        {
            self.presentation_events.remove(index);
            self.presentation_events.push_back(event);
            self.network_metrics.record_queue_full();
            return true;
        }

        if replaceable_key.is_some() {
            self.network_metrics.record_queue_full();
            return false;
        }

        if self.presentation_events.len() < MAX_PRESENTATION_QUEUE_LEN {
            self.presentation_events.push_back(event);
            self.network_metrics.record_queue_full();
            return true;
        }

        self.network_metrics.record_queue_full();
        false
    }

    pub fn drain_routed_updates(&mut self) -> Vec<RoutedInterestUpdate> {
        std::mem::take(&mut self.routed_updates)
    }

    pub(super) fn queue_interest_update(
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

    pub(super) fn queue_block_change(
        &mut self,
        dimension: Dimension,
        revision: u64,
        x: i32,
        y: i32,
        z: i32,
        block: u32,
        state: u8,
        raw_fluid: u8,
    ) {
        let targets =
            self.queue_interest_update(dimension, revision, InterestKind::Block((x, y, z)));
        for target in targets {
            self.send_targeted(
                target,
                (),
                |_| RuntimePresentationEvent::BlockChange {
                    target,
                    dimension: dimension as u8,
                    revision,
                    x,
                    y,
                    z,
                    block,
                    state,
                    raw_fluid,
                },
                |_| HostToServer::BlockChange {
                    to: Some(target),
                    dimension: dimension as u8,
                    revision,
                    x,
                    y,
                    z,
                    block,
                    state,
                    raw_fluid,
                },
            );
        }
    }

    /// Project every fixed-tick authority mutation through the authenticated
    /// interest sets.  This is the only runtime fanout path for automation,
    /// entity AI and block-entity/container deltas; presentation roots never
    /// replay these mutations locally.
    pub(super) fn route_authority_snapshot(&mut self, snapshot: &AuthoritySnapshot) {
        let mut session_ids: Vec<_> = self.players.keys().copied().collect();
        session_ids.sort_unstable();
        for id in session_ids {
            if let Some((dimension, position)) = self
                .players
                .get(&id)
                .map(|session| (session.dimension, session.data.position))
            {
                self.update_interest_for_at(id, dimension, position, snapshot.tick);
            }
        }
        self.drain_initial_chunk_projections();

        for mutation in &snapshot.mutations {
            let Some(dimension) = Dimension::from_wire(mutation.dimension) else {
                continue;
            };
            if !self.routed_mutations.insert((dimension, mutation.revision)) {
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
                mutation.raw_fluid,
            );

            let entity = self
                .authority
                .world_ref(dimension)
                .and_then(|world| world.get_block_entity(x, y, z).cloned());
            let block_entity_targets = self.queue_interest_update(
                dimension,
                mutation.revision,
                InterestKind::BlockEntity(mutation.position),
            );
            for target in block_entity_targets {
                self.send_block_entity_delta(
                    target,
                    dimension,
                    mutation.revision,
                    mutation.position,
                    entity.clone(),
                );
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
                if let Some(slots) = self
                    .authority
                    .world_mut(dimension)
                    .and_then(|world| world.container_slots_wire(mutation.position))
                {
                    for (slot_index, slot) in slots.into_iter().enumerate() {
                        let slot_index = slot_index.min(u16::MAX as usize) as u16;
                        for target in &container_targets {
                            self.send_container_slot_update(
                                *target,
                                dimension,
                                mutation.revision,
                                mutation.position,
                                slot_index,
                                slot,
                            );
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
        for dimension in self.authority.dimensions() {
            let mut entities: Vec<_> = self
                .authority
                .world_ref(dimension)
                .map(|world| {
                    world
                        .entities
                        .entities
                        .iter()
                        .map(|entity| (entity.id, entity_state_wire(entity)))
                        .collect()
                })
                .unwrap_or_default();
            entities.sort_by_key(|(id, _)| *id);
            for (entity_id, state) in entities {
                let targets = self.queue_interest_update(
                    dimension,
                    // Snapshot revision is an aggregate max across worlds and
                    // cannot be used as a client gate for this dimension.
                    self.authority.revision_for_dimension(dimension),
                    InterestKind::EntityState(entity_id),
                );
                for target in targets {
                    self.send_entity_state(target, dimension, snapshot.tick, state);
                }
            }
        }
        for update in &snapshot.session_updates {
            let Some(dimension) = Dimension::from_wire(update.dimension) else {
                continue;
            };
            let should_send = self.players.get(&update.player_id).is_some_and(|session| {
                session.dimension == dimension
                    && session.last_projected_session_revision.map_or(
                        true,
                        |(projected_dimension, revision)| {
                            projected_dimension != dimension || update.state.revision > revision
                        },
                    )
            });
            if !should_send {
                continue;
            }
            self.sync_gameplay_projection(update.player_id);
            if let Some(session) = self.players.get_mut(&update.player_id) {
                session.last_projected_session_revision = Some((dimension, update.state.revision));
            }
            self.send_session_update(update.player_id, snapshot.tick, dimension, update.state);
        }
    }

    pub(super) fn update_interest(&mut self, session: &mut PlayerSessionState) {
        let _ = session
            .interest
            .update_position(session.dimension, session.data.position);
        let center = Vec3::from_array(session.data.position);
        let radius = f32::from(session.interest.view_distance) * 16.0;
        let (entities, simulation_entities) = self
            .authority
            .world_ref(session.dimension)
            .map(|world| {
                let entities = world
                    .entities
                    .query_radius(center, radius)
                    .map(|entity| entity.id)
                    .collect();
                let simulation_entities = world
                    .entities
                    .query_radius(
                        center,
                        f32::from(session.interest.simulation_distance) * 16.0,
                    )
                    .map(|entity| entity.id)
                    .collect();
                (entities, simulation_entities)
            })
            .unwrap_or_else(|| (Vec::new(), Vec::new()));
        let _ = session.interest.update_entities(entities);
        session
            .interest
            .update_simulation_entities(simulation_entities);
    }

    pub(super) fn update_interest_for(
        &mut self,
        id: u64,
        dimension: Dimension,
        position: [f32; 3],
    ) {
        let sequence = self.authority.last_snapshot().tick.saturating_add(1).max(1);
        self.update_interest_for_at(id, dimension, position, sequence);
    }

    pub(super) fn update_interest_for_at(
        &mut self,
        id: u64,
        dimension: Dimension,
        position: [f32; 3],
        sequence: u64,
    ) {
        let (entity_states, simulation_entities) = self
            .authority
            .world_ref(dimension)
            .map(|world| {
                let entities = world
                    .entities
                    .query_radius(
                        Vec3::from_array(position),
                        f32::from(self.properties.view_distance) * 16.0,
                    )
                    .map(|entity| (entity.id, entity_state_wire(entity)))
                    .collect::<Vec<_>>();
                let simulation_entities = world
                    .entities
                    .query_radius(
                        Vec3::from_array(position),
                        f32::from(self.properties.simulation_distance) * 16.0,
                    )
                    .map(|entity| entity.id)
                    .collect();
                (entities, simulation_entities)
            })
            .unwrap_or_else(|| (Vec::new(), Vec::new()));
        let entities: Vec<_> = entity_states
            .iter()
            .map(|(entity_id, _)| *entity_id)
            .collect();
        let (entity_delta, old_dimension, departed_containers) = {
            let Some(session) = self.players.get_mut(&id) else {
                return;
            };
            let old_dimension = session.interest.dimension;
            let old_entities = session.interest.entities.clone();
            let old_open_containers = session.interest.open_containers.clone();
            session.dimension = dimension;
            let mut chunk_delta = session.interest.update_position(dimension, position);
            let center_chunk = (
                (position[0] / 16.0).floor() as i32,
                (position[2] / 16.0).floor() as i32,
            );
            chunk_delta.entered.sort_by_key(|(cx, cz)| {
                let dx = i64::from(*cx - center_chunk.0);
                let dz = i64::from(*cz - center_chunk.1);
                (dx * dx + dz * dz, *cx, *cz)
            });
            let departed_containers: Vec<_> = old_open_containers
                .difference(&session.interest.open_containers)
                .copied()
                .collect();
            let mut entity_delta = session.interest.update_entities(entities);
            if old_dimension != dimension {
                entity_delta.departed = old_entities.into_iter().collect();
                entity_delta.departed.sort_unstable();
                entity_delta.entered = session.interest.entities.iter().copied().collect();
                entity_delta.entered.sort_unstable();
                session.pending_initial_chunks.clear();
            }
            session
                .interest
                .update_simulation_entities(simulation_entities);
            session
                .pending_initial_chunks
                .retain(|(queued_dimension, cx, cz)| {
                    *queued_dimension == dimension && session.interest.chunks.contains(&(*cx, *cz))
                });
            session.queue_initial_chunks(dimension, chunk_delta.entered.iter().copied());
            (entity_delta, old_dimension, departed_containers)
        };
        for position in departed_containers {
            let _ = self.authority.with_world(old_dimension, |world| {
                world.close_container_viewer_forced(id, position)
            });
            self.send_container_close(id, old_dimension, position);
        }
        for entity_id in entity_delta.departed {
            self.record_interest_update(
                id,
                old_dimension,
                self.authority.revision_for_dimension(old_dimension),
                InterestKind::Entity(entity_id),
            );
            self.send_entity_despawn(id, old_dimension, sequence, entity_id);
        }
        for entity_id in entity_delta.entered {
            self.record_interest_update(
                id,
                dimension,
                self.authority.revision_for_dimension(dimension),
                InterestKind::Entity(entity_id),
            );
            if let Some(state) = entity_states
                .iter()
                .find_map(|(id, state)| (*id == entity_id).then_some(*state))
            {
                self.send_entity_spawn(id, dimension, sequence, state);
            }
        }
    }

    pub(super) fn record_interest_update(
        &mut self,
        target: u64,
        dimension: Dimension,
        revision: u64,
        kind: InterestKind,
    ) {
        if self.routed_updates.len() < MAX_INTEREST_UPDATES_PER_TICK {
            self.routed_updates.push(RoutedInterestUpdate {
                target,
                dimension,
                revision,
                kind,
            });
        }
    }

    pub(super) fn drain_initial_chunk_projections(&mut self) {
        let mut ids: Vec<_> = self.players.keys().copied().collect();
        ids.sort_unstable();
        let mut projected = 0usize;
        let mut inspected = 0usize;
        while projected < MAX_INITIAL_CHUNK_PROJECTIONS_PER_TICK
            && inspected < MAX_INITIAL_CHUNK_PROJECTIONS_PER_TICK * 4
        {
            let mut made_progress = false;
            for id in &ids {
                if projected >= MAX_INITIAL_CHUNK_PROJECTIONS_PER_TICK
                    || inspected >= MAX_INITIAL_CHUNK_PROJECTIONS_PER_TICK * 4
                {
                    break;
                }
                let next = self
                    .players
                    .get_mut(id)
                    .and_then(|session| session.pending_initial_chunks.pop_front());
                let Some((dimension, cx, cz)) = next else {
                    continue;
                };
                made_progress = true;
                inspected += 1;
                // Interest is the authoritative chunk-loading boundary. Load
                // only the bounded projection batch, nearest-first, so the
                // server never stalls one tick generating the full view
                // distance while block actions stop being rejected outside
                // the spawn chunk.
                let _ = self.authority.with_world(dimension, |world| {
                    world.ensure_chunk(cx, cz);
                });
                let payload = self.authority.world_ref(dimension).and_then(|world| {
                    if world.failed_restore_chunks().contains(&(cx, cz)) {
                        return None;
                    }
                    world.chunks.chunks.get(&(cx, cz)).and_then(|chunk| {
                        let mut data = ChunkSaveData::from_chunk(chunk).ok()?;
                        let revision = world.chunk_revision(cx, cz);
                        data.mutation_revision = revision;
                        Some((
                            revision,
                            chunk.min_section_y,
                            chunk.sections.len().min(u16::MAX as usize) as u16,
                            data.blocks,
                            data.block_states,
                            data.fluid_levels,
                            data.block_entities,
                        ))
                    })
                });
                let Some((
                    revision,
                    min_section_y,
                    section_count,
                    blocks,
                    block_states,
                    fluid_levels,
                    block_entities,
                )) = payload
                else {
                    if let Some(session) = self.players.get_mut(id) {
                        if session.interest.dimension == dimension
                            && session.interest.chunks.contains(&(cx, cz))
                            && session.pending_initial_chunks.len()
                                < MAX_PENDING_INITIAL_CHUNKS_PER_SESSION
                        {
                            session
                                .pending_initial_chunks
                                .push_back((dimension, cx, cz));
                        }
                    }
                    continue;
                };
                self.record_interest_update(
                    *id,
                    dimension,
                    revision,
                    InterestKind::Chunk((cx, cz)),
                );
                self.send_chunk_projection(
                    *id,
                    dimension,
                    cx,
                    cz,
                    revision,
                    min_section_y,
                    section_count,
                    blocks,
                    block_states,
                    fluid_levels,
                    block_entities,
                );
                projected += 1;
            }
            if !made_progress {
                break;
            }
        }
    }
}

pub(super) fn entity_state_wire(entity: &crate::entity::Entity) -> EntityStateWire {
    let animation_state = u8::from(entity.on_ground)
        | (u8::from(entity.target_player) << 1)
        | (u8::from(entity.is_ignited) << 2)
        | (u8::from(entity.fire_aspect_timer > 0.0) << 3);
    let item = entity
        .dropped_stack
        .as_ref()
        .map(ItemWire::from_stack)
        .or_else(|| {
            entity.dropped_item.map(|item| {
                let stack = crate::inventory::ItemStack::new(item, entity.dropped_count.max(1));
                ItemWire::from_stack(&stack)
            })
        })
        .or_else(|| {
            entity.potion.map(|potion| {
                let mut stack =
                    crate::inventory::ItemStack::new(crate::inventory::Item::SplashPotion, 1);
                stack.potion = Some(potion);
                ItemWire::from_stack(&stack)
            })
        });
    EntityStateWire {
        entity_id: entity.id,
        entity_type: entity.entity_type.to_wire(),
        position: entity.position.to_array(),
        velocity: entity.velocity.to_array(),
        yaw: entity.yaw,
        pitch: entity.pitch,
        health: entity.health,
        animation_state,
        item,
    }
}
