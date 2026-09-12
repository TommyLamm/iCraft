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
    HostEventSendError, HostEventSender, HostToServer, ProjectionDest, ProjectionEvent,
    ServerConfig, ServerToHost,
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
    authenticate_handshake_username, chat_exceeds_display_cap, prepare_gameplay_request,
    queue_initial_roster, remove_client, route_gameplay_request, run_client,
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
#[path = "server_tests.rs"]
mod tests;

