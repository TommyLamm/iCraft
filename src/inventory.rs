use crate::world::BlockType;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum GameMode {
    Creative,
    Survival,
}

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

    // Resources
    Stick,
    Coal,
    IronIngot,
    GoldIngot,
    Diamond,
    Redstone,

    // Food
    Apple,
    Bread,

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

pub const CREATIVE_ITEMS: [Item; 144] = [
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CreativeDragOrigin {
    Catalog,
    Inventory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolType {
    None,
    Pickaxe,
    Axe,
    Shovel,
    Sword,
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
pub struct ItemStack {
    pub item: Item,
    pub count: u32,
    pub durability: u32,
    pub enchantments: crate::enchantment::EnchantmentSet,
    pub potion: Option<crate::brewing::PotionData>,
    pub custom_name: crate::enchantment::ItemName,
}

impl ItemStack {
    pub fn new(item: Item, count: u32) -> Self {
        let durability = item.tool_properties().map(|t| t.durability).unwrap_or(0);
        let potion = match item {
            Item::Potion => Some(crate::brewing::PotionData::water()),
            Item::SplashPotion => {
                let mut potion = crate::brewing::PotionData::water();
                potion.splash = true;
                Some(potion)
            }
            _ => None,
        };
        Self {
            item,
            count,
            durability,
            enchantments: crate::enchantment::EnchantmentSet::default(),
            potion,
            custom_name: crate::enchantment::ItemName::default(),
        }
    }

    pub fn can_merge_with(&self, other: &Self) -> bool {
        self.item == other.item
            && self.durability == other.durability
            && self.enchantments == other.enchantments
            && self.potion == other.potion
            && self.custom_name == other.custom_name
    }
}

/// Inventory for a container block entity (e.g. chest).
/// Single chest = 27 slots; double chest uses two halves of 27 slots each.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ContainerInventory {
    pub slots: [Option<ItemStack>; 27],
}

impl ContainerInventory {
    pub fn new() -> Self {
        Self { slots: [None; 27] }
    }

    pub fn is_empty(&self) -> bool {
        self.slots.iter().all(|s| s.is_none())
    }

    pub fn total_items(&self) -> u32 {
        self.slots.iter().flatten().map(|s| s.count).sum()
    }

    pub fn clear(&mut self) {
        self.slots.fill(None);
    }

    /// Returns the remainder if the stack couldn't be fully added.
    pub fn add_stack(&mut self, mut incoming: ItemStack) -> Option<ItemStack> {
        if incoming.count == 0 || incoming.item == Item::Air {
            return None;
        }
        let max_stack = incoming.item.properties().max_stack;
        for slot in self.slots.iter_mut() {
            if let Some(existing) = slot {
                if existing.can_merge_with(&incoming) && existing.count < max_stack {
                    let moved = (max_stack - existing.count).min(incoming.count);
                    existing.count += moved;
                    incoming.count -= moved;
                    if incoming.count == 0 {
                        return None;
                    }
                }
            }
        }
        for slot in self.slots.iter_mut() {
            if slot.is_none() {
                let moved = incoming.count.min(max_stack);
                *slot = Some(ItemStack {
                    count: moved,
                    ..incoming
                });
                incoming.count -= moved;
                if incoming.count == 0 {
                    return None;
                }
            }
        }
        Some(incoming)
    }

    /// Return the number of slots that can accept incoming.
    pub fn storage_capacity_for(&self, incoming: ItemStack) -> u32 {
        let max_stack = incoming.item.properties().max_stack;
        self.slots
            .iter()
            .map(|slot| match slot {
                Some(existing) if existing.can_merge_with(&incoming) => {
                    max_stack.saturating_sub(existing.count)
                }
                None => max_stack,
                _ => 0,
            })
            .sum()
    }
}
pub(crate) struct StackClickResult {
    pub slot: Option<ItemStack>,
    pub dragged: Option<ItemStack>,
    pub cursor_from_slot: bool,
}

pub(crate) fn apply_stack_click(
    slot_item: Option<ItemStack>,
    dragged_item: Option<ItemStack>,
    is_left: bool,
) -> StackClickResult {
    let mut slot = slot_item;
    let mut dragged = dragged_item;
    let mut cursor_from_slot = false;

    if is_left {
        match (dragged, slot) {
            (Some(cursor), Some(existing)) if existing.can_merge_with(&cursor) => {
                let max_stack = existing.item.properties().max_stack;
                let transfer = max_stack.saturating_sub(existing.count).min(cursor.count);
                slot = Some(ItemStack {
                    count: existing.count + transfer,
                    ..existing
                });
                dragged = (cursor.count > transfer).then_some(ItemStack {
                    count: cursor.count - transfer,
                    ..cursor
                });
            }
            (Some(cursor), Some(existing)) => {
                slot = Some(cursor);
                dragged = Some(existing);
                cursor_from_slot = true;
            }
            (Some(cursor), None) => {
                slot = Some(cursor);
                dragged = None;
            }
            (None, Some(existing)) => {
                slot = None;
                dragged = Some(existing);
                cursor_from_slot = true;
            }
            (None, None) => {}
        }
    } else {
        match (dragged, slot) {
            (Some(cursor), Some(existing))
                if existing.can_merge_with(&cursor)
                    && existing.count < existing.item.properties().max_stack =>
            {
                slot = Some(ItemStack {
                    count: existing.count + 1,
                    ..existing
                });
                dragged = (cursor.count > 1).then_some(ItemStack {
                    count: cursor.count - 1,
                    ..cursor
                });
            }
            (Some(cursor), Some(existing)) if !existing.can_merge_with(&cursor) => {
                slot = Some(cursor);
                dragged = Some(existing);
                cursor_from_slot = true;
            }
            (Some(_), Some(_)) => {}
            (Some(cursor), None) => {
                slot = Some(ItemStack { count: 1, ..cursor });
                dragged = (cursor.count > 1).then_some(ItemStack {
                    count: cursor.count - 1,
                    ..cursor
                });
            }
            (None, Some(existing)) => {
                let take = existing.count.div_ceil(2);
                let keep = existing.count - take;
                slot = (keep > 0).then_some(ItemStack {
                    count: keep,
                    ..existing
                });
                dragged = Some(ItemStack {
                    count: take,
                    ..existing
                });
                cursor_from_slot = true;
            }
            (None, None) => {}
        }
    }

    StackClickResult {
        slot,
        dragged,
        cursor_from_slot,
    }
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
            | Item::Shears
            | Item::Bucket
            | Item::MilkBucket
            | Item::FlintAndSteel
            | Item::Elytra => Some(CreativeTab::Tools),
            Item::StoneSword
            | Item::IronSword
            | Item::DiamondSword
            | Item::Bow
            | Item::Arrow
            | Item::IronHelmet
            | Item::IronChestplate
            | Item::IronLeggings
            | Item::IronBoots
            | Item::EndCrystal => Some(CreativeTab::Combat),
            Item::Apple
            | Item::Bread
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
            | Item::NoteBlock => Some(CreativeTab::Redstone),
            Item::Stick
            | Item::Coal
            | Item::IronIngot
            | Item::GoldIngot
            | Item::Diamond
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
            | Item::ShulkerShell => Some(CreativeTab::Misc),
        }
    }

    pub fn is_armor(self) -> bool {
        matches!(
            self,
            Item::IronHelmet | Item::IronChestplate | Item::IronLeggings | Item::IronBoots
        )
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
                is_block: false,
                block_type: None,
                tex_coords: (13, 3),
            },
            Item::Carrot => ItemProperties {
                name: "Carrot",
                max_stack: 64,
                is_block: false,
                block_type: None,
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
            | Item::NoteBlock) => {
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
            item @ (Item::FlintAndSteel
            | Item::EyeOfEnder
            | Item::Elytra
            | Item::NetherStar
            | Item::EndCrystal
            | Item::BlazeRod
            | Item::ShulkerShell) => {
                let (name, max_stack, tex_coords) = match item {
                    Item::FlintAndSteel => ("Flint and Steel", 1, (11, 10)),
                    Item::EyeOfEnder => ("Eye of Ender", 64, (12, 10)),
                    Item::Elytra => ("Elytra", 1, (13, 10)),
                    Item::NetherStar => ("Nether Star", 64, (3, 4)),
                    Item::EndCrystal => ("End Crystal", 64, (4, 4)),
                    Item::BlazeRod => ("Blaze Rod", 64, (5, 4)),
                    Item::ShulkerShell => ("Shulker Shell", 64, (14, 14)),
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
        }
    }
}

pub struct Inventory {
    pub hotbar: [Option<ItemStack>; 9],
    pub main: [Option<ItemStack>; 27],
    pub armor: [Option<ItemStack>; 4],
    pub craft_input: Vec<Option<ItemStack>>, // 4 slots for 2x2, 9 slots for 3x3
    pub craft_output: Option<ItemStack>,
    pub dragged: Option<ItemStack>,
    pub creative_drag_origin: Option<CreativeDragOrigin>,
    pub creative_tab: CreativeTab,
    pub creative_scroll_row: usize,
    pub selected: usize, // Selected hotbar slot: 0..8
    pub is_open: bool,
    pub is_table_open: bool,
}

impl Inventory {
    pub fn new() -> Self {
        Self {
            hotbar: [None; 9],
            main: [None; 27],
            armor: [None; 4],
            craft_input: vec![None; 4],
            craft_output: None,
            dragged: None,
            creative_drag_origin: None,
            creative_tab: CreativeTab::All,
            creative_scroll_row: 0,
            selected: 0,
            is_open: false,
            is_table_open: false,
        }
    }

    pub fn new_creative() -> Self {
        let mut inv = Self::new();
        let creative_items = [
            Item::Grass,
            Item::Dirt,
            Item::Stone,
            Item::OakLog,
            Item::OakPlanks,
            Item::Glass,
            Item::Cobblestone,
            Item::Water,
            Item::Torch,
        ];
        for (i, &item) in creative_items.iter().enumerate() {
            inv.hotbar[i] = Some(ItemStack::new(item, 64));
        }

        let extra_items = [
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
            Item::DiamondPickaxe,
            Item::FlintAndSteel,
            Item::Netherrack,
            Item::SoulSand,
            Item::Glowstone,
            Item::EndStone,
            Item::EndPortalFrame,
            Item::EyeOfEnder,
            Item::WitherSkeletonSkull,
            Item::EndCrystal,
            Item::Elytra,
            Item::NetherStar,
        ];
        for (i, &item) in extra_items.iter().enumerate() {
            inv.main[i] = Some(ItemStack::new(item, item.properties().max_stack));
        }
        inv
    }

    pub fn creative_items_for_tab(tab: CreativeTab) -> Vec<Item> {
        CREATIVE_ITEMS
            .iter()
            .copied()
            .filter(|item| tab == CreativeTab::All || item.creative_tab() == Some(tab))
            .collect()
    }

    pub fn creative_max_scroll_for_tab(tab: CreativeTab) -> usize {
        let item_count = Self::creative_items_for_tab(tab).len();
        let total_rows = item_count.div_ceil(CREATIVE_COLUMNS);
        total_rows.saturating_sub(CREATIVE_ROWS)
    }

    pub fn creative_max_scroll(&self) -> usize {
        Self::creative_max_scroll_for_tab(self.creative_tab)
    }

    pub fn creative_visible_items(&self) -> Vec<Item> {
        let start = self.creative_scroll_row * CREATIVE_COLUMNS;
        Self::creative_items_for_tab(self.creative_tab)
            .into_iter()
            .skip(start)
            .take(CREATIVE_VISIBLE_SLOTS)
            .collect()
    }

    pub fn select_creative_tab(&mut self, tab: CreativeTab) {
        self.creative_tab = tab;
        self.creative_scroll_row = 0;
    }

    pub fn clamp_creative_scroll(&mut self) {
        self.creative_scroll_row = self.creative_scroll_row.min(self.creative_max_scroll());
    }

    pub fn scroll_creative(&mut self, direction: i32) {
        let max_scroll = self.creative_max_scroll() as i32;
        self.creative_scroll_row =
            (self.creative_scroll_row as i32 + direction).clamp(0, max_scroll) as usize;
    }

    pub fn creative_supply(&mut self, item: Item, is_left: bool) -> bool {
        if item == Item::Air {
            return false;
        }
        if self.dragged.is_some()
            && self.creative_drag_origin != Some(CreativeDragOrigin::Catalog)
            && !self.try_return_dragged_to_storage()
        {
            return false;
        }

        let count = if is_left {
            item.properties().max_stack
        } else {
            1
        };
        self.dragged = Some(ItemStack::new(item, count));
        self.creative_drag_origin = Some(CreativeDragOrigin::Catalog);
        true
    }

    pub fn write_creative_slot(&mut self, _item: Item, _stack: Option<ItemStack>) {
        // Creative catalog slots are immutable, infinite-supply views.
    }

    pub fn click_creative_hotbar(&mut self, index: usize, is_left: bool) {
        if index >= self.hotbar.len() {
            return;
        }
        if self.dragged.is_some() && self.creative_drag_origin.is_none() {
            self.creative_drag_origin = Some(CreativeDragOrigin::Inventory);
        }

        let previous_origin = self.creative_drag_origin;
        let result = apply_stack_click(self.hotbar[index], self.dragged, is_left);
        self.hotbar[index] = result.slot;
        self.dragged = result.dragged;
        self.creative_drag_origin = if self.dragged.is_none() {
            None
        } else if result.cursor_from_slot {
            Some(CreativeDragOrigin::Inventory)
        } else {
            previous_origin
        };
    }

    pub fn finish_creative_cursor(&mut self) -> bool {
        match self.creative_drag_origin {
            Some(CreativeDragOrigin::Catalog) => {
                self.dragged = None;
                self.creative_drag_origin = None;
                true
            }
            Some(CreativeDragOrigin::Inventory) | None => self.try_return_dragged_to_storage(),
        }
    }

    fn try_return_dragged_to_storage(&mut self) -> bool {
        let Some(stack) = self.dragged else {
            self.creative_drag_origin = None;
            return true;
        };
        if self.storage_capacity_for(stack) < stack.count {
            return false;
        }

        let origin = self.creative_drag_origin;
        self.dragged = None;
        match self.add_stack(stack) {
            None => {
                self.creative_drag_origin = None;
                true
            }
            Some(remainder) => {
                self.dragged = Some(remainder);
                self.creative_drag_origin = origin;
                false
            }
        }
    }

    pub fn find_item(&self, item: Item) -> Option<(usize, ItemStack)> {
        for (i, slot) in self.hotbar.iter().enumerate() {
            if let Some(stack) = slot {
                if stack.item == item {
                    return Some((i, *stack));
                }
            }
        }
        for (i, slot) in self.main.iter().enumerate() {
            if let Some(stack) = slot {
                if stack.item == item {
                    return Some((i + 9, *stack));
                }
            }
        }
        None
    }

    pub fn remove_at_slot(&mut self, slot_idx: usize) {
        if slot_idx < 9 {
            self.hotbar[slot_idx] = None;
        } else if slot_idx < 36 {
            self.main[slot_idx - 9] = None;
        }
    }

    fn storage_capacity_for(&self, incoming: ItemStack) -> u32 {
        let max_stack = incoming.item.properties().max_stack;
        self.hotbar
            .iter()
            .chain(self.main.iter())
            .map(|slot| match slot {
                Some(existing) if existing.can_merge_with(&incoming) => {
                    max_stack.saturating_sub(existing.count)
                }
                None => max_stack,
                _ => 0,
            })
            .sum()
    }

    #[cfg(test)]
    pub(crate) fn try_store_for_close(&mut self, returning_items: &[ItemStack]) -> bool {
        let original_hotbar = self.hotbar;
        let original_main = self.main;
        let original_dragged = self.dragged;
        let original_origin = self.creative_drag_origin;

        for &stack in returning_items {
            if self.add_stack(stack).is_some() {
                self.hotbar = original_hotbar;
                self.main = original_main;
                self.dragged = original_dragged;
                self.creative_drag_origin = original_origin;
                return false;
            }
        }

        if !self.finish_creative_cursor() {
            self.hotbar = original_hotbar;
            self.main = original_main;
            self.dragged = original_dragged;
            self.creative_drag_origin = original_origin;
            return false;
        }

        true
    }

    pub fn clear(&mut self) {
        self.hotbar = [None; 9];
        self.main = [None; 27];
        self.armor = [None; 4];
        self.craft_input.fill(None);
        self.craft_output = None;
        self.dragged = None;
        self.creative_drag_origin = None;
    }

    pub fn get_selected_block(&self) -> Option<BlockType> {
        self.hotbar[self.selected].and_then(|stack| stack.item.properties().block_type)
    }

    pub fn add_item(&mut self, item: Item) -> bool {
        self.add_stack(ItemStack::new(item, 1)).is_none()
    }

    pub fn add_stack(&mut self, mut incoming: ItemStack) -> Option<ItemStack> {
        if incoming.count == 0 {
            return None;
        }
        if incoming.item == Item::Air {
            return Some(incoming);
        }
        let max_stack = incoming.item.properties().max_stack;
        for slot in self.hotbar.iter_mut().chain(self.main.iter_mut()) {
            if let Some(existing) = slot {
                if existing.can_merge_with(&incoming) && existing.count < max_stack {
                    let moved = (max_stack - existing.count).min(incoming.count);
                    existing.count += moved;
                    incoming.count -= moved;
                    if incoming.count == 0 {
                        return None;
                    }
                }
            }
        }
        for slot in self.hotbar.iter_mut().chain(self.main.iter_mut()) {
            if slot.is_none() {
                let moved = incoming.count.min(max_stack);
                *slot = Some(ItemStack {
                    count: moved,
                    ..incoming
                });
                incoming.count -= moved;
                if incoming.count == 0 {
                    return None;
                }
            }
        }
        Some(incoming)
    }

    pub fn count_item(&self, item: Item) -> u32 {
        self.hotbar
            .iter()
            .chain(self.main.iter())
            .flatten()
            .filter(|stack| stack.item == item)
            .map(|stack| stack.count)
            .sum()
    }

    pub fn remove_one(&mut self, item: Item) -> bool {
        for slot in self.hotbar.iter_mut().chain(self.main.iter_mut()) {
            if slot.is_some_and(|stack| stack.item == item) {
                let stack = slot.as_mut().unwrap();
                if stack.count > 1 {
                    stack.count -= 1;
                } else {
                    *slot = None;
                }
                return true;
            }
        }
        false
    }

    pub fn use_selected_item(&mut self, is_creative: bool) {
        if is_creative {
            return;
        }
        if let Some(stack) = &mut self.hotbar[self.selected] {
            if stack.count > 1 {
                stack.count -= 1;
            } else {
                self.hotbar[self.selected] = None;
            }
        }
    }

    pub fn remove_selected_item(&mut self, count: u32) {
        if let Some(stack) = &mut self.hotbar[self.selected] {
            if stack.count > count {
                stack.count -= count;
            } else {
                self.hotbar[self.selected] = None;
            }
        }
    }

    pub fn replace_selected_item(&mut self, item: Item) {
        self.hotbar[self.selected] = Some(ItemStack::new(item, 1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_inventory_creative_init() {
        let inv = Inventory::new_creative();
        assert_eq!(inv.selected, 0);
        assert_eq!(inv.get_selected_block(), Some(BlockType::Grass));
        assert_eq!(inv.hotbar[0].unwrap().count, 64);
    }

    #[test]
    fn test_inventory_add_item() {
        let mut inv = Inventory::new();
        assert!(inv.add_item(Item::Stone));
        assert_eq!(inv.hotbar[0].unwrap().item, Item::Stone);
        assert_eq!(inv.hotbar[0].unwrap().count, 1);

        assert!(inv.add_item(Item::Stone));
        assert_eq!(inv.hotbar[0].unwrap().count, 2);
    }

    #[test]
    fn add_stack_returns_exact_metadata_preserving_remainder() {
        let mut inv = Inventory::new();
        inv.hotbar.fill(Some(ItemStack::new(Item::DiamondSword, 1)));
        inv.main.fill(Some(ItemStack::new(Item::DiamondPickaxe, 1)));

        let mut incoming = ItemStack::new(Item::Stone, 2);
        incoming.custom_name.set("Keepsake");
        incoming
            .enchantments
            .add_or_upgrade(crate::enchantment::Enchantment::Unbreaking(2));
        incoming.potion = Some(crate::brewing::PotionData {
            kind: crate::brewing::PotionKind::Speed,
            level: 2,
            duration_seconds: 45,
            splash: true,
        });
        inv.hotbar[0] = Some(ItemStack {
            count: 63,
            ..incoming
        });

        let remainder = inv.add_stack(incoming).expect("one item should remain");
        assert_eq!(inv.hotbar[0].unwrap().count, 64);
        assert_eq!(
            remainder,
            ItemStack {
                count: 1,
                ..incoming
            }
        );
    }

    #[test]
    fn stack_clicks_merge_only_identical_metadata() {
        let plain = ItemStack::new(Item::Dirt, 4);
        let mut named = ItemStack::new(Item::Dirt, 3);
        named.custom_name.set("Garden Soil");

        for is_left in [true, false] {
            let rejected = apply_stack_click(Some(plain), Some(named), is_left);
            assert_eq!(rejected.slot, Some(named));
            assert_eq!(rejected.dragged, Some(plain));
            assert!(rejected.cursor_from_slot);
        }

        let left = apply_stack_click(Some(plain), Some(ItemStack { count: 3, ..plain }), true);
        assert_eq!(left.slot.unwrap().count, 7);
        assert!(left.dragged.is_none());

        let right = apply_stack_click(Some(plain), Some(ItemStack { count: 3, ..plain }), false);
        assert_eq!(right.slot.unwrap().count, 5);
        assert_eq!(right.dragged.unwrap().count, 2);
    }

    #[test]
    fn stack_identity_checks_every_metadata_field() {
        let original = ItemStack::new(Item::Dirt, 1);
        assert!(original.can_merge_with(&ItemStack {
            count: 63,
            ..original
        }));

        let mut variants = Vec::new();
        variants.push(ItemStack::new(Item::Stone, 1));
        variants.push(ItemStack {
            durability: 1,
            ..original
        });

        let mut enchanted = original;
        enchanted
            .enchantments
            .add_or_upgrade(crate::enchantment::Enchantment::Unbreaking(1));
        variants.push(enchanted);

        variants.push(ItemStack {
            potion: Some(crate::brewing::PotionData::water()),
            ..original
        });

        let mut named = original;
        named.custom_name.set("Named");
        variants.push(named);

        assert!(variants
            .into_iter()
            .all(|variant| !original.can_merge_with(&variant)));
    }

    #[test]
    fn add_item_does_not_merge_plain_items_into_named_stacks() {
        let mut inv = Inventory::new();
        let mut named = ItemStack::new(Item::Dirt, 1);
        named.custom_name.set("Named");
        inv.hotbar[0] = Some(named);

        assert!(inv.add_item(Item::Dirt));
        assert_eq!(inv.hotbar[0], Some(named));
        assert_eq!(inv.hotbar[1], Some(ItemStack::new(Item::Dirt, 1)));
    }

    #[test]
    fn test_item_properties() {
        let pick = ItemStack::new(Item::StonePickaxe, 1);
        assert_eq!(pick.durability, 131);
        let grass = ItemStack::new(Item::Grass, 64);
        assert_eq!(grass.durability, 0);
    }

    #[test]
    fn test_renders_flat() {
        // Non-block items render as flat sprites.
        assert!(Item::Seeds.renders_flat());
        assert!(Item::Wheat.renders_flat());
        assert!(Item::StonePickaxe.renders_flat());
        assert!(Item::Apple.renders_flat());
        // Cross-model plant blocks render as flat sprites.
        assert!(Item::Dandelion.renders_flat());
        assert!(Item::Poppy.renders_flat());
        assert!(Item::TallGrass.renders_flat());
        assert!(Item::SugarCane.renders_flat());
        // Full-cube block items keep cube rendering.
        assert!(!Item::Stone.renders_flat());
        assert!(!Item::Grass.renders_flat());
        assert!(!Item::Torch.renders_flat());
        assert!(!Item::Air.renders_flat());
    }

    #[test]
    fn test_new_mob_items() {
        let flesh = Item::RottenFlesh;
        let prop = flesh.properties();
        assert_eq!(prop.name, "Rotten Flesh");
        assert_eq!(prop.tex_coords, (8, 3));
    }

    #[test]
    fn creative_catalog_contains_every_non_air_item_once() {
        assert_eq!(CREATIVE_ITEMS.len(), 144);
        assert!(!CREATIVE_ITEMS.contains(&Item::Air));
        let unique: HashSet<_> = CREATIVE_ITEMS.iter().copied().collect();
        assert_eq!(unique.len(), CREATIVE_ITEMS.len());
    }

    #[test]
    fn creative_catalog_items_have_valid_properties() {
        for item in CREATIVE_ITEMS {
            let properties = item.properties();
            assert!(!properties.name.is_empty(), "{item:?}");
            assert!(properties.max_stack > 0, "{item:?}");
            assert!(properties.tex_coords.0 < 16, "{item:?}");
            assert!(properties.tex_coords.1 < 16, "{item:?}");
            assert!(item.creative_tab().is_some(), "{item:?}");
        }
    }

    #[test]
    fn creative_tabs_partition_catalog_without_duplicates() {
        let mut partition = Vec::new();
        for tab in CreativeTab::TABS
            .into_iter()
            .filter(|tab| *tab != CreativeTab::All)
        {
            let tab_items = Inventory::creative_items_for_tab(tab);
            assert!(tab_items
                .iter()
                .all(|item| item.creative_tab() == Some(tab)));
            partition.extend(tab_items);
        }

        let unique: HashSet<_> = partition.iter().copied().collect();
        assert_eq!(partition.len(), CREATIVE_ITEMS.len());
        assert_eq!(unique.len(), CREATIVE_ITEMS.len());
        assert_eq!(
            unique,
            CREATIVE_ITEMS.iter().copied().collect::<HashSet<_>>()
        );
    }

    #[test]
    fn creative_window_scrolls_by_row_and_clamps() {
        let mut inventory = Inventory::new();
        inventory.selected = 4;
        assert_eq!(inventory.creative_visible_items().len(), 45);
        assert_eq!(inventory.creative_max_scroll(), 11);

        inventory.scroll_creative(1);
        assert_eq!(inventory.creative_scroll_row, 1);
        assert_eq!(inventory.selected, 4);
        assert_eq!(
            inventory.creative_visible_items()[0],
            CREATIVE_ITEMS[CREATIVE_COLUMNS]
        );

        inventory.scroll_creative(-999);
        assert_eq!(inventory.creative_scroll_row, 0);
        inventory.creative_scroll_row = usize::MAX;
        inventory.clamp_creative_scroll();
        assert_eq!(inventory.creative_scroll_row, 11);
        assert_eq!(inventory.creative_visible_items().len(), 45);

        inventory.select_creative_tab(CreativeTab::Tools);
        assert_eq!(inventory.creative_scroll_row, 0);
        assert_eq!(inventory.creative_visible_items().len(), 14);
        assert_eq!(inventory.creative_max_scroll(), 0);
    }

    #[test]
    fn creative_catalog_supplies_left_max_and_right_one() {
        let mut inventory = Inventory::new();
        assert!(inventory.creative_supply(Item::Stone, true));
        assert_eq!(inventory.dragged.unwrap().count, 64);
        assert_eq!(
            inventory.creative_drag_origin,
            Some(CreativeDragOrigin::Catalog)
        );

        assert!(inventory.creative_supply(Item::DiamondSword, false));
        assert_eq!(inventory.dragged.unwrap().count, 1);
        assert_eq!(inventory.dragged.unwrap().item, Item::DiamondSword);
    }

    #[test]
    fn creative_virtual_slot_write_is_a_no_op() {
        let mut inventory = Inventory::new_creative();
        let hotbar = inventory.hotbar;
        let main = inventory.main;
        inventory.write_creative_slot(Item::Stone, None);
        inventory.write_creative_slot(Item::Dirt, Some(ItemStack::new(Item::Diamond, 64)));
        assert_eq!(inventory.hotbar, hotbar);
        assert_eq!(inventory.main, main);
        assert!(CREATIVE_ITEMS.contains(&Item::Stone));
        assert!(CREATIVE_ITEMS.contains(&Item::Dirt));
    }

    #[test]
    fn creative_hotbar_reuses_drag_drop_and_close_semantics() {
        let mut inventory = Inventory::new();
        assert!(inventory.creative_supply(Item::Stone, true));
        inventory.click_creative_hotbar(0, true);
        assert_eq!(inventory.hotbar[0].unwrap().item, Item::Stone);
        assert_eq!(inventory.hotbar[0].unwrap().count, 64);
        assert!(inventory.dragged.is_none());

        inventory.click_creative_hotbar(0, false);
        assert_eq!(inventory.hotbar[0].unwrap().count, 32);
        assert_eq!(inventory.dragged.unwrap().count, 32);
        assert_eq!(
            inventory.creative_drag_origin,
            Some(CreativeDragOrigin::Inventory)
        );
        assert!(inventory.finish_creative_cursor());
        assert!(inventory.dragged.is_none());
        assert_eq!(inventory.count_item(Item::Stone), 64);

        assert!(inventory.creative_supply(Item::Dirt, true));
        assert!(inventory.finish_creative_cursor());
        assert!(inventory.dragged.is_none());
        assert_eq!(inventory.count_item(Item::Dirt), 0);
    }

    #[test]
    fn creative_close_keeps_real_cursor_when_storage_is_full() {
        let mut inventory = Inventory::new();
        inventory
            .hotbar
            .fill(Some(ItemStack::new(Item::DiamondSword, 1)));
        inventory
            .main
            .fill(Some(ItemStack::new(Item::DiamondPickaxe, 1)));
        inventory.dragged = Some(ItemStack::new(Item::Stone, 64));
        inventory.creative_drag_origin = Some(CreativeDragOrigin::Inventory);

        assert!(!inventory.finish_creative_cursor());
        assert_eq!(inventory.dragged.unwrap().item, Item::Stone);
        assert_eq!(inventory.dragged.unwrap().count, 64);
        assert_eq!(
            inventory.creative_drag_origin,
            Some(CreativeDragOrigin::Inventory)
        );
    }

    #[test]
    fn close_storage_transaction_rolls_back_partial_returns_and_real_cursor() {
        let mut inventory = Inventory::new();
        inventory
            .hotbar
            .fill(Some(ItemStack::new(Item::DiamondSword, 1)));
        inventory
            .main
            .fill(Some(ItemStack::new(Item::DiamondPickaxe, 1)));
        inventory.hotbar[0] = Some(ItemStack::new(Item::Stone, 63));
        inventory.dragged = Some(ItemStack::new(Item::Dirt, 64));
        inventory.creative_drag_origin = Some(CreativeDragOrigin::Inventory);
        let original_hotbar = inventory.hotbar;
        let original_main = inventory.main;

        assert!(!inventory.try_store_for_close(&[ItemStack::new(Item::Stone, 2)]));
        assert_eq!(inventory.hotbar, original_hotbar);
        assert_eq!(inventory.main, original_main);
        assert_eq!(inventory.dragged, Some(ItemStack::new(Item::Dirt, 64)));
        assert_eq!(
            inventory.creative_drag_origin,
            Some(CreativeDragOrigin::Inventory)
        );
    }

    #[test]
    fn close_storage_discards_only_catalog_cursor() {
        let mut inventory = Inventory::new();
        inventory
            .hotbar
            .fill(Some(ItemStack::new(Item::DiamondSword, 1)));
        inventory
            .main
            .fill(Some(ItemStack::new(Item::DiamondPickaxe, 1)));
        inventory.dragged = Some(ItemStack::new(Item::Stone, 64));
        inventory.creative_drag_origin = Some(CreativeDragOrigin::Catalog);

        assert!(inventory.try_store_for_close(&[]));
        assert!(inventory.dragged.is_none());
        assert!(inventory.creative_drag_origin.is_none());
    }

    #[test]
    fn creative_hotbar_rejects_same_item_with_different_metadata() {
        let mut inventory = Inventory::new();
        inventory.hotbar[0] = Some(ItemStack::new(Item::Dirt, 4));
        let mut named = ItemStack::new(Item::Dirt, 3);
        named.custom_name.set("Garden Soil");
        inventory.dragged = Some(named);
        inventory.creative_drag_origin = Some(CreativeDragOrigin::Inventory);

        inventory.click_creative_hotbar(0, true);
        assert_eq!(inventory.hotbar[0], Some(named));
        assert_eq!(inventory.dragged, Some(ItemStack::new(Item::Dirt, 4)));
        assert_eq!(
            inventory.creative_drag_origin,
            Some(CreativeDragOrigin::Inventory)
        );
    }

    #[test]
    fn splash_potion_stack_has_water_splash_metadata() {
        let stack = ItemStack::new(Item::SplashPotion, 1);
        let potion = stack.potion.expect("splash potion metadata");
        assert_eq!(potion.kind, crate::brewing::PotionKind::Water);
        assert!(potion.splash);
    }
}
