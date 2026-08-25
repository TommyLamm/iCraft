use crate::world::BlockType;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Item {
    Air,
    // Blocks
    Grass,
    Dirt,
    Stone,
    Sand,
    Gravel,
    OakLog,
    OakPlanks,
    OakLeaves,
    Cobblestone,
    Bedrock,
    Water,
    CoalOre,
    IronOre,
    GoldOre,
    DiamondOre,
    RedstoneOre,
    Glass,
    Brick,
    StoneBrick,
    Snow,
    Ice,
    Clay,
    Sandstone,
    Obsidian,
    CraftingTable,
    Furnace,
    Chest,
    TNT,
    Bookshelf,
    Torch,
    Lava,
    Bed,

    // Tools
    StoneSword,
    StonePickaxe,
    StoneAxe,
    StoneShovel,
    IronSword,
    IronPickaxe,
    IronAxe,
    IronShovel,
    DiamondSword,
    DiamondPickaxe,
    DiamondAxe,
    DiamondShovel,
    WoodenHoe,
    StoneHoe,
    IronHoe,
    GoldenHoe,
    DiamondHoe,

    // Resources
    Stick,
    Coal,
    IronIngot,
    GoldIngot,
    Diamond,
    Redstone,
    BoneMeal,

    // Food
    Apple,
    Bread,
    Potato,
    BakedPotato,
    PoisonousPotato,
    GoldenApple,

    // Mob Drops
    RottenFlesh,
    Bone,
    Bow,
    Gunpowder,

    // Passive Mob Items
    Wheat,
    Seeds,
    Carrot,
    Shears,
    Bucket,
    MilkBucket,
    RawPorkchop,
    CookedPorkchop,
    RawBeef,
    CookedBeef,
    RawMutton,
    CookedMutton,
    RawChicken,
    CookedChicken,
    Wool,
    Leather,
    Feather,
    Egg,
    RedDye,
    BlueDye,
    GreenDye,
    // Trees & Biomes Additions
    BirchLog,
    BirchPlanks,
    BirchLeaves,
    SpruceLog,
    SprucePlanks,
    SpruceLeaves,
    TallGrass,
    Dandelion,
    Poppy,
    Cactus,
    SugarCane,
    Pumpkin,
    Melon,
    // Enchanting, armor, and brewing
    EnchantingTable,
    BrewingStand,
    Anvil,
    LapisLazuli,
    IronHelmet,
    IronChestplate,
    IronLeggings,
    IronBoots,
    GlassBottle,
    Potion,
    SplashPotion,
    NetherWart,
    Sugar,
    BlazePowder,
    GlisteringMelon,
    GhastTear,
    GoldenCarrot,
    FermentedSpiderEye,
    MagmaCream,
    Pufferfish,
    SpiderEye,
    GlowstoneDust,
    RedstoneDust,
    Arrow,
    RedstoneWire,
    RedstoneTorch,
    Repeater,
    Comparator,
    StoneButton,
    Lever,
    PressurePlate,
    Piston,
    StickyPiston,
    RedstoneLamp,
    OakDoor,
    OakTrapdoor,
    Dispenser,
    Dropper,
    NoteBlock,
    // Dimensions, structures, and bosses
    Netherrack,
    SoulSand,
    Glowstone,
    EndStone,
    EndPortalFrame,
    Purpur,
    DragonEgg,
    WitherSkeletonSkull,
    NetherBrick,
    FlintAndSteel,
    EyeOfEnder,
    Elytra,
    NetherStar,
    EndCrystal,
    BlazeRod,
    ShulkerShell,
    OakSlab,
    CobblestoneSlab,
    OakStair,
    CobblestoneStair,
    OakFence,
    OakFenceGate,
    CobblestoneWall,
    GlassPane,
    OakLadder,
    OakSign,

    // Equipment & Weapons Additions (Plan 07)
    WoodenSword,
    WoodenPickaxe,
    WoodenAxe,
    WoodenShovel,
    GoldenSword,
    GoldenPickaxe,
    GoldenAxe,
    GoldenShovel,
    LeatherHelmet,
    LeatherChestplate,
    LeatherLeggings,
    LeatherBoots,
    GoldenHelmet,
    GoldenChestplate,
    GoldenLeggings,
    GoldenBoots,
    DiamondHelmet,
    DiamondChestplate,
    DiamondLeggings,
    DiamondBoots,
    Shield,
    Saddle,
    Emerald,
    Book,
    Paper,
    EnchantedBook,
    Compass,
    String,
    Slimeball,
    RawCod,
    RawSalmon,
    InkSac,
    OakBoat,
    Minecart,
    Rail,
    PoweredRail,
    DetectorRail,
    ActivatorRail,
    Clock,
    Map,
    FishingRod,
    RawFish,
    TropicalFish,
    LilyPad,

    // Automation items appended to preserve every legacy item discriminant.
    WaterBucket,
    LavaBucket,
    Hopper,
    Observer,
}

pub const ALL_ITEMS: &[Item] = &[
    Item::Air,
    Item::Grass,
    Item::Dirt,
    Item::Stone,
    Item::Sand,
    Item::Gravel,
    Item::OakLog,
    Item::OakPlanks,
    Item::OakLeaves,
    Item::Cobblestone,
    Item::Bedrock,
    Item::Water,
    Item::CoalOre,
    Item::IronOre,
    Item::GoldOre,
    Item::DiamondOre,
    Item::RedstoneOre,
    Item::Glass,
    Item::Brick,
    Item::StoneBrick,
    Item::Snow,
    Item::Ice,
    Item::Clay,
    Item::Sandstone,
    Item::Obsidian,
    Item::CraftingTable,
    Item::Furnace,
    Item::Chest,
    Item::TNT,
    Item::Bookshelf,
    Item::Torch,
    Item::Lava,
    Item::Bed,
    Item::StoneSword,
    Item::StonePickaxe,
    Item::StoneAxe,
    Item::StoneShovel,
    Item::IronSword,
    Item::IronPickaxe,
    Item::IronAxe,
    Item::IronShovel,
    Item::DiamondSword,
    Item::DiamondPickaxe,
    Item::DiamondAxe,
    Item::DiamondShovel,
    Item::WoodenHoe,
    Item::StoneHoe,
    Item::IronHoe,
    Item::GoldenHoe,
    Item::DiamondHoe,
    Item::Stick,
    Item::Coal,
    Item::IronIngot,
    Item::GoldIngot,
    Item::Diamond,
    Item::Redstone,
    Item::BoneMeal,
    Item::Apple,
    Item::Bread,
    Item::Potato,
    Item::BakedPotato,
    Item::PoisonousPotato,
    Item::GoldenApple,
    Item::RottenFlesh,
    Item::Bone,
    Item::Bow,
    Item::Gunpowder,
    Item::Wheat,
    Item::Seeds,
    Item::Carrot,
    Item::Shears,
    Item::Bucket,
    Item::MilkBucket,
    Item::RawPorkchop,
    Item::CookedPorkchop,
    Item::RawBeef,
    Item::CookedBeef,
    Item::RawMutton,
    Item::CookedMutton,
    Item::RawChicken,
    Item::CookedChicken,
    Item::Wool,
    Item::Leather,
    Item::Feather,
    Item::Egg,
    Item::RedDye,
    Item::BlueDye,
    Item::GreenDye,
    Item::BirchLog,
    Item::BirchPlanks,
    Item::BirchLeaves,
    Item::SpruceLog,
    Item::SprucePlanks,
    Item::SpruceLeaves,
    Item::TallGrass,
    Item::Dandelion,
    Item::Poppy,
    Item::Cactus,
    Item::SugarCane,
    Item::Pumpkin,
    Item::Melon,
    Item::EnchantingTable,
    Item::BrewingStand,
    Item::Anvil,
    Item::LapisLazuli,
    Item::IronHelmet,
    Item::IronChestplate,
    Item::IronLeggings,
    Item::IronBoots,
    Item::GlassBottle,
    Item::Potion,
    Item::SplashPotion,
    Item::NetherWart,
    Item::Sugar,
    Item::BlazePowder,
    Item::GlisteringMelon,
    Item::GhastTear,
    Item::GoldenCarrot,
    Item::FermentedSpiderEye,
    Item::MagmaCream,
    Item::Pufferfish,
    Item::SpiderEye,
    Item::GlowstoneDust,
    Item::RedstoneDust,
    Item::Arrow,
    Item::RedstoneWire,
    Item::RedstoneTorch,
    Item::Repeater,
    Item::Comparator,
    Item::StoneButton,
    Item::Lever,
    Item::PressurePlate,
    Item::Piston,
    Item::StickyPiston,
    Item::RedstoneLamp,
    Item::OakDoor,
    Item::OakTrapdoor,
    Item::Dispenser,
    Item::Dropper,
    Item::NoteBlock,
    Item::Netherrack,
    Item::SoulSand,
    Item::Glowstone,
    Item::EndStone,
    Item::EndPortalFrame,
    Item::Purpur,
    Item::DragonEgg,
    Item::WitherSkeletonSkull,
    Item::NetherBrick,
    Item::FlintAndSteel,
    Item::EyeOfEnder,
    Item::Elytra,
    Item::NetherStar,
    Item::EndCrystal,
    Item::BlazeRod,
    Item::ShulkerShell,
    Item::OakSlab,
    Item::CobblestoneSlab,
    Item::OakStair,
    Item::CobblestoneStair,
    Item::OakFence,
    Item::OakFenceGate,
    Item::CobblestoneWall,
    Item::GlassPane,
    Item::OakLadder,
    Item::OakSign,
    Item::WoodenSword,
    Item::WoodenPickaxe,
    Item::WoodenAxe,
    Item::WoodenShovel,
    Item::GoldenSword,
    Item::GoldenPickaxe,
    Item::GoldenAxe,
    Item::GoldenShovel,
    Item::LeatherHelmet,
    Item::LeatherChestplate,
    Item::LeatherLeggings,
    Item::LeatherBoots,
    Item::GoldenHelmet,
    Item::GoldenChestplate,
    Item::GoldenLeggings,
    Item::GoldenBoots,
    Item::DiamondHelmet,
    Item::DiamondChestplate,
    Item::DiamondLeggings,
    Item::DiamondBoots,
    Item::Shield,
    Item::Saddle,
    Item::Emerald,
    Item::Book,
    Item::Paper,
    Item::EnchantedBook,
    Item::Compass,
    Item::String,
    Item::Slimeball,
    Item::RawCod,
    Item::RawSalmon,
    Item::InkSac,
    Item::OakBoat,
    Item::Minecart,
    Item::Rail,
    Item::PoweredRail,
    Item::DetectorRail,
    Item::ActivatorRail,
    Item::Clock,
    Item::Map,
    Item::FishingRod,
    Item::RawFish,
    Item::TropicalFish,
    Item::LilyPad,
    Item::WaterBucket,
    Item::LavaBucket,
    Item::Hopper,
    Item::Observer,
];

impl Item {
    pub fn to_u32(self) -> u32 {
        self as u32
    }

    pub fn from_u32(val: u32) -> Option<Self> {
        let idx = val as usize;
        if idx < ALL_ITEMS.len() {
            Some(ALL_ITEMS[idx])
        } else {
            None
        }
    }
}

pub const CREATIVE_COLUMNS: usize = 9;
pub const CREATIVE_ROWS: usize = 5;
pub const CREATIVE_VISIBLE_SLOTS: usize = CREATIVE_COLUMNS * CREATIVE_ROWS;

pub const CREATIVE_ITEMS: [Item; 154] = [
    Item::Grass,
    Item::Dirt,
    Item::Stone,
    Item::Sand,
    Item::Gravel,
    Item::OakLog,
    Item::OakPlanks,
    Item::OakLeaves,
    Item::Cobblestone,
    Item::Bedrock,
    Item::Water,
    Item::CoalOre,
    Item::IronOre,
    Item::GoldOre,
    Item::DiamondOre,
    Item::RedstoneOre,
    Item::Glass,
    Item::Brick,
    Item::StoneBrick,
    Item::Snow,
    Item::Ice,
    Item::Clay,
    Item::Sandstone,
    Item::Obsidian,
    Item::CraftingTable,
    Item::Furnace,
    Item::Chest,
    Item::TNT,
    Item::Bookshelf,
    Item::Torch,
    Item::Lava,
    Item::StoneSword,
    Item::StonePickaxe,
    Item::StoneAxe,
    Item::StoneShovel,
    Item::IronSword,
    Item::IronPickaxe,
    Item::IronAxe,
    Item::IronShovel,
    Item::DiamondSword,
    Item::DiamondPickaxe,
    Item::DiamondAxe,
    Item::DiamondShovel,
    Item::Stick,
    Item::Coal,
    Item::IronIngot,
    Item::GoldIngot,
    Item::Diamond,
    Item::Redstone,
    Item::Apple,
    Item::Bread,
    Item::RottenFlesh,
    Item::Bone,
    Item::Bow,
    Item::Gunpowder,
    Item::Wheat,
    Item::Seeds,
    Item::Carrot,
    Item::Shears,
    Item::Bucket,
    Item::MilkBucket,
    Item::RawPorkchop,
    Item::CookedPorkchop,
    Item::RawBeef,
    Item::CookedBeef,
    Item::RawMutton,
    Item::CookedMutton,
    Item::RawChicken,
    Item::CookedChicken,
    Item::Wool,
    Item::Leather,
    Item::Feather,
    Item::Egg,
    Item::RedDye,
    Item::BlueDye,
    Item::GreenDye,
    Item::BirchLog,
    Item::BirchPlanks,
    Item::BirchLeaves,
    Item::SpruceLog,
    Item::SprucePlanks,
    Item::SpruceLeaves,
    Item::TallGrass,
    Item::Dandelion,
    Item::Poppy,
    Item::Cactus,
    Item::SugarCane,
    Item::Pumpkin,
    Item::Melon,
    Item::EnchantingTable,
    Item::BrewingStand,
    Item::Anvil,
    Item::LapisLazuli,
    Item::IronHelmet,
    Item::IronChestplate,
    Item::IronLeggings,
    Item::IronBoots,
    Item::GlassBottle,
    Item::Potion,
    Item::SplashPotion,
    Item::NetherWart,
    Item::Sugar,
    Item::BlazePowder,
    Item::GlisteringMelon,
    Item::GhastTear,
    Item::GoldenCarrot,
    Item::FermentedSpiderEye,
    Item::MagmaCream,
    Item::Pufferfish,
    Item::SpiderEye,
    Item::GlowstoneDust,
    Item::RedstoneDust,
    Item::Arrow,
    Item::RedstoneWire,
    Item::RedstoneTorch,
    Item::Repeater,
    Item::Comparator,
    Item::StoneButton,
    Item::Lever,
    Item::PressurePlate,
    Item::Piston,
    Item::StickyPiston,
    Item::RedstoneLamp,
    Item::OakDoor,
    Item::OakTrapdoor,
    Item::Dispenser,
    Item::Dropper,
    Item::NoteBlock,
    Item::Netherrack,
    Item::SoulSand,
    Item::Glowstone,
    Item::EndStone,
    Item::EndPortalFrame,
    Item::Purpur,
    Item::DragonEgg,
    Item::WitherSkeletonSkull,
    Item::NetherBrick,
    Item::FlintAndSteel,
    Item::EyeOfEnder,
    Item::Elytra,
    Item::NetherStar,
    Item::EndCrystal,
    Item::BlazeRod,
    Item::ShulkerShell,
    Item::OakSlab,
    Item::CobblestoneSlab,
    Item::OakStair,
    Item::CobblestoneStair,
    Item::OakFence,
    Item::OakFenceGate,
    Item::CobblestoneWall,
    Item::GlassPane,
    Item::OakLadder,
    Item::OakSign,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreativeTab {
    All,
    Blocks,
    Tools,
    Combat,
    FoodAndBrewing,
    Redstone,
    Misc,
}

impl CreativeTab {
    pub const TABS: [Self; 7] = [
        Self::All,
        Self::Blocks,
        Self::Tools,
        Self::Combat,
        Self::FoodAndBrewing,
        Self::Redstone,
        Self::Misc,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "ALL",
            Self::Blocks => "BLOCKS",
            Self::Tools => "TOOLS",
            Self::Combat => "COMBAT",
            Self::FoodAndBrewing => "FOOD+BREW",
            Self::Redstone => "REDSTONE",
            Self::Misc => "MISC",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolType {
    None,
    Pickaxe,
    Axe,
    Shovel,
    Sword,
    Hoe,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ToolMaterial {
    Wood,
    Stone,
    Iron,
    Gold,
    Diamond,
}

#[derive(Debug, Clone, Copy)]
pub struct ToolProperties {
    pub tool_type: ToolType,
    pub material: ToolMaterial,
    pub mining_speed: f32,
    pub durability: u32,
    pub damage: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ArmorSlot {
    Helmet,
    Chestplate,
    Leggings,
    Boots,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArmorProperties {
    pub slot: ArmorSlot,
    pub armor_points: f32,
    pub toughness: f32,
    pub knockback_resistance: f32,
    pub durability: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FoodProperties {
    pub hunger: f32,
    pub saturation: f32,
    pub use_duration_ticks: u32,
    pub always_edible: bool,
    pub return_item: Option<Item>,
}

pub struct ItemProperties {
    pub name: &'static str,
    pub max_stack: u32,
    pub is_block: bool,
    pub block_type: Option<BlockType>,
    pub tex_coords: (u32, u32), // (col, row) in texture atlas
}

impl Item {
    pub fn creative_tab(self) -> Option<CreativeTab> {
        match self {
            Item::Air => None,
            Item::Grass
            | Item::Dirt
            | Item::Stone
            | Item::Sand
            | Item::Gravel
            | Item::OakLog
            | Item::OakPlanks
            | Item::OakLeaves
            | Item::Cobblestone
            | Item::Bedrock
            | Item::Water
            | Item::CoalOre
            | Item::IronOre
            | Item::GoldOre
            | Item::DiamondOre
            | Item::RedstoneOre
            | Item::Glass
            | Item::Brick
            | Item::StoneBrick
            | Item::Snow
            | Item::Ice
            | Item::Clay
            | Item::Sandstone
            | Item::Obsidian
            | Item::CraftingTable
            | Item::Furnace
            | Item::Chest
            | Item::Bookshelf
            | Item::Torch
            | Item::Lava
            | Item::BirchLog
            | Item::BirchPlanks
            | Item::BirchLeaves
            | Item::SpruceLog
            | Item::SprucePlanks
            | Item::SpruceLeaves
            | Item::TallGrass
            | Item::Dandelion
            | Item::Poppy
            | Item::Cactus
            | Item::SugarCane
            | Item::Pumpkin
            | Item::Melon
            | Item::EnchantingTable
            | Item::BrewingStand
            | Item::Anvil
            | Item::Netherrack
            | Item::SoulSand
            | Item::Glowstone
            | Item::EndStone
            | Item::EndPortalFrame
            | Item::Purpur
            | Item::DragonEgg
            | Item::WitherSkeletonSkull
            | Item::NetherBrick => Some(CreativeTab::Blocks),
            Item::StonePickaxe
            | Item::StoneAxe
            | Item::StoneShovel
            | Item::IronPickaxe
            | Item::IronAxe
            | Item::IronShovel
            | Item::DiamondPickaxe
            | Item::DiamondAxe
            | Item::DiamondShovel
            | Item::WoodenHoe
            | Item::StoneHoe
            | Item::IronHoe
            | Item::GoldenHoe
            | Item::DiamondHoe
            | Item::Shears
            | Item::Bucket
            | Item::WaterBucket
            | Item::LavaBucket
            | Item::MilkBucket
            | Item::FlintAndSteel
            | Item::Elytra => Some(CreativeTab::Tools),
            Item::StoneSword
            | Item::IronSword
            | Item::DiamondSword
            | Item::WoodenSword
            | Item::WoodenPickaxe
            | Item::WoodenAxe
            | Item::WoodenShovel
            | Item::GoldenSword
            | Item::GoldenPickaxe
            | Item::GoldenAxe
            | Item::GoldenShovel
            | Item::Bow
            | Item::Arrow
            | Item::Shield
            | Item::LeatherHelmet
            | Item::LeatherChestplate
            | Item::LeatherLeggings
            | Item::LeatherBoots
            | Item::GoldenHelmet
            | Item::GoldenChestplate
            | Item::GoldenLeggings
            | Item::GoldenBoots
            | Item::IronHelmet
            | Item::IronChestplate
            | Item::IronLeggings
            | Item::IronBoots
            | Item::DiamondHelmet
            | Item::DiamondChestplate
            | Item::DiamondLeggings
            | Item::DiamondBoots
            | Item::EndCrystal => Some(CreativeTab::Combat),
            Item::Apple
            | Item::Bread
            | Item::Potato
            | Item::BakedPotato
            | Item::PoisonousPotato
            | Item::GoldenApple
            | Item::Gunpowder
            | Item::Wheat
            | Item::Carrot
            | Item::RawPorkchop
            | Item::CookedPorkchop
            | Item::RawBeef
            | Item::CookedBeef
            | Item::RawMutton
            | Item::CookedMutton
            | Item::RawChicken
            | Item::CookedChicken
            | Item::Egg
            | Item::GlassBottle
            | Item::Potion
            | Item::SplashPotion
            | Item::NetherWart
            | Item::Sugar
            | Item::BlazePowder
            | Item::GlisteringMelon
            | Item::GhastTear
            | Item::GoldenCarrot
            | Item::FermentedSpiderEye
            | Item::MagmaCream
            | Item::Pufferfish
            | Item::SpiderEye
            | Item::GlowstoneDust
            | Item::RedstoneDust => Some(CreativeTab::FoodAndBrewing),
            Item::TNT
            | Item::Redstone
            | Item::RedstoneWire
            | Item::RedstoneTorch
            | Item::Repeater
            | Item::Comparator
            | Item::StoneButton
            | Item::Lever
            | Item::PressurePlate
            | Item::Piston
            | Item::StickyPiston
            | Item::RedstoneLamp
            | Item::OakDoor
            | Item::OakTrapdoor
            | Item::Dispenser
            | Item::Dropper
            | Item::NoteBlock
            | Item::Hopper
            | Item::Observer => Some(CreativeTab::Redstone),
            Item::Stick
            | Item::Coal
            | Item::IronIngot
            | Item::GoldIngot
            | Item::Diamond
            | Item::BoneMeal
            | Item::RottenFlesh
            | Item::Bone
            | Item::Seeds
            | Item::Wool
            | Item::Leather
            | Item::Feather
            | Item::RedDye
            | Item::BlueDye
            | Item::GreenDye
            | Item::LapisLazuli
            | Item::EyeOfEnder
            | Item::NetherStar
            | Item::BlazeRod
            | Item::Bed
            | Item::ShulkerShell
            | Item::Saddle
            | Item::Emerald
            | Item::Book
            | Item::Paper
            | Item::EnchantedBook
            | Item::Compass => Some(CreativeTab::Misc),
            Item::OakSlab => Some(CreativeTab::Blocks),
            Item::CobblestoneSlab => Some(CreativeTab::Blocks),
            Item::OakStair => Some(CreativeTab::Blocks),
            Item::CobblestoneStair => Some(CreativeTab::Blocks),
            Item::OakFence => Some(CreativeTab::Blocks),
            Item::OakFenceGate => Some(CreativeTab::Blocks),
            Item::CobblestoneWall => Some(CreativeTab::Blocks),
            Item::GlassPane => Some(CreativeTab::Blocks),
            Item::OakLadder => Some(CreativeTab::Blocks),
            Item::OakSign => Some(CreativeTab::Blocks),
            Item::String
            | Item::Slimeball
            | Item::RawCod
            | Item::RawSalmon
            | Item::InkSac
            | Item::OakBoat
            | Item::Minecart
            | Item::Rail
            | Item::PoweredRail
            | Item::DetectorRail
            | Item::ActivatorRail
            | Item::Clock
            | Item::Map
            | Item::FishingRod
            | Item::RawFish
            | Item::TropicalFish
            | Item::LilyPad => Some(CreativeTab::Misc),
        }
    }

    pub fn tool_properties(self) -> Option<ToolProperties> {
        match self {
            Item::StoneSword => Some(ToolProperties {
                tool_type: ToolType::Sword,
                material: ToolMaterial::Stone,
                mining_speed: 4.0,
                durability: 131,
                damage: 5.0,
            }),
            Item::StonePickaxe => Some(ToolProperties {
                tool_type: ToolType::Pickaxe,
                material: ToolMaterial::Stone,
                mining_speed: 4.0,
                durability: 131,
                damage: 3.0,
            }),
            Item::StoneAxe => Some(ToolProperties {
                tool_type: ToolType::Axe,
                material: ToolMaterial::Stone,
                mining_speed: 4.0,
                durability: 131,
                damage: 4.0,
            }),
            Item::StoneShovel => Some(ToolProperties {
                tool_type: ToolType::Shovel,
                material: ToolMaterial::Stone,
                mining_speed: 4.0,
                durability: 131,
                damage: 2.0,
            }),
            Item::Shears => Some(ToolProperties {
                tool_type: ToolType::None,
                material: ToolMaterial::Iron,
                mining_speed: 1.0,
                durability: 238,
                damage: 1.0,
            }),

            Item::IronSword => Some(ToolProperties {
                tool_type: ToolType::Sword,
                material: ToolMaterial::Iron,
                mining_speed: 6.0,
                durability: 250,
                damage: 6.0,
            }),
            Item::IronPickaxe => Some(ToolProperties {
                tool_type: ToolType::Pickaxe,
                material: ToolMaterial::Iron,
                mining_speed: 6.0,
                durability: 250,
                damage: 4.0,
            }),
            Item::IronAxe => Some(ToolProperties {
                tool_type: ToolType::Axe,
                material: ToolMaterial::Iron,
                mining_speed: 6.0,
                durability: 250,
                damage: 5.0,
            }),
            Item::IronShovel => Some(ToolProperties {
                tool_type: ToolType::Shovel,
                material: ToolMaterial::Iron,
                mining_speed: 6.0,
                durability: 250,
                damage: 3.0,
            }),

            Item::DiamondSword => Some(ToolProperties {
                tool_type: ToolType::Sword,
                material: ToolMaterial::Diamond,
                mining_speed: 8.0,
                durability: 1561,
                damage: 7.0,
            }),
            Item::DiamondPickaxe => Some(ToolProperties {
                tool_type: ToolType::Pickaxe,
                material: ToolMaterial::Diamond,
                mining_speed: 8.0,
                durability: 1561,
                damage: 5.0,
            }),
            Item::DiamondAxe => Some(ToolProperties {
                tool_type: ToolType::Axe,
                material: ToolMaterial::Diamond,
                mining_speed: 8.0,
                durability: 1561,
                damage: 6.0,
            }),
            Item::DiamondShovel => Some(ToolProperties {
                tool_type: ToolType::Shovel,
                material: ToolMaterial::Diamond,
                mining_speed: 8.0,
                durability: 1561,
                damage: 4.0,
            }),

            Item::WoodenHoe => Some(ToolProperties {
                tool_type: ToolType::Hoe,
                material: ToolMaterial::Wood,
                mining_speed: 2.0,
                durability: 59,
                damage: 1.0,
            }),
            Item::StoneHoe => Some(ToolProperties {
                tool_type: ToolType::Hoe,
                material: ToolMaterial::Stone,
                mining_speed: 4.0,
                durability: 131,
                damage: 1.0,
            }),
            Item::IronHoe => Some(ToolProperties {
                tool_type: ToolType::Hoe,
                material: ToolMaterial::Iron,
                mining_speed: 6.0,
                durability: 250,
                damage: 1.0,
            }),
            Item::GoldenHoe => Some(ToolProperties {
                tool_type: ToolType::Hoe,
                material: ToolMaterial::Gold,
                mining_speed: 12.0,
                durability: 32,
                damage: 1.0,
            }),
            Item::DiamondHoe => Some(ToolProperties {
                tool_type: ToolType::Hoe,
                material: ToolMaterial::Diamond,
                mining_speed: 8.0,
                durability: 1561,
                damage: 1.0,
            }),

            Item::WoodenSword => Some(ToolProperties {
                tool_type: ToolType::Sword,
                material: ToolMaterial::Wood,
                mining_speed: 2.0,
                durability: 59,
                damage: 4.0,
            }),
            Item::WoodenPickaxe => Some(ToolProperties {
                tool_type: ToolType::Pickaxe,
                material: ToolMaterial::Wood,
                mining_speed: 2.0,
                durability: 59,
                damage: 2.0,
            }),
            Item::WoodenAxe => Some(ToolProperties {
                tool_type: ToolType::Axe,
                material: ToolMaterial::Wood,
                mining_speed: 2.0,
                durability: 59,
                damage: 7.0,
            }),
            Item::WoodenShovel => Some(ToolProperties {
                tool_type: ToolType::Shovel,
                material: ToolMaterial::Wood,
                mining_speed: 2.0,
                durability: 59,
                damage: 2.5,
            }),

            Item::GoldenSword => Some(ToolProperties {
                tool_type: ToolType::Sword,
                material: ToolMaterial::Gold,
                mining_speed: 12.0,
                durability: 32,
                damage: 4.0,
            }),
            Item::GoldenPickaxe => Some(ToolProperties {
                tool_type: ToolType::Pickaxe,
                material: ToolMaterial::Gold,
                mining_speed: 12.0,
                durability: 32,
                damage: 2.0,
            }),
            Item::GoldenAxe => Some(ToolProperties {
                tool_type: ToolType::Axe,
                material: ToolMaterial::Gold,
                mining_speed: 12.0,
                durability: 32,
                damage: 7.0,
            }),
            Item::GoldenShovel => Some(ToolProperties {
                tool_type: ToolType::Shovel,
                material: ToolMaterial::Gold,
                mining_speed: 12.0,
                durability: 32,
                damage: 2.5,
            }),
            _ => None,
        }
    }

    pub fn is_armor(self) -> bool {
        self.armor_properties().is_some()
    }

    pub fn armor_properties(self) -> Option<ArmorProperties> {
        match self {
            Item::LeatherHelmet => Some(ArmorProperties {
                slot: ArmorSlot::Helmet,
                armor_points: 1.0,
                toughness: 0.0,
                knockback_resistance: 0.0,
                durability: 55,
            }),
            Item::LeatherChestplate => Some(ArmorProperties {
                slot: ArmorSlot::Chestplate,
                armor_points: 3.0,
                toughness: 0.0,
                knockback_resistance: 0.0,
                durability: 80,
            }),
            Item::LeatherLeggings => Some(ArmorProperties {
                slot: ArmorSlot::Leggings,
                armor_points: 2.0,
                toughness: 0.0,
                knockback_resistance: 0.0,
                durability: 75,
            }),
            Item::LeatherBoots => Some(ArmorProperties {
                slot: ArmorSlot::Boots,
                armor_points: 1.0,
                toughness: 0.0,
                knockback_resistance: 0.0,
                durability: 65,
            }),
            Item::GoldenHelmet => Some(ArmorProperties {
                slot: ArmorSlot::Helmet,
                armor_points: 2.0,
                toughness: 0.0,
                knockback_resistance: 0.0,
                durability: 77,
            }),
            Item::GoldenChestplate => Some(ArmorProperties {
                slot: ArmorSlot::Chestplate,
                armor_points: 5.0,
                toughness: 0.0,
                knockback_resistance: 0.0,
                durability: 112,
            }),
            Item::GoldenLeggings => Some(ArmorProperties {
                slot: ArmorSlot::Leggings,
                armor_points: 3.0,
                toughness: 0.0,
                knockback_resistance: 0.0,
                durability: 105,
            }),
            Item::GoldenBoots => Some(ArmorProperties {
                slot: ArmorSlot::Boots,
                armor_points: 1.0,
                toughness: 0.0,
                knockback_resistance: 0.0,
                durability: 91,
            }),
            Item::IronHelmet => Some(ArmorProperties {
                slot: ArmorSlot::Helmet,
                armor_points: 2.0,
                toughness: 0.0,
                knockback_resistance: 0.0,
                durability: 165,
            }),
            Item::IronChestplate => Some(ArmorProperties {
                slot: ArmorSlot::Chestplate,
                armor_points: 6.0,
                toughness: 0.0,
                knockback_resistance: 0.0,
                durability: 240,
            }),
            Item::IronLeggings => Some(ArmorProperties {
                slot: ArmorSlot::Leggings,
                armor_points: 5.0,
                toughness: 0.0,
                knockback_resistance: 0.0,
                durability: 225,
            }),
            Item::IronBoots => Some(ArmorProperties {
                slot: ArmorSlot::Boots,
                armor_points: 2.0,
                toughness: 0.0,
                knockback_resistance: 0.0,
                durability: 195,
            }),
            Item::DiamondHelmet => Some(ArmorProperties {
                slot: ArmorSlot::Helmet,
                armor_points: 3.0,
                toughness: 2.0,
                knockback_resistance: 0.0,
                durability: 363,
            }),
            Item::DiamondChestplate => Some(ArmorProperties {
                slot: ArmorSlot::Chestplate,
                armor_points: 8.0,
                toughness: 2.0,
                knockback_resistance: 0.0,
                durability: 528,
            }),
            Item::DiamondLeggings => Some(ArmorProperties {
                slot: ArmorSlot::Leggings,
                armor_points: 6.0,
                toughness: 2.0,
                knockback_resistance: 0.0,
                durability: 495,
            }),
            Item::DiamondBoots => Some(ArmorProperties {
                slot: ArmorSlot::Boots,
                armor_points: 3.0,
                toughness: 2.0,
                knockback_resistance: 0.0,
                durability: 429,
            }),
            _ => None,
        }
    }

    pub fn attack_cooldown_ticks(self) -> u32 {
        if let Some(tool) = self.tool_properties() {
            match tool.tool_type {
                ToolType::Sword => 12,
                ToolType::Pickaxe => 17,
                ToolType::Shovel => 20,
                ToolType::Axe => 25,
                ToolType::Hoe => 5,
                ToolType::None => 5,
            }
        } else {
            5
        }
    }

    pub fn food_properties(self) -> Option<FoodProperties> {
        match self {
            Item::Apple => Some(FoodProperties {
                hunger: 4.0,
                saturation: 2.4,
                use_duration_ticks: 32,
                always_edible: false,
                return_item: None,
            }),
            Item::Bread => Some(FoodProperties {
                hunger: 5.0,
                saturation: 6.0,
                use_duration_ticks: 32,
                always_edible: false,
                return_item: None,
            }),
            Item::RawPorkchop => Some(FoodProperties {
                hunger: 3.0,
                saturation: 1.8,
                use_duration_ticks: 32,
                always_edible: false,
                return_item: None,
            }),
            Item::CookedPorkchop => Some(FoodProperties {
                hunger: 8.0,
                saturation: 12.8,
                use_duration_ticks: 32,
                always_edible: false,
                return_item: None,
            }),
            Item::RawBeef => Some(FoodProperties {
                hunger: 3.0,
                saturation: 1.8,
                use_duration_ticks: 32,
                always_edible: false,
                return_item: None,
            }),
            Item::CookedBeef => Some(FoodProperties {
                hunger: 8.0,
                saturation: 12.8,
                use_duration_ticks: 32,
                always_edible: false,
                return_item: None,
            }),
            Item::RawChicken => Some(FoodProperties {
                hunger: 2.0,
                saturation: 1.2,
                use_duration_ticks: 32,
                always_edible: false,
                return_item: None,
            }),
            Item::CookedChicken => Some(FoodProperties {
                hunger: 6.0,
                saturation: 7.2,
                use_duration_ticks: 32,
                always_edible: false,
                return_item: None,
            }),
            Item::RawMutton => Some(FoodProperties {
                hunger: 2.0,
                saturation: 1.2,
                use_duration_ticks: 32,
                always_edible: false,
                return_item: None,
            }),
            Item::CookedMutton => Some(FoodProperties {
                hunger: 6.0,
                saturation: 9.6,
                use_duration_ticks: 32,
                always_edible: false,
                return_item: None,
            }),
            Item::Carrot => Some(FoodProperties {
                hunger: 3.0,
                saturation: 3.6,
                use_duration_ticks: 32,
                always_edible: false,
                return_item: None,
            }),
            Item::Potato => Some(FoodProperties {
                hunger: 1.0,
                saturation: 0.6,
                use_duration_ticks: 32,
                always_edible: false,
                return_item: None,
            }),
            Item::BakedPotato => Some(FoodProperties {
                hunger: 5.0,
                saturation: 6.0,
                use_duration_ticks: 32,
                always_edible: false,
                return_item: None,
            }),
            Item::PoisonousPotato => Some(FoodProperties {
                hunger: 2.0,
                saturation: 1.2,
                use_duration_ticks: 32,
                always_edible: false,
                return_item: None,
            }),
            Item::GoldenCarrot => Some(FoodProperties {
                hunger: 6.0,
                saturation: 14.4,
                use_duration_ticks: 32,
                always_edible: false,
                return_item: None,
            }),
            Item::GoldenApple => Some(FoodProperties {
                hunger: 4.0,
                saturation: 9.6,
                use_duration_ticks: 32,
                always_edible: true,
                return_item: None,
            }),
            Item::RottenFlesh => Some(FoodProperties {
                hunger: 4.0,
                saturation: 0.8,
                use_duration_ticks: 32,
                always_edible: false,
                return_item: None,
            }),
            _ => None,
        }
    }

    /// True when the item should be drawn as a flat sprite instead of a cube
    /// in the hand and as a dropped item: every non-block item (seeds, tools,
    /// food, ...) plus cross-model plant blocks (flowers, tall grass, sugar
    /// cane).
    pub fn renders_flat(self) -> bool {
        match self.properties().block_type {
            Some(block) => block.is_cross_model(),
            None => self != Item::Air,
        }
    }

    pub fn properties(self) -> ItemProperties {
        match self {
            Item::Air => ItemProperties {
                name: "Air",
                max_stack: 64,
                is_block: false,
                block_type: None,
                tex_coords: (0, 0),
            },
            Item::Grass => ItemProperties {
                name: "Grass Block",
                max_stack: 64,
                is_block: true,
                block_type: Some(BlockType::Grass),
                tex_coords: (1, 0),
            },
            Item::Dirt => ItemProperties {
                name: "Dirt",
                max_stack: 64,
                is_block: true,
                block_type: Some(BlockType::Dirt),
                tex_coords: (2, 0),
            },
            Item::Stone => ItemProperties {
                name: "Stone",
                max_stack: 64,
                is_block: true,
                block_type: Some(BlockType::Stone),
                tex_coords: (3, 0),
            },
            Item::Sand => ItemProperties {
                name: "Sand",
                max_stack: 64,
                is_block: true,
                block_type: Some(BlockType::Sand),
                tex_coords: (4, 0),
            },
            Item::Gravel => ItemProperties {
                name: "Gravel",
                max_stack: 64,
                is_block: true,
                block_type: Some(BlockType::Gravel),
                tex_coords: (5, 0),
            },
            Item::OakLog => ItemProperties {
                name: "Oak Log",
                max_stack: 64,
                is_block: true,
                block_type: Some(BlockType::OakLog),
                tex_coords: (11, 1),
            },
            Item::OakPlanks => ItemProperties {
                name: "Oak Planks",
                max_stack: 64,
                is_block: true,
                block_type: Some(BlockType::OakPlanks),
                tex_coords: (6, 0),
            },
            Item::OakLeaves => ItemProperties {
                name: "Oak Leaves",
                max_stack: 64,
                is_block: true,
                block_type: Some(BlockType::OakLeaves),
                tex_coords: (7, 0),
            },
            Item::Cobblestone => ItemProperties {
                name: "Cobblestone",
                max_stack: 64,
                is_block: true,
                block_type: Some(BlockType::Cobblestone),
                tex_coords: (8, 0),
            },
            Item::Bedrock => ItemProperties {
                name: "Bedrock",
                max_stack: 64,
                is_block: true,
                block_type: Some(BlockType::Bedrock),
                tex_coords: (9, 0),
            },
            Item::Water => ItemProperties {
                name: "Water",
                max_stack: 64,
                is_block: true,
                block_type: Some(BlockType::Water),
                tex_coords: (10, 0),
            },
            Item::CoalOre => ItemProperties {
                name: "Coal Ore",
                max_stack: 64,
                is_block: true,
                block_type: Some(BlockType::CoalOre),
                tex_coords: (11, 0),
            },
            Item::IronOre => ItemProperties {
                name: "Iron Ore",
                max_stack: 64,
                is_block: true,
                block_type: Some(BlockType::IronOre),
                tex_coords: (12, 0),
            },
            Item::GoldOre => ItemProperties {
                name: "Gold Ore",
                max_stack: 64,
                is_block: true,
                block_type: Some(BlockType::GoldOre),
                tex_coords: (13, 0),
            },
            Item::DiamondOre => ItemProperties {
                name: "Diamond Ore",
                max_stack: 64,
                is_block: true,
                block_type: Some(BlockType::DiamondOre),
                tex_coords: (14, 0),
            },
            Item::RedstoneOre => ItemProperties {
                name: "Redstone Ore",
                max_stack: 64,
                is_block: true,
                block_type: Some(BlockType::RedstoneOre),
                tex_coords: (15, 0),
            },
            Item::Glass => ItemProperties {
                name: "Glass",
                max_stack: 64,
                is_block: true,
                block_type: Some(BlockType::Glass),
                tex_coords: (0, 1),
            },
            Item::Brick => ItemProperties {
                name: "Brick Block",
                max_stack: 64,
                is_block: true,
                block_type: Some(BlockType::Brick),
                tex_coords: (1, 1),
            },
            Item::StoneBrick => ItemProperties {
                name: "Stone Brick",
                max_stack: 64,
                is_block: true,
                block_type: Some(BlockType::StoneBrick),
                tex_coords: (2, 1),
            },
            Item::Snow => ItemProperties {
                name: "Snow Block",
                max_stack: 64,
                is_block: true,
                block_type: Some(BlockType::Snow),
                tex_coords: (4, 1),
            },
            Item::Ice => ItemProperties {
                name: "Ice",
                max_stack: 64,
                is_block: true,
                block_type: Some(BlockType::Ice),
                tex_coords: (5, 1),
            },
            Item::Clay => ItemProperties {
                name: "Clay Block",
                max_stack: 64,
                is_block: true,
                block_type: Some(BlockType::Clay),
                tex_coords: (6, 1),
            },
            Item::Sandstone => ItemProperties {
                name: "Sandstone",
                max_stack: 64,
                is_block: true,
                block_type: Some(BlockType::Sandstone),
                tex_coords: (8, 1),
            },
            Item::Obsidian => ItemProperties {
                name: "Obsidian",
                max_stack: 64,
                is_block: true,
                block_type: Some(BlockType::Obsidian),
                tex_coords: (9, 1),
            },
            Item::CraftingTable => ItemProperties {
                name: "Crafting Table",
                max_stack: 64,
                is_block: true,
                block_type: Some(BlockType::CraftingTable),
                tex_coords: (13, 1),
            },
            Item::Furnace => ItemProperties {
                name: "Furnace",
                max_stack: 64,
                is_block: true,
                block_type: Some(BlockType::Furnace),
                tex_coords: (14, 1),
            },
            Item::Chest => ItemProperties {
                name: "Chest",
                max_stack: 64,
                is_block: true,
                block_type: Some(BlockType::Chest),
                tex_coords: (15, 1),
            },
            Item::TNT => ItemProperties {
                name: "TNT",
                max_stack: 64,
                is_block: true,
                block_type: Some(BlockType::TNT),
                tex_coords: (2, 2),
            },
            Item::Bookshelf => ItemProperties {
                name: "Bookshelf",
                max_stack: 64,
                is_block: true,
                block_type: Some(BlockType::Bookshelf),
                tex_coords: (3, 2),
            },
            Item::Torch => ItemProperties {
                name: "Torch",
                max_stack: 64,
                is_block: true,
                block_type: Some(BlockType::Torch),
                tex_coords: (4, 2),
            },
            Item::Lava => ItemProperties {
                name: "Lava",
                max_stack: 64,
                is_block: true,
                block_type: Some(BlockType::Lava),
                tex_coords: (15, 2),
            },

            // Tools (row 4-7)
            Item::StoneSword => ItemProperties {
                name: "Stone Sword",
                max_stack: 1,
                is_block: false,
                block_type: None,
                tex_coords: (0, 4),
            },
            Item::IronSword => ItemProperties {
                name: "Iron Sword",
                max_stack: 1,
                is_block: false,
                block_type: None,
                tex_coords: (1, 4),
            },
            Item::DiamondSword => ItemProperties {
                name: "Diamond Sword",
                max_stack: 1,
                is_block: false,
                block_type: None,
                tex_coords: (2, 4),
            },
            Item::StonePickaxe => ItemProperties {
                name: "Stone Pickaxe",
                max_stack: 1,
                is_block: false,
                block_type: None,
                tex_coords: (0, 5),
            },
            Item::IronPickaxe => ItemProperties {
                name: "Iron Pickaxe",
                max_stack: 1,
                is_block: false,
                block_type: None,
                tex_coords: (1, 5),
            },
            Item::DiamondPickaxe => ItemProperties {
                name: "Diamond Pickaxe",
                max_stack: 1,
                is_block: false,
                block_type: None,
                tex_coords: (2, 5),
            },
            Item::StoneAxe => ItemProperties {
                name: "Stone Axe",
                max_stack: 1,
                is_block: false,
                block_type: None,
                tex_coords: (0, 6),
            },
            Item::IronAxe => ItemProperties {
                name: "Iron Axe",
                max_stack: 1,
                is_block: false,
                block_type: None,
                tex_coords: (1, 6),
            },
            Item::DiamondAxe => ItemProperties {
                name: "Diamond Axe",
                max_stack: 1,
                is_block: false,
                block_type: None,
                tex_coords: (2, 6),
            },
            Item::StoneShovel => ItemProperties {
                name: "Stone Shovel",
                max_stack: 1,
                is_block: false,
                block_type: None,
                tex_coords: (0, 7),
            },
            Item::IronShovel => ItemProperties {
                name: "Iron Shovel",
                max_stack: 1,
                is_block: false,
                block_type: None,
                tex_coords: (1, 7),
            },
            Item::DiamondShovel => ItemProperties {
                name: "Diamond Shovel",
                max_stack: 1,
                is_block: false,
                block_type: None,
                tex_coords: (2, 7),
            },

            // Resources (row 3)
            Item::Stick => ItemProperties {
                name: "Stick",
                max_stack: 64,
                is_block: false,
                block_type: None,
                tex_coords: (0, 3),
            },
            Item::Coal => ItemProperties {
                name: "Coal",
                max_stack: 64,
                is_block: false,
                block_type: None,
                tex_coords: (1, 3),
            },
            Item::IronIngot => ItemProperties {
                name: "Iron Ingot",
                max_stack: 64,
                is_block: false,
                block_type: None,
                tex_coords: (2, 3),
            },
            Item::GoldIngot => ItemProperties {
                name: "Gold Ingot",
                max_stack: 64,
                is_block: false,
                block_type: None,
                tex_coords: (3, 3),
            },
            Item::Diamond => ItemProperties {
                name: "Diamond",
                max_stack: 64,
                is_block: false,
                block_type: None,
                tex_coords: (4, 3),
            },
            Item::Redstone => ItemProperties {
                name: "Redstone Dust",
                max_stack: 64,
                is_block: false,
                block_type: None,
                tex_coords: (5, 3),
            },
            Item::Apple => ItemProperties {
                name: "Apple",
                max_stack: 64,
                is_block: false,
                block_type: None,
                tex_coords: (6, 3),
            },
            Item::Bread => ItemProperties {
                name: "Bread",
                max_stack: 64,
                is_block: false,
                block_type: None,
                tex_coords: (7, 3),
            },

            // Mob Drops on Row 3, Cols 8..11
            Item::RottenFlesh => ItemProperties {
                name: "Rotten Flesh",
                max_stack: 64,
                is_block: false,
                block_type: None,
                tex_coords: (8, 3),
            },
            Item::Bone => ItemProperties {
                name: "Bone",
                max_stack: 64,
                is_block: false,
                block_type: None,
                tex_coords: (9, 3),
            },
            Item::Bow => ItemProperties {
                name: "Bow",
                max_stack: 1,
                is_block: false,
                block_type: None,
                tex_coords: (10, 3),
            },
            Item::Gunpowder => ItemProperties {
                name: "Gunpowder",
                max_stack: 64,
                is_block: false,
                block_type: None,
                tex_coords: (11, 3),
            },

            // Passive Mob Items
            Item::Wheat => ItemProperties {
                name: "Wheat",
                max_stack: 64,
                is_block: false,
                block_type: None,
                tex_coords: (12, 3),
            },
            Item::Seeds => ItemProperties {
                name: "Seeds",
                max_stack: 64,
                is_block: true,
                block_type: Some(BlockType::WheatCrop),
                tex_coords: (13, 3),
            },
            Item::Carrot => ItemProperties {
                name: "Carrot",
                max_stack: 64,
                is_block: true,
                block_type: Some(BlockType::CarrotCrop),
                tex_coords: (14, 3),
            },
            Item::Shears => ItemProperties {
                name: "Shears",
                max_stack: 1,
                is_block: false,
                block_type: None,
                tex_coords: (0, 11),
            },
            Item::Bucket => ItemProperties {
                name: "Bucket",
                max_stack: 16,
                is_block: false,
                block_type: None,
                tex_coords: (1, 11),
            },
            Item::WaterBucket => ItemProperties {
                name: "Water Bucket",
                max_stack: 1,
                is_block: false,
                block_type: None,
                tex_coords: (1, 11),
            },
            Item::LavaBucket => ItemProperties {
                name: "Lava Bucket",
                max_stack: 1,
                is_block: false,
                block_type: None,
                tex_coords: (1, 11),
            },
            Item::MilkBucket => ItemProperties {
                name: "Milk Bucket",
                max_stack: 1,
                is_block: false,
                block_type: None,
                tex_coords: (2, 11),
            },
            Item::RawPorkchop => ItemProperties {
                name: "Raw Porkchop",
                max_stack: 64,
                is_block: false,
                block_type: None,
                tex_coords: (3, 11),
            },
            Item::CookedPorkchop => ItemProperties {
                name: "Cooked Porkchop",
                max_stack: 64,
                is_block: false,
                block_type: None,
                tex_coords: (7, 11),
            },
            Item::RawBeef => ItemProperties {
                name: "Raw Beef",
                max_stack: 64,
                is_block: false,
                block_type: None,
                tex_coords: (4, 11),
            },
            Item::CookedBeef => ItemProperties {
                name: "Cooked Beef",
                max_stack: 64,
                is_block: false,
                block_type: None,
                tex_coords: (8, 11),
            },
            Item::RawMutton => ItemProperties {
                name: "Raw Mutton",
                max_stack: 64,
                is_block: false,
                block_type: None,
                tex_coords: (5, 11),
            },
            Item::CookedMutton => ItemProperties {
                name: "Cooked Mutton",
                max_stack: 64,
                is_block: false,
                block_type: None,
                tex_coords: (9, 11),
            },
            Item::RawChicken => ItemProperties {
                name: "Raw Chicken",
                max_stack: 64,
                is_block: false,
                block_type: None,
                tex_coords: (6, 11),
            },
            Item::CookedChicken => ItemProperties {
                name: "Cooked Chicken",
                max_stack: 64,
                is_block: false,
                block_type: None,
                tex_coords: (10, 11),
            },
            Item::Wool => ItemProperties {
                name: "Wool Block",
                max_stack: 64,
                is_block: true,
                block_type: Some(BlockType::Snow),
                tex_coords: (10, 11),
            },
            Item::Leather => ItemProperties {
                name: "Leather",
                max_stack: 64,
                is_block: false,
                block_type: None,
                tex_coords: (11, 11),
            },
            Item::Feather => ItemProperties {
                name: "Feather",
                max_stack: 64,
                is_block: false,
                block_type: None,
                tex_coords: (12, 11),
            },
            Item::Egg => ItemProperties {
                name: "Egg",
                max_stack: 16,
                is_block: false,
                block_type: None,
                tex_coords: (13, 11),
            },
            Item::RedDye => ItemProperties {
                name: "Red Dye",
                max_stack: 64,
                is_block: false,
                block_type: None,
                tex_coords: (14, 11),
            },
            Item::BlueDye => ItemProperties {
                name: "Blue Dye",
                max_stack: 64,
                is_block: false,
                block_type: None,
                tex_coords: (15, 11),
            },
            Item::GreenDye => ItemProperties {
                name: "Green Dye",
                max_stack: 64,
                is_block: false,
                block_type: None,
                tex_coords: (15, 11),
            },
            // Trees & Biomes Additions
            Item::BirchLog => ItemProperties {
                name: "Birch Log",
                max_stack: 64,
                is_block: true,
                block_type: Some(BlockType::BirchLog),
                tex_coords: (1, 12),
            },
            Item::BirchPlanks => ItemProperties {
                name: "Birch Planks",
                max_stack: 64,
                is_block: true,
                block_type: Some(BlockType::BirchPlanks),
                tex_coords: (2, 12),
            },
            Item::BirchLeaves => ItemProperties {
                name: "Birch Leaves",
                max_stack: 64,
                is_block: true,
                block_type: Some(BlockType::BirchLeaves),
                tex_coords: (3, 12),
            },
            Item::SpruceLog => ItemProperties {
                name: "Spruce Log",
                max_stack: 64,
                is_block: true,
                block_type: Some(BlockType::SpruceLog),
                tex_coords: (5, 12),
            },
            Item::SprucePlanks => ItemProperties {
                name: "Spruce Planks",
                max_stack: 64,
                is_block: true,
                block_type: Some(BlockType::SprucePlanks),
                tex_coords: (6, 12),
            },
            Item::SpruceLeaves => ItemProperties {
                name: "Spruce Leaves",
                max_stack: 64,
                is_block: true,
                block_type: Some(BlockType::SpruceLeaves),
                tex_coords: (7, 12),
            },
            Item::TallGrass => ItemProperties {
                name: "Tall Grass",
                max_stack: 64,
                is_block: true,
                block_type: Some(BlockType::TallGrass),
                tex_coords: (8, 12),
            },
            Item::Dandelion => ItemProperties {
                name: "Dandelion",
                max_stack: 64,
                is_block: true,
                block_type: Some(BlockType::Dandelion),
                tex_coords: (9, 12),
            },
            Item::Poppy => ItemProperties {
                name: "Poppy",
                max_stack: 64,
                is_block: true,
                block_type: Some(BlockType::Poppy),
                tex_coords: (10, 12),
            },
            Item::Cactus => ItemProperties {
                name: "Cactus",
                max_stack: 64,
                is_block: true,
                block_type: Some(BlockType::Cactus),
                tex_coords: (11, 12),
            },
            Item::SugarCane => ItemProperties {
                name: "Sugar Cane",
                max_stack: 64,
                is_block: true,
                block_type: Some(BlockType::SugarCane),
                tex_coords: (12, 12),
            },
            Item::Pumpkin => ItemProperties {
                name: "Pumpkin",
                max_stack: 64,
                is_block: true,
                block_type: Some(BlockType::Pumpkin),
                tex_coords: (13, 12),
            },
            Item::Melon => ItemProperties {
                name: "Melon",
                max_stack: 64,
                is_block: true,
                block_type: Some(BlockType::Melon),
                tex_coords: (14, 12),
            },
            item @ (Item::EnchantingTable | Item::BrewingStand | Item::Anvil) => {
                let (name, block_type, tex_coords) = match item {
                    Item::EnchantingTable => {
                        ("Enchanting Table", BlockType::EnchantingTable, (0, 13))
                    }
                    Item::BrewingStand => ("Brewing Stand", BlockType::BrewingStand, (1, 13)),
                    _ => ("Anvil", BlockType::Anvil, (2, 13)),
                };
                ItemProperties {
                    name,
                    max_stack: 64,
                    is_block: true,
                    block_type: Some(block_type),
                    tex_coords,
                }
            }
            item @ (Item::LapisLazuli
            | Item::IronHelmet
            | Item::IronChestplate
            | Item::IronLeggings
            | Item::IronBoots
            | Item::GlassBottle
            | Item::Potion
            | Item::SplashPotion
            | Item::NetherWart
            | Item::Sugar
            | Item::BlazePowder
            | Item::GlisteringMelon
            | Item::GhastTear
            | Item::GoldenCarrot
            | Item::FermentedSpiderEye
            | Item::MagmaCream
            | Item::Pufferfish
            | Item::SpiderEye
            | Item::GlowstoneDust
            | Item::RedstoneDust) => {
                let (name, max_stack, tex_coords) = match item {
                    Item::LapisLazuli => ("Lapis Lazuli", 64, (3, 13)),
                    Item::IronHelmet => ("Iron Helmet", 1, (4, 13)),
                    Item::IronChestplate => ("Iron Chestplate", 1, (5, 13)),
                    Item::IronLeggings => ("Iron Leggings", 1, (6, 13)),
                    Item::IronBoots => ("Iron Boots", 1, (7, 13)),
                    Item::GlassBottle => ("Glass Bottle", 64, (8, 13)),
                    Item::Potion => ("Potion", 1, (9, 13)),
                    Item::SplashPotion => ("Splash Potion", 1, (10, 13)),
                    Item::NetherWart => ("Nether Wart", 64, (11, 13)),
                    Item::Sugar => ("Sugar", 64, (12, 13)),
                    Item::BlazePowder => ("Blaze Powder", 64, (13, 13)),
                    Item::GlisteringMelon => ("Glistering Melon", 64, (14, 13)),
                    Item::GhastTear => ("Ghast Tear", 64, (15, 13)),
                    Item::GoldenCarrot => ("Golden Carrot", 64, (0, 14)),
                    Item::FermentedSpiderEye => ("Fermented Spider Eye", 64, (1, 14)),
                    Item::MagmaCream => ("Magma Cream", 64, (2, 14)),
                    Item::Pufferfish => ("Pufferfish", 64, (3, 14)),
                    Item::SpiderEye => ("Spider Eye", 64, (4, 14)),
                    Item::GlowstoneDust => ("Glowstone Dust", 64, (5, 14)),
                    Item::RedstoneDust => ("Redstone Dust", 64, (6, 14)),
                    _ => unreachable!(),
                };
                ItemProperties {
                    name,
                    max_stack,
                    is_block: false,
                    block_type: None,
                    tex_coords,
                }
            }
            Item::Arrow => ItemProperties {
                name: "Arrow",
                max_stack: 64,
                is_block: false,
                block_type: None,
                tex_coords: (7, 14),
            },
            item @ (Item::RedstoneWire
            | Item::RedstoneTorch
            | Item::Repeater
            | Item::Comparator
            | Item::StoneButton
            | Item::Lever
            | Item::PressurePlate
            | Item::Piston
            | Item::StickyPiston
            | Item::RedstoneLamp
            | Item::OakDoor
            | Item::OakTrapdoor
            | Item::Dispenser
            | Item::Dropper
            | Item::NoteBlock
            | Item::Hopper
            | Item::Observer) => {
                let (name, block_type, tex_coords) = match item {
                    Item::RedstoneWire => ("Redstone Wire", BlockType::RedstoneWire, (5, 2)),
                    Item::RedstoneTorch => ("Redstone Torch", BlockType::RedstoneTorch, (6, 2)),
                    Item::Repeater => ("Redstone Repeater", BlockType::Repeater, (7, 2)),
                    Item::Comparator => ("Redstone Comparator", BlockType::Comparator, (8, 2)),
                    Item::StoneButton => ("Stone Button", BlockType::StoneButton, (9, 2)),
                    Item::Lever => ("Lever", BlockType::Lever, (10, 2)),
                    Item::PressurePlate => {
                        ("Stone Pressure Plate", BlockType::PressurePlate, (11, 2))
                    }
                    Item::Piston => ("Piston", BlockType::Piston, (12, 2)),
                    Item::StickyPiston => ("Sticky Piston", BlockType::StickyPiston, (13, 2)),
                    Item::RedstoneLamp => ("Redstone Lamp", BlockType::RedstoneLamp, (14, 2)),
                    Item::OakDoor => ("Oak Door", BlockType::OakDoor, (9, 14)),
                    Item::OakTrapdoor => ("Oak Trapdoor", BlockType::OakTrapdoor, (10, 14)),
                    Item::Dispenser => ("Dispenser", BlockType::Dispenser, (11, 14)),
                    Item::Dropper => ("Dropper", BlockType::Dropper, (12, 14)),
                    Item::NoteBlock => ("Note Block", BlockType::NoteBlock, (13, 14)),
                    Item::Hopper => ("Hopper", BlockType::Hopper, (11, 15)),
                    Item::Observer => ("Observer", BlockType::Observer, (11, 16)),
                    _ => unreachable!(),
                };
                ItemProperties {
                    name,
                    max_stack: 64,
                    is_block: true,
                    block_type: Some(block_type),
                    tex_coords,
                }
            }
            item @ (Item::Netherrack
            | Item::SoulSand
            | Item::Glowstone
            | Item::EndStone
            | Item::EndPortalFrame
            | Item::Purpur
            | Item::DragonEgg
            | Item::WitherSkeletonSkull
            | Item::NetherBrick) => {
                let (name, block_type, tex_coords) = match item {
                    Item::Netherrack => ("Netherrack", BlockType::Netherrack, (10, 15)),
                    Item::SoulSand => ("Soul Sand", BlockType::SoulSand, (11, 15)),
                    Item::Glowstone => ("Glowstone", BlockType::Glowstone, (12, 15)),
                    Item::EndStone => ("End Stone", BlockType::EndStone, (14, 15)),
                    Item::EndPortalFrame => {
                        ("End Portal Frame", BlockType::EndPortalFrame, (15, 15))
                    }
                    Item::Purpur => ("Purpur Block", BlockType::Purpur, (15, 10)),
                    Item::DragonEgg => ("Dragon Egg", BlockType::DragonEgg, (14, 11)),
                    Item::WitherSkeletonSkull => (
                        "Wither Skeleton Skull",
                        BlockType::WitherSkeletonSkull,
                        (15, 11),
                    ),
                    Item::NetherBrick => ("Nether Bricks", BlockType::NetherBrick, (9, 10)),
                    _ => unreachable!(),
                };
                ItemProperties {
                    name,
                    max_stack: 64,
                    is_block: true,
                    block_type: Some(block_type),
                    tex_coords,
                }
            }
            Item::Bed => ItemProperties {
                name: "Bed",
                max_stack: 1,
                is_block: true,
                block_type: Some(BlockType::Bed),
                tex_coords: (6, 0),
            },
            item @ (Item::WoodenHoe
            | Item::StoneHoe
            | Item::IronHoe
            | Item::GoldenHoe
            | Item::DiamondHoe
            | Item::WoodenSword
            | Item::WoodenPickaxe
            | Item::WoodenAxe
            | Item::WoodenShovel
            | Item::GoldenSword
            | Item::GoldenPickaxe
            | Item::GoldenAxe
            | Item::GoldenShovel
            | Item::LeatherHelmet
            | Item::LeatherChestplate
            | Item::LeatherLeggings
            | Item::LeatherBoots
            | Item::GoldenHelmet
            | Item::GoldenChestplate
            | Item::GoldenLeggings
            | Item::GoldenBoots
            | Item::DiamondHelmet
            | Item::DiamondChestplate
            | Item::DiamondLeggings
            | Item::DiamondBoots
            | Item::Shield
            | Item::BoneMeal
            | Item::Potato
            | Item::BakedPotato
            | Item::PoisonousPotato
            | Item::GoldenApple
            | Item::FlintAndSteel
            | Item::EyeOfEnder
            | Item::Elytra
            | Item::NetherStar
            | Item::EndCrystal
            | Item::BlazeRod
            | Item::ShulkerShell
            | Item::Saddle
            | Item::Emerald
            | Item::Book
            | Item::Paper
            | Item::EnchantedBook
            | Item::Compass
            | Item::OakSlab
            | Item::CobblestoneSlab
            | Item::OakStair
            | Item::CobblestoneStair
            | Item::OakFence
            | Item::OakFenceGate
            | Item::CobblestoneWall
            | Item::GlassPane
            | Item::OakLadder
            | Item::OakSign
            | Item::String
            | Item::Slimeball
            | Item::RawCod
            | Item::RawSalmon
            | Item::InkSac
            | Item::OakBoat
            | Item::Minecart
            | Item::Rail
            | Item::PoweredRail
            | Item::DetectorRail
            | Item::ActivatorRail
            | Item::Clock
            | Item::Map
            | Item::FishingRod
            | Item::RawFish
            | Item::TropicalFish
            | Item::LilyPad) => {
                let (name, max_stack, is_block, block_type, tex_coords) = match item {
                    Item::WoodenHoe => ("Wooden Hoe", 1, false, None, (0, 8)),
                    Item::StoneHoe => ("Stone Hoe", 1, false, None, (1, 8)),
                    Item::IronHoe => ("Iron Hoe", 1, false, None, (2, 8)),
                    Item::GoldenHoe => ("Golden Hoe", 1, false, None, (4, 8)),
                    Item::DiamondHoe => ("Diamond Hoe", 1, false, None, (3, 8)),
                    Item::BoneMeal => ("Bone Meal", 64, false, None, (15, 10)),
                    Item::Potato => ("Potato", 64, true, Some(BlockType::PotatoCrop), (15, 3)),
                    Item::BakedPotato => ("Baked Potato", 64, false, None, (7, 11)),
                    Item::PoisonousPotato => ("Poisonous Potato", 64, false, None, (8, 11)),
                    Item::GoldenApple => ("Golden Apple", 64, false, None, (11, 0)),
                    Item::FlintAndSteel => ("Flint and Steel", 1, false, None, (11, 10)),
                    Item::EyeOfEnder => ("Eye of Ender", 64, false, None, (12, 10)),
                    Item::Elytra => ("Elytra", 1, false, None, (13, 10)),
                    Item::NetherStar => ("Nether Star", 64, false, None, (3, 4)),
                    Item::EndCrystal => ("End Crystal", 64, false, None, (4, 4)),
                    Item::BlazeRod => ("Blaze Rod", 64, false, None, (5, 4)),
                    Item::ShulkerShell => ("Shulker Shell", 64, false, None, (14, 14)),
                    Item::Saddle => ("Saddle", 1, false, None, (8, 6)),
                    Item::Emerald => ("Emerald", 64, false, None, (11, 1)),
                    Item::Book => ("Book", 64, false, None, (11, 3)),
                    Item::Paper => ("Paper", 64, false, None, (10, 3)),
                    Item::EnchantedBook => ("Enchanted Book", 1, false, None, (11, 3)),
                    Item::Compass => ("Compass", 64, false, None, (6, 3)),
                    Item::OakSlab => ("Oak Slab", 64, true, Some(BlockType::OakSlab), (6, 0)),
                    Item::CobblestoneSlab => (
                        "Cobblestone Slab",
                        64,
                        true,
                        Some(BlockType::CobblestoneSlab),
                        (8, 0),
                    ),
                    Item::OakStair => ("Oak Stairs", 64, true, Some(BlockType::OakStair), (6, 0)),
                    Item::CobblestoneStair => (
                        "Cobblestone Stairs",
                        64,
                        true,
                        Some(BlockType::CobblestoneStair),
                        (8, 0),
                    ),
                    Item::OakFence => ("Oak Fence", 64, true, Some(BlockType::OakFence), (6, 0)),
                    Item::OakFenceGate => (
                        "Oak Fence Gate",
                        64,
                        true,
                        Some(BlockType::OakFenceGate),
                        (6, 0),
                    ),
                    Item::CobblestoneWall => (
                        "Cobblestone Wall",
                        64,
                        true,
                        Some(BlockType::CobblestoneWall),
                        (8, 0),
                    ),
                    Item::GlassPane => ("Glass Pane", 64, true, Some(BlockType::GlassPane), (0, 1)),
                    Item::OakLadder => ("Ladder", 64, true, Some(BlockType::OakLadder), (3, 5)),
                    Item::OakSign => ("Oak Sign", 16, true, Some(BlockType::OakSign), (6, 0)),
                    Item::WoodenSword => ("Wooden Sword", 1, false, None, (0, 7)),
                    Item::WoodenPickaxe => ("Wooden Pickaxe", 1, false, None, (0, 6)),
                    Item::WoodenAxe => ("Wooden Axe", 1, false, None, (0, 5)),
                    Item::WoodenShovel => ("Wooden Shovel", 1, false, None, (0, 4)),
                    Item::GoldenSword => ("Golden Sword", 1, false, None, (4, 7)),
                    Item::GoldenPickaxe => ("Golden Pickaxe", 1, false, None, (4, 6)),
                    Item::GoldenAxe => ("Golden Axe", 1, false, None, (4, 5)),
                    Item::GoldenShovel => ("Golden Shovel", 1, false, None, (4, 4)),
                    Item::LeatherHelmet => ("Leather Cap", 1, false, None, (0, 13)),
                    Item::LeatherChestplate => ("Leather Tunic", 1, false, None, (0, 14)),
                    Item::LeatherLeggings => ("Leather Pants", 1, false, None, (0, 15)),
                    Item::LeatherBoots => ("Leather Boots", 1, false, None, (0, 12)),
                    Item::GoldenHelmet => ("Golden Helmet", 1, false, None, (3, 13)),
                    Item::GoldenChestplate => ("Golden Chestplate", 1, false, None, (3, 14)),
                    Item::GoldenLeggings => ("Golden Leggings", 1, false, None, (3, 15)),
                    Item::GoldenBoots => ("Golden Boots", 1, false, None, (3, 12)),
                    Item::DiamondHelmet => ("Diamond Helmet", 1, false, None, (2, 13)),
                    Item::DiamondChestplate => ("Diamond Chestplate", 1, false, None, (2, 14)),
                    Item::DiamondLeggings => ("Diamond Leggings", 1, false, None, (2, 15)),
                    Item::DiamondBoots => ("Diamond Boots", 1, false, None, (2, 12)),
                    Item::Shield => ("Shield", 1, false, None, (15, 12)),
                    Item::String => ("String", 64, false, None, (8, 3)),
                    Item::Slimeball => ("Slimeball", 64, false, None, (14, 1)),
                    Item::RawCod => ("Raw Cod", 64, false, None, (1, 10)),
                    Item::RawSalmon => ("Raw Salmon", 64, false, None, (2, 10)),
                    Item::InkSac => ("Ink Sac", 64, false, None, (15, 1)),
                    Item::OakBoat => ("Oak Boat", 1, false, None, (8, 6)),
                    Item::Minecart => ("Minecart", 1, false, None, (8, 7)),
                    Item::Rail => ("Rail", 64, true, Some(BlockType::Rail), (0, 8)),
                    Item::PoweredRail => (
                        "Powered Rail",
                        64,
                        true,
                        Some(BlockType::PoweredRail),
                        (3, 8),
                    ),
                    Item::DetectorRail => (
                        "Detector Rail",
                        64,
                        true,
                        Some(BlockType::DetectorRail),
                        (3, 9),
                    ),
                    Item::ActivatorRail => (
                        "Activator Rail",
                        64,
                        true,
                        Some(BlockType::ActivatorRail),
                        (3, 10),
                    ),
                    Item::Clock => ("Clock", 64, false, None, (8, 4)),
                    Item::Map => ("Map", 64, false, None, (8, 5)),
                    Item::FishingRod => ("Fishing Rod", 1, false, None, (5, 4)),
                    Item::RawFish => ("Raw Fish", 64, false, None, (1, 10)),
                    Item::TropicalFish => ("Tropical Fish", 64, false, None, (3, 10)),
                    Item::Pufferfish => ("Pufferfish", 64, false, None, (4, 10)),
                    Item::LilyPad => ("Lily Pad", 64, false, None, (12, 0)),
                    _ => ("Unknown", 64, false, None, (0, 0)),
                };
                ItemProperties {
                    name,
                    max_stack,
                    is_block,
                    block_type,
                    tex_coords,
                }
            }
        }
    }

    pub fn from_block(b: BlockType) -> Self {
        match b {
            BlockType::Air => Item::Air,
            BlockType::Grass => Item::Grass,
            BlockType::Dirt => Item::Dirt,
            BlockType::Stone => Item::Stone,
            BlockType::Sand => Item::Sand,
            BlockType::Gravel => Item::Gravel,
            BlockType::OakLog => Item::OakLog,
            BlockType::OakPlanks => Item::OakPlanks,
            BlockType::OakLeaves => Item::OakLeaves,
            BlockType::Cobblestone => Item::Cobblestone,
            BlockType::Bedrock => Item::Bedrock,
            BlockType::Water => Item::Water,
            BlockType::CoalOre => Item::CoalOre,
            BlockType::IronOre => Item::IronOre,
            BlockType::GoldOre => Item::GoldOre,
            BlockType::DiamondOre => Item::DiamondOre,
            BlockType::RedstoneOre => Item::RedstoneOre,
            BlockType::Glass => Item::Glass,
            BlockType::Brick => Item::Brick,
            BlockType::StoneBrick => Item::StoneBrick,
            BlockType::Snow => Item::Snow,
            BlockType::Ice => Item::Ice,
            BlockType::Clay => Item::Clay,
            BlockType::Sandstone => Item::Sandstone,
            BlockType::Obsidian => Item::Obsidian,
            BlockType::CraftingTable => Item::CraftingTable,
            BlockType::Furnace | BlockType::FurnaceLit => Item::Furnace,
            BlockType::Chest => Item::Chest,
            BlockType::TNT => Item::TNT,
            BlockType::Bookshelf => Item::Bookshelf,
            BlockType::Torch => Item::Torch,
            BlockType::Lava => Item::Lava,
            BlockType::Bed => Item::Bed,
            // Trees & Biomes Additions
            BlockType::BirchLog => Item::BirchLog,
            BlockType::BirchPlanks => Item::BirchPlanks,
            BlockType::BirchLeaves => Item::BirchLeaves,
            BlockType::SpruceLog => Item::SpruceLog,
            BlockType::SprucePlanks => Item::SprucePlanks,
            BlockType::SpruceLeaves => Item::SpruceLeaves,
            BlockType::TallGrass => Item::TallGrass,
            BlockType::Dandelion => Item::Dandelion,
            BlockType::Poppy => Item::Poppy,
            BlockType::Cactus => Item::Cactus,
            BlockType::Rail => Item::Rail,
            BlockType::PoweredRail => Item::PoweredRail,
            BlockType::DetectorRail => Item::DetectorRail,
            BlockType::ActivatorRail => Item::ActivatorRail,
            BlockType::SugarCane => Item::SugarCane,
            BlockType::Pumpkin => Item::Pumpkin,
            BlockType::Melon => Item::Melon,
            BlockType::EnchantingTable => Item::EnchantingTable,
            BlockType::BrewingStand => Item::BrewingStand,
            BlockType::Anvil => Item::Anvil,
            BlockType::RedstoneWire => Item::RedstoneWire,
            BlockType::RedstoneTorch | BlockType::RedstoneTorchOff => Item::RedstoneTorch,
            BlockType::Repeater | BlockType::RepeaterPowered => Item::Repeater,
            BlockType::Comparator | BlockType::ComparatorPowered => Item::Comparator,
            BlockType::StoneButton | BlockType::StoneButtonPressed => Item::StoneButton,
            BlockType::Lever | BlockType::LeverOn => Item::Lever,
            BlockType::PressurePlate | BlockType::PressurePlatePowered => Item::PressurePlate,
            BlockType::Piston | BlockType::PistonExtended => Item::Piston,
            BlockType::StickyPiston | BlockType::StickyPistonExtended => Item::StickyPiston,
            BlockType::RedstoneLamp | BlockType::RedstoneLampLit => Item::RedstoneLamp,
            BlockType::OakDoor | BlockType::OakDoorOpen => Item::OakDoor,
            BlockType::OakTrapdoor | BlockType::OakTrapdoorOpen => Item::OakTrapdoor,
            BlockType::Dispenser => Item::Dispenser,
            BlockType::Dropper => Item::Dropper,
            BlockType::NoteBlock => Item::NoteBlock,
            BlockType::Hopper => Item::Hopper,
            BlockType::Observer => Item::Observer,
            BlockType::SnowLayer => Item::Snow,
            BlockType::Fire => Item::Air,
            BlockType::Netherrack => Item::Netherrack,
            BlockType::SoulSand => Item::SoulSand,
            BlockType::Glowstone => Item::Glowstone,
            BlockType::NetherPortal => Item::Air,
            BlockType::EndStone => Item::EndStone,
            BlockType::EndPortalFrame | BlockType::EndPortalFrameFilled => Item::EndPortalFrame,
            BlockType::EndPortal => Item::Air,
            BlockType::Purpur => Item::Purpur,
            BlockType::DragonEgg => Item::DragonEgg,
            BlockType::WitherSkeletonSkull => Item::WitherSkeletonSkull,
            BlockType::NetherBrick => Item::NetherBrick,
            BlockType::EndCityChest => Item::Air,
            BlockType::Farmland => Item::Dirt,
            BlockType::WheatCrop => Item::Wheat,
            BlockType::CarrotCrop => Item::Carrot,
            BlockType::PotatoCrop => Item::Potato,
            BlockType::OakSlab => Item::OakSlab,
            BlockType::CobblestoneSlab => Item::CobblestoneSlab,
            BlockType::OakStair => Item::OakStair,
            BlockType::CobblestoneStair => Item::CobblestoneStair,
            BlockType::OakFence => Item::OakFence,
            BlockType::OakFenceGate => Item::OakFenceGate,
            BlockType::CobblestoneWall => Item::CobblestoneWall,
            BlockType::GlassPane => Item::GlassPane,
            BlockType::OakLadder => Item::OakLadder,
            BlockType::OakSign => Item::OakSign,
            BlockType::OakSapling | BlockType::BirchSapling | BlockType::SpruceSapling => Item::Air,
            BlockType::Spawner => Item::Air,
            BlockType::MossyCobblestone => Item::Cobblestone,
            BlockType::DirtPath => Item::Dirt,
            BlockType::NetherWartCrop => Item::NetherWart,
            BlockType::EndStoneBrick => Item::EndStone,
            BlockType::RespawnAnchor => Item::Obsidian,
            BlockType::EndGateway => Item::Air,
        }
    }
}
