use glam::Vec3;
use crate::state::Vertex;
use crate::entity::{EntityType, EntityManager};
use crate::chunk_manager::ChunkManager;

// South (+Z), North (-Z), West (-X), East (+X), Up (+Y), Down (-Y)
pub fn add_cuboid(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
    size: Vec3,
    offset: Vec3,
    pivot: Vec3,
    rot_yaw: f32,
    rot_pitch: f32,
    tex_cols: [u32; 6], // Columns: [Front, Back, Left, Right, Top, Bottom]
    tex_row: u32,       // Row 9 for mob skins
    light_val: f32,
) {
    let half = size * 0.5;

    // Corner coordinates for box faces: South (+Z), North (-Z), West (-X), East (+X), Up (+Y), Down (-Y)
    let local_corners = [
        // Face 0: South (+Z)
        (Vec3::new(-half.x, -half.y,  half.z), [0.0, 1.0]),
        (Vec3::new( half.x, -half.y,  half.z), [1.0, 1.0]),
        (Vec3::new( half.x,  half.y,  half.z), [1.0, 0.0]),
        (Vec3::new(-half.x,  half.y,  half.z), [0.0, 0.0]),

        // Face 1: North (-Z)
        (Vec3::new( half.x, -half.y, -half.z), [0.0, 1.0]),
        (Vec3::new(-half.x, -half.y, -half.z), [1.0, 1.0]),
        (Vec3::new(-half.x,  half.y, -half.z), [1.0, 0.0]),
        (Vec3::new( half.x,  half.y, -half.z), [0.0, 0.0]),

        // Face 2: West (-X)
        (Vec3::new(-half.x, -half.y, -half.z), [0.0, 1.0]),
        (Vec3::new(-half.x, -half.y,  half.z), [1.0, 1.0]),
        (Vec3::new(-half.x,  half.y,  half.z), [1.0, 0.0]),
        (Vec3::new(-half.x,  half.y, -half.z), [0.0, 0.0]),

        // Face 3: East (+X)
        (Vec3::new( half.x, -half.y,  half.z), [0.0, 1.0]),
        (Vec3::new( half.x, -half.y, -half.z), [1.0, 1.0]),
        (Vec3::new( half.x,  half.y, -half.z), [1.0, 0.0]),
        (Vec3::new( half.x,  half.y,  half.z), [0.0, 0.0]),

        // Face 4: Up (+Y)
        (Vec3::new(-half.x,  half.y,  half.z), [0.0, 1.0]),
        (Vec3::new( half.x,  half.y,  half.z), [1.0, 1.0]),
        (Vec3::new( half.x,  half.y, -half.z), [1.0, 0.0]),
        (Vec3::new(-half.x,  half.y, -half.z), [0.0, 0.0]),

        // Face 5: Down (-Y)
        (Vec3::new(-half.x, -half.y, -half.z), [0.0, 1.0]),
        (Vec3::new( half.x, -half.y, -half.z), [1.0, 1.0]),
        (Vec3::new( half.x, -half.y,  half.z), [1.0, 0.0]),
        (Vec3::new(-half.x, -half.y,  half.z), [0.0, 0.0]),
    ];

    let cos_pitch = rot_pitch.cos();
    let sin_pitch = rot_pitch.sin();
    let cos_yaw = rot_yaw.cos();
    let sin_yaw = rot_yaw.sin();

    let start_idx = vertices.len() as u32;

    for (face_idx, (local_pos, uv)) in local_corners.iter().enumerate() {
        // Shift by offset relative to joint pivot
        let v1 = *local_pos + offset;

        // Pitch rotation (around local X axis)
        let v2 = Vec3::new(
            v1.x,
            v1.y * cos_pitch - v1.z * sin_pitch,
            v1.y * sin_pitch + v1.z * cos_pitch,
        );

        // Yaw rotation (around local Y axis)
        let v3 = Vec3::new(
            v2.x * cos_yaw + v2.z * sin_yaw,
            v2.y,
            -v2.x * sin_yaw + v2.z * cos_yaw,
        );

        // Translate to global pivot in world space
        let final_pos = v3 + pivot;

        // Compute UV coordinate relative to 16x16 tile mapping
        let col = tex_cols[face_idx / 4];
        let u = (uv[0] + col as f32) * 0.0625;
        let v = (uv[1] + tex_row as f32) * 0.0625;

        vertices.push(Vertex {
            position: [final_pos.x, final_pos.y, final_pos.z],
            tex_coords: [u, v],
            light_level: light_val,
        });
    }

    // Connect indices for the 6 faces (each face has 4 vertices, 2 triangles)
    for f in 0..6 {
        let f_start = start_idx + (f * 4);
        indices.push(f_start + 0);
        indices.push(f_start + 1);
        indices.push(f_start + 2);

        indices.push(f_start + 0);
        indices.push(f_start + 2);
        indices.push(f_start + 3);
    }
}

pub fn render_mobs(
    entity_manager: &EntityManager,
    chunk_manager: &ChunkManager,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
    time: f32,
) {
    for entity in &entity_manager.entities {
        // Retrieve light level at entity position
        let mx = entity.position.x.floor() as i32;
        let my = entity.position.y.floor() as i32;
        let mz = entity.position.z.floor() as i32;
        let sky_l = chunk_manager.get_sky_light(mx, my, mz);
        let block_l = chunk_manager.get_block_light(mx, my, mz);
        
        // Base light packed value
        let mut light_val = (sky_l as f32) + (block_l as f32) * 16.0;
        
        // If entity recently took damage, add 1024 to trigger shader redness flashing
        if entity.invulnerable_time > 0.0 {
            light_val += 1024.0;
        }

        // Calculate walk swing animation factor based on horizontal velocity
        let speed_2d = Vec3::new(entity.velocity.x, 0.0, entity.velocity.z).length();
        let walking = speed_2d > 0.1;
        let swing = if walking {
            (time * 8.0).sin() * 0.6
        } else {
            0.0
        };

        match entity.entity_type {
            EntityType::Zombie => {
                // Head (Col 0 front face, Col 1 others)
                add_cuboid(
                    vertices, indices,
                    Vec3::new(0.5, 0.5, 0.5),
                    Vec3::new(0.0, 0.25, 0.0),
                    entity.position + Vec3::new(0.0, 1.4, 0.0),
                    entity.yaw, entity.pitch,
                    [0, 1, 1, 1, 1, 1], // front is col 0, others col 1
                    9, light_val
                );

                // Torso (Col 2)
                add_cuboid(
                    vertices, indices,
                    Vec3::new(0.5, 0.75, 0.25),
                    Vec3::new(0.0, 0.375, 0.0),
                    entity.position + Vec3::new(0.0, 0.65, 0.0),
                    entity.yaw, 0.0,
                    [2; 6],
                    9, light_val
                );

                // Zombie Arms: raised forward
                let arm_pitch = -std::f32::consts::FRAC_PI_2;
                // Left Arm (Col 3)
                add_cuboid(
                    vertices, indices,
                    Vec3::new(0.2, 0.75, 0.2),
                    Vec3::new(0.0, -0.325, 0.0),
                    entity.position + Vec3::new(-0.35, 1.3, 0.0),
                    entity.yaw, arm_pitch,
                    [3; 6],
                    9, light_val
                );
                // Right Arm (Col 3)
                add_cuboid(
                    vertices, indices,
                    Vec3::new(0.2, 0.75, 0.2),
                    Vec3::new(0.0, -0.325, 0.0),
                    entity.position + Vec3::new(0.35, 1.3, 0.0),
                    entity.yaw, arm_pitch,
                    [3; 6],
                    9, light_val
                );

                // Legs (Col 3)
                // Left Leg
                add_cuboid(
                    vertices, indices,
                    Vec3::new(0.2, 0.75, 0.2),
                    Vec3::new(0.0, -0.375, 0.0),
                    entity.position + Vec3::new(-0.125, 0.75, 0.0),
                    entity.yaw, swing,
                    [3; 6],
                    9, light_val
                );
                // Right Leg
                add_cuboid(
                    vertices, indices,
                    Vec3::new(0.2, 0.75, 0.2),
                    Vec3::new(0.0, -0.375, 0.0),
                    entity.position + Vec3::new(0.125, 0.75, 0.0),
                    entity.yaw, -swing,
                    [3; 6],
                    9, light_val
                );
            }
            EntityType::Skeleton => {
                // Head (Col 4 front face, Col 5 others)
                add_cuboid(
                    vertices, indices,
                    Vec3::new(0.5, 0.5, 0.5),
                    Vec3::new(0.0, 0.25, 0.0),
                    entity.position + Vec3::new(0.0, 1.4, 0.0),
                    entity.yaw, entity.pitch,
                    [4, 5, 5, 5, 5, 5],
                    9, light_val
                );

                // Torso (Col 5)
                add_cuboid(
                    vertices, indices,
                    Vec3::new(0.4, 0.75, 0.2),
                    Vec3::new(0.0, 0.375, 0.0),
                    entity.position + Vec3::new(0.0, 0.65, 0.0),
                    entity.yaw, 0.0,
                    [5; 6],
                    9, light_val
                );

                // Skeleton Arms: raised to aim bow if target_player is true, otherwise swing alternately
                let left_arm_pitch = if entity.target_player { -std::f32::consts::FRAC_PI_2 } else { -swing };
                let right_arm_pitch = if entity.target_player { -std::f32::consts::FRAC_PI_2 } else { swing };

                // Left Arm (Col 5)
                add_cuboid(
                    vertices, indices,
                    Vec3::new(0.15, 0.75, 0.15),
                    Vec3::new(0.0, -0.325, 0.0),
                    entity.position + Vec3::new(-0.275, 1.3, 0.0),
                    entity.yaw, left_arm_pitch,
                    [5; 6],
                    9, light_val
                );
                // Right Arm (Col 5)
                add_cuboid(
                    vertices, indices,
                    Vec3::new(0.15, 0.75, 0.15),
                    Vec3::new(0.0, -0.325, 0.0),
                    entity.position + Vec3::new(0.275, 1.3, 0.0),
                    entity.yaw, right_arm_pitch,
                    [5; 6],
                    9, light_val
                );

                // Legs (Col 5)
                // Left Leg
                add_cuboid(
                    vertices, indices,
                    Vec3::new(0.15, 0.75, 0.15),
                    Vec3::new(0.0, -0.375, 0.0),
                    entity.position + Vec3::new(-0.1, 0.75, 0.0),
                    entity.yaw, swing,
                    [5; 6],
                    9, light_val
                );
                // Right Leg
                add_cuboid(
                    vertices, indices,
                    Vec3::new(0.15, 0.75, 0.15),
                    Vec3::new(0.0, -0.375, 0.0),
                    entity.position + Vec3::new(0.1, 0.75, 0.0),
                    entity.yaw, -swing,
                    [5; 6],
                    9, light_val
                );
            }
            EntityType::Creeper => {
                // Creeper swelling expansion scale during fuse count down
                let scale = if entity.is_ignited {
                    let progress = ((1.5 - entity.action_cooldown) / 1.5).clamp(0.0, 1.0);
                    1.0 + 0.15 * progress * (time * 35.0).sin().abs()
                } else {
                    1.0
                };

                // Head (Col 6 front, Col 7 others)
                add_cuboid(
                    vertices, indices,
                    Vec3::new(0.5, 0.5, 0.5) * scale,
                    Vec3::new(0.0, 0.25, 0.0) * scale,
                    entity.position + Vec3::new(0.0, 1.2, 0.0),
                    entity.yaw, entity.pitch,
                    [6, 7, 7, 7, 7, 7],
                    9, light_val
                );

                // Torso (Col 7)
                add_cuboid(
                    vertices, indices,
                    Vec3::new(0.5, 0.75, 0.3) * scale,
                    Vec3::new(0.0, 0.375, 0.0) * scale,
                    entity.position + Vec3::new(0.0, 0.45, 0.0),
                    entity.yaw, 0.0,
                    [7; 6],
                    9, light_val
                );

                // Creeper has 4 legs (Col 7)
                // Front Left Leg
                add_cuboid(
                    vertices, indices,
                    Vec3::new(0.2, 0.35, 0.2) * scale,
                    Vec3::new(0.0, -0.175, 0.0) * scale,
                    entity.position + Vec3::new(-0.15, 0.35, 0.15),
                    entity.yaw, swing,
                    [7; 6],
                    9, light_val
                );
                // Front Right Leg
                add_cuboid(
                    vertices, indices,
                    Vec3::new(0.2, 0.35, 0.2) * scale,
                    Vec3::new(0.0, -0.175, 0.0) * scale,
                    entity.position + Vec3::new(0.15, 0.35, 0.15),
                    entity.yaw, -swing,
                    [7; 6],
                    9, light_val
                );
                // Back Left Leg
                add_cuboid(
                    vertices, indices,
                    Vec3::new(0.2, 0.35, 0.2) * scale,
                    Vec3::new(0.0, -0.175, 0.0) * scale,
                    entity.position + Vec3::new(-0.15, 0.35, -0.15),
                    entity.yaw, -swing,
                    [7; 6],
                    9, light_val
                );
                // Back Right Leg
                add_cuboid(
                    vertices, indices,
                    Vec3::new(0.2, 0.35, 0.2) * scale,
                    Vec3::new(0.0, -0.175, 0.0) * scale,
                    entity.position + Vec3::new(0.15, 0.35, -0.15),
                    entity.yaw, swing,
                    [7; 6],
                    9, light_val
                );
            }
            EntityType::Arrow => {
                // Render arrow as a thin box (skin Col 8)
                add_cuboid(
                    vertices, indices,
                    Vec3::new(0.06, 0.06, 0.6),
                    Vec3::new(0.0, 0.0, 0.0),
                    entity.position,
                    entity.yaw, entity.pitch,
                    [8; 6],
                    9, light_val
                );
            }
            _ => {}
        }
    }
}
