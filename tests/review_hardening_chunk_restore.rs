//! Plan 05: inner-zlib restore must fail closed and never generate-over-save.

use icraft::authority::contract::AuthorityTopology;
use icraft::dimension::Dimension;
use icraft::save::{
    compress_bytes, ChunkSaveData, RegionData, SaveError, SaveManager, CHUNK_SAVE_DATA_VERSION,
};
use icraft::server_runtime::{
    EmbeddedRuntimeOptions, ServerProperties, ServerRuntime, TransportMode,
};
use icraft::world::{BlockType, Chunk};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_dir(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "icraft_plan05_{label}_{}_{}",
        std::process::id(),
        unique
    ))
}

fn region_path(world_dir: &Path) -> PathBuf {
    world_dir.join("regions").join("r.0.0.bin")
}

fn region_chunk_payload(path: &Path, lx: u8, lz: u8) -> Vec<u8> {
    let region: RegionData = bincode::deserialize(&fs::read(path).unwrap()).unwrap();
    region.chunks.get(&(lx, lz)).cloned().unwrap()
}

fn overwrite_region_chunk_payload(path: &Path, lx: u8, lz: u8, payload: Vec<u8>) {
    let mut region: RegionData = bincode::deserialize(&fs::read(path).unwrap()).unwrap();
    region.chunks.insert((lx, lz), payload);
    fs::write(path, bincode::serialize(&region).unwrap()).unwrap();
}

fn decode_chunk_save(bytes: &[u8]) -> ChunkSaveData {
    bincode::deserialize(bytes).unwrap()
}

fn dedicated_runtime(world_dir: PathBuf) -> ServerRuntime {
    let mut properties = ServerProperties::default();
    properties.bind = "127.0.0.1".into();
    properties.port = 25580;
    properties.view_distance = 2;
    properties.simulation_distance = 2;
    properties.world_dir = world_dir;
    properties.seed = 12345;
    ServerRuntime::new_embedded(
        properties,
        EmbeddedRuntimeOptions {
            topology: AuthorityTopology::Dedicated,
            transport: TransportMode::Disabled,
            local_session: None,
        },
    )
    .unwrap()
    .0
}

fn save_player_modified_spawn(world_dir: &Path) -> ChunkSaveData {
    let mut chunk = Chunk::new_with_seed(0, 0, 12345);
    chunk.set_block_local(8, 80, 8, BlockType::DiamondOre);
    let mut data = ChunkSaveData::from_chunk(&chunk).unwrap();
    data.mutation_revision = 9;
    let mut manager = SaveManager::new(world_dir);
    manager.save_chunk(0, 0, data.clone()).unwrap();
    data
}

#[test]
fn legal_region_envelope_with_empty_or_truncated_zlib_is_restore_error() {
    let world_dir = temp_dir("empty_zlib");
    save_player_modified_spawn(&world_dir);
    let path = region_path(&world_dir);
    let original = region_chunk_payload(&path, 0, 0);

    let mut empty = decode_chunk_save(&original);
    empty.blocks.clear();
    overwrite_region_chunk_payload(&path, 0, 0, bincode::serialize(&empty).unwrap());
    let loaded = SaveManager::new(&world_dir).load_chunk(0, 0).unwrap();
    assert_eq!(loaded.data_version, CHUNK_SAVE_DATA_VERSION);
    assert!(loaded.restore_to_chunk(&mut Chunk::empty(0, 0)).is_err());

    let mut truncated = decode_chunk_save(&original);
    truncated.blocks.truncate(3);
    overwrite_region_chunk_payload(&path, 0, 0, bincode::serialize(&truncated).unwrap());
    let loaded = SaveManager::new(&world_dir).load_chunk(0, 0).unwrap();
    assert!(loaded.restore_to_chunk(&mut Chunk::empty(0, 0)).is_err());

    fs::remove_dir_all(world_dir).unwrap();
}

#[test]
fn save_all_does_not_replace_empty_inner_zlib_with_generated_terrain() {
    let world_dir = temp_dir("save_all_empty");
    save_player_modified_spawn(&world_dir);
    let path = region_path(&world_dir);
    let mut corrupt = decode_chunk_save(&region_chunk_payload(&path, 0, 0));
    corrupt.blocks.clear();
    let corrupt_payload = bincode::serialize(&corrupt).unwrap();
    overwrite_region_chunk_payload(&path, 0, 0, corrupt_payload.clone());

    let mut runtime = dedicated_runtime(world_dir.clone());
    assert!(
        !runtime
            .authority
            .world_mut_active()
            .chunks
            .chunks
            .contains_key(&(0, 0)),
        "failed restore must not insert the column"
    );
    assert!(runtime
        .authority
        .world_mut_active()
        .failed_restore_chunks()
        .contains(&(0, 0)));

    runtime.save_all().unwrap();
    drop(runtime);

    assert_eq!(
        region_chunk_payload(&path, 0, 0),
        corrupt_payload,
        "save_all must not write generated terrain over a failed restore"
    );
    fs::remove_dir_all(world_dir).unwrap();
}

#[test]
fn player_modified_chunk_with_corrupt_inner_zlib_is_not_rewritten_as_generated() {
    let world_dir = temp_dir("player_corrupt");
    let saved = save_player_modified_spawn(&world_dir);
    assert_ne!(saved.blocks, compress_bytes(&[]).unwrap());

    let path = region_path(&world_dir);
    let mut corrupt = decode_chunk_save(&region_chunk_payload(&path, 0, 0));
    corrupt.blocks = vec![0x78, 0x01, 0xff];
    let corrupt_payload = bincode::serialize(&corrupt).unwrap();
    overwrite_region_chunk_payload(&path, 0, 0, corrupt_payload.clone());

    let mut runtime = dedicated_runtime(world_dir.clone());
    runtime.authority.world_mut_active().ensure_chunk(0, 0);
    assert!(!runtime
        .authority
        .world_mut_active()
        .chunks
        .chunks
        .contains_key(&(0, 0)));
    runtime.save_all().unwrap();
    drop(runtime);

    assert_eq!(region_chunk_payload(&path, 0, 0), corrupt_payload);
    let reloaded = SaveManager::new(&world_dir).load_chunk(0, 0).unwrap();
    assert!(reloaded.restore_to_chunk(&mut Chunk::empty(0, 0)).is_err());
    fs::remove_dir_all(world_dir).unwrap();
}

#[test]
fn corrupt_region_container_is_still_not_overwritten() {
    let world_dir = temp_dir("region_container");
    let mut manager = SaveManager::new(&world_dir);
    let path = region_path(&world_dir);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let corrupt = b"not a bincode region";
    fs::write(&path, corrupt).unwrap();

    let error = manager
        .save_chunk(0, 0, ChunkSaveData::from_chunk(&Chunk::new(0, 0)).unwrap())
        .unwrap_err();
    assert!(matches!(error, SaveError::RegionCorruption { .. }));
    assert_eq!(fs::read(&path).unwrap(), corrupt);
    fs::remove_dir_all(world_dir).unwrap();
}

#[test]
fn nether_height_mismatch_is_restore_error() {
    let mut overworld = Chunk::empty(1, 2);
    overworld.set_block_local(1, 70, 1, BlockType::Stone);
    let data = ChunkSaveData::from_chunk(&overworld).unwrap();
    let mut nether = Chunk::empty_in_dimension(Dimension::Nether, 1, 2);
    assert!(data.restore_to_chunk(&mut nether).is_err());
}
