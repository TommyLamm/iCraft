use glam::Vec3;
use icraft::authority::contract::AuthorityTopology;
use icraft::dimension::Dimension;
use icraft::inventory::{GameMode, Inventory};
use icraft::network::protocol::{
    GameplayOperation, GameplayOutcome, GameplayRequest, GameplayResponse,
};
use icraft::server_runtime::{
    EmbeddedRuntimeOptions, LocalSessionProfile, LocalSessionStorage, RuntimePresentationEvent,
    ServerProperties, ServerRuntime, TransportMode,
};
use icraft::{player::PlayerState, save::LevelData, save::PlayerData, save::SaveManager};
use std::fs;
use std::net::TcpListener;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_world(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!("icraft_runtime_topology_{label}_{unique}"))
}

fn available_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("reserve an ephemeral test port");
    listener.local_addr().unwrap().port()
}

fn properties(label: &str) -> ServerProperties {
    ServerProperties {
        bind: "127.0.0.1".into(),
        port: available_port(),
        world_dir: temp_world(label),
        ..ServerProperties::default()
    }
}

fn block_request(session_id: u64, client_revision: u64, request_id: u128) -> GameplayRequest {
    GameplayRequest {
        request_id,
        client_sequence: 1,
        session_id,
        dimension: Dimension::Overworld as u8,
        client_revision,
        operation: GameplayOperation::BlockUse {
            x: 8,
            y: 80,
            z: 8,
            block: 1,
        },
    }
}

fn response_for(
    events: &[RuntimePresentationEvent],
    target: u64,
    request_id: u128,
) -> Option<&GameplayResponse> {
    events.iter().find_map(|event| match event {
        RuntimePresentationEvent::GameplayResponse {
            target: event_target,
            response,
        } if *event_target == target && response.request_id == request_id => Some(response),
        _ => None,
    })
}

#[test]
fn disabled_singleplayer_drains_local_request_through_fixed_tick_fifo() {
    let properties = properties("singleplayer");
    let world_dir = properties.world_dir.clone();
    let local_id = u64::MAX - 1;
    let (mut runtime, input) = ServerRuntime::new_embedded(
        properties,
        EmbeddedRuntimeOptions::singleplayer(LocalSessionProfile::new(local_id, "local")),
    )
    .unwrap();

    assert_eq!(runtime.authority.topology, AuthorityTopology::Singleplayer);
    assert_eq!(runtime.transport_mode(), TransportMode::Disabled);
    let revision = runtime
        .authority
        .revision_for_dimension(Dimension::Overworld);
    input
        .submit_request(local_id, block_request(local_id, revision, 41))
        .unwrap();

    // Publication is queued: authority state cannot change before the fixed
    // tick consumes the same bounded FIFO used by listen transport events.
    assert_ne!(runtime.authority.world.get_block(8, 80, 8).to_wire(), 1);
    let output = runtime.tick_with_output().unwrap();
    let response = response_for(&output.presentation_events, local_id, 41).unwrap();
    assert!(matches!(response.outcome, GameplayOutcome::Accepted { .. }));
    assert!(output
        .snapshot
        .mutations
        .iter()
        .any(|mutation| mutation.position == (8, 80, 8) && mutation.block == 1));
    assert_eq!(runtime.authority.world.get_block(8, 80, 8).to_wire(), 1);
    assert_eq!(runtime.metrics().queue_depth, 0);
    assert_eq!(runtime.metrics().queue_full, 0);
    assert_eq!(runtime.metrics().outbound_packets, 0);

    runtime.shutdown().unwrap();
    drop(runtime);
    let _ = fs::remove_dir_all(world_dir);
}

#[test]
fn listen_runtime_routes_local_response_to_tick_output() {
    let properties = properties("listen");
    let world_dir = properties.world_dir.clone();
    let local_id = u64::MAX - 2;
    let (mut runtime, input) = ServerRuntime::new_embedded(
        properties,
        EmbeddedRuntimeOptions::listen(LocalSessionProfile::new(local_id, "host")),
    )
    .unwrap();

    assert_eq!(runtime.authority.topology, AuthorityTopology::ListenServer);
    assert_eq!(runtime.transport_mode(), TransportMode::Listen);
    let revision = runtime
        .authority
        .revision_for_dimension(Dimension::Overworld);
    input
        .submit_request(local_id, block_request(local_id, revision, 42))
        .unwrap();
    let output = runtime.tick_with_output().unwrap();
    assert!(response_for(&output.presentation_events, local_id, 42)
        .is_some_and(|response| matches!(response.outcome, GameplayOutcome::Accepted { .. })));
    assert!(output
        .snapshot
        .mutations
        .iter()
        .any(|mutation| mutation.position == (8, 80, 8)));

    runtime.shutdown().unwrap();
    drop(runtime);
    let _ = fs::remove_dir_all(world_dir);
}

#[test]
fn legacy_constructor_remains_dedicated_listen_runtime() {
    let properties = properties("dedicated");
    let world_dir = properties.world_dir.clone();
    let mut runtime = ServerRuntime::new(properties).unwrap();
    assert_eq!(runtime.authority.topology, AuthorityTopology::Dedicated);
    assert_eq!(runtime.transport_mode(), TransportMode::Listen);

    runtime.tick().unwrap();
    let output = runtime.tick_with_output().unwrap();
    assert_eq!(runtime.metrics().ticks, 2);
    assert_eq!(output.snapshot.tick, 2);
    assert!(output.presentation_events.is_empty());

    runtime.shutdown().unwrap();
    drop(runtime);
    let _ = fs::remove_dir_all(world_dir);
}

#[test]
fn world_player_storage_loads_and_rewrites_legacy_player_dat() {
    let properties = properties("world_player_restart");
    let world_dir = properties.world_dir.clone();
    let manager = SaveManager::new(&world_dir);
    let mut level = LevelData::default();
    level.seed = 0x51A9;
    let player = PlayerData::from_state(
        Vec3::new(13.0, 72.0, -9.0),
        Vec3::ZERO,
        0.25,
        -0.5,
        &PlayerState::new(),
        GameMode::Creative,
        &Inventory::new(),
        Default::default(),
    );
    manager.save_player_and_level(&level, &player).unwrap();
    manager.save_current_dimension(Dimension::Nether).unwrap();

    let local_id = u64::MAX - 3;
    let (mut runtime, _input) = ServerRuntime::new_embedded(
        properties.clone(),
        EmbeddedRuntimeOptions::singleplayer(LocalSessionProfile::new(local_id, "legacy")),
    )
    .unwrap();
    let session = &runtime.players[&local_id];
    assert_eq!(session.storage, LocalSessionStorage::WorldPlayer);
    assert_eq!(session.dimension, Dimension::Nether);
    assert_eq!(session.data.position, [13.0, 72.0, -9.0]);
    assert_eq!(
        runtime.authority.session(local_id).unwrap().game_mode,
        GameMode::Creative
    );

    assert!(runtime.set_session_dimension(local_id, Dimension::End));
    runtime
        .authority
        .session_mut(local_id)
        .unwrap()
        .gameplay
        .health_milli = 4_321;
    runtime.shutdown().unwrap();
    drop(runtime);

    let restart_id = u64::MAX - 4;
    let (mut restored, _input) = ServerRuntime::new_embedded(
        properties,
        EmbeddedRuntimeOptions::singleplayer(LocalSessionProfile::new(restart_id, "legacy")),
    )
    .unwrap();
    assert_eq!(restored.players[&restart_id].dimension, Dimension::End);
    assert_eq!(
        restored
            .authority
            .session(restart_id)
            .unwrap()
            .gameplay
            .health_milli,
        4_321
    );
    assert!(world_dir.join("player.dat").is_file());
    assert!(!world_dir.join("players").join("legacy.dat").exists());

    restored.shutdown().unwrap();
    drop(restored);
    let _ = fs::remove_dir_all(world_dir);
}
