use crate::state::Vertex;
use noise::{NoiseFn, Perlin};


pub const CHUNK_WIDTH: usize = 16;
pub const CHUNK_HEIGHT: usize = 256;
pub const CHUNK_DEPTH: usize = 16;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum BlockType {
    Air = 0,
    Grass = 1,
    Dirt = 2,
    Stone = 3,
    Sand = 4,
    Gravel = 5,
    OakLog = 6,
    OakPlanks = 7,
    OakLeaves = 8,
    Cobblestone = 9,
    Bedrock = 10,
    Water = 11,
    CoalOre = 12,
    IronOre = 13,
    GoldOre = 14,
    DiamondOre = 15,
    RedstoneOre = 16,
    Glass = 17,
    Brick = 18,
    StoneBrick = 19,
    Snow = 20,
    Ice = 21,
    Clay = 22,
    Sandstone = 23,
    Obsidian = 24,
    CraftingTable = 25,
    Furnace = 26,
    Chest = 27,
    TNT = 28,
    Bookshelf = 29,
    Torch = 30,
    Lava = 31,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RenderType {
    Opaque,
    Cutout,
    Translucent,
}

pub struct BlockProperties {
    pub name: &'static str,
    pub hardness: f32,
    pub render_type: RenderType,
    pub is_solid: bool,
    pub is_passable: bool,
    pub light_emission: u8,
}

impl BlockType {
    pub fn sound_material(self) -> Option<crate::audio::SoundMaterial> {
        match self {
            BlockType::Air | BlockType::Water | BlockType::Lava => None,
            BlockType::Grass | BlockType::OakLeaves => Some(crate::audio::SoundMaterial::Grass),
            BlockType::OakLog | BlockType::OakPlanks | BlockType::Bookshelf | BlockType::CraftingTable | BlockType::Chest => Some(crate::audio::SoundMaterial::Wood),
            BlockType::Sand | BlockType::Clay => Some(crate::audio::SoundMaterial::Sand),
            BlockType::Gravel => Some(crate::audio::SoundMaterial::Gravel),
            BlockType::Snow => Some(crate::audio::SoundMaterial::Snow),
            BlockType::Ice => Some(crate::audio::SoundMaterial::Ice),
            BlockType::Glass => Some(crate::audio::SoundMaterial::Glass),
            _ => Some(crate::audio::SoundMaterial::Stone),
        }
    }

    pub fn properties(self) -> BlockProperties {
        match self {
            BlockType::Air => BlockProperties {
                name: "Air",
                hardness: 0.0,
                render_type: RenderType::Cutout,
                is_solid: false,
                is_passable: true,
                light_emission: 0,
            },
            BlockType::Grass => BlockProperties {
                name: "Grass Block",
                hardness: 0.6,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::Dirt => BlockProperties {
                name: "Dirt",
                hardness: 0.5,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::Stone => BlockProperties {
                name: "Stone",
                hardness: 1.5,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::Sand => BlockProperties {
                name: "Sand",
                hardness: 0.5,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::Gravel => BlockProperties {
                name: "Gravel",
                hardness: 0.6,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::OakLog => BlockProperties {
                name: "Oak Log",
                hardness: 2.0,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::OakPlanks => BlockProperties {
                name: "Oak Planks",
                hardness: 2.0,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::OakLeaves => BlockProperties {
                name: "Oak Leaves",
                hardness: 0.2,
                render_type: RenderType::Cutout,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::Cobblestone => BlockProperties {
                name: "Cobblestone",
                hardness: 2.0,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::Bedrock => BlockProperties {
                name: "Bedrock",
                hardness: -1.0,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::Water => BlockProperties {
                name: "Water",
                hardness: 100.0,
                render_type: RenderType::Translucent,
                is_solid: false,
                is_passable: true,
                light_emission: 0,
            },
            BlockType::CoalOre => BlockProperties {
                name: "Coal Ore",
                hardness: 3.0,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::IronOre => BlockProperties {
                name: "Iron Ore",
                hardness: 3.0,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::GoldOre => BlockProperties {
                name: "Gold Ore",
                hardness: 3.0,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::DiamondOre => BlockProperties {
                name: "Diamond Ore",
                hardness: 3.0,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::RedstoneOre => BlockProperties {
                name: "Redstone Ore",
                hardness: 3.0,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::Glass => BlockProperties {
                name: "Glass",
                hardness: 0.3,
                render_type: RenderType::Cutout,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::Brick => BlockProperties {
                name: "Brick",
                hardness: 2.0,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::StoneBrick => BlockProperties {
                name: "Stone Brick",
                hardness: 1.5,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::Snow => BlockProperties {
                name: "Snow Block",
                hardness: 0.1,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::Ice => BlockProperties {
                name: "Ice",
                hardness: 0.5,
                render_type: RenderType::Translucent,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::Clay => BlockProperties {
                name: "Clay",
                hardness: 0.6,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::Sandstone => BlockProperties {
                name: "Sandstone",
                hardness: 0.8,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::Obsidian => BlockProperties {
                name: "Obsidian",
                hardness: 50.0,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::CraftingTable => BlockProperties {
                name: "Crafting Table",
                hardness: 2.5,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::Furnace => BlockProperties {
                name: "Furnace",
                hardness: 3.5,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::Chest => BlockProperties {
                name: "Chest",
                hardness: 2.5,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::TNT => BlockProperties {
                name: "TNT",
                hardness: 0.0,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::Bookshelf => BlockProperties {
                name: "Bookshelf",
                hardness: 1.5,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::Torch => BlockProperties {
                name: "Torch",
                hardness: 0.0,
                render_type: RenderType::Cutout,
                is_solid: false,
                is_passable: false,
                light_emission: 14,
            },
            BlockType::Lava => BlockProperties {
                name: "Lava",
                hardness: 100.0,
                render_type: RenderType::Opaque,
                is_solid: false,
                is_passable: true,
                light_emission: 15,
            },
        }
    }

    pub fn get_face_tex_index(self, face_idx: usize) -> (u32, u32) {
        match self {
            BlockType::Grass => {
                if face_idx == 4 { (0, 0) }
                else if face_idx == 5 { (2, 0) }
                else { (1, 0) }
            }
            BlockType::Dirt => (2, 0),
            BlockType::Stone => (3, 0),
            BlockType::Sand => (4, 0),
            BlockType::Gravel => (5, 0),
            BlockType::OakPlanks => (6, 0),
            BlockType::OakLeaves => (7, 0),
            BlockType::Cobblestone => (8, 0),
            BlockType::Bedrock => (9, 0),
            BlockType::Water => (10, 0),
            BlockType::CoalOre => (11, 0),
            BlockType::IronOre => (12, 0),
            BlockType::GoldOre => (13, 0),
            BlockType::DiamondOre => (14, 0),
            BlockType::RedstoneOre => (15, 0),
            
            BlockType::Glass => (0, 1),
            BlockType::Brick => (1, 1),
            BlockType::StoneBrick => (2, 1),
            BlockType::Snow => {
                if face_idx == 4 { (3, 1) }
                else if face_idx == 5 { (2, 0) }
                else { (4, 1) }
            }
            BlockType::Ice => (5, 1),
            BlockType::Clay => (6, 1),
            BlockType::Sandstone => {
                if face_idx == 4 || face_idx == 5 { (7, 1) }
                else { (8, 1) }
            }
            BlockType::Obsidian => (9, 1),
            BlockType::OakLog => {
                if face_idx == 4 || face_idx == 5 { (10, 1) }
                else { (11, 1) }
            }
            BlockType::CraftingTable => {
                if face_idx == 4 { (12, 1) }
                else if face_idx == 5 { (6, 0) }
                else { (13, 1) }
            }
            BlockType::Furnace => {
                if face_idx == 0 { (14, 1) }
                else { (3, 0) }
            }
            BlockType::Chest => (15, 1),
            
            BlockType::TNT => {
                if face_idx == 4 { (0, 2) }
                else if face_idx == 5 { (1, 2) }
                else { (2, 2) }
            }
            BlockType::Bookshelf => {
                if face_idx == 4 || face_idx == 5 { (6, 0) }
                else { (3, 2) }
            }
            BlockType::Torch => (4, 2),
            BlockType::Lava => (15, 2),
            BlockType::Air => (0, 0),
        }
    }
}

pub struct Chunk {
    pub chunk_x: i32,
    pub chunk_z: i32,
    pub blocks: Box<[[[BlockType; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH]>,
    pub sky_light: Box<[[[u8; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH]>,
    pub block_light: Box<[[[u8; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH]>,
    /// Per-column max Y of non-air blocks (indexed as [x][z])
    pub heightmap: Box<[[u16; CHUNK_DEPTH]; CHUNK_WIDTH]>,
    pub fluid_levels: Box<[[[u8; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH]>,
}

impl Chunk {
    pub fn new(chunk_x: i32, chunk_z: i32) -> Self {
        // Allocate on the heap to avoid stack overflow (~192 KB per chunk)
        let mut blocks: Box<[[[BlockType; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH]> =
            vec![[[BlockType::Air; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH]
                .try_into().unwrap();
        let perlin = Perlin::new(12345); // Seed: 12345
        let caves_perlin = Perlin::new(54321);
        let caverns_perlin = Perlin::new(65432);

        // Simple custom PRNG for ore distribution and bedrock blending
        let mut rng_seed = (chunk_x as u32).wrapping_mul(31) ^ (chunk_z as u32);
        let mut next_rand = |min: u8, max: u8| -> u8 {
            rng_seed = rng_seed.wrapping_mul(1103515245).wrapping_add(12345);
            let val = (rng_seed / 65536) % 32768;
            let diff = max - min;
            if diff == 0 { return min; }
            min + (val % diff as u32) as u8
        };

        for x in 0..CHUNK_WIDTH {
            for z in 0..CHUNK_DEPTH {
                // Calculate surface height using Perlin noise
                let world_x = chunk_x * (CHUNK_WIDTH as i32) + x as i32;
                let world_z = chunk_z * (CHUNK_DEPTH as i32) + z as i32;
                let noise_val = perlin.get([world_x as f64 * 0.04, world_z as f64 * 0.04]);
                // Map noise value (-1.0 to 1.0) to height (e.g. 55 to 75)
                let base_height = (64.0 + noise_val * 12.0) as usize;
                
                let is_beach = base_height <= 63;
                let entrance_noise = perlin.get([world_x as f64 * 0.015, world_z as f64 * 0.015]);
                let is_entrance_zone = entrance_noise > 0.55 && base_height > 63;

                for y in 0..CHUNK_HEIGHT {
                    let world_y = y as i32;
                    let mut block;

                    // Bedrock Y=0-4
                    if y <= 4 {
                        if y == 0 {
                            block = BlockType::Bedrock;
                        } else {
                            // Blended bedrock
                            let threshold = (5 - y) as u8 * 50; // Chance of bedrock
                            if next_rand(0, 255) < threshold {
                                block = BlockType::Bedrock;
                            } else {
                                block = BlockType::Stone;
                            }
                        }
                    }
                    // Underground Stone Layer
                    else if y < base_height - 4 {
                        block = BlockType::Stone;
                    }
                    // Dirt/Sand layer
                    else if y < base_height {
                        if is_beach {
                            block = BlockType::Sand;
                        } else {
                            block = BlockType::Dirt;
                        }
                    }
                    // Surface block
                    else if y == base_height {
                        if is_beach {
                            block = BlockType::Sand;
                        } else {
                            block = BlockType::Grass;
                        }
                    }
                    // Water/Air layer above base height
                    else {
                        if y <= 62 {
                            block = BlockType::Water;
                        } else {
                            block = BlockType::Air;
                        }
                    }

                    // Carve caves
                    if y > 4 && block != BlockType::Water && block != BlockType::Bedrock {
                        let in_cave_zone = (y < base_height.saturating_sub(6) && y < 62)
                            || (is_entrance_zone && y <= base_height);

                        if in_cave_zone {
                            let cave_val = caves_perlin.get([world_x as f64 * 0.05, world_y as f64 * 0.08, world_z as f64 * 0.05]);
                            let cavern_val = caverns_perlin.get([world_x as f64 * 0.01, world_y as f64 * 0.01, world_z as f64 * 0.01]);
                            let threshold = if cavern_val > 0.6 { 0.20 } else { 0.08 };

                            if cave_val.abs() < threshold {
                                block = BlockType::Air;
                            }
                        }
                    }

                    blocks[x][y][z] = block;
                }
            }
        }

        // --- Pass 2: Ore Vein Distribution ---
        struct OreConfig {
            block_type: BlockType,
            min_y: i32,
            max_y: i32,
            vein_size: usize,
            frequency: usize,
        }

        let ore_configs = [
            OreConfig {
                block_type: BlockType::CoalOre,
                min_y: 0,
                max_y: 128,
                vein_size: 17,
                frequency: 15,
            },
            OreConfig {
                block_type: BlockType::IronOre,
                min_y: 0,
                max_y: 64,
                vein_size: 9,
                frequency: 10,
            },
            OreConfig {
                block_type: BlockType::GoldOre,
                min_y: 0,
                max_y: 32,
                vein_size: 9,
                frequency: 3,
            },
            OreConfig {
                block_type: BlockType::RedstoneOre,
                min_y: 0,
                max_y: 16,
                vein_size: 8,
                frequency: 4,
            },
            OreConfig {
                block_type: BlockType::DiamondOre,
                min_y: 0,
                max_y: 16,
                vein_size: 8,
                frequency: 1,
            },
        ];

        let mut next_rand_range = |min: i32, max: i32| -> i32 {
            if min >= max { return min; }
            let diff = (max - min) as u32;
            rng_seed = rng_seed.wrapping_mul(1103515245).wrapping_add(12345);
            let val = (rng_seed / 65536) % 32768;
            min + (val % diff) as i32
        };

        for config in &ore_configs {
            for _ in 0..config.frequency {
                let start_x = next_rand_range(0, CHUNK_WIDTH as i32) as usize;
                let start_z = next_rand_range(0, CHUNK_DEPTH as i32) as usize;
                let start_y = next_rand_range(config.min_y, config.max_y + 1) as usize;

                if start_y >= CHUNK_HEIGHT { continue; }

                if blocks[start_x][start_y][start_z] == BlockType::Stone {
                    let mut queue = Vec::new();
                    queue.push((start_x, start_y, start_z));
                    blocks[start_x][start_y][start_z] = config.block_type;
                    
                    let mut placed = 1;
                    let mut head = 0;

                    while head < queue.len() && placed < config.vein_size {
                        let (cx, cy, cz) = queue[head];
                        head += 1;

                        // Randomly select one of the 6 neighbor directions
                        let dir = next_rand_range(0, 6);
                        let neighbors = [
                            (cx as i32 + 1, cy as i32, cz as i32),
                            (cx as i32 - 1, cy as i32, cz as i32),
                            (cx as i32, cy as i32 + 1, cz as i32),
                            (cx as i32, cy as i32 - 1, cz as i32),
                            (cx as i32, cy as i32, cz as i32 + 1),
                            (cx as i32, cy as i32, cz as i32 - 1),
                        ];

                        let (nx, ny, nz) = neighbors[dir as usize];
                        if nx >= 0 && nx < CHUNK_WIDTH as i32
                            && nz >= 0 && nz < CHUNK_DEPTH as i32
                            && ny > 4 && ny < CHUNK_HEIGHT as i32 {
                            
                            let ux = nx as usize;
                            let uy = ny as usize;
                            let uz = nz as usize;
                            
                            if blocks[ux][uy][uz] == BlockType::Stone {
                                blocks[ux][uy][uz] = config.block_type;
                                queue.push((ux, uy, uz));
                                placed += 1;
                            }
                        }
                    }
                }
            }
        }

        let mut sky_light: Box<[[[u8; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH]> =
            vec![[[0u8; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH]
                .try_into().unwrap();
        let mut block_light: Box<[[[u8; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH]> =
            vec![[[0u8; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH]
                .try_into().unwrap();

        for x in 0..CHUNK_WIDTH {
            for z in 0..CHUNK_DEPTH {
                let mut direct_sky = 15;
                for y in (0..CHUNK_HEIGHT).rev() {
                    let block = blocks[x][y][z];
                    if block.properties().render_type == RenderType::Opaque {
                        direct_sky = 0;
                    }
                    sky_light[x][y][z] = direct_sky;

                    if block == BlockType::Torch {
                        block_light[x][y][z] = 14;
                    }
                }
            }
        }

        // Build heightmap: per-column max Y of non-air blocks
        let mut heightmap: Box<[[u16; CHUNK_DEPTH]; CHUNK_WIDTH]> =
            vec![[0u16; CHUNK_DEPTH]; CHUNK_WIDTH]
                .try_into().unwrap();
        for x in 0..CHUNK_WIDTH {
            for z in 0..CHUNK_DEPTH {
                for y in (0..CHUNK_HEIGHT).rev() {
                    if blocks[x][y][z] != BlockType::Air {
                        heightmap[x][z] = y as u16;
                        break;
                    }
                }
            }
        }

        let fluid_levels: Box<[[[u8; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH]> =
            vec![[[0u8; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH]
                .try_into().unwrap();

        Self {
            chunk_x,
            chunk_z,
            blocks,
            sky_light,
            block_light,
            heightmap,
            fluid_levels,
        }
    }

    /// Update heightmap for a single column after block placement/removal
    pub fn update_heightmap(&mut self, x: usize, z: usize) {
        for y in (0..CHUNK_HEIGHT).rev() {
            if self.blocks[x][y][z] != BlockType::Air {
                self.heightmap[x][z] = y as u16;
                return;
            }
        }
        self.heightmap[x][z] = 0;
    }

    pub fn get_block(&self, x: i32, y: i32, z: i32) -> BlockType {
        if x < 0 || x >= CHUNK_WIDTH as i32 || y < 0 || y >= CHUNK_HEIGHT as i32 || z < 0 || z >= CHUNK_DEPTH as i32 {
            return BlockType::Air; // 超出範圍視為空氣
        }
        self.blocks[x as usize][y as usize][z as usize]
    }

    // 生成用於渲染的頂點和索引，區分為不透明與半透明網格
    pub fn generate_mesh<F>(&self, get_block_at: F) -> (Vec<Vertex>, Vec<u32>, Vec<Vertex>, Vec<u32>)
    where
        F: Fn(i32, i32, i32) -> (BlockType, u8, u8),
    {
        let mut opaque_vertices = Vec::new();
        let mut opaque_indices = Vec::new();
        let mut trans_vertices = Vec::new();
        let mut trans_indices = Vec::new();

        // 方塊的 6 個面法線偏移量與面頂點定義
        // 順序：前、後、左、右、上、下
        let faces = [
            // 前面 (South) (0, 0, 1)
            ([0.0, 0.0, 1.0], [
                ([0.0, 0.0, 1.0], [0.0, 1.0]),
                ([1.0, 0.0, 1.0], [1.0, 1.0]),
                ([1.0, 1.0, 1.0], [1.0, 0.0]),
                ([0.0, 1.0, 1.0], [0.0, 0.0]),
            ]),
            // 後面 (North) (0, 0, -1)
            ([0.0, 0.0, -1.0], [
                ([1.0, 0.0, 0.0], [0.0, 1.0]),
                ([0.0, 0.0, 0.0], [1.0, 1.0]),
                ([0.0, 1.0, 0.0], [1.0, 0.0]),
                ([1.0, 1.0, 0.0], [0.0, 0.0]),
            ]),
            // 左面 (West) (-1, 0, 0)
            ([-1.0, 0.0, 0.0], [
                ([0.0, 0.0, 0.0], [0.0, 1.0]),
                ([0.0, 0.0, 1.0], [1.0, 1.0]),
                ([0.0, 1.0, 1.0], [1.0, 0.0]),
                ([0.0, 1.0, 0.0], [0.0, 0.0]),
            ]),
            // 右面 (East) (1, 0, 0)
            ([1.0, 0.0, 0.0], [
                ([1.0, 0.0, 1.0], [0.0, 1.0]),
                ([1.0, 0.0, 0.0], [1.0, 1.0]),
                ([1.0, 1.0, 0.0], [1.0, 0.0]),
                ([1.0, 1.0, 1.0], [0.0, 0.0]),
            ]),
            // 上面 (Up) (0, 1, 0)
            ([0.0, 1.0, 0.0], [
                ([0.0, 1.0, 1.0], [0.0, 1.0]),
                ([1.0, 1.0, 1.0], [1.0, 1.0]),
                ([1.0, 1.0, 0.0], [1.0, 0.0]),
                ([0.0, 1.0, 0.0], [0.0, 0.0]),
            ]),
            // 下面 (Down) (0, -1, 0)
            ([0.0, -1.0, 0.0], [
                ([0.0, 0.0, 0.0], [0.0, 1.0]),
                ([1.0, 0.0, 0.0], [1.0, 1.0]),
                ([1.0, 0.0, 1.0], [1.0, 0.0]),
                ([0.0, 0.0, 1.0], [0.0, 0.0]),
            ]),
        ];

        for x in 0..CHUNK_WIDTH {
            for z in 0..CHUNK_DEPTH {
                let max_y = self.heightmap[x][z] as usize;
                for y in 0..=max_y {
                    let block = self.blocks[x][y][z];
                    if block == BlockType::Air {
                        continue;
                    }

                    let world_x = self.chunk_x * CHUNK_WIDTH as i32 + x as i32;
                    let world_y = y as i32;
                    let world_z = self.chunk_z * CHUNK_DEPTH as i32 + z as i32;

                    for (face_idx, (normal, corner_data)) in faces.iter().enumerate() {
                        let nx = world_x + normal[0] as i32;
                        let ny = world_y + normal[1] as i32;
                        let nz = world_z + normal[2] as i32;

                        let (neighbor, neighbor_sky, neighbor_block) = get_block_at(nx, ny, nz);
                        let neighbor_props = neighbor.properties();

                        // Face Culling: 只有鄰居非 Opaque 才渲染（且排除相同水體相鄰）
                        let should_render = if neighbor == BlockType::Air {
                            true
                        } else if neighbor_props.render_type != RenderType::Opaque {
                            !(block == BlockType::Water && neighbor == BlockType::Water)
                        } else {
                            false
                        };

                        if should_render {
                            let block_render_type = block.properties().render_type;
                            let is_translucent = block_render_type == RenderType::Translucent;

                            let (v_list, i_list) = if is_translucent {
                                (&mut trans_vertices, &mut trans_indices)
                            } else {
                                (&mut opaque_vertices, &mut opaque_indices)
                            };

                            let start_idx = v_list.len() as u32;
                            let (tx_col, tx_row) = block.get_face_tex_index(face_idx);

                            let multiplier_code = match face_idx {
                                4 => 0.0, // Top
                                5 => 2.0, // Bottom
                                _ => 1.0, // Sides
                            };
                            let light_val = (neighbor_sky as f32) + (neighbor_block as f32) * 16.0 + multiplier_code * 256.0;

                            for (offset, uv) in corner_data.iter() {
                                // 256x256 atlas -> 16 columns of 16x16 pixel blocks
                                // Apply half-pixel inset adjustment to prevent Nearest-neighbor coordinate bleeding
                                let u_adj = if uv[0] == 0.0 { 0.005 } else { 0.995 };
                                let v_adj = if uv[1] == 0.0 { 0.005 } else { 0.995 };
                                let u = (u_adj + tx_col as f32) * 0.0625;
                                let v = (v_adj + tx_row as f32) * 0.0625;
                                v_list.push(Vertex {
                                    position: [
                                        world_x as f32 + offset[0],
                                        world_y as f32 + offset[1],
                                        world_z as f32 + offset[2],
                                    ],
                                    tex_coords: [u, v],
                                    light_level: light_val,
                                });
                            }

                            i_list.push(start_idx + 0);
                            i_list.push(start_idx + 1);
                            i_list.push(start_idx + 2);
                            i_list.push(start_idx + 0);
                            i_list.push(start_idx + 2);
                            i_list.push(start_idx + 3);
                        }
                    }
                }
            }
        }

        (opaque_vertices, opaque_indices, trans_vertices, trans_indices)
    }
}

use crate::inventory::{ToolType, ToolMaterial};

impl BlockType {
    pub fn preferred_tool(self) -> ToolType {
        match self {
            BlockType::Grass | BlockType::Dirt | BlockType::Sand | BlockType::Gravel | BlockType::Snow | BlockType::Clay | BlockType::Sandstone => ToolType::Shovel,
            BlockType::Stone | BlockType::Cobblestone | BlockType::CoalOre | BlockType::IronOre | BlockType::GoldOre | BlockType::DiamondOre | BlockType::RedstoneOre | BlockType::StoneBrick | BlockType::Obsidian | BlockType::Furnace => ToolType::Pickaxe,
            BlockType::OakLog | BlockType::OakPlanks | BlockType::CraftingTable | BlockType::Chest | BlockType::Bookshelf => ToolType::Axe,
            _ => ToolType::None,
        }
    }

    pub fn min_harvest_material(self) -> Option<ToolMaterial> {
        match self {
            BlockType::Stone | BlockType::Cobblestone | BlockType::CoalOre | BlockType::Furnace | BlockType::StoneBrick | BlockType::Sandstone => Some(ToolMaterial::Wood), // Stone tier tools or above
            BlockType::IronOre => Some(ToolMaterial::Stone),
            BlockType::GoldOre | BlockType::RedstoneOre | BlockType::DiamondOre => Some(ToolMaterial::Iron),
            BlockType::Obsidian => Some(ToolMaterial::Diamond),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_block_harvest_properties() {
        assert_eq!(BlockType::Obsidian.preferred_tool(), ToolType::Pickaxe);
        assert_eq!(BlockType::Obsidian.min_harvest_material(), Some(ToolMaterial::Diamond));
        assert_eq!(BlockType::OakPlanks.preferred_tool(), ToolType::Axe);
        assert_eq!(BlockType::OakPlanks.min_harvest_material(), None);
    }

    #[test]
    fn test_cave_generation() {
        let chunk = Chunk::new(0, 0);
        let mut air_underground = 0;
        let mut stone_underground = 0;
        for x in 0..CHUNK_WIDTH {
            for z in 0..CHUNK_DEPTH {
                for y in 5..50 {
                    let block = chunk.blocks[x][y][z];
                    if block == BlockType::Air {
                        air_underground += 1;
                    } else if block == BlockType::Stone {
                        stone_underground += 1;
                    }
                }
            }
        }
        assert!(air_underground > 0, "Caves should carve some air underground");
        assert!(stone_underground > 0, "Caves should leave some stone underground");
    }

    #[test]
    fn test_ore_clustering() {
        let chunk = Chunk::new(0, 0);
        let mut clustered = false;
        let mut coal_count = 0;
        for x in 0..CHUNK_WIDTH {
            for z in 0..CHUNK_DEPTH {
                for y in 0..CHUNK_HEIGHT {
                    if chunk.blocks[x][y][z] == BlockType::CoalOre {
                        coal_count += 1;
                        let neighbors = [
                            (x as i32 + 1, y as i32, z as i32),
                            (x as i32 - 1, y as i32, z as i32),
                            (x as i32, y as i32 + 1, z as i32),
                            (x as i32, y as i32 - 1, z as i32),
                            (x as i32, y as i32, z as i32 + 1),
                            (x as i32, y as i32, z as i32 - 1),
                        ];
                        for &(nx, ny, nz) in &neighbors {
                            if nx >= 0 && nx < CHUNK_WIDTH as i32
                                && nz >= 0 && nz < CHUNK_DEPTH as i32
                                && ny >= 0 && ny < CHUNK_HEIGHT as i32 {
                                if chunk.blocks[nx as usize][ny as usize][nz as usize] == BlockType::CoalOre {
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
                        let entrance_noise = perlin.get([world_x as f64 * 0.015, world_z as f64 * 0.015]);
                        if entrance_noise > 0.55 && base_height > 63 {
                            found_entrance = true;
                            break;
                        }
                    }
                    if found_entrance { break; }
                }
                if found_entrance {
                    found_chunk = Some((cx, cz));
                    break;
                }
            }
            if found_chunk.is_some() { break; }
        }

        assert!(found_chunk.is_some(), "Should find a chunk with entrance zone in range");
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
                    if chunk.blocks[x][base_height][z] == BlockType::Air {
                        found_surface_air = true;
                        break;
                    }
                }
            }
            if found_surface_air { break; }
        }
        assert!(found_surface_air, "Should carve some cave air at surface in entrance zones");
    }

    #[test]
    fn test_fluid_level_encoding() {
        let mut chunk = Chunk::new(0, 0);
        chunk.fluid_levels[0][10][0] = 5 | 0x08; // level 5, falling = true
        assert_eq!(chunk.fluid_levels[0][10][0] & 0x07, 5);
        assert_eq!((chunk.fluid_levels[0][10][0] & 0x08) != 0, true);
    }
}
