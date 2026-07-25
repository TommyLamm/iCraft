use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc as std_mpsc, Arc};
use std::thread::JoinHandle;
use std::time::Duration;

use tokio::net::TcpListener;
use tokio::sync::{mpsc, watch, Mutex, Notify};
use tokio::time::{self, Instant};

use super::protocol::{Action, LightningStrike, Packet, PlayerId, PROTOCOL_VERSION};
use super::transport::Connection;

const CLIENT_QUEUE_CAPACITY: usize = 64;
const HOST_COMMAND_POLL_INTERVAL: Duration = Duration::from_millis(10);
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(5);
const CLIENT_TIMEOUT: Duration = Duration::from_secs(15);
const RELIABLE_ENQUEUE_TIMEOUT: Duration = Duration::from_millis(250);

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
        weather_remaining_ticks: f32,
    },
    SendTimeSync {
        ticks: u64,
        weather: u8,
        weather_remaining_ticks: f32,
        to: PlayerId,
    },
    BroadcastLightningStrike {
        strike: LightningStrike,
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
    pose_mailbox: Arc<PoseMailbox>,
    cancel_tx: watch::Sender<bool>,
}

type Sessions = Arc<Mutex<HashMap<PlayerId, ClientSession>>>;

#[derive(Default)]
struct PoseMailbox {
    pending: Mutex<HashMap<PlayerId, Packet>>,
    notify: Notify,
}

impl PoseMailbox {
    async fn replace(&self, player_id: PlayerId, packet: Packet) {
        self.pending.lock().await.insert(player_id, packet);
        self.notify.notify_one();
    }

    async fn drain(&self) -> Vec<Packet> {
        let mut packets: Vec<_> = self
            .pending
            .lock()
            .await
            .drain()
            .map(|(_, packet)| packet)
            .collect();
        packets.sort_by_key(|packet| match packet {
            Packet::PlayerPosition { id, .. } => *id,
            _ => PlayerId::MAX,
        });
        packets
    }
}

async fn queue_initial_roster(
    tx: &mpsc::Sender<Packet>,
    roster: impl IntoIterator<Item = (PlayerId, String)>,
) -> Result<(), mpsc::error::SendError<Packet>> {
    for (id, username) in roster {
        tx.send(Packet::PlayerJoin {
            protocol_version: PROTOCOL_VERSION,
            id,
            username,
        })
        .await?;
    }
    Ok(())
}

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
        let roster_tx = out_tx.clone();
        let pose_mailbox = Arc::new(PoseMailbox::default());
        let (cancel_tx, mut cancel_rx) = watch::channel(false);
        let (mut reader, mut writer) = connection.into_split();
        let writer_pose_mailbox = Arc::clone(&pose_mailbox);
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
                    _ = writer_pose_mailbox.notify.notified() => {
                        for packet in writer_pose_mailbox.drain().await {
                            if writer.send(&packet).await.is_err() {
                                eprintln!("[NetworkServer] Send task: writer send failed for pose");
                                return;
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

        sessions.lock().await.insert(
            id,
            ClientSession {
                id,
                username: handshake.clone(),
                out_tx,
                pose_mailbox: Arc::clone(&pose_mailbox),
                cancel_tx,
            },
        );
        let mut roster: Vec<(PlayerId, String)> = sessions
            .lock()
            .await
            .values()
            .filter(|session| session.id != id)
            .map(|session| (session.id, session.username.clone()))
            .collect();
        roster.sort_by_key(|(existing_id, _)| *existing_id);
        if !matches!(
            time::timeout(CLIENT_TIMEOUT, queue_initial_roster(&roster_tx, roster)).await,
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
                username: handshake,
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

        let _ = session.cancel_tx.send(true);
        eprintln!(
            "[NetworkServer] Client '{}' (Player ID: {}) disconnected",
            session.username, id
        );
        let _ = server_to_host.send(ServerToHost::ClientLeft { id });
        let failed = Self::broadcast_reliably(
            sessions,
            Packet::PlayerLeave {
                protocol_version: PROTOCOL_VERSION,
                id,
            },
        )
        .await;
        Self::evict_slow_clients(sessions, server_to_host, failed).await;
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
            HostToServer::BroadcastBlockChange { .. }
                | HostToServer::BroadcastChat { .. }
                | HostToServer::NotifyPlayerJoin { .. }
                | HostToServer::BroadcastTimeSync { .. }
                | HostToServer::BroadcastLightningStrike { .. }
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

        let failed = if let Some(id) = recipient {
            Self::send_to(&self.sessions, id, packet).await
        } else if reliable_broadcast {
            Self::broadcast_reliably(&self.sessions, packet).await
        } else {
            Self::broadcast_to(&self.sessions, packet).await;
            Vec::new()
        };
        Self::evict_slow_clients(&self.sessions, &self.server_to_host, failed).await;
    }

    async fn send_to(sessions: &Sessions, id: PlayerId, packet: Packet) -> Vec<PlayerId> {
        let tx = sessions
            .lock()
            .await
            .get(&id)
            .map(|session| session.out_tx.clone());
        if let Some(tx) = tx {
            // Targeted catch-up data is reliable. Bound the wait so a client
            // that has stopped draining its queue is disconnected instead of
            // stalling the host command loop forever.
            if !matches!(
                time::timeout(RELIABLE_ENQUEUE_TIMEOUT, tx.send(packet)).await,
                Ok(Ok(()))
            ) {
                return vec![id];
            }
        }
        Vec::new()
    }

    async fn broadcast_reliably(sessions: &Sessions, packet: Packet) -> Vec<PlayerId> {
        let senders: Vec<_> = sessions
            .lock()
            .await
            .values()
            .map(|session| (session.id, session.out_tx.clone()))
            .collect();
        let mut sends = tokio::task::JoinSet::new();
        for (id, tx) in senders {
            let packet = packet.clone();
            sends.spawn(async move {
                let delivered = matches!(
                    time::timeout(RELIABLE_ENQUEUE_TIMEOUT, tx.send(packet)).await,
                    Ok(Ok(()))
                );
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

    async fn evict_slow_clients(
        sessions: &Sessions,
        server_to_host: &std_mpsc::Sender<ServerToHost>,
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
                "[NetworkServer] Disconnecting slow client '{}' (Player ID: {}): reliable queue did not drain",
                session.username, id
            );
            let _ = server_to_host.send(ServerToHost::ClientLeft { id });
            let failed = Self::broadcast_reliably(
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

    async fn broadcast_pose(sessions: &Sessions, packet: Packet) {
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
    async fn block_change_reports_authenticated_session_id_to_host() {
        let server = TestServer::start(0xCAFE_BABE, 1);
        let (mut client, id) = server.connect("steve").await;

        client
            .send(&Packet::BlockChange {
                protocol_version: PROTOCOL_VERSION,
                x: 3,
                y: 80,
                z: -4,
                block: 3,
            })
            .await
            .unwrap();

        let event = server
            .next_event_matching(|event| matches!(event, ServerToHost::ClientBlockChange { .. }))
            .await;
        assert!(matches!(
            event,
            ServerToHost::ClientBlockChange {
                id: event_id,
                x: 3,
                y: 80,
                z: -4,
                block: 3,
            } if event_id == id
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
    async fn unsent_pose_updates_are_latest_wins_per_player() {
        let sessions: Sessions = Arc::new(Mutex::new(HashMap::new()));
        let (out_tx, _out_rx) = mpsc::channel(1);
        let pose_mailbox = Arc::new(PoseMailbox::default());
        sessions.lock().await.insert(
            1,
            ClientSession {
                id: 1,
                username: "alex".into(),
                out_tx,
                pose_mailbox: Arc::clone(&pose_mailbox),
                cancel_tx: watch::channel(false).0,
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
        observer_out_tx
            .try_send(Packet::Keepalive {
                protocol_version: PROTOCOL_VERSION,
            })
            .unwrap();
        sessions.lock().await.insert(
            1,
            ClientSession {
                id: 1,
                username: "observer".into(),
                out_tx: observer_out_tx,
                pose_mailbox: Arc::new(PoseMailbox::default()),
                cancel_tx: watch::channel(false).0,
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
                cancel_tx: watch::channel(false).0,
            },
        );

        let (event_tx, _event_rx) = std_mpsc::channel();
        let server = NetworkServer {
            seed: 0,
            gamemode: 1,
            next_player_id: Arc::new(AtomicU64::new(3)),
            sessions: Arc::clone(&sessions),
            server_to_host: event_tx.clone(),
        };

        let observer = tokio::spawn(async move {
            time::sleep(Duration::from_millis(25)).await;
            let queued = observer_out_rx.recv().await.unwrap();
            let joined = observer_out_rx.recv().await.unwrap();
            let left = observer_out_rx.recv().await.unwrap();
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
        out_tx
            .try_send(Packet::Keepalive {
                protocol_version: PROTOCOL_VERSION,
            })
            .unwrap();
        let (cancel_tx, mut cancel_rx) = watch::channel(false);
        sessions.lock().await.insert(
            1,
            ClientSession {
                id: 1,
                username: "slow-client".into(),
                out_tx,
                pose_mailbox: Arc::new(PoseMailbox::default()),
                cancel_tx,
            },
        );

        let (event_tx, event_rx) = std_mpsc::channel();
        let server = NetworkServer {
            seed: 0,
            gamemode: 1,
            next_player_id: Arc::new(AtomicU64::new(2)),
            sessions: Arc::clone(&sessions),
            server_to_host: event_tx,
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
                x: 7,
                y: 80,
                z: -3,
                block: 4,
            }),
        )
        .await;
        time::sleep(Duration::from_millis(25)).await;
        assert!(
            event_rx
                .try_iter()
                .all(|event| !matches!(event, ServerToHost::ClientBlockChange { id: event_id, .. } if event_id == id)),
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
            .send(HostToServer::SendTimeSync {
                ticks: 21_000,
                weather: 2,
                weather_remaining_ticks: 3_500.25,
                to: joining_id,
            })
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
            .send(HostToServer::BroadcastTimeSync {
                ticks: 22_000,
                weather: 2,
                weather_remaining_ticks: 4_500.0,
            })
            .unwrap();
        server
            .host_tx
            .send(HostToServer::BroadcastLightningStrike { strike })
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
