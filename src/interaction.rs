use glam::Vec3;
use crate::world::{Chunk, BlockType};

pub struct RaycastResult {
    pub block_pos: Vec3, // 命中的方塊整數座標
    pub normal: Vec3,    // 命中的表面法線（用於放置新方塊）
}

pub fn raycast(origin: Vec3, direction: Vec3, max_dist: f32, chunk: &Chunk) -> Option<RaycastResult> {
    // Avoid division by zero/NaN by ensuring direction components are non-zero
    let eps = 1e-8;
    let dx = if direction.x.abs() < eps { direction.x.signum() * eps } else { direction.x };
    let dy = if direction.y.abs() < eps { direction.y.signum() * eps } else { direction.y };
    let dz = if direction.z.abs() < eps { direction.z.signum() * eps } else { direction.z };

    let mut x = origin.x.floor() as i32;
    let mut y = origin.y.floor() as i32;
    let mut z = origin.z.floor() as i32;

    let step_x = if dx > 0.0 { 1 } else { -1 };
    let step_y = if dy > 0.0 { 1 } else { -1 };
    let step_z = if dz > 0.0 { 1 } else { -1 };

    let t_delta_x = (1.0 / dx).abs();
    let t_delta_y = (1.0 / dy).abs();
    let t_delta_z = (1.0 / dz).abs();

    let mut t_max_x = if dx > 0.0 { (x as f32 + 1.0 - origin.x) * t_delta_x } else { (origin.x - x as f32) * t_delta_x };
    let mut t_max_y = if dy > 0.0 { (y as f32 + 1.0 - origin.y) * t_delta_y } else { (origin.y - y as f32) * t_delta_y };
    let mut t_max_z = if dz > 0.0 { (z as f32 + 1.0 - origin.z) * t_delta_z } else { (origin.z - z as f32) * t_delta_z };

    let mut t = 0.0;
    let mut last_face = Vec3::ZERO;

    while t < max_dist {
        let block = chunk.get_block(x, y, z);
        if block != BlockType::Air {
            return Some(RaycastResult {
                block_pos: Vec3::new(x as f32, y as f32, z as f32),
                normal: last_face,
            });
        }

        if t_max_x < t_max_y {
            if t_max_x < t_max_z {
                t = t_max_x;
                x += step_x;
                t_max_x += t_delta_x;
                last_face = Vec3::new(-step_x as f32, 0.0, 0.0);
            } else {
                t = t_max_z;
                z += step_z;
                t_max_z += t_delta_z;
                last_face = Vec3::new(0.0, 0.0, -step_z as f32);
            }
        } else {
            if t_max_y < t_max_z {
                t = t_max_y;
                y += step_y;
                t_max_y += t_delta_y;
                last_face = Vec3::new(0.0, -step_y as f32, 0.0);
            } else {
                t = t_max_z;
                z += step_z;
                t_max_z += t_delta_z;
                last_face = Vec3::new(0.0, 0.0, -step_z as f32);
            }
        }
    }

    None
}
