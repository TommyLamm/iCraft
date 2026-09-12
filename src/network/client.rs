use std::cell::Cell;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError};
use std::thread::JoinHandle;
use std::time::Duration;

use tokio::net::TcpStream;
use tokio::time::{self, Instant};

use super::protocol::{
    Action, EntityStateWire, GameplayRequest, GameplayResponse, LightningStrike, Packet,
    PlayerEffectWire, PlayerId, SessionGameplayWire, PROTOCOL_VERSION,
};
use super::transport::{Connection, ConnectionWriter};
use crate::world::chunk_xz;

/// Bounded network-client → game-thread queue. A malicious server cannot grow
/// this without bound; sustained overflow disconnects the join client.
/// Sized to absorb one `ServerRuntime` presentation tick (`1024`) now that
/// clients no longer ACK-pace `ChunkData`.
pub const CLIENT_TO_GAME_QUEUE_CAPACITY: usize = 1024;
const CLIENT_TO_GAME_OVERFLOW_LIMIT: u32 = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClientQueueError {
    Full,
    Closed,
}

/// The client/game boundary is the app-level inbound queue (network client ->
/// game thread).  Account ownership only after the synchronous send accepts it;
/// transport socket writes are intentionally not included because the OS owns
/// that buffering.  Reliable replication (including revision-gated catch-up)
/// remains FIFO on this same inbound boundary.
fn send_to_game(
    sender: &SyncSender<ClientToGame>,
    event: ClientToGame,
) -> Result<(), ClientQueueError> {
    let bytes = std::mem::size_of_val(&event) as u64;
    let stats = crate::perf::queue_stats(crate::perf::QueueCategory::Inbound);
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis().min(u64::MAX as u128) as u64);
    stats.enqueue(bytes, now_ms);
    match sender.try_send(event) {
        Ok(()) => Ok(()),
        Err(TrySendError::Full(_)) => {
            stats.dequeue(bytes);
            stats.drop_item();
            Err(ClientQueueError::Full)
        }
        Err(TrySendError::Disconnected(_)) => {
            stats.dequeue(bytes);
            stats.drop_item();
            Err(ClientQueueError::Closed)
        }
    }
}

struct ClientEventSender {
    sender: SyncSender<ClientToGame>,
    overflow_streak: Cell<u32>,
    dead: Cell<bool>,
}

impl ClientEventSender {
    fn new(sender: SyncSender<ClientToGame>) -> Self {
        Self {
            sender,
            overflow_streak: Cell::new(0),
            dead: Cell::new(false),
        }
    }

    fn is_dead(&self) -> bool {
        self.dead.get()
    }

    fn mark_dead(&self, reason: &str) {
        self.dead.set(true);
        let _ = send_to_game(&self.sender, ClientToGame::disconnect(reason));
    }

    fn send(&self, event: ClientToGame) -> Result<(), ClientQueueError> {
        if self.dead.get() {
            return Err(ClientQueueError::Closed);
        }
        match send_to_game(&self.sender, event) {
            Ok(()) => {
                self.overflow_streak.set(0);
                Ok(())
            }
            Err(ClientQueueError::Full) => {
                let streak = self.overflow_streak.get().saturating_add(1);
                self.overflow_streak.set(streak);
                if streak >= CLIENT_TO_GAME_OVERFLOW_LIMIT {
                    self.mark_dead("inbound queue overflow");
                    Err(ClientQueueError::Full)
                } else {
                    Err(ClientQueueError::Full)
                }
            }
            Err(ClientQueueError::Closed) => {
                self.dead.set(true);
                Err(ClientQueueError::Closed)
            }
        }
    }
}

/// Join-client thread → game thread.
///
/// Wire projections are already-decoded [`Packet`] values after a single
/// protocol-version check. The only local-only variant is connection-progress
/// [`Self::StatusUpdate`] text (never encoded on the wire).
#[derive(Debug)]
pub enum ClientToGame {
    StatusUpdate {
        message: String,
    },
    Packet(Packet),
}

impl ClientToGame {
    pub fn packet(packet: Packet) -> Self {
        Self::Packet(packet)
    }

    pub fn disconnect(reason: impl Into<String>) -> Self {
        Self::Packet(Packet::Disconnect {
            reason: reason.into(),
        })
    }

    pub fn estimated_bytes(&self) -> usize {
        let inline = std::mem::size_of_val(self);
        let heap = match self {
            Self::StatusUpdate { message } => message.len(),
            Self::Packet(packet) => match packet {
                Packet::Disconnect { reason, .. } => reason.len(),
                Packet::GameplayResponse { response, .. } => std::mem::size_of_val(response),
                Packet::PlayerJoin { username, .. } => username.len(),
                Packet::ChunkData {
                    blocks,
                    block_states,
                    ..
                } => blocks.len().saturating_add(block_states.len()),
                Packet::PlayerEffect { effects, .. } => {
                    effects.len() * std::mem::size_of::<PlayerEffectWire>()
                }
                Packet::ChatMessage { sender, message, .. } => {
                    sender.len().saturating_add(message.len())
                }
                Packet::ContainerOpenResult { slots, .. } => {
                    slots.len() * std::mem::size_of::<Option<crate::network::protocol::ItemWire>>()
                }
                Packet::ContainerClickResult { slot, dragged, .. } => {
                    slot.as_ref().map_or(0, |w| std::mem::size_of_val(w))
                        + dragged.as_ref().map_or(0, |w| std::mem::size_of_val(w))
                }
                Packet::ContainerSlotUpdate { slot, .. } => {
                    slot.as_ref().map_or(0, |w| std::mem::size_of_val(w))
                }
                _ => 0,
            },
        };
        inline.saturating_add(heap)
    }
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
    SendChat {
        message: String,
    },
    Disconnect,
    PlayerRespawnRequest,
    GameplayRequest {
        request: GameplayRequest,
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
    raw_fluid: u8,
}

#[derive(Default)]
struct RevisionGate {
    applied: HashMap<(u8, i32, i32), u64>,
    buffered: HashMap<(u8, i32, i32), BTreeMap<u64, BufferedBlockChange>>,
}

#[derive(Default)]
struct ReplicationGate {
    entity_sequences: HashMap<(u8, u64), u64>,
    health_sequences: HashMap<PlayerId, u64>,
    effect_sequences: HashMap<PlayerId, u64>,
    /// Session snapshots carry a global per-session sequence in addition to
    /// a dimension-scoped gameplay revision.  The sequence is the ordering
    /// authority across dimension transfers; the revision is checked only
    /// when the dimension remains unchanged.
    session_revisions: HashMap<PlayerId, (u8, u64, u64)>,
    block_entity_revisions: HashMap<(u8, i32, i32, i32), u64>,
    container_revisions: HashMap<(u8, i32, i32, i32), u64>,
}

impl ReplicationGate {
    fn accept_entity(&mut self, dimension: u8, entity_id: u64, sequence: u64) -> bool {
        let latest = self
            .entity_sequences
            .entry((dimension, entity_id))
            .or_default();
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

    fn accept_session(
        &mut self,
        player_id: PlayerId,
        dimension: u8,
        sequence: u64,
        revision: u64,
    ) -> bool {
        if let Some((latest_dimension, latest_sequence, latest_revision)) =
            self.session_revisions.get(&player_id).copied()
        {
            if sequence <= latest_sequence {
                return false;
            }
            if latest_dimension == dimension && revision <= latest_revision {
                return false;
            }
        }
        self.session_revisions
            .insert(player_id, (dimension, sequence, revision));
        true
    }

    fn accept_block_entity(&mut self, key: (u8, i32, i32, i32), revision: u64) -> bool {
        let latest = self.block_entity_revisions.entry(key).or_default();
        if revision <= *latest {
            return false;
        }
        *latest = revision;
        true
    }

    fn accept_container_update(&mut self, key: (u8, i32, i32, i32), revision: u64) -> bool {
        let latest = self.container_revisions.entry(key).or_default();
        if revision <= *latest {
            return false;
        }
        *latest = revision;
        true
    }
}

#[derive(Default)]
struct GameplayResponseGate {
    latest_server_sequence: u64,
    responses: HashMap<crate::network::protocol::RequestId, GameplayResponse>,
    seen_order: VecDeque<crate::network::protocol::RequestId>,
}

impl GameplayResponseGate {
    fn accept(&mut self, response: &GameplayResponse) -> bool {
        // The server replays a byte-for-byte cached response for an idempotent
        // request retry. Once this client has already surfaced that request id
        // into its reliable app queue, suppress every replay or rewrite. If the
        // first network ACK was lost before reaching this gate, the first copy
        // that does arrive is still accepted below.
        if self.responses.contains_key(&response.request_id) {
            return false;
        }
        if response.server_sequence == 0 || response.server_sequence <= self.latest_server_sequence
        {
            return false;
        }
        if self.seen_order.len() >= crate::authority::RESPONSE_CACHE_CAPACITY {
            if let Some(evicted) = self.seen_order.pop_front() {
                self.responses.remove(&evicted);
            }
        }
        self.seen_order.push_back(response.request_id);
        self.responses.insert(response.request_id, response.clone());
        self.latest_server_sequence = response.server_sequence;
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
        raw_fluid: u8,
    ) -> Vec<Packet> {
        let key = { let (cx, cz) = chunk_xz(x, z); (dimension, cx, cz) };
        if let Some(current) = self.applied.get_mut(&key) {
            if revision <= *current {
                return Vec::new();
            }
            *current = revision;
            return vec![Packet::BlockChange {
                dimension,
                revision,
                x,
                y,
                z,
                block,
                state,
                raw_fluid,
            }];
        }
        self.buffered.entry(key).or_default().insert(
            revision,
            BufferedBlockChange {
                x,
                y,
                z,
                block,
                state,
                raw_fluid,
            },
        );
        Vec::new()
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
        fluid_levels: Vec<u8>,
        block_entities: Vec<u8>,
    ) -> Vec<Packet> {
        let key = (dimension, cx, cz);
        if self
            .applied
            .get(&key)
            .is_some_and(|current| revision <= *current)
        {
            return Vec::new();
        }
        self.applied.insert(key, revision);
        if let Some(changes) = self.buffered.get_mut(&key) {
            changes.retain(|buffered_revision, _| *buffered_revision > revision);
        }
        let mut events = vec![Packet::ChunkData {
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
        }];
        events.extend(self.flush_buffered(key));
        events
    }

    fn flush_buffered(&mut self, key: (u8, i32, i32)) -> Vec<Packet> {
        let mut events = Vec::new();
        let current = self.applied.get(&key).copied().unwrap_or(0);
        let pending = self.buffered.remove(&key).unwrap_or_default();
        for (revision, change) in pending {
            if revision <= current {
                continue;
            }
            self.applied.insert(key, revision);
            events.push(Packet::BlockChange {
                dimension: key.0,
                revision,
                x: change.x,
                y: change.y,
                z: change.z,
                block: change.block,
                state: change.state,
                raw_fluid: change.raw_fluid,
            });
        }
        events
    }
}

fn prepare_gameplay_request(
    mut request: GameplayRequest,
    player_id: PlayerId,
    next_request_id: &mut crate::network::protocol::RequestId,
    last_client_sequence: &mut u64,
    last_client_revision: &mut u64,
) -> GameplayRequest {
    let fresh_input = request.client_sequence == 0;
    if request.request_id == 0 {
        request.request_id = (*next_request_id).max(1);
    }
    *next_request_id = (*next_request_id)
        .max(request.request_id.saturating_add(1))
        .max(1);
    if request.client_sequence == 0 {
        request.client_sequence = (*last_client_sequence).wrapping_add(1).max(1);
    }
    if request.client_sequence > *last_client_sequence {
        *last_client_sequence = request.client_sequence;
    }
    // A zero sequence marks a new player input whose envelope is finalized by
    // this socket owner. Rebase only that input onto the latest owner-private
    // projection observed immediately before the frame write. Explicit
    // sequence/revision pairs remain untouched so stale/out-of-order probes
    // still reach the server's security gates verbatim.
    if fresh_input {
        request.client_revision = request.client_revision.max(*last_client_revision);
        *last_client_revision = request.client_revision;
    } else if request.client_revision > *last_client_revision {
        *last_client_revision = request.client_revision;
    }
    request.session_id = player_id;
    request
}

async fn send_or_die(
    writer: &mut ConnectionWriter,
    packet: &Packet,
    client_to_game: &ClientEventSender,
    what: &str,
) -> Result<(), ()> {
    if writer.send(packet).await.is_err() {
        eprintln!("[NetworkClient] Disconnecting: failed to send {what}");
        let _ = client_to_game.send(ClientToGame::disconnect("connection lost"));
        return Err(());
    }
    Ok(())
}

fn authoritative_weather_event(packet: &Packet) -> Option<ClientToGame> {
    match packet {
        Packet::TimeSync { .. } | Packet::LightningStrike { .. } => {
            Some(ClientToGame::packet(packet.clone()))
        }
        _ => None,
    }
}

impl NetworkClient {
    pub fn spawn(
        server_addr: String,
        username: String,
        game_to_client: Receiver<GameToClient>,
        client_to_game: SyncSender<ClientToGame>,
    ) -> JoinHandle<()> {
        std::thread::spawn(move || {
            let client_to_game = ClientEventSender::new(client_to_game);
            let runtime = match tokio::runtime::Runtime::new() {
                Ok(runtime) => runtime,
                Err(error) => {
                    let _ = client_to_game.send(ClientToGame::disconnect(format!(
                        "failed to create network runtime: {error}"
                    )));
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
                let _ = client_to_game.send(ClientToGame::disconnect(reason));
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
        let _ = client_to_game.send(ClientToGame::disconnect(reason));
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
            let _ = client_to_game.send(ClientToGame::packet(Packet::LoginSuccess {
                protocol_version: PROTOCOL_VERSION,
                player_id,
                seed,
                gamemode,
            }));
            player_id
        }
        Ok(Ok(Packet::Disconnect { reason, .. })) => {
            eprintln!("[NetworkClient] Server disconnected during login: {reason}");
            let _ = client_to_game.send(ClientToGame::disconnect(reason));
            return;
        }
        Ok(Ok(packet)) => {
            let reason = format!("unexpected handshake response: {packet:?}");
            eprintln!("[NetworkClient] {reason}");
            let _ = client_to_game.send(ClientToGame::disconnect(reason));
            return;
        }
        Ok(Err(error)) => {
            let reason = error.to_string();
            eprintln!("[NetworkClient] Connection recv error: {reason}");
            let _ = client_to_game.send(ClientToGame::disconnect(reason));
            return;
        }
        Err(_) => {
            let reason = "login timed out".to_string();
            eprintln!("[NetworkClient] Login timed out after 5s");
            let _ = client_to_game.send(ClientToGame::disconnect(reason));
            return;
        }
    };

    let (mut reader, mut writer) = connection.into_split();
    let mut tick = time::interval(Duration::from_millis(10));
    let mut revision_gate = RevisionGate::default();
    let mut replication_gate = ReplicationGate::default();
    let mut gameplay_response_gate = GameplayResponseGate::default();
    let mut next_request_id: crate::network::protocol::RequestId = 1;
    let mut last_client_sequence = 0u64;
    let mut last_client_revision = 0u64;
    let mut current_dimension = 0u8;
    let mut active_container: Option<(u8, i32, i32, i32)> = None;
    loop {
        tokio::select! {
            incoming = reader.recv() => {
                match incoming {
                    Ok(packet @ Packet::PlayerJoin { .. })
                    | Ok(packet @ Packet::PlayerLeave { .. })
                    | Ok(packet @ Packet::PlayerPosition { .. })
                    | Ok(packet @ Packet::PlayerAction { .. })
                    | Ok(packet @ Packet::ContainerClickResult { .. })
                    | Ok(packet @ Packet::PlayerRespawnResult { .. })
                    | Ok(packet @ Packet::SleepStateSync { .. })
                    | Ok(packet @ Packet::WorldRulesSync { .. })
                    | Ok(packet @ Packet::ChatMessage { .. })
                    | Ok(packet @ Packet::TimeSync { .. })
                    | Ok(packet @ Packet::LightningStrike { .. }) => {
                        // Protocol version is negotiated at handshake; post-auth
                        // packets no longer carry protocol_version.
                        if matches!(
                            &packet,
                            Packet::PlayerRespawnResult { dimension, .. }
                        ) {
                            if let Packet::PlayerRespawnResult { dimension, .. } = &packet {
                                current_dimension = *dimension;
                            }
                        }
                        let _ = client_to_game.send(ClientToGame::packet(packet));
                    }
                    Ok(Packet::BlockChange {
                        dimension,
                        revision,
                        x,
                        y,
                        z,
                        block,
                        state,
                        raw_fluid,
                        ..
                    }) => {
                        current_dimension = dimension;
                        last_client_revision = last_client_revision.max(revision);
                        for event in revision_gate.accept_block_change(
                            dimension, revision, x, y, z, block, state, raw_fluid,
                        ) {
                            let _ = client_to_game.send(ClientToGame::packet(event));
                        }
                    }
                    Ok(packet @ Packet::BlockEntityDelta {
                        dimension,
                        revision,
                        x,
                        y,
                        z,
                        ..
                    }) => {
                        current_dimension = dimension;
                        if replication_gate.accept_block_entity((dimension, x, y, z), revision) {
                            last_client_revision = last_client_revision.max(revision);
                            let _ = client_to_game.send(ClientToGame::packet(packet));
                        }
                    }
                    Ok(packet @ Packet::ContainerOpenResult {
                        dimension,
                        success,
                        x,
                        y,
                        z,
                        revision,
                        ..
                    }) => {
                        let key = (dimension, x, y, z);
                        if !success {
                            // A failed open is not a slot delta.  If it names
                            // the active session, treat it as a forced close;
                            // otherwise do not disturb a different container
                            // that the player opened in the meantime.
                            if active_container == Some(key) {
                                active_container = None;
                                let _ = client_to_game.send(ClientToGame::packet(Packet::ContainerClose {
                                    dimension,
                                    x,
                                    y,
                                    z,
                                }));
                            }
                        } else if replication_gate.accept_container_update(key, revision) {
                            current_dimension = dimension;
                            last_client_revision = last_client_revision.max(revision);
                            active_container = Some(key);
                            let _ = client_to_game.send(ClientToGame::packet(packet));
                        }
                    }
                    Ok(packet @ Packet::ContainerClose { dimension, x, y, z, .. }) => {
                        let key = (dimension, x, y, z);
                        if active_container == Some(key) {
                            active_container = None;
                            let _ = client_to_game.send(ClientToGame::packet(packet));
                        }
                    }
                    Ok(packet @ Packet::DimensionTransfer {
                        player_id: target_id,
                        dimension,
                        ..
                    }) => {
                        if target_id == player_id {
                            current_dimension = dimension;
                            // Revisions are independent per dimension; the
                            // first target-world snapshot/request must not be
                            // compared to the source world's high-water mark.
                            last_client_revision = 0;
                            active_container = None;
                            let _ = client_to_game.send(ClientToGame::packet(packet));
                        }
                    }
                    Ok(packet @ Packet::ContainerSlotUpdate {
                        dimension,
                        revision,
                        x,
                        y,
                        z,
                        ..
                    }) => {
                        current_dimension = dimension;
                        if replication_gate.accept_container_update((dimension, x, y, z), revision) {
                            last_client_revision = last_client_revision.max(revision);
                            let _ = client_to_game.send(ClientToGame::packet(packet));
                        }
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
                        fluid_levels,
                        block_entities,
                        ..
                    }) => {
                        current_dimension = dimension;
                        last_client_revision = last_client_revision.max(revision);
                        for event in revision_gate.accept_snapshot(
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
                        ) {
                            let _ = client_to_game.send(ClientToGame::packet(event));
                        }
                    }
                    Ok(packet @ Packet::EntitySpawn { .. }) => {
                        let accept = match &packet {
                            Packet::EntitySpawn {
                                dimension,
                                sequence,
                                state,
                                ..
                            } => replication_gate.accept_entity(*dimension, state.entity_id, *sequence),
                            _ => false,
                        };
                        if accept {
                            let _ = client_to_game.send(ClientToGame::packet(packet));
                        }
                    }
                    Ok(packet @ Packet::EntityState { .. }) => {
                        let accept = match &packet {
                            Packet::EntityState {
                                dimension,
                                sequence,
                                state,
                                ..
                            } => replication_gate.accept_entity(*dimension, state.entity_id, *sequence),
                            _ => false,
                        };
                        if accept {
                            let _ = client_to_game.send(ClientToGame::packet(packet));
                        }
                    }
                    Ok(packet @ Packet::EntityDespawn {
                        dimension,
                        sequence,
                        entity_id,
                        ..
                    }) => {
                        if replication_gate.accept_entity(dimension, entity_id, sequence) {
                            let _ = client_to_game.send(ClientToGame::packet(packet));
                        }
                    }
                    Ok(packet @ Packet::PlayerHealth {
                        sequence,
                        player_id: health_player_id,
                        ..
                    }) => {
                        if replication_gate.accept_health(health_player_id, sequence) {
                            let _ = client_to_game.send(ClientToGame::packet(packet));
                        }
                    }
                    Ok(packet @ Packet::PlayerEffect {
                        sequence,
                        player_id: effect_player_id,
                        ..
                    }) => {
                        if replication_gate.accept_effect(effect_player_id, sequence) {
                            let _ = client_to_game.send(ClientToGame::packet(packet));
                        }
                    }
                    Ok(packet @ Packet::PlayerSessionUpdate { .. }) => {
                        let accept = match &packet {
                            Packet::PlayerSessionUpdate {
                                sequence,
                                player_id: session_player_id,
                                dimension,
                                state,
                                ..
                            } => {
                                if *session_player_id != player_id {
                                    false
                                } else if state.validate_bounds().is_ok()
                                    && replication_gate.accept_session(
                                        *session_player_id,
                                        *dimension,
                                        *sequence,
                                        state.revision,
                                    )
                                {
                                    if *dimension == current_dimension {
                                        last_client_revision =
                                            last_client_revision.max(state.revision);
                                    }
                                    true
                                } else {
                                    false
                                }
                            }
                            _ => false,
                        };
                        if accept {
                            let _ = client_to_game.send(ClientToGame::packet(packet));
                        }
                    }
                    Ok(Packet::GameplayResponse { response, .. }) => {
                        if let crate::network::protocol::GameplayOutcome::Accepted { revision } = response.outcome {
                            last_client_revision = last_client_revision.max(revision);
                        }
                        if gameplay_response_gate.accept(&response) {
                            let _ = client_to_game.send(ClientToGame::packet(Packet::GameplayResponse {
                                response,
                            }));
                        }
                    }
                    Ok(Packet::Keepalive { .. }) => {
                        if writer.send(&Packet::Keepalive).await.is_err() {
                            eprintln!("[NetworkClient] Disconnecting: failed to reply to keepalive");
                            let _ = client_to_game.send(ClientToGame::disconnect("connection lost"));
                            break;
                        }
                    }
                    Ok(Packet::Disconnect { reason, .. }) => {
                        eprintln!("[NetworkClient] Disconnecting: server sent Disconnect: {reason}");
                        let _ = client_to_game.send(ClientToGame::disconnect(reason));
                        break;
                    }
                    Ok(_) => {}
                    Err(error) => {
                        eprintln!("[NetworkClient] Disconnecting: reader recv error: {error}");
                        let _ = client_to_game.send(ClientToGame::disconnect("connection lost"));
                        break;
                    }
                }
                if client_to_game.is_dead() {
                    eprintln!("[NetworkClient] Disconnecting: inbound queue overflow");
                    break;
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
                            if send_or_die(
                                &mut writer,
                                &Packet::PlayerAction { id: player_id, action },
                                &client_to_game,
                                "PlayerAction",
                            ).await.is_err() {
                                return;
                            }
                        }
                        Ok(GameToClient::SendChat { message }) => {
                            if send_or_die(
                                &mut writer,
                                &Packet::ChatMessage { sender: username.clone(), message },
                                &client_to_game,
                                "ChatMessage",
                            ).await.is_err() {
                                return;
                            }
                        }
                        Ok(GameToClient::PlayerRespawnRequest) => {
                            if send_or_die(
                                &mut writer,
                                &Packet::PlayerRespawnRequest,
                                &client_to_game,
                                "PlayerRespawnRequest",
                            ).await.is_err() {
                                return;
                            }
                        }
                        Ok(GameToClient::GameplayRequest { request }) => {
                            let request = prepare_gameplay_request(
                                request,
                                player_id,
                                &mut next_request_id,
                                &mut last_client_sequence,
                                &mut last_client_revision,
                            );
                            if send_or_die(
                                &mut writer,
                                &Packet::GameplayRequest {
                                    request,
                                },
                                &client_to_game,
                                "GameplayRequest",
                            ).await.is_err() {
                                return;
                            }
                        }
                        Ok(GameToClient::Disconnect) => {
                            eprintln!("[NetworkClient] Disconnecting: game thread requested disconnect");
                            let _ = writer.send(&Packet::Disconnect { reason: "client disconnect".into() }).await;
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
                    if send_or_die(
                        &mut writer,
                        &Packet::PlayerPosition {
                            id: player_id,
                            sequence,
                            sender_time_millis,
                            x,
                            y,
                            z,
                            yaw,
                            pitch,
                        },
                        &client_to_game,
                        "PlayerPosition",
                    )
                    .await
                    .is_err()
                    {
                        return;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;

