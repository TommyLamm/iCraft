# Block Breaking Animation & Particle System Design

This document details the design for introducing the block breaking animation, a lightweight particle system (supporting block breaking debris, footstep dust, and torch smoke), and floating/rotating animations for dropped item entities.

---

## 1. Overview

The goal is to enhance the visual feedback and gameplay immersion of the voxel world by adding:
- **Block Breaking Overlay**: A smoother multiply-blend texture overlay that scales from stage 0 to 9 on mined blocks.
- **Lightweight Particle System**: A CPU-driven particle physics and mesh generation system that packs billboard quads into a dynamic vertex buffer, rendered via the GPU using existing texture atlas references.
- **Specific Emitters**:
  - *Debris*: Spawns small quads matching the texture of the block being broken.
  - *Footsteps*: Spawns small quads matching the ground texture when moving.
  - *Smoke*: Spawns rising grey particles at active torch block locations.
- **Dropped Item Entities**: Real entities spawned in the world that float (sinusoidal bobbing) and rotate.

---

## 2. Block Breaking Animation Overlay

Currently, a cracking overlay is rendered by creating a slightly scaled cube (`1.002f32`) centered at the mined block, textured with procedural crack patterns located at Row 15 of the texture atlas.

### 2.1. Loading 10-Stage Crack Textures
To support high-quality textures, we will load 10 png files from `assets/textures/destroy_stages/destroy_stage_0.png` through `destroy_stage_9.png`.
If these files are missing or load fails (e.g. running in a self-contained single-binary mode), the game will fallback to the existing procedural crack generator (`draw_crack_pattern` in `src/texture.rs`).
During `TextureAtlas::new_procedural`, we will attempt to read the png files and stitch them into Row 15 of the 256x256 texture atlas:
- Column 0: Stage 0 (minimal crack)
- Column 9: Stage 9 (extreme crack)

### 2.2. Multiply Blending in Shader
To make the cracking overlay look natural, instead of simply drawing black lines over the block, we will modify the fragment shader `src/shader.wgsl` to support a blend operation, or configure WGPU to render the crack mesh using a multiply-blend color write state:
```rust
// In src/state.rs, update render pipeline blending configuration:
targets: &[Some(wgpu::ColorTargetState {
    format: config.format,
    blend: Some(wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::Dst,     // Multiply blending
            dst_factor: wgpu::BlendFactor::Zero,
            operation: wgpu::BlendOperation::Add,
        },
        alpha: wgpu::BlendComponent::REPLACE,
    }),
    write_mask: wgpu::ColorWrites::ALL,
})],
```
This forces the texture color of the crack (which has transparent backgrounds and dark lines) to multiply with the destination color (the already drawn block underneath), creating a realistic crack shadow effect.

---

## 3. Lightweight Particle System

A dedicated particle module `src/particles.rs` will be introduced. To keep execution fast, particles are managed separately from heavyweight game entities.

### 3.1. Particle and Emitter Structures (`src/particles.rs`)
```rust
use glam::Vec3;
use crate::world::BlockType;

pub struct Particle {
    pub position: Vec3,
    pub velocity: Vec3,
    pub lifetime: f32,
    pub age: f32,
    pub size: f32,
    // Tex coords in the atlas mapping [u0, v0, u1, v1]
    pub tex_coords: [f32; 4],
    pub gravity: f32,
}

pub struct ParticleSystem {
    pub particles: Vec<Particle>,
    vertex_buffer: Option<wgpu::Buffer>,
    index_buffer: Option<wgpu::Buffer>,
}
```

### 3.2. Particle Physics and Emitters
The `ParticleSystem` will expose methods to update and spawn specific particle types:

```rust
impl ParticleSystem {
    pub fn update(&mut self, dt: f32, chunk_manager: &crate::chunk_manager::ChunkManager) {
        self.particles.retain_mut(|p| {
            // Apply gravity
            p.velocity.y -= p.gravity * dt;
            
            // Basic physics update
            p.position += p.velocity * dt;
            p.age += dt;
            
            // Keep particle if it hasn't expired
            p.age < p.lifetime
        });
    }
}
```

### 3.3. Particle Billboarding and Vertex Compilation
To render particles efficiently, each particle is drawn as a 2D quad that rotates to always face the camera (billboarding).
Inside `ParticleSystem::compile_mesh`, we build a vertex array from the active particles. The quad corners are calculated using the camera's right and up vectors:

```rust
pub fn compile_mesh(
    &self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    camera_pos: Vec3,
    cam_right: Vec3,
    cam_up: Vec3,
) -> Option<(wgpu::Buffer, wgpu::Buffer, u32)> {
    if self.particles.is_empty() {
        return None;
    }

    let mut vertices = Vec::with_capacity(self.particles.len() * 4);
    let mut indices = Vec::with_capacity(self.particles.len() * 6);

    for (i, p) in self.particles.iter().enumerate() {
        let start_idx = (i * 4) as u32;
        let half_size = p.size * 0.5;

        // Quad corners offset using camera orientation
        let c0 = p.position - cam_right * half_size - cam_up * half_size;
        let c1 = p.position + cam_right * half_size - cam_up * half_size;
        let c2 = p.position + cam_right * half_size + cam_up * half_size;
        let c3 = p.position - cam_right * half_size + cam_up * half_size;

        let u0 = p.tex_coords[0];
        let v0 = p.tex_coords[1];
        let u1 = p.tex_coords[2];
        let v1 = p.tex_coords[3];

        // Retrieve light level at particle center
        // We push Vertex structs (reusing crate::state::Vertex)
        vertices.push(Vertex { position: c0.into(), tex_coords: [u0, v1], light_level: 240.0 });
        vertices.push(Vertex { position: c1.into(), tex_coords: [u1, v1], light_level: 240.0 });
        vertices.push(Vertex { position: c2.into(), tex_coords: [u1, v0], light_level: 240.0 });
        vertices.push(Vertex { position: c3.into(), tex_coords: [u0, v0], light_level: 240.0 });

        indices.push(start_idx + 0);
        indices.push(start_idx + 1);
        indices.push(start_idx + 2);
        indices.push(start_idx + 0);
        indices.push(start_idx + 2);
        indices.push(start_idx + 3);
    }

    // Allocate / write buffers
    // ...
}
```

---

## 4. Specific Emitters

### 4.1. Block Breaking Debris
When a block is broken in `State::break_block`, we spawn 15-25 particles.
- **Colors & Textures**: To give particles authentic colors, their UVs point to a tiny random sub-rect (e.g. 2x2 texels) inside the corresponding block texture tile in the atlas. For example, wood particles map to wood texture UVs.
- **Motion**: Initial velocities are randomly distributed in a sphere, with gravity pushing them down.

### 4.2. Footstep Dust
In `State::update`, when the player is moving on the ground (`player_physics.on_ground` is true), we accumulate distance. Every few steps (e.g. every 1.5 meters), we spawn 4-8 tiny dust particles from the block below the player's feet.
- **Texture**: Reuses sub-rect UVs of the block type directly below the player.
- **Motion**: Sprayed backwards and upwards.

### 4.3. Torch Smoke
Every tick (or random interval), active chunks inspect loaded torch blocks. If a torch is present, we spawn a smoke particle at the top center of the torch (`torch_pos + Vec3::new(0.5, 0.6, 0.5)`).
- **Appearance**: Reuses a tiny, semi-transparent grey pixel square in the atlas or a procedurally drawn smoke tile.
- **Motion**: Slowly rises upward (`velocity.y` = 0.5 to 1.2 m/s, `velocity.x`/`velocity.z` with slight horizontal drift), fading out by scaling size down as age approaches lifetime.

---

## 5. Dropped Item Entity Bobbing & Rotating

Currently, mined blocks are immediately inserted into the player inventory. We will introduce real dropped item entities.

### 5.1. Entity Struct Extension (`src/entity.rs`)
Add `EntityType::DroppedItem` to the entity types:
```rust
pub enum EntityType {
    // ...
    HeartParticle,
    DroppedItem,
}
```

In `src/inventory.rs` / `src/entity.rs`, define mapping from `Item` types to dropped entity mesh properties.

### 5.2. Physics and Collection
- **Physics**: Dropped items fall with gravity and slide/collide with blocks using the existing `update_physics` and `resolve_collisions` in `src/entity.rs`.
- **Collection**: If a player's distance to a `DroppedItem` is less than `1.5` meters, the item is collected, added to the inventory, and its entity is despawned.

### 5.3. Sinusoidal Bobbing and Rotation
In `src/mob_renderer.rs::render_mobs`, when an entity has `EntityType::DroppedItem`:
- Apply rotation based on global game time: `yaw = time * 2.0` (radians).
- Apply vertical float offset using a sine wave: `y_offset = (time * 3.0).sin() * 0.1`.
- Render as a scaled-down 3D block mesh (e.g. `0.25` scale) or a flat double-sided sprite in the center.

---

## 6. Verification Plan

### 6.1. Automated Unit Tests
- **Particle Updates**: Test that `ParticleSystem::update` decrements lifetime, applies gravity, and correctly drops expired particles.
- **Dropped Item Collection**: Test that player coordinates near a dropped item triggers collection, adds it to the player inventory, and removes the entity.

### 6.2. Manual Verification
1. **Mining Overlay**: Start mining a stone block in survival mode. Confirm that crack textures darken and overlay the block texture.
2. **Break Debris**: Break a leaf/wood/stone block. Verify that 15+ tiny colored quads matching the block texture spray out and fall to the ground.
3. **Footsteps**: Walk on grass, dirt, and stone. Confirm that matching-colored dust quads puff out from beneath the feet.
4. **Torch Smoke**: Place a torch. Verify that grey smoke particles rise slowly from the top of the torch and fade out.
5. **Dropped Items**: Drop an item (or mine a block with inventory full). Verify that the item floats, bobs up and down smoothly, rotates, and can be collected when walked over.
