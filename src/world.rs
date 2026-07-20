use crate::state::Vertex;
use noise::{NoiseFn, Perlin};

pub const CHUNK_WIDTH: usize = 16;
pub const CHUNK_HEIGHT: usize = 256;
pub const CHUNK_DEPTH: usize = 16;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Biome {
    Plains,
    Forest,
    Desert,
    Taiga,
    Swamp,
    Mountains,
    Ocean,
}

impl Biome {
    pub fn get_biome(
        world_x: i32,
        world_z: i32,
        temp_perlin: &Perlin,
        moist_perlin: &Perlin,
        ocean_perlin: &Perlin,
    ) -> Self {
        let ocean_val = ocean_perlin.get([world_x as f64 * 0.001, world_z as f64 * 0.001]);
        if ocean_val < -0.35 {
            return Biome::Ocean;
        }

        let temp = temp_perlin.get([world_x as f64 * 0.002, world_z as f64 * 0.002]);
        let moist = moist_perlin.get([world_x as f64 * 0.002, world_z as f64 * 0.002]);

        if temp < -0.3 {
            if moist < -0.2 {
                Biome::Mountains
            } else {
                Biome::Taiga
            }
        } else if temp > 0.4 && moist < -0.3 {
            Biome::Desert
        } else if temp > 0.2 && moist > 0.4 {
            Biome::Swamp
        } else {
            if moist > 0.0 {
                Biome::Forest
            } else {
                Biome::Plains
            }
        }
    }

    pub fn terrain_params(self) -> (f64, f64) {
        match self {
            Biome::Plains => (65.0, 4.0),
            Biome::Forest => (66.0, 6.0),
            Biome::Desert => (65.0, 5.0),
            Biome::Taiga => (68.0, 8.0),
            Biome::Swamp => (62.0, 1.5),
            Biome::Mountains => (82.0, 22.0),
            Biome::Ocean => (50.0, 6.0),
        }
    }
}

fn get_interpolated_height(
    world_x: i32,
    world_z: i32,
    perlin: &Perlin,
    temp_perlin: &Perlin,
    moist_perlin: &Perlin,
    ocean_perlin: &Perlin,
) -> usize {
    let mut height_sum = 0.0;
    let mut weight_sum = 0.0;

    const SAMPLE_STEPS: [i32; 3] = [-8, 0, 8];

    for &dx in &SAMPLE_STEPS {
        for &dz in &SAMPLE_STEPS {
            let sx = world_x + dx;
            let sz = world_z + dz;

            let biome = Biome::get_biome(sx, sz, temp_perlin, moist_perlin, ocean_perlin);
            let (base, scale) = biome.terrain_params();

            let noise_val = perlin.get([sx as f64 * 0.04, sz as f64 * 0.04]);
            let local_height = base + noise_val * scale;

            let weight = match (dx == 0, dz == 0) {
                (true, true) => 1.0,                  // Center
                (true, false) | (false, true) => 0.5, // Cardinal
                (false, false) => 0.25,               // Diagonal
            };

            height_sum += local_height * weight;
            weight_sum += weight;
        }
    }

    (height_sum / weight_sum).round() as usize
}

fn place_oak_tree(
    blocks: &mut Box<[[[BlockType; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH]>,
    local_x: i32,
    local_z: i32,
    start_y: i32,
    height: i32,
) {
    // Place log trunk
    for dy in 0..height {
        let y = start_y + dy;
        if y >= 0
            && y < CHUNK_HEIGHT as i32
            && local_x >= 0
            && local_x < CHUNK_WIDTH as i32
            && local_z >= 0
            && local_z < CHUNK_DEPTH as i32
        {
            blocks[local_x as usize][y as usize][local_z as usize] = BlockType::OakLog;
        }
    }
    // Place leaves canopy
    for ly in (height - 3)..=height {
        let y = start_y + ly;
        if y < 0 || y >= CHUNK_HEIGHT as i32 {
            continue;
        }
        let radius: i32 = if ly == height {
            1
        } else if ly == height - 1 {
            1
        } else {
            2
        };
        for dx in -radius..=radius {
            for dz in -radius..=radius {
                if radius == 2 && dx.abs() == 2 && dz.abs() == 2 {
                    continue;
                } // Remove corners for 5x5
                let lx = local_x + dx;
                let lz = local_z + dz;
                if lx >= 0 && lx < CHUNK_WIDTH as i32 && lz >= 0 && lz < CHUNK_DEPTH as i32 {
                    let block = blocks[lx as usize][y as usize][lz as usize];
                    if block == BlockType::Air || block == BlockType::OakLeaves {
                        blocks[lx as usize][y as usize][lz as usize] = BlockType::OakLeaves;
                    }
                }
            }
        }
    }
}

fn place_birch_tree(
    blocks: &mut Box<[[[BlockType; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH]>,
    local_x: i32,
    local_z: i32,
    start_y: i32,
    height: i32,
) {
    for dy in 0..height {
        let y = start_y + dy;
        if y >= 0
            && y < CHUNK_HEIGHT as i32
            && local_x >= 0
            && local_x < CHUNK_WIDTH as i32
            && local_z >= 0
            && local_z < CHUNK_DEPTH as i32
        {
            blocks[local_x as usize][y as usize][local_z as usize] = BlockType::BirchLog;
        }
    }
    for ly in (height - 3)..=height {
        let y = start_y + ly;
        if y < 0 || y >= CHUNK_HEIGHT as i32 {
            continue;
        }
        let is_cross = ly == height || ly == height - 3;
        let radius: i32 = 1;
        for dx in -radius..=radius {
            for dz in -radius..=radius {
                if is_cross && dx.abs() == 1 && dz.abs() == 1 {
                    continue;
                }
                let lx = local_x + dx;
                let lz = local_z + dz;
                if lx >= 0 && lx < CHUNK_WIDTH as i32 && lz >= 0 && lz < CHUNK_DEPTH as i32 {
                    let block = blocks[lx as usize][y as usize][lz as usize];
                    if block == BlockType::Air {
                        blocks[lx as usize][y as usize][lz as usize] = BlockType::BirchLeaves;
                    }
                }
            }
        }
    }
}

fn place_spruce_tree(
    blocks: &mut Box<[[[BlockType; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH]>,
    local_x: i32,
    local_z: i32,
    start_y: i32,
    height: i32,
) {
    for dy in 0..height {
        let y = start_y + dy;
        if y >= 0
            && y < CHUNK_HEIGHT as i32
            && local_x >= 0
            && local_x < CHUNK_WIDTH as i32
            && local_z >= 0
            && local_z < CHUNK_DEPTH as i32
        {
            blocks[local_x as usize][y as usize][local_z as usize] = BlockType::SpruceLog;
        }
    }
    for ly in 2..=height {
        let y = start_y + ly;
        if y < 0 || y >= CHUNK_HEIGHT as i32 {
            continue;
        }
        let layer_from_top = height - ly;
        let (radius, is_cross): (i32, bool) = if layer_from_top == 0 {
            (0, false)
        } else if layer_from_top == 1 {
            (1, true)
        } else if layer_from_top % 2 == 0 {
            (1, false)
        } else {
            (2, true)
        };
        for dx in -radius..=radius {
            for dz in -radius..=radius {
                if is_cross && dx.abs() == radius && dz.abs() == radius {
                    continue;
                }
                let lx = local_x + dx;
                let lz = local_z + dz;
                if lx >= 0 && lx < CHUNK_WIDTH as i32 && lz >= 0 && lz < CHUNK_DEPTH as i32 {
                    let block = blocks[lx as usize][y as usize][lz as usize];
                    if block == BlockType::Air {
                        blocks[lx as usize][y as usize][lz as usize] = BlockType::SpruceLeaves;
                    }
                }
            }
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
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
    // Trees & Biomes Additions
    BirchLog = 32,
    BirchPlanks = 33,
    BirchLeaves = 34,
    SpruceLog = 35,
    SprucePlanks = 36,
    SpruceLeaves = 37,
    TallGrass = 38,
    Dandelion = 39,
    Poppy = 40,
    Cactus = 41,
    SugarCane = 42,
    Pumpkin = 43,
    Melon = 44,
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
    pub fn from_u8(val: u8) -> Self {
        if val <= 44 {
            unsafe { std::mem::transmute(val) }
        } else {
            BlockType::Air
        }
    }

    pub fn sound_material(self) -> Option<crate::audio::SoundMaterial> {
        match self {
            BlockType::Air | BlockType::Water | BlockType::Lava => None,
            BlockType::Grass
            | BlockType::OakLeaves
            | BlockType::BirchLeaves
            | BlockType::SpruceLeaves
            | BlockType::TallGrass
            | BlockType::Dandelion
            | BlockType::Poppy
            | BlockType::SugarCane => Some(crate::audio::SoundMaterial::Grass),
            BlockType::OakLog
            | BlockType::OakPlanks
            | BlockType::BirchLog
            | BlockType::BirchPlanks
            | BlockType::SpruceLog
            | BlockType::SprucePlanks
            | BlockType::Bookshelf
            | BlockType::CraftingTable
            | BlockType::Chest
            | BlockType::Pumpkin
            | BlockType::Melon => Some(crate::audio::SoundMaterial::Wood),
            BlockType::Sand | BlockType::Clay => Some(crate::audio::SoundMaterial::Sand),
            BlockType::Gravel | BlockType::Cactus => Some(crate::audio::SoundMaterial::Gravel),
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
            BlockType::BirchLog => BlockProperties {
                name: "Birch Log",
                hardness: 2.0,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::BirchPlanks => BlockProperties {
                name: "Birch Planks",
                hardness: 2.0,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::BirchLeaves => BlockProperties {
                name: "Birch Leaves",
                hardness: 0.2,
                render_type: RenderType::Cutout,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::SpruceLog => BlockProperties {
                name: "Spruce Log",
                hardness: 2.0,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::SprucePlanks => BlockProperties {
                name: "Spruce Planks",
                hardness: 2.0,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::SpruceLeaves => BlockProperties {
                name: "Spruce Leaves",
                hardness: 0.2,
                render_type: RenderType::Cutout,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::TallGrass => BlockProperties {
                name: "Tall Grass",
                hardness: 0.0,
                render_type: RenderType::Cutout,
                is_solid: false,
                is_passable: true,
                light_emission: 0,
            },
            BlockType::Dandelion => BlockProperties {
                name: "Dandelion",
                hardness: 0.0,
                render_type: RenderType::Cutout,
                is_solid: false,
                is_passable: true,
                light_emission: 0,
            },
            BlockType::Poppy => BlockProperties {
                name: "Poppy",
                hardness: 0.0,
                render_type: RenderType::Cutout,
                is_solid: false,
                is_passable: true,
                light_emission: 0,
            },
            BlockType::Cactus => BlockProperties {
                name: "Cactus",
                hardness: 0.4,
                render_type: RenderType::Cutout,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::SugarCane => BlockProperties {
                name: "Sugar Cane",
                hardness: 0.0,
                render_type: RenderType::Cutout,
                is_solid: false,
                is_passable: true,
                light_emission: 0,
            },
            BlockType::Pumpkin => BlockProperties {
                name: "Pumpkin",
                hardness: 1.0,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::Melon => BlockProperties {
                name: "Melon",
                hardness: 1.0,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
        }
    }

    /// Whether this block is a full, opaque cube that casts vertex ambient occlusion.
    pub fn is_ao_occluder(self) -> bool {
        let properties = self.properties();
        properties.is_solid && properties.render_type == RenderType::Opaque
    }

    pub fn get_face_tex_index(self, face_idx: usize) -> (u32, u32) {
        match self {
            BlockType::Grass => {
                if face_idx == 4 {
                    (0, 0)
                } else if face_idx == 5 {
                    (2, 0)
                } else {
                    (1, 0)
                }
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
                if face_idx == 4 {
                    (3, 1)
                } else if face_idx == 5 {
                    (2, 0)
                } else {
                    (4, 1)
                }
            }
            BlockType::Ice => (5, 1),
            BlockType::Clay => (6, 1),
            BlockType::Sandstone => {
                if face_idx == 4 || face_idx == 5 {
                    (7, 1)
                } else {
                    (8, 1)
                }
            }
            BlockType::Obsidian => (9, 1),
            BlockType::OakLog => {
                if face_idx == 4 || face_idx == 5 {
                    (10, 1)
                } else {
                    (11, 1)
                }
            }
            BlockType::CraftingTable => {
                if face_idx == 4 {
                    (12, 1)
                } else if face_idx == 5 {
                    (6, 0)
                } else {
                    (13, 1)
                }
            }
            BlockType::Furnace => {
                if face_idx == 0 {
                    (14, 1)
                } else {
                    (3, 0)
                }
            }
            BlockType::Chest => (15, 1),

            BlockType::TNT => {
                if face_idx == 4 {
                    (0, 2)
                } else if face_idx == 5 {
                    (1, 2)
                } else {
                    (2, 2)
                }
            }
            BlockType::Bookshelf => {
                if face_idx == 4 || face_idx == 5 {
                    (6, 0)
                } else {
                    (3, 2)
                }
            }
            BlockType::Torch => (4, 2),
            BlockType::Lava => (15, 2),
            BlockType::Air => (0, 0),
            // Trees & Biomes Additions
            BlockType::BirchLog => {
                if face_idx == 4 || face_idx == 5 {
                    (0, 12)
                } else {
                    (1, 12)
                }
            }
            BlockType::BirchPlanks => (2, 12),
            BlockType::BirchLeaves => (3, 12),
            BlockType::SpruceLog => {
                if face_idx == 4 || face_idx == 5 {
                    (4, 12)
                } else {
                    (5, 12)
                }
            }
            BlockType::SprucePlanks => (6, 12),
            BlockType::SpruceLeaves => (7, 12),
            BlockType::TallGrass => (8, 12),
            BlockType::Dandelion => (9, 12),
            BlockType::Poppy => (10, 12),
            BlockType::Cactus => (11, 12),
            BlockType::SugarCane => (12, 12),
            BlockType::Pumpkin => (13, 12),
            BlockType::Melon => (14, 12),
        }
    }
}

type FaceCorner = ([f32; 3], [f32; 2]);

// Face order: south, north, west, east, up, down.
const BLOCK_FACES: [([i32; 3], [FaceCorner; 4]); 6] = [
    (
        [0, 0, 1],
        [
            ([0.0, 0.0, 1.0], [0.0, 1.0]),
            ([1.0, 0.0, 1.0], [1.0, 1.0]),
            ([1.0, 1.0, 1.0], [1.0, 0.0]),
            ([0.0, 1.0, 1.0], [0.0, 0.0]),
        ],
    ),
    (
        [0, 0, -1],
        [
            ([1.0, 0.0, 0.0], [0.0, 1.0]),
            ([0.0, 0.0, 0.0], [1.0, 1.0]),
            ([0.0, 1.0, 0.0], [1.0, 0.0]),
            ([1.0, 1.0, 0.0], [0.0, 0.0]),
        ],
    ),
    (
        [-1, 0, 0],
        [
            ([0.0, 0.0, 0.0], [0.0, 1.0]),
            ([0.0, 0.0, 1.0], [1.0, 1.0]),
            ([0.0, 1.0, 1.0], [1.0, 0.0]),
            ([0.0, 1.0, 0.0], [0.0, 0.0]),
        ],
    ),
    (
        [1, 0, 0],
        [
            ([1.0, 0.0, 1.0], [0.0, 1.0]),
            ([1.0, 0.0, 0.0], [1.0, 1.0]),
            ([1.0, 1.0, 0.0], [1.0, 0.0]),
            ([1.0, 1.0, 1.0], [0.0, 0.0]),
        ],
    ),
    (
        [0, 1, 0],
        [
            ([0.0, 1.0, 1.0], [0.0, 1.0]),
            ([1.0, 1.0, 1.0], [1.0, 1.0]),
            ([1.0, 1.0, 0.0], [1.0, 0.0]),
            ([0.0, 1.0, 0.0], [0.0, 0.0]),
        ],
    ),
    (
        [0, -1, 0],
        [
            ([0.0, 0.0, 0.0], [0.0, 1.0]),
            ([1.0, 0.0, 0.0], [1.0, 1.0]),
            ([1.0, 0.0, 1.0], [1.0, 0.0]),
            ([0.0, 0.0, 1.0], [0.0, 0.0]),
        ],
    ),
];

fn ambient_occlusion_value(occluders: u8) -> f32 {
    match occluders.min(3) {
        0 => 1.0,
        1 => 0.75,
        2 => 0.5,
        _ => 0.25,
    }
}

fn ao_sample_positions(
    block_position: [i32; 3],
    normal: [i32; 3],
    corner: [f32; 3],
) -> [[i32; 3]; 3] {
    let tangent_axes = if normal[0] != 0 {
        [1, 2]
    } else if normal[1] != 0 {
        [0, 2]
    } else {
        [0, 1]
    };

    let mut side_u = [0; 3];
    let mut side_v = [0; 3];
    side_u[tangent_axes[0]] = if corner[tangent_axes[0]] == 0.0 {
        -1
    } else {
        1
    };
    side_v[tangent_axes[1]] = if corner[tangent_axes[1]] == 0.0 {
        -1
    } else {
        1
    };

    let outside = [
        block_position[0] + normal[0],
        block_position[1] + normal[1],
        block_position[2] + normal[2],
    ];
    [
        [
            outside[0] + side_u[0],
            outside[1] + side_u[1],
            outside[2] + side_u[2],
        ],
        [
            outside[0] + side_v[0],
            outside[1] + side_v[1],
            outside[2] + side_v[2],
        ],
        [
            outside[0] + side_u[0] + side_v[0],
            outside[1] + side_u[1] + side_v[1],
            outside[2] + side_u[2] + side_v[2],
        ],
    ]
}

fn ambient_occlusion_for_vertex<F>(
    block_position: [i32; 3],
    normal: [i32; 3],
    corner: [f32; 3],
    get_block_at: &F,
) -> f32
where
    F: Fn(i32, i32, i32) -> (BlockType, u8, u8, u8, bool),
{
    let occluders = ao_sample_positions(block_position, normal, corner)
        .iter()
        .filter(|position| {
            get_block_at(position[0], position[1], position[2])
                .0
                .is_ao_occluder()
        })
        .count() as u8;
    ambient_occlusion_value(occluders)
}

fn quad_indices_for_ao(ao: [f32; 4]) -> [u32; 6] {
    if ao[0] + ao[2] > ao[1] + ao[3] {
        [0, 1, 3, 1, 2, 3]
    } else {
        [0, 1, 2, 0, 2, 3]
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
                .try_into()
                .unwrap();
        let perlin = Perlin::new(12345); // Seed: 12345
        let caves_perlin = Perlin::new(54321);
        let caverns_perlin = Perlin::new(65432);
        let temp_perlin = Perlin::new(99999);
        let moist_perlin = Perlin::new(88888);
        let ocean_perlin = Perlin::new(77777);

        // Simple custom PRNG for ore distribution and bedrock blending
        let mut rng_seed = (chunk_x as u32).wrapping_mul(31) ^ (chunk_z as u32);
        let mut next_rand = |min: u8, max: u8| -> u8 {
            rng_seed = rng_seed.wrapping_mul(1103515245).wrapping_add(12345);
            let val = (rng_seed / 65536) % 32768;
            let diff = max - min;
            if diff == 0 {
                return min;
            }
            min + (val % diff as u32) as u8
        };

        for x in 0..CHUNK_WIDTH {
            for z in 0..CHUNK_DEPTH {
                let world_x = chunk_x * (CHUNK_WIDTH as i32) + x as i32;
                let world_z = chunk_z * (CHUNK_DEPTH as i32) + z as i32;

                let base_height = get_interpolated_height(
                    world_x,
                    world_z,
                    &perlin,
                    &temp_perlin,
                    &moist_perlin,
                    &ocean_perlin,
                );
                let biome =
                    Biome::get_biome(world_x, world_z, &temp_perlin, &moist_perlin, &ocean_perlin);

                let entrance_noise = perlin.get([world_x as f64 * 0.015, world_z as f64 * 0.015]);
                let is_entrance_zone = entrance_noise > 0.55 && base_height > 63;

                for y in 0..CHUNK_HEIGHT {
                    let world_y = y as i32;
                    let mut block;

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
                    } else if y < base_height.saturating_sub(4) {
                        block = BlockType::Stone;
                    } else if y < base_height {
                        block = match biome {
                            Biome::Desert => BlockType::Sandstone,
                            Biome::Ocean => BlockType::Sand,
                            _ => BlockType::Dirt,
                        };
                    } else if y == base_height {
                        block = match biome {
                            Biome::Desert => BlockType::Sand,
                            Biome::Ocean => BlockType::Sand,
                            Biome::Taiga => BlockType::Snow,
                            Biome::Mountains => {
                                if y > 90 {
                                    BlockType::Snow
                                } else {
                                    BlockType::Stone
                                }
                            }
                            _ => BlockType::Grass,
                        };
                    } else {
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
                            let cave_val = caves_perlin.get([
                                world_x as f64 * 0.05,
                                world_y as f64 * 0.08,
                                world_z as f64 * 0.05,
                            ]);
                            let cavern_val = caverns_perlin.get([
                                world_x as f64 * 0.01,
                                world_y as f64 * 0.01,
                                world_z as f64 * 0.01,
                            ]);
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
            if min >= max {
                return min;
            }
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

                if start_y >= CHUNK_HEIGHT {
                    continue;
                }

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
                        if nx >= 0
                            && nx < CHUNK_WIDTH as i32
                            && nz >= 0
                            && nz < CHUNK_DEPTH as i32
                            && ny > 4
                            && ny < CHUNK_HEIGHT as i32
                        {
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

        // Trees Pass:
        for dx in -1..=1 {
            for dz in -1..=1 {
                let nx = chunk_x + dx;
                let nz = chunk_z + dz;

                // Seed PRNG deterministically for the neighbor chunk
                let mut n_seed = (nx as u32).wrapping_mul(31) ^ (nz as u32);
                let mut n_rand = |min: u8, max: u8| -> u8 {
                    n_seed = n_seed.wrapping_mul(1103515245).wrapping_add(12345);
                    let val = (n_seed / 65536) % 32768;
                    let diff = max - min;
                    if diff == 0 {
                        return min;
                    }
                    min + (val % diff as u32) as u8
                };

                // Try 4 tree candidate spots per chunk
                for _ in 0..4 {
                    let tx = n_rand(0, 15) as i32;
                    let tz = n_rand(0, 15) as i32;
                    let n_world_x = nx * 16 + tx;
                    let n_world_z = nz * 16 + tz;

                    let n_biome = Biome::get_biome(
                        n_world_x,
                        n_world_z,
                        &temp_perlin,
                        &moist_perlin,
                        &ocean_perlin,
                    );
                    let tree_prob = match n_biome {
                        Biome::Plains => 5,
                        Biome::Forest => 60,
                        Biome::Taiga => 40,
                        Biome::Swamp => 20,
                        Biome::Mountains => 2,
                        _ => 0,
                    };

                    if n_rand(0, 100) < tree_prob {
                        let n_height = get_interpolated_height(
                            n_world_x,
                            n_world_z,
                            &perlin,
                            &temp_perlin,
                            &moist_perlin,
                            &ocean_perlin,
                        ) as i32;
                        if n_height > 5 && n_height < CHUNK_HEIGHT as i32 - 12 {
                            // Project to current chunk local coordinates
                            let local_x = n_world_x - (chunk_x * 16);
                            let local_z = n_world_z - (chunk_z * 16);

                            let tree_height = n_rand(4, 7) as i32;
                            match n_biome {
                                Biome::Taiga => place_spruce_tree(
                                    &mut blocks,
                                    local_x,
                                    local_z,
                                    n_height + 1,
                                    tree_height + 2,
                                ),
                                Biome::Forest => {
                                    if n_rand(0, 10) < 4 {
                                        place_birch_tree(
                                            &mut blocks,
                                            local_x,
                                            local_z,
                                            n_height + 1,
                                            tree_height + 1,
                                        );
                                    } else {
                                        place_oak_tree(
                                            &mut blocks,
                                            local_x,
                                            local_z,
                                            n_height + 1,
                                            tree_height,
                                        );
                                    }
                                }
                                _ => place_oak_tree(
                                    &mut blocks,
                                    local_x,
                                    local_z,
                                    n_height + 1,
                                    tree_height,
                                ),
                            }
                        }
                    }
                }
            }
        }

        // Plant & Decoration Pass (only for columns inside current chunk):
        for x in 0..CHUNK_WIDTH {
            for z in 0..CHUNK_DEPTH {
                let world_x = chunk_x * 16 + x as i32;
                let world_z = chunk_z * 16 + z as i32;
                let biome =
                    Biome::get_biome(world_x, world_z, &temp_perlin, &moist_perlin, &ocean_perlin);

                // Seed PRNG deterministically for columns
                let mut c_seed = (world_x as u32).wrapping_mul(17) ^ (world_z as u32);
                let mut c_rand = |min: u32, max: u32| -> u32 {
                    c_seed = c_seed.wrapping_mul(1103515245).wrapping_add(12345);
                    let diff = max - min;
                    if diff == 0 {
                        return min;
                    }
                    min + ((c_seed / 65536) % 32768) % diff
                };

                // Find surface block
                let mut surface_y = 0;
                for y in (0..CHUNK_HEIGHT).rev() {
                    if blocks[x][y][z] != BlockType::Air && blocks[x][y][z] != BlockType::Water {
                        surface_y = y;
                        break;
                    }
                }

                let surface_block = blocks[x][surface_y][z];
                if surface_block == BlockType::Grass {
                    let r = c_rand(0, 100);
                    if r < 10 {
                        // Tall grass
                        if surface_y + 1 < CHUNK_HEIGHT {
                            blocks[x][surface_y + 1][z] = BlockType::TallGrass;
                        }
                    } else if r < 12 {
                        // Dandelion
                        if surface_y + 1 < CHUNK_HEIGHT {
                            blocks[x][surface_y + 1][z] = BlockType::Dandelion;
                        }
                    } else if r < 13 {
                        // Poppy
                        if surface_y + 1 < CHUNK_HEIGHT {
                            blocks[x][surface_y + 1][z] = BlockType::Poppy;
                        }
                    } else if r < 14 && (biome == Biome::Plains || biome == Biome::Forest) {
                        // Pumpkin / Melon
                        if surface_y + 1 < CHUNK_HEIGHT {
                            blocks[x][surface_y + 1][z] = if c_rand(0, 2) == 0 {
                                BlockType::Pumpkin
                            } else {
                                BlockType::Melon
                            };
                        }
                    }
                } else if surface_block == BlockType::Sand && biome == Biome::Desert {
                    if c_rand(0, 100) < 2 {
                        // Cactus
                        let cactus_height = c_rand(1, 4) as usize;
                        for dy in 1..=cactus_height {
                            if surface_y + dy < CHUNK_HEIGHT {
                                blocks[x][surface_y + dy][z] = BlockType::Cactus;
                            }
                        }
                    }
                }

                // Sugar Cane (must be next to water)
                if (surface_block == BlockType::Grass
                    || surface_block == BlockType::Dirt
                    || surface_block == BlockType::Sand)
                    && surface_y > 0
                {
                    let mut near_water = false;
                    for dx in -1..=1 {
                        for dz in -1..=1 {
                            if dx == 0 && dz == 0 {
                                continue;
                            }
                            let nx = x as i32 + dx;
                            let nz = z as i32 + dz;
                            if nx >= 0
                                && nx < CHUNK_WIDTH as i32
                                && nz >= 0
                                && nz < CHUNK_DEPTH as i32
                            {
                                let b = blocks[nx as usize][surface_y][nz as usize];
                                let b_below = blocks[nx as usize][surface_y - 1][nz as usize];
                                if b == BlockType::Water || b_below == BlockType::Water {
                                    near_water = true;
                                    break;
                                }
                            }
                        }
                        if near_water {
                            break;
                        }
                    }
                    if near_water && c_rand(0, 100) < 10 {
                        let cane_height = c_rand(2, 5) as usize;
                        for dy in 1..=cane_height {
                            if surface_y + dy < CHUNK_HEIGHT {
                                blocks[x][surface_y + dy][z] = BlockType::SugarCane;
                            }
                        }
                    }
                }
            }
        }

        let mut sky_light: Box<[[[u8; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH]> =
            vec![[[0u8; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH]
                .try_into()
                .unwrap();
        let mut block_light: Box<[[[u8; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH]> =
            vec![[[0u8; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH]
                .try_into()
                .unwrap();

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
            vec![[0u16; CHUNK_DEPTH]; CHUNK_WIDTH].try_into().unwrap();
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
                .try_into()
                .unwrap();

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
        if x < 0
            || x >= CHUNK_WIDTH as i32
            || y < 0
            || y >= CHUNK_HEIGHT as i32
            || z < 0
            || z >= CHUNK_DEPTH as i32
        {
            return BlockType::Air; // 超出範圍視為空氣
        }
        self.blocks[x as usize][y as usize][z as usize]
    }

    // 生成用於渲染的頂點和索引，區分為不透明與半透明網格
    pub fn generate_mesh<F>(
        &self,
        get_block_at: F,
    ) -> (Vec<Vertex>, Vec<u32>, Vec<Vertex>, Vec<u32>)
    where
        F: Fn(i32, i32, i32) -> (BlockType, u8, u8, u8, bool),
    {
        let mut opaque_vertices = Vec::new();
        let mut opaque_indices = Vec::new();
        let mut trans_vertices = Vec::new();
        let mut trans_indices = Vec::new();

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

                    for (face_idx, (normal, corner_data)) in BLOCK_FACES.iter().enumerate() {
                        let nx = world_x + normal[0];
                        let ny = world_y + normal[1];
                        let nz = world_z + normal[2];

                        let (
                            neighbor,
                            neighbor_sky,
                            neighbor_block,
                            neighbor_level,
                            neighbor_falling,
                        ) = get_block_at(nx, ny, nz);
                        let neighbor_props = neighbor.properties();

                        let is_fluid = block == BlockType::Water || block == BlockType::Lava;
                        let level = self.fluid_levels[x][y][z] & 0x07;
                        let falling = (self.fluid_levels[x][y][z] & 0x08) != 0;

                        // Face Culling: 只有鄰居非 Opaque 才渲染（且排除相同流體相鄰）
                        let should_render = if neighbor == BlockType::Air {
                            true
                        } else if neighbor_props.render_type != RenderType::Opaque {
                            if is_fluid && neighbor == block {
                                if face_idx == 4 {
                                    false
                                } else if face_idx == 5 {
                                    false
                                } else {
                                    if neighbor_falling {
                                        false
                                    } else if falling {
                                        true
                                    } else {
                                        neighbor_level > level
                                    }
                                }
                            } else {
                                true
                            }
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
                            let light_val = if block == BlockType::Lava {
                                15.0 * 16.0 + 15.0 + multiplier_code * 256.0
                            } else {
                                (neighbor_sky as f32)
                                    + (neighbor_block as f32) * 16.0
                                    + multiplier_code * 256.0
                            };

                            let h = if is_fluid {
                                if falling {
                                    1.0
                                } else {
                                    (8 - level) as f32 / 8.0 * 0.9
                                }
                            } else {
                                1.0
                            };

                            let mut ao = [1.0; 4];
                            for (corner_idx, (offset, _)) in corner_data.iter().enumerate() {
                                ao[corner_idx] = ambient_occlusion_for_vertex(
                                    [world_x, world_y, world_z],
                                    *normal,
                                    *offset,
                                    &get_block_at,
                                );
                            }

                            for (corner_idx, (offset, uv)) in corner_data.iter().enumerate() {
                                // 256x256 atlas -> 16 columns of 16x16 pixel blocks
                                // Apply half-pixel inset adjustment to prevent Nearest-neighbor coordinate bleeding
                                let u_adj = if uv[0] == 0.0 { 0.005 } else { 0.995 };
                                let v_adj = if uv[1] == 0.0 { 0.005 } else { 0.995 };
                                let u = (u_adj + tx_col as f32) * 0.0625;
                                let v = (v_adj + tx_row as f32) * 0.0625;

                                let mut vy = world_y as f32 + offset[1];
                                if is_fluid && offset[1] > 0.0 {
                                    vy = world_y as f32 + h;
                                }

                                v_list.push(Vertex {
                                    position: [
                                        world_x as f32 + offset[0],
                                        vy,
                                        world_z as f32 + offset[2],
                                    ],
                                    tex_coords: [u, v],
                                    light_level: light_val,
                                    ao: ao[corner_idx],
                                });
                            }

                            i_list.extend(
                                quad_indices_for_ao(ao)
                                    .iter()
                                    .map(|index| start_idx + index),
                            );
                        }
                    }
                }
            }
        }

        (
            opaque_vertices,
            opaque_indices,
            trans_vertices,
            trans_indices,
        )
    }
}

use crate::inventory::{ToolMaterial, ToolType};

impl BlockType {
    pub fn preferred_tool(self) -> ToolType {
        match self {
            BlockType::Grass
            | BlockType::Dirt
            | BlockType::Sand
            | BlockType::Gravel
            | BlockType::Snow
            | BlockType::Clay
            | BlockType::Sandstone => ToolType::Shovel,
            BlockType::Stone
            | BlockType::Cobblestone
            | BlockType::CoalOre
            | BlockType::IronOre
            | BlockType::GoldOre
            | BlockType::DiamondOre
            | BlockType::RedstoneOre
            | BlockType::StoneBrick
            | BlockType::Obsidian
            | BlockType::Furnace => ToolType::Pickaxe,
            BlockType::OakLog
            | BlockType::OakPlanks
            | BlockType::BirchLog
            | BlockType::BirchPlanks
            | BlockType::SpruceLog
            | BlockType::SprucePlanks
            | BlockType::CraftingTable
            | BlockType::Chest
            | BlockType::Bookshelf
            | BlockType::Pumpkin
            | BlockType::Melon => ToolType::Axe,
            _ => ToolType::None,
        }
    }

    pub fn min_harvest_material(self) -> Option<ToolMaterial> {
        match self {
            BlockType::Stone
            | BlockType::Cobblestone
            | BlockType::CoalOre
            | BlockType::Furnace
            | BlockType::StoneBrick
            | BlockType::Sandstone => Some(ToolMaterial::Wood), // Stone tier tools or above
            BlockType::IronOre => Some(ToolMaterial::Stone),
            BlockType::GoldOre | BlockType::RedstoneOre | BlockType::DiamondOre => {
                Some(ToolMaterial::Iron)
            }
            BlockType::Obsidian => Some(ToolMaterial::Diamond),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn triangle_normal(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> [f32; 3] {
        let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        [
            ab[1] * ac[2] - ab[2] * ac[1],
            ab[2] * ac[0] - ab[0] * ac[2],
            ab[0] * ac[1] - ab[1] * ac[0],
        ]
    }

    #[test]
    fn ambient_occlusion_levels_match_occluder_counts() {
        assert_eq!(ambient_occlusion_value(0), 1.0);
        assert_eq!(ambient_occlusion_value(1), 0.75);
        assert_eq!(ambient_occlusion_value(2), 0.5);
        assert_eq!(ambient_occlusion_value(3), 0.25);
    }

    #[test]
    fn only_solid_opaque_blocks_cast_ambient_occlusion() {
        assert!(BlockType::Stone.is_ao_occluder());
        assert!(BlockType::Grass.is_ao_occluder());
        for block in [
            BlockType::Air,
            BlockType::Water,
            BlockType::Lava,
            BlockType::Glass,
            BlockType::OakLeaves,
            BlockType::Torch,
            BlockType::TallGrass,
            BlockType::Cactus,
        ] {
            assert!(!block.is_ao_occluder(), "{block:?} should not cast AO");
        }
    }

    #[test]
    fn ao_samples_follow_every_face_and_corner_direction() {
        let block = [10, 20, 30];
        for (normal, corners) in BLOCK_FACES {
            let normal_axis = (0..3).find(|&axis| normal[axis] != 0).unwrap();
            let tangent_axes: Vec<usize> = (0..3).filter(|&axis| normal[axis] == 0).collect();
            let outside = [
                block[0] + normal[0],
                block[1] + normal[1],
                block[2] + normal[2],
            ];

            for (corner, _) in corners {
                let samples = ao_sample_positions(block, normal, corner);
                let sign_u = if corner[tangent_axes[0]] == 0.0 {
                    -1
                } else {
                    1
                };
                let sign_v = if corner[tangent_axes[1]] == 0.0 {
                    -1
                } else {
                    1
                };

                assert!(samples
                    .iter()
                    .all(|sample| sample[normal_axis] == outside[normal_axis]));
                assert_eq!(
                    samples[0][tangent_axes[0]],
                    outside[tangent_axes[0]] + sign_u
                );
                assert_eq!(samples[0][tangent_axes[1]], outside[tangent_axes[1]]);
                assert_eq!(samples[1][tangent_axes[0]], outside[tangent_axes[0]]);
                assert_eq!(
                    samples[1][tangent_axes[1]],
                    outside[tangent_axes[1]] + sign_v
                );
                assert_eq!(
                    samples[2][tangent_axes[0]],
                    outside[tangent_axes[0]] + sign_u
                );
                assert_eq!(
                    samples[2][tangent_axes[1]],
                    outside[tangent_axes[1]] + sign_v
                );
            }
        }
    }

    #[test]
    fn ao_diagonal_selection_preserves_face_winding() {
        let default_indices = quad_indices_for_ao([1.0, 0.75, 0.5, 0.75]);
        let flipped_indices = quad_indices_for_ao([1.0, 0.25, 1.0, 0.25]);
        let tie_indices = quad_indices_for_ao([1.0, 0.5, 0.5, 1.0]);
        assert_eq!(default_indices, [0, 1, 2, 0, 2, 3]);
        assert_eq!(flipped_indices, [0, 1, 3, 1, 2, 3]);
        assert_eq!(tie_indices, [0, 1, 2, 0, 2, 3]);

        for (normal, corners) in BLOCK_FACES {
            for indices in [default_indices, flipped_indices] {
                for triangle in indices.chunks_exact(3) {
                    let face_normal = triangle_normal(
                        corners[triangle[0] as usize].0,
                        corners[triangle[1] as usize].0,
                        corners[triangle[2] as usize].0,
                    );
                    let dot = face_normal[0] * normal[0] as f32
                        + face_normal[1] * normal[1] as f32
                        + face_normal[2] * normal[2] as f32;
                    assert!(dot > 0.0, "triangle winding changed for face {normal:?}");
                }
            }
        }
    }

    #[test]
    fn generated_mesh_writes_ao_for_isolated_and_occluded_vertices() {
        let mut chunk = Chunk::new(0, 0);
        for x in 0..CHUNK_WIDTH {
            for y in 0..CHUNK_HEIGHT {
                for z in 0..CHUNK_DEPTH {
                    chunk.blocks[x][y][z] = BlockType::Air;
                }
            }
            for z in 0..CHUNK_DEPTH {
                chunk.heightmap[x][z] = 0;
            }
        }
        chunk.blocks[8][1][8] = BlockType::Stone;
        chunk.heightmap[8][8] = 1;

        let empty_lookup = |_: i32, _: i32, _: i32| (BlockType::Air, 15, 0, 0, false);
        let (vertices, indices, _, _) = chunk.generate_mesh(empty_lookup);
        assert_eq!(vertices.len(), 24);
        assert_eq!(indices.len(), 36);
        assert!(vertices.iter().all(|vertex| vertex.ao == 1.0));

        let occluders = HashSet::from([(7, 2, 8), (8, 2, 9)]);
        let lookup = |x: i32, y: i32, z: i32| {
            let block = if occluders.contains(&(x, y, z)) {
                BlockType::Stone
            } else {
                BlockType::Air
            };
            (block, 15, 0, 0, false)
        };
        let (vertices, _, _, _) = chunk.generate_mesh(lookup);
        assert_eq!(vertices[16].position, [8.0, 2.0, 9.0]);
        assert_eq!(vertices[16].ao, 0.5);
    }

    #[test]
    fn test_block_harvest_properties() {
        assert_eq!(BlockType::Obsidian.preferred_tool(), ToolType::Pickaxe);
        assert_eq!(
            BlockType::Obsidian.min_harvest_material(),
            Some(ToolMaterial::Diamond)
        );
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
                            if nx >= 0
                                && nx < CHUNK_WIDTH as i32
                                && nz >= 0
                                && nz < CHUNK_DEPTH as i32
                                && ny >= 0
                                && ny < CHUNK_HEIGHT as i32
                            {
                                if chunk.blocks[nx as usize][ny as usize][nz as usize]
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
                    if chunk.blocks[x][base_height][z] == BlockType::Air {
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
        chunk.fluid_levels[0][10][0] = 5 | 0x08; // level 5, falling = true
        assert_eq!(chunk.fluid_levels[0][10][0] & 0x07, 5);
        assert_eq!((chunk.fluid_levels[0][10][0] & 0x08) != 0, true);
    }

    #[test]
    fn test_biome_distribution() {
        let temp_perlin = Perlin::new(99999);
        let moist_perlin = Perlin::new(88888);
        let ocean_perlin = Perlin::new(77777);

        // Verify that biomes evaluate correctly and don't panic
        let biome_land = Biome::get_biome(1000, 1000, &temp_perlin, &moist_perlin, &ocean_perlin);
        println!("Sample Biome at (1000, 1000): {:?}", biome_land);
    }

    #[test]
    fn test_tree_placement_bounds() {
        let mut blocks = vec![[[BlockType::Air; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH]
            .try_into()
            .unwrap();
        // Oak tree at local coordinates: should not panic when inside or touching edges
        place_oak_tree(&mut blocks, 8, 8, 64, 5);
        assert_eq!(blocks[8][64][8], BlockType::OakLog);
        assert_eq!(blocks[8][65][8], BlockType::OakLog);
        assert_eq!(blocks[8][68][8], BlockType::OakLog);

        // Spruce tree at border
        place_spruce_tree(&mut blocks, 0, 0, 64, 7);
        assert_eq!(blocks[0][64][0], BlockType::SpruceLog);
    }
}
