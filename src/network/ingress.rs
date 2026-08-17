use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use tokio::sync::{mpsc, watch};
use tokio::time::{self, Instant};

use super::channels::{HostEventSendError, HostEventSender, ServerConfig, ServerToHost};
use super::egress::{broadcast_reliably, evict_slow_clients, send_to};
use super::protocol::{
    wrap_legacy, Action, GameplayRequest, LegacyGameplay, Packet, PlayerId, RejectReason,
    PROTOCOL_VERSION,
};
use super::session::{
    packet_bytes, queue_now_ms, queue_stats, reliable_send, reliable_send_and_wait,
    send_connection_packet, send_writer_packet, CatchupMailbox, ClientSession,
    GameplaySessionState, NetworkMetrics, PoseMailbox, PreAuthSlot, QueuedPacket,
    RequestRateLimiter, Sessions, StateMailbox, CLIENT_QUEUE_CAPACITY, CLIENT_TIMEOUT,
    KEEPALIVE_INTERVAL, MAX_CHAT_CHARS,
};
use super::transport::Connection;

pub(crate) fn authenticate_handshake_username(raw: &str) -> Result<String, &'static str> {
    crate::save::normalize_player_identity(raw).map_err(|_| "invalid username")
}

pub(crate) fn chat_exceeds_display_cap(message: &str) -> bool {
    message.chars().count() > MAX_CHAT_CHARS
}

pub(crate) async fn queue_initial_roster(
    tx: &mpsc::Sender<QueuedPacket>,
    roster: impl IntoIterator<Item = (PlayerId, String)>,
    metrics: &NetworkMetrics,
) -> Result<(), ()> {
    for (id, username) in roster {
        let packet = Packet::PlayerJoin {
            protocol_version: PROTOCOL_VERSION,
            id,
            username,
        };
        let bytes = packet_bytes(&packet);
        let permit = match tx.reserve().await {
            Ok(permit) => permit,
            Err(_) => return Err(()),
        };
        queue_stats().enqueue(bytes, queue_now_ms());
        permit.send(QueuedPacket::Outbound(
            super::session::TrackedPacket::new(packet, metrics),
        ));
    }
    Ok(())
}

pub(crate) fn prepare_gameplay_request(
    session: &mut ClientSession,
    mut request: GameplayRequest,
) -> GameplayRequest {
    let state = &mut session.gameplay;
    if request.request_id == 0 {
        request.request_id = state.allocate_request_id();
    } else if request.request_id >= state.next_request_id {
        state.next_request_id = request.request_id.wrapping_add(1).max(1);
    }
    if request.client_sequence == 0 {
        request.client_sequence = state.last_client_sequence.wrapping_add(1).max(1);
    }
    // The connection, rather than the packet, is the authenticated owner.
    request.session_id = session.id;
    if request.dimension <= 2 {
        state.current_dimension = request.dimension;
    }
    request
}

pub(crate) fn legacy_gameplay_request(
    session: &mut ClientSession,
    leftover: LegacyGameplay,
    dimension: u8,
    client_revision: u64,
) -> Option<GameplayRequest> {
    Some(prepare_gameplay_request(
        session,
        wrap_legacy(session.id, dimension, client_revision, leftover)?,
    ))
}

/// Apply transport/session gates once for both native envelopes and legacy
/// adapters. A rejection is sent with a real server sequence and cached;
/// accepted requests are forwarded exactly once to the authority channel.
pub(crate) async fn route_gameplay_request<S: HostEventSender>(
    sessions: &Sessions,
    id: PlayerId,
    mut request: GameplayRequest,
    request_rate: &mut RequestRateLimiter,
    server_to_host: &S,
) -> Result<(), String> {
    let mut immediate_response = None;
    let mut forward = None;
    {
        let mut sessions_guard = sessions.lock().await;
        let Some(session) = sessions_guard.get_mut(&id) else {
            return Err("authenticated session disappeared".into());
        };
        request = prepare_gameplay_request(session, request);
        let state = &mut session.gameplay;

        if let Some(cached) = state.cached_response(request.request_id) {
            session.metrics.record_duplicate_request();
            immediate_response = Some(cached);
        } else if state.in_flight.contains(&request.request_id) {
            // The first copy is still being processed by the authority;
            // retransmission remains idempotent and needs no second event.
            session.metrics.record_duplicate_request();
            return Ok(());
        } else if let Err(reason) = request.validate_bounds() {
            session.metrics.record_rejected_request();
            immediate_response = Some(state.rejection(request.request_id, reason));
        } else if request.client_sequence <= state.last_client_sequence {
            session.metrics.record_rejected_request();
            immediate_response =
                Some(state.rejection(request.request_id, RejectReason::OutOfOrder));
        } else if request.client_revision < state.last_client_revision {
            session.metrics.record_rejected_request();
            immediate_response =
                Some(state.rejection(request.request_id, RejectReason::InvalidRevision));
        } else if !request_rate.allow() {
            session.metrics.record_rejected_request();
            immediate_response =
                Some(state.rejection(request.request_id, RejectReason::RateLimited));
        } else {
            state.last_client_sequence = request.client_sequence;
            state.last_client_revision = request.client_revision;
            state.in_flight.insert(request.request_id);
            forward = Some(request);
        }
    }

    if let Some(response) = immediate_response {
        let failed = send_to(
            sessions,
            id,
            Packet::GameplayResponse {
                protocol_version: PROTOCOL_VERSION,
                response,
            },
        )
        .await;
        if !failed.is_empty() {
            return Err("gameplay response queue is unavailable".into());
        }
        return Ok(());
    }

    if let Some(request) = forward {
        match server_to_host.send(ServerToHost::GameplayRequest {
            id,
            request: request.clone(),
        }) {
            Ok(()) => {}
            Err(HostEventSendError::Full) => {
                let failed = send_to(
                    sessions,
                    id,
                    Packet::GameplayResponse {
                        protocol_version: PROTOCOL_VERSION,
                        response: {
                            let mut sessions_guard = sessions.lock().await;
                            let Some(session) = sessions_guard.get_mut(&id) else {
                                return Err("authenticated session disappeared".into());
                            };
                            session.gameplay.in_flight.remove(&request.request_id);
                            session
                                .gameplay
                                .rejection(request.request_id, RejectReason::QueueFull)
                        },
                    },
                )
                .await;
                if !failed.is_empty() {
                    return Err("gameplay response queue is unavailable".into());
                }
            }
            Err(HostEventSendError::Closed) => {
                return Err("host channel closed (GameplayRequest)".into());
            }
        }
    }
    Ok(())
}

pub(crate) async fn run_client<S: HostEventSender>(
    mut connection: Connection,
    seed: u64,
    gamemode: u8,
    next_player_id: Arc<AtomicU64>,
    sessions: Sessions,
    server_to_host: S,
    config: ServerConfig,
    metrics: NetworkMetrics,
    pre_auth: Option<PreAuthSlot>,
) {
    let handshake_result = time::timeout(config.handshake_timeout, connection.recv()).await;
    if let Ok(Ok(packet)) = &handshake_result {
        metrics.record_inbound(packet);
    }
    let handshake = match handshake_result {
        Ok(Ok(Packet::Handshake {
            protocol_version,
            username,
        })) => {
            eprintln!("[NetworkServer] Received Handshake: username='{username}', protocol_version={protocol_version}");
            if protocol_version != PROTOCOL_VERSION {
                eprintln!("[NetworkServer] Handshake rejected: version mismatch (expected {PROTOCOL_VERSION}, got {protocol_version})");
                let _ = send_connection_packet(
                    &mut connection,
                    Packet::Disconnect {
                        protocol_version: PROTOCOL_VERSION,
                        reason: format!(
                            "protocol version mismatch: server {PROTOCOL_VERSION}, client {protocol_version}"
                        ),
                    },
                    &metrics,
                )
                .await;
                return;
            }
            username
        }
        Ok(Ok(Packet::ServerListPingRequest { protocol_version })) => {
            let online_players = sessions.lock().await.len().min(u16::MAX as usize) as u16;
            let _ = send_connection_packet(
                &mut connection,
                Packet::ServerListPingResponse {
                    protocol_version: PROTOCOL_VERSION,
                    version: env!("CARGO_PKG_VERSION").to_string(),
                    motd: config.motd.clone(),
                    online_players,
                    max_players: config.max_players.min(u16::MAX as usize) as u16,
                },
                &metrics,
            )
            .await;
            if protocol_version != PROTOCOL_VERSION {
                eprintln!(
                    "[NetworkServer] server-list ping version mismatch: client {protocol_version}, server {PROTOCOL_VERSION}"
                );
            }
            return;
        }
        Ok(Ok(packet)) => {
            eprintln!("[NetworkServer] Handshake rejected: expected Packet::Handshake, got {packet:?}");
            let _ = send_connection_packet(
                &mut connection,
                Packet::Disconnect {
                    protocol_version: PROTOCOL_VERSION,
                    reason: "expected handshake".into(),
                },
                &metrics,
            )
            .await;
            return;
        }
        Ok(Err(err)) => {
            eprintln!("[NetworkServer] Handshake receive error: {err}");
            return;
        }
        Err(_) => {
            eprintln!("[NetworkServer] Handshake timed out");
            return;
        }
    };

    let normalized_username = match authenticate_handshake_username(&handshake) {
        Ok(identity) => identity,
        Err(reason) => {
            let _ = send_connection_packet(
                &mut connection,
                Packet::Disconnect {
                    protocol_version: PROTOCOL_VERSION,
                    reason: reason.into(),
                },
                &metrics,
            )
            .await;
            return;
        }
    };
    {
        let sessions_guard = sessions.lock().await;
        if sessions_guard.len() >= config.max_players.max(1) {
            let _ = send_connection_packet(
                &mut connection,
                Packet::Disconnect {
                    protocol_version: PROTOCOL_VERSION,
                    reason: "server is full".into(),
                },
                &metrics,
            )
            .await;
            return;
        }
        if !config.whitelist.is_empty() && !config.whitelist.contains(&normalized_username) {
            let _ = send_connection_packet(
                &mut connection,
                Packet::Disconnect {
                    protocol_version: PROTOCOL_VERSION,
                    reason: "not whitelisted".into(),
                },
                &metrics,
            )
            .await;
            return;
        }
        if sessions_guard
            .values()
            .any(|session| session.username == normalized_username)
        {
            let _ = send_connection_packet(
                &mut connection,
                Packet::Disconnect {
                    protocol_version: PROTOCOL_VERSION,
                    reason: "duplicate login".into(),
                },
                &metrics,
            )
            .await;
            return;
        }
    }

    let id = next_player_id.fetch_add(1, Ordering::Relaxed);

    let (out_tx, mut out_rx) = mpsc::channel(CLIENT_QUEUE_CAPACITY);
    let roster_tx = out_tx.clone();
    let pose_mailbox = Arc::new(PoseMailbox::with_metrics(metrics.clone()));
    let state_mailbox = Arc::new(StateMailbox::with_metrics(metrics.clone()));
    let catchup_mailbox = Arc::new(CatchupMailbox::with_capacity_and_metrics(
        config.catchup_queue_capacity,
        metrics.clone(),
    ));
    let (cancel_tx, mut cancel_rx) = watch::channel(false);
    let (mut reader, mut writer) = connection.into_split();
    let writer_pose_mailbox = Arc::clone(&pose_mailbox);
    let writer_state_mailbox = Arc::clone(&state_mailbox);
    let writer_catchup_mailbox = Arc::clone(&catchup_mailbox);
    let writer_metrics = metrics.clone();
    let mut send_task = tokio::spawn(async move {
        let mut keepalive =
            time::interval_at(Instant::now() + KEEPALIVE_INTERVAL, KEEPALIVE_INTERVAL);

        loop {
            tokio::select! {
                biased;
                queued = out_rx.recv() => {
                    match queued {
                        Some(queued) => {
                            let (packet, stats, completion) = match queued {
                                QueuedPacket::Reliable(packet) => (packet, crate::perf::QueueCategory::Reliable, None),
                                QueuedPacket::ReliableWithAck(packet, completion) => (
                                    packet,
                                    crate::perf::QueueCategory::Reliable,
                                    Some(completion),
                                ),
                                QueuedPacket::Outbound(packet) => (packet, crate::perf::QueueCategory::Outbound, None),
                            };
                            let packet = packet.into_packet();
                            crate::perf::queue_stats(stats).dequeue(packet_bytes(&packet));
                            let sent = send_writer_packet(&mut writer, &packet, &writer_metrics)
                                .await
                                .is_ok();
                            if let Some(completion) = completion {
                                let _ = completion.send(sent);
                            }
                            if !sent {
                                eprintln!("[NetworkServer] Send task: writer send failed for queued packet");
                                break;
                            }
                        }
                        None => {
                            eprintln!("[NetworkServer] Send task: out_rx closed (session removed)");
                            break;
                        }
                    }
                }
                _ = writer_pose_mailbox.notify.notified() => {
                    for packet in writer_pose_mailbox.drain().await {
                        if send_writer_packet(&mut writer, &packet, &writer_metrics)
                            .await
                            .is_err()
                        {
                            eprintln!("[NetworkServer] Send task: writer send failed for pose");
                            return;
                        }
                    }
                }
                _ = writer_state_mailbox.notify.notified() => {
                    for packet in writer_state_mailbox.drain().await {
                        if send_writer_packet(&mut writer, &packet, &writer_metrics)
                            .await
                            .is_err()
                        {
                            eprintln!("[NetworkServer] Send task: writer send failed for state");
                            return;
                        }
                    }
                }
                _ = writer_catchup_mailbox.notify.notified() => {
                    if !config.catchup_drain_delay.is_zero() {
                        time::sleep(config.catchup_drain_delay).await;
                    }
                    if let Some(packet) = writer_catchup_mailbox.pop().await {
                        if send_writer_packet(&mut writer, &packet, &writer_metrics)
                            .await
                            .is_err()
                        {
                            eprintln!("[NetworkServer] Send task: writer send failed for catchup chunk");
                            return;
                        }
                    }
                }
                _ = keepalive.tick() => {
                    let packet = Packet::Keepalive {
                        protocol_version: PROTOCOL_VERSION,
                    };
                    if send_writer_packet(&mut writer, &packet, &writer_metrics)
                        .await
                        .is_err()
                    {
                        eprintln!("[NetworkServer] Send task: keepalive send failed");
                        break;
                    }
                }
            }
        }
    });

    let mut request_rate = RequestRateLimiter::new(config.request_rate_per_second);
    let mut pose_rate = RequestRateLimiter::new(config.pose_rate_per_second);
    let mut chat_rate = RequestRateLimiter::new(config.chat_rate_per_second);

    // Re-check and reserve the authenticated identity while inserting the
    // transport session. The handshake preflight above is only an early
    // rejection; this lock closes the concurrent duplicate/max-player
    // race before LoginSuccess is allowed onto the wire.
    let reservation_error = {
        let mut sessions_guard = sessions.lock().await;
        if sessions_guard.len() >= config.max_players.max(1) {
            Some("server is full")
        } else if sessions_guard
            .values()
            .any(|session| session.username == normalized_username)
        {
            Some("duplicate login")
        } else {
            sessions_guard.insert(
                id,
                ClientSession {
                    id,
                    username: normalized_username.clone(),
                    out_tx: out_tx.clone(),
                    pose_mailbox: Arc::clone(&pose_mailbox),
                    state_mailbox: Arc::clone(&state_mailbox),
                    catchup_mailbox: Arc::clone(&catchup_mailbox),
                    cancel_tx: cancel_tx.clone(),
                    gameplay: GameplaySessionState::default(),
                    metrics: metrics.clone(),
                },
            );
            None
        }
    };
    if let Some(reason) = reservation_error {
        let _ = reliable_send_and_wait(
            &roster_tx,
            Packet::Disconnect {
                protocol_version: PROTOCOL_VERSION,
                reason: reason.into(),
            },
            &metrics,
        )
        .await;
        send_task.abort();
        return;
    }
    // Authenticated sessions count against max_players, not the pre-auth
    // handshake cap.
    drop(pre_auth);

    if !reliable_send(
        &roster_tx,
        Packet::LoginSuccess {
            protocol_version: PROTOCOL_VERSION,
            player_id: id,
            seed,
            gamemode,
        },
        &metrics,
    )
    .await
    {
        sessions.lock().await.remove(&id);
        send_task.abort();
        return;
    }
    eprintln!("[NetworkServer] Sent LoginSuccess to '{normalized_username}' (Player ID: {id})");
    let mut roster: Vec<(PlayerId, String)> = sessions
        .lock()
        .await
        .values()
        .filter(|session| session.id != id)
        .map(|session| (session.id, session.username.clone()))
        .collect();
    roster.sort_by_key(|(existing_id, _)| *existing_id);
    if !matches!(
        time::timeout(
            CLIENT_TIMEOUT,
            queue_initial_roster(&roster_tx, roster, &metrics),
        )
        .await,
        Ok(Ok(()))
    ) {
        sessions.lock().await.remove(&id);
        send_task.abort();
        return;
    }
    drop(roster_tx);
    if server_to_host
        .send(ServerToHost::ClientJoined {
            id,
            username: normalized_username,
        })
        .is_err()
    {
        sessions.lock().await.remove(&id);
        send_task.abort();
        return;
    }

    #[allow(unused_assignments)]
    let mut disconnect_reason = "unknown".to_string();
    loop {
        tokio::select! {
            incoming = time::timeout(CLIENT_TIMEOUT, reader.recv()) => {
                let incoming = incoming.map(|result| {
                    result.map(|packet| {
                        metrics.record_inbound(&packet);
                        packet
                    })
                });
                match incoming {
                    Ok(Ok(packet)) if packet.protocol_version() != PROTOCOL_VERSION => {
                        disconnect_reason = format!("protocol version mismatch (got {}, expected {})", packet.protocol_version(), PROTOCOL_VERSION);
                        break;
                    }
                    Ok(Ok(Packet::PlayerPosition {
                        sequence,
                        sender_time_millis,
                        x,
                        y,
                        z,
                        yaw,
                        pitch,
                        ..
                    })) => {
                        if !pose_rate.allow() {
                            continue;
                        }
                        match server_to_host.send(ServerToHost::ClientPosition {
                            id,
                            sequence,
                            sender_time_millis,
                            x,
                            y,
                            z,
                            yaw,
                            pitch,
                        }) {
                            Ok(()) => {}
                            Err(HostEventSendError::Full) => {}
                            Err(HostEventSendError::Closed) => {
                                disconnect_reason = "host channel closed (ClientPosition)".into();
                                break;
                            }
                        }
                    }
                    Ok(Ok(Packet::PlayerAction { action, .. })) => {
                        if server_to_host.send(ServerToHost::ClientAction { id, action }).is_err() {
                            disconnect_reason = "host channel closed (ClientAction)".into();
                            break;
                        }
                    }
                    Ok(Ok(Packet::GameplayRequest { request, .. })) => {
                        if let Err(reason) = route_gameplay_request(
                            &sessions,
                            id,
                            request,
                            &mut request_rate,
                            &server_to_host,
                        )
                        .await
                        {
                            disconnect_reason = reason;
                            break;
                        }
                    }
                    Ok(Ok(Packet::BlockChange {
                        dimension,
                        revision,
                        x,
                        y,
                        z,
                        block,
                        ..
                    })) => {
                        let request = {
                            let mut sessions_guard = sessions.lock().await;
                            let Some(session) = sessions_guard.get_mut(&id) else {
                                disconnect_reason = "authenticated session disappeared".into();
                                break;
                            };
                            legacy_gameplay_request(
                                session,
                                LegacyGameplay::BlockChange { x, y, z, block },
                                dimension,
                                revision,
                            )
                            .expect("BlockChange leftover always wraps")
                        };
                        if let Err(reason) = route_gameplay_request(
                            &sessions,
                            id,
                            request,
                            &mut request_rate,
                            &server_to_host,
                        )
                        .await
                        {
                            disconnect_reason = reason;
                            break;
                        }
                    }
                    Ok(Ok(Packet::BlockActionRequest {
                        action,
                        x,
                        y,
                        z,
                        block,
                        held_item,
                        ..
                    })) => {
                        let request = {
                            let mut sessions_guard = sessions.lock().await;
                            let Some(session) = sessions_guard.get_mut(&id) else {
                                disconnect_reason = "authenticated session disappeared".into();
                                break;
                            };
                            legacy_gameplay_request(
                                session,
                                LegacyGameplay::BlockAction {
                                    action,
                                    x,
                                    y,
                                    z,
                                    block,
                                    held_item,
                                },
                                session.gameplay.current_dimension,
                                session.gameplay.last_client_revision,
                            )
                        };
                        let Some(request) = request else {
                            // Action::Use has no BlockAction kind. Do not
                            // invent Place/StartBreak or fall back to BlockUse.
                            continue;
                        };
                        if let Err(reason) = route_gameplay_request(
                            &sessions,
                            id,
                            request,
                            &mut request_rate,
                            &server_to_host,
                        )
                        .await
                        {
                            disconnect_reason = reason;
                            break;
                        }
                    }
                    Ok(Ok(Packet::ChatMessage { message, .. })) => {
                        if chat_exceeds_display_cap(&message) || !chat_rate.allow() {
                            continue;
                        }
                        match server_to_host.send(ServerToHost::ChatFromClient { id, message }) {
                            Ok(()) => {}
                            Err(HostEventSendError::Full) => {}
                            Err(HostEventSendError::Closed) => {
                                disconnect_reason = "host channel closed (ChatFromClient)".into();
                                break;
                            }
                        }
                    }
                    Ok(Ok(Packet::ChunkAck {
                        dimension,
                        cx,
                        cz,
                        revision,
                        ..
                    })) => {
                        if server_to_host.send(ServerToHost::CatchupAck {
                            id,
                            dimension,
                            cx,
                            cz,
                            revision,
                        }).is_err() {
                            disconnect_reason = "host channel closed (CatchupAck)".into();
                            break;
                        }
                    }
                    Ok(Ok(Packet::PlayerRespawnRequest { .. })) => {
                        if server_to_host.send(ServerToHost::ClientRespawnRequest { id }).is_err() {
                            disconnect_reason = "host channel closed (ClientRespawnRequest)".into();
                            break;
                        }
                    }
                    Ok(Ok(Packet::SleepRequest { x, y, z, .. })) => {
                        let request = {
                            let mut sessions_guard = sessions.lock().await;
                            let Some(session) = sessions_guard.get_mut(&id) else {
                                disconnect_reason = "authenticated session disappeared".into();
                                break;
                            };
                            let dimension = session.gameplay.current_dimension;
                            let revision = session.gameplay.last_client_revision;
                            legacy_gameplay_request(
                                session,
                                LegacyGameplay::Sleep { x, y, z },
                                dimension,
                                revision,
                            )
                            .expect("Sleep leftover always wraps")
                        };
                        if let Err(reason) = route_gameplay_request(
                            &sessions,
                            id,
                            request,
                            &mut request_rate,
                            &server_to_host,
                        )
                        .await
                        {
                            disconnect_reason = reason;
                            break;
                        }
                    }
                    Ok(Ok(Packet::ContainerOpenRequest { dimension, x, y, z, .. })) => {
                        let request = {
                            let mut sessions_guard = sessions.lock().await;
                            let Some(session) = sessions_guard.get_mut(&id) else {
                                disconnect_reason = "authenticated session disappeared".into();
                                break;
                            };
                            session.gameplay.active_container = Some((dimension, x, y, z));
                            legacy_gameplay_request(
                                session,
                                LegacyGameplay::ContainerOpen { x, y, z },
                                dimension,
                                session.gameplay.last_client_revision,
                            )
                            .expect("ContainerOpen leftover always wraps")
                        };
                        if let Err(reason) = route_gameplay_request(
                            &sessions,
                            id,
                            request,
                            &mut request_rate,
                            &server_to_host,
                        )
                        .await
                        {
                            disconnect_reason = reason;
                            break;
                        }
                    }
                    Ok(Ok(Packet::ContainerClickRequest {
                        dimension,
                        revision,
                        slot_index,
                        is_left,
                        dragged,
                        ..
                    })) => {
                        let request = {
                            let mut sessions_guard = sessions.lock().await;
                            let Some(session) = sessions_guard.get_mut(&id) else {
                                disconnect_reason = "authenticated session disappeared".into();
                                break;
                            };
                            match session.gameplay.active_container {
                                Some((active_dimension, x, y, z))
                                    if active_dimension == dimension => {
                                        legacy_gameplay_request(
                                            session,
                                            LegacyGameplay::ContainerClick {
                                                x,
                                                y,
                                                z,
                                                slot: slot_index,
                                                is_left,
                                                dragged,
                                            },
                                            dimension,
                                            revision,
                                        )
                                    }
                                _ => None,
                            }
                        };
                        let Some(request) = request else {
                            continue;
                        };
                        if let Err(reason) = route_gameplay_request(
                            &sessions,
                            id,
                            request,
                            &mut request_rate,
                            &server_to_host,
                        )
                        .await
                        {
                            disconnect_reason = reason;
                            break;
                        }
                    }
                    Ok(Ok(Packet::ContainerClose { dimension, x, y, z, .. })) => {
                        let request = {
                            let mut sessions_guard = sessions.lock().await;
                            let Some(session) = sessions_guard.get_mut(&id) else {
                                disconnect_reason = "authenticated session disappeared".into();
                                break;
                            };
                            let request = legacy_gameplay_request(
                                session,
                                LegacyGameplay::ContainerClose { x, y, z },
                                dimension,
                                session.gameplay.last_client_revision,
                            )
                            .expect("ContainerClose leftover always wraps");
                            session.gameplay.active_container = None;
                            request
                        };
                        if let Err(reason) = route_gameplay_request(
                            &sessions,
                            id,
                            request,
                            &mut request_rate,
                            &server_to_host,
                        )
                        .await
                        {
                            disconnect_reason = reason;
                            break;
                        }
                    }
                    Ok(Ok(Packet::Keepalive { .. })) => {}
                    Ok(Ok(Packet::Disconnect { reason, .. })) => {
                        disconnect_reason = format!("client sent Disconnect: {reason}");
                        break;
                    }
                    Ok(Err(error)) => {
                        disconnect_reason = format!("connection recv error: {error}");
                        break;
                    }
                    Err(_) => {
                        disconnect_reason = format!("timeout: no packet received within {CLIENT_TIMEOUT:?}");
                        break;
                    }
                    Ok(Ok(_)) => {}
                }
            }
            _ = &mut send_task => {
                disconnect_reason = "send task exited".into();
                break;
            }
            changed = cancel_rx.changed() => {
                disconnect_reason = if changed.is_ok() && *cancel_rx.borrow() {
                    "session cancelled".into()
                } else {
                    "session cancellation channel closed".into()
                };
                break;
            }
        }
    }

    eprintln!(
        "[NetworkServer] Client '{}' (Player ID: {}) disconnecting: {disconnect_reason}",
        sessions
            .lock()
            .await
            .get(&id)
            .map(|s| s.username.clone())
            .unwrap_or_default(),
        id
    );
    remove_client(id, &sessions, &server_to_host).await;
    send_task.abort();
}

pub(crate) async fn remove_client<S: HostEventSender>(
    id: PlayerId,
    sessions: &Sessions,
    server_to_host: &S,
) {
    let removed = sessions.lock().await.remove(&id);
    let Some(session) = removed else {
        return;
    };

    let _ = session.cancel_tx.send(true);
    eprintln!(
        "[NetworkServer] Client '{}' (Player ID: {}) disconnected",
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
    evict_slow_clients(sessions, server_to_host, failed).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handshake_rejects_mutating_and_reserved_identities() {
        assert_eq!(authenticate_handshake_username("Alice").unwrap(), "alice");
        assert_eq!(
            authenticate_handshake_username("foo_bar").unwrap(),
            "foo_bar"
        );
        assert_eq!(
            authenticate_handshake_username("foo.bar").unwrap_err(),
            "invalid username"
        );
        assert_eq!(
            authenticate_handshake_username("Alice/../Alice").unwrap_err(),
            "invalid username"
        );
        assert_eq!(
            authenticate_handshake_username("CON").unwrap_err(),
            "invalid username"
        );
        assert_eq!(
            authenticate_handshake_username("con.txt").unwrap_err(),
            "invalid username"
        );
        assert_eq!(
            authenticate_handshake_username("").unwrap_err(),
            "invalid username"
        );
    }

    #[test]
    fn chat_display_cap_is_256_chars() {
        assert!(!chat_exceeds_display_cap(&"a".repeat(256)));
        assert!(chat_exceeds_display_cap(&"a".repeat(257)));
        assert!(!chat_exceeds_display_cap(&"😀".repeat(256)));
        assert!(chat_exceeds_display_cap(&"😀".repeat(257)));
    }
}
