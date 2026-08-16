mod common;

use common::tcp_harness::{
    drive_until, gameplay_request as request, loopback_properties, session_slot as slot,
    wait_for_response, HeldLoopback, TcpClient,
};
use icraft::authority::contract::{AuthorityTopology, SessionGameplayState};
use icraft::block_entity::BlockEntity;
use icraft::dimension::Dimension;
use icraft::entity::EntityType;
use icraft::inventory::{GameMode, Item, ItemStack};
use icraft::network::client::ClientToGame;
use icraft::network::protocol::{
    BlockActionKind, GameplayOperation, GameplayOutcome, GameplayRequest, ItemWire, RejectReason,
    SessionSlotWire,
};
use icraft::server_runtime::{
    EmbeddedRuntimeOptions, LocalSessionProfile, ServerProperties, ServerRuntime, TransportMode,
};
use icraft::structure::StructureId;
use icraft::world::BlockType;
use std::fs;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const LOCAL_ID: u64 = 0x32_0000;
const FRAME_BASE: (i32, i32, i32) = (10, 65, 10);
const PORTAL_CELL: (i32, i32, i32) = (11, 66, 10);
const PORTAL_LOOK: [i16; 3] = [0, -500, 866];

fn properties(label: &str) -> ServerProperties {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let mut properties = loopback_properties(
        std::env::temp_dir().join(format!(
            "icraft_plan32_{label}_{}_{}",
            std::process::id(),
            nonce
        )),
        "127.0.0.1",
    );
    properties.seed = 12_345;
    properties.view_distance = 4;
    properties.simulation_distance = 4;
    properties.max_players = 20;
    properties
}

fn held(stack: &ItemStack) -> SessionSlotWire {
    SessionSlotWire::new(
        ItemWire::from_stack(stack),
        stack.can_break,
        stack.can_place_on,
    )
}

fn block_action(
    runtime: &ServerRuntime,
    player_id: u64,
    request_id: u128,
    sequence: u64,
    action: BlockActionKind,
    position: (i32, i32, i32),
    held: Option<SessionSlotWire>,
    block: BlockType,
) -> GameplayRequest {
    request(
        runtime,
        player_id,
        request_id,
        sequence,
        GameplayOperation::BlockAction {
            action,
            x: position.0,
            y: position.1,
            z: position.2,
            face: if matches!(action, BlockActionKind::EnterPortal) {
                [0, 0, 0]
            } else {
                [0, 1, 0]
            },
            hand: 0,
            held,
            block: block.to_wire(),
            look_milli: PORTAL_LOOK,
        },
    )
}

fn seed_nether_frame(runtime: &mut ServerRuntime) {
    runtime.authority.with_world(Dimension::Overworld, |world| {
        let (base_x, base_y, base_z) = FRAME_BASE;
        for x in base_x..=base_x + 3 {
            world
                .set_block(x, base_y, base_z, BlockType::Obsidian, 0)
                .unwrap();
            world
                .set_block(x, base_y + 4, base_z, BlockType::Obsidian, 0)
                .unwrap();
        }
        for y in base_y + 1..=base_y + 3 {
            world
                .set_block(base_x, y, base_z, BlockType::Obsidian, 0)
                .unwrap();
            world
                .set_block(base_x + 3, y, base_z, BlockType::Obsidian, 0)
                .unwrap();
        }
    });
}

fn prepare_player(runtime: &mut ServerRuntime, id: u64, position: [f32; 3], item: ItemStack) {
    let mut gameplay = SessionGameplayState::default();
    if item.item != Item::Air && item.count > 0 {
        gameplay.inventory[0] = Some(slot(item));
    }
    assert!(runtime.authority.set_session_gameplay(id, gameplay));
    if let Some(session) = runtime.players.get_mut(&id) {
        session.data.position = position;
    }
    if let Some(session) = runtime.authority.session_mut(id) {
        session.position = position;
    }
}

#[test]
fn singleplayer_typed_nether_activation_and_transfer() {
    let props = properties("singleplayer");
    let (mut runtime, input) = ServerRuntime::new_embedded(
        props.clone(),
        EmbeddedRuntimeOptions::singleplayer(LocalSessionProfile::new(LOCAL_ID, "plan32-local")),
    )
    .unwrap();
    seed_nether_frame(&mut runtime);
    let flint = ItemStack::new(Item::FlintAndSteel, 1);
    prepare_player(&mut runtime, LOCAL_ID, [11.0, 65.0, 9.0], flint);

    let ignite = block_action(
        &runtime,
        LOCAL_ID,
        1,
        1,
        BlockActionKind::IgnitePortal,
        PORTAL_CELL,
        Some(held(&flint)),
        BlockType::Fire,
    );
    input.submit_request(LOCAL_ID, ignite.clone()).unwrap();
    let mut output = runtime.tick_with_output().unwrap();
    let first = output
        .presentation_events
        .iter()
        .find_map(|event| match event {
            icraft::server_runtime::RuntimePresentationEvent::GameplayResponse {
                response, ..
            } if response.request_id == 1 => Some(response.clone()),
            _ => None,
        })
        .expect("typed ignite ACK");
    assert!(matches!(first.outcome, GameplayOutcome::Accepted { .. }));
    input.submit_request(LOCAL_ID, ignite).unwrap();
    output = runtime.tick_with_output().unwrap();
    assert!(output.presentation_events.iter().any(|event| matches!(
        event,
        icraft::server_runtime::RuntimePresentationEvent::GameplayResponse { response, .. }
            if *response == first
    )));
    assert_eq!(
        runtime
            .authority
            .world_ref(Dimension::Overworld)
            .unwrap()
            .get_block(PORTAL_CELL.0, PORTAL_CELL.1, PORTAL_CELL.2),
        BlockType::NetherPortal
    );

    prepare_player(
        &mut runtime,
        LOCAL_ID,
        [
            PORTAL_CELL.0 as f32,
            PORTAL_CELL.1 as f32,
            PORTAL_CELL.2 as f32,
        ],
        ItemStack::new(Item::Air, 0),
    );
    let enter = block_action(
        &runtime,
        LOCAL_ID,
        2,
        2,
        BlockActionKind::EnterPortal,
        PORTAL_CELL,
        None,
        BlockType::NetherPortal,
    );
    input.submit_request(LOCAL_ID, enter).unwrap();
    let mut transferred = false;
    for _ in 0..25 {
        let output = runtime.tick_with_output().unwrap();
        transferred |= output.presentation_events.iter().any(|event| matches!(
            event,
            icraft::server_runtime::RuntimePresentationEvent::DimensionTransfer { target, dimension, .. }
                if *target == LOCAL_ID && *dimension == Dimension::Nether as u8
        ));
    }
    assert!(transferred);
    assert_eq!(runtime.players[&LOCAL_ID].dimension, Dimension::Nether);
    assert_eq!(
        runtime.authority.session(LOCAL_ID).unwrap().dimension,
        Dimension::Nether as u8
    );
    runtime.shutdown().unwrap();
    let _ = fs::remove_dir_all(props.world_dir);
}

fn run_tcp_travel(label: &str, listen: bool) {
    let reserved = HeldLoopback::bind();
    let mut props = properties(label);
    props.port = reserved.port();
    let address = format!("{}:{}", props.bind, props.port);
    let _port = reserved.release();
    let options = EmbeddedRuntimeOptions {
        topology: if listen {
            AuthorityTopology::ListenServer
        } else {
            AuthorityTopology::Dedicated
        },
        transport: TransportMode::Listen,
        local_session: listen.then(|| LocalSessionProfile::new(LOCAL_ID, "plan32-host")),
    };
    let (mut runtime, _) = ServerRuntime::new_embedded(props.clone(), options).unwrap();
    let mut clients = vec![
        TcpClient::connect(&address, "plan32-traveler"),
        TcpClient::connect(&address, "plan32-observer"),
    ];
    {
        let mut refs: Vec<_> = clients.iter_mut().collect();
        drive_until(
            &mut runtime,
            &mut refs,
            "Plan32 clients login",
            |runtime, views| {
                views.iter().all(|client| client.player_id().is_some())
                    && runtime.players.len() == if listen { 3 } else { 2 }
            },
        );
    }
    let owner = clients[0].player_id().unwrap();
    let observer = clients[1].player_id().unwrap();
    runtime.authority.with_world(Dimension::Overworld, |world| {
        world
            .set_block(
                PORTAL_CELL.0,
                PORTAL_CELL.1,
                PORTAL_CELL.2,
                BlockType::NetherPortal,
                0,
            )
            .unwrap();
    });
    prepare_player(
        &mut runtime,
        owner,
        [
            PORTAL_CELL.0 as f32,
            PORTAL_CELL.1 as f32,
            PORTAL_CELL.2 as f32,
        ],
        ItemStack::new(Item::Air, 0),
    );
    if let Some(session) = runtime.players.get_mut(&observer) {
        session.data.position = [8.0, 80.0, 8.0];
    }
    if let Some(session) = runtime.authority.session_mut(observer) {
        session.position = [8.0, 80.0, 8.0];
    }
    clients[0].clear_events();
    clients[1].clear_events();
    let enter = block_action(
        &runtime,
        owner,
        1,
        1,
        BlockActionKind::EnterPortal,
        PORTAL_CELL,
        None,
        BlockType::NetherPortal,
    );
    clients[0].send_request(enter.clone());
    let first = {
        let mut refs: Vec<_> = clients.iter_mut().collect();
        wait_for_response(&mut runtime, &mut refs, 0, 1)
    };
    assert!(matches!(first.outcome, GameplayOutcome::Accepted { .. }));
    let duplicate_before = runtime.metrics.duplicate_requests;
    clients[0].send_request(enter);
    {
        let mut refs: Vec<_> = clients.iter_mut().collect();
        drive_until(
            &mut runtime,
            &mut refs,
            "Plan32 cached portal duplicate and TCP transfer",
            |runtime, views| {
                runtime.metrics.duplicate_requests > duplicate_before
                    && runtime.players[&owner].dimension == Dimension::Nether
                    && views[0].events().iter().any(|event| {
                        matches!(
                            event,
                            ClientToGame::DimensionTransfer { dimension, .. }
                                if *dimension == Dimension::Nether as u8
                        )
                    })
            },
        );
    }
    assert_eq!(
        runtime
            .authority
            .session(owner)
            .and_then(|session| session.cached_response(1)),
        Some(first),
        "Plan32 duplicate must retain the byte-identical authority ACK"
    );
    assert!(
        clients[0].take_response(1).is_none(),
        "NetworkClient must not surface an already-observed portal ACK twice"
    );
    assert!(!clients[1]
        .events()
        .iter()
        .any(|event| matches!(event, ClientToGame::DimensionTransfer { .. })));
    assert!(!clients[1].events().iter().any(|event| matches!(
        event,
        ClientToGame::PlayerSessionUpdate { player_id, .. } if *player_id == owner
    )));
    let mut stale = request(
        &runtime,
        owner,
        2,
        2,
        GameplayOperation::Command {
            command: "/help".into(),
        },
    );
    assert!(stale.client_revision > 0, "Plan32 stale revision fixture");
    stale.client_revision -= 1;
    clients[0].send_request(stale);
    let stale_response = {
        let mut refs: Vec<_> = clients.iter_mut().collect();
        wait_for_response(&mut runtime, &mut refs, 0, 2)
    };
    assert_eq!(
        stale_response.outcome,
        GameplayOutcome::Rejected {
            reason: RejectReason::InvalidRevision
        }
    );
    clients[0].disconnect_and_join();
    {
        let mut refs = [&mut clients[1]];
        drive_until(
            &mut runtime,
            &mut refs,
            "Plan32 owner disconnect",
            |runtime, _| !runtime.players.contains_key(&owner),
        );
    }
    clients[0] = TcpClient::connect(&address, "plan32-traveler");
    {
        let mut refs: Vec<_> = clients.iter_mut().collect();
        drive_until(
            &mut runtime,
            &mut refs,
            "Plan32 owner reconnect",
            |runtime, views| {
                views[0].player_id().is_some_and(|id| {
                    runtime
                        .players
                        .get(&id)
                        .is_some_and(|session| session.dimension == Dimension::Nether)
                })
            },
        );
    }
    clients[0].disconnect_and_join();
    clients[1].disconnect_and_join();
    runtime.shutdown().unwrap();
    let _ = fs::remove_dir_all(props.world_dir);
}

#[test]
fn listen_tcp_typed_portal_travel_is_owner_private_and_persistent() {
    run_tcp_travel("listen", true);
}

#[test]
fn dedicated_tcp_typed_portal_travel_is_owner_private_and_persistent() {
    run_tcp_travel("dedicated", false);
}

#[test]
fn dedicated_tcp_combat_completes_generated_dragon_lifecycle() {
    let reserved = HeldLoopback::bind();
    let mut props = properties("dragon-combat");
    props.port = reserved.port();
    props.operators.insert("plan32-dragon".into());
    let address = format!("{}:{}", props.bind, props.port);
    let _port = reserved.release();
    let (mut runtime, _) = ServerRuntime::new_embedded(
        props.clone(),
        EmbeddedRuntimeOptions {
            topology: AuthorityTopology::Dedicated,
            transport: TransportMode::Listen,
            local_session: None,
        },
    )
    .unwrap();
    let mut client = TcpClient::connect(&address, "plan32-dragon");
    {
        let mut refs = [&mut client];
        drive_until(
            &mut runtime,
            &mut refs,
            "Plan32 dragon login",
            |_, views| views[0].player_id().is_some(),
        );
    }
    let owner = client.player_id().unwrap();
    runtime.authority.with_world(Dimension::Overworld, |world| {
        world
            .set_block(
                PORTAL_CELL.0,
                PORTAL_CELL.1,
                PORTAL_CELL.2,
                BlockType::EndPortal,
                0,
            )
            .unwrap();
    });
    prepare_player(
        &mut runtime,
        owner,
        [
            PORTAL_CELL.0 as f32,
            PORTAL_CELL.1 as f32,
            PORTAL_CELL.2 as f32,
        ],
        ItemStack::new(Item::Air, 0),
    );
    client.send_request(block_action(
        &runtime,
        owner,
        1,
        1,
        BlockActionKind::EnterPortal,
        PORTAL_CELL,
        None,
        BlockType::EndPortal,
    ));
    {
        let mut refs = [&mut client];
        assert!(matches!(
            wait_for_response(&mut runtime, &mut refs, 0, 1).outcome,
            GameplayOutcome::Accepted { .. }
        ));
        drive_until(
            &mut runtime,
            &mut refs,
            "Plan32 generated End dragon",
            |runtime, _| runtime.players[&owner].dimension == Dimension::End,
        );
    }
    runtime.tick().unwrap();
    assert!(runtime.authority.with_world(Dimension::End, |world| {
        world
            .entities
            .get_entities_by_type(EntityType::EnderDragon)
            .next()
            .is_some()
    }));
    let end_revision = runtime.authority.revision_for_dimension(Dimension::End);
    let session_revision = runtime.authority.session(owner).unwrap().last_revision;
    assert!(
        session_revision <= end_revision,
        "dimension transfer left session revision {session_revision} ahead of End revision {end_revision}"
    );
    // Let the bounded attack-cooldown projection settle before constructing a
    // new request revision; Plan33 separately covers an in-flight fixed-tick
    // lifecycle whose operation must tolerate its own progress revisions.
    for _ in 0..65 {
        runtime.tick().unwrap();
        client.drain();
    }
    client.send_request(request(
        &runtime,
        owner,
        2,
        2,
        GameplayOperation::Command {
            command: "/gamemode creative".into(),
        },
    ));
    std::thread::sleep(Duration::from_millis(50));
    {
        let mut refs = [&mut client];
        let response = wait_for_response(&mut runtime, &mut refs, 0, 2);
        assert!(
            matches!(response.outcome, GameplayOutcome::Accepted { .. }),
            "gamemode command was rejected: {response:?}"
        );
    }
    client.send_request(request(
        &runtime,
        owner,
        3,
        3,
        GameplayOperation::Command {
            command: "/give @s diamond_sword".into(),
        },
    ));
    std::thread::sleep(Duration::from_millis(50));
    {
        let mut refs = [&mut client];
        assert!(matches!(
            wait_for_response(&mut runtime, &mut refs, 0, 3).outcome,
            GameplayOutcome::Accepted { .. }
        ));
        drive_until(
            &mut runtime,
            &mut refs,
            "Plan32 command-authorized combat loadout",
            |runtime, _| {
                runtime.authority.session(owner).is_some_and(|session| {
                    session.game_mode == GameMode::Creative
                        && session
                            .gameplay
                            .inventory
                            .iter()
                            .flatten()
                            .any(|slot| slot.item.item == Item::DiamondSword.to_u32())
                })
            },
        );
    }

    let mut combat_sequence = 4u64;
    for attack in 0..40u128 {
        let Some((dragon_id, dragon_position, dragon_velocity)) =
            runtime.authority.with_world(Dimension::End, |world| {
                world
                    .entities
                    .get_entities_by_type(EntityType::EnderDragon)
                    .next()
                    .map(|dragon| {
                        (
                            dragon.id,
                            dragon.position.to_array(),
                            dragon.velocity.to_array(),
                        )
                    })
            })
        else {
            break;
        };
        let horizontal_velocity =
            glam::Vec2::new(dragon_velocity[0], dragon_velocity[2]).normalize_or_zero();
        let side = glam::Vec2::new(-horizontal_velocity.y, horizontal_velocity.x);
        let destination = [
            (dragon_position[0] + side.x * 2.0).round() as i32,
            dragon_position[1].round() as i32,
            (dragon_position[2] + side.y * 2.0).round() as i32,
        ];
        let teleport_request_id = 100 + attack;
        client.send_request(request(
            &runtime,
            owner,
            teleport_request_id,
            combat_sequence,
            GameplayOperation::Command {
                command: format!(
                    "/tp {} {} {}",
                    destination[0], destination[1], destination[2]
                ),
            },
        ));
        std::thread::sleep(Duration::from_millis(50));
        {
            let mut refs = [&mut client];
            assert!(matches!(
                wait_for_response(&mut runtime, &mut refs, 0, teleport_request_id).outcome,
                GameplayOutcome::Accepted { .. }
            ));
            drive_until(
                &mut runtime,
                &mut refs,
                "Plan32 dragon combat teleport",
                |runtime, _| {
                    runtime.authority.session(owner).is_some_and(|session| {
                        (session.position[0] - (destination[0] as f32 + 0.5)).abs() < 0.1
                            && (session.position[1] - destination[1] as f32).abs() < 0.1
                            && (session.position[2] - (destination[2] as f32 + 0.5)).abs() < 0.1
                    })
                },
            );
        }
        let attacker = runtime.authority.session(owner).unwrap().position;
        let current_dragon = runtime
            .authority
            .world_ref(Dimension::End)
            .unwrap()
            .entities
            .get_by_id(dragon_id)
            .expect("dragon remains alive before combat request")
            .position
            .to_array();
        let direction = (glam::Vec3::from_array(current_dragon) - glam::Vec3::from_array(attacker))
            .normalize_or_zero();
        let yaw = f32::atan2(-direction.x, direction.z).to_degrees();
        let pitch = (-direction.y.asin()).to_degrees();
        client.send_position(
            attack as u32 + 1,
            10_000 + attack as u64,
            attacker,
            yaw,
            pitch,
        );
        std::thread::sleep(Duration::from_millis(50));
        {
            let mut refs = [&mut client];
            drive_until(
                &mut runtime,
                &mut refs,
                "Plan32 dragon facing pose",
                |runtime, _| {
                    runtime.authority.session(owner).is_some_and(|session| {
                        (session.yaw - yaw).abs() < 0.1 && (session.pitch - pitch).abs() < 0.1
                    })
                },
            );
        }
        let attacker_session = runtime.authority.session(owner).unwrap();
        let dragon_now = runtime
            .authority
            .world_ref(Dimension::End)
            .unwrap()
            .entities
            .get_by_id(dragon_id)
            .unwrap();
        assert!(
            attacker_session.gameplay.attack_cooldown_ticks >= 5,
            "attack cooldown was not ready: {}",
            attacker_session.gameplay.attack_cooldown_ticks
        );
        assert!(
            dragon_now.invulnerable_time <= 0.0,
            "dragon remained invulnerable: {}",
            dragon_now.invulnerable_time
        );
        assert!(
            dragon_now.health > 0.0 && dragon_now.health <= dragon_now.max_health,
            "dragon health was invalid: {}/{}",
            dragon_now.health,
            dragon_now.max_health
        );
        let request_id = 1_000 + attack;
        client.send_request(request(
            &runtime,
            owner,
            request_id,
            combat_sequence + 1,
            GameplayOperation::Combat {
                target: dragon_id,
                action: 0,
            },
        ));
        std::thread::sleep(Duration::from_millis(50));
        {
            let mut refs = [&mut client];
            let response = wait_for_response(&mut runtime, &mut refs, 0, request_id);
            assert!(
                matches!(response.outcome, GameplayOutcome::Accepted { .. }),
                "dragon combat request was rejected: {response:?}"
            );
            for _ in 0..10 {
                runtime.tick().unwrap();
                refs[0].drain();
            }
        }
        combat_sequence += 2;
    }
    runtime.authority.with_world(Dimension::End, |world| {
        let surviving_dragon = world
            .entities
            .get_entities_by_type(EntityType::EnderDragon)
            .next();
        assert!(
            surviving_dragon.is_none(),
            "dragon survived forty authoritative sword hits: {:?}",
            surviving_dragon.map(|dragon| dragon.health)
        );
        assert_eq!(world.get_block(0, 78, 0), BlockType::DragonEgg);
        assert_eq!(world.get_block(8, 74, 0), BlockType::EndGateway);
        assert_eq!(world.get_block(0, 73, 0), BlockType::EndPortal);
        assert_eq!(world.get_block(1, 73, 0), BlockType::EndPortal);
    });
    client.disconnect_and_join();
    runtime.shutdown().unwrap();
    let _ = fs::remove_dir_all(props.world_dir);
}

#[test]
fn generated_end_city_loot_is_lazy_revisioned_and_persistent() {
    let props = properties("end-city");
    let (mut runtime, _) = ServerRuntime::new_embedded(
        props.clone(),
        EmbeddedRuntimeOptions {
            topology: AuthorityTopology::Dedicated,
            transport: TransportMode::Disabled,
            local_session: None,
        },
    )
    .unwrap();
    let fortress_origin = icraft::structure::locate_structure(
        StructureId::NetherFortress,
        (0, 64, 0),
        runtime.level.seed,
        Dimension::Nether,
    )
    .expect("bounded Nether fortress locator result");
    let fortress_chest = (
        fortress_origin.0 + 3,
        fortress_origin.1 + 1,
        fortress_origin.2 + 2,
    );
    runtime.authority.with_world(Dimension::Nether, |world| {
        world.ensure_chunk(
            fortress_chest.0.div_euclid(16),
            fortress_chest.2.div_euclid(16),
        );
        assert!(matches!(
            world.get_block_entity(fortress_chest.0, fortress_chest.1, fortress_chest.2),
            Some(BlockEntity::Chest(chest))
                if chest.loot_table.as_deref() == Some("chests/nether_bridge")
        ));
        let before = world.revisions.current();
        assert!(world
            .container_slots_wire(fortress_chest)
            .is_some_and(|slots| slots.iter().flatten().next().is_some()));
        assert!(world.revisions.current() > before);
    });
    let chest_pos = (1035, 89, 11);
    runtime.authority.with_world(Dimension::End, |world| {
        world.ensure_chunk(64, 0);
        assert_eq!(
            world.get_block(chest_pos.0, chest_pos.1, chest_pos.2),
            BlockType::Chest
        );
        assert!(matches!(
            world.get_block_entity(chest_pos.0, chest_pos.1, chest_pos.2),
            Some(BlockEntity::Chest(chest)) if chest.loot_table.is_some()
        ));
        let before = world.revisions.current();
        let slots = world.container_slots_wire(chest_pos).unwrap();
        assert!(slots
            .iter()
            .flatten()
            .any(|item| item.item == Item::Elytra as u32));
        assert!(world.revisions.current() > before);
        assert!(matches!(
            world.get_block_entity(chest_pos.0, chest_pos.1, chest_pos.2),
            Some(BlockEntity::Chest(chest)) if chest.loot_table.is_none()
        ));
    });
    runtime.shutdown().unwrap();
    let (mut restored, _) = ServerRuntime::new_embedded(
        props.clone(),
        EmbeddedRuntimeOptions {
            topology: AuthorityTopology::Dedicated,
            transport: TransportMode::Disabled,
            local_session: None,
        },
    )
    .unwrap();
    restored.authority.with_world(Dimension::Nether, |world| {
        assert!(matches!(
            world.get_block_entity(fortress_chest.0, fortress_chest.1, fortress_chest.2),
            Some(BlockEntity::Chest(chest))
                if chest.loot_table.is_none()
                    && chest.inventory.slots.iter().flatten().next().is_some()
        ));
    });
    restored.authority.with_world(Dimension::End, |world| {
        assert!(matches!(
            world.get_block_entity(chest_pos.0, chest_pos.1, chest_pos.2),
            Some(BlockEntity::Chest(chest))
                if chest.loot_table.is_none()
                    && chest.inventory.slots.iter().flatten().any(|stack| stack.item == Item::Elytra)
        ));
    });
    restored.shutdown().unwrap();
    let _ = fs::remove_dir_all(props.world_dir);
}
