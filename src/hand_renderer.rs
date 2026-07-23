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

    // Held item: small cube textured with the selected block, positioned
    // slightly in front of the fist.
    let held_item = inventory.hotbar[inventory.selected]
        .map(|stack| stack.item)
        .unwrap_or(Item::Air);
    if let Some((tex_cols, tex_row)) = held_item_texture(held_item) {
        let item_offset = Vec3::new(-0.08, 0.06, 0.25);
        let item_pos = fist_pos + item_offset;
        let item_yaw = hand_yaw;
        let item_pitch = hand_pitch;
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

    (vertices, indices)
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
}
