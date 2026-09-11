use std::collections::{HashMap, HashSet, VecDeque};
use std::future::Future;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{mpsc, oneshot, watch, Mutex, Notify};
use tokio::time::{self, Instant};

use super::channels::MAX_CATCHUP_QUEUE_DEPTH;
use super::protocol::{
    GameplayResponse, Packet, PlayerId, RejectReason, RequestId, ServerSequence,
};
use super::transport::Connection;

pub(crate) const CLIENT_QUEUE_CAPACITY: usize = 64;
pub(crate) const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(5);
pub(crate) const CLIENT_TIMEOUT: Duration = Duration::from_secs(15);
/// Handshake is shorter than the post-auth idle timeout so unauthenticated
/// sockets cannot occupy a pre-auth slot for a full 15s.
pub(crate) const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
pub(crate) const RELIABLE_ENQUEUE_TIMEOUT: Duration = Duration::from_millis(250);
/// Matches the host display cap (`message.chars().take(256)`).
pub(crate) const MAX_CHAT_CHARS: usize = 256;
pub(crate) const DEFAULT_POSE_RATE_PER_SECOND: u32 = 20;
pub(crate) const DEFAULT_CHAT_RATE_PER_SECOND: u32 = 8;
pub(crate) const PRE_AUTH_CONNECTION_MULTIPLIER: usize = 2;

#[derive(Clone, Default)]
pub struct NetworkMetrics {
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
pub struct NetworkMetricsSnapshot {
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

    pub(crate) fn record_inbound(&self, packet: &Packet) {
        Self::add(&self.inner.inbound_packets, 1);
        Self::add(&self.inner.inbound_bytes, packet_bytes(packet));
    }

    /// Reserve the outbound frame counters before a socket write begins. A
    /// peer may observe a successful frame as soon as `write_all` completes,
    /// so publishing after the await leaves a visibility race. The guard
    /// rolls the reservation back when the write fails.
    pub(crate) fn reserve_outbound_bytes(&self, bytes: u64) -> OutboundMetricReservation {
        Self::add(&self.inner.outbound_packets, 1);
        Self::add(&self.inner.outbound_bytes, bytes);
        OutboundMetricReservation {
            metrics: self.clone(),
            bytes,
            committed: false,
        }
    }

    pub(crate) fn reserve_outbound(&self, packet: &Packet) -> OutboundMetricReservation {
        self.reserve_outbound_bytes(packet_bytes(packet))
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

    pub(crate) fn record_rejected_request(&self) {
        Self::add(&self.inner.rejected_requests, 1);
    }

    pub(crate) fn record_duplicate_request(&self) {
        Self::add(&self.inner.duplicate_requests, 1);
    }

    pub fn snapshot(&self) -> NetworkMetricsSnapshot {
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

pub(crate) struct OutboundMetricReservation {
    metrics: NetworkMetrics,
    bytes: u64,
    committed: bool,
}

impl OutboundMetricReservation {
    pub(crate) fn commit(mut self) {
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

/// Pre-encoded outbound packet. The bincode payload is shared via `Arc` so
/// broadcast/fanout meters and writes the same bytes without re-serializing.
///
/// Assumption: authenticated live sessions speak a single `PROTOCOL_VERSION`
/// (handshake rejects others). Shared payload Arcs are therefore never mixed
/// across protocol versions on the current wire.
#[derive(Clone, Debug)]
pub(crate) struct EncodedPacket {
    packet: Packet,
    payload: Arc<[u8]>,
    protocol_version: u32,
}

impl EncodedPacket {
    pub(crate) fn new(packet: Packet) -> Result<Self, &'static str> {
        // Live sessions speak one protocol after handshake; the connection
        // holds the negotiated version, so payloads no longer embed it.
        let protocol_version = crate::network::protocol::PROTOCOL_VERSION;
        let payload = packet.encode_payload()?;
        Ok(Self {
            packet,
            payload: Arc::<[u8]>::from(payload),
            protocol_version,
        })
    }

    pub(crate) fn packet(&self) -> &Packet {
        &self.packet
    }

    pub(crate) fn payload(&self) -> &Arc<[u8]> {
        &self.payload
    }

    pub(crate) fn protocol_version(&self) -> u32 {
        self.protocol_version
    }

    /// TCP frame size: 4-byte BE length prefix + payload.
    pub(crate) fn frame_bytes(&self) -> u64 {
        frame_byte_len(self.payload.len())
    }

    pub(crate) fn into_packet(self) -> Packet {
        self.packet
    }
}

impl PartialEq for EncodedPacket {
    fn eq(&self, other: &Self) -> bool {
        self.packet == other.packet
    }
}

impl PartialEq<Packet> for EncodedPacket {
    fn eq(&self, other: &Packet) -> bool {
        &self.packet == other
    }
}

pub(crate) struct TrackedPacket {
    encoded: Option<EncodedPacket>,
    metrics: NetworkMetrics,
}

impl TrackedPacket {
    pub(crate) fn new(encoded: EncodedPacket, metrics: &NetworkMetrics) -> Self {
        metrics.enqueue();
        Self {
            encoded: Some(encoded),
            metrics: metrics.clone(),
        }
    }

    pub(crate) fn try_from_packet(packet: Packet, metrics: &NetworkMetrics) -> Option<Self> {
        EncodedPacket::new(packet)
            .ok()
            .map(|encoded| Self::new(encoded, metrics))
    }

    pub(crate) fn packet(&self) -> &Packet {
        self.encoded
            .as_ref()
            .expect("queued packet is present until it leaves its backlog")
            .packet()
    }

    pub(crate) fn encoded(&self) -> &EncodedPacket {
        self.encoded
            .as_ref()
            .expect("queued packet is present until it leaves its backlog")
    }

    pub(crate) fn into_encoded(mut self) -> EncodedPacket {
        self.metrics.dequeue();
        self.encoded
            .take()
            .expect("queued packet is consumed exactly once")
    }

    pub(crate) fn into_packet(self) -> Packet {
        self.into_encoded().into_packet()
    }
}

impl Drop for TrackedPacket {
    fn drop(&mut self) {
        if self.encoded.is_some() {
            self.metrics.dequeue();
        }
    }
}

pub(crate) enum QueuedPacket {
    Reliable(TrackedPacket),
    ReliableWithAck(TrackedPacket, oneshot::Sender<bool>),
    Outbound(TrackedPacket),
}

/// TCP frame byte count from an already-encoded payload length.
pub(crate) fn frame_byte_len(payload_len: usize) -> u64 {
    (payload_len as u64).saturating_add(4)
}

pub(crate) fn packet_bytes(packet: &Packet) -> u64 {
    // `ConnectionWriter` emits a four-byte big-endian frame length before the
    // bincode payload. Count the bytes that actually cross TCP, not just the
    // serialized message body. Prefer `EncodedPacket::frame_bytes` on the
    // outbound hot path so metering reuses the same encode.
    frame_byte_len(packet.encode().len())
}

pub(crate) fn queue_stats() -> Arc<crate::perf::SharedQueueStats> {
    crate::perf::queue_stats(crate::perf::QueueCategory::Outbound)
}

pub(crate) fn queue_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis().min(u64::MAX as u128) as u64)
}

pub(crate) async fn reliable_send(
    tx: &mpsc::Sender<QueuedPacket>,
    packet: Packet,
    metrics: &NetworkMetrics,
) -> bool {
    let Ok(encoded) = EncodedPacket::new(packet) else {
        return false;
    };
    reliable_send_encoded(tx, encoded, metrics).await
}

pub(crate) async fn reliable_send_encoded(
    tx: &mpsc::Sender<QueuedPacket>,
    encoded: EncodedPacket,
    metrics: &NetworkMetrics,
) -> bool {
    let bytes = encoded.frame_bytes();
    let stats = crate::perf::queue_stats(crate::perf::QueueCategory::Reliable);
    match time::timeout(RELIABLE_ENQUEUE_TIMEOUT, tx.reserve()).await {
        Ok(Ok(permit)) => {
            stats.enqueue(bytes, queue_now_ms());
            permit.send(QueuedPacket::Reliable(TrackedPacket::new(encoded, metrics)));
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

pub(crate) async fn reliable_send_and_wait(
    tx: &mpsc::Sender<QueuedPacket>,
    packet: Packet,
    metrics: &NetworkMetrics,
) -> bool {
    let Ok(encoded) = EncodedPacket::new(packet) else {
        return false;
    };
    let bytes = encoded.frame_bytes();
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
        TrackedPacket::new(encoded, metrics),
        completion_tx,
    ));
    matches!(
        time::timeout(CLIENT_TIMEOUT, completion_rx).await,
        Ok(Ok(true))
    )
}

pub(crate) fn best_effort_send(
    tx: &mpsc::Sender<QueuedPacket>,
    packet: Packet,
    metrics: &NetworkMetrics,
) {
    let Ok(encoded) = EncodedPacket::new(packet) else {
        return;
    };
    best_effort_send_encoded(tx, encoded, metrics);
}

pub(crate) fn best_effort_send_encoded(
    tx: &mpsc::Sender<QueuedPacket>,
    encoded: EncodedPacket,
    metrics: &NetworkMetrics,
) {
    let bytes = encoded.frame_bytes();
    match tx.try_reserve() {
        Ok(permit) => {
            queue_stats().enqueue(bytes, queue_now_ms());
            permit.send(QueuedPacket::Outbound(TrackedPacket::new(encoded, metrics)));
        }
        Err(mpsc::error::TrySendError::Full(_)) => {
            queue_stats().drop_item();
            metrics.record_queue_full();
        }
        Err(mpsc::error::TrySendError::Closed(_)) => queue_stats().drop_item(),
    }
}

pub(crate) async fn send_connection_packet(
    connection: &mut Connection,
    packet: Packet,
    metrics: &NetworkMetrics,
) -> std::io::Result<()> {
    let encoded = EncodedPacket::new(packet)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    send_encoded_connection_packet(connection, &encoded, metrics).await
}

pub(crate) async fn send_encoded_connection_packet(
    connection: &mut Connection,
    encoded: &EncodedPacket,
    metrics: &NetworkMetrics,
) -> std::io::Result<()> {
    send_with_outbound_byte_metrics(encoded.frame_bytes(), metrics, || {
        connection.send_payload(encoded.payload())
    })
    .await
}

pub(crate) async fn send_writer_packet(
    writer: &mut super::transport::ConnectionWriter,
    packet: &Packet,
    metrics: &NetworkMetrics,
) -> std::io::Result<()> {
    let payload = packet
        .encode_payload()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    send_with_outbound_byte_metrics(frame_byte_len(payload.len()), metrics, || {
        writer.send_payload(&payload)
    })
    .await
}

pub(crate) async fn send_encoded_packet(
    writer: &mut super::transport::ConnectionWriter,
    encoded: &EncodedPacket,
    metrics: &NetworkMetrics,
) -> std::io::Result<()> {
    send_with_outbound_byte_metrics(encoded.frame_bytes(), metrics, || {
        writer.send_payload(encoded.payload())
    })
    .await
}

pub(crate) async fn send_with_outbound_metrics<F, Fut>(
    packet: &Packet,
    metrics: &NetworkMetrics,
    send: F,
) -> std::io::Result<()>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = std::io::Result<()>>,
{
    send_with_outbound_byte_metrics(packet_bytes(packet), metrics, send).await
}

pub(crate) async fn send_with_outbound_byte_metrics<F, Fut>(
    bytes: u64,
    metrics: &NetworkMetrics,
    send: F,
) -> std::io::Result<()>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = std::io::Result<()>>,
{
    let reservation = metrics.reserve_outbound_bytes(bytes);
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

pub(crate) struct PreAuthSlot {
    count: Arc<AtomicUsize>,
}

impl PreAuthSlot {
    pub(crate) fn try_acquire(count: &Arc<AtomicUsize>, cap: usize) -> Option<Self> {
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

pub(crate) struct PoseMailbox {
    pub(crate) pending: Mutex<HashMap<PlayerId, TrackedPacket>>,
    pub(crate) notify: Notify,
    pub(crate) stats: Arc<crate::perf::SharedQueueStats>,
    pub(crate) metrics: NetworkMetrics,
}

impl Default for PoseMailbox {
    fn default() -> Self {
        Self::with_metrics(NetworkMetrics::default())
    }
}

impl PoseMailbox {
    pub(crate) fn with_metrics(metrics: NetworkMetrics) -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
            notify: Notify::new(),
            stats: crate::perf::queue_stats(crate::perf::QueueCategory::Outbound),
            metrics,
        }
    }

    pub(crate) async fn replace(&self, player_id: PlayerId, packet: Packet) {
        let Ok(encoded) = EncodedPacket::new(packet) else {
            return;
        };
        self.replace_encoded(player_id, encoded).await;
    }

    pub(crate) async fn replace_encoded(&self, player_id: PlayerId, encoded: EncodedPacket) {
        let bytes = encoded.frame_bytes();
        let mut pending = self.pending.lock().await;
        if let Some(old) = pending.insert(player_id, TrackedPacket::new(encoded, &self.metrics)) {
            self.stats.dequeue(old.encoded().frame_bytes());
        }
        self.stats.enqueue(bytes, queue_now_ms());
        drop(pending);
        self.notify.notify_one();
    }

    pub(crate) async fn drain(&self) -> Vec<EncodedPacket> {
        let mut packets: Vec<_> = self
            .pending
            .lock()
            .await
            .drain()
            .map(|(_, packet)| packet.into_encoded())
            .collect();
        for packet in &packets {
            self.stats.dequeue(packet.frame_bytes());
        }
        packets.sort_by_key(|packet| match packet.packet() {
            Packet::PlayerPosition { id, .. } => *id,
            _ => PlayerId::MAX,
        });
        packets
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum StateMailboxKey {
    Entity(u64),
    PlayerHealth(PlayerId),
    PlayerEffect(PlayerId),
    PlayerSession(PlayerId),
}

pub(crate) struct StateMailbox {
    pub(crate) pending: Mutex<HashMap<StateMailboxKey, TrackedPacket>>,
    pub(crate) notify: Notify,
    pub(crate) stats: Arc<crate::perf::SharedQueueStats>,
    pub(crate) metrics: NetworkMetrics,
}

impl Default for StateMailbox {
    fn default() -> Self {
        Self::with_metrics(NetworkMetrics::default())
    }
}

impl StateMailbox {
    pub(crate) fn with_metrics(metrics: NetworkMetrics) -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
            notify: Notify::new(),
            stats: crate::perf::queue_stats(crate::perf::QueueCategory::Outbound),
            metrics,
        }
    }

    pub(crate) async fn replace(&self, packet: Packet) {
        let Ok(encoded) = EncodedPacket::new(packet) else {
            return;
        };
        self.replace_encoded(encoded).await;
    }

    pub(crate) async fn replace_encoded(&self, encoded: EncodedPacket) {
        let (key, sequence) = match encoded.packet() {
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
        let bytes = encoded.frame_bytes();
        if let Some(old) = pending.insert(key, TrackedPacket::new(encoded, &self.metrics)) {
            self.stats.dequeue(old.encoded().frame_bytes());
        }
        self.stats.enqueue(bytes, queue_now_ms());
        drop(pending);
        self.notify.notify_one();
    }

    pub(crate) async fn drain(&self) -> Vec<EncodedPacket> {
        let mut packets: Vec<_> = self.pending.lock().await.drain().collect();
        for (_, packet) in &packets {
            self.stats.dequeue(packet.encoded().frame_bytes());
        }
        packets.sort_by_key(|(key, _)| *key);
        packets
            .into_iter()
            .map(|(_, packet)| packet.into_encoded())
            .collect()
    }
}

pub(crate) struct CatchupMailbox {
    pub(crate) capacity: usize,
    pub(crate) pending: Mutex<VecDeque<TrackedPacket>>,
    pub(crate) notify: Notify,
    pub(crate) full_count: AtomicU64,
    pub(crate) stats: Arc<crate::perf::SharedQueueStats>,
    pub(crate) metrics: NetworkMetrics,
}

impl CatchupMailbox {
    pub(crate) fn with_capacity(capacity: usize) -> Self {
        Self::with_capacity_and_metrics(capacity, NetworkMetrics::default())
    }

    pub(crate) fn with_capacity_and_metrics(capacity: usize, metrics: NetworkMetrics) -> Self {
        Self {
            capacity: capacity.max(1),
            pending: Mutex::new(VecDeque::new()),
            notify: Notify::new(),
            full_count: AtomicU64::new(0),
            stats: crate::perf::queue_stats(crate::perf::QueueCategory::CatchUp),
            metrics,
        }
    }

    pub(crate) async fn replace(&self, packet: Packet) -> Result<(), u64> {
        let Ok(encoded) = EncodedPacket::new(packet) else {
            return Ok(());
        };
        self.replace_encoded(encoded).await
    }

    pub(crate) async fn replace_encoded(&self, encoded: EncodedPacket) -> Result<(), u64> {
        let key = match encoded.packet() {
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
        let incoming_bytes = encoded.frame_bytes();
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
            let old_bytes = existing.encoded().frame_bytes();
            let existing_revision = match existing.packet() {
                Packet::ChunkData { revision, .. } => *revision,
                _ => 0,
            };
            if key.3 >= existing_revision {
                *existing = TrackedPacket::new(encoded, &self.metrics);
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
        let bytes = encoded.frame_bytes();
        guard.push_back(TrackedPacket::new(encoded, &self.metrics));
        self.stats.enqueue(bytes, queue_now_ms());
        self.notify.notify_one();
        Ok(())
    }

    pub(crate) async fn pop(&self) -> Option<EncodedPacket> {
        let mut guard = self.pending.lock().await;
        let packet = guard.pop_front();
        if let Some(packet) = &packet {
            self.stats.dequeue(packet.encoded().frame_bytes());
        }
        if !guard.is_empty() {
            self.notify.notify_one();
        }
        packet.map(TrackedPacket::into_encoded)
    }

    #[allow(dead_code)]
    pub(crate) async fn len(&self) -> usize {
        self.pending.lock().await.len()
    }
}

impl Default for CatchupMailbox {
    fn default() -> Self {
        Self::with_capacity(MAX_CATCHUP_QUEUE_DEPTH)
    }
}

/// Transport-side state for the authoritative gameplay envelope. The
/// authority core owns accepted sequences, the response cache, and world
/// mutation. The network allocates missing client sequences, tracks in-flight
/// request ids, and keeps a sequence watermark so out-of-order packets never
/// cross the host channel (NetworkServer tests and the TCP thread have no
/// AuthorityCore). Completed request ids (not responses) are remembered so a
/// retransmit can be forwarded to the single authority cache.
#[derive(Debug)]
pub(crate) struct GameplaySessionState {
    pub(crate) next_request_id: RequestId,
    pub(crate) last_client_sequence: u64,
    pub(crate) last_client_revision: u64,
    pub(crate) last_server_sequence: ServerSequence,
    pub(crate) in_flight: HashSet<RequestId>,
    /// Bounded set of finished request ids used only to forward retransmits to
    /// authority. Does not store `GameplayResponse` bodies.
    completed_request_ids: VecDeque<RequestId>,
    pub(crate) current_dimension: u8,
}

impl Default for GameplaySessionState {
    fn default() -> Self {
        Self {
            next_request_id: 1,
            last_client_sequence: 0,
            last_client_revision: 0,
            last_server_sequence: 0,
            in_flight: HashSet::new(),
            completed_request_ids: VecDeque::with_capacity(crate::authority::RESPONSE_CACHE_CAPACITY),
            current_dimension: 0,
        }
    }
}

impl GameplaySessionState {
    pub(crate) fn allocate_request_id(&mut self) -> RequestId {
        let id = self.next_request_id.max(1);
        self.next_request_id = id.wrapping_add(1).max(1);
        id
    }

    pub(crate) fn allocate_server_sequence(&mut self) -> ServerSequence {
        self.last_server_sequence = self.last_server_sequence.wrapping_add(1).max(1);
        self.last_server_sequence
    }

    pub(crate) fn clear_in_flight(&mut self, request_id: RequestId) {
        self.in_flight.remove(&request_id);
    }

    pub(crate) fn mark_completed(&mut self, request_id: RequestId) {
        self.clear_in_flight(request_id);
        if self.completed_request_ids.iter().any(|id| *id == request_id) {
            return;
        }
        if self.completed_request_ids.len() >= crate::authority::RESPONSE_CACHE_CAPACITY {
            self.completed_request_ids.pop_front();
        }
        self.completed_request_ids.push_back(request_id);
    }

    pub(crate) fn is_completed(&self, request_id: RequestId) -> bool {
        self.completed_request_ids.iter().any(|id| *id == request_id)
    }

    pub(crate) fn rejection(
        &mut self,
        request_id: RequestId,
        reason: RejectReason,
    ) -> GameplayResponse {
        self.mark_completed(request_id);
        GameplayResponse {
            request_id,
            server_sequence: self.allocate_server_sequence(),
            outcome: crate::network::protocol::GameplayOutcome::Rejected { reason },
        }
    }
}

pub(crate) struct RequestRateLimiter {
    pub(crate) window_started: Instant,
    pub(crate) count: u32,
    pub(crate) limit: u32,
}

impl RequestRateLimiter {
    pub(crate) fn new(limit: u32) -> Self {
        Self {
            window_started: Instant::now(),
            count: 0,
            limit: limit.max(1),
        }
    }

    pub(crate) fn allow(&mut self) -> bool {
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

pub(crate) struct ClientSession {
    pub(crate) id: PlayerId,
    pub(crate) username: String,
    pub(crate) out_tx: mpsc::Sender<QueuedPacket>,
    pub(crate) pose_mailbox: Arc<PoseMailbox>,
    pub(crate) state_mailbox: Arc<StateMailbox>,
    pub(crate) catchup_mailbox: Arc<CatchupMailbox>,
    pub(crate) cancel_tx: watch::Sender<bool>,
    pub(crate) gameplay: GameplaySessionState,
    pub(crate) metrics: NetworkMetrics,
}

pub(crate) type Sessions = Arc<Mutex<HashMap<PlayerId, ClientSession>>>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::protocol::{EntityStateWire, PROTOCOL_VERSION};

    #[tokio::test]
    async fn transport_metrics_count_exact_successful_tcp_frames() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let client_task = tokio::spawn(async move {
            Connection::new(tokio::net::TcpStream::connect(addr).await.unwrap())
        });
        let (server_stream, _) = listener.accept().await.unwrap();
        let mut server_conn = Connection::new(server_stream);
        let mut client_conn = client_task.await.unwrap();

        let client_metrics = NetworkMetrics::default();
        let server_metrics = NetworkMetrics::default();
        let packet = Packet::Keepalive;

        send_connection_packet(&mut client_conn, packet.clone(), &client_metrics)
            .await
            .unwrap();
        let received = server_conn.recv().await.unwrap();
        server_metrics.record_inbound(&received);

        let client_snap = client_metrics.snapshot();
        let server_snap = server_metrics.snapshot();
        assert_eq!(client_snap.outbound_packets, 1);
        assert_eq!(client_snap.outbound_bytes, packet_bytes(&packet));
        assert_eq!(server_snap.inbound_packets, 1);
        assert_eq!(server_snap.inbound_bytes, packet_bytes(&packet));
    }

    #[tokio::test]
    async fn outbound_metrics_publish_before_write_and_rollback_on_failure() {
        let metrics = NetworkMetrics::default();
        let packet = Packet::Keepalive;
        let expected_bytes = packet_bytes(&packet);

        let reservation = metrics.reserve_outbound(&packet);
        let snap_during = metrics.snapshot();
        assert_eq!(snap_during.outbound_packets, 1);
        assert_eq!(snap_during.outbound_bytes, expected_bytes);
        drop(reservation);
        let snap_after_drop = metrics.snapshot();
        assert_eq!(snap_after_drop.outbound_packets, 0);
        assert_eq!(snap_after_drop.outbound_bytes, 0);

        let reservation = metrics.reserve_outbound(&packet);
        reservation.commit();
        let snap_after_commit = metrics.snapshot();
        assert_eq!(snap_after_commit.outbound_packets, 1);
        assert_eq!(snap_after_commit.outbound_bytes, expected_bytes);
    }

    #[tokio::test]
    async fn queue_metrics_track_backlog_replacement_drain_and_saturation() {
        let metrics = NetworkMetrics::default();
        let pose = PoseMailbox::with_metrics(metrics.clone());
        let catchup = CatchupMailbox::with_capacity_and_metrics(1, metrics.clone());

        pose.replace(
            1,
            Packet::PlayerPosition {
                id: 1,
                sequence: 1,
                sender_time_millis: 1,
                x: 0.0,
                y: 0.0,
                z: 0.0,
                yaw: 0.0,
                pitch: 0.0,
            },
        )
        .await;
        assert_eq!(metrics.snapshot().queue_depth, 1);

        pose.replace(
            1,
            Packet::PlayerPosition {
                id: 1,
                sequence: 2,
                sender_time_millis: 2,
                x: 1.0,
                y: 0.0,
                z: 0.0,
                yaw: 0.0,
                pitch: 0.0,
            },
        )
        .await;
        assert_eq!(metrics.snapshot().queue_depth, 1);

        let drained = pose.drain().await;
        assert_eq!(drained.len(), 1);
        assert_eq!(metrics.snapshot().queue_depth, 0);

        let chunk_a = Packet::ChunkData {
            dimension: 0,
            cx: 0,
            cz: 0,
            revision: 1,
            min_section_y: 0,
            section_count: 1,
            blocks: vec![1],
            block_states: vec![0],
            fluid_levels: vec![0],
            block_entities: Vec::new(),
        };
        let chunk_b = Packet::ChunkData {
            dimension: 0,
            cx: 1,
            cz: 0,
            revision: 1,
            min_section_y: 0,
            section_count: 1,
            blocks: vec![2],
            block_states: vec![0],
            fluid_levels: vec![0],
            block_entities: Vec::new(),
        };

        assert!(catchup.replace(chunk_a).await.is_ok());
        assert_eq!(metrics.snapshot().queue_depth, 1);
        assert_eq!(catchup.replace(chunk_b).await, Err(1));
        assert_eq!(metrics.snapshot().queue_depth, 1);
        assert_eq!(metrics.snapshot().queue_full, 1);
        assert!(catchup.pop().await.is_some());
        assert_eq!(metrics.snapshot().queue_depth, 0);
    }

    #[tokio::test]
    async fn catchup_mailbox_is_latest_wins_and_bounded() {
        let mailbox = CatchupMailbox::with_capacity(1);
        let p1 = Packet::ChunkData {
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

        assert_eq!(mailbox.pop().await.as_ref().map(|p| p.packet()), Some(&p2));
        assert!(mailbox.replace(p3.clone()).await.is_ok());
        assert_eq!(mailbox.pop().await.as_ref().map(|p| p.packet()), Some(&p3));
    }

    #[tokio::test]
    async fn catchup_mailbox_preserves_distance_priority_insertion_order() {
        let mailbox = CatchupMailbox::with_capacity(2);
        let near = Packet::ChunkData {
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
        assert_eq!(mailbox.pop().await.as_ref().map(|p| p.packet()), Some(&near));
        assert_eq!(
            mailbox.pop().await.as_ref().map(|p| p.packet()),
            Some(&farther)
        );
    }

    #[tokio::test]
    async fn slow_client_backpressure_does_not_starve_other_mailboxes() {
        let metrics = NetworkMetrics::default();
        let slow = CatchupMailbox::with_capacity_and_metrics(1, metrics.clone());
        let fast = CatchupMailbox::with_capacity_and_metrics(1, metrics.clone());
        let packet = |cx, value| Packet::ChunkData {
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
        assert_eq!(fast.pop().await.as_ref().map(|p| p.packet()), Some(&packet(2, 3)));
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
                dimension: 0,
                sequence: 1,
                state: state(7, 1.0),
            })
            .await;
        mailbox
            .replace(Packet::EntityState {
                dimension: 0,
                sequence: 2,
                state: state(7, 2.0),
            })
            .await;
        mailbox
            .replace(Packet::EntityState {
                dimension: 0,
                sequence: 1,
                state: state(7, -1.0),
            })
            .await;
        mailbox
            .replace(Packet::EntityState {
                dimension: 0,
                sequence: 2,
                state: state(8, 8.0),
            })
            .await;

        let packets = mailbox.drain().await;
        assert_eq!(packets.len(), 2);
        assert!(packets.iter().any(|packet| matches!(
            packet.packet(),
            Packet::EntityState {
                sequence: 2,
                state,
                ..
            } if state.entity_id == 7 && state.position[0] == 2.0
        )));
        assert!(packets.iter().any(|packet| matches!(
            packet.packet(),
            Packet::EntityState { state, .. } if state.entity_id == 8
        )));
    }

    #[test]
    fn encoded_packet_fanout_shares_one_payload_arc() {
        let packet = Packet::EntityState {
            dimension: 0,
            sequence: 1,
            state: EntityStateWire {
                entity_id: 1,
                entity_type: 0,
                position: [0.0; 3],
                velocity: [0.0; 3],
                yaw: 0.0,
                pitch: 0.0,
                health: 20.0,
                animation_state: 0,
                item: None,
            },
        };
        let first = EncodedPacket::new(packet).unwrap();
        let second = first.clone();
        assert!(std::sync::Arc::ptr_eq(first.payload(), second.payload()));
        assert_eq!(first.protocol_version(), PROTOCOL_VERSION);
        assert_eq!(first.frame_bytes(), packet_bytes(first.packet()));
    }
}
