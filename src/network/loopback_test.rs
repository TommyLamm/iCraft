//! Shared loopback NetworkServer fixture for unit tests.
//!
//! Keeps the Windows self-connect retry in one place so server unit tests do
//! not re-copy ephemeral-port workarounds.

use super::channels::{HostToServer, ServerConfig, ServerToHost};
use super::protocol::{Packet, PlayerId, PROTOCOL_VERSION};
use super::server::{NetworkServer, MAX_CATCHUP_QUEUE_DEPTH};
use super::session::{NetworkMetrics, NetworkMetricsSnapshot};
use super::transport::Connection;
use std::net::TcpListener as StdTcpListener;
use std::sync::mpsc as std_mpsc;
use std::thread::JoinHandle;
use std::time::Duration;
use tokio::time::{self, Instant};

/// Connect to a freshly spawned loopback server, retrying Windows self-connect.
///
/// On Windows, connecting before the server has bound can transiently
/// self-connect when the reserved server port is selected as the client's
/// ephemeral port. Reject those sockets and retry until a real peer appears.
pub(crate) async fn connect_loopback_stream(addr: &str) -> tokio::net::TcpStream {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match tokio::net::TcpStream::connect(addr).await {
            Ok(stream) if stream.local_addr().ok() != stream.peer_addr().ok() => {
                break stream;
            }
            Ok(_) if Instant::now() < deadline => {
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

pub(crate) struct LoopbackTestServer {
    pub(crate) addr: String,
    pub(crate) host_tx: tokio::sync::mpsc::Sender<HostToServer>,
    pub(crate) event_rx: std_mpsc::Receiver<ServerToHost>,
    pub(crate) handle: JoinHandle<()>,
    pub(crate) metrics: NetworkMetrics,
}

impl LoopbackTestServer {
    pub(crate) fn start(seed: u64, gamemode: u8) -> Self {
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

    pub(crate) fn start_with_config(seed: u64, gamemode: u8, config: ServerConfig) -> Self {
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

    pub(crate) async fn connect_stream(&self) -> tokio::net::TcpStream {
        connect_loopback_stream(&self.addr).await
    }

    pub(crate) async fn connect(&self, username: &str) -> (Connection, PlayerId) {
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

    pub(crate) async fn next_event_matching(
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

    pub(crate) async fn stop(self) -> NetworkMetricsSnapshot {
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
