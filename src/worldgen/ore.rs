use crate::world::{BlockType, CHUNK_DEPTH, CHUNK_WIDTH};
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

    /// Places ore veins into a chunk's dense block array.
    ///
    /// locks[x][local_y][z] where local_y is an index into the full
    /// signed-height array (0..=height-1, with min_y_offset added to get
    /// the world Y).
    pub fn place_ores(
        &self,
        blocks: &mut [Vec<[BlockType; CHUNK_DEPTH]>],
        chunk_x: i32,
        _chunk_z: i32,
        min_y_offset: usize,
    ) {
        for (ci, config) in self.configs.iter().enumerate() {
            for attempt in 0..config.frequency {
                let h = hash_coord(self.seed, chunk_x, ci as i32, attempt as i32, 0x0E_60_51);
                let lx = (h & 0xF) as usize;
                let lz = ((h >> 4) & 0xF) as usize;

                // Y in the config range, mapped to local array index.
                let range = (config.max_y - config.min_y + 1).max(1) as u32;
                let wy = config.min_y + ((h >> 8) % range) as i32;
                let ly = wy as i32 - min_y_offset as i32;
                if ly < 0 || (ly as usize) >= blocks.len() {
                    continue;
                }
                let ly = ly as usize;

                if ly >= blocks.len() {
                    continue;
                }

                if blocks[lx][ly][lz] != BlockType::Stone {
                    continue;
                }

                // Breadth-first vein with deterministic direction.
                let mut seed2 = h;
                let mut queue = vec![(lx, ly, lz)];
                blocks[lx][ly][lz] = config.block;
                let mut placed = 1;
                let mut head = 0;

                while head < queue.len() && placed < config.vein_size {
                    let (cx, cy, cz) = queue[head];
                    head += 1;

                    seed2 = seed2.wrapping_mul(1103515245).wrapping_add(12345);
                    let dir = ((seed2 >> 16) % 6) as usize;
                    let neighbors = [
                        (cx + 1, cy, cz),
                        (cx.wrapping_sub(1), cy, cz),
                        (cx, cy + 1, cz),
                        (cx, cy.wrapping_sub(1), cz),
                        (cx, cy, cz + 1),
                        (cx, cy, cz.wrapping_sub(1)),
                    ];
                    let (nx, ny, nz) = neighbors[dir];

                    if nx < CHUNK_WIDTH
                        && nz < CHUNK_DEPTH
                        && ny < blocks.len()
                        && ny > 0
                        && blocks[nx][ny][nz] == BlockType::Stone
                    {
                        blocks[nx][ny][nz] = config.block;
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

    #[test]
    fn ore_placement_is_deterministic() {
        let a = OreGenerator::new(12345);
        let b = OreGenerator::new(12345);
        let min_y_offset = 64usize;
        let mut blocks_a: Vec<Vec<[BlockType; CHUNK_DEPTH]>> =
            vec![vec![[BlockType::Stone; CHUNK_DEPTH]; 384]; CHUNK_WIDTH];
        let mut blocks_b: Vec<Vec<[BlockType; CHUNK_DEPTH]>> =
            vec![vec![[BlockType::Stone; CHUNK_DEPTH]; 384]; CHUNK_WIDTH];
        a.place_ores(&mut blocks_a, 3, -2, min_y_offset);
        b.place_ores(&mut blocks_b, 3, -2, min_y_offset);
        for x in 0..CHUNK_WIDTH {
            for y in 0..384 {
                for z in 0..CHUNK_DEPTH {
                    assert_eq!(blocks_a[x][y][z], blocks_b[x][y][z]);
                }
            }
        }
    }
}
