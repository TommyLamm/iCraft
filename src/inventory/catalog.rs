use crate::world::{BlockType, BLOCK_TYPE_COUNT};
use std::sync::OnceLock;

#[path = "item_table.rs"]
mod item_table;
pub use item_table::{ItemDef, ITEM_COUNT, ITEM_DEFS};

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
    #[inline]
    pub fn def(self) -> &'static ItemDef {
        &ITEM_DEFS[self as usize]
    }

    pub fn creative_tab(self) -> Option<CreativeTab> {
        self.def().creative_tab
    }

    pub fn tool_properties(self) -> Option<ToolProperties> {
        self.def().tool
    }

    pub fn is_armor(self) -> bool {
        self.armor_properties().is_some()
    }

    pub fn armor_properties(self) -> Option<ArmorProperties> {
        self.def().armor
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
        self.def().food
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
        let d = self.def();
        ItemProperties {
            name: d.name,
            max_stack: d.max_stack,
            is_block: d.is_block,
            block_type: d.block_type,
            tex_coords: d.tex_coords,
        }
    }

    pub fn from_block(b: BlockType) -> Self {
        from_block_map()[b.canonicalize() as usize]
    }
}

/// Reverse map BlockType -> Item, built once from ITEM_DEFS (first-wins) plus
/// explicit overrides for blocks that are not 1:1 with an item's `block_type`.
fn from_block_map() -> &'static [Item; BLOCK_TYPE_COUNT] {
    static MAP: OnceLock<[Item; BLOCK_TYPE_COUNT]> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut map = [Item::Air; BLOCK_TYPE_COUNT];
        for (idx, def) in ITEM_DEFS.iter().enumerate() {
            if let Some(bt) = def.block_type {
                let slot = &mut map[bt as usize];
                // First-wins keeps Item::Snow over the historical Wool→Snow typo.
                if *slot == Item::Air {
                    *slot = ALL_ITEMS[idx];
                }
            }
        }
        // Non-1:1 block drops / aliases (byte-identical to the former match).
        // Seeds place WheatCrop, but breaking the crop yields Wheat.
        map[BlockType::WheatCrop as usize] = Item::Wheat;
        map[BlockType::SnowLayer as usize] = Item::Snow;
        map[BlockType::Farmland as usize] = Item::Dirt;
        map[BlockType::MossyCobblestone as usize] = Item::Cobblestone;
        map[BlockType::DirtPath as usize] = Item::Dirt;
        map[BlockType::NetherWartCrop as usize] = Item::NetherWart;
        map[BlockType::EndStoneBrick as usize] = Item::EndStone;
        map[BlockType::RespawnAnchor as usize] = Item::Obsidian;
        // Explicit Air (already default): Fire, portals, saplings, spawner, …
        map[BlockType::Fire as usize] = Item::Air;
        map[BlockType::NetherPortal as usize] = Item::Air;
        map[BlockType::EndPortal as usize] = Item::Air;
        map[BlockType::EndCityChest as usize] = Item::Air;
        map[BlockType::OakSapling as usize] = Item::Air;
        map[BlockType::BirchSapling as usize] = Item::Air;
        map[BlockType::SpruceSapling as usize] = Item::Air;
        map[BlockType::Spawner as usize] = Item::Air;
        map[BlockType::EndGateway as usize] = Item::Air;
        map
    })
}

#[cfg(test)]
mod item_def_tests {
    use super::*;
    use crate::world::BlockType;

    #[test]
    fn item_defs_cover_every_variant() {
        assert_eq!(ITEM_DEFS.len(), ITEM_COUNT);
        assert_eq!(ITEM_COUNT, Item::Observer as usize + 1);
        assert_eq!(ALL_ITEMS.len(), ITEM_COUNT);
        for (idx, &item) in ALL_ITEMS.iter().enumerate() {
            assert_eq!(item as usize, idx);
            assert!(
                std::ptr::eq(item.def(), &ITEM_DEFS[idx]),
                "variant {item:?} must index its own row"
            );
        }
    }

    #[test]
    fn item_static_property_snapshot_is_byte_identical() {
        let mut lines = Vec::with_capacity(ITEM_COUNT);
        for (idx, &item) in ALL_ITEMS.iter().enumerate() {
            let d = item.def();
            let tool_s = match d.tool {
                None => "None".into(),
                Some(t) => format!(
                    "{:?}/{:?}/{:.3}/{}/{:.3}",
                    t.tool_type, t.material, t.mining_speed, t.durability, t.damage
                ),
            };
            let armor_s = match d.armor {
                None => "None".into(),
                Some(a) => format!(
                    "{:?}/{:.3}/{:.3}/{:.3}/{}",
                    a.slot, a.armor_points, a.toughness, a.knockback_resistance, a.durability
                ),
            };
            let food_s = match d.food {
                None => "None".into(),
                Some(f) => format!(
                    "{:.3}/{:.3}/{}/{}/{:?}",
                    f.hunger,
                    f.saturation,
                    f.use_duration_ticks,
                    f.always_edible as u8,
                    f.return_item
                ),
            };
            lines.push(format!(
                "{idx}|{item:?}|{name}|{max}|{is_block}|{block:?}|{tc0},{tc1}|{tab:?}|{tool_s}|{armor_s}|{food_s}",
                name = d.name,
                max = d.max_stack,
                is_block = d.is_block as u8,
                block = d.block_type,
                tc0 = d.tex_coords.0,
                tc1 = d.tex_coords.1,
                tab = d.creative_tab,
            ));
        }
        let snapshot = lines.join("\n");
        let expected = include_str!("item_property_snapshot.txt")
            .replace("\r\n", "\n")
            .trim_end()
            .to_string();
        assert_eq!(
            snapshot, expected,
            "ItemDef table drifted from the locked snapshot"
        );
        for &item in ALL_ITEMS {
            let d = item.def();
            let p = item.properties();
            assert_eq!(p.name, d.name);
            assert_eq!(p.max_stack, d.max_stack);
            assert_eq!(p.is_block, d.is_block);
            assert_eq!(p.block_type, d.block_type);
            assert_eq!(p.tex_coords, d.tex_coords);
            assert_eq!(item.creative_tab(), d.creative_tab);
            assert_eq!(
                item.tool_properties().map(|t| (
                    t.tool_type,
                    t.material,
                    t.mining_speed.to_bits(),
                    t.durability,
                    t.damage.to_bits()
                )),
                d.tool.map(|t| (
                    t.tool_type,
                    t.material,
                    t.mining_speed.to_bits(),
                    t.durability,
                    t.damage.to_bits()
                ))
            );
            assert_eq!(item.armor_properties(), d.armor);
            assert_eq!(item.food_properties(), d.food);
        }
    }

    #[test]
    fn from_block_snapshot_is_byte_identical() {
        let mut lines = Vec::new();
        for id in 0..BLOCK_TYPE_COUNT as u8 {
            let raw: BlockType = unsafe { std::mem::transmute(id) };
            if raw.is_reserved_hole() {
                continue;
            }
            let b = BlockType::from_u8(id);
            let item = Item::from_block(b);
            lines.push(format!("{id}|{b:?}|{item:?}"));
        }
        let snapshot = lines.join("\n");
        let expected = include_str!("from_block_snapshot.txt")
            .replace("\r\n", "\n")
            .trim_end()
            .to_string();
        assert_eq!(
            snapshot, expected,
            "from_block reverse map drifted from the locked snapshot"
        );
    }
}
