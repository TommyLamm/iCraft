mod common;

use common::tcp_harness::{
    drive_until, gameplay_request as request, seeded_properties, session_slot, wait_for_response,
    HeldLoopback, TcpClient,
};
use icraft::authority::contract::SessionGameplayState;
use icraft::authority::fishing::water_probe_position;
use icraft::fishing::{FishingHookStage, FISHING_INITIAL_WAIT_TICKS};
use icraft::inventory::{Item, ItemStack};
use icraft::network::client::ClientToGame;
use icraft::network::protocol::{
    GameplayOperation, GameplayOutcome, GameplayRequest, GameplayResponse, RejectReason,
};
use icraft::server_runtime::{
    EmbeddedRuntimeOptions, LocalSessionProfile, RuntimePresentationEvent, RuntimeTickOutput,
    ServerProperties, ServerRuntime, TransportMode,
};
use icraft::world::BlockType;
use std::fs;

const EMBEDDED_OWNER: u64 = 0x33_0000;
const EMBEDDED_OBSERVER: u64 = EMBEDDED_OWNER + 1;
const POSITION: [f32; 3] = [8.0, 80.0, 8.0];
const OBSERVER_POSITION: [f32; 3] = [10.0, 80.0, 8.0];
const LOOK: [i16; 3] = [0, 0, 1_000];

fn properties(label: &str) -> ServerProperties {
    seeded_properties(&format!("plan33-{label}"), 0x33_33_33_33)
}

fn fishing(action: u8) -> GameplayOperation {
    GameplayOperation::Fishing {
        action,
        hand: 0,
        look_milli: LOOK,
    }
}

fn fresh_tcp_request(
    runtime: &ServerRuntime,
    player_id: u64,
    request_id: u128,
    operation: GameplayOperation,
) -> GameplayRequest {
    let mut request = request(runtime, player_id, request_id, 0, operation);
    // Deliberately stale at queue time. A zero sequence marks a fresh input;
    // NetworkClient must bind it to the latest owner projection immediately
    // before the socket write.
    request.client_revision = 0;
    request
}

fn prepare(runtime: &mut ServerRuntime, owner: u64, observer: u64) {
    runtime.authority.world_mut_active().ensure_chunk(0, 0);
    let mut owner_gameplay = SessionGameplayState::default();
    owner_gameplay.inventory[0] = Some(session_slot(ItemStack::new(Item::FishingRod, 1)));
    owner_gameplay.selected_hotbar_slot = 0;
    assert!(runtime
        .authority
        .set_session_gameplay(owner, owner_gameplay));
    assert!(runtime
        .authority
        .set_session_gameplay(observer, SessionGameplayState::default()));
    for id in [owner, observer] {
        let position = if id == owner {
            POSITION
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

fn seed_water_under_hook(runtime: &mut ServerRuntime, owner: u64) {
    let state = runtime
        .authority
        .session(owner)
        .expect("Plan33 owner session")
        .gameplay;
    let probe = water_probe_position(&state).expect("active Plan33 hook probe");
    let position = (
        probe[0].div_euclid(1_000),
        probe[1].div_euclid(1_000),
        probe[2].div_euclid(1_000),
    );
    runtime
        .authority
        .world_mut_active()
        .ensure_chunk(position.0.div_euclid(16), position.2.div_euclid(16));
    if runtime
        .authority
        .world()
        .get_block(position.0, position.1, position.2)
        != BlockType::Water
    {
        runtime
            .authority
            .world_mut_active()
            .set_block(position.0, position.1, position.2, BlockType::Water, 0)
            .expect("seed deterministic Plan33 open water");
    }
}

fn inventory_loot_count(state: SessionGameplayState) -> u32 {
    state
        .inventory
        .iter()
        .flatten()
        .filter(|slot| slot.item.item != Item::FishingRod.to_u32())
        .map(|slot| u32::from(slot.item.count))
        .sum()
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
        .expect("Plan33 embedded gameplay response")
}

fn embedded_submit(
    runtime: &mut ServerRuntime,
    input: &icraft::server_runtime::RuntimeInput,
    request: GameplayRequest,
) -> (GameplayResponse, RuntimeTickOutput) {
    let request_id = request.request_id;
    let player_id = request.session_id;
    input
        .submit_request(player_id, request)
        .expect("submit embedded Plan33 request");
    let output = runtime
        .tick_with_output()
        .expect("tick embedded Plan33 request");
    (embedded_response(&output, player_id, request_id), output)
}

fn run_embedded() {
    let properties = properties("embedded");
    let world_dir = properties.world_dir.clone();
    let (mut runtime, input) = ServerRuntime::new_embedded(
        properties,
        EmbeddedRuntimeOptions {
            transport: TransportMode::Disabled,
            local_session: Some(LocalSessionProfile::new(EMBEDDED_OWNER, "plan33-owner")),
        },
    )
    .expect("construct embedded Plan33 runtime");
    runtime
        .login_session(EMBEDDED_OBSERVER, "plan33-observer")
        .expect("login embedded Plan33 observer");
    let _ = runtime
        .tick_with_output()
        .expect("drain embedded Plan33 login");
    prepare(&mut runtime, EMBEDDED_OWNER, EMBEDDED_OBSERVER);

    let cast = request(&runtime, EMBEDDED_OWNER, 0x33_001, 1, fishing(0));
    let (cast_response, cast_output) = embedded_submit(&mut runtime, &input, cast.clone());
    assert!(matches!(
        cast_response.outcome,
        GameplayOutcome::Accepted { .. }
    ));
    assert!(cast_output.presentation_events.iter().any(|event| {
        matches!(
            event,
            RuntimePresentationEvent::PlayerSessionUpdate { target, state, .. }
                if *target == EMBEDDED_OWNER && state.fishing_hook.is_some()
        )
    }));
    assert!(!cast_output.presentation_events.iter().any(|event| {
        matches!(
            event,
            RuntimePresentationEvent::PlayerSessionUpdate {
                target,
                player_id,
                ..
            } if *target == EMBEDDED_OBSERVER && *player_id == EMBEDDED_OWNER
        )
    }));

    let (duplicate_cast, _) = embedded_submit(&mut runtime, &input, cast);
    assert_eq!(duplicate_cast, cast_response);
    let hook_id = runtime
        .authority
        .session(EMBEDDED_OWNER)
        .and_then(|session| session.gameplay.fishing_hook)
        .expect("embedded cast hook")
        .entity_id;

    let mut nibbled = false;
    for _ in 0..(FISHING_INITIAL_WAIT_TICKS + 8) {
        seed_water_under_hook(&mut runtime, EMBEDDED_OWNER);
        let _ = runtime
            .tick_with_output()
            .expect("embedded Plan33 fishing tick");
        nibbled = runtime
            .authority
            .session(EMBEDDED_OWNER)
            .and_then(|session| session.gameplay.fishing_hook)
            .is_some_and(|hook| hook.stage == FishingHookStage::Nibbling.to_wire());
        if nibbled {
            break;
        }
    }
    assert!(nibbled, "embedded Plan33 hook reaches a bite");

    let before = runtime
        .authority
        .session(EMBEDDED_OWNER)
        .expect("embedded state before reel")
        .gameplay;
    let reel = request(&runtime, EMBEDDED_OWNER, 0x33_002, 2, fishing(1));
    let (reel_response, _) = embedded_submit(&mut runtime, &input, reel.clone());
    assert!(matches!(
        reel_response.outcome,
        GameplayOutcome::Accepted { .. }
    ));
    let after = runtime
        .authority
        .session(EMBEDDED_OWNER)
        .expect("embedded state after reel")
        .gameplay;
    assert!(after.fishing_hook.is_none());
    assert!(after.experience > before.experience);
    assert!(inventory_loot_count(after) > inventory_loot_count(before));
    assert!(
        after.inventory[0].unwrap().item.durability < before.inventory[0].unwrap().item.durability
    );
    assert!(runtime
        .authority
        .world_mut_active()
        .entities
        .get_by_id(hook_id)
        .is_none());
    let (duplicate_reel, _) = embedded_submit(&mut runtime, &input, reel);
    assert_eq!(duplicate_reel, reel_response);
    assert_eq!(
        runtime
            .authority
            .session(EMBEDDED_OWNER)
            .expect("embedded state after duplicate reel")
            .gameplay,
        after
    );

    let cast_cancel = request(&runtime, EMBEDDED_OWNER, 0x33_003, 3, fishing(0));
    assert!(matches!(
        embedded_submit(&mut runtime, &input, cast_cancel).0.outcome,
        GameplayOutcome::Accepted { .. }
    ));
    let cancel = request(&runtime, EMBEDDED_OWNER, 0x33_004, 4, fishing(2));
    assert!(matches!(
        embedded_submit(&mut runtime, &input, cancel).0.outcome,
        GameplayOutcome::Accepted { .. }
    ));
    assert!(runtime
        .authority
        .session(EMBEDDED_OWNER)
        .is_some_and(|session| session.gameplay.fishing_hook.is_none()));

    let out_of_order = request(&runtime, EMBEDDED_OWNER, 0x33_005, 4, fishing(0));
    assert_eq!(
        embedded_submit(&mut runtime, &input, out_of_order)
            .0
            .outcome,
        GameplayOutcome::Rejected {
            reason: RejectReason::OutOfOrder
        }
    );
    let mut stale = request(&runtime, EMBEDDED_OWNER, 0x33_006, 5, fishing(0));
    stale.client_revision = 0;
    assert_eq!(
        embedded_submit(&mut runtime, &input, stale).0.outcome,
        GameplayOutcome::Rejected {
            reason: RejectReason::InvalidRevision
        }
    );

    runtime
        .shutdown()
        .expect("shutdown embedded Plan33 runtime");
    let _ = fs::remove_dir_all(world_dir);
}

fn run_tcp(label: &str, listen: bool) {
    let reserved = HeldLoopback::bind();
    let mut properties = properties(label);
    properties.port = reserved.port();
    let address = format!("{}:{}", properties.bind, properties.port);
    let _port = reserved.release();
    let (mut runtime, local_host) = if listen {
        let (runtime, _input) = ServerRuntime::new_embedded(
            properties.clone(),
            EmbeddedRuntimeOptions {
                transport: TransportMode::Listen,
                local_session: Some(LocalSessionProfile::new(EMBEDDED_OWNER, "plan33-host")),
            },
        )
        .expect("construct listen Plan33 runtime");
        (runtime, true)
    } else {
        (
            ServerRuntime::new(properties.clone()).expect("construct dedicated Plan33 runtime"),
            false,
        )
    };
    let mut clients = vec![
        TcpClient::connect(&address, "plan33-owner"),
        TcpClient::connect(&address, "plan33-observer"),
    ];
    {
        let mut refs: Vec<&mut TcpClient> = clients.iter_mut().collect();
        drive_until(
            &mut runtime,
            &mut refs,
            "Plan33 TCP clients authenticated",
            |runtime, views| {
                views.iter().all(|client| client.player_id().is_some())
                    && runtime.players.len() == if local_host { 3 } else { 2 }
            },
        );
        for client in refs.iter_mut() {
            client.clear_events();
        }
    }
    let owner = clients[0].player_id().expect("Plan33 TCP owner id");
    let observer = clients[1].player_id().expect("Plan33 TCP observer id");
    prepare(&mut runtime, owner, observer);

    let cast = request(&runtime, owner, 0x33_101, 1, fishing(0));
    clients[0].send_request(cast.clone());
    let cast_response = {
        let mut refs: Vec<&mut TcpClient> = clients.iter_mut().collect();
        wait_for_response(&mut runtime, &mut refs, 0, cast.request_id)
    };
    assert!(matches!(
        cast_response.outcome,
        GameplayOutcome::Accepted { .. }
    ));
    let duplicate_before = runtime.metrics.duplicate_requests;
    clients[0].send_request(cast);
    {
        let mut refs: Vec<&mut TcpClient> = clients.iter_mut().collect();
        drive_until(
            &mut runtime,
            &mut refs,
            "Plan33 cached cast duplicate",
            |runtime, _| runtime.metrics.duplicate_requests > duplicate_before,
        );
    }
    assert_eq!(
        runtime
            .authority
            .session(owner)
            .and_then(|session| session.cached_response(0x33_101)),
        Some(cast_response)
    );
    assert!(clients[0].events().iter().any(|event| {
        matches!(
            event,
            ClientToGame::PlayerSessionUpdate { player_id, state, .. }
                if *player_id == owner && state.fishing_hook.is_some()
        )
    }));
    assert!(!clients[1].events().iter().any(|event| {
        matches!(
            event,
            ClientToGame::PlayerSessionUpdate { player_id, .. } if *player_id == owner
        )
    }));
    let hook_id = runtime
        .authority
        .session(owner)
        .and_then(|session| session.gameplay.fishing_hook)
        .expect("Plan33 TCP cast hook")
        .entity_id;

    let mut nibbled = false;
    for _ in 0..(FISHING_INITIAL_WAIT_TICKS + 8) {
        seed_water_under_hook(&mut runtime, owner);
        runtime.tick().expect("Plan33 TCP fishing tick");
        for client in &mut clients {
            client.drain();
        }
        nibbled = runtime
            .authority
            .session(owner)
            .and_then(|session| session.gameplay.fishing_hook)
            .is_some_and(|hook| hook.stage == FishingHookStage::Nibbling.to_wire());
        if nibbled {
            break;
        }
    }
    assert!(nibbled, "{label} TCP hook reaches a bite");
    let before = runtime
        .authority
        .session(owner)
        .expect("Plan33 TCP state before reel")
        .gameplay;

    clients[0].send_request(fresh_tcp_request(&runtime, owner, 0x33_102, fishing(1)));
    let reel_response = {
        let mut refs: Vec<&mut TcpClient> = clients.iter_mut().collect();
        wait_for_response(&mut runtime, &mut refs, 0, 0x33_102)
    };
    assert!(
        matches!(reel_response.outcome, GameplayOutcome::Accepted { .. }),
        "{label} fresh reel response: {reel_response:?}"
    );
    let after = runtime
        .authority
        .session(owner)
        .expect("Plan33 TCP state after reel")
        .gameplay;
    assert!(after.fishing_hook.is_none());
    assert!(after.experience > before.experience);
    assert!(inventory_loot_count(after) > inventory_loot_count(before));
    assert!(
        after.inventory[0].unwrap().item.durability < before.inventory[0].unwrap().item.durability
    );
    assert!(runtime
        .authority
        .world_mut_active()
        .entities
        .get_by_id(hook_id)
        .is_none());

    let duplicate_before = runtime.metrics.duplicate_requests;
    clients[0].send_request(fresh_tcp_request(&runtime, owner, 0x33_102, fishing(1)));
    {
        let mut refs: Vec<&mut TcpClient> = clients.iter_mut().collect();
        drive_until(
            &mut runtime,
            &mut refs,
            "Plan33 cached reel duplicate",
            |runtime, _| runtime.metrics.duplicate_requests > duplicate_before,
        );
    }
    assert_eq!(
        runtime
            .authority
            .session(owner)
            .expect("Plan33 TCP state after duplicate reel")
            .gameplay,
        after
    );

    clients[0].send_request(fresh_tcp_request(&runtime, owner, 0x33_103, fishing(0)));
    {
        let mut refs: Vec<&mut TcpClient> = clients.iter_mut().collect();
        assert!(matches!(
            wait_for_response(&mut runtime, &mut refs, 0, 0x33_103).outcome,
            GameplayOutcome::Accepted { .. }
        ));
    }
    clients[0].send_request(fresh_tcp_request(&runtime, owner, 0x33_104, fishing(2)));
    {
        let mut refs: Vec<&mut TcpClient> = clients.iter_mut().collect();
        assert!(matches!(
            wait_for_response(&mut runtime, &mut refs, 0, 0x33_104).outcome,
            GameplayOutcome::Accepted { .. }
        ));
    }
    assert!(runtime
        .authority
        .session(owner)
        .is_some_and(|session| session.gameplay.fishing_hook.is_none()));

    let authority_sequence = runtime
        .authority
        .session(owner)
        .expect("Plan33 sequence baseline")
        .last_client_sequence;
    let out_of_order = request(&runtime, owner, 0x33_105, authority_sequence, fishing(0));
    clients[0].send_request(out_of_order);
    {
        let mut refs: Vec<&mut TcpClient> = clients.iter_mut().collect();
        assert_eq!(
            wait_for_response(&mut runtime, &mut refs, 0, 0x33_105).outcome,
            GameplayOutcome::Rejected {
                reason: RejectReason::OutOfOrder
            }
        );
    }
    let mut stale = request(
        &runtime,
        owner,
        0x33_106,
        authority_sequence.saturating_add(1),
        fishing(0),
    );
    stale.client_revision = 0;
    clients[0].send_request(stale);
    {
        let mut refs: Vec<&mut TcpClient> = clients.iter_mut().collect();
        assert_eq!(
            wait_for_response(&mut runtime, &mut refs, 0, 0x33_106).outcome,
            GameplayOutcome::Rejected {
                reason: RejectReason::InvalidRevision
            }
        );
    }

    clients[0].disconnect_and_join();
    clients[1].disconnect_and_join();
    runtime.shutdown().expect("shutdown TCP Plan33 runtime");
    let _ = fs::remove_dir_all(properties.world_dir);
}

#[test]
fn plan33_embedded_fishing_lifecycle_matches_revision_contract() {
    run_embedded();
}

#[test]
fn plan33_listen_tcp_fishing_lifecycle_uses_latest_owner_revision() {
    run_tcp("listen", true);
}

#[test]
fn plan33_dedicated_tcp_fishing_lifecycle_uses_latest_owner_revision() {
    run_tcp("dedicated", false);
}
