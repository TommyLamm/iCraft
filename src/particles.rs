use glam::Vec3;
use wgpu::{Buffer, Device, Queue};

use crate::state::Vertex;
use crate::world::BlockType;

/// Maximum number of particles the system can render in a single frame.
/// 4096 vertices would only cover 1024 quads; we instead size buffers to hold
/// up to `MAX_PARTICLES` quads (4 verts + 6 indices each).
pub const MAX_PARTICLES: usize = 4096;

/// A single billboard particle. UVs map into the 256x256 texture atlas.
#[derive(Clone, Debug)]
pub struct Particle {
    pub position: Vec3,
    pub velocity: Vec3,
    pub lifetime: f32,
    pub age: f32,
    pub size: f32,
    /// Atlas UV rect: `[u0, v0, u1, v1]`.
    pub tex_coords: [f32; 4],
    pub gravity: f32,
    /// When true the particle shrinks as it ages (used for smoke).
    pub fade_scale: bool,
}

pub struct ParticleSystem {
    pub particles: Vec<Particle>,
}

impl Default for ParticleSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl ParticleSystem {
    pub fn new() -> Self {
        Self {
            particles: Vec::new(),
        }
    }

    pub fn spawn(
        &mut self,
        position: Vec3,
        velocity: Vec3,
        size: f32,
        lifetime: f32,
        tex_coords: [f32; 4],
        gravity: f32,
    ) {
        if self.particles.len() >= MAX_PARTICLES {
            return;
        }
        self.particles.push(Particle {
            position,
            velocity,
            lifetime,
            age: 0.0,
            size,
            tex_coords,
            gravity,
            fade_scale: false,
        });
    }

    /// Advance all particles by `dt` seconds, dropping expired ones.
    pub fn update(&mut self, dt: f32) {
        self.particles.retain_mut(|p| {
            p.velocity.y -= p.gravity * dt;
            p.position += p.velocity * dt;
            p.age += dt;
            p.age < p.lifetime
        });
    }

    /// Build billboard quads facing the camera and write them into the
    /// pre-allocated dynamic vertex/index buffers. Returns the number of
    /// indices to draw, or `None` if there are no particles.
    pub fn compile_mesh(
        &self,
        device: &Device,
        queue: &Queue,
        cam_right: Vec3,
        cam_up: Vec3,
        vertex_buffer: &Buffer,
        index_buffer: &Buffer,
    ) -> Option<u32> {
        if self.particles.is_empty() {
            return None;
        }

        let cam_right = cam_right.normalize_or_zero();
        let cam_up = cam_up.normalize_or_zero();

        let mut vertices: Vec<Vertex> = Vec::with_capacity(self.particles.len() * 4);
        let mut indices: Vec<u32> = Vec::with_capacity(self.particles.len() * 6);

        for (i, p) in self.particles.iter().enumerate() {
            let start_idx = (i * 4) as u32;

            // Particles optionally shrink as they age (smoke fade-out).
            let scale = if p.fade_scale {
                let t = (p.age / p.lifetime).clamp(0.0, 1.0);
                (1.0 - t).max(0.05)
            } else {
                1.0
            };
            let half_size = p.size * 0.5 * scale;

            let c0 = p.position - cam_right * half_size - cam_up * half_size;
            let c1 = p.position + cam_right * half_size - cam_up * half_size;
            let c2 = p.position + cam_right * half_size + cam_up * half_size;
            let c3 = p.position - cam_right * half_size + cam_up * half_size;

            let [u0, v0, u1, v1] = p.tex_coords;

            // Particles render fully lit (240 = ~max light).
            vertices.push(Vertex {
                position: c0.into(),
                tex_coords: [u0, v1],
                light_level: 240.0,
                ao: 1.0,
            });
            vertices.push(Vertex {
                position: c1.into(),
                tex_coords: [u1, v1],
                light_level: 240.0,
                ao: 1.0,
            });
            vertices.push(Vertex {
                position: c2.into(),
                tex_coords: [u1, v0],
                light_level: 240.0,
                ao: 1.0,
            });
            vertices.push(Vertex {
                position: c3.into(),
                tex_coords: [u0, v0],
                light_level: 240.0,
                ao: 1.0,
            });

            indices.push(start_idx + 0);
            indices.push(start_idx + 1);
            indices.push(start_idx + 2);
            indices.push(start_idx + 0);
            indices.push(start_idx + 2);
            indices.push(start_idx + 3);
        }

        // Truncate to the buffer capacity (safety against overflow).
        let max_quads = MAX_PARTICLES;
        if vertices.len() > max_quads * 4 {
            vertices.truncate(max_quads * 4);
        }
        if indices.len() > max_quads * 6 {
            indices.truncate(max_quads * 6);
        }

        if vertices.is_empty() {
            return None;
        }

        queue.write_buffer(vertex_buffer, 0, bytemuck::cast_slice(&vertices));
        queue.write_buffer(index_buffer, 0, bytemuck::cast_slice(&indices));

        let _ = device; // device reserved for future allocations

        Some(indices.len() as u32)
    }
}

/// Atlas layout helpers -------------------------------------------------------

/// Tile size in texels (16x16) inside the 256x256 atlas.
const ATLAS_TILES_PER_ROW: f32 = 16.0;

/// Convert a `(col, row)` atlas tile coordinate into a `[u0, v0, u1, v1]` UV
/// rect covering the entire tile.
pub fn tile_uv(col: u32, row: u32) -> [f32; 4] {
    let u0 = col as f32 / ATLAS_TILES_PER_ROW;
    let v0 = row as f32 / ATLAS_TILES_PER_ROW;
    let u1 = (col + 1) as f32 / ATLAS_TILES_PER_ROW;
    let v1 = (row + 1) as f32 / ATLAS_TILES_PER_ROW;
    [u0, v0, u1, v1]
}

/// Pick a small random sub-rect (roughly 2x2 texels) inside the tile for the
/// given block, so debris particles pick up authentic colors instead of the
/// full tile. Falls back to the full tile if the block has no atlas mapping.
pub fn block_debris_uv(block: BlockType, rng: &mut u32) -> [f32; 4] {
    let (col, row) = block.get_face_tex_index(4); // top face texture
    let mut next = |min: i32, max: i32| -> i32 {
        *rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
        let val = (*rng / 65536) % 32768;
        let diff = max - min;
        if diff <= 0 {
            min
        } else {
            min + (val as i32 % diff)
        }
    };
    // Pick a 3x3-texel window inside the 16x16 tile and map to UV space.
    let tx = next(2, 12) as f32 / 16.0;
    let ty = next(2, 12) as f32 / 16.0;
    let sub = 3.0 / 16.0;
    let u0 = (col as f32 + tx) / ATLAS_TILES_PER_ROW;
    let v0 = (row as f32 + ty) / ATLAS_TILES_PER_ROW;
    let u1 = u0 + sub / ATLAS_TILES_PER_ROW;
    let v1 = v0 + sub / ATLAS_TILES_PER_ROW;
    [u0, v0, u1, v1]
}

/// Spawn `count` debris particles for a freshly-broken block at `pos`. The
/// particles inherit a small random sub-rect of the block's top-face texture.
pub fn spawn_block_debris(
    system: &mut ParticleSystem,
    pos: Vec3,
    block: BlockType,
    count: usize,
    rng: &mut u32,
) {
    for _ in 0..count {
        let theta = (*rng as f32 / 32768.0) * std::f32::consts::TAU;
        *rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
        let phi = (*rng as f32 / 32768.0) * std::f32::consts::FRAC_PI_2;
        *rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
        let speed = 2.0 + (*rng as f32 / 32768.0) * 2.5;
        *rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
        let vx = theta.cos() * phi.cos() * speed;
        let vz = theta.sin() * phi.cos() * speed;
        let vy = 2.0 + phi.sin() * speed;
        let tex = block_debris_uv(block, rng);
        system.spawn(
            pos,
            Vec3::new(vx, vy, vz),
            0.08,
            0.8 + (*rng as f32 / 32768.0) * 0.6,
            tex,
            9.81,
        );
        *rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
    }
}

/// Spawn a few footstep dust particles at the player's feet, using the texture
/// of the block directly below.
pub fn spawn_footstep_dust(
    system: &mut ParticleSystem,
    feet_pos: Vec3,
    block: BlockType,
    rng: &mut u32,
) {
    let count = 4 + ((*rng / 32768) % 5) as usize;
    *rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
    for _ in 0..count {
        let tex = block_debris_uv(block, rng);
        let theta = (*rng as f32 / 32768.0) * std::f32::consts::TAU;
        *rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
        let speed = 0.5 + (*rng as f32 / 32768.0) * 0.8;
        *rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
        // Spray backwards and upwards.
        let vx = theta.cos() * speed * 0.4;
        let vz = theta.sin() * speed * 0.4;
        let vy = 0.6 + (*rng as f32 / 32768.0) * 0.6;
        *rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
        system.spawn(feet_pos, Vec3::new(vx, vy, vz), 0.06, 0.6, tex, 4.0);
    }
}

/// Spawn a single torch smoke particle that rises slowly and fades by scaling
/// down over its lifetime.
pub fn spawn_torch_smoke(system: &mut ParticleSystem, torch_pos: Vec3, rng: &mut u32) {
    let drift_x = ((*rng as f32 / 32768.0) - 0.5) * 0.2;
    *rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
    let drift_z = ((*rng as f32 / 32768.0) - 0.5) * 0.2;
    *rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
    let rise = 0.8 + (*rng as f32 / 32768.0) * 0.4;
    *rng = rng.wrapping_mul(1103515245).wrapping_add(12345);

    // Use a small grey sub-rect from the gravel tile (neutral grey) as smoke.
    let tex = {
        let [u0, v0, u1, v1] = tile_uv(5, 0); // gravel tile
        let cx = (u0 + u1) * 0.5;
        let cy = (v0 + v1) * 0.5;
        let w = (u1 - u0) * 0.12;
        let h = (v1 - v0) * 0.12;
        [cx - w * 0.5, cy - h * 0.5, cx + w * 0.5, cy + h * 0.5]
    };

    let len_before = system.particles.len();
    system.spawn(
        torch_pos,
        Vec3::new(drift_x, rise, drift_z),
        0.12,
        1.6,
        tex,
        -0.4, // slight upward buoyancy (negative gravity)
    );
    // Mark as fade-scale so the smoke shrinks over time.
    if system.particles.len() > len_before {
        if let Some(p) = system.particles.last_mut() {
            p.fade_scale = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn particle_update_applies_gravity_and_expires() {
        let mut sys = ParticleSystem::new();
        sys.spawn(
            Vec3::new(0.0, 10.0, 0.0),
            Vec3::new(0.0, 0.0, 0.0),
            0.2,
            1.0,
            [0.0, 0.0, 1.0, 1.0],
            9.81,
        );

        // After 0.5s the particle should have fallen and aged.
        sys.update(0.5);
        assert_eq!(sys.particles.len(), 1);
        let p = &sys.particles[0];
        assert!((p.age - 0.5).abs() < 1e-4);
        assert!(p.velocity.y < 0.0, "gravity should pull particle down");
        assert!(p.position.y < 10.0, "particle should have fallen");

        // Exceeding lifetime drops the particle.
        sys.update(0.6);
        assert!(
            sys.particles.is_empty(),
            "expired particle should be removed"
        );
    }

    #[test]
    fn spawn_respects_max_capacity() {
        let mut sys = ParticleSystem::new();
        for _ in 0..(MAX_PARTICLES + 100) {
            sys.spawn(Vec3::ZERO, Vec3::ZERO, 0.1, 1.0, [0.0, 0.0, 1.0, 1.0], 0.0);
        }
        assert_eq!(sys.particles.len(), MAX_PARTICLES);
    }

    #[test]
    fn fade_scale_particle_shrinks_with_age() {
        let mut sys = ParticleSystem::new();
        sys.spawn(
            Vec3::new(0.0, 5.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            0.3,
            2.0,
            [0.0, 0.0, 1.0, 1.0],
            0.0,
        );
        // Mark it as a fading smoke particle.
        sys.particles[0].fade_scale = true;
        sys.update(1.0);
        assert_eq!(sys.particles.len(), 1);
        // Age advanced.
        assert!((sys.particles[0].age - 1.0).abs() < 1e-4);
    }
}
