use crate::inventory::{Item, ItemStack};

#[derive(Debug, Clone)]
pub struct Recipe {
    pub pattern: Vec<Vec<Item>>, // 2D grid
    pub width: usize,
    pub height: usize,
    pub result: ItemStack,
    pub shapeless: bool,
}

pub struct RecipeManager {
    pub recipes: Vec<Recipe>,
}

fn add_shaped(
    recipes: &mut Vec<Recipe>,
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
    recipes.push(Recipe {
        pattern,
        width,
        height,
        result,
        shapeless: false,
    });
}

fn add_shapeless(recipes: &mut Vec<Recipe>, ingredients: Vec<Item>, result: ItemStack) {
    let mut sorted = ingredients;
    sorted.sort_by_key(|&it| it as i32);
    recipes.push(Recipe {
        pattern: vec![sorted],
        width: 0,
        height: 0,
        result,
        shapeless: true,
    });
}

impl RecipeManager {
    pub fn new() -> Self {
        let mut recipes = Vec::new();

        // 1. Logs -> Planks
        add_shaped(
            &mut recipes,
            vec!["L"],
            &[("L", Item::OakLog)],
            ItemStack::new(Item::OakPlanks, 4),
        );
        add_shaped(
            &mut recipes,
            vec!["L"],
            &[("L", Item::BirchLog)],
            ItemStack::new(Item::BirchPlanks, 4),
        );
        add_shaped(
            &mut recipes,
            vec!["L"],
            &[("L", Item::SpruceLog)],
            ItemStack::new(Item::SprucePlanks, 4),
        );
        // 2. Sticks
        add_shaped(
            &mut recipes,
            vec!["P", "P"],
            &[("P", Item::OakPlanks)],
            ItemStack::new(Item::Stick, 4),
        );
        add_shaped(
            &mut recipes,
            vec!["P", "P"],
            &[("P", Item::BirchPlanks)],
            ItemStack::new(Item::Stick, 4),
        );
        add_shaped(
            &mut recipes,
            vec!["P", "P"],
            &[("P", Item::SprucePlanks)],
            ItemStack::new(Item::Stick, 4),
        );
        // 3. Crafting Table
        add_shaped(
            &mut recipes,
            vec!["PP", "PP"],
            &[("P", Item::OakPlanks)],
            ItemStack::new(Item::CraftingTable, 1),
        );
        add_shaped(
            &mut recipes,
            vec!["PP", "PP"],
            &[("P", Item::BirchPlanks)],
            ItemStack::new(Item::CraftingTable, 1),
        );
        add_shaped(
            &mut recipes,
            vec!["PP", "PP"],
            &[("P", Item::SprucePlanks)],
            ItemStack::new(Item::CraftingTable, 1),
        );
        // 4. Chest
        add_shaped(
            &mut recipes,
            vec!["PPP", "P P", "PPP"],
            &[("P", Item::OakPlanks)],
            ItemStack::new(Item::Chest, 1),
        );
        add_shaped(
            &mut recipes,
            vec!["PPP", "P P", "PPP"],
            &[("P", Item::BirchPlanks)],
            ItemStack::new(Item::Chest, 1),
        );
        add_shaped(
            &mut recipes,
            vec!["PPP", "P P", "PPP"],
            &[("P", Item::SprucePlanks)],
            ItemStack::new(Item::Chest, 1),
        );
        // 5. Furnace
        add_shaped(
            &mut recipes,
            vec!["CCC", "C C", "CCC"],
            &[("C", Item::Cobblestone)],
            ItemStack::new(Item::Furnace, 1),
        );
        // 6. Torch
        add_shaped(
            &mut recipes,
            vec!["C", "S"],
            &[("C", Item::Coal), ("S", Item::Stick)],
            ItemStack::new(Item::Torch, 4),
        );

        // Ore Smelting Conversion
        add_shapeless(
            &mut recipes,
            vec![Item::IronOre],
            ItemStack::new(Item::IronIngot, 1),
        );
        add_shapeless(
            &mut recipes,
            vec![Item::GoldOre],
            ItemStack::new(Item::GoldIngot, 1),
        );

        // 7. Stone Tools
        add_shaped(
            &mut recipes,
            vec!["SSS", " t ", " t "],
            &[("S", Item::Cobblestone), ("t", Item::Stick)],
            ItemStack::new(Item::StonePickaxe, 1),
        );
        add_shaped(
            &mut recipes,
            vec!["SS ", "St ", " t "],
            &[("S", Item::Cobblestone), ("t", Item::Stick)],
            ItemStack::new(Item::StoneAxe, 1),
        );
        add_shaped(
            &mut recipes,
            vec!["S", "t", "t"],
            &[("S", Item::Cobblestone), ("t", Item::Stick)],
            ItemStack::new(Item::StoneShovel, 1),
        );
        add_shaped(
            &mut recipes,
            vec!["S", "S", "t"],
            &[("S", Item::Cobblestone), ("t", Item::Stick)],
            ItemStack::new(Item::StoneSword, 1),
        );

        // 8. Iron Tools
        add_shaped(
            &mut recipes,
            vec!["III", " t ", " t "],
            &[("I", Item::IronIngot), ("t", Item::Stick)],
            ItemStack::new(Item::IronPickaxe, 1),
        );
        add_shaped(
            &mut recipes,
            vec!["II ", "It ", " t "],
            &[("I", Item::IronIngot), ("t", Item::Stick)],
            ItemStack::new(Item::IronAxe, 1),
        );
        add_shaped(
            &mut recipes,
            vec!["I", "t", "t"],
            &[("I", Item::IronIngot), ("t", Item::Stick)],
            ItemStack::new(Item::IronShovel, 1),
        );
        add_shaped(
            &mut recipes,
            vec!["I", "I", "t"],
            &[("I", Item::IronIngot), ("t", Item::Stick)],
            ItemStack::new(Item::IronSword, 1),
        );

        // 9. Diamond Tools
        add_shaped(
            &mut recipes,
            vec!["DDD", " t ", " t "],
            &[("D", Item::Diamond), ("t", Item::Stick)],
            ItemStack::new(Item::DiamondPickaxe, 1),
        );
        add_shaped(
            &mut recipes,
            vec!["DD ", "Dt ", " t "],
            &[("D", Item::Diamond), ("t", Item::Stick)],
            ItemStack::new(Item::DiamondAxe, 1),
        );
        add_shaped(
            &mut recipes,
            vec!["D", "t", "t"],
            &[("D", Item::Diamond), ("t", Item::Stick)],
            ItemStack::new(Item::DiamondShovel, 1),
        );
        add_shaped(
            &mut recipes,
            vec!["D", "D", "t"],
            &[("D", Item::Diamond), ("t", Item::Stick)],
            ItemStack::new(Item::DiamondSword, 1),
        );

        // 10. Block Conversions
        add_shaped(
            &mut recipes,
            vec!["SS", "SS"],
            &[("S", Item::Stone)],
            ItemStack::new(Item::StoneBrick, 4),
        );
        add_shaped(
            &mut recipes,
            vec!["CC", "CC"],
            &[("C", Item::Clay)],
            ItemStack::new(Item::Brick, 4),
        );
        add_shaped(
            &mut recipes,
            vec!["SS", "SS"],
            &[("S", Item::Sand)],
            ItemStack::new(Item::Sandstone, 4),
        );
        add_shaped(
            &mut recipes,
            vec!["SS", "SS"],
            &[("S", Item::Snow)],
            ItemStack::new(Item::Snow, 1),
        );
        // TNT (Redstone + Sand)
        add_shaped(
            &mut recipes,
            vec!["RSR", "SRS", "RSR"],
            &[("R", Item::Redstone), ("S", Item::Sand)],
            ItemStack::new(Item::TNT, 1),
        );

        // Bread (3 Apples horizontal)
        add_shaped(
            &mut recipes,
            vec!["AAA"],
            &[("A", Item::Apple)],
            ItemStack::new(Item::Bread, 1),
        );

        // Enchanting / brewing / anvil workstations.
        add_shaped(
            &mut recipes,
            vec![" B ", "D D", "OOO"],
            &[
                ("B", Item::Bookshelf),
                ("D", Item::Diamond),
                ("O", Item::Obsidian),
            ],
            ItemStack::new(Item::EnchantingTable, 1),
        );
        add_shaped(
            &mut recipes,
            vec![" B ", "CCC"],
            &[("B", Item::BlazePowder), ("C", Item::Cobblestone)],
            ItemStack::new(Item::BrewingStand, 1),
        );
        add_shaped(
            &mut recipes,
            vec!["III", " I ", "III"],
            &[("I", Item::IronIngot)],
            ItemStack::new(Item::Anvil, 1),
        );
        add_shaped(
            &mut recipes,
            vec!["G G", " G "],
            &[("G", Item::Glass)],
            ItemStack::new(Item::GlassBottle, 3),
        );
        add_shaped(
            &mut recipes,
            vec!["III", "I I"],
            &[("I", Item::IronIngot)],
            ItemStack::new(Item::IronHelmet, 1),
        );
        add_shaped(
            &mut recipes,
            vec!["I I", "III", "III"],
            &[("I", Item::IronIngot)],
            ItemStack::new(Item::IronChestplate, 1),
        );
        add_shaped(
            &mut recipes,
            vec!["III", "I I", "I I"],
            &[("I", Item::IronIngot)],
            ItemStack::new(Item::IronLeggings, 1),
        );
        add_shaped(
            &mut recipes,
            vec!["I I", "I I"],
            &[("I", Item::IronIngot)],
            ItemStack::new(Item::IronBoots, 1),
        );
        add_shaped(
            &mut recipes,
            vec!["G", "S", "F"],
            &[
                ("G", Item::Gravel),
                ("S", Item::Stick),
                ("F", Item::Feather),
            ],
            ItemStack::new(Item::Arrow, 4),
        );

        // Substitute recipes keep brewing ingredients obtainable before Nether exists.
        add_shapeless(
            &mut recipes,
            vec![Item::Seeds, Item::RottenFlesh],
            ItemStack::new(Item::NetherWart, 1),
        );
        add_shapeless(
            &mut recipes,
            vec![Item::Coal, Item::Gunpowder],
            ItemStack::new(Item::BlazePowder, 1),
        );
        add_shapeless(
            &mut recipes,
            vec![Item::SugarCane],
            ItemStack::new(Item::Sugar, 1),
        );
        add_shapeless(
            &mut recipes,
            vec![Item::Melon, Item::GoldIngot],
            ItemStack::new(Item::GlisteringMelon, 1),
        );
        add_shapeless(
            &mut recipes,
            vec![Item::Apple, Item::GoldIngot],
            ItemStack::new(Item::GoldenCarrot, 1),
        );
        add_shapeless(
            &mut recipes,
            vec![Item::SpiderEye, Item::Sugar, Item::RottenFlesh],
            ItemStack::new(Item::FermentedSpiderEye, 1),
        );
        add_shapeless(
            &mut recipes,
            vec![Item::Gunpowder, Item::BlazePowder],
            ItemStack::new(Item::MagmaCream, 1),
        );
        add_shapeless(
            &mut recipes,
            vec![Item::Redstone],
            ItemStack::new(Item::RedstoneDust, 1),
        );
        add_shapeless(
            &mut recipes,
            vec![Item::Torch, Item::Gunpowder],
            ItemStack::new(Item::GlowstoneDust, 1),
        );
        add_shapeless(
            &mut recipes,
            vec![Item::Diamond, Item::RottenFlesh],
            ItemStack::new(Item::GhastTear, 1),
        );
        add_shapeless(
            &mut recipes,
            vec![Item::RawChicken, Item::Water],
            ItemStack::new(Item::Pufferfish, 1),
        );
        add_shapeless(
            &mut recipes,
            vec![Item::RottenFlesh, Item::Bone],
            ItemStack::new(Item::SpiderEye, 1),
        );

        // Redstone components. A few substitutions keep every component
        // craftable with the clone's currently obtainable resource set.
        add_shapeless(
            &mut recipes,
            vec![Item::RedstoneDust],
            ItemStack::new(Item::RedstoneWire, 1),
        );
        add_shaped(
            &mut recipes,
            vec!["R", "S"],
            &[("R", Item::RedstoneDust), ("S", Item::Stick)],
            ItemStack::new(Item::RedstoneTorch, 1),
        );
        add_shaped(
            &mut recipes,
            vec!["TRT", "SSS"],
            &[
                ("T", Item::RedstoneTorch),
                ("R", Item::RedstoneDust),
                ("S", Item::Stone),
            ],
            ItemStack::new(Item::Repeater, 1),
        );
        add_shaped(
            &mut recipes,
            vec![" T ", "TRT", "SSS"],
            &[
                ("T", Item::RedstoneTorch),
                ("R", Item::RedstoneDust),
                ("S", Item::Stone),
            ],
            ItemStack::new(Item::Comparator, 1),
        );
        add_shapeless(
            &mut recipes,
            vec![Item::Stone],
            ItemStack::new(Item::StoneButton, 1),
        );
        add_shaped(
            &mut recipes,
            vec!["S", "C"],
            &[("S", Item::Stick), ("C", Item::Cobblestone)],
            ItemStack::new(Item::Lever, 1),
        );
        add_shaped(
            &mut recipes,
            vec!["SS"],
            &[("S", Item::Stone)],
            ItemStack::new(Item::PressurePlate, 1),
        );
        add_shaped(
            &mut recipes,
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
            &mut recipes,
            vec![Item::Piston, Item::SugarCane],
            ItemStack::new(Item::StickyPiston, 1),
        );
        add_shaped(
            &mut recipes,
            vec![" R ", "RGR", " R "],
            &[("R", Item::RedstoneDust), ("G", Item::GlowstoneDust)],
            ItemStack::new(Item::RedstoneLamp, 1),
        );
        add_shaped(
            &mut recipes,
            vec!["PP", "PP", "PP"],
            &[("P", Item::OakPlanks)],
            ItemStack::new(Item::OakDoor, 3),
        );
        add_shaped(
            &mut recipes,
            vec!["PPP", "PPP"],
            &[("P", Item::OakPlanks)],
            ItemStack::new(Item::OakTrapdoor, 2),
        );
        add_shaped(
            &mut recipes,
            vec!["CCC", "CBC", "CRC"],
            &[
                ("C", Item::Cobblestone),
                ("B", Item::Bow),
                ("R", Item::RedstoneDust),
            ],
            ItemStack::new(Item::Dispenser, 1),
        );
        add_shaped(
            &mut recipes,
            vec!["CCC", "C C", "CRC"],
            &[("C", Item::Cobblestone), ("R", Item::RedstoneDust)],
            ItemStack::new(Item::Dropper, 1),
        );
        add_shaped(
            &mut recipes,
            vec!["PPP", "PRP", "PPP"],
            &[("P", Item::OakPlanks), ("R", Item::RedstoneDust)],
            ItemStack::new(Item::NoteBlock, 1),
        );

        Self { recipes }
    }

    pub fn match_recipe(&self, grid: &[Option<ItemStack>], grid_size: usize) -> Option<ItemStack> {
        // 1. Check for Shapeless match first
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

        for recipe in &self.recipes {
            if recipe.shapeless {
                if recipe.pattern[0] == active_items {
                    return Some(recipe.result);
                }
            }
        }

        // 2. Shaped Match: Find bounding box of input grid
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

        // Crop the input grid pattern
        let mut cropped = vec![vec![Item::Air; w_size]; h_size];
        for r in 0..h_size {
            for c in 0..w_size {
                if let Some(stack) = grid[(min_r + r) * grid_size + (min_c + c)] {
                    cropped[r][c] = stack.item;
                }
            }
        }

        // Match against shaped recipes
        for recipe in &self.recipes {
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

    #[test]
    fn test_crafting_planks() {
        let manager = RecipeManager::new();
        let mut grid = vec![None; 4];
        grid[0] = Some(ItemStack::new(Item::OakLog, 1));
        let res = manager.match_recipe(&grid, 2);
        assert!(res.is_some());
        assert_eq!(res.unwrap().item, Item::OakPlanks);
        assert_eq!(res.unwrap().count, 4);
    }

    #[test]
    fn test_crafting_sticks() {
        let manager = RecipeManager::new();
        let mut grid = vec![None; 4];
        grid[0] = Some(ItemStack::new(Item::OakPlanks, 1));
        grid[2] = Some(ItemStack::new(Item::OakPlanks, 1));
        let res = manager.match_recipe(&grid, 2);
        assert!(res.is_some());
        assert_eq!(res.unwrap().item, Item::Stick);
        assert_eq!(res.unwrap().count, 4);
    }

    #[test]
    fn test_crafting_redstone_wire_from_dust() {
        let manager = RecipeManager::new();
        let mut grid = vec![None; 4];
        grid[3] = Some(ItemStack::new(Item::RedstoneDust, 1));
        let result = manager.match_recipe(&grid, 2).unwrap();
        assert_eq!(result.item, Item::RedstoneWire);
    }
}
