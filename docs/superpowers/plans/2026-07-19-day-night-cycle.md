# Day/Night Cycle Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement a smooth Day/Night Cycle with dynamic GPU-side lighting updates, stars, F3 debug overlay, and time acceleration.

**Architecture:** We keep block sky light propagation static on the CPU, and pack sky light, block light, and face orientation multiplier into the single float `light_level` attribute per vertex. In the shader, we unpack this float, scale sky light based on a dynamic time-of-day uniform, and combine it with block light. The skybox shader calculates celestial rotation and procedurally renders stars at night.

**Tech Stack:** Rust, wgpu (WGSL Shaders)

---

### Task 1: Add WorldTime and show_debug Fields

**Files:**
- Modify: [camera.rs](file:///f:/Desktop/MC/src/camera.rs)
- Modify: [state.rs](file:///f:/Desktop/MC/src/state.rs)

- [ ] **Step 1: Define WorldTime and color constants in `src/camera.rs`**
  Add `WorldTime` definition and helper function at the bottom of the file:
  ```rust
  pub struct WorldTime {
      pub ticks: u64,
      pub day_length: u64,
      pub tick_accumulator: f32,
  }

  impl WorldTime {
      pub fn new() -> Self {
          Self {
              ticks: 6000, // Start at noon
              day_length: 24000,
              tick_accumulator: 0.0,
          }
      }

      pub fn time_of_day_smooth(&self) -> f32 {
          let current_ticks = (self.ticks % self.day_length) as f32 + self.tick_accumulator;
          current_ticks / self.day_length as f32
      }

      pub fn sun_angle(&self) -> f32 {
          self.time_of_day_smooth() * 2.0 * std::f32::consts::PI
      }

      pub fn sky_light_level(&self) -> u8 {
          let angle = self.sun_angle();
          let sin_angle = angle.sin();
          if sin_angle >= 0.2 {
              15
          } else if sin_angle <= -0.2 {
              4
          } else {
              let t = (sin_angle + 0.2) / 0.4;
              (4.0 + t * 11.0).round() as u8
          }
      }
  }

  fn lerp_color(c1: [f32; 4], c2: [f32; 4], t: f32) -> [f32; 4] {
      [
          c1[0] + (c2[0] - c1[0]) * t,
          c1[1] + (c2[1] - c1[1]) * t,
          c1[2] + (c2[2] - c1[2]) * t,
          c1[3] + (c2[3] - c1[3]) * t,
      ]
  }

  const SUNRISE_TOP: [f32; 4] = [0.1, 0.15, 0.3, 1.0];
  const SUNRISE_HORIZON: [f32; 4] = [0.9, 0.5, 0.2, 1.0];
  const DAY_TOP: [f32; 4] = [0.1, 0.25, 0.45, 1.0];
  const DAY_HORIZON: [f32; 4] = [0.53, 0.81, 0.92, 1.0];
  const SUNSET_TOP: [f32; 4] = [0.05, 0.1, 0.25, 1.0];
  const SUNSET_HORIZON: [f32; 4] = [0.9, 0.4, 0.15, 1.0];
  const NIGHT_TOP: [f32; 4] = [0.01, 0.01, 0.03, 1.0];
  const NIGHT_HORIZON: [f32; 4] = [0.02, 0.02, 0.05, 1.0];
  ```

- [ ] **Step 2: Update `CameraUniform::update_view_proj` signature and logic in `src/camera.rs`**
  Modify the implementation:
  ```rust
      pub fn update_view_proj(&mut self, camera: &Camera, aspect: f32, render_distance: u32, world_time: &WorldTime) {
          let view_proj = camera.build_view_projection_matrix(aspect);
          self.view_proj = view_proj.to_cols_array_2d();
          self.inv_view_proj = view_proj.inverse().to_cols_array_2d();
          self.camera_pos = [camera.position.x, camera.position.y, camera.position.z, 0.0];

          let t = world_time.time_of_day_smooth();
          let (sky_top, sky_horizon) = if t < 0.25 {
              let factor = t / 0.25;
              (
                  lerp_color(SUNRISE_TOP, DAY_TOP, factor),
                  lerp_color(SUNRISE_HORIZON, DAY_HORIZON, factor),
              )
          } else if t < 0.5 {
              let factor = (t - 0.25) / 0.25;
              (
                  lerp_color(DAY_TOP, SUNSET_TOP, factor),
                  lerp_color(DAY_HORIZON, SUNSET_HORIZON, factor),
              )
          } else if t < 0.75 {
              let factor = (t - 0.5) / 0.25;
              (
                  lerp_color(SUNSET_TOP, NIGHT_TOP, factor),
                  lerp_color(SUNSET_HORIZON, NIGHT_HORIZON, factor),
              )
          } else {
              let factor = (t - 0.75) / 0.25;
              (
                  lerp_color(NIGHT_TOP, SUNRISE_TOP, factor),
                  lerp_color(NIGHT_HORIZON, SUNRISE_HORIZON, factor),
              )
          };

          self.sky_color_top = sky_top;
          self.sky_color_horizon = sky_horizon;

          let sun_angle = world_time.sun_angle();
          let x = sun_angle.cos() * 0.95;
          let y = sun_angle.sin() * 0.95;
          let z = 0.3f32;
          let sun_dir_vec = Vec3::new(x, y, z).normalize();
          let sky_intensity = world_time.sky_light_level() as f32 / 15.0;
          self.sun_dir = [sun_dir_vec.x, sun_dir_vec.y, sun_dir_vec.z, sky_intensity];

          let fog_end = (render_distance as f32) * 16.0;
          self.fog_end = fog_end;
          self.fog_start = fog_end * 0.6;
          self.padding = [0.0; 2];
      }
  ```

- [ ] **Step 3: Update `State` definition and initialization in `src/state.rs`**
  Modify `State` struct:
  ```rust
      pub world_time: crate::camera::WorldTime,
      pub show_debug: bool,
  ```
  In `State::new` update initialization:
  ```rust
          let world_time = crate::camera::WorldTime::new();
          let show_debug = false;
  ```
  Also update the `camera_uniform.update_view_proj` call in `State::new` to:
  ```rust
          camera_uniform.update_view_proj(&camera, config.width as f32 / config.height as f32, settings.render_distance as u32, &world_time);
  ```
  And populate the fields in the struct return:
  ```rust
              world_time,
              show_debug,
  ```

- [ ] **Step 4: Update other `update_view_proj` calls in `src/state.rs`**
  In `handle_menu_click`:
  ```rust
                  self.camera_uniform.update_view_proj(&self.camera, self.config.width as f32 / self.config.height as f32, self.chunk_manager.render_distance as u32, &self.world_time);
  ```
  In `update`:
  ```rust
          self.camera_uniform.update_view_proj(&self.camera, self.config.width as f32 / self.config.height as f32, self.chunk_manager.render_distance as u32, &self.world_time);
  ```

- [ ] **Step 5: Verify compilation**
  Run: `cargo check`
  Expected: Successful compilation (or warnings about unused variables only)

- [ ] **Step 6: Commit**
  ```bash
  git add src/camera.rs src/state.rs
  git commit -m "feat: add WorldTime and show_debug fields to camera and state"
  ```

---

### Task 2: Keyboard Inputs for F3 and T keys

**Files:**
- Modify: [state.rs](file:///f:/Desktop/MC/src/state.rs)
- Modify: [app.rs](file:///f:/Desktop/MC/src/app.rs)

- [ ] **Step 1: Add key state `t` in `src/state.rs`**
  Add `pub t: bool` to `KeyState` struct:
  ```rust
  #[derive(Default)]
  pub struct KeyState {
      pub w: bool,
      pub a: bool,
      pub s: bool,
      pub d: bool,
      pub space: bool,
      pub t: bool,
  }
  ```

- [ ] **Step 2: Intercept KeyCode::F3 and KeyCode::KeyT in `src/app.rs`**
  In `src/app.rs` inside the `match physical_key` block:
  ```rust
                          winit::keyboard::PhysicalKey::Code(winit::keyboard::KeyCode::F3) => {
                              if pressed {
                                  state.show_debug = !state.show_debug;
                              }
                          }
                          winit::keyboard::PhysicalKey::Code(winit::keyboard::KeyCode::KeyT) => {
                              state.keys.t = pressed;
                          }
  ```

- [ ] **Step 3: Verify compilation**
  Run: `cargo check`
  Expected: PASS

- [ ] **Step 4: Commit**
  ```bash
  git add src/state.rs src/app.rs
  git commit -m "feat: handle F3 and T key inputs for debugging and time acceleration"
  ```

---

### Task 3: Update WorldTime in Update Loop

**Files:**
- Modify: [state.rs](file:///f:/Desktop/MC/src/state.rs)

- [ ] **Step 1: Accumulate time in `State::update`**
  At the beginning of `State::update` (before early return checks, so time updates but ignores pause if required, or after early return checks so pause stops time):
  Let's place it after the `is_paused` check so that pausing the game stops time progression:
  ```rust
      pub fn update(&mut self, dt: f32) {
          if self.player_state.is_dead {
              return;
          }
          if self.is_paused {
              return;
          }
          
          // Update game time
          let speed_multiplier = if self.keys.t { 200.0 } else { 1.0 };
          self.world_time.tick_accumulator += dt * 20.0 * speed_multiplier;
          let new_ticks = self.world_time.tick_accumulator.floor() as u64;
          self.world_time.ticks += new_ticks;
          self.world_time.tick_accumulator -= new_ticks as f32;
  ```

- [ ] **Step 2: Update the clear color of the RenderPass**
  In `State::render`, update the color attachment clear color to use `self.camera_uniform.sky_color_horizon`:
  ```rust
                  color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                      view: &view,
                      resolve_target: None,
                      ops: wgpu::Operations {
                          load: wgpu::LoadOp::Clear(wgpu::Color {
                              r: self.camera_uniform.sky_color_horizon[0] as f64,
                              g: self.camera_uniform.sky_color_horizon[1] as f64,
                              b: self.camera_uniform.sky_color_horizon[2] as f64,
                              a: 1.0,
                          }),
                          store: wgpu::StoreOp::Store,
                      },
                  })],
  ```

- [ ] **Step 3: Verify compilation**
  Run: `cargo check`
  Expected: PASS

- [ ] **Step 4: Commit**
  ```bash
  git add src/state.rs
  git commit -m "feat: accumulate ticks in state update and dynamically update clear color"
  ```

---

### Task 4: Pack Lights in Mesh Buffers

**Files:**
- Modify: [world.rs](file:///f:/Desktop/MC/src/world.rs)
- Modify: [state.rs](file:///f:/Desktop/MC/src/state.rs)

- [ ] **Step 1: Update light packing during terrain mesh construction in `src/world.rs`**
  Modify lines ~616-623 to pack sky light, block light, and orientation:
  ```rust
                             let multiplier_code = match face_idx {
                                 4 => 0.0, // Top
                                 5 => 2.0, // Bottom
                                 _ => 1.0, // Sides
                             };
                             let light_val = (neighbor_sky as f32) + (neighbor_block as f32) * 16.0 + multiplier_code * 256.0;
  ```

- [ ] **Step 2: Update light packing during cracking overlay update in `src/state.rs`**
  In `update_crack_buffers`, update lines ~1178-1188:
  ```rust
          let sky_light = self.chunk_manager.get_sky_light(wx as i32, wy as i32, wz as i32);
          let block_light = self.chunk_manager.get_block_light(wx as i32, wy as i32, wz as i32);
          
          for (face_idx, (_normal, corners)) in faces.iter().enumerate() {
              let start_idx = vertices.len() as u32;
              let multiplier_code = match face_idx {
                  4 => 0.0, // Top
                  5 => 2.0, // Bottom
                  _ => 1.0, // Sides
              };
              let light_val = (sky_light as f32) + (block_light as f32) * 16.0 + multiplier_code * 256.0;
  ```

- [ ] **Step 3: Verify compilation**
  Run: `cargo check`
  Expected: PASS

- [ ] **Step 4: Commit**
  ```bash
  git add src/world.rs src/state.rs
  git commit -m "feat: pack sky light, block light, and face multiplier in mesh vertices"
  ```

---

### Task 5: Dynamic GPU Light Unpacking in Shader

**Files:**
- Modify: [shader.wgsl](file:///f:/Desktop/MC/src/shader.wgsl)

- [ ] **Step 1: Update block rendering fragment shader (`fs_main`)**
  Replace lines ~44-58 in `src/shader.wgsl` with:
  ```wgsl
  @fragment
  fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
      let color = textureSample(t_diffuse, s_diffuse, in.tex_coords);
      if (color.a < 0.5) {
          discard;
      }

      // Unpack lighting
      let packed = in.light_level;
      let multiplier_code = floor(packed / 256.0);
      let rest = packed - multiplier_code * 256.0;
      let block_light = floor(rest / 16.0);
      let sky_light = rest - block_light * 16.0;

      var multiplier = 1.0;
      if (multiplier_code > 1.5) {
          multiplier = 0.5;
      } else if (multiplier_code > 0.5) {
          multiplier = 0.8;
      }

      // Dynamically scale sky light with global intensity
      let sky_intensity = camera.sun_dir.w;
      let adjusted_sky_light = sky_light * sky_intensity;
      let max_light = max(adjusted_sky_light, block_light);

      let ambient = 0.08;
      let final_light = max(max_light / 15.0, ambient) * multiplier;
      let fragment_color = color * final_light;

      let dist = length(in.world_pos - camera.camera_pos.xyz);
      let fog_factor = clamp((dist - camera.fog_start) / (camera.fog_end - camera.fog_start), 0.0, 1.0);

      return mix(fragment_color, camera.sky_color_horizon, fog_factor);
  }
  ```

- [ ] **Step 2: Verify compilation**
  Run: `cargo check`
  Expected: PASS

- [ ] **Step 3: Commit**
  ```bash
  git add src/shader.wgsl
  git commit -m "feat: implement GPU-side light unpacking and dynamic scaling in terrain shader"
  ```

---

### Task 6: Star Shader and Celestial Rotation

**Files:**
- Modify: [shader.wgsl](file:///f:/Desktop/MC/src/shader.wgsl)

- [ ] **Step 1: Add hashing function and procedural star function in `src/shader.wgsl`**
  Add the helper functions just above `fs_sky`:
  ```wgsl
  fn hash3(p: vec3<f32>) -> f32 {
      let sin_val = sin(dot(p, vec3<f32>(127.1, 311.7, 74.7)));
      return fract(sin_val * 43758.5453123);
  }

  fn get_star(dir: vec3<f32>) -> f32 {
      if (dir.y <= 0.0) {
          return 0.0;
      }
      
      let grid_size = 120.0;
      let grid_pos = floor(dir * grid_size);
      
      let h1 = hash3(grid_pos);
      let h2 = hash3(grid_pos + vec3<f32>(1.0, 2.0, 3.0));
      let h3 = hash3(grid_pos + vec3<f32>(4.0, 5.0, 6.0));
      
      if (h1 < 0.992) {
          return 0.0;
      }
      
      let cell_center = (grid_pos + vec3<f32>(0.5, 0.5, 0.5)) / grid_size;
      let offset = (vec3<f32>(h1, h2, h3) - vec3<f32>(0.5)) * 0.4 / grid_size;
      let star_pos = normalize(cell_center + offset);
      
      let d = dot(dir, star_pos);
      let star_size = 0.9998;
      if (d > star_size) {
          let intensity = (d - star_size) / (1.0 - star_size);
          return intensity * h2; 
      }
      return 0.0;
  }
  ```

- [ ] **Step 2: Update `fs_sky` to rotate sky and add stars**
  Replace `fs_sky` implementation:
  ```wgsl
  @fragment
  fn fs_sky(in: SkyVertexOutput) -> @location(0) vec4<f32> {
      let unprojected = camera.inv_view_proj * vec4<f32>(in.ndc_pos.x, in.ndc_pos.y, 1.0, 1.0);
      let world_pos = unprojected.xyz / unprojected.w;
      let view_dir = normalize(world_pos - camera.camera_pos.xyz);

      let h = max(view_dir.y, 0.0);
      var sky_color = mix(camera.sky_color_horizon, camera.sky_color_top, h);

      // Sun
      let sun_dot = dot(view_dir, normalize(camera.sun_dir.xyz));
      if (sun_dot > 0.995) {
          let sun_factor = smoothstep(0.995, 0.997, sun_dot);
          sky_color = mix(sky_color, vec4<f32>(1.0, 1.0, 1.0, 1.0), sun_factor);
      }

      // Moon
      let moon_dot = dot(view_dir, normalize(-camera.sun_dir.xyz));
      if (moon_dot > 0.997) {
          let moon_factor = smoothstep(0.997, 0.998, moon_dot);
          sky_color = mix(sky_color, vec4<f32>(0.9, 0.9, 0.95, 1.0), moon_factor);
      }

      // Stars (with celestial rotation around the Z axis)
      let sun_angle = atan2(camera.sun_dir.y, camera.sun_dir.x);
      let cos_a = cos(-sun_angle);
      let sin_a = sin(-sun_angle);
      let rotated_dir = vec3<f32>(
          view_dir.x * cos_a - view_dir.y * sin_a,
          view_dir.x * sin_a + view_dir.y * cos_a,
          view_dir.z
      );
      
      let star_intensity = smoothstep(0.1, -0.1, camera.sun_dir.y);
      let star_val = get_star(rotated_dir) * star_intensity;
      sky_color = sky_color + vec4<f32>(star_val, star_val, star_val, 0.0);

      return sky_color;
  }
  ```

- [ ] **Step 3: Verify compilation**
  Run: `cargo check`
  Expected: PASS

- [ ] **Step 4: Commit**
  ```bash
  git add src/shader.wgsl
  git commit -m "feat: implement celestial sphere rotation and procedural stars in skybox shader"
  ```

---

### Task 7: Implement F3 Debug Overlay

**Files:**
- Modify: [state.rs](file:///f:/Desktop/MC/src/state.rs)

- [ ] **Step 1: Draw F3 overlay in `State::render`**
  Add the following block of code at the end of the `else` block (around line 2335, just before `// Write Buffers`):
  ```rust
              // F3 Debug Screen
              if self.show_debug {
                  let time_of_day = self.world_time.time_of_day_smooth();
                  let hour = ((time_of_day * 24.0 + 6.0) % 24.0).floor() as u32;
                  let minute = (((time_of_day * 24.0 + 6.0) % 1.0) * 60.0).floor() as u32;
                  let day = self.world_time.ticks / self.world_time.day_length;
                  let time_str = format!("TIME: {:02}:{:02} (DAY {}, TICKS: {})", hour, minute, day, self.world_time.ticks);
                  
                  let pos = self.player_physics.position;
                  let pos_str = format!("XYZ: {:.3} / {:.5} / {:.3}", pos.x, pos.y, pos.z);
                  
                  let dir_x = self.camera.yaw.cos() * self.camera.pitch.cos();
                  let dir_y = self.camera.pitch.sin();
                  let dir_z = self.camera.yaw.sin() * self.camera.pitch.cos();
                  let dir_str = format!("DIR: {:.2} / {:.2} / {:.2}", dir_x, dir_y, dir_z);

                  let light_lvl = self.world_time.sky_light_level();
                  let light_str = format!("SKY LIGHT: {} (DAYTIME: {})", light_lvl, if light_lvl == 15 { "YES" } else if light_lvl == 4 { "NO" } else { "TRANSITION" });
                  
                  let char_w = 0.007;
                  let char_h = 0.014;
                  let spacing = 0.002;
                  
                  let start_x = -0.98;
                  let start_y = 0.95;
                  let line_gap = 0.025;
                  
                  add_string_lines(&time_str, start_x, start_y, char_w, char_h, spacing, [1.0, 1.0, 1.0, 1.0], &mut ui_line_vertices);
                  add_string_lines(&pos_str, start_x, start_y - line_gap, char_w, char_h, spacing, [1.0, 1.0, 1.0, 1.0], &mut ui_line_vertices);
                  add_string_lines(&dir_str, start_x, start_y - line_gap * 2.0, char_w, char_h, spacing, [1.0, 1.0, 1.0, 1.0], &mut ui_line_vertices);
                  add_string_lines(&light_str, start_x, start_y - line_gap * 3.0, char_w, char_h, spacing, [1.0, 1.0, 1.0, 1.0], &mut ui_line_vertices);
              }
  ```

- [ ] **Step 2: Verify compilation and run the application**
  Run: `cargo run`
  Expected: Game launches successfully. Hold `T` to see time accelerate (sun/moon moving, sky colors changing, stars appearing at night). Press `F3` to toggle the debug display overlay and verify all text info matches.

- [ ] **Step 3: Commit**
  ```bash
  git add src/state.rs
  git commit -m "feat: implement F3 debug screen overlay displaying game time, position, and light levels"
  ```
