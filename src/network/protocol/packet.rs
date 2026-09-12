use super::decode::PlayerId;
use super::decode::{deserialize_bounded_bytes, deserialize_bounded_vec, set_decode_frame_len, DecodeFrameGuard, MAX_PACKET_SIZE};
use super::gameplay::GameplayRequest;
use super::wire_types::*;
use bincode::Options;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    Place,
    Break,
    Use,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LightningStrike {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub visual_seed: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EntityStateWire {
    pub entity_id: u64,
    pub entity_type: u8,
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub health: f32,
    pub animation_state: u8,
    /// Item/potion payload for DroppedItem and projectile convergence. Plan27
    /// and Plan28 finalized this in the same not-yet-published v18 development
    /// sequence; the intermediate Plan27 wire shape is not compatible.
    #[serde(default)]
    pub item: Option<ItemWire>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PlayerEffectWire {
    pub kind: u8,
    pub level: u8,
    pub remaining_seconds: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Packet {
    Handshake {
        protocol_version: u32,
        username: String,
    },
    LoginSuccess {
        protocol_version: u32,
        player_id: PlayerId,
        seed: u64,
        gamemode: u8,
    },
    Disconnect {
        reason: String,
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
    PlayerJoin {
        id: PlayerId,
        username: String,
    },
    PlayerLeave {
        id: PlayerId,
    },
    BlockChange {
        dimension: u8,
        revision: u64,
        x: i32,
        y: i32,
        z: i32,
        block: u32,
        state: u8,
        /// Complete raw fluid byte; bit 7 is waterlogged for Plan27 slabs.
        #[serde(default)]
        raw_fluid: u8,
    },
    ChunkData {
        dimension: u8,
        cx: i32,
        cz: i32,
        revision: u64,
        min_section_y: i8,
        section_count: u16,
        #[serde(deserialize_with = "deserialize_bounded_bytes")]
        blocks: Vec<u8>,
        #[serde(default, deserialize_with = "deserialize_bounded_bytes")]
        block_states: Vec<u8>,
        /// Raw fluid bytes in section-major, voxel-major order.
        #[serde(default, deserialize_with = "deserialize_bounded_bytes")]
        fluid_levels: Vec<u8>,
        #[serde(default, deserialize_with = "deserialize_bounded_bytes")]
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
        #[serde(deserialize_with = "deserialize_bounded_vec")]
        effects: Vec<PlayerEffectWire>,
    },
    /// Private, owner-targeted projection of the complete authoritative
    /// gameplay session. Servers must never broadcast this packet.
    PlayerSessionUpdate {
        sequence: u64,
        player_id: PlayerId,
        dimension: u8,
        state: SessionGameplayWire,
    },
    TimeSync {
        ticks: u64,
        weather: u8,
        weather_remaining_ticks: f32,
    },
    LightningStrike {
        strike: LightningStrike,
    },
    ChatMessage {
        sender: String,
        message: String,
    },
    Keepalive,
    PlayerRespawnRequest,
    PlayerRespawnResult {
        position: [f32; 3],
        dimension: u8,
    },
    ContainerOpenResult {
        dimension: u8,
        success: bool,
        x: i32,
        y: i32,
        z: i32,
        #[serde(deserialize_with = "deserialize_bounded_vec")]
        slots: Vec<Option<ItemWire>>,
        revision: u64,
    },
    ContainerClickResult {
        dimension: u8,
        success: bool,
        slot_index: u16,
        slot: Option<ItemWire>,
        dragged: Option<ItemWire>,
    },
    ContainerClose {
        dimension: u8,
        x: i32,
        y: i32,
        z: i32,
    },
    ContainerSlotUpdate {
        dimension: u8,
        revision: u64,
        x: i32,
        y: i32,
        z: i32,
        slot_index: u16,
        slot: Option<ItemWire>,
    },
    SleepStateSync {
        player_id: PlayerId,
        is_sleeping: bool,
    },
    WorldRulesSync {
        rules: crate::game_rules::WorldRules,
    },
    /// One envelope is used for block/container/item/combat/sleep/trade/mount
    /// and command operations.  The server rewrites `session_id` from the
    /// authenticated connection before forwarding it to the authority.
    GameplayRequest {
        request: GameplayRequest,
    },
    GameplayResponse {
        response: GameplayResponse,
    },
    ServerListPingRequest {
        protocol_version: u32,
    },
    ServerListPingResponse {
        protocol_version: u32,
        version: String,
        motd: String,
        online_players: u16,
        max_players: u16,
    },
    DimensionTransfer {
        player_id: PlayerId,
        dimension: u8,
        position: [f32; 3],
    },
}

impl Packet {

    pub fn encode(&self) -> Vec<u8> {
        bincode::serialize(self).expect("packet serialization is infallible")
    }

    /// Bincode payload if within the 2 MiB `MAX_PACKET_SIZE` cap.
    pub fn encode_payload(&self) -> Result<Vec<u8>, &'static str> {
        let payload = self.encode();
        if payload.len() > MAX_PACKET_SIZE {
            return Err("packet payload exceeds maximum");
        }
        Ok(payload)
    }

    /// 4-byte big-endian length prefix plus bincode payload.
    pub fn encode_frame(&self) -> Result<Vec<u8>, &'static str> {
        let payload = self.encode_payload()?;
        let mut frame = Vec::with_capacity(4 + payload.len());
        frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        frame.extend_from_slice(&payload);
        Ok(frame)
    }

    pub fn decode(bytes: &[u8]) -> Result<Packet, bincode::Error> {
        if bytes.len() > MAX_PACKET_SIZE {
            return Err(Box::new(bincode::ErrorKind::SizeLimit));
        }
        set_decode_frame_len(bytes.len());
        let _guard = DecodeFrameGuard;
        // Same integer/trailing options as `bincode::deserialize`, plus the
        // shared frame cap. The limit charges bytes as they are read; Vec
        // visitors still reject claimed lengths above the remaining slice
        // before `with_capacity`.
        bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .allow_trailing_bytes()
            .with_limit(MAX_PACKET_SIZE as u64)
            .deserialize(bytes)
    }
}
