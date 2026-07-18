# Hotbar & Block Selection Design Specification

> **Date**: 2026-07-18  
> **Status**: Approved  

---

## 1. Goal

Implement a hotbar system at the bottom of the screen containing 9 slots. Allow users to select different block types using digit keys `1-9` and the mouse scroll wheel. Support both Creative and Survival modes, where blocks are consumed when placed and collected when dug in Survival mode.

---

## 2. Architecture & Data Structures

We introduce a new module `inventory` (`src/inventory.rs`) containing:

### 2.1 Game Mode
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameMode {
    Creative,
    Survival,
}
```

### 2.2 Item Stack
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemStack {
    pub block_type: BlockType,
    pub count: u32,
}
```

### 2.3 Hotbar
```rust
pub struct Hotbar {
    pub slots: [Option<ItemStack>; 9],
    pub selected: usize, // 0..8
}
```

* **`new_creative()`**: Initializes the 9 slots with pre-filled default blocks (Grass, Dirt, Stone, Oak Log, Oak Planks, Glass, Cobblestone, Water, Torch) each with a stack count of 64.
* **`get_selected_block()`**: Returns `Option<BlockType>` representing the block in the active slot.
* **`add_item(block_type)`**: Collects a block (e.g. from digging). Adds to an existing matching stack (max size 64) or places in the first empty slot. Returns `bool` for success.
* **`use_selected_item(infinite)`**: Consumes 1 block from the active slot if not infinite (Creative). Removes stack if count drops to 0.

---

## 3. UI & Rendering Pipeline

### 3.1 Vertex Types
We define `TexturedUiVertex` in `src/state.rs` for 2D UI elements that sample the texture atlas:
```rust
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct TexturedUiVertex {
    pub position: [f32; 3],
    pub tex_coords: [f32; 2],
    pub color: [f32; 4],
}
```

### 3.2 Pipeline Setup
We configure a new pipeline `ui_textured_pipeline` in `State::new`. It uses the existing `render_pipeline_layout` to bind the texture atlas (at bind group slot 0) and rendering to screen with alpha blending enabled.
We pre-allocate `ui_textured_vertex_buffer` of size 1024 vertices.

### 3.3 Shaders
We add `vs_textured_ui` and `fs_textured_ui` in `src/shader.wgsl` to render textured UI components using the shared bind group.

### 3.4 HUD Elements
* **Background Bar**: A dark semi-transparent rectangle drawn under the slots: `[-0.41, 0.41]` NDC.
* **Slot Borders**: Wireframe borders for the 9 slots. Drawn in white for the selected slot, and dark gray for others.
* **Block Thumbnails**: Flat 2D quads representing block textures inside each slot. Retreived via `block_type.get_face_tex_index(0)` (front/side texture) and mapped to UVs.
* **Stack Counts**: String representation of counts (e.g. "64") rendered using `add_string_lines` at the bottom-right of each slot.

---

## 4. Input & Event Handling

* **Digits 1-9**: Switch `state.hotbar.selected` directly to `0..8`.
* **Mouse Scroll Wheel**:
  * Scroll Up (positive delta): Moves selection left (decrement index, wrapping around).
  * Scroll Down (negative delta): Moves selection right (increment index, wrapping around).
* **Key `G`**: Toggles game mode between Creative and Survival.
* **Mouse Clicks**:
  * Left Click: Digging block. Sets block to `Air`, and adds the block type to `hotbar` if in `Survival` mode.
  * Right Click: Placing block. Retrieves selected block from `hotbar` and places it. If in `Survival` mode, decrements count from active slot.
