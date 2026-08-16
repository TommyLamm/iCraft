mod common;

use common::tcp_harness::{
    drive_until, gameplay_request as request, loopback_properties, session_slot, temp_world,
    wait_for_response, HeldLoopback, TcpClient,
};
use icraft::authority::contract::{AuthorityTopology, SessionGameplayState};
use icraft::authority::transactions::BREW_TICKS;
use icraft::block_entity::{BlockEntity, FurnaceBlockEntity};
use icraft::dimension::Dimension;
use icraft::inventory::{Item, ItemStack};
use icraft::network::client::{ClientToGame, GameToClient};
use icraft::network::protocol::{
    GameplayOperation, GameplayOutcome, GameplayRequest, GameplayResponse, RejectReason,
    SlotRefWire,
};
use icraft::network::server::ServerToHost;
use icraft::server_runtime::{
    EmbeddedRuntimeOptions, LocalSessionProfile, RuntimeInput, RuntimePresentationEvent,
    RuntimeTickOutput, ServerProperties, ServerRuntime, TransportMode,
};
use icraft::world::BlockType;
use std::fs;

const HOST_SESSION_ID: u64 = 0xD30_000;
const EMBEDDED_VICTIM_ID: u64 = HOST_SESSION_ID + 1;
const POSITION: [f32; 3] = [8.0, 80.0, 8.0];
const VICTIM_POSITION: [f32; 3] = [8.0, 80.0, 9.0];

fn properties(label: &str) -> ServerProperties {
    let mut properties = loopback_properties(temp_world(&format!("plan30-{label}")), "127.0.0.1");
    properties.seed = 0x30_30_30_30;
    properties
}

fn source(state: SessionGameplayState, index: u8, count: u16) -> SlotRefWire {
    SlotRefWire {
        index,
        count,
        expected: state.inventory[usize::from(index)]
            .expect("fixture source slot")
            .into(),
    }
}

fn current_revision(runtime: &ServerRuntime, id: u64) -> u64 {
    let dimension = runtime
        .authority
        .session(id)
        .and_then(|session| Dimension::from_wire(session.dimension))
        .expect("fixture session dimension");
    runtime.authority.revision_for_dimension(dimension)
}

/// Reset long-lived domain reservations between independent wire lanes.  This
/// is fixture teardown, not a gameplay assertion: the cast/duplicate lane is
/// already asserted through TCP, while the following workstation/brew lane
/// starts from a clean authority state so fixed ticks cannot turn a request
/// revision into a race.
fn reset_persistent_domains(runtime: &mut ServerRuntime, id: u64) {
    let hook = runtime
        .authority
        .session(id)
        .and_then(|session| session.gameplay.fishing_hook)
        .map(|hook| hook.entity_id);
    if let Some(hook) = hook {
        runtime.authority.world_mut_active().remove_authority_entity(hook);
    }
    let revision = current_revision(runtime, id);
    if let Some(session) = runtime.authority.session_mut(id) {
        session.gameplay.fishing_hook = None;
        session.gameplay.brew = None;
        session.gameplay.revision = revision;
        session.last_revision = revision;
    }
}

fn prepare_fixture(runtime: &mut ServerRuntime, owner: u64, victim: u64) {
    let furnace_position = (8, 80, 9);
    let brew_position = (8, 80, 10);
    let enchanting_position = (8, 80, 11);
    let anvil_position = (8, 80, 12);
    runtime
        .authority
        .world_mut_active()
        .set_block(
            furnace_position.0,
            furnace_position.1,
            furnace_position.2,
            BlockType::Furnace,
            0,
        )
        .expect("fixture furnace block");
    runtime
        .authority
        .world_mut_active()
        .set_block(
            brew_position.0,
            brew_position.1,
            brew_position.2,
            BlockType::BrewingStand,
            0,
        )
        .expect("fixture brewing stand");
    for (position, block) in [
        (enchanting_position, BlockType::EnchantingTable),
        (anvil_position, BlockType::Anvil),
    ] {
        runtime
            .authority
            .world_mut_active()
            .set_block(position.0, position.1, position.2, block, 0)
            .expect("fixture workstation block");
    }
    let mut furnace = FurnaceBlockEntity::new();
    furnace.slots[2] = Some(ItemStack::new(Item::IronIngot, 2));
    furnace.accumulated_xp = 4.0;
    runtime.authority.world_mut_active().chunks.set_block_entity(
        furnace_position.0,
        furnace_position.1,
        furnace_position.2,
        Some(BlockEntity::Furnace(furnace)),
    );

    for id in [owner, victim] {
        let player = runtime
            .players
            .get_mut(&id)
            .expect("fixture player runtime state");
        player.data.position = if id == owner {
            POSITION
        } else {
            VICTIM_POSITION
        };
        player.data.yaw = 0.0;
        player.data.pitch = 0.0;
        let session = runtime
            .authority
            .session_mut(id)
            .expect("fixture authority session");
        session.position = if id == owner {
            POSITION
        } else {
            VICTIM_POSITION
        };
        session.yaw = 0.0;
        session.pitch = 0.0;
        let mut gameplay = session.gameplay;
        // Keep the fixed-tick session projection stable while a socket request
        // is in flight; five is the authority's ready sentinel.
        gameplay.attack_cooldown_ticks = 5;
        gameplay.experience_level = 30;
        gameplay.enchant_seed = 42;
        if id == owner {
            gameplay.inventory[0] = Some(session_slot(ItemStack::new(Item::DiamondSword, 1)));
            gameplay.inventory[1] = Some(session_slot(ItemStack::new(Item::OakPlanks, 2)));
            gameplay.inventory[2] = Some(session_slot(ItemStack::new(Item::Potion, 1)));
            gameplay.inventory[3] = Some(session_slot(ItemStack::new(Item::IronPickaxe, 1)));
            gameplay.inventory[4] = Some(session_slot(ItemStack::new(Item::LapisLazuli, 3)));
            gameplay.inventory[5] = Some(session_slot(ItemStack::new(Item::NetherWart, 1)));
            gameplay.inventory[40] = Some(session_slot(ItemStack::new(Item::FishingRod, 1)));
            gameplay.selected_hotbar_slot = 0;
        } else {
            gameplay.health_milli = 1_000;
            gameplay.inventory[0] = Some(session_slot(ItemStack::new(Item::Diamond, 1)));
        }
        assert!(runtime.authority.set_session_gameplay(id, gameplay));
    }
}

fn drain_clients(clients: &mut [&mut TcpClient]) {
    for client in clients.iter_mut() {
        client.drain();
    }
}

fn embedded_response(
    output: &RuntimeTickOutput,
    target: u64,
    request_id: u128,
) -> GameplayResponse {
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
        .unwrap_or_else(|| panic!("missing embedded GameplayResponse {request_id}"))
}

fn embedded_owner_projection(output: &RuntimeTickOutput, target: u64) -> bool {
    output.presentation_events.iter().any(|event| {
        matches!(
            event,
            RuntimePresentationEvent::PlayerSessionUpdate {
                target: event_target,
                player_id,
                ..
            } if *event_target == target && *player_id == target
        )
    })
}

fn embedded_submit(
    runtime: &mut ServerRuntime,
    input: &RuntimeInput,
    session_id: u64,
    request_id: u128,
    sequence: u64,
    operation: GameplayOperation,
) -> (GameplayRequest, RuntimeTickOutput) {
    let request = request(runtime, session_id, request_id, sequence, operation);
    input
        .submit_request(session_id, request.clone())
        .expect("embedded RuntimeInput accepts request");
    let output = runtime
        .tick_with_output()
        .expect("embedded runtime fixed tick");
    (request, output)
}

fn run_singleplayer_embedded_contract() {
    let properties = properties("singleplayer");
    let world_dir = properties.world_dir.clone();
    let (mut runtime, input) = ServerRuntime::new_embedded(
        properties,
        EmbeddedRuntimeOptions {
            topology: AuthorityTopology::Singleplayer,
            transport: TransportMode::Disabled,
            local_session: Some(LocalSessionProfile::new(HOST_SESSION_ID, "embedded-owner")),
        },
    )
    .expect("construct embedded singleplayer runtime");
    runtime
        .login_session(EMBEDDED_VICTIM_ID, "embedded-victim")
        .expect("embedded fixture victim login");
    let _ = runtime
        .tick_with_output()
        .expect("drain embedded login projection");
    prepare_fixture(&mut runtime, HOST_SESSION_ID, EMBEDDED_VICTIM_ID);

    let (cast, cast_output) = embedded_submit(
        &mut runtime,
        &input,
        HOST_SESSION_ID,
        0x40_001,
        1,
        GameplayOperation::Fishing {
            action: 0,
            hand: 1,
            look_milli: [0, 0, 1_000],
        },
    );
    let cast_response = embedded_response(&cast_output, HOST_SESSION_ID, cast.request_id);
    assert!(matches!(
        cast_response.outcome,
        GameplayOutcome::Accepted { .. }
    ));
    assert!(embedded_owner_projection(&cast_output, HOST_SESSION_ID));
    let (_, duplicate_output) = embedded_submit(
        &mut runtime,
        &input,
        HOST_SESSION_ID,
        cast.request_id,
        cast.client_sequence,
        cast.operation.clone(),
    );
    assert_eq!(
        embedded_response(&duplicate_output, HOST_SESSION_ID, cast.request_id),
        cast_response,
        "embedded duplicate reuses the cached ACK"
    );
    let _ = runtime
        .tick_with_output()
        .expect("embedded fishing fixed tick");
    let (_, reel_output) = embedded_submit(
        &mut runtime,
        &input,
        HOST_SESSION_ID,
        0x40_002,
        2,
        GameplayOperation::Fishing {
            action: 1,
            hand: 1,
            look_milli: [0, 0, 1_000],
        },
    );
    assert!(matches!(
        embedded_response(&reel_output, HOST_SESSION_ID, 0x40_002).outcome,
        GameplayOutcome::Accepted { .. }
    ));

    let (_, furnace_output) = embedded_submit(
        &mut runtime,
        &input,
        HOST_SESSION_ID,
        0x40_003,
        3,
        GameplayOperation::FurnaceTakeOutput {
            x: 8,
            y: 80,
            z: 9,
            count: 1,
        },
    );
    assert!(matches!(
        embedded_response(&furnace_output, HOST_SESSION_ID, 0x40_003).outcome,
        GameplayOutcome::Accepted { .. }
    ));

    let state = runtime
        .authority
        .session(HOST_SESSION_ID)
        .expect("embedded owner state")
        .gameplay;
    let plank = source(state, 1, 1);
    let mut sources = [None; 9];
    sources[0] = Some(plank);
    sources[2] = Some(plank);
    let (_, craft_output) = embedded_submit(
        &mut runtime,
        &input,
        HOST_SESSION_ID,
        0x40_004,
        4,
        GameplayOperation::Craft {
            grid: 2,
            sources,
            station: None,
        },
    );
    assert!(matches!(
        embedded_response(&craft_output, HOST_SESSION_ID, 0x40_004).outcome,
        GameplayOutcome::Accepted { .. }
    ));
    assert!(embedded_owner_projection(&craft_output, HOST_SESSION_ID));

    let state = runtime
        .authority
        .session(HOST_SESSION_ID)
        .expect("embedded owner state after craft")
        .gameplay;
    let (_, enchant_output) = embedded_submit(
        &mut runtime,
        &input,
        HOST_SESSION_ID,
        0x40_005,
        5,
        GameplayOperation::Enchant {
            x: 8,
            y: 80,
            z: 11,
            source: source(state, 3, 1),
            option: 2,
        },
    );
    assert!(matches!(
        embedded_response(&enchant_output, HOST_SESSION_ID, 0x40_005).outcome,
        GameplayOutcome::Accepted { .. }
    ));

    let state = runtime
        .authority
        .session(HOST_SESSION_ID)
        .expect("embedded owner state after enchant")
        .gameplay;
    let (_, anvil_output) = embedded_submit(
        &mut runtime,
        &input,
        HOST_SESSION_ID,
        0x40_006,
        6,
        GameplayOperation::Anvil {
            x: 8,
            y: 80,
            z: 12,
            left: source(state, 3, 1),
            right: None,
            rename: "Plan30 Pick".into(),
        },
    );
    assert!(matches!(
        embedded_response(&anvil_output, HOST_SESSION_ID, 0x40_006).outcome,
        GameplayOutcome::Accepted { .. }
    ));

    let owner_state = runtime
        .authority
        .session(HOST_SESSION_ID)
        .expect("embedded owner state before brew")
        .gameplay;
    let (_, brew_output) = embedded_submit(
        &mut runtime,
        &input,
        HOST_SESSION_ID,
        0x40_007,
        7,
        GameplayOperation::Brew {
            action: 0,
            x: 8,
            y: 80,
            z: 10,
            ingredient: Some(source(owner_state, 5, 1)),
            bottles: [Some(source(owner_state, 2, 1)), None, None],
        },
    );
    assert!(matches!(
        embedded_response(&brew_output, HOST_SESSION_ID, 0x40_007).outcome,
        GameplayOutcome::Accepted { .. }
    ));
    for _ in 0..BREW_TICKS {
        runtime
            .tick_with_output()
            .expect("embedded brew fixed tick");
    }
    let (_, brew_take_output) = embedded_submit(
        &mut runtime,
        &input,
        HOST_SESSION_ID,
        0x40_008,
        8,
        GameplayOperation::Brew {
            action: 2,
            x: 8,
            y: 80,
            z: 10,
            ingredient: None,
            bottles: [None, None, None],
        },
    );
    assert!(matches!(
        embedded_response(&brew_take_output, HOST_SESSION_ID, 0x40_008).outcome,
        GameplayOutcome::Accepted { .. }
    ));

    let (_, combat_output) = embedded_submit(
        &mut runtime,
        &input,
        HOST_SESSION_ID,
        0x40_009,
        9,
        GameplayOperation::Combat {
            target: EMBEDDED_VICTIM_ID,
            action: 0,
        },
    );
    assert!(matches!(
        embedded_response(&combat_output, HOST_SESSION_ID, 0x40_009).outcome,
        GameplayOutcome::Accepted { .. }
    ));
    assert!(runtime
        .authority
        .session(EMBEDDED_VICTIM_ID)
        .is_some_and(|session| session.gameplay.is_dead));
    input
        .try_send(ServerToHost::ClientRespawnRequest {
            id: EMBEDDED_VICTIM_ID,
        })
        .expect("embedded respawn request");
    let respawn_output = runtime
        .tick_with_output()
        .expect("embedded respawn fixed tick");
    assert!(runtime
        .authority
        .session(EMBEDDED_VICTIM_ID)
        .is_some_and(|session| !session.gameplay.is_dead));
    assert!(respawn_output
        .snapshot
        .session_updates
        .iter()
        .any(|update| { update.player_id == EMBEDDED_VICTIM_ID && !update.state.is_dead }));

    let (_, out_of_order_output) = embedded_submit(
        &mut runtime,
        &input,
        HOST_SESSION_ID,
        0x40_00a,
        1,
        GameplayOperation::ItemUse { item: 1, count: 1 },
    );
    assert_eq!(
        embedded_response(&out_of_order_output, HOST_SESSION_ID, 0x40_00a).outcome,
        GameplayOutcome::Rejected {
            reason: RejectReason::OutOfOrder
        }
    );
    let stale = GameplayRequest {
        client_revision: 0,
        ..request(
            &runtime,
            HOST_SESSION_ID,
            0x40_00b,
            10,
            GameplayOperation::ItemUse { item: 1, count: 1 },
        )
    };
    input
        .submit_request(HOST_SESSION_ID, stale)
        .expect("embedded stale request");
    let stale_output = runtime
        .tick_with_output()
        .expect("embedded stale fixed tick");
    assert_eq!(
        embedded_response(&stale_output, HOST_SESSION_ID, 0x40_00b).outcome,
        GameplayOutcome::Rejected {
            reason: RejectReason::InvalidRevision
        }
    );

    runtime
        .shutdown()
        .expect("shutdown embedded Plan30 runtime");
    let _ = fs::remove_dir_all(world_dir);
}

fn run_topology(label: &str, listen: bool) {
    let reserved = HeldLoopback::bind();
    let mut properties = properties(label);
    properties.port = reserved.port();
    let address = format!("{}:{}", properties.bind, properties.port);
    let _port = reserved.release();
    let (mut runtime, _input, local_host) = if listen {
        let (runtime, input) = ServerRuntime::new_embedded(
            properties.clone(),
            EmbeddedRuntimeOptions {
                topology: AuthorityTopology::ListenServer,
                transport: TransportMode::Listen,
                local_session: Some(LocalSessionProfile::new(HOST_SESSION_ID, "local-host")),
            },
        )
        .expect("construct listen runtime");
        (runtime, Some(input), Some(HOST_SESSION_ID))
    } else {
        (
            ServerRuntime::new(properties.clone()).expect("construct dedicated runtime"),
            None,
            None,
        )
    };

    // Two remote clients are intentional in ListenServer: one satisfies the
    // local-host+TCP-remote topology requirement, and the second gives the
    // same owner/observer assertions as DedicatedTwoClients.
    let owner = TcpClient::connect(&address, "plan30-owner");
    let victim = TcpClient::connect(&address, "plan30-victim");
    let mut clients = vec![owner, victim];
    {
        let mut refs: Vec<&mut TcpClient> = clients.iter_mut().collect();
        drive_until(
            &mut runtime,
            &mut refs,
            "real TCP clients authenticated",
            |runtime, views| {
                views.iter().all(|client| client.player_id().is_some())
                    && runtime.players.len() == if local_host.is_some() { 3 } else { 2 }
            },
        );
    }
    let owner_id = clients[0].player_id().expect("owner id");
    let victim_id = clients[1].player_id().expect("victim id");
    assert_ne!(owner_id, victim_id);
    prepare_fixture(&mut runtime, owner_id, victim_id);
    clients[0].send_position(1, 1, POSITION, 0.0, 0.0);
    clients[1].send_position(1, 1, VICTIM_POSITION, 180.0, 0.0);
    {
        let mut refs: Vec<&mut TcpClient> = clients.iter_mut().collect();
        drive_until(
            &mut runtime,
            &mut refs,
            "real TCP pose accepted",
            |runtime, _| {
                runtime
                    .players
                    .get(&owner_id)
                    .is_some_and(|player| player.data.position == POSITION)
                    && runtime
                        .players
                        .get(&victim_id)
                        .is_some_and(|player| player.data.position == VICTIM_POSITION)
            },
        );
        drain_clients(&mut refs);
    }
    for client in &mut clients {
        client.clear_events();
    }

    let cast = request(
        &runtime,
        owner_id,
        0x30_001,
        1,
        GameplayOperation::Fishing {
            action: 0,
            hand: 1,
            look_milli: [0, 0, 1_000],
        },
    );
    clients[0].send_request(cast.clone());
    let mut refs: Vec<&mut TcpClient> = clients.iter_mut().collect();
    let cast_response = wait_for_response(&mut runtime, &mut refs, 0, cast.request_id);
    assert!(
        matches!(cast_response.outcome, GameplayOutcome::Accepted { .. }),
        "cast response: {:?}",
        cast_response
    );
    assert!(runtime
        .authority
        .session(owner_id)
        .and_then(|session| session.gameplay.fishing_hook)
        .is_some());
    drop(refs);
    let duplicate_count = runtime.metrics.duplicate_requests;
    clients[0].send_request(cast.clone());
    let mut refs: Vec<&mut TcpClient> = clients.iter_mut().collect();
    drive_until(
        &mut runtime,
        &mut refs,
        "fishing duplicate gate",
        |runtime, _| runtime.metrics.duplicate_requests > duplicate_count,
    );
    drop(refs);
    let cached_duplicate = runtime
        .authority
        .session(owner_id)
        .and_then(|session| session.cached_response(cast.request_id));
    assert_eq!(
        cached_duplicate,
        Some(cast_response.clone()),
        "TCP duplicate reuses the cached ACK/outcome/revision"
    );
    assert!(runtime.metrics.duplicate_requests > duplicate_count);

    // A zero client sequence marks a fresh player input. NetworkClient assigns
    // the next sequence and the latest owner-private revision immediately
    // before the real socket write, after any intervening hook fixed ticks.
    let mut reel = request(
        &runtime,
        owner_id,
        0x30_002,
        2,
        GameplayOperation::Fishing {
            action: 1,
            hand: 1,
            look_milli: [0, 0, 1_000],
        },
    );
    reel.client_sequence = 0;
    clients[0].send_request(reel);
    let mut refs: Vec<&mut TcpClient> = clients.iter_mut().collect();
    let reel_response = wait_for_response(&mut runtime, &mut refs, 0, 0x30_002);
    drop(refs);
    assert!(
        matches!(reel_response.outcome, GameplayOutcome::Accepted { .. }),
        "fresh TCP reel must use the latest owner revision: {reel_response:?}"
    );
    assert!(runtime
        .authority
        .session(owner_id)
        .is_some_and(|session| session.gameplay.fishing_hook.is_none()));

    reset_persistent_domains(&mut runtime, owner_id);

    let furnace = request(
        &runtime,
        owner_id,
        0x30_003,
        3,
        GameplayOperation::FurnaceTakeOutput {
            x: 8,
            y: 80,
            z: 9,
            count: 1,
        },
    );
    clients[0].send_request(furnace);
    let mut refs: Vec<&mut TcpClient> = clients.iter_mut().collect();
    assert!(matches!(
        wait_for_response(&mut runtime, &mut refs, 0, 0x30_003).outcome,
        GameplayOutcome::Accepted { .. }
    ));
    drop(refs);

    let owner_state = runtime
        .authority
        .session(owner_id)
        .expect("owner state after furnace")
        .gameplay;
    let plank = source(owner_state, 1, 1);
    let mut craft_sources = [None; 9];
    craft_sources[0] = Some(plank);
    craft_sources[2] = Some(plank);
    let craft = request(
        &runtime,
        owner_id,
        0x30_004,
        4,
        GameplayOperation::Craft {
            grid: 2,
            sources: craft_sources,
            station: None,
        },
    );
    clients[0].send_request(craft);
    let mut refs: Vec<&mut TcpClient> = clients.iter_mut().collect();
    assert!(matches!(
        wait_for_response(&mut runtime, &mut refs, 0, 0x30_004).outcome,
        GameplayOutcome::Accepted { .. }
    ));
    drop(refs);
    assert!(clients[0].has_session_update(owner_id));

    let owner_state = runtime
        .authority
        .session(owner_id)
        .expect("owner state after craft")
        .gameplay;
    let enchant = request(
        &runtime,
        owner_id,
        0x30_005,
        5,
        GameplayOperation::Enchant {
            x: 8,
            y: 80,
            z: 11,
            source: source(owner_state, 3, 1),
            option: 2,
        },
    );
    clients[0].send_request(enchant);
    let mut refs: Vec<&mut TcpClient> = clients.iter_mut().collect();
    assert!(matches!(
        wait_for_response(&mut runtime, &mut refs, 0, 0x30_005).outcome,
        GameplayOutcome::Accepted { .. }
    ));
    drop(refs);

    let owner_state = runtime
        .authority
        .session(owner_id)
        .expect("owner state after enchant")
        .gameplay;
    let anvil = request(
        &runtime,
        owner_id,
        0x30_006,
        6,
        GameplayOperation::Anvil {
            x: 8,
            y: 80,
            z: 12,
            left: source(owner_state, 3, 1),
            right: None,
            rename: "Plan30 Pick".into(),
        },
    );
    clients[0].send_request(anvil);
    let mut refs: Vec<&mut TcpClient> = clients.iter_mut().collect();
    assert!(matches!(
        wait_for_response(&mut runtime, &mut refs, 0, 0x30_006).outcome,
        GameplayOutcome::Accepted { .. }
    ));
    drop(refs);

    let owner_state = runtime
        .authority
        .session(owner_id)
        .expect("owner state after workstation")
        .gameplay;
    let brew = request(
        &runtime,
        owner_id,
        0x30_007,
        7,
        GameplayOperation::Brew {
            action: 0,
            x: 8,
            y: 80,
            z: 10,
            ingredient: Some(source(owner_state, 5, 1)),
            bottles: [Some(source(owner_state, 2, 1)), None, None],
        },
    );
    clients[0].send_request(brew);
    let mut refs: Vec<&mut TcpClient> = clients.iter_mut().collect();
    assert!(matches!(
        wait_for_response(&mut runtime, &mut refs, 0, 0x30_007).outcome,
        GameplayOutcome::Accepted { .. }
    ));
    for _ in 0..BREW_TICKS {
        runtime.tick().expect("brew fixed tick");
        drain_clients(&mut refs);
    }
    drop(refs);
    assert_eq!(
        runtime
            .authority
            .session(owner_id)
            .and_then(|session| session.gameplay.brew)
            .map(|brew| brew.remaining_ticks),
        Some(0)
    );
    let brew_take = request(
        &runtime,
        owner_id,
        0x30_008,
        8,
        GameplayOperation::Brew {
            action: 2,
            x: 8,
            y: 80,
            z: 10,
            ingredient: None,
            bottles: [None, None, None],
        },
    );
    clients[0].send_request(brew_take);
    let mut refs: Vec<&mut TcpClient> = clients.iter_mut().collect();
    assert!(matches!(
        wait_for_response(&mut runtime, &mut refs, 0, 0x30_008).outcome,
        GameplayOutcome::Accepted { .. }
    ));
    drop(refs);
    assert!(runtime
        .authority
        .session(owner_id)
        .is_some_and(|session| session.gameplay.brew.is_none()));

    let player_combat = request(
        &runtime,
        owner_id,
        0x30_009,
        9,
        GameplayOperation::Combat {
            target: victim_id,
            action: 0,
        },
    );
    clients[0].send_request(player_combat);
    let mut refs: Vec<&mut TcpClient> = clients.iter_mut().collect();
    assert!(matches!(
        wait_for_response(&mut runtime, &mut refs, 0, 0x30_009).outcome,
        GameplayOutcome::Accepted { .. }
    ));
    drop(refs);
    assert!(runtime
        .authority
        .session(victim_id)
        .is_some_and(|session| session.gameplay.is_dead));
    clients[1].send(GameToClient::PlayerRespawnRequest);
    let mut refs: Vec<&mut TcpClient> = clients.iter_mut().collect();
    drive_until(
        &mut runtime,
        &mut refs,
        "TCP respawn projection",
        |runtime, views| {
            runtime
                .authority
                .session(victim_id)
                .is_some_and(|session| !session.gameplay.is_dead)
                && views[1]
                    .events()
                    .iter()
                    .any(|event| matches!(event, ClientToGame::PlayerRespawnResult { .. }))
        },
    );
    assert_eq!(
        runtime
            .authority
            .session(victim_id)
            .expect("respawned victim")
            .gameplay
            .velocity_milli,
        [0; 3]
    );
    drop(refs);

    // Domain rejects still consume a transport sequence; the subsequent
    // stale request therefore uses the next sequence rather than replaying
    // the attacker packet.
    let out_of_order = request(
        &runtime,
        owner_id,
        0x30_00a,
        1,
        GameplayOperation::ItemUse { item: 1, count: 1 },
    );
    clients[0].send_request(out_of_order);
    let mut refs: Vec<&mut TcpClient> = clients.iter_mut().collect();
    assert_eq!(
        wait_for_response(&mut runtime, &mut refs, 0, 0x30_00a).outcome,
        GameplayOutcome::Rejected {
            reason: RejectReason::OutOfOrder
        }
    );
    drop(refs);
    let stale = request(
        &runtime,
        owner_id,
        0x30_00b,
        10,
        GameplayOperation::ItemUse { item: 1, count: 1 },
    );
    let stale = GameplayRequest {
        client_revision: 0,
        ..stale
    };
    clients[0].send_request(stale);
    let mut refs: Vec<&mut TcpClient> = clients.iter_mut().collect();
    assert_eq!(
        wait_for_response(&mut runtime, &mut refs, 0, 0x30_00b).outcome,
        GameplayOutcome::Rejected {
            reason: RejectReason::InvalidRevision
        }
    );
    drop(refs);

    // Rich session payloads are owner-private even though both clients share
    // the same interest area.  This is observed over TCP, not by inspecting a
    // presentation event queue.
    let update_seen = clients[0].has_session_update(owner_id);
    assert!(
        update_seen,
        "owner must receive private gameplay projection"
    );
    assert!(!clients[1].events().iter().any(|event| {
        matches!(
            event,
            ClientToGame::PlayerSessionUpdate { player_id, .. } if *player_id == owner_id
        )
    }));

    // Reconnect is deliberately transport-driven.  Dimension travel remains
    // a Plan32 blocker because no player-facing travel operation exists yet.
    clients[1].disconnect_and_join();
    {
        let mut refs: Vec<&mut TcpClient> = vec![&mut clients[0]];
        drive_until(
            &mut runtime,
            &mut refs,
            "TCP client disconnect",
            |runtime, _| runtime.players.len() == if local_host.is_some() { 2 } else { 1 },
        );
    }
    let mut reconnected = TcpClient::connect(&address, "plan30-victim");
    {
        let mut refs: Vec<&mut TcpClient> = vec![&mut clients[0], &mut reconnected];
        drive_until(
            &mut runtime,
            &mut refs,
            "TCP reconnect",
            |runtime, views| {
                views[1].player_id().is_some()
                    && runtime.players.len() == if local_host.is_some() { 3 } else { 2 }
                    && views[1]
                        .has_session_update(views[1].player_id().expect("reconnected player id"))
            },
        );
    }
    reconnected.disconnect_and_join();
    clients[0].disconnect_and_join();
    for _ in 0..4 {
        runtime.tick().expect("process TCP disconnect");
    }
    runtime.shutdown().expect("shutdown Plan30 runtime");
    let _ = fs::remove_dir_all(properties.world_dir);
}

#[test]
fn plan30_embedded_singleplayer_contract_matches_domain_assertions() {
    run_singleplayer_embedded_contract();
}

#[test]
fn plan30_real_tcp_gameplay_vector_listen_and_dedicated() {
    run_topology("listen", true);
    run_topology("dedicated", false);
}
