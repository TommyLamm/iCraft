use icraft::chunk_manager::PresentationChunks;
use icraft::dimension::Dimension;
use icraft::presentation_inventory_policy::{MultiplayerRole, PresentationTopology};
use icraft::world::{BlockType, Chunk};

fn join_client() -> MultiplayerRole {
    MultiplayerRole::Client {
        server_addr: "127.0.0.1".to_string(),
        port: 25565,
        username: "JOINER".to_string(),
    }
}

#[test]
fn join_client_presentation_chunks_stay_unloaded_without_authoritative_payload() {
    let role = join_client();
    let topology = PresentationTopology::from(&role, false);
    assert!(topology.is_join_client());

    let manager = PresentationChunks::new(2);
    assert!(
        !manager.chunks.contains_key(&(0, 0)),
        "missing chunk must stay unloaded, not fake terrain"
    );
    assert_eq!(manager.chunks.len(), 0);
}

#[test]
fn chunk_data_inserts_column_matching_payload_without_prior_worldgen() {
    let mut source = Chunk::empty_in_dimension(Dimension::Overworld, 3, -1);
    source.set_block_local(4, 70, 5, BlockType::Stone);
    source.set_block_local(0, 64, 0, BlockType::DiamondOre);
    source.set_block_local(15, -60, 15, BlockType::Bedrock);
    let payload = icraft::save::ChunkSaveData::from_chunk(&source).expect("compress payload");

    let mut manager = PresentationChunks::new(2);
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
fn embedded_presentation_receives_authoritative_chunk_projection() {
    let role = MultiplayerRole::Singleplayer;
    let topology = PresentationTopology::from(&role, true);
    assert!(topology.is_embedded());

    let mut manager = PresentationChunks::new(2);
    assert!(!manager.chunks.contains_key(&(1, 2)));

    let mut chunk = Chunk::empty_in_dimension(Dimension::Overworld, 1, 2);
    chunk.set_block_local(2, 65, 2, BlockType::OakPlanks);
    manager.insert_resident_chunk((1, 2), chunk);

    let resident = manager
        .chunks
        .get(&(1, 2))
        .expect("embedded projected chunk must be present");
    assert_eq!(resident.get_block_local(2, 65, 2), BlockType::OakPlanks);
}
