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
            BlockType::Air => (0, 0),
        }
    }
}

pub struct Chunk {
    pub chunk_x: i32,
    pub chunk_z: i32,
    pub blocks: [[[BlockType; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH],
    pub sky_light: [[[u8; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH],
    pub block_light: [[[u8; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH],
}

impl Chunk {
    pub fn new(chunk_x: i32, chunk_z: i32) -> Self {
        let mut blocks = [[[BlockType::Air; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH];
        let perlin = Perlin::new(12345); // Seed: 12345

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
                for y in 0..CHUNK_HEIGHT {
                    // Bedrock Y=0-4
                    if y <= 4 {
                        if y == 0 {
                            blocks[x][y][z] = BlockType::Bedrock;
                        } else {
                            // Blended bedrock
                            let threshold = (5 - y) as u8 * 50; // Chance of bedrock
                            if next_rand(0, 255) < threshold {
                                blocks[x][y][z] = BlockType::Bedrock;
                            } else {
                                blocks[x][y][z] = BlockType::Stone;
                            }
                        }
                    }
                    // Underground Stone Layer
                    else if y < base_height - 4 {
                        // Ore generation distribution
                        let block = if y < 16 && next_rand(0, 100) < 2 {
                            if next_rand(0, 2) == 0 { BlockType::DiamondOre } else { BlockType::RedstoneOre }
                        } else if y < 32 && next_rand(0, 100) < 3 {
                            BlockType::GoldOre
                        } else if y < 64 && next_rand(0, 100) < 5 {
                            BlockType::IronOre
                        } else if y < 128 && next_rand(0, 100) < 8 {
                            BlockType::CoalOre
                        } else {
                            BlockType::Stone
                        };
                        blocks[x][y][z] = block;
                    }
                    // Dirt/Sand layer
                    else if y < base_height {
                        if is_beach {
                            blocks[x][y][z] = BlockType::Sand;
                        } else {
                            blocks[x][y][z] = BlockType::Dirt;
                        }
                    }
                    // Surface block
                    else if y == base_height {
                        if is_beach {
                            blocks[x][y][z] = BlockType::Sand;
                        } else {
                            blocks[x][y][z] = BlockType::Grass;
                        }
                    }
                    // Water/Air layer above base height
                    else {
                        if y <= 62 {
                            blocks[x][y][z] = BlockType::Water;
                        } else {
                            blocks[x][y][z] = BlockType::Air;
                        }
                    }
                }
            }
        }

        let mut sky_light = [[[0u8; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH];
        let mut block_light = [[[0u8; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH];

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

        Self {
            chunk_x,
            chunk_z,
            blocks,
            sky_light,
            block_light,
        }
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
            for y in 0..CHUNK_HEIGHT {
                for z in 0..CHUNK_DEPTH {
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

                            let max_light = neighbor_sky.max(neighbor_block);
                            let multiplier = match face_idx {
                                4 => 1.0, // Top
                                5 => 0.5, // Bottom
                                _ => 0.8, // Sides
                            };
                            let light_val = (max_light as f32 / 15.0) * multiplier;

                            for (offset, uv) in corner_data.iter() {
                                // 256x256 atlas -> 16 columns of 16x16 pixel blocks
                                let u = (uv[0] + tx_col as f32) * 0.0625;
                                let v = (uv[1] + tx_row as f32) * 0.0625;
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
