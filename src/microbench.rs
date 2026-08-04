//! Deterministic storage/engine microbenchmarks.
//!
//! Run with `cargo test --release microbench -- --ignored --nocapture`.
//! Each JSON line contains a stable operation name, iteration count, elapsed
//! nanoseconds per operation, and a checksum of the observed results.

use std::hint::black_box;
use std::time::Instant;

use glam::Vec3;

use crate::chunk_manager::ChunkManager;
use crate::network::protocol::Packet;
use crate::physics::PlayerPhysics;
use crate::save::ChunkSaveData;
use crate::world::{BlockStorage, BlockType, Chunk, LightStorage, SectionKey};

const WARMUP: usize = 64;
const ITERS: usize = 64;

fn report(name: &str, iters: usize, elapsed: u128, checksum: u64) {
    println!(
        "{{\"name\":\"{name}\",\"iterations\":{iters},\"ns_per_op\":{:.2},\"checksum\":{checksum}}}",
        elapsed as f64 / iters as f64
    );
}

const BENCH_BLOCKS: [BlockType; 17] = [
    BlockType::Air,
    BlockType::Stone,
    BlockType::Dirt,
    BlockType::Grass,
    BlockType::Sand,
    BlockType::Gravel,
    BlockType::Bedrock,
    BlockType::OakLog,
    BlockType::OakLeaves,
    BlockType::Glass,
    BlockType::Water,
    BlockType::Lava,
    BlockType::Brick,
    BlockType::TNT,
    BlockType::Bookshelf,
    BlockType::Obsidian,
    BlockType::CoalOre,
];

fn storage_fixture(unique: usize) -> BlockStorage {
    let mut dense = [BENCH_BLOCKS[0]; 4096];
    for (i, block) in dense.iter_mut().enumerate() {
        *block = BENCH_BLOCKS[i % unique];
    }
    BlockStorage::from_dense(&dense)
}

fn bench_storage_case(name: &str, mut store: BlockStorage, values: &[BlockType]) -> u64 {
    let mut checksum = 0u64;
    for i in 0..WARMUP {
        let idx = (i * 37) & 4095;
        black_box(store.get(idx));
        store.set(idx, values[i % values.len()]);
    }
    let start = Instant::now();
    for n in 0..ITERS {
        let idx = (n * 37) & 4095;
        checksum = checksum.wrapping_add(store.get(idx) as u64);
        checksum = checksum.wrapping_add(store.set(idx, values[n % values.len()]) as u64);
    }
    report(name, ITERS, start.elapsed().as_nanos(), checksum);
    checksum
}

fn bench_storage() -> u64 {
    let mut checksum = 0u64;
    for (name, store, values) in [
        (
            "storage_get_set_uniform",
            BlockStorage::Uniform(BlockType::Stone),
            &BENCH_BLOCKS[1..2],
        ),
        (
            "storage_get_set_paletted1",
            storage_fixture(2),
            &BENCH_BLOCKS[..2],
        ),
        (
            "storage_get_set_paletted2",
            storage_fixture(3),
            &BENCH_BLOCKS[..3],
        ),
        (
            "storage_get_set_paletted4",
            storage_fixture(5),
            &BENCH_BLOCKS[..5],
        ),
        (
            "storage_get_set_paletted8",
            storage_fixture(17),
            &BENCH_BLOCKS[..17],
        ),
        (
            "storage_get_set_global",
            BlockStorage::Global(Box::new([BlockType::Stone; 4096])),
            &BENCH_BLOCKS[1..3],
        ),
    ] {
        checksum = checksum.wrapping_add(bench_storage_case(name, store, values));
    }
    checksum
}

fn bench_lighting() -> u64 {
    let mut checksum = 0u64;
    for (name, mut light, mutate) in [
        (
            "lighting_get_set_uniform",
            LightStorage::Uniform { sky: 15, block: 0 },
            false,
        ),
        (
            "lighting_get_set_packed",
            LightStorage::Packed(Box::new([0xF0; 4096])),
            true,
        ),
    ] {
        for i in 0..WARMUP {
            black_box(light.get_sky(i));
            black_box(light.get_block(i));
        }
        let start = Instant::now();
        let mut case_checksum = 0u64;
        for n in 0..ITERS {
            let idx = (n * 29) & 4095;
            if mutate {
                light.set_block(idx, (n & 15) as u8);
            } else {
                light.set_block(idx, 0);
            }
            case_checksum = case_checksum
                .wrapping_add(light.get_block(idx) as u64)
                .wrapping_add(light.get_sky(idx) as u64);
        }
        report(name, ITERS, start.elapsed().as_nanos(), case_checksum);
        checksum = checksum.wrapping_add(case_checksum);
    }
    checksum
}

fn bench_physics() -> u64 {
    let mut manager = ChunkManager::new_in_dimension(2, crate::dimension::Dimension::Overworld);
    manager.chunks.insert((0, 0), Chunk::new(0, 0));
    let mut player = PlayerPhysics::new(Vec3::new(8.5, 80.0, 8.5));
    let start = Instant::now();
    for n in 0..ITERS {
        player.update(
            1.0 / 60.0,
            &manager,
            Vec3::new(0.2, 0.0, 0.1),
            false,
            n & 1 == 0,
        );
    }
    let checksum = player.position.x.to_bits() as u64
        ^ player.position.y.to_bits() as u64
        ^ player.position.z.to_bits() as u64;
    report(
        "player_physics_collision",
        ITERS,
        start.elapsed().as_nanos(),
        checksum,
    );
    checksum
}

fn bench_mesh() -> u64 {
    let chunk = Chunk::new(0, 0);
    let key = SectionKey::new(0, 0, 0);
    let start = Instant::now();
    let mut checksum = 0u64;
    for _ in 0..ITERS {
        let mesh = chunk.generate_section_mesh_bundle(key, 1, 1, |x, y, z| {
            let inside = (0..16).contains(&x) && (0..256).contains(&y) && (0..16).contains(&z);
            if inside {
                (
                    chunk.get_block(x, y, z),
                    chunk.get_block_state(x, y, z),
                    chunk.get_sky_light(x as usize, y, z as usize),
                    chunk.get_block_light(x as usize, y, z as usize),
                    false,
                )
            } else {
                (BlockType::Air, 0, 0, 0, false)
            }
        });
        let l0 = &mesh.levels[0].opaque;
        checksum = checksum
            .wrapping_add(l0.vertices.len() as u64)
            .wrapping_add(l0.indices.len() as u64);
    }
    report(
        "section_l0_mesh",
        ITERS,
        start.elapsed().as_nanos(),
        checksum,
    );
    checksum
}

fn bench_save() -> u64 {
    let chunk = Chunk::new(0, 0);
    let start = Instant::now();
    let mut checksum = 0u64;
    for _ in 0..ITERS {
        let data = ChunkSaveData::from_chunk(&chunk);
        checksum = checksum.wrapping_add(data.blocks.len() as u64);
        checksum = checksum.wrapping_add(bincode::serialize(&data).unwrap().len() as u64);
    }
    report(
        "chunk_save_flatten_serialize",
        ITERS,
        start.elapsed().as_nanos(),
        checksum,
    );
    checksum
}

fn bench_network() -> u64 {
    let chunk = Chunk::new(0, 0);
    let start = Instant::now();
    let mut checksum = 0u64;
    for _ in 0..ITERS {
        let flattened = ChunkSaveData::from_chunk(&chunk);
        let packet = Packet::ChunkData {
            protocol_version: crate::network::protocol::PROTOCOL_VERSION,
            dimension: 0,
            cx: chunk.chunk_x,
            cz: chunk.chunk_z,
            revision: 1,
            min_section_y: chunk.min_section_y,
            section_count: chunk.sections.len() as u16,
            blocks: flattened.blocks,
            block_states: flattened.block_states,
            block_entities: flattened.block_entities,
        };
        let bytes = packet.encode();
        let decoded = Packet::decode(&bytes).unwrap();
        let Packet::ChunkData {
            blocks,
            block_states,
            revision,
            ..
        } = decoded
        else {
            unreachable!("encoded chunk data decoded as another packet variant")
        };
        checksum = checksum.wrapping_add(bytes.len() as u64);
        checksum = checksum.wrapping_add(blocks.len() as u64);
        checksum = checksum.wrapping_add(block_states.len() as u64);
        checksum = checksum.wrapping_add(revision);
    }
    report(
        "network_chunk_flatten_serialize_deserialize",
        ITERS,
        start.elapsed().as_nanos(),
        checksum,
    );
    checksum
}

pub fn run() -> [u64; 6] {
    [
        bench_storage(),
        bench_lighting(),
        bench_physics(),
        bench_mesh(),
        bench_save(),
        bench_network(),
    ]
}

#[test]
fn smoke_checksums_are_stable() {
    assert_eq!(run(), run());
}

#[test]
fn storage_fixtures_cover_every_runtime_representation() {
    assert!(matches!(storage_fixture(2), BlockStorage::Paletted1 { .. }));
    assert!(matches!(storage_fixture(3), BlockStorage::Paletted2 { .. }));
    assert!(matches!(storage_fixture(5), BlockStorage::Paletted4 { .. }));
    assert!(matches!(
        storage_fixture(17),
        BlockStorage::Paletted8 { .. }
    ));
}

#[test]
#[ignore = "timing benchmark; run explicitly in release mode"]
fn microbench_release() {
    let _ = run();
}
