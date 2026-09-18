use crate::world::block::{BlockType, CHUNK_DEPTH, CHUNK_WIDTH};
use crate::world::section::{
    section_and_local_y_to_world_y, world_y_to_local_y, world_y_to_section_y, ChunkSection,
    NO_HEIGHT, SECTION_SIZE,
};
use crate::worldgen::surface::{self, BiomeSurfaceData};
use std::mem::{size_of, size_of_val};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockEntityError {
    OutOfBounds,
    TypeMismatch,
    ExceedsLimit,
}

impl std::fmt::Display for BlockEntityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BlockEntityError::OutOfBounds => write!(f, "block entity position out of bounds"),
            BlockEntityError::TypeMismatch => {
                write!(f, "block entity type mismatch with block at position")
            }
            BlockEntityError::ExceedsLimit => write!(f, "chunk block entity count exceeds limit"),
        }
    }
}

impl std::error::Error for BlockEntityError {}

fn is_furnace_block(block: BlockType) -> bool {
    matches!(block, BlockType::Furnace)
}

fn is_hopper_block(block: BlockType) -> bool {
    matches!(block, BlockType::Hopper)
}

#[inline]
fn update_index_membership(
    positions: &mut Vec<u32>,
    encoded: u32,
    old_member: bool,
    new_member: bool,
) {
    if old_member && !new_member {
        if let Some(index) = positions.iter().position(|&p| p == encoded) {
            positions.swap_remove(index);
        }
    } else if new_member && !old_member {
        positions.push(encoded);
    }
}

#[derive(Clone)]
pub struct Chunk {
    pub chunk_x: i32,
    pub chunk_z: i32,
    pub min_section_y: i8,
    pub sections: Vec<Option<ChunkSection>>,
    /// Per-column max Y of non-air blocks (indexed as [x][z])
    pub heightmap: Box<[[i16; CHUNK_DEPTH]; CHUNK_WIDTH]>,
    /// Compact local coordinates of ordinary torch blocks.
    pub(crate) torch_positions: Vec<u32>,
    /// Compact local coordinates of redstone component blocks.
    pub(crate) redstone_positions: Vec<u32>,
    /// Compact local coordinates of furnace / lit-furnace blocks.
    pub(crate) furnace_positions: Vec<u32>,
    /// Compact local coordinates of hopper blocks (same encoding as furnaces).
    pub(crate) hopper_positions: Vec<u32>,
    /// Ascending `section_y` values whose `random_tick_count() > 0`.
    /// Maintained on load / `set_block_local` so authority sampling never
    /// rescans empty sections each tick.
    pub(crate) random_tick_sections: Vec<i8>,
    /// Block entities keyed by Chunk-local coordinates (x: u8, y: i16, z: u8).
    pub(crate) block_entities:
        std::collections::HashMap<(u8, i16, u8), crate::block_entity::BlockEntity>,
}

impl Chunk {
    pub fn empty(chunk_x: i32, chunk_z: i32) -> Self {
        Self::empty_in_dimension(crate::dimension::Dimension::Overworld, chunk_x, chunk_z)
    }

    pub fn empty_in_dimension(
        dimension: crate::dimension::Dimension,
        chunk_x: i32,
        chunk_z: i32,
    ) -> Self {
        let height = dimension.height();
        let section_count = height.section_count();
        Self {
            chunk_x,
            chunk_z,
            min_section_y: height.min_section_y(),
            sections: vec![None; section_count],
            heightmap: vec![[NO_HEIGHT; CHUNK_DEPTH]; CHUNK_WIDTH]
                .try_into()
                .unwrap(),
            torch_positions: Vec::new(),
            redstone_positions: Vec::new(),
            furnace_positions: Vec::new(),
            hopper_positions: Vec::new(),
            random_tick_sections: Vec::new(),
            block_entities: std::collections::HashMap::new(),
        }
    }

    pub fn new(chunk_x: i32, chunk_z: i32) -> Self {
        Self::new_with_seed(chunk_x, chunk_z, 12345)
    }

    pub fn new_with_seed(chunk_x: i32, chunk_z: i32, world_seed: u32) -> Self {
        let height = crate::dimension::WorldHeight::OVERWORLD;
        let min_y = height.min_y();
        let mut chunk = Self::empty_in_dimension(
            crate::dimension::Dimension::Overworld,
            chunk_x,
            chunk_z,
        );

        let ctx = crate::worldgen::WorldGenContext::new(world_seed);

        // Fill terrain density and carve caves directly into paletted sections.
        for x in 0..CHUNK_WIDTH {
            for z in 0..CHUNK_DEPTH {
                let wx = chunk_x * CHUNK_WIDTH as i32 + x as i32;
                let wz = chunk_z * CHUNK_DEPTH as i32 + z as i32;
                let surface_y = ctx.surface_height_at(wx, wz);
                let biome = ctx.biome_at(wx, wz);
                let surface = BiomeSurfaceData::for_biome(biome);

                for wy in min_y..height.max_y_exclusive() {
                    let block = surface::block_for_column(wy, surface_y, &surface)
                        .unwrap_or(BlockType::Air);
                    if block != BlockType::Air {
                        chunk.set_block_local(x, wy, z, block);
                    }
                }

                // Carve caves after surface generation. Density fill only places
                // solids at or below the surface, so the carver has nothing to do
                // above `surface_y`.
                let carve_top = surface_y.min(height.max_y_exclusive() - 1);
                for wy in min_y..=carve_top {
                    let current = chunk.get_block_local(x, wy, z);
                    if current == BlockType::Air || current == BlockType::Water {
                        continue;
                    }
                    if ctx.carver.is_carved(wx, wy, wz, surface_y) {
                        if ctx.carver.is_lava_lake(wx, wy, wz) {
                            chunk.set_block_local(x, wy, z, BlockType::Lava);
                        } else {
                            chunk.set_block_local(x, wy, z, BlockType::Air);
                        }
                    }
                }
            }
        }

        ctx.ore.place_ores(&mut chunk, chunk_x, chunk_z);
        crate::worldgen::feature::FeaturePlacer::new(world_seed).place_features(
            &ctx, &mut chunk, chunk_x, chunk_z,
        );

        for x in 0..CHUNK_WIDTH {
            for z in 0..CHUNK_DEPTH {
                chunk.update_heightmap(x, z);
            }
        }
        chunk.recompute_direct_column_lighting();
        chunk
    }

    /// Encodes local `(x, y, z)` coordinates into a compact torch/component index.
    pub fn encode_torch_position(x: usize, y: i32, z: usize) -> u32 {
        (x as u32) | ((z as u32) << 4) | (((y as u32) & 0xFFFF) << 8)
    }

    /// Decodes a compact local torch/component index into `(x, y, z)` coordinates.
    pub fn decode_torch_position(index: u32) -> (usize, i32, usize) {
        (
            (index & 0x0f) as usize,
            ((index >> 8) as u16) as i16 as i32,
            ((index >> 4) & 0x0f) as usize,
        )
    }

    /// Returns the indexed local positions of ordinary torches.
    pub fn torch_positions(&self) -> &[u32] {
        &self.torch_positions
    }

    /// Returns the indexed local positions of redstone components.
    pub fn redstone_positions(&self) -> &[u32] {
        &self.redstone_positions
    }

    /// Returns the indexed local positions of furnace blocks.
    pub fn furnace_positions(&self) -> &[u32] {
        &self.furnace_positions
    }

    /// Returns the indexed local positions of hopper blocks.
    pub fn hopper_positions(&self) -> &[u32] {
        &self.hopper_positions
    }

    /// Returns ascending section Y values that still have random-tickable blocks.
    pub fn random_tick_sections(&self) -> &[i8] {
        &self.random_tick_sections
    }

    /// Bytes owned by this chunk, including representation-specific section
    /// storage, vector spare capacity, and the boxed heightmap allocation.
    pub fn memory_usage(&self) -> usize {
        size_of::<Self>()
            + self.sections.capacity() * size_of::<Option<ChunkSection>>()
            + self
                .sections
                .iter()
                .filter_map(|s| s.as_ref())
                .map(|section| {
                    section
                        .memory_usage()
                        .saturating_sub(size_of::<ChunkSection>())
                })
                .sum::<usize>()
            + size_of_val(self.heightmap.as_ref())
            + self.torch_positions.capacity() * size_of::<u32>()
            + self.redstone_positions.capacity() * size_of::<u32>()
            + self.furnace_positions.capacity() * size_of::<u32>()
            + self.hopper_positions.capacity() * size_of::<u32>()
            + self.random_tick_sections.capacity() * size_of::<i8>()
            + self.block_entities.capacity()
                * (size_of::<(u8, i16, u8)>() + size_of::<crate::block_entity::BlockEntity>())
            + self
                .block_entities
                .values()
                .map(|e| e.memory_usage())
                .sum::<usize>()
    }

    pub fn section_index(&self, section_y: i8) -> Option<usize> {
        let idx = (section_y as i32) - (self.min_section_y as i32);
        if idx >= 0 && (idx as usize) < self.sections.len() {
            Some(idx as usize)
        } else {
            None
        }
    }

    pub fn section_y_at_index(&self, index: usize) -> i8 {
        self.min_section_y + index as i8
    }

    /// Lowest world Y covered by this column's first section.
    pub fn min_world_y(&self) -> i32 {
        section_and_local_y_to_world_y(self.min_section_y, 0)
    }

    /// Exclusive upper world Y of this column's last section.
    pub fn max_world_y_exclusive(&self) -> i32 {
        self.min_world_y() + (self.sections.len() as i32) * SECTION_SIZE as i32
    }

    /// World-Y span of stored sections (`min_section_y` + `sections.len()`).
    pub fn world_y_range(&self) -> std::ops::Range<i32> {
        self.min_world_y()..self.max_world_y_exclusive()
    }

    pub fn get_block_entity(
        &self,
        x: u8,
        y: i16,
        z: u8,
    ) -> Option<&crate::block_entity::BlockEntity> {
        if (x as usize) >= CHUNK_WIDTH || (z as usize) >= CHUNK_DEPTH {
            return None;
        }
        self.block_entities.get(&(x, y, z))
    }

    pub fn get_block_entity_mut(
        &mut self,
        x: u8,
        y: i16,
        z: u8,
    ) -> Option<&mut crate::block_entity::BlockEntity> {
        if (x as usize) >= CHUNK_WIDTH || (z as usize) >= CHUNK_DEPTH {
            return None;
        }
        self.block_entities.get_mut(&(x, y, z))
    }

    pub fn insert_block_entity(
        &mut self,
        x: u8,
        y: i16,
        z: u8,
        entity: crate::block_entity::BlockEntity,
    ) -> Result<(), BlockEntityError> {
        if (x as usize) >= CHUNK_WIDTH || (z as usize) >= CHUNK_DEPTH {
            return Err(BlockEntityError::OutOfBounds);
        }
        let block_type = self.get_block_local(x as usize, y as i32, z as usize);
        if !entity.matches_block_type(block_type) {
            return Err(BlockEntityError::TypeMismatch);
        }
        if self.block_entities.len() >= 4096 && !self.block_entities.contains_key(&(x, y, z)) {
            return Err(BlockEntityError::ExceedsLimit);
        }
        self.block_entities.insert((x, y, z), entity);
        Ok(())
    }

    pub fn remove_block_entity(
        &mut self,
        x: u8,
        y: i16,
        z: u8,
    ) -> Option<crate::block_entity::BlockEntity> {
        if (x as usize) >= CHUNK_WIDTH || (z as usize) >= CHUNK_DEPTH {
            return None;
        }
        self.block_entities.remove(&(x, y, z))
    }

    pub fn iter_block_entities(
        &self,
    ) -> impl Iterator<Item = ((u8, i16, u8), &crate::block_entity::BlockEntity)> {
        self.block_entities
            .iter()
            .map(|(&pos, entity)| (pos, entity))
    }

    /// Rebuilds derived block and section indexes after bulk mutations (generation/load).
    pub fn rebuild_derived_indexes(&mut self) {
        self.torch_positions.clear();
        self.redstone_positions.clear();
        self.furnace_positions.clear();
        self.hopper_positions.clear();
        self.random_tick_sections.clear();

        for (sec_idx, sec_opt) in self.sections.iter().enumerate() {
            let Some(sec) = sec_opt else {
                continue;
            };
            let sec_y = self.min_section_y + sec_idx as i8;
            if sec.random_tick_count() > 0 {
                self.random_tick_sections.push(sec_y);
            }
            if sec.non_air_count() == 0 {
                continue;
            }
            for ly in 0..SECTION_SIZE {
                let wy = section_and_local_y_to_world_y(sec_y, ly as u8);
                for z in 0..CHUNK_DEPTH {
                    for x in 0..CHUNK_WIDTH {
                        let idx = (ly << 8) | (z << 4) | x;
                        let block = sec.get_block(idx);
                        if block == BlockType::Air {
                            continue;
                        }
                        let encoded = Self::encode_torch_position(x, wy, z);
                        if block == BlockType::Torch {
                            self.torch_positions.push(encoded);
                        } else if crate::redstone::is_component(block) {
                            self.redstone_positions.push(encoded);
                        } else if is_furnace_block(block) {
                            self.furnace_positions.push(encoded);
                        } else if is_hopper_block(block) {
                            self.hopper_positions.push(encoded);
                        }
                    }
                }
            }
        }
    }

    /// Sets a local block and keeps the torch and redstone indices synchronized.
    pub fn set_block_local(&mut self, x: usize, wy: i32, z: usize, block: BlockType) {
        let sec_y = world_y_to_section_y(wy);
        let Some(sec_idx) = self.section_index(sec_y) else {
            return;
        };
        let ly = world_y_to_local_y(wy) as usize;
        let idx = (ly << 8) | (z << 4) | x;

        let sec_y_val = self.section_y_at_index(sec_idx);
        let sec = self.sections[sec_idx].get_or_insert_with(|| ChunkSection::new(sec_y_val));
        let old = sec.set_block(idx, block);
        if old == block {
            return;
        }
        let random_tick_count = sec.random_tick_count();

        if let Some(entity) = self.block_entities.get(&(x as u8, wy as i16, z as u8)) {
            if !entity.matches_block_type(block) {
                self.block_entities.remove(&(x as u8, wy as i16, z as u8));
            }
        }

        let encoded = Self::encode_torch_position(x, wy, z);
        update_index_membership(
            &mut self.torch_positions,
            encoded,
            old == BlockType::Torch,
            block == BlockType::Torch,
        );
        update_index_membership(
            &mut self.redstone_positions,
            encoded,
            crate::redstone::is_component(old),
            crate::redstone::is_component(block),
        );
        update_index_membership(
            &mut self.furnace_positions,
            encoded,
            is_furnace_block(old),
            is_furnace_block(block),
        );
        update_index_membership(
            &mut self.hopper_positions,
            encoded,
            is_hopper_block(old),
            is_hopper_block(block),
        );

        match self.random_tick_sections.binary_search(&sec_y_val) {
            Ok(index) if random_tick_count == 0 => {
                self.random_tick_sections.remove(index);
            }
            Err(index) if random_tick_count > 0 => {
                self.random_tick_sections.insert(index, sec_y_val);
            }
            _ => {}
        }
    }

    /// Update heightmap for a single column after block placement/removal
    pub fn update_heightmap(&mut self, x: usize, z: usize) {
        for sec_idx in (0..self.sections.len()).rev() {
            if let Some(ref sec) = self.sections[sec_idx] {
                if sec.non_air_count() == 0 {
                    continue;
                }
                let sec_y = self.section_y_at_index(sec_idx);
                for ly in (0..SECTION_SIZE).rev() {
                    let idx = (ly << 8) | (z << 4) | x;
                    if sec.get_block(idx) != BlockType::Air {
                        let wy = section_and_local_y_to_world_y(sec_y, ly as u8);
                        self.heightmap[x][z] = wy as i16;
                        return;
                    }
                }
            }
        }
        self.heightmap[x][z] = NO_HEIGHT;
    }

    pub fn get_block_local(&self, x: usize, wy: i32, z: usize) -> BlockType {
        let sec_y = world_y_to_section_y(wy);
        let Some(sec_idx) = self.section_index(sec_y) else {
            return BlockType::Air;
        };
        let Some(ref sec) = self.sections[sec_idx] else {
            return BlockType::Air;
        };
        let ly = world_y_to_local_y(wy) as usize;
        let idx = (ly << 8) | (z << 4) | x;
        sec.get_block(idx)
    }

    pub fn get_block(&self, x: i32, wy: i32, z: i32) -> BlockType {
        if x < 0 || x >= CHUNK_WIDTH as i32 || z < 0 || z >= CHUNK_DEPTH as i32 {
            return BlockType::Air;
        }
        self.get_block_local(x as usize, wy, z as usize)
    }

    pub fn get_sky_light(&self, x: usize, wy: i32, z: usize) -> u8 {
        let sec_y = world_y_to_section_y(wy);
        let Some(sec_idx) = self.section_index(sec_y) else {
            return 0;
        };
        let Some(ref sec) = self.sections[sec_idx] else {
            return 0;
        };
        let ly = world_y_to_local_y(wy) as usize;
        let idx = (ly << 8) | (z << 4) | x;
        sec.light.get_sky(idx)
    }

    pub fn set_sky_light(&mut self, x: usize, wy: i32, z: usize, val: u8) {
        let sec_y = world_y_to_section_y(wy);
        let Some(sec_idx) = self.section_index(sec_y) else {
            return;
        };
        let sec_y_val = self.section_y_at_index(sec_idx);
        let sec = self.sections[sec_idx].get_or_insert_with(|| ChunkSection::new(sec_y_val));
        let ly = world_y_to_local_y(wy) as usize;
        let idx = (ly << 8) | (z << 4) | x;
        sec.light.set_sky(idx, val);
        sec.storage_changes = sec.storage_changes.saturating_add(1);
    }

    pub fn get_block_light(&self, x: usize, wy: i32, z: usize) -> u8 {
        let sec_y = world_y_to_section_y(wy);
        let Some(sec_idx) = self.section_index(sec_y) else {
            return 0;
        };
        let Some(ref sec) = self.sections[sec_idx] else {
            return 0;
        };
        let ly = world_y_to_local_y(wy) as usize;
        let idx = (ly << 8) | (z << 4) | x;
        sec.light.get_block(idx)
    }

    pub fn set_block_light(&mut self, x: usize, wy: i32, z: usize, val: u8) {
        let sec_y = world_y_to_section_y(wy);
        let Some(sec_idx) = self.section_index(sec_y) else {
            return;
        };
        let sec_y_val = self.section_y_at_index(sec_idx);
        let sec = self.sections[sec_idx].get_or_insert_with(|| ChunkSection::new(sec_y_val));
        let ly = world_y_to_local_y(wy) as usize;
        let idx = (ly << 8) | (z << 4) | x;
        sec.light.set_block(idx, val);
        sec.storage_changes = sec.storage_changes.saturating_add(1);
    }

    pub fn get_block_state(&self, x: i32, wy: i32, z: i32) -> u8 {
        if x < 0 || x >= CHUNK_WIDTH as i32 || z < 0 || z >= CHUNK_DEPTH as i32 {
            return 0;
        }
        let sec_y = world_y_to_section_y(wy);
        let Some(sec_idx) = self.section_index(sec_y) else {
            return 0;
        };
        let Some(ref sec) = self.sections[sec_idx] else {
            return 0;
        };
        let ly = world_y_to_local_y(wy) as usize;
        let idx = (ly << 8) | ((z as usize) << 4) | (x as usize);
        sec.get_block_state(idx)
    }

    pub fn set_block_state(&mut self, x: i32, wy: i32, z: i32, state: u8) {
        if x < 0 || x >= CHUNK_WIDTH as i32 || z < 0 || z >= CHUNK_DEPTH as i32 {
            return;
        }
        let sec_y = world_y_to_section_y(wy);
        let Some(sec_idx) = self.section_index(sec_y) else {
            return;
        };
        let sec_y_val = self.section_y_at_index(sec_idx);
        let sec = self.sections[sec_idx].get_or_insert_with(|| ChunkSection::new(sec_y_val));
        let ly = world_y_to_local_y(wy) as usize;
        let idx = (ly << 8) | ((z as usize) << 4) | (x as usize);
        sec.set_block_state(idx, state);
    }

    pub fn get_fluid_level(&self, x: usize, wy: i32, z: usize) -> u8 {
        let sec_y = world_y_to_section_y(wy);
        let Some(sec_idx) = self.section_index(sec_y) else {
            return 0;
        };
        let Some(ref sec) = self.sections[sec_idx] else {
            return 0;
        };
        let ly = world_y_to_local_y(wy) as usize;
        let idx = (ly << 8) | (z << 4) | x;
        sec.get_fluid_level(idx)
    }

    pub fn set_fluid_level(&mut self, x: usize, wy: i32, z: usize, level: u8) {
        let sec_y = world_y_to_section_y(wy);
        let Some(sec_idx) = self.section_index(sec_y) else {
            return;
        };
        let sec_y_val = self.section_y_at_index(sec_idx);
        let sec = self.sections[sec_idx].get_or_insert_with(|| ChunkSection::new(sec_y_val));
        let ly = world_y_to_local_y(wy) as usize;
        let idx = (ly << 8) | (z << 4) | x;
        sec.set_fluid_level(idx, level);
    }

    /// Rebuild column sky/block light from the current blocks. Used when a
    /// network payload omits light streams so join clients are not left dark.
    pub fn recompute_direct_column_lighting(&mut self) {
        let min_y = self.min_world_y();
        let max_y = self.max_world_y_exclusive();
        let enable_sky = self.min_section_y < 0 || (max_y - min_y) > 128;
        for x in 0..CHUNK_WIDTH {
            for z in 0..CHUNK_DEPTH {
                let mut direct_sky = if enable_sky { 15u8 } else { 0u8 };
                for wy in (min_y..max_y).rev() {
                    let block = self.get_block_local(x, wy, z);
                    let props = &block.def().properties;
                    if enable_sky && props.is_opaque() {
                        direct_sky = 0;
                    }
                    self.set_sky_light(x, wy, z, direct_sky);
                    self.set_block_light(x, wy, z, props.light_emission);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use noise::{NoiseFn, Perlin};

    #[test]
    fn world_seed_changes_generated_terrain() {
        let chunk_a = Chunk::new_with_seed(0, 0, 11111);
        let chunk_b = Chunk::new_with_seed(0, 0, 22222);
        assert_ne!(
            chunk_a.heightmap, chunk_b.heightmap,
            "different world seeds should produce different heightmaps"
        );
    }

    #[test]
    fn test_cave_generation() {
        let chunk = Chunk::new(0, 0);
        let mut air_underground = 0;
        let mut stone_underground = 0;
        for x in 0..CHUNK_WIDTH {
            for z in 0..CHUNK_DEPTH {
                for y in 5..50 {
                    let block = chunk.get_block_local(x, y, z);
                    if block == BlockType::Air {
                        air_underground += 1;
                    } else if block == BlockType::Stone {
                        stone_underground += 1;
                    }
                }
            }
        }
        assert!(
            air_underground > 0,
            "Caves should carve some air underground"
        );
        assert!(
            stone_underground > 0,
            "Caves should leave some stone underground"
        );
    }

    #[test]
    fn test_ore_clustering() {
        let chunk = Chunk::new(0, 0);
        let mut clustered = false;
        let mut coal_count = 0;
        for x in 0..CHUNK_WIDTH {
            for z in 0..CHUNK_DEPTH {
                for y in chunk.world_y_range() {
                    if chunk.get_block_local(x, y, z) == BlockType::CoalOre {
                        coal_count += 1;
                        let neighbors = [
                            (x as i32 + 1, y, z as i32),
                            (x as i32 - 1, y, z as i32),
                            (x as i32, y + 1, z as i32),
                            (x as i32, y - 1, z as i32),
                            (x as i32, y, z as i32 + 1),
                            (x as i32, y, z as i32 - 1),
                        ];
                        for &(nx, ny, nz) in &neighbors {
                            if nx >= 0
                                && nx < CHUNK_WIDTH as i32
                                && nz >= 0
                                && nz < CHUNK_DEPTH as i32
                                && ny >= chunk.min_world_y()
                                && ny < chunk.max_world_y_exclusive()
                            {
                                if chunk.get_block_local(nx as usize, ny, nz as usize)
                                    == BlockType::CoalOre
                                {
                                    clustered = true;
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }
        assert!(coal_count > 0, "Coal should be generated in the chunk");
        assert!(clustered, "Coal ores should generate in clusters (veins)");
    }

    #[test]
    fn test_cave_entrances() {
        let perlin = Perlin::new(12345);
        let mut found_chunk = None;
        for cx in -20..20 {
            for cz in -20..20 {
                let mut found_entrance = false;
                for x in 0..CHUNK_WIDTH {
                    for z in 0..CHUNK_DEPTH {
                        let world_x = cx * CHUNK_WIDTH as i32 + x as i32;
                        let world_z = cz * CHUNK_DEPTH as i32 + z as i32;
                        let noise_val = perlin.get([world_x as f64 * 0.04, world_z as f64 * 0.04]);
                        let base_height = (64.0 + noise_val * 12.0) as usize;
                        let entrance_noise =
                            perlin.get([world_x as f64 * 0.015, world_z as f64 * 0.015]);
                        if entrance_noise > 0.55 && base_height > 63 {
                            found_entrance = true;
                            break;
                        }
                    }
                    if found_entrance {
                        break;
                    }
                }
                if found_entrance {
                    found_chunk = Some((cx, cz));
                    break;
                }
            }
            if found_chunk.is_some() {
                break;
            }
        }

        assert!(
            found_chunk.is_some(),
            "Should find a chunk with entrance zone in range"
        );
        let (cx, cz) = found_chunk.unwrap();
        let chunk = Chunk::new(cx, cz);

        let mut found_surface_air = false;
        for x in 0..CHUNK_WIDTH {
            for z in 0..CHUNK_DEPTH {
                let world_x = cx * CHUNK_WIDTH as i32 + x as i32;
                let world_z = cz * CHUNK_DEPTH as i32 + z as i32;
                let noise_val = perlin.get([world_x as f64 * 0.04, world_z as f64 * 0.04]);
                let base_height = (64.0 + noise_val * 12.0) as usize;
                let entrance_noise = perlin.get([world_x as f64 * 0.015, world_z as f64 * 0.015]);
                if entrance_noise > 0.55 && base_height > 63 {
                    if chunk.get_block_local(x, base_height as i32, z) == BlockType::Air {
                        found_surface_air = true;
                        break;
                    }
                }
            }
            if found_surface_air {
                break;
            }
        }
        assert!(
            found_surface_air,
            "Should carve some cave air at surface in entrance zones"
        );
    }

    #[test]
    fn test_fluid_level_encoding() {
        let mut chunk = Chunk::new(0, 0);
        chunk.set_fluid_level(0, 10, 0, 5 | 0x08);
        assert_eq!(chunk.get_fluid_level(0, 10, 0) & 0x07, 5);
        assert_eq!((chunk.get_fluid_level(0, 10, 0) & 0x08) != 0, true);
    }

    #[test]
    fn torch_index_tracks_local_mutations_without_duplicates() {
        let mut chunk = Chunk::new(0, 0);
        assert!(chunk.torch_positions().is_empty());
        chunk.set_block_local(3, 40, 5, BlockType::Torch);
        assert_eq!(chunk.torch_positions().len(), 1);
        let encoded = chunk.torch_positions()[0];
        assert_eq!(Chunk::decode_torch_position(encoded), (3, 40, 5));
        chunk.set_block_local(3, 40, 5, BlockType::Torch);
        assert_eq!(chunk.torch_positions().len(), 1);
        chunk.set_block_local(3, 40, 5, BlockType::Stone);
        assert!(chunk.torch_positions().is_empty());
    }

    #[test]
    fn furnace_index_tracks_local_mutations_without_duplicates() {
        let mut chunk = Chunk::new(0, 0);
        assert!(chunk.furnace_positions().is_empty());
        chunk.set_block_local(3, 40, 5, BlockType::Furnace);
        assert_eq!(chunk.furnace_positions().len(), 1);
        let encoded = chunk.furnace_positions()[0];
        assert_eq!(Chunk::decode_torch_position(encoded), (3, 40, 5));
        chunk.set_block_local(3, 40, 5, BlockType::Furnace);
        assert_eq!(chunk.furnace_positions().len(), 1);
        chunk.set_block_local(3, 40, 5, BlockType::Furnace);
        assert_eq!(chunk.furnace_positions().len(), 1);
        chunk.set_block_local(3, 40, 5, BlockType::Stone);
        assert!(chunk.furnace_positions().is_empty());
    }

    #[test]
    fn hopper_index_tracks_local_mutations_without_duplicates() {
        let mut chunk = Chunk::new(0, 0);
        assert!(chunk.hopper_positions().is_empty());
        chunk.set_block_local(3, 40, 5, BlockType::Hopper);
        assert_eq!(chunk.hopper_positions().len(), 1);
        let encoded = chunk.hopper_positions()[0];
        assert_eq!(Chunk::decode_torch_position(encoded), (3, 40, 5));
        chunk.set_block_local(3, 40, 5, BlockType::Hopper);
        assert_eq!(chunk.hopper_positions().len(), 1);
        chunk.set_block_local(3, 40, 5, BlockType::Stone);
        assert!(chunk.hopper_positions().is_empty());
    }

    #[test]
    fn random_tick_index_tracks_section_eligibility() {
        let mut chunk = Chunk::empty(0, 0);
        assert!(chunk.random_tick_sections().is_empty());
        chunk.set_block_local(1, 20, 2, BlockType::WheatCrop);
        assert_eq!(chunk.random_tick_sections(), &[1]);
        chunk.set_block_local(2, 20, 2, BlockType::WheatCrop);
        assert_eq!(chunk.random_tick_sections(), &[1]);
        chunk.set_block_local(1, 20, 2, BlockType::Stone);
        assert_eq!(chunk.random_tick_sections(), &[1]);
        chunk.set_block_local(2, 20, 2, BlockType::Stone);
        assert!(chunk.random_tick_sections().is_empty());
        chunk.set_block_local(0, -10, 0, BlockType::Fire);
        assert_eq!(chunk.random_tick_sections(), &[-1]);
        chunk.rebuild_derived_indexes();
        assert_eq!(chunk.random_tick_sections(), &[-1]);
    }

    #[test]
    fn chunk_memory_usage_tracks_section_promotion_and_demotion() {
        let mut chunk = Chunk {
            chunk_x: 0,
            chunk_z: 0,
            min_section_y: -4,
            sections: (0..24).map(|_| Some(ChunkSection::empty_dark())).collect(),
            heightmap: Box::new([[NO_HEIGHT; CHUNK_DEPTH]; CHUNK_WIDTH]),
            torch_positions: Vec::new(),
            redstone_positions: Vec::new(),
            furnace_positions: Vec::new(),
            hopper_positions: Vec::new(),
            random_tick_sections: Vec::new(),
            block_entities: std::collections::HashMap::new(),
        };
        let empty_bytes = chunk.memory_usage();
        chunk.set_block_local(0, 0, 0, BlockType::Stone);
        let promoted_bytes = chunk.memory_usage();
        assert!(promoted_bytes > empty_bytes);

        chunk.set_block_local(0, 0, 0, BlockType::Air);
        if let Some(ref mut sec) = chunk.sections[4] {
            assert!(sec.compact_if_worthwhile());
        }
        assert!(chunk.memory_usage() < promoted_bytes);
    }

    #[test]
    fn chunk_block_entity_operations() {
        use crate::block_entity::{BlockEntity, ChestBlockEntity, FurnaceBlockEntity};

        let mut chunk = Chunk::new(0, 0);
        chunk.set_block_local(4, 10, 4, BlockType::Chest);

        let chest_entity = BlockEntity::Chest(ChestBlockEntity {
            inventory: crate::inventory::ContainerInventory::new(),
            custom_name: None,
            loot_table: None,
            loot_seed: None,
            revision: 0,
        });
        // Valid insert
        assert_eq!(
            chunk.insert_block_entity(4, 10, 4, chest_entity.clone()),
            Ok(())
        );
        assert_eq!(chunk.get_block_entity(4, 10, 4), Some(&chest_entity));

        // Out of bounds insert
        assert_eq!(
            chunk.insert_block_entity(16, 10, 4, chest_entity.clone()),
            Err(BlockEntityError::OutOfBounds)
        );

        // Type mismatch insert
        chunk.set_block_local(5, 10, 4, BlockType::Stone);
        let furnace_entity = BlockEntity::Furnace(FurnaceBlockEntity::new());
        assert_eq!(
            chunk.insert_block_entity(5, 10, 4, furnace_entity),
            Err(BlockEntityError::TypeMismatch)
        );

        // Changing state preserves block entity
        chunk.set_block_state(4, 10, 4, 2);
        assert_eq!(chunk.get_block_entity(4, 10, 4), Some(&chest_entity));

        // Changing block type auto-removes block entity
        chunk.set_block_local(4, 10, 4, BlockType::Air);
        assert_eq!(chunk.get_block_entity(4, 10, 4), None);
    }

    #[test]
    fn redstone_index_tracks_component_mutations_without_loss_or_duplicates() {
        let mut chunk = Chunk::new(0, 0);
        assert!(chunk.redstone_positions().is_empty());

        // Place initial redstone component
        chunk.set_block_local(3, 40, 5, BlockType::RedstoneWire);
        assert_eq!(chunk.redstone_positions().len(), 1);
        let encoded = chunk.redstone_positions()[0];
        assert_eq!(Chunk::decode_torch_position(encoded), (3, 40, 5));

        // Component -> component mutation must NOT lose the index
        chunk.set_block_local(3, 40, 5, BlockType::Repeater);
        assert_eq!(chunk.redstone_positions().len(), 1);
        assert_eq!(chunk.redstone_positions()[0], encoded);

        // Transition to yet another component
        chunk.set_block_local(3, 40, 5, BlockType::Comparator);
        assert_eq!(chunk.redstone_positions().len(), 1);
        assert_eq!(chunk.redstone_positions()[0], encoded);

        // Setting the same component again does not duplicate
        chunk.set_block_local(3, 40, 5, BlockType::Comparator);
        assert_eq!(chunk.redstone_positions().len(), 1);

        // Back to RedstoneWire
        chunk.set_block_local(3, 40, 5, BlockType::RedstoneWire);
        assert_eq!(chunk.redstone_positions().len(), 1);
        assert_eq!(chunk.redstone_positions()[0], encoded);

        // Non-redstone block removes it
        chunk.set_block_local(3, 40, 5, BlockType::Stone);
        assert!(chunk.redstone_positions().is_empty());

        // Setting non-redstone again does nothing
        chunk.set_block_local(3, 40, 5, BlockType::Stone);
        assert!(chunk.redstone_positions().is_empty());

        // Re-adding component adds it back
        chunk.set_block_local(3, 40, 5, BlockType::Lever);
        assert_eq!(chunk.redstone_positions().len(), 1);
        assert_eq!(chunk.redstone_positions()[0], encoded);

        // Multiple other component transitions
        chunk.set_block_local(3, 40, 5, BlockType::OakDoor);
        assert_eq!(chunk.redstone_positions().len(), 1);
        assert_eq!(chunk.redstone_positions()[0], encoded);

        chunk.set_block_local(3, 40, 5, BlockType::TNT);
        assert_eq!(chunk.redstone_positions().len(), 1);
        assert_eq!(chunk.redstone_positions()[0], encoded);

        chunk.set_block_local(3, 40, 5, BlockType::Air);
        assert!(chunk.redstone_positions().is_empty());
    }

    #[test]
    fn random_tick_index_handles_negative_sections_and_last_block_removal() {
        let mut chunk = Chunk {
            chunk_x: 0,
            chunk_z: 0,
            min_section_y: -4,
            sections: (0..24).map(|_| None).collect(),
            heightmap: Box::new([[NO_HEIGHT; CHUNK_DEPTH]; CHUNK_WIDTH]),
            torch_positions: Vec::new(),
            redstone_positions: Vec::new(),
            furnace_positions: Vec::new(),
            hopper_positions: Vec::new(),
            random_tick_sections: Vec::new(),
            block_entities: std::collections::HashMap::new(),
        };

        assert!(chunk.random_tick_sections().is_empty());

        // Place random-tick blocks in negative sections:
        // world Y -60 -> section Y -4
        // world Y -30 -> section Y -2
        // world Y 10  -> section Y 0
        chunk.set_block_local(1, -60, 1, BlockType::Fire);
        chunk.set_block_local(2, -30, 2, BlockType::WheatCrop);
        chunk.set_block_local(3, 10, 3, BlockType::OakLeaves);

        assert_eq!(chunk.random_tick_sections(), &[-4, -2, 0]);

        // Place a second random-tick block in section -2
        chunk.set_block_local(5, -28, 5, BlockType::Cactus);
        assert_eq!(chunk.random_tick_sections(), &[-4, -2, 0]);

        // Rebuilding derived indexes yields identical sorted sections
        chunk.rebuild_derived_indexes();
        assert_eq!(chunk.random_tick_sections(), &[-4, -2, 0]);

        // Remove the first block in section -2; section remains eligible
        chunk.set_block_local(2, -30, 2, BlockType::Stone);
        assert_eq!(chunk.random_tick_sections(), &[-4, -2, 0]);

        // Remove the last random-tick block in section -2; section is demoted
        chunk.set_block_local(5, -28, 5, BlockType::Air);
        assert_eq!(chunk.random_tick_sections(), &[-4, 0]);

        // Rebuild confirms consistency
        chunk.rebuild_derived_indexes();
        assert_eq!(chunk.random_tick_sections(), &[-4, 0]);
    }

    #[test]
    fn bulk_restore_and_rebuild_derived_indexes_match_exhaustive_scan() {
        let mut chunk = Chunk {
            chunk_x: 0,
            chunk_z: 0,
            min_section_y: -4,
            sections: (0..24).map(|_| None).collect(),
            heightmap: Box::new([[NO_HEIGHT; CHUNK_DEPTH]; CHUNK_WIDTH]),
            torch_positions: Vec::new(),
            redstone_positions: Vec::new(),
            furnace_positions: Vec::new(),
            hopper_positions: Vec::new(),
            random_tick_sections: Vec::new(),
            block_entities: std::collections::HashMap::new(),
        };

        // Populate various blocks across negative, zero, and positive sections
        chunk.set_block_local(2, -50, 3, BlockType::Torch);
        chunk.set_block_local(5, -45, 6, BlockType::RedstoneWire);
        chunk.set_block_local(1, -20, 1, BlockType::Furnace);
        chunk.set_block_local(4, -10, 4, BlockType::Hopper);
        chunk.set_block_local(7, -5, 7, BlockType::WheatCrop);

        chunk.set_block_local(0, 5, 0, BlockType::Stone);
        chunk.set_block_local(3, 12, 3, BlockType::Torch);
        chunk.set_block_local(8, 20, 8, BlockType::Repeater);
        chunk.set_block_local(9, 25, 9, BlockType::Furnace);
        chunk.set_block_local(10, 30, 10, BlockType::Hopper);
        chunk.set_block_local(11, 35, 11, BlockType::OakLeaves);

        chunk.set_block_local(14, 100, 14, BlockType::Comparator);
        chunk.set_block_local(15, 200, 15, BlockType::Fire);

        // Repeated mutations at same position to verify no duplicate positions
        chunk.set_block_local(8, 20, 8, BlockType::Repeater);
        chunk.set_block_local(8, 20, 8, BlockType::RedstoneTorch);
        chunk.set_block_local(3, 12, 3, BlockType::Torch);

        // Reference exhaustive scan
        let mut ref_torch = Vec::new();
        let mut ref_redstone = Vec::new();
        let mut ref_furnace = Vec::new();
        let mut ref_hopper = Vec::new();
        let mut ref_random_tick = Vec::new();

        for (sec_idx, sec_opt) in chunk.sections.iter().enumerate() {
            let Some(sec) = sec_opt else { continue; };
            let sec_y = chunk.min_section_y + sec_idx as i8;
            if sec.random_tick_count() > 0 {
                ref_random_tick.push(sec_y);
            }
            if sec.non_air_count() == 0 { continue; };
            for ly in 0..SECTION_SIZE {
                let wy = section_and_local_y_to_world_y(sec_y, ly as u8);
                for z in 0..CHUNK_DEPTH {
                    for x in 0..CHUNK_WIDTH {
                        let idx = (ly << 8) | (z << 4) | x;
                        let block = sec.get_block(idx);
                        let encoded = Chunk::encode_torch_position(x, wy, z);
                        if block == BlockType::Torch {
                            ref_torch.push(encoded);
                        }
                        if crate::redstone::is_component(block) {
                            ref_redstone.push(encoded);
                        }
                        if is_furnace_block(block) {
                            ref_furnace.push(encoded);
                        }
                        if is_hopper_block(block) {
                            ref_hopper.push(encoded);
                        }
                    }
                }
            }
        }

        // 1. Live chunk matches reference scan
        assert_eq!(chunk.torch_positions(), &ref_torch[..]);
        assert_eq!(chunk.redstone_positions(), &ref_redstone[..]);
        assert_eq!(chunk.furnace_positions(), &ref_furnace[..]);
        assert_eq!(chunk.hopper_positions(), &ref_hopper[..]);
        assert_eq!(chunk.random_tick_sections(), &ref_random_tick[..]);

        // 2. Rebuild matches reference scan
        chunk.rebuild_derived_indexes();
        assert_eq!(chunk.torch_positions(), &ref_torch[..]);
        assert_eq!(chunk.redstone_positions(), &ref_redstone[..]);
        assert_eq!(chunk.furnace_positions(), &ref_furnace[..]);
        assert_eq!(chunk.hopper_positions(), &ref_hopper[..]);
        assert_eq!(chunk.random_tick_sections(), &ref_random_tick[..]);

        // 3. Save and restore matches reference scan
        let save_data = crate::save::format::ChunkSaveData::from_chunk(&chunk).unwrap();
        let mut restored = Chunk {
            chunk_x: 0,
            chunk_z: 0,
            min_section_y: -4,
            sections: (0..24).map(|_| None).collect(),
            heightmap: Box::new([[NO_HEIGHT; CHUNK_DEPTH]; CHUNK_WIDTH]),
            torch_positions: Vec::new(),
            redstone_positions: Vec::new(),
            furnace_positions: Vec::new(),
            hopper_positions: Vec::new(),
            random_tick_sections: Vec::new(),
            block_entities: std::collections::HashMap::new(),
        };
        save_data.restore_to_chunk(&mut restored).unwrap();
        assert_eq!(restored.torch_positions(), &ref_torch[..]);
        assert_eq!(restored.redstone_positions(), &ref_redstone[..]);
        assert_eq!(restored.furnace_positions(), &ref_furnace[..]);
        assert_eq!(restored.hopper_positions(), &ref_hopper[..]);
        assert_eq!(restored.random_tick_sections(), &ref_random_tick[..]);
    }
}
