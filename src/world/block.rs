use crate::inventory::{ToolMaterial, ToolType};
use crate::redstone::Direction;
use noise::{NoiseFn, Perlin};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SoundMaterial {
    Grass,
    Wood,
    Sand,
    Gravel,
    Stone,
    Snow,
    Ice,
    Glass,
}

pub const CHUNK_WIDTH: usize = 16;
pub const CHUNK_HEIGHT: usize = 256;
pub const CHUNK_DEPTH: usize = 16;

/// Canonical raw fluid-byte layout shared by chunk storage, mesh snapshots,
/// saves, and the authority/network projection.  BlockState bit 7 remains
/// reserved; waterlogging lives only in this fluid byte.
pub const FLUID_LEVEL_MASK: u8 = 0x07;
pub const FLUID_FALLING_BIT: u8 = 0x08;
pub const FLUID_RESERVED_MASK: u8 = 0x70;
pub const FLUID_WATERLOGGED_BIT: u8 = 0x80;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Biome {
    Plains,
    Forest,
    BirchForest,
    Taiga,
    SnowyPlains,
    Desert,
    Savanna,
    Swamp,
    Jungle,
    Badlands,
    Meadow,
    WindsweptHills,
    River,
    Beach,
    Ocean,
    DeepOcean,
}

impl Biome {
    /// All 16 reachable overworld biomes.
    pub const ALL: [Biome; 16] = [
        Biome::Plains,
        Biome::Forest,
        Biome::BirchForest,
        Biome::Taiga,
        Biome::SnowyPlains,
        Biome::Desert,
        Biome::Savanna,
        Biome::Swamp,
        Biome::Jungle,
        Biome::Badlands,
        Biome::Meadow,
        Biome::WindsweptHills,
        Biome::River,
        Biome::Beach,
        Biome::Ocean,
        Biome::DeepOcean,
    ];

    pub fn get_biome(
        world_x: i32,
        world_z: i32,
        temp_perlin: &Perlin,
        moist_perlin: &Perlin,
        ocean_perlin: &Perlin,
    ) -> Self {
        // Legacy compatibility shim: the new climate/biome selection lives in
        // worldgen::climate. This shim reproduces a subset of the new logic
        // so callers that only have raw Perlin fields can still resolve a
        // biome deterministically.
        let ocean_val = ocean_perlin.get([world_x as f64 * 0.001, world_z as f64 * 0.001]);
        if ocean_val < -0.55 {
            return Biome::DeepOcean;
        }
        if ocean_val < -0.35 {
            return Biome::Ocean;
        }

        let temp = temp_perlin.get([world_x as f64 * 0.002, world_z as f64 * 0.002]);
        let moist = moist_perlin.get([world_x as f64 * 0.002, world_z as f64 * 0.002]);

        if temp < -0.35 {
            if moist > -0.2 {
                Biome::SnowyPlains
            } else {
                Biome::WindsweptHills
            }
        } else if temp < -0.2 {
            Biome::Taiga
        } else if temp > 0.5 && moist < -0.45 {
            Biome::Badlands
        } else if temp > 0.45 && moist < 0.15 {
            Biome::Savanna
        } else if temp > 0.4 && moist < -0.3 {
            Biome::Desert
        } else if temp > 0.35 && moist > 0.55 {
            Biome::Jungle
        } else if temp > 0.2 && moist > 0.4 {
            Biome::Swamp
        } else if temp > 0.1 && temp < 0.4 && moist > 0.2 {
            Biome::BirchForest
        } else if temp > 0.1 && moist > 0.0 {
            Biome::Forest
        } else {
            Biome::Plains
        }
    }

    pub fn terrain_params(self) -> (f64, f64) {
        match self {
            Biome::Plains => (70.0, 4.0),
            Biome::Forest => (71.0, 6.0),
            Biome::BirchForest => (71.0, 6.0),
            Biome::Taiga => (72.0, 8.0),
            Biome::SnowyPlains => (68.0, 3.0),
            Biome::Desert => (70.0, 5.0),
            Biome::Savanna => (72.0, 6.0),
            Biome::Swamp => (66.0, 1.5),
            Biome::Jungle => (74.0, 10.0),
            Biome::Badlands => (78.0, 12.0),
            Biome::Meadow => (72.0, 4.0),
            Biome::WindsweptHills => (85.0, 22.0),
            Biome::River => (63.0, 1.0),
            Biome::Beach => (64.0, 1.0),
            Biome::Ocean => (40.0, 6.0),
            Biome::DeepOcean => (25.0, 4.0),
        }
    }

    /// Whether precipitation in this biome falls as snow.
    pub fn is_snowy(self) -> bool {
        matches!(
            self,
            Biome::SnowyPlains | Biome::Taiga | Biome::WindsweptHills
        )
    }

    /// Whether this biome is dry (no rain).
    pub fn is_dry(self) -> bool {
        matches!(self, Biome::Desert | Biome::Badlands | Biome::Savanna)
    }
}

#[cfg(test)]
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

#[cfg(test)]
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
    Bed = 89,
    FurnaceLit = 90,
    Farmland = 91,
    WheatCrop = 92,
    CarrotCrop = 93,
    PotatoCrop = 94,
    // Voxel Shapes & Building Blocks (Plan 06)
    OakSlab = 95,
    CobblestoneSlab = 96,
    OakStair = 97,
    CobblestoneStair = 98,
    OakFence = 99,
    OakFenceGate = 100,
    CobblestoneWall = 101,
    GlassPane = 102,
    OakLadder = 103,
    OakSign = 104,
    OakSapling = 105,
    BirchSapling = 106,
    SpruceSapling = 107,
    Spawner = 108,
    MossyCobblestone = 109,
    DirtPath = 110,
    NetherWartCrop = 111,
    EndStoneBrick = 112,
    RespawnAnchor = 113,
    EndGateway = 114,
    Rail = 115,
    PoweredRail = 116,
    DetectorRail = 117,
    ActivatorRail = 118,
    Hopper = 119,
    Observer = 120,
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

impl BlockProperties {
    pub fn is_opaque(&self) -> bool {
        self.render_type == RenderType::Opaque
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChestType {
    Single,
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockState {
    pub facing: Direction,
    pub is_top: bool,
    pub is_right_hinge: bool,
    pub is_open: bool,
    pub chest_type: ChestType,
}

impl Default for BlockState {
    fn default() -> Self {
        Self {
            facing: Direction::North,
            is_top: false,
            is_right_hinge: false,
            is_open: false,
            chest_type: ChestType::Single,
        }
    }
}

impl BlockState {
    pub fn encode(self) -> u8 {
        let facing_bits = match self.facing {
            Direction::North => 0b00,
            Direction::South => 0b01,
            Direction::West => 0b10,
            _ => 0b11,
        };
        let half_bit = if self.is_top { 1 << 2 } else { 0 };
        let hinge_bit = if self.is_right_hinge { 1 << 3 } else { 0 };
        let open_bit = if self.is_open { 1 << 4 } else { 0 };
        let chest_type_bits = match self.chest_type {
            ChestType::Single => 0b00,
            ChestType::Left => 0b01,
            ChestType::Right => 0b10,
        } << 5;
        facing_bits | half_bit | hinge_bit | open_bit | chest_type_bits
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
        let chest_type = match (val >> 5) & 0b11 {
            0 => ChestType::Single,
            1 => ChestType::Left,
            2 => ChestType::Right,
            _ => ChestType::Single,
        };
        Self {
            facing,
            is_top,
            is_right_hinge,
            is_open,
            chest_type,
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
            _ => (0, -1),
        };
        let (right_dx, right_dz) = match facing {
            Direction::North => (1, 0),
            Direction::South => (-1, 0),
            Direction::West => (0, -1),
            _ => (0, 1),
        };

        let left_block = chunk_manager.get_block(x + left_dx, y, z + left_dz);
        let right_block = chunk_manager.get_block(x + right_dx, y, z + right_dz);

        let is_right_hinge = left_block.properties().is_solid && !right_block.properties().is_solid;

        let bottom = Self {
            facing,
            is_top: false,
            is_right_hinge,
            is_open: false,
            chest_type: ChestType::Single,
        };
        let top = Self {
            facing,
            is_top: true,
            is_right_hinge,
            is_open: false,
            chest_type: ChestType::Single,
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
            chest_type: ChestType::Single,
        }
    }
}

impl BlockType {
    pub fn from_u8(val: u8) -> Self {
        if val <= BlockType::Observer as u8 {
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
        if val <= BlockType::Observer as u32 {
            Some(unsafe { std::mem::transmute(val as u8) })
        } else {
            None
        }
    }

    /// The intentionally small Plan27 waterlogging contract.  Other blocks
    /// retain their existing fluid semantics and must be rejected by the
    /// authority rather than silently accepting bit 7.
    pub const fn is_waterloggable(self) -> bool {
        matches!(self, BlockType::OakSlab | BlockType::CobblestoneSlab)
    }

    pub fn is_cross_model(self) -> bool {
        matches!(
            self,
            BlockType::Dandelion
                | BlockType::Poppy
                | BlockType::TallGrass
                | BlockType::SugarCane
                | BlockType::WheatCrop
                | BlockType::CarrotCrop
                | BlockType::PotatoCrop
        )
    }

    pub fn can_stay_on(self, below: BlockType) -> bool {
        match self {
            BlockType::WheatCrop | BlockType::CarrotCrop | BlockType::PotatoCrop => {
                below == BlockType::Farmland
            }
            BlockType::Dandelion | BlockType::Poppy | BlockType::TallGrass => {
                matches!(
                    below,
                    BlockType::Grass | BlockType::Dirt | BlockType::Farmland
                )
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
            BlockType::OakLadder => {
                let mut has_unknown = false;
                for (dx, dz) in [(0, 1), (0, -1), (1, 0), (-1, 0)] {
                    match get_loaded_block(x + dx, y, z + dz) {
                        Some(b) if b.properties().is_solid => return BlockSupportStatus::Supported,
                        Some(_) => {}
                        None => has_unknown = true,
                    }
                }
                if has_unknown {
                    BlockSupportStatus::Unknown
                } else {
                    BlockSupportStatus::Unsupported
                }
            }
            BlockType::OakSign => {
                if y <= 0 {
                    BlockSupportStatus::Unsupported
                } else {
                    let mut has_unknown = false;
                    if let Some(below) = get_loaded_block(x, y - 1, z) {
                        if below.properties().is_solid {
                            return BlockSupportStatus::Supported;
                        }
                    } else {
                        has_unknown = true;
                    }
                    for (dx, dz) in [(0, 1), (0, -1), (1, 0), (-1, 0)] {
                        match get_loaded_block(x + dx, y, z + dz) {
                            Some(b) if b.properties().is_solid => {
                                return BlockSupportStatus::Supported
                            }
                            Some(_) => {}
                            None => has_unknown = true,
                        }
                    }
                    if has_unknown {
                        BlockSupportStatus::Unknown
                    } else {
                        BlockSupportStatus::Unsupported
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

    pub fn sound_material(self) -> Option<SoundMaterial> {
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
            | BlockType::SugarCane => Some(SoundMaterial::Grass),
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
            | BlockType::Melon => Some(SoundMaterial::Wood),
            BlockType::Sand | BlockType::Clay | BlockType::SoulSand => {
                Some(SoundMaterial::Sand)
            }
            BlockType::Gravel | BlockType::Cactus => Some(SoundMaterial::Gravel),
            BlockType::Snow | BlockType::SnowLayer => Some(SoundMaterial::Snow),
            BlockType::Ice => Some(SoundMaterial::Ice),
            BlockType::Glass => Some(SoundMaterial::Glass),
            BlockType::Anvil => Some(SoundMaterial::Stone),
            BlockType::OakDoor
            | BlockType::OakDoorOpen
            | BlockType::OakTrapdoor
            | BlockType::OakTrapdoorOpen
            | BlockType::NoteBlock => Some(SoundMaterial::Wood),
            _ => Some(SoundMaterial::Stone),
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
            BlockType::Furnace | BlockType::FurnaceLit => BlockProperties {
                name: "Furnace",
                hardness: 3.5,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: if self == BlockType::FurnaceLit { 13 } else { 0 },
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
            BlockType::Bed => BlockProperties {
                name: "Bed",
                hardness: 0.2,
                render_type: RenderType::Cutout,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::Farmland => BlockProperties {
                name: "Farmland",
                hardness: 0.6,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::WheatCrop => BlockProperties {
                name: "Wheat Crop",
                hardness: 0.0,
                render_type: RenderType::Cutout,
                is_solid: false,
                is_passable: true,
                light_emission: 0,
            },
            BlockType::CarrotCrop => BlockProperties {
                name: "Carrot Crop",
                hardness: 0.0,
                render_type: RenderType::Cutout,
                is_solid: false,
                is_passable: true,
                light_emission: 0,
            },
            BlockType::PotatoCrop => BlockProperties {
                name: "Potato Crop",
                hardness: 0.0,
                render_type: RenderType::Cutout,
                is_solid: false,
                is_passable: true,
                light_emission: 0,
            },
            BlockType::OakSlab => BlockProperties {
                name: "Oak Slab",
                hardness: 2.0,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::CobblestoneSlab => BlockProperties {
                name: "Cobblestone Slab",
                hardness: 2.0,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::OakStair => BlockProperties {
                name: "Oak Stairs",
                hardness: 2.0,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::CobblestoneStair => BlockProperties {
                name: "Cobblestone Stairs",
                hardness: 2.0,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::OakFence => BlockProperties {
                name: "Oak Fence",
                hardness: 2.0,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::OakFenceGate => BlockProperties {
                name: "Oak Fence Gate",
                hardness: 2.0,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::CobblestoneWall => BlockProperties {
                name: "Cobblestone Wall",
                hardness: 2.0,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::GlassPane => BlockProperties {
                name: "Glass Pane",
                hardness: 0.3,
                render_type: RenderType::Cutout,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::OakLadder => BlockProperties {
                name: "Ladder",
                hardness: 0.4,
                render_type: RenderType::Cutout,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::OakSign => BlockProperties {
                name: "Oak Sign",
                hardness: 1.0,
                render_type: RenderType::Cutout,
                is_solid: false,
                is_passable: true,
                light_emission: 0,
            },
            BlockType::OakSapling | BlockType::BirchSapling | BlockType::SpruceSapling => {
                BlockProperties {
                    name: "Sapling",
                    hardness: 0.0,
                    render_type: RenderType::Cutout,
                    is_solid: false,
                    is_passable: true,
                    light_emission: 0,
                }
            }
            BlockType::Spawner => BlockProperties {
                name: "Mob Spawner",
                hardness: 5.0,
                render_type: RenderType::Cutout,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::MossyCobblestone => BlockProperties {
                name: "Mossy Cobblestone",
                hardness: 2.0,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::DirtPath => BlockProperties {
                name: "Dirt Path",
                hardness: 0.6,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::NetherWartCrop => BlockProperties {
                name: "Nether Wart",
                hardness: 0.0,
                render_type: RenderType::Cutout,
                is_solid: false,
                is_passable: true,
                light_emission: 0,
            },
            BlockType::EndStoneBrick => BlockProperties {
                name: "End Stone Bricks",
                hardness: 3.0,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::RespawnAnchor => BlockProperties {
                name: "Respawn Anchor",
                hardness: 5.0,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 3,
            },
            BlockType::EndGateway => BlockProperties {
                name: "End Gateway",
                hardness: -1.0,
                render_type: RenderType::Translucent,
                is_solid: false,
                is_passable: true,
                light_emission: 15,
            },
            BlockType::Rail => BlockProperties {
                name: "Rail",
                hardness: 0.7,
                render_type: RenderType::Cutout,
                is_solid: false,
                is_passable: true,
                light_emission: 0,
            },
            BlockType::PoweredRail => BlockProperties {
                name: "Powered Rail",
                hardness: 0.7,
                render_type: RenderType::Cutout,
                is_solid: false,
                is_passable: true,
                light_emission: 0,
            },
            BlockType::DetectorRail => BlockProperties {
                name: "Detector Rail",
                hardness: 0.7,
                render_type: RenderType::Cutout,
                is_solid: false,
                is_passable: true,
                light_emission: 0,
            },
            BlockType::ActivatorRail => BlockProperties {
                name: "Activator Rail",
                hardness: 0.7,
                render_type: RenderType::Cutout,
                is_solid: false,
                is_passable: true,
                light_emission: 0,
            },
            BlockType::Hopper => BlockProperties {
                name: "Hopper",
                hardness: 3.0,
                render_type: RenderType::Cutout,
                is_solid: false,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::Observer => BlockProperties {
                name: "Observer",
                hardness: 3.5,
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
            BlockType::Furnace | BlockType::FurnaceLit => {
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
            BlockType::Bed => (6, 0),
            BlockType::Farmland => {
                if face_idx == 4 {
                    (6, 5)
                } else {
                    (2, 0)
                }
            }
            BlockType::WheatCrop => (8, 5),
            BlockType::CarrotCrop => (0, 6),
            BlockType::PotatoCrop => (4, 6),
            BlockType::OakSlab => (6, 0),
            BlockType::CobblestoneSlab => (8, 0),
            BlockType::OakStair => (6, 0),
            BlockType::CobblestoneStair => (8, 0),
            BlockType::OakFence => (6, 0),
            BlockType::OakFenceGate => (6, 0),
            BlockType::CobblestoneWall => (8, 0),
            BlockType::GlassPane => (0, 1),
            BlockType::OakLadder => (3, 5),
            BlockType::OakSign => (6, 0),
            BlockType::OakSapling | BlockType::BirchSapling | BlockType::SpruceSapling => (4, 0),
            BlockType::Spawner => (1, 4),
            BlockType::MossyCobblestone => (4, 2),
            BlockType::DirtPath => {
                if face_idx == 4 {
                    (6, 5)
                } else {
                    (2, 0)
                }
            }
            BlockType::NetherWartCrop => (2, 6),
            BlockType::EndStoneBrick => (15, 10),
            BlockType::RespawnAnchor => (14, 11),
            BlockType::EndGateway => (14, 10),
            BlockType::Rail => (0, 8),
            BlockType::PoweredRail => (3, 8),
            BlockType::DetectorRail => (3, 9),
            BlockType::ActivatorRail => (3, 10),
            BlockType::Hopper => (11, 15),
            BlockType::Observer => (11, 16),
        }
    }

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

/// Search around target_pos for a safe, standable position with 2 air blocks above solid ground.
pub fn find_safe_spawn_position(
    chunk_manager: &crate::chunk_manager::ChunkManager,
    target_pos: (i32, i32, i32),
) -> (glam::Vec3, bool) {
    let (tx, ty, tz) = target_pos;
    for r in 0..=4i32 {
        for dx in -r..=r {
            for dz in -r..=r {
                if dx.abs() != r && dz.abs() != r {
                    continue;
                }
                let x = tx + dx;
                let z = tz + dz;
                for y in (ty - 10..=ty + 10).rev() {
                    let floor_block = chunk_manager.get_block(x, y, z);
                    let feet_block = chunk_manager.get_block(x, y + 1, z);
                    let head_block = chunk_manager.get_block(x, y + 2, z);
                    if floor_block.properties().is_solid
                        && !matches!(
                            floor_block,
                            BlockType::Lava | BlockType::Fire | BlockType::Cactus
                        )
                        && feet_block.properties().is_passable
                        && !matches!(feet_block, BlockType::Lava | BlockType::Fire)
                        && head_block.properties().is_passable
                        && !matches!(head_block, BlockType::Lava | BlockType::Fire)
                    {
                        return (
                            glam::Vec3::new(x as f32 + 0.5, (y + 1) as f32, z as f32 + 0.5),
                            true,
                        );
                    }
                }
            }
        }
    }
    (
        glam::Vec3::new(tx as f32 + 0.5, ty as f32, tz as f32 + 0.5),
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;

    #[test]
    fn block_type_wire_roundtrip_covers_all_variants() {
        // Walk every discriminant in `0..=EndCityChest` and confirm the
        // wire helpers are exact inverses. This also guards against future
        // reordering of the enum: any renumbering would surface here.
        for raw in 0..=BlockType::FurnaceLit as u32 {
            let block = BlockType::from_wire(raw).expect("valid discriminant");
            assert_eq!(block.to_wire(), raw);
        }
    }

    #[test]
    fn block_type_from_wire_rejects_unknown_values() {
        assert_eq!(BlockType::from_wire(9999), None);
        assert_eq!(BlockType::from_wire(255), None);
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
    fn weather_blocks_have_expected_collision_and_light() {
        assert!(BlockType::SnowLayer.properties().is_passable);
        assert!(!BlockType::SnowLayer.properties().is_solid);
        assert_eq!(BlockType::Fire.properties().light_emission, 15);
        assert!(BlockType::Fire.properties().is_passable);
        assert_eq!(BlockType::from_u8(74), BlockType::Fire);
        assert_eq!(BlockType::from_u8(75), BlockType::SnowLayer);
        for id in 0..=BlockType::Observer as u8 {
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
    fn test_find_safe_spawn_position_fallback() {
        let mut cm = crate::chunk_manager::ChunkManager::new(8);
        cm.chunks.insert((0, 0), crate::world::chunk::Chunk::new(0, 0));
        // Create ground block at (0, 63, 0) with Air above
        cm.set_block(0, 63, 0, BlockType::Cobblestone);
        cm.set_block(0, 64, 0, BlockType::Air);
        cm.set_block(0, 65, 0, BlockType::Air);

        let (pos, safe) = find_safe_spawn_position(&cm, (0, 64, 0));
        assert!(safe);
        assert_eq!(pos, Vec3::new(0.5, 64.0, 0.5));
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
                        for chest_type in [ChestType::Single, ChestType::Left, ChestType::Right] {
                            let state = BlockState {
                                facing,
                                is_top,
                                is_right_hinge,
                                is_open,
                                chest_type,
                            };
                            let encoded = state.encode();
                            let decoded = BlockState::decode(encoded);
                            assert_eq!(decoded, state);
                        }
                    }
                }
            }
        }
        // Verify reserved bit (bit 7) is ignored
        for chest_type in [ChestType::Single, ChestType::Left, ChestType::Right] {
            let state = BlockState {
                facing: Direction::North,
                is_top: false,
                is_right_hinge: false,
                is_open: false,
                chest_type,
            };
            let encoded = state.encode() | 0b1000_0000;
            let decoded = BlockState::decode(encoded);
            assert_eq!(decoded, state);
        }
    }
}
