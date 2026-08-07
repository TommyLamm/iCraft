use crate::world::{BlockType, Chunk, RenderType, CHUNK_DEPTH, CHUNK_HEIGHT, CHUNK_WIDTH};
use glam::Vec3;
use noise::{NoiseFn, Perlin};
use std::collections::VecDeque;

pub type BlockPos = (i32, i32, i32);

const NETHER_HEIGHT: usize = 128;
const LAVA_LEVEL: usize = 31;
const END_CITY_X: i32 = 1_032;
const END_CITY_Z: i32 = 8;
const END_CITY_BASE_Y: i32 = 71;

/// Block-center X/Z and entity Y for the eight main-island healing crystals.
pub const END_CRYSTAL_TOWERS: [(i32, i32, i32); 8] = [
    (42, 78, 0),
    (30, 84, 30),
    (0, 88, 42),
    (-30, 82, 30),
    (-42, 80, 0),
    (-30, 86, -30),
    (0, 90, -42),
    (30, 82, -30),
];

#[derive(
    Copy, Clone, Debug, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
#[repr(u8)]
pub enum Dimension {
    #[default]
    Overworld,
    Nether,
    End,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WorldHeight {
    pub min_y: i32,
    pub height: u32,
}

impl WorldHeight {
    pub const OVERWORLD: Self = Self {
        min_y: -64,
        height: 384,
    };
    pub const NETHER: Self = Self {
        min_y: 0,
        height: 128,
    };
    pub const END: Self = Self {
        min_y: 0,
        height: 256,
    };

    pub const fn new(min_y: i32, height: u32) -> Self {
        Self { min_y, height }
    }

    pub const fn min_y(&self) -> i32 {
        self.min_y
    }

    pub const fn height(&self) -> u32 {
        self.height
    }

    pub const fn max_y_exclusive(&self) -> i32 {
        self.min_y + self.height as i32
    }

    pub const fn min_section_y(&self) -> i8 {
        (self.min_y >> 4) as i8
    }

    pub const fn max_section_y_exclusive(&self) -> i8 {
        (self.max_y_exclusive() >> 4) as i8
    }

    pub const fn section_count(&self) -> usize {
        (self.height / 16) as usize
    }

    pub const fn contains_y(&self, y: i32) -> bool {
        y >= self.min_y && y < self.max_y_exclusive()
    }

    pub fn section_index(&self, section_y: i8) -> Option<usize> {
        let min_sec = self.min_section_y();
        let max_sec = self.max_section_y_exclusive();
        if section_y >= min_sec && section_y < max_sec {
            Some((section_y - min_sec) as usize)
        } else {
            None
        }
    }

    pub fn section_y_at_index(&self, index: usize) -> i8 {
        self.min_section_y() + index as i8
    }
}

impl Dimension {
    pub const fn height(self) -> WorldHeight {
        match self {
            Self::Overworld => WorldHeight::OVERWORLD,
            Self::Nether => WorldHeight::NETHER,
            Self::End => WorldHeight::END,
        }
    }

    pub const fn from_wire(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Overworld),
            1 => Some(Self::Nether),
            2 => Some(Self::End),
            _ => None,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Overworld => "Overworld",
            Self::Nether => "Nether",
            Self::End => "The End",
        }
    }

    /// Relative directory below a world's root. The Overworld is stored at the
    /// root, matching the existing `SaveManager` layout.
    pub const fn save_subdir(self) -> &'static str {
        match self {
            Self::Overworld => "",
            Self::Nether => "dimensions/nether",
            Self::End => "dimensions/end",
        }
    }

    pub const fn has_sky_light(self) -> bool {
        matches!(self, Self::Overworld)
    }

    /// Dimension-wide minimum light, expressed on the same normalized 0..1
    /// scale used by the renderer.
    pub const fn ambient_light(self) -> f32 {
        match self {
            Self::Overworld => 0.0,
            Self::Nether => 0.10,
            Self::End => 0.15,
        }
    }
}

/// Converts a position between dimension coordinate systems. Only travel
/// between the Overworld and Nether changes horizontal scale; the End retains
/// coordinates in either direction.
pub fn transform_position(from: Dimension, to: Dimension, mut position: Vec3) -> Vec3 {
    match (from, to) {
        (Dimension::Overworld, Dimension::Nether) => {
            position.x /= 8.0;
            position.z /= 8.0;
        }
        (Dimension::Nether, Dimension::Overworld) => {
            position.x *= 8.0;
            position.z *= 8.0;
        }
        _ => {}
    }
    position
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorldGenerationOptions {
    pub world_type: crate::game_rules::WorldType,
    pub generate_structures: bool,
}

impl Default for WorldGenerationOptions {
    fn default() -> Self {
        Self {
            world_type: crate::game_rules::WorldType::Default,
            generate_structures: true,
        }
    }
}

pub fn generate_chunk(dimension: Dimension, chunk_x: i32, chunk_z: i32, seed: u32) -> Chunk {
    generate_chunk_with_options(
        dimension,
        chunk_x,
        chunk_z,
        seed,
        WorldGenerationOptions::default(),
    )
}

pub fn generate_chunk_with_options(
    dimension: Dimension,
    chunk_x: i32,
    chunk_z: i32,
    seed: u32,
    options: WorldGenerationOptions,
) -> Chunk {
    let mut chunk = match dimension {
        Dimension::Overworld if options.world_type == crate::game_rules::WorldType::Superflat => {
            generate_superflat_chunk(chunk_x, chunk_z, seed)
        }
        Dimension::Overworld => generate_overworld_chunk(chunk_x, chunk_z, seed),
        Dimension::Nether => generate_nether_chunk(chunk_x, chunk_z, seed),
        Dimension::End => generate_end_chunk(chunk_x, chunk_z, seed),
    };
    if options.generate_structures {
        static STRUCTURE_MANAGER: std::sync::OnceLock<crate::structure::StructureManager> =
            std::sync::OnceLock::new();
        let manager = STRUCTURE_MANAGER.get_or_init(|| crate::structure::StructureManager::new());
        manager.apply_structures_to_chunk(&mut chunk, dimension, seed);
    }
    chunk
}

fn generate_superflat_chunk(chunk_x: i32, chunk_z: i32, seed: u32) -> Chunk {
    let mut chunk = Chunk::new_with_seed(chunk_x, chunk_z, seed);
    let height = Dimension::Overworld.height();
    for x in 0..CHUNK_WIDTH {
        for z in 0..CHUNK_DEPTH {
            for y in height.min_y()..height.max_y_exclusive() {
                let block = match y {
                    -64 => BlockType::Bedrock,
                    -63..=61 => BlockType::Stone,
                    62..=63 => BlockType::Dirt,
                    64 => BlockType::Grass,
                    _ => BlockType::Air,
                };
                chunk.set_block_local(x, y, z, block);
            }
            chunk.update_heightmap(x, z);
        }
    }
    chunk.rebuild_torch_index();
    chunk.rebuild_redstone_index();
    chunk
}

fn generate_overworld_chunk(chunk_x: i32, chunk_z: i32, seed: u32) -> Chunk {
    let mut chunk = Chunk::new_with_seed(chunk_x, chunk_z, seed);
    chunk.rebuild_torch_index();
    chunk.rebuild_redstone_index();
    chunk
}

fn empty_blocks() -> Box<[[[BlockType; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH]> {
    vec![[[BlockType::Air; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH]
        .try_into()
        .expect("chunk block dimensions are fixed")
}

fn empty_light() -> Box<[[[u8; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH]> {
    vec![[[0; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH]
        .try_into()
        .expect("chunk light dimensions are fixed")
}

fn hash3(seed: u32, x: i32, y: i32, z: i32) -> u32 {
    let mut value = seed
        ^ (x as u32).wrapping_mul(0x9E37_79B9)
        ^ (y as u32).wrapping_mul(0x85EB_CA6B)
        ^ (z as u32).wrapping_mul(0xC2B2_AE35);
    value ^= value >> 16;
    value = value.wrapping_mul(0x7FEB_352D);
    value ^= value >> 15;
    value = value.wrapping_mul(0x846C_A68B);
    value ^ (value >> 16)
}

fn generate_nether_chunk(chunk_x: i32, chunk_z: i32, seed: u32) -> Chunk {
    let mut blocks = empty_blocks();
    let caves = Perlin::new(seed ^ 0x4E45_5448);
    let caverns = Perlin::new(seed ^ 0xC0A7_3E55);
    let valleys = Perlin::new(seed ^ 0x5015_A4D0);

    for x in 0..CHUNK_WIDTH {
        for z in 0..CHUNK_DEPTH {
            let world_x = chunk_x * CHUNK_WIDTH as i32 + x as i32;
            let world_z = chunk_z * CHUNK_DEPTH as i32 + z as i32;
            let valley = valleys
                .get([world_x as f64 * 0.012, world_z as f64 * 0.012])
                .abs()
                < 0.20;

            for y in 0..NETHER_HEIGHT {
                let block = if y == 0 || y == NETHER_HEIGHT - 1 {
                    BlockType::Bedrock
                } else if y <= 4 && hash3(seed, world_x, y as i32, world_z) % 5 < (5 - y) as u32
                    || y >= NETHER_HEIGHT - 5
                        && hash3(seed ^ 0xBED0_CAFE, world_x, y as i32, world_z) % 5
                            < (y - (NETHER_HEIGHT - 5) + 1) as u32
                {
                    BlockType::Bedrock
                } else {
                    let fine = caves.get([
                        world_x as f64 * 0.045,
                        y as f64 * 0.052,
                        world_z as f64 * 0.045,
                    ]);
                    let broad = caverns.get([
                        world_x as f64 * 0.014,
                        y as f64 * 0.018,
                        world_z as f64 * 0.014,
                    ]);
                    let open = fine.abs() < 0.115 || broad > 0.53;
                    if open {
                        if y <= LAVA_LEVEL {
                            BlockType::Lava
                        } else {
                            BlockType::Air
                        }
                    } else {
                        BlockType::Netherrack
                    }
                };
                blocks[x][y][z] = block;
            }

            if valley {
                for y in 6..(NETHER_HEIGHT - 5) {
                    if blocks[x][y][z] == BlockType::Netherrack
                        && matches!(blocks[x][y + 1][z], BlockType::Air | BlockType::Lava)
                    {
                        blocks[x][y][z] = BlockType::SoulSand;
                    }
                }
            }
        }
    }

    // Seed glowstone at cave ceilings. If a particularly solid chunk has no
    // natural ceiling candidate, add one deterministic embedded cluster so
    // every generated region still exposes the Nether's light source.
    let mut glowstone_count = 0;
    for x in 0..CHUNK_WIDTH {
        for z in 0..CHUNK_DEPTH {
            let world_x = chunk_x * CHUNK_WIDTH as i32 + x as i32;
            let world_z = chunk_z * CHUNK_DEPTH as i32 + z as i32;
            for y in 35..(NETHER_HEIGHT - 6) {
                if blocks[x][y][z] == BlockType::Air
                    && blocks[x][y + 1][z] == BlockType::Netherrack
                    && hash3(seed ^ 0x6105_700E, world_x, y as i32, world_z) % 149 == 0
                {
                    blocks[x][y][z] = BlockType::Glowstone;
                    if y > 35 && hash3(seed, world_x, y as i32, world_z) & 1 == 0 {
                        blocks[x][y - 1][z] = BlockType::Glowstone;
                    }
                    glowstone_count += 1;
                }
            }
        }
    }
    if glowstone_count == 0 {
        let x = (hash3(seed, chunk_x, 17, chunk_z) as usize) % CHUNK_WIDTH;
        let z = (hash3(seed, chunk_x, 29, chunk_z) as usize) % CHUNK_DEPTH;
        blocks[x][112][z] = BlockType::Glowstone;
        if blocks[x][111][z] == BlockType::Netherrack {
            blocks[x][111][z] = BlockType::Air;
        }
    }

    // Make both signature low-Nether features observable even for seeds whose
    // noise happens to keep this chunk unusually solid.
    if !contains_block(&blocks, BlockType::Lava) {
        blocks[CHUNK_WIDTH / 2][LAVA_LEVEL][CHUNK_DEPTH / 2] = BlockType::Lava;
    }
    if !contains_block(&blocks, BlockType::SoulSand) {
        'surface: for x in 0..CHUNK_WIDTH {
            for z in 0..CHUNK_DEPTH {
                for y in (6..123).rev() {
                    if blocks[x][y][z] == BlockType::Netherrack
                        && blocks[x][y + 1][z] == BlockType::Air
                    {
                        blocks[x][y][z] = BlockType::SoulSand;
                        break 'surface;
                    }
                }
            }
        }
        if !contains_block(&blocks, BlockType::SoulSand) {
            let x = (hash3(seed ^ 0x5015_A4D0, chunk_x, 32, chunk_z) as usize) % CHUNK_WIDTH;
            let z = (hash3(seed ^ 0x5015_A4D0, chunk_z, 32, chunk_x) as usize) % CHUNK_DEPTH;
            blocks[x][32][z] = BlockType::SoulSand;
            blocks[x][33][z] = BlockType::Air;
        }
    }

    finish_chunk(chunk_x, chunk_z, blocks, false)
}

fn contains_block(
    blocks: &[[[BlockType; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH],
    target: BlockType,
) -> bool {
    blocks
        .iter()
        .flat_map(|column| column.iter())
        .flat_map(|row| row.iter())
        .any(|block| *block == target)
}

fn end_surface_at(world_x: i32, world_z: i32, seed: u32) -> Option<i32> {
    let x = world_x as f64;
    let z = world_z as f64;
    let central_distance = x.hypot(z);
    if central_distance <= 112.0 {
        return Some(62 + ((1.0 - central_distance / 112.0) * 10.0).round() as i32);
    }

    let city_dx = (world_x - END_CITY_X) as f64;
    let city_dz = (world_z - END_CITY_Z) as f64;
    let city_distance = city_dx.hypot(city_dz);
    if city_distance <= 46.0 {
        return Some(64 + ((1.0 - city_distance / 46.0) * 6.0).round() as i32);
    }

    const GRID: i32 = 192;
    let cell_x = world_x.div_euclid(GRID);
    let cell_z = world_z.div_euclid(GRID);
    let mut best = None;
    for gx in (cell_x - 1)..=(cell_x + 1) {
        for gz in (cell_z - 1)..=(cell_z + 1) {
            let h = hash3(seed ^ 0xE0D1_51A0, gx, 0, gz);
            if h % 100 >= 38 {
                continue;
            }
            let center_x = gx * GRID + ((h & 0x7f) as i32 - 64);
            let center_z = gz * GRID + (((h >> 8) & 0x7f) as i32 - 64);
            if (center_x as f64).hypot(center_z as f64) < 300.0 {
                continue;
            }
            let radius = 28.0 + ((h >> 16) % 21) as f64;
            let distance = ((world_x - center_x) as f64).hypot((world_z - center_z) as f64);
            if distance <= radius {
                let surface = 58 + ((1.0 - distance / radius) * 8.0).round() as i32;
                best = Some(best.map_or(surface, |old: i32| old.max(surface)));
            }
        }
    }
    best
}

fn generate_end_chunk(chunk_x: i32, chunk_z: i32, seed: u32) -> Chunk {
    let mut blocks = empty_blocks();

    for x in 0..CHUNK_WIDTH {
        for z in 0..CHUNK_DEPTH {
            let world_x = chunk_x * CHUNK_WIDTH as i32 + x as i32;
            let world_z = chunk_z * CHUNK_DEPTH as i32 + z as i32;
            let Some(top) = end_surface_at(world_x, world_z, seed) else {
                continue;
            };
            let distance_from_origin = (world_x as f64).hypot(world_z as f64);
            let thickness = if distance_from_origin <= 112.0 {
                10 + ((1.0 - distance_from_origin / 112.0).max(0.0) * 18.0) as i32
            } else {
                9 + (hash3(seed, world_x, 0, world_z) % 7) as i32
            };
            for y in (top - thickness).max(1)..=top {
                blocks[x][y as usize][z] = BlockType::EndStone;
            }
        }
    }

    place_end_crystal_towers(&mut blocks, chunk_x, chunk_z, seed);
    place_end_exit(&mut blocks, chunk_x, chunk_z);
    finish_chunk(chunk_x, chunk_z, blocks, false)
}

fn place_end_crystal_towers(
    blocks: &mut Box<[[[BlockType; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH]>,
    chunk_x: i32,
    chunk_z: i32,
    seed: u32,
) {
    for (center_x, crystal_y, center_z) in END_CRYSTAL_TOWERS {
        let top_y = crystal_y - 1;
        for dx in -2..=2 {
            for dz in -2..=2 {
                if dx * dx + dz * dz > 4 {
                    continue;
                }
                let world_x = center_x + dx;
                let world_z = center_z + dz;
                let Some(surface_y) = end_surface_at(world_x, world_z, seed) else {
                    continue;
                };
                for y in (surface_y + 1)..=top_y {
                    set_world_block(
                        blocks,
                        chunk_x,
                        chunk_z,
                        world_x,
                        y,
                        world_z,
                        BlockType::Obsidian,
                    );
                }
            }
        }
    }
}

fn set_world_block(
    blocks: &mut Box<[[[BlockType; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH]>,
    chunk_x: i32,
    chunk_z: i32,
    world_x: i32,
    y: i32,
    world_z: i32,
    block: BlockType,
) {
    let local_x = world_x - chunk_x * CHUNK_WIDTH as i32;
    let local_z = world_z - chunk_z * CHUNK_DEPTH as i32;
    if (0..CHUNK_WIDTH as i32).contains(&local_x)
        && (0..CHUNK_DEPTH as i32).contains(&local_z)
        && (0..CHUNK_HEIGHT as i32).contains(&y)
    {
        blocks[local_x as usize][y as usize][local_z as usize] = block;
    }
}

fn place_end_exit(
    blocks: &mut Box<[[[BlockType; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH]>,
    chunk_x: i32,
    chunk_z: i32,
) {
    const EXIT_Y: i32 = 73;
    // Generate only the dormant bedrock fountain. The portal blocks and egg
    // are materialized by the dragon-death event, so a fresh End always starts
    // with a live boss encounter.
    for offset in -2..=2 {
        set_world_block(
            blocks,
            chunk_x,
            chunk_z,
            offset,
            EXIT_Y,
            -2,
            BlockType::Bedrock,
        );
        set_world_block(
            blocks,
            chunk_x,
            chunk_z,
            offset,
            EXIT_Y,
            2,
            BlockType::Bedrock,
        );
        set_world_block(
            blocks,
            chunk_x,
            chunk_z,
            -2,
            EXIT_Y,
            offset,
            BlockType::Bedrock,
        );
        set_world_block(
            blocks,
            chunk_x,
            chunk_z,
            2,
            EXIT_Y,
            offset,
            BlockType::Bedrock,
        );
    }
    for y in EXIT_Y..=(EXIT_Y + 4) {
        set_world_block(blocks, chunk_x, chunk_z, 0, y, 0, BlockType::Bedrock);
    }
}

fn finish_chunk(
    chunk_x: i32,
    chunk_z: i32,
    blocks: Box<[[[BlockType; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH]>,
    has_sky_light: bool,
) -> Chunk {
    let mut sky_light = empty_light();
    let mut block_light = empty_light();
    let mut fluid_levels = empty_light();
    let mut heightmap: Box<[[i16; CHUNK_DEPTH]; CHUNK_WIDTH]> =
        vec![[crate::world::NO_HEIGHT; CHUNK_DEPTH]; CHUNK_WIDTH]
            .try_into()
            .expect("chunk heightmap dimensions are fixed");
    let mut light_queue = VecDeque::new();

    for x in 0..CHUNK_WIDTH {
        for z in 0..CHUNK_DEPTH {
            let mut direct_sky = if has_sky_light { 15 } else { 0 };
            let mut found_height = false;
            for y in (0..CHUNK_HEIGHT).rev() {
                let block = blocks[x][y][z];
                if !found_height && block != BlockType::Air {
                    heightmap[x][z] = y as i16;
                    found_height = true;
                }
                if block.properties().render_type == RenderType::Opaque {
                    direct_sky = 0;
                }
                sky_light[x][y][z] = direct_sky;
                let emission = block.properties().light_emission;
                if emission > 0 {
                    block_light[x][y][z] = emission;
                    light_queue.push_back((x, y, z));
                }
                if matches!(block, BlockType::Water | BlockType::Lava) {
                    // Level zero represents a full source block in the fluid
                    // system; the remaining bits are reserved for falling flow.
                    fluid_levels[x][y][z] = 0;
                }
            }
        }
    }

    const NEIGHBORS: [(isize, isize, isize); 6] = [
        (1, 0, 0),
        (-1, 0, 0),
        (0, 1, 0),
        (0, -1, 0),
        (0, 0, 1),
        (0, 0, -1),
    ];
    while let Some((x, y, z)) = light_queue.pop_front() {
        let current_light = block_light[x][y][z];
        if current_light <= 1 {
            continue;
        }
        let next_light = current_light - 1;
        for (dx, dy, dz) in NEIGHBORS {
            let nx = x as isize + dx;
            let ny = y as isize + dy;
            let nz = z as isize + dz;
            if nx < 0
                || nx >= CHUNK_WIDTH as isize
                || ny < 0
                || ny >= CHUNK_HEIGHT as isize
                || nz < 0
                || nz >= CHUNK_DEPTH as isize
            {
                continue;
            }
            let (nx, ny, nz) = (nx as usize, ny as usize, nz as usize);
            if blocks[nx][ny][nz].properties().render_type != RenderType::Opaque
                && block_light[nx][ny][nz] < next_light
            {
                block_light[nx][ny][nz] = next_light;
                light_queue.push_back((nx, ny, nz));
            }
        }
    }

    let mut sections = Vec::with_capacity(crate::world::SECTION_COUNT);
    for sec_y in 0..crate::world::SECTION_COUNT {
        let mut sec_b = [BlockType::Air; 4096];
        let mut sec_sk = [0u8; 4096];
        let mut sec_bl = [0u8; 4096];
        let mut sec_fl = [0u8; 4096];
        for ly in 0..crate::world::SECTION_SIZE {
            let y = sec_y * crate::world::SECTION_SIZE + ly;
            for z in 0..CHUNK_DEPTH {
                for x in 0..CHUNK_WIDTH {
                    let idx = (ly << 8) | (z << 4) | x;
                    sec_b[idx] = blocks[x][y][z];
                    sec_sk[idx] = sky_light[x][y][z];
                    sec_bl[idx] = block_light[x][y][z];
                    sec_fl[idx] = fluid_levels[x][y][z];
                }
            }
        }
        let sec =
            crate::world::ChunkSection::from_dense(&sec_b, &sec_sk, &sec_bl, None, Some(&sec_fl));
        if sec.is_empty() {
            sections.push(None);
        } else {
            sections.push(Some(sec));
        }
    }

    let mut chunk = Chunk {
        chunk_x,
        chunk_z,
        min_section_y: 0,
        sections,
        heightmap,
        torch_positions: Vec::new(),
        redstone_positions: Vec::new(),
        block_entities: std::collections::HashMap::new(),
    };
    chunk.rebuild_torch_index();
    chunk.rebuild_redstone_index();
    chunk
}

/// Finds a complete 4x5 obsidian frame in either vertical axis and returns its
/// ordered 2x3 interior. The clicked position may be any frame or interior
/// block, which makes this suitable for both flint-and-steel and block-update
/// call sites.
pub fn detect_nether_frame<F>(clicked: BlockPos, mut getter: F) -> Option<Vec<BlockPos>>
where
    F: FnMut(i32, i32, i32) -> BlockType,
{
    for horizontal_offset in 0..=3 {
        for vertical_offset in 0..=4 {
            let base_x = clicked.0 - horizontal_offset;
            let base_y = clicked.1 - vertical_offset;
            if let Some(interior) = nether_frame_x(base_x, base_y, clicked.2, &mut getter) {
                return Some(interior);
            }

            let base_z = clicked.2 - horizontal_offset;
            if let Some(interior) = nether_frame_z(clicked.0, base_y, base_z, &mut getter) {
                return Some(interior);
            }
        }
    }
    None
}

fn nether_frame_x<F>(base_x: i32, base_y: i32, z: i32, getter: &mut F) -> Option<Vec<BlockPos>>
where
    F: FnMut(i32, i32, i32) -> BlockType,
{
    for x in base_x..=(base_x + 3) {
        if getter(x, base_y, z) != BlockType::Obsidian
            || getter(x, base_y + 4, z) != BlockType::Obsidian
        {
            return None;
        }
    }
    for y in (base_y + 1)..=(base_y + 3) {
        if getter(base_x, y, z) != BlockType::Obsidian
            || getter(base_x + 3, y, z) != BlockType::Obsidian
        {
            return None;
        }
    }
    let mut interior = Vec::with_capacity(6);
    for x in (base_x + 1)..=(base_x + 2) {
        for y in (base_y + 1)..=(base_y + 3) {
            if !matches!(
                getter(x, y, z),
                BlockType::Air | BlockType::Fire | BlockType::NetherPortal
            ) {
                return None;
            }
            interior.push((x, y, z));
        }
    }
    Some(interior)
}

fn nether_frame_z<F>(x: i32, base_y: i32, base_z: i32, getter: &mut F) -> Option<Vec<BlockPos>>
where
    F: FnMut(i32, i32, i32) -> BlockType,
{
    for z in base_z..=(base_z + 3) {
        if getter(x, base_y, z) != BlockType::Obsidian
            || getter(x, base_y + 4, z) != BlockType::Obsidian
        {
            return None;
        }
    }
    for y in (base_y + 1)..=(base_y + 3) {
        if getter(x, y, base_z) != BlockType::Obsidian
            || getter(x, y, base_z + 3) != BlockType::Obsidian
        {
            return None;
        }
    }
    let mut interior = Vec::with_capacity(6);
    for z in (base_z + 1)..=(base_z + 2) {
        for y in (base_y + 1)..=(base_y + 3) {
            if !matches!(
                getter(x, y, z),
                BlockType::Air | BlockType::Fire | BlockType::NetherPortal
            ) {
                return None;
            }
            interior.push((x, y, z));
        }
    }
    Some(interior)
}

/// Recognizes the twelve filled frame blocks around a horizontal 3x3 End
/// portal and returns the nine interior positions.
pub fn detect_completed_end_portal<F>(changed: BlockPos, mut getter: F) -> Option<Vec<BlockPos>>
where
    F: FnMut(i32, i32, i32) -> BlockType,
{
    let y = changed.1;
    for x_offset in 0..=4 {
        for z_offset in 0..=4 {
            let base_x = changed.0 - x_offset;
            let base_z = changed.2 - z_offset;
            let mut complete = true;
            for offset in 1..=3 {
                complete &= getter(base_x + offset, y, base_z) == BlockType::EndPortalFrameFilled;
                complete &=
                    getter(base_x + offset, y, base_z + 4) == BlockType::EndPortalFrameFilled;
                complete &= getter(base_x, y, base_z + offset) == BlockType::EndPortalFrameFilled;
                complete &=
                    getter(base_x + 4, y, base_z + offset) == BlockType::EndPortalFrameFilled;
            }
            if complete {
                let mut interior = Vec::with_capacity(9);
                for x in (base_x + 1)..=(base_x + 3) {
                    for z in (base_z + 1)..=(base_z + 3) {
                        interior.push((x, y, z));
                    }
                }
                return Some(interior);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn dimension_scaling_only_affects_overworld_nether_travel() {
        let position = Vec3::new(80.0, 70.0, -40.0);
        assert_eq!(
            transform_position(Dimension::Overworld, Dimension::Nether, position),
            Vec3::new(10.0, 70.0, -5.0)
        );
        assert_eq!(
            transform_position(
                Dimension::Nether,
                Dimension::Overworld,
                Vec3::new(10.0, 70.0, -5.0)
            ),
            position
        );
        assert_eq!(
            transform_position(Dimension::Overworld, Dimension::End, position),
            position
        );
    }

    fn chunk_contains_block(chunk: &Chunk, target: BlockType) -> bool {
        for x in 0..CHUNK_WIDTH {
            for y in 0..CHUNK_HEIGHT {
                let wy = y as i32 + (chunk.min_section_y as i32 * 16);
                for z in 0..CHUNK_DEPTH {
                    if chunk.get_block_local(x, wy, z) == target {
                        return true;
                    }
                }
            }
        }
        false
    }

    #[test]
    fn nether_has_roof_lava_features_and_no_sky_light() {
        let chunk = generate_chunk(Dimension::Nether, 0, 0, 42);
        for x in 0..CHUNK_WIDTH {
            for z in 0..CHUNK_DEPTH {
                assert_eq!(chunk.get_block_local(x, 0, z), BlockType::Bedrock);
                assert_eq!(
                    chunk.get_block_local(x, (NETHER_HEIGHT - 1) as i32, z),
                    BlockType::Bedrock
                );
            }
        }
        assert!(chunk_contains_block(&chunk, BlockType::Lava));
        assert!(chunk_contains_block(&chunk, BlockType::SoulSand));
        assert!(chunk_contains_block(&chunk, BlockType::Glowstone));
        // Verify no sky light anywhere in the nether chunk
        let mut all_sky_zero = true;
        'outer_sky: for x in 0..CHUNK_WIDTH {
            for y in 0..CHUNK_HEIGHT {
                let wy = y as i32 + (chunk.min_section_y as i32 * 16);
                for z in 0..CHUNK_DEPTH {
                    if chunk.get_sky_light(x, wy, z) != 0 {
                        all_sky_zero = false;
                        break 'outer_sky;
                    }
                }
            }
        }
        assert!(all_sky_zero);
        // Verify some block light == 15 exists (from lava/glowstone)
        let mut has_max_block_light = false;
        'outer_bl: for x in 0..CHUNK_WIDTH {
            for y in 0..CHUNK_HEIGHT {
                let wy = y as i32 + (chunk.min_section_y as i32 * 16);
                for z in 0..CHUNK_DEPTH {
                    if chunk.get_block_light(x, wy, z) == 15 {
                        has_max_block_light = true;
                        break 'outer_bl;
                    }
                }
            }
        }
        assert!(has_max_block_light);
    }

    #[test]
    fn end_has_dormant_origin_fountain_and_reachable_city_island() {
        let origin = generate_chunk(Dimension::End, 0, 0, 7);
        assert!(chunk_contains_block(&origin, BlockType::EndStone));
        assert!(chunk_contains_block(&origin, BlockType::Bedrock));
        assert!(!chunk_contains_block(&origin, BlockType::EndPortal));
        assert!(!chunk_contains_block(&origin, BlockType::DragonEgg));

        let seed = 7;
        let pos = crate::structure::locate_structure(
            crate::structure::StructureId::EndCity,
            (1000, 64, 0),
            seed,
            Dimension::End,
        )
        .expect("End City candidate found");

        let chunk_x = pos.0.div_euclid(CHUNK_WIDTH as i32);
        let chunk_z = pos.2.div_euclid(CHUNK_DEPTH as i32);
        let city = generate_chunk(Dimension::End, chunk_x, chunk_z, seed);
        assert!(
            chunk_contains_block(&city, BlockType::EndStone)
                || chunk_contains_block(&city, BlockType::EndStoneBrick)
        );
        assert!(chunk_contains_block(&city, BlockType::Purpur));
    }

    #[test]
    fn overworld_stronghold_contains_twelve_empty_frames() {
        let seed = 12345;
        let pos = crate::structure::locate_structure(
            crate::structure::StructureId::Stronghold,
            (0, 64, 0),
            seed,
            Dimension::Overworld,
        )
        .expect("Stronghold located");

        let chunk_x = pos.0.div_euclid(CHUNK_WIDTH as i32);
        let chunk_z = pos.2.div_euclid(CHUNK_DEPTH as i32);

        let mut frames = 0;
        for cx in (chunk_x - 1)..=(chunk_x + 1) {
            for cz in (chunk_z - 1)..=(chunk_z + 1) {
                let chunk = generate_chunk(Dimension::Overworld, cx, cz, seed);
                for x in 0..CHUNK_WIDTH {
                    for y in 0..CHUNK_HEIGHT {
                        let wy = y as i32 + (chunk.min_section_y as i32 * 16);
                        for z in 0..CHUNK_DEPTH {
                            let block = chunk.get_block_local(x, wy, z);
                            if block == BlockType::EndPortalFrame
                                || block == BlockType::EndPortalFrameFilled
                            {
                                frames += 1;
                            }
                        }
                    }
                }
            }
        }
        assert_eq!(frames, 12);
    }

    #[test]
    fn end_healing_crystals_have_obsidian_tower_support() {
        let seed = 7;
        let (center_x, crystal_y, center_z) = END_CRYSTAL_TOWERS[0];
        let chunk = generate_chunk(
            Dimension::End,
            center_x.div_euclid(CHUNK_WIDTH as i32),
            center_z.div_euclid(CHUNK_DEPTH as i32),
            seed,
        );
        let local_x = center_x.rem_euclid(CHUNK_WIDTH as i32) as usize;
        let local_z = center_z.rem_euclid(CHUNK_DEPTH as i32) as usize;
        assert_eq!(
            chunk.get_block_local(local_x, crystal_y - 1, local_z),
            BlockType::Obsidian
        );
        let surface_y = end_surface_at(center_x, center_z, seed).unwrap();
        assert_eq!(
            chunk.get_block_local(local_x, surface_y + 1, local_z),
            BlockType::Obsidian
        );
    }

    #[test]
    fn custom_dimension_generation_is_deterministic() {
        let a = generate_chunk(Dimension::Nether, -3, 5, 99);
        let b = generate_chunk(Dimension::Nether, -3, 5, 99);
        assert_eq!(a.sections.len(), b.sections.len());
        for x in 0..CHUNK_WIDTH {
            for y in 0..CHUNK_HEIGHT {
                let wy = y as i32 + (a.min_section_y as i32 * 16);
                for z in 0..CHUNK_DEPTH {
                    assert_eq!(a.get_block_local(x, wy, z), b.get_block_local(x, wy, z));
                    assert_eq!(a.get_sky_light(x, wy, z), b.get_sky_light(x, wy, z));
                    assert_eq!(a.get_block_light(x, wy, z), b.get_block_light(x, wy, z));
                }
            }
        }
        assert_eq!(&*a.heightmap, &*b.heightmap);

        let end_a = generate_chunk(Dimension::End, 15, -8, 123);
        let end_b = generate_chunk(Dimension::End, 15, -8, 123);
        for x in 0..CHUNK_WIDTH {
            for y in 0..CHUNK_HEIGHT {
                let wy = y as i32 + (end_a.min_section_y as i32 * 16);
                for z in 0..CHUNK_DEPTH {
                    assert_eq!(
                        end_a.get_block_local(x, wy, z),
                        end_b.get_block_local(x, wy, z)
                    );
                }
            }
        }
    }

    fn put_x_frame(blocks: &mut HashMap<BlockPos, BlockType>, base: BlockPos) {
        for x in base.0..=(base.0 + 3) {
            blocks.insert((x, base.1, base.2), BlockType::Obsidian);
            blocks.insert((x, base.1 + 4, base.2), BlockType::Obsidian);
        }
        for y in (base.1 + 1)..=(base.1 + 3) {
            blocks.insert((base.0, y, base.2), BlockType::Obsidian);
            blocks.insert((base.0 + 3, y, base.2), BlockType::Obsidian);
        }
    }

    fn put_z_frame(blocks: &mut HashMap<BlockPos, BlockType>, base: BlockPos) {
        for z in base.2..=(base.2 + 3) {
            blocks.insert((base.0, base.1, z), BlockType::Obsidian);
            blocks.insert((base.0, base.1 + 4, z), BlockType::Obsidian);
        }
        for y in (base.1 + 1)..=(base.1 + 3) {
            blocks.insert((base.0, y, base.2), BlockType::Obsidian);
            blocks.insert((base.0, y, base.2 + 3), BlockType::Obsidian);
        }
    }

    #[test]
    fn detects_nether_frames_in_both_vertical_axes() {
        let mut x_blocks = HashMap::new();
        put_x_frame(&mut x_blocks, (10, 20, -4));
        let x_interior = detect_nether_frame((10, 22, -4), |x, y, z| {
            x_blocks.get(&(x, y, z)).copied().unwrap_or(BlockType::Air)
        })
        .expect("X-axis frame");
        assert_eq!(x_interior.len(), 6);
        assert!(x_interior.iter().all(|position| position.2 == -4));

        let mut z_blocks = HashMap::new();
        put_z_frame(&mut z_blocks, (-7, 9, 30));
        let z_interior = detect_nether_frame((-7, 13, 31), |x, y, z| {
            z_blocks.get(&(x, y, z)).copied().unwrap_or(BlockType::Air)
        })
        .expect("Z-axis frame");
        assert_eq!(z_interior.len(), 6);
        assert!(z_interior.iter().all(|position| position.0 == -7));
    }

    #[test]
    fn recognizes_twelve_filled_end_frames() {
        let mut blocks = HashMap::new();
        let (base_x, y, base_z) = (5, 64, -11);
        for offset in 1..=3 {
            blocks.insert(
                (base_x + offset, y, base_z),
                BlockType::EndPortalFrameFilled,
            );
            blocks.insert(
                (base_x + offset, y, base_z + 4),
                BlockType::EndPortalFrameFilled,
            );
            blocks.insert(
                (base_x, y, base_z + offset),
                BlockType::EndPortalFrameFilled,
            );
            blocks.insert(
                (base_x + 4, y, base_z + offset),
                BlockType::EndPortalFrameFilled,
            );
        }
        let interior = detect_completed_end_portal((base_x + 2, y, base_z), |x, y, z| {
            blocks.get(&(x, y, z)).copied().unwrap_or(BlockType::Air)
        })
        .expect("completed End portal");
        assert_eq!(interior.len(), 9);
        assert!(interior.contains(&(base_x + 2, y, base_z + 2)));

        blocks.remove(&(base_x + 2, y, base_z));
        assert!(
            detect_completed_end_portal((base_x + 2, y, base_z), |x, y, z| {
                blocks.get(&(x, y, z)).copied().unwrap_or(BlockType::Air)
            })
            .is_none()
        );
    }

    #[test]
    fn superflat_generation_is_signed_height_and_deterministic() {
        let options = WorldGenerationOptions {
            world_type: crate::game_rules::WorldType::Superflat,
            generate_structures: false,
        };
        let first = generate_chunk_with_options(Dimension::Overworld, 3, -2, 123, options);
        let second = generate_chunk_with_options(Dimension::Overworld, 3, -2, 123, options);
        for x in 0..CHUNK_WIDTH {
            for z in 0..CHUNK_DEPTH {
                assert_eq!(first.get_block_local(x, -64, z), BlockType::Bedrock);
                assert_eq!(first.get_block_local(x, 64, z), BlockType::Grass);
                for y in [-63, 0, 61] {
                    assert_eq!(first.get_block_local(x, y, z), BlockType::Stone);
                }
                assert_eq!(first.get_block_local(x, 65, z), BlockType::Air);
                for y in -64..320 {
                    assert_eq!(
                        first.get_block_local(x, y, z),
                        second.get_block_local(x, y, z)
                    );
                }
            }
        }
    }
}
