# 2026-07-18 Block Types & Textures Design Spec

This document details the design for expanding block types, upgrading the texture atlas, implementing dual-mesh rendering passes for opaque and transparent/translucent blocks, and updating world generation with ores, bedrock, and beaches.

---

## 1. Voxel definition & Properties System

We will expand `BlockType` to support at least 30 block types and assign physical and rendering properties to each using a static properties lookup table.

### 1.1 Block Type Enumeration

We define 31 block types in `src/world.rs`:
```rust
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum BlockType {
    Air = 0,
    Grass = 1,
    Dirt = 2,
    Stone = 3,
    Sand = 4,
    Gravel = 5,
    OakLog = 6,
    OakPlanks = 7,
    OakLeaves = 8,
    Cobblestone = 9,
    Bedrock = 10,
    Water = 11,
    CoalOre = 12,
    IronOre = 13,
    GoldOre = 14,
    DiamondOre = 15,
    RedstoneOre = 16,
    Glass = 17,
    Brick = 18,
    StoneBrick = 19,
    Snow = 20,
    Ice = 21,
    Clay = 22,
    Sandstone = 23,
    Obsidian = 24,
    CraftingTable = 25,
    Furnace = 26,
    Chest = 27,
    TNT = 28,
    Bookshelf = 29,
    Torch = 30,
}
```

### 1.2 Block Properties

Each block type is associated with a static `BlockProperties` struct defining its attributes:
```rust
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum RenderType {
    Opaque,
    Cutout,      // Fully opaque or fully transparent (leaves, glass, torch)
    Translucent, // Partially transparent with alpha blending (water, ice)
}

pub struct BlockProperties {
    pub name: &'static str,
    pub hardness: f32,
    pub render_type: RenderType,
    pub is_solid: bool,      // Collides with player AABB
    pub is_passable: bool,   // Passable for camera raycast selection
    pub light_emission: u8,
}
```

We implement `properties()` on `BlockType` to query this metadata.

### 1.3 Collision & Raycast Adjustments
- **Physics**: In `src/physics.rs`, check `chunk_manager.get_block(x, y, z).properties().is_solid` instead of `block != BlockType::Air` to resolve collisions.
- **Raycast Selection**: In `src/interaction.rs`, update `raycast` to allow targeting any non-Air blocks except those explicitly passable/non-targetable (e.g. water or air). Note: Torches are passable to walk through but should be breakable/targetable by raycast.

---

## 2. Texture Atlas Upgrade (256x256 Atlas)

To support 30+ block types, we upgrade the texture atlas from 64x64 to 256x256. This layout supports a 16x16 grid of 16x16 pixel sub-textures (256 sub-textures total).

### 2.1 Atlas Layout & Block Texture Mapping

Each texture in the atlas is identified by a 1D index (0 to 255). We define a mapping from `BlockType` and a face direction index (0: Front, 1: Back, 2: Left, 3: Right, 4: Top, 5: Bottom) to a texture ID:
- **Grass**: Top = 0, Side = 1, Bottom = 2 (Dirt)
- **Dirt**: All faces = 2
- **Stone**: All faces = 3
- **Sand**: All faces = 4
- **Gravel**: All faces = 5
- **OakLog**: Top/Bottom = 6, Sides = 7
- **OakPlanks**: All faces = 8
- **OakLeaves**: All faces = 9
- **Cobblestone**: All faces = 10
- **Bedrock**: All faces = 11
- **Water**: All faces = 12
- **CoalOre**: All faces = 13
- **IronOre**: All faces = 14
- **GoldOre**: All faces = 15
- **DiamondOre**: All faces = 16
- **RedstoneOre**: All faces = 17
- **Glass**: All faces = 18
- **Brick**: All faces = 19
- **StoneBrick**: All faces = 20
- **Snow**: Top = 21, Side = 22, Bottom = 2 (Dirt)
- **Ice**: All faces = 23
- **Clay**: All faces = 24
- **Sandstone**: Top = 25, Side = 26, Bottom = 25
- **Obsidian**: All faces = 27
- **CraftingTable**: Top = 28, Side = 29, Bottom = 8 (Planks)
- **Furnace**: Front = 30, Top/Bottom/Sides = 3 (Stone)
- **Chest**: Top/Bottom/Sides = 31
- **TNT**: Top = 32, Bottom = 33, Side = 34
- **Bookshelf**: Top/Bottom = 8 (Planks), Sides = 35
- **Torch**: All faces = 36

### 2.2 Procedural Texture Atlas Generation

In `src/texture.rs`, we replace the old procedural generation loop with a grid-based pixel drawer using a set of helper functions:
1. `draw_noise(img, tx, ty, base_color, noise_level)`: Stone, dirt, sand, bedrock, gravel, clay, obsidian.
2. `draw_brick(img, tx, ty, base_color, mortar_color, rows, cols)`: Brick, stone brick, cobblestone.
3. `draw_planks(img, tx, ty, base_color, line_color)`: Oak planks, bookshelf tops.
4. `draw_log(img, tx, ty, side_color, rings)`: Log top and log side.
5. `draw_ore(img, tx, ty, ore_color)`: Draws stone background with scattered ore speckles.
6. `draw_leaves(img, tx, ty, leaf_color)`: Green noise with some pixels set to transparent (Alpha = 0).
7. `draw_glass(img, tx, ty)`: Solid border pixels, transparent interior.
8. `draw_water(img, tx, ty)`: Semi-transparent blue waves (Alpha = 150).
9. `draw_ice(img, tx, ty)`: Light blue glassy pattern with high transparency (Alpha = 180).
10. `draw_torch(img, tx, ty)`: Draws a tiny torch model outline, keeping the rest transparent.

---

## 3. Dual-Mesh Rendering Pipeline

To render transparent blocks (like Water, Ice) correctly with blending without interfering with depth checks for opaque geometry, each chunk mesh is split.

### 3.1 Chunk Mesh Buffers

In `src/state.rs`, `ChunkMesh` holds two buffer sets:
```rust
pub struct ChunkMesh {
    pub opaque_vertex_buffer: wgpu::Buffer,
    pub opaque_index_buffer: wgpu::Buffer,
    pub opaque_num_indices: u32,

    pub transparent_vertex_buffer: wgpu::Buffer,
    pub transparent_index_buffer: wgpu::Buffer,
    pub transparent_num_indices: u32,

    pub dirty: bool,
}
```

### 3.2 Mesh Generation with Neighbor Transparency

In `Chunk::generate_mesh()`, we do not cull faces if the neighboring block is transparent/cutout/translucent:
- Query `get_block_at(nx, ny, nz)`.
- If the neighbor block has a `render_type` of `Cutout` or `Translucent`, do not cull the face of the current block.
- Categorize faces by block `RenderType`:
  * `Opaque` or `Cutout` -> write to `opaque_vertices` and `opaque_indices`.
  * `Translucent` -> write to `transparent_vertices` and `transparent_indices`.

### 3.3 Shader Updates (Cutout Discards)

We update `src/shader.wgsl` to handle cutout alpha. In the fragment shader:
```wgsl
@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(t_diffuse, s_diffuse, in.tex_coords);
    if (color.a < 0.5) {
        discard;
    }
    return color;
}
```
This automatically discards transparent pixels in cutout textures like OakLeaves and Glass, allowing them to render correctly in the opaque pass.

### 3.4 Rendering Pipelines configuration
We create two rendering pipelines in `State`:
1. **Opaque Pipeline**: Standard pipeline with `depth_write_enabled: true`, depth compare `Less`, blend set to `REPLACE`.
2. **Transparent Pipeline**:
   - `depth_write_enabled: false` to allow see-through overlays.
   - `depth_compare: Less` (so depth tests against opaque blocks succeed/fail correctly).
   - `blend` set to standard alpha blending:
     ```rust
     blend: Some(wgpu::BlendState {
         color: wgpu::BlendComponent {
             src_factor: wgpu::BlendFactor::SrcAlpha,
             dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
             operation: wgpu::BlendOperation::Add,
         },
         alpha: wgpu::BlendComponent {
             src_factor: wgpu::BlendFactor::One,
             dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
             operation: wgpu::BlendOperation::Add,
         },
     })
     ```

In the render loop:
1. Bind camera and draw all opaque chunk meshes.
2. Bind camera and draw all transparent chunk meshes.

---

## 4. World Generation Updates

We modify `Chunk::new()` to enrich the generated terrain:
1. **Bedrock Layer**: At Y = 0 to 4, replace stone blocks with Bedrock with decreasing probability. Y = 0 is 100% Bedrock.
2. **Underground Ore Veins**:
   - For Y between 0 and surface_height - 4, replace Stone with Ores using pseudo-random deterministic seeding:
     - `DiamondOre` & `RedstoneOre`: Y < 16, low chance.
     - `GoldOre`: Y < 32, low chance.
     - `IronOre`: Y < 64, medium chance.
     - `CoalOre`: Y < 128, high chance.
3. **Beaches & Sea Level**:
   - Establish sea level at Y = 62.
   - If Perlin noise height is $\le 63$, generate `Sand` as the top layer instead of `Grass`/`Dirt`.
   - If height is $< 62$, fill all blocks from height to Y = 62 with `Water`.

---

## 5. Verification Plan

### Automated Verification
- Run `cargo check` and `cargo test` to verify compilability and existing collision unit tests.

### Manual Verification
- Walk around, inspect sand beaches and water bodies.
- Verify that glass, leaves, and water render correctly (translucent water shows blocks underneath).
- Mine down to find bedrock and ore blocks.
- Try placing blocks next to/inside water and check collision.
