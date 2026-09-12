use super::*;


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
    /// Reserved wire/save hole (was RedstoneTorchOff).
    #[doc(hidden)]
    Reserved50 = 50,
    Repeater = 51,
    /// Reserved wire/save hole (was RepeaterPowered).
    #[doc(hidden)]
    Reserved52 = 52,
    Comparator = 53,
    /// Reserved wire/save hole (was ComparatorPowered).
    #[doc(hidden)]
    Reserved54 = 54,
    StoneButton = 55,
    /// Reserved wire/save hole (was StoneButtonPressed).
    #[doc(hidden)]
    Reserved56 = 56,
    Lever = 57,
    /// Reserved wire/save hole (was LeverOn).
    #[doc(hidden)]
    Reserved58 = 58,
    PressurePlate = 59,
    /// Reserved wire/save hole (was PressurePlatePowered).
    #[doc(hidden)]
    Reserved60 = 60,
    Piston = 61,
    /// Reserved wire/save hole (was PistonExtended).
    #[doc(hidden)]
    Reserved62 = 62,
    StickyPiston = 63,
    /// Reserved wire/save hole (was StickyPistonExtended).
    #[doc(hidden)]
    Reserved64 = 64,
    RedstoneLamp = 65,
    /// Reserved wire/save hole (was RedstoneLampLit).
    #[doc(hidden)]
    Reserved66 = 66,
    OakDoor = 67,
    /// Reserved wire/save hole (was OakDoorOpen).
    #[doc(hidden)]
    Reserved68 = 68,
    OakTrapdoor = 69,
    /// Reserved wire/save hole (was OakTrapdoorOpen).
    #[doc(hidden)]
    Reserved70 = 70,
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
    /// Reserved wire/save hole (was EndPortalFrameFilled).
    #[doc(hidden)]
    Reserved82 = 82,
    EndPortal = 83,
    Purpur = 84,
    DragonEgg = 85,
    WitherSkeletonSkull = 86,
    NetherBrick = 87,
    EndCityChest = 88,
    Bed = 89,
    /// Reserved wire/save hole (was FurnaceLit).
    #[doc(hidden)]
    Reserved90 = 90,
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

