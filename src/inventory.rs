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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
        Self {
            item,
            count,
            durability,
            enchantments: crate::enchantment::EnchantmentSet::default(),
            potion: if item == Item::Potion {
                Some(crate::brewing::PotionData::water())
            } else {
                None
            },
            custom_name: crate::enchantment::ItemName::default(),
        }
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
            BlockType::Furnace => Item::Furnace,
            BlockType::Chest => Item::Chest,
            BlockType::TNT => Item::TNT,
            BlockType::Bookshelf => Item::Bookshelf,
            BlockType::Torch => Item::Torch,
            BlockType::Lava => Item::Lava,
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
            Item::StoneSword,
            Item::IronPickaxe,
            Item::DiamondPickaxe,
            Item::Apple,
            Item::Bread,
            Item::Coal,
            Item::IronIngot,
            Item::Diamond,
            Item::Redstone,
            Item::CraftingTable,
            Item::TNT,
        ];
        for (i, &item) in extra_items.iter().enumerate() {
            inv.main[i] = Some(ItemStack::new(item, 64));
        }
        inv
    }

    pub fn clear(&mut self) {
        self.hotbar = [None; 9];
        self.main = [None; 27];
        self.armor = [None; 4];
        self.craft_input.fill(None);
        self.craft_output = None;
        self.dragged = None;
    }

    pub fn get_selected_block(&self) -> Option<BlockType> {
        self.hotbar[self.selected].and_then(|stack| stack.item.properties().block_type)
    }

    pub fn add_item(&mut self, item: Item) -> bool {
        if item == Item::Air {
            return false;
        }
        let max_stack = item.properties().max_stack;

        // 1. Try to add to existing stack in hotbar
        for slot in self.hotbar.iter_mut() {
            if let Some(stack) = slot {
                if stack.item == item && stack.count < max_stack {
                    stack.count += 1;
                    return true;
                }
            }
        }
        // 2. Try to add to existing stack in main backpack
        for slot in self.main.iter_mut() {
            if let Some(stack) = slot {
                if stack.item == item && stack.count < max_stack {
                    stack.count += 1;
                    return true;
                }
            }
        }
        // 3. Try to add to empty slot in hotbar
        for slot in self.hotbar.iter_mut() {
            if slot.is_none() {
                *slot = Some(ItemStack::new(item, 1));
                return true;
            }
        }
        // 4. Try to add to empty slot in main backpack
        for slot in self.main.iter_mut() {
            if slot.is_none() {
                *slot = Some(ItemStack::new(item, 1));
                return true;
            }
        }
        false
    }

    pub fn add_stack(&mut self, mut incoming: ItemStack) -> bool {
        if incoming.item == Item::Air || incoming.count == 0 {
            return false;
        }
        let max_stack = incoming.item.properties().max_stack;
        for slot in self.hotbar.iter_mut().chain(self.main.iter_mut()) {
            if let Some(existing) = slot {
                if existing.item == incoming.item
                    && existing.enchantments == incoming.enchantments
                    && existing.potion == incoming.potion
                    && existing.custom_name == incoming.custom_name
                    && existing.count < max_stack
                {
                    let moved = (max_stack - existing.count).min(incoming.count);
                    existing.count += moved;
                    incoming.count -= moved;
                    if incoming.count == 0 {
                        return true;
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
                    return true;
                }
            }
        }
        false
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
    fn test_item_properties() {
        let pick = ItemStack::new(Item::StonePickaxe, 1);
        assert_eq!(pick.durability, 131);
        let grass = ItemStack::new(Item::Grass, 64);
        assert_eq!(grass.durability, 0);
    }

    #[test]
    fn test_new_mob_items() {
        let flesh = Item::RottenFlesh;
        let prop = flesh.properties();
        assert_eq!(prop.name, "Rotten Flesh");
        assert_eq!(prop.tex_coords, (8, 3));
    }
}
