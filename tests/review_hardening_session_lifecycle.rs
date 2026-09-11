//! Plan 08: authority session lifecycle — gateway hop, alive respawn, ignite
//! debit failure, and identity release after a failed persist.

mod common;

use common::tcp_harness::{
    gameplay_request, held, loopback_properties, session_slot as slot, temp_world,
};
use icraft::authority::contract::{SessionBrewState, SessionGameplayState};
use icraft::dimension::Dimension;
use icraft::inventory::{Item, ItemStack};
use icraft::network::protocol::{
    BlockActionKind, GameplayOperation, GameplayOutcome, RejectReason, SlotRefWire,
};
use icraft::network::server::ServerToHost;
use icraft::server_runtime::{
    EmbeddedRuntimeOptions, LocalSessionProfile, ServerProperties, ServerRuntime,
};
use icraft::world::BlockType;
use std::fs;

const LOCAL_ID: u64 = 0x08_0001;
const GATEWAY: (i32, i32, i32) = (8, 65, 8);
const GATEWAY_POSE: [f32; 3] = [8.5, 65.0, 8.5];
const OUTER_ISLAND: [f32; 3] = [1035.5, 89.0, 11.5];
const PORTAL_LOOK: [i16; 3] = [0, -500, 866];

fn properties(label: &str) -> ServerProperties {
    let mut properties = loopback_properties(temp_world(&format!("plan08-{label}")), "127.0.0.1");
    properties.seed = 8_008;
    properties.view_distance = 4;
    properties.simulation_distance = 4;
    properties.max_players = 20;
    properties
}

#[test]
fn end_gateway_hop_accepts_destination_pose() {
    let props = properties("gateway");
    let (mut runtime, input) = ServerRuntime::new_embedded(
        props.clone(),
        EmbeddedRuntimeOptions::singleplayer(LocalSessionProfile::new(LOCAL_ID, "plan08-local")),
    )
    .unwrap();
    assert!(runtime.set_session_dimension(LOCAL_ID, Dimension::End));
    runtime.authority.with_world(Dimension::End, |world| {
        world
            .set_block(GATEWAY.0, GATEWAY.1, GATEWAY.2, BlockType::EndGateway, 0)
            .unwrap();
    });
    assert!(runtime.teleport_session(LOCAL_ID, GATEWAY_POSE));

    input
        .try_send(ServerToHost::ClientPosition {
            id: LOCAL_ID,
            sequence: 1,
            sender_time_millis: 100,
            x: GATEWAY_POSE[0],
            y: GATEWAY_POSE[1],
            z: GATEWAY_POSE[2],
            yaw: 0.0,
            pitch: 0.0,
        })
        .unwrap();
    runtime.tick().unwrap();

    let enter = gameplay_request(
        &runtime,
        LOCAL_ID,
        1,
        1,
        GameplayOperation::BlockAction {
            action: BlockActionKind::EnterPortal,
            x: GATEWAY.0,
            y: GATEWAY.1,
            z: GATEWAY.2,
            face: [0, 0, 0],
            hand: 0,
            held: None,
            block: BlockType::EndGateway.to_wire(),
            look_milli: PORTAL_LOOK,
        },
    );
    input.submit_request(LOCAL_ID, enter).unwrap();
    runtime.tick().unwrap();

    let authority = runtime.authority.session(LOCAL_ID).unwrap();
    assert_eq!(authority.dimension, Dimension::End as u8);
    assert_eq!(authority.position, OUTER_ISLAND);
    assert_eq!(runtime.players[&LOCAL_ID].data.position, OUTER_ISLAND);
    assert!(authority.gameplay.revision > 0);

    input
        .try_send(ServerToHost::ClientPosition {
            id: LOCAL_ID,
            sequence: 2,
            sender_time_millis: 200,
            x: OUTER_ISLAND[0],
            y: OUTER_ISLAND[1],
            z: OUTER_ISLAND[2],
            yaw: 0.0,
            pitch: 0.0,
        })
        .unwrap();
    runtime.tick().unwrap();

    let stepped = [OUTER_ISLAND[0], OUTER_ISLAND[1], OUTER_ISLAND[2] + 1.0];
    input
        .try_send(ServerToHost::ClientPosition {
            id: LOCAL_ID,
            sequence: 3,
            sender_time_millis: 400,
            x: stepped[0],
            y: stepped[1],
            z: stepped[2],
            yaw: 0.0,
            pitch: 0.0,
        })
        .unwrap();
    runtime.tick().unwrap();

    assert_eq!(runtime.players[&LOCAL_ID].data.position, stepped);
    assert_eq!(
        runtime.authority.session(LOCAL_ID).unwrap().position,
        stepped
    );
    runtime.shutdown().unwrap();
    let _ = fs::remove_dir_all(props.world_dir);
}

#[test]
fn alive_respawn_request_does_not_mutate_session() {
    let props = properties("alive-respawn");
    let (mut runtime, input) = ServerRuntime::new_embedded(
        props.clone(),
        EmbeddedRuntimeOptions::singleplayer(LocalSessionProfile::new(LOCAL_ID, "plan08-local")),
    )
    .unwrap();
    assert!(runtime.set_session_dimension(LOCAL_ID, Dimension::Nether));
    let pose = [32.5, 70.0, -16.5];
    let diamond = ItemStack::new(Item::Diamond, 3);
    let mut gameplay = SessionGameplayState::default();
    gameplay.inventory[0] = Some(slot(diamond));
    gameplay.health_milli = 12_000;
    gameplay.hunger_milli = 9_000;
    assert!(runtime.authority.set_session_gameplay(LOCAL_ID, gameplay));
    assert!(runtime.teleport_session(LOCAL_ID, pose));
    let before_inventory = runtime
        .authority
        .session(LOCAL_ID)
        .unwrap()
        .gameplay
        .inventory;
    let before_health = runtime
        .authority
        .session(LOCAL_ID)
        .unwrap()
        .gameplay
        .health_milli;

    input
        .try_send(ServerToHost::ClientRespawnRequest { id: LOCAL_ID })
        .unwrap();
    runtime.tick().unwrap();

    let player = runtime.players.get(&LOCAL_ID).unwrap();
    let authority = runtime.authority.session(LOCAL_ID).unwrap();
    assert_eq!(player.interest.dimension, Dimension::Nether);
    assert_eq!(player.data.position, pose);
    assert_eq!(authority.dimension, Dimension::Nether as u8);
    assert_eq!(authority.position, pose);
    assert_eq!(authority.gameplay.inventory, before_inventory);
    assert_eq!(authority.gameplay.health_milli, before_health);
    assert!(!authority.gameplay.is_dead);
    runtime.shutdown().unwrap();
    let _ = fs::remove_dir_all(props.world_dir);
}

#[test]
fn ignite_portal_brew_lock_does_not_place_fire() {
    let props = properties("ignite-brew-lock");
    let (mut runtime, _input) = ServerRuntime::new_embedded(
        props.clone(),
        EmbeddedRuntimeOptions::singleplayer(LocalSessionProfile::new(LOCAL_ID, "plan08-local")),
    )
    .unwrap();
    let fire = (11, 66, 10);
    runtime.authority.with_world(Dimension::Overworld, |world| {
        world
            .set_block(fire.0, fire.1 - 1, fire.2, BlockType::Obsidian, 0)
            .unwrap();
        world
            .set_block(fire.0, fire.1, fire.2, BlockType::Air, 0)
            .unwrap();
    });
    let flint = ItemStack::new(Item::FlintAndSteel, 1);
    let held_wire = held(&flint);
    let mut gameplay = SessionGameplayState::default();
    gameplay.inventory[0] = Some(slot(flint));
    gameplay.brew = Some(SessionBrewState {
        station: [8, 80, 8],
        ingredient: SlotRefWire {
            index: 0,
            count: 1,
            expected: held_wire,
        },
        bottles: [None; 3],
        remaining_ticks: 40,
    });
    assert!(runtime.authority.set_session_gameplay(LOCAL_ID, gameplay));
    if let Some(session) = runtime.players.get_mut(&LOCAL_ID) {
        session.data.position = [11.0, 65.0, 9.0];
    }
    if let Some(session) = runtime.authority.session_mut(LOCAL_ID) {
        session.position = [11.0, 65.0, 9.0];
    }
    let before = runtime.authority.session(LOCAL_ID).unwrap().gameplay;

    let ignite = gameplay_request(
        &runtime,
        LOCAL_ID,
        1,
        1,
        GameplayOperation::BlockAction {
            action: BlockActionKind::IgnitePortal,
            x: fire.0,
            y: fire.1,
            z: fire.2,
            face: [0, 1, 0],
            hand: 0,
            held: Some(held_wire),
            block: BlockType::Fire.to_wire(),
            look_milli: PORTAL_LOOK,
        },
    );
    let response = runtime
        .submit_request(LOCAL_ID, ignite)
        .expect("ignite ACK");
    assert!(matches!(
        response.outcome,
        GameplayOutcome::Rejected {
            reason: RejectReason::InvalidState
        }
    ));
    assert_eq!(
        runtime
            .authority
            .world_ref(Dimension::Overworld)
            .unwrap()
            .get_block(fire.0, fire.1, fire.2),
        BlockType::Air
    );
    assert!(runtime.authority.take_pending_mutations().is_empty());
    let after = runtime.authority.session(LOCAL_ID).unwrap().gameplay;
    assert_eq!(after.inventory, before.inventory);
    assert_eq!(after.brew, before.brew);
    runtime.shutdown().unwrap();
    let _ = fs::remove_dir_all(props.world_dir);
}
