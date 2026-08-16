mod common;

use common::tcp_harness::{drive_until, wait_for_response, TcpClient};
use icraft::authority::contract::{
    AuthorityTopology, SessionGameplayState, SessionInventorySlot, SESSION_INVENTORY_SLOTS,
};
use icraft::dimension::Dimension;
use icraft::entity::EntityType;
use icraft::inventory::{Item, ItemStack};
use icraft::network::protocol::{
    GameplayOperation, GameplayOutcome, GameplayRequest, RejectReason,
};
use icraft::server_runtime::{
    EmbeddedRuntimeOptions, LocalSessionProfile, RuntimePresentationEvent, ServerProperties,
    ServerRuntime, TransportMode,
};
use icraft::world::BlockType;
use std::fs;
use std::net::TcpListener;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const OWNER_ID: u64 = 0x01_0001;
const TARGET: (i32, i32, i32) = (8, 80, 8);
const OWNER_POSITION: [f32; 3] = [8.0, 80.0, 8.0];

fn temp_world(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!("icraft-plan01-block-use-{label}-{nonce}"))
}

fn reserve_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("reserve loopback test port")
        .local_addr()
        .expect("read loopback port")
        .port()
}

fn properties(label: &str, port: u16) -> ServerProperties {
    ServerProperties {
        bind: "127.0.0.1".into(),
        port,
        max_players: 4,
        view_distance: 2,
        simulation_distance: 2,
        seed: 0x01_01_01_01,
        world_dir: temp_world(label),
        ..ServerProperties::default()
    }
}

fn slot(stack: ItemStack) -> SessionInventorySlot {
    SessionInventorySlot::from_wire(
        icraft::network::protocol::ItemWire::from_stack(&stack),
        stack.can_break,
        stack.can_place_on,
    )
}

fn request(
    runtime: &ServerRuntime,
    player_id: u64,
    request_id: u128,
    sequence: u64,
    operation: GameplayOperation,
) -> GameplayRequest {
    let dimension = runtime
        .authority
        .session(player_id)
        .and_then(|session| Dimension::from_wire(session.dimension))
        .expect("fixture session dimension");
    GameplayRequest {
        request_id,
        client_sequence: sequence,
        session_id: player_id,
        dimension: dimension as u8,
        client_revision: runtime.authority.revision_for_dimension(dimension),
        operation,
    }
}

fn seed_chest_and_inventory(runtime: &mut ServerRuntime, player_id: u64) {
    runtime.authority.world.ensure_chunk(0, 0);
    runtime
        .authority
        .world
        .set_block(TARGET.0, TARGET.1, TARGET.2, BlockType::Chest, 0)
        .expect("seed chest through authoritative set_block");
    let mut gameplay = SessionGameplayState::default();
    gameplay.inventory[0] = Some(slot(ItemStack::new(Item::Diamond, 4)));
    runtime
        .authority
        .set_session_gameplay(player_id, gameplay);
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
        .world
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
            .world
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
    let properties = properties("embedded-ore", reserve_port());
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
            .world
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
            RuntimePresentationEvent::GameplayResponse {
                target,
                response,
            } if *target == OWNER_ID && response.request_id == 1 => Some(response),
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

    runtime.shutdown().expect("shutdown embedded Plan01 runtime");
    let _ = fs::remove_dir_all(world_dir);
}

#[test]
fn embedded_block_use_air_cannot_clear_chest() {
    let properties = properties("embedded-air", reserve_port());
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
            .world
            .get_block_entity(TARGET.0, TARGET.1, TARGET.2)
            .is_some(),
        "rejected Air BlockUse must not delete the chest block entity"
    );

    runtime.shutdown().expect("shutdown embedded Plan01 runtime");
    let _ = fs::remove_dir_all(world_dir);
}

#[test]
fn tcp_block_use_is_rejected_without_world_or_inventory_mutation() {
    let properties = properties("tcp", reserve_port());
    let address = format!("{}:{}", properties.bind, properties.port);
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
