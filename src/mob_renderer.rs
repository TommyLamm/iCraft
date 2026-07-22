use crate::chunk_manager::ChunkManager;
use crate::entity::{EntityManager, EntityType};
use crate::state::Vertex;
use glam::Vec3;

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
        (Vec3::new(-half.x, -half.y, half.z), [0.0, 1.0]),
        (Vec3::new(half.x, -half.y, half.z), [1.0, 1.0]),
        (Vec3::new(half.x, half.y, half.z), [1.0, 0.0]),
        (Vec3::new(-half.x, half.y, half.z), [0.0, 0.0]),
        // Face 1: North (-Z)
        (Vec3::new(half.x, -half.y, -half.z), [0.0, 1.0]),
        (Vec3::new(-half.x, -half.y, -half.z), [1.0, 1.0]),
        (Vec3::new(-half.x, half.y, -half.z), [1.0, 0.0]),
        (Vec3::new(half.x, half.y, -half.z), [0.0, 0.0]),
        // Face 2: West (-X)
        (Vec3::new(-half.x, -half.y, -half.z), [0.0, 1.0]),
        (Vec3::new(-half.x, -half.y, half.z), [1.0, 1.0]),
        (Vec3::new(-half.x, half.y, half.z), [1.0, 0.0]),
        (Vec3::new(-half.x, half.y, -half.z), [0.0, 0.0]),
        // Face 3: East (+X)
        (Vec3::new(half.x, -half.y, half.z), [0.0, 1.0]),
        (Vec3::new(half.x, -half.y, -half.z), [1.0, 1.0]),
        (Vec3::new(half.x, half.y, -half.z), [1.0, 0.0]),
        (Vec3::new(half.x, half.y, half.z), [0.0, 0.0]),
        // Face 4: Up (+Y)
        (Vec3::new(-half.x, half.y, half.z), [0.0, 1.0]),
        (Vec3::new(half.x, half.y, half.z), [1.0, 1.0]),
        (Vec3::new(half.x, half.y, -half.z), [1.0, 0.0]),
        (Vec3::new(-half.x, half.y, -half.z), [0.0, 0.0]),
        // Face 5: Down (-Y)
        (Vec3::new(-half.x, -half.y, -half.z), [0.0, 1.0]),
        (Vec3::new(half.x, -half.y, -half.z), [1.0, 1.0]),
        (Vec3::new(half.x, -half.y, half.z), [1.0, 0.0]),
        (Vec3::new(-half.x, -half.y, half.z), [0.0, 0.0]),
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
            ao: 1.0,
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

        let sin_yaw = entity.yaw.sin();
        let cos_yaw = entity.yaw.cos();
        let to_world = |local: Vec3| {
            entity.position
                + Vec3::new(
                    local.x * cos_yaw + local.z * sin_yaw,
                    local.y,
                    -local.x * sin_yaw + local.z * cos_yaw,
                )
        };

        match entity.entity_type {
            EntityType::Zombie => {
                // Head (Col 0 front face, Col 1 others)
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.5, 0.5, 0.5),
                    Vec3::new(0.0, 0.25, 0.0),
                    to_world(Vec3::new(0.0, 1.4, 0.0)),
                    entity.yaw,
                    entity.pitch,
                    [0, 1, 1, 1, 1, 1], // front is col 0, others col 1
                    9,
                    light_val,
                );

                // Torso (Col 2)
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.5, 0.75, 0.25),
                    Vec3::new(0.0, 0.375, 0.0),
                    to_world(Vec3::new(0.0, 0.65, 0.0)),
                    entity.yaw,
                    0.0,
                    [2; 6],
                    9,
                    light_val,
                );

                // Zombie Arms: raised forward
                let arm_pitch = -std::f32::consts::FRAC_PI_2;
                // Left Arm (Col 3)
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.2, 0.75, 0.2),
                    Vec3::new(0.0, -0.325, 0.0),
                    to_world(Vec3::new(-0.35, 1.3, 0.0)),
                    entity.yaw,
                    arm_pitch,
                    [3; 6],
                    9,
                    light_val,
                );
                // Right Arm (Col 3)
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.2, 0.75, 0.2),
                    Vec3::new(0.0, -0.325, 0.0),
                    to_world(Vec3::new(0.35, 1.3, 0.0)),
                    entity.yaw,
                    arm_pitch,
                    [3; 6],
                    9,
                    light_val,
                );

                // Legs (Col 3)
                // Left Leg
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.2, 0.75, 0.2),
                    Vec3::new(0.0, -0.375, 0.0),
                    to_world(Vec3::new(-0.125, 0.75, 0.0)),
                    entity.yaw,
                    swing,
                    [3; 6],
                    9,
                    light_val,
                );
                // Right Leg
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.2, 0.75, 0.2),
                    Vec3::new(0.0, -0.375, 0.0),
                    to_world(Vec3::new(0.125, 0.75, 0.0)),
                    entity.yaw,
                    -swing,
                    [3; 6],
                    9,
                    light_val,
                );
            }
            EntityType::Skeleton => {
                // Head (Col 4 front face, Col 5 others)
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.5, 0.5, 0.5),
                    Vec3::new(0.0, 0.25, 0.0),
                    to_world(Vec3::new(0.0, 1.4, 0.0)),
                    entity.yaw,
                    entity.pitch,
                    [4, 5, 5, 5, 5, 5],
                    9,
                    light_val,
                );

                // Torso (Col 5)
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.4, 0.75, 0.2),
                    Vec3::new(0.0, 0.375, 0.0),
                    to_world(Vec3::new(0.0, 0.65, 0.0)),
                    entity.yaw,
                    0.0,
                    [5; 6],
                    9,
                    light_val,
                );

                // Aiming calculation
                let target = entity.target_player;
                let aim_pitch = if target { entity.pitch } else { 0.0 };

                let left_arm_pitch = if target {
                    -std::f32::consts::FRAC_PI_2 + aim_pitch
                } else {
                    -swing
                };

                // Draw animation progress: action_cooldown from 2.0 to 0.0
                let draw_progress = if target {
                    ((2.0 - entity.action_cooldown) / 2.0).clamp(0.0, 1.0)
                } else {
                    0.0
                };

                let right_arm_pitch = if target {
                    -std::f32::consts::FRAC_PI_2 + aim_pitch + 0.2 * (1.0 - draw_progress)
                } else {
                    swing
                };

                // Left Arm (holding bow)
                let left_shoulder = Vec3::new(-0.275, 1.3, 0.0);
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.15, 0.75, 0.15),
                    Vec3::new(0.0, -0.325, 0.0),
                    to_world(left_shoulder),
                    entity.yaw,
                    left_arm_pitch,
                    [5; 6],
                    9,
                    light_val,
                );

                // Right Arm (drawing string)
                let right_shoulder = Vec3::new(0.275, 1.3, 0.0);
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.15, 0.75, 0.15),
                    Vec3::new(0.0, -0.325, 0.0),
                    to_world(right_shoulder),
                    entity.yaw,
                    right_arm_pitch,
                    [5; 6],
                    9,
                    light_val,
                );

                // 3D Bow Model attached to Left Hand
                // Calculate left hand position dynamically based on left_arm_pitch
                let cos_lp = left_arm_pitch.cos();
                let sin_lp = left_arm_pitch.sin();
                let hand_rel_shoulder = Vec3::new(0.0, -0.65 * cos_lp, -0.65 * sin_lp);
                let left_hand_local = left_shoulder + hand_rel_shoulder;
                let bow_pivot = to_world(left_hand_local);

                // Bow Grip (Center)
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.08, 0.25, 0.08),
                    Vec3::ZERO,
                    bow_pivot,
                    entity.yaw,
                    aim_pitch,
                    [9; 6], // Bow Wood texture (Col 9 Row 9)
                    9,
                    light_val,
                );

                // Bow Upper Limb
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.06, 0.35, 0.06),
                    Vec3::new(0.0, 0.25, 0.04),
                    bow_pivot,
                    entity.yaw,
                    aim_pitch,
                    [9; 6],
                    9,
                    light_val,
                );

                // Bow Lower Limb
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.06, 0.35, 0.06),
                    Vec3::new(0.0, -0.25, 0.04),
                    bow_pivot,
                    entity.yaw,
                    aim_pitch,
                    [9; 6],
                    9,
                    light_val,
                );

                // Bow String (Center pull-back driven by draw_progress)
                let string_offset_z = -0.04 - 0.25 * draw_progress;
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.02, 0.85, 0.02),
                    Vec3::new(0.0, 0.0, string_offset_z),
                    bow_pivot,
                    entity.yaw,
                    aim_pitch,
                    [10; 6], // Bow String texture (Col 10 Row 9)
                    9,
                    light_val,
                );

                // Arrow on Bow (when targeting player)
                if target {
                    let arrow_offset_z = 0.15 - 0.25 * draw_progress;
                    add_cuboid(
                        vertices,
                        indices,
                        Vec3::new(0.03, 0.03, 0.75),
                        Vec3::new(0.0, 0.0, arrow_offset_z),
                        bow_pivot,
                        entity.yaw,
                        aim_pitch,
                        [8; 6], // Arrow texture (Col 8 Row 9)
                        9,
                        light_val,
                    );
                }

                // Legs (Col 5)
                // Left Leg
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.15, 0.75, 0.15),
                    Vec3::new(0.0, -0.375, 0.0),
                    to_world(Vec3::new(-0.1, 0.75, 0.0)),
                    entity.yaw,
                    swing,
                    [5; 6],
                    9,
                    light_val,
                );
                // Right Leg
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.15, 0.75, 0.15),
                    Vec3::new(0.0, -0.375, 0.0),
                    to_world(Vec3::new(0.1, 0.75, 0.0)),
                    entity.yaw,
                    -swing,
                    [5; 6],
                    9,
                    light_val,
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
                    vertices,
                    indices,
                    Vec3::new(0.5, 0.5, 0.5) * scale,
                    Vec3::new(0.0, 0.25, 0.0) * scale,
                    to_world(Vec3::new(0.0, 1.2, 0.0)),
                    entity.yaw,
                    entity.pitch,
                    [6, 7, 7, 7, 7, 7],
                    9,
                    light_val,
                );

                // Torso (Col 7)
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.5, 0.75, 0.3) * scale,
                    Vec3::new(0.0, 0.375, 0.0) * scale,
                    to_world(Vec3::new(0.0, 0.45, 0.0)),
                    entity.yaw,
                    0.0,
                    [7; 6],
                    9,
                    light_val,
                );

                // Creeper has 4 legs (Col 7)
                // Front Left Leg
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.2, 0.35, 0.2) * scale,
                    Vec3::new(0.0, -0.175, 0.0) * scale,
                    to_world(Vec3::new(-0.15, 0.35, 0.15)),
                    entity.yaw,
                    swing,
                    [7; 6],
                    9,
                    light_val,
                );
                // Front Right Leg
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.2, 0.35, 0.2) * scale,
                    Vec3::new(0.0, -0.175, 0.0) * scale,
                    to_world(Vec3::new(0.15, 0.35, 0.15)),
                    entity.yaw,
                    -swing,
                    [7; 6],
                    9,
                    light_val,
                );
                // Back Left Leg
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.2, 0.35, 0.2) * scale,
                    Vec3::new(0.0, -0.175, 0.0) * scale,
                    to_world(Vec3::new(-0.15, 0.35, -0.15)),
                    entity.yaw,
                    -swing,
                    [7; 6],
                    9,
                    light_val,
                );
                // Back Right Leg
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.2, 0.35, 0.2) * scale,
                    Vec3::new(0.0, -0.175, 0.0) * scale,
                    to_world(Vec3::new(0.15, 0.35, -0.15)),
                    entity.yaw,
                    swing,
                    [7; 6],
                    9,
                    light_val,
                );
            }
            EntityType::Arrow | EntityType::SplashPotion => {
                // Render arrow as a thin box (skin Col 8)
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.06, 0.06, 0.6),
                    Vec3::new(0.0, 0.0, 0.0),
                    entity.position,
                    entity.yaw,
                    entity.pitch,
                    [8; 6],
                    9,
                    light_val,
                );
            }
            EntityType::Pig => {
                let scale = if entity.age < 0.0 { 0.5f32 } else { 1.0f32 };
                let head_scale = if entity.age < 0.0 { 0.75f32 } else { 1.0f32 };

                // Pig Head (Row 10, Col 0) - offset forward
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.5, 0.5, 0.5) * head_scale,
                    Vec3::new(0.0, 0.15, 0.2) * head_scale,
                    to_world(Vec3::new(0.0, 0.8 * scale, 0.2 * scale)),
                    entity.yaw,
                    entity.pitch,
                    [0, 0, 0, 0, 0, 0], // Col 0
                    10,
                    light_val,
                );
                // Torso (Row 10, Col 1) - horizontal: length = 0.8, height = 0.6, width = 0.6
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.6, 0.6, 0.8) * scale,
                    Vec3::new(0.0, 0.0, 0.0),
                    to_world(Vec3::new(0.0, 0.7 * scale, 0.0)),
                    entity.yaw,
                    0.0,
                    [1; 6], // Col 1
                    10,
                    light_val,
                );
                // 4 Legs (Row 10, Col 1)
                // Left Front
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.2, 0.4, 0.2) * scale,
                    Vec3::new(0.0, -0.2, 0.0) * scale,
                    to_world(Vec3::new(-0.25 * scale, 0.4 * scale, 0.25 * scale)),
                    entity.yaw,
                    swing,
                    [1; 6],
                    10,
                    light_val,
                );
                // Right Front
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.2, 0.4, 0.2) * scale,
                    Vec3::new(0.0, -0.2, 0.0) * scale,
                    to_world(Vec3::new(0.25 * scale, 0.4 * scale, 0.25 * scale)),
                    entity.yaw,
                    -swing,
                    [1; 6],
                    10,
                    light_val,
                );
                // Left Back
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.2, 0.4, 0.2) * scale,
                    Vec3::new(0.0, -0.2, 0.0) * scale,
                    to_world(Vec3::new(-0.25 * scale, 0.4 * scale, -0.25 * scale)),
                    entity.yaw,
                    -swing,
                    [1; 6],
                    10,
                    light_val,
                );
                // Right Back
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.2, 0.4, 0.2) * scale,
                    Vec3::new(0.0, -0.2, 0.0) * scale,
                    to_world(Vec3::new(0.25 * scale, 0.4 * scale, -0.25 * scale)),
                    entity.yaw,
                    swing,
                    [1; 6],
                    10,
                    light_val,
                );
            }
            EntityType::Cow => {
                let scale = if entity.age < 0.0 { 0.5 } else { 1.0 };
                let head_scale = if entity.age < 0.0 { 0.75 } else { 1.0 };

                // Cow Head (Row 10, Col 2) - offset forward
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.5, 0.5, 0.5) * head_scale,
                    Vec3::new(0.0, 0.15, 0.2) * head_scale,
                    to_world(Vec3::new(0.0, 1.1 * scale, 0.35 * scale)),
                    entity.yaw,
                    entity.pitch,
                    [2, 2, 2, 2, 2, 2],
                    10,
                    light_val,
                );
                // Torso (Row 10, Col 3) - horizontal: length = 1.0, height = 0.8, width = 0.7
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.7, 0.8, 1.0) * scale,
                    Vec3::new(0.0, 0.0, 0.0),
                    to_world(Vec3::new(0.0, 1.0 * scale, 0.0)),
                    entity.yaw,
                    0.0,
                    [3; 6],
                    10,
                    light_val,
                );
                // 4 Legs (Row 10, Col 3)
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.22, 0.6, 0.22) * scale,
                    Vec3::new(0.0, -0.3, 0.0) * scale,
                    to_world(Vec3::new(-0.25 * scale, 0.6 * scale, 0.35 * scale)),
                    entity.yaw,
                    swing,
                    [3; 6],
                    10,
                    light_val,
                );
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.22, 0.6, 0.22) * scale,
                    Vec3::new(0.0, -0.3, 0.0) * scale,
                    to_world(Vec3::new(0.25 * scale, 0.6 * scale, 0.35 * scale)),
                    entity.yaw,
                    -swing,
                    [3; 6],
                    10,
                    light_val,
                );
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.22, 0.6, 0.22) * scale,
                    Vec3::new(0.0, -0.3, 0.0) * scale,
                    to_world(Vec3::new(-0.25 * scale, 0.6 * scale, -0.35 * scale)),
                    entity.yaw,
                    -swing,
                    [3; 6],
                    10,
                    light_val,
                );
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.22, 0.6, 0.22) * scale,
                    Vec3::new(0.0, -0.3, 0.0) * scale,
                    to_world(Vec3::new(0.25 * scale, 0.6 * scale, -0.35 * scale)),
                    entity.yaw,
                    swing,
                    [3; 6],
                    10,
                    light_val,
                );
            }
            EntityType::Sheep => {
                let scale = if entity.age < 0.0 { 0.5 } else { 1.0 };
                let head_scale = if entity.age < 0.0 { 0.75 } else { 1.0 };

                // Grazing animation head tilt
                let is_grazing = entity.grass_eat_timer > 0.0;
                let final_pitch = if is_grazing {
                    std::f32::consts::FRAC_PI_4 // look down
                } else {
                    entity.pitch
                };

                // Head (Row 10, Col 4) - offset forward
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.45, 0.45, 0.45) * head_scale,
                    Vec3::new(0.0, 0.15, 0.2) * head_scale,
                    to_world(Vec3::new(0.0, 0.9 * scale, 0.3 * scale)),
                    entity.yaw,
                    final_pitch,
                    [4, 4, 4, 4, 4, 4],
                    10,
                    light_val,
                );

                // Body (sheared skin Col 6 or wool layer Col 5) - horizontal: length = 0.9, height = 0.6, width = 0.6
                let body_col = if entity.has_wool { 5 } else { 6 };
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.6, 0.6, 0.9) * scale,
                    Vec3::new(0.0, 0.0, 0.0),
                    to_world(Vec3::new(0.0, 0.8 * scale, 0.0)),
                    entity.yaw,
                    0.0,
                    [body_col; 6],
                    10,
                    light_val,
                );

                // 4 Legs (Col 4)
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.2, 0.5, 0.2) * scale,
                    Vec3::new(0.0, -0.25, 0.0) * scale,
                    to_world(Vec3::new(-0.25 * scale, 0.5 * scale, 0.3 * scale)),
                    entity.yaw,
                    swing,
                    [4; 6],
                    10,
                    light_val,
                );
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.2, 0.5, 0.2) * scale,
                    Vec3::new(0.0, -0.25, 0.0) * scale,
                    to_world(Vec3::new(0.25 * scale, 0.5 * scale, 0.3 * scale)),
                    entity.yaw,
                    -swing,
                    [4; 6],
                    10,
                    light_val,
                );
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.2, 0.5, 0.2) * scale,
                    Vec3::new(0.0, -0.25, 0.0) * scale,
                    to_world(Vec3::new(-0.25 * scale, 0.5 * scale, -0.3 * scale)),
                    entity.yaw,
                    -swing,
                    [4; 6],
                    10,
                    light_val,
                );
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.2, 0.5, 0.2) * scale,
                    Vec3::new(0.0, -0.25, 0.0) * scale,
                    to_world(Vec3::new(0.25 * scale, 0.5 * scale, -0.3 * scale)),
                    entity.yaw,
                    swing,
                    [4; 6],
                    10,
                    light_val,
                );
            }
            EntityType::Chicken => {
                let scale = if entity.age < 0.0 { 0.5 } else { 1.0 };
                let head_scale = if entity.age < 0.0 { 0.75 } else { 1.0 };
                let flap = if entity.velocity.y < 0.0 {
                    (time * 40.0).sin() * 0.7
                } else {
                    0.0
                };

                // Head (Row 10, Col 7) - offset forward
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.25, 0.35, 0.25) * head_scale,
                    Vec3::new(0.0, 0.1, 0.15) * head_scale,
                    to_world(Vec3::new(0.0, 0.45 * scale, 0.1 * scale)),
                    entity.yaw,
                    entity.pitch,
                    [7, 7, 7, 7, 7, 7],
                    10,
                    light_val,
                );
                // Body (Row 10, Col 8) - horizontal: length = 0.4, height = 0.3, width = 0.3
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.3, 0.3, 0.4) * scale,
                    Vec3::new(0.0, 0.0, 0.0),
                    to_world(Vec3::new(0.0, 0.35 * scale, 0.0)),
                    entity.yaw,
                    0.0,
                    [8; 6],
                    10,
                    light_val,
                );
                // Wings: rotate along Z axis for flapping animation
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.05, 0.25, 0.25) * scale,
                    Vec3::new(0.0, -0.1, 0.0) * scale,
                    to_world(Vec3::new(-0.175 * scale, 0.35 * scale, 0.0)),
                    entity.yaw,
                    flap, // Left wing rotation
                    [8; 6],
                    10,
                    light_val,
                );
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.05, 0.25, 0.25) * scale,
                    Vec3::new(0.0, -0.1, 0.0) * scale,
                    to_world(Vec3::new(0.175 * scale, 0.35 * scale, 0.0)),
                    entity.yaw,
                    -flap, // Right wing rotation
                    [8; 6],
                    10,
                    light_val,
                );
                // Legs (thin boxes, Col 8)
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.06, 0.2, 0.06) * scale,
                    Vec3::new(0.0, -0.1, 0.0) * scale,
                    to_world(Vec3::new(-0.06 * scale, 0.2 * scale, 0.0)),
                    entity.yaw,
                    swing,
                    [8; 6],
                    10,
                    light_val,
                );
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.06, 0.2, 0.06) * scale,
                    Vec3::new(0.0, -0.1, 0.0) * scale,
                    to_world(Vec3::new(0.06 * scale, 0.2 * scale, 0.0)),
                    entity.yaw,
                    -swing,
                    [8; 6],
                    10,
                    light_val,
                );
            }
            EntityType::Piglin | EntityType::Husk => {
                let is_piglin = entity.entity_type == EntityType::Piglin;
                let (head_cols, body_col) = if is_piglin {
                    ([12, 13, 13, 13, 13, 13], 13)
                } else {
                    ([14, 15, 15, 15, 15, 15], 15)
                };

                // Both mobs use the familiar humanoid silhouette. Husks hold
                // their arms forward, while piglins walk with alternating arms.
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.5, 0.5, 0.5),
                    Vec3::new(0.0, 0.25, 0.0),
                    to_world(Vec3::new(0.0, 1.45, 0.0)),
                    entity.yaw,
                    entity.pitch,
                    head_cols,
                    15,
                    light_val,
                );
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.5, 0.7, 0.28),
                    Vec3::new(0.0, 0.35, 0.0),
                    to_world(Vec3::new(0.0, 0.75, 0.0)),
                    entity.yaw,
                    0.0,
                    [body_col; 6],
                    15,
                    light_val,
                );

                let (left_arm_pitch, right_arm_pitch) = if is_piglin {
                    (-swing, swing)
                } else {
                    let raised = -std::f32::consts::FRAC_PI_2;
                    (raised, raised)
                };
                for (x, pitch) in [(-0.35, left_arm_pitch), (0.35, right_arm_pitch)] {
                    add_cuboid(
                        vertices,
                        indices,
                        Vec3::new(0.2, 0.75, 0.2),
                        Vec3::new(0.0, -0.325, 0.0),
                        to_world(Vec3::new(x, 1.4, 0.0)),
                        entity.yaw,
                        pitch,
                        [body_col; 6],
                        15,
                        light_val,
                    );
                }
                for (x, pitch) in [(-0.13, swing), (0.13, -swing)] {
                    add_cuboid(
                        vertices,
                        indices,
                        Vec3::new(0.22, 0.75, 0.22),
                        Vec3::new(0.0, -0.375, 0.0),
                        to_world(Vec3::new(x, 0.75, 0.0)),
                        entity.yaw,
                        pitch,
                        [body_col; 6],
                        15,
                        light_val,
                    );
                }

                if is_piglin {
                    // Wide ears and a protruding snout keep the piglin readable
                    // even with the deliberately compact atlas treatment.
                    for x in [-0.34, 0.34] {
                        add_cuboid(
                            vertices,
                            indices,
                            Vec3::new(0.18, 0.24, 0.08),
                            Vec3::ZERO,
                            to_world(Vec3::new(x, 1.72, 0.0)),
                            entity.yaw,
                            0.0,
                            [12; 6],
                            15,
                            light_val,
                        );
                    }
                    add_cuboid(
                        vertices,
                        indices,
                        Vec3::new(0.2, 0.16, 0.12),
                        Vec3::new(0.0, 0.0, 0.29),
                        to_world(Vec3::new(0.0, 1.68, 0.0)),
                        entity.yaw,
                        entity.pitch,
                        [12; 6],
                        15,
                        light_val,
                    );
                }
            }
            EntityType::Blaze => {
                let hover = (time * 2.2).sin() * 0.08;
                let blaze_light = light_val.max(255.0);

                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.5, 0.5, 0.5),
                    Vec3::ZERO,
                    to_world(Vec3::new(0.0, 1.5 + hover, 0.0)),
                    entity.yaw,
                    entity.pitch,
                    [10, 11, 11, 11, 11, 11],
                    15,
                    blaze_light,
                );
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.34, 0.7, 0.34),
                    Vec3::ZERO,
                    to_world(Vec3::new(0.0, 0.92 + hover, 0.0)),
                    entity.yaw,
                    0.0,
                    [11; 6],
                    15,
                    blaze_light,
                );

                // Two counter-rotating rings of rods surround the hot core.
                for ring in 0..2 {
                    for rod in 0..4 {
                        let direction = if ring == 0 { 1.0 } else { -1.0 };
                        let angle = direction * time * 1.8
                            + rod as f32 * std::f32::consts::FRAC_PI_2
                            + ring as f32 * std::f32::consts::FRAC_PI_4;
                        let radius = if ring == 0 { 0.62 } else { 0.48 };
                        let y = if ring == 0 {
                            1.18 + (angle * 2.0).sin() * 0.1
                        } else {
                            0.55 + (angle * 2.0).cos() * 0.1
                        };
                        add_cuboid(
                            vertices,
                            indices,
                            Vec3::new(0.12, 0.62, 0.12),
                            Vec3::ZERO,
                            to_world(Vec3::new(
                                angle.cos() * radius,
                                y + hover,
                                angle.sin() * radius,
                            )),
                            angle,
                            0.0,
                            [10; 6],
                            15,
                            blaze_light,
                        );
                    }
                }
            }
            EntityType::Shulker => {
                let lid_gap = 0.08 + (time * 1.3).sin().abs() * 0.1;

                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.95, 0.18, 0.95),
                    Vec3::ZERO,
                    to_world(Vec3::new(0.0, 0.09, 0.0)),
                    entity.yaw,
                    0.0,
                    [9; 6],
                    10,
                    light_val,
                );
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.88, 0.36, 0.88),
                    Vec3::ZERO,
                    to_world(Vec3::new(0.0, 0.35, 0.0)),
                    entity.yaw,
                    0.0,
                    [10; 6],
                    10,
                    light_val,
                );
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::splat(0.34),
                    Vec3::ZERO,
                    to_world(Vec3::new(0.0, 0.63, 0.0)),
                    entity.yaw,
                    entity.pitch,
                    [9; 6],
                    10,
                    light_val,
                );
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.9, 0.36, 0.9),
                    Vec3::ZERO,
                    to_world(Vec3::new(0.0, 0.82 + lid_gap, 0.0)),
                    entity.yaw,
                    0.0,
                    [10; 6],
                    10,
                    light_val,
                );
            }
            EntityType::EnderDragon => {
                let flap = (time * 3.0).sin() * 0.22;

                // Body, neck, head and jaw.
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(2.0, 1.65, 3.2),
                    Vec3::ZERO,
                    to_world(Vec3::new(0.0, 2.05, -0.35)),
                    entity.yaw,
                    0.0,
                    [12; 6],
                    10,
                    light_val,
                );
                for (center, size) in [
                    (Vec3::new(0.0, 2.3, 1.45), Vec3::new(1.25, 1.1, 1.15)),
                    (Vec3::new(0.0, 2.48, 2.3), Vec3::new(1.05, 0.95, 1.0)),
                ] {
                    add_cuboid(
                        vertices,
                        indices,
                        size,
                        Vec3::ZERO,
                        to_world(center),
                        entity.yaw,
                        -0.12,
                        [11; 6],
                        10,
                        light_val,
                    );
                }
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(1.45, 0.9, 1.35),
                    Vec3::ZERO,
                    to_world(Vec3::new(0.0, 2.62, 3.05)),
                    entity.yaw,
                    entity.pitch,
                    [11, 12, 12, 12, 12, 12],
                    10,
                    light_val,
                );
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.95, 0.25, 0.85),
                    Vec3::ZERO,
                    to_world(Vec3::new(0.0, 2.22, 3.42)),
                    entity.yaw,
                    entity.pitch,
                    [13; 6],
                    10,
                    light_val,
                );
                for x in [-0.48, 0.48] {
                    add_cuboid(
                        vertices,
                        indices,
                        Vec3::new(0.2, 0.38, 0.62),
                        Vec3::ZERO,
                        to_world(Vec3::new(x, 3.12, 2.75)),
                        entity.yaw,
                        0.35,
                        [13; 6],
                        10,
                        light_val,
                    );
                }

                // Four thin wing sections retain the broad silhouette without
                // pushing a dragon beyond 800 generated vertices.
                for side in [-1.0_f32, 1.0] {
                    add_cuboid(
                        vertices,
                        indices,
                        Vec3::new(2.8, 0.14, 1.35),
                        Vec3::ZERO,
                        to_world(Vec3::new(side * 2.05, 2.55 + flap, -0.2)),
                        entity.yaw,
                        -flap * side,
                        [13; 6],
                        10,
                        light_val,
                    );
                    add_cuboid(
                        vertices,
                        indices,
                        Vec3::new(2.2, 0.1, 0.95),
                        Vec3::ZERO,
                        to_world(Vec3::new(side * 4.25, 2.72 + flap * 1.6, -0.55)),
                        entity.yaw,
                        -flap * side,
                        [13; 6],
                        10,
                        light_val,
                    );
                }

                // Tapered, gently swaying tail.
                for segment in 0..5 {
                    let i = segment as f32;
                    let curve = (time * 2.0 + i * 0.7).sin() * 0.13;
                    add_cuboid(
                        vertices,
                        indices,
                        Vec3::new(0.75 - i * 0.1, 0.65 - i * 0.07, 1.25 - i * 0.1),
                        Vec3::ZERO,
                        to_world(Vec3::new(
                            curve * i * 0.55,
                            2.0 - i * 0.12,
                            -2.35 - i * 0.95,
                        )),
                        entity.yaw - curve,
                        0.08 * i,
                        [12; 6],
                        10,
                        light_val,
                    );
                }

                for (x, z) in [(-0.68, 0.65), (0.68, 0.65), (-0.68, -0.8), (0.68, -0.8)] {
                    add_cuboid(
                        vertices,
                        indices,
                        Vec3::new(0.42, 1.0, 0.42),
                        Vec3::ZERO,
                        to_world(Vec3::new(x, 0.85, z)),
                        entity.yaw,
                        0.1,
                        [12; 6],
                        10,
                        light_val,
                    );
                }
            }
            EntityType::Wither => {
                let hover = (time * 1.8).sin() * 0.1;
                let wither_light = light_val.max(192.0);

                // Central spine and the signature three-headed shoulder bar.
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(2.25, 0.36, 0.4),
                    Vec3::ZERO,
                    to_world(Vec3::new(0.0, 2.25 + hover, 0.0)),
                    entity.yaw,
                    0.0,
                    [15; 6],
                    10,
                    wither_light,
                );
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.38, 1.5, 0.4),
                    Vec3::ZERO,
                    to_world(Vec3::new(0.0, 1.4 + hover, 0.0)),
                    entity.yaw,
                    0.0,
                    [15; 6],
                    10,
                    wither_light,
                );
                for (x, y, scale) in [(-0.92, 2.52, 0.82), (0.0, 2.7, 1.0), (0.92, 2.52, 0.82)] {
                    add_cuboid(
                        vertices,
                        indices,
                        Vec3::new(0.72, 0.62, 0.62) * scale,
                        Vec3::ZERO,
                        to_world(Vec3::new(x, y + hover, 0.12)),
                        entity.yaw,
                        entity.pitch,
                        [14, 15, 15, 15, 15, 15],
                        10,
                        wither_light,
                    );
                }
                for (y, width) in [(1.72, 1.45), (1.28, 1.05)] {
                    add_cuboid(
                        vertices,
                        indices,
                        Vec3::new(width, 0.2, 0.28),
                        Vec3::ZERO,
                        to_world(Vec3::new(0.0, y + hover, 0.0)),
                        entity.yaw,
                        0.0,
                        [15; 6],
                        10,
                        wither_light,
                    );
                }
            }
            EntityType::EndCrystal => {
                let crystal_light = light_val.max(255.0);
                let spin = time * 1.7;

                for (size, y) in [(1.25, 0.12), (0.95, 0.25), (0.65, 0.38)] {
                    add_cuboid(
                        vertices,
                        indices,
                        Vec3::new(size, 0.16, size),
                        Vec3::ZERO,
                        to_world(Vec3::new(0.0, y, 0.0)),
                        entity.yaw,
                        0.0,
                        [3; 6],
                        4,
                        light_val,
                    );
                }
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.12, 0.85, 0.12),
                    Vec3::ZERO,
                    to_world(Vec3::new(0.0, 0.85, 0.0)),
                    spin,
                    0.0,
                    [3; 6],
                    4,
                    crystal_light,
                );
                for (size, yaw, pitch) in [(0.72, spin, 0.65), (0.46, -spin * 1.4, -0.45)] {
                    add_cuboid(
                        vertices,
                        indices,
                        Vec3::splat(size),
                        Vec3::ZERO,
                        to_world(Vec3::new(0.0, 1.35, 0.0)),
                        yaw,
                        pitch,
                        [4; 6],
                        4,
                        crystal_light,
                    );
                }
            }
            EntityType::WitherSkull | EntityType::DragonBreath => {
                let is_skull = entity.entity_type == EntityType::WitherSkull;
                let pulse = if is_skull {
                    1.0
                } else {
                    0.85 + (time * 10.0).sin().abs() * 0.25
                };
                let col = if is_skull { 5 } else { 6 };
                let size = if is_skull { 0.3 } else { 0.22 };
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::splat(size * pulse),
                    Vec3::ZERO,
                    entity.position,
                    entity.yaw,
                    entity.pitch,
                    [col; 6],
                    4,
                    light_val.max(255.0),
                );
            }
            EntityType::HeartParticle => {
                // Heart Particle billboard rendering
                // Reuses Row 8, Col 0 Heart icon
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.25, 0.25, 0.01),
                    Vec3::new(0.0, 0.0, 0.0),
                    entity.position,
                    entity.yaw,
                    entity.pitch,
                    [0, 0, 0, 0, 0, 0], // Col 0
                    8,
                    light_val,
                );
            }
            EntityType::DroppedItem => {
                // Floating + rotating dropped item. The entity is rendered as a
                // small cuboid textured from the carried item's atlas tile.
                let yaw = time * 2.0;
                let y_offset = (time * 3.0).sin() * 0.1;

                let (col, row) = entity
                    .dropped_item
                    .map(|item| item.properties().tex_coords)
                    .unwrap_or((0, 0));

                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.25, 0.25, 0.25),
                    Vec3::new(0.0, 0.0, 0.0),
                    to_world(Vec3::new(0.0, 0.25 + y_offset, 0.0)),
                    yaw,
                    0.0,
                    [col; 6],
                    row,
                    light_val,
                );
            }
            EntityType::RemotePlayer => {
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.3, 0.9, 0.3),
                    Vec3::ZERO,
                    to_world(Vec3::new(0.0, 0.9, 0.0)),
                    entity.yaw,
                    0.0,
                    [1; 6],
                    0,
                    light_val,
                );
            }
        }
    }
}
