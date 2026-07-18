# 2026-07-18 Skybox and Fog Design

This design document outlines the implementation details for Task 4 of the Minecraft Clone: **Skybox and Fog**.

## 1. Requirements

### 1.1 Skybox Rendering
- Draw a vertical sky gradient (deep blue at the top to light blue/white at the horizon) using a procedural shader.
- Render the sky before rendering the terrain chunk meshes.
- Set up a pipeline with depth writing disabled so that the terrain blocks render in front of the sky.
- Draw a procedural sun (bright white circle) and moon (smaller pale circle in the opposite direction of the sun) based on view direction.
- Avoid external texture assets by generating everything procedurally.

### 1.2 Fog Effect (Distance Fog)
- Calculate the fragment's distance to the camera in the chunk shader.
- Apply a linear fog equation:
  $$fog\_factor = \text{clamp}\left(\frac{\text{distance} - \text{fog\_start}}{\text{fog\_end} - \text{fog\_start}}, 0.0, 1.0\right)$$
- Blend the block fragment color with the horizon color (which also serves as the fog color) to ensure the terrain blends seamlessly into the sky at the render distance edge.
- Tie the fog start and end distances dynamically to the game's render distance setting.

---

## 2. Architecture & Data Structures

We will implement a **procedural sky dome** using a fullscreen quad.

### 2.1 Uniform Update (`CameraUniform`)
We will modify the `CameraUniform` struct in [src/camera.rs](file:///f:/Desktop/MC/src/camera.rs) to include all parameters required for skybox rendering and fog:

```rust
#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CameraUniform {
    pub view_proj: [[f32; 4]; 4],
    pub inv_view_proj: [[f32; 4]; 4],
    pub camera_pos: [f32; 4],        // [x, y, z, 0.0]
    pub sky_color_top: [f32; 4],     // [r, g, b, 1.0] (default deep blue [0.1, 0.25, 0.45, 1.0])
    pub sky_color_horizon: [f32; 4], // [r, g, b, 1.0] (default light blue [0.53, 0.81, 0.92, 1.0])
    pub sun_dir: [f32; 4],           // [x, y, z, 0.0] (normalized sun direction vector)
    pub fog_start: f32,
    pub fog_end: f32,
    pub padding: [f32; 2],
}
```

This structure is perfectly aligned to 16 bytes:
- `view_proj`: 64 bytes
- `inv_view_proj`: 64 bytes
- `camera_pos`: 16 bytes
- `sky_color_top`: 16 bytes
- `sky_color_horizon`: 16 bytes
- `sun_dir`: 16 bytes
- `fog_start`: 4 bytes
- `fog_end`: 4 bytes
- `padding`: 8 bytes
**Total**: 176 bytes (which is $11 \times 16$, a multiple of 16).

### 2.2 Sky Rendering Pipeline
We will create a separate rendering pipeline for the sky:
- **Depth Stencil**:
  - `depth_write_enabled: false` (to prevent updating the depth buffer)
  - `depth_compare: wgpu::CompareFunction::Always` (or we can just render first so it renders behind everything).
- **Vertex Buffer**: None. The vertex shader `vs_sky` will generate a fullscreen quad using the `@builtin(vertex_index)` input.

---

## 3. Detailed Shader Implementation

### 3.1 WGSL Uniform Definition
```wgsl
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
```

### 3.2 Sky Vertex and Fragment Shader
The vertex shader constructs a screen-covering fullscreen quad:
```wgsl
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
    // Reconstruct world space ray direction
    let unprojected = camera.inv_view_proj * vec4<f32>(in.ndc_pos.x, in.ndc_pos.y, 1.0, 1.0);
    let world_pos = unprojected.xyz / unprojected.w;
    let view_dir = normalize(world_pos - camera.camera_pos.xyz);

    // Vertical sky gradient based on view_dir.y
    let h = max(view_dir.y, 0.0);
    var sky_color = mix(camera.sky_color_horizon, camera.sky_color_top, h);

    // Procedural Sun
    let sun_dot = dot(view_dir, normalize(camera.sun_dir.xyz));
    if (sun_dot > 0.995) {
        let sun_factor = smoothstep(0.995, 0.997, sun_dot);
        sky_color = mix(sky_color, vec4<f32>(1.0, 1.0, 1.0, 1.0), sun_factor);
    }

    // Procedural Moon (directly opposite the sun)
    let moon_dot = dot(view_dir, normalize(-camera.sun_dir.xyz));
    if (moon_dot > 0.997) {
        let moon_factor = smoothstep(0.997, 0.998, moon_dot);
        sky_color = mix(sky_color, vec4<f32>(0.9, 0.9, 0.95, 1.0), moon_factor);
    }

    return sky_color;
}
```

### 3.3 Chunk Mesh Shaders with Fog
We pass the world position down:
```wgsl
struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) tex_coords: vec2<f32>,
    @location(1) light_level: f32,
    @location(2) world_pos: vec3<f32>,
};

@vertex
fn vs_main(model: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = camera.view_proj * vec4<f32>(model.position, 1.0);
    out.tex_coords = model.tex_coords;
    out.light_level = model.light_level;
    out.world_pos = model.position;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(t_diffuse, s_diffuse, in.tex_coords);
    if (color.a < 0.5) {
        discard;
    }
    let ambient = 0.08;
    let final_light = max(in.light_level, ambient);
    let fragment_color = color * final_light;

    // Calculate fragment distance to camera
    let dist = length(in.world_pos - camera.camera_pos.xyz);
    let fog_factor = clamp((dist - camera.fog_start) / (camera.fog_end - camera.fog_start), 0.0, 1.0);

    // Interpolate with the horizon sky color
    return mix(fragment_color, camera.sky_color_horizon, fog_factor);
}
```

---

## 4. Verification Plan

- **Compilation Check**: Run `cargo check` and verify that all structs are correctly sized and align correctly with shader uniforms.
- **Visual Checks**:
  1. The sky should transition from a dark blue at the top of the viewport to a light blue/white color at the horizon.
  2. A bright white sun should be visible when looking up and in its direction. A smaller moon should be visible in the opposite direction.
  3. Terrain chunks at the boundary of the render distance should fade smoothly into the horizon sky color, removing harsh edges.
  4. Changing the render distance in the pause menu should dynamically adjust the fog start and end distance.
