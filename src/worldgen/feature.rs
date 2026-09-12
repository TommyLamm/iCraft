use crate::world::{Biome, BlockType, Chunk, CHUNK_DEPTH, CHUNK_WIDTH};
use crate::worldgen::{hash_coord, WorldGenContext, SEA_LEVEL};

/// Places biome-appropriate features (trees, plants, cactus, sugar cane)
/// directly into paletted chunk sections.
#[derive(Debug, Clone)]
pub struct FeaturePlacer {
    seed: u32,
}

impl FeaturePlacer {
    pub fn new(world_seed: u32) -> Self {
        Self { seed: world_seed }
    }

    /// Places biome-appropriate features (trees, plants, cactus, sugar cane)
    /// directly into paletted chunk sections.
    pub fn place_features(
        &self,
        ctx: &WorldGenContext,
        chunk: &mut Chunk,
        chunk_x: i32,
        chunk_z: i32,
    ) {
        // Trees are placed from neighbor chunks so trunks/leaves can cross
        // boundaries deterministically.
        for dx in -1..=1 {
            for dz in -1..=1 {
                let nx = chunk_x + dx;
                let nz = chunk_z + dz;
                self.place_trees(ctx, chunk, nx, nz, chunk_x, chunk_z);
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
                self.place_column_features(chunk, wx, wz, surface_y, biome, x, z);
            }
        }
    }

    fn place_trees(
        &self,
        ctx: &WorldGenContext,
        chunk: &mut Chunk,
        neighbor_cx: i32,
        neighbor_cz: i32,
        chunk_x: i32,
        chunk_z: i32,
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
                Biome::Plains => 18,
                Biome::Forest => 55,
                Biome::BirchForest => 60,
                Biome::Taiga => 40,
                Biome::Swamp => 18,
                Biome::Jungle => 70,
                Biome::Savanna => 12,
                Biome::Meadow => 12,
                Biome::WindsweptHills => 8,
                _ => 0,
            };

            let roll = hash_coord(
                self.seed ^ 0x3C6E_F372,
                n_world_x,
                attempt,
                n_world_z,
                0xBB67_AE85,
            ) % 100;
            if roll >= tree_prob {
                continue;
            }

            let tree_height = 4 + ((h >> 16) % 4) as i32;
            // Local coords may fall outside this chunk when the trunk lives in a
            // neighbor. Still place the tree so overhanging leaves write here.
            let local_x = n_world_x - chunk_x * CHUNK_WIDTH as i32;
            let local_z = n_world_z - chunk_z * CHUNK_DEPTH as i32;

            match biome {
                Biome::Taiga => self.place_spruce(chunk,
                    local_x,
                    local_z,
                    surface_h + 1,
                    tree_height + 2,
                ),
                Biome::BirchForest => self.place_birch(chunk,
                    local_x,
                    local_z,
                    surface_h + 1,
                    tree_height + 1,
                ),
                Biome::Jungle => self.place_jungle(chunk,
                    local_x,
                    local_z,
                    surface_h + 1,
                    tree_height + 3,
                ),
                _ => self.place_oak(chunk,
                    local_x,
                    local_z,
                    surface_h + 1,
                    tree_height,
                ),
            }
        }
    }

    fn place_column_features(
        &self,
        chunk: &mut Chunk,
        wx: i32,
        wz: i32,
        surface_y: i32,
        biome: Biome,
        lx: usize,
        lz: usize,
    ) {
        let h = hash_coord(self.seed, wx, surface_y, wz, 0xC01D_0C0A);
        // Keep the vegetation roll independent from the height/biome fields.
        // Reusing the surface-correlated hash produced regression seeds with
        // thousands of grass columns but zero plants.
        let vegetation_hash = hash_coord(self.seed ^ 0xA511_E9B3, wx, 0, wz, 0x6D2B_79F5);
        let roll = vegetation_hash % 100;

        // Find the surface block.
        let Some(surface_block) =
            self.block_at_local(chunk, lx as i32, surface_y, lz as i32)
        else {
            return;
        };

        // Plants on grass.
        if surface_block == BlockType::Grass {
            if roll < 10 {
                self.set_block_local(chunk,
                    lx as i32,
                    surface_y + 1,
                    lz as i32,
                    BlockType::TallGrass,
                );
            } else if roll < 12 {
                self.set_block_local(chunk,
                    lx as i32,
                    surface_y + 1,
                    lz as i32,
                    BlockType::Dandelion,
                );
            } else if roll < 13 {
                self.set_block_local(chunk,
                    lx as i32,
                    surface_y + 1,
                    lz as i32,
                    BlockType::Poppy,
                );
            } else if roll < 14 && (biome == Biome::Plains || biome == Biome::Forest) {
                let veg = if (h >> 8) & 1 == 0 {
                    BlockType::Pumpkin
                } else {
                    BlockType::Melon
                };
                self.set_block_local(chunk,
                    lx as i32,
                    surface_y + 1,
                    lz as i32,
                    veg,
                );
            }
        }

        // Cactus in desert.
        if surface_block == BlockType::Sand && (biome == Biome::Desert || biome == Biome::Badlands)
        {
            if roll < 2 {
                let height = 1 + ((h >> 8) % 3) as i32;
                for dy in 1..=height {
                    self.set_block_local(chunk,
                        lx as i32,
                        surface_y + dy,
                        lz as i32,
                        BlockType::Cactus,
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
            let near_water = self.is_near_water(chunk, lx, surface_y, lz);
            if near_water {
                let cane_roll = hash_coord(self.seed, wx, surface_y, wz, 0x5A7A_317E);
                if cane_roll % 100 < 10 {
                    let height = (2 + (cane_roll >> 8) % 3) as i32;
                    for dy in 1..=height {
                        self.set_block_local(chunk,
                            lx as i32,
                            surface_y + dy,
                            lz as i32,
                            BlockType::SugarCane,
                        );
                    }
                }
            }
        }
    }

    fn is_near_water(
        &self,
        chunk: &Chunk,
        lx: usize,
        ly: i32,
        lz: usize,
    ) -> bool {
        for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let nx = lx as i32 + dx;
            let nz = lz as i32 + dz;
            if nx >= 0 && nx < CHUNK_WIDTH as i32 && nz >= 0 && nz < CHUNK_DEPTH as i32 {
                if let Some(BlockType::Water) =
                    self.block_at_local(chunk, nx, ly, nz)
                {
                    return true;
                }
            }
        }
        false
    }

    fn block_at_local(&self, chunk: &Chunk, x: i32, wy: i32, z: i32) -> Option<BlockType> {
        if x < 0 || z < 0 || x >= CHUNK_WIDTH as i32 || z >= CHUNK_DEPTH as i32 {
            return None;
        }
        if wy < chunk.min_world_y() || wy >= chunk.max_world_y_exclusive() {
            return None;
        }
        Some(chunk.get_block_local(x as usize, wy, z as usize))
    }

    fn set_block_local(&self, chunk: &mut Chunk, x: i32, wy: i32, z: i32, block: BlockType) {
        if x < 0 || z < 0 || x >= CHUNK_WIDTH as i32 || z >= CHUNK_DEPTH as i32 {
            return;
        }
        if wy < chunk.min_world_y() || wy >= chunk.max_world_y_exclusive() {
            return;
        }
        chunk.set_block_local(x as usize, wy, z as usize, block);
    }

    fn place_oak(
        &self,
        chunk: &mut Chunk,
        lx: i32,
        lz: i32,
        start_y: i32,
        height: i32,
    ) {
        for dy in 0..height {
            self.set_block_local(chunk,
                lx,
                start_y + dy,
                lz,
                BlockType::OakLog,
            );
        }
        for ly in (height - 3)..=height {
            let radius: i32 = if ly == height { 1 } else { 2 };
            for dx in -radius..=radius {
                for dz in -radius..=radius {
                    if radius == 2 && dx.abs() == 2 && dz.abs() == 2 {
                        continue;
                    }
                    let bx = lx + dx;
                    let bz = lz + dz;
                    let current = self.block_at_local(chunk, bx, start_y + ly, bz);
                    if matches!(
                        current,
                        None | Some(BlockType::Air) | Some(BlockType::OakLeaves)
                    ) {
                        self.set_block_local(chunk,
                            bx,
                            start_y + ly,
                            bz,
                            BlockType::OakLeaves,
                        );
                    }
                }
            }
        }
    }

    fn place_birch(
        &self,
        chunk: &mut Chunk,
        lx: i32,
        lz: i32,
        start_y: i32,
        height: i32,
    ) {
        for dy in 0..height {
            self.set_block_local(chunk,
                lx,
                start_y + dy,
                lz,
                BlockType::BirchLog,
            );
        }
        for ly in (height - 3)..=height {
            for dx in -1..=1 {
                for dz in -1..=1 {
                    let bx = lx + dx;
                    let bz = lz + dz;
                    let current = self.block_at_local(chunk, bx, start_y + ly, bz);
                    if matches!(current, None | Some(BlockType::Air)) {
                        self.set_block_local(chunk,
                            bx,
                            start_y + ly,
                            bz,
                            BlockType::BirchLeaves,
                        );
                    }
                }
            }
        }
    }

    fn place_spruce(
        &self,
        chunk: &mut Chunk,
        lx: i32,
        lz: i32,
        start_y: i32,
        height: i32,
    ) {
        for dy in 0..height {
            self.set_block_local(chunk,
                lx,
                start_y + dy,
                lz,
                BlockType::SpruceLog,
            );
        }
        for ly in (height - 6)..=height {
            let radius: i32 = if ly >= height - 1 { 1 } else { 2 };
            for dx in -radius..=radius {
                for dz in -radius..=radius {
                    if radius == 2 && dx.abs() == 2 && dz.abs() == 2 {
                        continue;
                    }
                    let bx = lx + dx;
                    let bz = lz + dz;
                    let current = self.block_at_local(chunk, bx, start_y + ly, bz);
                    if matches!(current, None | Some(BlockType::Air)) {
                        self.set_block_local(chunk,
                            bx,
                            start_y + ly,
                            bz,
                            BlockType::SpruceLeaves,
                        );
                    }
                }
            }
        }
    }

    fn place_jungle(
        &self,
        chunk: &mut Chunk,
        lx: i32,
        lz: i32,
        start_y: i32,
        height: i32,
    ) {
        for dy in 0..height {
            self.set_block_local(chunk,
                lx,
                start_y + dy,
                lz,
                BlockType::OakLog,
            );
        }
        for ly in (height - 4)..=height {
            let radius: i32 = if ly == height { 1 } else { 2 };
            for dx in -radius..=radius {
                for dz in -radius..=radius {
                    if radius == 2 && dx.abs() == 2 && dz.abs() == 2 {
                        continue;
                    }
                    let bx = lx + dx;
                    let bz = lz + dz;
                    let current = self.block_at_local(chunk, bx, start_y + ly, bz);
                    if matches!(current, None | Some(BlockType::Air)) {
                        self.set_block_local(chunk,
                            bx,
                            start_y + ly,
                            bz,
                            BlockType::OakLeaves,
                        );
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
    fn feature_placement_is_deterministic() {
        let ctx_a = WorldGenContext::new(12345);
        let ctx_b = WorldGenContext::new(12345);
        let placer_a = FeaturePlacer::new(12345);
        let placer_b = FeaturePlacer::new(12345);

        let mut chunk_a = Chunk::empty_in_dimension(Dimension::Overworld, 3, -2);
        let mut chunk_b = Chunk::empty_in_dimension(Dimension::Overworld, 3, -2);

        placer_a.place_features(&ctx_a, &mut chunk_a, 3, -2);
        placer_b.place_features(&ctx_b, &mut chunk_b, 3, -2);

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
    fn neighbor_trunk_at_x15_writes_leaves_into_chunk_x0() {
        let placer = FeaturePlacer::new(1);
        let mut this_chunk = Chunk::empty_in_dimension(Dimension::Overworld, 0, 0);
        placer.place_oak(&mut this_chunk, -1, 8, 70, 5);

        let mut found_leaf = false;
        for y in this_chunk.world_y_range() {
            for z in 0..CHUNK_DEPTH {
                if this_chunk.get_block_local(0, y, z) == BlockType::OakLeaves {
                    found_leaf = true;
                }
                assert_ne!(
                    this_chunk.get_block_local(0, y, z),
                    BlockType::OakLog,
                    "trunk at neighbor X=15 must not appear in this chunk"
                );
            }
        }
        assert!(
            found_leaf,
            "tree trunk at local X=15 of the west neighbor must write leaves at this chunk X=0"
        );
    }
}
