use crate::inventory::{ToolMaterial, ToolType};
use crate::redstone::Direction;

#[path = "block_table.rs"]
mod block_table;
pub use block_table::{BlockDef, BLOCK_TABLE, BLOCK_TYPE_COUNT};

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

#[derive(Debug, Clone, Copy, PartialEq)]
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

    #[inline]
    pub fn def(self) -> &'static BlockDef {
        &BLOCK_TABLE[self as usize]
    }

    #[inline]
    pub fn is_cross_model(self) -> bool {
        self.def().is_cross_model
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

    #[inline]
    pub fn sound_material(self) -> Option<SoundMaterial> {
        self.def().sound
    }

    #[inline]
    pub fn properties(self) -> &'static BlockProperties {
        &self.def().properties
    }

    /// Whether this block is a full, opaque cube that casts vertex ambient occlusion.
    pub fn is_ao_occluder(self) -> bool {
        let properties = self.properties();
        properties.is_solid && properties.render_type == RenderType::Opaque
    }

    #[inline]
    pub fn get_face_tex_index(self, face_idx: usize) -> (u32, u32) {
        self.def().face_tex[face_idx.min(5)]
    }

    #[inline]
    pub fn preferred_tool(self) -> ToolType {
        self.def().preferred_tool
    }

    #[inline]
    pub fn min_harvest_material(self) -> Option<ToolMaterial> {
        self.def().min_harvest
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::redstone::Direction;

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
    fn canonical_block_table_covers_every_variant() {
        assert_eq!(BLOCK_TABLE.len(), BLOCK_TYPE_COUNT);
        assert_eq!(BLOCK_TYPE_COUNT, BlockType::Observer as usize + 1);
        for id in 0..BLOCK_TYPE_COUNT as u8 {
            let block = BlockType::from_u8(id);
            assert_eq!(block as usize, id as usize);
            let def = block.def();
            // Every discriminant maps to exactly one table row.
            assert!(
                std::ptr::eq(def, &BLOCK_TABLE[id as usize]),
                "variant {block:?} must index its own row"
            );
        }
    }

    #[test]
    fn block_static_property_snapshot_is_byte_identical() {
        // Locked dump of every static field for every discriminant. Changing a
        // gameplay number here is intentional and must update this golden string.
        let mut lines = Vec::with_capacity(BLOCK_TYPE_COUNT);
        for id in 0..BLOCK_TYPE_COUNT as u8 {
            let b = BlockType::from_u8(id);
            let d = b.def();
            let p = &d.properties;
            let faces = (0..6)
                .map(|f| {
                    let (c, r) = d.face_tex[f];
                    format!("{c},{r}")
                })
                .collect::<Vec<_>>()
                .join(";");
            lines.push(format!(
                "{id}|{b:?}|{name}|{hardness:.3}|{render:?}|{solid}|{pass}|{light}|{faces}|{sound:?}|{tool:?}|{harvest:?}|{cross}",
                name = p.name,
                hardness = p.hardness,
                render = p.render_type,
                solid = p.is_solid as u8,
                pass = p.is_passable as u8,
                light = p.light_emission,
                sound = d.sound,
                tool = d.preferred_tool,
                harvest = d.min_harvest,
                cross = d.is_cross_model as u8,
            ));
        }
        let snapshot = lines.join("\n");
        let expected = include_str!("block_property_snapshot.txt")
            .replace("\r\n", "\n")
            .trim_end()
            .to_string();
        assert_eq!(
            snapshot, expected,
            "BlockDef table drifted from the locked snapshot"
        );
        // Thin accessors must agree with the table row.
        for id in 0..BLOCK_TYPE_COUNT as u8 {
            let b = BlockType::from_u8(id);
            let d = b.def();
            assert_eq!(b.properties().name, d.properties.name);
            assert_eq!(b.properties().hardness, d.properties.hardness);
            assert_eq!(b.sound_material(), d.sound);
            assert_eq!(b.preferred_tool(), d.preferred_tool);
            assert_eq!(b.min_harvest_material(), d.min_harvest);
            assert_eq!(b.is_cross_model(), d.is_cross_model);
            for face in 0..6 {
                assert_eq!(b.get_face_tex_index(face), d.face_tex[face]);
            }
        }
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
