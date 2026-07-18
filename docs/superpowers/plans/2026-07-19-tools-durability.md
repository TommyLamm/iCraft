# Tools & Durability Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the tools, durability, mining progress tracking, progressive block cracking animations, and durability slot bar indicators in the Minecraft clone.

**Architecture:** Refactor `ItemStack` to store durability. Introduce tool material tier structures. Capture mouse inputs dynamically, tracking holding behavior to update mining progress on a targeted block coordinates. Render a 3D cracking overlay cube scaled by $1.002\times$ in the translucent pass and colored 2D durability bars inside UI slots.

**Tech Stack:** Rust, WGPU, glam, image, winit

---

### Task 1: Refactor `ItemStack`, Define Tool Enums & Properties

**Files:**
- Modify: [inventory.rs](file:///f:/Desktop/MC/src/inventory.rs)
- Modify: [crafting.rs](file:///f:/Desktop/MC/src/crafting.rs)
- Modify: [state.rs](file:///f:/Desktop/MC/src/state.rs)

- [ ] **Step 1: Define Tool Enums and Properties in `inventory.rs`**
Add the enums and properties to `inventory.rs`:
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
```

Implement the `tool_properties` method on `Item` to map the 12 tool items to their properties:
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

- [ ] **Step 2: Add `durability` to `ItemStack` and update test assertions**
Modify `ItemStack` struct:
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
Update all literal creations of `ItemStack` in `src/inventory.rs` (e.g. creative mode setup, `add_item`) to either call `ItemStack::new` or specify `durability` explicitly.

- [ ] **Step 3: Update `src/crafting.rs` recipe instantiations**
Refactor all instances of `ItemStack` in `src/crafting.rs` to include the `durability: 0` or call `ItemStack::new` (e.g. in recipe list generation and matching test cases).

- [ ] **Step 4: Update `src/state.rs` slot item manipulation**
Search for all `ItemStack` literals inside `src/state.rs` (specifically in UI clicks, drag-and-drop, and crafting extraction) and modify them to copy/maintain the `durability` from the original slot stacks.
```rust
// Example:
ItemStack { item: slot.item, count: new_slot_count, durability: slot.durability }
```

- [ ] **Step 5: Write unit tests in `src/inventory.rs`**
Add tests to verify:
```rust
#[test]
fn test_item_properties() {
    let pick = ItemStack::new(Item::StonePickaxe, 1);
    assert_eq!(pick.durability, 131);
    let grass = ItemStack::new(Item::Grass, 64);
    assert_eq!(grass.durability, 0);
}
```
Verify tests compile and pass.

---

### Task 2: Map Block Preferred Tools and Harvest Requirements

**Files:**
- Modify: [world.rs](file:///f:/Desktop/MC/src/world.rs)

- [ ] **Step 1: Implement Preferred Tools and Harvest Tiers in `world.rs`**
Add methods to `BlockType` in `world.rs`:
```rust
use crate::inventory::{ToolType, ToolMaterial};

impl BlockType {
    pub fn preferred_tool(self) -> ToolType {
        match self {
            BlockType::Grass | BlockType::Dirt | BlockType::Sand | BlockType::Gravel | BlockType::Snow | BlockType::Clay | BlockType::Sandstone => ToolType::Shovel,
            BlockType::Stone | BlockType::Cobblestone | BlockType::CoalOre | BlockType::IronOre | BlockType::GoldOre | BlockType::DiamondOre | BlockType::RedstoneOre | BlockType::StoneBrick | BlockType::Obsidian | BlockType::Furnace => ToolType::Pickaxe,
            BlockType::OakLog | BlockType::OakPlanks | BlockType::CraftingTable | BlockType::Chest | BlockType::Bookshelf => ToolType::Axe,
            _ => ToolType::None,
        }
    }

    pub fn min_harvest_material(self) -> Option<ToolMaterial> {
        match self {
            BlockType::Stone | BlockType::Cobblestone | BlockType::CoalOre | BlockType::Furnace | BlockType::StoneBrick | BlockType::Sandstone => Some(ToolMaterial::Wood), // Stone tier tools or above
            BlockType::IronOre => Some(ToolMaterial::Stone),
            BlockType::GoldOre | BlockType::RedstoneOre | BlockType::DiamondOre => Some(ToolMaterial::Iron),
            BlockType::Obsidian => Some(ToolMaterial::Diamond),
            _ => None,
        }
    }
}
```

- [ ] **Step 2: Add unit tests in `world.rs`**
Verify the mappings under `#[cfg(test)] mod tests` in `world.rs`:
```rust
#[test]
fn test_block_harvest_properties() {
    assert_eq!(BlockType::Obsidian.preferred_tool(), ToolType::Pickaxe);
    assert_eq!(BlockType::Obsidian.min_harvest_material(), Some(ToolMaterial::Diamond));
    assert_eq!(BlockType::OakPlanks.preferred_tool(), ToolType::Axe);
    assert_eq!(BlockType::OakPlanks.min_harvest_material(), None);
}
```
Verify tests compile and pass.

---

### Task 3: Continuous Mouse Clicks Input State

**Files:**
- Modify: [state.rs](file:///f:/Desktop/MC/src/state.rs)
- Modify: [app.rs](file:///f:/Desktop/MC/src/app.rs)

- [ ] **Step 1: Add input and mining fields to `State`**
In [state.rs](file:///f:/Desktop/MC/src/state.rs), modify `pub struct State`:
```rust
pub struct State {
    // ... existing fields
    pub left_mouse_pressed: bool,
    pub mining_target: Option<glam::Vec3>,
    pub mining_progress: f32,
}
```
Initialize these fields to `false`, `None`, and `0.0` in `State::new`.

- [ ] **Step 2: Track MouseButton Pressed and Released in `app.rs`**
Modify the `WindowEvent::MouseInput` branch in `app.rs`:
```rust
            WindowEvent::MouseInput {
                state: element_state,
                button,
                ..
            } => {
                if let Some(state) = &mut self.state {
                    let pressed = element_state == ElementState::Pressed;
                    if state.is_paused {
                        if pressed && button == MouseButton::Left {
                            state.handle_menu_click(event_loop);
                        }
                    } else if state.inventory.is_open {
                        if pressed && (button == MouseButton::Left || button == MouseButton::Right) {
                            state.handle_inventory_click(button == MouseButton::Left);
                        }
                    } else {
                        match button {
                            MouseButton::Left => {
                                state.left_mouse_pressed = pressed;
                                if pressed {
                                    // Initial click triggers instant check for Creative mode
                                    if state.game_mode == GameMode::Creative {
                                        state.handle_click(true);
                                    }
                                }
                            }
                            MouseButton::Right => {
                                if pressed {
                                    state.handle_click(false);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
```
Also update `state.is_paused` and `state.inventory.is_open` triggers to set `left_mouse_pressed = false` whenever the pause menu or inventory is opened.

---

### Task 4: Continuous Mining Logic in Update

**Files:**
- Modify: [state.rs](file:///f:/Desktop/MC/src/state.rs)

- [ ] **Step 1: Calculate mining time function**
Implement a helper method on `State` to compute mining time:
```rust
impl State {
    pub fn calculate_mining_time(&self, block: BlockType) -> f32 {
        let hardness = block.properties().hardness;
        if hardness < 0.0 {
            return f32::MAX; // Unbreakable (e.g. bedrock)
        }
        
        let held_item = self.inventory.hotbar[self.inventory.selected].map(|s| s.item).unwrap_or(Item::Air);
        let preferred = block.preferred_tool();
        
        let mut speed_multiplier = 1.0;
        let mut matching_tool = false;
        
        if let Some(tool_prop) = held_item.tool_properties() {
            if tool_prop.tool_type == preferred && preferred != ToolType::None {
                speed_multiplier = tool_prop.mining_speed;
                matching_tool = true;
            }
        }
        
        let base_time = if matching_tool || preferred == ToolType::None {
            hardness * 1.5
        } else {
            hardness * 5.0
        };
        
        base_time / speed_multiplier
    }
}
```

- [ ] **Step 2: Add break block helper method**
Add a method `break_block(&mut self, pos: glam::Vec3)` that implements survival block harvesting:
```rust
impl State {
    pub fn break_block(&mut self, pos: glam::Vec3) {
        let wx = pos.x as i32;
        let wy = pos.y as i32;
        let wz = pos.z as i32;
        let old_block = self.chunk_manager.get_block(wx, wy, wz);
        if old_block == BlockType::Air { return; }

        self.chunk_manager.set_block(wx, wy, wz, BlockType::Air);
        println!("[Debug] Block mined at ({}, {}, {})", wx, wy, wz);

        // Survival drops check
        if self.game_mode == GameMode::Survival {
            let mut eligible_to_harvest = true;
            if let Some(min_material) = old_block.min_harvest_material() {
                let held_item = self.inventory.hotbar[self.inventory.selected].map(|s| s.item).unwrap_or(Item::Air);
                if let Some(tool_prop) = held_item.tool_properties() {
                    eligible_to_harvest = tool_prop.tool_type == old_block.preferred_tool() && tool_prop.material >= min_material;
                } else {
                    eligible_to_harvest = false;
                }
            }

            if eligible_to_harvest {
                self.inventory.add_item(Item::from_block(old_block));
            }

            // Deduct tool durability
            if let Some(stack) = &mut self.inventory.hotbar[self.inventory.selected] {
                if stack.item.tool_properties().is_some() {
                    if stack.durability > 1 {
                        stack.durability -= 1;
                    } else {
                        // Destroy tool
                        println!("[Debug] Tool broke: {:?}", stack.item);
                        self.inventory.hotbar[self.inventory.selected] = None;
                    }
                }
            }
        }

        // recalculate lighting and redraw chunk
        let mut dirty_chunks = std::collections::HashSet::new();
        crate::lighting::update_sky_light_after_removed(&mut self.chunk_manager, wx, wy, wz, &mut dirty_chunks);
        crate::lighting::update_block_light_after_removed(&mut self.chunk_manager, wx, wy, wz, old_block.properties().light_emission, &mut dirty_chunks);

        let cx = wx.div_euclid(crate::world::CHUNK_WIDTH as i32);
        let cz = wz.div_euclid(crate::world::CHUNK_DEPTH as i32);
        let lx = wx.rem_euclid(crate::world::CHUNK_WIDTH as i32);
        let lz = wz.rem_euclid(crate::world::CHUNK_DEPTH as i32);

        dirty_chunks.insert((cx, cz));
        if lx == 0 { dirty_chunks.insert((cx - 1, cz)); }
        if lx == 15 { dirty_chunks.insert((cx + 1, cz)); }
        if lz == 0 { dirty_chunks.insert((cx, cz - 1)); }
        if lz == 15 { dirty_chunks.insert((cx, cz + 1)); }

        for (dcx, dcz) in dirty_chunks {
            if let Some(mesh) = self.chunk_meshes.get_mut(&(dcx, dcz)) {
                mesh.dirty = true;
            }
        }
    }
}
```

- [ ] **Step 3: Update `State::update(dt)` to process mining**
Modify `State::update` to execute continuous mining:
```rust
        // Inside State::update(dt):
        if self.left_mouse_pressed && self.game_mode == GameMode::Survival {
            let dir = glam::Vec3::new(
                self.camera.yaw.cos() * self.camera.pitch.cos(),
                self.camera.pitch.sin(),
                self.camera.yaw.sin() * self.camera.pitch.cos(),
            ).normalize_or_zero();

            if let Some(hit) = raycast(self.camera.position, dir, 5.0, &self.chunk_manager) {
                let target = hit.block_pos;
                let block = self.chunk_manager.get_block(target.x as i32, target.y as i32, target.z as i32);
                
                if block != BlockType::Air && block.properties().hardness >= 0.0 {
                    if self.mining_target != Some(target) {
                        self.mining_target = Some(target);
                        self.mining_progress = 0.0;
                    } else {
                        let mining_time = self.calculate_mining_time(block);
                        self.mining_progress += dt / mining_time;
                        if self.mining_progress >= 1.0 {
                            let pos = target;
                            self.break_block(pos);
                            self.mining_target = None;
                            self.mining_progress = 0.0;
                        }
                    }
                } else {
                    self.mining_target = None;
                    self.mining_progress = 0.0;
                }
            } else {
                self.mining_target = None;
                self.mining_progress = 0.0;
            }
        } else if !self.left_mouse_pressed {
            self.mining_target = None;
            self.mining_progress = 0.0;
        }
```

---

### Task 5: Generate Crack Textures in Texture Atlas

**Files:**
- Modify: [texture.rs](file:///f:/Desktop/MC/src/texture.rs)

- [ ] **Step 1: Implement procedural crack drawing**
Add the drawing method to `src/texture.rs`:
```rust
fn draw_crack_pattern(img: &mut RgbaImage, tx: u32, ty: u32, stage: u32) {
    // Determine crack pattern density based on stage (0..10)
    // We draw random dark gray lines.
    let mut seed = 54321 + stage;
    let mut next_rand = |min: i32, max: i32| -> i32 {
        seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
        let val = (seed / 65536) % 32768;
        let diff = max - min;
        if diff <= 0 { return min; }
        min + (val as i32 % diff)
    };

    // Background is transparent Rgba([0, 0, 0, 0])
    for y in 0..16 {
        for x in 0..16 {
            img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([0, 0, 0, 0]));
        }
    }

    // Number of crack lines scales with stage
    let num_lines = (stage + 1) * 2;
    for _ in 0..num_lines {
        let mut cx = next_rand(0, 16) as i32;
        let mut cy = next_rand(0, 16) as i32;
        let length = next_rand(3, 8);
        for _ in 0..length {
            if cx >= 0 && cx < 16 && cy >= 0 && cy < 16 {
                img.put_pixel(tx * 16 + cx as u32, ty * 16 + cy as u32, Rgba([20, 20, 20, 200])); // Dark grey crack line
            }
            cx += next_rand(-1, 2);
            cy += next_rand(-1, 2);
        }
    }
}
```

- [ ] **Step 2: Generate 10 crack stages in atlas row 15**
In `TextureAtlas::new_procedural`, replace the clear loops for row 15:
```rust
        // Row 15: Crack overlays (cols 0..10)
        for tx in 0..10 {
            draw_crack_pattern(&mut img, tx, 15, tx);
        }
        for tx in 10..16 {
            for y in 0..16 {
                for x in 0..16 {
                    img.put_pixel(tx * 16 + x, 15 * 16 + y, Rgba([0, 0, 0, 0]));
                }
            }
        }
```
Verify compilation.

---

### Task 6: Populate & Render Crack Overlay Box

**Files:**
- Modify: [state.rs](file:///f:/Desktop/MC/src/state.rs)

- [ ] **Step 1: Initialize Buffers in `State`**
In `State::new`, initialize a vertex and index buffer:
```rust
        let crack_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Crack Vertex Buffer"),
            size: (24 * std::mem::size_of::<Vertex>()) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let crack_index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Crack Index Buffer"),
            size: (36 * std::mem::size_of::<u32>()) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
```
Add `crack_vertex_buffer` and `crack_index_buffer` to `State` struct fields.

- [ ] **Step 2: Implement dynamic crack geometry generation**
Create the crack box vertices based on `mining_progress` and `mining_target`:
```rust
impl State {
    pub fn update_crack_buffers(&self, target_pos: glam::Vec3, progress: f32) -> Option<(u32, u32)> {
        let stage = (progress * 10.0).floor().clamp(0.0, 9.0) as u32;
        let wx = target_pos.x;
        let wy = target_pos.y;
        let wz = target_pos.z;

        // Cube corner scale (slightly expanded to 1.002 to avoid z-fighting)
        let s = 1.002;
        let offset_min = 0.5 - 0.5 * s;
        let offset_max = 0.5 + 0.5 * s;

        let faces = [
            // South
            ([0.0, 0.0, 1.0], [
                ([offset_min, offset_min, offset_max], [0.0, 1.0]),
                ([offset_max, offset_min, offset_max], [1.0, 1.0]),
                ([offset_max, offset_max, offset_max], [1.0, 0.0]),
                ([offset_min, offset_max, offset_max], [0.0, 0.0]),
            ]),
            // North
            ([0.0, 0.0, -1.0], [
                ([offset_max, offset_min, offset_min], [0.0, 1.0]),
                ([offset_min, offset_min, offset_min], [1.0, 1.0]),
                ([offset_min, offset_max, offset_min], [1.0, 0.0]),
                ([offset_max, offset_max, offset_min], [0.0, 0.0]),
            ]),
            // West
            ([-1.0, 0.0, 0.0], [
                ([offset_min, offset_min, offset_min], [0.0, 1.0]),
                ([offset_min, offset_min, offset_max], [1.0, 1.0]),
                ([offset_min, offset_max, offset_max], [1.0, 0.0]),
                ([offset_min, offset_max, offset_min], [0.0, 0.0]),
            ]),
            // East
            ([1.0, 0.0, 0.0], [
                ([offset_max, offset_min, offset_max], [0.0, 1.0]),
                ([offset_max, offset_min, offset_min], [1.0, 1.0]),
                ([offset_max, offset_max, offset_min], [1.0, 0.0]),
                ([offset_max, offset_max, offset_max], [0.0, 0.0]),
            ]),
            // Up
            ([0.0, 1.0, 0.0], [
                ([offset_min, offset_max, offset_max], [0.0, 1.0]),
                ([offset_max, offset_max, offset_max], [1.0, 1.0]),
                ([offset_max, offset_max, offset_min], [1.0, 0.0]),
                ([offset_min, offset_max, offset_min], [0.0, 0.0]),
            ]),
            // Down
            ([0.0, -1.0, 0.0], [
                ([offset_min, offset_min, offset_min], [0.0, 1.0]),
                ([offset_max, offset_min, offset_min], [1.0, 1.0]),
                ([offset_max, offset_min, offset_max], [1.0, 0.0]),
                ([offset_min, offset_min, offset_max], [0.0, 0.0]),
            ]),
        ];

        let mut vertices = Vec::new();
        let mut indices = Vec::new();

        let max_light = self.chunk_manager.get_sky_light(wx as i32, wy as i32, wz as i32)
            .max(self.chunk_manager.get_block_light(wx as i32, wy as i32, wz as i32));
        
        for (face_idx, (normal, corners)) in faces.iter().enumerate() {
            let start_idx = vertices.len() as u32;
            let multiplier = match face_idx {
                4 => 1.0,
                5 => 0.5,
                _ => 0.8,
            };
            let light_val = (max_light as f32 / 15.0) * multiplier;

            for &(corner, uv) in corners {
                // UV points to Row 15, Col "stage"
                let u = (uv[0] + stage as f32) * 0.0625;
                let v = (uv[1] + 15.0) * 0.0625;
                vertices.push(Vertex {
                    position: [wx + corner[0], wy + corner[1], wz + corner[2]],
                    tex_coords: [u, v],
                    light_level: light_val,
                });
            }

            indices.push(start_idx + 0);
            indices.push(start_idx + 1);
            indices.push(start_idx + 2);
            indices.push(start_idx + 0);
            indices.push(start_idx + 2);
            indices.push(start_idx + 3);
        }

        self.queue.write_buffer(&self.crack_vertex_buffer, 0, bytemuck::cast_slice(&vertices));
        self.queue.write_buffer(&self.crack_index_buffer, 0, bytemuck::cast_slice(&indices));

        Some((vertices.len() as u32, indices.len() as u32))
    }
}
```

- [ ] **Step 3: Execute crack rendering call in `State::render()`**
In the translucent rendering section of `State::render()` (right after rendering other translucent chunk meshes):
```rust
            // Pass 2: Translucent (Water/Ice)
            render_pass.set_pipeline(&self.trans_pipeline);
            for mesh in self.chunk_meshes.values() {
                if mesh.transparent_num_indices > 0 {
                    render_pass.set_vertex_buffer(0, mesh.transparent_vertex_buffer.slice(..));
                    render_pass.set_index_buffer(mesh.transparent_index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                    render_pass.draw_indexed(0..mesh.transparent_num_indices, 0, 0..1);
                }
            }

            // Draw Block cracking animation overlay
            if let Some(target) = self.mining_target {
                if self.mining_progress > 0.0 {
                    if let Some((_num_vertices, num_indices)) = self.update_crack_buffers(target, self.mining_progress) {
                        render_pass.set_vertex_buffer(0, self.crack_vertex_buffer.slice(..));
                        render_pass.set_index_buffer(self.crack_index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                        render_pass.draw_indexed(0..num_indices, 0, 0..1);
                    }
                }
            }
```
Verify compilation.

---

### Task 7: Render Colored Durability Bars in UI Slots

**Files:**
- Modify: [state.rs](file:///f:/Desktop/MC/src/state.rs)

- [ ] **Step 1: Add a helper to push durability bars in UI vertices**
In [state.rs](file:///f:/Desktop/MC/src/state.rs), locate `update_ui` or where slot stack items are rendered.
Implement drawing helper inside slot loops (both for backpack open state and hotbar-only closed state):
```rust
let draw_durability_bar = |stack: &ItemStack, x0: f32, x1: f32, y0: f32, y1: f32, aspect: f32, ui_vertices: &mut Vec<UiVertex>| {
    if let Some(tool_prop) = stack.item.tool_properties() {
        let max_dur = tool_prop.durability;
        if stack.durability < max_dur {
            let ratio = (stack.durability as f32 / max_dur as f32).clamp(0.0, 1.0);
            
            // Define bar bounds relative to slot size
            let slot_w = x1 - x0;
            let slot_h = y1 - y0;
            
            let bar_x0 = x0 + slot_w * 0.15;
            let bar_x1 = x1 - slot_w * 0.15;
            let bar_y0 = y0 + slot_h * 0.10;
            let bar_y1 = y0 + slot_h * 0.16;
            
            // 1. Black background bar
            let bg_color = [0.0, 0.0, 0.0, 1.0];
            ui_vertices.push(UiVertex { position: [bar_x0, bar_y1, 0.0], color: bg_color });
            ui_vertices.push(UiVertex { position: [bar_x0, bar_y0, 0.0], color: bg_color });
            ui_vertices.push(UiVertex { position: [bar_x1, bar_y0, 0.0], color: bg_color });
            ui_vertices.push(UiVertex { position: [bar_x0, bar_y1, 0.0], color: bg_color });
            ui_vertices.push(UiVertex { position: [bar_x1, bar_y0, 0.0], color: bg_color });
            ui_vertices.push(UiVertex { position: [bar_x1, bar_y1, 0.0], color: bg_color });
            
            // 2. Colored foreground bar
            let fg_x1 = bar_x0 + (bar_x1 - bar_x0) * ratio;
            let (r, g) = if ratio > 0.5 {
                ((1.0 - ratio) * 2.0, 1.0)
            } else {
                (1.0, ratio * 2.0)
            };
            let fg_color = [r, g, 0.0, 1.0];
            
            ui_vertices.push(UiVertex { position: [bar_x0, bar_y1, 0.0], color: fg_color });
            ui_vertices.push(UiVertex { position: [bar_x0, bar_y0, 0.0], color: fg_color });
            ui_vertices.push(UiVertex { position: [fg_x1, bar_y0, 0.0], color: fg_color });
            ui_vertices.push(UiVertex { position: [bar_x0, bar_y1, 0.0], color: fg_color });
            ui_vertices.push(UiVertex { position: [fg_x1, bar_y0, 0.0], color: fg_color });
            ui_vertices.push(UiVertex { position: [fg_x1, bar_y1, 0.0], color: fg_color });
        }
    }
};
```

- [ ] **Step 2: Inject durability bar drawing inside `inventory.is_open` slot loop**
Find the item rendering block inside `self.inventory.is_open` slot iteration:
```rust
                    // Slot Item
                    if let Some(stack) = self.get_item_at_slot(slot_type) {
                        // ... existing item drawing ...
                        
                        // Draw durability bar
                        draw_durability_bar(&stack, x0, x1, y0, y1, aspect, &mut ui_vertices);
                    }
```

- [ ] **Step 3: Inject durability bar drawing inside HUD hotbar slot loop**
Find the HUD hotbar slot drawing block:
```rust
                    if let Some(stack) = &self.inventory.hotbar[i] {
                        // ... existing item drawing ...

                        // Draw durability bar
                        draw_durability_bar(stack, x0, x1, y0, y1, aspect, &mut ui_vertices);
                    }
```
Verify the build and run unit tests.
