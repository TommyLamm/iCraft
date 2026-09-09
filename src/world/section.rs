use crate::world::block::{BlockType, RenderType, CHUNK_HEIGHT};
use std::mem::{size_of, size_of_val};

pub const SECTION_SIZE: usize = 16;
/// Number of 16-high sections in a legacy 256-tall dense column.
/// Live dimensions use `WorldHeight::section_count()`.
pub const SECTION_COUNT: usize = CHUNK_HEIGHT / SECTION_SIZE;
pub const SECTION_VOLUME: usize = SECTION_SIZE * SECTION_SIZE * SECTION_SIZE;

pub const fn world_y_to_section_y(y: i32) -> i8 {
    (y >> 4) as i8
}

pub const fn world_y_to_local_y(y: i32) -> u8 {
    (y.rem_euclid(16)) as u8
}

pub const fn section_and_local_y_to_world_y(section_y: i8, local_y: u8) -> i32 {
    (section_y as i32 * 16) + local_y as i32
}

pub const NO_HEIGHT: i16 = -9999;

/// Stable identity for one vertical 16^3 mesh section.
#[derive(
    Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct SectionKey {
    pub cx: i32,
    pub section_y: i8,
    pub cz: i32,
}

impl SectionKey {
    pub const fn new(cx: i32, section_y: i8, cz: i32) -> Self {
        Self { cx, section_y, cz }
    }
    pub const fn min_world_y(self) -> i32 {
        section_and_local_y_to_world_y(self.section_y, 0)
    }
    pub const fn max_world_y(self) -> i32 {
        self.min_world_y() + SECTION_SIZE as i32
    }
}

/// Revision/lifetime token carried by workers so stale meshes can be rejected.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct SectionIdentity {
    pub key: SectionKey,
    pub revision: u64,
    pub lifetime: u64,
}

impl SectionIdentity {
    pub const fn new(key: SectionKey, revision: u64, lifetime: u64) -> Self {
        Self {
            key,
            revision,
            lifetime,
        }
    }
    pub const fn accepts(self, candidate: Self) -> bool {
        self.key.cx == candidate.key.cx
            && self.key.cz == candidate.key.cz
            && self.key.section_y == candidate.key.section_y
            && self.lifetime == candidate.lifetime
            && candidate.revision == self.revision
    }
}

fn is_random_tick(block: BlockType) -> bool {
    matches!(
        block,
        BlockType::OakLeaves
            | BlockType::BirchLeaves
            | BlockType::SpruceLeaves
            | BlockType::Cactus
            | BlockType::SugarCane
            | BlockType::Grass
            | BlockType::Dirt
            | BlockType::Ice
            | BlockType::Snow
            | BlockType::SnowLayer
            | BlockType::Fire
            | BlockType::Farmland
            | BlockType::WheatCrop
            | BlockType::CarrotCrop
            | BlockType::PotatoCrop
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlockStorage {
    Empty,
    Uniform(BlockType),
    Paletted1 {
        palette: Vec<BlockType>,
        data: Box<[u64; 64]>,
    },
    Paletted2 {
        palette: Vec<BlockType>,
        data: Box<[u64; 128]>,
    },
    Paletted4 {
        palette: Vec<BlockType>,
        data: Box<[u64; 256]>,
    },
    Paletted8 {
        palette: Vec<BlockType>,
        data: Box<[u8; 4096]>,
    },
    Global(Box<[BlockType; 4096]>),
}

impl BlockStorage {
    /// Collapse an allocated representation whose values are all identical.
    ///
    /// This deliberately does not perform general palette rebuilding: it is
    /// the cheap-to-qualify demotion checked at every explicit safe point.
    fn compact_uniform(&mut self) -> bool {
        if matches!(self, Self::Empty | Self::Uniform(_)) {
            return false;
        }
        let first = self.get(0);
        if (1..4096).any(|idx| self.get(idx) != first) {
            return false;
        }
        *self = if first == BlockType::Air {
            Self::Empty
        } else {
            Self::Uniform(first)
        };
        true
    }

    /// Rebuild this storage at the smallest representation for its current values.
    /// This is an explicit safe-point operation; ordinary `set` calls remain incremental.
    pub fn compact(&mut self) {
        let before = self.memory_usage();
        let mut dense = [BlockType::Air; 4096];
        for (i, value) in dense.iter_mut().enumerate() {
            *value = self.get(i);
        }
        let compacted = Self::from_dense(&dense);
        if compacted.memory_usage() <= before {
            *self = compacted;
        }
    }

    /// Heap bytes owned by this storage (including palette/index allocations).
    pub fn memory_usage(&self) -> usize {
        size_of::<Self>()
            + match self {
                Self::Empty | Self::Uniform(_) => 0,
                Self::Paletted1 { palette, data } => {
                    palette.capacity() * size_of::<BlockType>() + size_of_val(data.as_ref())
                }
                Self::Paletted2 { palette, data } => {
                    palette.capacity() * size_of::<BlockType>() + size_of_val(data.as_ref())
                }
                Self::Paletted4 { palette, data } => {
                    palette.capacity() * size_of::<BlockType>() + size_of_val(data.as_ref())
                }
                Self::Paletted8 { palette, data } => {
                    palette.capacity() * size_of::<BlockType>() + size_of_val(data.as_ref())
                }
                Self::Global(data) => size_of_val(data.as_ref()),
            }
    }

    pub fn get(&self, idx: usize) -> BlockType {
        match self {
            BlockStorage::Empty => BlockType::Air,
            BlockStorage::Uniform(b) => *b,
            BlockStorage::Paletted1 { palette, data } => {
                let word = idx >> 6;
                let bit = idx & 63;
                let p_idx = ((data[word] >> bit) & 1) as usize;
                palette.get(p_idx).copied().unwrap_or(BlockType::Air)
            }
            BlockStorage::Paletted2 { palette, data } => {
                let bit_idx = idx << 1;
                let word = bit_idx >> 6;
                let bit = bit_idx & 63;
                let p_idx = ((data[word] >> bit) & 3) as usize;
                palette.get(p_idx).copied().unwrap_or(BlockType::Air)
            }
            BlockStorage::Paletted4 { palette, data } => {
                let bit_idx = idx << 2;
                let word = bit_idx >> 6;
                let bit = bit_idx & 63;
                let p_idx = ((data[word] >> bit) & 15) as usize;
                palette.get(p_idx).copied().unwrap_or(BlockType::Air)
            }
            BlockStorage::Paletted8 { palette, data } => {
                let p_idx = data[idx] as usize;
                palette.get(p_idx).copied().unwrap_or(BlockType::Air)
            }
            BlockStorage::Global(data) => data[idx],
        }
    }

    pub fn set(&mut self, idx: usize, block: BlockType) -> BlockType {
        let old_block = self.get(idx);
        if old_block == block {
            return old_block;
        }

        match self {
            BlockStorage::Empty => {
                let palette = vec![BlockType::Air, block];
                let mut data = Box::new([0u64; 64]);
                data[idx >> 6] |= 1u64 << (idx & 63);
                *self = BlockStorage::Paletted1 { palette, data };
            }
            BlockStorage::Uniform(old_b) => {
                let old_b = *old_b;
                let palette = vec![old_b, block];
                let mut data = Box::new([0u64; 64]);
                data[idx >> 6] |= 1u64 << (idx & 63);
                *self = BlockStorage::Paletted1 { palette, data };
            }
            BlockStorage::Paletted1 { palette, data } => {
                if let Some(pos) = palette.iter().position(|&b| b == block) {
                    let word = idx >> 6;
                    let bit = idx & 63;
                    data[word] = (data[word] & !(1u64 << bit)) | ((pos as u64 & 1) << bit);
                } else if palette.len() < 2 {
                    let pos = palette.len();
                    palette.push(block);
                    let word = idx >> 6;
                    let bit = idx & 63;
                    data[word] = (data[word] & !(1u64 << bit)) | ((pos as u64 & 1) << bit);
                } else {
                    let mut new_palette = palette.clone();
                    new_palette.push(block);
                    let mut new_data = Box::new([0u64; 128]);
                    for i in 0..4096 {
                        let w1 = i >> 6;
                        let b1 = i & 63;
                        let old_idx = (data[w1] >> b1) & 1;
                        let bit2 = (i << 1) & 63;
                        let word2 = (i << 1) >> 6;
                        new_data[word2] |= old_idx << bit2;
                    }
                    let bit_idx = idx << 1;
                    let word2 = bit_idx >> 6;
                    let bit2 = bit_idx & 63;
                    new_data[word2] = (new_data[word2] & !(3u64 << bit2)) | (2u64 << bit2);
                    *self = BlockStorage::Paletted2 {
                        palette: new_palette,
                        data: new_data,
                    };
                }
            }
            BlockStorage::Paletted2 { palette, data } => {
                if let Some(pos) = palette.iter().position(|&b| b == block) {
                    let bit_idx = idx << 1;
                    let word = bit_idx >> 6;
                    let bit = bit_idx & 63;
                    data[word] = (data[word] & !(3u64 << bit)) | ((pos as u64 & 3) << bit);
                } else if palette.len() < 4 {
                    let pos = palette.len();
                    palette.push(block);
                    let bit_idx = idx << 1;
                    let word = bit_idx >> 6;
                    let bit = bit_idx & 63;
                    data[word] = (data[word] & !(3u64 << bit)) | ((pos as u64 & 3) << bit);
                } else {
                    let mut new_palette = palette.clone();
                    new_palette.push(block);
                    let mut new_data = Box::new([0u64; 256]);
                    for i in 0..4096 {
                        let bit2 = (i << 1) & 63;
                        let word2 = (i << 1) >> 6;
                        let old_idx = (data[word2] >> bit2) & 3;
                        let bit4 = (i << 2) & 63;
                        let word4 = (i << 2) >> 6;
                        new_data[word4] |= old_idx << bit4;
                    }
                    let bit_idx = idx << 2;
                    let word4 = bit_idx >> 6;
                    let bit4 = bit_idx & 63;
                    new_data[word4] = (new_data[word4] & !(15u64 << bit4)) | (4u64 << bit4);
                    *self = BlockStorage::Paletted4 {
                        palette: new_palette,
                        data: new_data,
                    };
                }
            }
            BlockStorage::Paletted4 { palette, data } => {
                if let Some(pos) = palette.iter().position(|&b| b == block) {
                    let bit_idx = idx << 2;
                    let word = bit_idx >> 6;
                    let bit = bit_idx & 63;
                    data[word] = (data[word] & !(15u64 << bit)) | ((pos as u64 & 15) << bit);
                } else if palette.len() < 16 {
                    let pos = palette.len();
                    palette.push(block);
                    let bit_idx = idx << 2;
                    let word = bit_idx >> 6;
                    let bit = bit_idx & 63;
                    data[word] = (data[word] & !(15u64 << bit)) | ((pos as u64 & 15) << bit);
                } else {
                    let mut new_palette = palette.clone();
                    new_palette.push(block);
                    let mut new_data = Box::new([0u8; 4096]);
                    for i in 0..4096 {
                        let bit4 = (i << 2) & 63;
                        let word4 = (i << 2) >> 6;
                        let old_idx = (data[word4] >> bit4) & 15;
                        new_data[i] = old_idx as u8;
                    }
                    new_data[idx] = 16;
                    *self = BlockStorage::Paletted8 {
                        palette: new_palette,
                        data: new_data,
                    };
                }
            }
            BlockStorage::Paletted8 { palette, data } => {
                if let Some(pos) = palette.iter().position(|&b| b == block) {
                    data[idx] = pos as u8;
                } else if palette.len() < 256 {
                    let pos = palette.len();
                    palette.push(block);
                    data[idx] = pos as u8;
                } else {
                    let mut new_data = Box::new([BlockType::Air; 4096]);
                    for i in 0..4096 {
                        new_data[i] = palette[data[i] as usize];
                    }
                    new_data[idx] = block;
                    *self = BlockStorage::Global(new_data);
                }
            }
            BlockStorage::Global(data) => {
                data[idx] = block;
            }
        }
        old_block
    }

    pub fn from_dense(dense: &[BlockType; 4096]) -> Self {
        let first = dense[0];
        let mut all_same = true;
        let mut unique = Vec::new();

        for &b in dense.iter() {
            if b != first {
                all_same = false;
            }
            if !unique.contains(&b) {
                unique.push(b);
            }
        }

        if all_same {
            if first == BlockType::Air {
                return BlockStorage::Empty;
            } else {
                return BlockStorage::Uniform(first);
            }
        }

        if unique.len() <= 2 {
            unique.shrink_to_fit();
            let mut data = Box::new([0u64; 64]);
            for i in 0..4096 {
                let pos = unique.iter().position(|&b| b == dense[i]).unwrap();
                let word = i >> 6;
                let bit = i & 63;
                data[word] |= (pos as u64 & 1) << bit;
            }
            return BlockStorage::Paletted1 {
                palette: unique,
                data,
            };
        }

        if unique.len() <= 4 {
            unique.shrink_to_fit();
            let mut data = Box::new([0u64; 128]);
            for i in 0..4096 {
                let pos = unique.iter().position(|&b| b == dense[i]).unwrap();
                let bit_idx = i << 1;
                let word = bit_idx >> 6;
                let bit = bit_idx & 63;
                data[word] |= (pos as u64 & 3) << bit;
            }
            return BlockStorage::Paletted2 {
                palette: unique,
                data,
            };
        }

        if unique.len() <= 16 {
            unique.shrink_to_fit();
            let mut data = Box::new([0u64; 256]);
            for i in 0..4096 {
                let pos = unique.iter().position(|&b| b == dense[i]).unwrap();
                let bit_idx = i << 2;
                let word = bit_idx >> 6;
                let bit = bit_idx & 63;
                data[word] |= (pos as u64 & 15) << bit;
            }
            return BlockStorage::Paletted4 {
                palette: unique,
                data,
            };
        }

        if unique.len() <= 256 {
            unique.shrink_to_fit();
            let mut data = Box::new([0u8; 4096]);
            for i in 0..4096 {
                let pos = unique.iter().position(|&b| b == dense[i]).unwrap();
                data[i] = pos as u8;
            }
            return BlockStorage::Paletted8 {
                palette: unique,
                data,
            };
        }

        BlockStorage::Global(Box::new(*dense))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LightStorage {
    Uniform { sky: u8, block: u8 },
    Packed(Box<[u8; 4096]>),
}

impl LightStorage {
    fn compact_uniform(&mut self) -> bool {
        if matches!(self, Self::Uniform { .. }) {
            return false;
        }
        let first_sky = self.get_sky(0);
        let first_block = self.get_block(0);
        if (1..4096).any(|idx| self.get_sky(idx) != first_sky || self.get_block(idx) != first_block)
        {
            return false;
        }
        *self = Self::Uniform {
            sky: first_sky,
            block: first_block,
        };
        true
    }

    pub fn compact(&mut self) {
        let mut sky = [0u8; 4096];
        let mut block = [0u8; 4096];
        for i in 0..4096 {
            sky[i] = self.get_sky(i);
            block[i] = self.get_block(i);
        }
        *self = Self::from_dense(&sky, &block);
    }

    pub fn memory_usage(&self) -> usize {
        size_of::<Self>()
            + match self {
                Self::Uniform { .. } => 0,
                Self::Packed(data) => size_of_val(data.as_ref()),
            }
    }

    pub fn get_sky(&self, idx: usize) -> u8 {
        match self {
            LightStorage::Uniform { sky, .. } => *sky,
            LightStorage::Packed(data) => data[idx] >> 4,
        }
    }

    pub fn get_block(&self, idx: usize) -> u8 {
        match self {
            LightStorage::Uniform { block, .. } => *block,
            LightStorage::Packed(data) => data[idx] & 0x0F,
        }
    }

    pub fn set_sky(&mut self, idx: usize, val: u8) {
        let val = val & 0x0F;
        match self {
            LightStorage::Uniform { sky, block } => {
                if *sky == val {
                    return;
                }
                let mut data = Box::new([(*sky << 4) | (*block & 0x0F); 4096]);
                data[idx] = (val << 4) | (*block & 0x0F);
                *self = LightStorage::Packed(data);
            }
            LightStorage::Packed(data) => {
                data[idx] = (val << 4) | (data[idx] & 0x0F);
            }
        }
    }

    pub fn set_block(&mut self, idx: usize, val: u8) {
        let val = val & 0x0F;
        match self {
            LightStorage::Uniform { sky, block } => {
                if *block == val {
                    return;
                }
                let mut data = Box::new([(*sky << 4) | (*block & 0x0F); 4096]);
                data[idx] = (data[idx] & 0xF0) | val;
                *self = LightStorage::Packed(data);
            }
            LightStorage::Packed(data) => {
                data[idx] = (data[idx] & 0xF0) | val;
            }
        }
    }

    pub fn from_dense(sky_dense: &[u8; 4096], block_dense: &[u8; 4096]) -> Self {
        let first_sky = sky_dense[0] & 0x0F;
        let first_block = block_dense[0] & 0x0F;
        let mut uniform = true;

        for i in 0..4096 {
            if (sky_dense[i] & 0x0F) != first_sky || (block_dense[i] & 0x0F) != first_block {
                uniform = false;
                break;
            }
        }

        if uniform {
            LightStorage::Uniform {
                sky: first_sky,
                block: first_block,
            }
        } else {
            let mut data = Box::new([0u8; 4096]);
            for i in 0..4096 {
                data[i] = ((sky_dense[i] & 0x0F) << 4) | (block_dense[i] & 0x0F);
            }
            LightStorage::Packed(data)
        }
    }
}

#[derive(Clone, Debug)]
pub struct ChunkSection {
    pub(crate) blocks: BlockStorage,
    pub(crate) light: LightStorage,
    pub(crate) block_states: Option<Box<[u8; 4096]>>,
    pub(crate) fluid_levels: Option<Box<[u8; 4096]>>,
    pub(crate) non_air_count: u16,
    pub(crate) opaque_count: u16,
    pub(crate) random_tick_count: u16,
    pub(crate) fluid_count: u16,
    pub(crate) emitter_count: u16,
    pub(crate) redstone_count: u16,
    pub(crate) block_state_nonzero_count: u16,
    pub(crate) fluid_level_nonzero_count: u16,
    pub(crate) storage_changes: u16,
}

impl ChunkSection {
    pub fn new(section_y: i8) -> Self {
        if section_y >= 0 {
            Self::empty_sky()
        } else {
            Self::empty_dark()
        }
    }

    pub fn empty_sky() -> Self {
        Self {
            blocks: BlockStorage::Empty,
            light: LightStorage::Uniform { sky: 15, block: 0 },
            block_states: None,
            fluid_levels: None,
            non_air_count: 0,
            opaque_count: 0,
            random_tick_count: 0,
            fluid_count: 0,
            emitter_count: 0,
            redstone_count: 0,
            block_state_nonzero_count: 0,
            fluid_level_nonzero_count: 0,
            storage_changes: 0,
        }
    }

    pub fn non_air_count(&self) -> u16 {
        self.non_air_count
    }

    pub fn is_empty(&self) -> bool {
        self.non_air_count == 0
    }

    pub fn empty_dark() -> Self {
        Self {
            blocks: BlockStorage::Empty,
            light: LightStorage::Uniform { sky: 0, block: 0 },
            block_states: None,
            fluid_levels: None,
            non_air_count: 0,
            opaque_count: 0,
            random_tick_count: 0,
            fluid_count: 0,
            emitter_count: 0,
            redstone_count: 0,
            block_state_nonzero_count: 0,
            fluid_level_nonzero_count: 0,
            storage_changes: 0,
        }
    }

    pub fn from_dense(
        blocks: &[BlockType; 4096],
        sky_light: &[u8; 4096],
        block_light: &[u8; 4096],
        states: Option<&[u8; 4096]>,
        fluids: Option<&[u8; 4096]>,
    ) -> Self {
        let mut non_air_count = 0u16;
        let mut opaque_count = 0u16;
        let mut random_tick_count = 0u16;
        let mut fluid_count = 0u16;
        let mut emitter_count = 0u16;
        let mut redstone_count = 0u16;
        let block_state_nonzero_count =
            states.map_or(0, |st| st.iter().filter(|&&v| v != 0).count() as u16);
        let fluid_level_nonzero_count =
            fluids.map_or(0, |fl| fl.iter().filter(|&&v| v != 0).count() as u16);

        for &b in blocks.iter() {
            if b != BlockType::Air {
                non_air_count += 1;
            }
            let props = b.properties();
            if props.render_type == RenderType::Opaque {
                opaque_count += 1;
            }
            if is_random_tick(b) {
                random_tick_count += 1;
            }
            if b == BlockType::Water || b == BlockType::Lava {
                fluid_count += 1;
            }
            if props.light_emission > 0 {
                emitter_count += 1;
            }
            if crate::redstone::is_component(b) {
                redstone_count += 1;
            }
        }

        let block_storage = BlockStorage::from_dense(blocks);
        let light_storage = LightStorage::from_dense(sky_light, block_light);

        let block_states = states.and_then(|st| {
            if st.iter().all(|&s| s == 0) {
                None
            } else {
                Some(Box::new(*st))
            }
        });

        let fluid_levels = fluids.and_then(|fl| {
            if fl.iter().all(|&f| f == 0) {
                None
            } else {
                Some(Box::new(*fl))
            }
        });

        ChunkSection {
            blocks: block_storage,
            light: light_storage,
            block_states,
            fluid_levels,
            non_air_count,
            opaque_count,
            random_tick_count,
            fluid_count,
            emitter_count,
            redstone_count,
            block_state_nonzero_count,
            fluid_level_nonzero_count,
            storage_changes: 0,
        }
    }

    pub fn random_tick_count(&self) -> u16 {
        self.random_tick_count
    }

    /// Read a block through the section API without exposing its representation.
    pub fn get_block(&self, idx: usize) -> BlockType {
        self.blocks.get(idx)
    }

    /// Returns whether this section contains at least one instance of `block`.
    pub fn contains_block(&self, block: BlockType) -> bool {
        match &self.blocks {
            BlockStorage::Empty => block == BlockType::Air,
            BlockStorage::Uniform(value) => *value == block,
            BlockStorage::Paletted1 { palette, data } => palette
                .iter()
                .position(|value| *value == block)
                .is_some_and(|palette_index| {
                    data.iter().any(|word| {
                        if palette_index == 0 {
                            *word != u64::MAX
                        } else {
                            *word != 0
                        }
                    })
                }),
            BlockStorage::Paletted2 { palette, data } => palette
                .iter()
                .position(|value| *value == block)
                .is_some_and(|palette_index| {
                    (0..4096).any(|index| {
                        let bit_index = index << 1;
                        ((data[bit_index >> 6] >> (bit_index & 63)) & 3) as usize == palette_index
                    })
                }),
            BlockStorage::Paletted4 { palette, data } => palette
                .iter()
                .position(|value| *value == block)
                .is_some_and(|palette_index| {
                    (0..4096).any(|index| {
                        let bit_index = index << 2;
                        ((data[bit_index >> 6] >> (bit_index & 63)) & 15) as usize == palette_index
                    })
                }),
            BlockStorage::Paletted8 { palette, data } => palette
                .iter()
                .position(|value| *value == block)
                .is_some_and(|palette_index| data.contains(&(palette_index as u8))),
            BlockStorage::Global(data) => data.contains(&block),
        }
    }

    pub fn set_block(&mut self, idx: usize, block: BlockType) -> BlockType {
        let old_block = self.blocks.set(idx, block);
        if old_block != block {
            self.storage_changes = self.storage_changes.saturating_add(1);
            if old_block != BlockType::Air {
                self.non_air_count = self.non_air_count.saturating_sub(1);
            }
            if block != BlockType::Air {
                self.non_air_count += 1;
            }

            let old_props = old_block.properties();
            let new_props = block.properties();

            if old_props.render_type == RenderType::Opaque {
                self.opaque_count = self.opaque_count.saturating_sub(1);
            }
            if new_props.render_type == RenderType::Opaque {
                self.opaque_count += 1;
            }

            if is_random_tick(old_block) {
                self.random_tick_count = self.random_tick_count.saturating_sub(1);
            }
            if is_random_tick(block) {
                self.random_tick_count += 1;
            }

            if old_block == BlockType::Water || old_block == BlockType::Lava {
                self.fluid_count = self.fluid_count.saturating_sub(1);
            }
            if block == BlockType::Water || block == BlockType::Lava {
                self.fluid_count += 1;
            }

            if old_props.light_emission > 0 {
                self.emitter_count = self.emitter_count.saturating_sub(1);
            }
            if new_props.light_emission > 0 {
                self.emitter_count += 1;
            }

            if crate::redstone::is_component(old_block) {
                self.redstone_count = self.redstone_count.saturating_sub(1);
            }
            if crate::redstone::is_component(block) {
                self.redstone_count += 1;
            }
        }
        old_block
    }

    pub fn get_block_state(&self, idx: usize) -> u8 {
        self.block_states.as_ref().map_or(0, |st| st[idx])
    }

    pub fn set_block_state(&mut self, idx: usize, state: u8) {
        let old = self.block_states.as_ref().map_or(0, |st| st[idx]);
        if old == state {
            return;
        }
        if state == 0 {
            if let Some(ref mut st) = self.block_states {
                st[idx] = 0;
            }
            self.block_state_nonzero_count = self.block_state_nonzero_count.saturating_sub(1);
            if self.block_state_nonzero_count == 0 {
                self.block_states = None;
            }
        } else {
            let st = self
                .block_states
                .get_or_insert_with(|| Box::new([0u8; 4096]));
            st[idx] = state;
            if old == 0 {
                self.block_state_nonzero_count = self.block_state_nonzero_count.saturating_add(1);
            }
        }
    }

    pub fn get_fluid_level(&self, idx: usize) -> u8 {
        self.fluid_levels.as_ref().map_or(0, |fl| fl[idx])
    }

    pub fn set_fluid_level(&mut self, idx: usize, level: u8) {
        let old = self.fluid_levels.as_ref().map_or(0, |fl| fl[idx]);
        if old == level {
            return;
        }
        if level == 0 {
            if let Some(ref mut fl) = self.fluid_levels {
                fl[idx] = 0;
            }
            self.fluid_level_nonzero_count = self.fluid_level_nonzero_count.saturating_sub(1);
            if self.fluid_level_nonzero_count == 0 {
                self.fluid_levels = None;
            }
        } else {
            let fl = self
                .fluid_levels
                .get_or_insert_with(|| Box::new([0u8; 4096]));
            fl[idx] = level;
            if old == 0 {
                self.fluid_level_nonzero_count = self.fluid_level_nonzero_count.saturating_add(1);
            }
        }
    }

    pub fn compact_storage(&mut self) {
        self.blocks.compact();
        self.light.compact();
        self.storage_changes = 0;
    }

    /// Compact at an explicit runtime safe point.
    ///
    /// Empty/uniform demotion is attempted immediately so short-lived edits
    /// can release their allocation without waiting for the churn interval.
    /// General palette rebuilding remains amortized behind the interval. The
    /// hot `set_*` paths only update values and counters.
    pub fn compact_if_worthwhile(&mut self) -> bool {
        const COMPACTION_INTERVAL: u16 = 256;
        let demoted = self.blocks.compact_uniform() | self.light.compact_uniform();
        if demoted {
            self.storage_changes = 0;
            return true;
        }
        if self.storage_changes < COMPACTION_INTERVAL {
            return false;
        }
        self.compact_storage();
        true
    }

    pub fn memory_usage(&self) -> usize {
        size_of::<Self>()
            + self
                .blocks
                .memory_usage()
                .saturating_sub(size_of::<BlockStorage>())
            + self
                .light
                .memory_usage()
                .saturating_sub(size_of::<LightStorage>())
            + self
                .block_states
                .as_ref()
                .map_or(0, |v| size_of_val(v.as_ref()))
            + self
                .fluid_levels
                .as_ref()
                .map_or(0, |v| size_of_val(v.as_ref()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paletted_block_storage_transitions() {
        let mut dense = [BlockType::Air; 4096];
        let storage = BlockStorage::from_dense(&dense);
        assert!(matches!(storage, BlockStorage::Empty));

        dense.fill(BlockType::Stone);
        let storage = BlockStorage::from_dense(&dense);
        assert!(matches!(storage, BlockStorage::Uniform(BlockType::Stone)));

        // 2 types -> Paletted1
        dense[0] = BlockType::Dirt;
        let storage = BlockStorage::from_dense(&dense);
        assert!(matches!(storage, BlockStorage::Paletted1 { .. }));

        // 4 types -> Paletted2
        dense[1] = BlockType::Grass;
        dense[2] = BlockType::Sand;
        let storage = BlockStorage::from_dense(&dense);
        assert!(matches!(storage, BlockStorage::Paletted2 { .. }));

        // 16 types -> Paletted4
        let types = [
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
        ];
        for (i, &t) in types.iter().enumerate() {
            dense[i] = t;
        }
        let storage = BlockStorage::from_dense(&dense);
        assert!(matches!(storage, BlockStorage::Paletted4 { .. }));
        for (i, &t) in types.iter().enumerate() {
            assert_eq!(storage.get(i), t);
        }
    }

    #[test]
    fn light_storage_packing_and_nibbles() {
        let sky = [15u8; 4096];
        let block = [0u8; 4096];
        let storage = LightStorage::from_dense(&sky, &block);
        assert!(matches!(
            storage,
            LightStorage::Uniform { sky: 15, block: 0 }
        ));
        assert_eq!(storage.get_sky(100), 15);
        assert_eq!(storage.get_block(100), 0);

        let mut sky2 = [15u8; 4096];
        sky2[50] = 7;
        let storage2 = LightStorage::from_dense(&sky2, &block);
        assert!(matches!(storage2, LightStorage::Packed(_)));
        assert_eq!(storage2.get_sky(50), 7);
        assert_eq!(storage2.get_sky(51), 15);
    }

    #[test]
    fn chunk_section_metadata_counts() {
        let mut section = ChunkSection::empty_sky();
        assert_eq!(section.non_air_count, 0);
        assert_eq!(section.opaque_count, 0);
        assert_eq!(section.random_tick_count, 0);

        section.set_block(0, BlockType::Stone);
        assert_eq!(section.non_air_count, 1);
        assert_eq!(section.opaque_count, 1);

        section.set_block(1, BlockType::OakLeaves);
        assert_eq!(section.non_air_count, 2);
        assert_eq!(section.opaque_count, 1); // leaves non-opaque
        assert_eq!(section.random_tick_count, 1);

        section.set_block(0, BlockType::Air);
        assert_eq!(section.non_air_count, 1);
        assert_eq!(section.opaque_count, 0);
    }

    #[test]
    fn storage_compact_demotes_and_preserves_values() {
        let mut storage = BlockStorage::Empty;
        for i in 0..300 {
            storage.set(
                i,
                if i == 0 {
                    BlockType::Stone
                } else {
                    BlockType::Air
                },
            );
        }
        assert!(matches!(storage, BlockStorage::Paletted1 { .. }));
        let before = storage.memory_usage();
        storage.compact();
        assert_eq!(storage.get(0), BlockType::Stone);
        assert!(storage.memory_usage() <= before);
        storage.set(0, BlockType::Air);
        let allocated = storage.memory_usage();
        storage.compact();
        assert!(matches!(storage, BlockStorage::Empty));
        assert!(
            storage.memory_usage() < allocated,
            "empty demotion must release the palette and packed indices"
        );
    }

    #[test]
    fn optional_arrays_release_when_zero() {
        let mut section = ChunkSection::empty_sky();
        section.set_block_state(7, 3);
        section.set_fluid_level(9, 4);
        assert!(section.block_states.is_some() && section.fluid_levels.is_some());
        section.set_block_state(7, 0);
        section.set_fluid_level(9, 0);
        assert!(section.block_states.is_none() && section.fluid_levels.is_none());
    }

    #[test]
    fn randomized_storage_matches_flat_oracle_after_compact() {
        let mut storage = BlockStorage::Empty;
        let mut oracle = [BlockType::Air; 4096];
        let mut seed = 0x1234_5678u32;
        for _ in 0..2000 {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let idx = (seed as usize) & 4095;
            let block = match (seed >> 16) % 5 {
                0 => BlockType::Air,
                1 => BlockType::Stone,
                2 => BlockType::Dirt,
                3 => BlockType::Water,
                _ => BlockType::Glass,
            };
            storage.set(idx, block);
            oracle[idx] = block;
        }
        storage.compact();
        for i in 0..4096 {
            assert_eq!(storage.get(i), oracle[i]);
        }
    }

    #[test]
    fn section_compaction_is_deferred_to_safe_point() {
        let mut section = ChunkSection::empty_sky();
        for i in 0..255 {
            section.set_block(
                0,
                if i & 1 == 0 {
                    BlockType::Stone
                } else {
                    BlockType::Dirt
                },
            );
        }
        assert!(!section.compact_if_worthwhile());
        section.set_block(1, BlockType::Stone);
        assert!(section.compact_if_worthwhile());
        assert_eq!(section.get_block(0), BlockType::Stone);
        assert_eq!(section.get_block(1), BlockType::Stone);
    }

    #[test]
    fn section_safe_point_immediately_demotes_empty_and_uniform_storage() {
        let mut empty = ChunkSection::empty_sky();
        empty.set_block(0, BlockType::Stone);
        empty.set_block(0, BlockType::Air);
        let empty_allocated = empty.memory_usage();
        assert!(empty.compact_if_worthwhile());
        assert_eq!(empty.get_block(0), BlockType::Air);
        assert!(empty.memory_usage() < empty_allocated);

        let blocks = [BlockType::Stone; 4096];
        let light = [0u8; 4096];
        let mut uniform = ChunkSection::from_dense(&blocks, &light, &light, None, None);
        uniform.set_block(7, BlockType::Dirt);
        uniform.set_block(7, BlockType::Stone);
        let uniform_allocated = uniform.memory_usage();
        assert!(uniform.compact_if_worthwhile());
        assert_eq!(uniform.get_block(7), BlockType::Stone);
        assert!(uniform.memory_usage() < uniform_allocated);
    }

    #[test]
    fn section_contains_block_ignores_unreferenced_palette_entries() {
        let mut section = ChunkSection::empty_dark();
        section.set_block(0, BlockType::Purpur);
        assert!(section.contains_block(BlockType::Purpur));

        section.set_block(0, BlockType::Air);
        assert!(!section.contains_block(BlockType::Purpur));
        assert!(section.contains_block(BlockType::Air));
    }

    #[test]
    fn section_safe_point_immediately_demotes_uniform_light_storage() {
        let mut section = ChunkSection::empty_dark();
        section.light.set_sky(11, 9);
        section.light.set_sky(11, 0);
        let allocated = section.memory_usage();
        assert!(section.compact_if_worthwhile());
        assert_eq!(section.light.get_sky(11), 0);
        assert!(section.memory_usage() < allocated);
    }
}
