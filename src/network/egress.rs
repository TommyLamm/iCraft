use std::collections::HashSet;
use std::sync::Arc;

use super::channels::{HostEventSender, HostToServer, ServerToHost};
use super::protocol::{GameplayResponse, Packet, PlayerId, PROTOCOL_VERSION};
use super::session::{best_effort_send, reliable_send, NetworkMetrics, Sessions};

pub(crate) async fn normalize_host_response(
    sessions: &Sessions,
    id: PlayerId,
    mut response: GameplayResponse,
) -> GameplayResponse {
    let mut sessions_guard = sessions.lock().await;
    let Some(session) = sessions_guard.get_mut(&id) else {
        if response.server_sequence == 0 {
            response.server_sequence = 1;
        }
        return response;
    };
    let state = &mut session.gameplay;
    if let Some(cached) = state.cached_response(response.request_id) {
        return cached;
    }
    if response.server_sequence == 0 || response.server_sequence <= state.last_server_sequence {
        response.server_sequence = state.allocate_server_sequence();
    } else {
        state.last_server_sequence = response.server_sequence;
    }
    if let crate::network::protocol::GameplayOutcome::Accepted { revision } = response.outcome {
        state.last_client_revision = state.last_client_revision.max(revision);
    }
    state.cache_response(response.clone());
    response
}

pub(crate) async fn handle_host_command<S: HostEventSender>(
    sessions: &Sessions,
    server_to_host: &S,
    metrics: &NetworkMetrics,
    command: HostToServer,
) {
    if let HostToServer::SendPlayerSessionUpdate { to, player_id, .. } = &command {
        if to != player_id {
            metrics.record_rejected_request();
            return;
        }
    }
    if let HostToServer::SendChunk {
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
        to,
    } = command
    {
        let packet = Packet::ChunkData {
            protocol_version: PROTOCOL_VERSION,
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
        };
        let mailbox = sessions
            .lock()
            .await
            .get(&to)
            .map(|session| Arc::clone(&session.catchup_mailbox));
        if let Some(mailbox) = mailbox {
            match mailbox.replace(packet).await {
                Ok(()) => {
                    let _ = server_to_host.send(ServerToHost::CatchupAccepted {
                        id: to,
                        dimension,
                        cx,
                        cz,
                        revision,
                    });
                }
                Err(mailbox_full_count) => {
                    let _ = server_to_host.send(ServerToHost::CatchupBackpressured {
                        id: to,
                        dimension,
                        cx,
                        cz,
                        revision,
                        mailbox_full_count,
                    });
                }
            }
        }
        return;
    }

    if let HostToServer::DisconnectCatchupClient { to, reason } = command {
        eprintln!("[NetworkServer] Applying slow catch-up policy to Player ID {to}: {reason}");
        evict_slow_clients(sessions, server_to_host, vec![to]).await;
        return;
    }

    if let HostToServer::DisconnectClient { to, reason } = &command {
        let failed = send_to(
            sessions,
            *to,
            Packet::Disconnect {
                protocol_version: PROTOCOL_VERSION,
                reason: reason.clone(),
            },
        )
        .await;
        evict_slow_clients(sessions, server_to_host, failed).await;
        return;
    }

    if let HostToServer::BroadcastPlayerPosition {
        id,
        sequence,
        sender_time_millis,
        x,
        y,
        z,
        yaw,
        pitch,
    } = &command
    {
        broadcast_pose_inner(
            sessions,
            Packet::PlayerPosition {
                protocol_version: PROTOCOL_VERSION,
                id: *id,
                sequence: *sequence,
                sender_time_millis: *sender_time_millis,
                x: *x,
                y: *y,
                z: *z,
                yaw: *yaw,
                pitch: *pitch,
            },
        )
        .await;
        return;
    }

    if let HostToServer::SendPlayerPosition {
        to,
        id,
        sequence,
        sender_time_millis,
        x,
        y,
        z,
        yaw,
        pitch,
    } = &command
    {
        let mailbox = sessions
            .lock()
            .await
            .get(to)
            .map(|session| Arc::clone(&session.pose_mailbox));
        if let Some(mailbox) = mailbox {
            mailbox
                .replace(
                    *id,
                    Packet::PlayerPosition {
                        protocol_version: PROTOCOL_VERSION,
                        id: *id,
                        sequence: *sequence,
                        sender_time_millis: *sender_time_millis,
                        x: *x,
                        y: *y,
                        z: *z,
                        yaw: *yaw,
                        pitch: *pitch,
                    },
                )
                .await;
        }
        return;
    }

    let targeted_state = match &command {
        HostToServer::SendEntityState {
            to,
            dimension,
            sequence,
            state,
        } => Some((
            *to,
            Packet::EntityState {
                protocol_version: PROTOCOL_VERSION,
                dimension: *dimension,
                sequence: *sequence,
                state: *state,
            },
        )),
        HostToServer::SendPlayerEffect {
            to,
            sequence,
            player_id,
            effects,
        } => Some((
            *to,
            Packet::PlayerEffect {
                protocol_version: PROTOCOL_VERSION,
                sequence: *sequence,
                player_id: *player_id,
                effects: effects.clone(),
            },
        )),
        HostToServer::SendPlayerSessionUpdate {
            to,
            sequence,
            player_id,
            dimension,
            state,
        } => Some((
            *to,
            Packet::PlayerSessionUpdate {
                protocol_version: PROTOCOL_VERSION,
                sequence: *sequence,
                player_id: *player_id,
                dimension: *dimension,
                state: *state,
            },
        )),
        _ => None,
    };
    if let Some((to, packet)) = targeted_state {
        let mailbox = sessions
            .lock()
            .await
            .get(&to)
            .map(|session| Arc::clone(&session.state_mailbox));
        if let Some(mailbox) = mailbox {
            mailbox.replace(packet).await;
        }
        return;
    }

    let state_packet = match &command {
        HostToServer::BroadcastEntityState {
            dimension,
            sequence,
            state,
        } => Some(Packet::EntityState {
            protocol_version: PROTOCOL_VERSION,
            dimension: *dimension,
            sequence: *sequence,
            state: *state,
        }),
        HostToServer::BroadcastPlayerHealth {
            sequence,
            player_id,
            health,
            max_health,
            hunger,
            saturation,
            oxygen,
            is_dead,
            death_reason,
        } => Some(Packet::PlayerHealth {
            protocol_version: PROTOCOL_VERSION,
            sequence: *sequence,
            player_id: *player_id,
            health: *health,
            max_health: *max_health,
            hunger: *hunger,
            saturation: *saturation,
            oxygen: *oxygen,
            is_dead: *is_dead,
            death_reason: *death_reason,
        }),
        HostToServer::BroadcastPlayerEffect {
            sequence,
            player_id,
            effects,
        } => Some(Packet::PlayerEffect {
            protocol_version: PROTOCOL_VERSION,
            sequence: *sequence,
            player_id: *player_id,
            effects: effects.clone(),
        }),
        _ => None,
    };
    if let Some(packet) = state_packet {
        broadcast_state(sessions, packet).await;
        return;
    }

    let reliable_broadcast = matches!(
        &command,
        HostToServer::BroadcastBlockChange { .. }
            | HostToServer::BroadcastBlockEntityDelta { .. }
            | HostToServer::BroadcastEntitySpawn { .. }
            | HostToServer::BroadcastEntityDespawn { .. }
            | HostToServer::BroadcastChat { .. }
            | HostToServer::NotifyPlayerJoin { .. }
            | HostToServer::BroadcastTimeSync { .. }
            | HostToServer::BroadcastWorldRules { .. }
            | HostToServer::BroadcastLightningStrike { .. }
            | HostToServer::BroadcastSleepStateSync { .. }
            | HostToServer::BroadcastContainerSlotUpdate { .. }
    );
    let (packet, recipient) = match command {
        HostToServer::BroadcastBlockChange {
            dimension,
            revision,
            x,
            y,
            z,
            block,
            state,
            raw_fluid,
        } => (
            Packet::BlockChange {
                protocol_version: PROTOCOL_VERSION,
                dimension,
                revision,
                x,
                y,
                z,
                block,
                state,
                raw_fluid,
            },
            None,
        ),
        HostToServer::SendBlockChange {
            to,
            dimension,
            revision,
            x,
            y,
            z,
            block,
            state,
            raw_fluid,
        } => (
            Packet::BlockChange {
                protocol_version: PROTOCOL_VERSION,
                dimension,
                revision,
                x,
                y,
                z,
                block,
                state,
                raw_fluid,
            },
            Some(to),
        ),
        HostToServer::BroadcastBlockEntityDelta {
            dimension,
            revision,
            x,
            y,
            z,
            entity,
        } => (
            Packet::BlockEntityDelta {
                protocol_version: PROTOCOL_VERSION,
                dimension,
                revision,
                x,
                y,
                z,
                entity,
            },
            None,
        ),
        HostToServer::BroadcastEntitySpawn {
            dimension,
            sequence,
            state,
        } => (
            Packet::EntitySpawn {
                protocol_version: PROTOCOL_VERSION,
                dimension,
                sequence,
                state,
            },
            None,
        ),
        HostToServer::SendEntitySpawn {
            to,
            dimension,
            sequence,
            state,
        } => (
            Packet::EntitySpawn {
                protocol_version: PROTOCOL_VERSION,
                dimension,
                sequence,
                state,
            },
            Some(to),
        ),
        HostToServer::BroadcastEntityDespawn {
            dimension,
            sequence,
            entity_id,
        } => (
            Packet::EntityDespawn {
                protocol_version: PROTOCOL_VERSION,
                dimension,
                sequence,
                entity_id,
            },
            None,
        ),
        HostToServer::SendEntityDespawn {
            to,
            dimension,
            sequence,
            entity_id,
        } => (
            Packet::EntityDespawn {
                protocol_version: PROTOCOL_VERSION,
                dimension,
                sequence,
                entity_id,
            },
            Some(to),
        ),
        HostToServer::SendBlockActionResult {
            to,
            x,
            y,
            z,
            success,
            consumed_item,
            drops,
        } => (
            Packet::BlockActionResult {
                protocol_version: PROTOCOL_VERSION,
                x,
                y,
                z,
                success,
                consumed_item,
                drops,
            },
            Some(to),
        ),
        HostToServer::BroadcastTimeSync {
            ticks,
            weather,
            weather_remaining_ticks,
        } => (
            Packet::TimeSync {
                protocol_version: PROTOCOL_VERSION,
                ticks,
                weather,
                weather_remaining_ticks,
            },
            None,
        ),
        HostToServer::BroadcastWorldRules { rules } => (
            Packet::WorldRulesSync {
                protocol_version: PROTOCOL_VERSION,
                rules,
            },
            None,
        ),
        HostToServer::SendBlockEntityDelta {
            to,
            dimension,
            revision,
            x,
            y,
            z,
            entity,
        } => (
            Packet::BlockEntityDelta {
                protocol_version: PROTOCOL_VERSION,
                dimension,
                revision,
                x,
                y,
                z,
                entity,
            },
            Some(to),
        ),
        HostToServer::SendWorldRules { rules, to } => (
            Packet::WorldRulesSync {
                protocol_version: PROTOCOL_VERSION,
                rules,
            },
            Some(to),
        ),
        HostToServer::SendTimeSync {
            ticks,
            weather,
            weather_remaining_ticks,
            to,
        } => (
            Packet::TimeSync {
                protocol_version: PROTOCOL_VERSION,
                ticks,
                weather,
                weather_remaining_ticks,
            },
            Some(to),
        ),
        HostToServer::BroadcastLightningStrike { strike } => (
            Packet::LightningStrike {
                protocol_version: PROTOCOL_VERSION,
                strike,
            },
            None,
        ),
        HostToServer::BroadcastPlayerPosition { .. } => {
            unreachable!("player positions use the latest-wins pose channel")
        }
        HostToServer::SendPlayerPosition { .. } => {
            unreachable!("targeted player positions use the latest-wins pose channel")
        }
        HostToServer::SendGameplayResponse { to, response } => {
            let response = normalize_host_response(sessions, to, response).await;
            let packet = Packet::GameplayResponse {
                protocol_version: PROTOCOL_VERSION,
                response,
            };
            (packet, Some(to))
        }
        HostToServer::BroadcastEntityState { .. }
        | HostToServer::SendEntityState { .. }
        | HostToServer::BroadcastPlayerHealth { .. }
        | HostToServer::BroadcastPlayerEffect { .. }
        | HostToServer::SendPlayerEffect { .. }
        | HostToServer::SendPlayerSessionUpdate { .. } => {
            unreachable!("state packets use the latest-wins state channel")
        }
        HostToServer::BroadcastPlayerAction { id, action } => (
            Packet::PlayerAction {
                protocol_version: PROTOCOL_VERSION,
                id,
                action,
            },
            None,
        ),
        HostToServer::BroadcastChat { sender, message } => (
            Packet::ChatMessage {
                protocol_version: PROTOCOL_VERSION,
                sender,
                message,
            },
            None,
        ),
        HostToServer::NotifyPlayerJoin { id, username } => (
            Packet::PlayerJoin {
                protocol_version: PROTOCOL_VERSION,
                id,
                username,
            },
            None,
        ),
        HostToServer::SendChunk { .. } => {
            unreachable!("chunk data payloads use catchup_mailbox")
        }
        HostToServer::DisconnectCatchupClient { .. } => {
            unreachable!("catch-up disconnects are handled before packet mapping")
        }
        HostToServer::DisconnectClient { .. } => {
            unreachable!("targeted disconnects are handled before packet mapping")
        }
        HostToServer::Stop => return,
        HostToServer::SendContainerOpenResult {
            to,
            dimension,
            success,
            x,
            y,
            z,
            slots,
            revision,
        } => {
            let packet = Packet::ContainerOpenResult {
                protocol_version: PROTOCOL_VERSION,
                dimension,
                success,
                x,
                y,
                z,
                slots,
                revision,
            };
            (packet, Some(to))
        }
        HostToServer::SendContainerClose {
            to,
            dimension,
            x,
            y,
            z,
        } => {
            let packet = Packet::ContainerClose {
                protocol_version: PROTOCOL_VERSION,
                dimension,
                x,
                y,
                z,
            };
            (packet, Some(to))
        }
        HostToServer::SendContainerClickResult {
            to,
            dimension,
            success,
            slot_index,
            slot,
            dragged,
        } => {
            let packet = Packet::ContainerClickResult {
                protocol_version: PROTOCOL_VERSION,
                dimension,
                success,
                slot_index,
                slot,
                dragged,
            };
            (packet, Some(to))
        }
        HostToServer::BroadcastContainerSlotUpdate {
            dimension,
            revision,
            x,
            y,
            z,
            slot_index,
            slot,
        } => {
            let packet = Packet::ContainerSlotUpdate {
                protocol_version: PROTOCOL_VERSION,
                dimension,
                revision,
                x,
                y,
                z,
                slot_index,
                slot,
            };
            (packet, None)
        }
        HostToServer::SendContainerSlotUpdate {
            to,
            dimension,
            revision,
            x,
            y,
            z,
            slot_index,
            slot,
        } => {
            let packet = Packet::ContainerSlotUpdate {
                protocol_version: PROTOCOL_VERSION,
                dimension,
                revision,
                x,
                y,
                z,
                slot_index,
                slot,
            };
            (packet, Some(to))
        }
        HostToServer::SendPlayerRespawnResult {
            to,
            position,
            dimension,
        } => {
            let packet = Packet::PlayerRespawnResult {
                protocol_version: PROTOCOL_VERSION,
                position,
                dimension,
            };
            (packet, Some(to))
        }
        HostToServer::BroadcastSleepStateSync {
            player_id,
            is_sleeping,
        } => {
            let packet = Packet::SleepStateSync {
                protocol_version: PROTOCOL_VERSION,
                player_id,
                is_sleeping,
            };
            (packet, None)
        }
        HostToServer::SendDimensionTransfer {
            to,
            dimension,
            position,
        } => {
            if let Some(session) = sessions.lock().await.get_mut(&to) {
                // Gameplay revisions are dimension-scoped. Crossing a
                // portal starts the target world's lane; retaining the
                // source revision would reject every lower target-world
                // revision before it could reach the authority.
                session.gameplay.current_dimension = dimension;
                session.gameplay.last_client_revision = 0;
                session.gameplay.active_container = None;
            }
            let packet = Packet::DimensionTransfer {
                protocol_version: PROTOCOL_VERSION,
                player_id: to,
                dimension,
                position,
            };
            (packet, Some(to))
        }
    };

    let failed = if let Some(id) = recipient {
        send_to(sessions, id, packet).await
    } else if reliable_broadcast {
        broadcast_reliably(sessions, packet).await
    } else {
        broadcast_to(sessions, packet).await;
        Vec::new()
    };
    evict_slow_clients(sessions, server_to_host, failed).await;
}

pub(crate) async fn send_to(sessions: &Sessions, id: PlayerId, packet: Packet) -> Vec<PlayerId> {
    let target = sessions
        .lock()
        .await
        .get(&id)
        .map(|session| (session.out_tx.clone(), session.metrics.clone()));
    if let Some((tx, metrics)) = target {
        // Targeted catch-up data is reliable. Bound the wait so a client
        // that has stopped draining its queue is disconnected instead of
        // stalling the host command loop forever.
        if !reliable_send(&tx, packet, &metrics).await {
            return vec![id];
        }
    }
    Vec::new()
}

pub(crate) async fn broadcast_reliably(sessions: &Sessions, packet: Packet) -> Vec<PlayerId> {
    let senders: Vec<_> = sessions
        .lock()
        .await
        .values()
        .map(|session| (session.id, session.out_tx.clone(), session.metrics.clone()))
        .collect();
    let mut sends = tokio::task::JoinSet::new();
    for (id, tx, metrics) in senders {
        let packet = packet.clone();
        sends.spawn(async move {
            let delivered = reliable_send(&tx, packet, &metrics).await;
            (!delivered).then_some(id)
        });
    }

    let mut failed = Vec::new();
    while let Some(result) = sends.join_next().await {
        if let Ok(Some(id)) = result {
            failed.push(id);
        }
    }
    failed
}

pub(crate) async fn evict_slow_clients<S: HostEventSender>(
    sessions: &Sessions,
    server_to_host: &S,
    initial: Vec<PlayerId>,
) {
    let mut pending = initial;
    let mut handled = HashSet::new();
    while let Some(id) = pending.pop() {
        if !handled.insert(id) {
            continue;
        }
        let removed = sessions.lock().await.remove(&id);
        let Some(session) = removed else {
            continue;
        };
        let _ = session.cancel_tx.send(true);
        eprintln!(
            "[NetworkServer] Disconnecting slow client '{}' (Player ID: {}): outbound backpressure policy",
            session.username, id
        );
        let _ = server_to_host.send(ServerToHost::ClientLeft { id });
        let failed = broadcast_reliably(
            sessions,
            Packet::PlayerLeave {
                protocol_version: PROTOCOL_VERSION,
                id,
            },
        )
        .await;
        pending.extend(failed);
    }
}

pub(crate) async fn broadcast_pose_inner(sessions: &Sessions, packet: Packet) {
    let player_id = match &packet {
        Packet::PlayerPosition { id, .. } => *id,
        _ => return,
    };
    let mailboxes: Vec<_> = sessions
        .lock()
        .await
        .values()
        .map(|session| Arc::clone(&session.pose_mailbox))
        .collect();
    for mailbox in mailboxes {
        mailbox.replace(player_id, packet.clone()).await;
    }
}

#[allow(dead_code)]
pub(crate) async fn broadcast_pose(sessions: &Sessions, packet: Packet) {
    broadcast_pose_inner(sessions, packet).await;
}

pub(crate) async fn broadcast_state(sessions: &Sessions, packet: Packet) {
    let mailboxes: Vec<_> = sessions
        .lock()
        .await
        .values()
        .map(|session| Arc::clone(&session.state_mailbox))
        .collect();
    for mailbox in mailboxes {
        mailbox.replace(packet.clone()).await;
    }
}

pub(crate) async fn broadcast_to(sessions: &Sessions, packet: Packet) {
    let senders: Vec<_> = sessions
        .lock()
        .await
        .values()
        .map(|session| (session.out_tx.clone(), session.metrics.clone()))
        .collect();
    for (tx, metrics) in senders {
        best_effort_send(&tx, packet.clone(), &metrics);
    }
}
