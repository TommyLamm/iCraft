use std::collections::HashSet;
use std::sync::Arc;

use super::channels::{
    HostEventSender, HostToServer, ProjectionDest, ProjectionEvent, ServerToHost,
};
use super::protocol::{Packet, PlayerId, PROTOCOL_VERSION};
use super::session::{
    best_effort_send_encoded, reliable_send, reliable_send_encoded, NetworkMetrics, Sessions,
};

pub(crate) async fn normalize_host_response(
    sessions: &Sessions,
    id: PlayerId,
    mut response: super::protocol::GameplayResponse,
) -> super::protocol::GameplayResponse {
    let mut sessions_guard = sessions.lock().await;
    let Some(session) = sessions_guard.get_mut(&id) else {
        if response.server_sequence == 0 {
            response.server_sequence = 1;
        }
        return response;
    };
    let state = &mut session.gameplay;
    state.mark_completed(response.request_id);
    if response.server_sequence == 0 {
        response.server_sequence = state.allocate_server_sequence();
    } else if response.server_sequence > state.last_server_sequence {
        state.last_server_sequence = response.server_sequence;
    }
    // Non-zero sequences at or below the watermark are kept as-is so an
    // authority cache replay stays byte-identical for the client.
    if let crate::network::protocol::GameplayOutcome::Accepted { revision } = response.outcome {
        state.last_client_revision = state.last_client_revision.max(revision);
    }
    response
}

/// Mailbox / delivery class derived from the wire `Packet` variant — not a
/// second payload schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PacketDelivery {
    Catchup,
    Pose,
    State,
    Reliable,
}

fn classify_packet(packet: &Packet) -> PacketDelivery {
    match packet {
        Packet::ChunkData { .. } => PacketDelivery::Catchup,
        Packet::PlayerPosition { .. } => PacketDelivery::Pose,
        Packet::EntityState { .. }
        | Packet::PlayerEffect { .. }
        | Packet::PlayerSessionUpdate { .. } => PacketDelivery::State,
        _ => PacketDelivery::Reliable,
    }
}

pub(crate) async fn handle_host_command<S: HostEventSender>(
    sessions: &Sessions,
    server_to_host: &S,
    metrics: &NetworkMetrics,
    command: HostToServer,
) {
    match command {
        HostToServer::Stop => {}
        HostToServer::DisconnectCatchupClient { to, reason } => {
            eprintln!("[NetworkServer] Applying slow catch-up policy to Player ID {to}: {reason}");
            evict_slow_clients(sessions, server_to_host, vec![to]).await;
        }
        HostToServer::DisconnectClient { to, reason } => {
            let failed = send_to(
                sessions,
                to,
                Packet::Disconnect {
                    reason,
                },
            )
            .await;
            evict_slow_clients(sessions, server_to_host, failed).await;
        }
        HostToServer::Project(event) => {
            deliver_projection(sessions, server_to_host, metrics, event).await;
        }
    }
}

async fn deliver_projection<S: HostEventSender>(
    sessions: &Sessions,
    server_to_host: &S,
    metrics: &NetworkMetrics,
    event: ProjectionEvent,
) {
    let ProjectionEvent {
        dest,
        mut packet,
    } = event;

    if let (
        ProjectionDest::Session(to),
        Packet::PlayerSessionUpdate { player_id, .. },
    ) = (dest, &packet)
    {
        if to != *player_id {
            metrics.record_rejected_request();
            return;
        }
    }

    if let (ProjectionDest::Session(to), Packet::GameplayResponse { response, .. }) =
        (dest, &mut packet)
    {
        *response = normalize_host_response(sessions, to, response.clone()).await;
    }

    if let Packet::DimensionTransfer {
        player_id,
        dimension,
        ..
    } = &packet
    {
        if let Some(session) = sessions.lock().await.get_mut(player_id) {
            session.gameplay.current_dimension = *dimension;
            session.gameplay.last_client_revision = 0;
        }
    }

    match (dest, classify_packet(&packet)) {
        (ProjectionDest::Session(to), PacketDelivery::Catchup) => {
            let mailbox = sessions
                .lock()
                .await
                .get(&to)
                .map(|session| Arc::clone(&session.catchup_mailbox));
            if let Some(mailbox) = mailbox {
                let _ = mailbox.replace(packet).await;
            }
        }
        (ProjectionDest::Session(to), PacketDelivery::Pose) => {
            let player_id = match &packet {
                Packet::PlayerPosition { id, .. } => *id,
                _ => return,
            };
            let mailbox = sessions
                .lock()
                .await
                .get(&to)
                .map(|session| Arc::clone(&session.pose_mailbox));
            if let Some(mailbox) = mailbox {
                mailbox.replace(player_id, packet).await;
            }
        }
        (ProjectionDest::Broadcast, PacketDelivery::Pose) => {
            broadcast_pose_inner(sessions, packet).await;
        }
        (ProjectionDest::Session(to), PacketDelivery::State) => {
            let mailbox = sessions
                .lock()
                .await
                .get(&to)
                .map(|session| Arc::clone(&session.state_mailbox));
            if let Some(mailbox) = mailbox {
                mailbox.replace(packet).await;
            }
        }
        (ProjectionDest::Broadcast, PacketDelivery::State) => {
            broadcast_state(sessions, packet).await;
        }
        (ProjectionDest::Session(to), PacketDelivery::Reliable) => {
            let failed = send_to(sessions, to, packet).await;
            evict_slow_clients(sessions, server_to_host, failed).await;
        }
        (ProjectionDest::Broadcast, PacketDelivery::Reliable) => {
            let failed = broadcast_reliably(sessions, packet).await;
            evict_slow_clients(sessions, server_to_host, failed).await;
        }
        (ProjectionDest::Broadcast, PacketDelivery::Catchup) => {
            // ChunkData is always session-targeted; ignore malformed broadcasts.
        }
    }
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
    let Ok(encoded) = super::session::EncodedPacket::new(packet) else {
        return Vec::new();
    };
    let senders: Vec<_> = sessions
        .lock()
        .await
        .values()
        .map(|session| (session.id, session.out_tx.clone(), session.metrics.clone()))
        .collect();
    let mut sends = tokio::task::JoinSet::new();
    for (id, tx, metrics) in senders {
        let encoded = encoded.clone();
        sends.spawn(async move {
            let delivered = reliable_send_encoded(&tx, encoded, &metrics).await;
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
    let Ok(encoded) = super::session::EncodedPacket::new(packet) else {
        return;
    };
    let mailboxes: Vec<_> = sessions
        .lock()
        .await
        .values()
        .map(|session| Arc::clone(&session.pose_mailbox))
        .collect();
    for mailbox in mailboxes {
        mailbox.replace_encoded(player_id, encoded.clone()).await;
    }
}

pub(crate) async fn broadcast_state(sessions: &Sessions, packet: Packet) {
    let Ok(encoded) = super::session::EncodedPacket::new(packet) else {
        return;
    };
    let mailboxes: Vec<_> = sessions
        .lock()
        .await
        .values()
        .map(|session| Arc::clone(&session.state_mailbox))
        .collect();
    for mailbox in mailboxes {
        mailbox.replace_encoded(encoded.clone()).await;
    }
}

pub(crate) async fn broadcast_to(sessions: &Sessions, packet: Packet) {
    let Ok(encoded) = super::session::EncodedPacket::new(packet) else {
        return;
    };
    let senders: Vec<_> = sessions
        .lock()
        .await
        .values()
        .map(|session| (session.out_tx.clone(), session.metrics.clone()))
        .collect();
    for (tx, metrics) in senders {
        best_effort_send_encoded(&tx, encoded.clone(), &metrics);
    }
}
