# Inventory and Crafting System Design Document

This document describes the design and architecture for the Inventory and Crafting system in the Rust wgpu Minecraft clone.

## 1. System Goals
*   **Unified Item System**: Define a clean `Item` enumeration that unifies blocks (placed in the world) and non-block items (tools, resources).
*   **Backpack & Hotbar Storage**: Implement a complete `Inventory` system supporting a 27-slot main backpack, a 9-slot hotbar, 4 armor slots, a 2x2 local crafting grid, and a floating cursor slot for dragging items.
*   **GUI Rendering**: Render the inventory UI screen in 2D using the existing WGPU shaders and pipelines when the player presses `E`.
*   **Mouse Interaction**: Implement click detection and drag-and-drop item transfer (left-click to swap/drop stacks, right-click to pick up half or place single items).
*   **Crafting Recipes**: Implement a translation-invariant 2D recipe matching engine supporting 30+ core recipes.
*   **Block & World Integration**: Make mined blocks drop items, allowing automatic pickup, and handle the 3x3 Crafting Table interface when right-clicking a placed Crafting Table.

---

## 2. Core Data Structures

### 2.1 Unified Item and ItemStack
We define `Item` as a flat enum representing all possible items. 

```rust
// In src/inventory.rs

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Item {
    Air,
    // Blocks (mapped to BlockType)
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
    
    // Tools
    StoneSword, StonePickaxe, StoneAxe, StoneShovel,
    IronSword, IronPickaxe, IronAxe, IronShovel,
    DiamondSword, DiamondPickaxe, DiamondAxe, DiamondShovel,
    
    // Resources
    Stick,
    Coal,
    IronIngot,
    GoldIngot,
    Diamond,
    Redstone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemStack {
    pub item: Item,
    pub count: u32,
}
```

Every `Item` has associated properties (max stack size, name, whether it represents a block, etc.):

```rust
pub struct ItemProperties {
    pub name: &'static str,
    pub max_stack: u32,
    pub is_block: bool,
    pub block_type: Option<crate::world::BlockType>,
}
```

### 2.2 Inventory Storage
The player's inventory consists of:
*   `hotbar`: 9 slots
*   `main`: 27 slots (3x9)
*   `armor`: 4 slots
*   `craft_input`: 4 slots (2x2) for inventory crafting, or 9 slots (3x3) for Crafting Table crafting.
*   `craft_output`: 1 slot
*   `dragged`: 1 optional slot (item attached to cursor)

```rust
pub struct Inventory {
    pub hotbar: [Option<ItemStack>; 9],
    pub main: [Option<ItemStack>; 27],
    pub armor: [Option<ItemStack>; 4],
    pub craft_input: Vec<Option<ItemStack>>, // Dynamic size: 4 for 2x2, 9 for 3x3
    pub craft_output: Option<ItemStack>,
    pub dragged: Option<ItemStack>,
    pub selected_slot: usize,
    pub open_table: bool, // True if viewing 3x3 crafting table
}
```

---

## 3. GUI Layout & Rendering

We will implement Option A (Classic Layout) with standard Minecraft proportions:

### 3.1 Layout coordinates (NDC: -1.0 to 1.0)
*   **Window Aspect Ratio**: Handled dynamically to ensure slots remain square.
*   **Grid Slot Rendering**: 
    *   Borders drawn using `ui_line_vertex_buffer` (flat gray lines, highlighted white when hover/selected).
    *   Fills/Backgrounds drawn using `ui_vertex_buffer` (dark semi-transparent gray).
*   **Item Icon Rendering**:
    *   Block items: Rendered using their 2D front face texture index.
    *   Non-block items (tools/resources): Rendered using dedicated 2D pixel-art icon index in the texture atlas.
*   **Text/Numbers**: Stack count and hover tooltips drawn using line-based font rendering (`add_string_lines`).

### 3.2 Texture Atlas Extensions
We will modify `src/texture.rs` to generate 16x16 pixel-art icons in rows 3 to 7:
*   Row 3: Stick (col 0), Coal (col 1), Iron Ingot (col 2), Gold Ingot (col 3), Diamond (col 4), Redstone (col 5)
*   Row 4: Stone Sword (col 0), Iron Sword (col 1), Diamond Sword (col 2)
*   Row 5: Stone Pickaxe (col 0), Iron Pickaxe (col 1), Diamond Pickaxe (col 2)
*   Row 6: Stone Axe (col 0), Iron Axe (col 1), Diamond Axe (col 2)
*   Row 7: Stone Shovel (col 0), Iron Shovel (col 1), Diamond Shovel (col 2)

---

## 4. Mouse and Key Interactions

### 4.1 Interface State
*   Pressing `E` toggles the inventory GUI.
*   When open:
    *   `is_paused` remains `false` but camera look controls are locked (mouse cursor is freed and shown).
    *   Keyboard controls for WASD/hotbar numbers are ignored.
    *   Esc closes the GUI.

### 4.2 Click Actions
We define slot positions in NDC. When a click occurs:
1.  Determine which slot was clicked based on cursor coordinates.
2.  Perform stack modification rules:
    *   **Left-Click (No Dragged Item)**: Pick up the entire stack into `dragged`.
    *   **Left-Click (With Dragged Item)**:
        *   If target slot is empty: Drop the entire `dragged` stack.
        *   If target slot contains same item: Merge stacks up to `max_stack`. Remainder stays in `dragged`.
        *   If target slot contains different item: Swap `dragged` and target slot.
    *   **Right-Click (No Dragged Item)**: Pick up half of the stack (rounded up) into `dragged`.
    *   **Right-Click (With Dragged Item)**:
        *   If target slot is empty or contains same item: Place 1 item from `dragged` into target.
3.  Clicking the **Crafting Output Slot**:
    *   Pick up the result stack.
    *   Decrease 1 item from each ingredient in the crafting input grid.
    *   Re-run matching algorithm to update output.

---

## 5. Crafting & Recipe Matching

### 5.1 Recipe Representation
```rust
// In src/crafting.rs

#[derive(Debug, Clone)]
pub struct Recipe {
    pub pattern: Vec<Vec<Item>>, // 2D grid of items (Air represents empty space)
    pub width: usize,
    pub height: usize,
    pub result: ItemStack,
    pub shapeless: bool,
}
```

### 5.2 Matching Algorithm (Option A: Flexible Shape Matching)
1.  Collect all items in the input grid (2x2 or 3x3).
2.  Find the bounding box of non-Air items (min_row, max_row, min_col, max_col).
3.  If grid is empty, result is `None`.
4.  Extract the cropped 2D pattern of items within the bounding box.
5.  Compare this cropped pattern to each recipe:
    *   If `shapeless` is true: Compare the sorted lists of ingredients (and counts) between grid and recipe.
    *   If `shapeless` is false: Check if the cropped pattern matches the recipe pattern exactly.
6.  If match is found, set `craft_output` to the recipe's `result`.

### 5.3 Core Recipe Database (30 Recipes)
1.  **Wood Conversion**: 1 OakLog -> 4 OakPlanks
2.  **Sticks**: 2 OakPlanks (vertical stack) -> 4 Stick
3.  **Crafting Table**: 4 OakPlanks (2x2) -> 1 CraftingTable
4.  **Chest**: 8 OakPlanks (outer ring) -> 1 Chest
5.  **Furnace**: 8 Cobblestone (outer ring) -> 1 Furnace
6.  **Torches**: 1 Coal + 1 Stick (vertical) -> 4 Torch
7.  **Smelting Conversion (Fallback)**:
    *   1 IronOre -> 1 IronIngot
    *   1 GoldOre -> 1 GoldIngot
8.  **Stone Tools**:
    *   Stone Pickaxe: 3 Cobblestone (top row) + 2 Stick (middle-center, bottom-center)
    *   Stone Axe: 3 Cobblestone (top-left, top-center, middle-left) + 2 Stick (middle-center, bottom-center)
    *   Stone Shovel: 1 Cobblestone (top-center) + 2 Stick (middle-center, bottom-center)
    *   Stone Sword: 2 Cobblestone (top-center, middle-center) + 1 Stick (bottom-center)
9.  **Iron Tools**:
    *   Iron Pickaxe: 3 IronIngot + 2 Stick
    *   Iron Axe: 3 IronIngot + 2 Stick
    *   Iron Shovel: 1 IronIngot + 2 Stick
    *   Iron Sword: 2 IronIngot + 1 Stick
10. **Diamond Tools**:
    *   Diamond Pickaxe: 3 Diamond + 2 Stick
    *   Diamond Axe: 3 Diamond + 2 Stick
    *   Diamond Shovel: 1 Diamond + 2 Stick
    *   Diamond Sword: 2 Diamond + 1 Stick
11. **Blocks Creation**:
    *   StoneBrick: 4 Stone (2x2) -> 4 StoneBrick
    *   Brick Block: 4 Clay (2x2) -> 4 Brick
    *   Sandstone: 4 Sand (2x2) -> 4 Sandstone
    *   Snow Block: 4 Snow (2x2) -> 1 Snow
    *   TNT: 5 Redstone (X-shape) + 4 Sand (cross-shape) -> 1 TNT

---

## 6. Verification Plan
*   **Unit Tests**:
    *   Add tests in `src/inventory.rs` for drag-and-drop stack merges, splits, and swaps.
    *   Add tests in `src/crafting.rs` for translation-invariant shape matching (e.g. Torches and Pickaxes) and shapeless recipes.
*   **Manual Verification**:
    *   Open inventory with `E` and verify camera locking, cursor release, and correct grid alignment.
    *   Verify block collection: Mining a block increments the count of that block in the inventory/hotbar.
    *   Verify drag-and-drop actions: Splitting stacks with right-click, merging stacks, and throwing items.
    *   Verify crafting: Craft planks, sticks, crafting tables, and tools. Placing a Crafting Table and right-clicking it opens the 3x3 GUI.
