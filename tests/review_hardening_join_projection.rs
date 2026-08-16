use icraft::chunk_manager::ChunkManager;
use icraft::dimension::Dimension;
use icraft::presentation_inventory_policy::{
    presentation_chunk_load_policy, schedule_presentation_chunk_load, MultiplayerRole,
    PresentationChunkLoadPolicy,
};
use icraft::world::{BlockType, Chunk};

fn join_client() -> MultiplayerRole {
    MultiplayerRole::Client {
        server_addr: "127.0.0.1".to_string(),
        port: 25565,
        username: "JOINER".to_string(),
    }
}

#[test]
fn schedule_chunk_load_does_not_insert_generated_column_for_join_client() {
    let role = join_client();
    assert_eq!(
        presentation_chunk_load_policy(&role),
        PresentationChunkLoadPolicy::AwaitAuthoritativePayload
    );

    let mut manager = ChunkManager::new_in_dimension(2, Dimension::Overworld);
    let mut generated = false;
    let loaded = schedule_presentation_chunk_load(presentation_chunk_load_policy(&role), || {
        generated = true;
        icraft::dimension::generate_chunk_with_options(
            Dimension::Overworld,
            0,
            0,
            0xC0FFEE,
            icraft::dimension::WorldGenerationOptions::default(),
        )
    });
    if let Some(chunk) = loaded {
        manager.chunks.insert((0, 0), chunk);
    }

    assert!(!generated, "join client must not call worldgen");
    assert!(
        !manager.chunks.contains_key(&(0, 0)),
        "missing chunk must stay unloaded, not fake terrain"
    );
}

#[test]
fn chunk_data_inserts_column_matching_payload_without_prior_worldgen() {
    let role = join_client();
    let mut source = Chunk::empty_in_dimension(Dimension::Overworld, 3, -1);
    source.set_block_local(4, 70, 5, BlockType::Stone);
    source.set_block_local(0, 64, 0, BlockType::DiamondOre);
    source.set_block_local(15, -60, 15, BlockType::Bedrock);
    let payload = icraft::save::ChunkSaveData::from_chunk(&source).expect("compress payload");

    let mut manager = ChunkManager::new_in_dimension(2, Dimension::Overworld);
    let scheduled = schedule_presentation_chunk_load(presentation_chunk_load_policy(&role), || {
        icraft::dimension::generate_chunk_with_options(
            Dimension::Overworld,
            3,
            -1,
            1,
            icraft::dimension::WorldGenerationOptions::default(),
        )
    });
    assert!(
        scheduled.is_none(),
        "schedule before ChunkData must not produce a column"
    );
    assert!(!manager.chunks.contains_key(&(3, -1)));

    manager
        .insert_authoritative_chunk_payload(
            3,
            -1,
            &payload.blocks,
            &payload.block_states,
            &payload.fluid_levels,
            &payload.block_entities,
        )
        .expect("payload restore");

    let chunk = manager
        .chunks
        .get(&(3, -1))
        .expect("ChunkData must insert the column");
    assert_eq!(chunk.get_block_local(4, 70, 5), BlockType::Stone);
    assert_eq!(chunk.get_block_local(0, 64, 0), BlockType::DiamondOre);
    assert_eq!(chunk.get_block_local(15, -60, 15), BlockType::Bedrock);
    assert_eq!(
        chunk.get_block_local(8, 80, 8),
        BlockType::Air,
        "unspecified voxels stay empty, not locally generated"
    );
}

#[test]
fn join_client_must_not_mutate_presentation_chunks() {
    assert_eq!(
        presentation_chunk_load_policy(&join_client()),
        PresentationChunkLoadPolicy::AwaitAuthoritativePayload
    );
    assert_eq!(
        presentation_chunk_load_policy(&MultiplayerRole::Singleplayer),
        PresentationChunkLoadPolicy::GenerateLocally
    );
    assert_eq!(
        presentation_chunk_load_policy(&MultiplayerRole::Host { port: 25565 }),
        PresentationChunkLoadPolicy::GenerateLocally
    );
}
