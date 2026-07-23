use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc as std_mpsc, Arc};
use std::thread::JoinHandle;
use std::time::Duration;

use tokio::net::TcpListener;
use tokio::sync::{mpsc, watch, Mutex};
use tokio::time::{self, Instant};

use super::protocol::{Action, Packet, PlayerId, PROTOCOL_VERSION};
use super::transport::Connection;

const CLIENT_QUEUE_CAPACITY: usize = 64;
const HOST_COMMAND_POLL_INTERVAL: Duration = Duration::from_millis(10);
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(5);
const CLIENT_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug)]
pub enum ServerToHost {
    Disconnected {
        reason: String,
    },
    ClientJoined {
        id: PlayerId,
        username: String,
    },
    ClientLeft {
        id: PlayerId,
    },
    ClientPosition {
        id: PlayerId,
        sequence: u32,
        sender_time_millis: u64,
        x: f32,
        y: f32,
        z: f32,
        yaw: f32,
        pitch: f32,
    },
    ClientAction {
        id: PlayerId,
        action: Action,
    },
    ClientBlockChange {
        id: PlayerId,
        x: i32,
        y: i32,
        z: i32,
        block: u32,
    },
    ChatFromClient {
        id: PlayerId,
        message: String,
    },
}

#[derive(Debug)]
pub enum HostToServer {
    BroadcastBlockChange {
        x: i32,
        y: i32,
        z: i32,
        block: u32,
    },
    SendChunk {
        cx: i32,
        cz: i32,
        blocks: Vec<u8>,
        to: PlayerId,
    },
    BroadcastTimeSync {
        ticks: u64,
        weather: u8,
    },
    BroadcastPlayerPosition {
        id: PlayerId,
        sequence: u32,
        sender_time_millis: u64,
        x: f32,
        y: f32,
        z: f32,
        yaw: f32,
        pitch: f32,
    },
    BroadcastPlayerAction {
        id: PlayerId,
        action: Action,
    },
    BroadcastChat {
        sender: String,
        message: String,
    },
    NotifyPlayerJoin {
        id: PlayerId,
        username: String,
    },
    Stop,
}

struct ClientSession {
    id: PlayerId,
    username: String,
    out_tx: mpsc::Sender<Packet>,
    pose_tx: watch::Sender<Option<Packet>>,
}

type Sessions = Arc<Mutex<HashMap<PlayerId, ClientSession>>>;

pub struct NetworkServer {
    seed: u64,
    gamemode: u8,
    next_player_id: Arc<AtomicU64>,
    sessions: Sessions,
    server_to_host: std_mpsc::Sender<ServerToHost>,
}

impl NetworkServer {
    pub fn spawn(
        bind_addr: String,
        seed: u64,
        gamemode: u8,
        host_to_server: std_mpsc::Receiver<HostToServer>,
        server_to_host: std_mpsc::Sender<ServerToHost>,
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
                };
                server.run(listener, host_to_server).await;
            });
        })
    }

    async fn run(self, listener: TcpListener, host_to_server: std_mpsc::Receiver<HostToServer>) {
        // Polling try_recv keeps the blocking std receiver off Tokio's workers and,
        // unlike spawn_blocking(recv), lets runtime shutdown finish immediately.
        let mut command_tick = time::interval(HOST_COMMAND_POLL_INTERVAL);

        loop {
            tokio::select! {
                accepted = listener.accept() => {
                    match accepted {
                        Ok((stream, peer_addr)) => {
                            eprintln!("[NetworkServer] Accepted TCP connection from {peer_addr}");
                            let sessions = Arc::clone(&self.sessions);
                            let next_player_id = Arc::clone(&self.next_player_id);
                            let server_to_host = self.server_to_host.clone();
                            let seed = self.seed;
                            let gamemode = self.gamemode;
                            tokio::spawn(async move {
                                Self::run_client(
                                    Connection::new(stream),
                                    seed,
                                    gamemode,
                                    next_player_id,
                                    sessions,
                                    server_to_host,
                                )
                                .await;
                            });
                        }
                        Err(error) => {
                            eprintln!("[NetworkServer] Multiplayer server accept failed: {error}");
                        }
                    }
                }
                _ = command_tick.tick() => {
                    let mut latest_positions = HashMap::new();
                    loop {
                        match host_to_server.try_recv() {
                            Ok(HostToServer::Stop) => return,
                            Ok(command @ HostToServer::BroadcastPlayerPosition { id, .. }) => {
                                latest_positions.insert(id, command);
                            }
                            Ok(command) => self.handle_host_command(command).await,
                            Err(std_mpsc::TryRecvError::Empty) => break,
                            Err(std_mpsc::TryRecvError::Disconnected) => return,
                        }
                    }
                    let mut latest_positions: Vec<_> = latest_positions.into_iter().collect();
                    latest_positions.sort_by_key(|(id, _)| *id);
                    for (_, command) in latest_positions {
                        self.handle_host_command(command).await;
                    }
                }
            }
        }
    }

    async fn run_client(
        mut connection: Connection,
        seed: u64,
        gamemode: u8,
        next_player_id: Arc<AtomicU64>,
        sessions: Sessions,
        server_to_host: std_mpsc::Sender<ServerToHost>,
    ) {
        let handshake = match time::timeout(CLIENT_TIMEOUT, connection.recv()).await {
            Ok(Ok(Packet::Handshake {
                protocol_version,
                username,
            })) => {
                eprintln!("[NetworkServer] Received Handshake: username='{username}', protocol_version={protocol_version}");
                if protocol_version != PROTOCOL_VERSION {
                    eprintln!("[NetworkServer] Handshake rejected: version mismatch (expected {PROTOCOL_VERSION}, got {protocol_version})");
                    let _ = connection
                        .send(&Packet::Disconnect {
                            protocol_version: PROTOCOL_VERSION,
                            reason: format!(
                                "protocol version mismatch: server {PROTOCOL_VERSION}, client {protocol_version}"
                            ),
                        })
                        .await;
                    return;
                }
                username
            }
            Ok(Ok(packet)) => {
                eprintln!("[NetworkServer] Handshake rejected: expected Packet::Handshake, got {packet:?}");
                let _ = connection
                    .send(&Packet::Disconnect {
                        protocol_version: PROTOCOL_VERSION,
                        reason: "expected handshake".into(),
                    })
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

        let id = next_player_id.fetch_add(1, Ordering::Relaxed);
        if connection
            .send(&Packet::LoginSuccess {
                protocol_version: PROTOCOL_VERSION,
                player_id: id,
                seed,
                gamemode,
            })
            .await
            .is_err()
        {
            eprintln!(
                "[NetworkServer] Failed to send LoginSuccess to '{handshake}' (Player ID: {id})"
            );
            return;
        }

        eprintln!("[NetworkServer] Sent LoginSuccess to '{handshake}' (Player ID: {id})");

        let (out_tx, mut out_rx) = mpsc::channel(CLIENT_QUEUE_CAPACITY);
        let (pose_tx, mut pose_rx) = watch::channel(None);
        let roster_tx = out_tx.clone();
        sessions.lock().await.insert(
            id,
            ClientSession {
                id,
                username: handshake.clone(),
                out_tx,
                pose_tx,
            },
        );
        let roster: Vec<(PlayerId, String)> = sessions
            .lock()
            .await
            .values()
            .filter(|session| session.id != id)
            .map(|session| (session.id, session.username.clone()))
            .collect();
        for (existing_id, username) in roster {
            let _ = roster_tx.try_send(Packet::PlayerJoin {
                protocol_version: PROTOCOL_VERSION,
                id: existing_id,
                username,
            });
        }
        if server_to_host
            .send(ServerToHost::ClientJoined {
                id,
                username: handshake,
            })
            .is_err()
        {
            sessions.lock().await.remove(&id);
            return;
        }

        let (mut reader, mut writer) = connection.into_split();
        let mut send_task = tokio::spawn(async move {
            let mut keepalive =
                time::interval_at(Instant::now() + KEEPALIVE_INTERVAL, KEEPALIVE_INTERVAL);

            loop {
                tokio::select! {
                    queued = out_rx.recv() => {
                        match queued {
                            Some(packet) => {
                                if writer.send(&packet).await.is_err() {
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
                    changed = pose_rx.changed() => {
                        if changed.is_err() {
                            eprintln!("[NetworkServer] Send task: pose channel closed");
                            break;
                        }
                        let packet = pose_rx.borrow_and_update().clone();
                        if let Some(packet) = packet {
                            if writer.send(&packet).await.is_err() {
                                eprintln!("[NetworkServer] Send task: writer send failed for pose");
                                break;
                            }
                        }
                    }
                    _ = keepalive.tick() => {
                        if writer.send(&Packet::Keepalive {
                            protocol_version: PROTOCOL_VERSION,
                        }).await.is_err() {
                            eprintln!("[NetworkServer] Send task: keepalive send failed");
                            break;
                        }
                    }
                }
            }
        });

        #[allow(unused_assignments)]
        let mut disconnect_reason = "unknown".to_string();
        loop {
            tokio::select! {
                incoming = time::timeout(CLIENT_TIMEOUT, reader.recv()) => {
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
                            if server_to_host.send(ServerToHost::ClientPosition {
                                id,
                                sequence,
                                sender_time_millis,
                                x,
                                y,
                                z,
                                yaw,
                                pitch,
                            }).is_err() {
                                disconnect_reason = "host channel closed (ClientPosition)".into();
                                break;
                            }
                        }
                        Ok(Ok(Packet::PlayerAction { action, .. })) => {
                            if server_to_host.send(ServerToHost::ClientAction { id, action }).is_err() {
                                disconnect_reason = "host channel closed (ClientAction)".into();
                                break;
                            }
                        }
                        Ok(Ok(Packet::BlockChange { x, y, z, block, .. })) => {
                            if server_to_host.send(ServerToHost::ClientBlockChange {
                                id, x, y, z, block,
                            }).is_err() {
                                disconnect_reason = "host channel closed (ClientBlockChange)".into();
                                break;
                            }
                        }
                        Ok(Ok(Packet::ChatMessage { message, .. })) => {
                            if server_to_host.send(ServerToHost::ChatFromClient { id, message }).is_err() {
                                disconnect_reason = "host channel closed (ChatFromClient)".into();
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
        Self::remove_client(id, &sessions, &server_to_host).await;
        send_task.abort();
        send_task.abort();
    }

    async fn remove_client(
        id: PlayerId,
        sessions: &Sessions,
        server_to_host: &std_mpsc::Sender<ServerToHost>,
    ) {
        let removed = sessions.lock().await.remove(&id);
        let Some(session) = removed else {
            return;
        };

        eprintln!(
            "[NetworkServer] Client '{}' (Player ID: {}) disconnected",
            session.username, id
        );
        let _ = server_to_host.send(ServerToHost::ClientLeft { id });
        Self::broadcast_to(
            sessions,
            Packet::PlayerLeave {
                protocol_version: PROTOCOL_VERSION,
                id,
            },
        )
        .await;
    }

    async fn handle_host_command(&self, command: HostToServer) {
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
            Self::broadcast_pose(
                &self.sessions,
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

        let reliable_broadcast = matches!(
            &command,
            HostToServer::BroadcastBlockChange { .. } | HostToServer::BroadcastChat { .. }
        );
        let (packet, recipient) = match command {
            HostToServer::BroadcastBlockChange { x, y, z, block } => (
                Packet::BlockChange {
                    protocol_version: PROTOCOL_VERSION,
                    x,
                    y,
                    z,
                    block,
                },
                None,
            ),
            HostToServer::SendChunk { cx, cz, blocks, to } => (
                Packet::ChunkData {
                    protocol_version: PROTOCOL_VERSION,
                    cx,
                    cz,
                    blocks,
                },
                Some(to),
            ),
            HostToServer::BroadcastTimeSync { ticks, weather } => (
                Packet::TimeSync {
                    protocol_version: PROTOCOL_VERSION,
                    ticks,
                    weather,
                },
                None,
            ),
            HostToServer::BroadcastPlayerPosition { .. } => {
                unreachable!("player positions use the latest-wins pose channel")
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
            HostToServer::Stop => return,
        };

        if let Some(id) = recipient {
            Self::send_to(&self.sessions, id, packet).await;
        } else if reliable_broadcast {
            Self::broadcast_reliably(&self.sessions, packet).await;
        } else {
            Self::broadcast_to(&self.sessions, packet).await;
        }
    }

    async fn send_to(sessions: &Sessions, id: PlayerId, packet: Packet) {
        let tx = sessions
            .lock()
            .await
            .get(&id)
            .map(|session| session.out_tx.clone());
        if let Some(tx) = tx {
            // Targeted packets are join catch-up data and must not be dropped.
            let _ = tx.send(packet).await;
        }
    }

    async fn broadcast_reliably(sessions: &Sessions, packet: Packet) {
        let senders: Vec<_> = sessions
            .lock()
            .await
            .values()
            .map(|session| session.out_tx.clone())
            .collect();
        for tx in senders {
            // Block mutations and user chat are ordered authoritative state,
            // so applying backpressure is preferable to silently dropping them.
            let _ = tx.send(packet.clone()).await;
        }
    }

    async fn broadcast_pose(sessions: &Sessions, packet: Packet) {
        let senders: Vec<_> = sessions
            .lock()
            .await
            .values()
            .map(|session| session.pose_tx.clone())
            .collect();
        for tx in senders {
            tx.send_replace(Some(packet.clone()));
        }
    }

    async fn broadcast_to(sessions: &Sessions, packet: Packet) {
        let senders: Vec<_> = sessions
            .lock()
            .await
            .values()
            .map(|session| session.out_tx.clone())
            .collect();
        for tx in senders {
            let _ = tx.try_send(packet.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener as StdTcpListener;

    struct TestServer {
        addr: String,
        host_tx: std_mpsc::Sender<HostToServer>,
        event_rx: std_mpsc::Receiver<ServerToHost>,
        handle: JoinHandle<()>,
    }

    impl TestServer {
        fn start(seed: u64, gamemode: u8) -> Self {
            let reserved = StdTcpListener::bind("127.0.0.1:0").unwrap();
            let addr = reserved.local_addr().unwrap().to_string();
            drop(reserved);

            let (host_tx, host_rx) = std_mpsc::channel();
            let (event_tx, event_rx) = std_mpsc::channel();
            let handle = NetworkServer::spawn(addr.clone(), seed, gamemode, host_rx, event_tx);
            Self {
                addr,
                host_tx,
                event_rx,
                handle,
            }
        }

        async fn connect_stream(&self) -> tokio::net::TcpStream {
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                match tokio::net::TcpStream::connect(&self.addr).await {
                    Ok(stream) => break stream,
                    Err(_) if Instant::now() < deadline => {
                        time::sleep(Duration::from_millis(10)).await;
                    }
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

        async fn stop(self) {
            let _ = self.host_tx.send(HostToServer::Stop);
            time::timeout(
                Duration::from_secs(2),
                tokio::task::spawn_blocking(move || {
                    self.handle.join().unwrap();
                }),
            )
            .await
            .expect("server thread did not stop")
            .unwrap();
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
        let (host_tx, host_rx) = std_mpsc::channel();
        let (event_tx, event_rx) = std_mpsc::channel();
        let handle = NetworkServer::spawn(addr.clone(), 1, 0, host_rx, event_tx);

        let event = match event_rx.recv_timeout(Duration::from_secs(3)) {
            Ok(event) => event,
            Err(error) => {
                let _ = host_tx.send(HostToServer::Stop);
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
        let (_client, id) = server.connect("steve").await;

        let joined = server
            .next_event_matching(|event| matches!(event, ServerToHost::ClientJoined { .. }))
            .await;
        match joined {
            ServerToHost::ClientJoined {
                id: joined_id,
                username,
            } => {
                assert_eq!(joined_id, id);
                assert_eq!(username, "steve");
            }
            _ => unreachable!(),
        }

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
            .send(HostToServer::BroadcastPlayerPosition {
                id: id_a,
                sequence: 12,
                sender_time_millis: 600,
                x: 10.0,
                y: 65.0,
                z: -4.0,
                yaw: 1.5,
                pitch: -0.25,
            })
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
    async fn unsent_pose_updates_are_latest_wins_per_client() {
        let sessions: Sessions = Arc::new(Mutex::new(HashMap::new()));
        let (out_tx, _out_rx) = mpsc::channel(1);
        let (pose_tx, mut pose_rx) = watch::channel(None);
        sessions.lock().await.insert(
            1,
            ClientSession {
                id: 1,
                username: "alex".into(),
                out_tx,
                pose_tx,
            },
        );

        for sequence in [4, 5] {
            NetworkServer::broadcast_pose(
                &sessions,
                Packet::PlayerPosition {
                    protocol_version: PROTOCOL_VERSION,
                    id: 9,
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

        pose_rx.changed().await.unwrap();
        let latest = pose_rx.borrow_and_update().clone().unwrap();
        assert!(matches!(
            latest,
            Packet::PlayerPosition {
                sequence: 5,
                sender_time_millis: 250,
                x: 5.0,
                ..
            }
        ));
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
}
