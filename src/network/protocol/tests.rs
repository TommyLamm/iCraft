// Tests extracted from protocol.rs (Plan 27).

use super::wire_types::enc_to_u8;
use super::*;
use crate::brewing::{PotionData, PotionKind};
use crate::enchantment::{Enchantment, EnchantmentSet};
use crate::inventory::{Item, ItemStack};

fn v() -> u32 {
    PROTOCOL_VERSION
}

#[test]
fn current_protocol_version_is_21() {
    assert_eq!(PROTOCOL_VERSION, 21);
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
fn container_close_roundtrip_keeps_v16_shape() {
    let packet = Packet::ContainerClose {
        dimension: 2,
        x: -11,
        y: 64,
        z: 19,
    };
    assert_eq!(Packet::decode(&packet.encode()).unwrap(), packet);
}

#[test]
fn disconnect_roundtrip() {
    let p = Packet::Disconnect {
        reason: "kicked".into(),
    };
    let decoded = Packet::decode(&p.encode()).unwrap();
    assert_eq!(p, decoded);
}

#[test]
fn player_position_roundtrip() {
    let p = Packet::PlayerPosition {
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
        id: 7,
        action: Action::Place,
    };
    let decoded = Packet::decode(&p.encode()).unwrap();
    assert_eq!(p, decoded);
}

#[test]
fn player_join_roundtrip() {
    let p = Packet::PlayerJoin {
        id: 99,
        username: "alex".into(),
    };
    let decoded = Packet::decode(&p.encode()).unwrap();
    assert_eq!(p, decoded);
}

#[test]
fn player_leave_roundtrip() {
    let p = Packet::PlayerLeave {
        id: 99,
    };
    let decoded = Packet::decode(&p.encode()).unwrap();
    assert_eq!(p, decoded);
}

#[test]
fn block_change_roundtrip() {
    let p = Packet::BlockChange {
        dimension: 0,
        revision: 1,
        x: -10,
        y: 64,
        z: 200,
        block: 12,
        state: 0,
        raw_fluid: 0,
    };
    let decoded = Packet::decode(&p.encode()).unwrap();
    assert_eq!(p, decoded);
}

#[test]
fn chunk_data_roundtrip() {
    let p = Packet::ChunkData {
        dimension: 0,
        cx: -3,
        cz: 4,
        revision: 1,
        min_section_y: -4,
        section_count: 24,
        blocks: vec![0u8; 4096],
        block_states: Vec::new(),
        fluid_levels: Vec::new(),
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
fn rich_item_wire_requires_canonical_enchantments() {
    let mut stack = ItemStack::new(Item::DiamondPickaxe, 1);
    stack
        .enchantments
        .add_or_upgrade(Enchantment::Efficiency(4));
    stack
        .enchantments
        .add_or_upgrade(Enchantment::Unbreaking(3));
    stack.enchantments.add_or_upgrade(Enchantment::Fortune(2));
    stack.custom_name.set("Miner");
    let canonical = ItemWire::from_stack(&stack);
    SessionSlotWire::new(canonical, 0x55, 0xaa)
        .validate_bounds()
        .unwrap();
    assert_eq!(
        ItemWire::from_stack(&canonical.to_stack().unwrap()),
        canonical
    );

    let mut silk_level_two = canonical;
    silk_level_two.enchantments = [
        (enc_to_u8(&Enchantment::SilkTouch) & 0xf0) | 2,
        0,
        0,
        0,
        0,
        0,
    ];
    assert_eq!(
        silk_level_two.validate_rich_bounds(false),
        Err(RejectReason::Malformed)
    );

    let mut fortune_four = canonical;
    fortune_four.enchantments = [
        (enc_to_u8(&Enchantment::Fortune(3)) & 0xf0) | 4,
        0,
        0,
        0,
        0,
        0,
    ];
    assert_eq!(
        fortune_four.validate_rich_bounds(false),
        Err(RejectReason::Malformed)
    );

    let mut duplicate_kind = canonical;
    duplicate_kind.enchantments = [
        enc_to_u8(&Enchantment::Efficiency(1)),
        enc_to_u8(&Enchantment::Efficiency(2)),
        0,
        0,
        0,
        0,
    ];
    assert_eq!(
        duplicate_kind.validate_rich_bounds(false),
        Err(RejectReason::Malformed)
    );

    let mut order_hole = canonical;
    order_hole.enchantments = [0, enc_to_u8(&Enchantment::Efficiency(1)), 0, 0, 0, 0];
    assert_eq!(
        order_hole.validate_rich_bounds(false),
        Err(RejectReason::Malformed)
    );
}

#[test]
fn version_mismatch_detectable() {
    let p = Packet::Handshake {
        protocol_version: 999,
        username: "old".into(),
    };
    let decoded = Packet::decode(&p.encode()).unwrap();
    match decoded {
        Packet::Handshake {
            protocol_version, ..
        } => assert_ne!(protocol_version, PROTOCOL_VERSION),
        other => panic!("expected Handshake, got {other:?}"),
    }
}

#[test]
fn post_auth_packets_omit_protocol_version() {
    // TimeSync (and other live gameplay packets) no longer embed a
    // protocol_version field; version is held by the authenticated session.
    let packet = Packet::TimeSync {
        ticks: 20_000,
        weather: 1,
        weather_remaining_ticks: 4_000.0,
    };
    assert_eq!(Packet::decode(&packet.encode()).unwrap(), packet);
}

#[test]
fn intermediate_legacy_handshake_is_not_accepted_by_current_contract() {
    let packet = Packet::Handshake {
        protocol_version: PROTOCOL_VERSION - 1,
        username: "legacy".into(),
    };
    let decoded = Packet::decode(&packet.encode()).unwrap();
    match decoded {
        Packet::Handshake {
            protocol_version, ..
        } => {
            assert_eq!(protocol_version, PROTOCOL_VERSION - 1);
            assert_ne!(protocol_version, PROTOCOL_VERSION);
        }
        other => panic!("expected Handshake, got {other:?}"),
    }
}

#[test]
fn world_rules_sync_roundtrip() {
    let packet = Packet::WorldRulesSync {
        rules: crate::game_rules::WorldRules {
            keep_inventory: true,
            pvp: false,
            ..Default::default()
        },
    };
    assert_eq!(Packet::decode(&packet.encode()).unwrap(), packet);
}

#[test]
fn gameplay_request_response_roundtrip_and_bounds() {
    let request = GameplayRequest {
        request_id: 42,
        client_sequence: 7,
        session_id: 99,
        dimension: 0,
        client_revision: 12,
        operation: GameplayOperation::Container {
            action: ContainerAction::Open,
            x: 4,
            y: 64,
            z: -2,
            slot: 3,
        },
    };
    request.validate_bounds().unwrap();
    let packet = Packet::GameplayRequest {
        request: request.clone(),
    };
    assert_eq!(Packet::decode(&packet.encode()).unwrap(), packet);

    let response = Packet::GameplayResponse {
        response: GameplayResponse {
            request_id: request.request_id,
            server_sequence: 13,
            outcome: GameplayOutcome::Accepted { revision: 13 },
        },
    };
    assert_eq!(Packet::decode(&response.encode()).unwrap(), response);
}

#[test]
fn typed_block_action_and_owner_mining_projection_roundtrip() {
    let held = SessionSlotWire::new(
        ItemWire::from_stack(&ItemStack::new(Item::Stone, 2)),
        0x11,
        0x22,
    );
    let request = GameplayRequest {
        request_id: 43,
        client_sequence: 8,
        session_id: 99,
        dimension: 0,
        client_revision: 12,
        operation: GameplayOperation::BlockAction {
            action: BlockActionKind::StartBreak,
            x: 4,
            y: 64,
            z: -2,
            face: [0, 0, -1],
            hand: 0,
            held: Some(held),
            block: crate::world::BlockType::Air.to_wire(),
            look_milli: [0, -100, 995],
        },
    };
    request.validate_bounds().unwrap();
    let packet = Packet::GameplayRequest {
        request: request.clone(),
    };
    assert_eq!(Packet::decode(&packet.encode()).unwrap(), packet);

    let mut gameplay = SessionGameplayWire::default();
    gameplay.mining = Some(MiningProgressWire {
        dimension: 0,
        target: [4, 64, -2],
        progress_milli: 350,
        hand: 0,
        slot_index: 0,
        held: Some(held),
        block: crate::world::BlockType::Stone.to_wire(),
        state: 3,
        look_milli: [0, -100, 995],
    });
    gameplay.validate_bounds().unwrap();
    let mut invalid = gameplay;
    invalid.mining.as_mut().unwrap().slot_index = 40;
    assert_eq!(invalid.validate_bounds(), Err(RejectReason::InvalidState));
}

#[test]
fn gameplay_command_limit_is_rejected() {
    let request = GameplayRequest {
        request_id: 1,
        client_sequence: 1,
        session_id: 1,
        dimension: 0,
        client_revision: 0,
        operation: GameplayOperation::Command {
            command: "x".repeat(MAX_COMMAND_BYTES + 1),
        },
    };
    assert_eq!(request.validate_bounds(), Err(RejectReason::StringTooLong));
}

#[test]
fn gameplay_bounds_keep_specific_reject_reasons() {
    let oversized = GameplayRequest {
        request_id: 1,
        client_sequence: 1,
        session_id: 1,
        dimension: 0,
        client_revision: 0,
        operation: GameplayOperation::Command {
            command: "x".repeat(MAX_REQUEST_BYTES),
        },
    };
    assert_eq!(oversized.validate_bounds(), Err(RejectReason::QueueFull));

    let invalid_dimension = GameplayRequest {
        request_id: 2,
        client_sequence: 2,
        session_id: 1,
        dimension: 3,
        client_revision: 0,
        operation: GameplayOperation::ItemUse { item: 1, count: 1 },
    };
    assert_eq!(
        invalid_dimension.validate_bounds(),
        Err(RejectReason::InvalidDimension)
    );
}

#[test]
fn unknown_container_action_discriminant_fails_decode() {
    let base = GameplayRequest {
        request_id: 1,
        client_sequence: 1,
        session_id: 1,
        dimension: 0,
        client_revision: 0,
        operation: GameplayOperation::Container {
            action: ContainerAction::Open,
            x: 0,
            y: 64,
            z: 0,
            slot: MAX_CONTAINER_SLOTS - 1,
        },
    };
    base.validate_bounds().unwrap();
    GameplayRequest {
        operation: GameplayOperation::Container {
            action: ContainerAction::Close,
            x: 0,
            y: 64,
            z: 0,
            slot: MAX_CONTAINER_SLOTS - 1,
        },
        ..base.clone()
    }
    .validate_bounds()
    .unwrap();

    let open_bytes = Packet::GameplayRequest {
        request: base.clone(),
    }
    .encode();
    let close_bytes = Packet::GameplayRequest {
        request: GameplayRequest {
            operation: GameplayOperation::Container {
                action: ContainerAction::Close,
                x: 0,
                y: 64,
                z: 0,
                slot: MAX_CONTAINER_SLOTS - 1,
            },
            ..base.clone()
        },
    }
    .encode();
    assert_eq!(
        open_bytes.len(),
        close_bytes.len(),
        "Open/Close container frames must share layout"
    );
    let diff_indexes: Vec<usize> = open_bytes
        .iter()
        .zip(close_bytes.iter())
        .enumerate()
        .filter_map(|(index, (a, b))| (a != b).then_some(index))
        .collect();
    assert!(
        !diff_indexes.is_empty(),
        "Open and Close must differ by ContainerAction discriminant"
    );
    let mut unknown = open_bytes;
    for index in diff_indexes {
        unknown[index] = 99;
    }
    assert!(
        Packet::decode(&unknown).is_err(),
        "unknown ContainerAction discriminant must fail decode"
    );

    assert_eq!(
        GameplayRequest {
            request_id: 1,
            client_sequence: 1,
            session_id: 1,
            dimension: 0,
            client_revision: 0,
            operation: GameplayOperation::ContainerClick {
                x: 0,
                y: 64,
                z: 0,
                slot: MAX_CONTAINER_SLOTS,
                is_left: true,
                dragged: None,
            },
        }
        .validate_bounds(),
        Err(RejectReason::InvalidState)
    );

    // The pre-v15 self-damage adapter reserves the high bit and quantizes
    // damage into the remaining seven bits. B0 keeps that wire contract.
    GameplayRequest {
        request_id: 1,
        client_sequence: 1,
        session_id: 1,
        dimension: 0,
        client_revision: 0,
        operation: GameplayOperation::Combat {
            target: 1,
            action: 0x80 | 127,
        },
    }
    .validate_bounds()
    .unwrap();
}

#[test]
fn invalid_bytes_rejected() {
    assert!(Packet::decode(&[0xFF; 3]).is_err());
}

/// Honest empty `ChunkData` ends with four `Vec<u8>` lengths. Overwrite
/// `blocks` and drop the rest so the claimed length far exceeds remaining
/// bytes, matching the pre-handshake OOM frame from the review.
fn crafted_chunk_data_blocks_len(claimed: u64) -> Vec<u8> {
    let packet = Packet::ChunkData {
        dimension: 0,
        cx: 0,
        cz: 0,
        revision: 0,
        min_section_y: 0,
        section_count: 0,
        blocks: Vec::new(),
        block_states: Vec::new(),
        fluid_levels: Vec::new(),
        block_entities: Vec::new(),
    };
    let mut bytes = packet.encode();
    assert!(
        bytes.len() >= 32,
        "empty ChunkData should end with four u64 lengths"
    );
    let len_offset = bytes.len() - 32;
    bytes[len_offset..len_offset + 8].copy_from_slice(&claimed.to_le_bytes());
    bytes.truncate(len_offset + 8);
    bytes
}

fn crafted_player_effect_len(claimed: u64) -> Vec<u8> {
    let packet = Packet::PlayerEffect {
        sequence: 0,
        player_id: 1,
        effects: Vec::new(),
    };
    let mut bytes = packet.encode();
    let len_offset = bytes.len() - 8;
    bytes[len_offset..].copy_from_slice(&claimed.to_le_bytes());
    bytes
}

fn crafted_handshake_username_len(claimed: u64) -> Vec<u8> {
    let packet = Packet::Handshake {
        protocol_version: v(),
        username: String::new(),
    };
    let mut bytes = packet.encode();
    let len_offset = bytes.len() - 8;
    bytes[len_offset..].copy_from_slice(&claimed.to_le_bytes());
    bytes
}

fn crafted_chat_message_len(claimed: u64) -> Vec<u8> {
    let packet = Packet::ChatMessage {
        sender: "s".into(),
        message: String::new(),
    };
    let mut bytes = packet.encode();
    let len_offset = bytes.len() - 8;
    bytes[len_offset..].copy_from_slice(&claimed.to_le_bytes());
    bytes
}

fn assert_decode_rejects(bytes: &[u8]) {
    let result = std::panic::catch_unwind(|| Packet::decode(bytes));
    match result {
        Ok(Err(_)) => {}
        Ok(Ok(packet)) => panic!("expected InvalidData, decoded {packet:?}"),
        Err(_) => panic!("Packet::decode must not panic or abort on a crafted Vec length"),
    }
}

#[test]
fn probe_bincode_vec_u8_claims_capacity_before_elements() {
    // Confirm actual bincode 1.3.3 behavior: deserialize_seq for Vec<u8>
    // (no serde_bytes) uses the claimed u64 as Vec::with_capacity before
    // reading elements. A 1 MiB claim against an 8-byte slice errors after
    // that reserve instead of rejecting on remaining-slice first.
    let mut claimed = Vec::new();
    claimed.extend_from_slice(&(1024u64 * 1024).to_le_bytes());
    let started = std::time::Instant::now();
    let result = bincode::deserialize::<Vec<u8>>(&claimed);
    eprintln!(
        "bincode Vec<u8> 1MiB-claim-on-8-bytes: err={} elapsed={:?}",
        result.is_err(),
        started.elapsed()
    );
    assert!(result.is_err());

    let huge = crafted_chunk_data_blocks_len(1 << 40);
    let packet_result = std::panic::catch_unwind(|| bincode::deserialize::<Packet>(&huge));
    eprintln!("raw Packet deserialize 1<<40 blocks claim: {packet_result:?}");
    match packet_result {
        Ok(Err(_)) => {}
        Ok(Ok(_)) => panic!("huge claimed Vec len must not decode as a valid packet"),
        Err(_) => {}
    }
}

#[test]
fn decode_rejects_claimed_vec_len_far_beyond_remaining() {
    assert_decode_rejects(&crafted_chunk_data_blocks_len(1 << 40));
    assert_decode_rejects(&crafted_chunk_data_blocks_len(1_000_000));
    assert_decode_rejects(&crafted_player_effect_len(1_000_000));
    assert_decode_rejects(&crafted_handshake_username_len(1_000_000));
    assert_decode_rejects(&crafted_chat_message_len(1_000_000));
}

#[test]
fn honest_chunk_data_bytes_keep_seq_wire_layout() {
    let blocks = vec![1u8, 2, 3, 4];
    let packet = Packet::ChunkData {
        dimension: 0,
        cx: 1,
        cz: -2,
        revision: 3,
        min_section_y: -4,
        section_count: 24,
        blocks: blocks.clone(),
        block_states: vec![5, 6],
        fluid_levels: vec![7],
        block_entities: Vec::new(),
    };
    let encoded = packet.encode();
    assert_eq!(Packet::decode(&encoded).unwrap(), packet);

    let prefix = Packet::ChunkData {
        dimension: 0,
        cx: 1,
        cz: -2,
        revision: 3,
        min_section_y: -4,
        section_count: 24,
        blocks: Vec::new(),
        block_states: Vec::new(),
        fluid_levels: Vec::new(),
        block_entities: Vec::new(),
    }
    .encode();
    let blocks_len_offset = prefix.len() - 32;
    assert_eq!(
        &encoded[blocks_len_offset..blocks_len_offset + 8],
        &(blocks.len() as u64).to_le_bytes()
    );
    assert_eq!(
        &encoded[blocks_len_offset + 8..blocks_len_offset + 8 + blocks.len()],
        blocks.as_slice()
    );
}

#[test]
fn block_change_and_chunk_data_state_roundtrip() {
    let bc = Packet::BlockChange {
        dimension: 0,
        revision: 7,
        x: 10,
        y: 64,
        z: -5,
        block: 67,
        state: 0b0000_1101,
        raw_fluid: crate::world::FLUID_WATERLOGGED_BIT,
    };
    let decoded_bc = Packet::decode(&bc.encode()).unwrap();
    assert_eq!(bc, decoded_bc);

    let cd = Packet::ChunkData {
        dimension: 0,
        cx: 2,
        cz: -3,
        revision: 7,
        min_section_y: -4,
        section_count: 24,
        blocks: vec![1, 2, 3],
        block_states: vec![4, 5, 6],
        fluid_levels: vec![0, crate::world::FLUID_WATERLOGGED_BIT, 9],
        block_entities: Vec::new(),
    };
    let decoded_cd = Packet::decode(&cd.encode()).unwrap();
    assert_eq!(cd, decoded_cd);

    let delta = Packet::BlockEntityDelta {
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
}

#[test]
fn entity_lifecycle_and_player_authority_roundtrip() {
    let mut dropped = ItemWire::from_stack(&ItemStack {
        item: Item::Stone,
        count: 3,
        durability: 7,
        enchantments: EnchantmentSet::default(),
        potion: None,
        custom_name: {
            let mut name = crate::enchantment::ItemName::default();
            name.set("drop");
            name
        },
        can_break: 0x55,
        can_place_on: 0xaa,
    });
    assert_eq!(dropped.to_stack().unwrap().can_break, 0x55);
    assert_eq!(dropped.to_stack().unwrap().can_place_on, 0xaa);
    dropped.count = 3;
    let state = EntityStateWire {
        entity_id: 42,
        entity_type: 3,
        position: [1.0, 70.0, -2.0],
        velocity: [0.25, 0.0, -0.5],
        yaw: 1.25,
        pitch: -0.2,
        health: 17.0,
        animation_state: 0b0000_0111,
        item: Some(dropped),
    };
    for packet in [
        Packet::EntitySpawn {
            dimension: 0,
            sequence: 8,
            state,
        },
        Packet::EntityState {
            dimension: 0,
            sequence: 9,
            state,
        },
        Packet::EntityDespawn {
            dimension: 0,
            sequence: 10,
            entity_id: state.entity_id,
        },
    ] {
        assert_eq!(packet, Packet::decode(&packet.encode()).unwrap());
    }

    let health = Packet::PlayerHealth {
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
        dimension: 1,
        revision: 99,
        x: -2,
        y: 64,
        z: 8,
        entity: Some(crate::block_entity::BlockEntity::Hopper(hopper)),
    };
    assert_eq!(delta, Packet::decode(&delta.encode()).unwrap());
}

fn rich_source(index: u8, item: Item, stack_count: u16, debit: u16) -> SlotRefWire {
    SlotRefWire {
        index,
        count: debit,
        expected: SessionSlotWire::new(
            ItemWire::from_stack(&ItemStack::new(item, u32::from(stack_count))),
            0x55,
            0xaa,
        ),
    }
}

fn domain_request(operation: GameplayOperation) -> GameplayRequest {
    GameplayRequest {
        request_id: 77,
        client_sequence: 3,
        session_id: 9,
        dimension: 0,
        client_revision: 2,
        operation,
    }
}

#[test]
fn private_session_projection_is_fixed_width_bounded_and_roundtrips() {
    let source = rich_source(0, Item::FishingRod, 1, 1);
    let mut state = SessionGameplayWire {
        health_milli: 7_500,
        hunger_milli: 11_000,
        saturation_milli: 3_000,
        is_dead: true,
        death_source: Some(4),
        velocity_milli: [2_000, 500, -1_000],
        experience: 321,
        experience_level: 12,
        selected_hotbar_slot: 2,
        mounted_entity: Some(55),
        shield_active: true,
        enchant_seed: 99,
        fishing_hook: Some(SessionFishingHookWire {
            entity_id: 77,
            position_milli: [1_000, 64_000, -2_000],
            velocity_milli: [10, -20, 30],
            stage: 2,
            wait_ticks_remaining: 14,
            bite_ticks_remaining: 3,
        }),
        brew: Some(SessionBrewWire {
            station: [4, 64, -2],
            ingredient: source,
            bottles: [Some(rich_source(1, Item::Potion, 1, 1)), None, None],
            remaining_ticks: 40,
        }),
        revision: 8,
        ..SessionGameplayWire::default()
    };
    state.hotbar[0] = Some(source.expected);
    state.offhand = Some(SessionSlotWire::new(
        ItemWire::from_stack(&ItemStack::new(Item::Shield, 1)),
        0,
        0,
    ));
    state.validate_bounds().unwrap();
    let packet = Packet::PlayerSessionUpdate {
        sequence: 19,
        player_id: 4,
        dimension: 1,
        state,
    };
    assert_eq!(packet, Packet::decode(&packet.encode()).unwrap());

    state.selected_hotbar_slot = 9;
    assert_eq!(state.validate_bounds(), Err(RejectReason::InvalidState));
    state.selected_hotbar_slot = 0;
    state.velocity_milli[0] = MAX_SESSION_VELOCITY_MILLI + 1;
    assert_eq!(state.validate_bounds(), Err(RejectReason::InvalidState));
}

#[test]
fn authority_domain_requests_roundtrip_with_rich_slot_refs() {
    let source = rich_source(4, Item::IronIngot, 8, 3);
    let operations = vec![
        GameplayOperation::Fishing {
            action: 0,
            hand: 0,
            look_milli: [0, 0, 1_000],
        },
        GameplayOperation::FurnaceTakeOutput {
            x: 4,
            y: 64,
            z: -2,
            count: 1,
        },
        GameplayOperation::Craft {
            grid: 2,
            sources: [Some(source), None, None, None, None, None, None, None, None],
            station: None,
        },
        GameplayOperation::Enchant {
            x: 4,
            y: 64,
            z: -2,
            source,
            option: 2,
        },
        GameplayOperation::Brew {
            action: 0,
            x: 4,
            y: 64,
            z: -2,
            ingredient: Some(source),
            bottles: [Some(rich_source(5, Item::Potion, 1, 1)), None, None],
        },
        GameplayOperation::Anvil {
            x: 4,
            y: 64,
            z: -2,
            left: source,
            right: None,
            rename: "Miner".into(),
        },
        GameplayOperation::UseState {
            hand: 1,
            active: true,
        },
        GameplayOperation::FluidUse {
            x: 4,
            y: 64,
            z: -2,
            face: [0, 1, 0],
            hand: 0,
            source: rich_source(0, Item::WaterBucket, 1, 1),
        },
    ];

    for operation in operations {
        let request = domain_request(operation);
        request.validate_bounds().unwrap();
        let packet = Packet::GameplayRequest {
            request,
        };
        assert_eq!(Packet::decode(&packet.encode()).unwrap(), packet);
    }
}

#[test]
fn authority_domain_bounds_reject_invalid_envelopes() {
    let source = rich_source(4, Item::IronIngot, 8, 3);
    let invalid_slot = SlotRefWire {
        index: SESSION_SLOT_COUNT,
        ..source
    };
    assert_eq!(
        invalid_slot.validate_bounds(),
        Err(RejectReason::InvalidState)
    );

    for operation in [
        GameplayOperation::Fishing {
            action: 3,
            hand: 0,
            look_milli: [0, 0, 1_000],
        },
        GameplayOperation::Fishing {
            action: 0,
            hand: 2,
            look_milli: [0, 0, 1_000],
        },
        GameplayOperation::FurnaceTakeOutput {
            x: 0,
            y: 64,
            z: 0,
            count: 0,
        },
        GameplayOperation::Enchant {
            x: 0,
            y: 64,
            z: 0,
            source,
            option: 3,
        },
        GameplayOperation::UseState {
            hand: 2,
            active: true,
        },
        GameplayOperation::FluidUse {
            x: 0,
            y: 64,
            z: 0,
            face: [1, 1, 0],
            hand: 0,
            source,
        },
        GameplayOperation::FluidUse {
            x: 0,
            y: 64,
            z: 0,
            face: [i8::MIN, 0, 0],
            hand: 0,
            source,
        },
        GameplayOperation::FluidUse {
            x: 0,
            y: 64,
            z: 0,
            face: [0, 1, 0],
            hand: 2,
            source,
        },
    ] {
        assert_eq!(
            domain_request(operation).validate_bounds(),
            Err(RejectReason::InvalidState)
        );
    }

    let bad_grid = GameplayOperation::Craft {
        grid: 4,
        sources: [None; 9],
        station: None,
    };
    assert_eq!(
        domain_request(bad_grid).validate_bounds(),
        Err(RejectReason::InvalidState)
    );
    let bad_coordinate = GameplayOperation::FurnaceTakeOutput {
        x: MAX_BLOCK_COORDINATE + 1,
        y: 64,
        z: 0,
        count: 1,
    };
    assert_eq!(
        domain_request(bad_coordinate).validate_bounds(),
        Err(RejectReason::InvalidCoordinate)
    );
    let long_rename = GameplayOperation::Anvil {
        x: 0,
        y: 64,
        z: 0,
        left: source,
        right: None,
        rename: "x".repeat(MAX_ANVIL_RENAME_BYTES + 1),
    };
    assert_eq!(
        domain_request(long_rename).validate_bounds(),
        Err(RejectReason::StringTooLong)
    );

    let mut malformed = source;
    malformed.expected.item.custom_name = [0; 24];
    malformed.expected.item.custom_name[0] = 0xff;
    assert_eq!(malformed.validate_bounds(), Err(RejectReason::Malformed));
}
