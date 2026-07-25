use crate::inventory::{Inventory, Item};
use crate::state::Vertex;
use glam::Vec3;

/// Returns the texture columns and row for the held item in the hand.
/// Block items use the block's top-face texture; non-block items use their
/// own inventory icon tile.
fn held_item_texture(item: Item) -> Option<([u32; 6], u32)> {
    let props = item.properties();
    if let Some(block) = props.block_type {
        let (col, row) = block.get_face_tex_index(4); // top face
        Some(([col; 6], row))
    } else if item == Item::Air {
        None
    } else {
        let (col, row) = props.tex_coords;
        Some(([col; 6], row))
    }
}

/// Builds the first-person right-hand mesh in view space.
///
/// The hand is positioned on the right side of the screen, angled slightly
/// inward and upward like Minecraft. The view space convention is the same
/// as the main renderer: +X right, +Y up, +Z forward (left-handed).
pub fn build_first_person_hand_mesh(
    inventory: &Inventory,
    walk_swing: f32,
    attack_swing: f32,
) -> (Vec<Vertex>, Vec<u32>) {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();

    // The arm is an elongated rectangular box angled from off-screen
    // bottom-right toward the fist, like Minecraft's first-person arm
    // connecting back to the body.
    let hand_yaw = -0.35_f32;
    let hand_pitch = -0.25_f32 + walk_swing * 0.08 - attack_swing * 0.5;

    let attack_offset = Vec3::new(-0.1 * attack_swing, 0.1 * attack_swing, 0.25 * attack_swing);
    let fist_pos = Vec3::new(0.42, -0.42 + walk_swing * 0.03, 0.95) + attack_offset;

    // Right arm: plain player skin tile (col 9, row 10) with no face/eye features.
    add_cuboid_view(
        &mut vertices,
        &mut indices,
        Vec3::new(0.24, 0.24, 1.2),
        Vec3::new(0.0, 0.0, -0.55),
        fist_pos,
        hand_yaw,
        hand_pitch,
        [9; 6],
        10,
        1.0,
    );

    // Held item: full-cube blocks render as a small cube textured with the
    // block's top face; flat items (flowers, seeds, tools, food, ...) render
    // as a sprite quad. Both sit slightly in front of the fist.
    let held_item = inventory.hotbar[inventory.selected]
        .map(|stack| stack.item)
        .unwrap_or(Item::Air);
    if held_item != Item::Air {
        let item_offset = Vec3::new(-0.08, 0.06, 0.25);
        let item_pos = fist_pos + item_offset;
        let item_yaw = hand_yaw;
        let item_pitch = hand_pitch;
        if held_item.renders_flat() {
            let (tex_col, tex_row) = held_item.properties().tex_coords;
            add_sprite_view(
                &mut vertices,
                &mut indices,
                0.3,
                item_pos,
                item_yaw,
                item_pitch,
                tex_col,
                tex_row,
                1.0,
            );
        } else if let Some((tex_cols, tex_row)) = held_item_texture(held_item) {
            add_cuboid_view(
                &mut vertices,
                &mut indices,
                Vec3::new(0.18, 0.18, 0.18),
                Vec3::new(0.0, 0.0, 0.0),
                item_pos,
                item_yaw,
                item_pitch,
                tex_cols,
                tex_row,
                1.0,
            );
        }
    }

    (vertices, indices)
}

/// View-space flat sprite helper for held items that render flat (flowers,
/// seeds, tools, ...). Emits a single double-sided quad in the local XY
/// plane, rotated like `add_cuboid_view`.
#[allow(clippy::too_many_arguments)]
fn add_sprite_view(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
    size: f32,
    pivot: Vec3,
    rot_yaw: f32,
    rot_pitch: f32,
    tex_col: u32,
    tex_row: u32,
    light_val: f32,
) {
    let half = size * 0.5;
    let local_corners = [
        (Vec3::new(-half, -half, 0.0), [0.0, 1.0]),
        (Vec3::new(half, -half, 0.0), [1.0, 1.0]),
        (Vec3::new(half, half, 0.0), [1.0, 0.0]),
        (Vec3::new(-half, half, 0.0), [0.0, 0.0]),
    ];

    let cos_pitch = rot_pitch.cos();
    let sin_pitch = rot_pitch.sin();
    let cos_yaw = rot_yaw.cos();
    let sin_yaw = rot_yaw.sin();

    let start_idx = vertices.len() as u32;

    for (local_pos, uv) in local_corners.iter() {
        let v2 = Vec3::new(
            local_pos.x,
            local_pos.y * cos_pitch - local_pos.z * sin_pitch,
            local_pos.y * sin_pitch + local_pos.z * cos_pitch,
        );
        let v3 = Vec3::new(
            v2.x * cos_yaw + v2.z * sin_yaw,
            v2.y,
            -v2.x * sin_yaw + v2.z * cos_yaw,
        );
        let final_pos = v3 + pivot;

        let u = (uv[0] + tex_col as f32) * 0.0625;
        let v = (uv[1] + tex_row as f32) * 0.0625;

        vertices.push(Vertex {
            position: [final_pos.x, final_pos.y, final_pos.z],
            tex_coords: [u, v],
            light_level: light_val,
            ao: 1.0,
        });
    }

    // Double-sided quad: the pipeline culls back faces, so emit both windings.
    indices.push(start_idx + 0);
    indices.push(start_idx + 1);
    indices.push(start_idx + 2);
    indices.push(start_idx + 0);
    indices.push(start_idx + 2);
    indices.push(start_idx + 3);
    indices.push(start_idx + 2);
    indices.push(start_idx + 1);
    indices.push(start_idx + 0);
    indices.push(start_idx + 3);
    indices.push(start_idx + 2);
    indices.push(start_idx + 0);
}

/// View-space cuboid helper. Identical to `mob_renderer::add_cuboid` except
/// it does not need chunk light because the hand is always fully lit.
fn add_cuboid_view(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
    size: Vec3,
    offset: Vec3,
    pivot: Vec3,
    rot_yaw: f32,
    rot_pitch: f32,
    tex_cols: [u32; 6],
    tex_row: u32,
    light_val: f32,
) {
    let half = size * 0.5;

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
        (Vec3::new(half.x, half.y, -half.z), [0.0, 0.0]),
        // Face 3: East (+X)
        (Vec3::new(half.x, -half.y, half.z), [0.0, 1.0]),
        (Vec3::new(-half.x, -half.y, -half.z), [1.0, 1.0]),
        (Vec3::new(half.x, half.y, -half.z), [1.0, 0.0]),
        (Vec3::new(-half.x, half.y, half.z), [0.0, 0.0]),
        // Face 4: Up (+Y)
        (Vec3::new(-half.x, half.y, half.z), [0.0, 1.0]),
        (Vec3::new(half.x, half.y, half.z), [1.0, 1.0]),
        (Vec3::new(half.x, half.y, -half.z), [1.0, 0.0]),
        (Vec3::new(-half.x, half.y, -half.z), [0.0, 0.0]),
        // Face 5: Down (-Y)
        (Vec3::new(-half.x, -half.y, half.z), [0.0, 1.0]),
        (Vec3::new(half.x, -half.y, -half.z), [1.0, 1.0]),
        (Vec3::new(half.x, half.y, -half.z), [1.0, 0.0]),
        (Vec3::new(-half.x, half.y, -half.z), [0.0, 0.0]),
    ];

    let cos_pitch = rot_pitch.cos();
    let sin_pitch = rot_pitch.sin();
    let cos_yaw = rot_yaw.cos();
    let sin_yaw = rot_yaw.sin();

    let start_idx = vertices.len() as u32;

    for (face_idx, (local_pos, uv)) in local_corners.iter().enumerate() {
        let v1 = *local_pos + offset;
        let v2 = Vec3::new(
            v1.x,
            v1.y * cos_pitch - v1.z * sin_pitch,
            v1.y * sin_pitch + v1.z * cos_pitch,
        );
        let v3 = Vec3::new(
            v2.x * cos_yaw + v2.z * sin_yaw,
            v2.y,
            -v2.x * sin_yaw + v2.z * cos_yaw,
        );
        let final_pos = v3 + pivot;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::{Inventory, ItemStack};

    #[test]
    fn hand_mesh_contains_right_arm_and_held_block() {
        let mut inv = Inventory::new();
        inv.hotbar[0] = Some(ItemStack::new(crate::inventory::Item::Stone, 1));
        let (vertices, indices) = build_first_person_hand_mesh(&inv, 0.0, 0.0);
        assert!(!vertices.is_empty());
        assert!(!indices.is_empty());
        assert!(indices.len() % 3 == 0);
        assert!(vertices
            .iter()
            .all(|v| v.position.into_iter().all(f32::is_finite)));
    }

    #[test]
    fn hand_mesh_omits_item_when_slot_is_empty() {
        let inv = Inventory::new();
        let (vertices, indices) = build_first_person_hand_mesh(&inv, 0.0, 0.0);
        assert!(!vertices.is_empty());
        assert!(!indices.is_empty());
        assert!(indices.len() % 3 == 0);
    }

    #[test]
    fn hand_mesh_renders_flat_item_as_sprite_quad() {
        // The arm alone is a 24-vertex/36-index cuboid; a flat held item adds
        // one double-sided quad (4 vertices/12 indices) instead of a cube.
        let empty = Inventory::new();
        let (empty_vertices, empty_indices) = build_first_person_hand_mesh(&empty, 0.0, 0.0);

        let mut inv = Inventory::new();
        inv.hotbar[0] = Some(ItemStack::new(crate::inventory::Item::Seeds, 1));
        let (vertices, indices) = build_first_person_hand_mesh(&inv, 0.0, 0.0);
        assert_eq!(vertices.len(), empty_vertices.len() + 4);
        assert_eq!(indices.len(), empty_indices.len() + 12);
        assert!(vertices
            .iter()
            .all(|v| v.position.into_iter().all(f32::is_finite)));

        // A cross-model flower block uses the same flat sprite path.
        let mut flower_inv = Inventory::new();
        flower_inv.hotbar[0] = Some(ItemStack::new(crate::inventory::Item::Dandelion, 1));
        let (flower_vertices, flower_indices) = build_first_person_hand_mesh(&flower_inv, 0.0, 0.0);
        assert_eq!(flower_vertices.len(), empty_vertices.len() + 4);
        assert_eq!(flower_indices.len(), empty_indices.len() + 12);

        // A full-cube block still renders as a 24-vertex/36-index cuboid.
        let mut block_inv = Inventory::new();
        block_inv.hotbar[0] = Some(ItemStack::new(crate::inventory::Item::Stone, 1));
        let (block_vertices, block_indices) = build_first_person_hand_mesh(&block_inv, 0.0, 0.0);
        assert_eq!(block_vertices.len(), empty_vertices.len() + 24);
        assert_eq!(block_indices.len(), empty_indices.len() + 36);
    }
}
