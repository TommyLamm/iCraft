mod common;

use common::authority_harness::authority_request;
use icraft::authority::contract::SessionContract;
use icraft::authority::{AuthorityConfig, AuthorityCore};
use icraft::dimension::Dimension;
use icraft::inventory::Item;
use icraft::network::protocol::{
    GameplayOperation, GameplayOutcome, GameplayRequest, ItemWire, Packet, SessionSlotWire,
    SlotRefWire, PROTOCOL_VERSION,
};
use icraft::save::ChunkSaveData;
use icraft::world::{BlockType, Chunk, FLUID_WATERLOGGED_BIT};

const SESSION_ID: u64 = 0x27;

fn item_wire(item: Item) -> ItemWire {
    ItemWire {
        item: item.to_u32(),
        count: 1,
        durability: 0,
        enchantments: [0; 6],
        potion: None,
        custom_name: [0; 24],
        can_break: 0,
        can_place_on: 0,
    }
}

fn fluid_request(
    core: &AuthorityCore,
    request_id: u128,
    sequence: u64,
    source: SlotRefWire,
) -> GameplayRequest {
    authority_request(
        core,
        SESSION_ID,
        request_id,
        sequence,
        GameplayOperation::FluidUse {
            x: 8,
            y: 80,
            z: 8,
            face: [0, 1, 0],
            hand: 0,
            source,
        },
    )
}

fn setup_core() -> AuthorityCore {
    let mut core = AuthorityCore::new(AuthorityConfig::default());
    core.register_session(SessionContract::new(
        SESSION_ID,
        "water",
        Dimension::Overworld as u8,
        [8.0, 80.0, 8.0],
        false,
        false,
    ))
    .unwrap();
    core.with_world(Dimension::Overworld, |world| {
        world.set_block(8, 80, 8, BlockType::OakSlab, 0).unwrap();
    });
    let state = core.session_mut(SESSION_ID).unwrap();
    state.gameplay.inventory[0] = Some(
        icraft::authority::contract::SessionInventorySlot::from_wire(
            item_wire(Item::WaterBucket),
            0,
            0,
        ),
    );
    core
}

fn source(core: &AuthorityCore) -> SlotRefWire {
    let slot = core.session(SESSION_ID).unwrap().gameplay.inventory[0].unwrap();
    SlotRefWire {
        index: 0,
        count: 1,
        expected: SessionSlotWire::from(slot),
    }
}

#[test]
fn bucket_place_pickup_is_atomic_and_duplicate_idempotent() {
    let mut core = setup_core();
    let original_metadata = {
        let slot = core.session_mut(SESSION_ID).unwrap().gameplay.inventory[0]
            .as_mut()
            .unwrap();
        slot.item.durability = 37;
        slot.item.enchantments = [0; 6];
        slot.item.potion = Some(icraft::network::protocol::PotionWire {
            kind: 2,
            level: 3,
            duration_seconds: 45,
            splash: true,
        });
        slot.item.custom_name = [0; 24];
        slot.item.custom_name[..2].copy_from_slice(b"wb");
        slot.item
    };
    let place = fluid_request(&core, 1, 1, source(&core));
    let place_response = core.submit_request(place.clone());
    assert!(matches!(
        place_response.outcome,
        GameplayOutcome::Accepted { .. }
    ));
    assert!(core
        .world_ref(Dimension::Overworld)
        .unwrap()
        .chunks
        .is_waterlogged(8, 80, 8));
    assert_eq!(
        core.session(SESSION_ID).unwrap().gameplay.inventory[0]
            .unwrap()
            .item
            .item,
        Item::Bucket.to_u32()
    );

    let duplicate = core.submit_request(place);
    assert_eq!(duplicate, place_response);
    assert_eq!(core.revision_for_dimension(Dimension::Overworld), 2);

    let pickup = fluid_request(&core, 2, 2, source(&core));
    let pickup_response = core.submit_request(pickup.clone());
    assert!(matches!(
        pickup_response.outcome,
        GameplayOutcome::Accepted { .. }
    ));
    assert!(!core
        .world_ref(Dimension::Overworld)
        .unwrap()
        .chunks
        .is_waterlogged(8, 80, 8));
    assert_eq!(
        core.session(SESSION_ID).unwrap().gameplay.inventory[0]
            .unwrap()
            .item
            .item,
        Item::WaterBucket.to_u32()
    );
    let restored = core.session(SESSION_ID).unwrap().gameplay.inventory[0]
        .unwrap()
        .item;
    assert_eq!(restored.item, Item::WaterBucket.to_u32());
    assert_eq!(restored.count, original_metadata.count);
    assert_eq!(restored.durability, original_metadata.durability);
    assert_eq!(restored.enchantments, original_metadata.enchantments);
    assert_eq!(restored.potion, original_metadata.potion);
    assert_eq!(restored.custom_name, original_metadata.custom_name);
    assert_eq!(core.submit_request(pickup), pickup_response);
    assert_eq!(core.revision_for_dimension(Dimension::Overworld), 3);
}

#[test]
fn fluid_mutation_projection_is_stale_noop() {
    let mut core = setup_core();
    let place = fluid_request(&core, 1, 1, source(&core));
    let response = core.submit_request(place.clone());
    assert!(matches!(response.outcome, GameplayOutcome::Accepted { .. }));
    let snapshot = core.tick();
    let mutation = snapshot
        .mutations
        .iter()
        .find(|mutation| mutation.position == (8, 80, 8))
        .expect("fluid mutation must be projected through the fixed tick");
    assert_eq!(mutation.block, BlockType::OakSlab.to_wire());
    assert_eq!(mutation.raw_fluid, FLUID_WATERLOGGED_BIT);

    let stale = fluid_request(&core, 2, 0, source(&core));
    let stale_response = core.submit_request(stale);
    assert!(matches!(
        stale_response.outcome,
        GameplayOutcome::Rejected {
            reason: icraft::network::protocol::RejectReason::OutOfOrder
        }
    ));
    assert!(core
        .world_ref(Dimension::Overworld)
        .unwrap()
        .chunks
        .is_waterlogged(8, 80, 8));
    assert_eq!(
        core.session(SESSION_ID).unwrap().gameplay.inventory[0]
            .unwrap()
            .item
            .item,
        Item::Bucket.to_u32()
    );
}

#[test]
fn malformed_face_is_rejected_before_world_or_inventory_mutation() {
    let mut core = setup_core();
    let mut malformed = fluid_request(&core, 9, 1, source(&core));
    if let GameplayOperation::FluidUse { face, .. } = &mut malformed.operation {
        *face = [i8::MIN, 0, 0];
    }
    let response = core.submit_request(malformed);
    assert!(matches!(
        response.outcome,
        GameplayOutcome::Rejected {
            reason: icraft::network::protocol::RejectReason::InvalidState
        }
    ));
    assert!(!core
        .world_ref(Dimension::Overworld)
        .unwrap()
        .chunks
        .is_waterlogged(8, 80, 8));
    assert_eq!(
        core.session(SESSION_ID).unwrap().gameplay.inventory[0]
            .unwrap()
            .item
            .item,
        Item::WaterBucket.to_u32()
    );
}

#[test]
fn bucket_does_not_replace_passable_plant_at_adjacent_face() {
    let mut core = setup_core();
    core.with_world(Dimension::Overworld, |world| {
        world.set_block(8, 80, 8, BlockType::Stone, 0).unwrap();
        world.set_block(9, 80, 8, BlockType::Dandelion, 0).unwrap();
    });
    let mut request = fluid_request(&core, 10, 1, source(&core));
    if let GameplayOperation::FluidUse { face, .. } = &mut request.operation {
        *face = [1, 0, 0];
    }
    let response = core.submit_request(request);
    assert!(matches!(
        response.outcome,
        GameplayOutcome::Rejected {
            reason: icraft::network::protocol::RejectReason::InvalidState
        }
    ));
    assert_eq!(
        core.world_ref(Dimension::Overworld)
            .unwrap()
            .get_block(9, 80, 8),
        BlockType::Dandelion
    );
    assert_eq!(
        core.session(SESSION_ID).unwrap().gameplay.inventory[0]
            .unwrap()
            .item
            .item,
        Item::WaterBucket.to_u32()
    );
}

#[test]
fn raw_fluid_survives_v3_save_and_packet_roundtrip() {
    let mut core = setup_core();
    core.with_world(Dimension::Overworld, |world| {
        world.chunks.set_waterlogged(8, 80, 8, true);
        let chunk = world.chunks.chunks.get(&(0, 0)).unwrap();
        let data = ChunkSaveData::from_chunk(chunk).unwrap();
        assert_eq!(data.data_version, 3);
        let mut restored = Chunk::new(0, 0);
        data.restore_to_chunk(&mut restored).unwrap();
        assert_eq!(
            restored.get_fluid_level(8, 80, 8) & FLUID_WATERLOGGED_BIT,
            FLUID_WATERLOGGED_BIT
        );
    });

    let packet = Packet::BlockChange {
        protocol_version: PROTOCOL_VERSION,
        dimension: 0,
        revision: 1,
        x: 8,
        y: 80,
        z: 8,
        block: BlockType::OakSlab.to_wire(),
        state: 0,
        raw_fluid: FLUID_WATERLOGGED_BIT,
    };
    assert_eq!(Packet::decode(&packet.encode()).unwrap(), packet);
}
