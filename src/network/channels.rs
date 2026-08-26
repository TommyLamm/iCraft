use std::collections::HashSet;
use std::sync::{mpsc as std_mpsc, Arc};
use std::time::Duration;

use super::protocol::{
    Action, EntityStateWire, GameplayRequest, ItemWire, LightningStrike, PlayerEffectWire, PlayerId,
    SessionGameplayWire,
};
use super::session::NetworkMetrics;

pub(crate) const MAX_CATCHUP_QUEUE_DEPTH: usize = 32;
pub(crate) const DEFAULT_POSE_RATE_PER_SECOND: u32 = 20;
pub(crate) const DEFAULT_CHAT_RATE_PER_SECOND: u32 = 8;
pub(crate) const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

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
        drops: Vec<ItemWire>,
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
        slots: Vec<Option<ItemWire>>,
        revision: u64,
    },
    /// Targeted lifecycle invalidation. It maps to the existing v16
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
        slot: Option<ItemWire>,
        dragged: Option<ItemWire>,
    },
    BroadcastContainerSlotUpdate {
        dimension: u8,
        revision: u64,
        x: i32,
        y: i32,
        z: i32,
        slot_index: u16,
        slot: Option<ItemWire>,
    },
    SendContainerSlotUpdate {
        to: PlayerId,
        dimension: u8,
        revision: u64,
        x: i32,
        y: i32,
        z: i32,
        slot_index: u16,
        slot: Option<ItemWire>,
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
        response: super::protocol::GameplayResponse,
    },
    SendDimensionTransfer {
        to: PlayerId,
        dimension: u8,
        position: [f32; 3],
    },
    Stop,
}

/// Host event transport is bounded in production (`SyncSender`) while tests
/// may use the legacy unbounded sender. The trait keeps NetworkServer's
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
