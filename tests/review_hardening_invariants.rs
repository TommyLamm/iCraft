//! Cross-plan architecture invariants that 01/03/05/08 do not already lock.
//!
//! Out-of-view chunk materialization belongs to plan 11. Region-cache write
//! failure is locked in `save::tests::failed_region_write_does_not_replace_in_memory_region_cache`.

mod common;

use common::tcp_harness::HeldLoopback;
use icraft::authority::contract::{
    AuthorityTopology, SessionContract, SessionGameplayState, SessionInventorySlot,
};
use icraft::authority::{AuthorityConfig, AuthorityCore};
use icraft::chunk_manager::ChunkManager;
use icraft::dimension::Dimension;
use icraft::entity::EntityType;
use icraft::inventory::{Item, ItemStack};
use icraft::network::protocol::{
    BlockActionKind, GameplayOperation, GameplayOutcome, GameplayRequest, RejectReason,
    SessionSlotWire,
};
use icraft::world::{BlockType, Chunk};
use std::collections::HashMap;

const ALEX: u64 = 7;
const SAM: u64 = 8;
const POSITION: [f32; 3] = [8.0, 80.0, 8.0];

fn core() -> AuthorityCore {
    AuthorityCore::new(AuthorityConfig::default(), AuthorityTopology::Dedicated)
}

fn register(core: &mut AuthorityCore, id: u64, name: &str, dimension: Dimension) {
    core.register_session(SessionContract::new(
        id,
        name,
        dimension as u8,
        POSITION,
        true,
        true,
    ))
    .expect("register fixture session");
}

fn leftover_block_use(session_id: u64, request_id: u128, client_sequence: u64) -> GameplayRequest {
    GameplayRequest {
        request_id,
        client_sequence,
        session_id,
        dimension: Dimension::Overworld as u8,
        client_revision: 0,
        operation: GameplayOperation::BlockUse {
            x: 8,
            y: 80,
            z: 8,
            block: BlockType::Stone.to_wire(),
        },
    }
}

fn stone_wire() -> SessionSlotWire {
    let stack = ItemStack::new(Item::Stone, 2);
    SessionSlotWire::new(
        icraft::network::protocol::ItemWire::from_stack(&stack),
        0,
        0,
    )
}

fn give_stone(core: &mut AuthorityCore, id: u64, count: u32) {
    let stack = ItemStack::new(Item::Stone, count);
    let wire = SessionSlotWire::new(
        icraft::network::protocol::ItemWire::from_stack(&stack),
        0,
        0,
    );
    let mut gameplay = SessionGameplayState::default();
    gameplay.inventory[0] = Some(SessionInventorySlot::from(wire));
    assert!(core.set_session_gameplay(id, gameplay));
}

fn place_request(
    session_id: u64,
    request_id: u128,
    client_sequence: u64,
    dimension: Dimension,
    client_revision: u64,
    target: (i32, i32, i32),
    held: SessionSlotWire,
) -> GameplayRequest {
    GameplayRequest {
        request_id,
        client_sequence,
        session_id,
        dimension: dimension as u8,
        client_revision,
        operation: GameplayOperation::BlockAction {
            action: BlockActionKind::Place,
            x: target.0,
            y: target.1,
            z: target.2,
            face: [0, 1, 0],
            hand: 0,
            held: Some(held),
            block: BlockType::Stone.to_wire(),
            look_milli: [250, -550, 750],
        },
    }
}

fn seed_place_support(core: &mut AuthorityCore, dimension: Dimension, support: (i32, i32, i32)) {
    let target = (support.0, support.1 + 1, support.2);
    core.with_world(dimension, |world| {
        world
            .set_block(support.0, support.1, support.2, BlockType::Stone, 0)
            .expect("seed support");
        world
            .set_block(target.0, target.1, target.2, BlockType::Air, 0)
            .expect("clear place target");
    });
}

fn dropped_item_count(core: &AuthorityCore) -> usize {
    core.world
        .entities
        .entities
        .iter()
        .filter(|entity| entity.entity_type == EntityType::DroppedItem)
        .count()
}

fn checksum_after_inbound(order: [u64; 2]) -> u64 {
    let mut core = core();
    let names = [(ALEX, "alex"), (SAM, "sam")];
    for id in order {
        let name = names
            .iter()
            .find(|(candidate, _)| *candidate == id)
            .map(|(_, name)| *name)
            .expect("fixture session");
        register(&mut core, id, name, Dimension::Overworld);
    }
    core.world
        .entities
        .spawn(EntityType::Zombie, glam::Vec3::new(10.0, 80.0, 10.0));
    for (index, id) in order.iter().copied().enumerate() {
        let response = core.submit_request(leftover_block_use(
            id,
            u128::from(id) * 10 + index as u128,
            1,
        ));
        assert!(
            matches!(
                response.outcome,
                GameplayOutcome::Rejected {
                    reason: RejectReason::Unsupported
                }
            ),
            "leftover BlockUse must stay rejected: {:?}",
            response.outcome
        );
    }
    core.tick().checksum
}

#[test]
fn fixed_tick_checksum_is_independent_of_inbound_arrival_order() {
    assert_eq!(
        checksum_after_inbound([ALEX, SAM]),
        checksum_after_inbound([SAM, ALEX]),
        "session registration and leftover inbound order must not change the fixed-tick checksum"
    );
}

#[test]
fn invalid_dimension_envelope_is_rejected_without_world_or_inventory_mutation() {
    let mut core = core();
    register(&mut core, ALEX, "alex", Dimension::Overworld);
    give_stone(&mut core, ALEX, 2);
    seed_place_support(&mut core, Dimension::Overworld, (8, 80, 8));
    let before_block = core.world.get_block(8, 81, 8);
    let before_count = core
        .session(ALEX)
        .unwrap()
        .gameplay
        .count_item(Item::Stone as u32);
    let before_drops = dropped_item_count(&core);

    let response = core.submit_request(GameplayRequest {
        request_id: 21,
        client_sequence: 1,
        session_id: ALEX,
        dimension: 99,
        client_revision: 0,
        operation: GameplayOperation::BlockAction {
            action: BlockActionKind::Place,
            x: 8,
            y: 81,
            z: 8,
            face: [0, 1, 0],
            hand: 0,
            held: Some(stone_wire()),
            block: BlockType::Stone.to_wire(),
            look_milli: [250, -550, 750],
        },
    });
    assert!(
        matches!(
            response.outcome,
            GameplayOutcome::Rejected {
                reason: RejectReason::InvalidDimension
            }
        ),
        "unexpected invalid-dimension response: {:?}",
        response.outcome
    );
    assert_eq!(core.world.get_block(8, 81, 8), before_block);
    assert_eq!(
        core.session(ALEX)
            .unwrap()
            .gameplay
            .count_item(Item::Stone as u32),
        before_count
    );
    assert_eq!(dropped_item_count(&core), before_drops);
}

#[test]
fn nether_mutations_do_not_invalidate_overworld_client_revision() {
    let mut core = core();
    register(&mut core, ALEX, "alex", Dimension::Overworld);
    register(&mut core, SAM, "sam", Dimension::Nether);
    seed_place_support(&mut core, Dimension::Overworld, (8, 80, 8));
    seed_place_support(&mut core, Dimension::Nether, (8, 80, 8));
    give_stone(&mut core, ALEX, 2);
    give_stone(&mut core, SAM, 2);

    let overworld_revision = core.revision_for_dimension(Dimension::Overworld);
    let nether = core.submit_request(place_request(
        SAM,
        31,
        1,
        Dimension::Nether,
        core.revision_for_dimension(Dimension::Nether),
        (8, 81, 8),
        stone_wire(),
    ));
    assert!(
        matches!(nether.outcome, GameplayOutcome::Accepted { .. }),
        "nether place must succeed: {:?}",
        nether.outcome
    );
    assert_eq!(
        core.revision_for_dimension(Dimension::Overworld),
        overworld_revision,
        "nether mutation must not bump the Overworld revision namespace"
    );

    let overworld = core.submit_request(place_request(
        ALEX,
        32,
        1,
        Dimension::Overworld,
        overworld_revision,
        (8, 81, 8),
        stone_wire(),
    ));
    assert!(
        matches!(overworld.outcome, GameplayOutcome::Accepted { .. }),
        "overworld client_revision must stay valid after a nether mutation: {:?}",
        overworld.outcome
    );
    core.activate_dimension(Dimension::Overworld);
    assert_eq!(core.world.get_block(8, 81, 8), BlockType::Stone);
}

#[test]
fn stale_block_place_does_not_consume_held_stack_or_create_drops() {
    let mut core = core();
    register(&mut core, ALEX, "alex", Dimension::Overworld);
    give_stone(&mut core, ALEX, 2);
    seed_place_support(&mut core, Dimension::Overworld, (8, 80, 8));
    seed_place_support(&mut core, Dimension::Overworld, (9, 80, 8));

    let first = core.submit_request(place_request(
        ALEX,
        41,
        1,
        Dimension::Overworld,
        0,
        (8, 81, 8),
        stone_wire(),
    ));
    assert!(
        matches!(first.outcome, GameplayOutcome::Accepted { .. }),
        "fresh place must succeed: {:?}",
        first.outcome
    );
    let held_after_place = core
        .session(ALEX)
        .unwrap()
        .gameplay
        .count_item(Item::Stone as u32);
    assert_eq!(held_after_place, 1);
    let drops_after_place = dropped_item_count(&core);
    let second_target = core.world.get_block(9, 81, 8);

    let stale = core.submit_request(place_request(
        ALEX,
        42,
        2,
        Dimension::Overworld,
        0,
        (9, 81, 8),
        stone_wire(),
    ));
    assert!(
        matches!(
            stale.outcome,
            GameplayOutcome::Rejected {
                reason: RejectReason::InvalidRevision
            }
        ),
        "stale place must be InvalidRevision: {:?}",
        stale.outcome
    );
    assert_eq!(
        core.session(ALEX)
            .unwrap()
            .gameplay
            .count_item(Item::Stone as u32),
        held_after_place
    );
    assert_eq!(dropped_item_count(&core), drops_after_place);
    assert_eq!(core.world.get_block(9, 81, 8), second_target);
    assert_eq!(core.world.get_block(8, 81, 8), BlockType::Stone);
}

/// Join-client presentation sink: apply only revision-gated projections.
/// Never constructs `AuthorityCore` / `ServerWorld`.
struct HeadlessJoinSink {
    chunks: ChunkManager,
    applied: HashMap<(u8, i32, i32), u64>,
}

impl HeadlessJoinSink {
    fn new() -> Self {
        Self {
            chunks: ChunkManager::new_in_dimension(2, Dimension::Overworld),
            applied: HashMap::new(),
        }
    }

    fn apply_chunk_data(&mut self, dimension: u8, cx: i32, cz: i32, revision: u64, chunk: &Chunk) {
        let key = (dimension, cx, cz);
        if self
            .applied
            .get(&key)
            .is_some_and(|current| revision <= *current)
        {
            return;
        }
        let payload = icraft::save::ChunkSaveData::from_chunk(chunk).expect("compress payload");
        self.chunks
            .insert_authoritative_chunk_payload(
                cx,
                cz,
                &payload.blocks,
                &payload.block_states,
                &payload.fluid_levels,
                &payload.block_entities,
            )
            .expect("join client restores the authoritative column");
        self.applied.insert(key, revision);
    }

    fn apply_block_change(
        &mut self,
        dimension: u8,
        revision: u64,
        x: i32,
        y: i32,
        z: i32,
        block: BlockType,
    ) {
        let key = (dimension, x.div_euclid(16), z.div_euclid(16));
        match self.applied.get_mut(&key) {
            Some(current) if revision > *current => {
                *current = revision;
                self.chunks.set_block(x, y, z, block);
            }
            _ => {}
        }
    }
}

#[test]
fn join_client_applies_revision_gated_projections_without_local_authority() {
    let mut source = Chunk::empty_in_dimension(Dimension::Overworld, 0, 0);
    source.set_block_local(4, 70, 5, BlockType::Stone);

    let mut sink = HeadlessJoinSink::new();
    sink.apply_chunk_data(0, 0, 0, 1, &source);
    sink.apply_block_change(0, 3, 4, 70, 5, BlockType::Dirt);
    sink.apply_chunk_data(0, 0, 0, 1, &source);
    sink.apply_block_change(0, 2, 4, 70, 5, BlockType::Glass);

    let chunk = sink
        .chunks
        .chunks
        .get(&(0, 0))
        .expect("snapshot must insert the presentation column");
    assert_eq!(chunk.get_block_local(4, 70, 5), BlockType::Dirt);
    assert_ne!(
        chunk.get_block_local(4, 70, 5),
        BlockType::Stone,
        "stale snapshot must not overwrite a newer projection"
    );
    assert_ne!(
        chunk.get_block_local(4, 70, 5),
        BlockType::Glass,
        "stale block change must not overwrite a newer projection"
    );
}

#[test]
fn held_loopback_keeps_the_reserved_port_until_release() {
    let held = HeldLoopback::bind();
    let port = held.port();
    assert_eq!(held.addr().port(), port);
    assert!(
        std::net::TcpListener::bind(held.addr()).is_err(),
        "HeldLoopback must keep the OS reservation so a later bind cannot steal the port"
    );
    assert_eq!(held.release(), port);
    std::net::TcpListener::bind(("127.0.0.1", port)).expect("released port must be bindable again");
}
