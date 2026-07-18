# Multi-Chunk Support & Dynamic Loading Design

This document details the design for implementing multi-chunk rendering, dynamic chunk loading/unloading (rate-limited), cross-chunk face culling, and updated settings/UI to support render distance configurations.

## 1. Overview
The goal is to transition the game from a single static chunk to a dynamically loading, multi-chunk voxel world, similar to vanilla Minecraft.
We will:
- Support coordinate transformations between world coordinates and chunk coordinates (including negative coordinates).
- Implement a `ChunkManager` to manage chunks.
- Implement rate-limited dynamic chunk loading/unloading around the player on the main thread to prevent frame stutter.
- Update mesh generation to cull faces on chunk boundaries by querying neighbor chunks.
- Allocate and render separate GPU buffers per chunk.
- Add a "Render Distance" setting adjustable from the Pause Menu and persist it in `settings.txt`.
- Fix player physics AABB bounding check range to support negative coordinate regions.

---

## 2. Coordinate Conversion
A block's world coordinates $(wx, wy, wz)$ are mapped to a chunk coordinate $(cx, cz)$ and local block coordinates $(bx, by, bz)$:
- $cx = wx.\text{div\_euclid}(16)$
- $cz = wz.\text{div\_euclid}(16)$
- $bx = wx.\text{rem\_euclid}(16)$
- $bz = wz.\text{rem\_euclid}(16)$
- $by = wy$ (valid in range $[0, 255]$)

Using `.div_euclid` and `.rem_euclid` is essential to correctly handle negative coordinates.

---

## 3. Data Structures

### Chunk Manager
`ChunkManager` is a logical manager of the voxel grid, decoupling 3D rendering data from logic.
```rust
// src/chunk_manager.rs
use std::collections::HashMap;
use crate::world::{Chunk, BlockType};

pub struct ChunkManager {
    pub chunks: HashMap<(i32, i32), Chunk>,
    pub render_distance: i32,
}

impl ChunkManager {
    pub fn new(render_distance: i32) -> Self { ... }
    pub fn world_to_local(&self, wx: i32, wy: i32, wz: i32) -> Option<((i32, i32), (usize, usize, usize))>;
    pub fn get_block(&self, wx: i32, wy: i32, wz: i32) -> BlockType;
    pub fn set_block(&mut self, wx: i32, wy: i32, wz: i32, block: BlockType);
}
```

### Chunk Mesh Buffer
We keep rendering resources in `State` using `ChunkMesh`:
```rust
// src/state.rs
pub struct ChunkMesh {
    pub vertex_buffer: wgpu::Buffer,
    pub index_buffer: wgpu::Buffer,
    pub num_indices: u32,
    pub dirty: bool,
}
```

---

## 4. Chunk Loading & Rate Limiting
To prevent frame rate stuttering:
- **Initialization**: When initializing the game, synchronously generate and mesh all chunks within the initial render distance so the player does not fall into the void.
- **Dynamic Update (Rate Limiter)**: 
  - Every frame, compute active chunks within the player's render distance: $\max(|cx - px|, |cz - pz|) \le R$.
  - Unload out-of-bounds chunks from `chunks` and `chunk_meshes`.
  - Queue missing chunks in `load_queue` and sort them by distance to player chunk (closest first).
  - Pop and generate **at most 1 chunk** per frame.
  - When a new chunk is generated, mark its 4 neighbors as `dirty` so their boundary meshes regenerate to perform correct face culling.
  - Mesh regeneration: Rebuild **at most 2 dirty meshes** per frame to distribute GPU buffer allocation overhead.

---

## 5. Mesh Generation & Cross-Chunk Face Culling
Inside `Chunk::generate_mesh()`, we query neighboring blocks via a callback:
```rust
impl Chunk {
    pub fn generate_mesh<F>(&self, get_block_at: F) -> (Vec<Vertex>, Vec<u32>)
    where
        F: Fn(i32, i32, i32) -> BlockType;
}
```
If a block is on the border of a chunk, we look up the neighboring block in world space.
If the neighbor chunk is not loaded, we treat it as `BlockType::Air` to ensure the border face is rendered rather than culled (preventing see-through gaps).

Vertex positions are calculated as absolute world coordinates during mesh generation:
$$X_{\text{world}} = cx \times 16 + x$$
$$Z_{\text{world}} = cz \times 16 + z$$
This allows us to draw all chunk meshes using the same shader without changing model matrices or bindings per chunk draw call.

---

## 6. Physics and Raycast Updates
- **Physics**: In `src/physics.rs`, remove the `.max(0)` clamp in `resolve_collisions` bounding loops. Change bounding loops to check range using `.clamp(0, 255)` only on the Y-axis. Query the block type using `chunk_manager.get_block()`.
- **Raycast**: In `src/interaction.rs`, update `raycast` to query block type from `chunk_manager`.

---

## 7. Pause Menu & Render Distance Settings
Update the pause menu Y-coordinates to accommodate 5 buttons:
- **RESUME**: `Y: [0.24, 0.34]`
- **FOV**: `Y: [0.10, 0.20]`
- **SENSITIVITY**: `Y: [-0.04, 0.06]`
- **RENDER DISTANCE**: `Y: [-0.18, -0.08]` (adjustable 2 to 16, defaults to 8)
- **QUIT**: `Y: [-0.32, -0.22]`

Persistence:
Save and load `render_distance` in `settings.txt`. Format:
```text
fov:70
sensitivity:0.002
render_distance:8
```

---

## 8. Verification Plan

### Manual Verification
1. Launch the game, verify that initial chunks load correctly around the player.
2. Walk in different directions, verify that new chunks load seamlessly, and old chunks unload behind you.
3. Test crossing into negative X and negative Z regions, verify that collision detection still works perfectly.
4. Dig and place blocks at chunk boundaries, verify that face culling updates correctly on both sides of the boundary.
5. Open pause menu, change Render Distance, check that it saves to `settings.txt` and updates active chunks dynamically.
