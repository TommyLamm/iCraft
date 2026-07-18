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

impl RecipeManager {
    pub fn new() -> Self {
        let mut recipes = Vec::new();

        // Helper to add shaped recipe
        let mut add_shaped = |pat: Vec<&str>, mapping: &[(&str, Item)], result: ItemStack| {
            let height = pat.len();
            let width = pat[0].len();
            let mut pattern = vec![vec![Item::Air; width]; height];
            for r in 0..height {
                let chars: Vec<char> = pat[r].chars().collect();
                for c in 0..width {
                    let ch = chars[c].to_string();
                    if ch != " " {
                        let item = mapping.iter().find(|(s, _)| s == &ch).map(|(_, it)| *it).unwrap_or(Item::Air);
                        pattern[r][c] = item;
                    }
                }
            }
            recipes.push(Recipe { pattern, width, height, result, shapeless: false });
        };

        // Helper for shapeless recipe
        let mut add_shapeless = |ingredients: Vec<Item>, result: ItemStack| {
            // Store sorted list in pattern[0]
            let mut sorted = ingredients;
            sorted.sort_by_key(|&it| it as i32);
            recipes.push(Recipe {
                pattern: vec![sorted],
                width: 0,
                height: 0,
                result,
                shapeless: true,
            });
        };

        // 1. Logs -> Planks
        add_shaped(vec!["L"], &[("L", Item::OakLog)], ItemStack { item: Item::OakPlanks, count: 4 });
        // 2. Sticks
        add_shaped(vec!["P", "P"], &[("P", Item::OakPlanks)], ItemStack { item: Item::Stick, count: 4 });
        // 3. Crafting Table
        add_shaped(vec!["PP", "PP"], &[("P", Item::OakPlanks)], ItemStack { item: Item::CraftingTable, count: 1 });
        // 4. Chest
        add_shaped(vec!["PPP", "P P", "PPP"], &[("P", Item::OakPlanks)], ItemStack { item: Item::Chest, count: 1 });
        // 5. Furnace
        add_shaped(vec!["CCC", "C C", "CCC"], &[("C", Item::Cobblestone)], ItemStack { item: Item::Furnace, count: 1 });
        // 6. Torch
        add_shaped(vec!["C", "S"], &[("C", Item::Coal), ("S", Item::Stick)], ItemStack { item: Item::Torch, count: 4 });
        
        // Ore Smelting Conversion
        add_shapeless(vec![Item::IronOre], ItemStack { item: Item::IronIngot, count: 1 });
        add_shapeless(vec![Item::GoldOre], ItemStack { item: Item::GoldIngot, count: 1 });

        // 7. Stone Tools
        add_shaped(vec!["SSS", " t ", " t "], &[("S", Item::Cobblestone), ("t", Item::Stick)], ItemStack { item: Item::StonePickaxe, count: 1 });
        add_shaped(vec!["SS ", "St ", " t "], &[("S", Item::Cobblestone), ("t", Item::Stick)], ItemStack { item: Item::StoneAxe, count: 1 });
        add_shaped(vec!["S", "t", "t"], &[("S", Item::Cobblestone), ("t", Item::Stick)], ItemStack { item: Item::StoneShovel, count: 1 });
        add_shaped(vec!["S", "S", "t"], &[("S", Item::Cobblestone), ("t", Item::Stick)], ItemStack { item: Item::StoneSword, count: 1 });

        // 8. Iron Tools
        add_shaped(vec!["III", " t ", " t "], &[("I", Item::IronIngot), ("t", Item::Stick)], ItemStack { item: Item::IronPickaxe, count: 1 });
        add_shaped(vec!["II ", "It ", " t "], &[("I", Item::IronIngot), ("t", Item::Stick)], ItemStack { item: Item::IronAxe, count: 1 });
        add_shaped(vec!["I", "t", "t"], &[("I", Item::IronIngot), ("t", Item::Stick)], ItemStack { item: Item::IronShovel, count: 1 });
        add_shaped(vec!["I", "I", "t"], &[("I", Item::IronIngot), ("t", Item::Stick)], ItemStack { item: Item::IronSword, count: 1 });

        // 9. Diamond Tools
        add_shaped(vec!["DDD", " t ", " t "], &[("D", Item::Diamond), ("t", Item::Stick)], ItemStack { item: Item::DiamondPickaxe, count: 1 });
        add_shaped(vec!["DD ", "Dt ", " t "], &[("D", Item::Diamond), ("t", Item::Stick)], ItemStack { item: Item::DiamondAxe, count: 1 });
        add_shaped(vec!["D", "t", "t"], &[("D", Item::Diamond), ("t", Item::Stick)], ItemStack { item: Item::DiamondShovel, count: 1 });
        add_shaped(vec!["D", "D", "t"], &[("D", Item::Diamond), ("t", Item::Stick)], ItemStack { item: Item::DiamondSword, count: 1 });

        // 10. Block Conversions
        add_shaped(vec!["SS", "SS"], &[("S", Item::Stone)], ItemStack { item: Item::StoneBrick, count: 4 });
        add_shaped(vec!["CC", "CC"], &[("C", Item::Clay)], ItemStack { item: Item::Brick, count: 4 });
        add_shaped(vec!["SS", "SS"], &[("S", Item::Sand)], ItemStack { item: Item::Sandstone, count: 4 });
        add_shaped(vec!["SS", "SS"], &[("S", Item::Snow)], ItemStack { item: Item::Snow, count: 1 });
        // TNT (Redstone + Sand)
        add_shaped(vec!["RSR", "SRS", "RSR"], &[("R", Item::Redstone), ("S", Item::Sand)], ItemStack { item: Item::TNT, count: 1 });

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
        if active_items.is_empty() { return None; }
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
                        if r < min_r { min_r = r; }
                        if r > max_r { max_r = r; }
                        if c < min_c { min_c = c; }
                        if c > max_c { max_c = c; }
                    }
                }
            }
        }

        if !has_items { return None; }

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
            if recipe.shapeless { continue; }
            if recipe.width == w_size && recipe.height == h_size {
                let mut match_ok = true;
                for r in 0..h_size {
                    for c in 0..w_size {
                        if recipe.pattern[r][c] != cropped[r][c] {
                            match_ok = false;
                            break;
                        }
                    }
                    if !match_ok { break; }
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
        grid[0] = Some(ItemStack { item: Item::OakLog, count: 1 });
        let res = manager.match_recipe(&grid, 2);
        assert!(res.is_some());
        assert_eq!(res.unwrap().item, Item::OakPlanks);
        assert_eq!(res.unwrap().count, 4);
    }

    #[test]
    fn test_crafting_sticks() {
        let manager = RecipeManager::new();
        let mut grid = vec![None; 4];
        grid[0] = Some(ItemStack { item: Item::OakPlanks, count: 1 });
        grid[2] = Some(ItemStack { item: Item::OakPlanks, count: 1 });
        let res = manager.match_recipe(&grid, 2);
        assert!(res.is_some());
        assert_eq!(res.unwrap().item, Item::Stick);
        assert_eq!(res.unwrap().count, 4);
    }
}
