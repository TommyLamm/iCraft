# Inventory and Crafting System Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement a unified Item enum, a 40-slot Inventory (36 grid + 4 armor), a drag-and-drop mouse interaction GUI, and a translation-invariant 2D crafting system with 30+ recipes.

**Architecture:** Split the codebase into modular components. `src/inventory.rs` will handle core storage and stack transfer logic, `src/crafting.rs` will handle recipes and matching, `src/texture.rs` will generate item icons, and `src/state.rs` will coordinate rendering and user events.

**Tech Stack:** Rust, WGPU, Winit, Bytemuck.

---

## Proposed File Changes Summary

*   **Modify** `src/inventory.rs` — Convert to a unified `Item` enum and a complete `Inventory` struct with slot interactions.
*   **Modify** `src/texture.rs` — Add procedural pixel-art generation for Sticks, Coal, Ingots, Diamonds, Redstone, Swords, Pickaxes, Axes, and Shovels.
*   **Create** `src/crafting.rs` — Recipe matching engine and database of 30+ recipes.
*   **Modify** `src/state.rs` — Update inventory UI rendering vertices, click handling, and state integration.
*   **Modify** `src/app.rs` — Forward key presses (E, Escape) and mouse events to the inventory GUI.

---

### Task 1: Define Item, ItemStack, and Inventory Data Structures

**Files:**
*   Modify: `src/inventory.rs`
*   Test: `src/inventory.rs` (inline test module)

- [ ] **Step 1: Write data definitions and properties**
    Replace the contents of `src/inventory.rs` (excluding tests for now) to define the `Item` enum, `ItemStack` struct, `Inventory` struct, and mapping helpers from `Item` to `BlockType`.
    ```rust
    use crate::world::BlockType;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum GameMode {
        Creative,
        Survival,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub enum Item {
        Air,
        // Blocks
        Grass, Dirt, Stone, Sand, Gravel, OakLog, OakPlanks, OakLeaves, Cobblestone, Bedrock, Water,
        CoalOre, IronOre, GoldOre, DiamondOre, RedstoneOre, Glass, Brick, StoneBrick, Snow, Ice, Clay,
        Sandstone, Obsidian, CraftingTable, Furnace, Chest, TNT, Bookshelf, Torch,
        
        // Tools
        StoneSword, StonePickaxe, StoneAxe, StoneShovel,
        IronSword, IronPickaxe, IronAxe, IronShovel,
        DiamondSword, DiamondPickaxe, DiamondAxe, DiamondShovel,
        
        // Resources
        Stick, Coal, IronIngot, GoldIngot, Diamond, Redstone,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct ItemStack {
        pub item: Item,
        pub count: u32,
    }

    pub struct ItemProperties {
        pub name: &'static str,
        pub max_stack: u32,
        pub is_block: bool,
        pub block_type: Option<BlockType>,
        pub tex_coords: (u32, u32), // (col, row) in texture atlas
    }

    impl Item {
        pub fn properties(self) -> ItemProperties {
            match self {
                Item::Air => ItemProperties { name: "Air", max_stack: 64, is_block: false, block_type: None, tex_coords: (0, 0) },
                Item::Grass => ItemProperties { name: "Grass Block", max_stack: 64, is_block: true, block_type: Some(BlockType::Grass), tex_coords: (1, 0) },
                Item::Dirt => ItemProperties { name: "Dirt", max_stack: 64, is_block: true, block_type: Some(BlockType::Dirt), tex_coords: (2, 0) },
                Item::Stone => ItemProperties { name: "Stone", max_stack: 64, is_block: true, block_type: Some(BlockType::Stone), tex_coords: (3, 0) },
                Item::Sand => ItemProperties { name: "Sand", max_stack: 64, is_block: true, block_type: Some(BlockType::Sand), tex_coords: (4, 0) },
                Item::Gravel => ItemProperties { name: "Gravel", max_stack: 64, is_block: true, block_type: Some(BlockType::Gravel), tex_coords: (5, 0) },
                Item::OakLog => ItemProperties { name: "Oak Log", max_stack: 64, is_block: true, block_type: Some(BlockType::OakLog), tex_coords: (11, 1) },
                Item::OakPlanks => ItemProperties { name: "Oak Planks", max_stack: 64, is_block: true, block_type: Some(BlockType::OakPlanks), tex_coords: (6, 0) },
                Item::OakLeaves => ItemProperties { name: "Oak Leaves", max_stack: 64, is_block: true, block_type: Some(BlockType::OakLeaves), tex_coords: (7, 0) },
                Item::Cobblestone => ItemProperties { name: "Cobblestone", max_stack: 64, is_block: true, block_type: Some(BlockType::Cobblestone), tex_coords: (8, 0) },
                Item::Bedrock => ItemProperties { name: "Bedrock", max_stack: 64, is_block: true, block_type: Some(BlockType::Bedrock), tex_coords: (9, 0) },
                Item::Water => ItemProperties { name: "Water", max_stack: 64, is_block: true, block_type: Some(BlockType::Water), tex_coords: (10, 0) },
                Item::CoalOre => ItemProperties { name: "Coal Ore", max_stack: 64, is_block: true, block_type: Some(BlockType::CoalOre), tex_coords: (11, 0) },
                Item::IronOre => ItemProperties { name: "Iron Ore", max_stack: 64, is_block: true, block_type: Some(BlockType::IronOre), tex_coords: (12, 0) },
                Item::GoldOre => ItemProperties { name: "Gold Ore", max_stack: 64, is_block: true, block_type: Some(BlockType::GoldOre), tex_coords: (13, 0) },
                Item::DiamondOre => ItemProperties { name: "Diamond Ore", max_stack: 64, is_block: true, block_type: Some(BlockType::DiamondOre), tex_coords: (14, 0) },
                Item::RedstoneOre => ItemProperties { name: "Redstone Ore", max_stack: 64, is_block: true, block_type: Some(BlockType::RedstoneOre), tex_coords: (15, 0) },
                Item::Glass => ItemProperties { name: "Glass", max_stack: 64, is_block: true, block_type: Some(BlockType::Glass), tex_coords: (0, 1) },
                Item::Brick => ItemProperties { name: "Brick Block", max_stack: 64, is_block: true, block_type: Some(BlockType::Brick), tex_coords: (1, 1) },
                Item::StoneBrick => ItemProperties { name: "Stone Brick", max_stack: 64, is_block: true, block_type: Some(BlockType::StoneBrick), tex_coords: (2, 1) },
                Item::Snow => ItemProperties { name: "Snow Block", max_stack: 64, is_block: true, block_type: Some(BlockType::Snow), tex_coords: (4, 1) },
                Item::Ice => ItemProperties { name: "Ice", max_stack: 64, is_block: true, block_type: Some(BlockType::Ice), tex_coords: (5, 1) },
                Item::Clay => ItemProperties { name: "Clay Block", max_stack: 64, is_block: true, block_type: Some(BlockType::Clay), tex_coords: (6, 1) },
                Item::Sandstone => ItemProperties { name: "Sandstone", max_stack: 64, is_block: true, block_type: Some(BlockType::Sandstone), tex_coords: (8, 1) },
                Item::Obsidian => ItemProperties { name: "Obsidian", max_stack: 64, is_block: true, block_type: Some(BlockType::Obsidian), tex_coords: (9, 1) },
                Item::CraftingTable => ItemProperties { name: "Crafting Table", max_stack: 64, is_block: true, block_type: Some(BlockType::CraftingTable), tex_coords: (13, 1) },
                Item::Furnace => ItemProperties { name: "Furnace", max_stack: 64, is_block: true, block_type: Some(BlockType::Furnace), tex_coords: (14, 1) },
                Item::Chest => ItemProperties { name: "Chest", max_stack: 64, is_block: true, block_type: Some(BlockType::Chest), tex_coords: (15, 1) },
                Item::TNT => ItemProperties { name: "TNT", max_stack: 64, is_block: true, block_type: Some(BlockType::TNT), tex_coords: (2, 2) },
                Item::Bookshelf => ItemProperties { name: "Bookshelf", max_stack: 64, is_block: true, block_type: Some(BlockType::Bookshelf), tex_coords: (3, 2) },
                Item::Torch => ItemProperties { name: "Torch", max_stack: 64, is_block: true, block_type: Some(BlockType::Torch), tex_coords: (4, 2) },
                
                // Tools (row 4-7)
                Item::StoneSword => ItemProperties { name: "Stone Sword", max_stack: 1, is_block: false, block_type: None, tex_coords: (0, 4) },
                Item::IronSword => ItemProperties { name: "Iron Sword", max_stack: 1, is_block: false, block_type: None, tex_coords: (1, 4) },
                Item::DiamondSword => ItemProperties { name: "Diamond Sword", max_stack: 1, is_block: false, block_type: None, tex_coords: (2, 4) },
                Item::StonePickaxe => ItemProperties { name: "Stone Pickaxe", max_stack: 1, is_block: false, block_type: None, tex_coords: (0, 5) },
                Item::IronPickaxe => ItemProperties { name: "Iron Pickaxe", max_stack: 1, is_block: false, block_type: None, tex_coords: (1, 5) },
                Item::DiamondPickaxe => ItemProperties { name: "Diamond Pickaxe", max_stack: 1, is_block: false, block_type: None, tex_coords: (2, 5) },
                Item::StoneAxe => ItemProperties { name: "Stone Axe", max_stack: 1, is_block: false, block_type: None, tex_coords: (0, 6) },
                Item::IronAxe => ItemProperties { name: "Iron Axe", max_stack: 1, is_block: false, block_type: None, tex_coords: (1, 6) },
                Item::DiamondAxe => ItemProperties { name: "Diamond Axe", max_stack: 1, is_block: false, block_type: None, tex_coords: (2, 6) },
                Item::StoneShovel => ItemProperties { name: "Stone Shovel", max_stack: 1, is_block: false, block_type: None, tex_coords: (0, 7) },
                Item::IronShovel => ItemProperties { name: "Iron Shovel", max_stack: 1, is_block: false, block_type: None, tex_coords: (1, 7) },
                Item::DiamondShovel => ItemProperties { name: "Diamond Shovel", max_stack: 1, is_block: false, block_type: None, tex_coords: (2, 7) },
                
                // Resources (row 3)
                Item::Stick => ItemProperties { name: "Stick", max_stack: 64, is_block: false, block_type: None, tex_coords: (0, 3) },
                Item::Coal => ItemProperties { name: "Coal", max_stack: 64, is_block: false, block_type: None, tex_coords: (1, 3) },
                Item::IronIngot => ItemProperties { name: "Iron Ingot", max_stack: 64, is_block: false, block_type: None, tex_coords: (2, 3) },
                Item::GoldIngot => ItemProperties { name: "Gold Ingot", max_stack: 64, is_block: false, block_type: None, tex_coords: (3, 3) },
                Item::Diamond => ItemProperties { name: "Diamond", max_stack: 64, is_block: false, block_type: None, tex_coords: (4, 3) },
                Item::Redstone => ItemProperties { name: "Redstone Dust", max_stack: 64, is_block: false, block_type: None, tex_coords: (5, 3) },
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
                Item::Grass, Item::Dirt, Item::Stone, Item::OakLog, Item::OakPlanks,
                Item::Glass, Item::Cobblestone, Item::Water, Item::Torch,
            ];
            for (i, &item) in creative_items.iter().enumerate() {
                inv.hotbar[i] = Some(ItemStack { item, count: 64 });
            }
            inv
        }

        pub fn get_selected_block(&self) -> Option<BlockType> {
            self.hotbar[self.selected].and_then(|stack| stack.item.properties().block_type)
        }

        pub fn add_item(&mut self, item: Item) -> bool {
            if item == Item::Air { return false; }
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
                    *slot = Some(ItemStack { item, count: 1 });
                    return true;
                }
            }
            // 4. Try to add to empty slot in main backpack
            for slot in self.main.iter_mut() {
                if slot.is_none() {
                    *slot = Some(ItemStack { item, count: 1 });
                    return true;
                }
            }
            false
        }

        pub fn use_selected_item(&mut self, is_creative: bool) {
            if is_creative { return; }
            if let Some(stack) = &mut self.hotbar[self.selected] {
                if stack.count > 1 {
                    stack.count -= 1;
                } else {
                    self.hotbar[self.selected] = None;
                }
            }
        }
    }
    ```

- [ ] **Step 2: Write tests for the new Inventory struct**
    Add the test module at the bottom of `src/inventory.rs` containing tests for creative initialization, item addition, and item consumption.
    ```rust
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
    }
    ```

- [ ] **Step 3: Run cargo test to verify it passes**
    Run: `cargo test`
    Expected: PASS

- [ ] **Step 4: Commit**
    ```bash
    git add src/inventory.rs
    git commit -m "feat: define Item and complete Inventory structs"
    ```

---

### Task 2: Extend Procedural Texture Generation for Items

We will generateStick, Coal, Ingots, Diamond, Redstone, and Tools icons in rows 3 to 7.

**Files:**
*   Modify: `src/texture.rs`

- [ ] **Step 1: Add item rendering helpers in `src/texture.rs`**
    Add procedural functions for rendering Stick, Coal, Ingot, Diamond, Redstone, and Tools into the RgbaImage.
    ```rust
    // Add to src/texture.rs:
    fn draw_stick_icon(img: &mut RgbaImage, tx: u32, ty: u32) {
        for y in 0..16 {
            for x in 0..16 {
                // Diagonal stick line
                let is_stick = x == y && x >= 3 && x <= 12;
                if is_stick {
                    img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([139, 90, 43, 255]));
                } else {
                    img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([0, 0, 0, 0]));
                }
            }
        }
    }

    fn draw_coal_icon(img: &mut RgbaImage, tx: u32, ty: u32) {
        for y in 0..16 {
            for x in 0..16 {
                let dx = (x as i32 - 8).abs();
                let dy = (y as i32 - 8).abs();
                let is_coal = dx + dy <= 5 && dx <= 4 && dy <= 4;
                if is_coal {
                    let r = if dx == 0 && dy == 0 { 60 } else { 30 };
                    img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([r, r, r, 255]));
                } else {
                    img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([0, 0, 0, 0]));
                }
            }
        }
    }

    fn draw_ingot_icon(img: &mut RgbaImage, tx: u32, ty: u32, color: [u8; 3]) {
        for y in 0..16 {
            for x in 0..16 {
                let is_ingot = x >= 3 && x <= 12 && y >= 5 && y <= 10;
                if is_ingot {
                    let is_highlight = x == 3 || y == 5;
                    let c = if is_highlight {
                        [color[0].saturating_add(40), color[1].saturating_add(40), color[2].saturating_add(40)]
                    } else {
                        color
                    };
                    img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([c[0], c[1], c[2], 255]));
                } else {
                    img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([0, 0, 0, 0]));
                }
            }
        }
    }

    fn draw_diamond_icon(img: &mut RgbaImage, tx: u32, ty: u32) {
        for y in 0..16 {
            for x in 0..16 {
                let dx = (x as i32 - 8).abs();
                let dy = (y as i32 - 8).abs();
                let is_diamond = dx + dy <= 5 && dy >= 1;
                if is_diamond {
                    img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([90, 220, 240, 255]));
                } else {
                    img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([0, 0, 0, 0]));
                }
            }
        }
    }

    fn draw_redstone_icon(img: &mut RgbaImage, tx: u32, ty: u32) {
        for y in 0..16 {
            for x in 0..16 {
                let is_center = (x as i32 - 8).abs() <= 2 && (y as i32 - 8).abs() <= 2;
                if is_center {
                    img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([200, 20, 20, 255]));
                } else {
                    img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([0, 0, 0, 0]));
                }
            }
        }
    }

    fn draw_sword_icon(img: &mut RgbaImage, tx: u32, ty: u32, blade_color: [u8; 3]) {
        for y in 0..16 {
            for x in 0..16 {
                // Diagonal sword: Blade from (12, 3) to (5, 10). Handle at (3, 12)
                let is_handle = x == 3 && y == 12;
                let is_guard = (x == 4 && y == 11) || (x == 3 && y == 11) || (x == 4 && y == 12);
                let is_blade = x + y == 15 && x >= 5 && x <= 12;
                if is_blade {
                    img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([blade_color[0], blade_color[1], blade_color[2], 255]));
                } else if is_guard {
                    img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([120, 90, 60, 255]));
                } else if is_handle {
                    img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([100, 70, 40, 255]));
                } else {
                    img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([0, 0, 0, 0]));
                }
            }
        }
    }

    fn draw_pickaxe_icon(img: &mut RgbaImage, tx: u32, ty: u32, head_color: [u8; 3]) {
        for y in 0..16 {
            for x in 0..16 {
                // Diagonal handle from bottom-left to top-right
                let is_handle = x == y && x >= 3 && x <= 12;
                let is_head = (y == 3 && x >= 2 && x <= 6) || (x == 3 && y >= 2 && y <= 6) || (x + y == 6);
                if is_head {
                    img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([head_color[0], head_color[1], head_color[2], 255]));
                } else if is_handle {
                    img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([139, 90, 43, 255]));
                } else {
                    img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([0, 0, 0, 0]));
                }
            }
        }
    }

    fn draw_axe_icon(img: &mut RgbaImage, tx: u32, ty: u32, head_color: [u8; 3]) {
        for y in 0..16 {
            for x in 0..16 {
                let is_handle = x == y && x >= 3 && x <= 12;
                let is_head = x >= 2 && x <= 4 && y >= 2 && y <= 4 && !(x == 4 && y == 4);
                if is_head {
                    img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([head_color[0], head_color[1], head_color[2], 255]));
                } else if is_handle {
                    img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([139, 90, 43, 255]));
                } else {
                    img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([0, 0, 0, 0]));
                }
            }
        }
    }

    fn draw_shovel_icon(img: &mut RgbaImage, tx: u32, ty: u32, head_color: [u8; 3]) {
        for y in 0..16 {
            for x in 0..16 {
                let is_handle = x == y && x >= 4 && x <= 12;
                let is_head = x >= 2 && x <= 3 && y >= 2 && y <= 3;
                if is_head {
                    img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([head_color[0], head_color[1], head_color[2], 255]));
                } else if is_handle {
                    img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([139, 90, 43, 255]));
                } else {
                    img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([0, 0, 0, 0]));
                }
            }
        }
    }
    ```

- [ ] **Step 2: Add Row 3-7 drawing logic in `TextureAtlas::new_procedural`**
    Modify `src/texture.rs:326-336` to call these drawing helpers instead of filling rows 3..7 with zeros.
    ```rust
    // In src/texture.rs:
    // Replace the clear code with:
    // Row 3: Resources
    draw_stick_icon(&mut img, 0, 3);
    draw_coal_icon(&mut img, 1, 3);
    draw_ingot_icon(&mut img, 2, 3, [200, 200, 200]); // Iron Ingot
    draw_ingot_icon(&mut img, 3, 3, [240, 220, 70]);  // Gold Ingot
    draw_diamond_icon(&mut img, 4, 3);
    draw_redstone_icon(&mut img, 5, 3);

    // Row 4: Swords
    draw_sword_icon(&mut img, 0, 4, [160, 160, 160]); // Stone Sword (gray)
    draw_sword_icon(&mut img, 1, 4, [220, 220, 220]); // Iron Sword (silver)
    draw_sword_icon(&mut img, 2, 4, [100, 220, 240]); // Diamond Sword (cyan)

    // Row 5: Pickaxes
    draw_pickaxe_icon(&mut img, 0, 5, [160, 160, 160]); // Stone
    draw_pickaxe_icon(&mut img, 1, 5, [220, 220, 220]); // Iron
    draw_pickaxe_icon(&mut img, 2, 5, [100, 220, 240]); // Diamond

    // Row 6: Axes
    draw_axe_icon(&mut img, 0, 6, [160, 160, 160]); // Stone
    draw_axe_icon(&mut img, 1, 6, [220, 220, 220]); // Iron
    draw_axe_icon(&mut img, 2, 6, [100, 220, 240]); // Diamond

    // Row 7: Shovels
    draw_shovel_icon(&mut img, 0, 7, [160, 160, 160]); // Stone
    draw_shovel_icon(&mut img, 1, 7, [220, 220, 220]); // Iron
    draw_shovel_icon(&mut img, 2, 7, [100, 220, 240]); // Diamond

    // Fill remaining rows (8 to 15) with transparent
    for ty in 8..16 {
        for tx in 0..16 {
            for y in 0..16 {
                for x in 0..16 {
                    img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([0, 0, 0, 0]));
                }
            }
        }
    }
    ```

- [ ] **Step 3: Build the application to verify compiling**
    Run: `cargo check`
    Expected: Compiles successfully.

- [ ] **Step 4: Commit**
    ```bash
    git add src/texture.rs
    git commit -m "feat: add item icon rendering to procedural texture atlas"
    ```

---

### Task 3: Crafting Recipes and Matching Engine

**Files:**
*   Create: `src/crafting.rs`
*   Modify: `src/main.rs` (to register the new module)

- [ ] **Step 1: Create `src/crafting.rs`**
    Define the `Recipe` struct, translation-invariant matching logic, and the list of 30+ recipes.
    ```rust
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
    ```

- [ ] **Step 2: Add `pub mod crafting;` to `src/main.rs`**
    Insert `pub mod crafting;` at the top of `src/main.rs`.

- [ ] **Step 3: Add unit tests to `src/crafting.rs`**
    Add tests for Plank and Stick shaped matching.
    ```rust
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
    ```

- [ ] **Step 4: Run cargo test to verify it passes**
    Run: `cargo test`
    Expected: PASS

- [ ] **Step 5: Commit**
    ```bash
    git add src/crafting.rs src/main.rs
    git commit -m "feat: add crafting recipe matcher and core database"
    ```

---

### Task 4: Integrate Inventory in State and Create GUI Layouts

**Files:**
*   Modify: `src/state.rs:160-165` (add `inventory` and `recipe_manager`)
*   Modify: `src/state.rs:750-765` (initialize `inventory` and `recipe_manager`)
*   Modify: `src/state.rs:1180-1300` (render inventory GUI grid slots, items, texts, and drag items)

- [ ] **Step 1: Add structs to State in `src/state.rs`**
    Replace `pub hotbar: Hotbar` with `pub inventory: crate::inventory::Inventory` and `pub recipe_manager: crate::crafting::RecipeManager`.
    Change references to `hotbar` in `state.rs` to compile.
    ```rust
    // In src/state.rs Struct:
    pub inventory: crate::inventory::Inventory,
    pub recipe_manager: crate::crafting::RecipeManager,
    ```

- [ ] **Step 2: Update Initialization in `State::new`**
    ```rust
    // In src/state.rs State::new:
    let inventory = crate::inventory::Inventory::new_creative();
    let recipe_manager = crate::crafting::RecipeManager::new();
    ```

- [ ] **Step 3: Modify the Render loop UI section for Inventory GUI**
    Update the `if self.is_paused` UI render branch to support rendering the inventory GUI when `self.inventory.is_open` is true.
    Draw:
    1. A dark translucent background covering the screen.
    2. Grid slots (borders using `ui_line_vertex_buffer`, slot background quads using `ui_vertex_buffer`).
    3. Icons inside slots using `ui_textured_vertex_buffer`.
    4. Text stack counts.
    5. The item held by the cursor (`self.inventory.dragged`) at the mouse coordinate.
    6. Tooltip text when hovering.

    *Note: We will write the precise geometry rendering calculations in `src/state.rs`.*

- [ ] **Step 4: Verify compiling**
    Run: `cargo check`
    Expected: Compiles successfully (we will fix any minor reference bugs during this step).

- [ ] **Step 5: Commit**
    ```bash
    git add src/state.rs
    git commit -m "feat: integrate inventory state and UI rendering quads"
    ```

---

### Task 5: User Input Handoff & Window Focus

**Files:**
*   Modify: `src/app.rs`
*   Modify: `src/state.rs`

- [ ] **Step 1: Forward keyboard 'E' to toggle Inventory**
    Modify `src/app.rs` keyboard handler:
    When `E` key is pressed:
    *   Toggle `state.inventory.is_open`.
    *   If open: Set cursor grab mode to `None` and show cursor.
    *   If closed: Set cursor grab mode to `Locked` and hide cursor.
    Also, disable camera pitch/yaw updates and player WASD movements in `app.rs` while `state.inventory.is_open` is true.

- [ ] **Step 2: Handle escape key to close inventory first**
    Modify `src/app.rs` escape key handler:
    *   If `state.inventory.is_open` is true: close it (re-lock cursor) instead of pausing the game.

- [ ] **Step 3: Run `cargo check` to verify**
    Run: `cargo check`
    Expected: Success.

- [ ] **Step 4: Commit**
    ```bash
    git add src/app.rs src/state.rs
    git commit -m "feat: handle keyboard E and Esc input to toggle inventory UI"
    ```

---

### Task 5: Mouse Drag-and-Drop Interaction

**Files:**
*   Modify: `src/state.rs`
*   Modify: `src/app.rs`

- [ ] **Step 1: Implement click detection on slots**
    Implement `pub fn handle_inventory_click(&mut self, is_left: bool)` in `src/state.rs`.
    *   Calculate which slot corresponds to the current `self.mouse_ndc` cursor position.
    *   Apply left-click / right-click item transfer rules to swap, split, merge, or place items.
    *   If ingredients in `craft_input` change, trigger `self.inventory.craft_output = self.recipe_manager.match_recipe(&self.inventory.craft_input, grid_size)`.
    *   If clicking `craft_output`, collect output stack and decrease counts in `craft_input` by 1.

- [ ] **Step 2: Wire click handler to MouseInput**
    In `src/app.rs` MouseInput handler:
    *   If `state.inventory.is_open` is true, call `state.handle_inventory_click(button == MouseButton::Left)` and handle event.

- [ ] **Step 3: Run cargo test to verify**
    Run: `cargo test`
    Expected: PASS

- [ ] **Step 4: Commit**
    ```bash
    git add src/state.rs src/app.rs
    git commit -m "feat: implement slot clicking and drag-and-drop item transfer"
    ```

---

### Task 6: Blocks Mining Drops & Crafting Table World Interaction

**Files:**
*   Modify: `src/state.rs`
*   Modify: `src/interaction.rs`

- [ ] **Step 1: Mining drops block item**
    In `state.rs` where a block is mined:
    *   Retrieve the mined block type.
    *   Convert to `Item::from_block(block_type)`.
    *   In survival mode, add that item to the player's inventory using `self.inventory.add_item(...)`.

- [ ] **Step 2: Right-clicking Crafting Table opens 3x3 GUI**
    Modify block placement/interaction in `src/state.rs` or `src/interaction.rs`:
    *   When right-clicking a `BlockType::CraftingTable`:
        *   Do not place a block.
        *   Open inventory GUI with `is_table_open = true`. Resize `craft_input` to 9 slots.
        *   Lock cursor control.
    *   When closing the GUI:
        *   Return all items in `craft_input` back to the player's backpack.
        *   Reset `is_table_open = false` and resize `craft_input` back to 4 slots.

- [ ] **Step 3: Run cargo test to verify**
    Run: `cargo test`
    Expected: PASS

- [ ] **Step 4: Commit**
    ```bash
    git add src/state.rs src/interaction.rs
    git commit -m "feat: integrate block drops and crafting table 3x3 GUI interaction"
    ```
