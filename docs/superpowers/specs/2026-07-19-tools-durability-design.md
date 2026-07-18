# Tools & Durability Design Specification

This document specifies the design for the Tools & Durability system in the Rust-based Minecraft clone.

## 1. Overview
The Tools & Durability system enhances the survival gameplay loop by introducing tools with distinct materials and categories. Each block will require different mining times depending on the held tool, and some blocks will require a minimum tool material tier (harvest level) to drop items. Additionally, tools will lose durability upon use, showing a colored durability bar in the UI, and blocks will display a progressive cracking animation when mined.

---

## 2. Item & ItemStack Refactoring

### 2.1 Tool Definitions
We will introduce `ToolType` and `ToolMaterial` enums in `src/inventory.rs`:

```rust
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
    Wood,   // Included for completeness, unused for now
    Stone,
    Iron,
    Gold,   // Included for completeness, unused for now
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
```

We will implement a method `tool_properties` on `Item` to retrieve properties for tool items:
```rust
impl Item {
    pub fn tool_properties(self) -> Option<ToolProperties> {
        match self {
            Item::StoneSword => Some(ToolProperties { tool_type: ToolType::Sword, material: ToolMaterial::Stone, mining_speed: 4.0, durability: 131, damage: 5.0 }),
            Item::StonePickaxe => Some(ToolProperties { tool_type: ToolType::Pickaxe, material: ToolMaterial::Stone, mining_speed: 4.0, durability: 131, damage: 3.0 }),
            Item::StoneAxe => Some(ToolProperties { tool_type: ToolType::Axe, material: ToolMaterial::Stone, mining_speed: 4.0, durability: 131, damage: 4.0 }),
            Item::StoneShovel => Some(ToolProperties { tool_type: ToolType::Shovel, material: ToolMaterial::Stone, mining_speed: 4.0, durability: 131, damage: 2.0 }),

            Item::IronSword => Some(ToolProperties { tool_type: ToolType::Sword, material: ToolMaterial::Iron, mining_speed: 6.0, durability: 250, damage: 6.0 }),
            Item::IronPickaxe => Some(ToolProperties { tool_type: ToolType::Pickaxe, material: ToolMaterial::Iron, mining_speed: 6.0, durability: 250, damage: 4.0 }),
            Item::IronAxe => Some(ToolProperties { tool_type: ToolType::Axe, material: ToolMaterial::Iron, mining_speed: 6.0, durability: 250, damage: 5.0 }),
            Item::IronShovel => Some(ToolProperties { tool_type: ToolType::Shovel, material: ToolMaterial::Iron, mining_speed: 6.0, durability: 250, damage: 3.0 }),

            Item::DiamondSword => Some(ToolProperties { tool_type: ToolType::Sword, material: ToolMaterial::Diamond, mining_speed: 8.0, durability: 1561, damage: 7.0 }),
            Item::DiamondPickaxe => Some(ToolProperties { tool_type: ToolType::Pickaxe, material: ToolMaterial::Diamond, mining_speed: 8.0, durability: 1561, damage: 5.0 }),
            Item::DiamondAxe => Some(ToolProperties { tool_type: ToolType::Axe, material: ToolMaterial::Diamond, mining_speed: 8.0, durability: 1561, damage: 6.0 }),
            Item::DiamondShovel => Some(ToolProperties { tool_type: ToolType::Shovel, material: ToolMaterial::Diamond, mining_speed: 8.0, durability: 1561, damage: 4.0 }),

            _ => None,
        }
    }
}
```

### 2.2 ItemStack Struct Refactoring
To track tool durability, we will extend `ItemStack` to include a `durability` field. For non-tool items, this field defaults to 0 and is ignored.
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemStack {
    pub item: Item,
    pub count: u32,
    pub durability: u32,
}

impl ItemStack {
    pub fn new(item: Item, count: u32) -> Self {
        let durability = item.tool_properties().map(|t| t.durability).unwrap_or(0);
        Self { item, count, durability }
    }
}
```
All literal instantiations of `ItemStack` in the codebase will be updated to use `ItemStack::new` or provide the `durability` field explicitly, preserving durability when splitting, dragging, or merging stacks in the inventory UI.

---

## 3. Mining Properties & Mining Time Calculations

### 3.1 Block Mining Specifications
We map each block to its preferred tool category and the minimum harvest material tier required for dropping items:

| Block Type | Preferred Tool | Hardness | Min Harvest Tier |
|---|---|---|---|
| Grass / Dirt / Sand / Gravel / Clay / Snow | Shovel | 0.5 - 0.6 | None |
| Oak Log / Oak Planks / Bookshelf / Chest / Crafting Table | Axe | 1.5 - 2.5 | None |
| Stone / Cobblestone / Coal Ore / Furnace / Brick / Stone Brick / Sandstone | Pickaxe | 1.5 - 3.5 | Wood (Stone tool matches) |
| Iron Ore | Pickaxe | 3.0 | Stone |
| Gold Ore / Redstone Ore / Diamond Ore | Pickaxe | 3.0 | Iron |
| Obsidian | Pickaxe | 50.0 | Diamond |
| Glass / Leaves / Water / Bedrock / TNT / Torch | None (Hand) | - | None |

*Note: Bedrock is unbreakable in Survival Mode (hardness = -1.0).*

### 3.2 Mining Time Formula
In Survival mode, the time (in seconds) required to mine a block is calculated as:
1. Determine if the currently held item matches the block's preferred tool:
   - If yes: `speed_multiplier = tool.mining_speed`
   - If no: `speed_multiplier = 1.0`
2. Calculate the base time:
   - If preferred tool held: `base_time = hardness * 1.5`
   - If incorrect tool/hand: `base_time = hardness * 5.0`
3. Final mining time:
   - `mining_time = base_time / speed_multiplier`

If `mining_time` is zero or less (e.g. bedrock, air), the block cannot be mined.

---

## 4. Continuous Mining System Loop

### 4.1 Input State Tracking
In `State`, we will add fields to track:
* `left_mouse_pressed: bool` — Set via `WindowEvent::MouseInput` in `app.rs`.
* `mining_target: Option<Vec3>` — Coordinates of the block currently being mined.
* `mining_progress: f32` — Current progress value from `0.0` to `1.0`.

### 4.2 Mining State Update
In `State::update(dt)`, if `self.left_mouse_pressed` is `true` and `self.game_mode == GameMode::Survival`:
1. Execute a raycast from camera view direction (max distance 5.0 blocks).
2. If the raycast hits a solid block at coordinate `pos`:
   - If `self.mining_target` is `None` or `Some(target_pos)` where `target_pos != pos`:
     - Set `self.mining_target = Some(pos)`.
     - Reset `self.mining_progress = 0.0`.
   - If `self.mining_target == Some(pos)`:
     - Check if block is breakable (hardness >= 0.0).
     - Calculate `mining_time` based on current item properties.
     - Increment `self.mining_progress += dt / mining_time`.
     - If `self.mining_progress >= 1.0`:
       - Call block destruction logic:
         - Replace block with `Air`.
         - If player's held tool meets or exceeds the `Min Harvest Tier` required by the block, add the block item to the inventory.
         - Subtract 1 durability from the tool. If durability reaches 0, destroy the tool (remove item stack from hotbar slot) and log a message to the console.
         - Trigger lighting recalculation and dirty chunk mesh flag.
         - Reset `self.mining_target = None` and `self.mining_progress = 0.0`.
3. If `self.left_mouse_pressed` is `false`, or the raycast misses, or hits air:
   - Reset `self.mining_target = None` and `self.mining_progress = 0.0`.

---

## 5. Visual Break Cracks & UI Durability Bar

### 5.1 Crack Overlay Textures
In `src/texture.rs`, we will procedurally generate 10 crack stages in the texture atlas (Row 15, Columns 0 to 9).
- For each stage $S \in [0, 9]$, we will draw black pixels (representing growing crack lines) onto a transparent background `Rgba([0, 0, 0, 0])`.
- Since the crack lines are drawn in black with `a > 0.5` (e.g. `Rgba([0, 0, 0, 220])`), they will pass the fragment shader's alpha threshold (`color.a < 0.5` discard test) and overlay correctly on top of any block face.

### 5.2 Standalone Crack Overlay Box Rendering
In `State::new`, we will pre-allocate a vertex buffer (`crack_vertex_buffer`) and index buffer (`crack_index_buffer`) capable of holding 24 vertices and 36 indices for a single cube.
In `State::render()`, if `mining_target == Some(pos)` and `mining_progress > 0.0`:
1. Calculate the active crack stage: `stage = (mining_progress * 10.0).floor().clamp(0.0, 9.0) as u32`.
2. Generate vertex data for a cube at `pos`, scaled outward slightly by $1.002\times$ (to prevent z-fighting).
3. Set the texture coordinates (UVs) of all faces to point to the active crack stage tile (Row 15, Column `stage`).
4. Write the vertices and indices to the GPU buffers.
5. In the translucent render pass (`trans_pipeline`), set the vertex and index buffers, bind the camera bind group, and execute a `draw_indexed(0..36, 0, 0..1)` call.

### 5.3 Durability Bar UI
When rendering the inventory and hotbar slots:
1. Check if the item stack contains a tool and `durability < max_durability`.
2. If yes, calculate `ratio = durability as f32 / max_durability as f32`.
3. Calculate the 2D screen coordinates for a horizontal bar near the bottom of the slot:
   - Width: slot width $\times$ 0.7.
   - Height: slot height $\times$ 0.06.
4. Draw a background bar in black `[0.0, 0.0, 0.0, 1.0]`.
5. Draw a foreground bar scaled by `ratio`.
6. Color-code the foreground bar:
   - If `ratio > 0.5`: `r = (1.0 - ratio) * 2.0`, `g = 1.0`, `b = 0.0`.
   - If `ratio <= 0.5`: `r = 1.0`, `g = ratio * 2.0`, `b = 0.0`.

---

## 6. Verification Plan

### 6.1 Automated Tests
Verify item properties, durability, and mining time computations via unit tests in `src/inventory.rs` and `src/interaction.rs`:
- Verify stone, iron, diamond tools have correct max durability.
- Verify block hardness mapping.
- Verify correct item drop eligibility check based on harvest level tiers.

### 6.2 Manual Verification
Build and run the game:
1. Verify that holding Left-Click on a block shows a cracking overlay that grows progressively.
2. Verify that releasing the mouse button or looking away resets the cracking overlay instantly.
3. Verify that mining stone with a Pickaxe is much faster than with hand/sword, and that it drops a cobblestone item.
4. Verify that mining stone with hand drops nothing.
5. Verify that tool durability decreases by 1 after mining a block.
6. Verify that a durability bar (colored green $\rightarrow$ yellow $\rightarrow$ red) appears at the bottom of the tool's slot when damaged.
7. Verify that when durability drops to 0, the tool is removed and a console message is printed.
