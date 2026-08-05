use std::collections::{BTreeMap, HashMap};
use std::sync::mpsc::{Receiver, Sender};
use std::thread::JoinHandle;
use std::time::Duration;

use tokio::net::TcpStream;
use tokio::time::{self, Instant};

use super::protocol::{
    Action, EntityStateWire, LightningStrike, Packet, PlayerEffectWire, PlayerId, PROTOCOL_VERSION,
};
use super::transport::Connection;

/// The client/game boundary is the app-level inbound queue (network client ->
/// game thread).  Account ownership only after the synchronous send accepts it;
/// transport socket writes are intentionally not included because the OS owns
/// that buffering.  Reliable replication (including revision-gated catch-up)
/// remains FIFO on this same inbound boundary.
fn send_to_game(
    sender: &Sender<ClientToGame>,
    event: ClientToGame,
) -> Result<(), std::sync::mpsc::SendError<ClientToGame>> {
    let bytes = std::mem::size_of_val(&event) as u64;
    crate::perf::tracked_send(
        sender,
        event,
        bytes,
        &crate::perf::queue_stats(crate::perf::QueueCategory::Inbound),
    )
}

#[derive(Clone)]
struct ClientEventSender(Sender<ClientToGame>);

impl ClientEventSender {
    fn send(&self, event: ClientToGame) -> Result<(), std::sync::mpsc::SendError<ClientToGame>> {
        send_to_game(&self.0, event)
    }
}

#[derive(Debug)]
pub enum ClientToGame {
    Connected {
        player_id: PlayerId,
        seed: u64,
        gamemode: u8,
    },
    Disconnected {
        reason: String,
    },
    PlayerJoin {
        id: PlayerId,
        username: String,
    },
    PlayerLeave {
        id: PlayerId,
    },
    PlayerPosition {
        id: PlayerId,
        sequence: u32,
        sender_time_millis: u64,
        x: f32,
        y: f32,
        z: f32,
        yaw: f32,
        pitch: f32,
    },
    PlayerAction {
        id: PlayerId,
        action: Action,
    },
    BlockChange {
        dimension: u8,
        revision: u64,
        x: i32,
        y: i32,
        z: i32,
        block: u32,
        state: u8,
    },
    BlockActionResult {
        x: i32,
        y: i32,
        z: i32,
        success: bool,
        consumed_item: bool,
        drops: Vec<crate::network::protocol::ItemWire>,
    },
    ChunkData {
        dimension: u8,
        cx: i32,
        cz: i32,
        revision: u64,
        min_section_y: i8,
        section_count: u16,
        blocks: Vec<u8>,
        block_states: Vec<u8>,
        block_entities: Vec<u8>,
    },
    BlockEntityDelta {
        dimension: u8,
        revision: u64,
        x: i32,
        y: i32,
        z: i32,
        entity: Option<crate::block_entity::BlockEntity>,
    },
    EntitySpawn {
        dimension: u8,
        sequence: u64,
        state: EntityStateWire,
    },
    EntityState {
        dimension: u8,
        sequence: u64,
        state: EntityStateWire,
    },
    EntityDespawn {
        dimension: u8,
        sequence: u64,
        entity_id: u64,
    },
    PlayerHealth {
        sequence: u64,
        player_id: PlayerId,
        health: f32,
        max_health: f32,
        hunger: f32,
        saturation: f32,
        oxygen: f32,
        is_dead: bool,
        death_reason: u8,
    },
    PlayerEffect {
        sequence: u64,
        player_id: PlayerId,
        effects: Vec<PlayerEffectWire>,
    },
    TimeSync {
        ticks: u64,
        weather: u8,
        weather_remaining_ticks: f32,
    },
    LightningStrike(LightningStrike),
    Chat {
        sender: String,
        message: String,
    },
    StatusUpdate {
        message: String,
    },
    ContainerOpenResult {
        dimension: u8,
        success: bool,
        x: i32,
        y: i32,
        z: i32,
        slots: Vec<Option<crate::network::protocol::ItemWire>>,
        revision: u64,
    },
    ContainerClickResult {
        dimension: u8,
        success: bool,
        slot_index: u16,
        slot: Option<crate::network::protocol::ItemWire>,
        dragged: Option<crate::network::protocol::ItemWire>,
    },
    ContainerSlotUpdate {
        dimension: u8,
        revision: u64,
        x: i32,
        y: i32,
        z: i32,
        slot_index: u16,
        slot: Option<crate::network::protocol::ItemWire>,
    },
    PlayerRespawnResult {
        position: [f32; 3],
        dimension: u8,
    },
    SleepStateSync {
        player_id: PlayerId,
        is_sleeping: bool,
    },
}

#[derive(Debug)]
pub enum GameToClient {
    SendPosition {
        sequence: u32,
        sender_time_millis: u64,
        x: f32,
        y: f32,
        z: f32,
        yaw: f32,
        pitch: f32,
    },
    SendAction {
        action: Action,
    },
    RequestBlockChange {
        x: i32,
        y: i32,
        z: i32,
        block: u32,
    },
    RequestBlockAction {
        action: Action,
        x: i32,
        y: i32,
        z: i32,
        block: u32,
        held_item: Option<crate::network::protocol::ItemWire>,
    },
    SendChat {
        message: String,
    },
    Disconnect,
    ContainerOpenRequest {
        dimension: u8,
        x: i32,
        y: i32,
        z: i32,
    },
    ContainerClickRequest {
        dimension: u8,
        slot_index: u16,
        is_left: bool,
        dragged: Option<crate::network::protocol::ItemWire>,
    },
    ContainerClose {
        dimension: u8,
        x: i32,
        y: i32,
        z: i32,
    },
    PlayerRespawnRequest,
    SleepRequest {
        x: i32,
        y: i32,
        z: i32,
    },
}

pub struct NetworkClient;

#[derive(Debug)]
struct BufferedBlockChange {
    x: i32,
    y: i32,
    z: i32,
    block: u32,
    state: u8,
}

#[derive(Default)]
struct RevisionGate {
    applied: HashMap<(u8, i32, i32), u64>,
    buffered: HashMap<(u8, i32, i32), BTreeMap<u64, BufferedBlockChange>>,
}

#[derive(Default)]
struct ReplicationGate {
    entity_sequences: HashMap<u64, u64>,
    health_sequences: HashMap<PlayerId, u64>,
    effect_sequences: HashMap<PlayerId, u64>,
}

impl ReplicationGate {
    fn accept_entity(&mut self, entity_id: u64, sequence: u64) -> bool {
        let latest = self.entity_sequences.entry(entity_id).or_default();
        if sequence <= *latest {
            return false;
        }
        *latest = sequence;
        true
    }

    fn accept_health(&mut self, player_id: PlayerId, sequence: u64) -> bool {
        let latest = self.health_sequences.entry(player_id).or_default();
        if sequence <= *latest {
            return false;
        }
        *latest = sequence;
        true
    }

    fn accept_effect(&mut self, player_id: PlayerId, sequence: u64) -> bool {
        let latest = self.effect_sequences.entry(player_id).or_default();
        if sequence <= *latest {
            return false;
        }
        *latest = sequence;
        true
    }
}

impl RevisionGate {
    fn accept_block_change(
        &mut self,
        dimension: u8,
        revision: u64,
        x: i32,
        y: i32,
        z: i32,
        block: u32,
        state: u8,
    ) -> Vec<ClientToGame> {
        let key = (dimension, x.div_euclid(16), z.div_euclid(16));
        let current = self.applied.get(&key).copied().unwrap_or(0);
        if revision <= current {
            return Vec::new();
        }
        self.buffered.entry(key).or_default().insert(
            revision,
            BufferedBlockChange {
                x,
                y,
                z,
                block,
                state,
            },
        );
        self.flush_contiguous(key)
    }

    fn accept_snapshot(
        &mut self,
        dimension: u8,
        cx: i32,
        cz: i32,
        revision: u64,
        min_section_y: i8,
        section_count: u16,
        blocks: Vec<u8>,
        block_states: Vec<u8>,
        block_entities: Vec<u8>,
    ) -> Vec<ClientToGame> {
        let key = (dimension, cx, cz);
        let current = self.applied.get(&key).copied().unwrap_or(0);
        if revision < current {
            return Vec::new();
        }
        self.applied.insert(key, revision);
        if let Some(changes) = self.buffered.get_mut(&key) {
            changes.retain(|buffered_revision, _| *buffered_revision > revision);
        }
        let mut events = vec![ClientToGame::ChunkData {
            dimension,
            cx,
            cz,
            revision,
            min_section_y,
            section_count,
            blocks,
            block_states,
            block_entities,
        }];
        events.extend(self.flush_contiguous(key));
        events
    }

    fn flush_contiguous(&mut self, key: (u8, i32, i32)) -> Vec<ClientToGame> {
        let mut events = Vec::new();
        loop {
            let next_revision = self
                .applied
                .get(&key)
                .copied()
                .unwrap_or(0)
                .saturating_add(1);
            let change = self
                .buffered
                .get_mut(&key)
                .and_then(|changes| changes.remove(&next_revision));
            let Some(change) = change else {
                break;
            };
            self.applied.insert(key, next_revision);
            events.push(ClientToGame::BlockChange {
                dimension: key.0,
                revision: next_revision,
                x: change.x,
                y: change.y,
                z: change.z,
                block: change.block,
                state: change.state,
            });
        }
        if self.buffered.get(&key).is_some_and(BTreeMap::is_empty) {
            self.buffered.remove(&key);
        }
        events
    }
}

fn authoritative_weather_event(packet: &Packet) -> Option<ClientToGame> {
    match packet {
        Packet::TimeSync {
            ticks,
            weather,
            weather_remaining_ticks,
            ..
        } => Some(ClientToGame::TimeSync {
            ticks: *ticks,
            weather: *weather,
            weather_remaining_ticks: *weather_remaining_ticks,
        }),
        Packet::LightningStrike { strike, .. } => Some(ClientToGame::LightningStrike(*strike)),
        _ => None,
    }
}

impl NetworkClient {
    pub fn spawn(
        server_addr: String,
        username: String,
        game_to_client: Receiver<GameToClient>,
        client_to_game: Sender<ClientToGame>,
    ) -> JoinHandle<()> {
        std::thread::spawn(move || {
            let client_to_game = ClientEventSender(client_to_game);
            let runtime = match tokio::runtime::Runtime::new() {
                Ok(runtime) => runtime,
                Err(error) => {
                    let _ = client_to_game.send(ClientToGame::Disconnected {
                        reason: format!("failed to create network runtime: {error}"),
                    });
                    return;
                }
            };
            runtime.block_on(run_client(
                server_addr,
                username,
                game_to_client,
                client_to_game,
            ));
        })
    }
}

async fn run_client(
    server_addr: String,
    username: String,
    game_to_client: Receiver<GameToClient>,
    client_to_game: ClientEventSender,
) {
    eprintln!("[NetworkClient] Connecting to {server_addr}...");
    let _ = client_to_game.send(ClientToGame::StatusUpdate {
        message: format!("CONNECTING TO {server_addr}..."),
    });

    let deadline = Instant::now() + Duration::from_secs(3);
    let stream = loop {
        match TcpStream::connect(&server_addr).await {
            Ok(stream) => break stream,
            Err(error) if Instant::now() < deadline => {
                time::sleep(Duration::from_millis(20)).await;
                let _ = error;
            }
            Err(error) => {
                let reason = format!("connection failed: {error}");
                eprintln!("[NetworkClient] Connection failed: {error}");
                let _ = client_to_game.send(ClientToGame::Disconnected { reason });
                return;
            }
        }
    };

    eprintln!("[NetworkClient] TCP connection established to {server_addr}");
    let _ = client_to_game.send(ClientToGame::StatusUpdate {
        message: "TCP CONNECTED. HANDSHAKING...".into(),
    });

    let mut connection = Connection::new(stream);
    eprintln!("[NetworkClient] Sent Handshake (user: {username}, v{PROTOCOL_VERSION})");
    let _ = client_to_game.send(ClientToGame::StatusUpdate {
        message: "HANDSHAKE SENT. WAITING FOR SERVER...".into(),
    });

    if let Err(error) = connection
        .send(&Packet::Handshake {
            protocol_version: PROTOCOL_VERSION,
            username: username.clone(),
        })
        .await
    {
        let reason = error.to_string();
        eprintln!("[NetworkClient] Handshake send error: {reason}");
        let _ = client_to_game.send(ClientToGame::Disconnected { reason });
        return;
    }

    let player_id = match time::timeout(Duration::from_secs(5), connection.recv()).await {
        Ok(Ok(Packet::LoginSuccess {
            protocol_version,
            player_id,
            seed,
            gamemode,
        })) if protocol_version == PROTOCOL_VERSION => {
            eprintln!("[NetworkClient] Login success! Assigned Player ID: {player_id}, Seed: {seed}, Gamemode: {gamemode}");
            let _ = client_to_game.send(ClientToGame::StatusUpdate {
                message: "LOGIN SUCCESS. LOADING WORLD...".into(),
            });
            let _ = client_to_game.send(ClientToGame::Connected {
                player_id,
                seed,
                gamemode,
            });
            player_id
        }
        Ok(Ok(Packet::Disconnect { reason, .. })) => {
            eprintln!("[NetworkClient] Server disconnected during login: {reason}");
            let _ = client_to_game.send(ClientToGame::Disconnected { reason });
            return;
        }
        Ok(Ok(packet)) => {
            let reason = format!("unexpected handshake response: {packet:?}");
            eprintln!("[NetworkClient] {reason}");
            let _ = client_to_game.send(ClientToGame::Disconnected { reason });
            return;
        }
        Ok(Err(error)) => {
            let reason = error.to_string();
            eprintln!("[NetworkClient] Connection recv error: {reason}");
            let _ = client_to_game.send(ClientToGame::Disconnected { reason });
            return;
        }
        Err(_) => {
            let reason = "login timed out".to_string();
            eprintln!("[NetworkClient] Login timed out after 5s");
            let _ = client_to_game.send(ClientToGame::Disconnected { reason });
            return;
        }
    };

    let (mut reader, mut writer) = connection.into_split();
    let mut tick = time::interval(Duration::from_millis(10));
    let mut revision_gate = RevisionGate::default();
    let mut replication_gate = ReplicationGate::default();
    loop {
        tokio::select! {
            incoming = reader.recv() => {
                match incoming {
                    Ok(packet) if packet.protocol_version() != PROTOCOL_VERSION => {
                        eprintln!("[NetworkClient] Disconnecting: protocol version mismatch");
                        let _ = client_to_game.send(ClientToGame::Disconnected { reason: "protocol version mismatch".into() });
                        break;
                    }
                    Ok(Packet::PlayerJoin { id, username, .. }) => { let _ = client_to_game.send(ClientToGame::PlayerJoin { id, username }); }
                    Ok(Packet::PlayerLeave { id, .. }) => { let _ = client_to_game.send(ClientToGame::PlayerLeave { id }); }
                    Ok(Packet::PlayerPosition { id, sequence, sender_time_millis, x, y, z, yaw, pitch, .. }) => {
                        let _ = client_to_game.send(ClientToGame::PlayerPosition {
                            id, sequence, sender_time_millis, x, y, z, yaw, pitch,
                        });
                    }
                    Ok(Packet::PlayerAction { id, action, .. }) => { let _ = client_to_game.send(ClientToGame::PlayerAction { id, action }); }
                    Ok(Packet::BlockChange {
                        dimension,
                        revision,
                        x,
                        y,
                        z,
                        block,
                        state,
                        ..
                    }) => {
                        for event in revision_gate.accept_block_change(
                            dimension, revision, x, y, z, block, state,
                        ) {
                            let _ = client_to_game.send(event);
                        }
                    }
                    Ok(Packet::BlockEntityDelta {
                        dimension,
                        revision,
                        x,
                        y,
                        z,
                        entity,
                        ..
                    }) => {
                        let _ = client_to_game.send(ClientToGame::BlockEntityDelta {
                            dimension,
                            revision,
                            x,
                            y,
                            z,
                            entity,
                        });
                    }
                    Ok(Packet::BlockActionResult { x, y, z, success, consumed_item, drops, .. }) => {
                        let _ = client_to_game.send(ClientToGame::BlockActionResult { x, y, z, success, consumed_item, drops });
                    }
                    Ok(Packet::ContainerOpenResult { dimension, success, x, y, z, slots, revision, .. }) => {
                        let _ = client_to_game.send(ClientToGame::ContainerOpenResult { dimension, success, x, y, z, slots, revision });
                    }
                    Ok(Packet::ContainerClickResult { dimension, success, slot_index, slot, dragged, .. }) => {
                        let _ = client_to_game.send(ClientToGame::ContainerClickResult { dimension, success, slot_index, slot, dragged });
                    }
                    Ok(Packet::PlayerRespawnResult { position, dimension, .. }) => {
                        let _ = client_to_game.send(ClientToGame::PlayerRespawnResult { position, dimension });
                    }
                    Ok(Packet::SleepStateSync { player_id, is_sleeping, .. }) => {
                        let _ = client_to_game.send(ClientToGame::SleepStateSync { player_id, is_sleeping });
                    }
                    Ok(Packet::ContainerSlotUpdate { dimension, revision, x, y, z, slot_index, slot, .. }) => {
                        let _ = client_to_game.send(ClientToGame::ContainerSlotUpdate { dimension, revision, x, y, z, slot_index, slot });
                    }
                    Ok(Packet::ChunkData {
                        dimension,
                        cx,
                        cz,
                        revision,
                        min_section_y,
                        section_count,
                        blocks,
                        block_states,
                        block_entities,
                        ..
                    }) => {
                        for event in revision_gate.accept_snapshot(
                            dimension,
                            cx,
                            cz,
                            revision,
                            min_section_y,
                            section_count,
                            blocks,
                            block_states,
                            block_entities,
                        ) {
                            let _ = client_to_game.send(event);
                        }
                        if writer
                            .send(&Packet::ChunkAck {
                                protocol_version: PROTOCOL_VERSION,
                                dimension,
                                cx,
                                cz,
                                revision,
                            })
                            .await
                            .is_err()
                        {
                            let _ = client_to_game.send(ClientToGame::Disconnected {
                                reason: "connection lost".into(),
                            });
                            break;
                        }
                    }
                    Ok(Packet::EntitySpawn {
                        dimension,
                        sequence,
                        state,
                        ..
                    }) => {
                        if replication_gate.accept_entity(state.entity_id, sequence) {
                            let _ = client_to_game.send(ClientToGame::EntitySpawn {
                                dimension,
                                sequence,
                                state,
                            });
                        }
                    }
                    Ok(Packet::EntityState {
                        dimension,
                        sequence,
                        state,
                        ..
                    }) => {
                        if replication_gate.accept_entity(state.entity_id, sequence) {
                            let _ = client_to_game.send(ClientToGame::EntityState {
                                dimension,
                                sequence,
                                state,
                            });
                        }
                    }
                    Ok(Packet::EntityDespawn {
                        dimension,
                        sequence,
                        entity_id,
                        ..
                    }) => {
                        if replication_gate.accept_entity(entity_id, sequence) {
                            let _ = client_to_game.send(ClientToGame::EntityDespawn {
                                dimension,
                                sequence,
                                entity_id,
                            });
                        }
                    }
                    Ok(Packet::PlayerHealth {
                        sequence,
                        player_id,
                        health,
                        max_health,
                        hunger,
                        saturation,
                        oxygen,
                        is_dead,
                        death_reason,
                        ..
                    }) => {
                        if replication_gate.accept_health(player_id, sequence) {
                            let _ = client_to_game.send(ClientToGame::PlayerHealth {
                                sequence,
                                player_id,
                                health,
                                max_health,
                                hunger,
                                saturation,
                                oxygen,
                                is_dead,
                                death_reason,
                            });
                        }
                    }
                    Ok(Packet::PlayerEffect {
                        sequence,
                        player_id,
                        effects,
                        ..
                    }) => {
                        if replication_gate.accept_effect(player_id, sequence) {
                            let _ = client_to_game.send(ClientToGame::PlayerEffect {
                                sequence,
                                player_id,
                                effects,
                            });
                        }
                    }
                    Ok(packet @ Packet::TimeSync { .. })
                    | Ok(packet @ Packet::LightningStrike { .. }) => {
                        if let Some(event) = authoritative_weather_event(&packet) {
                            let _ = client_to_game.send(event);
                        }
                    }
                    Ok(Packet::ChatMessage { sender, message, .. }) => { let _ = client_to_game.send(ClientToGame::Chat { sender, message }); }
                    Ok(Packet::Keepalive { .. }) => {
                        if writer.send(&Packet::Keepalive { protocol_version: PROTOCOL_VERSION }).await.is_err() {
                            eprintln!("[NetworkClient] Disconnecting: failed to reply to keepalive");
                            let _ = client_to_game.send(ClientToGame::Disconnected { reason: "connection lost".into() });
                            break;
                        }
                    }
                    Ok(Packet::Disconnect { reason, .. }) => {
                        eprintln!("[NetworkClient] Disconnecting: server sent Disconnect: {reason}");
                        let _ = client_to_game.send(ClientToGame::Disconnected { reason });
                        break;
                    }
                    Ok(_) => {}
                    Err(error) => {
                        eprintln!("[NetworkClient] Disconnecting: reader recv error: {error}");
                        let _ = client_to_game.send(ClientToGame::Disconnected { reason: "connection lost".into() });
                        break;
                    }
                }
            }
            _ = tick.tick() => {
                let mut latest_position = None;
                loop {
                    // game -> client is the app-level outbound queue.  Position
                    // bursts are coalesced below, but each removed command is
                    // still dequeued exactly once from the raw channel.
                    match crate::perf::tracked_try_recv(
                        &game_to_client,
                        std::mem::size_of::<GameToClient>() as u64,
                        &crate::perf::queue_stats(crate::perf::QueueCategory::Outbound),
                    ) {
                        Ok(GameToClient::SendPosition {
                            sequence,
                            sender_time_millis,
                            x,
                            y,
                            z,
                            yaw,
                            pitch,
                        }) => latest_position = Some((
                            sequence,
                            sender_time_millis,
                            x,
                            y,
                            z,
                            yaw,
                            pitch,
                        )),
                        Ok(GameToClient::SendAction { action }) => {
                            if writer.send(&Packet::PlayerAction { protocol_version: PROTOCOL_VERSION, id: player_id, action }).await.is_err() {
                                eprintln!("[NetworkClient] Disconnecting: failed to send PlayerAction");
                                let _ = client_to_game.send(ClientToGame::Disconnected { reason: "connection lost".into() });
                                return;
                            }
                        }
                        Ok(GameToClient::RequestBlockChange { x, y, z, block }) => {
                            if writer.send(&Packet::BlockChange {
                                protocol_version: PROTOCOL_VERSION,
                                dimension: 0,
                                revision: 0,
                                x,
                                y,
                                z,
                                block,
                                state: 0,
                            }).await.is_err() {
                                eprintln!("[NetworkClient] Disconnecting: failed to send BlockChange");
                                let _ = client_to_game.send(ClientToGame::Disconnected { reason: "connection lost".into() });
                                return;
                            }
                        }
                        Ok(GameToClient::RequestBlockAction { action, x, y, z, block, held_item }) => {
                            if writer.send(&Packet::BlockActionRequest { protocol_version: PROTOCOL_VERSION, action, x, y, z, block, held_item }).await.is_err() {
                                eprintln!("[NetworkClient] Disconnecting: failed to send BlockActionRequest");
                                let _ = client_to_game.send(ClientToGame::Disconnected { reason: "connection lost".into() });
                                return;
                            }
                        }
                        Ok(GameToClient::SendChat { message }) => {
                            if writer.send(&Packet::ChatMessage { protocol_version: PROTOCOL_VERSION, sender: username.clone(), message }).await.is_err() {
                                eprintln!("[NetworkClient] Disconnecting: failed to send ChatMessage");
                                let _ = client_to_game.send(ClientToGame::Disconnected { reason: "connection lost".into() });
                                return;
                            }
                        }
                        Ok(GameToClient::ContainerOpenRequest { dimension, x, y, z }) => {
                            if writer.send(&Packet::ContainerOpenRequest { protocol_version: PROTOCOL_VERSION, dimension, x, y, z }).await.is_err() {
                                eprintln!("[NetworkClient] Disconnecting: failed to send ContainerOpenRequest");
                                let _ = client_to_game.send(ClientToGame::Disconnected { reason: "connection lost".into() });
                                return;
                            }
                        }
                        Ok(GameToClient::ContainerClickRequest { dimension, slot_index, is_left, dragged }) => {
                            if writer.send(&Packet::ContainerClickRequest { protocol_version: PROTOCOL_VERSION, dimension, slot_index, is_left, dragged }).await.is_err() {
                                eprintln!("[NetworkClient] Disconnecting: failed to send ContainerClickRequest");
                                let _ = client_to_game.send(ClientToGame::Disconnected { reason: "connection lost".into() });
                                return;
                            }
                        }
                        Ok(GameToClient::ContainerClose { dimension, x, y, z }) => {
                            if writer.send(&Packet::ContainerClose { protocol_version: PROTOCOL_VERSION, dimension, x, y, z }).await.is_err() {
                                eprintln!("[NetworkClient] Disconnecting: failed to send ContainerClose");
                                let _ = client_to_game.send(ClientToGame::Disconnected { reason: "connection lost".into() });
                                return;
                            }
                        }
                        Ok(GameToClient::PlayerRespawnRequest) => {
                            if writer.send(&Packet::PlayerRespawnRequest { protocol_version: PROTOCOL_VERSION }).await.is_err() {
                                eprintln!("[NetworkClient] Disconnecting: failed to send PlayerRespawnRequest");
                                let _ = client_to_game.send(ClientToGame::Disconnected { reason: "connection lost".into() });
                                return;
                            }
                        }
                        Ok(GameToClient::SleepRequest { x, y, z }) => {
                            if writer.send(&Packet::SleepRequest { protocol_version: PROTOCOL_VERSION, x, y, z }).await.is_err() {
                                eprintln!("[NetworkClient] Disconnecting: failed to send SleepRequest");
                                let _ = client_to_game.send(ClientToGame::Disconnected { reason: "connection lost".into() });
                                return;
                            }
                        }
                        Ok(GameToClient::Disconnect) => {
                            eprintln!("[NetworkClient] Disconnecting: game thread requested disconnect");
                            let _ = writer.send(&Packet::Disconnect { protocol_version: PROTOCOL_VERSION, reason: "client disconnect".into() }).await;
                            return;
                        }
                        Err(std::sync::mpsc::TryRecvError::Empty) => break,
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                            crate::perf::queue_stats(crate::perf::QueueCategory::Outbound).cancel();
                            eprintln!("[NetworkClient] Disconnecting: game_to_client channel closed (State dropped?)");
                            return;
                        }
                    }
                }
                if let Some((
                    sequence,
                    sender_time_millis,
                    x,
                    y,
                    z,
                    yaw,
                    pitch,
                )) = latest_position
                {
                    if writer
                        .send(&Packet::PlayerPosition {
                            protocol_version: PROTOCOL_VERSION,
                            id: player_id,
                            sequence,
                            sender_time_millis,
                            x,
                            y,
                            z,
                            yaw,
                            pitch,
                        })
                        .await
                        .is_err()
                    {
                        eprintln!("[NetworkClient] Disconnecting: failed to send PlayerPosition");
                        let _ = client_to_game.send(ClientToGame::Disconnected {
                            reason: "connection lost".into(),
                        });
                        return;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::server::{HostToServer, NetworkServer, ServerToHost};
    use std::net::TcpListener as StdTcpListener;
    use std::sync::{mpsc, Mutex, MutexGuard, OnceLock};

    fn network_test_guard() -> MutexGuard<'static, ()> {
        static GUARD: OnceLock<Mutex<()>> = OnceLock::new();
        GUARD
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn wait_for_event(rx: &Receiver<ClientToGame>) -> ClientToGame {
        loop {
            let event = rx
                .recv_timeout(Duration::from_secs(3))
                .expect("client event timed out");
            if !matches!(event, ClientToGame::StatusUpdate { .. }) {
                return event;
            }
        }
    }

    #[test]
    fn connects_and_receives_join_for_second_client() {
        let _guard = network_test_guard();
        let reserved = StdTcpListener::bind("127.0.0.1:0").unwrap();
        let addr = reserved.local_addr().unwrap().to_string();
        drop(reserved);
        let (host_tx, host_rx) = mpsc::channel();
        let (server_tx, server_rx) = mpsc::channel();
        let server = NetworkServer::spawn(addr.clone(), 0xCAFE_BABE, 1, host_rx, server_tx);

        let (game_tx_a, game_rx_a) = mpsc::channel();
        let (event_tx_a, event_rx_a) = mpsc::channel();
        let client_a = NetworkClient::spawn(addr.clone(), "steve".into(), game_rx_a, event_tx_a);
        let first = wait_for_event(&event_rx_a);
        let first_id = match first {
            ClientToGame::Connected {
                player_id,
                seed,
                gamemode,
            } => {
                assert_eq!(seed, 0xCAFE_BABE);
                assert_eq!(gamemode, 1);
                player_id
            }
            other => panic!("expected Connected, got {other:?}"),
        };
        let _ = server_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("first join missing");

        let (game_tx_b, game_rx_b) = mpsc::channel();
        let (event_tx_b, event_rx_b) = mpsc::channel();
        let client_b = NetworkClient::spawn(addr, "alex".into(), game_rx_b, event_tx_b);
        let second_id = match wait_for_event(&event_rx_b) {
            ClientToGame::Connected { player_id, .. } => player_id,
            other => panic!("expected second Connected, got {other:?}"),
        };
        let second_join = server_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("second join missing");
        let username = match second_join {
            ServerToHost::ClientJoined { id, username } => {
                assert_eq!(id, second_id);
                username
            }
            other => panic!("expected ClientJoined, got {other:?}"),
        };
        host_tx
            .send(HostToServer::NotifyPlayerJoin {
                id: second_id,
                username,
            })
            .unwrap();
        assert!(matches!(
            wait_for_event(&event_rx_a),
            ClientToGame::PlayerJoin { id, username } if id == second_id && username == "alex"
        ));

        game_tx_a.send(GameToClient::Disconnect).unwrap();
        game_tx_b.send(GameToClient::Disconnect).unwrap();
        let _ = client_a.join();
        let _ = client_b.join();
        host_tx.send(HostToServer::Stop).unwrap();
        let _ = server.join();
        assert_ne!(first_id, second_id);
    }

    #[test]
    fn receives_targeted_chunk_catchup_and_time_sync() {
        let _guard = network_test_guard();
        let reserved = StdTcpListener::bind("127.0.0.1:0").unwrap();
        let addr = reserved.local_addr().unwrap().to_string();
        drop(reserved);
        let (host_tx, host_rx) = mpsc::channel();
        let (server_tx, server_rx) = mpsc::channel();
        let server = NetworkServer::spawn(addr.clone(), 1234, 1, host_rx, server_tx);

        let (game_tx, game_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        let client = NetworkClient::spawn(addr, "catchup".into(), game_rx, event_tx);
        let player_id = match wait_for_event(&event_rx) {
            ClientToGame::Connected { player_id, .. } => player_id,
            other => panic!("expected Connected, got {other:?}"),
        };
        let _ = server_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("join event missing");

        host_tx
            .send(HostToServer::BroadcastBlockChange {
                dimension: 0,
                revision: 1,
                x: 7,
                y: 80,
                z: -9,
                block: 3,
                state: 0,
            })
            .unwrap();
        host_tx
            .send(HostToServer::SendChunk {
                dimension: 0,
                cx: -2,
                cz: 5,
                revision: 1,
                min_section_y: 0,
                section_count: 16,
                blocks: vec![1, 2, 3, 4],
                block_states: vec![0, 0, 0, 0],
                block_entities: vec![],
                to: player_id,
            })
            .unwrap();
        host_tx
            .send(HostToServer::BroadcastTimeSync {
                ticks: 19_000,
                weather: 2,
                weather_remaining_ticks: 8_000.5,
            })
            .unwrap();
        let strike = LightningStrike {
            x: -7,
            y: 90,
            z: 13,
            visual_seed: 77,
        };
        host_tx
            .send(HostToServer::BroadcastLightningStrike { strike })
            .unwrap();

        let events = [
            wait_for_event(&event_rx),
            wait_for_event(&event_rx),
            wait_for_event(&event_rx),
            wait_for_event(&event_rx),
        ];

        assert!(events.iter().any(|e| matches!(
            e,
            ClientToGame::BlockChange {
                x: 7,
                y: 80,
                z: -9,
                block: 3,
                state: 0,
                ..
            }
        )));
        assert!(events.iter().any(|e| matches!(
            e,
            ClientToGame::ChunkData { cx: -2, cz: 5, blocks, block_states: _, .. } if blocks == &vec![1, 2, 3, 4]
        )));
        assert!(events.iter().any(|e| matches!(
            e,
            ClientToGame::TimeSync {
                ticks: 19_000,
                weather: 2,
                weather_remaining_ticks: 8_000.5,
            }
        )));
        assert!(events.iter().any(|e| matches!(
            e,
            ClientToGame::LightningStrike(received) if *received == strike
        )));

        let mut accepted = false;
        let mut acknowledged = false;
        for _ in 0..4 {
            match server_rx.recv_timeout(Duration::from_secs(3)).unwrap() {
                ServerToHost::CatchupAccepted {
                    id,
                    dimension: 0,
                    cx: -2,
                    cz: 5,
                    revision: 1,
                } if id == player_id => accepted = true,
                ServerToHost::CatchupAck {
                    id,
                    dimension: 0,
                    cx: -2,
                    cz: 5,
                    revision: 1,
                } if id == player_id => acknowledged = true,
                _ => {}
            }
            if accepted && acknowledged {
                break;
            }
        }
        assert!(accepted, "server did not accept the catch-up transfer");
        assert!(acknowledged, "client did not ACK the catch-up transfer");

        game_tx.send(GameToClient::Disconnect).unwrap();
        client.join().unwrap();
        host_tx.send(HostToServer::Stop).unwrap();
        server.join().unwrap();
    }

    #[test]
    fn tcp_capacity_one_retries_without_starving_second_client_and_converges() {
        let _guard = network_test_guard();
        fn checksum(chunk: &crate::world::Chunk) -> u64 {
            let mut hash = 0xcbf2_9ce4_8422_2325u64;
            for x in 0..16 {
                for y in 0..256 {
                    for z in 0..16 {
                        hash ^= chunk.get_block_local(x, y, z) as u8 as u64;
                        hash = hash.wrapping_mul(0x100_0000_01b3);
                    }
                }
            }
            hash
        }

        fn receive_converged_chunk(
            rx: &Receiver<ClientToGame>,
            expected_payload: &crate::save::ChunkSaveData,
        ) -> crate::world::Chunk {
            let (blocks, block_states) = loop {
                match wait_for_event(rx) {
                    ClientToGame::ChunkData {
                        dimension: 0,
                        cx: 0,
                        cz: 0,
                        revision: 1,
                        blocks,
                        block_states,
                        block_entities: _,
                        ..
                    } => break (blocks, block_states),
                    ClientToGame::PlayerJoin { .. } => {}
                    other => panic!("expected persisted chunk snapshot, got {other:?}"),
                }
            };
            assert_eq!(blocks, expected_payload.blocks);
            assert_eq!(block_states, expected_payload.block_states);

            let mut chunk = crate::world::Chunk::new(0, 0);
            crate::save::ChunkSaveData {
                chunk_x: 0,
                chunk_z: 0,
                blocks,
                sky_light: Vec::new(),
                block_light: Vec::new(),
                fluid_levels: Vec::new(),
                redstone_metadata: Vec::new(),
                block_states,
                mutation_revision: 1,
                block_entities: Vec::new(),
                data_version: 0,
            }
            .restore_to_chunk(&mut chunk);

            match wait_for_event(rx) {
                ClientToGame::BlockChange {
                    dimension: 0,
                    revision: 2,
                    x: 2,
                    y: 70,
                    z: 2,
                    block,
                    ..
                } => chunk.set_block_local(
                    2,
                    70,
                    2,
                    crate::world::BlockType::from_wire(block).unwrap(),
                ),
                other => panic!("expected revision-2 block change, got {other:?}"),
            }
            chunk
        }

        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let world_dir = std::env::temp_dir().join(format!(
            "icraft_network_persisted_catchup_{}_{}",
            std::process::id(),
            unique
        ));
        let mut source_chunk = crate::world::Chunk::new(0, 0);
        source_chunk.set_block_local(1, 70, 1, crate::world::BlockType::Stone);
        let mut persisted = crate::save::ChunkSaveData::from_chunk(&source_chunk);
        persisted.mutation_revision = 1;
        crate::save::SaveManager::new(&world_dir)
            .save_chunk(0, 0, persisted)
            .unwrap();
        let persisted = crate::save::SaveManager::new(&world_dir)
            .load_chunk(0, 0)
            .expect("persisted, currently-unloaded chunk must be reloadable");

        let reserved = StdTcpListener::bind("127.0.0.1:0").unwrap();
        let addr = reserved.local_addr().unwrap().to_string();
        drop(reserved);
        let (host_tx, host_rx) = mpsc::channel();
        let (server_tx, server_rx) = mpsc::channel();
        let server = NetworkServer::spawn_for_test(
            addr.clone(),
            1234,
            1,
            host_rx,
            server_tx,
            1,
            Duration::from_millis(200),
        );

        let (game_tx_a, game_rx_a) = mpsc::channel();
        let (event_tx_a, event_rx_a) = mpsc::channel();
        let client_a = NetworkClient::spawn(addr.clone(), "slow".into(), game_rx_a, event_tx_a);
        let id_a = match wait_for_event(&event_rx_a) {
            ClientToGame::Connected { player_id, .. } => player_id,
            other => panic!("expected first Connected, got {other:?}"),
        };
        assert!(matches!(
            server_rx.recv_timeout(Duration::from_secs(3)).unwrap(),
            ServerToHost::ClientJoined { id, .. } if id == id_a
        ));

        let (game_tx_b, game_rx_b) = mpsc::channel();
        let (event_tx_b, event_rx_b) = mpsc::channel();
        let client_b = NetworkClient::spawn(addr, "fast".into(), game_rx_b, event_tx_b);
        let id_b = match wait_for_event(&event_rx_b) {
            ClientToGame::Connected { player_id, .. } => player_id,
            other => panic!("expected second Connected, got {other:?}"),
        };
        assert!(matches!(
            server_rx.recv_timeout(Duration::from_secs(3)).unwrap(),
            ServerToHost::ClientJoined { id, .. } if id == id_b
        ));

        host_tx
            .send(HostToServer::BroadcastBlockChange {
                dimension: 0,
                revision: 2,
                x: 2,
                y: 70,
                z: 2,
                block: crate::world::BlockType::Dirt.to_wire(),
                state: 0,
            })
            .unwrap();
        let snapshot = |to, cx, blocks, block_states| HostToServer::SendChunk {
            dimension: 0,
            cx,
            cz: 0,
            revision: 1,
            min_section_y: 0,
            section_count: 16,
            blocks,
            block_states,
            block_entities: vec![],
            to,
        };
        // The host/state priority selector submits the near chunk first. The
        // transport must preserve it while reporting, rather than dropping,
        // the farther chunk when this client's capacity-one mailbox is full.
        host_tx
            .send(snapshot(
                id_a,
                0,
                persisted.blocks.clone(),
                persisted.block_states.clone(),
            ))
            .unwrap();
        host_tx.send(snapshot(id_a, 8, vec![8], vec![0])).unwrap();
        host_tx
            .send(snapshot(
                id_b,
                0,
                persisted.blocks.clone(),
                persisted.block_states.clone(),
            ))
            .unwrap();

        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        let mut accepted_a = false;
        let mut backpressured_a = false;
        let mut accepted_b = false;
        while std::time::Instant::now() < deadline && !(accepted_a && backpressured_a && accepted_b)
        {
            match server_rx.recv_timeout(Duration::from_millis(250)) {
                Ok(ServerToHost::CatchupAccepted {
                    id,
                    cx: 0,
                    revision: 1,
                    ..
                }) if id == id_a => accepted_a = true,
                Ok(ServerToHost::CatchupBackpressured {
                    id,
                    cx: 8,
                    revision: 1,
                    mailbox_full_count,
                    ..
                }) if id == id_a => {
                    assert_eq!(mailbox_full_count, 1);
                    backpressured_a = true;
                }
                Ok(ServerToHost::CatchupAccepted {
                    id,
                    cx: 0,
                    revision: 1,
                    ..
                }) if id == id_b => accepted_b = true,
                Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(error) => panic!("server event channel failed: {error}"),
            }
        }
        assert!(accepted_a && backpressured_a && accepted_b);

        let converged_a = receive_converged_chunk(&event_rx_a, &persisted);
        let converged_b = receive_converged_chunk(&event_rx_b, &persisted);
        let mut expected = source_chunk;
        expected.set_block_local(2, 70, 2, crate::world::BlockType::Dirt);
        assert_eq!(checksum(&converged_a), checksum(&expected));
        assert_eq!(checksum(&converged_b), checksum(&expected));

        let mut ack_a = false;
        let mut ack_b = false;
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while std::time::Instant::now() < deadline && !(ack_a && ack_b) {
            match server_rx.recv_timeout(Duration::from_millis(250)) {
                Ok(ServerToHost::CatchupAck { id, cx: 0, .. }) if id == id_a => ack_a = true,
                Ok(ServerToHost::CatchupAck { id, cx: 0, .. }) if id == id_b => ack_b = true,
                Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(error) => panic!("server event channel failed: {error}"),
            }
        }
        assert!(
            ack_a && ack_b,
            "both TCP clients must ACK accepted snapshots"
        );

        host_tx.send(snapshot(id_a, 8, vec![8], vec![0])).unwrap();
        assert!(matches!(
            wait_for_event(&event_rx_a),
            ClientToGame::ChunkData {
                cx: 8,
                revision: 1,
                blocks,
                ..
            } if blocks == vec![8]
        ));
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        let mut retry_accepted = false;
        let mut retry_acked = false;
        while std::time::Instant::now() < deadline && !(retry_accepted && retry_acked) {
            match server_rx.recv_timeout(Duration::from_millis(250)) {
                Ok(ServerToHost::CatchupAccepted { id, cx: 8, .. }) if id == id_a => {
                    retry_accepted = true
                }
                Ok(ServerToHost::CatchupAck { id, cx: 8, .. }) if id == id_a => retry_acked = true,
                Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(error) => panic!("server event channel failed: {error}"),
            }
        }
        assert!(retry_accepted && retry_acked);

        game_tx_a.send(GameToClient::Disconnect).unwrap();
        game_tx_b.send(GameToClient::Disconnect).unwrap();
        client_a.join().unwrap();
        client_b.join().unwrap();
        host_tx.send(HostToServer::Stop).unwrap();
        server.join().unwrap();
        std::fs::remove_dir_all(world_dir).unwrap();
    }

    #[test]
    fn tcp_cross_channel_revision_gate_and_reliable_control_are_fifo() {
        let _guard = network_test_guard();
        let reserved = StdTcpListener::bind("127.0.0.1:0").unwrap();
        let addr = reserved.local_addr().unwrap().to_string();
        drop(reserved);
        let (host_tx, host_rx) = mpsc::channel();
        let (server_tx, server_rx) = mpsc::channel();
        let server = NetworkServer::spawn(addr.clone(), 1234, 1, host_rx, server_tx);
        let (game_tx, game_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        let client = NetworkClient::spawn(addr, "ordering".into(), game_rx, event_tx);
        let player_id = match wait_for_event(&event_rx) {
            ClientToGame::Connected { player_id, .. } => player_id,
            other => panic!("expected Connected, got {other:?}"),
        };
        assert!(matches!(
            server_rx.recv_timeout(Duration::from_secs(3)).unwrap(),
            ServerToHost::ClientJoined { id, .. } if id == player_id
        ));

        host_tx
            .send(HostToServer::BroadcastBlockChange {
                dimension: 0,
                revision: 2,
                x: 1,
                y: 70,
                z: 1,
                block: crate::world::BlockType::Dirt.to_wire(),
                state: 0,
            })
            .unwrap();
        host_tx
            .send(HostToServer::BroadcastChat {
                sender: "host".into(),
                message: "first".into(),
            })
            .unwrap();
        host_tx
            .send(HostToServer::BroadcastTimeSync {
                ticks: 42,
                weather: 1,
                weather_remaining_ticks: 99.0,
            })
            .unwrap();
        host_tx
            .send(HostToServer::BroadcastChat {
                sender: "host".into(),
                message: "second".into(),
            })
            .unwrap();
        host_tx
            .send(HostToServer::SendChunk {
                dimension: 0,
                cx: 0,
                cz: 0,
                revision: 1,
                min_section_y: 0,
                section_count: 16,
                blocks: vec![1],
                block_states: vec![0],
                block_entities: vec![],
                to: player_id,
            })
            .unwrap();

        assert!(matches!(
            wait_for_event(&event_rx),
            ClientToGame::Chat { message, .. } if message == "first"
        ));
        assert!(matches!(
            wait_for_event(&event_rx),
            ClientToGame::TimeSync { ticks: 42, .. }
        ));
        assert!(matches!(
            wait_for_event(&event_rx),
            ClientToGame::Chat { message, .. } if message == "second"
        ));
        assert!(matches!(
            wait_for_event(&event_rx),
            ClientToGame::ChunkData { revision: 1, .. }
        ));
        assert!(matches!(
            wait_for_event(&event_rx),
            ClientToGame::BlockChange { revision: 2, .. }
        ));

        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        let mut accepted = false;
        let mut acked = false;
        while std::time::Instant::now() < deadline && !(accepted && acked) {
            match server_rx.recv_timeout(Duration::from_millis(250)) {
                Ok(ServerToHost::CatchupAccepted { id, cx: 0, .. }) if id == player_id => {
                    accepted = true
                }
                Ok(ServerToHost::CatchupAck { id, cx: 0, .. }) if id == player_id => acked = true,
                Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(error) => panic!("server event channel failed: {error}"),
            }
        }
        assert!(accepted && acked);

        game_tx.send(GameToClient::Disconnect).unwrap();
        client.join().unwrap();
        host_tx.send(HostToServer::Stop).unwrap();
        server.join().unwrap();
    }

    #[test]
    fn authoritative_weather_packets_map_to_pure_client_events() {
        let sync = Packet::TimeSync {
            protocol_version: PROTOCOL_VERSION,
            ticks: 12_345,
            weather: 1,
            weather_remaining_ticks: 6_789.5,
        };
        assert!(matches!(
            authoritative_weather_event(&sync),
            Some(ClientToGame::TimeSync {
                ticks: 12_345,
                weather: 1,
                weather_remaining_ticks: 6_789.5,
            })
        ));

        let strike = LightningStrike {
            x: 3,
            y: 72,
            z: -9,
            visual_seed: 123,
        };
        assert!(matches!(
            authoritative_weather_event(&Packet::LightningStrike {
                protocol_version: PROTOCOL_VERSION,
                strike,
            }),
            Some(ClientToGame::LightningStrike(received)) if received == strike
        ));
    }

    #[test]
    fn sends_and_receives_chat() {
        let _guard = network_test_guard();
        let reserved = StdTcpListener::bind("127.0.0.1:0").unwrap();
        let addr = reserved.local_addr().unwrap().to_string();
        drop(reserved);
        let (host_tx, host_rx) = mpsc::channel();
        let (server_tx, server_rx) = mpsc::channel();
        let server = NetworkServer::spawn(addr.clone(), 0xCAFE_BABE, 1, host_rx, server_tx);

        let (game_tx, game_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        let client = NetworkClient::spawn(addr, "steve".into(), game_rx, event_tx);
        let player_id = match wait_for_event(&event_rx) {
            ClientToGame::Connected { player_id, .. } => player_id,
            other => panic!("expected Connected, got {other:?}"),
        };
        assert!(matches!(
            server_rx.recv_timeout(Duration::from_secs(3)).unwrap(),
            ServerToHost::ClientJoined { id, username }
                if id == player_id && username == "steve"
        ));

        game_tx
            .send(GameToClient::SendChat {
                message: "hello".into(),
            })
            .unwrap();
        assert!(matches!(
            server_rx.recv_timeout(Duration::from_secs(3)).unwrap(),
            ServerToHost::ChatFromClient { id, message }
                if id == player_id && message == "hello"
        ));

        host_tx
            .send(HostToServer::BroadcastChat {
                sender: "steve".into(),
                message: "hello".into(),
            })
            .unwrap();
        assert!(matches!(
            wait_for_event(&event_rx),
            ClientToGame::Chat { sender, message }
                if sender == "steve" && message == "hello"
        ));

        game_tx.send(GameToClient::Disconnect).unwrap();
        client.join().unwrap();
        host_tx.send(HostToServer::Stop).unwrap();
        server.join().unwrap();
    }

    /// Step 2 (Task 5) two-instance smoke test: when the host stops the server,
    /// the remaining client observes a `Disconnected` event and its background
    /// thread exits cleanly without hanging. This automates the "quitting either
    /// side cleans up the background thread without hanging" requirement that
    /// the two-window GUI scenario checks manually.
    #[test]
    fn host_stop_notifies_client_and_threads_join_without_hanging() {
        let _guard = network_test_guard();
        let reserved = StdTcpListener::bind("127.0.0.1:0").unwrap();
        let addr = reserved.local_addr().unwrap().to_string();
        drop(reserved);
        let (host_tx, host_rx) = mpsc::channel();
        let (server_tx, server_rx) = mpsc::channel();
        let server = NetworkServer::spawn(addr.clone(), 0xDEAD_BEEF, 0, host_rx, server_tx);

        let (_game_tx, game_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        let client = NetworkClient::spawn(addr, "host_quit_witness".into(), game_rx, event_tx);

        match wait_for_event(&event_rx) {
            ClientToGame::Connected { seed, gamemode, .. } => {
                assert_eq!(seed, 0xDEAD_BEEF);
                assert_eq!(gamemode, 0);
            }
            other => panic!("expected Connected, got {other:?}"),
        }
        let _ = server_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("join event missing");

        // Host quits: stop the server. The client must be notified and exit.
        host_tx.send(HostToServer::Stop).unwrap();
        match event_rx.recv_timeout(Duration::from_secs(3)) {
            Ok(ClientToGame::Disconnected { .. }) => {}
            Ok(other) => panic!("expected Disconnected, got {other:?}"),
            Err(_) => panic!("client did not observe disconnect after host stop"),
        }
        client
            .join()
            .expect("client thread panicked during host-stop shutdown");
        server
            .join()
            .expect("server thread panicked during shutdown");
    }

    #[test]
    fn revision_gate_orders_cross_channel_snapshot_and_block_change() {
        let mut gate = RevisionGate::default();
        assert!(gate.accept_block_change(0, 2, 1, 70, 1, 4, 0).is_empty());

        let events = gate.accept_snapshot(0, 0, 0, 1, 0, 16, vec![1, 2], vec![0, 0], vec![]);
        assert_eq!(events.len(), 2);
        assert!(matches!(
            &events[0],
            ClientToGame::ChunkData {
                dimension: 0,
                cx: 0,
                cz: 0,
                revision: 1,
                ..
            }
        ));
        assert!(matches!(
            &events[1],
            ClientToGame::BlockChange {
                dimension: 0,
                revision: 2,
                x: 1,
                y: 70,
                z: 1,
                block: 4,
                ..
            }
        ));

        assert!(gate
            .accept_snapshot(0, 0, 0, 1, 0, 16, vec![9], vec![9], vec![])
            .is_empty());
        assert!(gate.accept_block_change(0, 1, 1, 70, 1, 9, 0).is_empty());

        let mut same_revision = RevisionGate::default();
        assert!(same_revision
            .accept_block_change(0, 5, 1, 70, 1, 4, 0)
            .is_empty());
        assert_eq!(
            same_revision
                .accept_snapshot(0, 0, 0, 5, 0, 16, vec![1], vec![0], vec![])
                .len(),
            1
        );
        assert!(same_revision.buffered.is_empty());
    }

    #[test]
    fn revision_gate_multi_client_checksum_converges() {
        fn checksum(chunk: &crate::world::Chunk) -> u64 {
            let mut hash = 0xcbf2_9ce4_8422_2325u64;
            for x in 0..16 {
                for y in 0..256 {
                    for z in 0..16 {
                        hash ^= chunk.get_block_local(x, y, z) as u8 as u64;
                        hash = hash.wrapping_mul(0x100_0000_01b3);
                    }
                }
            }
            hash
        }

        let mut snapshot_chunk = crate::world::Chunk::new(0, 0);
        snapshot_chunk.set_block_local(1, 70, 1, crate::world::BlockType::Stone);
        let snapshot = crate::save::ChunkSaveData::from_chunk(&snapshot_chunk);
        let mut host = snapshot_chunk.clone();
        host.set_block_local(2, 70, 2, crate::world::BlockType::Dirt);

        for _ in 0..2 {
            let mut gate = RevisionGate::default();
            let mut events = gate.accept_block_change(
                0,
                2,
                2,
                70,
                2,
                crate::world::BlockType::Dirt.to_wire(),
                0,
            );
            assert!(events.is_empty());
            events.extend(gate.accept_snapshot(
                0,
                0,
                0,
                1,
                -4,
                24,
                snapshot.blocks.clone(),
                snapshot.block_states.clone(),
                snapshot.block_entities.clone(),
            ));

            let mut client = crate::world::Chunk::new(0, 0);
            for event in events {
                match event {
                    ClientToGame::ChunkData {
                        blocks,
                        block_states,
                        ..
                    } => {
                        crate::save::ChunkSaveData {
                            chunk_x: 0,
                            chunk_z: 0,
                            blocks,
                            sky_light: Vec::new(),
                            block_light: Vec::new(),
                            fluid_levels: Vec::new(),
                            redstone_metadata: Vec::new(),
                            block_states,
                            mutation_revision: 1,
                            block_entities: Vec::new(),
                            data_version: 0,
                        }
                        .restore_to_chunk(&mut client);
                    }
                    ClientToGame::BlockChange { x, y, z, block, .. } => {
                        client.set_block_local(
                            x.rem_euclid(16) as usize,
                            y,
                            z.rem_euclid(16) as usize,
                            crate::world::BlockType::from_wire(block).unwrap(),
                        );
                    }
                    _ => {}
                }
            }
            assert_eq!(checksum(&client), checksum(&host));
        }
    }

    #[test]
    fn replication_gate_rejects_stale_entity_health_and_effect_state() {
        let mut gate = ReplicationGate::default();
        assert!(gate.accept_entity(9, 1));
        assert!(!gate.accept_entity(9, 1));
        assert!(!gate.accept_entity(9, 0));
        assert!(gate.accept_entity(9, 2));
        assert!(gate.accept_entity(10, 1));

        assert!(gate.accept_health(4, 7));
        assert!(!gate.accept_health(4, 7));
        assert!(!gate.accept_health(4, 6));
        assert!(gate.accept_effect(4, 7));
        assert!(!gate.accept_effect(4, 7));
    }

    #[test]
    fn host_client_entity_health_and_effect_replication_converges() {
        let _guard = network_test_guard();
        let reserved = StdTcpListener::bind("127.0.0.1:0").unwrap();
        let addr = reserved.local_addr().unwrap().to_string();
        drop(reserved);
        let (host_tx, host_rx) = mpsc::channel();
        let (server_tx, server_rx) = mpsc::channel();
        let server = NetworkServer::spawn(addr.clone(), 1234, 1, host_rx, server_tx);

        let (game_tx, game_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        let client = NetworkClient::spawn(addr, "replica".into(), game_rx, event_tx);
        let player_id = match wait_for_event(&event_rx) {
            ClientToGame::Connected { player_id, .. } => player_id,
            other => panic!("expected Connected, got {other:?}"),
        };
        assert!(matches!(
            server_rx.recv_timeout(Duration::from_secs(3)).unwrap(),
            ServerToHost::ClientJoined { id, .. } if id == player_id
        ));

        let state = |x| EntityStateWire {
            entity_id: 77,
            entity_type: crate::entity::EntityType::Zombie.to_wire(),
            position: [x, 64.0, 0.0],
            velocity: [1.0, 0.0, 0.0],
            yaw: 0.5,
            pitch: 0.0,
            health: 18.0,
            animation_state: 1,
        };
        host_tx
            .send(HostToServer::BroadcastEntitySpawn {
                dimension: 0,
                sequence: 1,
                state: state(0.0),
            })
            .unwrap();
        host_tx
            .send(HostToServer::BroadcastEntityState {
                dimension: 0,
                sequence: 2,
                state: state(2.0),
            })
            .unwrap();
        host_tx
            .send(HostToServer::BroadcastEntityState {
                dimension: 0,
                sequence: 3,
                state: state(3.0),
            })
            .unwrap();
        host_tx
            .send(HostToServer::BroadcastPlayerHealth {
                sequence: 3,
                player_id,
                health: 14.0,
                max_health: 20.0,
                hunger: 17.0,
                saturation: 2.0,
                oxygen: 280.0,
                is_dead: false,
                death_reason: 0,
            })
            .unwrap();
        host_tx
            .send(HostToServer::BroadcastPlayerEffect {
                sequence: 3,
                player_id,
                effects: vec![PlayerEffectWire {
                    kind: 0,
                    level: 2,
                    remaining_seconds: 30.0,
                }],
            })
            .unwrap();

        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        let mut saw_spawn = false;
        let mut latest_entity_x = None;
        let mut health = None;
        let mut effects = None;
        while std::time::Instant::now() < deadline
            && (!saw_spawn || latest_entity_x != Some(3.0) || health.is_none() || effects.is_none())
        {
            let Ok(event) = event_rx.recv_timeout(Duration::from_millis(250)) else {
                continue;
            };
            match event {
                ClientToGame::EntitySpawn { state, .. } if state.entity_id == 77 => {
                    saw_spawn = true;
                }
                ClientToGame::EntityState { state, .. } if state.entity_id == 77 => {
                    latest_entity_x = Some(state.position[0]);
                }
                ClientToGame::PlayerHealth {
                    player_id: id,
                    health: value,
                    ..
                } if id == player_id => health = Some(value),
                ClientToGame::PlayerEffect {
                    player_id: id,
                    effects: value,
                    ..
                } if id == player_id => effects = Some(value),
                _ => {}
            }
        }
        assert!(saw_spawn);
        assert_eq!(latest_entity_x, Some(3.0));
        assert_eq!(health, Some(14.0));
        assert_eq!(effects.unwrap()[0].level, 2);

        host_tx
            .send(HostToServer::BroadcastEntityDespawn {
                dimension: 0,
                sequence: 4,
                entity_id: 77,
            })
            .unwrap();
        assert!(matches!(
            wait_for_event(&event_rx),
            ClientToGame::EntityDespawn {
                sequence: 4,
                entity_id: 77,
                ..
            }
        ));

        game_tx.send(GameToClient::Disconnect).unwrap();
        client.join().unwrap();
        host_tx.send(HostToServer::Stop).unwrap();
        server.join().unwrap();
    }

    #[test]
    fn queue_accounting_covers_success_and_closed_paths() {
        let stats = crate::perf::SharedQueueStats::new();
        let (tx, rx) = mpsc::channel();
        crate::perf::tracked_send(
            &tx,
            ClientToGame::StatusUpdate {
                message: "ok".into(),
            },
            16,
            &stats,
        )
        .unwrap();
        assert_eq!(stats.depth(), 1);
        let _ = crate::perf::tracked_try_recv(&rx, 16, &stats).unwrap();
        assert_eq!(stats.depth(), 0);

        drop(rx);
        assert!(crate::perf::tracked_send(
            &tx,
            ClientToGame::StatusUpdate {
                message: "closed".into()
            },
            16,
            &stats,
        )
        .is_err());
        assert_eq!(stats.depth(), 0);
        assert_eq!(stats.bytes(), 0);
        assert_eq!(stats.drops(), 1);
    }

    #[test]
    fn queue_accounting_closed_consumer_is_cancelled() {
        let stats = crate::perf::SharedQueueStats::new();
        let (tx, rx) = mpsc::channel::<GameToClient>();
        drop(tx);
        assert!(matches!(
            crate::perf::tracked_try_recv(&rx, 32, &stats),
            Err(std::sync::mpsc::TryRecvError::Disconnected)
        ));
        stats.cancel();
        assert_eq!(stats.cancels(), 1);
    }
}
