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
use crate::network::protocol::{
    wrap_legacy, ContainerAction, GameplayOperation, GameplayOutcome, GameplayRequest,
    GameplayResponse, LegacyGameplay, PlayerEffectWire, RejectReason,
};
use crate::network::server::{HostToServer, ServerToHost};
use crate::save::normalize_player_identity;
use std::io;
use std::sync::mpsc::TrySendError;
use std::time::Instant;

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
                let previous_dimension = self.players.get(&id).map(|session| session.dimension);
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
                let game_mode = self.authority.session(id).map(|session| session.game_mode);
                if let Some(session) = self.players.get_mut(&id) {
                    session.interest.open_containers.clear();
                    session.teleport_allowance = Some(respawn_position);
                    if let Some(game_mode) = game_mode {
                        session.data.game_mode = game_mode;
                    }
                }
                self.sync_dimension(id, dimension);
                let _ = self.write_pose(id, respawn_position, yaw, pitch, false);
                self.sync_gameplay_projection(id);
                self.send_respawn_result(id, respawn_position, dimension);
                self.update_interest_for(id, dimension, respawn_position);
                Ok(())
            }
            ServerToHost::ClientBlockAction {
                id,
                action,
                x,
                y,
                z,
                block,
                held_item,
            } => {
                let Some(request) = self.legacy_request(
                    id,
                    self.session_revision(id).unwrap_or(0),
                    LegacyGameplay::BlockAction {
                        action,
                        x,
                        y,
                        z,
                        block,
                        held_item,
                    },
                ) else {
                    return Ok(());
                };
                let response = self.handle_gameplay_request(request)?;
                self.send_response(id, response);
                Ok(())
            }
            ServerToHost::ClientSleepRequest {
                id,
                bed_x,
                bed_y,
                bed_z,
            } => {
                let Some(request) = self.legacy_request(
                    id,
                    self.session_revision(id).unwrap_or(0),
                    LegacyGameplay::Sleep {
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
                    self.session_revision(id).unwrap_or(0),
                    LegacyGameplay::ContainerOpen { x, y, z },
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
                        self.session_request_id(id),
                        RejectReason::InvalidState,
                    );
                    return Ok(());
                };
                let request = GameplayRequest {
                    request_id: self.session_request_id(id),
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
                    self.session_revision(id).unwrap_or(0),
                    LegacyGameplay::ContainerClose { x, y, z },
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
        if self
            .players
            .values()
            .any(|session| session.username == username)
        {
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
            id,
            username,
            storage,
            data,
            current_dimension,
            self.properties.view_distance,
            self.properties.simulation_distance,
        );
        session.effects = effects;
        let dimension = session.dimension as u8;
        let mut authority_session = SessionContract::new(
            id,
            session.username.clone(),
            dimension,
            session.data.position,
            self.properties.operators.contains(&session.username),
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
        self.players.insert(id, session);
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
            self.push_presentation_event(RuntimePresentationEvent::WorldRules {
                target: id,
                rules,
            });
            self.push_presentation_event(RuntimePresentationEvent::TimeSync {
                target: id,
                ticks: self.level.time,
                weather: 0,
                weather_remaining_ticks: 0.0,
            });
        } else {
            self.enqueue_host(HostToServer::SendWorldRules { rules, to: id });
            self.enqueue_host(HostToServer::SendTimeSync {
                ticks: self.level.time,
                weather: 0,
                weather_remaining_ticks: 0.0,
                to: id,
            });
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
        if let Some(session) = self.players.remove(&id) {
            let dimension = session.dimension;
            for &position in &session.interest.open_containers {
                let _ = self.authority.with_world(dimension, |world| {
                    world.close_container_viewer_forced(id, position)
                });
                self.send_container_close(id, dimension, position);
            }
            self.authority
                .with_world(dimension, |world| world.close_container_viewers_forced(id));
            if let Err(error) = self.save_player(&session) {
                eprintln!(
                    "[ServerRuntime] leave save failed for {}: {error}",
                    session.username
                );
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
        let dimension = session.dimension;
        let _ = session;
        if !self.write_pose(id, position, yaw, pitch, true) {
            return Ok(());
        }
        self.sync_dimension(id, dimension);
        let block_position = (x.floor() as i32, y.floor() as i32, z.floor() as i32);
        let mut targets: Vec<_> = self
            .players
            .values()
            .filter(|target| {
                target.id != id
                    && target
                        .interest
                        .wants(dimension, InterestKind::Block(block_position))
            })
            .map(|target| target.id)
            .collect();
        targets.sort_unstable();
        for target in targets {
            if self.local_session_id == Some(target) {
                self.push_presentation_event(RuntimePresentationEvent::PlayerPosition {
                    target,
                    id,
                    sequence,
                    sender_time_millis,
                    position,
                    yaw,
                    pitch,
                });
            } else {
                self.enqueue_host(HostToServer::SendPlayerPosition {
                    to: target,
                    id,
                    sequence,
                    sender_time_millis,
                    x,
                    y,
                    z,
                    yaw,
                    pitch,
                });
            }
        }
        Ok(())
    }

    pub(super) fn handle_block_change(
        &mut self,
        id: u64,
        x: i32,
        y: i32,
        z: i32,
        block: u32,
        _state: u8,
    ) -> io::Result<()> {
        // Leftover BlockChange has no held/face. Submit BlockUse so the
        // authority can reject Unsupported; never set_block or invent Air.
        let Some(request) = self.legacy_request(
            id,
            self.session_revision(id).unwrap_or(0),
            LegacyGameplay::BlockChange { x, y, z, block },
        ) else {
            return Ok(());
        };
        let response = self.handle_gameplay_request(request)?;
        self.send_response(id, response);
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
                        .wants(session.dimension, InterestKind::Block((*x, 0, *z)))
                })
                .map(|session| session.dimension)
            {
                let _ = self.authority.with_world(dimension, |world| {
                    world.ensure_chunk(target.0.div_euclid(16), target.1.div_euclid(16));
                    world.ensure_chunk(support.0.div_euclid(16), support.1.div_euclid(16));
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
                if let Some(session) = self.players.get_mut(&id) {
                    session.last_client_sequence = request.client_sequence;
                }
                if let Some(dimension) = self
                    .authority
                    .session(id)
                    .and_then(|authority_session| Dimension::from_wire(authority_session.dimension))
                {
                    self.sync_dimension(id, dimension);
                }
                match operation {
                    GameplayOperation::Command { .. } => {
                        let runtime_position =
                            self.players.get(&id).map(|session| session.data.position);
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
                        slot,
                    } => {
                        let action = ContainerAction::from_wire(action)
                            .expect("authority accepted only a typed container action");
                        self.route_container_result(id, *revision, x, y, z, slot, action);
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
                        self.route_container_result(
                            id,
                            *revision,
                            x,
                            y,
                            z,
                            slot,
                            ContainerAction::Click,
                        );
                        let dimension = self
                            .authority
                            .session(id)
                            .and_then(|session| Dimension::from_wire(session.dimension))
                            .unwrap_or_else(|| self.authority.active_dimension());
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
            .map(|session| (session.dimension, session.data.position))
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
            self.push_presentation_event(RuntimePresentationEvent::DimensionTransfer {
                target: id,
                dimension: transfer.to as u8,
                position: transfer.position,
            });
        } else {
            self.enqueue_host(HostToServer::SendDimensionTransfer {
                to: id,
                dimension: transfer.to as u8,
                position: transfer.position,
            });
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
            Err(TrySendError::Full(_)) => {
                self.network_metrics.dequeue();
                self.network_metrics.record_queue_full();
                false
            }
            Err(TrySendError::Disconnected(_)) => {
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
            Err(TrySendError::Full(stop)) => {
                self.network_metrics.record_queue_full();
                // A full command queue must not turn shutdown into a detached
                // network thread. `send` unblocks as soon as the live server
                // consumes one command; the pre-counted Stop remains part of
                // the aggregate backlog while the producer is waiting.
                if host_tx.send(stop).is_err() {
                    self.network_metrics.dequeue();
                }
            }
            Err(TrySendError::Disconnected(_)) => self.network_metrics.dequeue(),
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

    pub(super) fn legacy_request(
        &self,
        id: u64,
        client_revision: u64,
        leftover: LegacyGameplay,
    ) -> Option<GameplayRequest> {
        let session = self.authority.session(id)?;
        let dimension = Dimension::from_wire(session.dimension)?;
        let mut request = wrap_legacy(id, dimension as u8, client_revision, leftover)?;
        request.request_id = self.authority.revision_for_dimension(dimension) as u128 + 1;
        request.client_sequence = session.last_client_sequence.saturating_add(1).max(1);
        Some(request)
    }

    pub(super) fn send_legacy_rejection(
        &mut self,
        to: u64,
        request_id: u128,
        reason: RejectReason,
    ) {
        let dimension = self
            .authority
            .session(to)
            .and_then(|session| Dimension::from_wire(session.dimension))
            .unwrap_or_else(|| self.authority.active_dimension());
        let server_sequence = self
            .authority
            .with_world(dimension, |world| world.revisions.allocate());
        let response = GameplayResponse {
            request_id,
            server_sequence,
            outcome: GameplayOutcome::Rejected { reason },
        };
        self.send_response(to, response);
    }

    pub(super) fn session_revision(&self, id: u64) -> Option<u64> {
        let dimension = self
            .authority
            .session(id)
            .and_then(|session| Dimension::from_wire(session.dimension))?;
        Some(self.authority.revision_for_dimension(dimension))
    }

    pub(super) fn session_request_id(&self, id: u64) -> u128 {
        self.session_revision(id).unwrap_or(0) as u128 + 1
    }
}
