# Voxel Lighting System Design

This document details the design for implementing a classic Minecraft-style lighting system (sky light and block light with 15 levels of brightness), dynamic light propagation/removal using BFS queues, mesh generation updates to support vertex lighting, and shader changes.

---

## 1. Overview
The goal is to implement a robust, performant lighting engine for the voxel world. We will:
- Add `sky_light` and `block_light` u8 arrays (0 to 15) to the `Chunk` data structure.
- Initialize `sky_light` column-by-column, scanning down from the top.
- Build a BFS propagation system in `src/lighting.rs` to propagate light across chunk boundaries.
- Support dynamic light updates (propagation and removal BFS) when blocks are placed or dug.
- Pass interpolated light values from vertices to the fragment shader.
- Apply directional lighting modifiers (top face = 1.0, side faces = 0.8, bottom face = 0.5) and a minimum ambient light level (0.08) in the shader.

---

## 2. Lighting Data Structures

Each `Chunk` in `src/world.rs` will be modified to contain light data arrays:
```rust
pub struct Chunk {
    pub chunk_x: i32,
    pub chunk_z: i32,
    pub blocks: [[[BlockType; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH],
    pub sky_light: [[[u8; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH],
    pub block_light: [[[u8; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH],
}
```

### Light Values
- **0**: Completely dark.
- **15**: Maximum brightness (fully lit by sky or full light source).

---

## 3. Light Propagation Logic (`src/lighting.rs`)

We will create a dedicated module `src/lighting.rs` to manage the BFS queues and propagation logic.

### 3.1. Light Propagation Algorithms
Lighting changes propagate using 3D Breadth-First Search (BFS).

#### Light Increase Propagation
When light level increases (e.g., placing a torch or opening a light-blocking column):
1. Keep a queue of coordinates to propagate: `VecDeque<(i32, i32, i32)>`.
2. While the queue is not empty, pop `(wx, wy, wz)`:
   - For each of the 6 neighbors `(nx, ny, nz)`:
     - Check if neighbor is within Y-bounds (`0..256`) and is not an opaque block.
     - Get current light level $N$ of neighbor.
     - If $N < \text{current\_light} - 1$:
       - Set neighbor's light level to $\text{current\_light} - 1$.
       - Enqueue neighbor.
       - Mark the neighbor's chunk mesh as dirty.

#### Light Decrease Propagation (Removal)
When a light source is removed or a block is placed that blocks light:
1. Keep a queue of coordinates and old light levels: `VecDeque<(i32, i32, i32, u8)>`.
2. Pop `(wx, wy, wz, old_val)`:
   - Set current light level at `(wx, wy, wz)` to `0`.
   - For each of the 6 neighbors `(nx, ny, nz)`:
     - Get neighbor's current light level $N$.
     - If $N \neq 0$ and $N < old\_val$:
       - The neighbor was lit by this block. Set neighbor's light to `0` and enqueue `(nx, ny, nz, N)`.
       - Mark the neighbor's chunk mesh as dirty.
     - Else if $N \ge old\_val$:
       - The neighbor is lit by another source. Push `(nx, ny, nz)` to a re-propagation queue.
3. Once the removal queue is empty, run the Light Increase propagation starting with all nodes in the re-propagation queue.

---

## 4. Vertex Structure & Layout Changes

### 4.1. Vertex Struct (`src/state.rs`)
Add a new field `light_level: f32` to the vertex structure:
```rust
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub tex_coords: [f32; 2],
    pub light_level: f32,
}
```

### 4.2. Vertex Buffer Layout (`src/state.rs`)
Update the `Vertex::desc()` to include the third attribute (shader location 2):
```rust
impl Vertex {
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 3]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x2,
                },
                wgpu::VertexAttribute {
                    offset: (std::mem::size_of::<[f32; 3]>() + std::mem::size_of::<[f32; 2]>()) as wgpu::BufferAddress,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32,
                },
            ],
        }
    }
}
```

---

## 5. Mesh Generation & Light Queries

In `Chunk::generate_mesh()`, we look up the light value of the neighboring block in the face direction to shade that face.

### Shading Multiplier
- **Top face (Up)**: `1.0`
- **Side faces (North, South, East, West)**: `0.8`
- **Bottom face (Down)**: `0.5`

### Calculation
For a face being rendered, let `neighbor_light` be the maximum of `sky_light` and `block_light` of the neighbor block:
$$\text{light\_level} = \frac{\max(\text{sky\_light}, \text{block\_light})}{15.0} \times \text{multiplier}$$

Assign this `light_level` value to all 4 vertices of the face.

---

## 6. Shader Modifications (`src/shader.wgsl`)

Update the WGSL shader to multiply texture color by the interpolated `light_level` vertex attribute, with a small ambient constant so that the screen is not pitch black at light level 0:

```wgsl
struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) tex_coords: vec2<f32>,
    @location(2) light_level: f32,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) tex_coords: vec2<f32>,
    @location(1) light_level: f32,
};

@vertex
fn vs_main(model: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = camera.view_proj * vec4<f32>(model.position, 1.0);
    out.tex_coords = model.tex_coords;
    out.light_level = model.light_level;
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
    return color * final_light;
}
```

---

## 7. Verification Plan

### Automated Tests
- Run `cargo check` and `cargo test` to verify compilability and standard test suite passing.

### Manual Verification
1. **Dynamic Torch Lighting**: Place a torch, verify that the area around it lights up immediately. Destroy the torch, verify that the area darkens.
2. **Dynamic Sky Shadows**: Build a roof, verify that the area directly underneath the roof becomes shaded, and the shadow boundary transitions correctly.
3. **Cave Brightness**: Dig a tunnel deep underground, verify that it becomes very dark but remains faintly visible due to the `0.08` ambient lighting constant.
4. **Boundary Updates**: Build a roof or place a torch across chunk boundaries, verify that the lighting updates seamlessly in both chunks.
