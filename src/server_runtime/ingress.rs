//! Inbound network and local event handling for `ServerRuntime`.
//!
//! Owns ingress dispatch from `ServerToHost`, incoming gameplay requests,
//! player session joins/leaves, pose verification, chat broadcast, and
//! legacy command translations.

use super::projection::entity_state_wire;
use super::*;
use crate::authority::DimensionTransferIntent;
use crate::dimension::Dimension;
use crate::game_rules::persisted_player_game_mode;
use crate::network::protocol::{GameplayRequest, GameplayResponse, Packet, PROTOCOL_VERSION};
use crate::network::server::{HostToServer, ProjectionEvent, ServerToHost};
use crate::save::normalize_player_identity;
use std::io;
use std::time::Instant;
use crate::world::chunk_xz;

impl ServerRuntime {
    pub(super) fn handle_event(&mut self, event: ServerToHost) -> io::Result<()> {
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
            ServerToHost::ClientAction { id, action } => {
                self.enqueue_host(HostToServer::project_broadcast(Packet::PlayerAction {
                    id,
                    action,
                }));
                Ok(())
            }
            ServerToHost::ChatFromClient { id, message } => {
                if let Some(sender) = self
                    .authority
                    .session(id)
                    .map(|session| session.username.clone())
                {
                    self.enqueue_host(HostToServer::project_broadcast(Packet::ChatMessage {
                        sender,
                        message: message.chars().take(256).collect(),
                    }));
                }
                Ok(())
            }
            ServerToHost::ClientRespawnRequest { id } => {
                let Some(is_dead) = self
                    .authority
                    .session(id)
                    .map(|session| session.gameplay.is_dead)
                else {
                    return Ok(());
                };
                if !is_dead {
                    return Ok(());
                }
                let previous_dimension =
                    self.players.get(&id).map(|session| session.interest.dimension);
                if let Some(dimension) = previous_dimension {
                    self.authority
                        .with_world(dimension, |world| world.close_container_viewers_forced(id));
                }
                let respawn_position = [
                    self.level.spawn_x as f32,
                    self.level.spawn_y as f32,
                    self.level.spawn_z as f32,
                ];
                let dimension = self.level.spawn_dimension;
                // Both authority seams must succeed before runtime pose,
                // dimension, or inventory are written.
                if !self.authority.respawn_session(id) {
                    return Ok(());
                }
                if !self.authority.set_session_dimension(id, dimension) {
                    return Ok(());
                }
                let (yaw, pitch) = self
                    .players
                    .get(&id)
                    .map(|session| (session.data.yaw, session.data.pitch))
                    .unwrap_or((0.0, 0.0));
                if let Some(session) = self.players.get_mut(&id) {
                    session.interest.open_containers.clear();
                    session.teleport_allowance = Some(respawn_position);
                }
                self.sync_dimension(id, dimension);
                let _ = self.write_pose(id, respawn_position, yaw, pitch, false);
                // Hardcore→spectator (and any other respawn mode change) lives on
                // the contract; sync_gameplay_projection pulls game_mode too.
                self.sync_gameplay_projection(id);
                self.send_respawn_result(id, respawn_position, dimension);
                self.update_interest_for(id, dimension, respawn_position);
                Ok(())
            }
            ServerToHost::Disconnected { .. } => Ok(()),
        }
    }

    pub(super) fn handle_join(&mut self, id: u64, username: String) -> io::Result<()> {
        self.handle_join_with_storage(id, username, LocalSessionStorage::Named)
    }

    pub(super) fn handle_join_with_storage(
        &mut self,
        id: u64,
        username: String,
        storage: LocalSessionStorage,
    ) -> io::Result<()> {
        let username = normalize_player_identity(&username)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        if self.authority.sessions().any(|session| {
            session
                .username
                .eq_ignore_ascii_case(username.as_str())
        }) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("duplicate player identity: {username}"),
            ));
        }
        let (data, current_dimension, effects) = match storage {
            LocalSessionStorage::Named => self
                .save_manager
                .load_dedicated_player(&username)?
                .map(|file| {
                    let mut data = file.data;
                    data.game_mode = persisted_player_game_mode(
                        data.game_mode,
                        self.default_game_mode,
                        self.level.cheats_enabled,
                    );
                    (data, file.current_dimension, file.effects)
                })
                .unwrap_or_else(|| self.default_player_payload()),
            LocalSessionStorage::WorldPlayer => {
                let player_path = self.world_dir.join("player.dat");
                if player_path.exists() {
                    let (_saved_level, mut data) = self.save_manager.load_player_and_level()?;
                    data.game_mode = persisted_player_game_mode(
                        data.game_mode,
                        self.default_game_mode,
                        self.level.cheats_enabled,
                    );
                    // The legacy world-player format has no effect vector;
                    // effects start empty until that schema gains one.
                    (data, self.save_manager.load_current_dimension(), Vec::new())
                } else {
                    self.default_player_payload()
                }
            }
        };
        let mut session = PlayerSessionState::new(
            storage,
            data,
            current_dimension,
            self.properties.view_distance,
            self.properties.simulation_distance,
        );
        session.effects = effects;
        let dimension = session.interest.dimension as u8;
        let mut authority_session = SessionContract::new(
            id,
            username.clone(),
            dimension,
            session.data.position,
            self.properties.operators.contains(&username),
            self.level.cheats_enabled,
        );
        authority_session.game_mode = session.data.game_mode;
        authority_session.gameplay = gameplay_from_player_data(&session.data);
        self.authority
            .register_session_with_limit(authority_session, self.properties.max_players)
            .map_err(|reason| {
                io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!("session rejected: {reason:?}"),
                )
            })?;
        // Registering first ensures a saved non-active dimension exists in the
        // authority map before interest queries read its entities/chunks.
        self.update_interest(&mut session);
        let join_dimension = session.interest.dimension;
        let join_chunks: Vec<_> = session.interest.chunks.iter().copied().collect();
        session.chunk_index_dimension = Some(join_dimension);
        self.interest_index_seed_session(id, join_dimension, join_chunks.iter().copied());
        self.players.insert(id, session);
        // Seed was PlayerData → contract; re-enter through session_sync so both
        // records share the same writer for every later mode change.
        self.sync_game_mode(id);
        let (mut chunks, mut entities) = self
            .players
            .get(&id)
            .map(|session| {
                (
                    session.interest.chunks.iter().copied().collect::<Vec<_>>(),
                    session
                        .interest
                        .entities
                        .iter()
                        .copied()
                        .collect::<Vec<_>>(),
                )
            })
            .unwrap_or_default();
        chunks.sort_unstable();
        entities.sort_unstable();
        if let Some(session) = self.players.get_mut(&id) {
            session.queue_initial_chunks(current_dimension, chunks.iter().copied());
        }
        for chunk in chunks {
            self.record_interest_update(
                id,
                current_dimension,
                self.authority.revision_for_dimension(current_dimension),
                InterestKind::Chunk(chunk),
            );
        }
        for entity in entities {
            self.record_interest_update(
                id,
                current_dimension,
                self.authority.revision_for_dimension(current_dimension),
                InterestKind::Entity(entity),
            );
            if let Some(state) = self
                .authority
                .world_ref(current_dimension)
                .and_then(|world| {
                    world
                        .entities
                        .entities
                        .iter()
                        .find(|item| item.id == entity)
                })
                .map(entity_state_wire)
            {
                self.send_entity_spawn(id, current_dimension, self.level.time.max(1), state);
            }
        }
        let (rules, join_sequence, revision) =
            self.authority.with_world(current_dimension, |world| {
                (
                    world.rules,
                    world.revisions.allocate(),
                    world.revisions.current(),
                )
            });
        if self.local_session_id == Some(id) {
            self.push_presentation_event(ProjectionEvent::session(
                id,
                Packet::WorldRulesSync {
                    rules,
                },
            ));
            self.push_presentation_event(ProjectionEvent::session(
                id,
                Packet::TimeSync {
                    ticks: self.level.time,
                    weather: 0,
                    weather_remaining_ticks: 0.0,
                },
            ));
        } else {
            self.enqueue_host(HostToServer::project_session(
                id,
                Packet::WorldRulesSync {
                    rules,
                },
            ));
            self.enqueue_host(HostToServer::project_session(
                id,
                Packet::TimeSync {
                    ticks: self.level.time,
                    weather: 0,
                    weather_remaining_ticks: 0.0,
                },
            ));
        }
        if let Some((state, effects)) = self
            .authority
            .session(id)
            .map(|authority_session| authority_session.gameplay)
            .zip(self.players.get(&id).map(|session| session.effects.clone()))
        {
            self.send_session_update(id, join_sequence, current_dimension, state);
            if let Some(session) = self.players.get_mut(&id) {
                session.last_projected_session_revision = Some((current_dimension, state.revision));
            }
            self.send_player_effects(id, join_sequence, effects);
        }
        self.send_response(
            id,
            GameplayResponse {
                request_id: 0,
                server_sequence: join_sequence,
                outcome: GameplayOutcome::Accepted { revision },
            },
        );
        self.metrics.players_online = self.players.len();
        eprintln!("[ServerRuntime] player joined id={id} dimension={dimension}");
        Ok(())
    }

    pub(super) fn default_player_payload(&self) -> (PlayerData, Dimension, Vec<PlayerEffectWire>) {
        let data = default_player_data(self.default_game_mode);
        let dimension = data.spawn_dimension.unwrap_or(self.level.spawn_dimension);
        (data, dimension, Vec::new())
    }

    pub(super) fn handle_leave(&mut self, id: u64) -> io::Result<()> {
        self.sync_gameplay_projection(id);
        let username = self
            .authority
            .session(id)
            .map(|session| session.username.clone())
            .unwrap_or_else(|| format!("player-{id}"));
        if let Some(session) = self.players.remove(&id) {
            let dimension = session.interest.dimension;
            self.interest_index_clear_session(
                id,
                session.chunk_index_dimension,
                session.interest.chunks.iter().copied(),
            );
            for &position in &session.interest.open_containers {
                let _ = self.authority.with_world(dimension, |world| {
                    world.close_container_viewer_forced(id, position)
                });
                self.send_container_close(id, dimension, position);
            }
            self.authority
                .with_world(dimension, |world| world.close_container_viewers_forced(id));
            if let Err(error) = self.save_player(id, &session) {
                eprintln!("[ServerRuntime] leave save failed for {username}: {error}");
            }
        }
        self.authority.remove_session(id);
        self.metrics.players_online = self.players.len();
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn handle_position(
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
        let position = [x, y, z];
        if !session.accept_pose(
            sequence,
            sender_time_millis,
            position,
            yaw,
            pitch,
            Instant::now(),
        ) {
            return Ok(());
        }
        let dimension = session.interest.dimension;
        let _ = session;
        if !self.write_pose(id, position, yaw, pitch, true) {
            return Ok(());
        }
        let block_position = (x.floor() as i32, y.floor() as i32, z.floor() as i32);
        let mut targets: Vec<_> = self
            .players
            .iter()
            .filter(|(target_id, target)| {
                **target_id != id
                    && target
                        .interest
                        .wants(dimension, InterestKind::Block(block_position))
            })
            .map(|(target_id, _)| *target_id)
            .collect();
        targets.sort_unstable();
        for target in targets {
            if self.local_session_id == Some(target) {
                self.push_presentation_event(ProjectionEvent::session(
                    target,
                    Packet::PlayerPosition {
                        id,
                        sequence,
                        sender_time_millis,
                        x,
                        y,
                        z,
                        yaw,
                        pitch,
                    },
                ));
            } else {
                self.enqueue_host(HostToServer::project_session(
                    target,
                    Packet::PlayerPosition {
                        id,
                        sequence,
                        sender_time_millis,
                        x,
                        y,
                        z,
                        yaw,
                        pitch,
                    },
                ));
            }
        }
        Ok(())
    }

    pub(super) fn handle_gameplay_request(
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
        // A presentation may see a locally generated chunk a few frames
        // before its bounded initial projection reaches ServerWorld.  If the
        // authenticated player already has interest in the target, load the
        // target and its adjacent support chunk now; otherwise valid
        // place/break input was rejected as InvalidState until the background
        // projection queue happened to catch up. Out-of-interest BlockAction
        // must not call ensure_chunk.
        if let GameplayOperation::BlockAction { x, z, face, .. } = &operation {
            let target = (*x, *z);
            let support = (
                x.saturating_sub(i32::from(face[0])),
                z.saturating_sub(i32::from(face[2])),
            );
            if let Some(dimension) = self
                .players
                .get(&id)
                .filter(|session| {
                    session
                        .interest
                        .wants(session.interest.dimension, InterestKind::Block((*x, 0, *z)))
                })
                .map(|session| session.interest.dimension)
            {
                let _ = self.authority.with_world(dimension, |world| {
                    let (tcx, tcz) = chunk_xz(target.0, target.1);
                    let (scx, scz) = chunk_xz(support.0, support.1);
                    // Interest-gated BlockAction must see the column this tick;
                    // async ensure_chunk only queues and would reject as InvalidState.
                    world.materialize_chunk(tcx, tcz);
                    world.materialize_chunk(scx, scz);
                });
            }
        }
        let response = self.authority.submit_request(request.clone());
        if duplicate {
            self.metrics.duplicate_requests = self.metrics.duplicate_requests.saturating_add(1);
            return Ok(response);
        }
        match &response.outcome {
            GameplayOutcome::Accepted { revision } => {
                self.metrics.requests_accepted = self.metrics.requests_accepted.saturating_add(1);
                if let Some(dimension) = self
                    .authority
                    .session(id)
                    .and_then(|authority_session| Dimension::from_wire(authority_session.dimension))
                {
                    self.sync_dimension(id, dimension);
                }
                match operation {
                    GameplayOperation::Command { .. } => {
                        let runtime_position = self
                            .players
                            .get(&id)
                            .map(|session| session.last_pose_position);
                        let authority_position =
                            self.authority.session(id).map(|session| session.position);
                        if let (Some(runtime_position), Some(authority_position)) =
                            (runtime_position, authority_position)
                        {
                            if runtime_position != authority_position {
                                let _ = self.teleport_session(id, authority_position);
                            }
                        }
                    }
                    GameplayOperation::Container {
                        x,
                        y,
                        z,
                        action,
                        slot: _,
                    } => {
                        self.route_container_result(id, *revision, x, y, z, action);
                        // Container open/close changes the authoritative chest
                        // block state.  It is published by the next snapshot
                        // (including a double-chest partner mutation), so do
                        // not mark this revision as already routed here.
                    }
                    GameplayOperation::ContainerClick {
                        x,
                        y,
                        z,
                        slot,
                        dragged: _,
                        is_left: _,
                    } => {
                        self.route_container_click_result(id, *revision, x, y, z, slot);
                        let dimension = self
                            .authority
                            .session(id)
                            .and_then(|session| Dimension::from_wire(session.dimension))
                            .unwrap_or(self.level.spawn_dimension);
                        self.routed_mutations.insert((dimension, *revision));
                    }
                    _ => {}
                }
            }
            GameplayOutcome::Rejected { reason } => {
                self.metrics.requests_rejected = self.metrics.requests_rejected.saturating_add(1);
                if matches!(
                    operation,
                    GameplayOperation::Container { .. } | GameplayOperation::ContainerClick { .. }
                ) && matches!(
                    reason,
                    RejectReason::TooFar | RejectReason::InvalidDimension
                ) {
                    self.force_close_player_containers(id);
                }
            }
        }
        Ok(response)
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
        let Some((_old_dimension, position)) = self
            .players
            .get(&id)
            .map(|session| (session.interest.dimension, session.last_pose_position))
        else {
            return false;
        };
        self.force_close_player_containers(id);
        if !self.authority.set_session_dimension(id, dimension) {
            return false;
        }
        if self.players.get(&id).is_none() {
            return false;
        }
        self.sync_dimension(id, dimension);
        if let Some(session) = self.players.get_mut(&id) {
            session.interest.open_containers.clear();
        }
        self.update_interest_for(id, dimension, position);
        true
    }

    pub fn transfer_session_dimension(
        &mut self,
        id: u64,
        dimension: Dimension,
        position: [f32; 3],
    ) -> bool {
        if !self
            .authority
            .execute_portal_dimension_transfer(id, dimension, position)
        {
            return false;
        }
        let Some(transfer) = self
            .authority
            .take_pending_dimension_transfers()
            .into_iter()
            .find(|transfer| transfer.player_id == id)
        else {
            return false;
        };
        self.apply_authority_dimension_transfer(transfer);
        true
    }

    pub(super) fn apply_authority_dimension_transfer(&mut self, transfer: DimensionTransferIntent) {
        let id = transfer.player_id;
        self.force_close_player_containers(id);
        if let Some(session) = self.players.get_mut(&id) {
            session.teleport_allowance = Some(transfer.position);
            session.interest.open_containers.clear();
            session.pending_initial_chunks.clear();
            session.last_projected_session_revision = None;
        }
        self.sync_dimension(id, transfer.to);
        let _ = self.sync_pose_from_authority(id, true);
        if self.local_session_id == Some(id) {
            self.push_presentation_event(ProjectionEvent::session(
                id,
                Packet::DimensionTransfer {
                    player_id: id,
                    dimension: transfer.to as u8,
                    position: transfer.position,
                },
            ));
        } else {
            self.enqueue_host(HostToServer::project_session(
                id,
                Packet::DimensionTransfer {
                    player_id: id,
                    dimension: transfer.to as u8,
                    position: transfer.position,
                },
            ));
        }
    }

    /// Apply a server-authorized teleport to a connected session. The next
    /// client pose may converge to this position without being rejected by the
    /// normal speed gate; all interest routing and the authority position are
    /// updated before the method returns.
    pub fn teleport_session(&mut self, id: u64, position: [f32; 3]) -> bool {
        if !position
            .iter()
            .all(|component| component.is_finite() && component.abs() <= WORLD_BOUND)
        {
            return false;
        }
        let Some((yaw, pitch)) = self
            .players
            .get(&id)
            .map(|session| (session.data.yaw, session.data.pitch))
        else {
            return false;
        };
        if self.authority.session(id).is_none() {
            return false;
        }
        if let Some(session) = self.players.get_mut(&id) {
            session.teleport_allowance = Some(position);
        }
        self.write_pose(id, position, yaw, pitch, true)
    }

    pub(super) fn enqueue_host(&mut self, event: HostToServer) -> bool {
        let Some(host_tx) = self.host_tx.as_ref() else {
            return false;
        };
        // Reserve the gauge before publishing so the network thread cannot
        // receive and decrement the command before its enqueue is visible.
        self.network_metrics.enqueue();
        match host_tx.try_send(event) {
            Ok(()) => true,
            Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                self.network_metrics.dequeue();
                self.network_metrics.record_queue_full();
                false
            }
            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                self.network_metrics.dequeue();
                false
            }
        }
    }

    pub(super) fn enqueue_stop(&mut self) {
        let Some(host_tx) = self.host_tx.as_ref() else {
            return;
        };
        self.network_metrics.enqueue();
        match host_tx.try_send(HostToServer::Stop) {
            Ok(()) => {}
            Err(tokio::sync::mpsc::error::TrySendError::Full(stop)) => {
                self.network_metrics.record_queue_full();
                // A full command queue must not turn shutdown into a detached
                // network thread. `blocking_send` unblocks as soon as the live server
                // consumes one command; the pre-counted Stop remains part of
                // the aggregate backlog while the producer is waiting.
                if host_tx.blocking_send(stop).is_err() {
                    self.network_metrics.dequeue();
                }
            }
            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                self.network_metrics.dequeue()
            }
        }
    }

    pub(super) fn sync_network_metrics(&mut self) {
        let snapshot = self.network_metrics.snapshot();
        self.metrics.inbound_packets = snapshot.inbound_packets;
        self.metrics.inbound_bytes = snapshot.inbound_bytes;
        self.metrics.outbound_packets = snapshot.outbound_packets;
        self.metrics.outbound_bytes = snapshot.outbound_bytes;
        self.metrics.queue_depth = snapshot.queue_depth;
        self.metrics.queue_full = snapshot.queue_full;

        let new_rejections = snapshot
            .rejected_requests
            .saturating_sub(self.observed_transport_rejections);
        self.metrics.requests_rejected = self
            .metrics
            .requests_rejected
            .saturating_add(new_rejections);
        self.observed_transport_rejections = snapshot.rejected_requests;

        let new_duplicates = snapshot
            .duplicate_requests
            .saturating_sub(self.observed_transport_duplicates);
        self.metrics.duplicate_requests = self
            .metrics
            .duplicate_requests
            .saturating_add(new_duplicates);
        self.observed_transport_duplicates = snapshot.duplicate_requests;
    }

    pub(super) fn session_revision(&self, id: u64) -> Option<u64> {
        let dimension = self
            .authority
            .session(id)
            .and_then(|session| Dimension::from_wire(session.dimension))?;
        Some(self.authority.revision_for_dimension(dimension))
    }
}
