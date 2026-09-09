use crate::world::block::{BlockType, CHUNK_DEPTH, CHUNK_WIDTH};
use crate::world::section::{
    section_and_local_y_to_world_y, world_y_to_local_y, world_y_to_section_y, ChunkSection,
    NO_HEIGHT, SECTION_SIZE,
};
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
    matches!(block, BlockType::Furnace | BlockType::FurnaceLit)
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
            block_entities: std::collections::HashMap::new(),
        }
    }

    pub fn new(chunk_x: i32, chunk_z: i32) -> Self {
        Self::new_with_seed(chunk_x, chunk_z, 12345)
    }

    pub fn new_with_seed(chunk_x: i32, chunk_z: i32, world_seed: u32) -> Self {
        // Dense full-height block array for the signed overworld range.
        // Indexed [x][local_y][z] where local_y = world_y - min_y.
        let height = crate::dimension::WorldHeight::OVERWORLD;
        let min_y = height.min_y();
        let total_height = height.height() as usize;
        let mut blocks: Vec<Vec<[BlockType; CHUNK_DEPTH]>> =
            vec![vec![[BlockType::Air; CHUNK_DEPTH]; total_height]; CHUNK_WIDTH];

        let ctx = crate::worldgen::WorldGenContext::new(world_seed);

        // Fill terrain density.
        for x in 0..CHUNK_WIDTH {
            for z in 0..CHUNK_DEPTH {
                let wx = chunk_x * CHUNK_WIDTH as i32 + x as i32;
                let wz = chunk_z * CHUNK_DEPTH as i32 + z as i32;
                let surface_y = ctx.surface_height_at(wx, wz);
                let biome = ctx.biome_at(wx, wz);

                for wy in min_y..height.max_y_exclusive() {
                    let ly = (wy - min_y) as usize;
                    let block = ctx
                        .block_at_sampled(wx, wy, wz, surface_y, biome)
                        .unwrap_or(BlockType::Air);
                    blocks[x][ly][z] = block;
                }

                // Carve caves after surface generation. Density fill only places
                // solids at or below the surface, so the carver has nothing to do
                // above `surface_y`.
                let carve_top = surface_y.min(height.max_y_exclusive() - 1);
                for wy in min_y..=carve_top {
                    let ly = (wy - min_y) as usize;
                    let current = blocks[x][ly][z];
                    if current == BlockType::Air || current == BlockType::Water {
                        continue;
                    }
                    if ctx.carver.is_carved(wx, wy, wz, surface_y) {
                        if ctx.carver.is_lava_lake(wx, wy, wz) {
                            blocks[x][ly][z] = BlockType::Lava;
                        } else {
                            blocks[x][ly][z] = BlockType::Air;
                        }
                    }
                }
            }
        }

        // Place ore veins.
        ctx.ore.place_ores(
            &mut blocks,
            chunk_x,
            chunk_z,
            (min_y as i32).unsigned_abs() as usize,
        );

        // Place trees and plants.
        crate::worldgen::feature::FeaturePlacer::new(world_seed).place_features(
            &ctx,
            &mut blocks,
            chunk_x,
            chunk_z,
            (min_y as i32).unsigned_abs() as usize,
        );

        // Compute sky/block light and heightmap.
        let mut sky_light: Vec<Vec<[u8; CHUNK_DEPTH]>> =
            vec![vec![[0u8; CHUNK_DEPTH]; total_height]; CHUNK_WIDTH];
        let mut block_light: Vec<Vec<[u8; CHUNK_DEPTH]>> =
            vec![vec![[0u8; CHUNK_DEPTH]; total_height]; CHUNK_WIDTH];
        let mut heightmap: Box<[[i16; CHUNK_DEPTH]; CHUNK_WIDTH]> =
            vec![[NO_HEIGHT; CHUNK_DEPTH]; CHUNK_WIDTH]
                .try_into()
                .unwrap();

        for x in 0..CHUNK_WIDTH {
            for z in 0..CHUNK_DEPTH {
                let mut direct_sky = 15u8;
                let mut found_h = false;
                for wy in (min_y..height.max_y_exclusive()).rev() {
                    let ly = (wy - min_y) as usize;
                    let block = blocks[x][ly][z];
                    if !found_h && block != BlockType::Air {
                        heightmap[x][z] = wy.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
                        found_h = true;
                    }
                    if block.properties().render_type == crate::world::RenderType::Opaque {
                        direct_sky = 0;
                    }
                    sky_light[x][ly][z] = direct_sky;
                    block_light[x][ly][z] = block.properties().light_emission;
                }
            }
        }

        // Convert dense array to signed sections.
        let mut sections = Vec::with_capacity(height.section_count());
        for sec_idx in 0..height.section_count() {
            let sec_y = height.section_y_at_index(sec_idx);
            let mut sec_b = [BlockType::Air; 4096];
            let mut sec_sk = [0u8; 4096];
            let mut sec_bl = [0u8; 4096];
            for ly in 0..SECTION_SIZE {
                let wy = section_and_local_y_to_world_y(sec_y, ly as u8);
                if height.contains_y(wy) {
                    let arr_ly = (wy - min_y) as usize;
                    for z in 0..CHUNK_DEPTH {
                        for x in 0..CHUNK_WIDTH {
                            let idx = (ly << 8) | (z << 4) | x;
                            sec_b[idx] = blocks[x][arr_ly][z];
                            sec_sk[idx] = sky_light[x][arr_ly][z];
                            sec_bl[idx] = block_light[x][arr_ly][z];
                        }
                    }
                }
            }
            let sec = ChunkSection::from_dense(&sec_b, &sec_sk, &sec_bl, None, None);
            if sec.non_air_count() == 0
                && sec_sk.iter().all(|&l| l == 0)
                && sec_bl.iter().all(|&l| l == 0)
            {
                sections.push(None);
            } else {
                sections.push(Some(sec));
            }
        }

        let torch_positions =
            Self::build_torch_index_from_sections(height.min_section_y(), &sections);
        let redstone_positions =
            Self::build_redstone_index_from_sections(height.min_section_y(), &sections);
        let furnace_positions =
            Self::build_furnace_index_from_sections(height.min_section_y(), &sections);

        Self {
            chunk_x,
            chunk_z,
            min_section_y: height.min_section_y(),
            sections,
            heightmap,
            torch_positions,
            redstone_positions,
            furnace_positions,
            block_entities: std::collections::HashMap::new(),
        }
    }

    fn encode_torch_position(x: usize, y: i32, z: usize) -> u32 {
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

    fn build_torch_index_from_sections(
        min_sec_y: i8,
        sections: &[Option<ChunkSection>],
    ) -> Vec<u32> {
        let mut positions = Vec::new();
        for (sec_idx, sec_opt) in sections.iter().enumerate() {
            let Some(sec) = sec_opt else {
                continue;
            };
            if sec.non_air_count() == 0 {
                continue;
            }
            let sec_y = min_sec_y + sec_idx as i8;
            for ly in 0..SECTION_SIZE {
                let wy = section_and_local_y_to_world_y(sec_y, ly as u8);
                for z in 0..CHUNK_DEPTH {
                    for x in 0..CHUNK_WIDTH {
                        let idx = (ly << 8) | (z << 4) | x;
                        if sec.get_block(idx) == BlockType::Torch {
                            positions.push(Self::encode_torch_position(x, wy, z));
                        }
                    }
                }
            }
        }
        positions
    }

    fn build_redstone_index_from_sections(
        min_sec_y: i8,
        sections: &[Option<ChunkSection>],
    ) -> Vec<u32> {
        let mut positions = Vec::new();
        for (sec_idx, sec_opt) in sections.iter().enumerate() {
            let Some(sec) = sec_opt else {
                continue;
            };
            if sec.redstone_count == 0 {
                continue;
            }
            let sec_y = min_sec_y + sec_idx as i8;
            for ly in 0..SECTION_SIZE {
                let wy = section_and_local_y_to_world_y(sec_y, ly as u8);
                for z in 0..CHUNK_DEPTH {
                    for x in 0..CHUNK_WIDTH {
                        let idx = (ly << 8) | (z << 4) | x;
                        if crate::redstone::is_component(sec.get_block(idx)) {
                            positions.push(Self::encode_torch_position(x, wy, z));
                        }
                    }
                }
            }
        }
        positions
    }

    fn build_furnace_index_from_sections(
        min_sec_y: i8,
        sections: &[Option<ChunkSection>],
    ) -> Vec<u32> {
        let mut positions = Vec::new();
        for (sec_idx, sec_opt) in sections.iter().enumerate() {
            let Some(sec) = sec_opt else {
                continue;
            };
            if sec.non_air_count() == 0 {
                continue;
            }
            let sec_y = min_sec_y + sec_idx as i8;
            for ly in 0..SECTION_SIZE {
                let wy = section_and_local_y_to_world_y(sec_y, ly as u8);
                for z in 0..CHUNK_DEPTH {
                    for x in 0..CHUNK_WIDTH {
                        let idx = (ly << 8) | (z << 4) | x;
                        if is_furnace_block(sec.get_block(idx)) {
                            positions.push(Self::encode_torch_position(x, wy, z));
                        }
                    }
                }
            }
        }
        positions
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

    /// Rebuilds the torch index after bulk block mutations (generation/load).
    pub fn rebuild_torch_index(&mut self) {
        self.torch_positions =
            Self::build_torch_index_from_sections(self.min_section_y, &self.sections);
    }

    /// Rebuilds the redstone index after bulk block mutations (generation/load).
    pub fn rebuild_redstone_index(&mut self) {
        self.redstone_positions =
            Self::build_redstone_index_from_sections(self.min_section_y, &self.sections);
    }

    /// Rebuilds the furnace index after bulk block mutations (generation/load).
    pub fn rebuild_furnace_index(&mut self) {
        self.furnace_positions =
            Self::build_furnace_index_from_sections(self.min_section_y, &self.sections);
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

        if let Some(entity) = self.block_entities.get(&(x as u8, wy as i16, z as u8)) {
            if !entity.matches_block_type(block) {
                self.block_entities.remove(&(x as u8, wy as i16, z as u8));
            }
        }

        let encoded = Self::encode_torch_position(x, wy, z);
        if old == BlockType::Torch {
            if let Some(index) = self.torch_positions.iter().position(|&p| p == encoded) {
                self.torch_positions.swap_remove(index);
            }
        }
        if block == BlockType::Torch && old != BlockType::Torch {
            self.torch_positions.push(encoded);
        }

        let old_is_redstone = crate::redstone::is_component(old);
        let new_is_redstone = crate::redstone::is_component(block);
        if old_is_redstone {
            if let Some(index) = self.redstone_positions.iter().position(|&p| p == encoded) {
                self.redstone_positions.swap_remove(index);
            }
        }
        if new_is_redstone && !old_is_redstone {
            self.redstone_positions.push(encoded);
        }

        let old_is_furnace = is_furnace_block(old);
        let new_is_furnace = is_furnace_block(block);
        if old_is_furnace && !new_is_furnace {
            if let Some(index) = self.furnace_positions.iter().position(|&p| p == encoded) {
                self.furnace_positions.swap_remove(index);
            }
        }
        if new_is_furnace && !old_is_furnace {
            self.furnace_positions.push(encoded);
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
                    if enable_sky && block.properties().is_opaque() {
                        direct_sky = 0;
                    }
                    self.set_sky_light(x, wy, z, direct_sky);
                    self.set_block_light(x, wy, z, block.properties().light_emission);
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
        chunk.set_block_local(3, 40, 5, BlockType::FurnaceLit);
        assert_eq!(chunk.furnace_positions().len(), 1);
        chunk.set_block_local(3, 40, 5, BlockType::Stone);
        assert!(chunk.furnace_positions().is_empty());
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
}
