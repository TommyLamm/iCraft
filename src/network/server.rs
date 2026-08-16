use std::collections::{HashMap, HashSet, VecDeque};
use std::future::Future;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc as std_mpsc, Arc};
use std::thread::JoinHandle;
use std::time::Duration;

use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot, watch, Mutex, Notify};
use tokio::time::{self, Instant};

use super::protocol::{
    Action, EntityStateWire, GameplayOperation, GameplayRequest, GameplayResponse, LightningStrike,
    Packet, PlayerEffectWire, PlayerId, RejectReason, RequestId, ServerSequence,
    SessionGameplayWire, PROTOCOL_VERSION,
};
use super::transport::Connection;

#[derive(Clone, Default)]
pub(crate) struct NetworkMetrics {
    inner: Arc<NetworkMetricsInner>,
}

#[derive(Default)]
struct NetworkMetricsInner {
    inbound_packets: AtomicU64,
    inbound_bytes: AtomicU64,
    outbound_packets: AtomicU64,
    outbound_bytes: AtomicU64,
    queue_depth: AtomicUsize,
    queue_full: AtomicU64,
    rejected_requests: AtomicU64,
    duplicate_requests: AtomicU64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct NetworkMetricsSnapshot {
    pub inbound_packets: u64,
    pub inbound_bytes: u64,
    pub outbound_packets: u64,
    pub outbound_bytes: u64,
    pub queue_depth: usize,
    pub queue_full: u64,
    pub rejected_requests: u64,
    pub duplicate_requests: u64,
}

impl NetworkMetrics {
    fn add(counter: &AtomicU64, amount: u64) {
        let _ = counter.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
            Some(value.saturating_add(amount))
        });
    }

    fn subtract(counter: &AtomicU64, amount: u64) {
        let _ = counter.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
            Some(value.saturating_sub(amount))
        });
    }

    fn record_inbound(&self, packet: &Packet) {
        Self::add(&self.inner.inbound_packets, 1);
        Self::add(&self.inner.inbound_bytes, packet_bytes(packet));
    }

    /// Reserve the outbound frame counters before a socket write begins. A
    /// peer may observe a successful frame as soon as `write_all` completes,
    /// so publishing after the await leaves a visibility race. The guard
    /// rolls the reservation back when the write fails.
    fn reserve_outbound(&self, packet: &Packet) -> OutboundMetricReservation {
        let bytes = packet_bytes(packet);
        Self::add(&self.inner.outbound_packets, 1);
        Self::add(&self.inner.outbound_bytes, bytes);
        OutboundMetricReservation {
            metrics: self.clone(),
            bytes,
            committed: false,
        }
    }

    pub(crate) fn enqueue(&self) {
        let _ =
            self.inner
                .queue_depth
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                    Some(value.saturating_add(1))
                });
    }

    pub(crate) fn dequeue(&self) {
        let _ =
            self.inner
                .queue_depth
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                    Some(value.saturating_sub(1))
                });
    }

    pub(crate) fn record_queue_full(&self) {
        Self::add(&self.inner.queue_full, 1);
    }

    fn record_rejected_request(&self) {
        Self::add(&self.inner.rejected_requests, 1);
    }

    fn record_duplicate_request(&self) {
        Self::add(&self.inner.duplicate_requests, 1);
    }

    pub(crate) fn snapshot(&self) -> NetworkMetricsSnapshot {
        NetworkMetricsSnapshot {
            inbound_packets: self.inner.inbound_packets.load(Ordering::Relaxed),
            inbound_bytes: self.inner.inbound_bytes.load(Ordering::Relaxed),
            outbound_packets: self.inner.outbound_packets.load(Ordering::Relaxed),
            outbound_bytes: self.inner.outbound_bytes.load(Ordering::Relaxed),
            queue_depth: self.inner.queue_depth.load(Ordering::Relaxed),
            queue_full: self.inner.queue_full.load(Ordering::Relaxed),
            rejected_requests: self.inner.rejected_requests.load(Ordering::Relaxed),
            duplicate_requests: self.inner.duplicate_requests.load(Ordering::Relaxed),
        }
    }
}

struct OutboundMetricReservation {
    metrics: NetworkMetrics,
    bytes: u64,
    committed: bool,
}

impl OutboundMetricReservation {
    fn commit(mut self) {
        self.committed = true;
    }
}

impl Drop for OutboundMetricReservation {
    fn drop(&mut self) {
        if !self.committed {
            NetworkMetrics::subtract(&self.metrics.inner.outbound_packets, 1);
            NetworkMetrics::subtract(&self.metrics.inner.outbound_bytes, self.bytes);
        }
    }
}

struct TrackedPacket {
    packet: Option<Packet>,
    metrics: NetworkMetrics,
}

impl TrackedPacket {
    fn new(packet: Packet, metrics: &NetworkMetrics) -> Self {
        metrics.enqueue();
        Self {
            packet: Some(packet),
            metrics: metrics.clone(),
        }
    }

    fn packet(&self) -> &Packet {
        self.packet
            .as_ref()
            .expect("queued packet is present until it leaves its backlog")
    }

    fn into_packet(mut self) -> Packet {
        self.metrics.dequeue();
        self.packet
            .take()
            .expect("queued packet is consumed exactly once")
    }
}

impl Drop for TrackedPacket {
    fn drop(&mut self) {
        if self.packet.is_some() {
            self.metrics.dequeue();
        }
    }
}

enum QueuedPacket {
    Reliable(TrackedPacket),
    ReliableWithAck(TrackedPacket, oneshot::Sender<bool>),
    Outbound(TrackedPacket),
}

fn packet_bytes(packet: &Packet) -> u64 {
    // `ConnectionWriter` emits a four-byte big-endian frame length before the
    // bincode payload. Count the bytes that actually cross TCP, not just the
    // serialized message body.
    (packet.encode().len() as u64).saturating_add(4)
}

fn queue_stats() -> Arc<crate::perf::SharedQueueStats> {
    crate::perf::queue_stats(crate::perf::QueueCategory::Outbound)
}

fn queue_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis().min(u64::MAX as u128) as u64)
}

async fn reliable_send(
    tx: &mpsc::Sender<QueuedPacket>,
    packet: Packet,
    metrics: &NetworkMetrics,
) -> bool {
    let bytes = packet_bytes(&packet);
    let stats = crate::perf::queue_stats(crate::perf::QueueCategory::Reliable);
    match time::timeout(RELIABLE_ENQUEUE_TIMEOUT, tx.reserve()).await {
        Ok(Ok(permit)) => {
            stats.enqueue(bytes, queue_now_ms());
            permit.send(QueuedPacket::Reliable(TrackedPacket::new(packet, metrics)));
            true
        }
        Ok(Err(_)) => {
            stats.drop_item();
            false
        }
        Err(_) => {
            stats.retry();
            stats.drop_item();
            metrics.record_queue_full();
            false
        }
    }
}

async fn reliable_send_and_wait(
    tx: &mpsc::Sender<QueuedPacket>,
    packet: Packet,
    metrics: &NetworkMetrics,
) -> bool {
    let bytes = packet_bytes(&packet);
    let stats = crate::perf::queue_stats(crate::perf::QueueCategory::Reliable);
    let permit = match time::timeout(RELIABLE_ENQUEUE_TIMEOUT, tx.reserve()).await {
        Ok(Ok(permit)) => permit,
        Ok(Err(_)) => {
            stats.drop_item();
            return false;
        }
        Err(_) => {
            stats.retry();
            stats.drop_item();
            metrics.record_queue_full();
            return false;
        }
    };
    let (completion_tx, completion_rx) = oneshot::channel();
    stats.enqueue(bytes, queue_now_ms());
    permit.send(QueuedPacket::ReliableWithAck(
        TrackedPacket::new(packet, metrics),
        completion_tx,
    ));
    matches!(
        time::timeout(CLIENT_TIMEOUT, completion_rx).await,
        Ok(Ok(true))
    )
}
fn best_effort_send(tx: &mpsc::Sender<QueuedPacket>, packet: Packet, metrics: &NetworkMetrics) {
    let bytes = packet_bytes(&packet);
    match tx.try_reserve() {
        Ok(permit) => {
            queue_stats().enqueue(bytes, queue_now_ms());
            permit.send(QueuedPacket::Outbound(TrackedPacket::new(packet, metrics)));
        }
        Err(mpsc::error::TrySendError::Full(_)) => {
            queue_stats().drop_item();
            metrics.record_queue_full();
        }
        Err(mpsc::error::TrySendError::Closed(_)) => queue_stats().drop_item(),
    }
}

async fn send_connection_packet(
    connection: &mut Connection,
    packet: Packet,
    metrics: &NetworkMetrics,
) -> std::io::Result<()> {
    send_with_outbound_metrics(&packet, metrics, || connection.send(&packet)).await
}

async fn send_writer_packet(
    writer: &mut super::transport::ConnectionWriter,
    packet: &Packet,
    metrics: &NetworkMetrics,
) -> std::io::Result<()> {
    send_with_outbound_metrics(packet, metrics, || writer.send(packet)).await
}

async fn send_with_outbound_metrics<F, Fut>(
    packet: &Packet,
    metrics: &NetworkMetrics,
    send: F,
) -> std::io::Result<()>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = std::io::Result<()>>,
{
    let reservation = metrics.reserve_outbound(packet);
    match send().await {
        Ok(()) => {
            reservation.commit();
            Ok(())
        }
        Err(error) => {
            drop(reservation);
            Err(error)
        }
    }
}

const CLIENT_QUEUE_CAPACITY: usize = 64;
const HOST_COMMAND_POLL_INTERVAL: Duration = Duration::from_millis(10);
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(5);
const CLIENT_TIMEOUT: Duration = Duration::from_secs(15);
/// Handshake is shorter than the post-auth idle timeout so unauthenticated
/// sockets cannot occupy a pre-auth slot for a full 15s.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const RELIABLE_ENQUEUE_TIMEOUT: Duration = Duration::from_millis(250);
/// Matches the host display cap (`message.chars().take(256)`).
const MAX_CHAT_CHARS: usize = 256;
const DEFAULT_POSE_RATE_PER_SECOND: u32 = 20;
const DEFAULT_CHAT_RATE_PER_SECOND: u32 = 8;
const PRE_AUTH_CONNECTION_MULTIPLIER: usize = 2;

#[derive(Clone, Debug)]
pub struct ServerConfig {
    pub catchup_queue_capacity: usize,
    pub catchup_drain_delay: Duration,
    pub max_players: usize,
    pub motd: String,
    pub whitelist: HashSet<String>,
    pub request_rate_per_second: u32,
    pub pose_rate_per_second: u32,
    pub chat_rate_per_second: u32,
    pub handshake_timeout: Duration,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            catchup_queue_capacity: MAX_CATCHUP_QUEUE_DEPTH,
            catchup_drain_delay: Duration::ZERO,
            max_players: 20,
            motd: "iCraft server".to_string(),
            whitelist: HashSet::new(),
            request_rate_per_second: 120,
            pose_rate_per_second: DEFAULT_POSE_RATE_PER_SECOND,
            chat_rate_per_second: DEFAULT_CHAT_RATE_PER_SECOND,
            handshake_timeout: HANDSHAKE_TIMEOUT,
        }
    }
}

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
    GameplayRequest {
        id: PlayerId,
        request: GameplayRequest,
    },
    ClientBlockChange {
        id: PlayerId,
        x: i32,
        y: i32,
        z: i32,
        block: u32,
        state: u8,
    },
    ClientBlockAction {
        id: PlayerId,
        action: Action,
        x: i32,
        y: i32,
        z: i32,
        block: u32,
        held_item: Option<crate::network::protocol::ItemWire>,
    },
    ChatFromClient {
        id: PlayerId,
        message: String,
    },
    CatchupAccepted {
        id: PlayerId,
        dimension: u8,
        cx: i32,
        cz: i32,
        revision: u64,
    },
    CatchupBackpressured {
        id: PlayerId,
        dimension: u8,
        cx: i32,
        cz: i32,
        revision: u64,
        mailbox_full_count: u64,
    },
    CatchupAck {
        id: PlayerId,
        dimension: u8,
        cx: i32,
        cz: i32,
        revision: u64,
    },
    ClientRespawnRequest {
        id: PlayerId,
    },
    ClientSleepRequest {
        id: PlayerId,
        bed_x: i32,
        bed_y: i32,
        bed_z: i32,
    },
    ContainerOpenRequest {
        id: PlayerId,
        dimension: u8,
        x: i32,
        y: i32,
        z: i32,
    },
    ContainerClickRequest {
        id: PlayerId,
        dimension: u8,
        revision: u64,
        slot_index: u16,
        is_left: bool,
        dragged: Option<crate::network::protocol::ItemWire>,
    },
    ContainerClose {
        id: PlayerId,
        dimension: u8,
        x: i32,
        y: i32,
        z: i32,
    },
}

#[derive(Debug)]
pub enum HostToServer {
    BroadcastBlockChange {
        dimension: u8,
        revision: u64,
        x: i32,
        y: i32,
        z: i32,
        block: u32,
        state: u8,
        raw_fluid: u8,
    },
    SendBlockChange {
        to: PlayerId,
        dimension: u8,
        revision: u64,
        x: i32,
        y: i32,
        z: i32,
        block: u32,
        state: u8,
        raw_fluid: u8,
    },
    BroadcastBlockEntityDelta {
        dimension: u8,
        revision: u64,
        x: i32,
        y: i32,
        z: i32,
        entity: Option<crate::block_entity::BlockEntity>,
    },
    SendBlockEntityDelta {
        to: PlayerId,
        dimension: u8,
        revision: u64,
        x: i32,
        y: i32,
        z: i32,
        entity: Option<crate::block_entity::BlockEntity>,
    },
    SendBlockActionResult {
        to: PlayerId,
        x: i32,
        y: i32,
        z: i32,
        success: bool,
        consumed_item: bool,
        drops: Vec<crate::network::protocol::ItemWire>,
    },
    SendChunk {
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
        to: PlayerId,
    },
    DisconnectCatchupClient {
        to: PlayerId,
        reason: String,
    },
    DisconnectClient {
        to: PlayerId,
        reason: String,
    },
    BroadcastEntitySpawn {
        dimension: u8,
        sequence: u64,
        state: EntityStateWire,
    },
    SendEntitySpawn {
        to: PlayerId,
        dimension: u8,
        sequence: u64,
        state: EntityStateWire,
    },
    BroadcastEntityState {
        dimension: u8,
        sequence: u64,
        state: EntityStateWire,
    },
    SendEntityState {
        to: PlayerId,
        dimension: u8,
        sequence: u64,
        state: EntityStateWire,
    },
    BroadcastEntityDespawn {
        dimension: u8,
        sequence: u64,
        entity_id: u64,
    },
    SendEntityDespawn {
        to: PlayerId,
        dimension: u8,
        sequence: u64,
        entity_id: u64,
    },
    BroadcastPlayerHealth {
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
    BroadcastPlayerEffect {
        sequence: u64,
        player_id: PlayerId,
        effects: Vec<PlayerEffectWire>,
    },
    SendPlayerEffect {
        to: PlayerId,
        sequence: u64,
        player_id: PlayerId,
        effects: Vec<PlayerEffectWire>,
    },
    SendPlayerSessionUpdate {
        to: PlayerId,
        sequence: u64,
        player_id: PlayerId,
        dimension: u8,
        state: SessionGameplayWire,
    },
    BroadcastTimeSync {
        ticks: u64,
        weather: u8,
        weather_remaining_ticks: f32,
    },
    BroadcastWorldRules {
        rules: crate::game_rules::WorldRules,
    },
    SendWorldRules {
        rules: crate::game_rules::WorldRules,
        to: PlayerId,
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
    SendPlayerPosition {
        to: PlayerId,
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
    SendContainerOpenResult {
        to: PlayerId,
        dimension: u8,
        success: bool,
        x: i32,
        y: i32,
        z: i32,
        slots: Vec<Option<crate::network::protocol::ItemWire>>,
        revision: u64,
    },
    /// Targeted lifecycle invalidation.  It maps to the existing v16
    /// `Packet::ContainerClose` wire shape and therefore does not require a
    /// protocol-version bump.
    SendContainerClose {
        to: PlayerId,
        dimension: u8,
        x: i32,
        y: i32,
        z: i32,
    },
    SendContainerClickResult {
        to: PlayerId,
        dimension: u8,
        success: bool,
        slot_index: u16,
        slot: Option<crate::network::protocol::ItemWire>,
        dragged: Option<crate::network::protocol::ItemWire>,
    },
    BroadcastContainerSlotUpdate {
        dimension: u8,
        revision: u64,
        x: i32,
        y: i32,
        z: i32,
        slot_index: u16,
        slot: Option<crate::network::protocol::ItemWire>,
    },
    SendContainerSlotUpdate {
        to: PlayerId,
        dimension: u8,
        revision: u64,
        x: i32,
        y: i32,
        z: i32,
        slot_index: u16,
        slot: Option<crate::network::protocol::ItemWire>,
    },
    SendPlayerRespawnResult {
        to: PlayerId,
        position: [f32; 3],
        dimension: u8,
    },
    BroadcastSleepStateSync {
        player_id: PlayerId,
        is_sleeping: bool,
    },
    SendGameplayResponse {
        to: PlayerId,
        response: GameplayResponse,
    },
    SendDimensionTransfer {
        to: PlayerId,
        dimension: u8,
        position: [f32; 3],
    },
    Stop,
}

const MAX_CATCHUP_QUEUE_DEPTH: usize = 32;

fn authenticate_handshake_username(raw: &str) -> Result<String, &'static str> {
    crate::save::normalize_player_identity(raw).map_err(|_| "invalid username")
}

struct ClientSession {
    id: PlayerId,
    username: String,
    out_tx: mpsc::Sender<QueuedPacket>,
    pose_mailbox: Arc<PoseMailbox>,
    state_mailbox: Arc<StateMailbox>,
    catchup_mailbox: Arc<CatchupMailbox>,
    cancel_tx: watch::Sender<bool>,
    gameplay: GameplaySessionState,
    metrics: NetworkMetrics,
}

/// Transport-side state for the authoritative gameplay envelope.  The
/// authority core owns the world mutation, while the network owns the
/// authenticated identity and bounded replay window needed before/after the
/// request crosses the host channel.
#[derive(Debug)]
struct GameplaySessionState {
    next_request_id: RequestId,
    last_client_sequence: u64,
    last_client_revision: u64,
    last_server_sequence: ServerSequence,
    response_cache: VecDeque<GameplayResponse>,
    in_flight: HashSet<RequestId>,
    active_container: Option<(u8, i32, i32, i32)>,
    current_dimension: u8,
}

impl Default for GameplaySessionState {
    fn default() -> Self {
        Self {
            next_request_id: 1,
            last_client_sequence: 0,
            last_client_revision: 0,
            last_server_sequence: 0,
            response_cache: VecDeque::with_capacity(crate::authority::RESPONSE_CACHE_CAPACITY),
            in_flight: HashSet::new(),
            active_container: None,
            current_dimension: 0,
        }
    }
}

impl GameplaySessionState {
    fn allocate_request_id(&mut self) -> RequestId {
        let id = self.next_request_id.max(1);
        self.next_request_id = id.wrapping_add(1).max(1);
        id
    }

    fn allocate_server_sequence(&mut self) -> ServerSequence {
        self.last_server_sequence = self.last_server_sequence.wrapping_add(1).max(1);
        self.last_server_sequence
    }

    fn cache_response(&mut self, response: GameplayResponse) {
        self.in_flight.remove(&response.request_id);
        if self.response_cache.len() >= crate::authority::RESPONSE_CACHE_CAPACITY {
            self.response_cache.pop_front();
        }
        self.response_cache.push_back(response);
    }

    fn cached_response(&self, request_id: RequestId) -> Option<GameplayResponse> {
        self.response_cache
            .iter()
            .find(|response| response.request_id == request_id)
            .cloned()
    }

    fn rejection(&mut self, request_id: RequestId, reason: RejectReason) -> GameplayResponse {
        let response = GameplayResponse {
            request_id,
            server_sequence: self.allocate_server_sequence(),
            outcome: crate::network::protocol::GameplayOutcome::Rejected { reason },
        };
        self.cache_response(response.clone());
        response
    }
}

struct RequestRateLimiter {
    window_started: Instant,
    count: u32,
    limit: u32,
}

impl RequestRateLimiter {
    fn new(limit: u32) -> Self {
        Self {
            window_started: Instant::now(),
            count: 0,
            limit: limit.max(1),
        }
    }

    fn allow(&mut self) -> bool {
        let now = Instant::now();
        if now.duration_since(self.window_started) >= Duration::from_secs(1) {
            self.window_started = now;
            self.count = 0;
        }
        if self.count >= self.limit {
            return false;
        }
        self.count += 1;
        true
    }
}

type Sessions = Arc<Mutex<HashMap<PlayerId, ClientSession>>>;

/// Host event transport is bounded in production (`SyncSender`) while tests
/// may use the legacy unbounded sender.  The trait keeps NetworkServer's
/// protocol logic independent of that queue choice and makes `try_send`
/// semantics explicit for bounded channels.
///
/// `Full` is per-connection ingress backpressure. Callers must not treat it
/// as a dead host or kick unrelated clients.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostEventSendError {
    Full,
    Closed,
}

pub trait HostEventSender: Clone + Send + Sync + 'static {
    fn send(&self, event: ServerToHost) -> Result<(), HostEventSendError>;
}

impl HostEventSender for std_mpsc::Sender<ServerToHost> {
    fn send(&self, event: ServerToHost) -> Result<(), HostEventSendError> {
        std_mpsc::Sender::send(self, event).map_err(|_| HostEventSendError::Closed)
    }
}

impl HostEventSender for std_mpsc::SyncSender<ServerToHost> {
    fn send(&self, event: ServerToHost) -> Result<(), HostEventSendError> {
        match self.try_send(event) {
            Ok(()) => Ok(()),
            Err(std_mpsc::TrySendError::Full(_)) => Err(HostEventSendError::Full),
            Err(std_mpsc::TrySendError::Disconnected(_)) => Err(HostEventSendError::Closed),
        }
    }
}

/// Production host-event boundary with exact backlog and saturation accounting.
/// The runtime decrements the same gauge only after `Receiver::try_recv` takes
/// ownership, so the snapshot is a queue gauge rather than a processed-event
/// estimate.
#[derive(Clone)]
pub(crate) struct MeteredHostEventSender {
    sender: std_mpsc::SyncSender<ServerToHost>,
    metrics: NetworkMetrics,
}

impl MeteredHostEventSender {
    pub(crate) fn new(sender: std_mpsc::SyncSender<ServerToHost>, metrics: NetworkMetrics) -> Self {
        Self { sender, metrics }
    }
}

impl HostEventSender for MeteredHostEventSender {
    fn send(&self, event: ServerToHost) -> Result<(), HostEventSendError> {
        // Increment before publishing: a consumer on another thread may take
        // the event as soon as `try_send` succeeds. Failed publication rolls
        // the reservation back, keeping the gauge race-free.
        self.metrics.enqueue();
        match self.sender.try_send(event) {
            Ok(()) => Ok(()),
            Err(std_mpsc::TrySendError::Full(_)) => {
                self.metrics.dequeue();
                self.metrics.record_queue_full();
                Err(HostEventSendError::Full)
            }
            Err(std_mpsc::TrySendError::Disconnected(_)) => {
                self.metrics.dequeue();
                Err(HostEventSendError::Closed)
            }
        }
    }
}

fn chat_exceeds_display_cap(message: &str) -> bool {
    message.chars().count() > MAX_CHAT_CHARS
}

struct PreAuthSlot {
    count: Arc<AtomicUsize>,
}

impl PreAuthSlot {
    fn try_acquire(count: &Arc<AtomicUsize>, cap: usize) -> Option<Self> {
        let cap = cap.max(1);
        loop {
            let current = count.load(Ordering::Relaxed);
            if current >= cap {
                return None;
            }
            if count
                .compare_exchange_weak(current, current + 1, Ordering::SeqCst, Ordering::Relaxed)
                .is_ok()
            {
                return Some(Self {
                    count: Arc::clone(count),
                });
            }
        }
    }
}

impl Drop for PreAuthSlot {
    fn drop(&mut self) {
        self.count.fetch_sub(1, Ordering::SeqCst);
    }
}

struct PoseMailbox {
    pending: Mutex<HashMap<PlayerId, TrackedPacket>>,
    notify: Notify,
    stats: Arc<crate::perf::SharedQueueStats>,
    metrics: NetworkMetrics,
}

impl Default for PoseMailbox {
    fn default() -> Self {
        Self::with_metrics(NetworkMetrics::default())
    }
}

impl PoseMailbox {
    fn with_metrics(metrics: NetworkMetrics) -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
            notify: Notify::new(),
            stats: crate::perf::queue_stats(crate::perf::QueueCategory::Outbound),
            metrics,
        }
    }

    async fn replace(&self, player_id: PlayerId, packet: Packet) {
        let bytes = packet_bytes(&packet);
        let mut pending = self.pending.lock().await;
        if let Some(old) = pending.insert(player_id, TrackedPacket::new(packet, &self.metrics)) {
            self.stats.dequeue(packet_bytes(old.packet()));
        }
        self.stats.enqueue(bytes, queue_now_ms());
        drop(pending);
        self.notify.notify_one();
    }

    async fn drain(&self) -> Vec<Packet> {
        let mut packets: Vec<_> = self
            .pending
            .lock()
            .await
            .drain()
            .map(|(_, packet)| packet.into_packet())
            .collect();
        for packet in &packets {
            self.stats.dequeue(packet_bytes(packet));
        }
        packets.sort_by_key(|packet| match packet {
            Packet::PlayerPosition { id, .. } => *id,
            _ => PlayerId::MAX,
        });
        packets
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum StateMailboxKey {
    Entity(u64),
    PlayerHealth(PlayerId),
    PlayerEffect(PlayerId),
    PlayerSession(PlayerId),
}

struct StateMailbox {
    pending: Mutex<HashMap<StateMailboxKey, TrackedPacket>>,
    notify: Notify,
    stats: Arc<crate::perf::SharedQueueStats>,
    metrics: NetworkMetrics,
}

impl Default for StateMailbox {
    fn default() -> Self {
        Self::with_metrics(NetworkMetrics::default())
    }
}

impl StateMailbox {
    fn with_metrics(metrics: NetworkMetrics) -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
            notify: Notify::new(),
            stats: crate::perf::queue_stats(crate::perf::QueueCategory::Outbound),
            metrics,
        }
    }

    async fn replace(&self, packet: Packet) {
        let (key, sequence) = match &packet {
            Packet::EntityState {
                sequence, state, ..
            } => (StateMailboxKey::Entity(state.entity_id), *sequence),
            Packet::PlayerHealth {
                sequence,
                player_id,
                ..
            } => (StateMailboxKey::PlayerHealth(*player_id), *sequence),
            Packet::PlayerEffect {
                sequence,
                player_id,
                ..
            } => (StateMailboxKey::PlayerEffect(*player_id), *sequence),
            Packet::PlayerSessionUpdate {
                sequence,
                player_id,
                ..
            } => (StateMailboxKey::PlayerSession(*player_id), *sequence),
            _ => return,
        };
        let mut pending = self.pending.lock().await;
        let existing_sequence = pending
            .get(&key)
            .and_then(|existing| match existing.packet() {
                Packet::EntityState { sequence, .. }
                | Packet::PlayerHealth { sequence, .. }
                | Packet::PlayerEffect { sequence, .. }
                | Packet::PlayerSessionUpdate { sequence, .. } => Some(*sequence),
                _ => None,
            });
        if existing_sequence.is_some_and(|existing| existing > sequence) {
            return;
        }
        let bytes = packet_bytes(&packet);
        if let Some(old) = pending.insert(key, TrackedPacket::new(packet, &self.metrics)) {
            self.stats.dequeue(packet_bytes(old.packet()));
        }
        self.stats.enqueue(bytes, queue_now_ms());
        drop(pending);
        self.notify.notify_one();
    }

    async fn drain(&self) -> Vec<Packet> {
        let mut packets: Vec<_> = self.pending.lock().await.drain().collect();
        for (_, packet) in &packets {
            self.stats.dequeue(packet_bytes(packet.packet()));
        }
        packets.sort_by_key(|(key, _)| *key);
        packets
            .into_iter()
            .map(|(_, packet)| packet.into_packet())
            .collect()
    }
}

struct CatchupMailbox {
    capacity: usize,
    pending: Mutex<VecDeque<TrackedPacket>>,
    notify: Notify,
    full_count: AtomicU64,
    stats: Arc<crate::perf::SharedQueueStats>,
    metrics: NetworkMetrics,
}

impl CatchupMailbox {
    fn with_capacity(capacity: usize) -> Self {
        Self::with_capacity_and_metrics(capacity, NetworkMetrics::default())
    }

    fn with_capacity_and_metrics(capacity: usize, metrics: NetworkMetrics) -> Self {
        Self {
            capacity: capacity.max(1),
            pending: Mutex::new(VecDeque::new()),
            notify: Notify::new(),
            full_count: AtomicU64::new(0),
            stats: crate::perf::queue_stats(crate::perf::QueueCategory::CatchUp),
            metrics,
        }
    }

    async fn replace(&self, packet: Packet) -> Result<(), u64> {
        let key = match &packet {
            Packet::ChunkData {
                dimension,
                cx,
                cz,
                revision,
                ..
            } => (*dimension, *cx, *cz, *revision),
            _ => return Ok(()),
        };
        let mut guard = self.pending.lock().await;
        let incoming_bytes = packet_bytes(&packet);
        if let Some(existing) = guard.iter_mut().find(|candidate| {
            matches!(
                candidate.packet(),
                Packet::ChunkData {
                    dimension,
                    cx,
                    cz,
                    ..
                } if (*dimension, *cx, *cz) == (key.0, key.1, key.2)
            )
        }) {
            let old_bytes = packet_bytes(existing.packet());
            let existing_revision = match existing.packet() {
                Packet::ChunkData { revision, .. } => *revision,
                _ => 0,
            };
            if key.3 >= existing_revision {
                *existing = TrackedPacket::new(packet, &self.metrics);
                self.stats.dequeue(old_bytes);
                self.stats.enqueue(incoming_bytes, queue_now_ms());
            }
            self.notify.notify_one();
            return Ok(());
        }
        if guard.len() >= self.capacity {
            let count = self.full_count.fetch_add(1, Ordering::Relaxed) + 1;
            self.stats.drop_item();
            self.metrics.record_queue_full();
            return Err(count);
        }
        let bytes = packet_bytes(&packet);
        guard.push_back(TrackedPacket::new(packet, &self.metrics));
        self.stats.enqueue(bytes, queue_now_ms());
        self.notify.notify_one();
        Ok(())
    }

    async fn pop(&self) -> Option<Packet> {
        let mut guard = self.pending.lock().await;
        let packet = guard.pop_front();
        if let Some(packet) = &packet {
            self.stats.dequeue(packet_bytes(packet.packet()));
        }
        if !guard.is_empty() {
            self.notify.notify_one();
        }
        packet.map(TrackedPacket::into_packet)
    }

    #[allow(dead_code)]
    async fn len(&self) -> usize {
        self.pending.lock().await.len()
    }
}

impl Default for CatchupMailbox {
    fn default() -> Self {
        Self::with_capacity(MAX_CATCHUP_QUEUE_DEPTH)
    }
}

async fn queue_initial_roster(
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
        permit.send(QueuedPacket::Outbound(TrackedPacket::new(packet, metrics)));
    }
    Ok(())
}

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
        host_to_server: std_mpsc::Receiver<HostToServer>,
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
        host_to_server: std_mpsc::Receiver<HostToServer>,
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
        host_to_server: std_mpsc::Receiver<HostToServer>,
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
        host_to_server: std_mpsc::Receiver<HostToServer>,
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

    async fn run(self, listener: TcpListener, host_to_server: std_mpsc::Receiver<HostToServer>) {
        // Polling try_recv keeps the blocking std receiver off Tokio's workers and,
        // unlike spawn_blocking(recv), lets runtime shutdown finish immediately.
        let mut command_tick = time::interval(HOST_COMMAND_POLL_INTERVAL);

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
                                Self::run_client(
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
                _ = command_tick.tick() => {
                    let mut latest_positions = HashMap::new();
                    loop {
                        match host_to_server.try_recv() {
                            Ok(HostToServer::Stop) => {
                                queue_stats().dequeue(std::mem::size_of::<HostToServer>() as u64);
                                self.metrics.dequeue();
                                return
                            }
                            Ok(command @ HostToServer::BroadcastPlayerPosition { id, .. }) => {
                                queue_stats().dequeue(std::mem::size_of::<HostToServer>() as u64);
                                self.metrics.dequeue();
                                latest_positions.insert(id, command);
                            }
                            Ok(command) => {
                                queue_stats().dequeue(std::mem::size_of::<HostToServer>() as u64);
                                self.metrics.dequeue();
                                self.handle_host_command(command).await
                            }
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

    fn prepare_gameplay_request(
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

    fn legacy_gameplay_request(
        session: &mut ClientSession,
        dimension: u8,
        client_revision: u64,
        operation: GameplayOperation,
    ) -> GameplayRequest {
        let session_id = session.id;
        Self::prepare_gameplay_request(
            session,
            GameplayRequest {
                request_id: 0,
                client_sequence: 0,
                session_id,
                dimension,
                client_revision,
                operation,
            },
        )
    }

    /// Apply transport/session gates once for both native envelopes and legacy
    /// adapters. A rejection is sent with a real server sequence and cached;
    /// accepted requests are forwarded exactly once to the authority channel.
    async fn route_gameplay_request(
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
            request = Self::prepare_gameplay_request(session, request);
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
            let failed = Self::send_to(
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
                    let failed = Self::send_to(
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

    async fn run_client(
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
        // transport session.  The handshake preflight above is only an early
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
                            if let Err(reason) = Self::route_gameplay_request(
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
                                Self::legacy_gameplay_request(
                                    session,
                                    dimension,
                                    revision,
                                    GameplayOperation::BlockUse { x, y, z, block },
                                )
                            };
                            if let Err(reason) = Self::route_gameplay_request(
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
                            let Some(operation) = GameplayOperation::from_legacy_block_action(
                                action, x, y, z, block, held_item,
                            ) else {
                                // Action::Use has no BlockAction kind. Do not
                                // invent Place/StartBreak or fall back to BlockUse.
                                continue;
                            };
                            let request = {
                                let mut sessions_guard = sessions.lock().await;
                                let Some(session) = sessions_guard.get_mut(&id) else {
                                    disconnect_reason = "authenticated session disappeared".into();
                                    break;
                                };
                                Self::legacy_gameplay_request(
                                    session,
                                    session.gameplay.current_dimension,
                                    session.gameplay.last_client_revision,
                                    operation,
                                )
                            };
                            if let Err(reason) = Self::route_gameplay_request(
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
                                Self::legacy_gameplay_request(
                                    session,
                                    dimension,
                                    revision,
                                    GameplayOperation::Sleep { x, y, z },
                                )
                            };
                            if let Err(reason) = Self::route_gameplay_request(
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
                                Self::legacy_gameplay_request(
                                    session,
                                    dimension,
                                    session.gameplay.last_client_revision,
                                    GameplayOperation::Container {
                                        action: 0,
                                        x,
                                        y,
                                        z,
                                        slot: 0,
                                    },
                                )
                            };
                            if let Err(reason) = Self::route_gameplay_request(
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
                                            let operation = if dragged.is_some() || !is_left {
                                                GameplayOperation::ContainerClick {
                                                    x,
                                                    y,
                                                    z,
                                                    slot: slot_index,
                                                    is_left,
                                                    dragged,
                                                }
                                            } else {
                                                GameplayOperation::Container {
                                                    // The legacy boolean selects click
                                                    // semantics, while the envelope action
                                                    // identifies the container operation.
                                                    action: 1,
                                                    x,
                                                    y,
                                                    z,
                                                    slot: slot_index,
                                                }
                                            };
                                            Some(Self::legacy_gameplay_request(
                                                session,
                                                dimension,
                                                revision,
                                                operation,
                                            ))
                                        }
                                    _ => None,
                                }
                            };
                            let Some(request) = request else {
                                continue;
                            };
                            if let Err(reason) = Self::route_gameplay_request(
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
                                let request = Self::legacy_gameplay_request(
                                    session,
                                    dimension,
                                    session.gameplay.last_client_revision,
                                    GameplayOperation::Container {
                                        action: 2,
                                        x,
                                        y,
                                        z,
                                        slot: 0,
                                    },
                                );
                                session.gameplay.active_container = None;
                                request
                            };
                            if let Err(reason) = Self::route_gameplay_request(
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
        Self::remove_client(id, &sessions, &server_to_host).await;
        send_task.abort();
        send_task.abort();
    }

    async fn remove_client(id: PlayerId, sessions: &Sessions, server_to_host: &S) {
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

    async fn normalize_host_response(
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

    async fn handle_host_command(&self, command: HostToServer) {
        if let HostToServer::SendPlayerSessionUpdate { to, player_id, .. } = &command {
            if to != player_id {
                self.metrics.record_rejected_request();
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
            let mailbox = self
                .sessions
                .lock()
                .await
                .get(&to)
                .map(|session| Arc::clone(&session.catchup_mailbox));
            if let Some(mailbox) = mailbox {
                match mailbox.replace(packet).await {
                    Ok(()) => {
                        let _ = self.server_to_host.send(ServerToHost::CatchupAccepted {
                            id: to,
                            dimension,
                            cx,
                            cz,
                            revision,
                        });
                    }
                    Err(mailbox_full_count) => {
                        let _ = self
                            .server_to_host
                            .send(ServerToHost::CatchupBackpressured {
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
            Self::evict_slow_clients(&self.sessions, &self.server_to_host, vec![to]).await;
            return;
        }

        if let HostToServer::DisconnectClient { to, reason } = &command {
            let failed = Self::send_to(
                &self.sessions,
                *to,
                Packet::Disconnect {
                    protocol_version: PROTOCOL_VERSION,
                    reason: reason.clone(),
                },
            )
            .await;
            Self::evict_slow_clients(&self.sessions, &self.server_to_host, failed).await;
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
            Self::broadcast_pose_inner(
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
            let mailbox = self
                .sessions
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
            let mailbox = self
                .sessions
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
            Self::broadcast_state(&self.sessions, packet).await;
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
                let response = Self::normalize_host_response(&self.sessions, to, response).await;
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
                if let Some(session) = self.sessions.lock().await.get_mut(&to) {
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

    async fn broadcast_reliably(sessions: &Sessions, packet: Packet) -> Vec<PlayerId> {
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

    async fn evict_slow_clients(sessions: &Sessions, server_to_host: &S, initial: Vec<PlayerId>) {
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

    async fn broadcast_pose_inner(sessions: &Sessions, packet: Packet) {
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

    async fn broadcast_state(sessions: &Sessions, packet: Packet) {
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

    async fn broadcast_to(sessions: &Sessions, packet: Packet) {
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
}

impl NetworkServer<std_mpsc::Sender<ServerToHost>> {
    /// Compatibility entry point for headless tests that exercise pose
    /// coalescing without constructing a full server transport.
    async fn broadcast_pose(sessions: &Sessions, packet: Packet) {
        Self::broadcast_pose_inner(sessions, packet).await;
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

            let (host_tx, host_rx) = std_mpsc::channel();
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

    #[test]
    fn metered_host_event_queue_counts_success_full_and_receive_once() {
        let (tx, rx) = std_mpsc::sync_channel(1);
        let metrics = NetworkMetrics::default();
        let sender = MeteredHostEventSender::new(tx, metrics.clone());
        assert!(sender
            .send(ServerToHost::Disconnected {
                reason: "first".into(),
            })
            .is_ok());
        assert_eq!(metrics.snapshot().queue_depth, 1);
        assert_eq!(
            sender.send(ServerToHost::Disconnected {
                reason: "full".into(),
            }),
            Err(HostEventSendError::Full)
        );
        assert_eq!(metrics.snapshot().queue_depth, 1);
        assert_eq!(metrics.snapshot().queue_full, 1);

        let _ = rx.try_recv().expect("runtime takes one metered event");
        metrics.dequeue();
        assert_eq!(metrics.snapshot().queue_depth, 0);
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

        let shutdown_metrics = server.stop().await;
        assert_eq!(shutdown_metrics.queue_depth, 0);
    }

    #[tokio::test]
    async fn transport_metrics_count_exact_successful_tcp_frames() {
        let server = TestServer::start(0xCAFE_BABE, 1);
        let handshake = Packet::Handshake {
            protocol_version: PROTOCOL_VERSION,
            username: "wire-metrics".into(),
        };
        let (_client, id) = server.connect("wire-metrics").await;
        let login = Packet::LoginSuccess {
            protocol_version: PROTOCOL_VERSION,
            player_id: id,
            seed: 0xCAFE_BABE,
            gamemode: 1,
        };

        let metrics = server.metrics.snapshot();
        assert_eq!(metrics.inbound_packets, 1);
        assert_eq!(metrics.inbound_bytes, packet_bytes(&handshake));
        assert_eq!(metrics.outbound_packets, 1);
        assert_eq!(metrics.outbound_bytes, packet_bytes(&login));
        assert_eq!(metrics.queue_depth, 0);

        let shutdown_metrics = server.stop().await;
        assert_eq!(shutdown_metrics.queue_depth, 0);
    }

    #[tokio::test]
    async fn outbound_metrics_publish_before_write_and_rollback_on_failure() {
        let metrics = NetworkMetrics::default();
        let packet = Packet::Keepalive {
            protocol_version: PROTOCOL_VERSION,
        };
        let expected_bytes = packet_bytes(&packet);
        let observed = metrics.clone();
        assert!(
            send_with_outbound_metrics(&packet, &metrics, || async move {
                // The reservation is visible before the write future can publish
                // the frame to its peer.
                let snapshot = observed.snapshot();
                assert_eq!(snapshot.outbound_packets, 1);
                assert_eq!(snapshot.outbound_bytes, expected_bytes);
                Ok(())
            })
            .await
            .is_ok()
        );
        let successful = metrics.snapshot();
        assert_eq!(successful.outbound_packets, 1);
        assert_eq!(successful.outbound_bytes, expected_bytes);

        let failed = send_with_outbound_metrics(&packet, &metrics, || async {
            Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "injected frame write failure",
            ))
        })
        .await;
        assert!(failed.is_err());
        assert_eq!(metrics.snapshot(), successful);
    }

    #[tokio::test]
    async fn queue_metrics_track_backlog_replacement_drain_and_saturation() {
        let metrics = NetworkMetrics::default();
        let (tx, mut rx) = mpsc::channel(1);
        best_effort_send(
            &tx,
            Packet::Keepalive {
                protocol_version: PROTOCOL_VERSION,
            },
            &metrics,
        );
        assert_eq!(metrics.snapshot().queue_depth, 1);

        best_effort_send(
            &tx,
            Packet::Keepalive {
                protocol_version: PROTOCOL_VERSION,
            },
            &metrics,
        );
        assert_eq!(metrics.snapshot().queue_depth, 1);
        assert_eq!(metrics.snapshot().queue_full, 1);

        let packet = rx.recv().await.unwrap();
        match packet {
            QueuedPacket::Reliable(packet) | QueuedPacket::Outbound(packet) => {
                let _ = packet.into_packet();
            }
            QueuedPacket::ReliableWithAck(packet, completion) => {
                let _ = completion.send(true);
                let _ = packet.into_packet();
            }
        }
        assert_eq!(metrics.snapshot().queue_depth, 0);

        let mailbox = PoseMailbox::with_metrics(metrics.clone());
        for sequence in [1, 2] {
            mailbox
                .replace(
                    7,
                    Packet::PlayerPosition {
                        protocol_version: PROTOCOL_VERSION,
                        id: 7,
                        sequence,
                        sender_time_millis: u64::from(sequence),
                        x: 0.0,
                        y: 64.0,
                        z: 0.0,
                        yaw: 0.0,
                        pitch: 0.0,
                    },
                )
                .await;
            assert_eq!(metrics.snapshot().queue_depth, 1);
        }
        assert_eq!(mailbox.drain().await.len(), 1);
        assert_eq!(metrics.snapshot().queue_depth, 0);
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
            .unwrap();
        assert!(tokio::time::timeout(
            Duration::from_millis(100),
            recv_matching(&mut client_a, |packet| matches!(packet, Packet::PlayerSessionUpdate { state, .. } if state.revision == 2))
        )
        .await
        .is_err());

        server.stop().await;
    }

    #[tokio::test]
    async fn catchup_mailbox_is_latest_wins_and_bounded() {
        let mailbox = CatchupMailbox::with_capacity(1);
        let p1 = Packet::ChunkData {
            protocol_version: PROTOCOL_VERSION,
            dimension: 0,
            cx: 1,
            cz: 2,
            revision: 1,
            min_section_y: 0,
            section_count: 16,
            blocks: vec![1],
            block_states: vec![],
            fluid_levels: vec![],
            block_entities: vec![],
        };
        let p2 = Packet::ChunkData {
            protocol_version: PROTOCOL_VERSION,
            dimension: 0,
            cx: 1,
            cz: 2,
            revision: 2,
            min_section_y: 0,
            section_count: 16,
            blocks: vec![2],
            block_states: vec![],
            fluid_levels: vec![],
            block_entities: vec![],
        };
        let p3 = Packet::ChunkData {
            protocol_version: PROTOCOL_VERSION,
            dimension: 0,
            cx: 2,
            cz: 2,
            revision: 1,
            min_section_y: 0,
            section_count: 16,
            blocks: vec![3],
            block_states: vec![],
            fluid_levels: vec![],
            block_entities: vec![],
        };
        assert!(mailbox.replace(p1).await.is_ok());
        assert_eq!(mailbox.len().await, 1);
        assert!(mailbox.replace(p2.clone()).await.is_ok());
        assert_eq!(mailbox.len().await, 1);
        assert_eq!(mailbox.replace(p3.clone()).await, Err(1));

        assert_eq!(mailbox.pop().await, Some(p2));
        assert!(mailbox.replace(p3.clone()).await.is_ok());
        assert_eq!(mailbox.pop().await, Some(p3));
    }

    #[tokio::test]
    async fn catchup_mailbox_preserves_distance_priority_insertion_order() {
        let mailbox = CatchupMailbox::with_capacity(2);
        let near = Packet::ChunkData {
            protocol_version: PROTOCOL_VERSION,
            dimension: 0,
            cx: 10,
            cz: 10,
            revision: 1,
            min_section_y: 0,
            section_count: 16,
            blocks: vec![1],
            block_states: vec![],
            fluid_levels: vec![],
            block_entities: vec![],
        };
        let farther = Packet::ChunkData {
            protocol_version: PROTOCOL_VERSION,
            dimension: 0,
            cx: -10,
            cz: -10,
            revision: 1,
            min_section_y: 0,
            section_count: 16,
            blocks: vec![2],
            block_states: vec![],
            fluid_levels: vec![],
            block_entities: vec![],
        };
        mailbox.replace(near.clone()).await.unwrap();
        mailbox.replace(farther.clone()).await.unwrap();
        assert_eq!(mailbox.pop().await, Some(near));
        assert_eq!(mailbox.pop().await, Some(farther));
    }

    #[tokio::test]
    async fn slow_client_backpressure_does_not_starve_other_mailboxes() {
        let metrics = NetworkMetrics::default();
        let slow = CatchupMailbox::with_capacity_and_metrics(1, metrics.clone());
        let fast = CatchupMailbox::with_capacity_and_metrics(1, metrics.clone());
        let packet = |cx, value| Packet::ChunkData {
            protocol_version: PROTOCOL_VERSION,
            dimension: 0,
            cx,
            cz: 0,
            revision: 1,
            min_section_y: 0,
            section_count: 16,
            blocks: vec![value],
            block_states: vec![],
            fluid_levels: vec![],
            block_entities: vec![],
        };

        slow.replace(packet(0, 1)).await.unwrap();
        assert_eq!(slow.replace(packet(1, 2)).await, Err(1));
        assert_eq!(metrics.snapshot().queue_depth, 1);
        assert_eq!(metrics.snapshot().queue_full, 1);
        fast.replace(packet(2, 3)).await.unwrap();
        assert_eq!(metrics.snapshot().queue_depth, 2);
        assert_eq!(fast.pop().await, Some(packet(2, 3)));
        assert_eq!(slow.len().await, 1);
        assert_eq!(metrics.snapshot().queue_depth, 1);
        drop(slow);
        assert_eq!(metrics.snapshot().queue_depth, 0);
    }

    #[tokio::test]
    async fn entity_state_mailbox_is_latest_wins_per_entity() {
        let mailbox = StateMailbox::default();
        let state = |entity_id, x| EntityStateWire {
            entity_id,
            entity_type: 0,
            position: [x, 64.0, 0.0],
            velocity: [0.0; 3],
            yaw: 0.0,
            pitch: 0.0,
            health: 20.0,
            animation_state: 0,
            item: None,
        };
        mailbox
            .replace(Packet::EntityState {
                protocol_version: PROTOCOL_VERSION,
                dimension: 0,
                sequence: 1,
                state: state(7, 1.0),
            })
            .await;
        mailbox
            .replace(Packet::EntityState {
                protocol_version: PROTOCOL_VERSION,
                dimension: 0,
                sequence: 2,
                state: state(7, 2.0),
            })
            .await;
        mailbox
            .replace(Packet::EntityState {
                protocol_version: PROTOCOL_VERSION,
                dimension: 0,
                sequence: 1,
                state: state(7, -1.0),
            })
            .await;
        mailbox
            .replace(Packet::EntityState {
                protocol_version: PROTOCOL_VERSION,
                dimension: 0,
                sequence: 2,
                state: state(8, 8.0),
            })
            .await;

        let packets = mailbox.drain().await;
        assert_eq!(packets.len(), 2);
        assert!(packets.iter().any(|packet| matches!(
            packet,
            Packet::EntityState {
                sequence: 2,
                state,
                ..
            } if state.entity_id == 7 && state.position[0] == 2.0
        )));
        assert!(packets.iter().any(|packet| matches!(
            packet,
            Packet::EntityState { state, .. } if state.entity_id == 8
        )));
    }

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

    #[test]
    fn chat_display_cap_is_256_chars() {
        assert!(!chat_exceeds_display_cap(&"a".repeat(256)));
        assert!(chat_exceeds_display_cap(&"a".repeat(257)));
        assert!(!chat_exceeds_display_cap(&"😀".repeat(256)));
        assert!(chat_exceeds_display_cap(&"😀".repeat(257)));
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
        let (host_tx, host_rx) = std_mpsc::channel();
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
        let _ = host_tx.send(HostToServer::Stop);
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
            .send(HostToServer::BroadcastContainerSlotUpdate {
                dimension: 0,
                revision: 11,
                x: 8,
                y: 80,
                z: 8,
                slot_index: 3,
                slot: None,
            })
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
