use crate::chunk_manager::PresentationChunks;
use crate::entity::{Entity, EntityManager, EntityType};
use crate::state::Vertex;
use glam::Vec3;

/// Per-face atlas columns for the player head on row 8:
/// [Front, Back, Left, Right, Top, Bottom]. Column 15 contains Steve's face;
/// column 13 contains the hair/back crop used for every non-front face.
pub const PLAYER_HEAD_COLS: [u32; 6] = [15, 13, 13, 13, 13, 13];
pub const PLAYER_HEAD_ROW: u32 = 8;

/// Atlas column and row for Steve's right-arm front crop.
pub const PLAYER_ARM_COL: u32 = 15;
pub const PLAYER_ARM_ROW: u32 = 9;

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MobPrototypeVertex {
    pub position: [f32; 3],
    pub uv: [f32; 2],
    pub face_idx: u32,
}

impl MobPrototypeVertex {
    pub fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: 12,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x2,
                },
                wgpu::VertexAttribute {
                    offset: 20,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Uint32,
                },
            ],
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MobInstance {
    pub pivot: [f32; 3],
    pub size: [f32; 3],
    pub offset: [f32; 3],
    pub rot_yaw: f32,
    pub rot_pitch: f32,
    pub tex_cols_packed: u32,
    pub tex_row: u32,
    pub light_level: f32,
}

impl MobInstance {
    pub fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 3,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: 12,
                    shader_location: 4,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: 24,
                    shader_location: 5,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: 36,
                    shader_location: 6,
                    format: wgpu::VertexFormat::Float32,
                },
                wgpu::VertexAttribute {
                    offset: 40,
                    shader_location: 7,
                    format: wgpu::VertexFormat::Float32,
                },
                wgpu::VertexAttribute {
                    offset: 44,
                    shader_location: 8,
                    format: wgpu::VertexFormat::Uint32,
                },
                wgpu::VertexAttribute {
                    offset: 48,
                    shader_location: 9,
                    format: wgpu::VertexFormat::Uint32,
                },
                wgpu::VertexAttribute {
                    offset: 52,
                    shader_location: 10,
                    format: wgpu::VertexFormat::Float32,
                },
            ],
        }
    }
}

pub fn pack_tex_cols(cols: [u32; 6]) -> u32 {
    (cols[0] & 0xF)
        | ((cols[1] & 0xF) << 4)
        | ((cols[2] & 0xF) << 8)
        | ((cols[3] & 0xF) << 12)
        | ((cols[4] & 0xF) << 16)
        | ((cols[5] & 0xF) << 20)
}

/// Unit cuboid corners shared by the GPU mob prototype and the CPU hand mesh.
/// Each entry is (local position in `[-0.5, 0.5]^3`, UV, face index 0..=5).
pub const UNIT_CUBOID_CORNERS: [([f32; 3], [f32; 2], u32); 24] = [
    ([-0.5, -0.5, 0.5], [0.0, 1.0], 0),
    ([0.5, -0.5, 0.5], [1.0, 1.0], 0),
    ([0.5, 0.5, 0.5], [1.0, 0.0], 0),
    ([-0.5, 0.5, 0.5], [0.0, 0.0], 0),
    ([0.5, -0.5, -0.5], [0.0, 1.0], 1),
    ([-0.5, -0.5, -0.5], [1.0, 1.0], 1),
    ([-0.5, 0.5, -0.5], [1.0, 0.0], 1),
    ([0.5, 0.5, -0.5], [0.0, 0.0], 1),
    ([-0.5, -0.5, -0.5], [0.0, 1.0], 2),
    ([-0.5, -0.5, 0.5], [1.0, 1.0], 2),
    ([-0.5, 0.5, 0.5], [1.0, 0.0], 2),
    ([-0.5, 0.5, -0.5], [0.0, 0.0], 2),
    ([0.5, -0.5, 0.5], [0.0, 1.0], 3),
    ([0.5, -0.5, -0.5], [1.0, 1.0], 3),
    ([0.5, 0.5, -0.5], [1.0, 0.0], 3),
    ([0.5, 0.5, 0.5], [0.0, 0.0], 3),
    ([-0.5, 0.5, 0.5], [0.0, 1.0], 4),
    ([0.5, 0.5, 0.5], [1.0, 1.0], 4),
    ([0.5, 0.5, -0.5], [1.0, 0.0], 4),
    ([-0.5, 0.5, -0.5], [0.0, 0.0], 4),
    ([-0.5, -0.5, -0.5], [0.0, 1.0], 5),
    ([0.5, -0.5, -0.5], [1.0, 1.0], 5),
    ([0.5, -0.5, 0.5], [1.0, 0.0], 5),
    ([-0.5, -0.5, 0.5], [0.0, 0.0], 5),
];

pub fn build_unit_cuboid_prototype() -> (Vec<MobPrototypeVertex>, Vec<u32>) {
    let mut vertices = Vec::with_capacity(24);
    let mut indices = Vec::with_capacity(36);

    for &(pos, uv, face_idx) in &UNIT_CUBOID_CORNERS {
        vertices.push(MobPrototypeVertex {
            position: pos,
            uv,
            face_idx,
        });
    }

    for f in 0..6 {
        let f_start = (f * 4) as u32;
        indices.push(f_start + 0);
        indices.push(f_start + 1);
        indices.push(f_start + 2);
        indices.push(f_start + 0);
        indices.push(f_start + 2);
        indices.push(f_start + 3);
    }

    (vertices, indices)
}

pub fn build_unit_quad_prototype() -> (Vec<MobPrototypeVertex>, Vec<u32>) {
    let vertices = vec![
        MobPrototypeVertex {
            position: [-0.5, -0.5, 0.0],
            uv: [0.0, 1.0],
            face_idx: 0,
        },
        MobPrototypeVertex {
            position: [0.5, -0.5, 0.0],
            uv: [1.0, 1.0],
            face_idx: 0,
        },
        MobPrototypeVertex {
            position: [0.5, 0.5, 0.0],
            uv: [1.0, 0.0],
            face_idx: 0,
        },
        MobPrototypeVertex {
            position: [-0.5, 0.5, 0.0],
            uv: [0.0, 0.0],
            face_idx: 0,
        },
    ];
    let indices = vec![0, 1, 2, 0, 2, 3, 2, 1, 0, 3, 2, 0];
    (vertices, indices)
}

pub fn add_cuboid(
    instances: &mut Vec<MobInstance>,
    size: Vec3,
    offset: Vec3,
    pivot: Vec3,
    rot_yaw: f32,
    rot_pitch: f32,
    tex_cols: [u32; 6],
    tex_row: u32,
    light_val: f32,
) {
    instances.push(MobInstance {
        pivot: pivot.into(),
        size: size.into(),
        offset: offset.into(),
        rot_yaw,
        rot_pitch,
        tex_cols_packed: pack_tex_cols(tex_cols),
        tex_row,
        light_level: light_val,
    });
}

fn add_flat_sprite(
    instances: &mut Vec<MobInstance>,
    size: f32,
    center: Vec3,
    yaw: f32,
    pitch: f32,
    tex_col: u32,
    tex_row: u32,
    light_val: f32,
) {
    instances.push(MobInstance {
        pivot: center.into(),
        size: [size, size, 1.0],
        offset: [0.0, 0.0, 0.0],
        rot_yaw: yaw,
        rot_pitch: pitch,
        tex_cols_packed: pack_tex_cols([tex_col; 6]),
        tex_row,
        light_level: light_val,
    });
}

pub fn render_mobs<'a>(
    entities: impl IntoIterator<Item = &'a Entity>,
    chunk_manager: &PresentationChunks,
    cuboid_instances: &mut Vec<MobInstance>,
    quad_instances: &mut Vec<MobInstance>,
    time: f32,
) {
    for entity in entities {
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
            let model_pitch = if entity.entity_type == EntityType::EnderDragon {
                entity.pitch
            } else {
                0.0
            };
            let cos_pitch = model_pitch.cos();
            let sin_pitch = model_pitch.sin();
            let pitched = Vec3::new(
                local.x,
                local.y * cos_pitch - local.z * sin_pitch,
                local.y * sin_pitch + local.z * cos_pitch,
            );
            entity.position
                + Vec3::new(
                    pitched.x * cos_yaw + pitched.z * sin_yaw,
                    pitched.y,
                    -pitched.x * sin_yaw + pitched.z * cos_yaw,
                )
        };

        if !crate::mob_parts::emit_table_parts(
            entity,
            cuboid_instances,
            &to_world,
            swing,
            time,
            light_val,
        ) {
            match entity.entity_type {
                EntityType::EnderDragon => {
                    render_ender_dragon(entity, cuboid_instances, &to_world, time, light_val);
                }
                EntityType::Wither => {
                    render_wither(entity, cuboid_instances, &to_world, time, light_val);
                }
                EntityType::DroppedItem => {
                    render_dropped_item(entity, cuboid_instances, quad_instances, &to_world, time, light_val);
                }
                _ => {}
            }
        }

        if (entity.fire_aspect_timer > 0.0 || entity.burn_timer > 0.0)
            && entity.entity_type != EntityType::RemotePlayer
        {
            let fire_size = entity.size + Vec3::splat(0.1);
            let fire_offset = Vec3::new(0.0, entity.size.y * 0.5, 0.0);
            add_cuboid(
                cuboid_instances,
                fire_size,
                fire_offset,
                entity.position,
                entity.yaw,
                0.0,
                [15; 6],
                12,
                255.0,
            );
        }
    }
}

fn render_ender_dragon(
    entity: &Entity,
    cuboid_instances: &mut Vec<MobInstance>,
    to_world: &dyn Fn(Vec3) -> Vec3,
    time: f32,
    light_val: f32,
) {
                let flap = (time * 3.0).sin() * 0.22;
                let dragon_pitch = entity.pitch;

                // Body, neck, head and jaw.
                add_cuboid(
                    cuboid_instances,
                    Vec3::new(2.0, 1.65, 3.2),
                    Vec3::ZERO,
                    to_world(Vec3::new(0.0, 2.05, -0.35)),
                    entity.yaw,
                    dragon_pitch,
                    [10; 6],
                    4,
                    light_val,
                );
                for (center, size) in [
                    (Vec3::new(0.0, 2.3, 1.45), Vec3::new(1.25, 1.1, 1.15)),
                    (Vec3::new(0.0, 2.48, 2.3), Vec3::new(1.05, 0.95, 1.0)),
                ] {
                    add_cuboid(
                        cuboid_instances,
                        size,
                        Vec3::ZERO,
                        to_world(center),
                        entity.yaw,
                        dragon_pitch - 0.12,
                        [11; 6],
                        4,
                        light_val,
                    );
                }
                add_cuboid(
                    cuboid_instances,
                    Vec3::new(1.45, 0.9, 1.35),
                    Vec3::ZERO,
                    to_world(Vec3::new(0.0, 2.62, 3.05)),
                    entity.yaw,
                    dragon_pitch,
                    [12; 6],
                    4,
                    light_val,
                );
                add_cuboid(
                    cuboid_instances,
                    Vec3::new(0.95, 0.25, 0.85),
                    Vec3::ZERO,
                    to_world(Vec3::new(0.0, 2.22, 3.42)),
                    entity.yaw,
                    dragon_pitch,
                    [12; 6],
                    4,
                    light_val,
                );
                for x in [-0.48, 0.48] {
                    add_cuboid(
                        cuboid_instances,
                        Vec3::new(0.2, 0.38, 0.62),
                        Vec3::ZERO,
                        to_world(Vec3::new(x, 3.12, 2.75)),
                        entity.yaw,
                        dragon_pitch + 0.35,
                        [12; 6],
                        4,
                        light_val,
                    );
                }

                // Four thin wing sections retain the broad silhouette without
                // pushing a dragon beyond 800 generated vertices.
                for side in [-1.0_f32, 1.0] {
                    add_cuboid(
                        cuboid_instances,
                        Vec3::new(2.8, 0.14, 1.35),
                        Vec3::ZERO,
                        to_world(Vec3::new(side * 2.05, 2.55 + flap, -0.2)),
                        entity.yaw,
                        dragon_pitch - flap * side,
                        [13; 6],
                        4,
                        light_val,
                    );
                    add_cuboid(
                        cuboid_instances,
                        Vec3::new(2.2, 0.1, 0.95),
                        Vec3::ZERO,
                        to_world(Vec3::new(side * 4.25, 2.72 + flap * 1.6, -0.55)),
                        entity.yaw,
                        dragon_pitch - flap * side,
                        [13; 6],
                        4,
                        light_val,
                    );
                }

                // Tapered, gently swaying tail.
                for segment in 0..5 {
                    let i = segment as f32;
                    let curve = (time * 2.0 + i * 0.7).sin() * 0.13;
                    add_cuboid(
                        cuboid_instances,
                        Vec3::new(0.75 - i * 0.1, 0.65 - i * 0.07, 1.25 - i * 0.1),
                        Vec3::ZERO,
                        to_world(Vec3::new(
                            curve * i * 0.55,
                            2.0 - i * 0.12,
                            -2.35 - i * 0.95,
                        )),
                        entity.yaw - curve,
                        dragon_pitch + 0.08 * i,
                        [11; 6],
                        4,
                        light_val,
                    );
                }

                for (x, z) in [(-0.68, 0.65), (0.68, 0.65), (-0.68, -0.8), (0.68, -0.8)] {
                    add_cuboid(
                        cuboid_instances,
                        Vec3::new(0.42, 1.0, 0.42),
                        Vec3::ZERO,
                        to_world(Vec3::new(x, 0.85, z)),
                        entity.yaw,
                        dragon_pitch + 0.1,
                        [10; 6],
                        4,
                        light_val,
                    );
                }
            }

fn render_wither(
    entity: &Entity,
    cuboid_instances: &mut Vec<MobInstance>,
    to_world: &dyn Fn(Vec3) -> Vec3,
    time: f32,
    light_val: f32,
) {
                let hover = (time * 1.8).sin() * 0.1;
                let wither_light = light_val.max(192.0);

                // Dedicated resource-pack wither skin at (8,8) face / (9,8) body.
                // Central spine and the signature three-headed shoulder bar.
                add_cuboid(
                    cuboid_instances,
                    Vec3::new(2.25, 0.36, 0.4),
                    Vec3::ZERO,
                    to_world(Vec3::new(0.0, 2.25 + hover, 0.0)),
                    entity.yaw,
                    0.0,
                    [9; 6],
                    8,
                    wither_light,
                );
                add_cuboid(
                    cuboid_instances,
                    Vec3::new(0.38, 1.5, 0.4),
                    Vec3::ZERO,
                    to_world(Vec3::new(0.0, 1.4 + hover, 0.0)),
                    entity.yaw,
                    0.0,
                    [9; 6],
                    8,
                    wither_light,
                );
                for (x, y, scale) in [(-0.92, 2.52, 0.82), (0.0, 2.7, 1.0), (0.92, 2.52, 0.82)] {
                    add_cuboid(
                        cuboid_instances,
                        Vec3::new(0.72, 0.62, 0.62) * scale,
                        Vec3::ZERO,
                        to_world(Vec3::new(x, y + hover, 0.12)),
                        entity.yaw,
                        entity.pitch,
                        [8, 9, 9, 9, 9, 9],
                        8,
                        wither_light,
                    );
                }
                for (y, width) in [(1.72, 1.45), (1.28, 1.05)] {
                    add_cuboid(
                        cuboid_instances,
                        Vec3::new(width, 0.2, 0.28),
                        Vec3::ZERO,
                        to_world(Vec3::new(0.0, y + hover, 0.0)),
                        entity.yaw,
                        0.0,
                        [9; 6],
                        8,
                        wither_light,
                    );
                }
            }

fn render_dropped_item(
    entity: &Entity,
    cuboid_instances: &mut Vec<MobInstance>,
    quad_instances: &mut Vec<MobInstance>,
    to_world: &dyn Fn(Vec3) -> Vec3,
    time: f32,
    light_val: f32,
) {
                // Floating + rotating dropped item. Full-cube blocks render as
                // a small cuboid textured from the item's atlas tile; flat
                // items (flowers, seeds, tools, food, ...) render as a
                // double-sided sprite quad, like their inventory icon.
                let yaw = time * 2.0;
                let y_offset = (time * 3.0).sin() * 0.1;

                let item = entity.dropped_item.unwrap_or(crate::inventory::Item::Air);

                if item.renders_flat() {
                    let (col, row) = item.properties().tex_coords;
                    add_flat_sprite(
                        quad_instances,
                        0.35,
                        entity.position + Vec3::new(0.0, 0.3 + y_offset, 0.0),
                        yaw,
                        0.0,
                        col,
                        row,
                        light_val,
                    );
                } else {
                    let (col, row) = entity
                        .dropped_item
                        .map(|item| item.properties().tex_coords)
                        .unwrap_or((0, 0));

                    add_cuboid(
                        cuboid_instances,
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
            }

/// Renders the local player as a Steve-like avatar in world space. Used when
/// the camera is in third-person mode.
pub fn render_local_player(
    position: Vec3,
    yaw: f32,
    pitch: f32,
    chunk_manager: &PresentationChunks,
    cuboid_instances: &mut Vec<MobInstance>,
    quad_instances: &mut Vec<MobInstance>,
    held_item: crate::inventory::Item,
    time: f32,
    velocity: Vec3,
) {
    let mx = position.x.floor() as i32;
    let my = position.y.floor() as i32;
    let mz = position.z.floor() as i32;
    let sky_l = chunk_manager.get_sky_light(mx, my, mz);
    let block_l = chunk_manager.get_block_light(mx, my, mz);
    let light_val = (sky_l as f32) + (block_l as f32) * 16.0;

    let speed_2d = Vec3::new(velocity.x, 0.0, velocity.z).length();
    let walking = speed_2d > 0.1;
    let swing = if walking {
        (time * 8.0).sin() * 0.6
    } else {
        0.0
    };

    let sin_yaw = yaw.sin();
    let cos_yaw = yaw.cos();
    let to_world = |local: Vec3| {
        position
            + Vec3::new(
                local.x * cos_yaw + local.z * sin_yaw,
                local.y,
                -local.x * sin_yaw + local.z * cos_yaw,
            )
    };

    // Head: dedicated per-face player skin tiles (front face, hair on the
    // back and top, skin with ears on the sides).
    add_cuboid(
        cuboid_instances,
        Vec3::new(0.5, 0.5, 0.5),
        Vec3::new(0.0, 0.25, 0.0),
        to_world(Vec3::new(0.0, 1.4, 0.0)),
        yaw,
        pitch,
        PLAYER_HEAD_COLS,
        PLAYER_HEAD_ROW,
        light_val,
    );

    // Torso (zombie teal shirt)
    add_cuboid(
        cuboid_instances,
        Vec3::new(0.5, 0.75, 0.25),
        Vec3::new(0.0, 0.375, 0.0),
        to_world(Vec3::new(0.0, 0.65, 0.0)),
        yaw,
        0.0,
        [2; 6],
        9,
        light_val,
    );

    // Arms counter-swing against the legs while walking. The arm tile shows
    // a teal sleeve at the shoulder and bare skin below.
    add_cuboid(
        cuboid_instances,
        Vec3::new(0.25, 0.75, 0.25),
        Vec3::new(0.0, -0.325, 0.0),
        to_world(Vec3::new(-0.375, 1.3, 0.0)),
        yaw,
        -swing,
        [PLAYER_ARM_COL; 6],
        PLAYER_ARM_ROW,
        light_val,
    );
    add_cuboid(
        cuboid_instances,
        Vec3::new(0.25, 0.75, 0.25),
        Vec3::new(0.0, -0.325, 0.0),
        to_world(Vec3::new(0.375, 1.3, 0.0)),
        yaw,
        swing,
        [PLAYER_ARM_COL; 6],
        PLAYER_ARM_ROW,
        light_val,
    );

    // Minecraft's skin model has a separate sleeve layer over the upper arm.
    // Keep it slightly larger than the arm so the clothing remains visible
    // from the front, back, and side while the arm swings.
    for (x, arm_pitch) in [(-0.375, -swing), (0.375, swing)] {
        add_cuboid(
            cuboid_instances,
            Vec3::new(0.27, 0.28, 0.27),
            Vec3::new(0.0, -0.09, 0.0),
            to_world(Vec3::new(x, 1.3, 0.0)),
            yaw,
            arm_pitch,
            [2; 6],
            9,
            light_val,
        );
    }

    // Attach the selected hotbar item to the right hand. Its center follows
    // the arm pitch so it stays in the player's grip while walking.
    if held_item != crate::inventory::Item::Air {
        let arm_pitch = -swing;
        let grip_offset = Vec3::new(0.0, -0.68 * arm_pitch.cos(), -0.68 * arm_pitch.sin());
        let held_center = to_world(Vec3::new(-0.375, 1.3, 0.0) + grip_offset);
        let (col, row) = held_item.properties().tex_coords;
        if held_item.renders_flat() {
            add_flat_sprite(
                quad_instances,
                0.42,
                held_center,
                yaw,
                arm_pitch,
                col,
                row,
                light_val,
            );
        } else {
            add_cuboid(
                cuboid_instances,
                Vec3::splat(0.28),
                Vec3::ZERO,
                held_center,
                yaw,
                arm_pitch,
                [col; 6],
                row,
                light_val,
            );
        }
    }

    // Legs (zombie dark blue pants)
    add_cuboid(
        cuboid_instances,
        Vec3::new(0.25, 0.75, 0.25),
        Vec3::new(0.0, -0.375, 0.0),
        to_world(Vec3::new(-0.125, 0.75, 0.0)),
        yaw,
        swing,
        [3; 6],
        9,
        light_val,
    );
    add_cuboid(
        cuboid_instances,
        Vec3::new(0.25, 0.75, 0.25),
        Vec3::new(0.0, -0.375, 0.0),
        to_world(Vec3::new(0.125, 0.75, 0.0)),
        yaw,
        -swing,
        [3; 6],
        9,
        light_val,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_player_renders_body_and_two_sleeve_layers() {
        let mut entities = EntityManager::new();
        entities.spawn(EntityType::RemotePlayer, Vec3::new(4.0, 8.0, -2.0));
        let chunks = PresentationChunks::new(1);
        let mut cuboids = Vec::new();
        let mut quads = Vec::new();

        render_mobs(
            entities.entities.iter(),
            &chunks,
            &mut cuboids,
            &mut quads,
            0.0,
        );

        assert_eq!(cuboids.len(), 8);
        assert!(quads.is_empty());
        assert!(cuboids.iter().all(|instance| {
            instance.pivot.iter().all(|&p| p.is_finite())
                && instance.size.iter().all(|&s| s.is_finite())
                && instance.offset.iter().all(|&o| o.is_finite())
        }));
    }

    #[test]
    fn local_player_renders_body_and_two_sleeve_layers() {
        let chunks = PresentationChunks::new(1);
        let mut instances = Vec::new();
        let mut quads = Vec::new();

        render_local_player(
            Vec3::new(0.0, 64.0, 0.0),
            0.0,
            0.0,
            &chunks,
            &mut instances,
            &mut quads,
            crate::inventory::Item::Air,
            0.0,
            Vec3::ZERO,
        );

        assert_eq!(instances.len(), 8);
        assert!(quads.is_empty());
        assert!(instances.iter().all(|instance| {
            instance.pivot.iter().all(|&p| p.is_finite())
                && instance.size.iter().all(|&s| s.is_finite())
                && instance.offset.iter().all(|&o| o.is_finite())
        }));
    }

    #[test]
    fn local_player_uses_player_skin_slots_instead_of_husk_head_slots() {
        let chunks = PresentationChunks::new(1);
        let mut instances = Vec::new();
        let mut quads = Vec::new();
        render_local_player(
            Vec3::new(0.0, 64.0, 0.0),
            0.0,
            0.0,
            &chunks,
            &mut instances,
            &mut quads,
            crate::inventory::Item::Air,
            0.0,
            Vec3::ZERO,
        );

        assert_eq!(instances[0].tex_row, PLAYER_HEAD_ROW);
        assert_eq!(
            instances[0].tex_cols_packed,
            pack_tex_cols(PLAYER_HEAD_COLS)
        );
        for arm in &instances[2..=3] {
            assert_eq!(arm.tex_row, PLAYER_ARM_ROW);
            assert_eq!(arm.tex_cols_packed, pack_tex_cols([PLAYER_ARM_COL; 6]));
        }
        for sleeve in &instances[4..=5] {
            assert_eq!(sleeve.tex_row, 9);
            assert_eq!(sleeve.tex_cols_packed, pack_tex_cols([2; 6]));
            assert!(sleeve.size[0] > instances[2].size[0]);
        }
    }

    #[test]
    fn local_player_renders_selected_block_and_flat_item_in_right_hand() {
        let chunks = PresentationChunks::new(1);

        let mut block_instances = Vec::new();
        let mut block_quads = Vec::new();
        render_local_player(
            Vec3::new(0.0, 64.0, 0.0),
            0.0,
            0.0,
            &chunks,
            &mut block_instances,
            &mut block_quads,
            crate::inventory::Item::Stone,
            0.0,
            Vec3::ZERO,
        );
        assert_eq!(block_instances.len(), 9);
        assert!(block_quads.is_empty());

        let mut item_instances = Vec::new();
        let mut item_quads = Vec::new();
        render_local_player(
            Vec3::new(0.0, 64.0, 0.0),
            0.0,
            0.0,
            &chunks,
            &mut item_instances,
            &mut item_quads,
            crate::inventory::Item::EyeOfEnder,
            0.0,
            Vec3::ZERO,
        );
        assert_eq!(item_instances.len(), 8);
        assert_eq!(item_quads.len(), 1);
    }

    #[test]
    fn local_player_faces_camera_direction_with_shifted_yaw() {
        // Third-person caller passes model_yaw = FRAC_PI_2 - camera_yaw. The
        // head's front face instance yaw must then point along the camera's
        // horizontal forward so the camera sees the back.
        let camera_yaw = 0.3_f32;
        let model_yaw = std::f32::consts::FRAC_PI_2 - camera_yaw;
        let position = Vec3::new(10.0, 64.0, -3.0);
        let chunks = PresentationChunks::new(1);
        let mut instances = Vec::new();
        let mut quads = Vec::new();

        render_local_player(
            position,
            model_yaw,
            0.0,
            &chunks,
            &mut instances,
            &mut quads,
            crate::inventory::Item::Air,
            0.0,
            Vec3::ZERO,
        );

        let head = &instances[0];
        let facing = Vec3::new(head.rot_yaw.sin(), 0.0, head.rot_yaw.cos());
        let expected = Vec3::new(camera_yaw.cos(), 0.0, camera_yaw.sin());
        assert!(
            facing.dot(expected) > 0.99,
            "avatar should face the camera forward direction: facing={facing:?} expected={expected:?}"
        );
    }

    #[test]
    fn dropped_flat_item_renders_as_sprite_quad() {
        let mut entities = EntityManager::new();
        entities.spawn(EntityType::DroppedItem, Vec3::new(0.0, 64.0, 0.0));
        entities.entities.last_mut().unwrap().dropped_item = Some(crate::inventory::Item::Seeds);

        let chunks = PresentationChunks::new(1);
        let mut cuboids = Vec::new();
        let mut quads = Vec::new();
        render_mobs(
            entities.entities.iter(),
            &chunks,
            &mut cuboids,
            &mut quads,
            1.0,
        );

        // Flat items render as one quad instance, not cuboids.
        assert_eq!(cuboids.len(), 0);
        assert_eq!(quads.len(), 1);
        assert!(quads.iter().all(|instance| {
            instance.pivot.iter().all(|&p| p.is_finite())
                && instance.size.iter().all(|&s| s.is_finite())
        }));
    }

    #[test]
    fn dropped_block_item_still_renders_as_cube() {
        let mut entities = EntityManager::new();
        entities.spawn(EntityType::DroppedItem, Vec3::new(0.0, 64.0, 0.0));
        entities.entities.last_mut().unwrap().dropped_item = Some(crate::inventory::Item::Stone);

        let chunks = PresentationChunks::new(1);
        let mut cuboids = Vec::new();
        let mut quads = Vec::new();
        render_mobs(
            entities.entities.iter(),
            &chunks,
            &mut cuboids,
            &mut quads,
            1.0,
        );

        assert_eq!(cuboids.len(), 1);
        assert_eq!(quads.len(), 0);
    }

    #[test]
    fn passive_mob_heads_have_single_face_texture_and_body_sides() {
        let test_cases = [
            (EntityType::Pig, 0u32, 1u32),
            (EntityType::Cow, 2u32, 3u32),
            (EntityType::Sheep, 3u32, 4u32),
            (EntityType::Chicken, 7u32, 8u32),
        ];

        let chunks = PresentationChunks::new(1);

        for (mob_type, face_col, body_col) in test_cases {
            let mut entities = EntityManager::new();
            entities.spawn(mob_type, Vec3::new(0.0, 64.0, 0.0));

            let mut cuboids = Vec::new();
            let mut quads = Vec::new();
            render_mobs(
                entities.entities.iter(),
                &chunks,
                &mut cuboids,
                &mut quads,
                0.0,
            );

            // Head is the first cuboid.
            // Face 0 (Front face) packed at bits 0..4.
            let front_col = (cuboids[0].tex_cols_packed >> 0) & 0xF;
            assert_eq!(
                front_col, face_col,
                "{mob_type:?} head front face should use face_col {face_col}"
            );

            // Faces 1..5 (Back, Left, Right, Top, Bottom) packed at bits (face * 4)..
            for face in 1..6 {
                let col = (cuboids[0].tex_cols_packed >> (face * 4)) & 0xF;
                assert_eq!(
                    col, body_col,
                    "{mob_type:?} head face {face} should use body_col {body_col}"
                );
            }
        }
    }

    #[test]
    fn cuboid_outside_faces_are_clockwise_on_screen_with_left_handed_camera() {
        // The camera builds its matrix with look_at_lh + perspective_lh.
        // Under that projection (plus the viewport's y flip), the cuboid's
        // CCW-outward world faces appear clockwise in window coordinates.
        // The mob pipeline must therefore use FrontFace::Cw; Ccw culls every
        // outside face and leaves the model see-through.
        let camera = crate::camera::Camera::new(Vec3::new(0.0, 64.0, 5.0), 0.0, 0.0, 70.0);
        let view_proj = camera.build_view_projection_matrix(16.0 / 9.0, 200.0);
        let (vertices, _) = build_unit_cuboid_prototype();

        // Face 0 is the front (+Z) face of the unit cuboid. Place the cuboid
        // in front of the camera (which looks toward +Z) at (0, 64.5, 0).
        let center = Vec3::new(0.0, 64.5, 0.0);
        let to_window = |p: Vec3| {
            let clip = view_proj * (p + center).extend(1.0);
            let ndc = clip.truncate() / clip.w;
            (ndc.x, 1.0 - ndc.y) // viewport y flip, scale omitted (sign-only)
        };
        let (x0, y0) = to_window(Vec3::from(vertices[0].position));
        let (x1, y1) = to_window(Vec3::from(vertices[1].position));
        let (x2, y2) = to_window(Vec3::from(vertices[2].position));
        let signed_area = (x1 - x0) * (y2 - y0) - (y1 - y0) * (x2 - x0);

        assert!(
            signed_area < 0.0,
            "front face must be clockwise on screen (area {signed_area}); \
             keep FrontFace::Cw on the mob pipeline or the outside faces are culled"
        );
    }

    #[test]
    fn test_burning_entity_fire_flag() {
        let mut entity_manager = EntityManager::new();
        let zombie_id = entity_manager.spawn(EntityType::Zombie, Vec3::new(0.0, 64.0, 0.0));

        let chunk_manager = PresentationChunks::new(1);
        let mut cuboids_normal = Vec::new();
        let mut quads_normal = Vec::new();
        render_mobs(
            entity_manager.entities.iter(),
            &chunk_manager,
            &mut cuboids_normal,
            &mut quads_normal,
            0.0,
        );

        // Turn on fire
        if let Some(zombie) = entity_manager.get_by_id_mut(zombie_id) {
            zombie.fire_aspect_timer = 5.0;
        }

        let mut cuboids_burning = Vec::new();
        let mut quads_burning = Vec::new();
        render_mobs(
            entity_manager.entities.iter(),
            &chunk_manager,
            &mut cuboids_burning,
            &mut quads_burning,
            0.0,
        );

        // Burning entity should generate 1 extra cuboid (fire overlay)
        assert_eq!(cuboids_burning.len(), cuboids_normal.len() + 1);

        // Verify the extra fire cuboid uses row 12
        let last_instance = cuboids_burning.last().unwrap();
        assert_eq!(last_instance.tex_row, 12);
    }

    #[test]
    fn ender_dragon_uses_only_dedicated_opaque_atlas_tiles() {
        let mut entities = EntityManager::new();
        entities.spawn(EntityType::EnderDragon, Vec3::ZERO);
        let chunks = PresentationChunks::new(1);
        let mut cuboids = Vec::new();
        let mut quads = Vec::new();
        render_mobs(
            entities.entities.iter(),
            &chunks,
            &mut cuboids,
            &mut quads,
            0.0,
        );

        assert!(!cuboids.is_empty());
        assert!(quads.is_empty());
        assert!(cuboids.iter().all(|instance| instance.tex_row == 4));
        assert!(cuboids.iter().all(|instance| {
            (0..6).all(|face| {
                let col = (instance.tex_cols_packed >> (face * 4)) & 0xF;
                (10..=13).contains(&col)
            })
        }));
    }

    #[test]
    fn enderman_uses_tall_model_and_dedicated_skin_tiles() {
        let mut entities = EntityManager::new();
        entities.spawn(EntityType::Enderman, Vec3::ZERO);
        let chunks = PresentationChunks::new(1);
        let mut cuboids = Vec::new();
        let mut quads = Vec::new();

        render_mobs(
            entities.entities.iter(),
            &chunks,
            &mut cuboids,
            &mut quads,
            0.0,
        );

        assert_eq!(cuboids.len(), 6);
        assert!(quads.is_empty());
        assert!(cuboids.iter().all(|part| part.tex_row == 8));
        assert!(cuboids.iter().all(|part| {
            (0..6).all(|face| {
                let col = (part.tex_cols_packed >> (face * 4)) & 0xF;
                matches!(col, 10 | 11 | 12 | 14)
            })
        }));

        let max_y = cuboids
            .iter()
            .map(|inst| inst.pivot[1] + inst.offset[1] + inst.size[1])
            .fold(f32::NEG_INFINITY, f32::max);
        assert!(
            max_y > 2.8,
            "Enderman model should be nearly three blocks tall"
        );
    }

    #[test]
    fn test_creeper_mesh_geometry_no_gap() {
        let mut entity_manager = EntityManager::new();
        entity_manager.spawn(EntityType::Creeper, Vec3::ZERO);

        let chunk_manager = PresentationChunks::new(1);
        let mut cuboids = Vec::new();
        let mut quads = Vec::new();
        render_mobs(
            entity_manager.entities.iter(),
            &chunk_manager,
            &mut cuboids,
            &mut quads,
            0.0,
        );

        // Creeper has 6 cuboids (1 head + 1 torso + 4 legs)
        assert_eq!(cuboids.len(), 6);

        // Find min and max Y across all cuboids
        let min_y = cuboids
            .iter()
            .map(|c| c.pivot[1] + c.offset[1] - c.size[1] * 0.5)
            .fold(f32::INFINITY, f32::min);
        let max_y = cuboids
            .iter()
            .map(|c| c.pivot[1] + c.offset[1] + c.size[1] * 0.5)
            .fold(f32::NEG_INFINITY, f32::max);

        // Legs start at Y=0.0 and head ends at Y=1.625
        assert!(
            (min_y - 0.0).abs() < 1e-4,
            "Min Y should be 0.0, got {}",
            min_y
        );
        assert!(
            (max_y - 1.625).abs() < 1e-4,
            "Max Y should be 1.625, got {}",
            max_y
        );

        // Check vertical continuity: no gap between leg top (0.375) and torso bottom (0.375)
        let has_y_0_375 = cuboids
            .iter()
            .any(|c| ((c.pivot[1] + c.offset[1] + c.size[1] * 0.5) - 0.375).abs() < 1e-4);
        assert!(
            has_y_0_375,
            "Cuboid must exist reaching Y=0.375 connecting legs and torso"
        );
    }
}
