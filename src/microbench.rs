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

fn bench_storage() -> u64 {
    let mut dense = [BlockType::Air; 4096];
    for (i, block) in dense.iter_mut().enumerate() {
        *block = if i % 3 == 0 {
            BlockType::Stone
        } else {
            BlockType::Dirt
        };
    }
    let mut stores = vec![
        BlockStorage::Uniform(BlockType::Stone),
        BlockStorage::from_dense(&[BlockType::Air; 4096]),
        BlockStorage::from_dense(&dense),
    ];
    let mut checksum = 0u64;
    for store in &mut stores {
        for i in 0..WARMUP {
            black_box(store.get(i));
            store.set(
                i,
                if i & 1 == 0 {
                    BlockType::Dirt
                } else {
                    BlockType::Stone
                },
            );
        }
    }
    let start = Instant::now();
    for n in 0..ITERS {
        for store in &mut stores {
            let i = (n * 37) & 4095;
            checksum = checksum.wrapping_add(store.get(i) as u64);
            store.set(i, BlockType::Stone);
        }
    }
    report(
        "storage_get_set_uniform_paletted_global",
        ITERS * stores.len(),
        start.elapsed().as_nanos(),
        checksum,
    );
    checksum
}

fn bench_lighting() -> u64 {
    let mut light = LightStorage::Uniform { sky: 15, block: 0 };
    for i in 0..WARMUP {
        light.set_sky(i, (i & 15) as u8);
    }
    let mut checksum = 0u64;
    let start = Instant::now();
    for n in 0..ITERS {
        let i = (n * 29) & 4095;
        light.set_block(i, (n & 15) as u8);
        checksum += light.get_block(i) as u64 + light.get_sky(i) as u64;
    }
    report(
        "lighting_mutation_packed",
        ITERS,
        start.elapsed().as_nanos(),
        checksum,
    );
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
                    chunk.get_sky_light(x as usize, y as usize, z as usize),
                    chunk.get_block_light(x as usize, y as usize, z as usize),
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
    let packet = Packet::Handshake {
        protocol_version: 7,
        username: "bench".to_string(),
    };
    let start = Instant::now();
    let mut checksum = 0u64;
    for _ in 0..ITERS {
        let bytes = packet.encode();
        let decoded = Packet::decode(&bytes).unwrap();
        checksum = checksum
            .wrapping_add(bytes.len() as u64)
            .wrapping_add(decoded.encode().len() as u64);
    }
    report(
        "network_packet_serialize_deserialize",
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
#[ignore = "timing benchmark; run explicitly in release mode"]
fn microbench_release() {
    let _ = run();
}
