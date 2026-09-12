use crate::world::{BlockType, Chunk, CHUNK_DEPTH, CHUNK_WIDTH};
use crate::worldgen::hash_coord;

/// Ore distribution configuration.
#[derive(Debug, Clone, Copy)]
pub struct OreConfig {
    pub block: BlockType,
    pub min_y: i32,
    pub max_y: i32,
    pub vein_size: usize,
    /// Number of vein attempts per chunk.
    pub frequency: usize,
}

/// Ore generator with data-driven Y ranges, vein sizes, and frequencies.
#[derive(Debug, Clone)]
pub struct OreGenerator {
    seed: u32,
    configs: Vec<OreConfig>,
}

impl OreGenerator {
    pub fn new(world_seed: u32) -> Self {
        Self {
            seed: world_seed,
            configs: vec![
                OreConfig {
                    block: BlockType::CoalOre,
                    min_y: -20,
                    max_y: 128,
                    vein_size: 17,
                    frequency: 15,
                },
                OreConfig {
                    block: BlockType::IronOre,
                    min_y: -48,
                    max_y: 64,
                    vein_size: 9,
                    frequency: 12,
                },
                OreConfig {
                    block: BlockType::GoldOre,
                    min_y: -56,
                    max_y: 32,
                    vein_size: 9,
                    frequency: 4,
                },
                OreConfig {
                    block: BlockType::RedstoneOre,
                    min_y: -56,
                    max_y: 16,
                    vein_size: 8,
                    frequency: 8,
                },
                OreConfig {
                    block: BlockType::DiamondOre,
                    min_y: -64,
                    max_y: 16,
                    vein_size: 8,
                    frequency: 2,
                },
            ],
        }
    }

    pub fn configs(&self) -> &[OreConfig] {
        &self.configs
    }

    /// Places ore veins directly into paletted chunk sections.
    pub fn place_ores(&self, chunk: &mut Chunk, chunk_x: i32, chunk_z: i32) {
        let min_y = chunk.min_world_y();
        let max_y = chunk.max_world_y_exclusive();
        for (ci, config) in self.configs.iter().enumerate() {
            for attempt in 0..config.frequency {
                let h = hash_coord(
                    self.seed,
                    chunk_x,
                    ci as i32,
                    chunk_z,
                    0x0E_60_51 ^ (attempt as u32).wrapping_mul(0x9E37_79B9),
                );
                let lx = (h & 0xF) as usize;
                let lz = ((h >> 4) & 0xF) as usize;
                if lx >= CHUNK_WIDTH || lz >= CHUNK_DEPTH {
                    continue;
                }

                let range = (config.max_y - config.min_y + 1).max(1) as u32;
                let wy = config.min_y + ((h >> 8) % range) as i32;
                if wy < min_y || wy >= max_y {
                    continue;
                }

                if chunk.get_block_local(lx, wy, lz) != BlockType::Stone {
                    continue;
                }

                let mut seed2 = h;
                let mut queue = vec![(lx as i32, wy, lz as i32)];
                chunk.set_block_local(lx, wy, lz, config.block);
                let mut placed = 1;
                let mut head = 0;

                while head < queue.len() && placed < config.vein_size {
                    let (cx, cy, cz) = queue[head];
                    head += 1;

                    seed2 = seed2.wrapping_mul(1103515245).wrapping_add(12345);
                    let dir = ((seed2 >> 16) % 6) as usize;
                    let neighbors = [
                        (cx + 1, cy, cz),
                        (cx - 1, cy, cz),
                        (cx, cy + 1, cz),
                        (cx, cy - 1, cz),
                        (cx, cy, cz + 1),
                        (cx, cy, cz - 1),
                    ];
                    let (nx, ny, nz) = neighbors[dir];

                    if nx >= 0
                        && nx < CHUNK_WIDTH as i32
                        && nz >= 0
                        && nz < CHUNK_DEPTH as i32
                        && ny >= min_y
                        && ny < max_y
                        && chunk.get_block_local(nx as usize, ny, nz as usize) == BlockType::Stone
                    {
                        chunk.set_block_local(nx as usize, ny, nz as usize, config.block);
                        queue.push((nx, ny, nz));
                        placed += 1;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dimension::Dimension;

    #[test]
    fn ore_configs_cover_negative_y() {
        let gen = OreGenerator::new(12345);
        assert!(gen
            .configs()
            .iter()
            .any(|c| c.block == BlockType::DiamondOre && c.min_y < 0));
        assert!(gen
            .configs()
            .iter()
            .any(|c| c.block == BlockType::RedstoneOre && c.min_y < 0));
    }

    fn stone_chunk(cx: i32, cz: i32) -> Chunk {
        let mut chunk = Chunk::empty_in_dimension(Dimension::Overworld, cx, cz);
        for x in 0..CHUNK_WIDTH {
            for z in 0..CHUNK_DEPTH {
                for y in chunk.world_y_range() {
                    chunk.set_block_local(x, y, z, BlockType::Stone);
                }
            }
        }
        chunk
    }

    #[test]
    fn ore_placement_is_deterministic() {
        let a = OreGenerator::new(12345);
        let b = OreGenerator::new(12345);
        let mut chunk_a = stone_chunk(3, -2);
        let mut chunk_b = stone_chunk(3, -2);
        a.place_ores(&mut chunk_a, 3, -2);
        b.place_ores(&mut chunk_b, 3, -2);
        for x in 0..CHUNK_WIDTH {
            for y in chunk_a.world_y_range() {
                for z in 0..CHUNK_DEPTH {
                    assert_eq!(
                        chunk_a.get_block_local(x, y, z),
                        chunk_b.get_block_local(x, y, z)
                    );
                }
            }
        }
    }

    #[test]
    fn ores_place_at_configured_world_y() {
        let gen = OreGenerator::new(12345);
        let mut diamond_below_16 = false;
        let mut coal_near_sea = false;
        let mut diamond_count = 0usize;
        let mut diamond_only_in_wrong_local_band = true;

        for cx in 0..8 {
            for cz in 0..8 {
                let mut chunk = stone_chunk(cx, cz);
                gen.place_ores(&mut chunk, cx, cz);
                for x in 0..CHUNK_WIDTH {
                    for y in chunk.world_y_range() {
                        for z in 0..CHUNK_DEPTH {
                            match chunk.get_block_local(x, y, z) {
                                BlockType::DiamondOre => {
                                    diamond_count += 1;
                                    assert!(
                                        y >= -64 && y <= 16 + 8,
                                        "diamond at world Y={y} outside config band"
                                    );
                                    if y < 16 {
                                        diamond_below_16 = true;
                                    }
                                    // local band for world Y -64..-49 is section-local 0..16
                                    // at the bottom of the column; diamonds must not only
                                    // appear there.
                                    if y >= -48 {
                                        diamond_only_in_wrong_local_band = false;
                                    }
                                }
                                BlockType::CoalOre if (40..=80).contains(&y) => {
                                    coal_near_sea = true;
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }

        assert!(
            diamond_count > 0 && diamond_below_16,
            "diamond must exist at configured world Y < 16, not a -64 local-band misfire"
        );
        assert!(
            !diamond_only_in_wrong_local_band,
            "diamonds must not only occupy local 0..16 (world Y -64..-49)"
        );
        assert!(
            coal_near_sea,
            "coal must appear near sea level (world Y 40..80)"
        );
    }

    #[test]
    fn ore_hash_includes_chunk_z() {
        let gen = OreGenerator::new(1);
        let mut a = stone_chunk(3, 0);
        let mut b = stone_chunk(3, 1);
        gen.place_ores(&mut a, 3, 0);
        gen.place_ores(&mut b, 3, 1);
        let mut differs = false;
        for x in 0..CHUNK_WIDTH {
            for y in a.world_y_range() {
                for z in 0..CHUNK_DEPTH {
                    if a.get_block_local(x, y, z) != b.get_block_local(x, y, z) {
                        differs = true;
                    }
                }
            }
        }
        assert!(
            differs,
            "identical X with different Z must not share the ore hash"
        );
    }
}
