# Block Breaking Animation & Particle System Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the 10-stage block breaking animation overlay (using files and multiply-blend), a lightweight particle system (block break debris, footstep dust, torch smoke), and dropped item entities with bobbing & rotating animations.

**Architecture:** Create a new `src/particles.rs` module. Hook particle lifetime updates and billboard vertex compilation into `src/state.rs::State`. Extend `src/entity.rs` and `src/mob_renderer.rs` to support `EntityType::DroppedItem` physics, floating (sine bobbing), and rotation rendering. Update `src/shader.wgsl` and `src/state.rs` pipelines for crack blending.

**Tech Stack:** Rust, wgpu, winit, glam, image

---

### Task 1: Update Texture Atlas and Shader for Mining Cracks

**Files:**
- Modify: `src/texture.rs`
- Modify: `src/state.rs`
- Modify: `src/shader.wgsl`

- [ ] **Step 1: Update `TextureAtlas::new_procedural` to load PNG textures**
  In `src/texture.rs`, attempt to load 10 files from `assets/textures/destroy_stages/destroy_stage_0.png` through `destroy_stage_9.png`.
  If they exist, stitch them onto Row 15 columns 0..9.
  If loading fails, fall back to drawing using the existing `draw_crack_pattern`.

- [ ] **Step 2: Update WGPU pipeline blend state for cracking overlay**
  In `src/state.rs` (around the creation of `trans_pipeline`), ensure the render pipeline uses multiply blending (color src: `Dst`, dst: `Zero`, op: `Add`) for transparent geometries, or create a dedicated `crack_pipeline` to avoid affecting water transparent rendering:
  ```rust
  let crack_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
      // ... similar to trans_pipeline, but with BlendState::MULTIPLY-equivalent
  });
  ```

- [ ] **Step 3: Run `cargo check` to verify pipeline setup**
  Run: `cargo check`
  Expected: Success.

---

### Task 2: Create Lightweight Particle Module

**Files:**
- Create: `src/particles.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Declare `particles` module in `src/main.rs`**
  Add `pub mod particles;` to `src/main.rs`.

- [ ] **Step 2: Implement structures in `src/particles.rs`**
  Write `Particle`, `ParticleSystem` structs, and the corresponding implementation block:
  - `ParticleSystem::new()`
  - `ParticleSystem::spawn(&mut self, pos: Vec3, vel: Vec3, size: f32, lifetime: f32, tex_coords: [f32; 4], gravity: f32)`
  - `ParticleSystem::update(&mut self, dt: f32)`

- [ ] **Step 3: Implement Particle billboard vertex compilation**
  Inside `src/particles.rs`, implement:
  ```rust
  pub fn compile_mesh(
      &self,
      device: &wgpu::Device,
      queue: &wgpu::Queue,
      cam_right: Vec3,
      cam_up: Vec3,
      vertex_buffer: &wgpu::Buffer,
      index_buffer: &wgpu::Buffer,
  ) -> Option<u32> // Returns number of indices to draw
  ```
  This creates billboard quads from active particles and writes them to the dynamic vertex buffer.

- [ ] **Step 4: Run `cargo check` to verify the module compiles**
  Run: `cargo check`
  Expected: Success.

---

### Task 3: Integrate Particle System into State Lifecycle

**Files:**
- Modify: `src/state.rs`

- [ ] **Step 1: Add `ParticleSystem` and GPU buffers to `State` struct**
  In `src/state.rs`, add `pub particles: crate::particles::ParticleSystem` and dynamic buffers to `State`:
  ```rust
  particle_vertex_buffer: wgpu::Buffer,
  particle_index_buffer: wgpu::Buffer,
  ```
  In `State::new`, initialize the particle buffers (preallocating size for up to 4096 vertices/6144 indices).

- [ ] **Step 2: Update particle system in `State::update`**
  In `State::update`, call:
  ```rust
  self.particles.update(dt);
  ```

- [ ] **Step 3: Compile and render particles in `State::render`**
  In `State::render`, compile the particle mesh:
  - Obtain camera right and up vectors from camera yaw/pitch:
    ```rust
    let cam_right = glam::Vec3::new(-self.camera.yaw.sin(), 0.0, self.camera.yaw.cos()).normalize_or_zero();
    let cam_up = glam::Vec3::new(
        -self.camera.yaw.cos() * self.camera.pitch.sin(),
        self.camera.pitch.cos(),
        -self.camera.yaw.sin() * self.camera.pitch.sin(),
    ).normalize_or_zero();
    ```
  - Call `compile_mesh` to write into the vertex buffer.
  - Draw particles in the translucent pass using `trans_pipeline` (to support alpha transparent particles).

- [ ] **Step 4: Run `cargo check`**
  Run: `cargo check`

---

### Task 4: Implement Debris, Footsteps, and Torch Smoke Emitters

**Files:**
- Modify: `src/state.rs`
- Modify: `src/particles.rs`

- [ ] **Step 1: Write debris spawning logic in `State::break_block`**
  In `State::break_block`, when a block is successfully mined, call a helper in `particles` to spawn 15-25 small particles.
  Map their UVs to random sub-rect coordinates within the broken block's texture tile coordinates (e.g. Grass Top, Oak Log, Cobblestone).

- [ ] **Step 2: Implement Footstep particles on walking**
  In `State::update`, track player distance walked on ground. Every 1.5 meters, spawn 4-8 particles at the feet level, querying the block type directly below the player's position (`player_physics.position.y - 0.05`). Use that block's texture sub-rects.

- [ ] **Step 3: Implement Torch smoke particles**
  In `State::update`, traverse active chunks for Torch blocks. Spawn a slowly rising smoke particle at a small random interval at `torch_pos + Vec3::new(0.5, 0.6, 0.5)`. The smoke particle rises at `0.8` m/s and decreases in size over time.

- [ ] **Step 4: Run `cargo check`**
  Run: `cargo check`

---

### Task 5: Implement Dropped Item Physics, Bobbing, and Collection

**Files:**
- Modify: `src/entity.rs`
- Modify: `src/mob_renderer.rs`
- Modify: `src/state.rs`

- [ ] **Step 1: Add `EntityType::DroppedItem` to `src/entity.rs`**
  Add `DroppedItem` to the `EntityType` enum and implement its default size (e.g. `0.25, 0.25, 0.25`) in `Entity::new`.
  Include a field `pub dropped_item: Option<Item>` to store which item is contained.

- [ ] **Step 2: Implement item spawning on block break**
  In `State::break_block`, instead of immediately adding the mined item to the inventory, spawn a `DroppedItem` entity in the world at the mined block center with a slight random initial upward velocity.

- [ ] **Step 3: Implement dropped item collection logic**
  In `State::update`, check the distance from the player to all `DroppedItem` entities. If the distance is less than `1.5` meters:
  - Add the item to player inventory (or ignore if inventory is full).
  - Despawn/remove the entity.

- [ ] **Step 4: Implement bobbing and rotating rendering in `src/mob_renderer.rs`**
  In `render_mobs`, handle `EntityType::DroppedItem`:
  - Calculate `yaw = time * 2.0` (smooth continuous rotation).
  - Calculate vertical offset `y_offset = (time * 3.0).sin() * 0.1` (sinusoidal bobbing).
  - Build and draw a mini 3D block cube or item model using `add_cuboid` at the bobbing position.

- [ ] **Step 5: Run `cargo check`**
  Run: `cargo check`

---

### Task 6: Add Particle and Dropped Item Unit Tests

**Files:**
- Modify: `src/particles.rs`
- Modify: `src/entity.rs`

- [ ] **Step 1: Write particle system test**
  Add a unit test verifying:
  - Particle updates (movement, gravity, expiration).
  - Debris count matching active state.

- [ ] **Step 2: Write dropped item collection test**
  Add a unit test verifying:
  - DroppedItem entities update physics, fall to the ground, and are collected when the player is near, adding the item stack to the player inventory.

- [ ] **Step 3: Run all cargo tests to verify correctness**
  Run: `cargo test`
  Expected: PASS.
