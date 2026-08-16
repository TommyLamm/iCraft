mod common;

use common::tcp_harness::{
    drive_until, gameplay_request as request, loopback_properties, session_slot as slot,
    temp_world, wait_for_response, HeldLoopback, TcpClient,
};
use icraft::authority::contract::{
    AuthorityTopology, SessionGameplayState, SessionInventorySlot, SESSION_INVENTORY_SLOTS,
};
use icraft::entity::EntityType;
use icraft::inventory::{Item, ItemStack};
use icraft::network::protocol::{GameplayOperation, GameplayOutcome, RejectReason};
use icraft::server_runtime::{
    EmbeddedRuntimeOptions, LocalSessionProfile, RuntimePresentationEvent, ServerProperties,
    ServerRuntime, TransportMode,
};
use icraft::world::BlockType;
use std::fs;

const OWNER_ID: u64 = 0x01_0001;
const TARGET: (i32, i32, i32) = (8, 80, 8);
const OWNER_POSITION: [f32; 3] = [8.0, 80.0, 8.0];

fn properties(label: &str) -> ServerProperties {
    let mut properties = loopback_properties(
        temp_world(&format!("plan01-block-use-{label}")),
        "127.0.0.1",
    );
    properties.seed = 0x01_01_01_01;
    properties
}

fn seed_chest_and_inventory(runtime: &mut ServerRuntime, player_id: u64) {
    runtime.authority.world_mut_active().ensure_chunk(0, 0);
    runtime
        .authority
        .world_mut_active()
        .set_block(TARGET.0, TARGET.1, TARGET.2, BlockType::Chest, 0)
        .expect("seed chest through authoritative set_block");
    let mut gameplay = SessionGameplayState::default();
    gameplay.inventory[0] = Some(slot(ItemStack::new(Item::Diamond, 4)));
    runtime.authority.set_session_gameplay(player_id, gameplay);
    if let Some(player) = runtime.players.get_mut(&player_id) {
        player.data.position = OWNER_POSITION;
    }
    if let Some(session) = runtime.authority.session_mut(player_id) {
        session.position = OWNER_POSITION;
    }
}

fn inventory_wire(
    runtime: &ServerRuntime,
    player_id: u64,
) -> [Option<SessionInventorySlot>; SESSION_INVENTORY_SLOTS] {
    runtime
        .authority
        .session(player_id)
        .expect("authenticated session")
        .gameplay
        .inventory
}

fn dropped_item_count(runtime: &ServerRuntime) -> usize {
    runtime
        .authority
        .world()
        .entities
        .entities
        .iter()
        .filter(|entity| entity.entity_type == EntityType::DroppedItem)
        .count()
}

fn assert_block_use_rejected(
    runtime: &ServerRuntime,
    response: &icraft::network::protocol::GameplayResponse,
    player_id: u64,
    before_inventory: &[Option<SessionInventorySlot>; SESSION_INVENTORY_SLOTS],
    before_drops: usize,
    expected_block: BlockType,
) {
    assert!(
        matches!(
            response.outcome,
            GameplayOutcome::Rejected {
                reason: RejectReason::Unsupported
            }
        ),
        "BlockUse must be Unsupported, got {:?}",
        response.outcome
    );
    assert_eq!(
        runtime
            .authority
            .world()
            .get_block(TARGET.0, TARGET.1, TARGET.2),
        expected_block,
        "BlockUse must not mutate the target cell"
    );
    assert_eq!(
        &inventory_wire(runtime, player_id),
        before_inventory,
        "BlockUse must not consume inventory"
    );
    assert_eq!(
        dropped_item_count(runtime),
        before_drops,
        "BlockUse must not spawn DroppedItem"
    );
}

#[test]
fn embedded_block_use_diamond_ore_is_unsupported_and_preserves_world() {
    let properties = properties("embedded-ore");
    let world_dir = properties.world_dir.clone();
    let (mut runtime, input) = ServerRuntime::new_embedded(
        properties,
        EmbeddedRuntimeOptions {
            topology: AuthorityTopology::Singleplayer,
            transport: TransportMode::Disabled,
            local_session: Some(LocalSessionProfile::new(OWNER_ID, "plan01-owner")),
        },
    )
    .expect("construct embedded Plan01 runtime");
    let _ = runtime.tick_with_output().expect("login tick");
    seed_chest_and_inventory(&mut runtime, OWNER_ID);

    let before_inventory = inventory_wire(&runtime, OWNER_ID);
    let before_drops = dropped_item_count(&runtime);
    assert_eq!(
        runtime
            .authority
            .world()
            .get_block(TARGET.0, TARGET.1, TARGET.2),
        BlockType::Chest
    );

    input
        .submit_request(
            OWNER_ID,
            request(
                &runtime,
                OWNER_ID,
                1,
                1,
                GameplayOperation::BlockUse {
                    x: TARGET.0,
                    y: TARGET.1,
                    z: TARGET.2,
                    block: BlockType::DiamondOre.to_wire(),
                },
            ),
        )
        .expect("queue leftover BlockUse");
    let output = runtime.tick_with_output().expect("tick leftover BlockUse");
    let response = output
        .presentation_events
        .iter()
        .find_map(|event| match event {
            RuntimePresentationEvent::GameplayResponse { target, response }
                if *target == OWNER_ID && response.request_id == 1 =>
            {
                Some(response)
            }
            _ => None,
        })
        .expect("embedded BlockUse response");
    assert_block_use_rejected(
        &runtime,
        response,
        OWNER_ID,
        &before_inventory,
        before_drops,
        BlockType::Chest,
    );

    runtime
        .shutdown()
        .expect("shutdown embedded Plan01 runtime");
    let _ = fs::remove_dir_all(world_dir);
}

#[test]
fn embedded_block_use_air_cannot_clear_chest() {
    let properties = properties("embedded-air");
    let world_dir = properties.world_dir.clone();
    let (mut runtime, _input) = ServerRuntime::new_embedded(
        properties,
        EmbeddedRuntimeOptions {
            topology: AuthorityTopology::Singleplayer,
            transport: TransportMode::Disabled,
            local_session: Some(LocalSessionProfile::new(OWNER_ID, "plan01-air")),
        },
    )
    .expect("construct embedded Plan01 runtime");
    let _ = runtime.tick_with_output().expect("login tick");
    seed_chest_and_inventory(&mut runtime, OWNER_ID);

    let before_inventory = inventory_wire(&runtime, OWNER_ID);
    let before_drops = dropped_item_count(&runtime);
    let response = runtime
        .submit_request(
            OWNER_ID,
            request(
                &runtime,
                OWNER_ID,
                2,
                1,
                GameplayOperation::BlockUse {
                    x: TARGET.0,
                    y: TARGET.1,
                    z: TARGET.2,
                    block: BlockType::Air.to_wire(),
                },
            ),
        )
        .expect("direct leftover BlockUse");
    assert_block_use_rejected(
        &runtime,
        &response,
        OWNER_ID,
        &before_inventory,
        before_drops,
        BlockType::Chest,
    );
    assert!(
        runtime
            .authority
            .world()
            .get_block_entity(TARGET.0, TARGET.1, TARGET.2)
            .is_some(),
        "rejected Air BlockUse must not delete the chest block entity"
    );

    runtime
        .shutdown()
        .expect("shutdown embedded Plan01 runtime");
    let _ = fs::remove_dir_all(world_dir);
}

#[test]
fn tcp_block_use_is_rejected_without_world_or_inventory_mutation() {
    let reserved = HeldLoopback::bind();
    let mut properties = properties("tcp");
    properties.port = reserved.port();
    let address = format!("{}:{}", properties.bind, properties.port);
    let _port = reserved.release();
    let world_dir = properties.world_dir.clone();
    let mut runtime = ServerRuntime::new(properties).expect("construct dedicated Plan01 runtime");
    let mut client = TcpClient::connect(&address, "plan01-tcp");
    {
        let mut refs = [&mut client];
        drive_until(
            &mut runtime,
            &mut refs,
            "Plan01 TCP client authenticated",
            |runtime, views| views[0].player_id().is_some() && runtime.players.len() == 1,
        );
    }
    let player_id = client.player_id().expect("TCP player id");
    seed_chest_and_inventory(&mut runtime, player_id);
    let before_inventory = inventory_wire(&runtime, player_id);
    let before_drops = dropped_item_count(&runtime);

    let block_use = request(
        &runtime,
        player_id,
        11,
        1,
        GameplayOperation::BlockUse {
            x: TARGET.0,
            y: TARGET.1,
            z: TARGET.2,
            block: BlockType::DiamondOre.to_wire(),
        },
    );
    client.send_request(block_use);
    let response = {
        let mut refs = [&mut client];
        wait_for_response(&mut runtime, &mut refs, 0, 11)
    };
    assert_block_use_rejected(
        &runtime,
        &response,
        player_id,
        &before_inventory,
        before_drops,
        BlockType::Chest,
    );

    runtime.shutdown().expect("shutdown TCP Plan01 runtime");
    let _ = fs::remove_dir_all(world_dir);
}
