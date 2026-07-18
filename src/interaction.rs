use glam::Vec3;
use crate::world::BlockType;
use crate::chunk_manager::ChunkManager;

pub struct RaycastResult {
    pub block_pos: Vec3, // 命中的方塊整數座標
    pub normal: Vec3,    // 命中的表面法線（用於放置新方塊）
}

pub fn raycast(origin: Vec3, direction: Vec3, max_dist: f32, chunk_manager: &ChunkManager) -> Option<RaycastResult> {
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
        let block = chunk_manager.get_block(x, y, z);
        let props = block.properties();
        if block != BlockType::Air && !props.is_passable {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{Chunk, BlockType};
    use crate::chunk_manager::ChunkManager;
    use glam::Vec3;

    #[test]
    fn test_raycast_air() {
        let mut chunk_manager = ChunkManager::new(8);
        chunk_manager.chunks.insert((0, 0), Chunk::new(0, 0));
        // Look up into the sky from the surface
        let hit = raycast(Vec3::new(8.0, 70.0, 8.0), Vec3::new(0.0, 1.0, 0.0), 10.0, &chunk_manager);
        assert!(hit.is_none());
    }

    #[test]
    fn test_raycast_hit() {
        let mut chunk_manager = ChunkManager::new(8);
        let mut chunk = Chunk::new(0, 0);
        // Place a block in the air
        chunk.blocks[8][72][8] = BlockType::Stone;
        chunk_manager.chunks.insert((0, 0), chunk);

        // Look straight up from 8.5, 70.5, 8.5 (distance 2.0 to the block min y=72)
        let hit = raycast(Vec3::new(8.5, 70.5, 8.5), Vec3::new(0.0, 1.0, 0.0), 5.0, &chunk_manager);
        assert!(hit.is_some());
        let res = hit.unwrap();
        assert_eq!(res.block_pos, Vec3::new(8.0, 72.0, 8.0));
        assert_eq!(res.normal, Vec3::new(0.0, -1.0, 0.0)); // Ray hits bottom face, normal points down
    }
}

