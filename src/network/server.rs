use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize};
use std::sync::{mpsc as std_mpsc, Arc};
use std::thread::JoinHandle;
use std::time::Duration;

use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio::time;

// Public re-exports for external callers (e.g. `use icraft::network::server::*` and `use crate::network::server::*`)
pub use super::channels::{
    HostEventSendError, HostEventSender, HostToServer, ServerConfig, ServerToHost,
};
pub use super::session::{NetworkMetrics, NetworkMetricsSnapshot};

// Crate-internal re-exports preserving existing internal usage
pub(crate) use super::channels::{
    MeteredHostEventSender, DEFAULT_CHAT_RATE_PER_SECOND, DEFAULT_POSE_RATE_PER_SECOND,
    HANDSHAKE_TIMEOUT, MAX_CATCHUP_QUEUE_DEPTH,
};
pub(crate) use super::egress::{
    broadcast_reliably, broadcast_state, broadcast_to, evict_slow_clients, handle_host_command,
    normalize_host_response, send_to,
};
pub(crate) use super::ingress::{
    authenticate_handshake_username, chat_exceeds_display_cap, legacy_gameplay_request,
    prepare_gameplay_request, queue_initial_roster, remove_client, route_gameplay_request,
    run_client,
};
#[cfg(test)]
use super::protocol::Packet;
use super::protocol::PlayerId;
pub(crate) use super::session::{
    best_effort_send, packet_bytes, queue_now_ms, queue_stats, reliable_send,
    reliable_send_and_wait, send_connection_packet, send_with_outbound_metrics, send_writer_packet,
    CatchupMailbox, ClientSession, GameplaySessionState, PoseMailbox, PreAuthSlot, QueuedPacket,
    RequestRateLimiter, Sessions, StateMailbox, StateMailboxKey, TrackedPacket,
    CLIENT_QUEUE_CAPACITY, CLIENT_TIMEOUT, KEEPALIVE_INTERVAL, MAX_CHAT_CHARS,
    PRE_AUTH_CONNECTION_MULTIPLIER, RELIABLE_ENQUEUE_TIMEOUT,
};
use super::transport::Connection;

pub struct NetworkServer<S: HostEventSender = std_mpsc::Sender<ServerToHost>> {
    seed: u64,
    gamemode: u8,
    next_player_id: Arc<AtomicU64>,
    sessions: Sessions,
    server_to_host: S,
    config: ServerConfig,
    metrics: NetworkMetrics,
    pre_auth: Arc<AtomicUsize>,
}

impl<S: HostEventSender> NetworkServer<S> {
    pub fn spawn(
        bind_addr: String,
        seed: u64,
        gamemode: u8,
        host_to_server: tokio::sync::mpsc::Receiver<HostToServer>,
        server_to_host: S,
    ) -> JoinHandle<()> {
        Self::spawn_with_config(
            bind_addr,
            seed,
            gamemode,
            host_to_server,
            server_to_host,
            ServerConfig::default(),
        )
    }

    #[cfg(test)]
    pub(crate) fn spawn_for_test(
        bind_addr: String,
        seed: u64,
        gamemode: u8,
        host_to_server: tokio::sync::mpsc::Receiver<HostToServer>,
        server_to_host: S,
        catchup_queue_capacity: usize,
        catchup_drain_delay: Duration,
    ) -> JoinHandle<()> {
        Self::spawn_with_config(
            bind_addr,
            seed,
            gamemode,
            host_to_server,
            server_to_host,
            ServerConfig {
                catchup_queue_capacity: catchup_queue_capacity.max(1),
                catchup_drain_delay,
                // Stress tests intentionally exercise rosters larger than the
                // production default player cap.
                max_players: 128,
                ..ServerConfig::default()
            },
        )
    }

    pub fn spawn_with_config(
        bind_addr: String,
        seed: u64,
        gamemode: u8,
        host_to_server: tokio::sync::mpsc::Receiver<HostToServer>,
        server_to_host: S,
        config: ServerConfig,
    ) -> JoinHandle<()> {
        Self::spawn_with_config_and_metrics(
            bind_addr,
            seed,
            gamemode,
            host_to_server,
            server_to_host,
            config,
            NetworkMetrics::default(),
        )
    }

    pub(crate) fn spawn_with_config_and_metrics(
        bind_addr: String,
        seed: u64,
        gamemode: u8,
        host_to_server: tokio::sync::mpsc::Receiver<HostToServer>,
        server_to_host: S,
        config: ServerConfig,
        metrics: NetworkMetrics,
    ) -> JoinHandle<()> {
        std::thread::spawn(move || {
            let runtime = match tokio::runtime::Runtime::new() {
                Ok(runtime) => runtime,
                Err(error) => {
                    let _ = server_to_host.send(ServerToHost::Disconnected {
                        reason: format!("failed to create network runtime: {error}"),
                    });
                    return;
                }
            };
            runtime.block_on(async move {
                let listener = match TcpListener::bind(&bind_addr).await {
                    Ok(listener) => {
                        eprintln!("[NetworkServer] Listening on {bind_addr} (Seed: {seed}, Gamemode: {gamemode})");
                        listener
                    }
                    Err(error) => {
                        let reason =
                            format!("failed to bind multiplayer server to {bind_addr}: {error}");
                        eprintln!("[NetworkServer] {reason}");
                        let _ = server_to_host.send(ServerToHost::Disconnected { reason });
                        return;
                    }
                };

                let server = NetworkServer {
                    seed,
                    gamemode,
                    next_player_id: Arc::new(AtomicU64::new(1)),
                    sessions: Arc::new(Mutex::new(HashMap::new())),
                    server_to_host,
                    config,
                    metrics,
                    pre_auth: Arc::new(AtomicUsize::new(0)),
                };
                server.run(listener, host_to_server).await;
            });
        })
    }

    async fn run(
        self,
        listener: TcpListener,
        mut host_to_server: tokio::sync::mpsc::Receiver<HostToServer>,
    ) {
        loop {
            tokio::select! {
                accepted = listener.accept() => {
                    match accepted {
                        Ok((stream, peer_addr)) => {
                            let pre_auth_cap = self
                                .config
                                .max_players
                                .max(1)
                                .saturating_mul(PRE_AUTH_CONNECTION_MULTIPLIER);
                            let Some(pre_auth) = PreAuthSlot::try_acquire(&self.pre_auth, pre_auth_cap)
                            else {
                                eprintln!(
                                    "[NetworkServer] Rejecting {peer_addr}: pre-auth connection cap ({pre_auth_cap})"
                                );
                                drop(stream);
                                continue;
                            };
                            eprintln!("[NetworkServer] Accepted TCP connection from {peer_addr}");
                            let sessions = Arc::clone(&self.sessions);
                            let next_player_id = Arc::clone(&self.next_player_id);
                            let server_to_host = self.server_to_host.clone();
                            let seed = self.seed;
                            let gamemode = self.gamemode;
                            let config = self.config.clone();
                            let metrics = self.metrics.clone();
                            tokio::spawn(async move {
                                run_client(
                                    Connection::new(stream),
                                    seed,
                                    gamemode,
                                    next_player_id,
                                    sessions,
                                    server_to_host,
                                    config,
                                    metrics,
                                    Some(pre_auth),
                                )
                                .await;
                            });
                        }
                        Err(error) => {
                            eprintln!("[NetworkServer] Multiplayer server accept failed: {error}");
                        }
                    }
                }
                cmd = host_to_server.recv() => {
                    match cmd {
                        Some(HostToServer::Stop) => {
                            queue_stats().dequeue(std::mem::size_of::<HostToServer>() as u64);
                            self.metrics.dequeue();
                            return;
                        }
                        Some(command) => {
                            queue_stats().dequeue(std::mem::size_of::<HostToServer>() as u64);
                            self.metrics.dequeue();
                            handle_host_command(&self.sessions, &self.server_to_host, &self.metrics, command).await;
                        }
                        None => return,
                    }
                }
            }
        }
    }

    #[cfg(test)]
    pub(crate) async fn handle_host_command(&self, command: HostToServer) {
        handle_host_command(&self.sessions, &self.server_to_host, &self.metrics, command).await;
    }

    #[cfg(test)]
    pub(crate) async fn remove_client(id: PlayerId, sessions: &Sessions, server_to_host: &S) {
        remove_client(id, sessions, server_to_host).await;
    }

    #[cfg(test)]
    pub(crate) async fn evict_slow_clients(
        sessions: &Sessions,
        server_to_host: &S,
        initial: Vec<PlayerId>,
    ) {
        evict_slow_clients(sessions, server_to_host, initial).await;
    }

    #[cfg(test)]
    pub(crate) async fn run_client(
        connection: Connection,
        seed: u64,
        gamemode: u8,
        next_player_id: Arc<AtomicU64>,
        sessions: Sessions,
        server_to_host: S,
        config: ServerConfig,
        metrics: NetworkMetrics,
        pre_auth: Option<PreAuthSlot>,
    ) {
        run_client(
            connection,
            seed,
            gamemode,
            next_player_id,
            sessions,
            server_to_host,
            config,
            metrics,
            pre_auth,
        )
        .await;
    }
}

impl NetworkServer<std_mpsc::Sender<ServerToHost>> {
    #[cfg(test)]
    pub(crate) async fn broadcast_pose(sessions: &Sessions, packet: Packet) {
        super::egress::broadcast_pose_inner(sessions, packet).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::protocol::{
        Action, GameplayOperation, GameplayRequest, GameplayResponse, LightningStrike, Packet,
        RejectReason, SessionGameplayWire, PROTOCOL_VERSION,
    };
    use std::collections::HashSet;
    use std::net::TcpListener as StdTcpListener;
    use tokio::sync::{mpsc, watch};
    use tokio::time::Instant;

    struct TestServer {
        addr: String,
        host_tx: tokio::sync::mpsc::Sender<HostToServer>,
        event_rx: std_mpsc::Receiver<ServerToHost>,
        handle: JoinHandle<()>,
        metrics: NetworkMetrics,
    }

    impl TestServer {
        fn start(seed: u64, gamemode: u8) -> Self {
            Self::start_with_config(
                seed,
                gamemode,
                ServerConfig {
                    catchup_queue_capacity: MAX_CATCHUP_QUEUE_DEPTH,
                    catchup_drain_delay: Duration::ZERO,
                    // Stress tests intentionally exercise rosters larger than
                    // the production default player cap.
                    max_players: 128,
                    ..ServerConfig::default()
                },
            )
        }

        fn start_with_config(seed: u64, gamemode: u8, config: ServerConfig) -> Self {
            let reserved = StdTcpListener::bind("127.0.0.1:0").unwrap();
            let addr = reserved.local_addr().unwrap().to_string();
            drop(reserved);

            let (host_tx, host_rx) = tokio::sync::mpsc::channel(128);
            let (event_tx, event_rx) = std_mpsc::channel();
            let metrics = NetworkMetrics::default();
            let handle = NetworkServer::spawn_with_config_and_metrics(
                addr.clone(),
                seed,
                gamemode,
                host_rx,
                event_tx,
                config,
                metrics.clone(),
            );
            Self {
                addr,
                host_tx,
                event_rx,
                handle,
                metrics,
            }
        }

        async fn connect_stream(&self) -> tokio::net::TcpStream {
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                match tokio::net::TcpStream::connect(&self.addr).await {
                    Ok(stream) if stream.local_addr().ok() != stream.peer_addr().ok() => {
                        break stream;
                    }
                    Ok(_) if Instant::now() < deadline => {
                        // On Windows, connecting before the server has bound can
                        // transiently self-connect when the reserved server port
                        // is selected as the client's ephemeral port.
                        time::sleep(Duration::from_millis(10)).await;
                    }
                    Err(_) if Instant::now() < deadline => {
                        time::sleep(Duration::from_millis(10)).await;
                    }
                    Ok(_) => panic!("server did not start before the connection deadline"),
                    Err(error) => panic!("server did not start: {error}"),
                }
            }
        }

        async fn connect(&self, username: &str) -> (Connection, PlayerId) {
            let mut connection = Connection::new(self.connect_stream().await);
            connection
                .send(&Packet::Handshake {
                    protocol_version: PROTOCOL_VERSION,
                    username: username.into(),
                })
                .await
                .unwrap();

            match time::timeout(Duration::from_secs(2), connection.recv())
                .await
                .unwrap()
                .unwrap()
            {
                Packet::LoginSuccess {
                    protocol_version,
                    player_id,
                    seed,
                    gamemode,
                } => {
                    assert_eq!(protocol_version, PROTOCOL_VERSION);
                    assert_ne!(player_id, 0);
                    assert_eq!(seed, 0xCAFE_BABE);
                    assert_eq!(gamemode, 1);
                    (connection, player_id)
                }
                packet => panic!("expected login success, got {packet:?}"),
            }
        }

        async fn next_event_matching(
            &self,
            predicate: impl Fn(&ServerToHost) -> bool,
        ) -> ServerToHost {
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                while let Ok(event) = self.event_rx.try_recv() {
                    if predicate(&event) {
                        return event;
                    }
                }
                assert!(
                    Instant::now() < deadline,
                    "timed out waiting for server event"
                );
                time::sleep(Duration::from_millis(10)).await;
            }
        }

        async fn stop(self) -> NetworkMetricsSnapshot {
            let metrics = self.metrics.clone();
            let _ = self.host_tx.send(HostToServer::Stop).await;
            time::timeout(
                Duration::from_secs(2),
                tokio::task::spawn_blocking(move || {
                    self.handle.join().unwrap();
                }),
            )
            .await
            .expect("server thread did not stop")
            .unwrap();
            metrics.snapshot()
        }
    }

    async fn recv_matching(
        connection: &mut Connection,
        predicate: impl Fn(&Packet) -> bool,
    ) -> Packet {
        time::timeout(Duration::from_secs(2), async {
            loop {
                let packet = connection.recv().await.unwrap();
                if predicate(&packet) {
                    return packet;
                }
            }
        })
        .await
        .expect("timed out waiting for packet")
    }

    #[test]
    fn bind_failure_notifies_host_and_thread_exits() {
        let occupied = StdTcpListener::bind("127.0.0.1:0").unwrap();
        let addr = occupied.local_addr().unwrap().to_string();
        let (host_tx, host_rx) = tokio::sync::mpsc::channel(16);
        let (event_tx, event_rx) = std_mpsc::channel();
        let handle = NetworkServer::spawn(addr.clone(), 1, 0, host_rx, event_tx);

        let event = match event_rx.recv_timeout(Duration::from_secs(3)) {
            Ok(event) => event,
            Err(error) => {
                let _ = host_tx.try_send(HostToServer::Stop);
                handle.join().unwrap();
                panic!("server did not report bind failure for {addr}: {error}");
            }
        };
        handle.join().unwrap();
        assert!(matches!(
            event,
            ServerToHost::Disconnected { reason }
                if reason.contains("failed to bind multiplayer server")
        ));
    }

    #[tokio::test]
    async fn connect_and_login() {
        let server = TestServer::start(0xCAFE_BABE, 1);
        let mut connection = Connection::new(server.connect_stream().await);
        connection
            .send(&Packet::Handshake {
                protocol_version: PROTOCOL_VERSION,
                username: "steve".into(),
            })
            .await
            .unwrap();

        let packet = connection.recv().await.unwrap();
        match packet {
            Packet::LoginSuccess {
                protocol_version,
                player_id,
                seed,
                gamemode,
            } => {
                assert_eq!(protocol_version, PROTOCOL_VERSION);
                assert_ne!(player_id, 0);
                assert_eq!(seed, 0xCAFE_BABE);
                assert_eq!(gamemode, 1);
            }
            packet => panic!("expected login success, got {packet:?}"),
        }

        let event = server
            .next_event_matching(|event| matches!(event, ServerToHost::ClientJoined { .. }))
            .await;
        assert!(matches!(
            event,
            ServerToHost::ClientJoined { username, .. } if username == "steve"
        ));
        server.stop().await;
    }

    #[tokio::test]
    async fn gameplay_request_is_bound_to_authenticated_session() {
        let server = TestServer::start(0xCAFE_BABE, 1);
        let (mut client, id) = server.connect("steve").await;
        let request = crate::network::protocol::GameplayRequest {
            request_id: 123,
            client_sequence: 1,
            session_id: 999_999,
            dimension: 0,
            client_revision: 0,
            operation: crate::network::protocol::GameplayOperation::ItemUse { item: 1, count: 1 },
        };
        client
            .send(&Packet::GameplayRequest {
                protocol_version: PROTOCOL_VERSION,
                request,
            })
            .await
            .unwrap();
        let event = server
            .next_event_matching(|event| matches!(event, ServerToHost::GameplayRequest { .. }))
            .await;
        assert!(matches!(
            event,
            ServerToHost::GameplayRequest { id: event_id, request }
                if event_id == id && request.session_id == id
        ));
        server.stop().await;
    }

    #[tokio::test]
    async fn gameplay_requests_are_idempotent_and_rejections_keep_sequences() {
        let server = TestServer::start(0xCAFE_BABE, 1);
        let (mut client, id) = server.connect("sequencer").await;
        let request = GameplayRequest {
            request_id: 700,
            client_sequence: 1,
            session_id: 999_999,
            dimension: 0,
            client_revision: 0,
            operation: GameplayOperation::ItemUse { item: 1, count: 1 },
        };
        client
            .send(&Packet::GameplayRequest {
                protocol_version: PROTOCOL_VERSION,
                request: request.clone(),
            })
            .await
            .unwrap();
        let forwarded = server
            .next_event_matching(|event| matches!(event, ServerToHost::GameplayRequest { .. }))
            .await;
        assert!(matches!(
            forwarded,
            ServerToHost::GameplayRequest { id: event_id, request }
                if event_id == id
                    && request.request_id == 700
                    && request.session_id == id
                    && request.client_sequence == 1
        ));

        let accepted = GameplayResponse {
            request_id: 700,
            server_sequence: 40,
            outcome: crate::network::protocol::GameplayOutcome::Accepted { revision: 5 },
        };
        server
            .host_tx
            .send(HostToServer::SendGameplayResponse {
                to: id,
                response: accepted.clone(),
            })
            .await
            .unwrap();
        let first = recv_matching(&mut client, |packet| {
            matches!(packet, Packet::GameplayResponse { .. })
        })
        .await;
        assert!(matches!(
            first,
            Packet::GameplayResponse { response, .. } if response == accepted
        ));

        // A retransmit is answered from the bounded cache and never forwarded
        // to the authority a second time.
        client
            .send(&Packet::GameplayRequest {
                protocol_version: PROTOCOL_VERSION,
                request: request.clone(),
            })
            .await
            .unwrap();
        let duplicate = recv_matching(&mut client, |packet| {
            matches!(packet, Packet::GameplayResponse { .. })
        })
        .await;
        assert!(matches!(
            duplicate,
            Packet::GameplayResponse { response, .. } if response == accepted
        ));
        assert!(
            time::timeout(Duration::from_millis(100), async {
                loop {
                    if matches!(
                        server.event_rx.try_recv(),
                        Ok(ServerToHost::GameplayRequest { .. })
                    ) {
                        return true;
                    }
                    time::sleep(Duration::from_millis(5)).await;
                }
            })
            .await
            .is_err(),
            "duplicate request was forwarded"
        );

        client
            .send(&Packet::GameplayRequest {
                protocol_version: PROTOCOL_VERSION,
                request: GameplayRequest {
                    request_id: 701,
                    client_sequence: 1,
                    ..request.clone()
                },
            })
            .await
            .unwrap();
        let out_of_order = recv_matching(&mut client, |packet| {
            matches!(packet, Packet::GameplayResponse { .. })
        })
        .await;
        assert!(matches!(
            out_of_order,
            Packet::GameplayResponse { response, .. }
                if response.server_sequence > accepted.server_sequence
                    && matches!(
                        response.outcome,
                        crate::network::protocol::GameplayOutcome::Rejected {
                            reason: RejectReason::OutOfOrder
                        }
                    )
        ));

        client
            .send(&Packet::GameplayRequest {
                protocol_version: PROTOCOL_VERSION,
                request: GameplayRequest {
                    request_id: 702,
                    client_sequence: 2,
                    client_revision: 4,
                    ..request.clone()
                },
            })
            .await
            .unwrap();
        let stale = recv_matching(&mut client, |packet| {
            matches!(packet, Packet::GameplayResponse { .. })
        })
        .await;
        assert!(matches!(
            stale,
            Packet::GameplayResponse { response, .. }
                if response.server_sequence > accepted.server_sequence
                    && matches!(
                        response.outcome,
                        crate::network::protocol::GameplayOutcome::Rejected {
                            reason: RejectReason::InvalidRevision
                        }
                    )
        ));

        client
            .send(&Packet::GameplayRequest {
                protocol_version: PROTOCOL_VERSION,
                request: GameplayRequest {
                    request_id: 703,
                    client_sequence: 3,
                    client_revision: 5,
                    operation: GameplayOperation::Command {
                        command: "x".repeat(crate::network::protocol::MAX_COMMAND_BYTES + 1),
                    },
                    ..request
                },
            })
            .await
            .unwrap();
        let bounds = recv_matching(&mut client, |packet| {
            matches!(packet, Packet::GameplayResponse { .. })
        })
        .await;
        assert!(matches!(
            bounds,
            Packet::GameplayResponse { response, .. }
                if response.server_sequence > accepted.server_sequence
                    && matches!(
                        response.outcome,
                        crate::network::protocol::GameplayOutcome::Rejected {
                            reason: RejectReason::StringTooLong
                        }
                    )
        ));
        let metrics = server.metrics.snapshot();
        assert_eq!(metrics.duplicate_requests, 1);
        assert_eq!(metrics.rejected_requests, 3);
        server.stop().await;
    }

    #[tokio::test]
    async fn request_rate_limit_rejects_without_forwarding_the_second_request() {
        let server = TestServer::start_with_config(
            0xCAFE_BABE,
            1,
            ServerConfig {
                request_rate_per_second: 1,
                ..ServerConfig::default()
            },
        );
        let (mut client, id) = server.connect("rate-limited").await;
        let request = GameplayRequest {
            request_id: 900,
            client_sequence: 1,
            session_id: id,
            dimension: 0,
            client_revision: 0,
            operation: GameplayOperation::ItemUse { item: 1, count: 1 },
        };
        client
            .send(&Packet::GameplayRequest {
                protocol_version: PROTOCOL_VERSION,
                request: request.clone(),
            })
            .await
            .unwrap();
        let first = server
            .next_event_matching(|event| matches!(event, ServerToHost::GameplayRequest { .. }))
            .await;
        assert!(
            matches!(first, ServerToHost::GameplayRequest { request, .. } if request.request_id == 900)
        );

        client
            .send(&Packet::GameplayRequest {
                protocol_version: PROTOCOL_VERSION,
                request: GameplayRequest {
                    request_id: 901,
                    client_sequence: 2,
                    ..request
                },
            })
            .await
            .unwrap();
        let rejected = recv_matching(&mut client, |packet| {
            matches!(packet, Packet::GameplayResponse { .. })
        })
        .await;
        assert!(matches!(
            rejected,
            Packet::GameplayResponse { response, .. }
                if response.request_id == 901
                    && matches!(
                        response.outcome,
                        crate::network::protocol::GameplayOutcome::Rejected {
                            reason: RejectReason::RateLimited
                        }
                    )
        ));
        assert_eq!(server.metrics.snapshot().rejected_requests, 1);
        server.stop().await;
    }

    #[tokio::test]
    async fn concurrent_logins_reserve_max_player_slot_atomically() {
        let server = TestServer::start_with_config(
            0xCAFE_BABE,
            1,
            ServerConfig {
                max_players: 1,
                ..ServerConfig::default()
            },
        );
        let mut first = Connection::new(server.connect_stream().await);
        let mut second = Connection::new(server.connect_stream().await);
        let first_handshake = Packet::Handshake {
            protocol_version: PROTOCOL_VERSION,
            username: "slot-a".into(),
        };
        let second_handshake = Packet::Handshake {
            protocol_version: PROTOCOL_VERSION,
            username: "slot-b".into(),
        };
        let (first_sent, second_sent) =
            tokio::join!(first.send(&first_handshake), second.send(&second_handshake));
        first_sent.unwrap();
        second_sent.unwrap();
        let (first_reply, second_reply) = tokio::join!(first.recv(), second.recv());
        let replies = [first_reply.unwrap(), second_reply.unwrap()];
        assert_eq!(
            replies
                .iter()
                .filter(|packet| matches!(packet, Packet::LoginSuccess { .. }))
                .count(),
            1
        );
        assert_eq!(
            replies
                .iter()
                .filter(|packet| matches!(packet, Packet::Disconnect { reason, .. } if reason == "server is full"))
                .count(),
            1
        );
        let shutdown_metrics = server.stop().await;
        assert_eq!(shutdown_metrics.queue_depth, 0);
    }

    #[tokio::test]
    async fn concurrent_case_insensitive_duplicate_login_is_atomic() {
        let server = TestServer::start_with_config(
            0xCAFE_BABE,
            1,
            ServerConfig {
                max_players: 2,
                ..ServerConfig::default()
            },
        );
        let mut first = Connection::new(server.connect_stream().await);
        let mut second = Connection::new(server.connect_stream().await);
        let first_handshake = Packet::Handshake {
            protocol_version: PROTOCOL_VERSION,
            username: "Alex".into(),
        };
        let second_handshake = Packet::Handshake {
            protocol_version: PROTOCOL_VERSION,
            username: "aLEX".into(),
        };
        let (first_sent, second_sent) =
            tokio::join!(first.send(&first_handshake), second.send(&second_handshake));
        first_sent.unwrap();
        second_sent.unwrap();
        let (first_reply, second_reply) = tokio::join!(first.recv(), second.recv());
        let replies = [first_reply.unwrap(), second_reply.unwrap()];
        assert_eq!(
            replies
                .iter()
                .filter(|packet| matches!(packet, Packet::LoginSuccess { .. }))
                .count(),
            1
        );
        assert_eq!(
            replies
                .iter()
                .filter(|packet| matches!(packet, Packet::Disconnect { reason, .. } if reason == "duplicate login"))
                .count(),
            1
        );
        let shutdown_metrics = server.stop().await;
        assert_eq!(shutdown_metrics.queue_depth, 0);
    }

    #[tokio::test]
    async fn legacy_sleep_and_container_fields_are_preserved_in_envelopes() {
        let server = TestServer::start(0xCAFE_BABE, 1);
        let (mut client, id) = server.connect("legacy-adapter").await;

        client
            .send(&Packet::SleepRequest {
                protocol_version: PROTOCOL_VERSION,
                x: -3,
                y: 70,
                z: 11,
            })
            .await
            .unwrap();
        let sleep = server
            .next_event_matching(|event| matches!(event, ServerToHost::GameplayRequest { .. }))
            .await;
        assert!(matches!(
            sleep,
            ServerToHost::GameplayRequest { id: event_id, request }
                if event_id == id
                    && request.client_sequence == 1
                    && matches!(
                        request.operation,
                        GameplayOperation::Sleep { x: -3, y: 70, z: 11 }
                    )
        ));

        client
            .send(&Packet::ContainerOpenRequest {
                protocol_version: PROTOCOL_VERSION,
                dimension: 1,
                x: 12,
                y: 65,
                z: -8,
            })
            .await
            .unwrap();
        let open = server
            .next_event_matching(|event| matches!(event, ServerToHost::GameplayRequest { .. }))
            .await;
        assert!(matches!(
            open,
            ServerToHost::GameplayRequest { id: event_id, request }
                if event_id == id
                    && request.dimension == 1
                    && request.client_sequence == 2
                    && matches!(
                        request.operation,
                        GameplayOperation::Container {
                            action: 0,
                            x: 12,
                            y: 65,
                            z: -8,
                            slot: 0,
                        }
                    )
        ));

        client
            .send(&Packet::ContainerClickRequest {
                protocol_version: PROTOCOL_VERSION,
                dimension: 1,
                revision: 17,
                slot_index: 4,
                is_left: true,
                dragged: None,
            })
            .await
            .unwrap();
        let click = server
            .next_event_matching(|event| matches!(event, ServerToHost::GameplayRequest { .. }))
            .await;
        assert!(matches!(
            click,
            ServerToHost::GameplayRequest { id: event_id, request }
                if event_id == id
                    && request.dimension == 1
                    && request.client_revision == 17
                    && request.client_sequence == 3
                    && matches!(
                        request.operation,
                        GameplayOperation::Container {
                            action: 1,
                            x: 12,
                            y: 65,
                            z: -8,
                            slot: 4,
                        }
                    )
        ));

        client
            .send(&Packet::ContainerClose {
                protocol_version: PROTOCOL_VERSION,
                dimension: 1,
                x: 12,
                y: 65,
                z: -8,
            })
            .await
            .unwrap();
        let close = server
            .next_event_matching(|event| matches!(event, ServerToHost::GameplayRequest { .. }))
            .await;
        assert!(matches!(
            close,
            ServerToHost::GameplayRequest { id: event_id, request }
                if event_id == id
                    && request.client_sequence == 4
                    && matches!(
                        request.operation,
                        GameplayOperation::Container {
                            action: 2,
                            x: 12,
                            y: 65,
                            z: -8,
                            slot: 0,
                        }
                    )
        ));
        server.stop().await;
    }

    #[tokio::test]
    async fn server_list_ping_reports_version_and_capacity_without_login() {
        let server = TestServer::start(0xCAFE_BABE, 1);
        let mut client = Connection::new(server.connect_stream().await);
        client
            .send(&Packet::ServerListPingRequest {
                protocol_version: PROTOCOL_VERSION,
            })
            .await
            .unwrap();
        let response = time::timeout(Duration::from_secs(2), client.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(
            response,
            Packet::ServerListPingResponse {
                protocol_version,
                online_players: 0,
                max_players,
                ..
            } if protocol_version == PROTOCOL_VERSION && max_players > 0
        ));
        server.stop().await;
    }

    #[tokio::test]
    async fn block_change_reports_authenticated_session_id_to_host() {
        let server = TestServer::start(0xCAFE_BABE, 1);
        let (mut client, id) = server.connect("steve").await;

        client
            .send(&Packet::BlockChange {
                protocol_version: PROTOCOL_VERSION,
                dimension: 0,
                revision: 0,
                x: 3,
                y: 80,
                z: -4,
                block: 3,
                state: 0,
                raw_fluid: 0,
            })
            .await
            .unwrap();

        let event = server
            .next_event_matching(|event| matches!(event, ServerToHost::GameplayRequest { .. }))
            .await;
        assert!(matches!(
            event,
            ServerToHost::GameplayRequest { id: event_id, request }
                if event_id == id
                    && request.session_id == id
                    && request.request_id != 0
                    && request.client_sequence == 1
                    && request.client_revision == 0
                    && matches!(
                        request.operation,
                        crate::network::protocol::GameplayOperation::BlockUse {
                            x: 3,
                            y: 80,
                            z: -4,
                            block: 3,
                        }
                    )
        ));

        server.stop().await;
    }

    #[tokio::test]
    async fn rejects_old_protocol_during_handshake() {
        let server = TestServer::start(0xCAFE_BABE, 1);
        let mut connection = Connection::new(server.connect_stream().await);
        connection
            .send(&Packet::Handshake {
                protocol_version: PROTOCOL_VERSION - 1,
                username: "outdated-client".into(),
            })
            .await
            .unwrap();

        let packet = time::timeout(Duration::from_secs(2), connection.recv())
            .await
            .expect("server did not reject outdated protocol")
            .expect("server closed without a disconnect packet");
        assert!(matches!(
            packet,
            Packet::Disconnect {
                protocol_version,
                reason,
            } if protocol_version == PROTOCOL_VERSION
                && reason.contains("protocol version mismatch")
        ));

        server.stop().await;
    }

    #[tokio::test]
    async fn relays_player_position_through_host() {
        let server = TestServer::start(0xCAFE_BABE, 1);
        let (mut client_a, id_a) = server.connect("steve").await;
        let (mut client_b, _) = server.connect("alex").await;

        client_a
            .send(&Packet::PlayerPosition {
                protocol_version: PROTOCOL_VERSION,
                id: 999,
                sequence: 12,
                sender_time_millis: 600,
                x: 10.0,
                y: 65.0,
                z: -4.0,
                yaw: 1.5,
                pitch: -0.25,
            })
            .await
            .unwrap();
        let event = server
            .next_event_matching(|event| matches!(event, ServerToHost::ClientPosition { .. }))
            .await;
        assert!(matches!(
            event,
            ServerToHost::ClientPosition {
                id,
                sequence,
                sender_time_millis,
                x,
                y,
                z,
                yaw,
                pitch,
            }
                if id == id_a
                    && sequence == 12
                    && sender_time_millis == 600
                    && x == 10.0
                    && y == 65.0
                    && z == -4.0
                    && yaw == 1.5
                    && pitch == -0.25
        ));

        server
            .host_tx
            .send(HostToServer::PlayerPosition {
                to: None,
                id: id_a,
                sequence: 12,
                sender_time_millis: 600,
                x: 10.0,
                y: 65.0,
                z: -4.0,
                yaw: 1.5,
                pitch: -0.25,
            })
            .await
            .unwrap();
        let packet = recv_matching(&mut client_b, |packet| {
            matches!(packet, Packet::PlayerPosition { .. })
        })
        .await;
        assert!(matches!(
            packet,
            Packet::PlayerPosition {
                id,
                sequence,
                sender_time_millis,
                x,
                y,
                z,
                yaw,
                pitch,
                ..
            }
                if id == id_a
                    && sequence == 12
                    && sender_time_millis == 600
                    && x == 10.0
                    && y == 65.0
                    && z == -4.0
                    && yaw == 1.5
                    && pitch == -0.25
        ));

        server.stop().await;
    }

    #[tokio::test]
    async fn unsent_pose_updates_are_latest_wins_per_player() {
        let sessions: Sessions = Arc::new(Mutex::new(HashMap::new()));
        let (out_tx, _out_rx) = mpsc::channel(1);
        let metrics = NetworkMetrics::default();
        let pose_mailbox = Arc::new(PoseMailbox::with_metrics(metrics.clone()));
        sessions.lock().await.insert(
            1,
            ClientSession {
                id: 1,
                username: "alex".into(),
                out_tx,
                pose_mailbox: Arc::clone(&pose_mailbox),
                state_mailbox: Arc::new(StateMailbox::default()),
                catchup_mailbox: Arc::new(CatchupMailbox::default()),
                cancel_tx: watch::channel(false).0,
                gameplay: GameplaySessionState::default(),
                metrics,
            },
        );

        for (player_id, sequence) in [(9, 4), (12, 8), (9, 5)] {
            NetworkServer::broadcast_pose(
                &sessions,
                Packet::PlayerPosition {
                    protocol_version: PROTOCOL_VERSION,
                    id: player_id,
                    sequence,
                    sender_time_millis: u64::from(sequence) * 50,
                    x: sequence as f32,
                    y: 64.0,
                    z: 0.0,
                    yaw: 0.0,
                    pitch: 0.0,
                },
            )
            .await;
        }

        let pending = pose_mailbox.drain().await;
        assert_eq!(pending.len(), 2);
        assert!(matches!(
            pending[0],
            Packet::PlayerPosition {
                id: 9,
                sequence: 5,
                sender_time_millis: 250,
                x: 5.0,
                ..
            }
        ));
        assert!(matches!(
            pending[1],
            Packet::PlayerPosition {
                id: 12,
                sequence: 8,
                sender_time_millis: 400,
                x: 8.0,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn reliable_join_and_leave_wait_for_bounded_queue_capacity() {
        let sessions: Sessions = Arc::new(Mutex::new(HashMap::new()));
        let (observer_out_tx, mut observer_out_rx) = mpsc::channel(1);
        let metrics = NetworkMetrics::default();
        observer_out_tx
            .try_send(QueuedPacket::Outbound(TrackedPacket::new(
                Packet::Keepalive {
                    protocol_version: PROTOCOL_VERSION,
                },
                &metrics,
            )))
            .unwrap();
        sessions.lock().await.insert(
            1,
            ClientSession {
                id: 1,
                username: "observer".into(),
                out_tx: observer_out_tx,
                pose_mailbox: Arc::new(PoseMailbox::default()),
                state_mailbox: Arc::new(StateMailbox::default()),
                catchup_mailbox: Arc::new(CatchupMailbox::default()),
                cancel_tx: watch::channel(false).0,
                gameplay: GameplaySessionState::default(),
                metrics: metrics.clone(),
            },
        );

        let (departing_out_tx, _departing_out_rx) = mpsc::channel(1);
        sessions.lock().await.insert(
            2,
            ClientSession {
                id: 2,
                username: "departing".into(),
                out_tx: departing_out_tx,
                pose_mailbox: Arc::new(PoseMailbox::default()),
                state_mailbox: Arc::new(StateMailbox::default()),
                catchup_mailbox: Arc::new(CatchupMailbox::default()),
                cancel_tx: watch::channel(false).0,
                gameplay: GameplaySessionState::default(),
                metrics: metrics.clone(),
            },
        );

        let (event_tx, _event_rx) = std_mpsc::channel();
        let server = NetworkServer {
            seed: 0,
            gamemode: 1,
            next_player_id: Arc::new(AtomicU64::new(3)),
            sessions: Arc::clone(&sessions),
            server_to_host: event_tx.clone(),
            config: ServerConfig::default(),
            metrics: metrics.clone(),
            pre_auth: Arc::new(AtomicUsize::new(0)),
        };

        let observer = tokio::spawn(async move {
            time::sleep(Duration::from_millis(25)).await;
            let unwrap_packet = |queued| match queued {
                QueuedPacket::Reliable(packet) | QueuedPacket::Outbound(packet) => {
                    packet.into_packet()
                }
                QueuedPacket::ReliableWithAck(packet, completion) => {
                    let _ = completion.send(true);
                    packet.into_packet()
                }
            };
            let queued = unwrap_packet(observer_out_rx.recv().await.unwrap());
            let joined = unwrap_packet(observer_out_rx.recv().await.unwrap());
            let left = unwrap_packet(observer_out_rx.recv().await.unwrap());
            (queued, joined, left)
        });

        time::timeout(
            Duration::from_secs(1),
            server.handle_host_command(HostToServer::NotifyPlayerJoin {
                id: 3,
                username: "joining".into(),
            }),
        )
        .await
        .expect("reliable join should be delivered when bounded capacity becomes available");

        time::timeout(
            Duration::from_secs(1),
            NetworkServer::remove_client(2, &sessions, &event_tx),
        )
        .await
        .expect("reliable leave should be delivered when bounded capacity becomes available");

        let (queued, joined, left) = observer.await.unwrap();
        assert!(matches!(queued, Packet::Keepalive { .. }));
        assert!(matches!(
            joined,
            Packet::PlayerJoin {
                id: 3,
                username,
                ..
            } if username == "joining"
        ));
        assert!(matches!(left, Packet::PlayerLeave { id: 2, .. }));
    }

    #[tokio::test]
    async fn full_reliable_queue_evicts_slow_client_without_ghost_session() {
        let sessions: Sessions = Arc::new(Mutex::new(HashMap::new()));
        let (out_tx, _out_rx) = mpsc::channel(1);
        let metrics = NetworkMetrics::default();
        out_tx
            .try_send(QueuedPacket::Outbound(TrackedPacket::new(
                Packet::Keepalive {
                    protocol_version: PROTOCOL_VERSION,
                },
                &metrics,
            )))
            .unwrap();
        let (cancel_tx, mut cancel_rx) = watch::channel(false);
        sessions.lock().await.insert(
            1,
            ClientSession {
                id: 1,
                username: "slow-client".into(),
                out_tx,
                pose_mailbox: Arc::new(PoseMailbox::default()),
                state_mailbox: Arc::new(StateMailbox::default()),
                catchup_mailbox: Arc::new(CatchupMailbox::default()),
                cancel_tx,
                gameplay: GameplaySessionState::default(),
                metrics: metrics.clone(),
            },
        );

        let (event_tx, event_rx) = std_mpsc::channel();
        let server = NetworkServer {
            seed: 0,
            gamemode: 1,
            next_player_id: Arc::new(AtomicU64::new(2)),
            sessions: Arc::clone(&sessions),
            server_to_host: event_tx,
            config: ServerConfig::default(),
            metrics: metrics.clone(),
            pre_auth: Arc::new(AtomicUsize::new(0)),
        };

        time::timeout(
            Duration::from_secs(1),
            server.handle_host_command(HostToServer::NotifyPlayerJoin {
                id: 2,
                username: "joining".into(),
            }),
        )
        .await
        .expect("a permanently full reliable queue should be evicted deterministically");

        assert!(!sessions.lock().await.contains_key(&1));
        time::timeout(Duration::from_millis(100), cancel_rx.changed())
            .await
            .expect("eviction did not signal session cancellation")
            .expect("session cancellation sender disappeared before signaling");
        assert!(*cancel_rx.borrow());
        assert!(matches!(
            event_rx.recv_timeout(Duration::from_millis(100)),
            Ok(ServerToHost::ClientLeft { id: 1 })
        ));
        assert!(metrics.snapshot().queue_full >= 1);
        drop(_out_rx);
        assert_eq!(metrics.snapshot().queue_depth, 0);
    }

    #[tokio::test]
    async fn evicted_client_task_exits_and_cannot_forward_gameplay() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let sessions: Sessions = Arc::new(Mutex::new(HashMap::new()));
        let task_sessions = Arc::clone(&sessions);
        let next_player_id = Arc::new(AtomicU64::new(1));
        let (event_tx, event_rx) = std_mpsc::channel();
        let task_event_tx = event_tx.clone();
        let client_task = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            NetworkServer::run_client(
                Connection::new(stream),
                0xCAFE_BABE,
                1,
                next_player_id,
                task_sessions,
                task_event_tx,
                ServerConfig::default(),
                NetworkMetrics::default(),
                None,
            )
            .await;
        });

        let mut client = Connection::new(tokio::net::TcpStream::connect(addr).await.unwrap());
        client
            .send(&Packet::Handshake {
                protocol_version: PROTOCOL_VERSION,
                username: "evicted".into(),
            })
            .await
            .unwrap();
        let id = match client.recv().await.unwrap() {
            Packet::LoginSuccess { player_id, .. } => player_id,
            packet => panic!("expected login success, got {packet:?}"),
        };
        time::timeout(Duration::from_secs(1), async {
            loop {
                if matches!(
                    event_rx.try_recv(),
                    Ok(ServerToHost::ClientJoined {
                        id: joined_id,
                        ..
                    }) if joined_id == id
                ) {
                    break;
                }
                time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("client did not finish joining");

        NetworkServer::evict_slow_clients(&sessions, &event_tx, vec![id]).await;
        time::timeout(Duration::from_millis(250), client_task)
            .await
            .expect("evicted client task did not observe cancellation")
            .unwrap();

        let _ = time::timeout(
            Duration::from_millis(250),
            client.send(&Packet::BlockChange {
                protocol_version: PROTOCOL_VERSION,
                dimension: 0,
                revision: 0,
                x: 7,
                y: 80,
                z: -3,
                block: 4,
                state: 0,
                raw_fluid: 0,
            }),
        )
        .await;
        time::sleep(Duration::from_millis(25)).await;
        assert!(
            event_rx
                .try_iter()
                .all(|event| !matches!(event, ServerToHost::GameplayRequest { id: event_id, .. } if event_id == id)),
            "evicted client forwarded a gameplay packet after cancellation"
        );
    }

    #[tokio::test]
    async fn newcomer_receives_roster_larger_than_queue_capacity() {
        let server = TestServer::start(0xCAFE_BABE, 1);
        let player_count = CLIENT_QUEUE_CAPACITY + 1;
        let mut existing_clients = Vec::with_capacity(player_count);
        let mut expected_ids = HashSet::with_capacity(player_count);
        for index in 0..player_count {
            let (connection, id) = server.connect(&format!("player-{index}")).await;
            existing_clients.push(connection);
            expected_ids.insert(id);
        }

        let (mut newcomer, newcomer_id) = server.connect("newcomer").await;
        let received_ids = time::timeout(Duration::from_secs(5), async {
            let mut received_ids = HashSet::with_capacity(player_count);
            while received_ids.len() < player_count {
                if let Packet::PlayerJoin { id, .. } = newcomer.recv().await.unwrap() {
                    received_ids.insert(id);
                }
            }
            received_ids
        })
        .await
        .expect("newcomer did not receive the complete roster");

        assert_eq!(received_ids, expected_ids);
        assert!(!received_ids.contains(&newcomer_id));
        drop(existing_clients);
        server.stop().await;
    }

    #[tokio::test]
    async fn weather_snapshot_can_target_only_the_joining_client() {
        let server = TestServer::start(0xCAFE_BABE, 1);
        let (mut existing, _) = server.connect("existing").await;
        let (mut joining, joining_id) = server.connect("joining").await;

        server
            .host_tx
            .send(HostToServer::TimeSync {
                ticks: 21_000,
                weather: 2,
                weather_remaining_ticks: 3_500.25,
                to: Some(joining_id),
            })
            .await
            .unwrap();

        let packet = recv_matching(&mut joining, |packet| {
            matches!(packet, Packet::TimeSync { .. })
        })
        .await;
        assert!(matches!(
            packet,
            Packet::TimeSync {
                ticks: 21_000,
                weather: 2,
                weather_remaining_ticks: 3_500.25,
                ..
            }
        ));

        let existing_received_snapshot = time::timeout(Duration::from_millis(150), async {
            loop {
                if matches!(existing.recv().await.unwrap(), Packet::TimeSync { .. }) {
                    break;
                }
            }
        })
        .await;
        assert!(
            existing_received_snapshot.is_err(),
            "targeted late-join weather snapshot leaked to an existing client"
        );

        server.stop().await;
    }

    #[tokio::test]
    async fn weather_snapshot_and_lightning_broadcast_in_reliable_order() {
        let server = TestServer::start(0xCAFE_BABE, 1);
        let (mut client, _) = server.connect("observer").await;
        let strike = LightningStrike {
            x: -8,
            y: 77,
            z: 19,
            visual_seed: 0x1234_ABCD,
        };

        server
            .host_tx
            .send(HostToServer::TimeSync {
                to: None,
                ticks: 22_000,
                weather: 2,
                weather_remaining_ticks: 4_500.0,
            })
            .await
            .unwrap();
        server
            .host_tx
            .send(HostToServer::BroadcastLightningStrike { strike })
            .await
            .unwrap();

        let weather_packets = time::timeout(Duration::from_secs(2), async {
            let mut packets = Vec::new();
            while packets.len() < 2 {
                let packet = client.recv().await.unwrap();
                if matches!(
                    packet,
                    Packet::TimeSync { .. } | Packet::LightningStrike { .. }
                ) {
                    packets.push(packet);
                }
            }
            packets
        })
        .await
        .expect("weather packets were not delivered");

        assert!(matches!(
            weather_packets[0],
            Packet::TimeSync {
                ticks: 22_000,
                weather: 2,
                weather_remaining_ticks: 4_500.0,
                ..
            }
        ));
        assert!(matches!(
            weather_packets[1],
            Packet::LightningStrike {
                strike: received,
                ..
            } if received == strike
        ));

        server.stop().await;
    }

    #[tokio::test]
    async fn client_cannot_inject_authoritative_lightning() {
        let server = TestServer::start(0xCAFE_BABE, 1);
        let (mut attacker, _) = server.connect("attacker").await;
        let (mut observer, _) = server.connect("observer").await;

        attacker
            .send(&Packet::LightningStrike {
                protocol_version: PROTOCOL_VERSION,
                strike: LightningStrike {
                    x: 0,
                    y: 255,
                    z: 0,
                    visual_seed: 1,
                },
            })
            .await
            .unwrap();

        let forged_strike_relayed = time::timeout(Duration::from_millis(150), async {
            loop {
                if matches!(
                    observer.recv().await.unwrap(),
                    Packet::LightningStrike { .. }
                ) {
                    break;
                }
            }
        })
        .await;
        assert!(
            forged_strike_relayed.is_err(),
            "server relayed a client-authored lightning event"
        );

        server.stop().await;
    }

    #[tokio::test]
    async fn relays_player_action_through_host() {
        let server = TestServer::start(0xCAFE_BABE, 1);
        let (_client_a, id_a) = server.connect("steve").await;
        let (mut client_b, _) = server.connect("alex").await;
        server
            .host_tx
            .send(HostToServer::BroadcastPlayerAction {
                id: id_a,
                action: Action::Break,
            })
            .await
            .unwrap();
        let packet =
            recv_matching(&mut client_b, |p| matches!(p, Packet::PlayerAction { .. })).await;
        assert!(
            matches!(packet, Packet::PlayerAction { id, action: Action::Break, .. } if id == id_a)
        );
        server.stop().await;
    }

    #[tokio::test]
    async fn relays_chat_through_host_with_canonical_sender() {
        let server = TestServer::start(0xCAFE_BABE, 1);
        let (mut client_a, id_a) = server.connect("steve").await;
        let (mut client_b, _) = server.connect("alex").await;

        client_a
            .send(&Packet::ChatMessage {
                protocol_version: PROTOCOL_VERSION,
                sender: "spoofed".into(),
                message: "hello".into(),
            })
            .await
            .unwrap();

        let event = server
            .next_event_matching(|event| matches!(event, ServerToHost::ChatFromClient { .. }))
            .await;
        assert!(matches!(
            event,
            ServerToHost::ChatFromClient { id, message }
                if id == id_a && message == "hello"
        ));

        server
            .host_tx
            .send(HostToServer::BroadcastChat {
                sender: "steve".into(),
                message: "hello".into(),
            })
            .await
            .unwrap();

        for client in [&mut client_a, &mut client_b] {
            let packet = recv_matching(client, |packet| {
                matches!(packet, Packet::ChatMessage { .. })
            })
            .await;
            assert!(matches!(
                packet,
                Packet::ChatMessage { sender, message, .. }
                    if sender == "steve" && message == "hello"
            ));
        }

        server.stop().await;
    }

    #[tokio::test]
    async fn newcomer_receives_existing_roster() {
        let server = TestServer::start(0xCAFE_BABE, 1);
        let (_client_a, id_a) = server.connect("steve").await;
        let (mut client_b, _) = server.connect("alex").await;
        let packet = recv_matching(
            &mut client_b,
            |p| matches!(p, Packet::PlayerJoin { id, .. } if *id == id_a),
        )
        .await;
        assert!(
            matches!(packet, Packet::PlayerJoin { id, username, .. } if id == id_a && username == "steve")
        );
        server.stop().await;
    }

    #[tokio::test]
    async fn disconnect_cleans_up_and_notifies_remaining_clients() {
        let server = TestServer::start(0xCAFE_BABE, 1);
        let (client_a, id_a) = server.connect("steve").await;
        let (mut client_b, _) = server.connect("alex").await;
        drop(client_a);

        let left = server
            .next_event_matching(
                |event| matches!(event, ServerToHost::ClientLeft { id } if *id == id_a),
            )
            .await;
        assert!(matches!(left, ServerToHost::ClientLeft { id } if id == id_a));

        let packet = recv_matching(
            &mut client_b,
            |packet| matches!(packet, Packet::PlayerLeave { id, .. } if *id == id_a),
        )
        .await;
        assert!(matches!(packet, Packet::PlayerLeave { id, .. } if id == id_a));

        server.stop().await;
    }

    #[tokio::test]
    async fn relays_block_action_request_and_targeted_result() {
        let server = TestServer::start(0xCAFE_BABE, 1);
        let (mut client_a, id_a) = server.connect("steve").await;
        let (mut client_b, _id_b) = server.connect("alex").await;

        let held = crate::network::protocol::ItemWire::from_stack(
            &crate::inventory::ItemStack::new(crate::inventory::Item::StonePickaxe, 1),
        );
        client_a
            .send(&Packet::BlockActionRequest {
                protocol_version: PROTOCOL_VERSION,
                action: Action::Break,
                x: 10,
                y: 64,
                z: 20,
                block: crate::inventory::Item::Air as u32,
                held_item: Some(held),
            })
            .await
            .unwrap();

        let event = server
            .next_event_matching(|e| matches!(e, ServerToHost::GameplayRequest { .. }))
            .await;
        assert!(matches!(
            event,
            ServerToHost::GameplayRequest { id, request } if id == id_a
                && request.session_id == id_a
                && request.client_sequence == 1
                    && matches!(
                        request.operation,
                        crate::network::protocol::GameplayOperation::BlockAction {
                            action: crate::network::protocol::BlockActionKind::StartBreak,
                            x: 10,
                            y: 64,
                            z: 20,
                            block: 0,
                            ..
                        }
                    )
        ));

        // Host sends targeted result to client_a
        let drop = crate::network::protocol::ItemWire::from_stack(
            &crate::inventory::ItemStack::new(crate::inventory::Item::Cobblestone, 1),
        );
        server
            .host_tx
            .send(HostToServer::SendBlockActionResult {
                to: id_a,
                x: 10,
                y: 64,
                z: 20,
                success: true,
                consumed_item: false,
                drops: vec![drop],
            })
            .await
            .unwrap();

        // client_a receives result
        let res_a = recv_matching(&mut client_a, |p| {
            matches!(p, Packet::BlockActionResult { .. })
        })
        .await;
        assert!(matches!(
            res_a,
            Packet::BlockActionResult {
                x: 10,
                y: 64,
                z: 20,
                success: true,
                ..
            }
        ));

        // client_b should NOT receive targeted result (wait short time with recv timeout)
        let res_b = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            recv_matching(&mut client_b, |p| {
                matches!(p, Packet::BlockActionResult { .. })
            }),
        )
        .await;
        assert!(res_b.is_err());

        server.stop().await;
    }

    #[tokio::test]
    async fn player_session_projection_is_private_and_rejects_mismatched_owner() {
        let server = TestServer::start(0xCAFE_BABE, 1);
        let (mut client_a, id_a) = server.connect("steve").await;
        let (mut client_b, id_b) = server.connect("alex").await;
        let state = SessionGameplayWire {
            health_milli: 5_000,
            is_dead: true,
            death_source: Some(3),
            experience: 44,
            revision: 1,
            ..SessionGameplayWire::default()
        };
        server
            .host_tx
            .send(HostToServer::SendPlayerSessionUpdate {
                to: id_a,
                sequence: 7,
                player_id: id_a,
                dimension: 0,
                state,
            })
            .await
            .unwrap();

        let received = recv_matching(&mut client_a, |packet| {
            matches!(packet, Packet::PlayerSessionUpdate { .. })
        })
        .await;
        assert!(matches!(
            received,
            Packet::PlayerSessionUpdate {
                player_id,
                state: received_state,
                ..
            } if player_id == id_a && received_state == state
        ));
        assert!(tokio::time::timeout(
            Duration::from_millis(100),
            recv_matching(&mut client_b, |packet| matches!(
                packet,
                Packet::PlayerSessionUpdate { .. }
            ))
        )
        .await
        .is_err());

        server
            .host_tx
            .send(HostToServer::SendPlayerSessionUpdate {
                to: id_a,
                sequence: 8,
                player_id: id_b,
                dimension: 0,
                state: SessionGameplayWire {
                    revision: 2,
                    ..state
                },
            })
            .await
            .unwrap();
        assert!(tokio::time::timeout(
            Duration::from_millis(100),
            recv_matching(&mut client_a, |packet| matches!(packet, Packet::PlayerSessionUpdate { state, .. } if state.revision == 2))
        )
        .await
        .is_err());

        server.stop().await;
    }

    async fn handshake_once(server: &TestServer, username: &str) -> (Connection, Packet) {
        let mut connection = Connection::new(server.connect_stream().await);
        connection
            .send(&Packet::Handshake {
                protocol_version: PROTOCOL_VERSION,
                username: username.into(),
            })
            .await
            .unwrap();
        let packet = time::timeout(Duration::from_secs(2), connection.recv())
            .await
            .expect("server did not answer handshake")
            .expect("server closed without a handshake reply");
        (connection, packet)
    }

    #[tokio::test]
    async fn mutating_username_does_not_share_identity_with_sanitized_form() {
        let server = TestServer::start_with_config(
            0xCAFE_BABE,
            1,
            ServerConfig {
                max_players: 2,
                ..ServerConfig::default()
            },
        );
        let (_online, first) = handshake_once(&server, "foo_bar").await;
        assert!(matches!(first, Packet::LoginSuccess { .. }));
        let (_rejected_dot, second) = handshake_once(&server, "foo.bar").await;
        assert!(matches!(
            second,
            Packet::Disconnect { reason, .. } if reason == "invalid username"
        ));
        let (_rejected_dup, third) = handshake_once(&server, "FOO_BAR").await;
        assert!(matches!(
            third,
            Packet::Disconnect { reason, .. } if reason == "duplicate login"
        ));
        let (_rejected_path, fourth) = handshake_once(&server, "Alice/../Alice").await;
        assert!(matches!(
            fourth,
            Packet::Disconnect { reason, .. } if reason == "invalid username"
        ));
        let (_rejected_reserved, fifth) = handshake_once(&server, "CON").await;
        assert!(matches!(
            fifth,
            Packet::Disconnect { reason, .. } if reason == "invalid username"
        ));
        server.stop().await;
    }

    fn item_use_request(request_id: u128, client_sequence: u64) -> GameplayRequest {
        GameplayRequest {
            request_id,
            client_sequence,
            session_id: 999_999,
            dimension: 0,
            client_revision: 0,
            operation: GameplayOperation::ItemUse { item: 1, count: 1 },
        }
    }

    fn pose_packet(id: PlayerId, sequence: u32) -> Packet {
        Packet::PlayerPosition {
            protocol_version: PROTOCOL_VERSION,
            id,
            sequence,
            sender_time_millis: u64::from(sequence),
            x: 0.0,
            y: 64.0,
            z: 0.0,
            yaw: 0.0,
            pitch: 0.0,
        }
    }

    fn drain_host_events(server: &TestServer) -> Vec<ServerToHost> {
        let mut events = Vec::new();
        while let Ok(event) = server.event_rx.try_recv() {
            events.push(event);
        }
        events
    }

    #[tokio::test]
    async fn oversized_chat_is_rejected_before_host_enqueue() {
        let server = TestServer::start(0xCAFE_BABE, 1);
        let (mut flooder, flooder_id) = server.connect("flood").await;
        let (mut peer, peer_id) = server.connect("peer").await;

        flooder
            .send(&Packet::ChatMessage {
                protocol_version: PROTOCOL_VERSION,
                sender: "flood".into(),
                message: "x".repeat(257),
            })
            .await
            .unwrap();
        flooder.send(&pose_packet(flooder_id, 1)).await.unwrap();
        for sequence in 2..80 {
            flooder
                .send(&pose_packet(flooder_id, sequence))
                .await
                .unwrap();
        }

        peer.send(&Packet::GameplayRequest {
            protocol_version: PROTOCOL_VERSION,
            request: item_use_request(42, 1),
        })
        .await
        .unwrap();

        let forwarded = server
            .next_event_matching(|event| matches!(event, ServerToHost::GameplayRequest { .. }))
            .await;
        assert!(matches!(
            forwarded,
            ServerToHost::GameplayRequest { id, request }
                if id == peer_id && request.request_id == 42
        ));

        let events = drain_host_events(&server);
        assert!(
            events.iter().all(|event| !matches!(
                event,
                ServerToHost::ChatFromClient { message, .. } if message.chars().count() > MAX_CHAT_CHARS
            )),
            "oversized chat entered the host queue: {events:?}"
        );
        assert!(events.iter().all(|event| !matches!(
            event,
            ServerToHost::ClientLeft { id } if *id == peer_id
        )));

        server
            .host_tx
            .send(HostToServer::SendGameplayResponse {
                to: peer_id,
                response: GameplayResponse {
                    request_id: 42,
                    server_sequence: 1,
                    outcome: crate::network::protocol::GameplayOutcome::Accepted { revision: 1 },
                },
            })
            .await
            .unwrap();
        let response = recv_matching(&mut peer, |packet| {
            matches!(packet, Packet::GameplayResponse { .. })
        })
        .await;
        assert!(matches!(
            response,
            Packet::GameplayResponse { response, .. } if response.request_id == 42
        ));
        server.stop().await;
    }

    #[tokio::test]
    async fn chat_and_pose_rate_limits_are_independent() {
        let server = TestServer::start_with_config(
            0xCAFE_BABE,
            1,
            ServerConfig {
                pose_rate_per_second: 1,
                chat_rate_per_second: 1,
                ..ServerConfig::default()
            },
        );
        let (mut client, id) = server.connect("limiter").await;
        let _ = server
            .next_event_matching(|event| matches!(event, ServerToHost::ClientJoined { .. }))
            .await;

        client.send(&pose_packet(id, 1)).await.unwrap();
        client.send(&pose_packet(id, 2)).await.unwrap();
        client
            .send(&Packet::ChatMessage {
                protocol_version: PROTOCOL_VERSION,
                sender: "limiter".into(),
                message: "after-pose".into(),
            })
            .await
            .unwrap();

        let pose = server
            .next_event_matching(|event| matches!(event, ServerToHost::ClientPosition { .. }))
            .await;
        assert!(matches!(
            pose,
            ServerToHost::ClientPosition { id: event_id, sequence: 1, .. } if event_id == id
        ));
        let chat = server
            .next_event_matching(|event| matches!(event, ServerToHost::ChatFromClient { .. }))
            .await;
        assert!(matches!(
            chat,
            ServerToHost::ChatFromClient { id: event_id, message }
                if event_id == id && message == "after-pose"
        ));
        time::sleep(Duration::from_millis(30)).await;
        assert!(drain_host_events(&server)
            .iter()
            .all(|event| !matches!(event, ServerToHost::ClientPosition { sequence: 2, .. })));
        server.stop().await;
    }

    #[tokio::test]
    async fn host_queue_full_backpressures_pose_without_kicking_peer() {
        let reserved = StdTcpListener::bind("127.0.0.1:0").unwrap();
        let addr = reserved.local_addr().unwrap().to_string();
        drop(reserved);
        let (host_tx, host_rx) = tokio::sync::mpsc::channel(16);
        let (event_tx, event_rx) = std_mpsc::sync_channel(4);
        let metrics = NetworkMetrics::default();
        let handle = NetworkServer::spawn_with_config_and_metrics(
            addr.clone(),
            0xCAFE_BABE,
            1,
            host_rx,
            event_tx,
            ServerConfig {
                max_players: 4,
                pose_rate_per_second: 1_000,
                chat_rate_per_second: 1_000,
                ..ServerConfig::default()
            },
            metrics,
        );
        let connect = |username: &'static str| async {
            let deadline = Instant::now() + Duration::from_secs(2);
            let stream = loop {
                match tokio::net::TcpStream::connect(&addr).await {
                    Ok(stream) if stream.local_addr().ok() != stream.peer_addr().ok() => {
                        break stream;
                    }
                    _ if Instant::now() < deadline => {
                        time::sleep(Duration::from_millis(10)).await;
                    }
                    Ok(_) => panic!("server did not start before the connection deadline"),
                    Err(error) => panic!("server did not start: {error}"),
                }
            };
            let mut connection = Connection::new(stream);
            connection
                .send(&Packet::Handshake {
                    protocol_version: PROTOCOL_VERSION,
                    username: username.into(),
                })
                .await
                .unwrap();
            let id = match time::timeout(Duration::from_secs(2), connection.recv())
                .await
                .unwrap()
                .unwrap()
            {
                Packet::LoginSuccess { player_id, .. } => player_id,
                packet => panic!("expected login success, got {packet:?}"),
            };
            (connection, id)
        };
        let (mut flooder, flooder_id) = connect("flood").await;
        let (mut peer, peer_id) = connect("peer").await;
        {
            let deadline = Instant::now() + Duration::from_secs(2);
            let mut joined = 0u8;
            while joined < 2 {
                match event_rx.try_recv() {
                    Ok(ServerToHost::ClientJoined { .. }) => joined += 1,
                    Ok(_) => {}
                    Err(std_mpsc::TryRecvError::Empty) if Instant::now() < deadline => {
                        time::sleep(Duration::from_millis(10)).await;
                    }
                    Err(_) => panic!("host event channel closed before both clients joined"),
                }
            }
        }
        for sequence in 1..=16 {
            flooder
                .send(&pose_packet(flooder_id, sequence))
                .await
                .unwrap();
        }
        peer.send(&Packet::GameplayRequest {
            protocol_version: PROTOCOL_VERSION,
            request: item_use_request(7, 1),
        })
        .await
        .unwrap();
        time::sleep(Duration::from_millis(50)).await;
        let mut saw_peer_request = false;
        let mut peer_left = false;
        while let Ok(event) = event_rx.try_recv() {
            match event {
                ServerToHost::GameplayRequest { id, request }
                    if id == peer_id && request.request_id == 7 =>
                {
                    saw_peer_request = true;
                }
                ServerToHost::ClientLeft { id } if id == peer_id => peer_left = true,
                _ => {}
            }
        }
        assert!(!peer_left, "host-queue Full must not kick the peer");
        if !saw_peer_request {
            peer.send(&Packet::GameplayRequest {
                protocol_version: PROTOCOL_VERSION,
                request: item_use_request(8, 2),
            })
            .await
            .unwrap();
            let deadline = Instant::now() + Duration::from_secs(2);
            let mut recovered = false;
            while Instant::now() < deadline {
                if let Ok(ServerToHost::GameplayRequest { id, request }) = event_rx.try_recv() {
                    if id == peer_id && request.request_id == 8 {
                        recovered = true;
                        break;
                    }
                }
                time::sleep(Duration::from_millis(10)).await;
            }
            assert!(
                recovered,
                "peer GameplayRequest must complete after pose flood backpressure"
            );
        }
        let _ = host_tx.try_send(HostToServer::Stop);
        handle.join().unwrap();
    }

    #[tokio::test]
    async fn broadcast_container_slot_is_reliable_or_evicts_slow_viewer() {
        let server = TestServer::start(0xCAFE_BABE, 1);
        let (mut fast, _) = server.connect("fast").await;
        let (slow, slow_id) = server.connect("slow").await;

        for index in 0..(CLIENT_QUEUE_CAPACITY + 8) {
            server
                .host_tx
                .send(HostToServer::BroadcastChat {
                    sender: "pad".into(),
                    message: format!("pad-{index}"),
                })
                .await
                .unwrap();
            let _ = time::timeout(
                Duration::from_millis(80),
                recv_matching(&mut fast, |packet| {
                    matches!(packet, Packet::ChatMessage { .. })
                }),
            )
            .await;
        }

        server
            .host_tx
            .send(HostToServer::ContainerSlotUpdate {
                to: None,
                dimension: 0,
                revision: 11,
                x: 8,
                y: 80,
                z: 8,
                slot_index: 3,
                slot: None,
            })
            .await
            .unwrap();

        let fast_slot = recv_matching(&mut fast, |packet| {
            matches!(
                packet,
                Packet::ContainerSlotUpdate {
                    revision: 11,
                    slot_index: 3,
                    ..
                }
            )
        })
        .await;
        assert!(matches!(
            fast_slot,
            Packet::ContainerSlotUpdate {
                x: 8,
                y: 80,
                z: 8,
                ..
            }
        ));

        let mut slow = slow;
        let mut slow_got_slot = false;
        let mut slow_kicked = false;
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline && !slow_got_slot && !slow_kicked {
            while let Ok(event) = server.event_rx.try_recv() {
                if matches!(event, ServerToHost::ClientLeft { id } if id == slow_id) {
                    slow_kicked = true;
                }
            }
            match time::timeout(Duration::from_millis(50), slow.recv()).await {
                Ok(Ok(Packet::ContainerSlotUpdate { revision: 11, .. })) => {
                    slow_got_slot = true;
                }
                Ok(Ok(_)) => {}
                Ok(Err(_)) => slow_kicked = true,
                Err(_) => {}
            }
        }
        assert!(
            slow_got_slot || slow_kicked,
            "slow viewer must receive the slot or be kicked; silent chest fork is forbidden"
        );
        server.stop().await;
    }

    #[tokio::test]
    async fn pre_auth_connections_are_capped_at_twice_max_players() {
        let server = TestServer::start_with_config(
            0xCAFE_BABE,
            1,
            ServerConfig {
                max_players: 1,
                handshake_timeout: Duration::from_millis(200),
                ..ServerConfig::default()
            },
        );
        let _hold_a = server.connect_stream().await;
        time::sleep(Duration::from_millis(40)).await;
        let _hold_b = server.connect_stream().await;
        time::sleep(Duration::from_millis(40)).await;

        let mut excess = Connection::new(server.connect_stream().await);
        let excess_result = time::timeout(Duration::from_millis(500), excess.recv()).await;
        assert!(
            matches!(excess_result, Ok(Err(_)) | Err(_)),
            "excess pre-auth socket must be closed immediately, got {excess_result:?}"
        );

        drop(_hold_a);
        drop(_hold_b);
        time::sleep(Duration::from_millis(300)).await;
        let (_joined, packet) = handshake_once(&server, "late").await;
        assert!(matches!(packet, Packet::LoginSuccess { .. }));
        server.stop().await;
    }
}
