# Multi-Chunk Support & Dynamic Loading Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement dynamic multi-chunk loading/unloading, cross-chunk face culling, per-chunk GPU buffers, render distance adjustments, and fix collision detection to support negative coordinates.

**Architecture:** We use a rate-limited, distance-based grid loader centered around the player's chunk coordinates. Mesh data generation queries neighboring chunks for face culling. Each chunk's GPU buffers are managed separately.

**Tech Stack:** Rust, wgpu, winit, noise (Perlin), glam

---

### Task 1: Update Chunk Coord & Noise Generation in world.rs

**Files:**
- Modify: `src/world.rs`

- [ ] **Step 1: Modify `Chunk` struct to add coordinates**
  Change `Chunk` definition to include `chunk_x` and `chunk_z`:
  ```rust
  pub struct Chunk {
      pub chunk_x: i32,
      pub chunk_z: i32,
      pub blocks: [[[BlockType; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH],
  }
  ```

- [ ] **Step 2: Update `Chunk::new` to accept offsets and generate offset Perlin noise**
  Change `Chunk::new` signature and its terrain generation loops to offset inputs:
  ```rust
  impl Chunk {
      pub fn new(chunk_x: i32, chunk_z: i32) -> Self {
          let mut blocks = [[[BlockType::Air; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH];
          let perlin = noise::Perlin::new(12345);

          for x in 0..CHUNK_WIDTH {
              for z in 0..CHUNK_DEPTH {
                  let world_x = chunk_x * (CHUNK_WIDTH as i32) + x as i32;
                  let world_z = chunk_z * (CHUNK_DEPTH as i32) + z as i32;
                  let noise_val = perlin.get([world_x as f64 * 0.08, world_z as f64 * 0.08]);
                  let height = (64.0 + noise_val * 10.0) as usize;

                  for y in 0..CHUNK_HEIGHT {
                      if y < height - 4 {
                          blocks[x][y][z] = BlockType::Stone;
                      } else if y < height {
                          blocks[x][y][z] = BlockType::Dirt;
                      } else if y == height {
                          blocks[x][y][z] = BlockType::Grass;
                      } else {
                          blocks[x][y][z] = BlockType::Air;
                      }
                  }
              }
          }
          Self { chunk_x, chunk_z, blocks }
      }
  }
  ```

- [ ] **Step 3: Run cargo check to verify compiles**
  Run: `cargo check`
  Expected: Compile errors in `state.rs` because of `Chunk::new()` arguments and missing coordinate mapping. (This is expected before Task 2/5).

---

### Task 2: Create Chunk Manager

**Files:**
- Create: `src/chunk_manager.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Create `src/chunk_manager.rs`**
  Implement coordinate conversion and basic getters/setters:
  ```rust
  use std::collections::HashMap;
  use crate::world::{Chunk, BlockType, CHUNK_WIDTH, CHUNK_HEIGHT, CHUNK_DEPTH};

  pub struct ChunkManager {
      pub chunks: HashMap<(i32, i32), Chunk>,
      pub render_distance: i32,
  }

  impl ChunkManager {
      pub fn new(render_distance: i32) -> Self {
          Self {
              chunks: HashMap::new(),
              render_distance,
          }
      }

      pub fn world_to_local(&self, wx: i32, wy: i32, wz: i32) -> Option<((i32, i32), (usize, usize, usize))> {
          if wy < 0 || wy >= CHUNK_HEIGHT as i32 {
              return None;
          }
          let cx = wx.div_euclid(CHUNK_WIDTH as i32);
          let cz = wz.div_euclid(CHUNK_DEPTH as i32);
          let bx = wx.rem_euclid(CHUNK_WIDTH as i32) as usize;
          let bz = wz.rem_euclid(CHUNK_DEPTH as i32) as usize;
          let by = wy as usize;
          Some(((cx, cz), (bx, by, bz)))
      }

      pub fn get_block(&self, wx: i32, wy: i32, wz: i32) -> BlockType {
          if let Some(((cx, cz), (bx, by, bz))) = self.world_to_local(wx, wy, wz) {
              if let Some(chunk) = self.chunks.get(&(cx, cz)) {
                  return chunk.blocks[bx][by][bz];
              }
          }
          BlockType::Air
      }

      pub fn set_block(&mut self, wx: i32, wy: i32, wz: i32, block: BlockType) {
          if let Some(((cx, cz), (bx, by, bz))) = self.world_to_local(wx, wy, wz) {
              if let Some(chunk) = self.chunks.get_mut(&(cx, cz)) {
                  chunk.blocks[bx][by][bz] = block;
              }
          }
      }
  }
  ```

- [ ] **Step 2: Add `chunk_manager` module to `src/main.rs`**
  Add `mod chunk_manager;` to `src/main.rs`.

- [ ] **Step 3: Run cargo check**
  Run: `cargo check`

---

### Task 3: Refactor Physics and Raycasting for ChunkManager

**Files:**
- Modify: `src/physics.rs`
- Modify: `src/interaction.rs`

- [ ] **Step 1: Modify `PlayerPhysics::update` and `resolve_collisions` in `src/physics.rs`**
  Change `chunk: &Chunk` parameters to `chunk_manager: &ChunkManager`.
  Remove `.max(0)` checks and use Y-clamp only.
  ```rust
  // src/physics.rs
  use crate::chunk_manager::ChunkManager;

  // In PlayerPhysics::update:
  pub fn update(&mut self, dt: f32, chunk_manager: &ChunkManager, movement_input: Vec3) {
      ...
      self.position.x += self.velocity.x * dt;
      self.resolve_collisions(chunk_manager, 0);

      self.position.z += self.velocity.z * dt;
      self.resolve_collisions(chunk_manager, 2);

      self.position.y += self.velocity.y * dt;
      self.on_ground = false;
      self.resolve_collisions(chunk_manager, 1);
  }

  // In PlayerPhysics::resolve_collisions:
  fn resolve_collisions(&mut self, chunk_manager: &ChunkManager, axis: usize) {
      let player_aabb = self.get_aabb();

      let min_x = player_aabb.min.x.floor() as i32;
      let max_x = player_aabb.max.x.floor() as i32;
      let min_y = (player_aabb.min.y.floor() as i32).clamp(0, crate::world::CHUNK_HEIGHT as i32 - 1);
      let max_y = (player_aabb.max.y.floor() as i32).clamp(0, crate::world::CHUNK_HEIGHT as i32 - 1);
      let min_z = player_aabb.min.z.floor() as i32;
      let max_z = player_aabb.max.z.floor() as i32;

      for x in min_x..=max_x {
          for y in min_y..=max_y {
              for z in min_z..=max_z {
                  let block = chunk_manager.get_block(x, y, z);
                  if block != crate::world::BlockType::Air {
                      ...
                  }
              }
          }
      }
  }
  ```

- [ ] **Step 2: Modify `raycast` in `src/interaction.rs`**
  Replace `chunk: &Chunk` with `chunk_manager: &ChunkManager`:
  ```rust
  use crate::chunk_manager::ChunkManager;
  
  pub fn raycast(origin: Vec3, direction: Vec3, max_dist: f32, chunk_manager: &ChunkManager) -> Option<RaycastResult> {
      ...
      while t < max_dist {
          let block = chunk_manager.get_block(x, y, z);
          ...
      }
      ...
  }
  ```

- [ ] **Step 3: Update unit tests in `src/physics.rs` and `src/interaction.rs`**
  Update tests to create a dummy `ChunkManager` and mock the chunk data.
  For `physics.rs` tests:
  ```rust
  #[test]
  fn test_aabb_intersection() {
      // Keep existing tests, they don't depend on Chunk
  }
  ```
  For `interaction.rs` tests:
  ```rust
  #[test]
  fn test_raycast_air() {
      let mut chunk_manager = ChunkManager::new(8);
      chunk_manager.chunks.insert((0, 0), Chunk::new(0, 0));
      let hit = raycast(Vec3::new(8.0, 70.0, 8.0), Vec3::new(0.0, 1.0, 0.0), 10.0, &chunk_manager);
      assert!(hit.is_none());
  }

  #[test]
  fn test_raycast_hit() {
      let mut chunk_manager = ChunkManager::new(8);
      let mut chunk = Chunk::new(0, 0);
      chunk.blocks[8][72][8] = BlockType::Stone;
      chunk_manager.chunks.insert((0, 0), chunk);

      let hit = raycast(Vec3::new(8.5, 70.5, 8.5), Vec3::new(0.0, 1.0, 0.0), 5.0, &chunk_manager);
      assert!(hit.is_some());
      let res = hit.unwrap();
      assert_eq!(res.block_pos, Vec3::new(8.0, 72.0, 8.0));
  }
  ```

- [ ] **Step 4: Run cargo test to verify tests compile and pass**
  Run: `cargo test`
  Expected: Test passes successfully.

---

### Task 4: Implement Cross-Chunk Face Culling in generate_mesh

**Files:**
- Modify: `src/world.rs`

- [ ] **Step 1: Modify `Chunk::generate_mesh` in `src/world.rs`**
  Accept a block query callback and calculate vertex positions as world coordinates:
  ```rust
      pub fn generate_mesh<F>(&self, get_block_at: F) -> (Vec<Vertex>, Vec<u32>)
      where
          F: Fn(i32, i32, i32) -> BlockType
      {
          ...
          for x in 0..CHUNK_WIDTH {
              for y in 0..CHUNK_HEIGHT {
                  for z in 0..CHUNK_DEPTH {
                      let block = self.blocks[x][y][z];
                      if block == BlockType::Air {
                          continue;
                      }

                      let world_x = self.chunk_x * CHUNK_WIDTH as i32 + x as i32;
                      let world_y = y as i32;
                      let world_z = self.chunk_z * CHUNK_DEPTH as i32 + z as i32;

                      for (face_idx, (normal, corner_data)) in faces.iter().enumerate() {
                          let nx = world_x + normal[0] as i32;
                          let ny = world_y + normal[1] as i32;
                          let nz = world_z + normal[2] as i32;

                          let neighbor = get_block_at(nx, ny, nz);
                          if neighbor == BlockType::Air {
                              let start_idx = vertices.len() as u32;
                              // Match atlas UV tex_idx...
                              // Push vertices using:
                              // position: [world_x as f32 + offset[0], world_y as f32 + offset[1], world_z as f32 + offset[2]]
                              ...
                          }
                      }
                  }
              }
          }
          (vertices, indices)
      }
  ```

---

### Task 5: Refactor State to use ChunkManager & ChunkMesh Map

**Files:**
- Modify: `src/state.rs`

- [ ] **Step 1: Define `ChunkMesh` struct at the top of `src/state.rs`**
  ```rust
  pub struct ChunkMesh {
      pub vertex_buffer: wgpu::Buffer,
      pub index_buffer: wgpu::Buffer,
      pub num_indices: u32,
      pub dirty: bool,
  }
  ```

- [ ] **Step 2: Update `State` struct fields**
  Replace `chunk: Chunk` with `pub chunk_manager: ChunkManager`.
  Replace single `vertex_buffer`, `index_buffer`, `num_indices` with `pub chunk_meshes: std::collections::HashMap<(i32, i32), ChunkMesh>`.
  Add `render_distance` configurations if needed.

- [ ] **Step 3: Modify `State::new()` initialization**
  Load setting `render_distance` from `settings.txt` (defaulting to 8 if not specified).
  Initialize `ChunkManager` with `render_distance`.
  Synchronously load and generate meshes for all chunks in the initial render range around player spawn (0, 0):
  ```rust
          // In State::new():
          let render_distance = settings.render_distance;
          let mut chunk_manager = ChunkManager::new(render_distance);
          let mut chunk_meshes = std::collections::HashMap::new();

          // Synchronously load spawn region
          for cx in -render_distance..=render_distance {
              for cz in -render_distance..=render_distance {
                  let chunk = Chunk::new(cx, cz);
                  chunk_manager.chunks.insert((cx, cz), chunk);
              }
          }

          // Build meshes for spawn region chunks
          for cx in -render_distance..=render_distance {
              for cz in -render_distance..=render_distance {
                  let chunk = &chunk_manager.chunks[&(cx, cz)];
                  let (vertices, indices) = chunk.generate_mesh(|wx, wy, wz| {
                      // Note: Borrow checker workaround might require local helper or temporary reference
                      // Since we are building all chunks, we can query chunk_manager
                      chunk_manager.get_block(wx, wy, wz)
                  });

                  let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                      label: Some("Initial Chunk Vertex Buffer"),
                      contents: bytemuck::cast_slice(&vertices),
                      usage: wgpu::BufferUsages::VERTEX,
                  });
                  let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                      label: Some("Initial Chunk Index Buffer"),
                      contents: bytemuck::cast_slice(&indices),
                      usage: wgpu::BufferUsages::INDEX,
                  });

                  chunk_meshes.insert((cx, cz), ChunkMesh {
                      vertex_buffer,
                      index_buffer,
                      num_indices: indices.len() as u32,
                      dirty: false,
                  });
              }
          }
  ```

- [ ] **Step 4: Update `State::update` and `State::handle_click` to use `chunk_manager`**
  - In `State::update`: update player physics using `&self.chunk_manager`.
  - In `State::handle_click`: update hit logic. When block is placed/broken, update via `self.chunk_manager.set_block`.
    Then mark the hit chunk and its relevant neighbors dirty:
    ```rust
                let cx = target.x.div_euclid(16.0) as i32;
                let cz = target.z.div_euclid(16.0) as i32;
                let lx = target.x.rem_euclid(16.0) as usize;
                let lz = target.z.rem_euclid(16.0) as usize;

                if let Some(mesh) = self.chunk_meshes.get_mut(&(cx, cz)) {
                    mesh.dirty = true;
                }
                if lx == 0 {
                    if let Some(mesh) = self.chunk_meshes.get_mut(&(cx - 1, cz)) { mesh.dirty = true; }
                }
                if lx == 15 {
                    if let Some(mesh) = self.chunk_meshes.get_mut(&(cx + 1, cz)) { mesh.dirty = true; }
                }
                if lz == 0 {
                    if let Some(mesh) = self.chunk_meshes.get_mut(&(cx, cz - 1)) { mesh.dirty = true; }
                }
                if lz == 15 {
                    if let Some(mesh) = self.chunk_meshes.get_mut(&(cx, cz + 1)) { mesh.dirty = true; }
                }
    ```

- [ ] **Step 5: Modify `State::render()` drawing loop**
  Draw all loaded `chunk_meshes`.
  ```rust
              render_pass.set_pipeline(&self.render_pipeline);
              render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
              for mesh in self.chunk_meshes.values() {
                  if mesh.num_indices > 0 {
                      render_pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                      render_pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                      render_pass.draw_indexed(0..mesh.num_indices, 0, 0..1);
                  }
              }
  ```

---

### Task 6: Implement Rate-Limited Chunk Loading Queue

**Files:**
- Modify: `src/state.rs`

- [ ] **Step 1: Implement `update_chunks()` method on `State`**
  ```rust
  impl State {
      pub fn update_chunks(&mut self) {
          let player_pos = self.player_physics.position;
          let px = (player_pos.x / 16.0).floor() as i32;
          let pz = (player_pos.z / 16.0).floor() as i32;
          let r = self.chunk_manager.render_distance;

          // 1. Unload out-of-bounds chunks
          self.chunk_manager.chunks.retain(|&(cx, cz), _| {
              (cx - px).abs() <= r && (cz - pz).abs() <= r
          });
          self.chunk_meshes.retain(|&(cx, cz), _| {
              (cx - px).abs() <= r && (cz - pz).abs() <= r
          });

          // 2. Queue missing chunks
          let mut load_queue = Vec::new();
          for dx in -r..=r {
              for dz in -r..=r {
                  let cx = px + dx;
                  let cz = pz + dz;
                  if !self.chunk_manager.chunks.contains_key(&(cx, cz)) {
                      load_queue.push((cx, cz));
                  }
              }
          }

          load_queue.sort_by_key(|&(cx, cz)| {
              let dx = cx - px;
              let dz = cz - pz;
              dx * dx + dz * dz
          });

          // 3. Load 1 chunk per frame
          if let Some(&(cx, cz)) = load_queue.first() {
              let chunk = Chunk::new(cx, cz);
              self.chunk_manager.chunks.insert((cx, cz), chunk);

              // Mark neighbors dirty
              for &(ncx, ncz) in &[(cx - 1, cz), (cx + 1, cz), (cx, cz - 1), (cx, cz + 1)] {
                  if let Some(mesh) = self.chunk_meshes.get_mut(&(ncx, ncz)) {
                      mesh.dirty = true;
                  }
              }
          }

          // 4. Rebuild at most 2 dirty meshes per frame
          let mut to_rebuild = Vec::new();
          for (&(cx, cz), _) in &self.chunk_manager.chunks {
              let needs_mesh = !self.chunk_meshes.contains_key(&(cx, cz));
              let is_dirty = self.chunk_meshes.get(&(cx, cz)).map(|m| m.dirty).unwrap_or(false);
              if needs_mesh || is_dirty {
                  to_rebuild.push((cx, cz));
              }
          }

          // Limit to 2 per frame
          for (cx, cz) in to_rebuild.into_iter().take(2) {
              let chunk = &self.chunk_manager.chunks[&(cx, cz)];
              // Workaround borrow checker by creating a tiny helper closure
              let cm = &self.chunk_manager;
              let (vertices, indices) = chunk.generate_mesh(|wx, wy, wz| {
                  cm.get_block(wx, wy, wz)
              });

              let vertex_buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                  label: Some("Chunk Vertex Buffer"),
                  contents: bytemuck::cast_slice(&vertices),
                  usage: wgpu::BufferUsages::VERTEX,
              });
              let index_buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                  label: Some("Chunk Index Buffer"),
                  contents: bytemuck::cast_slice(&indices),
                  usage: wgpu::BufferUsages::INDEX,
              });

              self.chunk_meshes.insert((cx, cz), ChunkMesh {
                  vertex_buffer,
                  index_buffer,
                  num_indices: indices.len() as u32,
                  dirty: false,
              });
          }
      }
  }
  ```

- [ ] **Step 2: Add call to `update_chunks()` inside `State::update`**
  ```rust
  pub fn update(&mut self, dt: f32) {
      if self.is_paused {
          return;
      }
      ...
      self.player_physics.update(dt, &self.chunk_manager, movement);
      self.update_chunks();
      ...
  }
  ```

---

### Task 7: Render Distance Settings persistence & Pause Menu UI

**Files:**
- Modify: `src/state.rs`

- [ ] **Step 1: Update `GameSettings` struct definition and persistence logic**
  Add `render_distance: i32` (default 8) to `GameSettings` serialization.
  ```rust
  pub struct GameSettings {
      pub fov: f32,
      pub sensitivity: f32,
      pub render_distance: i32,
  }
  ```
  Update `load()` and `save()` to support `render_distance`.

- [ ] **Step 2: Rearrange Pause Menu UI buttons**
  Adjust Y bounds in `handle_menu_click` and `render` to draw the new RENDER DISTANCE button.
  ```rust
  // UI coordinates:
  // Resume: Y [0.24, 0.34]
  // FOV: Y [0.10, 0.20]
  // Sensitivity: Y [-0.04, 0.06]
  // Render Distance: Y [-0.18, -0.08]
  // Quit: Y [-0.32, -0.22]
  ```

- [ ] **Step 3: Handle click events for Render Distance**
  Inside `State::handle_menu_click`:
  ```rust
              else if x >= -0.3 && x <= 0.3 && y >= -0.18 && y <= -0.08 {
                  if x < 0.0 {
                      self.chunk_manager.render_distance = (self.chunk_manager.render_distance - 1).max(2);
                  } else {
                      self.chunk_manager.render_distance = (self.chunk_manager.render_distance + 1).min(16);
                  }
                  self.save_settings();
              }
  ```
  Ensure settings saves the value correctly.

- [ ] **Step 4: Draw "RENDER DISTANCE" text**
  In `State::render()`, draw the button and display the value:
  ```rust
              let rd_text = format!("RENDER DISTANCE < {} >", self.chunk_manager.render_distance);
              draw_centered_text(&rd_text, -0.14, 0.02, 0.04, 0.008, text_color, &mut ui_line_vertices);
  ```

---

### Task 8: Manual Verification

**Files:**
- None

- [ ] **Step 1: Run the game**
  Command: `cargo run --release`
  Expected: Minecraft clone opens smoothly in a window, spawn terrain is generated, player is standing on grass chunk.

- [ ] **Step 2: Walk around**
  Walk in directions, check that chunks load ahead and unload behind. Watch output logs/console to verify no panic occurs.

- [ ] **Step 3: Pause and modify render distance**
  Press ESC. Verify "RENDER DISTANCE" button shows up. Adjust the slider. Verify that closing menu adjusts the loading distance.
