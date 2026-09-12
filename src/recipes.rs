//! Crafting and smelting recipe tables (Plan 21).
//! Pattern strings + wood-family expansion; shaped/smelting lookups are indexed.

use crate::inventory::{Item, ItemStack};
use std::collections::HashMap;

pub type RecipeId = &'static str;

#[derive(Debug, Clone)]
pub struct CraftingRecipe {
    pub id: RecipeId,
    pub pattern: Vec<Vec<Item>>, // 2D grid
    pub width: usize,
    pub height: usize,
    pub result: ItemStack,
    pub shapeless: bool,
}

#[derive(Debug, Clone)]
pub struct SmeltingRecipe {
    pub id: RecipeId,
    pub input: Item,
    pub output: ItemStack,
    pub cook_time: u16,  // Default 200 ticks (10s)
    pub experience: f32, // XP awarded on taking output
}

pub struct FuelDefinition;

impl FuelDefinition {
    pub fn burn_time(item: Item) -> u16 {
        match item {
            Item::Coal => 1600,
            Item::OakLog | Item::BirchLog | Item::SpruceLog => 300,
            Item::OakPlanks | Item::BirchPlanks | Item::SprucePlanks => 300,
            Item::Stick => 100,
            Item::CraftingTable | Item::Chest | Item::OakDoor | Item::OakTrapdoor => 300,
            Item::BlazeRod => 2400,
            Item::Lava => 20000,
            _ => 0,
        }
    }

    pub fn is_fuel(item: Item) -> bool {
        Self::burn_time(item) > 0
    }
}

pub struct RecipeManager {
    pub crafting_recipes: Vec<CraftingRecipe>,
    pub smelting_recipes: Vec<SmeltingRecipe>,
    /// (width, height, pattern[0][0]) → recipe indices into `crafting_recipes`.
    shaped_index: HashMap<(usize, usize, Item), Vec<usize>>,
    smelting_by_input: HashMap<Item, usize>,
}

fn add_shaped(
    recipes: &mut Vec<CraftingRecipe>,
    id: RecipeId,
    pat: &[&str],
    mapping: &[(char, Item)],
    result: ItemStack,
) {
    let height = pat.len();
    let width = pat[0].len();
    let mut pattern = vec![vec![Item::Air; width]; height];
    for (r, row) in pat.iter().enumerate() {
        for (c, ch) in row.chars().enumerate() {
            if ch != ' ' {
                pattern[r][c] = mapping
                    .iter()
                    .find(|(k, _)| *k == ch)
                    .map(|(_, it)| *it)
                    .unwrap_or(Item::Air);
            }
        }
    }
    recipes.push(CraftingRecipe {
        id,
        pattern,
        width,
        height,
        result,
        shapeless: false,
    });
}

fn add_shapeless(
    recipes: &mut Vec<CraftingRecipe>,
    id: RecipeId,
    ingredients: Vec<Item>,
    result: ItemStack,
) {
    let mut sorted = ingredients;
    sorted.sort_by_key(|&it| it as i32);
    recipes.push(CraftingRecipe {
        id,
        pattern: vec![sorted],
        width: 0,
        height: 0,
        result,
        shapeless: true,
    });
}

#[derive(Clone, Copy)]
struct WoodFamily {
    name: &'static str,
    log: Item,
    planks: Item,
}

const WOODS: [WoodFamily; 3] = [
    WoodFamily {
        name: "oak",
        log: Item::OakLog,
        planks: Item::OakPlanks,
    },
    WoodFamily {
        name: "birch",
        log: Item::BirchLog,
        planks: Item::BirchPlanks,
    },
    WoodFamily {
        name: "spruce",
        log: Item::SpruceLog,
        planks: Item::SprucePlanks,
    },
];

/// Static shaped recipe rows: (id, pattern rows, char→item map, result item, count).
/// Wood-family plank recipes are expanded separately in `RecipeManager::new`.
struct ShapedDef {
    id: RecipeId,
    pat: &'static [&'static str],
    map: &'static [(char, Item)],
    result: Item,
    count: u32,
}

struct ShapelessDef {
    id: RecipeId,
    ingredients: &'static [Item],
    result: Item,
    count: u32,
}

struct SmeltDef {
    id: RecipeId,
    input: Item,
    output: Item,
    count: u32,
    cook_time: u16,
    experience: f32,
}

const SHAPED: &[ShapedDef] = &[
    ShapedDef {
        id: "crafting/bed",
        pat: &["WWW", "PPP"],
        map: &[('W', Item::Wool), ('P', Item::OakPlanks)],
        result: Item::Bed,
        count: 1,
    },
    ShapedDef {
        id: "crafting/furnace",
        pat: &["CCC", "C C", "CCC"],
        map: &[('C', Item::Cobblestone)],
        result: Item::Furnace,
        count: 1,
    },
    ShapedDef {
        id: "crafting/torch",
        pat: &["C", "S"],
        map: &[('C', Item::Coal), ('S', Item::Stick)],
        result: Item::Torch,
        count: 4,
    },
    // Stone tools
    ShapedDef {
        id: "crafting/stone_pickaxe",
        pat: &["SSS", " t ", " t "],
        map: &[('S', Item::Cobblestone), ('t', Item::Stick)],
        result: Item::StonePickaxe,
        count: 1,
    },
    ShapedDef {
        id: "crafting/stone_axe",
        pat: &["SS ", "St ", " t "],
        map: &[('S', Item::Cobblestone), ('t', Item::Stick)],
        result: Item::StoneAxe,
        count: 1,
    },
    ShapedDef {
        id: "crafting/stone_shovel",
        pat: &["S", "t", "t"],
        map: &[('S', Item::Cobblestone), ('t', Item::Stick)],
        result: Item::StoneShovel,
        count: 1,
    },
    ShapedDef {
        id: "crafting/stone_sword",
        pat: &["S", "S", "t"],
        map: &[('S', Item::Cobblestone), ('t', Item::Stick)],
        result: Item::StoneSword,
        count: 1,
    },
    // Iron tools
    ShapedDef {
        id: "crafting/iron_pickaxe",
        pat: &["III", " t ", " t "],
        map: &[('I', Item::IronIngot), ('t', Item::Stick)],
        result: Item::IronPickaxe,
        count: 1,
    },
    ShapedDef {
        id: "crafting/iron_axe",
        pat: &["II ", "It ", " t "],
        map: &[('I', Item::IronIngot), ('t', Item::Stick)],
        result: Item::IronAxe,
        count: 1,
    },
    ShapedDef {
        id: "crafting/iron_shovel",
        pat: &["I", "t", "t"],
        map: &[('I', Item::IronIngot), ('t', Item::Stick)],
        result: Item::IronShovel,
        count: 1,
    },
    ShapedDef {
        id: "crafting/iron_sword",
        pat: &["I", "I", "t"],
        map: &[('I', Item::IronIngot), ('t', Item::Stick)],
        result: Item::IronSword,
        count: 1,
    },
    // Diamond tools
    ShapedDef {
        id: "crafting/diamond_pickaxe",
        pat: &["DDD", " t ", " t "],
        map: &[('D', Item::Diamond), ('t', Item::Stick)],
        result: Item::DiamondPickaxe,
        count: 1,
    },
    ShapedDef {
        id: "crafting/diamond_axe",
        pat: &["DD ", "Dt ", " t "],
        map: &[('D', Item::Diamond), ('t', Item::Stick)],
        result: Item::DiamondAxe,
        count: 1,
    },
    ShapedDef {
        id: "crafting/diamond_shovel",
        pat: &["D", "t", "t"],
        map: &[('D', Item::Diamond), ('t', Item::Stick)],
        result: Item::DiamondShovel,
        count: 1,
    },
    ShapedDef {
        id: "crafting/diamond_sword",
        pat: &["D", "D", "t"],
        map: &[('D', Item::Diamond), ('t', Item::Stick)],
        result: Item::DiamondSword,
        count: 1,
    },
    ShapedDef {
        id: "crafting/stone_brick",
        pat: &["SS", "SS"],
        map: &[('S', Item::Stone)],
        result: Item::StoneBrick,
        count: 4,
    },
    ShapedDef {
        id: "crafting/brick",
        pat: &["CC", "CC"],
        map: &[('C', Item::Clay)],
        result: Item::Brick,
        count: 4,
    },
    ShapedDef {
        id: "crafting/sandstone",
        pat: &["SS", "SS"],
        map: &[('S', Item::Sand)],
        result: Item::Sandstone,
        count: 4,
    },
    ShapedDef {
        id: "crafting/snow_block",
        pat: &["SS", "SS"],
        map: &[('S', Item::Snow)],
        result: Item::Snow,
        count: 1,
    },
    ShapedDef {
        id: "crafting/tnt",
        pat: &["RSR", "SRS", "RSR"],
        map: &[('R', Item::Redstone), ('S', Item::Sand)],
        result: Item::TNT,
        count: 1,
    },
    ShapedDef {
        id: "crafting/bread",
        pat: &["WWW"],
        map: &[('W', Item::Wheat)],
        result: Item::Bread,
        count: 1,
    },
    ShapedDef {
        id: "crafting/enchanting_table",
        pat: &[" B ", "D D", "OOO"],
        map: &[
            ('B', Item::Bookshelf),
            ('D', Item::Diamond),
            ('O', Item::Obsidian),
        ],
        result: Item::EnchantingTable,
        count: 1,
    },
    ShapedDef {
        id: "crafting/brewing_stand",
        pat: &[" B ", "CCC"],
        map: &[('B', Item::BlazePowder), ('C', Item::Cobblestone)],
        result: Item::BrewingStand,
        count: 1,
    },
    ShapedDef {
        id: "crafting/anvil",
        pat: &["III", " I ", "III"],
        map: &[('I', Item::IronIngot)],
        result: Item::Anvil,
        count: 1,
    },
    ShapedDef {
        id: "crafting/glass_bottle",
        pat: &["G G", " G "],
        map: &[('G', Item::Glass)],
        result: Item::GlassBottle,
        count: 3,
    },
    ShapedDef {
        id: "crafting/iron_helmet",
        pat: &["III", "I I"],
        map: &[('I', Item::IronIngot)],
        result: Item::IronHelmet,
        count: 1,
    },
    ShapedDef {
        id: "crafting/iron_chestplate",
        pat: &["I I", "III", "III"],
        map: &[('I', Item::IronIngot)],
        result: Item::IronChestplate,
        count: 1,
    },
    ShapedDef {
        id: "crafting/iron_leggings",
        pat: &["III", "I I", "I I"],
        map: &[('I', Item::IronIngot)],
        result: Item::IronLeggings,
        count: 1,
    },
    ShapedDef {
        id: "crafting/iron_boots",
        pat: &["I I", "I I"],
        map: &[('I', Item::IronIngot)],
        result: Item::IronBoots,
        count: 1,
    },
    ShapedDef {
        id: "crafting/leather_helmet",
        pat: &["LLL", "L L"],
        map: &[('L', Item::Leather)],
        result: Item::LeatherHelmet,
        count: 1,
    },
    ShapedDef {
        id: "crafting/leather_chestplate",
        pat: &["L L", "LLL", "LLL"],
        map: &[('L', Item::Leather)],
        result: Item::LeatherChestplate,
        count: 1,
    },
    ShapedDef {
        id: "crafting/leather_leggings",
        pat: &["LLL", "L L", "L L"],
        map: &[('L', Item::Leather)],
        result: Item::LeatherLeggings,
        count: 1,
    },
    ShapedDef {
        id: "crafting/leather_boots",
        pat: &["L L", "L L"],
        map: &[('L', Item::Leather)],
        result: Item::LeatherBoots,
        count: 1,
    },
    ShapedDef {
        id: "crafting/diamond_helmet",
        pat: &["DDD", "D D"],
        map: &[('D', Item::Diamond)],
        result: Item::DiamondHelmet,
        count: 1,
    },
    ShapedDef {
        id: "crafting/diamond_chestplate",
        pat: &["D D", "DDD", "DDD"],
        map: &[('D', Item::Diamond)],
        result: Item::DiamondChestplate,
        count: 1,
    },
    ShapedDef {
        id: "crafting/diamond_leggings",
        pat: &["DDD", "D D", "D D"],
        map: &[('D', Item::Diamond)],
        result: Item::DiamondLeggings,
        count: 1,
    },
    ShapedDef {
        id: "crafting/diamond_boots",
        pat: &["D D", "D D"],
        map: &[('D', Item::Diamond)],
        result: Item::DiamondBoots,
        count: 1,
    },
    ShapedDef {
        id: "crafting/shield",
        pat: &["PIP", "PPP", " P "],
        map: &[('P', Item::OakPlanks), ('I', Item::IronIngot)],
        result: Item::Shield,
        count: 1,
    },
    ShapedDef {
        id: "crafting/wooden_sword",
        pat: &["W", "W", "S"],
        map: &[('W', Item::OakPlanks), ('S', Item::Stick)],
        result: Item::WoodenSword,
        count: 1,
    },
    ShapedDef {
        id: "crafting/wooden_pickaxe",
        pat: &["WWW", " S ", " S "],
        map: &[('W', Item::OakPlanks), ('S', Item::Stick)],
        result: Item::WoodenPickaxe,
        count: 1,
    },
    ShapedDef {
        id: "crafting/wooden_axe",
        pat: &["WW", "WS", " S"],
        map: &[('W', Item::OakPlanks), ('S', Item::Stick)],
        result: Item::WoodenAxe,
        count: 1,
    },
    ShapedDef {
        id: "crafting/wooden_shovel",
        pat: &["W", "S", "S"],
        map: &[('W', Item::OakPlanks), ('S', Item::Stick)],
        result: Item::WoodenShovel,
        count: 1,
    },
    ShapedDef {
        id: "crafting/arrow",
        pat: &["G", "S", "F"],
        map: &[
            ('G', Item::Gravel),
            ('S', Item::Stick),
            ('F', Item::Feather),
        ],
        result: Item::Arrow,
        count: 4,
    },
    ShapedDef {
        id: "crafting/wooden_hoe",
        pat: &["WW", " S", " S"],
        map: &[('W', Item::OakPlanks), ('S', Item::Stick)],
        result: Item::WoodenHoe,
        count: 1,
    },
    ShapedDef {
        id: "crafting/stone_hoe",
        pat: &["CC", " S", " S"],
        map: &[('C', Item::Cobblestone), ('S', Item::Stick)],
        result: Item::StoneHoe,
        count: 1,
    },
    ShapedDef {
        id: "crafting/iron_hoe",
        pat: &["II", " S", " S"],
        map: &[('I', Item::IronIngot), ('S', Item::Stick)],
        result: Item::IronHoe,
        count: 1,
    },
    ShapedDef {
        id: "crafting/golden_hoe",
        pat: &["GG", " S", " S"],
        map: &[('G', Item::GoldIngot), ('S', Item::Stick)],
        result: Item::GoldenHoe,
        count: 1,
    },
    ShapedDef {
        id: "crafting/diamond_hoe",
        pat: &["DD", " S", " S"],
        map: &[('D', Item::Diamond), ('S', Item::Stick)],
        result: Item::DiamondHoe,
        count: 1,
    },
    ShapedDef {
        id: "crafting/glowstone",
        pat: &["DD", "DD"],
        map: &[('D', Item::GlowstoneDust)],
        result: Item::Glowstone,
        count: 1,
    },
    ShapedDef {
        id: "crafting/end_crystal",
        pat: &["GGG", "GEG", "GTG"],
        map: &[
            ('G', Item::Glass),
            ('E', Item::EyeOfEnder),
            ('T', Item::GhastTear),
        ],
        result: Item::EndCrystal,
        count: 1,
    },
    ShapedDef {
        id: "crafting/redstone_torch",
        pat: &["R", "S"],
        map: &[('R', Item::RedstoneDust), ('S', Item::Stick)],
        result: Item::RedstoneTorch,
        count: 1,
    },
    ShapedDef {
        id: "crafting/repeater",
        pat: &["TRT", "SSS"],
        map: &[
            ('T', Item::RedstoneTorch),
            ('R', Item::RedstoneDust),
            ('S', Item::Stone),
        ],
        result: Item::Repeater,
        count: 1,
    },
    ShapedDef {
        id: "crafting/comparator",
        pat: &[" T ", "TRT", "SSS"],
        map: &[
            ('T', Item::RedstoneTorch),
            ('R', Item::RedstoneDust),
            ('S', Item::Stone),
        ],
        result: Item::Comparator,
        count: 1,
    },
    ShapedDef {
        id: "crafting/lever",
        pat: &["S", "C"],
        map: &[('S', Item::Stick), ('C', Item::Cobblestone)],
        result: Item::Lever,
        count: 1,
    },
    ShapedDef {
        id: "crafting/pressure_plate",
        pat: &["SS"],
        map: &[('S', Item::Stone)],
        result: Item::PressurePlate,
        count: 1,
    },
    ShapedDef {
        id: "crafting/piston",
        pat: &["PPP", "CIC", "CRC"],
        map: &[
            ('P', Item::OakPlanks),
            ('C', Item::Cobblestone),
            ('I', Item::IronIngot),
            ('R', Item::RedstoneDust),
        ],
        result: Item::Piston,
        count: 1,
    },
    ShapedDef {
        id: "crafting/redstone_lamp",
        pat: &[" R ", "RGR", " R "],
        map: &[('R', Item::RedstoneDust), ('G', Item::GlowstoneDust)],
        result: Item::RedstoneLamp,
        count: 1,
    },
    ShapedDef {
        id: "crafting/oak_door",
        pat: &["PP", "PP", "PP"],
        map: &[('P', Item::OakPlanks)],
        result: Item::OakDoor,
        count: 3,
    },
    ShapedDef {
        id: "crafting/oak_trapdoor",
        pat: &["PPP", "PPP"],
        map: &[('P', Item::OakPlanks)],
        result: Item::OakTrapdoor,
        count: 2,
    },
    ShapedDef {
        id: "crafting/dispenser",
        pat: &["CCC", "CBC", "CRC"],
        map: &[
            ('C', Item::Cobblestone),
            ('B', Item::Bow),
            ('R', Item::RedstoneDust),
        ],
        result: Item::Dispenser,
        count: 1,
    },
    ShapedDef {
        id: "crafting/dropper",
        pat: &["CCC", "C C", "CRC"],
        map: &[('C', Item::Cobblestone), ('R', Item::RedstoneDust)],
        result: Item::Dropper,
        count: 1,
    },
    ShapedDef {
        id: "crafting/note_block",
        pat: &["PPP", "PRP", "PPP"],
        map: &[('P', Item::OakPlanks), ('R', Item::RedstoneDust)],
        result: Item::NoteBlock,
        count: 1,
    },
    ShapedDef {
        id: "crafting/oak_slab",
        pat: &["PPP"],
        map: &[('P', Item::OakPlanks)],
        result: Item::OakSlab,
        count: 6,
    },
    ShapedDef {
        id: "crafting/cobblestone_slab",
        pat: &["CCC"],
        map: &[('C', Item::Cobblestone)],
        result: Item::CobblestoneSlab,
        count: 6,
    },
    ShapedDef {
        id: "crafting/oak_stair",
        pat: &["P  ", "PP ", "PPP"],
        map: &[('P', Item::OakPlanks)],
        result: Item::OakStair,
        count: 4,
    },
    ShapedDef {
        id: "crafting/cobblestone_stair",
        pat: &["C  ", "CC ", "CCC"],
        map: &[('C', Item::Cobblestone)],
        result: Item::CobblestoneStair,
        count: 4,
    },
    ShapedDef {
        id: "crafting/oak_fence",
        pat: &["PSP", "PSP"],
        map: &[('P', Item::OakPlanks), ('S', Item::Stick)],
        result: Item::OakFence,
        count: 3,
    },
    ShapedDef {
        id: "crafting/oak_fence_gate",
        pat: &["SPS", "SPS"],
        map: &[('P', Item::OakPlanks), ('S', Item::Stick)],
        result: Item::OakFenceGate,
        count: 1,
    },
    ShapedDef {
        id: "crafting/cobblestone_wall",
        pat: &["CCC", "CCC"],
        map: &[('C', Item::Cobblestone)],
        result: Item::CobblestoneWall,
        count: 6,
    },
    ShapedDef {
        id: "crafting/glass_pane",
        pat: &["GGG", "GGG"],
        map: &[('G', Item::Glass)],
        result: Item::GlassPane,
        count: 16,
    },
    ShapedDef {
        id: "crafting/oak_ladder",
        pat: &["S S", "SSS", "S S"],
        map: &[('S', Item::Stick)],
        result: Item::OakLadder,
        count: 3,
    },
    ShapedDef {
        id: "crafting/oak_sign",
        pat: &["PPP", "PPP", " S "],
        map: &[('P', Item::OakPlanks), ('S', Item::Stick)],
        result: Item::OakSign,
        count: 3,
    },
];

const SHAPELESS: &[ShapelessDef] = &[
    ShapelessDef {
        id: "crafting/bone_meal",
        ingredients: &[Item::Bone],
        result: Item::BoneMeal,
        count: 3,
    },
    ShapelessDef {
        id: "crafting/flint_and_steel",
        ingredients: &[Item::IronIngot, Item::Gravel],
        result: Item::FlintAndSteel,
        count: 1,
    },
    ShapelessDef {
        id: "crafting/blaze_powder",
        ingredients: &[Item::BlazeRod],
        result: Item::BlazePowder,
        count: 2,
    },
    ShapelessDef {
        id: "crafting/eye_of_ender",
        ingredients: &[Item::Diamond, Item::BlazePowder],
        result: Item::EyeOfEnder,
        count: 1,
    },
    ShapelessDef {
        id: "crafting/sugar",
        ingredients: &[Item::SugarCane],
        result: Item::Sugar,
        count: 1,
    },
    ShapelessDef {
        id: "crafting/redstone_wire",
        ingredients: &[Item::RedstoneDust],
        result: Item::RedstoneWire,
        count: 1,
    },
    ShapelessDef {
        id: "crafting/stone_button",
        ingredients: &[Item::Stone],
        result: Item::StoneButton,
        count: 1,
    },
    ShapelessDef {
        id: "crafting/sticky_piston",
        ingredients: &[Item::Piston, Item::SugarCane],
        result: Item::StickyPiston,
        count: 1,
    },
];

const SMELTING: &[SmeltDef] = &[
    SmeltDef {
        id: "smelting/iron_ingot",
        input: Item::IronOre,
        output: Item::IronIngot,
        count: 1,
        cook_time: 200,
        experience: 0.7,
    },
    SmeltDef {
        id: "smelting/gold_ingot",
        input: Item::GoldOre,
        output: Item::GoldIngot,
        count: 1,
        cook_time: 200,
        experience: 1.0,
    },
    SmeltDef {
        id: "smelting/glass",
        input: Item::Sand,
        output: Item::Glass,
        count: 1,
        cook_time: 200,
        experience: 0.1,
    },
    SmeltDef {
        id: "smelting/stone",
        input: Item::Cobblestone,
        output: Item::Stone,
        count: 1,
        cook_time: 200,
        experience: 0.1,
    },
    SmeltDef {
        id: "smelting/brick",
        input: Item::Clay,
        output: Item::Brick,
        count: 1,
        cook_time: 200,
        experience: 0.3,
    },
    SmeltDef {
        id: "smelting/charcoal_oak",
        input: Item::OakLog,
        output: Item::Coal,
        count: 1,
        cook_time: 200,
        experience: 0.15,
    },
    SmeltDef {
        id: "smelting/cooked_porkchop",
        input: Item::RawPorkchop,
        output: Item::CookedPorkchop,
        count: 1,
        cook_time: 200,
        experience: 0.35,
    },
    SmeltDef {
        id: "smelting/cooked_beef",
        input: Item::RawBeef,
        output: Item::CookedBeef,
        count: 1,
        cook_time: 200,
        experience: 0.35,
    },
    SmeltDef {
        id: "smelting/cooked_mutton",
        input: Item::RawMutton,
        output: Item::CookedMutton,
        count: 1,
        cook_time: 200,
        experience: 0.35,
    },
    SmeltDef {
        id: "smelting/cooked_chicken",
        input: Item::RawChicken,
        output: Item::CookedChicken,
        count: 1,
        cook_time: 200,
        experience: 0.35,
    },
    SmeltDef {
        id: "smelting/baked_potato",
        input: Item::Potato,
        output: Item::BakedPotato,
        count: 1,
        cook_time: 200,
        experience: 0.35,
    },
    SmeltDef {
        id: "smelting/charcoal_birch",
        input: Item::BirchLog,
        output: Item::Coal,
        count: 1,
        cook_time: 200,
        experience: 0.15,
    },
    SmeltDef {
        id: "smelting/charcoal_spruce",
        input: Item::SpruceLog,
        output: Item::Coal,
        count: 1,
        cook_time: 200,
        experience: 0.15,
    },
    SmeltDef {
        id: "smelting/nether_brick",
        input: Item::Netherrack,
        output: Item::NetherBrick,
        count: 1,
        cook_time: 200,
        experience: 0.1,
    },
];

impl RecipeManager {
    pub fn new() -> Self {
        let mut crafting_recipes = Vec::new();
        let mut smelting_recipes = Vec::new();

        // Wood family expansion — registration order matches the locked golden:
        // all planks → bed → sticks → crafting tables → chests → rest.
        for w in WOODS {
            let id: RecipeId = match w.name {
                "oak" => "crafting/oak_planks",
                "birch" => "crafting/birch_planks",
                "spruce" => "crafting/spruce_planks",
                _ => unreachable!(),
            };
            add_shaped(
                &mut crafting_recipes,
                id,
                &["L"],
                &[('L', w.log)],
                ItemStack::new(w.planks, 4),
            );
        }

        for def in SHAPED {
            // Bed is the first SHAPED entry and must sit between planks and sticks.
            if def.id == "crafting/bed" {
                add_shaped(
                    &mut crafting_recipes,
                    def.id,
                    def.pat,
                    def.map,
                    ItemStack::new(def.result, def.count),
                );
                break;
            }
        }

        for w in WOODS {
            let id: RecipeId = match w.name {
                "oak" => "crafting/stick_oak",
                "birch" => "crafting/stick_birch",
                "spruce" => "crafting/stick_spruce",
                _ => unreachable!(),
            };
            add_shaped(
                &mut crafting_recipes,
                id,
                &["P", "P"],
                &[('P', w.planks)],
                ItemStack::new(Item::Stick, 4),
            );
        }
        for w in WOODS {
            let id: RecipeId = match w.name {
                "oak" => "crafting/crafting_table_oak",
                "birch" => "crafting/crafting_table_birch",
                "spruce" => "crafting/crafting_table_spruce",
                _ => unreachable!(),
            };
            add_shaped(
                &mut crafting_recipes,
                id,
                &["PP", "PP"],
                &[('P', w.planks)],
                ItemStack::new(Item::CraftingTable, 1),
            );
        }
        for w in WOODS {
            let id: RecipeId = match w.name {
                "oak" => "crafting/chest_oak",
                "birch" => "crafting/chest_birch",
                "spruce" => "crafting/chest_spruce",
                _ => unreachable!(),
            };
            add_shaped(
                &mut crafting_recipes,
                id,
                &["PPP", "P P", "PPP"],
                &[('P', w.planks)],
                ItemStack::new(Item::Chest, 1),
            );
        }

        for def in SHAPED {
            if def.id == "crafting/bed" {
                continue; // already registered above
            }
            add_shaped(
                &mut crafting_recipes,
                def.id,
                def.pat,
                def.map,
                ItemStack::new(def.result, def.count),
            );
        }
        for def in SHAPELESS {
            add_shapeless(
                &mut crafting_recipes,
                def.id,
                def.ingredients.to_vec(),
                ItemStack::new(def.result, def.count),
            );
        }
        for def in SMELTING {
            smelting_recipes.push(SmeltingRecipe {
                id: def.id,
                input: def.input,
                output: ItemStack::new(def.output, def.count),
                cook_time: def.cook_time,
                experience: def.experience,
            });
        }

        let mut shaped_index: HashMap<(usize, usize, Item), Vec<usize>> = HashMap::new();
        for (idx, recipe) in crafting_recipes.iter().enumerate() {
            if recipe.shapeless {
                continue;
            }
            let key = (recipe.width, recipe.height, recipe.pattern[0][0]);
            shaped_index.entry(key).or_default().push(idx);
        }

        let mut smelting_by_input = HashMap::new();
        for (idx, recipe) in smelting_recipes.iter().enumerate() {
            smelting_by_input.insert(recipe.input, idx);
        }

        Self {
            crafting_recipes,
            smelting_recipes,
            shaped_index,
            smelting_by_input,
        }
    }

    pub fn get_smelting_recipes(&self) -> &[SmeltingRecipe] {
        &self.smelting_recipes
    }

    pub fn is_fuel(&self, item: Item) -> bool {
        FuelDefinition::burn_time(item) > 0
    }

    pub fn find_smelting_recipe(&self, input: Item) -> Option<&SmeltingRecipe> {
        if input == Item::Air {
            return None;
        }
        self.smelting_by_input
            .get(&input)
            .map(|&idx| &self.smelting_recipes[idx])
    }

    pub fn match_crafting_recipe(
        &self,
        grid: &[Option<ItemStack>],
        grid_size: usize,
    ) -> Option<ItemStack> {
        let mut active_items = Vec::new();
        for slot in grid {
            if let Some(stack) = slot {
                if stack.item != Item::Air {
                    active_items.push(stack.item);
                }
            }
        }
        if active_items.is_empty() {
            return None;
        }
        active_items.sort_by_key(|&it| it as i32);

        // 1. Shapeless match (small set — linear is fine)
        for recipe in &self.crafting_recipes {
            if recipe.shapeless && recipe.pattern[0] == active_items {
                return Some(recipe.result);
            }
        }

        // 2. Shaped match via (w, h, first-cell) index
        let mut min_r = grid_size;
        let mut max_r = 0;
        let mut min_c = grid_size;
        let mut max_c = 0;
        let mut has_items = false;

        for r in 0..grid_size {
            for c in 0..grid_size {
                if let Some(stack) = grid[r * grid_size + c] {
                    if stack.item != Item::Air {
                        has_items = true;
                        min_r = min_r.min(r);
                        max_r = max_r.max(r);
                        min_c = min_c.min(c);
                        max_c = max_c.max(c);
                    }
                }
            }
        }

        if !has_items {
            return None;
        }

        let h_size = max_r - min_r + 1;
        let w_size = max_c - min_c + 1;

        let mut cropped = vec![vec![Item::Air; w_size]; h_size];
        for r in 0..h_size {
            for c in 0..w_size {
                if let Some(stack) = grid[(min_r + r) * grid_size + (min_c + c)] {
                    cropped[r][c] = stack.item;
                }
            }
        }

        let key = (w_size, h_size, cropped[0][0]);
        let candidates = self.shaped_index.get(&key)?;
        for &idx in candidates {
            let recipe = &self.crafting_recipes[idx];
            if recipe.pattern == cropped {
                return Some(recipe.result);
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::fmt::Write as _;

    #[test]
    fn test_recipe_id_uniqueness() {
        let manager = RecipeManager::new();
        let mut seen = HashSet::new();
        for r in &manager.crafting_recipes {
            assert!(seen.insert(r.id), "Duplicate crafting recipe ID: {}", r.id);
            assert_ne!(
                r.result.item,
                Item::Air,
                "Crafting recipe result is Air: {}",
                r.id
            );
            assert!(
                r.result.count > 0,
                "Crafting recipe result count 0: {}",
                r.id
            );
        }
        for r in &manager.smelting_recipes {
            assert!(seen.insert(r.id), "Duplicate smelting recipe ID: {}", r.id);
            assert_ne!(
                r.output.item,
                Item::Air,
                "Smelting recipe output is Air: {}",
                r.id
            );
            assert!(
                r.output.count > 0,
                "Smelting recipe output count 0: {}",
                r.id
            );
        }
    }

    #[test]
    fn test_bread_recipe_requires_wheat() {
        let manager = RecipeManager::new();
        let mut apple_grid = vec![None; 9];
        apple_grid[0] = Some(ItemStack::new(Item::Apple, 1));
        apple_grid[1] = Some(ItemStack::new(Item::Apple, 1));
        apple_grid[2] = Some(ItemStack::new(Item::Apple, 1));
        assert!(manager.match_crafting_recipe(&apple_grid, 3).is_none());

        let mut wheat_grid = vec![None; 9];
        wheat_grid[0] = Some(ItemStack::new(Item::Wheat, 1));
        wheat_grid[1] = Some(ItemStack::new(Item::Wheat, 1));
        wheat_grid[2] = Some(ItemStack::new(Item::Wheat, 1));
        let res = manager.match_crafting_recipe(&wheat_grid, 3);
        assert!(res.is_some());
        assert_eq!(res.unwrap().item, Item::Bread);
    }

    #[test]
    fn test_ore_cannot_be_crafted_in_grid() {
        let manager = RecipeManager::new();
        let mut grid = vec![None; 4];
        grid[0] = Some(ItemStack::new(Item::IronOre, 1));
        assert!(manager.match_crafting_recipe(&grid, 2).is_none());

        let mut grid_gold = vec![None; 4];
        grid_gold[0] = Some(ItemStack::new(Item::GoldOre, 1));
        assert!(manager.match_crafting_recipe(&grid_gold, 2).is_none());

        let iron_smelt = manager.find_smelting_recipe(Item::IronOre);
        assert!(iron_smelt.is_some());
        assert_eq!(iron_smelt.unwrap().output.item, Item::IronIngot);

        let gold_smelt = manager.find_smelting_recipe(Item::GoldOre);
        assert!(gold_smelt.is_some());
        assert_eq!(gold_smelt.unwrap().output.item, Item::GoldIngot);
    }

    #[test]
    fn test_fuels() {
        assert_eq!(FuelDefinition::burn_time(Item::Coal), 1600);
        assert_eq!(FuelDefinition::burn_time(Item::OakLog), 300);
        assert_eq!(FuelDefinition::burn_time(Item::OakPlanks), 300);
        assert_eq!(FuelDefinition::burn_time(Item::Stick), 100);
        assert_eq!(FuelDefinition::burn_time(Item::Dirt), 0);
    }

    #[test]
    fn recipe_golden_snapshot_is_byte_identical() {
        let mgr = RecipeManager::new();
        let mut craft_entries: Vec<(String, String, String)> = Vec::new();
        for r in &mgr.crafting_recipes {
            let pattern = if r.shapeless {
                format!(
                    "shapeless:{}",
                    r.pattern[0]
                        .iter()
                        .map(|i| format!("{i:?}"))
                        .collect::<Vec<_>>()
                        .join(",")
                )
            } else {
                r.pattern
                    .iter()
                    .map(|row| {
                        row.iter()
                            .map(|i| {
                                if *i == Item::Air {
                                    ".".into()
                                } else {
                                    format!("{i:?}")
                                }
                            })
                            .collect::<Vec<_>>()
                            .join(",")
                    })
                    .collect::<Vec<_>>()
                    .join(";")
            };
            let head = format!(
                "craft|{}|{}x{}|{}|{:?}|{}",
                r.id, r.width, r.height, r.shapeless as u8, r.result.item, r.result.count
            );
            craft_entries.push((r.id.to_string(), head, format!("  pattern|{pattern}")));
        }
        craft_entries.sort_by(|a, b| a.0.cmp(&b.0));

        let mut smelt_entries: Vec<(String, String)> = Vec::new();
        for r in &mgr.smelting_recipes {
            let head = format!(
                "smelt|{}|{:?}|{:?}|{}|{}|{:.3}",
                r.id, r.input, r.output.item, r.output.count, r.cook_time, r.experience
            );
            smelt_entries.push((r.id.to_string(), head));
        }
        smelt_entries.sort_by(|a, b| a.0.cmp(&b.0));

        let mut out = String::new();
        for (_, head, pat) in &craft_entries {
            writeln!(out, "{head}").unwrap();
            writeln!(out, "{pat}").unwrap();
        }
        for (_, head) in &smelt_entries {
            writeln!(out, "{head}").unwrap();
        }
        let snapshot = out.trim_end().to_string();
        let expected = include_str!("recipe_golden_snapshot.txt")
            .replace("\r\n", "\n")
            .trim_end()
            .to_string();
        assert_eq!(
            snapshot, expected,
            "Recipe table drifted from the locked golden snapshot"
        );
    }

    #[test]
    fn recipe_manager_new_stays_compact() {
        // Guardrail: registration body should stay table-driven (~200 lines).
        let src = include_str!("recipes.rs");
        let start = src
            .find("pub fn new() -> Self {")
            .expect("RecipeManager::new");
        let rest = &src[start..];
        let end = rest
            .find("\n    pub fn get_smelting_recipes")
            .expect("next method");
        let new_body = &rest[..end];
        let lines = new_body.lines().count();
        assert!(
            lines <= 200,
            "RecipeManager::new is {lines} lines; keep ≤200 via tables"
        );
    }
}
