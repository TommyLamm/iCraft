use glam::Vec3;
use icraft::authority::interest::{InterestKind, InterestSet};
use icraft::dimension::Dimension;
use icraft::entity::EntityManager;
use icraft::inventory::{GameMode, Inventory};
use icraft::network::protocol::{
    GameplayOperation, GameplayOutcome, GameplayRequest, PlayerEffectWire,
};
use icraft::save::{ChunkSaveData, EntitySaveData, PlayerData, SaveManager};
use icraft::server_runtime::{ServerProperties, ServerRuntime};
use icraft::world::{BlockType, Chunk};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_dir(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!("icraft_authority_c_{label}_{unique}"))
}

fn player_data() -> PlayerData {
    let state = icraft::player::PlayerState::new();
    PlayerData::from_state(
        Vec3::new(12.0, 70.0, -4.0),
        Vec3::new(0.1, 0.0, -0.2),
        0.5,
        -0.25,
        &state,
        GameMode::Survival,
        &Inventory::new(),
        Default::default(),
    )
}

#[test]
fn dedicated_player_file_roundtrips_current_dimension_and_effects() {
    let world_dir = temp_dir("player");
    let manager = SaveManager::new(&world_dir);
    let mut data = player_data();
    data.health = 7.5;
    data.spawn_dimension = Some(Dimension::Overworld);
    let effects = vec![PlayerEffectWire {
        kind: 3,
        level: 2,
        remaining_seconds: 17.0,
    }];
    manager
        .save_dedicated_player("Alice", Dimension::Nether, &data, &effects)
        .unwrap();
    let loaded = manager
        .load_dedicated_player("alice")
        .unwrap()
        .expect("dedicated player should exist");
    assert_eq!(loaded.current_dimension, Dimension::Nether);
    assert_eq!(loaded.data.spawn_dimension, Some(Dimension::Overworld));
    assert_eq!(loaded.data.health, 7.5);
    assert_eq!(loaded.effects, effects);
    assert!(manager
        .dedicated_player_file_path("Alice/../Alice")
        .starts_with(world_dir.join("players")));
    fs::remove_dir_all(world_dir).unwrap();
}

#[test]
fn authoritative_chunks_and_entities_roundtrip_with_revisions() {
    let world_dir = temp_dir("world");
    let mut manager = SaveManager::new(&world_dir);
    let mut chunk = Chunk::new(2, -1);
    chunk.set_block_local(1, 70, 1, BlockType::Brick);
    let mut data = ChunkSaveData::from_chunk(&chunk);
    data.mutation_revision = 37;
    manager
        .save_chunk_in(Dimension::Overworld, 2, -1, data)
        .unwrap();
    let saved_chunks = manager.load_saved_chunks_in(Dimension::Overworld).unwrap();
    assert_eq!(saved_chunks.len(), 1);
    assert_eq!(saved_chunks[0].mutation_revision, 37);
    let mut restored = Chunk::new(2, -1);
    saved_chunks[0].restore_to_chunk(&mut restored);
    assert_eq!(restored.get_block_local(1, 70, 1), BlockType::Brick);

    let mut entities = EntityManager::new();
    let entity_id = entities.spawn(icraft::entity::EntityType::Pig, Vec3::new(4.0, 65.0, 5.0));
    let entity = entities.get_by_id(entity_id).unwrap();
    manager
        .save_entities_in(Dimension::Overworld, &[EntitySaveData::from(entity)])
        .unwrap();
    let loaded_entities = manager
        .load_entities_in_checked(Dimension::Overworld)
        .unwrap();
    assert_eq!(loaded_entities.len(), 1);
    assert_eq!(loaded_entities[0].position, [4.0, 65.0, 5.0]);
    fs::remove_dir_all(world_dir).unwrap();
}

#[test]
fn runtime_reconnects_dimension_and_routes_without_cross_dimension_leak() {
    let world_dir = temp_dir("runtime");
    let mut properties = ServerProperties::default();
    properties.bind = "127.0.0.1".into();
    properties.port = 26000 + (std::process::id() as u16 % 500);
    properties.world_dir = world_dir.clone();
    properties.view_distance = 4;
    properties.simulation_distance = 2;
    let mut runtime = ServerRuntime::new(properties.clone()).unwrap();
    runtime.login_session(1, "Alice").unwrap();
    assert!(runtime.login_session(2, "alice").is_err());
    runtime.set_session_dimension(1, Dimension::Nether);
    runtime.players.get_mut(&1).unwrap().data.health = 6.0;
    runtime
        .players
        .get_mut(&1)
        .unwrap()
        .effects
        .push(PlayerEffectWire {
            kind: 1,
            level: 1,
            remaining_seconds: 4.0,
        });
    runtime.logout_session(1).unwrap();
    runtime.shutdown().unwrap();

    let mut restarted = ServerRuntime::new(properties).unwrap();
    restarted.login_session(9, "ALICE").unwrap();
    let session = restarted.players.get(&9).unwrap();
    assert_eq!(session.dimension, Dimension::Nether);
    assert_eq!(session.data.health, 6.0);
    assert_eq!(session.effects.len(), 1);

    // Put a second identity in another dimension. A block update from Alice
    // must route only to the matching dimension's interest set.
    restarted.set_session_dimension(9, Dimension::Overworld);
    restarted.login_session(10, "Bob").unwrap();
    restarted.set_session_dimension(10, Dimension::End);
    let _ = restarted.drain_routed_updates();
    let old = restarted.authority.world.get_block(8, 80, 8);
    let new_block = if old == BlockType::Air {
        BlockType::Stone
    } else {
        BlockType::Air
    };
    let response = restarted
        .submit_request(
            9,
            GameplayRequest {
                request_id: 900,
                client_sequence: 1,
                session_id: 9,
                dimension: Dimension::Overworld as u8,
                client_revision: restarted.authority.current_revision(),
                operation: GameplayOperation::BlockUse {
                    x: 8,
                    y: 80,
                    z: 8,
                    block: new_block.to_wire(),
                },
            },
        )
        .unwrap();
    assert!(matches!(response.outcome, GameplayOutcome::Accepted { .. }));
    let updates = restarted.drain_routed_updates();
    assert!(updates.iter().all(|update| update.target == 9));
    assert!(updates
        .iter()
        .all(|update| update.dimension == Dimension::Overworld));

    let mut overworld_interest = InterestSet::new(Dimension::Overworld, 4, 2);
    overworld_interest.update_position(Dimension::Overworld, [0.0, 64.0, 0.0]);
    assert!(!overworld_interest.wants(Dimension::Nether, InterestKind::Block((0, 64, 0)),));
    restarted.shutdown().unwrap();
    fs::remove_dir_all(world_dir).unwrap();
}
