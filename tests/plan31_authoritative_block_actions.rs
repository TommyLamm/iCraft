mod common;

use common::tcp_harness::{drive_until, wait_for_response, TcpClient};
use icraft::authority::contract::{AuthorityTopology, SessionGameplayState, SessionInventorySlot};
use icraft::dimension::Dimension;
use icraft::inventory::{Item, ItemStack};
use icraft::network::client::ClientToGame;
use icraft::network::protocol::{
    BlockActionKind, GameplayOperation, GameplayOutcome, GameplayRequest, SessionSlotWire,
};
use icraft::server_runtime::{
    EmbeddedRuntimeOptions, LocalSessionProfile, RuntimePresentationEvent, RuntimeTickOutput,
    ServerProperties, ServerRuntime, TransportMode,
};
use icraft::world::BlockType;
use std::fs;
use std::net::TcpListener;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const OWNER_ID: u64 = 0x31_0000;
const OBSERVER_ID: u64 = OWNER_ID + 1;
const TARGET: (i32, i32, i32) = (8, 81, 9);
const PLACE_TARGET: (i32, i32, i32) = (8, 81, 10);
const PLACE_SUPPORT: (i32, i32, i32) = (8, 80, 10);
const OWNER_POSITION: [f32; 3] = [8.0, 80.0, 8.0];
const OBSERVER_POSITION: [f32; 3] = [8.0, 80.0, 7.0];
const LOOK: [i16; 3] = [0, -100, 995];
const PLACE_LOOK: [i16; 3] = [180, -403, 899];

fn temp_world(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!("icraft-plan31-{label}-{nonce}"))
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
        seed: 0x31_31_31_31,
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

fn held(stack: &ItemStack) -> SessionSlotWire {
    SessionSlotWire::new(
        icraft::network::protocol::ItemWire::from_stack(stack),
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

fn start_request_at(
    runtime: &ServerRuntime,
    player_id: u64,
    request_id: u128,
    sequence: u64,
    position: (i32, i32, i32),
    wire: SessionSlotWire,
) -> GameplayRequest {
    request(
        runtime,
        player_id,
        request_id,
        sequence,
        GameplayOperation::BlockAction {
            action: BlockActionKind::StartBreak,
            x: position.0,
            y: position.1,
            z: position.2,
            face: [0, 0, -1],
            hand: 0,
            held: Some(wire),
            block: BlockType::Air.to_wire(),
            look_milli: LOOK,
        },
    )
}

fn start_request(
    runtime: &ServerRuntime,
    player_id: u64,
    request_id: u128,
    sequence: u64,
    wire: SessionSlotWire,
) -> GameplayRequest {
    start_request_at(runtime, player_id, request_id, sequence, TARGET, wire)
}

fn place_request(
    runtime: &ServerRuntime,
    player_id: u64,
    request_id: u128,
    sequence: u64,
    position: (i32, i32, i32),
    wire: SessionSlotWire,
    block: BlockType,
) -> GameplayRequest {
    request(
        runtime,
        player_id,
        request_id,
        sequence,
        GameplayOperation::BlockAction {
            action: BlockActionKind::Place,
            x: position.0,
            y: position.1,
            z: position.2,
            face: [0, 1, 0],
            hand: 0,
            held: Some(wire),
            block: block.to_wire(),
            look_milli: PLACE_LOOK,
        },
    )
}

fn cancel_request(
    runtime: &ServerRuntime,
    player_id: u64,
    request_id: u128,
    sequence: u64,
) -> GameplayRequest {
    request(
        runtime,
        player_id,
        request_id,
        sequence,
        GameplayOperation::BlockAction {
            action: BlockActionKind::CancelBreak,
            x: TARGET.0,
            y: TARGET.1,
            z: TARGET.2,
            face: [0, 0, 0],
            hand: 0,
            held: None,
            block: BlockType::Air.to_wire(),
            look_milli: LOOK,
        },
    )
}

fn prepare(runtime: &mut ServerRuntime, owner: u64, observer: u64, target_block: BlockType) {
    runtime
        .authority
        .world
        .set_block(TARGET.0, TARGET.1, TARGET.2, target_block, 0)
        .expect("seed mining target");
    runtime
        .authority
        .world
        .set_block(
            PLACE_SUPPORT.0,
            PLACE_SUPPORT.1,
            PLACE_SUPPORT.2,
            BlockType::Stone,
            0,
        )
        .expect("seed placement support");
    let mut owner_gameplay = SessionGameplayState::default();
    let pick = ItemStack::new(Item::StonePickaxe, 1);
    owner_gameplay.inventory[0] = Some(slot(pick));
    runtime
        .authority
        .set_session_gameplay(owner, owner_gameplay);
    runtime
        .authority
        .set_session_gameplay(observer, SessionGameplayState::default());
    for id in [owner, observer] {
        let position = if id == owner {
            OWNER_POSITION
        } else {
            OBSERVER_POSITION
        };
        if let Some(player) = runtime.players.get_mut(&id) {
            player.data.position = position;
            player.data.yaw = 0.0;
            player.data.pitch = 0.0;
        }
        if let Some(session) = runtime.authority.session_mut(id) {
            session.position = position;
            session.yaw = 0.0;
            session.pitch = 0.0;
        }
    }
}

fn embedded_response(
    output: &RuntimeTickOutput,
    target: u64,
    request_id: u128,
) -> icraft::network::protocol::GameplayResponse {
    output
        .presentation_events
        .iter()
        .find_map(|event| match event {
            RuntimePresentationEvent::GameplayResponse {
                target: event_target,
                response,
            } if *event_target == target && response.request_id == request_id => {
                Some(response.clone())
            }
            _ => None,
        })
        .expect("embedded typed gameplay response")
}

fn run_embedded_vector() {
    let properties = properties("embedded", reserve_port());
    let world_dir = properties.world_dir.clone();
    let (mut runtime, input) = ServerRuntime::new_embedded(
        properties,
        EmbeddedRuntimeOptions {
            topology: AuthorityTopology::Singleplayer,
            transport: TransportMode::Disabled,
            local_session: Some(LocalSessionProfile::new(OWNER_ID, "plan31-owner")),
        },
    )
    .expect("construct embedded Plan31 runtime");
    runtime
        .login_session(OBSERVER_ID, "plan31-observer")
        .expect("login embedded observer");
    let _ = runtime.tick_with_output().expect("embedded login tick");
    prepare(&mut runtime, OWNER_ID, OBSERVER_ID, BlockType::Stone);
    let pick = ItemStack::new(Item::StonePickaxe, 1);
    let wire = held(&pick);

    input
        .submit_request(OWNER_ID, start_request(&runtime, OWNER_ID, 1, 1, wire))
        .expect("embedded start ingress");
    let start_output = runtime.tick_with_output().expect("embedded start tick");
    assert!(matches!(
        embedded_response(&start_output, OWNER_ID, 1).outcome,
        GameplayOutcome::Accepted { .. }
    ));
    for _ in 0..5 {
        let _ = runtime.tick_with_output().expect("embedded progress tick");
    }
    input
        .submit_request(OWNER_ID, cancel_request(&runtime, OWNER_ID, 2, 2))
        .expect("embedded cancel ingress");
    let cancel_output = runtime.tick_with_output().expect("embedded cancel tick");
    assert!(matches!(
        embedded_response(&cancel_output, OWNER_ID, 2).outcome,
        GameplayOutcome::Accepted { .. }
    ));
    assert_eq!(
        runtime
            .authority
            .world
            .get_block(TARGET.0, TARGET.1, TARGET.2),
        BlockType::Stone
    );

    input
        .submit_request(OWNER_ID, start_request(&runtime, OWNER_ID, 3, 3, wire))
        .expect("embedded second start ingress");
    let _ = runtime
        .tick_with_output()
        .expect("embedded second start tick");
    let mut events = Vec::new();
    for _ in 0..80 {
        let output = runtime.tick_with_output().expect("embedded mining tick");
        events.extend(output.presentation_events);
        if runtime
            .authority
            .world
            .get_block(TARGET.0, TARGET.1, TARGET.2)
            == BlockType::Air
        {
            break;
        }
    }
    assert_eq!(
        runtime
            .authority
            .world
            .get_block(TARGET.0, TARGET.1, TARGET.2),
        BlockType::Air
    );
    assert!(events.iter().any(|event| {
        matches!(
            event,
            RuntimePresentationEvent::BlockChange {
                target: event_target,
                x,
                y,
                z,
                block,
                ..
            } if *event_target == OWNER_ID
                && (*x, *y, *z) == TARGET
                && *block == BlockType::Air.to_wire()
        )
    }));
    assert!(runtime
        .drain_routed_updates()
        .iter()
        .any(|update| update.target == OBSERVER_ID));
    runtime
        .shutdown()
        .expect("shutdown embedded Plan31 runtime");
    let _ = fs::remove_dir_all(world_dir);
}

fn run_tcp_vector(label: &str, listen: bool) {
    let properties = properties(label, reserve_port());
    let address = format!("{}:{}", properties.bind, properties.port);
    let (mut runtime, local_host) = if listen {
        let (runtime, _input) = ServerRuntime::new_embedded(
            properties.clone(),
            EmbeddedRuntimeOptions {
                topology: AuthorityTopology::ListenServer,
                transport: TransportMode::Listen,
                local_session: Some(LocalSessionProfile::new(OWNER_ID, "plan31-host")),
            },
        )
        .expect("construct listen Plan31 runtime");
        (runtime, true)
    } else {
        (
            ServerRuntime::new(properties.clone()).expect("construct dedicated Plan31 runtime"),
            false,
        )
    };
    let owner = TcpClient::connect(&address, "plan31-owner");
    let observer = TcpClient::connect(&address, "plan31-observer");
    let mut clients = vec![owner, observer];
    {
        let mut refs: Vec<&mut TcpClient> = clients.iter_mut().collect();
        drive_until(
            &mut runtime,
            &mut refs,
            "Plan31 TCP clients authenticated",
            |runtime, views| {
                views.iter().all(|client| client.player_id().is_some())
                    && runtime.players.len() == if local_host { 3 } else { 2 }
            },
        );
        for client in refs.iter_mut() {
            client.clear_events();
        }
    }
    let owner_id = clients[0].player_id().expect("TCP owner id");
    let observer_id = clients[1].player_id().expect("TCP observer id");
    // Use a deliberately slow first target so real socket scheduling cannot
    // finish the break before the bounded cancel/stale sequence assertions.
    prepare(&mut runtime, owner_id, observer_id, BlockType::Obsidian);
    let pick = ItemStack::new(Item::StonePickaxe, 1);
    let wire = held(&pick);
    let start = start_request(&runtime, owner_id, 1, 1, wire);
    clients[0].send_request(start.clone());
    let first = {
        let mut refs: Vec<&mut TcpClient> = clients.iter_mut().collect();
        wait_for_response(&mut runtime, &mut refs, 0, 1)
    };
    assert!(matches!(first.outcome, GameplayOutcome::Accepted { .. }));
    clients[0].clear_events();
    clients[1].clear_events();
    // Request-id caching is part of the TCP ingress contract: replaying
    // the same StartBreak cannot reset or duplicate the owner session.
    clients[0].send_request(start);
    {
        let mut refs: Vec<&mut TcpClient> = clients.iter_mut().collect();
        let duplicate = wait_for_response(&mut runtime, &mut refs, 0, 1);
        assert_eq!(duplicate, first);
    }
    for _ in 0..5 {
        runtime.tick().expect("TCP progress tick");
        for client in &mut clients {
            client.drain();
        }
    }
    assert!(clients[0].events().iter().any(|event| {
        matches!(
            event,
            ClientToGame::PlayerSessionUpdate { player_id, state, .. }
                if *player_id == owner_id && state.mining.is_some()
        )
    }));
    assert!(!clients[1].events().iter().any(|event| {
        matches!(
            event,
            ClientToGame::PlayerSessionUpdate { player_id, .. }
                if *player_id == owner_id
        )
    }));
    // Out-of-order and stale requests traverse the real NetworkClient socket
    // and must not clear the latched mining session.
    clients[0].send_request(cancel_request(&runtime, owner_id, 4, 1));
    {
        let mut refs: Vec<&mut TcpClient> = clients.iter_mut().collect();
        let response = wait_for_response(&mut runtime, &mut refs, 0, 4);
        assert!(matches!(
            response.outcome,
            GameplayOutcome::Rejected {
                reason: icraft::network::protocol::RejectReason::OutOfOrder
            }
        ));
    }
    let mut stale = cancel_request(&runtime, owner_id, 5, 2);
    stale.client_revision = 0;
    clients[0].send_request(stale);
    {
        let mut refs: Vec<&mut TcpClient> = clients.iter_mut().collect();
        let response = wait_for_response(&mut runtime, &mut refs, 0, 5);
        assert!(matches!(
            response.outcome,
            GameplayOutcome::Rejected {
                reason: icraft::network::protocol::RejectReason::InvalidRevision
            }
        ));
    }
    clients[0].send_request(cancel_request(&runtime, owner_id, 2, 2));
    {
        let mut refs: Vec<&mut TcpClient> = clients.iter_mut().collect();
        let response = wait_for_response(&mut runtime, &mut refs, 0, 2);
        assert!(
            matches!(response.outcome, GameplayOutcome::Accepted { .. }),
            "cancel response: {response:?}"
        );
    }
    assert_eq!(
        runtime
            .authority
            .world
            .get_block(TARGET.0, TARGET.1, TARGET.2),
        BlockType::Obsidian
    );

    runtime
        .authority
        .world
        .set_block(TARGET.0, TARGET.1, TARGET.2, BlockType::CoalOre, 0)
        .expect("seed bounded XP mining target after cancel");

    clients[0].clear_events();
    clients[1].clear_events();
    clients[0].send_request(start_request(&runtime, owner_id, 3, 3, wire));
    {
        let mut refs: Vec<&mut TcpClient> = clients.iter_mut().collect();
        let response = wait_for_response(&mut runtime, &mut refs, 0, 3);
        assert!(
            matches!(response.outcome, GameplayOutcome::Accepted { .. }),
            "second start response: {response:?}"
        );
    }
    {
        let mut refs: Vec<&mut TcpClient> = clients.iter_mut().collect();
        drive_until(
            &mut runtime,
            &mut refs,
            "Plan31 TCP authoritative break",
            |runtime, views| {
                runtime
                    .authority
                    .world
                    .get_block(TARGET.0, TARGET.1, TARGET.2)
                    == BlockType::Air
                    && views.iter().all(|client| {
                        client.events().iter().any(|event| {
                            matches!(
                                event,
                                ClientToGame::BlockChange { x, y, z, block, .. }
                                    if (*x, *y, *z) == TARGET
                                        && *block == BlockType::Air.to_wire()
                            )
                        })
                    })
                    && views.iter().all(|client| {
                        client.events().iter().any(|event| {
                            matches!(
                                event,
                                ClientToGame::EntitySpawn { state, .. }
                                    | ClientToGame::EntityState { state, .. }
                                    if state.item.is_some()
                            )
                        })
                    })
                    && views[0].events().iter().any(|event| {
                        matches!(
                            event,
                            ClientToGame::PlayerSessionUpdate { player_id, state, .. }
                                if *player_id == owner_id && state.experience >= 2
                        )
                    })
            },
        );
    }
    for client in &mut clients {
        client.drain();
        assert!(client.events().iter().any(|event| {
            matches!(
                event,
                ClientToGame::BlockChange { x, y, z, block, .. }
                    if (*x, *y, *z) == TARGET && *block == BlockType::Air.to_wire()
            )
        }));
    }
    assert!(clients.iter().all(|client| {
        client.events().iter().any(|event| {
            matches!(
                event,
                ClientToGame::EntitySpawn { state, .. } | ClientToGame::EntityState { state, .. }
                    if state.item.is_some()
            )
        })
    }));

    // Place a chest through the same TCP typed ingress, then break it again.
    // The two observers must converge on both BE creation and removal.
    let chest = ItemStack::new(Item::Chest, 1);
    let chest_wire = held(&chest);
    let mut chest_gameplay = runtime
        .authority
        .session(owner_id)
        .expect("owner session after coal break")
        .gameplay;
    chest_gameplay.inventory[0] = Some(slot(chest));
    chest_gameplay.mining = None;
    assert!(runtime
        .authority
        .set_session_gameplay(owner_id, chest_gameplay));
    clients[0].clear_events();
    clients[1].clear_events();
    clients[0].send_request(place_request(
        &runtime,
        owner_id,
        6,
        4,
        PLACE_TARGET,
        chest_wire,
        BlockType::Chest,
    ));
    {
        let mut refs: Vec<&mut TcpClient> = clients.iter_mut().collect();
        let response = wait_for_response(&mut runtime, &mut refs, 0, 6);
        assert!(
            matches!(response.outcome, GameplayOutcome::Accepted { .. }),
            "place response: {response:?}; authority_revision={}; session_revision={}",
            runtime.authority.current_revision(),
            runtime.authority.session(owner_id).unwrap().last_revision,
        );
    }
    {
        let mut refs: Vec<&mut TcpClient> = clients.iter_mut().collect();
        drive_until(
            &mut runtime,
            &mut refs,
            "Plan31 TCP chest place projection",
            |runtime, views| {
                runtime
                    .authority
                    .world
                    .get_block(PLACE_TARGET.0, PLACE_TARGET.1, PLACE_TARGET.2)
                    == BlockType::Chest
                    && views.iter().all(|client| {
                        client.events().iter().any(|event| {
                            matches!(
                                event,
                                ClientToGame::BlockChange { x, y, z, block, .. }
                                    if (*x, *y, *z) == PLACE_TARGET
                                        && *block == BlockType::Chest.to_wire()
                            )
                        })
                    })
                    && views.iter().all(|client| {
                        client.events().iter().any(|event| {
                            matches!(
                                event,
                                ClientToGame::BlockEntityDelta { x, y, z, entity, .. }
                                    if (*x, *y, *z) == PLACE_TARGET && entity.is_some()
                            )
                        })
                    })
            },
        );
    }
    let mut pick_gameplay = runtime
        .authority
        .session(owner_id)
        .expect("owner session after chest place")
        .gameplay;
    pick_gameplay.inventory[0] = Some(slot(pick.clone()));
    pick_gameplay.mining = None;
    assert!(runtime
        .authority
        .set_session_gameplay(owner_id, pick_gameplay));
    clients[0].clear_events();
    clients[1].clear_events();
    clients[0].send_request(start_request_at(
        &runtime,
        owner_id,
        7,
        5,
        PLACE_TARGET,
        wire,
    ));
    {
        let mut refs: Vec<&mut TcpClient> = clients.iter_mut().collect();
        let response = wait_for_response(&mut runtime, &mut refs, 0, 7);
        assert!(matches!(response.outcome, GameplayOutcome::Accepted { .. }));
    }
    {
        let mut refs: Vec<&mut TcpClient> = clients.iter_mut().collect();
        drive_until(
            &mut runtime,
            &mut refs,
            "Plan31 TCP chest break projection",
            |runtime, views| {
                runtime
                    .authority
                    .world
                    .get_block(PLACE_TARGET.0, PLACE_TARGET.1, PLACE_TARGET.2)
                    == BlockType::Air
                    && views.iter().all(|client| {
                        client.events().iter().any(|event| {
                            matches!(
                                event,
                                ClientToGame::BlockChange { x, y, z, block, .. }
                                    if (*x, *y, *z) == PLACE_TARGET
                                        && *block == BlockType::Air.to_wire()
                            )
                        })
                    })
                    && views.iter().all(|client| {
                        client.events().iter().any(|event| {
                            matches!(
                                event,
                                ClientToGame::BlockEntityDelta { x, y, z, entity, .. }
                                    if (*x, *y, *z) == PLACE_TARGET && entity.is_none()
                            )
                        })
                    })
            },
        );
    }
    clients[0].disconnect_and_join();
    clients[1].disconnect_and_join();
    runtime.shutdown().expect("shutdown TCP Plan31 runtime");
    let (mut restored, _) = ServerRuntime::new_embedded(
        properties.clone(),
        EmbeddedRuntimeOptions {
            topology: AuthorityTopology::Dedicated,
            transport: TransportMode::Disabled,
            local_session: None,
        },
    )
    .expect("reload saved Plan31 world");
    assert_eq!(
        restored
            .authority
            .world
            .get_block(TARGET.0, TARGET.1, TARGET.2),
        BlockType::Air
    );
    assert_eq!(
        restored
            .authority
            .world
            .get_block(PLACE_TARGET.0, PLACE_TARGET.1, PLACE_TARGET.2),
        BlockType::Air
    );
    restored.shutdown().expect("shutdown reloaded Plan31 world");
    let _ = fs::remove_dir_all(properties.world_dir);
}

#[test]
fn plan31_embedded_typed_block_action_projection() {
    run_embedded_vector();
}

#[test]
fn plan31_listen_tcp_typed_block_action_projection() {
    run_tcp_vector("listen", true);
}

#[test]
fn plan31_dedicated_tcp_typed_block_action_projection() {
    run_tcp_vector("dedicated", false);
}
