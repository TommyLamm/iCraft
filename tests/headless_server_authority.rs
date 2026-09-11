mod common;

use common::tcp_harness::{
    drive_until, gameplay_request, loopback_properties, session_slot, temp_world,
    wait_for_response, HeldLoopback, TcpClient, STEP_SLEEP,
};
use icraft::authority::contract::SessionGameplayState;
use icraft::authority::interest::InterestKind;
use icraft::block_entity::{BlockEntity, ChestBlockEntity, DispenserBlockEntity};
use icraft::dimension::Dimension;
use icraft::entity::EntityType;
use icraft::inventory::{Item, ItemStack};
use icraft::network::client::{ClientToGame, GameToClient};
use icraft::network::protocol::{BlockActionKind, ContainerAction, GameplayOperation, GameplayOutcome, GameplayRequest,
    GameplayResponse, ItemWire, RejectReason, SessionSlotWire, Packet};
use icraft::redstone::Direction;
use icraft::server_runtime::{ServerProperties, ServerRuntime};
use icraft::world::BlockType;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

const CHEST_POSITION: (i32, i32, i32) = (8, 80, 8);

struct TempWorld {
    path: PathBuf,
}

impl TempWorld {
    fn new() -> Self {
        let path = temp_world("headless-authority");
        fs::create_dir_all(&path).expect("create isolated headless test world");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempWorld {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn properties(world_dir: &Path, port: u16) -> ServerProperties {
    let mut properties = loopback_properties(world_dir, "127.0.0.1");
    properties.port = port;
    properties.max_players = 2;
    properties.seed = 0xC0FF_EE11;
    properties
}

fn drive_pair_until(
    runtime: &mut ServerRuntime,
    first: &mut TcpClient,
    second: &mut TcpClient,
    description: &str,
    mut ready: impl FnMut(&ServerRuntime, &TcpClient, &TcpClient) -> bool,
) {
    drive_until(
        runtime,
        &mut [first, second],
        description,
        |runtime, views| ready(runtime, views[0], views[1]),
    );
}

fn drive_one_until(
    runtime: &mut ServerRuntime,
    client: &mut TcpClient,
    description: &str,
    mut ready: impl FnMut(&ServerRuntime, &TcpClient) -> bool,
) {
    drive_until(
        runtime,
        &mut [client],
        description,
        |runtime, views| ready(runtime, views[0]),
    );
}

fn drive_pair_for(
    runtime: &mut ServerRuntime,
    first: &mut TcpClient,
    second: &mut TcpClient,
    duration: Duration,
) {
    let deadline = std::time::Instant::now() + duration;
    while std::time::Instant::now() < deadline {
        runtime.tick().expect("headless authority tick succeeds");
        first.drain();
        second.drain();
        thread::sleep(STEP_SLEEP);
    }
}

fn wait_for_pair_response(
    runtime: &mut ServerRuntime,
    client: &mut TcpClient,
    observer: &mut TcpClient,
    request_id: u128,
) -> GameplayResponse {
    wait_for_response(runtime, &mut [client, observer], 0, request_id)
}

fn accepted_revision(response: &GameplayResponse) -> u64 {
    match response.outcome {
        GameplayOutcome::Accepted { revision } => revision,
        GameplayOutcome::Rejected { reason } => {
            panic!("request {} was rejected: {reason:?}", response.request_id)
        }
    }
}

#[test]
fn two_clients_share_headless_authority_with_revision_interest_and_reconnect() {
    let world = TempWorld::new();
    let reserved = HeldLoopback::bind();
    let server_properties = properties(world.path(), reserved.port());
    let address = format!("127.0.0.1:{}", server_properties.port);
    let _port = reserved.release();
    let mut runtime =
        ServerRuntime::new(server_properties.clone()).expect("start headless authority runtime");
    let mut alice = TcpClient::connect(&address, "alice");
    let mut bob = TcpClient::connect(&address, "bob");

    drive_pair_until(
        &mut runtime,
        &mut alice,
        &mut bob,
        "two authenticated clients",
        |runtime, alice, bob| {
            alice.player_id().is_some()
                && bob.player_id().is_some()
                && runtime.players.len() == 2
                && runtime.metrics.queue_depth == 0
        },
    );
    let alice_id = alice.player_id().expect("alice authenticated");
    let bob_id = bob.player_id().expect("bob authenticated");
    assert_ne!(alice_id, bob_id);
    assert_eq!(alice.connected().map(|value| value.1), Some(0xC0FF_EE11));
    assert_eq!(bob.connected().map(|value| value.1), Some(0xC0FF_EE11));
    assert!(runtime.metrics.ticks > 0);
    assert!(runtime.metrics.last_tick_time_us > 0);
    assert!(runtime.metrics.max_tick_time_us >= runtime.metrics.last_tick_time_us);
    assert_eq!(runtime.metrics.players_online, 2);
    assert_eq!(
        runtime.metrics.loaded_chunks,
        runtime.authority.world_mut_active().chunks.chunks.len()
    );
    assert_eq!(
        runtime.metrics.entities,
        runtime.authority.world_mut_active().entities.entities.len()
    );
    assert!(runtime.metrics.inbound_packets >= 2);
    assert!(runtime.metrics.outbound_packets >= 2);
    assert!(runtime.metrics.inbound_bytes >= runtime.metrics.inbound_packets.saturating_mul(4));
    assert!(runtime.metrics.outbound_bytes >= runtime.metrics.outbound_packets.saturating_mul(4));
    assert_eq!(runtime.metrics.queue_depth, 0);

    // SessionGameplayUpdate is a private projection lane: the owner receives
    // its rich gameplay snapshot during login while the other authenticated
    // client never sees Alice's session payload.
    assert!(alice.events().iter().any(|event| {
        matches!(
            event,
            ClientToGame::Packet(Packet::PlayerSessionUpdate { player_id, .. }) if *player_id == alice_id
        )
    }));
    assert!(!bob.events().iter().any(|event| {
        matches!(
            event,
            ClientToGame::Packet(Packet::PlayerSessionUpdate { player_id, .. }) if *player_id == alice_id
        )
    }));

    alice.send(GameToClient::SendPosition {
        sequence: 1,
        sender_time_millis: 1,
        x: 8.0,
        y: 80.0,
        z: 8.0,
        yaw: 0.0,
        pitch: 0.0,
    });
    bob.send(GameToClient::SendPosition {
        sequence: 1,
        sender_time_millis: 1,
        x: 512.0,
        y: 80.0,
        z: 512.0,
        yaw: 0.0,
        pitch: 0.0,
    });
    drive_pair_until(
        &mut runtime,
        &mut alice,
        &mut bob,
        "dimension-aware interest positions",
        |runtime, _, _| {
            runtime
                .players
                .get(&alice_id)
                .is_some_and(|player| player.data.position == [8.0, 80.0, 8.0])
                && runtime
                    .players
                    .get(&bob_id)
                    .is_some_and(|player| player.data.position == [512.0, 80.0, 512.0])
        },
    );
    runtime.drain_routed_updates();
    alice.clear_events();
    bob.clear_events();

    let base_revision = runtime.authority.current_revision();
    let chest_mutation = runtime
        .authority
        .world_mut_active()
        .set_block(
            CHEST_POSITION.0,
            CHEST_POSITION.1,
            CHEST_POSITION.2,
            BlockType::Chest,
            0,
        )
        .expect("authoritative chest seed")
        .expect("chest seed must change the cell");
    let block_revision = chest_mutation.revision;
    assert!(block_revision > base_revision);
    assert_eq!(
        runtime
            .authority
            .world()
            .get_block(CHEST_POSITION.0, CHEST_POSITION.1, CHEST_POSITION.2),
        BlockType::Chest
    );

    const BLOCK_REQUEST: u128 = 0xA001;
    let leftover_block_use = gameplay_request(
        &runtime,
        alice_id,
        BLOCK_REQUEST,
        1,
        GameplayOperation::BlockAction {
            action: BlockActionKind::Place,
            x: CHEST_POSITION.0,
            y: CHEST_POSITION.1,
            z: CHEST_POSITION.2,
            face: [0, 1, 0],
            hand: 0,
            held: Some(SessionSlotWire::new(
                ItemWire::from_stack(&ItemStack::new(Item::Stone, 1)),
                0,
                0,
            )),
            block: BlockType::DiamondOre.to_wire(),
            look_milli: [0, 0, 1000],
        },
    );
    alice.send(GameToClient::GameplayRequest {
        request: leftover_block_use.clone(),
    });
    let block_response = wait_for_pair_response(&mut runtime, &mut alice, &mut bob, BLOCK_REQUEST);
    assert_eq!(
        block_response.outcome,
        GameplayOutcome::Rejected {
            reason: RejectReason::InvalidState
        }
    );
    assert_eq!(
        runtime
            .authority
            .world()
            .get_block(CHEST_POSITION.0, CHEST_POSITION.1, CHEST_POSITION.2),
        BlockType::Chest,
        "rejected BlockAction must not overwrite the seeded chest"
    );

    let accepted_before_replay = runtime.metrics.requests_accepted;
    let rejected_before_replay = runtime.metrics.requests_rejected;
    let duplicate_before_replay = runtime.metrics.duplicate_requests;
    alice.send(GameToClient::GameplayRequest {
        request: leftover_block_use,
    });
    drive_pair_for(
        &mut runtime,
        &mut alice,
        &mut bob,
        Duration::from_millis(250),
    );
    assert!(
        alice.take_response(BLOCK_REQUEST).is_none(),
        "the client response gate must suppress a replayed cached response"
    );
    assert_eq!(runtime.metrics.requests_accepted, accepted_before_replay);
    assert_eq!(runtime.metrics.requests_rejected, rejected_before_replay);
    assert_eq!(
        runtime.metrics.duplicate_requests,
        duplicate_before_replay + 1,
        "the replay is observed once while the authority still executes only once"
    );
    assert_eq!(
        runtime
            .authority
            .session(alice_id)
            .and_then(|session| session.cached_response(BLOCK_REQUEST)),
        Some(block_response),
        "the original authority response remains the sole cached execution"
    );

    const OUT_OF_ORDER_REQUEST: u128 = 0xA002;
    alice.send(GameToClient::GameplayRequest {
        request: gameplay_request(
            &runtime,
            alice_id,
            OUT_OF_ORDER_REQUEST,
            1,
            GameplayOperation::ItemUse {
                item: Item::Bread as u32,
                count: 1,
            },
        ),
    });
    let out_of_order = wait_for_pair_response(&mut runtime, &mut alice, &mut bob, OUT_OF_ORDER_REQUEST);
    assert_eq!(
        out_of_order.outcome,
        GameplayOutcome::Rejected {
            reason: RejectReason::OutOfOrder
        }
    );

    const STALE_REQUEST: u128 = 0xA003;
    alice.send(GameToClient::GameplayRequest {
        request: GameplayRequest {
            client_revision: 0,
            ..gameplay_request(
                &runtime,
                alice_id,
                STALE_REQUEST,
                2,
                GameplayOperation::ItemUse {
                    item: Item::Bread as u32,
                    count: 1,
                },
            )
        },
    });
    let stale = wait_for_pair_response(&mut runtime, &mut alice, &mut bob, STALE_REQUEST);
    assert_eq!(
        stale.outcome,
        GameplayOutcome::Rejected {
            reason: RejectReason::InvalidRevision
        }
    );
    assert_eq!(runtime.metrics.requests_accepted, accepted_before_replay);
    assert_eq!(
        runtime.metrics.requests_rejected,
        rejected_before_replay + 2
    );
    assert_eq!(
        runtime.metrics.duplicate_requests,
        duplicate_before_replay + 1
    );

    assert!(runtime.teleport_session(bob_id, [10.0, 80.0, 8.0]));
    drive_pair_until(
        &mut runtime,
        &mut alice,
        &mut bob,
        "nearby non-viewer interest",
        |runtime, _, _| {
            runtime
                .players
                .get(&bob_id)
                .is_some_and(|player| player.data.position == [10.0, 80.0, 8.0])
        },
    );
    runtime.drain_routed_updates();
    alice.clear_events();
    bob.clear_events();

    const OPEN_REQUEST: u128 = 0xA004;
    alice.send(GameToClient::GameplayRequest {
        request: gameplay_request(
            &runtime,
            alice_id,
            OPEN_REQUEST,
            3,
            GameplayOperation::Container {
                action: ContainerAction::Open,
                x: CHEST_POSITION.0,
                y: CHEST_POSITION.1,
                z: CHEST_POSITION.2,
                slot: 0,
            },
        ),
    });
    let open_response = wait_for_pair_response(&mut runtime, &mut alice, &mut bob, OPEN_REQUEST);
    let open_revision = accepted_revision(&open_response);
    drive_pair_until(
        &mut runtime,
        &mut alice,
        &mut bob,
        "container-open projection",
        |_, alice, _| {
            alice.events().iter().any(|event| {
                matches!(
                    event,
                    ClientToGame::Packet(Packet::ContainerOpenResult { x, y, z, .. })
                        if (*x, *y, *z) == CHEST_POSITION
                )
            })
        },
    );
    let (slots, projected_open_revision) = alice
        .take_open_result(CHEST_POSITION)
        .expect("alice receives the container contents");
    assert_eq!(slots.len(), 27);
    assert_eq!(projected_open_revision, open_revision);
    let open_updates = runtime.drain_routed_updates();
    let block_entity_targets: BTreeSet<_> = open_updates
        .iter()
        .filter_map(|update| {
            (update.kind == InterestKind::BlockEntity(CHEST_POSITION)).then_some(update.target)
        })
        .collect();
    let container_targets: BTreeSet<_> = open_updates
        .iter()
        .filter_map(|update| {
            (update.kind == InterestKind::Container(CHEST_POSITION)).then_some(update.target)
        })
        .collect();
    assert_eq!(block_entity_targets, BTreeSet::from([alice_id, bob_id]));
    assert_eq!(
        container_targets,
        BTreeSet::from([alice_id]),
        "container contents are routed only to authenticated viewers"
    );
    drive_pair_for(
        &mut runtime,
        &mut alice,
        &mut bob,
        Duration::from_millis(100),
    );
    assert!(
        !bob.has_private_container_event(),
        "a nearby non-viewer received private container state"
    );

    alice.clear_events();
    bob.clear_events();
    runtime.drain_routed_updates();
    let dirt = ItemStack::new(Item::Dirt, 1);
    let mut chest = ChestBlockEntity::new();
    chest.set_stack(0, Some(dirt));
    runtime
        .authority
        .world_mut_active()
        .chunks
        .set_block_entity(
            CHEST_POSITION.0,
            CHEST_POSITION.1,
            CHEST_POSITION.2,
            Some(BlockEntity::Chest(chest)),
        );
    let stone_stack = ItemStack::new(Item::Stone, 2);
    let stone = ItemWire::from_stack(&stone_stack);
    let mut alice_gameplay = runtime
        .authority
        .session(alice_id)
        .map(|session| session.gameplay)
        .unwrap_or_else(SessionGameplayState::default);
    alice_gameplay.inventory[0] = Some(session_slot(stone_stack));
    assert!(runtime
        .authority
        .set_session_gameplay(alice_id, alice_gameplay));
    const CLICK_REQUEST: u128 = 0xA005;
    alice.send(GameToClient::GameplayRequest {
        request: gameplay_request(
            &runtime,
            alice_id,
            CLICK_REQUEST,
            4,
            GameplayOperation::ContainerClick {
                x: CHEST_POSITION.0,
                y: CHEST_POSITION.1,
                z: CHEST_POSITION.2,
                slot: 0,
                is_left: true,
                dragged: Some(stone),
            },
        ),
    });
    let click_response = wait_for_pair_response(&mut runtime, &mut alice, &mut bob, CLICK_REQUEST);
    let click_revision = accepted_revision(&click_response);
    drive_pair_until(
        &mut runtime,
        &mut alice,
        &mut bob,
        "container-click projection",
        |_, alice, _| {
            alice.events().iter().any(|event| {
                matches!(
                    event,
                    ClientToGame::Packet(Packet::ContainerClickResult { slot_index: 0, .. })
                )
            })
        },
    );
    let (slot, dragged) = alice
        .take_click_result(0)
        .expect("alice receives the authoritative clicked slot");
    assert_eq!(slot, Some(stone));
    assert_eq!(dragged, Some(ItemWire::from_stack(&dirt)));
    let click_container_targets: BTreeSet<_> = runtime
        .drain_routed_updates()
        .iter()
        .filter_map(|update| {
            (update.kind == InterestKind::Container(CHEST_POSITION)).then_some(update.target)
        })
        .collect();
    assert_eq!(click_container_targets, BTreeSet::from([alice_id]));
    drive_pair_for(
        &mut runtime,
        &mut alice,
        &mut bob,
        Duration::from_millis(100),
    );
    assert!(
        !bob.has_private_container_event(),
        "a non-viewer received a container slot mutation"
    );

    alice.disconnect_and_join();
    bob.disconnect_and_join();
    drive_pair_until(
        &mut runtime,
        &mut alice,
        &mut bob,
        "disconnect persistence",
        |runtime, _, _| runtime.players.is_empty() && runtime.metrics.queue_depth == 0,
    );
    let saves_before_shutdown = runtime.metrics.saves;
    runtime.shutdown().expect("save and stop first runtime");
    assert_eq!(runtime.metrics.saves, saves_before_shutdown + 1);
    assert!(runtime.metrics.last_save_latency_ms >= 1);
    assert_eq!(runtime.metrics.players_online, 0);
    assert_eq!(runtime.metrics.queue_depth, 0);
    drop(runtime);

    let mut restarted =
        ServerRuntime::new(server_properties).expect("restart authority from persisted world");
    assert_eq!(
        restarted
            .authority
            .world()
            .get_block(CHEST_POSITION.0, CHEST_POSITION.1, CHEST_POSITION.2),
        BlockType::Chest
    );
    assert_eq!(
        restarted
            .authority
            .world_mut_active()
            .container_slot_wire(CHEST_POSITION, 0),
        Some(Some(stone)),
        "container mutation must survive server restart exactly once"
    );
    assert!(
        restarted
            .authority
            .world_mut_active()
            .container_viewers_at(CHEST_POSITION)
            .next()
            .is_none(),
        "ephemeral container viewers must not leak across restart"
    );
    assert!(restarted.authority.current_revision() >= click_revision);

    let mut reconnected = TcpClient::connect(&address, "alice");
    drive_one_until(
        &mut restarted,
        &mut reconnected,
        "saved player reconnect",
        |runtime, client| {
            client.player_id().is_some()
                && client
                    .player_id()
                    .and_then(|id| runtime.players.get(&id))
                    .is_some_and(|player| player.data.position == [8.0, 80.0, 8.0])
        },
    );
    let reconnected_id = reconnected.player_id().expect("alice reconnected");
    assert_eq!(
        restarted.players[&reconnected_id].interest.dimension,
        Dimension::Overworld
    );
    reconnected.disconnect_and_join();
    drive_one_until(
        &mut restarted,
        &mut reconnected,
        "reconnected player logout",
        |runtime, _| runtime.players.is_empty(),
    );
    restarted.shutdown().expect("stop restarted runtime");
}

fn projected_dropped_item(client: &TcpClient) -> Option<(u64, ItemWire)> {
    client.events().iter().find_map(|event| {
        let state = match event {
            ClientToGame::Packet(Packet::EntitySpawn { state, .. }) | ClientToGame::Packet(Packet::EntityState { state, .. })
                if state.entity_type == EntityType::DroppedItem.to_wire() =>
            {
                state
            }
            _ => return None,
        };
        state.item.map(|item| (state.entity_id, item))
    })
}

fn projected_block_entity(
    client: &TcpClient,
    position: (i32, i32, i32),
) -> Option<(u64, BlockEntity)> {
    client.events().iter().find_map(|event| match event {
        ClientToGame::Packet(Packet::BlockEntityDelta {
            x,
            y,
            z,
            revision,
            entity: Some(entity),
            ..
        }) if (*x, *y, *z) == position => Some((*revision, entity.clone())),
        _ => None,
    })
}

#[test]
fn tcp_dispenser_drop_projection_converges_complete_item_metadata() {
    let world = TempWorld::new();
    let reserved = HeldLoopback::bind();
    let server_properties = properties(world.path(), reserved.port());
    let address = format!("127.0.0.1:{}", server_properties.port);
    let _port = reserved.release();
    let mut runtime =
        ServerRuntime::new(server_properties).expect("start headless dispenser runtime");
    let mut alice = TcpClient::connect(&address, "alice");
    let mut bob = TcpClient::connect(&address, "bob");

    drive_pair_until(
        &mut runtime,
        &mut alice,
        &mut bob,
        "two authenticated dispenser viewers",
        |runtime, alice, bob| {
            alice.player_id().is_some() && bob.player_id().is_some() && runtime.players.len() == 2
        },
    );
    let alice_id = alice.player_id().expect("alice authenticated");
    let bob_id = bob.player_id().expect("bob authenticated");
    assert!(runtime.teleport_session(alice_id, [8.0, 80.0, 8.0]));
    assert!(runtime.teleport_session(bob_id, [8.0, 80.0, 8.0]));
    drive_pair_until(
        &mut runtime,
        &mut alice,
        &mut bob,
        "both clients entering dispenser interest",
        |runtime, _, _| {
            runtime
                .players
                .get(&alice_id)
                .is_some_and(|session| session.data.position == [8.0, 80.0, 8.0])
                && runtime
                    .players
                    .get(&bob_id)
                    .is_some_and(|session| session.data.position == [8.0, 80.0, 8.0])
        },
    );
    runtime.drain_routed_updates();
    alice.clear_events();
    bob.clear_events();

    // Fixture setup is intentionally direct authority mutation.  The edge,
    // entity allocation, transport fanout, and metadata assertions below all
    // cross the real TCP/runtime projection boundary.
    let source = (8, 80, 8);
    let front = (9, 80, 8);
    let lever = (7, 80, 8);
    runtime
        .authority
        .world_mut_active()
        .set_block(source.0, source.1, source.2, BlockType::Dispenser, 0)
        .expect("place dispenser fixture");
    runtime
        .authority
        .world_mut_active()
        .set_block(front.0, front.1, front.2, BlockType::Air, 0)
        .expect("clear dispenser front");
    runtime
        .authority
        .world_mut_active()
        .set_block(lever.0, lever.1, lever.2, BlockType::LeverOn, 0)
        .expect("place powered lever fixture");
    let mut stack = ItemStack::new(Item::Stone, 2)
        .with_can_break(BlockType::Dirt)
        .with_can_place_on(BlockType::Stone);
    stack.custom_name.set("tcp-drop");
    if let Some(BlockEntity::Dispenser(dispenser)) = runtime
        .authority
        .world_mut_active()
        .chunks
        .get_block_entity_mut(source.0, source.1, source.2)
    {
        let mut entity = DispenserBlockEntity::new();
        entity.slots[0] = Some(stack);
        *dispenser = entity;
    } else {
        panic!("dispenser block entity fixture is missing");
    }
    {
        let world = runtime.authority.world_mut_active();

        world
            .redstone
            .on_block_changed(&world.chunks, lever, Direction::East);
    }
    {
        let world = runtime.authority.world_mut_active();

        world
            .redstone
            .on_block_changed(&world.chunks, source, Direction::East);
    }

    drive_pair_until(
        &mut runtime,
        &mut alice,
        &mut bob,
        "authoritative dropped-item metadata over TCP",
        |_, alice, bob| {
            projected_dropped_item(alice).is_some()
                && projected_dropped_item(bob).is_some()
                && projected_block_entity(alice, source).is_some()
                && projected_block_entity(bob, source).is_some()
        },
    );
    let (alice_entity, alice_item) =
        projected_dropped_item(&alice).expect("alice receives dropped item projection");
    let (bob_entity, bob_item) =
        projected_dropped_item(&bob).expect("bob receives dropped item projection");
    assert_eq!(
        alice_entity, bob_entity,
        "authority uses one global entity id"
    );
    assert_eq!(alice_item.item, Item::Stone.to_u32());
    assert_eq!(bob_item.item, Item::Stone.to_u32());
    assert_eq!(alice_item.count, 1);
    assert_eq!(bob_item.count, 1);
    assert_eq!(alice_item.custom_name, bob_item.custom_name);
    assert_eq!(alice_item.can_break, 1u128 << (BlockType::Dirt as u8));
    assert_eq!(bob_item.can_place_on, 1u128 << (BlockType::Stone as u8));
    let (alice_source_revision, alice_source_entity) =
        projected_block_entity(&alice, source).expect("alice receives dispenser slot delta");
    let (bob_source_revision, bob_source_entity) =
        projected_block_entity(&bob, source).expect("bob receives dispenser slot delta");
    let (alice_source_be_revision, alice_source_slot) = match alice_source_entity {
        BlockEntity::Dispenser(dispenser) => (
            dispenser.revision,
            dispenser.slots[0].map(|stack| ItemWire::from_stack(&stack)),
        ),
        _ => panic!("alice received a non-dispenser source delta"),
    };
    let (bob_source_be_revision, bob_source_slot) = match bob_source_entity {
        BlockEntity::Dispenser(dispenser) => (
            dispenser.revision,
            dispenser.slots[0].map(|stack| ItemWire::from_stack(&stack)),
        ),
        _ => panic!("bob received a non-dispenser source delta"),
    };
    assert_eq!(alice_source_revision, bob_source_revision);
    assert_eq!(alice_source_be_revision, bob_source_be_revision);
    assert_eq!(alice_source_slot, bob_source_slot);
    assert_eq!(alice_source_slot.map(|slot| slot.count), Some(1));

    // Sustained power is a latch, not a repeated action.  A second source item
    // remains after several fixed ticks, proving no phantom duplicate spawn.
    drive_pair_for(
        &mut runtime,
        &mut alice,
        &mut bob,
        Duration::from_millis(150),
    );
    let source_count = match runtime
        .authority
        .world()
        .get_block_entity(source.0, source.1, source.2)
    {
        Some(BlockEntity::Dispenser(dispenser)) => dispenser.slots[0].map_or(0, |item| item.count),
        _ => 0,
    };
    assert_eq!(source_count, 1, "sustained power must not dispense twice");

    // Turn the edge off, replace the source/front fixture, then raise it again
    // as a Dropper -> Chest insertion. Both viewers must observe source
    // decrement and target merge; no second DroppedItem may be spawned.
    runtime
        .authority
        .world_mut_active()
        .set_block(lever.0, lever.1, lever.2, BlockType::Lever, 0)
        .expect("turn dispenser fixture off");
    {
        let world = runtime.authority.world_mut_active();

        world
            .redstone
            .on_block_changed(&world.chunks, lever, Direction::East);
    }
    drive_pair_for(
        &mut runtime,
        &mut alice,
        &mut bob,
        Duration::from_millis(80),
    );
    runtime.drain_routed_updates();
    alice.clear_events();
    bob.clear_events();

    runtime
        .authority
        .world_mut_active()
        .chunks
        .set_block_entity(source.0, source.1, source.2, None);
    runtime
        .authority
        .world_mut_active()
        .set_block(source.0, source.1, source.2, BlockType::Dropper, 0)
        .expect("replace source with dropper");
    runtime
        .authority
        .world_mut_active()
        .set_block(front.0, front.1, front.2, BlockType::Chest, 0)
        .expect("place dropper target chest");
    let mut dropper_stack = stack;
    dropper_stack.count = 2;
    if let Some(BlockEntity::Dropper(dropper)) = runtime
        .authority
        .world_mut_active()
        .chunks
        .get_block_entity_mut(source.0, source.1, source.2)
    {
        let mut entity = icraft::block_entity::DropperBlockEntity::new();
        entity.slots[0] = Some(dropper_stack);
        *dropper = entity;
    } else {
        panic!("dropper block entity fixture is missing");
    }
    if let Some(BlockEntity::Chest(chest)) = runtime
        .authority
        .world_mut_active()
        .chunks
        .get_block_entity_mut(front.0, front.1, front.2)
    {
        let mut target_stack = stack;
        target_stack.count = 4;
        chest.inventory.slots[0] = Some(target_stack);
    } else {
        panic!("dropper target chest fixture is missing");
    }
    {
        let world = runtime.authority.world_mut_active();

        world
            .redstone
            .on_block_changed(&world.chunks, source, Direction::East);
    }
    runtime
        .authority
        .world_mut_active()
        .set_block(lever.0, lever.1, lever.2, BlockType::LeverOn, 0)
        .expect("raise dropper fixture edge");
    {
        let world = runtime.authority.world_mut_active();

        world
            .redstone
            .on_block_changed(&world.chunks, lever, Direction::East);
    }

    drive_pair_until(
        &mut runtime,
        &mut alice,
        &mut bob,
        "dropper target insertion over TCP",
        |_, alice, bob| {
            projected_block_entity(alice, source).is_some()
                && projected_block_entity(bob, source).is_some()
                && projected_block_entity(alice, front).is_some()
                && projected_block_entity(bob, front).is_some()
        },
    );
    let (_, alice_dropper) =
        projected_block_entity(&alice, source).expect("alice receives dropper source delta");
    let (_, bob_dropper) =
        projected_block_entity(&bob, source).expect("bob receives dropper source delta");
    let (_, alice_chest) =
        projected_block_entity(&alice, front).expect("alice receives chest target delta");
    let (_, bob_chest) =
        projected_block_entity(&bob, front).expect("bob receives chest target delta");
    let source_slot = |entity: &BlockEntity| match entity {
        BlockEntity::Dropper(dropper) => dropper.slots[0].map(|stack| ItemWire::from_stack(&stack)),
        _ => None,
    };
    let chest_slot = |entity: &BlockEntity| match entity {
        BlockEntity::Chest(chest) => {
            chest.inventory.slots[0].map(|stack| ItemWire::from_stack(&stack))
        }
        _ => None,
    };
    assert_eq!(source_slot(&alice_dropper), source_slot(&bob_dropper));
    assert_eq!(chest_slot(&alice_chest), chest_slot(&bob_chest));
    assert_eq!(source_slot(&alice_dropper).map(|slot| slot.count), Some(1));
    assert_eq!(chest_slot(&alice_chest).map(|slot| slot.count), Some(5));
    let authority_dropped_ids: BTreeSet<_> = runtime
        .authority
        .world()
        .entities
        .entities
        .iter()
        .filter(|entity| entity.entity_type == EntityType::DroppedItem)
        .map(|entity| entity.id)
        .collect();
    assert_eq!(
        authority_dropped_ids,
        BTreeSet::from([alice_entity]),
        "dropper insertion must not create a fallback dropped entity"
    );
    assert_eq!(alice_entity, bob_entity);

    alice.disconnect_and_join();
    bob.disconnect_and_join();
    drive_pair_until(
        &mut runtime,
        &mut alice,
        &mut bob,
        "dispenser viewer disconnect",
        |runtime, _, _| runtime.players.is_empty(),
    );
    runtime.shutdown().expect("stop headless dispenser runtime");
}
