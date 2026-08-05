use crate::world::{Biome, BlockType, CHUNK_DEPTH, CHUNK_WIDTH};
use crate::worldgen::{hash_coord, WorldGenContext, SEA_LEVEL};

/// Places biome-appropriate features (trees, plants, cactus, sugar cane)
/// into a chunk's dense block array.
#[derive(Debug, Clone)]
pub struct FeaturePlacer {
    seed: u32,
}

impl FeaturePlacer {
    pub fn new(world_seed: u32) -> Self {
        Self { seed: world_seed }
    }

    /// Places features for a chunk. The dense array is indexed
    /// [x][local_y][z] where min_y_offset is added to get world Y.
    pub fn place_features(
        &self,
        ctx: &WorldGenContext,
        blocks: &mut [Vec<[BlockType; CHUNK_DEPTH]>],
        chunk_x: i32,
        chunk_z: i32,
        min_y_offset: usize,
    ) {
        // Trees are placed from neighbor chunks so trunks/leaves can cross
        // boundaries deterministically.
        for dx in -1..=1 {
            for dz in -1..=1 {
                let nx = chunk_x + dx;
                let nz = chunk_z + dz;
                self.place_trees(ctx, blocks, nx, nz, chunk_x, chunk_z, min_y_offset);
            }
        }

        // Column features (plants, cactus, sugar cane) use only the current
        // column, so they are placed from the current chunk without probing
        // neighbors.
        for x in 0..CHUNK_WIDTH {
            for z in 0..CHUNK_DEPTH {
                let wx = chunk_x * CHUNK_WIDTH as i32 + x as i32;
                let wz = chunk_z * CHUNK_DEPTH as i32 + z as i32;
                let surface_y = ctx.surface_height_at(wx, wz);
                let biome = ctx.biome_at(wx, wz);
                self.place_column_features(blocks, wx, wz, surface_y, biome, x, z, min_y_offset);
            }
        }
    }

    fn place_trees(
        &self,
        ctx: &WorldGenContext,
        blocks: &mut [Vec<[BlockType; CHUNK_DEPTH]>],
        neighbor_cx: i32,
        neighbor_cz: i32,
        chunk_x: i32,
        chunk_z: i32,
        min_y_offset: usize,
    ) {
        // Try 4 candidate spots per neighbor chunk.
        for attempt in 0..4 {
            let h = hash_coord(self.seed, neighbor_cx, attempt, neighbor_cz, 0x7EED_0FD5);
            let tx = ((h) & 0xF) as i32;
            let tz = ((h >> 4) & 0xF) as i32;
            let n_world_x = neighbor_cx * CHUNK_WIDTH as i32 + tx;
            let n_world_z = neighbor_cz * CHUNK_DEPTH as i32 + tz;

            let biome = ctx.biome_at(n_world_x, n_world_z);
            let surface_h = ctx.surface_height_at(n_world_x, n_world_z);
            if surface_h <= SEA_LEVEL {
                continue;
            }

            let tree_prob: u32 = match biome {
                Biome::Plains => 5,
                Biome::Forest => 55,
                Biome::BirchForest => 60,
                Biome::Taiga => 40,
                Biome::Swamp => 18,
                Biome::Jungle => 70,
                Biome::Savanna => 4,
                Biome::Meadow => 2,
                Biome::WindsweptHills => 2,
                _ => 0,
            };

            let roll = ((h >> 8) % 100) as u32;
            if roll >= tree_prob {
                continue;
            }

            let tree_height = 4 + ((h >> 16) % 4) as i32;
            let local_x = n_world_x - chunk_x * CHUNK_WIDTH as i32;
            let local_z = n_world_z - chunk_z * CHUNK_DEPTH as i32;
            if local_x < 0
                || local_x >= CHUNK_WIDTH as i32
                || local_z < 0
                || local_z >= CHUNK_DEPTH as i32
            {
                continue;
            }

            match biome {
                Biome::Taiga => self.place_spruce(
                    blocks,
                    local_x as usize,
                    local_z as usize,
                    surface_h + 1,
                    tree_height + 2,
                    min_y_offset,
                ),
                Biome::BirchForest => self.place_birch(
                    blocks,
                    local_x as usize,
                    local_z as usize,
                    surface_h + 1,
                    tree_height + 1,
                    min_y_offset,
                ),
                Biome::Jungle => self.place_jungle(
                    blocks,
                    local_x as usize,
                    local_z as usize,
                    surface_h + 1,
                    tree_height + 3,
                    min_y_offset,
                ),
                _ => self.place_oak(
                    blocks,
                    local_x as usize,
                    local_z as usize,
                    surface_h + 1,
                    tree_height,
                    min_y_offset,
                ),
            }
        }
    }

    fn place_column_features(
        &self,
        blocks: &mut [Vec<[BlockType; CHUNK_DEPTH]>],
        wx: i32,
        wz: i32,
        surface_y: i32,
        biome: Biome,
        lx: usize,
        lz: usize,
        min_y_offset: usize,
    ) {
        let h = hash_coord(self.seed, wx, surface_y, wz, 0xC01D_0C0A);
        let roll = h % 100;

        // Find the surface block.
        let Some(surface_block) = self.block_at_local(blocks, lx, surface_y, lz, min_y_offset)
        else {
            return;
        };

        // Plants on grass.
        if surface_block == BlockType::Grass {
            if roll < 10 {
                self.set_block_local(
                    blocks,
                    lx,
                    surface_y + 1,
                    lz,
                    BlockType::TallGrass,
                    min_y_offset,
                );
            } else if roll < 12 {
                self.set_block_local(
                    blocks,
                    lx,
                    surface_y + 1,
                    lz,
                    BlockType::Dandelion,
                    min_y_offset,
                );
            } else if roll < 13 {
                self.set_block_local(
                    blocks,
                    lx,
                    surface_y + 1,
                    lz,
                    BlockType::Poppy,
                    min_y_offset,
                );
            } else if roll < 14 && (biome == Biome::Plains || biome == Biome::Forest) {
                let veg = if (h >> 8) & 1 == 0 {
                    BlockType::Pumpkin
                } else {
                    BlockType::Melon
                };
                self.set_block_local(blocks, lx, surface_y + 1, lz, veg, min_y_offset);
            }
        }

        // Cactus in desert.
        if surface_block == BlockType::Sand && (biome == Biome::Desert || biome == Biome::Badlands)
        {
            if roll < 2 {
                let height = 1 + ((h >> 8) % 3) as i32;
                for dy in 1..=height {
                    self.set_block_local(
                        blocks,
                        lx,
                        surface_y + dy,
                        lz,
                        BlockType::Cactus,
                        min_y_offset,
                    );
                }
            }
        }

        // Sugar cane near water.
        if matches!(
            surface_block,
            BlockType::Grass | BlockType::Dirt | BlockType::Sand
        ) && surface_y > 0
        {
            let near_water = self.is_near_water(blocks, lx, surface_y, lz, min_y_offset);
            if near_water {
                let cane_roll = hash_coord(self.seed, wx, surface_y, wz, 0x5A7A_317E);
                if cane_roll % 100 < 10 {
                    let height = (2 + (cane_roll >> 8) % 3) as i32;
                    for dy in 1..=height {
                        self.set_block_local(
                            blocks,
                            lx,
                            surface_y + dy,
                            lz,
                            BlockType::SugarCane,
                            min_y_offset,
                        );
                    }
                }
            }
        }
    }

    fn is_near_water(
        &self,
        blocks: &[Vec<[BlockType; CHUNK_DEPTH]>],
        lx: usize,
        ly: i32,
        lz: usize,
        min_y_offset: usize,
    ) -> bool {
        for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let nx = lx as i32 + dx;
            let nz = lz as i32 + dz;
            if nx >= 0 && nx < CHUNK_WIDTH as i32 && nz >= 0 && nz < CHUNK_DEPTH as i32 {
                if let Some(BlockType::Water) =
                    self.block_at_local(blocks, nx as usize, ly, nz as usize, min_y_offset)
                {
                    return true;
                }
            }
        }
        false
    }

    fn block_at_local(
        &self,
        blocks: &[Vec<[BlockType; CHUNK_DEPTH]>],
        x: usize,
        wy: i32,
        z: usize,
        min_y_offset: usize,
    ) -> Option<BlockType> {
        let ly = wy as i32 - min_y_offset as i32;
        if ly >= 0 && (ly as usize) < blocks.len() {
            Some(blocks[x][ly as usize][z])
        } else {
            None
        }
    }

    fn set_block_local(
        &self,
        blocks: &mut [Vec<[BlockType; CHUNK_DEPTH]>],
        x: usize,
        wy: i32,
        z: usize,
        block: BlockType,
        min_y_offset: usize,
    ) {
        let ly = wy as i32 - min_y_offset as i32;
        if ly >= 0 && (ly as usize) < blocks.len() {
            blocks[x][ly as usize][z] = block;
        }
    }

    fn place_oak(
        &self,
        blocks: &mut [Vec<[BlockType; CHUNK_DEPTH]>],
        lx: usize,
        lz: usize,
        start_y: i32,
        height: i32,
        min_y_offset: usize,
    ) {
        for dy in 0..height {
            self.set_block_local(
                blocks,
                lx,
                start_y + dy,
                lz,
                BlockType::OakLog,
                min_y_offset,
            );
        }
        for ly in (height - 3)..=height {
            let radius: i32 = if ly == height { 1 } else { 2 };
            for dx in -radius..=radius {
                for dz in -radius..=radius {
                    if radius == 2 && dx.abs() == 2 && dz.abs() == 2 {
                        continue;
                    }
                    let bx = lx as i32 + dx;
                    let bz = lz as i32 + dz;
                    if bx >= 0 && bx < CHUNK_WIDTH as i32 && bz >= 0 && bz < CHUNK_DEPTH as i32 {
                        let current = self.block_at_local(
                            blocks,
                            bx as usize,
                            start_y + ly,
                            bz as usize,
                            min_y_offset,
                        );
                        if matches!(
                            current,
                            None | Some(BlockType::Air) | Some(BlockType::OakLeaves)
                        ) {
                            self.set_block_local(
                                blocks,
                                bx as usize,
                                start_y + ly,
                                bz as usize,
                                BlockType::OakLeaves,
                                min_y_offset,
                            );
                        }
                    }
                }
            }
        }
    }

    fn place_birch(
        &self,
        blocks: &mut [Vec<[BlockType; CHUNK_DEPTH]>],
        lx: usize,
        lz: usize,
        start_y: i32,
        height: i32,
        min_y_offset: usize,
    ) {
        for dy in 0..height {
            self.set_block_local(
                blocks,
                lx,
                start_y + dy,
                lz,
                BlockType::BirchLog,
                min_y_offset,
            );
        }
        for ly in (height - 3)..=height {
            for dx in -1..=1 {
                for dz in -1..=1 {
                    let bx = lx as i32 + dx;
                    let bz = lz as i32 + dz;
                    if bx >= 0 && bx < CHUNK_WIDTH as i32 && bz >= 0 && bz < CHUNK_DEPTH as i32 {
                        let current = self.block_at_local(
                            blocks,
                            bx as usize,
                            start_y + ly,
                            bz as usize,
                            min_y_offset,
                        );
                        if matches!(current, None | Some(BlockType::Air)) {
                            self.set_block_local(
                                blocks,
                                bx as usize,
                                start_y + ly,
                                bz as usize,
                                BlockType::BirchLeaves,
                                min_y_offset,
                            );
                        }
                    }
                }
            }
        }
    }

    fn place_spruce(
        &self,
        blocks: &mut [Vec<[BlockType; CHUNK_DEPTH]>],
        lx: usize,
        lz: usize,
        start_y: i32,
        height: i32,
        min_y_offset: usize,
    ) {
        for dy in 0..height {
            self.set_block_local(
                blocks,
                lx,
                start_y + dy,
                lz,
                BlockType::SpruceLog,
                min_y_offset,
            );
        }
        for ly in (height - 6)..=height {
            let radius: i32 = if ly >= height - 1 { 1 } else { 2 };
            for dx in -radius..=radius {
                for dz in -radius..=radius {
                    if radius == 2 && dx.abs() == 2 && dz.abs() == 2 {
                        continue;
                    }
                    let bx = lx as i32 + dx;
                    let bz = lz as i32 + dz;
                    if bx >= 0 && bx < CHUNK_WIDTH as i32 && bz >= 0 && bz < CHUNK_DEPTH as i32 {
                        let current = self.block_at_local(
                            blocks,
                            bx as usize,
                            start_y + ly,
                            bz as usize,
                            min_y_offset,
                        );
                        if matches!(current, None | Some(BlockType::Air)) {
                            self.set_block_local(
                                blocks,
                                bx as usize,
                                start_y + ly,
                                bz as usize,
                                BlockType::SpruceLeaves,
                                min_y_offset,
                            );
                        }
                    }
                }
            }
        }
    }

    fn place_jungle(
        &self,
        blocks: &mut [Vec<[BlockType; CHUNK_DEPTH]>],
        lx: usize,
        lz: usize,
        start_y: i32,
        height: i32,
        min_y_offset: usize,
    ) {
        for dy in 0..height {
            self.set_block_local(
                blocks,
                lx,
                start_y + dy,
                lz,
                BlockType::OakLog,
                min_y_offset,
            );
        }
        for ly in (height - 4)..=height {
            let radius: i32 = if ly == height { 1 } else { 2 };
            for dx in -radius..=radius {
                for dz in -radius..=radius {
                    if radius == 2 && dx.abs() == 2 && dz.abs() == 2 {
                        continue;
                    }
                    let bx = lx as i32 + dx;
                    let bz = lz as i32 + dz;
                    if bx >= 0 && bx < CHUNK_WIDTH as i32 && bz >= 0 && bz < CHUNK_DEPTH as i32 {
                        let current = self.block_at_local(
                            blocks,
                            bx as usize,
                            start_y + ly,
                            bz as usize,
                            min_y_offset,
                        );
                        if matches!(current, None | Some(BlockType::Air)) {
                            self.set_block_local(
                                blocks,
                                bx as usize,
                                start_y + ly,
                                bz as usize,
                                BlockType::OakLeaves,
                                min_y_offset,
                            );
                        }
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
    fn feature_placement_is_deterministic() {
        let ctx_a = WorldGenContext::new(12345);
        let ctx_b = WorldGenContext::new(12345);
        let placer_a = FeaturePlacer::new(12345);
        let placer_b = FeaturePlacer::new(12345);

        let min_y_offset = 64usize;
        let mut blocks_a: Vec<Vec<[BlockType; CHUNK_DEPTH]>> =
            vec![vec![[BlockType::Air; CHUNK_DEPTH]; 384]; CHUNK_WIDTH];
        let mut blocks_b: Vec<Vec<[BlockType; CHUNK_DEPTH]>> =
            vec![vec![[BlockType::Air; CHUNK_DEPTH]; 384]; CHUNK_WIDTH];

        placer_a.place_features(&ctx_a, &mut blocks_a, 3, -2, min_y_offset);
        placer_b.place_features(&ctx_b, &mut blocks_b, 3, -2, min_y_offset);

        for x in 0..CHUNK_WIDTH {
            for y in 0..384 {
                for z in 0..CHUNK_DEPTH {
                    assert_eq!(blocks_a[x][y][z], blocks_b[x][y][z]);
                }
            }
        }
    }
}
