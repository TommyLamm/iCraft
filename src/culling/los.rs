use glam::Vec3;

/// Fast 3D DDA voxel line-of-sight raycast.
pub fn is_los_blocked<F>(origin: Vec3, target: Vec3, mut is_occluder: F) -> bool
where
    F: FnMut(i32, i32, i32) -> bool,
{
    let dir = target - origin;
    let dist = dir.length();
    if dist < 0.001 {
        return false;
    }
    let norm = dir / dist;

    let mut x = origin.x.floor() as i32;
    let mut y = origin.y.floor() as i32;
    let mut z = origin.z.floor() as i32;

    let target_x = target.x.floor() as i32;
    let target_y = target.y.floor() as i32;
    let target_z = target.z.floor() as i32;

    let step_x = if norm.x > 0.0 {
        1
    } else if norm.x < 0.0 {
        -1
    } else {
        0
    };
    let step_y = if norm.y > 0.0 {
        1
    } else if norm.y < 0.0 {
        -1
    } else {
        0
    };
    let step_z = if norm.z > 0.0 {
        1
    } else if norm.z < 0.0 {
        -1
    } else {
        0
    };

    let delta_x = if step_x != 0 {
        (1.0 / norm.x.abs()).min(100.0)
    } else {
        100.0
    };
    let delta_y = if step_y != 0 {
        (1.0 / norm.y.abs()).min(100.0)
    } else {
        100.0
    };
    let delta_z = if step_z != 0 {
        (1.0 / norm.z.abs()).min(100.0)
    } else {
        100.0
    };

    let mut t_max_x = if step_x > 0 {
        (x as f32 + 1.0 - origin.x) * delta_x
    } else if step_x < 0 {
        (origin.x - x as f32) * delta_x
    } else {
        f32::INFINITY
    };
    let mut t_max_y = if step_y > 0 {
        (y as f32 + 1.0 - origin.y) * delta_y
    } else if step_y < 0 {
        (origin.y - y as f32) * delta_y
    } else {
        f32::INFINITY
    };
    let mut t_max_z = if step_z > 0 {
        (z as f32 + 1.0 - origin.z) * delta_z
    } else if step_z < 0 {
        (origin.z - z as f32) * delta_z
    } else {
        f32::INFINITY
    };

    let start_x = x;
    let start_y = y;
    let start_z = z;

    for _ in 0..48 {
        if x == target_x && y == target_y && z == target_z {
            return false;
        }

        if !(x == start_x && y == start_y && z == start_z) {
            if is_occluder(x, y, z) {
                return true;
            }
        }

        if t_max_x < t_max_y {
            if t_max_x < t_max_z {
                x += step_x;
                t_max_x += delta_x;
            } else {
                z += step_z;
                t_max_z += delta_z;
            }
        } else {
            if t_max_y < t_max_z {
                y += step_y;
                t_max_y += delta_y;
            } else {
                z += step_z;
                t_max_z += delta_z;
            }
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::culling::connectivity::is_section_occluder;

    #[test]
    fn opaque_cube_blocks_los() {
        assert!(is_los_blocked(
            Vec3::new(0.2, 1.2, 0.2),
            Vec3::new(3.8, 1.2, 0.2),
            |x, y, z| {
                (x, y, z) == (1, 1, 0) && is_section_occluder(crate::world::BlockType::Stone)
            }
        ));
    }
}
