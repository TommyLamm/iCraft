# Skybox and Fog Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement a procedural skybox (gradient with sun and moon) and distance fog in the Minecraft clone using WGPU.

**Architecture:** Use a fullscreen quad rendered first in the frame to draw a skybox gradient and procedural sun/moon. Pass sky/fog parameters from Rust to the shader by updating the `CameraUniform` block. Add distance fog computation inside the chunk fragment shader.

**Tech Stack:** Rust, WGPU, WGSL

---

### Task 1: Update Camera and Uniform Structure

**Files:**
- Modify: [src/camera.rs](file:///f:/Desktop/MC/src/camera.rs)

- [ ] **Step 1: Modify `CameraUniform` struct**
  Update the struct definition in `src/camera.rs` to include matrices, colors, directions, and fog parameters.
  
  ```rust
  #[repr(C)]
  #[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
  pub struct CameraUniform {
      pub view_proj: [[f32; 4]; 4],
      pub inv_view_proj: [[f32; 4]; 4],
      pub camera_pos: [f32; 4],
      pub sky_color_top: [f32; 4],
      pub sky_color_horizon: [f32; 4],
      pub sun_dir: [f32; 4],
      pub fog_start: f32,
      pub fog_end: f32,
      pub padding: [f32; 2],
  }
  ```

- [ ] **Step 2: Update `CameraUniform::update_view_proj`**
  Modify the `update_view_proj` function signature and implementation in `src/camera.rs` to accept `render_distance` and calculate inverse matrices and set uniforms:
  
  ```rust
  impl CameraUniform {
      pub fn new() -> Self {
          Self {
              view_proj: glam::Mat4::IDENTITY.to_cols_array_2d(),
              inv_view_proj: glam::Mat4::IDENTITY.to_cols_array_2d(),
              camera_pos: [0.0, 0.0, 0.0, 0.0],
              sky_color_top: [0.1, 0.25, 0.45, 1.0],
              sky_color_horizon: [0.53, 0.81, 0.92, 1.0],
              sun_dir: [0.5, 0.8, 0.3, 0.0],
              fog_start: 0.0,
              fog_end: 100.0,
              padding: [0.0; 2],
          }
      }

      pub fn update_view_proj(&mut self, camera: &Camera, aspect: f32, render_distance: u32) {
          let view_proj = camera.build_view_projection_matrix(aspect);
          self.view_proj = view_proj.to_cols_array_2d();
          self.inv_view_proj = view_proj.inverse().to_cols_array_2d();
          self.camera_pos = [camera.position.x, camera.position.y, camera.position.z, 0.0];
          self.sky_color_top = [0.1, 0.25, 0.45, 1.0];
          self.sky_color_horizon = [0.53, 0.81, 0.92, 1.0];
          
          let raw_sun = glam::Vec3::new(0.5, 0.8, 0.3).normalize();
          self.sun_dir = [raw_sun.x, raw_sun.y, raw_sun.z, 0.0];

          let fog_end = (render_distance as f32) * 16.0;
          self.fog_end = fog_end;
          self.fog_start = fog_end * 0.6;
      }
  }
  ```

- [ ] **Step 3: Run `cargo check` to verify compilation failure at call sites**
  Run: `cargo check`
  Expected: Compiler errors at `src/state.rs` due to mismatched argument count.

---

### Task 2: Fix Uniform Call Sites in `src/state.rs`

**Files:**
- Modify: [src/state.rs](file:///f:/Desktop/MC/src/state.rs)

- [ ] **Step 1: Fix `CameraUniform` call in `State::new`**
  Modify line 214 in `src/state.rs`:
  ```rust
  camera_uniform.update_view_proj(&camera, config.width as f32 / config.height as f32, render_distance as u32);
  ```

- [ ] **Step 2: Fix `CameraUniform` call in `State::handle_menu_click`**
  Modify line 810 in `src/state.rs`:
  ```rust
  self.camera_uniform.update_view_proj(&self.camera, self.config.width as f32 / self.config.height as f32, self.chunk_manager.render_distance as u32);
  ```

- [ ] **Step 3: Fix `CameraUniform` call in `State::update`**
  Modify line 871 in `src/state.rs`:
  ```rust
  self.camera_uniform.update_view_proj(&self.camera, self.config.width as f32 / self.config.height as f32, self.chunk_manager.render_distance as u32);
  ```

- [ ] **Step 4: Run `cargo check` to verify compilation passes**
  Run: `cargo check`
  Expected: Successful compilation.

- [ ] **Step 5: Commit changes**
  Run:
  ```bash
  git add src/camera.rs src/state.rs
  git commit -m "feat: update CameraUniform structure and call sites to support sky/fog"
  ```

---

### Task 3: Implement Procedural Skybox Shader and Pipeline

**Files:**
- Modify: [src/shader.wgsl](file:///f:/Desktop/MC/src/shader.wgsl)
- Modify: [src/state.rs](file:///f:/Desktop/MC/src/state.rs)

- [ ] **Step 1: Update shader definitions in `src/shader.wgsl`**
  Modify `CameraUniform` definition, and add `vs_sky` and `fs_sky` at the bottom of the file:
  
  ```wgsl
  // In src/shader.wgsl:
  struct CameraUniform {
      view_proj: mat4x4<f32>,
      inv_view_proj: mat4x4<f32>,
      camera_pos: vec4<f32>,
      sky_color_top: vec4<f32>,
      sky_color_horizon: vec4<f32>,
      sun_dir: vec4<f32>,
      fog_start: f32,
      fog_end: f32,
      padding: vec2<f32>,
  };
  
  struct SkyVertexOutput {
      @builtin(position) clip_position: vec4<f32>,
      @location(0) ndc_pos: vec2<f32>,
  };

  @vertex
  fn vs_sky(@builtin(vertex_index) vertex_index: u32) -> SkyVertexOutput {
      var pos = array<vec2<f32>, 6>(
          vec2<f32>(-1.0, -1.0),
          vec2<f32>( 1.0, -1.0),
          vec2<f32>(-1.0,  1.0),
          vec2<f32>(-1.0,  1.0),
          vec2<f32>( 1.0, -1.0),
          vec2<f32>( 1.0,  1.0)
      );
      var out: SkyVertexOutput;
      let p = pos[vertex_index];
      out.clip_position = vec4<f32>(p, 0.99999, 1.0);
      out.ndc_pos = p;
      return out;
  }

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

      return sky_color;
  }
  ```

- [ ] **Step 2: Add `sky_pipeline` field to `State`**
  Modify the `State` struct in `src/state.rs` to include `sky_pipeline: wgpu::RenderPipeline`.

- [ ] **Step 3: Create `sky_pipeline` in `State::new`**
  In `State::new`, build the sky pipeline using the existing `camera_bind_group_layout` and `depth_stencil` configuration (with write disabled and compare function set to `Always`):
  
  ```rust
          let sky_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
              label: Some("Sky Render Pipeline"),
              layout: Some(&render_pipeline_layout),
              vertex: wgpu::VertexState {
                  module: &shader,
                  entry_point: "vs_sky",
                  buffers: &[],
              },
              fragment: Some(wgpu::FragmentState {
                  module: &shader,
                  entry_point: "fs_sky",
                  targets: &[Some(wgpu::ColorTargetState {
                      format: config.format,
                      blend: Some(wgpu::BlendState::REPLACE),
                      write_mask: wgpu::ColorWrites::ALL,
                  })],
              }),
              primitive: wgpu::PrimitiveState {
                  topology: wgpu::PrimitiveTopology::TriangleList,
                  strip_index_format: None,
                  front_face: wgpu::FrontFace::Cw,
                  cull_mode: None,
                  polygon_mode: wgpu::PolygonMode::Fill,
                  unclipped_depth: false,
                  conservative: false,
              },
              depth_stencil: Some(wgpu::DepthStencilState {
                  format: wgpu::TextureFormat::Depth32Float,
                  depth_write_enabled: false,
                  depth_compare: wgpu::CompareFunction::Always,
                  stencil: wgpu::StencilState::default(),
                  bias: wgpu::DepthBiasState::default(),
              }),
              multisample: wgpu::MultisampleState::default(),
              multiview: None,
          });
  ```
  Store this in `Self { ..., sky_pipeline, ... }`.

- [ ] **Step 4: Draw sky at the start of `State::render`**
  In `State::render`, at the start of `render_pass`, draw the sky before drawing the chunk meshes:
  
  ```rust
              // Draw Skybox first
              render_pass.set_pipeline(&self.sky_pipeline);
              render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
              render_pass.draw(0..6, 0..1);
  ```

- [ ] **Step 5: Run `cargo check` to verify compilation passes**
  Run: `cargo check`
  Expected: Successful compilation.

- [ ] **Step 6: Commit changes**
  Run:
  ```bash
  git add src/state.rs src/shader.wgsl
  git commit -m "feat: add skybox pipeline and shaders, render sky first"
  ```

---

### Task 4: Implement Distance Fog in Chunk Shaders

**Files:**
- Modify: [src/shader.wgsl](file:///f:/Desktop/MC/src/shader.wgsl)

- [ ] **Step 1: Modify `VertexOutput` to include world position**
  In `src/shader.wgsl`, add `@location(2) world_pos: vec3<f32>` to `VertexOutput`:
  ```wgsl
  struct VertexOutput {
      @builtin(position) clip_position: vec4<f32>,
      @location(0) tex_coords: vec2<f32>,
      @location(1) light_level: f32,
      @location(2) world_pos: vec3<f32>,
  };
  ```

- [ ] **Step 2: Update `vs_main` in `src/shader.wgsl` to propagate world position**
  Update `vs_main`:
  ```wgsl
  @vertex
  fn vs_main(model: VertexInput) -> VertexOutput {
      var out: VertexOutput;
      out.clip_position = camera.view_proj * vec4<f32>(model.position, 1.0);
      out.tex_coords = model.tex_coords;
      out.light_level = model.light_level;
      out.world_pos = model.position;
      return out;
  }
  ```

- [ ] **Step 3: Update `fs_main` in `src/shader.wgsl` to apply fog blending**
  Update `fs_main`:
  ```wgsl
  @fragment
  fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
      let color = textureSample(t_diffuse, s_diffuse, in.tex_coords);
      if (color.a < 0.5) {
          discard;
      }
      let ambient = 0.08;
      let final_light = max(in.light_level, ambient);
      let fragment_color = color * final_light;

      let dist = length(in.world_pos - camera.camera_pos.xyz);
      let fog_factor = clamp((dist - camera.fog_start) / (camera.fog_end - camera.fog_start), 0.0, 1.0);

      return mix(fragment_color, camera.sky_color_horizon, fog_factor);
  }
  ```

- [ ] **Step 4: Run `cargo check` to verify compilation passes**
  Run: `cargo check`
  Expected: Successful compilation.

- [ ] **Step 5: Commit changes**
  Run:
  ```bash
  git add src/shader.wgsl
  git commit -m "feat: apply distance-based fog blending in chunk fragment shader"
  ```
