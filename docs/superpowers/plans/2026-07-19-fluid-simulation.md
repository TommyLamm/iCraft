# Voxel Fluid Simulation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement water and lava fluid dynamics, flowing mesh height adjustments, texture UV scrolling animation, player swimming/buoyancy physics, head underwater visual effects, and oxygen drowning HUD in the Minecraft clone.

**Architecture:** Extend `BlockType` and `Item` to support `Lava`. Store level and falling flags in a separate `fluid_levels` array per `Chunk`. Build a tick-based propagation solver in `src/fluid.rs` to handle spreading, infinite water, and fluid interactions. Update `generate_mesh` to lower fluid heights based on level, and animate fluid UVs in the shader using a time uniform. Implement water/lava forces in `PlayerPhysics` and breath tracking in `PlayerState`.

**Tech Stack:** Rust, WGPU, glam, noise

---

### Task 1: Add `Lava` to `BlockType` & `Item`, Define properties

**Files:**
- Modify: [world.rs](file:///f:/Desktop/MC/src/world.rs:11-45)
- Modify: [inventory.rs](file:///f:/Desktop/MC/src/inventory.rs:10-33)
- Modify: [crafting.rs](file:///f:/Desktop/MC/src/crafting.rs:10-50)

- [ ] **Step 1: Add Lava enum variant to `BlockType` and update properties**
In `src/world.rs`, add `Lava` to `BlockType` and update `properties()` and `get_face_tex_index()`:
```rust
// In BlockType:
pub enum BlockType {
    // ...
    Torch = 30,
    Lava = 31,
}

// In BlockProperties match:
BlockType::Lava => BlockProperties {
    name: "Lava",
    hardness: 100.0,
    render_type: RenderType::Opaque,
    is_solid: false,
    is_passable: true,
    light_emission: 15,
},

// In get_face_tex_index match (using column 15, row 2 for lava, or coordinate 15, 2):
BlockType::Lava => (15, 2),
```

- [ ] **Step 2: Add Lava block to `Item` enum**
In `src/inventory.rs`, add `Lava` item:
```rust
// In Item:
pub enum Item {
    // ...
    Torch,
    Lava,
}
```
Update all matches over `Item` in `src/inventory.rs` to handle `Item::Lava` (e.g. creative slot filling, `add_item` logic, matching `Item::properties()`).

- [ ] **Step 3: Update `src/crafting.rs` Item matches**
Ensure `src/crafting.rs` handles the new `Item::Lava` enum variant (if there are matches over all `Item` values in recipe checking/generation).

- [ ] **Step 4: Commit**
```bash
git add src/world.rs src/inventory.rs src/crafting.rs
git commit -m "feat: add Lava variant to BlockType and Item enums"
```

---

### Task 2: Define `fluid_levels` Array in `Chunk` & Allocation

**Files:**
- Modify: [world.rs](file:///f:/Desktop/MC/src/world.rs:396-410)
- Modify: [chunk_manager.rs](file:///f:/Desktop/MC/src/chunk_manager.rs:4-83)

- [ ] **Step 1: Add `fluid_levels` to `Chunk` struct**
Modify `Chunk` struct definition in `src/world.rs`:
```rust
pub struct Chunk {
    pub chunk_x: i32,
    pub chunk_z: i32,
    pub blocks: Box<[[[BlockType; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH]>,
    pub sky_light: Box<[[[u8; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH]>,
    pub block_light: Box<[[[u8; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH]>,
    pub heightmap: Box<[[u16; CHUNK_DEPTH]; CHUNK_WIDTH]>,
    pub fluid_levels: Box<[[[u8; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH]>,
}
```
Initialize `fluid_levels` to `0` inside `Chunk::new()`:
```rust
let fluid_levels: Box<[[[u8; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH]> =
    vec![[[0u8; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH]
        .try_into().unwrap();
```

- [ ] **Step 2: Add getter/setter helpers to `ChunkManager`**
In `src/chunk_manager.rs`, implement fluid getter/setter methods:
```rust
impl ChunkManager {
    pub fn get_fluid_level(&self, wx: i32, wy: i32, wz: i32) -> u8 {
        if let Some(((cx, cz), (bx, by, bz))) = self.world_to_local(wx, wy, wz) {
            if let Some(chunk) = self.chunks.get(&(cx, cz)) {
                return chunk.fluid_levels[bx][by][bz] & 0x07;
            }
        }
        0
    }

    pub fn set_fluid_level(&mut self, wx: i32, wy: i32, wz: i32, level: u8) {
        if let Some(((cx, cz), (bx, by, bz))) = self.world_to_local(wx, wy, wz) {
            if let Some(chunk) = self.chunks.get_mut(&(cx, cz)) {
                let current = chunk.fluid_levels[bx][by][bz];
                chunk.fluid_levels[bx][by][bz] = (current & 0xF8) | (level & 0x07);
            }
        }
    }

    pub fn get_fluid_falling(&self, wx: i32, wy: i32, wz: i32) -> bool {
        if let Some(((cx, cz), (bx, by, bz))) = self.world_to_local(wx, wy, wz) {
            if let Some(chunk) = self.chunks.get(&(cx, cz)) {
                return (chunk.fluid_levels[bx][by][bz] & 0x08) != 0;
            }
        }
        false
    }

    pub fn set_fluid_falling(&mut self, wx: i32, wy: i32, wz: i32, falling: bool) {
        if let Some(((cx, cz), (bx, by, bz))) = self.world_to_local(wx, wy, wz) {
            if let Some(chunk) = self.chunks.get_mut(&(cx, cz)) {
                let current = chunk.fluid_levels[bx][by][bz];
                if falling {
                    chunk.fluid_levels[bx][by][bz] = current | 0x08;
                } else {
                    chunk.fluid_levels[bx][by][bz] = current & !0x08;
                }
            }
        }
    }
}
```

- [ ] **Step 3: Write tests for fluid levels pack/unpack**
In `src/world.rs`, add a unit test:
```rust
#[test]
fn test_fluid_level_encoding() {
    let mut chunk = Chunk::new(0, 0);
    chunk.fluid_levels[0][10][0] = 5 | 0x08; // level 5, falling = true
    assert_eq!(chunk.fluid_levels[0][10][0] & 0x07, 5);
    assert_eq!((chunk.fluid_levels[0][10][0] & 0x08) != 0, true);
}
```
Verify tests compile and pass.

- [ ] **Step 4: Commit**
```bash
git add src/world.rs src/chunk_manager.rs
git commit -m "feat: add fluid_levels array and getter/setter helpers"
```

---

### Task 3: Implement Water and Lava Tick Simulation Logic

**Files:**
- Create: [fluid.rs](file:///f:/Desktop/MC/src/fluid.rs)
- Modify: [state.rs](file:///f:/Desktop/MC/src/state.rs)

- [ ] **Step 1: Write Fluid Simulation Solver in `src/fluid.rs`**
Create a new file `src/fluid.rs` containing the tick propagation solver:
```rust
use crate::world::{BlockType, CHUNK_HEIGHT};
use crate::chunk_manager::ChunkManager;
use std::collections::{HashSet, VecDeque};

pub fn tick_fluids(chunk_manager: &mut ChunkManager, is_lava: bool) -> HashSet<(i32, i32)> {
    let mut dirty_chunks = HashSet::new();
    let target_type = if is_lava { BlockType::Lava } else { BlockType::Water };
    let flow_limit = 7; // Max level is 7 (thinnest)

    // Gather all fluid blocks of target type
    let mut fluids = Vec::new();
    for (&(cx, cz), chunk) in chunk_manager.chunks.iter() {
        for x in 0..16 {
            for z in 0..16 {
                for y in 0..CHUNK_HEIGHT {
                    let block = chunk.blocks[x][y][z];
                    if block == target_type {
                        let wx = cx * 16 + x as i32;
                        let wy = y as i32;
                        let wz = cz * 16 + z as i32;
                        fluids.push((wx, wy, wz));
                    }
                }
            }
        }
    }

    // Keep track of visited and newly updated blocks
    let mut to_update = Vec::new();

    for (wx, wy, wz) in fluids {
        let level = chunk_manager.get_fluid_level(wx, wy, wz);
        let falling = chunk_manager.get_fluid_falling(wx, wy, wz);

        // 1. Check downwards
        if wy > 0 {
            let below = chunk_manager.get_block(wx, wy - 1, wz);
            if below == BlockType::Air || below.properties().is_passable {
                to_update.push((wx, wy - 1, wz, target_type, 0, true));
                continue;
            }
        }

        // 2. Check horizontally if below is solid
        if level < flow_limit && !falling {
            let neighbors = [
                (wx + 1, wy, wz),
                (wx - 1, wy, wz),
                (wx, wy, wz + 1),
                (wx, wy, wz - 1),
            ];
            for &(nx, ny, nz) in neighbors.iter() {
                let n_block = chunk_manager.get_block(nx, ny, nz);
                if n_block == BlockType::Air || n_block.properties().is_passable {
                    let n_level = chunk_manager.get_fluid_level(nx, ny, nz);
                    let target_level = level + 1;
                    if n_block != target_type || n_level > target_level {
                        to_update.push((nx, ny, nz, target_type, target_level, false));
                    }
                }
            }
        }
    }

    // Apply updates
    for (wx, wy, wz, block, level, falling) in to_update {
        chunk_manager.set_block(wx, wy, wz, block);
        chunk_manager.set_fluid_level(wx, wy, wz, level);
        chunk_manager.set_fluid_falling(wx, wy, wz, falling);
        
        let cx = wx.div_euclid(16);
        let cz = wz.div_euclid(16);
        dirty_chunks.insert((cx, cz));
    }

    dirty_chunks
}
```

- [ ] **Step 2: Add Ticking loop to `State::update(dt)`**
In `src/state.rs`, add fluid timers to `State`:
```rust
// Add fields in State:
pub water_tick_timer: f32,
pub lava_tick_timer: f32,
```
Initialize them to `0.0` in `State::new`.
In `State::update(dt)`, trigger fluid ticks based on intervals (Water = 0.25s, Lava = 1.5s):
```rust
self.water_tick_timer += dt;
if self.water_tick_timer >= 0.25 {
    self.water_tick_timer = 0.0;
    let dirty = crate::fluid::tick_fluids(&mut self.chunk_manager, false);
    for (cx, cz) in dirty {
        if let Some(mesh) = self.chunk_meshes.get_mut(&(cx, cz)) {
            mesh.dirty = true;
        }
    }
}

self.lava_tick_timer += dt;
if self.lava_tick_timer >= 1.5 {
    self.lava_tick_timer = 0.0;
    let dirty = crate::fluid::tick_fluids(&mut self.chunk_manager, true);
    for (cx, cz) in dirty {
        if let Some(mesh) = self.chunk_meshes.get_mut(&(cx, cz)) {
            mesh.dirty = true;
        }
    }
}
```

- [ ] **Step 3: Commit**
```bash
git add src/fluid.rs src/state.rs
git commit -m "feat: implement fluid ticking simulation loop"
```

---

### Task 4: Add Infinite Water & Fluid Interactions

**Files:**
- Modify: [fluid.rs](file:///f:/Desktop/MC/src/fluid.rs)

- [ ] **Step 1: Implement Infinite Water logic in `src/fluid.rs`**
Add check to see if an air/flowing water block can form an infinite water source block:
```rust
// Inside tick_fluids logic:
// If block is air/water level > 0, check 4 horizontal neighbors for Water Level 0 (source)
let mut source_count = 0;
let neighbors = [(wx + 1, wy, wz), (wx - 1, wy, wz), (wx, wy, wz + 1), (wx, wy, wz - 1)];
for &(nx, ny, nz) in neighbors.iter() {
    if chunk_manager.get_block(nx, ny, nz) == BlockType::Water 
       && chunk_manager.get_fluid_level(nx, ny, nz) == 0 {
        source_count += 1;
    }
}
if source_count >= 2 {
    // Promote to source water!
    chunk_manager.set_block(wx, wy, wz, BlockType::Water);
    chunk_manager.set_fluid_level(wx, wy, wz, 0);
    chunk_manager.set_fluid_falling(wx, wy, wz, false);
}
```

- [ ] **Step 2: Add Water-Lava interaction solver**
Add a method `solve_interaction(chunk_manager, x, y, z)`:
```rust
fn handle_interaction(chunk_manager: &mut ChunkManager, wx: i32, wy: i32, wz: i32) -> bool {
    let block = chunk_manager.get_block(wx, wy, wz);
    let neighbors = [(wx+1, wy, wz), (wx-1, wy, wz), (wx, wy, wz+1), (wx, wy, wz-1), (wx, wy+1, wz), (wx, wy-1, wz)];
    
    for &(nx, ny, nz) in neighbors.iter() {
        let n_block = chunk_manager.get_block(nx, ny, nz);
        
        if block == BlockType::Water && n_block == BlockType::Lava {
            let l_level = chunk_manager.get_fluid_level(nx, ny, nz);
            if l_level == 0 {
                // Water hits lava source -> Obsidian
                chunk_manager.set_block(nx, ny, nz, BlockType::Obsidian);
            } else {
                // Water hits flowing lava -> Cobblestone
                chunk_manager.set_block(nx, ny, nz, BlockType::Cobblestone);
            }
            return true;
        } else if block == BlockType::Lava && n_block == BlockType::Water {
            let w_level = chunk_manager.get_fluid_level(nx, ny, nz);
            if w_level == 0 {
                // Lava hits water source -> Stone
                chunk_manager.set_block(nx, ny, nz, BlockType::Stone);
            } else {
                // Lava hits flowing water -> Cobblestone
                chunk_manager.set_block(nx, ny, nz, BlockType::Cobblestone);
            }
            return true;
        }
    }
    false
}
```
Run `handle_interaction` prior to resolving fluid movement updates.

- [ ] **Step 3: Commit**
```bash
git add src/fluid.rs
git commit -m "feat: add infinite water sources and water-lava interaction logic"
```

---

### Task 5: Adjust Mesh Generation to Render Flowing Fluid Heights

**Files:**
- Modify: [world.rs](file:///f:/Desktop/MC/src/world.rs:680-820)

- [ ] **Step 1: Modify `get_block_at` closure signature in `src/state.rs`**
Update chunk mesh generation closure in `src/state.rs` to return the fluid level and falling status as well:
```rust
// Change closure type to: Fn(i32, i32, i32) -> (BlockType, u8, u8, u8, bool)
```

- [ ] **Step 2: Adjust Top Vertex Y Coordinate inside `Chunk::generate_mesh`**
Inside `generate_mesh()`:
```rust
let is_fluid = block == BlockType::Water || block == BlockType::Lava;
let level = self.fluid_levels[x][y][z] & 0x07;
let falling = (self.fluid_levels[x][y][z] & 0x08) != 0;

let h = if is_fluid {
    if falling { 1.0 } else { (8 - level) as f32 / 8.0 * 0.9 }
} else {
    1.0
};
```
When generating vertices, adjust position's Y coordinate based on face orientation:
```rust
let mut vy = world_y as f32 + offset[1];
if is_fluid && offset[1] > 0.0 {
    // Adjust top vertices Y
    vy = world_y as f32 + h;
}
```

- [ ] **Step 3: Support culling between fluids**
Ensure culling allows water next to water (with level differences) or lava next to lava:
```rust
let should_render = if neighbor == BlockType::Air {
    true
} else if neighbor_props.render_type != RenderType::Opaque {
    !(block == BlockType::Water && neighbor == BlockType::Water)
} else {
    false
};
```

- [ ] **Step 4: Commit**
```bash
git add src/world.rs
git commit -m "feat: adjust fluid vertex height dynamically in mesh generation"
```

---

### Task 6: Fluid Render Pass & Shader Scrolling UV Animations

**Files:**
- Modify: [camera.rs](file:///f:/Desktop/MC/src/camera.rs:30-106)
- Modify: [state.rs](file:///f:/Desktop/MC/src/state.rs:130-185)
- Modify: [shader.wgsl](file:///f:/Desktop/MC/src/shader.wgsl)

- [ ] **Step 1: Pass total_time inside CameraUniform**
In `src/camera.rs`, add `total_time` to `CameraUniform`:
```rust
pub struct CameraUniform {
    pub view_proj: [[f32; 4]; 4],
    pub inv_view_proj: [[f32; 4]; 4],
    pub camera_pos: [f32; 4],
    pub sky_color_top: [f32; 4],
    pub sky_color_horizon: [f32; 4],
    pub sun_dir: [f32; 4],
    pub fog_start: f32,
    pub fog_end: f32,
    pub total_time: f32, // Replaces one padding float
    pub padding: [f32; 1],
}
```
Update `total_time` in `update_view_proj`:
```rust
self.total_time = total_time_elapsed; // Pass as parameter
```

- [ ] **Step 2: Scroll UV coordinates in `src/shader.wgsl`**
Modify the WGSL vertex shader to scroll texture coordinates:
```wgsl
// Inside vs_main:
var out_tex = model.tex_coords;
// If texture coordinates belong to Water (row 0, col 10) or Lava (row 2, col 15):
// Check row & col via divisions or add a flag
let is_water = model.tex_coords.x >= 10.0 * 0.0625 && model.tex_coords.x < 11.0 * 0.0625 
            && model.tex_coords.y >= 0.0 * 0.0625 && model.tex_coords.y < 1.0 * 0.0625;
let is_lava = model.tex_coords.x >= 15.0 * 0.0625 && model.tex_coords.x < 16.0 * 0.0625 
            && model.tex_coords.y >= 2.0 * 0.0625 && model.tex_coords.y < 3.0 * 0.0625;

if (is_water) {
    out_tex.y = out_tex.y + camera.total_time * 0.05;
} else if (is_lava) {
    out_tex.y = out_tex.y + camera.total_time * 0.01;
}
out.tex_coords = out_tex;
```

- [ ] **Step 3: Make Lava full-emissive in Mesh Generation**
In `src/world.rs` `generate_mesh`, if block is Lava, set `light_val` directly to max:
```rust
let light_val = if block == BlockType::Lava {
    15.0 + multiplier_code * 256.0
} else {
    (neighbor_sky as f32) + (neighbor_block as f32) * 16.0 + multiplier_code * 256.0
};
```

- [ ] **Step 4: Commit**
```bash
git add src/camera.rs src/state.rs src/shader.wgsl
git commit -m "feat: animate fluid texture coordinates via total_time uniform"
```

---

### Task 7: Player Swimming Physics & Buoyancy

**Files:**
- Modify: [physics.rs](file:///f:/Desktop/MC/src/physics.rs:50-97)
- Modify: [state.rs](file:///f:/Desktop/MC/src/state.rs:204-260)

- [ ] **Step 1: Check overlapping fluid in `PlayerPhysics::update`**
Implement check if player is inside Water or Lava in `src/physics.rs`:
```rust
let block_at_player = chunk_manager.get_block(self.position.x as i32, self.position.y as i32, self.position.z as i32);
let is_in_water = block_at_player == BlockType::Water;
let is_in_lava = block_at_player == BlockType::Lava;
```
If in water:
- Apply terminal velocity cap `self.velocity.y = self.velocity.y.max(-2.0);`
- Apply movement damping `self.velocity.x *= 0.6; self.velocity.z *= 0.6;`
- Swim up buoyancy: If space pressed, `self.velocity.y = 2.5;`

If in lava:
- Apply terminal velocity cap `self.velocity.y = self.velocity.y.max(-0.5);`
- Apply movement damping `self.velocity.x *= 0.3; self.velocity.z *= 0.3;`
- Swim up buoyancy: If space pressed, `self.velocity.y = 1.0;`

- [ ] **Step 2: Apply Lava burn damage**
In `State::update(dt)`, if player is inside lava (determined by eye or feet position), trigger fire damage:
```rust
if player_in_lava {
    self.player_state.take_damage(4.0 * dt, DamageSource::Mob); // Deal constant fire damage
}
```

- [ ] **Step 3: Commit**
```bash
git add src/physics.rs src/state.rs
git commit -m "feat: implement player buoyancy, swimming damping, and lava burning physics"
```

---

### Task 8: Head Underwater Visual Overlay and Fog

**Files:**
- Modify: [camera.rs](file:///f:/Desktop/MC/src/camera.rs:30-106)
- Modify: [shader.wgsl](file:///f:/Desktop/MC/src/shader.wgsl)

- [ ] **Step 1: Pass `is_underwater` flag in `CameraUniform`**
Add an `is_underwater` float flag (0.0 or 1.0) to `CameraUniform` padding slot.
Check if the block at head coordinates `(camera.position.x, camera.position.y + 0.1, camera.position.z)` (eye level) is `Water`:
```rust
let is_underwater = chunk_manager.get_block(cx, cy, cz) == BlockType::Water;
self.padding[0] = if is_underwater { 1.0 } else { 0.0 };
```

- [ ] **Step 2: Apply blue visual filter and thick fog underwater in shader**
In `src/shader.wgsl`, if `is_underwater` is active:
- Set `fog_start = 0.2` and `fog_end = 4.0`.
- Mix fog color with deep blue `[0.05, 0.15, 0.45, 1.0]` instead of sky color:
```wgsl
let is_underwater = camera.padding[0] > 0.5;
if (is_underwater) {
    // Interpolate fog aggressively
    let fog_factor = clamp((dist - 0.2) / (4.0 - 0.2), 0.0, 1.0);
    return mix(color, vec4<f32>(0.05, 0.15, 0.45, 1.0), fog_factor);
}
```

- [ ] **Step 3: Commit**
```bash
git add src/camera.rs src/shader.wgsl
git commit -m "feat: apply deep blue camera overlay and dense fog when underwater"
```

---

### Task 9: Oxygen Bar HUD & Drowning Damage

**Files:**
- Modify: [player.rs](file:///f:/Desktop/MC/src/player.rs:10-115)
- Modify: [state.rs](file:///f:/Desktop/MC/src/state.rs:130-260)

- [ ] **Step 1: Add Oxygen and Drowning state in `player.rs`**
Add fields in `PlayerState` struct:
```rust
pub oxygen: f32,          // 0.0 to 300.0
pub drowning_timer: f32,  // in seconds
```
Initialize `oxygen` to `300.0` and `drowning_timer` to `0.0`.
Add `DamageSource::Drowning` variant to `DamageSource`.
Update `PlayerState::update(dt)`:
- If underwater (passed as flag):
  - Deplete oxygen: `self.oxygen = (self.oxygen - dt * 20.0).max(0.0);`
  - If `self.oxygen == 0.0`:
    - `self.drowning_timer += dt;`
    - If `self.drowning_timer >= 1.0`:
      - `self.drowning_timer = 0.0;`
      - Trigger drowning damage: `take_damage(2.0, DamageSource::Drowning);`
- Else:
  - Restore oxygen rapidly: `self.oxygen = (self.oxygen + dt * 100.0).min(300.0);`
  - `self.drowning_timer = 0.0;`

- [ ] **Step 2: Draw bubble icon graphics inside Texture Atlas**
Add a bubble pattern in procedural texture atlas generation `src/texture.rs`. Draw a light blue 2D bubble at slot (15, 3).

- [ ] **Step 3: Render Oxygen bubbles in GUI slots**
In `state.rs`, if `self.player_state.oxygen < 300.0`:
- Render up to 10 bubble icons in HUD above the hunger bar slots.

- [ ] **Step 4: Write unit tests in `src/player.rs`**
Add tests to verify:
```rust
#[test]
fn test_player_drowning() {
    let mut state = PlayerState::new();
    assert_eq!(state.oxygen, 300.0);
    // Deplete oxygen underwater
    for _ in 0..15 {
        state.update_underwater(1.0, true); // update with underwater=true
    }
    assert_eq!(state.oxygen, 0.0);
    // Next second should trigger drowning damage
    let damage = state.update_underwater(1.0, true);
    assert_eq!(damage, Some((2.0, DamageSource::Drowning)));
}
```
Verify tests compile and pass.

- [ ] **Step 5: Commit**
```bash
git add src/player.rs src/state.rs
git commit -m "feat: implement oxygen bar hud and drowning damage state"
```
