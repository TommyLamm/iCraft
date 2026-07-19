# Sprint and Sneak Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement sprinting (Ctrl key / double-click W to run faster at 1.3x speed, deplete hunger faster, expand FOV) and sneaking (Shift key to crouch at 0.3x speed, drop camera height, shrink hitbox to 1.5 blocks, and prevent falling off edges).

**Architecture:** Extend input bindings in `src/app.rs` and `src/state.rs`. Update the movement update system in `src/physics.rs` to process sprint and sneak speed factors, implement edge-fall prevention via foot-block projection, and apply dynamic FOV interpolation in `src/state.rs`.

**Tech Stack:** Rust, wgpu, winit, glam

---

### Task 1: Update KeyState and Keyboard Input Binding

**Files:**
- Modify: `src/state.rs`
- Modify: `src/app.rs`

- [ ] **Step 1: Add `ctrl` and `shift` fields to `KeyState` in `src/state.rs`**
  Modify `KeyState` definition:
  ```rust
  #[derive(Default)]
  pub struct KeyState {
      pub w: bool,
      pub a: bool,
      pub s: bool,
      pub d: bool,
      pub space: bool,
      pub t: bool,
      pub ctrl: bool,  // Added
      pub shift: bool, // Added
  }
  ```

- [ ] **Step 2: Bind Left Ctrl and Left Shift in `src/app.rs`**
  Add matches under `WindowEvent::KeyboardInput` in `src/app.rs`:
  ```rust
  winit::keyboard::PhysicalKey::Code(winit::keyboard::KeyCode::ControlLeft) => {
      state.keys.ctrl = pressed;
  }
  winit::keyboard::PhysicalKey::Code(winit::keyboard::KeyCode::ShiftLeft) => {
      state.keys.shift = pressed;
  }
  ```

- [ ] **Step 3: Run cargo check to verify binding compilation**
  Run: `cargo check`
  Expected: Success.

---

### Task 2: Implement Sprinting State Trigger and FOV Interpolation

**Files:**
- Modify: `src/state.rs`

- [ ] **Step 1: Add sprint variables to `State` struct in `src/state.rs`**
  Add the following fields to `State`:
  ```rust
  pub is_sprinting: bool,
  pub base_fov: f32,
  pub w_click_timer: f32, // For double click W
  pub last_w_pressed: bool,
  ```
  In `State::new`, initialize them:
  ```rust
  let base_fov = camera.fov;
  // inside State::new return struct:
  is_sprinting: false,
  base_fov,
  w_click_timer: 0.0,
  last_w_pressed: false,
  ```

- [ ] **Step 2: Add trigger & cancellation state logic inside `State::update`**
  Add sprinting checks inside `State::update` before resetting keys or applying physics:
  ```rust
  // Double click W logic
  if self.keys.w && !self.last_w_pressed {
      if self.w_click_timer > 0.0 && self.player_state.hunger > 6.0 {
          self.is_sprinting = true;
      }
      self.w_click_timer = 0.3; // 0.3 seconds window
  }
  self.last_w_pressed = self.keys.w;
  if self.w_click_timer > 0.0 {
      self.w_click_timer -= dt;
  }

  // Ctrl key sprint check
  if self.keys.ctrl && self.keys.w && self.player_state.hunger > 6.0 {
      self.is_sprinting = true;
  }

  // Cancel sprinting conditions
  if !self.keys.w || self.keys.shift || self.player_state.hunger <= 6.0 {
      self.is_sprinting = false;
  }

  // Cancel if player collides with a wall but has movement inputs
  if self.is_sprinting && (self.player_physics.velocity.x.abs() < 0.01 && self.player_physics.velocity.z.abs() < 0.01) && (self.keys.w || self.keys.a || self.keys.s || self.keys.d) {
      self.is_sprinting = false;
  }
  ```

- [ ] **Step 3: Interpolate FOV smoothly inside `State::update`**
  Add the smooth zoom logic:
  ```rust
  let target_fov = if self.is_sprinting {
      self.base_fov * 1.12
  } else {
      self.base_fov
  };
  self.camera.fov = self.camera.fov + (target_fov - self.camera.fov) * dt * 10.0;
  ```

- [ ] **Step 4: Consume more hunger when sprinting**
  Add inside `State::update` if sprinting and moving:
  ```rust
  if self.is_sprinting && (self.keys.w || self.keys.a || self.keys.s || self.keys.d) {
      self.player_state.add_exhaustion(dt * 0.15);
  }
  ```

- [ ] **Step 5: Run cargo check**
  Run: `cargo check`

---

### Task 3: Implement PlayerPhysics Speed Modifications & Sneaking Hitbox

**Files:**
- Modify: `src/physics.rs`
- Modify: `src/state.rs`

- [ ] **Step 1: Modify `PlayerPhysics::update` parameters**
  Update signature in `src/physics.rs`:
  ```rust
  pub fn update(
      &mut self,
      dt: f32,
      chunk_manager: &ChunkManager,
      movement_input: Vec3,
      is_sneaking: bool,
      is_sprinting: bool,
  ) -> f32
  ```

- [ ] **Step 2: Adjust movement speed and hitbox inside `PlayerPhysics::update`**
  Modify size and speed based on sprint/sneak states:
  ```rust
  // Hitbox size adjustment
  if is_sneaking {
      self.size.y = 1.5;
  } else {
      self.size.y = 1.8;
  }

  // Speed factor modifiers
  let mut speed = 8.0;
  if is_sprinting {
      speed *= 1.3;
  } else if is_sneaking {
      speed *= 0.3;
  }

  if is_in_water {
      speed *= 0.6;
  } else if is_in_lava {
      speed *= 0.3;
  }
  self.velocity.x = movement_input.x * speed;
  self.velocity.z = movement_input.z * speed;
  ```

- [ ] **Step 3: Update camera height and update call in `src/state.rs`**
  Modify `State::update` to pass `keys.shift` and `self.is_sprinting` to `player_physics.update`:
  ```rust
  let fall_damage = self.player_physics.update(
      dt,
      &self.chunk_manager,
      movement_input,
      self.keys.shift,
      self.is_sprinting,
  );
  ```
  Adjust the camera position sync under `State::update`:
  ```rust
  let eye_height = if self.keys.shift {
      1.42
  } else {
      1.62
  };
  self.camera.position = self.player_physics.position + glam::Vec3::new(0.0, eye_height, 0.0);
  ```

- [ ] **Step 4: Run cargo check**
  Run: `cargo check`
  Expected: Error about unit tests in `src/physics.rs` calling `update` with mismatched parameter count.

- [ ] **Step 5: Fix inline unit tests in `src/physics.rs` and `src/player.rs` if any**
  Locate any physics `update` calls in tests and supply `false, false` for sneaking/sprinting.

---

### Task 4: Implement Sneaking Edge-Fall Prevention (Edge Guard)

**Files:**
- Modify: `src/physics.rs`

- [ ] **Step 1: Write `is_block_below` helper method on `PlayerPhysics`**
  Add the method inside `impl PlayerPhysics` in `src/physics.rs`:
  ```rust
  pub fn is_block_below(&self, chunk_manager: &ChunkManager) -> bool {
      let mut check_aabb = self.get_aabb();
      check_aabb.min.y -= 0.05;
      check_aabb.max.y = self.position.y;

      let min_x = check_aabb.min.x.floor() as i32;
      let max_x = check_aabb.max.x.floor() as i32;
      let min_y = (check_aabb.min.y.floor() as i32).clamp(0, crate::world::CHUNK_HEIGHT as i32 - 1);
      let max_y = (check_aabb.max.y.floor() as i32).clamp(0, crate::world::CHUNK_HEIGHT as i32 - 1);
      let min_z = check_aabb.min.z.floor() as i32;
      let max_z = check_aabb.max.z.floor() as i32;

      for x in min_x..=max_x {
          for y in min_y..=max_y {
              for z in min_z..=max_z {
                  let block = chunk_manager.get_block(x, y, z);
                  if block.properties().is_solid {
                      let block_aabb = AABB::new(
                          Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5),
                          Vec3::ONE,
                      );
                      if check_aabb.intersects(&block_aabb) {
                          return true;
                      }
                  }
              }
          }
      }
      false
  }
  ```

- [ ] **Step 2: Add edge-guard checks in `PlayerPhysics::update` X and Z displacement loops**
  Modify X and Z translations in `src/physics.rs`:
  ```rust
  // 3. 沿 X 軸位移並處理碰撞
  let old_x = self.position.x;
  self.position.x += self.velocity.x * dt;
  self.resolve_collisions(chunk_manager, 0);
  if is_sneaking && self.on_ground {
      if !self.is_block_below(chunk_manager) {
          self.position.x = old_x;
          self.velocity.x = 0.0;
      }
  }

  // 4. 沿 Z 軸位移並處理碰撞
  let old_z = self.position.z;
  self.position.z += self.velocity.z * dt;
  self.resolve_collisions(chunk_manager, 2);
  if is_sneaking && self.on_ground {
      if !self.is_block_below(chunk_manager) {
          self.position.z = old_z;
          self.velocity.z = 0.0;
      }
  }
  ```

- [ ] **Step 3: Run cargo test to check physics unit tests**
  Run: `cargo test`
  Expected: PASS

---

### Task 5: Add Sneaking Physics Unit Tests

**Files:**
- Modify: `src/physics.rs`

- [ ] **Step 1: Write sneaking edge-guard and speed modification unit tests**
  Add tests inside `mod tests` block at the end of `src/physics.rs`:
  ```rust
  #[test]
  fn test_player_sneaking_speed() {
      let chunk_manager = ChunkManager::new(); // Assuming new() creates empty/spawn chunks
      let mut physics = PlayerPhysics::new(Vec3::new(0.0, 10.0, 0.0));
      physics.on_ground = true;
      let dt = 0.1;
      
      // Moving with sneak
      physics.update(dt, &chunk_manager, Vec3::new(1.0, 0.0, 0.0), true, false);
      // Speed factor should be 8.0 * 0.3 = 2.4. Displacement: 2.4 * 0.1 = 0.24
      assert_eq!(physics.velocity.x, 2.4);
  }

  #[test]
  fn test_player_sprinting_speed() {
      let chunk_manager = ChunkManager::new();
      let mut physics = PlayerPhysics::new(Vec3::new(0.0, 10.0, 0.0));
      physics.on_ground = true;
      let dt = 0.1;
      
      physics.update(dt, &chunk_manager, Vec3::new(1.0, 0.0, 0.0), false, true);
      // Speed factor should be 8.0 * 1.3 = 10.4. Displacement: 10.4 * 0.1 = 1.04
      assert_eq!(physics.velocity.x, 10.4);
  }
  ```

- [ ] **Step 2: Run all tests to verify**
  Run: `cargo test`
  Expected: PASS
