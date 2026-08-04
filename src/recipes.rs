use crate::inventory::{Item, ItemStack};

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
}

fn add_shaped(
    recipes: &mut Vec<CraftingRecipe>,
    id: RecipeId,
    pat: Vec<&str>,
    mapping: &[(&str, Item)],
    result: ItemStack,
) {
    let height = pat.len();
    let width = pat[0].len();
    let mut pattern = vec![vec![Item::Air; width]; height];
    for r in 0..height {
        let chars: Vec<char> = pat[r].chars().collect();
        for c in 0..width {
            let ch = chars[c].to_string();
            if ch != " " {
                let item = mapping
                    .iter()
                    .find(|(s, _)| s == &ch)
                    .map(|(_, it)| *it)
                    .unwrap_or(Item::Air);
                pattern[r][c] = item;
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

impl RecipeManager {
    pub fn new() -> Self {
        let mut crafting_recipes = Vec::new();
        let mut smelting_recipes = Vec::new();

        // --- Crafting Recipes ---

        // 1. Logs -> Planks
        add_shaped(
            &mut crafting_recipes,
            "crafting/oak_planks",
            vec!["L"],
            &[("L", Item::OakLog)],
            ItemStack::new(Item::OakPlanks, 4),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/birch_planks",
            vec!["L"],
            &[("L", Item::BirchLog)],
            ItemStack::new(Item::BirchPlanks, 4),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/spruce_planks",
            vec!["L"],
            &[("L", Item::SpruceLog)],
            ItemStack::new(Item::SprucePlanks, 4),
        );

        // Bed
        add_shaped(
            &mut crafting_recipes,
            "crafting/bed",
            vec!["WWW", "PPP"],
            &[("W", Item::Wool), ("P", Item::OakPlanks)],
            ItemStack::new(Item::Bed, 1),
        );

        // 2. Sticks
        add_shaped(
            &mut crafting_recipes,
            "crafting/stick_oak",
            vec!["P", "P"],
            &[("P", Item::OakPlanks)],
            ItemStack::new(Item::Stick, 4),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/stick_birch",
            vec!["P", "P"],
            &[("P", Item::BirchPlanks)],
            ItemStack::new(Item::Stick, 4),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/stick_spruce",
            vec!["P", "P"],
            &[("P", Item::SprucePlanks)],
            ItemStack::new(Item::Stick, 4),
        );

        // 3. Crafting Table
        add_shaped(
            &mut crafting_recipes,
            "crafting/crafting_table_oak",
            vec!["PP", "PP"],
            &[("P", Item::OakPlanks)],
            ItemStack::new(Item::CraftingTable, 1),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/crafting_table_birch",
            vec!["PP", "PP"],
            &[("P", Item::BirchPlanks)],
            ItemStack::new(Item::CraftingTable, 1),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/crafting_table_spruce",
            vec!["PP", "PP"],
            &[("P", Item::SprucePlanks)],
            ItemStack::new(Item::CraftingTable, 1),
        );

        // 4. Chest
        add_shaped(
            &mut crafting_recipes,
            "crafting/chest_oak",
            vec!["PPP", "P P", "PPP"],
            &[("P", Item::OakPlanks)],
            ItemStack::new(Item::Chest, 1),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/chest_birch",
            vec!["PPP", "P P", "PPP"],
            &[("P", Item::BirchPlanks)],
            ItemStack::new(Item::Chest, 1),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/chest_spruce",
            vec!["PPP", "P P", "PPP"],
            &[("P", Item::SprucePlanks)],
            ItemStack::new(Item::Chest, 1),
        );

        // 5. Furnace
        add_shaped(
            &mut crafting_recipes,
            "crafting/furnace",
            vec!["CCC", "C C", "CCC"],
            &[("C", Item::Cobblestone)],
            ItemStack::new(Item::Furnace, 1),
        );

        // 6. Torch
        add_shaped(
            &mut crafting_recipes,
            "crafting/torch",
            vec!["C", "S"],
            &[("C", Item::Coal), ("S", Item::Stick)],
            ItemStack::new(Item::Torch, 4),
        );

        // NOTE: Ore shapeless conversions (IronOre -> IronIngot, GoldOre -> GoldIngot)
        // have been explicitly removed per Plan 03 requirement.

        // 7. Stone Tools
        add_shaped(
            &mut crafting_recipes,
            "crafting/stone_pickaxe",
            vec!["SSS", " t ", " t "],
            &[("S", Item::Cobblestone), ("t", Item::Stick)],
            ItemStack::new(Item::StonePickaxe, 1),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/stone_axe",
            vec!["SS ", "St ", " t "],
            &[("S", Item::Cobblestone), ("t", Item::Stick)],
            ItemStack::new(Item::StoneAxe, 1),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/stone_shovel",
            vec!["S", "t", "t"],
            &[("S", Item::Cobblestone), ("t", Item::Stick)],
            ItemStack::new(Item::StoneShovel, 1),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/stone_sword",
            vec!["S", "S", "t"],
            &[("S", Item::Cobblestone), ("t", Item::Stick)],
            ItemStack::new(Item::StoneSword, 1),
        );

        // 8. Iron Tools
        add_shaped(
            &mut crafting_recipes,
            "crafting/iron_pickaxe",
            vec!["III", " t ", " t "],
            &[("I", Item::IronIngot), ("t", Item::Stick)],
            ItemStack::new(Item::IronPickaxe, 1),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/iron_axe",
            vec!["II ", "It ", " t "],
            &[("I", Item::IronIngot), ("t", Item::Stick)],
            ItemStack::new(Item::IronAxe, 1),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/iron_shovel",
            vec!["I", "t", "t"],
            &[("I", Item::IronIngot), ("t", Item::Stick)],
            ItemStack::new(Item::IronShovel, 1),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/iron_sword",
            vec!["I", "I", "t"],
            &[("I", Item::IronIngot), ("t", Item::Stick)],
            ItemStack::new(Item::IronSword, 1),
        );

        // 9. Diamond Tools
        add_shaped(
            &mut crafting_recipes,
            "crafting/diamond_pickaxe",
            vec!["DDD", " t ", " t "],
            &[("D", Item::Diamond), ("t", Item::Stick)],
            ItemStack::new(Item::DiamondPickaxe, 1),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/diamond_axe",
            vec!["DD ", "Dt ", " t "],
            &[("D", Item::Diamond), ("t", Item::Stick)],
            ItemStack::new(Item::DiamondAxe, 1),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/diamond_shovel",
            vec!["D", "t", "t"],
            &[("D", Item::Diamond), ("t", Item::Stick)],
            ItemStack::new(Item::DiamondShovel, 1),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/diamond_sword",
            vec!["D", "D", "t"],
            &[("D", Item::Diamond), ("t", Item::Stick)],
            ItemStack::new(Item::DiamondSword, 1),
        );

        // 10. Block Conversions & Miscellaneous
        add_shaped(
            &mut crafting_recipes,
            "crafting/stone_brick",
            vec!["SS", "SS"],
            &[("S", Item::Stone)],
            ItemStack::new(Item::StoneBrick, 4),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/brick",
            vec!["CC", "CC"],
            &[("C", Item::Clay)],
            ItemStack::new(Item::Brick, 4),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/sandstone",
            vec!["SS", "SS"],
            &[("S", Item::Sand)],
            ItemStack::new(Item::Sandstone, 4),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/snow_block",
            vec!["SS", "SS"],
            &[("S", Item::Snow)],
            ItemStack::new(Item::Snow, 1),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/tnt",
            vec!["RSR", "SRS", "RSR"],
            &[("R", Item::Redstone), ("S", Item::Sand)],
            ItemStack::new(Item::TNT, 1),
        );

        // Bread (3 Wheat) - Corrected from 3 Apples per Plan 03 requirement.
        add_shaped(
            &mut crafting_recipes,
            "crafting/bread",
            vec!["WWW"],
            &[("W", Item::Wheat)],
            ItemStack::new(Item::Bread, 1),
        );

        // Workstations & Equipment
        add_shaped(
            &mut crafting_recipes,
            "crafting/enchanting_table",
            vec![" B ", "D D", "OOO"],
            &[
                ("B", Item::Bookshelf),
                ("D", Item::Diamond),
                ("O", Item::Obsidian),
            ],
            ItemStack::new(Item::EnchantingTable, 1),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/brewing_stand",
            vec![" B ", "CCC"],
            &[("B", Item::BlazePowder), ("C", Item::Cobblestone)],
            ItemStack::new(Item::BrewingStand, 1),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/anvil",
            vec!["III", " I ", "III"],
            &[("I", Item::IronIngot)],
            ItemStack::new(Item::Anvil, 1),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/glass_bottle",
            vec!["G G", " G "],
            &[("G", Item::Glass)],
            ItemStack::new(Item::GlassBottle, 3),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/iron_helmet",
            vec!["III", "I I"],
            &[("I", Item::IronIngot)],
            ItemStack::new(Item::IronHelmet, 1),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/iron_chestplate",
            vec!["I I", "III", "III"],
            &[("I", Item::IronIngot)],
            ItemStack::new(Item::IronChestplate, 1),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/iron_leggings",
            vec!["III", "I I", "I I"],
            &[("I", Item::IronIngot)],
            ItemStack::new(Item::IronLeggings, 1),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/iron_boots",
            vec!["I I", "I I"],
            &[("I", Item::IronIngot)],
            ItemStack::new(Item::IronBoots, 1),
        );

        // Leather Armor
        add_shaped(
            &mut crafting_recipes,
            "crafting/leather_helmet",
            vec!["LLL", "L L"],
            &[("L", Item::Leather)],
            ItemStack::new(Item::LeatherHelmet, 1),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/leather_chestplate",
            vec!["L L", "LLL", "LLL"],
            &[("L", Item::Leather)],
            ItemStack::new(Item::LeatherChestplate, 1),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/leather_leggings",
            vec!["LLL", "L L", "L L"],
            &[("L", Item::Leather)],
            ItemStack::new(Item::LeatherLeggings, 1),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/leather_boots",
            vec!["L L", "L L"],
            &[("L", Item::Leather)],
            ItemStack::new(Item::LeatherBoots, 1),
        );

        // Diamond Armor
        add_shaped(
            &mut crafting_recipes,
            "crafting/diamond_helmet",
            vec!["DDD", "D D"],
            &[("D", Item::Diamond)],
            ItemStack::new(Item::DiamondHelmet, 1),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/diamond_chestplate",
            vec!["D D", "DDD", "DDD"],
            &[("D", Item::Diamond)],
            ItemStack::new(Item::DiamondChestplate, 1),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/diamond_leggings",
            vec!["DDD", "D D", "D D"],
            &[("D", Item::Diamond)],
            ItemStack::new(Item::DiamondLeggings, 1),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/diamond_boots",
            vec!["D D", "D D"],
            &[("D", Item::Diamond)],
            ItemStack::new(Item::DiamondBoots, 1),
        );

        // Shield
        add_shaped(
            &mut crafting_recipes,
            "crafting/shield",
            vec!["PIP", "PPP", " P "],
            &[("P", Item::OakPlanks), ("I", Item::IronIngot)],
            ItemStack::new(Item::Shield, 1),
        );

        // Wooden Tools
        add_shaped(
            &mut crafting_recipes,
            "crafting/wooden_sword",
            vec!["W", "W", "S"],
            &[("W", Item::OakPlanks), ("S", Item::Stick)],
            ItemStack::new(Item::WoodenSword, 1),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/wooden_pickaxe",
            vec!["WWW", " S ", " S "],
            &[("W", Item::OakPlanks), ("S", Item::Stick)],
            ItemStack::new(Item::WoodenPickaxe, 1),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/wooden_axe",
            vec!["WW", "WS", " S"],
            &[("W", Item::OakPlanks), ("S", Item::Stick)],
            ItemStack::new(Item::WoodenAxe, 1),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/wooden_shovel",
            vec!["W", "S", "S"],
            &[("W", Item::OakPlanks), ("S", Item::Stick)],
            ItemStack::new(Item::WoodenShovel, 1),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/arrow",
            vec!["G", "S", "F"],
            &[
                ("G", Item::Gravel),
                ("S", Item::Stick),
                ("F", Item::Feather),
            ],
            ItemStack::new(Item::Arrow, 4),
        );

        // Farming tools & Bone Meal
        add_shaped(
            &mut crafting_recipes,
            "crafting/wooden_hoe",
            vec!["WW", " S", " S"],
            &[("W", Item::OakPlanks), ("S", Item::Stick)],
            ItemStack::new(Item::WoodenHoe, 1),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/stone_hoe",
            vec!["CC", " S", " S"],
            &[("C", Item::Cobblestone), ("S", Item::Stick)],
            ItemStack::new(Item::StoneHoe, 1),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/iron_hoe",
            vec!["II", " S", " S"],
            &[("I", Item::IronIngot), ("S", Item::Stick)],
            ItemStack::new(Item::IronHoe, 1),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/golden_hoe",
            vec!["GG", " S", " S"],
            &[("G", Item::GoldIngot), ("S", Item::Stick)],
            ItemStack::new(Item::GoldenHoe, 1),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/diamond_hoe",
            vec!["DD", " S", " S"],
            &[("D", Item::Diamond), ("S", Item::Stick)],
            ItemStack::new(Item::DiamondHoe, 1),
        );
        add_shapeless(
            &mut crafting_recipes,
            "crafting/bone_meal",
            vec![Item::Bone],
            ItemStack::new(Item::BoneMeal, 3),
        );

        // Dimension progression
        add_shapeless(
            &mut crafting_recipes,
            "crafting/flint_and_steel",
            vec![Item::IronIngot, Item::Gravel],
            ItemStack::new(Item::FlintAndSteel, 1),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/glowstone",
            vec!["DD", "DD"],
            &[("D", Item::GlowstoneDust)],
            ItemStack::new(Item::Glowstone, 1),
        );
        add_shapeless(
            &mut crafting_recipes,
            "crafting/blaze_powder",
            vec![Item::BlazeRod],
            ItemStack::new(Item::BlazePowder, 2),
        );
        add_shapeless(
            &mut crafting_recipes,
            "crafting/eye_of_ender",
            vec![Item::Diamond, Item::BlazePowder],
            ItemStack::new(Item::EyeOfEnder, 1),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/end_crystal",
            vec!["GGG", "GEG", "GTG"],
            &[
                ("G", Item::Glass),
                ("E", Item::EyeOfEnder),
                ("T", Item::GhastTear),
            ],
            ItemStack::new(Item::EndCrystal, 1),
        );

        // SugarCane -> Sugar (Standard)
        add_shapeless(
            &mut crafting_recipes,
            "crafting/sugar",
            vec![Item::SugarCane],
            ItemStack::new(Item::Sugar, 1),
        );

        // Redstone components
        add_shapeless(
            &mut crafting_recipes,
            "crafting/redstone_wire",
            vec![Item::RedstoneDust],
            ItemStack::new(Item::RedstoneWire, 1),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/redstone_torch",
            vec!["R", "S"],
            &[("R", Item::RedstoneDust), ("S", Item::Stick)],
            ItemStack::new(Item::RedstoneTorch, 1),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/repeater",
            vec!["TRT", "SSS"],
            &[
                ("T", Item::RedstoneTorch),
                ("R", Item::RedstoneDust),
                ("S", Item::Stone),
            ],
            ItemStack::new(Item::Repeater, 1),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/comparator",
            vec![" T ", "TRT", "SSS"],
            &[
                ("T", Item::RedstoneTorch),
                ("R", Item::RedstoneDust),
                ("S", Item::Stone),
            ],
            ItemStack::new(Item::Comparator, 1),
        );
        add_shapeless(
            &mut crafting_recipes,
            "crafting/stone_button",
            vec![Item::Stone],
            ItemStack::new(Item::StoneButton, 1),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/lever",
            vec!["S", "C"],
            &[("S", Item::Stick), ("C", Item::Cobblestone)],
            ItemStack::new(Item::Lever, 1),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/pressure_plate",
            vec!["SS"],
            &[("S", Item::Stone)],
            ItemStack::new(Item::PressurePlate, 1),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/piston",
            vec!["PPP", "CIC", "CRC"],
            &[
                ("P", Item::OakPlanks),
                ("C", Item::Cobblestone),
                ("I", Item::IronIngot),
                ("R", Item::RedstoneDust),
            ],
            ItemStack::new(Item::Piston, 1),
        );
        add_shapeless(
            &mut crafting_recipes,
            "crafting/sticky_piston",
            vec![Item::Piston, Item::SugarCane],
            ItemStack::new(Item::StickyPiston, 1),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/redstone_lamp",
            vec![" R ", "RGR", " R "],
            &[("R", Item::RedstoneDust), ("G", Item::GlowstoneDust)],
            ItemStack::new(Item::RedstoneLamp, 1),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/oak_door",
            vec!["PP", "PP", "PP"],
            &[("P", Item::OakPlanks)],
            ItemStack::new(Item::OakDoor, 3),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/oak_trapdoor",
            vec!["PPP", "PPP"],
            &[("P", Item::OakPlanks)],
            ItemStack::new(Item::OakTrapdoor, 2),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/dispenser",
            vec!["CCC", "CBC", "CRC"],
            &[
                ("C", Item::Cobblestone),
                ("B", Item::Bow),
                ("R", Item::RedstoneDust),
            ],
            ItemStack::new(Item::Dispenser, 1),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/dropper",
            vec!["CCC", "C C", "CRC"],
            &[("C", Item::Cobblestone), ("R", Item::RedstoneDust)],
            ItemStack::new(Item::Dropper, 1),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/note_block",
            vec!["PPP", "PRP", "PPP"],
            &[("P", Item::OakPlanks), ("R", Item::RedstoneDust)],
            ItemStack::new(Item::NoteBlock, 1),
        );

        // Building blocks (Plan 06)
        add_shaped(
            &mut crafting_recipes,
            "crafting/oak_slab",
            vec!["PPP"],
            &[("P", Item::OakPlanks)],
            ItemStack::new(Item::OakSlab, 6),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/cobblestone_slab",
            vec!["CCC"],
            &[("C", Item::Cobblestone)],
            ItemStack::new(Item::CobblestoneSlab, 6),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/oak_stair",
            vec!["P  ", "PP ", "PPP"],
            &[("P", Item::OakPlanks)],
            ItemStack::new(Item::OakStair, 4),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/cobblestone_stair",
            vec!["C  ", "CC ", "CCC"],
            &[("C", Item::Cobblestone)],
            ItemStack::new(Item::CobblestoneStair, 4),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/oak_fence",
            vec!["PSP", "PSP"],
            &[("P", Item::OakPlanks), ("S", Item::Stick)],
            ItemStack::new(Item::OakFence, 3),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/oak_fence_gate",
            vec!["SPS", "SPS"],
            &[("P", Item::OakPlanks), ("S", Item::Stick)],
            ItemStack::new(Item::OakFenceGate, 1),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/cobblestone_wall",
            vec!["CCC", "CCC"],
            &[("C", Item::Cobblestone)],
            ItemStack::new(Item::CobblestoneWall, 6),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/glass_pane",
            vec!["GGG", "GGG"],
            &[("G", Item::Glass)],
            ItemStack::new(Item::GlassPane, 16),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/oak_ladder",
            vec!["S S", "SSS", "S S"],
            &[("S", Item::Stick)],
            ItemStack::new(Item::OakLadder, 3),
        );
        add_shaped(
            &mut crafting_recipes,
            "crafting/oak_sign",
            vec!["PPP", "PPP", " S "],
            &[("P", Item::OakPlanks), ("S", Item::Stick)],
            ItemStack::new(Item::OakSign, 3),
        );

        // --- Smelting Recipes ---

        smelting_recipes.push(SmeltingRecipe {
            id: "smelting/iron_ingot",
            input: Item::IronOre,
            output: ItemStack::new(Item::IronIngot, 1),
            cook_time: 200,
            experience: 0.7,
        });

        smelting_recipes.push(SmeltingRecipe {
            id: "smelting/gold_ingot",
            input: Item::GoldOre,
            output: ItemStack::new(Item::GoldIngot, 1),
            cook_time: 200,
            experience: 1.0,
        });

        smelting_recipes.push(SmeltingRecipe {
            id: "smelting/glass",
            input: Item::Sand,
            output: ItemStack::new(Item::Glass, 1),
            cook_time: 200,
            experience: 0.1,
        });

        smelting_recipes.push(SmeltingRecipe {
            id: "smelting/stone",
            input: Item::Cobblestone,
            output: ItemStack::new(Item::Stone, 1),
            cook_time: 200,
            experience: 0.1,
        });

        smelting_recipes.push(SmeltingRecipe {
            id: "smelting/brick",
            input: Item::Clay,
            output: ItemStack::new(Item::Brick, 1),
            cook_time: 200,
            experience: 0.3,
        });

        smelting_recipes.push(SmeltingRecipe {
            id: "smelting/charcoal_oak",
            input: Item::OakLog,
            output: ItemStack::new(Item::Coal, 1),
            cook_time: 200,
            experience: 0.15,
        });

        smelting_recipes.push(SmeltingRecipe {
            id: "smelting/cooked_porkchop",
            input: Item::RawPorkchop,
            output: ItemStack::new(Item::CookedPorkchop, 1),
            cook_time: 200,
            experience: 0.35,
        });

        smelting_recipes.push(SmeltingRecipe {
            id: "smelting/cooked_beef",
            input: Item::RawBeef,
            output: ItemStack::new(Item::CookedBeef, 1),
            cook_time: 200,
            experience: 0.35,
        });

        smelting_recipes.push(SmeltingRecipe {
            id: "smelting/cooked_chicken",
            input: Item::RawChicken,
            output: ItemStack::new(Item::CookedChicken, 1),
            cook_time: 200,
            experience: 0.35,
        });

        smelting_recipes.push(SmeltingRecipe {
            id: "smelting/cooked_mutton",
            input: Item::RawMutton,
            output: ItemStack::new(Item::CookedMutton, 1),
            cook_time: 200,
            experience: 0.35,
        });

        smelting_recipes.push(SmeltingRecipe {
            id: "smelting/baked_potato",
            input: Item::Potato,
            output: ItemStack::new(Item::BakedPotato, 1),
            cook_time: 200,
            experience: 0.35,
        });

        smelting_recipes.push(SmeltingRecipe {
            id: "smelting/charcoal_birch",
            input: Item::BirchLog,
            output: ItemStack::new(Item::Coal, 1),
            cook_time: 200,
            experience: 0.15,
        });

        smelting_recipes.push(SmeltingRecipe {
            id: "smelting/charcoal_spruce",
            input: Item::SpruceLog,
            output: ItemStack::new(Item::Coal, 1),
            cook_time: 200,
            experience: 0.15,
        });

        smelting_recipes.push(SmeltingRecipe {
            id: "smelting/nether_brick",
            input: Item::Netherrack,
            output: ItemStack::new(Item::NetherBrick, 1),
            cook_time: 200,
            experience: 0.1,
        });

        Self {
            crafting_recipes,
            smelting_recipes,
        }
    }

    pub fn get_smelting_recipes(&self) -> &[SmeltingRecipe] {
        &self.smelting_recipes
    }

    pub fn is_fuel(&self, item: Item) -> bool {
        FuelDefinition::burn_time(item) > 0
    }

    pub fn match_smelting(&self, item: Item) -> Option<&SmeltingRecipe> {
        self.find_smelting_recipe(item)
    }

    pub fn find_smelting_recipe(&self, input: Item) -> Option<&SmeltingRecipe> {
        if input == Item::Air {
            return None;
        }
        self.smelting_recipes.iter().find(|r| r.input == input)
    }

    pub fn match_recipe(&self, grid: &[Option<ItemStack>], grid_size: usize) -> Option<ItemStack> {
        self.match_crafting_recipe(grid, grid_size)
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

        // 1. Shapeless match
        for recipe in &self.crafting_recipes {
            if recipe.shapeless {
                if recipe.pattern[0] == active_items {
                    return Some(recipe.result);
                }
            }
        }

        // 2. Shaped Match: bounding box
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
                        if r < min_r {
                            min_r = r;
                        }
                        if r > max_r {
                            max_r = r;
                        }
                        if c < min_c {
                            min_c = c;
                        }
                        if c > max_c {
                            max_c = c;
                        }
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

        for recipe in &self.crafting_recipes {
            if recipe.shapeless {
                continue;
            }
            if recipe.width == w_size && recipe.height == h_size {
                let mut match_ok = true;
                for r in 0..h_size {
                    for c in 0..w_size {
                        if recipe.pattern[r][c] != cropped[r][c] {
                            match_ok = false;
                            break;
                        }
                    }
                    if !match_ok {
                        break;
                    }
                }
                if match_ok {
                    return Some(recipe.result);
                }
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

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
        // 3 Apples in a row should NOT yield Bread
        let mut apple_grid = vec![None; 9];
        apple_grid[0] = Some(ItemStack::new(Item::Apple, 1));
        apple_grid[1] = Some(ItemStack::new(Item::Apple, 1));
        apple_grid[2] = Some(ItemStack::new(Item::Apple, 1));
        assert!(manager.match_crafting_recipe(&apple_grid, 3).is_none());

        // 3 Wheat in a row SHOULD yield Bread
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

        // But ore SHOULD be smeltable
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
}
