//! Plan 11: interest is the chunk materialization gate; out-of-view columns
//! are not ensured, tick walks the simulation set, and eviction is bounded.

use icraft::authority::contract::AuthorityTopology;
use icraft::dimension::Dimension;
use icraft::network::protocol::{
    BlockActionKind, GameplayOperation, GameplayOutcome, GameplayRequest,
};
use icraft::save::SaveManager;
use icraft::server_runtime::{
    EmbeddedRuntimeOptions, LocalSessionProfile, ServerProperties, ServerRuntime, TransportMode,
};
use icraft::world::BlockType;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const LOCAL_ID: u64 = 11;
const FAR_CHUNK: (i32, i32) = (8, 0);
const FAR_BLOCK: (i32, i32, i32) = (128, 80, 8);
const WALK_CHUNKS: i32 = 32;

fn temp_world(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "icraft_plan11_{label}_{}_{}",
        std::process::id(),
        nonce
    ))
}

fn properties(label: &str) -> ServerProperties {
    ServerProperties {
        bind: "127.0.0.1".into(),
        port: 25582,
        view_distance: 2,
        simulation_distance: 2,
        seed: 11_011,
        world_dir: temp_world(label),
        ..ServerProperties::default()
    }
}

fn embedded(label: &str) -> ServerRuntime {
    let props = properties(label);
    ServerRuntime::new_embedded(
        props,
        EmbeddedRuntimeOptions::singleplayer(LocalSessionProfile::new(LOCAL_ID, "plan11-local")),
    )
    .unwrap()
    .0
}

fn request(
    runtime: &ServerRuntime,
    request_id: u128,
    sequence: u64,
    operation: GameplayOperation,
) -> GameplayRequest {
    let dimension = runtime
        .authority
        .session(LOCAL_ID)
        .and_then(|session| Dimension::from_wire(session.dimension))
        .expect("plan11 session dimension");
    GameplayRequest {
        request_id,
        client_sequence: sequence,
        session_id: LOCAL_ID,
        dimension: dimension as u8,
        client_revision: runtime.authority.revision_for_dimension(dimension),
        operation,
    }
}

fn loaded_len(runtime: &ServerRuntime) -> usize {
    runtime
        .authority
        .dimensions()
        .into_iter()
        .filter_map(|dimension| runtime.authority.world_ref(dimension))
        .map(|world| world.chunks.chunks.len())
        .sum()
}

fn overworld_has(runtime: &ServerRuntime, cx: i32, cz: i32) -> bool {
    runtime
        .authority
        .world_ref(Dimension::Overworld)
        .is_some_and(|world| world.chunks.chunks.contains_key(&(cx, cz)))
}

#[test]
fn out_of_view_chunk_is_not_materialized_and_block_action_does_not_ensure() {
    let mut runtime = embedded("out_of_view");
    runtime.run_for_ticks(16).unwrap();
    assert!(
        !overworld_has(&runtime, FAR_CHUNK.0, FAR_CHUNK.1),
        "chunk (8,0) must stay out of residency at origin view=2"
    );

    let before = overworld_has(&runtime, FAR_CHUNK.0, FAR_CHUNK.1);
    let response = runtime
        .submit_request(
            LOCAL_ID,
            request(
                &runtime,
                1,
                1,
                GameplayOperation::BlockAction {
                    action: BlockActionKind::StartBreak,
                    x: FAR_BLOCK.0,
                    y: FAR_BLOCK.1,
                    z: FAR_BLOCK.2,
                    face: [0, 1, 0],
                    hand: 0,
                    held: None,
                    block: BlockType::Stone.to_wire(),
                    look_milli: [0, 0, 1000],
                },
            ),
        )
        .expect("block action response");
    assert!(matches!(response.outcome, GameplayOutcome::Rejected { .. }));
    assert_eq!(overworld_has(&runtime, FAR_CHUNK.0, FAR_CHUNK.1), before);
    assert!(!overworld_has(&runtime, FAR_CHUNK.0, FAR_CHUNK.1));
    assert_eq!(runtime.metrics.loaded_chunks, loaded_len(&runtime));

    let _ = runtime.shutdown();
    let _ = fs::remove_dir_all(&runtime.properties.world_dir);
}

#[test]
fn walking_32_chunks_evicts_origin_after_flushing_dirty() {
    let mut runtime = embedded("walk_origin");
    runtime.run_for_ticks(8).unwrap();
    assert!(overworld_has(&runtime, 0, 0));
    runtime.authority.with_world(Dimension::Overworld, |world| {
        world.set_block(8, 80, 8, BlockType::DiamondOre, 0).unwrap();
    });
    assert!(runtime.teleport_session(
        LOCAL_ID,
        [f32::from(WALK_CHUNKS as i16) * 16.0 + 8.0, 80.0, 8.0]
    ));
    runtime.tick().unwrap();
    assert!(
        !overworld_has(&runtime, 0, 0),
        "origin must leave residency after the next evict"
    );
    assert_eq!(runtime.metrics.loaded_chunks, loaded_len(&runtime));

    let mut saves = SaveManager::new(&runtime.properties.world_dir);
    let saved = saves
        .load_chunk(0, 0)
        .expect("dirty origin must be flushed before evict");
    let mut restored = icraft::world::Chunk::empty_in_dimension(Dimension::Overworld, 0, 0);
    saved.restore_to_chunk(&mut restored).unwrap();
    assert_eq!(restored.get_block(8, 80, 8), BlockType::DiamondOre);

    let _ = runtime.shutdown();
    let _ = fs::remove_dir_all(&runtime.properties.world_dir);
}

#[test]
fn portal_linked_columns_are_evict_candidates_when_nobody_is_present() {
    let props = properties("portal_evict");
    let (mut runtime, _input) = ServerRuntime::new_embedded(
        props.clone(),
        EmbeddedRuntimeOptions::singleplayer(LocalSessionProfile::new(LOCAL_ID, "plan11-portal")),
    )
    .unwrap();

    runtime.authority.with_world(Dimension::Overworld, |world| {
        for x in 10..=13 {
            world.set_block(x, 65, 10, BlockType::Obsidian, 0).unwrap();
            world.set_block(x, 69, 10, BlockType::Obsidian, 0).unwrap();
        }
        for y in 66..=68 {
            world.set_block(10, y, 10, BlockType::Obsidian, 0).unwrap();
            world.set_block(13, y, 10, BlockType::Obsidian, 0).unwrap();
            world
                .set_block(11, y, 10, BlockType::NetherPortal, 0)
                .unwrap();
            world
                .set_block(12, y, 10, BlockType::NetherPortal, 0)
                .unwrap();
        }
    });
    assert!(runtime.teleport_session(LOCAL_ID, [11.5, 66.0, 10.5]));
    let enter = request(
        &runtime,
        1,
        1,
        GameplayOperation::BlockAction {
            action: BlockActionKind::EnterPortal,
            x: 11,
            y: 66,
            z: 10,
            face: [0, 0, 0],
            hand: 0,
            held: None,
            block: BlockType::NetherPortal.to_wire(),
            look_milli: [0, -500, 866],
        },
    );
    runtime.submit_request(LOCAL_ID, enter).unwrap();

    let mut transferred = false;
    for _ in 0..30 {
        runtime.tick().unwrap();
        if runtime
            .authority
            .session(LOCAL_ID)
            .is_some_and(|session| session.dimension == Dimension::Nether as u8)
        {
            transferred = true;
            break;
        }
    }
    assert!(transferred, "nether portal linking must run");

    let linked: Vec<(i32, i32)> = runtime
        .authority
        .world_ref(Dimension::Nether)
        .map(|world| world.chunks.chunks.keys().copied().collect())
        .unwrap_or_default();
    assert!(
        !linked.is_empty(),
        "portal linking is allowed to ensure destination columns"
    );

    let nether_pos = runtime
        .authority
        .session(LOCAL_ID)
        .map(|session| session.position)
        .expect("nether session");
    let far = [nether_pos[0] + 32.0 * 16.0, nether_pos[1], nether_pos[2]];
    assert!(runtime.teleport_session(LOCAL_ID, far));
    runtime.tick().unwrap();

    let nether = runtime.authority.world_ref(Dimension::Nether).unwrap();
    for (cx, cz) in linked {
        if (cx - (far[0] / 16.0).floor() as i32).abs() > 4
            || (cz - (far[2] / 16.0).floor() as i32).abs() > 4
        {
            assert!(
                !nether.chunks.chunks.contains_key(&(cx, cz)),
                "linked column ({cx}, {cz}) must evict when nobody is present"
            );
        }
    }
    assert_eq!(runtime.metrics.loaded_chunks, loaded_len(&runtime));

    let _ = runtime.shutdown();
    let _ = fs::remove_dir_all(props.world_dir);
}

#[test]
fn dedicated_without_sessions_caps_spawn_ring() {
    let props = properties("spawn_cap");
    let (mut runtime, _input) = ServerRuntime::new_embedded(
        props.clone(),
        EmbeddedRuntimeOptions {
            topology: AuthorityTopology::Dedicated,
            transport: TransportMode::Disabled,
            local_session: None,
        },
    )
    .unwrap();
    runtime.authority.with_world(Dimension::Overworld, |world| {
        for cx in -6..=6 {
            world.ensure_chunk(cx, 0);
        }
    });
    runtime.tick().unwrap();
    let world = runtime.authority.world_ref(Dimension::Overworld).unwrap();
    assert!(world.chunks.chunks.contains_key(&(0, 0)));
    assert!(!world.chunks.chunks.contains_key(&(8, 0)));
    assert!(world.chunks.chunks.len() <= icraft::authority::interest::SPAWN_RESIDENCY_CAP);
    assert_eq!(runtime.metrics.loaded_chunks, loaded_len(&runtime));

    let _ = runtime.shutdown();
    let _ = fs::remove_dir_all(props.world_dir);
}
