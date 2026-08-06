use crate::brewing::{PotionData, PotionKind};
use crate::enchantment::{Enchantment, EnchantmentSet};
use crate::inventory::{Item, ItemStack};
use serde::{Deserialize, Serialize};

pub type PlayerId = u64;

/// Protocol v12 adds stable block-entity variants and revision-bearing
/// automation state to chunk/entity deltas.  Older clients are rejected during
/// the existing handshake instead of being allowed to simulate containers.
pub const PROTOCOL_VERSION: u32 = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PotionWire {
    pub kind: u8,
    pub level: u8,
    pub duration_seconds: u16,
    pub splash: bool,
}

impl PotionWire {
    pub fn from_potion(data: PotionData) -> Self {
        let kind = match data.kind {
            PotionKind::Water => 0,
            PotionKind::Awkward => 1,
            PotionKind::Speed => 2,
            PotionKind::Strength => 3,
            PotionKind::Healing => 4,
            PotionKind::Regeneration => 5,
            PotionKind::NightVision => 6,
            PotionKind::Invisibility => 7,
            PotionKind::FireResistance => 8,
            PotionKind::WaterBreathing => 9,
            PotionKind::Poison => 10,
            PotionKind::Slowness => 11,
        };
        Self {
            kind,
            level: data.level,
            duration_seconds: data.duration_seconds,
            splash: data.splash,
        }
    }

    pub fn to_potion(&self) -> Option<PotionData> {
        let kind = match self.kind {
            0 => PotionKind::Water,
            1 => PotionKind::Awkward,
            2 => PotionKind::Speed,
            3 => PotionKind::Strength,
            4 => PotionKind::Healing,
            5 => PotionKind::Regeneration,
            6 => PotionKind::NightVision,
            7 => PotionKind::Invisibility,
            8 => PotionKind::FireResistance,
            9 => PotionKind::WaterBreathing,
            10 => PotionKind::Poison,
            11 => PotionKind::Slowness,
            _ => return None,
        };
        Some(PotionData {
            kind,
            level: self.level,
            duration_seconds: self.duration_seconds,
            splash: self.splash,
        })
    }
}

fn enc_to_u8(enc: &Enchantment) -> u8 {
    let kind = match enc {
        Enchantment::Efficiency(_) => 0,
        Enchantment::Unbreaking(_) => 1,
        Enchantment::SilkTouch => 2,
        Enchantment::Fortune(_) => 3,
        Enchantment::Sharpness(_) => 4,
        Enchantment::Knockback(_) => 5,
        Enchantment::FireAspect(_) => 6,
        Enchantment::Looting(_) => 7,
        Enchantment::Protection(_) => 8,
        Enchantment::FeatherFalling(_) => 9,
        Enchantment::Respiration(_) => 10,
        Enchantment::Power(_) => 11,
        Enchantment::Infinity => 12,
    };
    let lvl = enc.level().max(1);
    ((kind + 1) << 4) | (lvl & 0x0F)
}

fn enc_from_u8(b: u8) -> Option<Enchantment> {
    if b == 0 {
        return None;
    }
    let kind_code = (b >> 4).checked_sub(1)?;
    let lvl = b & 0x0F;
    if lvl == 0 {
        return None;
    }
    match kind_code {
        0 => Some(Enchantment::Efficiency(lvl)),
        1 => Some(Enchantment::Unbreaking(lvl)),
        2 => Some(Enchantment::SilkTouch),
        3 => Some(Enchantment::Fortune(lvl)),
        4 => Some(Enchantment::Sharpness(lvl)),
        5 => Some(Enchantment::Knockback(lvl)),
        6 => Some(Enchantment::FireAspect(lvl)),
        7 => Some(Enchantment::Looting(lvl)),
        8 => Some(Enchantment::Protection(lvl)),
        9 => Some(Enchantment::FeatherFalling(lvl)),
        10 => Some(Enchantment::Respiration(lvl)),
        11 => Some(Enchantment::Power(lvl)),
        12 => Some(Enchantment::Infinity),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemWire {
    pub item: u32,
    pub count: u16,
    pub durability: u16,
    pub enchantments: [u8; 6],
    pub potion: Option<PotionWire>,
    pub custom_name: [u8; 24],
}

impl ItemWire {
    pub fn empty() -> Self {
        Self {
            item: Item::Air as u32,
            count: 0,
            durability: 0,
            enchantments: [0; 6],
            potion: None,
            custom_name: [0; 24],
        }
    }

    pub fn from_stack(stack: &ItemStack) -> Self {
        if stack.count == 0 || stack.item == Item::Air {
            return Self::empty();
        }
        let mut enchantments = [0u8; 6];
        for (i, entry) in stack.enchantments.entries.iter().enumerate() {
            if i < 6 {
                if let Some(enc) = entry {
                    enchantments[i] = enc_to_u8(enc);
                }
            }
        }
        let potion = stack.potion.map(PotionWire::from_potion);
        let mut custom_name = [0u8; 24];
        let name_bytes = stack.custom_name.as_str().as_bytes();
        let len = name_bytes.len().min(24);
        custom_name[..len].copy_from_slice(&name_bytes[..len]);

        Self {
            item: stack.item.to_u32(),
            count: stack.count as u16,
            durability: stack.durability as u16,
            enchantments,
            potion,
            custom_name,
        }
    }

    pub fn to_stack(&self) -> Option<ItemStack> {
        if self.count == 0 {
            return None;
        }
        let item = Item::from_u32(self.item)?;
        if item == Item::Air {
            return None;
        }
        let mut stack = ItemStack::new(item, self.count as u32);
        stack.durability = self.durability as u32;
        let mut enc_set = EnchantmentSet::default();
        for &b in &self.enchantments {
            if let Some(enc) = enc_from_u8(b) {
                enc_set.add_or_upgrade(enc);
            }
        }
        stack.enchantments = enc_set;
        if let Some(pw) = self.potion {
            stack.potion = pw.to_potion();
        }
        let name_str = std::str::from_utf8(&self.custom_name)
            .unwrap_or("")
            .trim_matches('\0');
        if !name_str.is_empty() {
            stack.custom_name.set(name_str);
        }
        Some(stack)
    }
}

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
        protocol_version: u32,
        reason: String,
    },
    PlayerPosition {
        protocol_version: u32,
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
        protocol_version: u32,
        id: PlayerId,
        action: Action,
    },
    PlayerJoin {
        protocol_version: u32,
        id: PlayerId,
        username: String,
    },
    PlayerLeave {
        protocol_version: u32,
        id: PlayerId,
    },
    BlockChange {
        protocol_version: u32,
        dimension: u8,
        revision: u64,
        x: i32,
        y: i32,
        z: i32,
        block: u32,
        state: u8,
    },
    ChunkData {
        protocol_version: u32,
        dimension: u8,
        cx: i32,
        cz: i32,
        revision: u64,
        min_section_y: i8,
        section_count: u16,
        blocks: Vec<u8>,
        #[serde(default)]
        block_states: Vec<u8>,
        #[serde(default)]
        block_entities: Vec<u8>,
    },
    BlockEntityDelta {
        protocol_version: u32,
        dimension: u8,
        revision: u64,
        x: i32,
        y: i32,
        z: i32,
        entity: Option<crate::block_entity::BlockEntity>,
    },
    ChunkAck {
        protocol_version: u32,
        dimension: u8,
        cx: i32,
        cz: i32,
        revision: u64,
    },
    EntitySpawn {
        protocol_version: u32,
        dimension: u8,
        sequence: u64,
        state: EntityStateWire,
    },
    EntityState {
        protocol_version: u32,
        dimension: u8,
        sequence: u64,
        state: EntityStateWire,
    },
    EntityDespawn {
        protocol_version: u32,
        dimension: u8,
        sequence: u64,
        entity_id: u64,
    },
    PlayerHealth {
        protocol_version: u32,
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
        protocol_version: u32,
        sequence: u64,
        player_id: PlayerId,
        effects: Vec<PlayerEffectWire>,
    },
    TimeSync {
        protocol_version: u32,
        ticks: u64,
        weather: u8,
        weather_remaining_ticks: f32,
    },
    LightningStrike {
        protocol_version: u32,
        strike: LightningStrike,
    },
    ChatMessage {
        protocol_version: u32,
        sender: String,
        message: String,
    },
    Keepalive {
        protocol_version: u32,
    },
    BlockActionRequest {
        protocol_version: u32,
        action: Action,
        x: i32,
        y: i32,
        z: i32,
        block: u32,
        held_item: Option<ItemWire>,
    },
    BlockActionResult {
        protocol_version: u32,
        x: i32,
        y: i32,
        z: i32,
        success: bool,
        consumed_item: bool,
        drops: Vec<ItemWire>,
    },
    ContainerOpenRequest {
        protocol_version: u32,
        dimension: u8,
        x: i32,
        y: i32,
        z: i32,
    },
    PlayerRespawnRequest {
        protocol_version: u32,
    },
    PlayerRespawnResult {
        protocol_version: u32,
        position: [f32; 3],
        dimension: u8,
    },
    SleepRequest {
        protocol_version: u32,
        x: i32,
        y: i32,
        z: i32,
    },
    ContainerOpenResult {
        protocol_version: u32,
        dimension: u8,
        success: bool,
        x: i32,
        y: i32,
        z: i32,
        slots: Vec<Option<ItemWire>>,
        revision: u64,
    },
    ContainerClickRequest {
        protocol_version: u32,
        dimension: u8,
        revision: u64,
        slot_index: u16,
        is_left: bool,
        dragged: Option<ItemWire>,
    },
    ContainerClickResult {
        protocol_version: u32,
        dimension: u8,
        success: bool,
        slot_index: u16,
        slot: Option<ItemWire>,
        dragged: Option<ItemWire>,
    },
    ContainerClose {
        protocol_version: u32,
        dimension: u8,
        x: i32,
        y: i32,
        z: i32,
    },
    ContainerSlotUpdate {
        protocol_version: u32,
        dimension: u8,
        revision: u64,
        x: i32,
        y: i32,
        z: i32,
        slot_index: u16,
        slot: Option<ItemWire>,
    },
    SleepStateSync {
        protocol_version: u32,
        player_id: PlayerId,
        is_sleeping: bool,
    },
    OpenTradeWindow {
        protocol_version: u32,
        villager_id: u64,
        profession: u8,
        level: u8,
        xp: u32,
        offers: Vec<crate::village::trade::TradeOffer>,
    },
    ExecuteTradeRequest {
        protocol_version: u32,
        villager_id: u64,
        offer_index: u16,
    },
    ExecuteTradeResult {
        protocol_version: u32,
        success: bool,
        offer_index: u16,
        new_uses: u32,
        villager_xp: u32,
        new_level: u8,
    },
    CloseTradeWindow {
        protocol_version: u32,
        villager_id: u64,
    },
    RaidStatusSync {
        protocol_version: u32,
        current_wave: u8,
        max_waves: u8,
        status: u8,
    },
}

impl Packet {
    pub fn protocol_version(&self) -> u32 {
        match self {
            Packet::Handshake {
                protocol_version, ..
            }
            | Packet::LoginSuccess {
                protocol_version, ..
            }
            | Packet::Disconnect {
                protocol_version, ..
            }
            | Packet::PlayerPosition {
                protocol_version, ..
            }
            | Packet::PlayerAction {
                protocol_version, ..
            }
            | Packet::PlayerJoin {
                protocol_version, ..
            }
            | Packet::PlayerLeave {
                protocol_version, ..
            }
            | Packet::BlockChange {
                protocol_version, ..
            }
            | Packet::ChunkData {
                protocol_version, ..
            }
            | Packet::BlockEntityDelta {
                protocol_version, ..
            }
            | Packet::ChunkAck {
                protocol_version, ..
            }
            | Packet::EntitySpawn {
                protocol_version, ..
            }
            | Packet::EntityState {
                protocol_version, ..
            }
            | Packet::EntityDespawn {
                protocol_version, ..
            }
            | Packet::PlayerHealth {
                protocol_version, ..
            }
            | Packet::PlayerEffect {
                protocol_version, ..
            }
            | Packet::TimeSync {
                protocol_version, ..
            }
            | Packet::LightningStrike {
                protocol_version, ..
            }
            | Packet::ChatMessage {
                protocol_version, ..
            }
            | Packet::Keepalive { protocol_version }
            | Packet::BlockActionRequest {
                protocol_version, ..
            }
            | Packet::BlockActionResult {
                protocol_version, ..
            }
            | Packet::PlayerRespawnRequest { protocol_version }
            | Packet::PlayerRespawnResult {
                protocol_version, ..
            }
            | Packet::SleepRequest {
                protocol_version, ..
            }
            | Packet::SleepStateSync {
                protocol_version, ..
            }
            | Packet::OpenTradeWindow {
                protocol_version, ..
            }
            | Packet::ExecuteTradeRequest {
                protocol_version, ..
            }
            | Packet::ExecuteTradeResult {
                protocol_version, ..
            }
            | Packet::CloseTradeWindow {
                protocol_version, ..
            }
            | Packet::RaidStatusSync {
                protocol_version, ..
            } => *protocol_version,
            Packet::ContainerOpenRequest {
                protocol_version, ..
            }
            | Packet::ContainerOpenResult {
                protocol_version, ..
            }
            | Packet::ContainerClickRequest {
                protocol_version, ..
            }
            | Packet::ContainerClickResult {
                protocol_version, ..
            }
            | Packet::ContainerClose {
                protocol_version, ..
            }
            | Packet::ContainerSlotUpdate {
                protocol_version, ..
            } => *protocol_version,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        bincode::serialize(self).expect("packet serialization is infallible")
    }

    pub fn decode(bytes: &[u8]) -> Result<Packet, bincode::Error> {
        bincode::deserialize(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v() -> u32 {
        PROTOCOL_VERSION
    }

    #[test]
    fn handshake_roundtrip() {
        let p = Packet::Handshake {
            protocol_version: v(),
            username: "steve".into(),
        };
        let decoded = Packet::decode(&p.encode()).unwrap();
        assert_eq!(p, decoded);
    }

    #[test]
    fn login_success_roundtrip() {
        let p = Packet::LoginSuccess {
            protocol_version: v(),
            player_id: 42,
            seed: 0xDEAD_BEEF_CAFE,
            gamemode: 1,
        };
        let decoded = Packet::decode(&p.encode()).unwrap();
        assert_eq!(p, decoded);
    }

    #[test]
    fn disconnect_roundtrip() {
        let p = Packet::Disconnect {
            protocol_version: v(),
            reason: "kicked".into(),
        };
        let decoded = Packet::decode(&p.encode()).unwrap();
        assert_eq!(p, decoded);
    }

    #[test]
    fn player_position_roundtrip() {
        let p = Packet::PlayerPosition {
            protocol_version: v(),
            id: 7,
            sequence: 42,
            sender_time_millis: 12_345,
            x: 1.5,
            y: 64.0,
            z: -2.25,
            yaw: 90.0,
            pitch: -45.5,
        };
        let decoded = Packet::decode(&p.encode()).unwrap();
        assert_eq!(p, decoded);
    }

    #[test]
    fn player_action_roundtrip() {
        let p = Packet::PlayerAction {
            protocol_version: v(),
            id: 7,
            action: Action::Place,
        };
        let decoded = Packet::decode(&p.encode()).unwrap();
        assert_eq!(p, decoded);
    }

    #[test]
    fn player_join_roundtrip() {
        let p = Packet::PlayerJoin {
            protocol_version: v(),
            id: 99,
            username: "alex".into(),
        };
        let decoded = Packet::decode(&p.encode()).unwrap();
        assert_eq!(p, decoded);
    }

    #[test]
    fn player_leave_roundtrip() {
        let p = Packet::PlayerLeave {
            protocol_version: v(),
            id: 99,
        };
        let decoded = Packet::decode(&p.encode()).unwrap();
        assert_eq!(p, decoded);
    }

    #[test]
    fn block_change_roundtrip() {
        let p = Packet::BlockChange {
            protocol_version: v(),
            dimension: 0,
            revision: 1,
            x: -10,
            y: 64,
            z: 200,
            block: 12,
            state: 0,
        };
        let decoded = Packet::decode(&p.encode()).unwrap();
        assert_eq!(p, decoded);
    }

    #[test]
    fn chunk_data_roundtrip() {
        let p = Packet::ChunkData {
            protocol_version: v(),
            dimension: 0,
            cx: -3,
            cz: 4,
            revision: 1,
            min_section_y: -4,
            section_count: 24,
            blocks: vec![0u8; 4096],
            block_states: Vec::new(),
            block_entities: Vec::new(),
        };
        let decoded = Packet::decode(&p.encode()).unwrap();
        assert_eq!(p, decoded);
    }

    #[test]
    fn item_wire_roundtrip() {
        // Plain block
        let s1 = ItemStack::new(Item::Stone, 64);
        let w1 = ItemWire::from_stack(&s1);
        let back1 = w1.to_stack().unwrap();
        assert_eq!(back1.item, Item::Stone);
        assert_eq!(back1.count, 64);

        // Tool with durability, enchantments, and custom name
        let mut s2 = ItemStack::new(Item::DiamondPickaxe, 1);
        s2.durability = 120;
        s2.enchantments.add_or_upgrade(Enchantment::Sharpness(5));
        s2.enchantments.add_or_upgrade(Enchantment::Efficiency(4));
        s2.enchantments.add_or_upgrade(Enchantment::Unbreaking(3));
        s2.enchantments.add_or_upgrade(Enchantment::Fortune(3));
        s2.custom_name.set("SuperPick");

        let w2 = ItemWire::from_stack(&s2);
        let back2 = w2.to_stack().unwrap();
        assert_eq!(back2.item, Item::DiamondPickaxe);
        assert_eq!(back2.durability, 120);
        assert_eq!(back2.enchantments.level_of(Enchantment::Sharpness(1)), 5);
        assert_eq!(back2.enchantments.level_of(Enchantment::Efficiency(1)), 4);
        assert_eq!(back2.enchantments.level_of(Enchantment::Unbreaking(1)), 3);
        assert_eq!(back2.enchantments.level_of(Enchantment::Fortune(1)), 3);
        assert_eq!(back2.custom_name.as_str(), "SuperPick");

        // Potion item
        let mut s3 = ItemStack::new(Item::SplashPotion, 1);
        s3.potion = Some(PotionData {
            kind: PotionKind::Speed,
            level: 2,
            duration_seconds: 180,
            splash: true,
        });
        let w3 = ItemWire::from_stack(&s3);
        let back3 = w3.to_stack().unwrap();
        assert_eq!(back3.item, Item::SplashPotion);
        let pot = back3.potion.unwrap();
        assert_eq!(pot.kind, PotionKind::Speed);
        assert_eq!(pot.level, 2);
        assert_eq!(pot.duration_seconds, 180);
        assert!(pot.splash);

        // Air / count 0
        let s4 = ItemStack::new(Item::Air, 0);
        let w4 = ItemWire::from_stack(&s4);
        assert_eq!(w4.to_stack(), None);
    }

    #[test]
    fn block_action_request_roundtrip() {
        let held = ItemWire::from_stack(&ItemStack::new(Item::StonePickaxe, 1));
        let p = Packet::BlockActionRequest {
            protocol_version: v(),
            action: Action::Break,
            x: 10,
            y: 64,
            z: -5,
            block: Item::Air as u32,
            held_item: Some(held),
        };
        let decoded = Packet::decode(&p.encode()).unwrap();
        assert_eq!(p, decoded);
    }

    #[test]
    fn block_action_result_roundtrip() {
        let drop = ItemWire::from_stack(&ItemStack::new(Item::Cobblestone, 1));
        let p = Packet::BlockActionResult {
            protocol_version: v(),
            x: 10,
            y: 64,
            z: -5,
            success: true,
            consumed_item: false,
            drops: vec![drop],
        };
        let decoded = Packet::decode(&p.encode()).unwrap();
        assert_eq!(p, decoded);
    }

    #[test]
    fn version_mismatch_detectable() {
        let p = Packet::Handshake {
            protocol_version: 999,
            username: "old".into(),
        };
        let decoded = Packet::decode(&p.encode()).unwrap();
        assert_ne!(decoded.protocol_version(), PROTOCOL_VERSION);
    }

    #[test]
    fn old_weather_packet_version_is_rejected_by_version_check() {
        let p = Packet::TimeSync {
            protocol_version: PROTOCOL_VERSION - 1,
            ticks: 20_000,
            weather: 1,
            weather_remaining_ticks: 4_000.0,
        };
        let decoded = Packet::decode(&p.encode()).unwrap();
        assert_ne!(decoded.protocol_version(), PROTOCOL_VERSION);
    }

    #[test]
    fn invalid_bytes_rejected() {
        assert!(Packet::decode(&[0xFF; 3]).is_err());
    }

    #[test]
    fn block_change_and_chunk_data_state_roundtrip() {
        let bc = Packet::BlockChange {
            protocol_version: v(),
            dimension: 0,
            revision: 7,
            x: 10,
            y: 64,
            z: -5,
            block: 67,
            state: 0b0000_1101,
        };
        let decoded_bc = Packet::decode(&bc.encode()).unwrap();
        assert_eq!(bc, decoded_bc);

        let cd = Packet::ChunkData {
            protocol_version: v(),
            dimension: 0,
            cx: 2,
            cz: -3,
            revision: 7,
            min_section_y: -4,
            section_count: 24,
            blocks: vec![1, 2, 3],
            block_states: vec![4, 5, 6],
            block_entities: Vec::new(),
        };
        let decoded_cd = Packet::decode(&cd.encode()).unwrap();
        assert_eq!(cd, decoded_cd);

        let delta = Packet::BlockEntityDelta {
            protocol_version: v(),
            dimension: 0,
            revision: 8,
            x: 2,
            y: 64,
            z: 3,
            entity: Some(crate::block_entity::BlockEntity::Chest(
                crate::block_entity::ChestBlockEntity {
                    inventory: crate::inventory::ContainerInventory::new(),
                    custom_name: None,
                    loot_table: None,
                    loot_seed: None,
                    revision: 0,
                },
            )),
        };
        let decoded_delta = Packet::decode(&delta.encode()).unwrap();
        assert_eq!(delta, decoded_delta);

        let ack = Packet::ChunkAck {
            protocol_version: v(),
            dimension: 0,
            cx: 2,
            cz: -3,
            revision: 7,
        };
        assert_eq!(ack, Packet::decode(&ack.encode()).unwrap());
    }

    #[test]
    fn entity_lifecycle_and_player_authority_roundtrip() {
        let state = EntityStateWire {
            entity_id: 42,
            entity_type: 3,
            position: [1.0, 70.0, -2.0],
            velocity: [0.25, 0.0, -0.5],
            yaw: 1.25,
            pitch: -0.2,
            health: 17.0,
            animation_state: 0b0000_0111,
        };
        for packet in [
            Packet::EntitySpawn {
                protocol_version: v(),
                dimension: 0,
                sequence: 8,
                state,
            },
            Packet::EntityState {
                protocol_version: v(),
                dimension: 0,
                sequence: 9,
                state,
            },
            Packet::EntityDespawn {
                protocol_version: v(),
                dimension: 0,
                sequence: 10,
                entity_id: state.entity_id,
            },
        ] {
            assert_eq!(packet, Packet::decode(&packet.encode()).unwrap());
        }

        let health = Packet::PlayerHealth {
            protocol_version: v(),
            sequence: 11,
            player_id: 7,
            health: 12.0,
            max_health: 20.0,
            hunger: 16.0,
            saturation: 3.0,
            oxygen: 240.0,
            is_dead: false,
            death_reason: 0,
        };
        assert_eq!(health, Packet::decode(&health.encode()).unwrap());

        let effects = Packet::PlayerEffect {
            protocol_version: v(),
            sequence: 11,
            player_id: 7,
            effects: vec![PlayerEffectWire {
                kind: 2,
                level: 1,
                remaining_seconds: 15.5,
            }],
        };
        assert_eq!(effects, Packet::decode(&effects.encode()).unwrap());
    }

    #[test]
    fn automation_block_entity_and_revision_roundtrip() {
        let mut hopper = crate::block_entity::HopperBlockEntity::new();
        hopper.facing = crate::redstone::Direction::East;
        hopper.transfer_cooldown = 7;
        hopper.is_powered = true;
        hopper.revision = 11;
        hopper.slots[0] = Some(ItemStack::new(Item::SplashPotion, 2));
        let delta = Packet::BlockEntityDelta {
            protocol_version: v(),
            dimension: 1,
            revision: 99,
            x: -2,
            y: 64,
            z: 8,
            entity: Some(crate::block_entity::BlockEntity::Hopper(hopper)),
        };
        assert_eq!(delta, Packet::decode(&delta.encode()).unwrap());

        let click = Packet::ContainerClickRequest {
            protocol_version: v(),
            dimension: 1,
            revision: 99,
            slot_index: 0,
            is_left: true,
            dragged: Some(ItemWire::from_stack(&ItemStack::new(Item::Diamond, 1))),
        };
        assert_eq!(click, Packet::decode(&click.encode()).unwrap());
    }
}
