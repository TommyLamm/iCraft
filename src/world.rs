use crate::chunk_render::{ChunkLodMeshData, ChunkMeshBundle, TerrainVertex};
use crate::redstone::Direction;
use noise::{NoiseFn, Perlin};
use std::mem::{size_of, size_of_val};

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
    EnchantingTable = 45,
    BrewingStand = 46,
    Anvil = 47,
    RedstoneWire = 48,
    RedstoneTorch = 49,
    RedstoneTorchOff = 50,
    Repeater = 51,
    RepeaterPowered = 52,
    Comparator = 53,
    ComparatorPowered = 54,
    StoneButton = 55,
    StoneButtonPressed = 56,
    Lever = 57,
    LeverOn = 58,
    PressurePlate = 59,
    PressurePlatePowered = 60,
    Piston = 61,
    PistonExtended = 62,
    StickyPiston = 63,
    StickyPistonExtended = 64,
    RedstoneLamp = 65,
    RedstoneLampLit = 66,
    OakDoor = 67,
    OakDoorOpen = 68,
    OakTrapdoor = 69,
    OakTrapdoorOpen = 70,
    Dispenser = 71,
    Dropper = 72,
    NoteBlock = 73,
    Fire = 74,
    SnowLayer = 75,
    Netherrack = 76,
    SoulSand = 77,
    Glowstone = 78,
    NetherPortal = 79,
    EndStone = 80,
    EndPortalFrame = 81,
    EndPortalFrameFilled = 82,
    EndPortal = 83,
    Purpur = 84,
    DragonEgg = 85,
    WitherSkeletonSkull = 86,
    NetherBrick = 87,
    EndCityChest = 88,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RenderType {
    Opaque,
    Cutout,
    Translucent,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BlockSupportStatus {
    Supported,
    Unsupported,
    Unknown,
}

pub struct BlockProperties {
    pub name: &'static str,
    pub hardness: f32,
    pub render_type: RenderType,
    pub is_solid: bool,
    pub is_passable: bool,
    pub light_emission: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockState {
    pub facing: Direction,
    pub is_top: bool,
    pub is_right_hinge: bool,
    pub is_open: bool,
}

impl Default for BlockState {
    fn default() -> Self {
        Self {
            facing: Direction::North,
            is_top: false,
            is_right_hinge: false,
            is_open: false,
        }
    }
}

impl BlockState {
    pub fn encode(self) -> u8 {
        let facing_bits = match self.facing {
            Direction::North => 0b00,
            Direction::South => 0b01,
            Direction::West => 0b10,
            Direction::East => 0b11,
        };
        let half_bit = if self.is_top { 1 << 2 } else { 0 };
        let hinge_bit = if self.is_right_hinge { 1 << 3 } else { 0 };
        let open_bit = if self.is_open { 1 << 4 } else { 0 };
        facing_bits | half_bit | hinge_bit | open_bit
    }

    pub fn decode(val: u8) -> Self {
        let facing = match val & 0b11 {
            0 => Direction::North,
            1 => Direction::South,
            2 => Direction::West,
            3 => Direction::East,
            _ => unreachable!(),
        };
        let is_top = (val & (1 << 2)) != 0;
        let is_right_hinge = (val & (1 << 3)) != 0;
        let is_open = (val & (1 << 4)) != 0;
        Self {
            facing,
            is_top,
            is_right_hinge,
            is_open,
        }
    }

    pub fn for_door_placement(
        chunk_manager: &crate::chunk_manager::ChunkManager,
        x: i32,
        y: i32,
        z: i32,
        yaw: f32,
    ) -> (Self, Self) {
        let facing = Direction::from_yaw(yaw);
        let (left_dx, left_dz) = match facing {
            Direction::North => (-1, 0),
            Direction::South => (1, 0),
            Direction::West => (0, 1),
            Direction::East => (0, -1),
        };
        let (right_dx, right_dz) = match facing {
            Direction::North => (1, 0),
            Direction::South => (-1, 0),
            Direction::West => (0, -1),
            Direction::East => (0, 1),
        };

        let left_block = chunk_manager.get_block(x + left_dx, y, z + left_dz);
        let right_block = chunk_manager.get_block(x + right_dx, y, z + right_dz);

        let is_right_hinge = left_block.properties().is_solid && !right_block.properties().is_solid;

        let bottom = Self {
            facing,
            is_top: false,
            is_right_hinge,
            is_open: false,
        };
        let top = Self {
            facing,
            is_top: true,
            is_right_hinge,
            is_open: false,
        };
        (bottom, top)
    }

    pub fn for_trapdoor_placement(yaw: f32) -> Self {
        let facing = Direction::from_yaw(yaw);
        Self {
            facing,
            is_top: false,
            is_right_hinge: false,
            is_open: false,
        }
    }
}

impl BlockType {
    pub fn from_u8(val: u8) -> Self {
        if val <= BlockType::EndCityChest as u8 {
            unsafe { std::mem::transmute(val) }
        } else {
            BlockType::Air
        }
    }

    /// Wire encoding for multiplayer block sync.
    ///
    /// `BlockType` is `#[repr(u8)]` with explicit, stable discriminants, so the
    /// numeric value is part of the network protocol contract. Adding a new
    /// variant is allowed (append a new value), but never reuse an existing
    /// wire value for a different block: older clients would misdecode it.
    pub fn to_wire(&self) -> u32 {
        *self as u32
    }

    /// Inverse of `to_wire`. Returns `None` for values that do not map to a
    /// known variant so unknown (newer) blocks are dropped gracefully instead
    /// of corrupting world state.
    pub fn from_wire(val: u32) -> Option<Self> {
        if val <= BlockType::EndCityChest as u32 {
            // SAFETY: `BlockType` is `#[repr(u8)]`, so every value in
            // `0..=EndCityChest` is a valid discriminant.
            Some(unsafe { std::mem::transmute(val as u8) })
        } else {
            None
        }
    }

    pub fn is_cross_model(self) -> bool {
        matches!(
            self,
            BlockType::Dandelion | BlockType::Poppy | BlockType::TallGrass | BlockType::SugarCane
        )
    }

    pub fn can_stay_on(self, below: BlockType) -> bool {
        match self {
            BlockType::Dandelion | BlockType::Poppy | BlockType::TallGrass => {
                matches!(below, BlockType::Grass | BlockType::Dirt)
            }
            BlockType::SugarCane => {
                matches!(
                    below,
                    BlockType::Grass | BlockType::Dirt | BlockType::Sand | BlockType::SugarCane
                )
            }
            BlockType::Cactus => {
                matches!(below, BlockType::Sand | BlockType::Cactus)
            }
            BlockType::SnowLayer => below.properties().is_solid,
            BlockType::Torch
            | BlockType::RedstoneTorch
            | BlockType::RedstoneTorchOff
            | BlockType::RedstoneWire
            | BlockType::Repeater
            | BlockType::RepeaterPowered
            | BlockType::Comparator
            | BlockType::ComparatorPowered
            | BlockType::PressurePlate
            | BlockType::PressurePlatePowered => below.properties().is_solid,
            _ => true,
        }
    }

    /// Validates support using loaded world context. `None` means the queried
    /// position belongs to a chunk whose data is not currently available.
    ///
    /// Existing blocks are only removed for `Unsupported`; `Unknown` preserves
    /// them until the missing neighbor loads. New player placements require
    /// `Supported`, so they never assume an unloaded neighbor contains water or
    /// empty space.
    pub fn support_status_at<F>(
        self,
        position: (i32, i32, i32),
        mut get_loaded_block: F,
    ) -> BlockSupportStatus
    where
        F: FnMut(i32, i32, i32) -> Option<BlockType>,
    {
        let (x, y, z) = position;

        match self {
            BlockType::SugarCane => {
                if y <= 0 {
                    return BlockSupportStatus::Unsupported;
                }
                let Some(below) = get_loaded_block(x, y - 1, z) else {
                    return BlockSupportStatus::Unknown;
                };
                if below == BlockType::SugarCane {
                    return BlockSupportStatus::Supported;
                }
                if !matches!(below, BlockType::Grass | BlockType::Dirt | BlockType::Sand) {
                    return BlockSupportStatus::Unsupported;
                }

                let mut has_unknown_neighbor = false;
                for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    match get_loaded_block(x + dx, y - 1, z + dz) {
                        Some(BlockType::Water) => return BlockSupportStatus::Supported,
                        Some(_) => {}
                        None => has_unknown_neighbor = true,
                    }
                }
                if has_unknown_neighbor {
                    BlockSupportStatus::Unknown
                } else {
                    BlockSupportStatus::Unsupported
                }
            }
            BlockType::Cactus => {
                if y <= 0 {
                    return BlockSupportStatus::Unsupported;
                }
                let Some(below) = get_loaded_block(x, y - 1, z) else {
                    return BlockSupportStatus::Unknown;
                };
                if !matches!(below, BlockType::Sand | BlockType::Cactus) {
                    return BlockSupportStatus::Unsupported;
                }

                let mut has_unknown_neighbor = false;
                for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    match get_loaded_block(x + dx, y, z + dz) {
                        Some(block) if block.properties().is_solid || block == BlockType::Lava => {
                            return BlockSupportStatus::Unsupported;
                        }
                        Some(_) => {}
                        None => has_unknown_neighbor = true,
                    }
                }
                if has_unknown_neighbor {
                    BlockSupportStatus::Unknown
                } else {
                    BlockSupportStatus::Supported
                }
            }
            BlockType::OakDoor => {
                if y <= 0 {
                    BlockSupportStatus::Unsupported
                } else {
                    match get_loaded_block(x, y - 1, z) {
                        Some(below) if below == BlockType::OakDoor || self.can_stay_on(below) => {
                            BlockSupportStatus::Supported
                        }
                        Some(_) => BlockSupportStatus::Unsupported,
                        None => BlockSupportStatus::Unknown,
                    }
                }
            }
            BlockType::Dandelion
            | BlockType::Poppy
            | BlockType::TallGrass
            | BlockType::SnowLayer
            | BlockType::Torch
            | BlockType::RedstoneTorch
            | BlockType::RedstoneTorchOff
            | BlockType::RedstoneWire
            | BlockType::Repeater
            | BlockType::RepeaterPowered
            | BlockType::Comparator
            | BlockType::ComparatorPowered
            | BlockType::PressurePlate
            | BlockType::PressurePlatePowered => {
                if y <= 0 {
                    BlockSupportStatus::Unsupported
                } else {
                    match get_loaded_block(x, y - 1, z) {
                        Some(below) if self.can_stay_on(below) => BlockSupportStatus::Supported,
                        Some(_) => BlockSupportStatus::Unsupported,
                        None => BlockSupportStatus::Unknown,
                    }
                }
            }
            _ => BlockSupportStatus::Supported,
        }
    }

    pub fn sound_material(self) -> Option<crate::audio::SoundMaterial> {
        match self {
            BlockType::Air
            | BlockType::Water
            | BlockType::Lava
            | BlockType::Fire
            | BlockType::NetherPortal
            | BlockType::EndPortal => None,
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
            | BlockType::EnchantingTable
            | BlockType::BrewingStand
            | BlockType::Pumpkin
            | BlockType::Melon => Some(crate::audio::SoundMaterial::Wood),
            BlockType::Sand | BlockType::Clay | BlockType::SoulSand => {
                Some(crate::audio::SoundMaterial::Sand)
            }
            BlockType::Gravel | BlockType::Cactus => Some(crate::audio::SoundMaterial::Gravel),
            BlockType::Snow | BlockType::SnowLayer => Some(crate::audio::SoundMaterial::Snow),
            BlockType::Ice => Some(crate::audio::SoundMaterial::Ice),
            BlockType::Glass => Some(crate::audio::SoundMaterial::Glass),
            BlockType::Anvil => Some(crate::audio::SoundMaterial::Stone),
            BlockType::OakDoor
            | BlockType::OakDoorOpen
            | BlockType::OakTrapdoor
            | BlockType::OakTrapdoorOpen
            | BlockType::NoteBlock => Some(crate::audio::SoundMaterial::Wood),
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
            BlockType::EnchantingTable => BlockProperties {
                name: "Enchanting Table",
                hardness: 5.0,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 7,
            },
            BlockType::BrewingStand => BlockProperties {
                name: "Brewing Stand",
                hardness: 0.5,
                render_type: RenderType::Cutout,
                is_solid: true,
                is_passable: false,
                light_emission: 1,
            },
            BlockType::Anvil => BlockProperties {
                name: "Anvil",
                hardness: 5.0,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::RedstoneWire => BlockProperties {
                name: "Redstone Wire",
                hardness: 0.0,
                render_type: RenderType::Cutout,
                is_solid: false,
                is_passable: true,
                light_emission: 0,
            },
            BlockType::RedstoneTorch | BlockType::RedstoneTorchOff => BlockProperties {
                name: "Redstone Torch",
                hardness: 0.0,
                render_type: RenderType::Cutout,
                is_solid: false,
                is_passable: true,
                light_emission: if self == BlockType::RedstoneTorch {
                    7
                } else {
                    0
                },
            },
            BlockType::Repeater
            | BlockType::RepeaterPowered
            | BlockType::Comparator
            | BlockType::ComparatorPowered
            | BlockType::StoneButton
            | BlockType::StoneButtonPressed
            | BlockType::Lever
            | BlockType::LeverOn
            | BlockType::PressurePlate
            | BlockType::PressurePlatePowered => BlockProperties {
                name: match self {
                    BlockType::Repeater | BlockType::RepeaterPowered => "Redstone Repeater",
                    BlockType::Comparator | BlockType::ComparatorPowered => "Redstone Comparator",
                    BlockType::StoneButton | BlockType::StoneButtonPressed => "Stone Button",
                    BlockType::Lever | BlockType::LeverOn => "Lever",
                    _ => "Stone Pressure Plate",
                },
                hardness: 0.5,
                render_type: RenderType::Cutout,
                is_solid: false,
                is_passable: true,
                light_emission: 0,
            },
            BlockType::Piston
            | BlockType::PistonExtended
            | BlockType::StickyPiston
            | BlockType::StickyPistonExtended => BlockProperties {
                name: if matches!(
                    self,
                    BlockType::StickyPiston | BlockType::StickyPistonExtended
                ) {
                    "Sticky Piston"
                } else {
                    "Piston"
                },
                hardness: 1.5,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::RedstoneLamp | BlockType::RedstoneLampLit => BlockProperties {
                name: "Redstone Lamp",
                hardness: 0.3,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: if self == BlockType::RedstoneLampLit {
                    15
                } else {
                    0
                },
            },
            BlockType::OakDoor | BlockType::OakDoorOpen => BlockProperties {
                name: "Oak Door",
                hardness: 3.0,
                render_type: RenderType::Cutout,
                is_solid: self == BlockType::OakDoor,
                is_passable: self == BlockType::OakDoorOpen,
                light_emission: 0,
            },
            BlockType::OakTrapdoor | BlockType::OakTrapdoorOpen => BlockProperties {
                name: "Oak Trapdoor",
                hardness: 3.0,
                render_type: RenderType::Cutout,
                is_solid: self == BlockType::OakTrapdoor,
                is_passable: self == BlockType::OakTrapdoorOpen,
                light_emission: 0,
            },
            BlockType::Dispenser | BlockType::Dropper => BlockProperties {
                name: if self == BlockType::Dispenser {
                    "Dispenser"
                } else {
                    "Dropper"
                },
                hardness: 3.5,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::NoteBlock => BlockProperties {
                name: "Note Block",
                hardness: 0.8,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::Fire => BlockProperties {
                name: "Fire",
                hardness: 0.0,
                render_type: RenderType::Cutout,
                is_solid: false,
                is_passable: true,
                light_emission: 15,
            },
            BlockType::SnowLayer => BlockProperties {
                name: "Snow Layer",
                hardness: 0.1,
                render_type: RenderType::Cutout,
                is_solid: false,
                is_passable: true,
                light_emission: 0,
            },
            BlockType::Netherrack => BlockProperties {
                name: "Netherrack",
                hardness: 0.4,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::SoulSand => BlockProperties {
                name: "Soul Sand",
                hardness: 0.5,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::Glowstone => BlockProperties {
                name: "Glowstone",
                hardness: 0.3,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 15,
            },
            BlockType::NetherPortal => BlockProperties {
                name: "Nether Portal",
                hardness: -1.0,
                render_type: RenderType::Translucent,
                is_solid: false,
                is_passable: true,
                light_emission: 11,
            },
            BlockType::EndStone => BlockProperties {
                name: "End Stone",
                hardness: 3.0,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::EndPortalFrame | BlockType::EndPortalFrameFilled => BlockProperties {
                name: "End Portal Frame",
                hardness: -1.0,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: if self == BlockType::EndPortalFrameFilled {
                    2
                } else {
                    0
                },
            },
            BlockType::EndPortal => BlockProperties {
                name: "End Portal",
                hardness: -1.0,
                render_type: RenderType::Translucent,
                is_solid: false,
                is_passable: true,
                light_emission: 15,
            },
            BlockType::Purpur => BlockProperties {
                name: "Purpur Block",
                hardness: 1.5,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::DragonEgg => BlockProperties {
                name: "Dragon Egg",
                hardness: 3.0,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 1,
            },
            BlockType::WitherSkeletonSkull => BlockProperties {
                name: "Wither Skeleton Skull",
                hardness: 1.0,
                render_type: RenderType::Cutout,
                is_solid: false,
                is_passable: true,
                light_emission: 0,
            },
            BlockType::NetherBrick => BlockProperties {
                name: "Nether Bricks",
                hardness: 2.0,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::EndCityChest => BlockProperties {
                name: "End City Chest",
                hardness: 2.5,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 3,
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
            BlockType::EnchantingTable => (0, 13),
            BlockType::BrewingStand => (1, 13),
            BlockType::Anvil => (2, 13),
            BlockType::RedstoneWire => (5, 2),
            BlockType::RedstoneTorch | BlockType::RedstoneTorchOff => (6, 2),
            BlockType::Repeater | BlockType::RepeaterPowered => (7, 2),
            BlockType::Comparator | BlockType::ComparatorPowered => (8, 2),
            BlockType::StoneButton | BlockType::StoneButtonPressed => (9, 2),
            BlockType::Lever | BlockType::LeverOn => (10, 2),
            BlockType::PressurePlate | BlockType::PressurePlatePowered => (11, 2),
            BlockType::Piston | BlockType::PistonExtended => (12, 2),
            BlockType::StickyPiston | BlockType::StickyPistonExtended => (13, 2),
            BlockType::RedstoneLamp => (14, 2),
            BlockType::RedstoneLampLit => (8, 14),
            BlockType::OakDoor | BlockType::OakDoorOpen => (9, 14),
            BlockType::OakTrapdoor | BlockType::OakTrapdoorOpen => (10, 14),
            BlockType::Dispenser => (11, 14),
            BlockType::Dropper => (12, 14),
            BlockType::NoteBlock => (13, 14),
            BlockType::Fire => (15, 12),
            BlockType::SnowLayer => (3, 1),
            BlockType::Netherrack => (10, 15),
            BlockType::SoulSand => (11, 15),
            BlockType::Glowstone => (12, 15),
            BlockType::NetherPortal => (13, 15),
            BlockType::EndStone => (14, 15),
            BlockType::EndPortalFrame => match face_idx {
                4 => (15, 15), // top
                _ => (9, 4),   // sides and bottom
            },
            BlockType::EndPortalFrameFilled => match face_idx {
                4 => (6, 4), // frame top composited with the Eye of Ender
                _ => (9, 4), // sides and bottom retain the frame texture
            },
            BlockType::EndPortal => (14, 10),
            BlockType::Purpur => (15, 10),
            BlockType::DragonEgg => (14, 11),
            BlockType::WitherSkeletonSkull => (15, 11),
            BlockType::NetherBrick => (9, 10),
            BlockType::EndCityChest => (10, 10),
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct GreedyFace {
    block: BlockType,
    atlas_tile: (u32, u32),
    light_level: u16,
    ao_levels: [u8; 4],
}

impl GreedyFace {
    fn can_merge_with(self, other: Self) -> bool {
        self.ao_levels
            .iter()
            .all(|level| *level == self.ao_levels[0])
            && other
                .ao_levels
                .iter()
                .all(|level| *level == other.ao_levels[0])
            && self == other
    }

    fn ao(self) -> [f32; 4] {
        self.ao_levels.map(ambient_occlusion_value)
    }
}

fn ao_level(value: f32) -> u8 {
    if value >= 0.875 {
        0
    } else if value >= 0.625 {
        1
    } else if value >= 0.375 {
        2
    } else {
        3
    }
}

fn push_terrain_quad(
    vertices: &mut Vec<TerrainVertex>,
    indices: &mut Vec<u32>,
    positions: [[f32; 3]; 4],
    local_uvs: [[f32; 2]; 4],
    atlas_tile: (u32, u32),
    light_level: f32,
    ao: [f32; 4],
    region_coord: (i32, i32),
) {
    let start = vertices.len() as u32;
    for corner in 0..4 {
        vertices.push(TerrainVertex::new(
            positions[corner],
            local_uvs[corner],
            [atlas_tile.0 as f32, atlas_tile.1 as f32],
            light_level,
            ao[corner],
            region_coord,
        ));
    }
    indices.extend(quad_indices_for_ao(ao).iter().map(|index| start + index));
}

const TORCH_MIN: f32 = 7.0 / 16.0;
const TORCH_MAX: f32 = 9.0 / 16.0;
const TORCH_HEIGHT: f32 = 10.0 / 16.0;
const TORCH_ATLAS_TILE: (u32, u32) = (4, 2);
const REDSTONE_TORCH_ATLAS_TILE: (u32, u32) = (6, 2);

const CACTUS_MIN: f32 = 1.0 / 16.0;
const CACTUS_MAX: f32 = 15.0 / 16.0;
const END_PORTAL_FRAME_HEIGHT: f32 = 13.0 / 16.0;
const END_PORTAL_SURFACE_HEIGHT: f32 = 12.0 / 16.0;

// Tile-local UV rectangles with a half-texel inset. Side faces use the full
// flame/stem artwork, the cap uses the flame, and the base stretches the final
// stem texel across the otherwise unseen bottom face.
const TORCH_SIDE_UV: [f32; 4] = [6.5 / 16.0, 2.5 / 16.0, 8.5 / 16.0, 13.5 / 16.0];
const TORCH_TOP_UV: [f32; 4] = [6.5 / 16.0, 2.5 / 16.0, 8.5 / 16.0, 4.5 / 16.0];
const TORCH_BOTTOM_UV: [f32; 4] = [7.5 / 16.0, 13.5 / 16.0, 7.5 / 16.0, 13.5 / 16.0];

fn append_torch_mesh(
    vertices: &mut Vec<TerrainVertex>,
    indices: &mut Vec<u32>,
    origin: [f32; 3],
    sky_light: u8,
    block_light: u8,
    atlas_tile: (u32, u32),
    region_coord: (i32, i32),
) {
    let light_level = sky_light as f32 + block_light as f32 * 16.0;

    for (face_idx, (_, corner_data)) in BLOCK_FACES.iter().enumerate() {
        let uv_rect = match face_idx {
            0..=3 => TORCH_SIDE_UV,
            4 => TORCH_TOP_UV,
            5 => TORCH_BOTTOM_UV,
            _ => unreachable!(),
        };
        let mut positions = [[0.0; 3]; 4];
        let mut local_uvs = [[0.0; 2]; 4];

        for (corner_idx, (offset, uv)) in corner_data.iter().enumerate() {
            positions[corner_idx] = [
                origin[0]
                    + if offset[0] == 0.0 {
                        TORCH_MIN
                    } else {
                        TORCH_MAX
                    },
                origin[1] + if offset[1] == 0.0 { 0.0 } else { TORCH_HEIGHT },
                origin[2]
                    + if offset[2] == 0.0 {
                        TORCH_MIN
                    } else {
                        TORCH_MAX
                    },
            ];
            local_uvs[corner_idx] = [
                if uv[0] == 0.0 { uv_rect[0] } else { uv_rect[2] },
                if uv[1] == 0.0 { uv_rect[1] } else { uv_rect[3] },
            ];
        }

        push_terrain_quad(
            vertices,
            indices,
            positions,
            local_uvs,
            atlas_tile,
            light_level,
            [1.0; 4],
            region_coord,
        );
    }
}

fn append_cactus_mesh(
    vertices: &mut Vec<TerrainVertex>,
    indices: &mut Vec<u32>,
    origin: [f32; 3],
    sky_light: u8,
    block_light: u8,
    atlas_tile: (u32, u32),
    region_coord: (i32, i32),
) {
    let light_level = sky_light as f32 + block_light as f32 * 16.0;

    for (face_idx, (_, corner_data)) in BLOCK_FACES.iter().enumerate() {
        let multiplier_code = match face_idx {
            4 => 0.0, // Top
            5 => 2.0, // Bottom
            _ => 1.0, // Sides
        };
        let face_light_level = light_level + multiplier_code * 256.0;

        let mut positions = [[0.0; 3]; 4];
        let mut local_uvs = [[0.0; 2]; 4];

        for (corner_idx, (offset, uv)) in corner_data.iter().enumerate() {
            let vx = origin[0]
                + if offset[0] == 0.0 {
                    CACTUS_MIN
                } else {
                    CACTUS_MAX
                };
            let vy = origin[1] + offset[1];
            let vz = origin[2]
                + if offset[2] == 0.0 {
                    CACTUS_MIN
                } else {
                    CACTUS_MAX
                };
            positions[corner_idx] = [vx, vy, vz];

            let u = if uv[0] == 0.0 { CACTUS_MIN } else { CACTUS_MAX };
            let v = if face_idx < 4 {
                uv[1]
            } else if uv[1] == 0.0 {
                CACTUS_MIN
            } else {
                CACTUS_MAX
            };
            local_uvs[corner_idx] = [u, v];
        }

        push_terrain_quad(
            vertices,
            indices,
            positions,
            local_uvs,
            atlas_tile,
            face_light_level,
            [1.0; 4],
            region_coord,
        );
    }
}

fn append_end_portal_frame_mesh(
    vertices: &mut Vec<TerrainVertex>,
    indices: &mut Vec<u32>,
    origin: [f32; 3],
    block: BlockType,
    sky_light: u8,
    block_light: u8,
    region_coord: (i32, i32),
) {
    for (face_idx, (_, corner_data)) in BLOCK_FACES.iter().enumerate() {
        let multiplier_code = match face_idx {
            4 => 0.0,
            5 => 2.0,
            _ => 1.0,
        };
        let light_level = sky_light as f32 + block_light as f32 * 16.0 + multiplier_code * 256.0;
        let mut positions = [[0.0; 3]; 4];
        let mut local_uvs = [[0.0; 2]; 4];
        for (corner_idx, (offset, uv)) in corner_data.iter().enumerate() {
            positions[corner_idx] = [
                origin[0] + offset[0],
                origin[1]
                    + if offset[1] == 0.0 {
                        0.0
                    } else {
                        END_PORTAL_FRAME_HEIGHT
                    },
                origin[2] + offset[2],
            ];
            local_uvs[corner_idx] = *uv;
        }
        push_terrain_quad(
            vertices,
            indices,
            positions,
            local_uvs,
            block.get_face_tex_index(face_idx),
            light_level,
            [1.0; 4],
            region_coord,
        );
    }
}

fn append_end_portal_surface(
    vertices: &mut Vec<TerrainVertex>,
    indices: &mut Vec<u32>,
    origin: [f32; 3],
    region_coord: (i32, i32),
) {
    let y = origin[1] + END_PORTAL_SURFACE_HEIGHT;
    let positions = [
        [origin[0], y, origin[2] + 1.0],
        [origin[0] + 1.0, y, origin[2] + 1.0],
        [origin[0] + 1.0, y, origin[2]],
        [origin[0], y, origin[2]],
    ];
    let uvs = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    let tile = BlockType::EndPortal.get_face_tex_index(4);
    let light_level = 15.0 * 16.0 + 15.0;
    push_terrain_quad(
        vertices,
        indices,
        positions,
        uvs,
        tile,
        light_level,
        [1.0; 4],
        region_coord,
    );
    push_terrain_quad(
        vertices,
        indices,
        [positions[3], positions[2], positions[1], positions[0]],
        uvs,
        tile,
        light_level,
        [1.0; 4],
        region_coord,
    );
}

fn face_should_render(
    block: BlockType,
    face_idx: usize,
    level: u8,
    falling: bool,
    neighbor: BlockType,
    neighbor_level: u8,
    neighbor_falling: bool,
) -> bool {
    if neighbor == BlockType::Air {
        return true;
    }

    if neighbor.properties().render_type == RenderType::Opaque {
        return false;
    }

    let is_fluid = matches!(block, BlockType::Water | BlockType::Lava);
    if !is_fluid || neighbor != block {
        return true;
    }

    match face_idx {
        4 | 5 => false,
        _ if neighbor_falling => false,
        _ if falling => true,
        _ => neighbor_level > level,
    }
}

fn append_box_mesh(
    vertices: &mut Vec<TerrainVertex>,
    indices: &mut Vec<u32>,
    origin: [f32; 3],
    bounds: ([f32; 3], [f32; 3]),
    sky_light: u8,
    block_light: u8,
    atlas_tile: (u32, u32),
    region_coord: (i32, i32),
) {
    let light_level = sky_light as f32 + block_light as f32 * 16.0;
    let (min, max) = bounds;

    for (_, (_, corner_data)) in BLOCK_FACES.iter().enumerate() {
        let mut positions = [[0.0; 3]; 4];
        let mut local_uvs = [[0.0; 2]; 4];

        for (corner_idx, (offset, uv)) in corner_data.iter().enumerate() {
            positions[corner_idx] = [
                origin[0] + if offset[0] == 0.0 { min[0] } else { max[0] },
                origin[1] + if offset[1] == 0.0 { min[1] } else { max[1] },
                origin[2] + if offset[2] == 0.0 { min[2] } else { max[2] },
            ];
            local_uvs[corner_idx] = [uv[0], uv[1]];
        }

        push_terrain_quad(
            vertices,
            indices,
            positions,
            local_uvs,
            atlas_tile,
            light_level,
            [1.0; 4],
            region_coord,
        );
    }
}

fn append_door_mesh(
    vertices: &mut Vec<TerrainVertex>,
    indices: &mut Vec<u32>,
    origin: [f32; 3],
    state: BlockState,
    sky_light: u8,
    block_light: u8,
    atlas_tile: (u32, u32),
    region_coord: (i32, i32),
) {
    const THICKNESS: f32 = 3.0 / 16.0;

    let (min_x, max_x, min_z, max_z) = if !state.is_open {
        match state.facing {
            Direction::North => (0.0, 1.0, 0.0, THICKNESS),
            Direction::South => (0.0, 1.0, 1.0 - THICKNESS, 1.0),
            Direction::West => (0.0, THICKNESS, 0.0, 1.0),
            Direction::East => (1.0 - THICKNESS, 1.0, 0.0, 1.0),
        }
    } else if !state.is_right_hinge {
        match state.facing {
            Direction::North => (0.0, THICKNESS, 0.0, 1.0),
            Direction::South => (1.0 - THICKNESS, 1.0, 0.0, 1.0),
            Direction::West => (0.0, 1.0, 1.0 - THICKNESS, 1.0),
            Direction::East => (0.0, 1.0, 0.0, THICKNESS),
        }
    } else {
        match state.facing {
            Direction::North => (1.0 - THICKNESS, 1.0, 0.0, 1.0),
            Direction::South => (0.0, THICKNESS, 0.0, 1.0),
            Direction::West => (0.0, 1.0, 0.0, THICKNESS),
            Direction::East => (0.0, 1.0, 1.0 - THICKNESS, 1.0),
        }
    };

    append_box_mesh(
        vertices,
        indices,
        origin,
        ([min_x, 0.0, min_z], [max_x, 1.0, max_z]),
        sky_light,
        block_light,
        atlas_tile,
        region_coord,
    );
}

fn append_trapdoor_mesh(
    vertices: &mut Vec<TerrainVertex>,
    indices: &mut Vec<u32>,
    origin: [f32; 3],
    state: BlockState,
    sky_light: u8,
    block_light: u8,
    atlas_tile: (u32, u32),
    region_coord: (i32, i32),
) {
    const THICKNESS: f32 = 3.0 / 16.0;

    let bounds = if !state.is_open {
        ([0.0, 0.0, 0.0], [1.0, THICKNESS, 1.0])
    } else {
        match state.facing {
            Direction::North => ([0.0, 0.0, 0.0], [1.0, 1.0, THICKNESS]),
            Direction::South => ([0.0, 0.0, 1.0 - THICKNESS], [1.0, 1.0, 1.0]),
            Direction::West => ([0.0, 0.0, 0.0], [THICKNESS, 1.0, 1.0]),
            Direction::East => ([1.0 - THICKNESS, 0.0, 0.0], [1.0, 1.0, 1.0]),
        }
    };

    append_box_mesh(
        vertices,
        indices,
        origin,
        bounds,
        sky_light,
        block_light,
        atlas_tile,
        region_coord,
    );
}

fn is_greedy_cube(block: BlockType) -> bool {
    block.properties().is_solid
        && !block.is_cross_model()
        && !matches!(
            block,
            BlockType::Water
                | BlockType::Lava
                | BlockType::SnowLayer
                | BlockType::OakDoor
                | BlockType::OakDoorOpen
                | BlockType::OakTrapdoor
                | BlockType::OakTrapdoorOpen
                | BlockType::Cactus
                | BlockType::EndPortalFrame
                | BlockType::EndPortalFrameFilled
        )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SurfaceCell {
    height: i32,
    block: BlockType,
    top_tile: (u32, u32),
    light_level: u16,
}

fn is_lod_surface(block: BlockType) -> bool {
    block != BlockType::Air
        && !block.is_cross_model()
        && (block.properties().is_solid
            || matches!(
                block,
                BlockType::Water | BlockType::Lava | BlockType::SnowLayer
            ))
}

pub const SECTION_SIZE: usize = 16;
pub const SECTION_COUNT: usize = CHUNK_HEIGHT / SECTION_SIZE;
pub const SECTION_VOLUME: usize = SECTION_SIZE * SECTION_SIZE * SECTION_SIZE;

/// Stable identity for one vertical 16^3 mesh section.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SectionKey {
    pub cx: i32,
    pub section_y: u16,
    pub cz: i32,
}

impl SectionKey {
    pub const fn new(cx: i32, section_y: u16, cz: i32) -> Self {
        Self { cx, section_y, cz }
    }
    pub const fn min_world_y(self) -> i32 {
        self.section_y as i32 * SECTION_SIZE as i32
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

/// Complete voxel value consumed by section meshing. Keeping all render
/// inputs together prevents workers from falling back to live world lookups.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MeshVoxel {
    pub block: BlockType,
    pub state: u8,
    pub sky: u8,
    pub block_light: u8,
    pub raw_fluid: u8,
}

impl Default for MeshVoxel {
    fn default() -> Self {
        Self {
            block: BlockType::Air,
            state: 0,
            sky: 0,
            block_light: 0,
            raw_fluid: 0,
        }
    }
}

/// Immutable 18^3 voxel snapshot (one-cell halo on all six sides).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SectionHaloSnapshot {
    pub key: SectionKey,
    pub voxels: Box<[MeshVoxel]>,
}

impl SectionHaloSnapshot {
    pub const SIDE: usize = SECTION_SIZE + 2;
    pub const VOLUME: usize = Self::SIDE * Self::SIDE * Self::SIDE;
    pub fn from_chunk<F>(key: SectionKey, mut get: F) -> Self
    where
        F: FnMut(i32, i32, i32) -> MeshVoxel,
    {
        let mut voxels = vec![MeshVoxel::default(); Self::VOLUME].into_boxed_slice();
        for ly in 0..Self::SIDE {
            for z in 0..Self::SIDE {
                for x in 0..Self::SIDE {
                    let wx = key.cx * CHUNK_WIDTH as i32 + x as i32 - 1;
                    let wy = key.min_world_y() + ly as i32 - 1;
                    let wz = key.cz * CHUNK_DEPTH as i32 + z as i32 - 1;
                    voxels[(ly * Self::SIDE + z) * Self::SIDE + x] = get(wx, wy, wz);
                }
            }
        }
        Self { key, voxels }
    }
    pub fn get(&self, x: usize, y: usize, z: usize) -> MeshVoxel {
        self.voxels[(y * Self::SIDE + z) * Self::SIDE + x]
    }
    pub fn get_block(&self, x: usize, y: usize, z: usize) -> BlockType {
        self.get(x, y, z).block
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
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum BlockStorage {
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
pub(crate) enum LightStorage {
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
    blocks: BlockStorage,
    light: LightStorage,
    block_states: Option<Box<[u8; 4096]>>,
    fluid_levels: Option<Box<[u8; 4096]>>,
    non_air_count: u16,
    opaque_count: u16,
    random_tick_count: u16,
    fluid_count: u16,
    emitter_count: u16,
    redstone_count: u16,
    pub(crate) block_state_nonzero_count: u16,
    pub(crate) fluid_level_nonzero_count: u16,
    storage_changes: u16,
}

impl ChunkSection {
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

    /// Read a block through the section API without exposing its representation.
    pub fn get_block(&self, idx: usize) -> BlockType {
        self.blocks.get(idx)
    }

    /// Returns whether this section contains at least one instance of `block`.
    pub fn contains_block(&self, block: BlockType) -> bool {
        (0..4096).any(|idx| self.blocks.get(idx) == block)
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

#[derive(Clone)]
pub struct Chunk {
    pub chunk_x: i32,
    pub chunk_z: i32,
    pub sections: Vec<ChunkSection>,
    /// Per-column max Y of non-air blocks (indexed as [x][z])
    pub heightmap: Box<[[u16; CHUNK_DEPTH]; CHUNK_WIDTH]>,
    /// Compact local coordinates of ordinary torch blocks. Each entry packs
    /// x (4 bits), z (4 bits), and y (8 bits) into a u16.
    pub(crate) torch_positions: Vec<u16>,
    /// Compact local coordinates of redstone component blocks.
    pub(crate) redstone_positions: Vec<u16>,
    /// Block entities keyed by Chunk-local coordinates (x: u8, y: i16, z: u8).
    pub(crate) block_entities:
        std::collections::HashMap<(u8, i16, u8), crate::block_entity::BlockEntity>,
}

impl Chunk {
    pub fn new(chunk_x: i32, chunk_z: i32) -> Self {
        Self::new_with_seed(chunk_x, chunk_z, 12345)
    }

    pub fn new_with_seed(chunk_x: i32, chunk_z: i32, world_seed: u32) -> Self {
        // Allocate on the heap to avoid stack overflow (~192 KB per chunk)
        let mut blocks: Box<[[[BlockType; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH]> =
            vec![[[BlockType::Air; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH]
                .try_into()
                .unwrap();
        let perlin = Perlin::new(world_seed);
        let caves_perlin = Perlin::new(world_seed ^ 0xA341_316C);
        let caverns_perlin = Perlin::new(world_seed ^ 0xC801_3EA4);
        let temp_perlin = Perlin::new(world_seed ^ 0xAD90_777D);
        let moist_perlin = Perlin::new(world_seed ^ 0x7E95_761E);
        let ocean_perlin = Perlin::new(world_seed ^ 0x4CF5_AD43);

        // Simple custom PRNG for ore distribution and bedrock blending
        let mut rng_seed =
            (chunk_x as u32).wrapping_mul(31) ^ (chunk_z as u32) ^ world_seed.rotate_left(13);
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
                    for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                        let nx = x as i32 + dx;
                        let nz = z as i32 + dz;
                        if nx >= 0
                            && nx < CHUNK_WIDTH as i32
                            && nz >= 0
                            && nz < CHUNK_DEPTH as i32
                            && blocks[nx as usize][surface_y][nz as usize] == BlockType::Water
                        {
                            near_water = true;
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
        let mut heightmap: Box<[[u16; CHUNK_DEPTH]; CHUNK_WIDTH]> =
            vec![[0u16; CHUNK_DEPTH]; CHUNK_WIDTH].try_into().unwrap();

        for x in 0..CHUNK_WIDTH {
            for z in 0..CHUNK_DEPTH {
                let mut direct_sky = 15;
                let mut found_h = false;
                for y in (0..CHUNK_HEIGHT).rev() {
                    let block = blocks[x][y][z];
                    if !found_h && block != BlockType::Air {
                        heightmap[x][z] = y as u16;
                        found_h = true;
                    }
                    if block.properties().render_type == RenderType::Opaque {
                        direct_sky = 0;
                    }
                    sky_light[x][y][z] = direct_sky;
                    block_light[x][y][z] = block.properties().light_emission;
                }
            }
        }

        let mut sections = Vec::with_capacity(SECTION_COUNT);
        for sec_y in 0..SECTION_COUNT {
            let mut sec_blocks = [BlockType::Air; 4096];
            let mut sec_sky = [0u8; 4096];
            let mut sec_block_light = [0u8; 4096];
            for ly in 0..SECTION_SIZE {
                let y = sec_y * SECTION_SIZE + ly;
                for z in 0..CHUNK_DEPTH {
                    for x in 0..CHUNK_WIDTH {
                        let idx = (ly << 8) | (z << 4) | x;
                        sec_blocks[idx] = blocks[x][y][z];
                        sec_sky[idx] = sky_light[x][y][z];
                        sec_block_light[idx] = block_light[x][y][z];
                    }
                }
            }
            sections.push(ChunkSection::from_dense(
                &sec_blocks,
                &sec_sky,
                &sec_block_light,
                None,
                None,
            ));
        }

        let torch_positions = Self::build_torch_index_from_sections(&sections);
        let redstone_positions = Self::build_redstone_index_from_sections(&sections);

        Self {
            chunk_x,
            chunk_z,
            sections,
            heightmap,
            torch_positions,
            redstone_positions,
            block_entities: std::collections::HashMap::new(),
        }
    }

    fn encode_torch_position(x: usize, y: usize, z: usize) -> u16 {
        (x as u16) | ((z as u16) << 4) | ((y as u16) << 8)
    }

    /// Decodes a compact local torch/component index into `(x, y, z)` coordinates.
    pub fn decode_torch_position(index: u16) -> (usize, usize, usize) {
        (
            (index & 0x0f) as usize,
            (index >> 8) as usize,
            ((index >> 4) & 0x0f) as usize,
        )
    }

    fn build_torch_index_from_sections(sections: &[ChunkSection]) -> Vec<u16> {
        let mut positions = Vec::new();
        for sec_y in 0..SECTION_COUNT {
            let sec = &sections[sec_y];
            if sec.non_air_count == 0 {
                continue;
            }
            for ly in 0..SECTION_SIZE {
                let y = sec_y * SECTION_SIZE + ly;
                for z in 0..CHUNK_DEPTH {
                    for x in 0..CHUNK_WIDTH {
                        let idx = (ly << 8) | (z << 4) | x;
                        if sec.blocks.get(idx) == BlockType::Torch {
                            positions.push(Self::encode_torch_position(x, y, z));
                        }
                    }
                }
            }
        }
        positions
    }

    fn build_redstone_index_from_sections(sections: &[ChunkSection]) -> Vec<u16> {
        let mut positions = Vec::new();
        for sec_y in 0..SECTION_COUNT {
            let sec = &sections[sec_y];
            if sec.redstone_count == 0 {
                continue;
            }
            for ly in 0..SECTION_SIZE {
                let y = sec_y * SECTION_SIZE + ly;
                for z in 0..CHUNK_DEPTH {
                    for x in 0..CHUNK_WIDTH {
                        let idx = (ly << 8) | (z << 4) | x;
                        if crate::redstone::is_component(sec.blocks.get(idx)) {
                            positions.push(Self::encode_torch_position(x, y, z));
                        }
                    }
                }
            }
        }
        positions
    }

    /// Returns the indexed local positions of ordinary torches.
    pub fn torch_positions(&self) -> &[u16] {
        &self.torch_positions
    }

    /// Returns the indexed local positions of redstone components.
    pub fn redstone_positions(&self) -> &[u16] {
        &self.redstone_positions
    }

    /// Bytes owned by this chunk, including representation-specific section
    /// storage, vector spare capacity, and the boxed heightmap allocation.
    pub fn memory_usage(&self) -> usize {
        size_of::<Self>()
            + self.sections.capacity() * size_of::<ChunkSection>()
            + self
                .sections
                .iter()
                .map(|section| {
                    section
                        .memory_usage()
                        .saturating_sub(size_of::<ChunkSection>())
                })
                .sum::<usize>()
            + size_of_val(self.heightmap.as_ref())
            + self.torch_positions.capacity() * size_of::<u16>()
            + self.redstone_positions.capacity() * size_of::<u16>()
            + self.block_entities.capacity()
                * (size_of::<(u8, i16, u8)>() + size_of::<crate::block_entity::BlockEntity>())
            + self
                .block_entities
                .values()
                .map(|e| e.memory_usage())
                .sum::<usize>()
    }

    pub fn get_block_entity(
        &self,
        x: u8,
        y: i16,
        z: u8,
    ) -> Option<&crate::block_entity::BlockEntity> {
        if (x as usize) >= CHUNK_WIDTH
            || (z as usize) >= CHUNK_DEPTH
            || y < 0
            || (y as usize) >= CHUNK_HEIGHT
        {
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
        if (x as usize) >= CHUNK_WIDTH
            || (z as usize) >= CHUNK_DEPTH
            || y < 0
            || (y as usize) >= CHUNK_HEIGHT
        {
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
        if (x as usize) >= CHUNK_WIDTH
            || (z as usize) >= CHUNK_DEPTH
            || y < 0
            || (y as usize) >= CHUNK_HEIGHT
        {
            return Err(BlockEntityError::OutOfBounds);
        }
        let block_type = self.get_block_local(x as usize, y as usize, z as usize);
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
        if (x as usize) >= CHUNK_WIDTH
            || (z as usize) >= CHUNK_DEPTH
            || y < 0
            || (y as usize) >= CHUNK_HEIGHT
        {
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
        self.torch_positions = Self::build_torch_index_from_sections(&self.sections);
    }

    /// Rebuilds the redstone index after bulk block mutations (generation/load).
    pub fn rebuild_redstone_index(&mut self) {
        self.redstone_positions = Self::build_redstone_index_from_sections(&self.sections);
    }

    /// Sets a local block and keeps the torch and redstone indices synchronized.
    pub fn set_block_local(&mut self, x: usize, y: usize, z: usize, block: BlockType) {
        let sec_y = y / SECTION_SIZE;
        let ly = y % SECTION_SIZE;
        let idx = (ly << 8) | (z << 4) | x;
        let old = self.sections[sec_y].set_block(idx, block);
        if old == block {
            return;
        }

        if let Some(entity) = self.block_entities.get(&(x as u8, y as i16, z as u8)) {
            if !entity.matches_block_type(block) {
                self.block_entities.remove(&(x as u8, y as i16, z as u8));
            }
        }

        let encoded = Self::encode_torch_position(x, y, z);
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
    }

    /// Update heightmap for a single column after block placement/removal
    pub fn update_heightmap(&mut self, x: usize, z: usize) {
        for sec_y in (0..SECTION_COUNT).rev() {
            if self.sections[sec_y].non_air_count == 0 {
                continue;
            }
            for ly in (0..SECTION_SIZE).rev() {
                let y = sec_y * SECTION_SIZE + ly;
                let idx = (ly << 8) | (z << 4) | x;
                if self.sections[sec_y].blocks.get(idx) != BlockType::Air {
                    self.heightmap[x][z] = y as u16;
                    return;
                }
            }
        }
        self.heightmap[x][z] = 0;
    }

    pub fn get_block_local(&self, x: usize, y: usize, z: usize) -> BlockType {
        let sec_y = y / SECTION_SIZE;
        let ly = y % SECTION_SIZE;
        let idx = (ly << 8) | (z << 4) | x;
        self.sections[sec_y].blocks.get(idx)
    }

    pub fn get_block(&self, x: i32, y: i32, z: i32) -> BlockType {
        if x < 0
            || x >= CHUNK_WIDTH as i32
            || y < 0
            || y >= CHUNK_HEIGHT as i32
            || z < 0
            || z >= CHUNK_DEPTH as i32
        {
            return BlockType::Air;
        }
        self.get_block_local(x as usize, y as usize, z as usize)
    }

    pub fn get_sky_light(&self, x: usize, y: usize, z: usize) -> u8 {
        let sec_y = y / SECTION_SIZE;
        let ly = y % SECTION_SIZE;
        let idx = (ly << 8) | (z << 4) | x;
        self.sections[sec_y].light.get_sky(idx)
    }

    pub fn set_sky_light(&mut self, x: usize, y: usize, z: usize, val: u8) {
        let sec_y = y / SECTION_SIZE;
        let ly = y % SECTION_SIZE;
        let idx = (ly << 8) | (z << 4) | x;
        self.sections[sec_y].light.set_sky(idx, val);
        self.sections[sec_y].storage_changes =
            self.sections[sec_y].storage_changes.saturating_add(1);
    }

    pub fn get_block_light(&self, x: usize, y: usize, z: usize) -> u8 {
        let sec_y = y / SECTION_SIZE;
        let ly = y % SECTION_SIZE;
        let idx = (ly << 8) | (z << 4) | x;
        self.sections[sec_y].light.get_block(idx)
    }

    pub fn set_block_light(&mut self, x: usize, y: usize, z: usize, val: u8) {
        let sec_y = y / SECTION_SIZE;
        let ly = y % SECTION_SIZE;
        let idx = (ly << 8) | (z << 4) | x;
        self.sections[sec_y].light.set_block(idx, val);
        self.sections[sec_y].storage_changes =
            self.sections[sec_y].storage_changes.saturating_add(1);
    }

    pub fn get_block_state(&self, x: i32, y: i32, z: i32) -> u8 {
        if x < 0
            || x >= CHUNK_WIDTH as i32
            || y < 0
            || y >= CHUNK_HEIGHT as i32
            || z < 0
            || z >= CHUNK_DEPTH as i32
        {
            0
        } else {
            let ux = x as usize;
            let uy = y as usize;
            let uz = z as usize;
            let sec_y = uy / SECTION_SIZE;
            let ly = uy % SECTION_SIZE;
            let idx = (ly << 8) | (uz << 4) | ux;
            self.sections[sec_y].get_block_state(idx)
        }
    }

    pub fn set_block_state(&mut self, x: i32, y: i32, z: i32, state: u8) {
        if x >= 0
            && x < CHUNK_WIDTH as i32
            && y >= 0
            && y < CHUNK_HEIGHT as i32
            && z >= 0
            && z < CHUNK_DEPTH as i32
        {
            let ux = x as usize;
            let uy = y as usize;
            let uz = z as usize;
            let sec_y = uy / SECTION_SIZE;
            let ly = uy % SECTION_SIZE;
            let idx = (ly << 8) | (uz << 4) | ux;
            self.sections[sec_y].set_block_state(idx, state);
        }
    }

    pub fn get_fluid_level(&self, x: usize, y: usize, z: usize) -> u8 {
        let sec_y = y / SECTION_SIZE;
        let ly = y % SECTION_SIZE;
        let idx = (ly << 8) | (z << 4) | x;
        self.sections[sec_y].get_fluid_level(idx)
    }

    pub fn set_fluid_level(&mut self, x: usize, y: usize, z: usize, level: u8) {
        let sec_y = y / SECTION_SIZE;
        let ly = y % SECTION_SIZE;
        let idx = (ly << 8) | (z << 4) | x;
        self.sections[sec_y].set_fluid_level(idx, level);
    }

    // Generate opaque/cutout and translucent terrain meshes. Full cube faces
    // use conservative greedy merging: material/light must match and AO must
    // be uniform so removing internal vertices cannot change shading.
    fn mesh_l0_volume<F>(
        origin: [i32; 3],
        extent: [usize; 3],
        get_voxel: F,
    ) -> (Vec<TerrainVertex>, Vec<u32>, Vec<TerrainVertex>, Vec<u32>)
    where
        F: Fn(i32, i32, i32) -> MeshVoxel,
    {
        let get_block_at = |x: i32, y: i32, z: i32| {
            let v = get_voxel(x, y, z);
            (
                v.block,
                v.sky,
                v.block_light,
                v.raw_fluid & 7,
                v.raw_fluid & 8 != 0,
            )
        };
        let mut opaque_vertices = Vec::new();
        let mut opaque_indices = Vec::new();
        let mut trans_vertices = Vec::new();
        let mut trans_indices = Vec::new();

        let region_coord = crate::chunk_render::chunk_to_region_coord(
            origin[0] / CHUNK_WIDTH as i32,
            origin[2] / CHUNK_DEPTH as i32,
        );

        // Non-cubic geometry and non-solid decorative blocks retain the exact
        // per-block path. They cannot be combined into rectangular cube faces.
        for x in 0..extent[0] {
            for z in 0..extent[2] {
                for y in 0..extent[1] {
                    let voxel = get_voxel(
                        origin[0] + x as i32,
                        origin[1] + y as i32,
                        origin[2] + z as i32,
                    );
                    let block = voxel.block;
                    if block == BlockType::Air || is_greedy_cube(block) {
                        continue;
                    }

                    let world_x = origin[0] + x as i32;
                    let world_y = origin[1] + y as i32;
                    let world_z = origin[2] + z as i32;

                    let torch_atlas_tile = match block {
                        BlockType::Torch => Some(TORCH_ATLAS_TILE),
                        BlockType::RedstoneTorch | BlockType::RedstoneTorchOff => {
                            Some(REDSTONE_TORCH_ATLAS_TILE)
                        }
                        _ => None,
                    };
                    if let Some(atlas_tile) = torch_atlas_tile {
                        append_torch_mesh(
                            &mut opaque_vertices,
                            &mut opaque_indices,
                            [world_x as f32, world_y as f32, world_z as f32],
                            voxel.sky,
                            voxel.block_light,
                            atlas_tile,
                            region_coord,
                        );
                        continue;
                    }

                    if matches!(block, BlockType::OakDoor | BlockType::OakDoorOpen) {
                        let state = BlockState::decode(voxel.state);
                        append_door_mesh(
                            &mut opaque_vertices,
                            &mut opaque_indices,
                            [world_x as f32, world_y as f32, world_z as f32],
                            state,
                            voxel.sky,
                            voxel.block_light,
                            (9, 14),
                            region_coord,
                        );
                        continue;
                    }

                    if matches!(block, BlockType::OakTrapdoor | BlockType::OakTrapdoorOpen) {
                        let state = BlockState::decode(voxel.state);
                        append_trapdoor_mesh(
                            &mut opaque_vertices,
                            &mut opaque_indices,
                            [world_x as f32, world_y as f32, world_z as f32],
                            state,
                            voxel.sky,
                            voxel.block_light,
                            (10, 14),
                            region_coord,
                        );
                        continue;
                    }

                    if block == BlockType::Cactus {
                        append_cactus_mesh(
                            &mut opaque_vertices,
                            &mut opaque_indices,
                            [world_x as f32, world_y as f32, world_z as f32],
                            voxel.sky,
                            voxel.block_light,
                            (11, 12),
                            region_coord,
                        );
                        continue;
                    }

                    if matches!(
                        block,
                        BlockType::EndPortalFrame | BlockType::EndPortalFrameFilled
                    ) {
                        append_end_portal_frame_mesh(
                            &mut opaque_vertices,
                            &mut opaque_indices,
                            [world_x as f32, world_y as f32, world_z as f32],
                            block,
                            voxel.sky,
                            voxel.block_light,
                            region_coord,
                        );
                        continue;
                    }

                    if block == BlockType::EndPortal {
                        append_end_portal_surface(
                            &mut trans_vertices,
                            &mut trans_indices,
                            [world_x as f32, world_y as f32, world_z as f32],
                            region_coord,
                        );
                        continue;
                    }

                    if block.is_cross_model() {
                        let sky_val = voxel.sky;
                        let block_val = voxel.block_light;
                        let light_val = sky_val as f32 + block_val as f32 * 16.0 + 1.0 * 256.0;

                        let atlas_tile = block.get_face_tex_index(0);

                        let wx = world_x as f32;
                        let wy = world_y as f32;
                        let wz = world_z as f32;

                        let min_off = 0.1464466;
                        let max_off = 0.8535534;

                        let plane1_p0 = [wx + min_off, wy, wz + min_off];
                        let plane1_p1 = [wx + max_off, wy, wz + max_off];
                        let plane1_p2 = [wx + max_off, wy + 1.0, wz + max_off];
                        let plane1_p3 = [wx + min_off, wy + 1.0, wz + min_off];

                        let plane2_p0 = [wx + max_off, wy, wz + min_off];
                        let plane2_p1 = [wx + min_off, wy, wz + max_off];
                        let plane2_p2 = [wx + min_off, wy + 1.0, wz + max_off];
                        let plane2_p3 = [wx + max_off, wy + 1.0, wz + min_off];

                        let planes = [
                            (plane1_p0, plane1_p1, plane1_p2, plane1_p3),
                            (plane2_p0, plane2_p1, plane2_p2, plane2_p3),
                        ];

                        for (p0, p1, p2, p3) in planes {
                            push_terrain_quad(
                                &mut opaque_vertices,
                                &mut opaque_indices,
                                [p0, p1, p2, p3],
                                [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
                                atlas_tile,
                                light_val,
                                [1.0; 4],
                                region_coord,
                            );
                            push_terrain_quad(
                                &mut opaque_vertices,
                                &mut opaque_indices,
                                [p1, p0, p3, p2],
                                [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
                                atlas_tile,
                                light_val,
                                [1.0; 4],
                                region_coord,
                            );
                        }

                        continue;
                    }

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
                        let is_fluid = block == BlockType::Water || block == BlockType::Lava;
                        let fl_raw = voxel.raw_fluid;
                        let level = fl_raw & 0x07;
                        let falling = (fl_raw & 0x08) != 0;

                        if face_should_render(
                            block,
                            face_idx,
                            level,
                            falling,
                            neighbor,
                            neighbor_level,
                            neighbor_falling,
                        ) {
                            let block_render_type = block.properties().render_type;
                            let is_translucent = block_render_type == RenderType::Translucent;

                            let (v_list, i_list) = if is_translucent {
                                (&mut trans_vertices, &mut trans_indices)
                            } else {
                                (&mut opaque_vertices, &mut opaque_indices)
                            };

                            let atlas_tile = block.get_face_tex_index(face_idx);

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
                            } else if block == BlockType::SnowLayer {
                                0.125
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

                            let mut positions = [[0.0; 3]; 4];
                            let mut local_uvs = [[0.0; 2]; 4];
                            for (corner_idx, (offset, uv)) in corner_data.iter().enumerate() {
                                let mut vy = world_y as f32 + offset[1];
                                if (is_fluid || block == BlockType::SnowLayer) && offset[1] > 0.0 {
                                    vy = world_y as f32 + h;
                                }

                                positions[corner_idx] =
                                    [world_x as f32 + offset[0], vy, world_z as f32 + offset[2]];
                                local_uvs[corner_idx] = *uv;
                            }
                            push_terrain_quad(
                                v_list,
                                i_list,
                                positions,
                                local_uvs,
                                atlas_tile,
                                light_val,
                                ao,
                                region_coord,
                            );
                        }
                    }
                }
            }
        }

        // Full cube faces are processed one direction/slice at a time. Each
        // mask cell describes one visible face. Rectangles only grow across
        // identical material/light and uniform AO.
        let dimensions = extent;
        for (face_idx, (normal, corner_data)) in BLOCK_FACES.iter().enumerate() {
            let normal_axis = (0..3).find(|axis| normal[*axis] != 0).unwrap();
            let u_axis = (0..3)
                .find(|axis| corner_data[0].0[*axis] != corner_data[1].0[*axis])
                .unwrap();
            let v_axis = (0..3)
                .find(|axis| corner_data[0].0[*axis] != corner_data[3].0[*axis])
                .unwrap();
            let u_len = dimensions[u_axis];
            let v_len = dimensions[v_axis];

            for slice in 0..dimensions[normal_axis] {
                let mut mask = vec![None::<GreedyFace>; u_len * v_len];
                for v in 0..v_len {
                    for u in 0..u_len {
                        let mut local = [0usize; 3];
                        local[normal_axis] = slice;
                        local[u_axis] = u;
                        local[v_axis] = v;
                        let [x, y, z] = local;

                        let voxel = get_voxel(
                            origin[0] + x as i32,
                            origin[1] + y as i32,
                            origin[2] + z as i32,
                        );
                        let block = voxel.block;
                        if !is_greedy_cube(block) {
                            continue;
                        }

                        let world = [
                            origin[0] + x as i32,
                            origin[1] + y as i32,
                            origin[2] + z as i32,
                        ];
                        let nx = world[0] + normal[0];
                        let ny = world[1] + normal[1];
                        let nz = world[2] + normal[2];
                        let (
                            neighbor,
                            neighbor_sky,
                            neighbor_block,
                            neighbor_level,
                            neighbor_falling,
                        ) = get_block_at(nx, ny, nz);
                        if !face_should_render(
                            block,
                            face_idx,
                            0,
                            false,
                            neighbor,
                            neighbor_level,
                            neighbor_falling,
                        ) {
                            continue;
                        }

                        let multiplier_code = match face_idx {
                            4 => 0u16,
                            5 => 2u16,
                            _ => 1u16,
                        };
                        let light_level = neighbor_sky as u16
                            + neighbor_block as u16 * 16
                            + multiplier_code * 256;
                        let mut ao_levels = [0u8; 4];
                        for (corner_idx, (offset, _)) in corner_data.iter().enumerate() {
                            ao_levels[corner_idx] = ao_level(ambient_occlusion_for_vertex(
                                world,
                                *normal,
                                *offset,
                                &get_block_at,
                            ));
                        }

                        let (tile_x, tile_y) = block.get_face_tex_index(face_idx);
                        mask[v * u_len + u] = Some(GreedyFace {
                            block,
                            atlas_tile: (tile_x, tile_y),
                            light_level,
                            ao_levels,
                        });
                    }
                }

                for v in 0..v_len {
                    let mut u = 0;
                    while u < u_len {
                        let index = v * u_len + u;
                        let Some(face) = mask[index] else {
                            u += 1;
                            continue;
                        };

                        let mut width = 1;
                        if face
                            .ao_levels
                            .iter()
                            .all(|level| *level == face.ao_levels[0])
                        {
                            while u + width < u_len
                                && mask[v * u_len + u + width]
                                    .is_some_and(|other| face.can_merge_with(other))
                            {
                                width += 1;
                            }
                        }

                        let mut height = 1;
                        'grow_height: while v + height < v_len {
                            for offset in 0..width {
                                if !mask[(v + height) * u_len + u + offset]
                                    .is_some_and(|other| face.can_merge_with(other))
                                {
                                    break 'grow_height;
                                }
                            }
                            height += 1;
                        }

                        for row in 0..height {
                            for column in 0..width {
                                mask[(v + row) * u_len + u + column] = None;
                            }
                        }

                        let mut min_local = [0.0f32; 3];
                        min_local[normal_axis] = slice as f32;
                        min_local[u_axis] = u as f32;
                        min_local[v_axis] = v as f32;
                        let mut max_local =
                            [min_local[0] + 1.0, min_local[1] + 1.0, min_local[2] + 1.0];
                        max_local[u_axis] = min_local[u_axis] + width as f32;
                        max_local[v_axis] = min_local[v_axis] + height as f32;

                        let world_origin = [origin[0] as f32, origin[1] as f32, origin[2] as f32];
                        let mut positions = [[0.0f32; 3]; 4];
                        let mut local_uvs = [[0.0f32; 2]; 4];
                        for (corner_idx, (offset, uv)) in corner_data.iter().enumerate() {
                            for axis in 0..3 {
                                positions[corner_idx][axis] = world_origin[axis]
                                    + if offset[axis] == 0.0 {
                                        min_local[axis]
                                    } else {
                                        max_local[axis]
                                    };
                            }
                            local_uvs[corner_idx] = [uv[0] * width as f32, uv[1] * height as f32];
                        }

                        let (vertices, indices) =
                            if face.block.properties().render_type == RenderType::Translucent {
                                (&mut trans_vertices, &mut trans_indices)
                            } else {
                                (&mut opaque_vertices, &mut opaque_indices)
                            };
                        push_terrain_quad(
                            vertices,
                            indices,
                            positions,
                            local_uvs,
                            face.atlas_tile,
                            face.light_level as f32,
                            face.ao(),
                            region_coord,
                        );
                        u += width;
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

    pub fn generate_mesh<F>(
        &self,
        get_block_at: F,
    ) -> (Vec<TerrainVertex>, Vec<u32>, Vec<TerrainVertex>, Vec<u32>)
    where
        F: Fn(i32, i32, i32) -> (BlockType, u8, u8, u8, bool),
    {
        let origin = [
            self.chunk_x * CHUNK_WIDTH as i32,
            0,
            self.chunk_z * CHUNK_DEPTH as i32,
        ];
        Self::mesh_l0_volume(
            origin,
            [CHUNK_WIDTH, CHUNK_HEIGHT, CHUNK_DEPTH],
            |x, y, z| {
                let (lookup_block, sky, block_light, level, falling) = get_block_at(x, y, z);
                let in_chunk = x.div_euclid(CHUNK_WIDTH as i32) == self.chunk_x
                    && z.div_euclid(CHUNK_DEPTH as i32) == self.chunk_z
                    && (0..CHUNK_HEIGHT as i32).contains(&y);
                let block = if in_chunk {
                    self.get_block_local(
                        x.rem_euclid(CHUNK_WIDTH as i32) as usize,
                        y as usize,
                        z.rem_euclid(CHUNK_DEPTH as i32) as usize,
                    )
                } else {
                    lookup_block
                };
                MeshVoxel {
                    block,
                    state: self.get_block_state(x - origin[0], y, z - origin[2]),
                    sky,
                    block_light,
                    raw_fluid: level | if falling { 8 } else { 0 },
                }
            },
        )
    }

    pub fn generate_mesh_bundle<F>(&self, get_block_at: F) -> ChunkMeshBundle
    where
        F: Fn(i32, i32, i32) -> (BlockType, u8, u8, u8, bool) + Copy,
    {
        let region_coord = crate::chunk_render::chunk_to_region_coord(self.chunk_x, self.chunk_z);
        let (o0, oi0, t0, ti0) = self.generate_mesh(get_block_at);
        let l1 = self.generate_surface_mesh(get_block_at, 1);
        let l2 = self.generate_surface_mesh(get_block_at, 4);
        let mut section_connectivity = [crate::culling::SectionConnectivity::FULL; SECTION_COUNT];
        for sec_y in 0..SECTION_COUNT {
            section_connectivity[sec_y] = crate::culling::compute_section_connectivity(self, sec_y);
        }
        ChunkMeshBundle {
            levels: [
                ChunkLodMeshData::from_parts(o0, oi0, t0, ti0, region_coord),
                l1,
                l2,
            ],
            section_connectivity,
        }
    }

    /// Generates meshes for one section only. Blocks outside the requested
    /// 16-block Y interval are blanked in the meshing copy while the supplied
    /// lookup remains world-backed, preserving the one-cell halo semantics.
    pub fn generate_section_mesh_bundle<F>(
        &self,
        key: SectionKey,
        revision: u64,
        lifetime: u64,
        get_block_at: F,
    ) -> crate::chunk_render::SectionMeshBundle
    where
        F: Fn(i32, i32, i32) -> (BlockType, u8, u8, u8, bool) + Copy,
    {
        assert_eq!((self.chunk_x, self.chunk_z), (key.cx, key.cz));
        assert!((key.section_y as usize) < SECTION_COUNT);
        // Materialize the immutable 18^3 halo up front. The worker lookup
        // below consults this snapshot for all block-occlusion decisions,
        // ensuring boundary/AO results are independent of later mutations.
        let halo = SectionHaloSnapshot::from_chunk(key, |wx, wy, wz| {
            if wx.div_euclid(CHUNK_WIDTH as i32) == key.cx
                && wz.div_euclid(CHUNK_DEPTH as i32) == key.cz
                && (0..CHUNK_HEIGHT as i32).contains(&wy)
            {
                let x = wx.rem_euclid(CHUNK_WIDTH as i32) as usize;
                let y = wy as usize;
                let z = wz.rem_euclid(CHUNK_DEPTH as i32) as usize;
                MeshVoxel {
                    block: self.get_block_local(x, y, z),
                    state: self.get_block_state(
                        wx - key.cx * CHUNK_WIDTH as i32,
                        wy,
                        wz - key.cz * CHUNK_DEPTH as i32,
                    ),
                    sky: self.get_sky_light(x, y, z),
                    block_light: self.get_block_light(x, y, z),
                    raw_fluid: self.get_fluid_level(x, y, z),
                }
            } else {
                let (block, sky, block_light, level, falling) = get_block_at(wx, wy, wz);
                MeshVoxel {
                    block,
                    sky,
                    block_light,
                    raw_fluid: level | if falling { 8 } else { 0 },
                    ..MeshVoxel::default()
                }
            }
        });
        Self::generate_section_mesh_bundle_from_halo(
            SectionIdentity::new(key, revision, lifetime),
            &halo,
        )
    }

    /// Builds a section mesh exclusively from the immutable 18^3 worker
    /// snapshot. This is the runtime entry point; no live Chunk/ChunkManager
    /// state is consulted after dispatch.
    pub fn generate_section_mesh_bundle_from_halo(
        identity: SectionIdentity,
        halo: &SectionHaloSnapshot,
    ) -> crate::chunk_render::SectionMeshBundle {
        let key = identity.key;
        debug_assert_eq!(halo.key, key);
        let section_voxel = |wx: i32, wy: i32, wz: i32| {
            let hx = wx - key.cx * CHUNK_WIDTH as i32 + 1;
            let hy = wy - key.min_world_y() + 1;
            let hz = wz - key.cz * CHUNK_DEPTH as i32 + 1;
            if (0..SectionHaloSnapshot::SIDE as i32).contains(&hx)
                && (0..SectionHaloSnapshot::SIDE as i32).contains(&hy)
                && (0..SectionHaloSnapshot::SIDE as i32).contains(&hz)
            {
                let voxel = halo.get(hx as usize, hy as usize, hz as usize);
                return voxel;
            }
            // Coordinates outside the captured halo are never queried by the
            // section worker. Keep a deterministic sentinel for defensive use.
            MeshVoxel::default()
        };
        let origin = [
            key.cx * CHUNK_WIDTH as i32,
            key.min_world_y(),
            key.cz * CHUNK_DEPTH as i32,
        ];
        let (o, oi, t, ti) = Self::mesh_l0_volume(
            origin,
            [CHUNK_WIDTH, SECTION_SIZE, CHUNK_DEPTH],
            section_voxel,
        );
        let region_coord = crate::chunk_render::chunk_to_region_coord(key.cx, key.cz);
        let l0 = ChunkLodMeshData::from_parts(o, oi, t, ti, region_coord);
        let l1 = Self::mesh_section_lod_from_halo(key, halo, 2);
        let l2 = Self::mesh_section_lod_from_halo(key, halo, 4);
        let levels = [l0, l1, l2];
        let bounds = levels
            .iter()
            .filter_map(ChunkLodMeshData::bounds)
            .reduce(|a, b| a.union(b));
        crate::chunk_render::SectionMeshBundle {
            identity,
            levels,
            bounds,
            connectivity: crate::culling::compute_section_connectivity_snapshot(halo),
        }
    }

    fn mesh_section_lod_from_halo(
        key: SectionKey,
        halo: &SectionHaloSnapshot,
        step: usize,
    ) -> ChunkLodMeshData {
        debug_assert!(step > 1 && SECTION_SIZE % step == 0);
        let mut coarse = [MeshVoxel::default(); SECTION_VOLUME];

        for cell_y in (0..SECTION_SIZE).step_by(step) {
            for cell_z in (0..CHUNK_DEPTH).step_by(step) {
                for cell_x in (0..CHUNK_WIDTH).step_by(step) {
                    let mut representative = MeshVoxel::default();
                    'sample: for dy in 0..step {
                        for dz in 0..step {
                            for dx in 0..step {
                                let voxel =
                                    halo.get(cell_x + dx + 1, cell_y + dy + 1, cell_z + dz + 1);
                                if voxel.block != BlockType::Air {
                                    representative = voxel;
                                    break 'sample;
                                }
                            }
                        }
                    }
                    if representative.block == BlockType::Air {
                        continue;
                    }
                    for dy in 0..step {
                        for dz in 0..step {
                            for dx in 0..step {
                                let x = cell_x + dx;
                                let y = cell_y + dy;
                                let z = cell_z + dz;
                                coarse[(y * CHUNK_DEPTH + z) * CHUNK_WIDTH + x] = representative;
                            }
                        }
                    }
                }
            }
        }

        let origin = [
            key.cx * CHUNK_WIDTH as i32,
            key.min_world_y(),
            key.cz * CHUNK_DEPTH as i32,
        ];
        let voxel = |wx: i32, wy: i32, wz: i32| {
            let x = wx - origin[0];
            let y = wy - origin[1];
            let z = wz - origin[2];
            if (0..CHUNK_WIDTH as i32).contains(&x)
                && (0..SECTION_SIZE as i32).contains(&y)
                && (0..CHUNK_DEPTH as i32).contains(&z)
            {
                return coarse[(y as usize * CHUNK_DEPTH + z as usize) * CHUNK_WIDTH + x as usize];
            }
            let hx = x + 1;
            let hy = y + 1;
            let hz = z + 1;
            if (0..SectionHaloSnapshot::SIDE as i32).contains(&hx)
                && (0..SectionHaloSnapshot::SIDE as i32).contains(&hy)
                && (0..SectionHaloSnapshot::SIDE as i32).contains(&hz)
            {
                halo.get(hx as usize, hy as usize, hz as usize)
            } else {
                MeshVoxel::default()
            }
        };
        let (opaque, opaque_indices, transparent, transparent_indices) =
            Self::mesh_l0_volume(origin, [CHUNK_WIDTH, SECTION_SIZE, CHUNK_DEPTH], voxel);
        ChunkLodMeshData::from_parts(
            opaque,
            opaque_indices,
            transparent,
            transparent_indices,
            crate::chunk_render::chunk_to_region_coord(key.cx, key.cz),
        )
    }

    fn generate_surface_mesh<F>(&self, get_block_at: F, step: usize) -> ChunkLodMeshData
    where
        F: Fn(i32, i32, i32) -> (BlockType, u8, u8, u8, bool) + Copy,
    {
        let region_coord = crate::chunk_render::chunk_to_region_coord(self.chunk_x, self.chunk_z);
        debug_assert!(step > 0 && CHUNK_WIDTH % step == 0 && CHUNK_DEPTH % step == 0);
        let grid_width = CHUNK_WIDTH / step;
        let grid_depth = CHUNK_DEPTH / step;
        let mut cells = vec![None::<SurfaceCell>; grid_width * grid_depth];

        for gz in 0..grid_depth {
            for gx in 0..grid_width {
                let mut best: Option<(usize, usize, usize, BlockType)> = None;
                for dz in 0..step {
                    for dx in 0..step {
                        let x = gx * step + dx;
                        let z = gz * step + dz;
                        let mut y = self.heightmap[x][z] as usize;
                        loop {
                            let block = self.get_block_local(x, y, z);
                            if is_lod_surface(block) {
                                if best.map_or(true, |(_, best_y, _, _)| y > best_y) {
                                    best = Some((x, y, z, block));
                                }
                                break;
                            }
                            if y == 0 {
                                break;
                            }
                            y -= 1;
                        }
                    }
                }

                let Some((x, y, z, block)) = best else {
                    continue;
                };
                let world_x = self.chunk_x * CHUNK_WIDTH as i32 + x as i32;
                let world_z = self.chunk_z * CHUNK_DEPTH as i32 + z as i32;
                let (_, sky, block_light, _, _) = get_block_at(world_x, y as i32 + 1, world_z);
                cells[gz * grid_width + gx] = Some(SurfaceCell {
                    height: y as i32,
                    block,
                    top_tile: block.get_face_tex_index(4),
                    light_level: sky as u16 + block_light as u16 * 16,
                });
            }
        }

        let mut opaque_vertices = Vec::new();
        let mut opaque_indices = Vec::new();
        let mut trans_vertices = Vec::new();
        let mut trans_indices = Vec::new();
        let world_x0 = (self.chunk_x * CHUNK_WIDTH as i32) as f32;
        let world_z0 = (self.chunk_z * CHUNK_DEPTH as i32) as f32;

        // Greedily merge equal top surface cells.
        let mut top_mask = cells.clone();
        for gz in 0..grid_depth {
            let mut gx = 0;
            while gx < grid_width {
                let index = gz * grid_width + gx;
                let Some(cell) = top_mask[index] else {
                    gx += 1;
                    continue;
                };
                let mut width = 1;
                while gx + width < grid_width
                    && top_mask[gz * grid_width + gx + width] == Some(cell)
                {
                    width += 1;
                }
                let mut depth = 1;
                'grow_depth: while gz + depth < grid_depth {
                    for offset in 0..width {
                        if top_mask[(gz + depth) * grid_width + gx + offset] != Some(cell) {
                            break 'grow_depth;
                        }
                    }
                    depth += 1;
                }
                for row in 0..depth {
                    for column in 0..width {
                        top_mask[(gz + row) * grid_width + gx + column] = None;
                    }
                }

                let x0 = world_x0 + (gx * step) as f32;
                let x1 = world_x0 + ((gx + width) * step) as f32;
                let z0 = world_z0 + (gz * step) as f32;
                let z1 = world_z0 + ((gz + depth) * step) as f32;
                let y = cell.height as f32 + 1.0;
                let (vertices, indices) =
                    if cell.block.properties().render_type == RenderType::Translucent {
                        (&mut trans_vertices, &mut trans_indices)
                    } else {
                        (&mut opaque_vertices, &mut opaque_indices)
                    };
                push_terrain_quad(
                    vertices,
                    indices,
                    [[x0, y, z1], [x1, y, z1], [x1, y, z0], [x0, y, z0]],
                    [
                        [0.0, (depth * step) as f32],
                        [(width * step) as f32, (depth * step) as f32],
                        [(width * step) as f32, 0.0],
                        [0.0, 0.0],
                    ],
                    cell.top_tile,
                    cell.light_level as f32,
                    [1.0; 4],
                    region_coord,
                );
                gx += width;
            }
        }

        // Add vertical skirts wherever a coarse cell is higher than its
        // neighbor. Adjacent equal skirts are merged along their tangent axis
        // so a flat 16x16 surface remains five quads instead of 65.
        let side_at = |face_idx: usize, gx: usize, gz: usize| {
            let cell = cells[gz * grid_width + gx]?;
            let neighbor = match face_idx {
                0 => (gx as i32, gz as i32 + 1),
                1 => (gx as i32, gz as i32 - 1),
                2 => (gx as i32 - 1, gz as i32),
                _ => (gx as i32 + 1, gz as i32),
            };
            let neighbor_height = if neighbor.0 >= 0
                && neighbor.0 < grid_width as i32
                && neighbor.1 >= 0
                && neighbor.1 < grid_depth as i32
            {
                cells[neighbor.1 as usize * grid_width + neighbor.0 as usize]
                    .map(|neighbor| neighbor.height)
                    .unwrap_or(-1)
            } else {
                -1
            };
            (neighbor_height < cell.height).then_some((cell, neighbor_height))
        };

        for face_idx in 0..4 {
            let (line_count, line_length) = if face_idx < 2 {
                (grid_depth, grid_width)
            } else {
                (grid_width, grid_depth)
            };
            for line in 0..line_count {
                let mut cursor = 0;
                while cursor < line_length {
                    let (gx, gz) = if face_idx < 2 {
                        (cursor, line)
                    } else {
                        (line, cursor)
                    };
                    let Some(side) = side_at(face_idx, gx, gz) else {
                        cursor += 1;
                        continue;
                    };
                    let mut run = 1;
                    while cursor + run < line_length {
                        let (next_gx, next_gz) = if face_idx < 2 {
                            (cursor + run, line)
                        } else {
                            (line, cursor + run)
                        };
                        if side_at(face_idx, next_gx, next_gz) != Some(side) {
                            break;
                        }
                        run += 1;
                    }

                    let (cell, neighbor_height) = side;
                    let top = cell.height as f32 + 1.0;
                    let bottom = neighbor_height as f32 + 1.0;
                    let run_blocks = (run * step) as f32;
                    let x0 = world_x0 + (gx * step) as f32;
                    let z0 = world_z0 + (gz * step) as f32;
                    let positions = match face_idx {
                        0 => {
                            let x1 = x0 + run_blocks;
                            let z1 = z0 + step as f32;
                            [
                                [x0, bottom, z1],
                                [x1, bottom, z1],
                                [x1, top, z1],
                                [x0, top, z1],
                            ]
                        }
                        1 => {
                            let x1 = x0 + run_blocks;
                            [
                                [x1, bottom, z0],
                                [x0, bottom, z0],
                                [x0, top, z0],
                                [x1, top, z0],
                            ]
                        }
                        2 => {
                            let z1 = z0 + run_blocks;
                            [
                                [x0, bottom, z0],
                                [x0, bottom, z1],
                                [x0, top, z1],
                                [x0, top, z0],
                            ]
                        }
                        _ => {
                            let x1 = x0 + step as f32;
                            let z1 = z0 + run_blocks;
                            [
                                [x1, bottom, z1],
                                [x1, bottom, z0],
                                [x1, top, z0],
                                [x1, top, z1],
                            ]
                        }
                    };
                    let side_tile = cell.block.get_face_tex_index(face_idx);
                    let (vertices, indices) =
                        if cell.block.properties().render_type == RenderType::Translucent {
                            (&mut trans_vertices, &mut trans_indices)
                        } else {
                            (&mut opaque_vertices, &mut opaque_indices)
                        };
                    push_terrain_quad(
                        vertices,
                        indices,
                        positions,
                        [
                            [0.0, top - bottom],
                            [run_blocks, top - bottom],
                            [run_blocks, 0.0],
                            [0.0, 0.0],
                        ],
                        side_tile,
                        cell.light_level as f32 + 256.0,
                        [1.0; 4],
                        region_coord,
                    );
                    cursor += run;
                }
            }
        }

        ChunkLodMeshData::from_parts(
            opaque_vertices,
            opaque_indices,
            trans_vertices,
            trans_indices,
            region_coord,
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
            | BlockType::SnowLayer
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
            | BlockType::Furnace
            | BlockType::EnchantingTable
            | BlockType::BrewingStand
            | BlockType::Anvil
            | BlockType::Netherrack
            | BlockType::Glowstone
            | BlockType::EndStone
            | BlockType::EndPortalFrame
            | BlockType::EndPortalFrameFilled
            | BlockType::Purpur
            | BlockType::DragonEgg
            | BlockType::NetherBrick
            | BlockType::EndCityChest => ToolType::Pickaxe,
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
            BlockType::BrewingStand
            | BlockType::Anvil
            | BlockType::Netherrack
            | BlockType::Glowstone
            | BlockType::EndStone
            | BlockType::Purpur
            | BlockType::NetherBrick
            | BlockType::EndCityChest => Some(ToolMaterial::Stone),
            BlockType::EnchantingTable => Some(ToolMaterial::Diamond),
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
    use glam::Vec3;

    #[test]
    fn end_portal_frames_use_distinct_top_side_and_filled_top_tiles() {
        for block in [BlockType::EndPortalFrame, BlockType::EndPortalFrameFilled] {
            assert_eq!(block.get_face_tex_index(0), (9, 4));
            assert_eq!(block.get_face_tex_index(5), (9, 4));
        }
        assert_eq!(BlockType::EndPortalFrame.get_face_tex_index(4), (15, 15));
        assert_eq!(
            BlockType::EndPortalFrameFilled.get_face_tex_index(4),
            (6, 4)
        );
    }

    fn empty_test_chunk() -> Chunk {
        let mut chunk = Chunk::new(0, 0);
        for x in 0..CHUNK_WIDTH {
            for y in 0..CHUNK_HEIGHT {
                for z in 0..CHUNK_DEPTH {
                    chunk.set_block_local(x, y, z, BlockType::Air);
                    chunk.set_sky_light(x, y, z, 15);
                    chunk.set_block_light(x, y, z, 0);
                    chunk.set_fluid_level(x, y, z, 0);
                }
            }
            for z in 0..CHUNK_DEPTH {
                chunk.heightmap[x][z] = 0;
            }
        }
        chunk
    }

    #[test]
    fn l0_volume_respects_section_extent_bounds() {
        let origin = [0, 32, 0];
        let (ov, _, tv, _) = Chunk::mesh_l0_volume(origin, [16, 16, 16], |x, y, z| MeshVoxel {
            block: if x == 0 && y == 32 && z == 0 {
                BlockType::Stone
            } else {
                BlockType::Air
            },
            ..MeshVoxel::default()
        });
        assert!(!ov.is_empty() || !tv.is_empty());
    }

    #[test]
    fn section_bundle_builds_distinct_bounded_lods_and_preserves_identity() {
        let mut chunk = empty_test_chunk();
        let key = SectionKey::new(0, 1, 0);
        for cell_y in (0..SECTION_SIZE).step_by(2) {
            for cell_z in (0..CHUNK_DEPTH).step_by(2) {
                for cell_x in (0..CHUNK_WIDTH).step_by(2) {
                    if (cell_x / 2 + cell_y / 2 + cell_z / 2) & 1 == 0 {
                        chunk.set_block_local(
                            cell_x,
                            key.min_world_y() as usize + cell_y,
                            cell_z,
                            BlockType::Stone,
                        );
                    }
                }
            }
        }

        let identity = SectionIdentity::new(key, 41, 7);
        let bundle = chunk.generate_section_mesh_bundle(
            key,
            identity.revision,
            identity.lifetime,
            |x, y, z| test_chunk_lookup(&chunk, x, y, z),
        );

        assert_eq!(bundle.identity, identity);
        assert_ne!(bundle.levels[0].opaque, bundle.levels[1].opaque);
        assert_ne!(bundle.levels[1].opaque, bundle.levels[2].opaque);
        assert!(
            bundle.levels[2].opaque.indices.len() < bundle.levels[1].opaque.indices.len(),
            "L2 must submit genuinely coarser geometry"
        );

        let section_min = Vec3::new(0.0, key.min_world_y() as f32, 0.0);
        let section_max = section_min + Vec3::splat(SECTION_SIZE as f32);
        for level in &bundle.levels {
            let bounds = level.bounds().expect("fixture produces opaque geometry");
            assert!(bounds.min.cmpge(section_min).all());
            assert!(bounds.max.cmple(section_max).all());
        }
        assert_eq!(
            bundle.bounds,
            bundle
                .levels
                .iter()
                .filter_map(ChunkLodMeshData::bounds)
                .reduce(|a, b| a.union(b))
        );
    }

    #[test]
    fn section_halo_occludes_boundary_neighbor() {
        let origin = [0, 0, 0];
        let voxel = |x: i32, _y: i32, _z: i32| MeshVoxel {
            block: if x <= 0 {
                BlockType::Stone
            } else {
                BlockType::Air
            },
            ..MeshVoxel::default()
        };
        let (_, isolated_i, _, _) = Chunk::mesh_l0_volume(origin, [1, 1, 1], |_, _, _| MeshVoxel {
            block: BlockType::Stone,
            ..MeshVoxel::default()
        });
        let (_, occluded_i, _, _) = Chunk::mesh_l0_volume(origin, [1, 1, 1], voxel);
        assert!(occluded_i.len() > isolated_i.len());
    }

    #[test]
    fn mesh_voxel_render_inputs_flow_through_core() {
        let v = MeshVoxel {
            block: BlockType::Torch,
            state: 3,
            sky: 7,
            block_light: 11,
            raw_fluid: 0,
        };
        let (a, _, _, _) = Chunk::mesh_l0_volume([0, 0, 0], [1, 1, 1], move |_, _, _| v);
        assert!(!a.is_empty());
    }

    #[test]
    fn legacy_generate_mesh_matches_l0_adapter_fixture() {
        let chunk = empty_test_chunk();
        let lookup = |x, y, z| test_chunk_lookup(&chunk, x, y, z);
        let legacy = chunk.generate_mesh(lookup);
        let origin = [0, 0, 0];
        let core = Chunk::mesh_l0_volume(
            origin,
            [CHUNK_WIDTH, CHUNK_HEIGHT, CHUNK_DEPTH],
            |x, y, z| {
                let (block, sky, bl, level, falling) = lookup(x, y, z);
                MeshVoxel {
                    block,
                    state: chunk.get_block_state(x, y, z),
                    sky,
                    block_light: bl,
                    raw_fluid: level | if falling { 8 } else { 0 },
                }
            },
        );
        assert_eq!(legacy, core);
    }

    fn test_chunk_lookup(
        chunk: &Chunk,
        world_x: i32,
        world_y: i32,
        world_z: i32,
    ) -> (BlockType, u8, u8, u8, bool) {
        if world_x < 0
            || world_x >= CHUNK_WIDTH as i32
            || world_y < 0
            || world_y >= CHUNK_HEIGHT as i32
            || world_z < 0
            || world_z >= CHUNK_DEPTH as i32
        {
            return (BlockType::Air, 15, 0, 0, false);
        }
        let x = world_x as usize;
        let y = world_y as usize;
        let z = world_z as usize;
        let fluid = chunk.get_fluid_level(x, y, z);
        (
            chunk.get_block_local(x, y, z),
            chunk.get_sky_light(x, y, z),
            chunk.get_block_light(x, y, z),
            fluid & 0x07,
            fluid & 0x08 != 0,
        )
    }

    fn single_torch_mesh(
        block: BlockType,
        sky_light: u8,
        block_light: u8,
    ) -> (Vec<TerrainVertex>, Vec<u32>, Vec<TerrainVertex>, Vec<u32>) {
        let mut chunk = empty_test_chunk();
        chunk.set_block_local(8, 1, 8, block);
        chunk.set_sky_light(8, 1, 8, sky_light);
        chunk.set_block_light(8, 1, 8, block_light);
        chunk.heightmap[8][8] = 1;
        chunk.generate_mesh(|x, y, z| test_chunk_lookup(&chunk, x, y, z))
    }

    #[test]
    fn block_type_wire_roundtrip_covers_all_variants() {
        // Walk every discriminant in `0..=EndCityChest` and confirm the
        // wire helpers are exact inverses. This also guards against future
        // reordering of the enum: any renumbering would surface here.
        for raw in 0..=BlockType::EndCityChest as u32 {
            let block = BlockType::from_wire(raw).expect("valid discriminant");
            assert_eq!(block.to_wire(), raw, "to_wire/from_wire mismatch");
        }
    }

    #[test]
    fn block_type_from_wire_rejects_unknown_values() {
        assert!(BlockType::from_wire(BlockType::EndCityChest as u32 + 1).is_none());
        assert!(BlockType::from_wire(u32::MAX).is_none());
    }

    #[test]
    fn world_seed_changes_generated_terrain() {
        let first = Chunk::new_with_seed(0, 0, 1);
        let same = Chunk::new_with_seed(0, 0, 1);
        let different = Chunk::new_with_seed(0, 0, 2);
        assert_eq!(first.heightmap, same.heightmap);
        assert_ne!(first.heightmap, different.heightmap);
    }
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
                    chunk.set_block_local(x, y, z, BlockType::Air);
                }
            }
            for z in 0..CHUNK_DEPTH {
                chunk.heightmap[x][z] = 0;
            }
        }
        chunk.set_block_local(8, 1, 8, BlockType::Stone);
        chunk.heightmap[8][8] = 1;

        let empty_lookup = |_: i32, _: i32, _: i32| (BlockType::Air, 15, 0, 0, false);
        let (vertices, indices, _, _) = chunk.generate_mesh(empty_lookup);
        assert_eq!(vertices.len(), 24);
        assert_eq!(indices.len(), 36);
        assert!(vertices.iter().all(|vertex| vertex.ao() == 1.0));

        chunk.set_block_local(7, 2, 8, BlockType::Stone);
        chunk.set_block_local(8, 2, 9, BlockType::Stone);
        let (vertices, _, _, _) = chunk.generate_mesh(empty_lookup);
        assert!(vertices.iter().any(|vertex| vertex.ao() < 1.0));
    }

    #[test]
    fn greedy_meshing_merges_equal_faces_and_repeats_the_atlas_tile() {
        let mut chunk = empty_test_chunk();
        for x in 8..10 {
            for z in 8..10 {
                chunk.set_block_local(x, 1, z, BlockType::Stone);
                chunk.heightmap[x][z] = 1;
            }
        }

        let lookup = |x, y, z| test_chunk_lookup(&chunk, x, y, z);
        let (vertices, indices, transparent_vertices, transparent_indices) =
            chunk.generate_mesh(lookup);

        // A 2x1x2 cuboid has six exterior rectangles after greedy merging.
        assert_eq!(vertices.len(), 6 * 4);
        assert_eq!(indices.len(), 6 * 6);
        assert!(transparent_vertices.is_empty());
        assert!(transparent_indices.is_empty());
        assert_eq!(
            vertices
                .iter()
                .flat_map(|vertex| vertex.local_uv_f32())
                .fold(0.0f32, f32::max),
            2.0
        );
    }

    #[test]
    fn greedy_meshing_does_not_merge_different_light_or_material() {
        let mut light_chunk = empty_test_chunk();
        for x in 8..10 {
            light_chunk.set_block_local(x, 1, 8, BlockType::Stone);
            light_chunk.heightmap[x][8] = 1;
        }
        light_chunk.set_sky_light(9, 2, 8, 14);
        let (light_vertices, _, _, _) =
            light_chunk.generate_mesh(|x, y, z| test_chunk_lookup(&light_chunk, x, y, z));
        let light_top_quads = light_vertices
            .chunks_exact(4)
            .filter(|quad| quad.iter().all(|vertex| vertex.local_position()[1] == 2.0))
            .count();
        assert_eq!(light_top_quads, 2);

        let mut material_chunk = empty_test_chunk();
        material_chunk.set_block_local(8, 1, 8, BlockType::Stone);
        material_chunk.set_block_local(9, 1, 8, BlockType::Dirt);
        material_chunk.heightmap[8][8] = 1;
        material_chunk.heightmap[9][8] = 1;
        let (material_vertices, _, _, _) =
            material_chunk.generate_mesh(|x, y, z| test_chunk_lookup(&material_chunk, x, y, z));
        let material_top_quads = material_vertices
            .chunks_exact(4)
            .filter(|quad| quad.iter().all(|vertex| vertex.local_position()[1] == 2.0))
            .count();
        assert_eq!(material_top_quads, 2);
    }

    #[test]
    fn surface_lod_merges_flat_skirts_and_coarsens_varied_terrain() {
        let mut flat = empty_test_chunk();
        for x in 0..CHUNK_WIDTH {
            for z in 0..CHUNK_DEPTH {
                flat.set_block_local(x, 1, z, BlockType::Stone);
                flat.heightmap[x][z] = 1;
            }
        }
        let flat_l1 = flat.generate_surface_mesh(|x, y, z| test_chunk_lookup(&flat, x, y, z), 1);
        let flat_l2 = flat.generate_surface_mesh(|x, y, z| test_chunk_lookup(&flat, x, y, z), 4);
        // One top plus four merged boundary skirts at either resolution.
        assert_eq!(flat_l1.opaque.indices.len(), 5 * 6);
        assert_eq!(flat_l2.opaque.indices.len(), 5 * 6);
        let bounds = flat_l2.opaque.bounds.expect("flat LOD should have bounds");
        assert_eq!(bounds.min, Vec3::new(0.0, 0.0, 0.0));
        assert_eq!(bounds.max, Vec3::new(16.0, 2.0, 16.0));

        let mut varied = empty_test_chunk();
        for x in 0..CHUNK_WIDTH {
            for z in 0..CHUNK_DEPTH {
                let height = 1 + (x + z) % 2;
                for y in 1..=height {
                    varied.set_block_local(x, y, z, BlockType::Stone);
                }
                varied.heightmap[x][z] = height as u16;
            }
        }
        let varied_l1 =
            varied.generate_surface_mesh(|x, y, z| test_chunk_lookup(&varied, x, y, z), 1);
        let varied_l2 =
            varied.generate_surface_mesh(|x, y, z| test_chunk_lookup(&varied, x, y, z), 4);
        assert!(
            varied_l2.opaque.indices.len() < varied_l1.opaque.indices.len(),
            "coarse LOD should submit fewer indices"
        );
    }

    #[test]
    fn snow_layer_mesh_is_one_eighth_of_a_block_high() {
        let mut chunk = Chunk::new(0, 0);
        for x in 0..CHUNK_WIDTH {
            for y in 0..CHUNK_HEIGHT {
                for z in 0..CHUNK_DEPTH {
                    chunk.set_block_local(x, y, z, BlockType::Air);
                }
            }
            for z in 0..CHUNK_DEPTH {
                chunk.heightmap[x][z] = 0;
            }
        }
        chunk.set_block_local(8, 1, 8, BlockType::SnowLayer);
        chunk.heightmap[8][8] = 1;
        let lookup = |_: i32, _: i32, _: i32| (BlockType::Air, 15, 0, 0, false);
        let (vertices, _, _, _) = chunk.generate_mesh(lookup);
        let max_y = vertices
            .iter()
            .map(|vertex| vertex.local_position()[1])
            .fold(f32::NEG_INFINITY, f32::max);
        assert!((max_y - 1.125).abs() < f32::EPSILON);
    }

    #[test]
    fn cross_model_blocks_generate_x_mesh() {
        let mut chunk = Chunk::new(0, 0);
        for x in 0..CHUNK_WIDTH {
            for y in 0..CHUNK_HEIGHT {
                for z in 0..CHUNK_DEPTH {
                    chunk.set_block_local(x, y, z, BlockType::Air);
                }
            }
            for z in 0..CHUNK_DEPTH {
                chunk.heightmap[x][z] = 0;
            }
        }
        chunk.set_block_local(8, 1, 8, BlockType::Poppy);
        chunk.heightmap[8][8] = 1;
        let lookup = |_: i32, _: i32, _: i32| (BlockType::Air, 15, 0, 0, false);
        let (vertices, indices, _, _) = chunk.generate_mesh(lookup);
        // 2 planes * 2 sides = 4 quads = 16 vertices, 24 indices
        assert_eq!(vertices.len(), 16);
        assert_eq!(indices.len(), 24);
    }

    #[test]
    fn torch_mesh_has_minecraft_bounds_and_six_outward_faces() {
        let (vertices, indices, transparent_vertices, transparent_indices) =
            single_torch_mesh(BlockType::Torch, 15, 14);

        assert_eq!(vertices.len(), 24);
        assert_eq!(indices.len(), 36);
        assert!(transparent_vertices.is_empty());
        assert!(transparent_indices.is_empty());

        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        for vertex in &vertices {
            let pos = vertex.local_position();
            for axis in 0..3 {
                min[axis] = min[axis].min(pos[axis]);
                max[axis] = max[axis].max(pos[axis]);
            }
        }
        assert_eq!(
            min,
            [8.0 + TORCH_MIN, 1.0, 8.0 + TORCH_MIN],
            "torch must start at the centered 7/16 inset"
        );
        for face_idx in 0..6 {
            let expected = BLOCK_FACES[face_idx].0.map(|component| component as f32);
            for triangle in indices[face_idx * 6..face_idx * 6 + 6].chunks_exact(3) {
                let normal = triangle_normal(
                    vertices[triangle[0] as usize].local_position(),
                    vertices[triangle[1] as usize].local_position(),
                    vertices[triangle[2] as usize].local_position(),
                );
                let dot =
                    normal[0] * expected[0] + normal[1] * expected[1] + normal[2] * expected[2];
                assert!(
                    dot > 0.0,
                    "torch face {face_idx} triangle {triangle:?} must wind outward"
                );
            }
        }
    }

    #[test]
    fn door_mesh_generation_bounds_and_quad_count() {
        let mut chunk = Chunk::new(0, 0);
        for x in 0..16 {
            for y in 0..256 {
                for z in 0..16 {
                    chunk.set_block_local(x, y, z, BlockType::Air);
                }
            }
        }
        chunk.heightmap[0][0] = 64;
        chunk.set_block_local(0, 64, 0, BlockType::OakDoor);
        let state = BlockState {
            facing: Direction::North,
            is_top: false,
            is_right_hinge: false,
            is_open: false,
        };
        chunk.set_block_state(0, 64, 0, state.encode());

        let (opaque_v, opaque_i, trans_v, trans_i) =
            chunk.generate_mesh(|_, _, _| (BlockType::Air, 0, 0, 0, false));

        assert_eq!(opaque_v.len(), 24);
        assert_eq!(opaque_i.len(), 36);
        assert!(trans_v.is_empty());
        assert!(trans_i.is_empty());

        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        for v in &opaque_v {
            let pos = v.local_position();
            for a in 0..3 {
                min[a] = min[a].min(pos[a]);
                max[a] = max[a].max(pos[a]);
            }
        }
        assert!((min[0] - 0.0).abs() < 1e-4);
        assert!((max[0] - 1.0).abs() < 1e-4);
        assert!((min[1] - 64.0).abs() < 1e-4);
        assert!((max[1] - 65.0).abs() < 1e-4);
        assert!((min[2] - 0.0).abs() < 1e-4);
        assert!((max[2] - 0.1875).abs() < 1e-4);
    }

    #[test]
    fn trapdoor_mesh_generation_open_and_closed_bounds() {
        let mut chunk = Chunk::new(0, 0);
        for x in 0..16 {
            for y in 0..256 {
                for z in 0..16 {
                    chunk.set_block_local(x, y, z, BlockType::Air);
                }
            }
        }
        chunk.heightmap[0][0] = 64;
        chunk.set_block_local(0, 64, 0, BlockType::OakTrapdoor);
        let closed_state = BlockState {
            facing: Direction::North,
            is_top: false,
            is_right_hinge: false,
            is_open: false,
        };
        chunk.set_block_state(0, 64, 0, closed_state.encode());

        let (opaque_v, opaque_i, _, _) =
            chunk.generate_mesh(|_, _, _| (BlockType::Air, 0, 0, 0, false));

        assert_eq!(opaque_v.len(), 24);
        assert_eq!(opaque_i.len(), 36);

        let mut max_y = f32::NEG_INFINITY;
        for v in &opaque_v {
            max_y = max_y.max(v.local_position()[1]);
        }
        assert!((max_y - 64.1875).abs() < 1e-4);

        // Open trapdoor
        let open_state = BlockState {
            facing: Direction::North,
            is_top: false,
            is_right_hinge: false,
            is_open: true,
        };
        chunk.set_block_state(0, 64, 0, open_state.encode());
        let (opaque_v2, _, _, _) = chunk.generate_mesh(|_, _, _| (BlockType::Air, 0, 0, 0, false));
        let mut max_y_open = f32::NEG_INFINITY;
        for v in &opaque_v2 {
            max_y_open = max_y_open.max(v.local_position()[1]);
        }
        assert!((max_y_open - 65.0).abs() < 1e-4);
    }

    #[test]
    fn torch_mesh_uses_inset_face_uvs_inside_its_atlas_tile() {
        let (vertices, _, _, _) = single_torch_mesh(BlockType::Torch, 15, 14);
        let expected_rects = [
            TORCH_SIDE_UV,
            TORCH_SIDE_UV,
            TORCH_SIDE_UV,
            TORCH_SIDE_UV,
            TORCH_TOP_UV,
            TORCH_BOTTOM_UV,
        ];

        assert_eq!(
            TORCH_SIDE_UV,
            [6.5 / 16.0, 2.5 / 16.0, 8.5 / 16.0, 13.5 / 16.0]
        );
        assert_eq!(
            TORCH_TOP_UV,
            [6.5 / 16.0, 2.5 / 16.0, 8.5 / 16.0, 4.5 / 16.0]
        );
        assert_eq!(
            TORCH_BOTTOM_UV,
            [7.5 / 16.0, 13.5 / 16.0, 7.5 / 16.0, 13.5 / 16.0]
        );

        for (face_idx, quad) in vertices.chunks_exact(4).enumerate() {
            let rect = expected_rects[face_idx];
            let mut observed_min = [f32::INFINITY; 2];
            let mut observed_max = [f32::NEG_INFINITY; 2];
            for vertex in quad {
                assert_eq!(vertex.atlas_tile_u32(), (4, 2));
                let uv = vertex.local_uv_f32();
                assert!(
                    (0.0..1.0).contains(&uv[0]) && (0.0..1.0).contains(&uv[1]),
                    "torch UV must remain inside atlas tile (4, 2)"
                );
                observed_min[0] = observed_min[0].min(uv[0]);
                observed_min[1] = observed_min[1].min(uv[1]);
                observed_max[0] = observed_max[0].max(uv[0]);
                observed_max[1] = observed_max[1].max(uv[1]);
            }
            assert_eq!(observed_min, [rect[0], rect[1]]);
            assert_eq!(observed_max, [rect[2], rect[3]]);
        }
    }

    #[test]
    fn torch_mesh_uses_source_light_without_ao_or_face_shading() {
        let sky_light = 9;
        let block_light = 14;
        let expected_packed_light = sky_light as f32 + block_light as f32 * 16.0;
        let (vertices, _, _, _) = single_torch_mesh(BlockType::Torch, sky_light, block_light);

        assert!(vertices.iter().all(|vertex| vertex.ao() == 1.0));
        assert!(
            vertices
                .iter()
                .all(|vertex| vertex.light_level() == expected_packed_light),
            "every torch face must use the source cell light without a face multiplier"
        );
    }

    #[test]
    fn redstone_torch_variants_use_thin_torch_mesh_and_redstone_tile() {
        let expected_rects = [
            TORCH_SIDE_UV,
            TORCH_SIDE_UV,
            TORCH_SIDE_UV,
            TORCH_SIDE_UV,
            TORCH_TOP_UV,
            TORCH_BOTTOM_UV,
        ];

        for block in [BlockType::RedstoneTorch, BlockType::RedstoneTorchOff] {
            let (vertices, indices, transparent_vertices, transparent_indices) =
                single_torch_mesh(block, 15, block.properties().light_emission);

            assert_eq!(vertices.len(), 24, "{block:?} must have six quads");
            assert_eq!(indices.len(), 36, "{block:?} must have twelve triangles");
            assert!(transparent_vertices.is_empty());
            assert!(transparent_indices.is_empty());

            let mut min = [f32::INFINITY; 3];
            let mut max = [f32::NEG_INFINITY; 3];
            for vertex in &vertices {
                let pos = vertex.local_position();
                for axis in 0..3 {
                    min[axis] = min[axis].min(pos[axis]);
                    max[axis] = max[axis].max(pos[axis]);
                }
            }
            assert_eq!(min, [8.0 + TORCH_MIN, 1.0, 8.0 + TORCH_MIN]);
            assert_eq!(max, [8.0 + TORCH_MAX, 1.0 + TORCH_HEIGHT, 8.0 + TORCH_MAX]);
            assert_eq!(
                [max[0] - min[0], max[1] - min[1], max[2] - min[2]],
                [2.0 / 16.0, 10.0 / 16.0, 2.0 / 16.0],
                "{block:?} must not fall back to full-cube geometry"
            );

            for (face_idx, quad) in vertices.chunks_exact(4).enumerate() {
                let rect = expected_rects[face_idx];
                let mut observed_min = [f32::INFINITY; 2];
                let mut observed_max = [f32::NEG_INFINITY; 2];
                for vertex in quad {
                    assert_eq!(
                        vertex.atlas_tile_u32(),
                        (
                            REDSTONE_TORCH_ATLAS_TILE.0 as u32,
                            REDSTONE_TORCH_ATLAS_TILE.1 as u32,
                        )
                    );
                    let uv = vertex.local_uv_f32();
                    observed_min[0] = observed_min[0].min(uv[0]);
                    observed_min[1] = observed_min[1].min(uv[1]);
                    observed_max[0] = observed_max[0].max(uv[0]);
                    observed_max[1] = observed_max[1].max(uv[1]);
                }
                assert_eq!(observed_min, [rect[0], rect[1]]);
                assert_eq!(observed_max, [rect[2], rect[3]]);
            }
        }
    }

    #[test]
    fn torch_properties_and_floor_support_semantics_are_preserved() {
        let properties = BlockType::Torch.properties();
        assert_eq!(properties.render_type, RenderType::Cutout);
        assert!(!properties.is_solid);
        assert!(!properties.is_passable);
        assert_eq!(properties.light_emission, 14);
        assert!(BlockType::Torch.can_stay_on(BlockType::Stone));
        assert!(!BlockType::Torch.can_stay_on(BlockType::Air));

        let mut manager = crate::chunk_manager::ChunkManager::new(2);
        manager.chunks.insert((0, 0), empty_test_chunk());
        manager.set_block(8, 64, 8, BlockType::Stone);
        manager.set_block(8, 65, 8, BlockType::Torch);

        let mut dirty = HashSet::new();
        crate::lighting::update_block_light_after_placed(
            &mut manager,
            8,
            65,
            8,
            properties.light_emission,
            &mut dirty,
        );
        assert_eq!(manager.get_block_light(8, 65, 8), 14);
        assert_eq!(manager.get_block_light(9, 65, 8), 13);

        manager.set_block(8, 64, 8, BlockType::Air);
        let mut broken = Vec::new();
        manager.check_and_break_unsupported_above(8, 64, 8, &mut dirty, |position, block| {
            broken.push((position, block));
        });

        assert_eq!(manager.get_block(8, 65, 8), BlockType::Air);
        assert_eq!(broken, vec![((8, 65, 8), BlockType::Torch)]);
        assert_eq!(manager.get_block_light(8, 65, 8), 0);
        assert_eq!(manager.get_block_light(9, 65, 8), 0);
    }

    #[test]
    fn weather_blocks_have_expected_collision_and_light() {
        assert!(BlockType::SnowLayer.properties().is_passable);
        assert!(!BlockType::SnowLayer.properties().is_solid);
        assert_eq!(BlockType::Fire.properties().light_emission, 15);
        assert!(BlockType::Fire.properties().is_passable);
        assert_eq!(BlockType::from_u8(74), BlockType::Fire);
        assert_eq!(BlockType::from_u8(75), BlockType::SnowLayer);
        for id in 0..=BlockType::EndCityChest as u8 {
            assert_eq!(BlockType::from_u8(id) as u8, id);
        }
        assert_eq!(BlockType::from_u8(255), BlockType::Air);
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
                for y in 0..CHUNK_HEIGHT {
                    if chunk.get_block_local(x, y, z) == BlockType::CoalOre {
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
                                if chunk.get_block_local(nx as usize, ny as usize, nz as usize)
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
                    if chunk.get_block_local(x, base_height, z) == BlockType::Air {
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

    #[test]
    fn test_plant_support_requirements() {
        assert!(BlockType::Dandelion.can_stay_on(BlockType::Grass));
        assert!(BlockType::Dandelion.can_stay_on(BlockType::Dirt));
        assert!(!BlockType::Dandelion.can_stay_on(BlockType::Air));
        assert!(!BlockType::Dandelion.can_stay_on(BlockType::Stone));
        assert!(!BlockType::Dandelion.can_stay_on(BlockType::OakPlanks));

        assert!(BlockType::Poppy.can_stay_on(BlockType::Grass));
        assert!(!BlockType::Poppy.can_stay_on(BlockType::Sand));

        assert!(BlockType::TallGrass.can_stay_on(BlockType::Grass));
        assert!(!BlockType::TallGrass.can_stay_on(BlockType::Stone));

        assert!(BlockType::SugarCane.can_stay_on(BlockType::Sand));
        assert!(BlockType::SugarCane.can_stay_on(BlockType::SugarCane));
        assert!(!BlockType::SugarCane.can_stay_on(BlockType::Air));

        assert!(BlockType::Cactus.can_stay_on(BlockType::Sand));
        assert!(BlockType::Cactus.can_stay_on(BlockType::Cactus));
        assert!(!BlockType::Cactus.can_stay_on(BlockType::Dirt));
    }

    #[test]
    fn contextual_plant_support_enforces_water_and_lateral_clearance() {
        let position = (8, 100, 8);
        let mut blocks = std::collections::HashMap::new();
        blocks.insert((8, 99, 8), BlockType::Sand);
        let lookup = |x, y, z| Some(*blocks.get(&(x, y, z)).unwrap_or(&BlockType::Air));

        assert_eq!(
            BlockType::SugarCane.support_status_at(position, lookup),
            BlockSupportStatus::Unsupported
        );

        blocks.insert((9, 99, 8), BlockType::Water);
        assert_eq!(
            BlockType::SugarCane.support_status_at(position, |x, y, z| {
                Some(*blocks.get(&(x, y, z)).unwrap_or(&BlockType::Air))
            }),
            BlockSupportStatus::Supported
        );

        blocks.insert((8, 99, 8), BlockType::SugarCane);
        blocks.remove(&(9, 99, 8));
        assert_eq!(
            BlockType::SugarCane.support_status_at(position, |x, y, z| {
                Some(*blocks.get(&(x, y, z)).unwrap_or(&BlockType::Air))
            }),
            BlockSupportStatus::Supported,
            "upper cane inherits support from the cane below"
        );

        blocks.insert((8, 99, 8), BlockType::Sand);
        assert_eq!(
            BlockType::Cactus.support_status_at(position, |x, y, z| {
                Some(*blocks.get(&(x, y, z)).unwrap_or(&BlockType::Air))
            }),
            BlockSupportStatus::Supported
        );
        blocks.insert((9, 100, 8), BlockType::Stone);
        assert_eq!(
            BlockType::Cactus.support_status_at(position, |x, y, z| {
                Some(*blocks.get(&(x, y, z)).unwrap_or(&BlockType::Air))
            }),
            BlockSupportStatus::Unsupported
        );
        blocks.insert((9, 100, 8), BlockType::Lava);
        assert_eq!(
            BlockType::Cactus.support_status_at(position, |x, y, z| {
                Some(*blocks.get(&(x, y, z)).unwrap_or(&BlockType::Air))
            }),
            BlockSupportStatus::Unsupported,
            "lava is a forbidden lateral cactus neighbor despite being non-solid"
        );
    }

    #[test]
    fn contextual_plant_support_reports_unknown_for_missing_neighbor_chunks() {
        let position = (15, 100, 8);
        let lookup = |x, y, z| {
            if x >= 16 {
                None
            } else if (x, y, z) == (15, 99, 8) {
                Some(BlockType::Sand)
            } else {
                Some(BlockType::Air)
            }
        };

        assert_eq!(
            BlockType::SugarCane.support_status_at(position, lookup),
            BlockSupportStatus::Unknown
        );
        assert_eq!(
            BlockType::Cactus.support_status_at(position, lookup),
            BlockSupportStatus::Unknown
        );
    }

    #[test]
    fn block_state_encoding_roundtrip() {
        assert_eq!(BlockState::default().encode(), 0);

        let directions = [
            Direction::North,
            Direction::South,
            Direction::West,
            Direction::East,
        ];
        for facing in directions {
            for is_top in [false, true] {
                for is_right_hinge in [false, true] {
                    for is_open in [false, true] {
                        let state = BlockState {
                            facing,
                            is_top,
                            is_right_hinge,
                            is_open,
                        };
                        let encoded = state.encode();
                        let decoded = BlockState::decode(encoded);
                        assert_eq!(decoded, state);

                        // Verify reserved bits (bits 5, 6, 7) are ignored
                        let decoded_with_reserved = BlockState::decode(encoded | 0b1110_0000);
                        assert_eq!(decoded_with_reserved, state);
                    }
                }
            }
        }
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
    fn section_safe_point_immediately_demotes_uniform_light_storage() {
        let mut section = ChunkSection::empty_dark();
        section.light.set_sky(11, 9);
        section.light.set_sky(11, 0);
        let allocated = section.memory_usage();
        assert!(section.compact_if_worthwhile());
        assert_eq!(section.light.get_sky(11), 0);
        assert!(section.memory_usage() < allocated);
    }

    #[test]
    fn chunk_memory_usage_tracks_section_promotion_and_demotion() {
        let mut chunk = Chunk {
            chunk_x: 0,
            chunk_z: 0,
            sections: (0..SECTION_COUNT)
                .map(|_| ChunkSection::empty_dark())
                .collect(),
            heightmap: Box::new([[0; CHUNK_DEPTH]; CHUNK_WIDTH]),
            torch_positions: Vec::new(),
            redstone_positions: Vec::new(),
            block_entities: std::collections::HashMap::new(),
        };
        let empty_bytes = chunk.memory_usage();
        chunk.set_block_local(0, 0, 0, BlockType::Stone);
        let promoted_bytes = chunk.memory_usage();
        assert!(promoted_bytes > empty_bytes);

        chunk.set_block_local(0, 0, 0, BlockType::Air);
        assert!(chunk.sections[0].compact_if_worthwhile());
        assert!(chunk.memory_usage() < promoted_bytes);
    }

    #[test]
    fn cactus_mesh_generation_bounds_and_quad_count() {
        let (opaque_v, opaque_i, trans_v, trans_i) = single_torch_mesh(BlockType::Cactus, 15, 0);
        assert!(trans_v.is_empty() && trans_i.is_empty());
        // 6 faces * 4 vertices = 24 vertices, 36 indices
        assert_eq!(opaque_v.len(), 24);
        assert_eq!(opaque_i.len(), 36);

        let min_x = opaque_v
            .iter()
            .map(|v| v.pos[0] as f32 / 32.0)
            .fold(f32::INFINITY, f32::min);
        let max_x = opaque_v
            .iter()
            .map(|v| v.pos[0] as f32 / 32.0)
            .fold(f32::NEG_INFINITY, f32::max);
        let min_z = opaque_v
            .iter()
            .map(|v| v.pos[2] as f32 / 32.0)
            .fold(f32::INFINITY, f32::min);
        let max_z = opaque_v
            .iter()
            .map(|v| v.pos[2] as f32 / 32.0)
            .fold(f32::NEG_INFINITY, f32::max);

        // Cactus placed at (8, 1, 8) -> origin = (8.0, 1.0, 8.0)
        // Inset by 1/16th: min = 8.0625, max = 8.9375
        assert!((min_x - (8.0 + 1.0 / 16.0)).abs() < 1e-4);
        assert!((max_x - (8.0 + 15.0 / 16.0)).abs() < 1e-4);
        assert!((min_z - (8.0 + 1.0 / 16.0)).abs() < 1e-4);
        assert!((max_z - (8.0 + 15.0 / 16.0)).abs() < 1e-4);
    }

    #[test]
    fn end_portal_frame_and_surface_use_lower_minecraft_heights() {
        for block in [BlockType::EndPortalFrame, BlockType::EndPortalFrameFilled] {
            let (opaque_v, opaque_i, trans_v, trans_i) = single_torch_mesh(block, 15, 0);
            assert!(trans_v.is_empty() && trans_i.is_empty());
            assert_eq!(opaque_v.len(), 24);
            assert_eq!(opaque_i.len(), 36);
            let max_y = opaque_v
                .iter()
                .map(|vertex| vertex.pos[1] as f32 / 32.0)
                .fold(f32::NEG_INFINITY, f32::max);
            assert!((max_y - (1.0 + END_PORTAL_FRAME_HEIGHT)).abs() < 1e-4);
        }

        let (opaque_v, opaque_i, trans_v, trans_i) =
            single_torch_mesh(BlockType::EndPortal, 15, 15);
        assert!(opaque_v.is_empty() && opaque_i.is_empty());
        assert_eq!(trans_v.len(), 8);
        assert_eq!(trans_i.len(), 12);
        assert!(trans_v.iter().all(|vertex| {
            (vertex.pos[1] as f32 / 32.0 - (1.0 + END_PORTAL_SURFACE_HEIGHT)).abs() < 1e-4
        }));
        assert!(END_PORTAL_SURFACE_HEIGHT < END_PORTAL_FRAME_HEIGHT);
    }

    #[test]
    fn chunk_block_entity_operations() {
        use crate::block_entity::{BlockEntity, ChestStub, FurnaceStub};

        let mut chunk = Chunk::new(0, 0);
        chunk.set_block_local(4, 10, 4, BlockType::Chest);

        let chest_entity = BlockEntity::Chest(ChestStub { custom_name: None });
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
        let furnace_entity = BlockEntity::Furnace(FurnaceStub { custom_name: None });
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
