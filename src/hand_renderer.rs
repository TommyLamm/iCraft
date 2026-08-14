use crate::inventory::{Inventory, Item};
use crate::state::Vertex;
use glam::{Mat4, Vec3};

pub const HAND_VERTEX_CAPACITY: usize = 1024;
pub const HAND_INDEX_CAPACITY: usize = 1536;

/// Number of complete mining swings per second. Vanilla Minecraft completes
/// roughly four arm swings per second while the attack button is held.
const MINING_SWINGS_PER_SECOND: f32 = 4.0;

// Minecraft's 0.68 baked-model scale cannot be used directly in this
// renderer's view-space units: it places the near edge almost on the camera
// and magnifies a tool across most of the screen. These converted values fit
// the same presentation into the dedicated 70-degree hand projection.
const TOOL_MODEL_SCALE: f32 = 0.60;
const TOOL_MODEL_THICKNESS: f32 = 1.0 / 16.0;
// Keep the tool upright, but turn the generated model around so its broad
// face points in the correct first-person direction.
const OUTWARD_TOOL_YAW: f32 = 80.0 * std::f32::consts::PI / 180.0 + std::f32::consts::PI;
const UPRIGHT_TOOL_ROLL: f32 = -45.0 * std::f32::consts::PI / 180.0;
const MINECRAFT_TOOL_CENTER: Vec3 = Vec3::new(0.64, -0.22, 0.85);

const TOOL_GRIP_U: f32 = 2.0 / 16.0;
const TOOL_GRIP_V: f32 = 14.0 / 16.0;
#[cfg(test)]
const TOOL_HEAD_U: f32 = 14.0 / 16.0;
#[cfg(test)]
const TOOL_HEAD_V: f32 = 2.0 / 16.0;

// Minecraft's generated-item model extrudes the alpha outline of each 16x16
// sprite. Variants of the same tool type share one outline, so the masks stay
// independent of image IO. Bit 15 is x=0 and each array entry is one texture
// row from top to bottom.
const AXE_ALPHA_MASK: [u16; 16] = [
    0x0000, 0x0060, 0x00F0, 0x01F0, 0x03F8, 0x03F8, 0x01FC, 0x00FC, 0x01D8, 0x0380, 0x0700, 0x0E00,
    0x1C00, 0x3800, 0x3000, 0x0000,
];
const PICKAXE_ALPHA_MASK: [u16; 16] = [
    0x0000, 0x0000, 0x03E0, 0x07FC, 0x03FC, 0x003C, 0x007E, 0x00EE, 0x01CE, 0x038E, 0x070E, 0x0E04,
    0x1C00, 0x3800, 0x3000, 0x0000,
];
const SHOVEL_ALPHA_MASK: [u16; 16] = [
    0x0000, 0x0000, 0x001C, 0x003E, 0x007E, 0x00FE, 0x007C, 0x00F8, 0x01D0, 0x0380, 0x0700, 0x0E00,
    0x3C00, 0x3800, 0x1800, 0x0000,
];
const SWORD_ALPHA_MASK: [u16; 16] = [
    0x0007, 0x000F, 0x001F, 0x003E, 0x007C, 0x00F8, 0x31F0, 0x3BE0, 0x1FC0, 0x1F80, 0x0F00, 0x1F80,
    0x3BC0, 0xF0C0, 0xE000, 0xE000,
];
const SHEARS_ALPHA_MASK: [u16; 16] = [
    0x0000, 0x0000, 0x00F8, 0x01F4, 0x03EC, 0x07DC, 0x0FBC, 0x0E7C, 0x1C78, 0x1CF0, 0x1FE0, 0x27C0,
    0x2700, 0x1800, 0x0000, 0x0000,
];
const HOE_ALPHA_MASK: [u16; 16] = [
    0x0000, 0x0078, 0x00FC, 0x018C, 0x0018, 0x0030, 0x0060, 0x00C0, 0x0180, 0x0300, 0x0600, 0x0C00,
    0x1800, 0x3000, 0x2000, 0x0000,
];

/// Cache identity for the static first-person hand mesh. Animation values are
/// deliberately excluded so walking/attacking only updates the uniform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HandMeshKey {
    pub held_item: Item,
}

/// Per-frame transform uploaded alongside the cached hand mesh.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct HandAnimationUniform {
    pub transform: [[f32; 4]; 4],
}

impl HandAnimationUniform {
    pub fn from_swings(walk_swing: f32, attack_swing: f32) -> Self {
        // The static mesh is authored at the neutral hand pitch/fist position;
        // these are the same delta pitch and offsets used by the legacy path.
        let delta_pitch = walk_swing * 0.08 - attack_swing * 0.5;
        let offset = glam::Vec3::new(
            -0.1 * attack_swing,
            walk_swing * 0.03 + 0.1 * attack_swing,
            0.25 * attack_swing,
        );
        // Rotate around the fist pivot (the arm's attachment point), matching
        // the legacy per-primitive pitch while keeping the neutral mesh fixed.
        let pivot = glam::Vec3::new(0.42, -0.42, 0.95);
        let transform = glam::Mat4::from_translation(offset)
            * glam::Mat4::from_translation(pivot)
            * glam::Mat4::from_rotation_x(delta_pitch)
            * glam::Mat4::from_translation(-pivot);
        Self {
            transform: transform.to_cols_array_2d(),
        }
    }

    pub fn matrix(self) -> glam::Mat4 {
        glam::Mat4::from_cols_array_2d(&self.transform)
    }

    fn for_tool(walk_swing: f32, attack_swing: f32) -> Self {
        // Swing around the bottom of the handle. This keeps the grip close to
        // the player's hand while the blade/axe/pick head travels through a
        // wide arc, instead of pushing the whole tool forward like a fist.
        let model = minecraft_tool_transform();
        let pivot = model.transform_point3(tool_model_position(
            TOOL_GRIP_U,
            TOOL_GRIP_V,
            -TOOL_MODEL_THICKNESS * 0.5,
        ));
        // Rotate around the normal of the broad, camera-visible sprite face.
        // The working end therefore swings down through the tool's own UV
        // plane instead of rotating sideways across the bottom of the screen.
        let face_normal = model.transform_vector3(-Vec3::Z).normalize();
        let strike_angle = attack_swing * 1.85;
        let bob = Vec3::new(0.0, walk_swing * 0.025, attack_swing * 0.04);
        let transform = Mat4::from_translation(bob)
            * Mat4::from_translation(pivot)
            * Mat4::from_axis_angle(face_normal, strike_angle + walk_swing * 0.035)
            * Mat4::from_translation(-pivot);
        Self {
            transform: transform.to_cols_array_2d(),
        }
    }
}

pub fn hand_mesh_key(inventory: &Inventory) -> HandMeshKey {
    HandMeshKey {
        held_item: inventory.hotbar[inventory.selected]
            .map(|stack| stack.item)
            .unwrap_or(Item::Air),
    }
}

pub fn should_rebuild_hand_mesh(previous: Option<HandMeshKey>, next: HandMeshKey) -> bool {
    previous != Some(next)
}

/// Tools pivot around their handle so their working end performs the strike.
/// Empty hands and ordinary held items retain the arm-led punch animation.
pub fn animation_for_hand_mesh(
    key: HandMeshKey,
    walk_swing: f32,
    attack_swing: f32,
) -> HandAnimationUniform {
    if tool_alpha_mask(key.held_item).is_some() {
        HandAnimationUniform::for_tool(walk_swing, attack_swing)
    } else {
        HandAnimationUniform::from_swings(walk_swing, attack_swing)
    }
}

/// Produces a smooth 0 -> 1 -> 0 cycle for mining, melee, and air swings.
/// Returning to zero every cycle keeps the hand from snapping between swings.
pub fn hand_swing_progress(elapsed_seconds: f32, swinging: bool) -> f32 {
    if !swinging {
        return 0.0;
    }

    let phase = elapsed_seconds * MINING_SWINGS_PER_SECOND * std::f32::consts::TAU;
    0.5 - 0.5 * phase.cos()
}

/// Builds the animation-independent mesh for a held-item key.
pub fn build_first_person_hand_base_mesh(
    key: HandMeshKey,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
) {
    build_first_person_hand_base_mesh_into(key.held_item, vertices, indices);
}

fn build_first_person_hand_base_mesh_into(
    held_item: Item,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
) {
    vertices.clear();
    indices.clear();
    let hand_yaw = -0.35_f32;
    let hand_pitch = -0.25_f32;
    let fist_pos = Vec3::new(0.42, -0.42, 0.95);
    let held_tool_mask = tool_alpha_mask(held_item);

    // A tool is the complete first-person presentation, so do not also draw
    // the arm underneath it. Empty hands and ordinary held items keep the arm.
    if held_tool_mask.is_none() {
        add_cuboid_view(
            vertices,
            indices,
            Vec3::new(0.24, 0.24, 1.2),
            Vec3::new(0.0, 0.0, -0.55),
            fist_pos,
            hand_yaw,
            hand_pitch,
            [(15, 8); 6],
            1.0,
        );
    }

    if held_item != Item::Air {
        let item_pos = fist_pos + Vec3::new(-0.08, 0.06, 0.25);
        if let Some(alpha_mask) = held_tool_mask {
            let (tex_col, tex_row) = held_item.properties().tex_coords;
            add_extruded_tool_view(
                vertices,
                indices,
                alpha_mask,
                minecraft_tool_transform(),
                tex_col,
                tex_row,
                1.0,
            );
        } else if held_item.renders_flat() {
            let (tex_col, tex_row) = held_item.properties().tex_coords;
            add_sprite_view(
                vertices,
                indices,
                0.3,
                item_pos,
                Vec3::ZERO,
                hand_yaw,
                hand_pitch,
                0.0,
                tex_col,
                tex_row,
                1.0,
            );
        } else if let Some(face_tiles) = held_item_face_tiles(held_item) {
            let held_size = if held_item == Item::EndPortalFrame {
                Vec3::new(0.18, 0.18 * 13.0 / 16.0, 0.18)
            } else {
                Vec3::splat(0.18)
            };
            add_cuboid_view(
                vertices,
                indices,
                held_size,
                Vec3::ZERO,
                item_pos,
                hand_yaw,
                hand_pitch,
                face_tiles,
                1.0,
            );
        }
    }
}

fn tool_alpha_mask(item: Item) -> Option<&'static [u16; 16]> {
    match item {
        Item::WoodenSword
        | Item::StoneSword
        | Item::IronSword
        | Item::GoldenSword
        | Item::DiamondSword => Some(&SWORD_ALPHA_MASK),
        Item::WoodenPickaxe
        | Item::StonePickaxe
        | Item::IronPickaxe
        | Item::GoldenPickaxe
        | Item::DiamondPickaxe => Some(&PICKAXE_ALPHA_MASK),
        Item::WoodenAxe | Item::StoneAxe | Item::IronAxe | Item::GoldenAxe | Item::DiamondAxe => {
            Some(&AXE_ALPHA_MASK)
        }
        Item::WoodenShovel
        | Item::StoneShovel
        | Item::IronShovel
        | Item::GoldenShovel
        | Item::DiamondShovel => Some(&SHOVEL_ALPHA_MASK),
        Item::WoodenHoe | Item::StoneHoe | Item::IronHoe | Item::GoldenHoe | Item::DiamondHoe => {
            Some(&HOE_ALPHA_MASK)
        }
        Item::Shears => Some(&SHEARS_ALPHA_MASK),
        _ => None,
    }
}

fn minecraft_tool_transform() -> Mat4 {
    // Keep the broad face turned enough to show the generated item's depth,
    // then counter-rotate the sprite's diagonal handle so its head is upright
    // on screen. The model itself is unit sized.
    Mat4::from_translation(MINECRAFT_TOOL_CENTER)
        * Mat4::from_rotation_y(OUTWARD_TOOL_YAW)
        * Mat4::from_rotation_z(UPRIGHT_TOOL_ROLL)
        * Mat4::from_scale(Vec3::splat(TOOL_MODEL_SCALE))
}

/// Applies a per-frame animation transform to a cached base mesh.
pub fn apply_hand_animation(vertices: &mut [Vertex], animation: HandAnimationUniform) {
    let matrix = animation.matrix();
    for vertex in vertices {
        let p = matrix.transform_point3(Vec3::from_array(vertex.position));
        vertex.position = p.to_array();
    }
}

/// Returns an atlas tile for every face of the held item. Block items preserve
/// their world top/side/bottom mapping instead of repeating the top texture.
fn held_item_face_tiles(item: Item) -> Option<[(u32, u32); 6]> {
    let props = item.properties();
    if let Some(block) = props.block_type {
        Some(std::array::from_fn(|face_idx| {
            block.get_face_tex_index(face_idx)
        }))
    } else if item == Item::Air {
        None
    } else {
        Some([props.tex_coords; 6])
    }
}

/// Builds the first-person right-hand mesh in view space.
///
/// The hand is positioned on the right side of the screen, angled slightly
/// inward and upward like Minecraft. The view space convention is the same
/// as the main renderer: +X right, +Y up, +Z forward (left-handed).
pub fn build_first_person_hand_mesh_into(
    inventory: &Inventory,
    walk_swing: f32,
    attack_swing: f32,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
) {
    let key = hand_mesh_key(inventory);
    build_first_person_hand_base_mesh(key, vertices, indices);
    apply_hand_animation(
        vertices,
        animation_for_hand_mesh(key, walk_swing, attack_swing),
    );
}

#[cfg(test)]
pub fn build_first_person_hand_mesh(
    inventory: &Inventory,
    walk_swing: f32,
    attack_swing: f32,
) -> (Vec<Vertex>, Vec<u32>) {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    build_first_person_hand_mesh_into(
        inventory,
        walk_swing,
        attack_swing,
        &mut vertices,
        &mut indices,
    );
    (vertices, indices)
}

#[derive(Clone, Copy)]
enum ToolOutlineEdge {
    Top,
    Bottom,
    Left,
    Right,
}

fn mask_pixel_is_opaque(mask: &[u16; 16], x: i32, y: i32) -> bool {
    if !(0..16).contains(&x) || !(0..16).contains(&y) {
        return false;
    }
    mask[y as usize] & (1_u16 << (15 - x as u32)) != 0
}

/// Maps source texture coordinates into Minecraft's generated-item model.
/// The camera-facing north face uses `[16, 0, 0, 16]`, so U is mirrored in
/// local geometry while V retains the texture's top-to-bottom direction.
fn tool_model_position(u: f32, v: f32, z: f32) -> Vec3 {
    Vec3::new(0.5 - u, 0.5 - v, z)
}

fn push_tool_quad(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
    corners: [(Vec3, [f32; 2]); 4],
    transform: Mat4,
    tex_col: u32,
    tex_row: u32,
    light_val: f32,
    ao: f32,
) {
    let start_idx = vertices.len() as u32;
    for (position, uv) in corners {
        let position = transform.transform_point3(position);
        vertices.push(Vertex {
            position: position.to_array(),
            tex_coords: [
                (tex_col as f32 + uv[0]) / 16.0,
                (tex_row as f32 + uv[1]) / 16.0,
            ],
            light_level: light_val,
            ao,
        });
    }
    indices.extend_from_slice(&[
        start_idx,
        start_idx + 1,
        start_idx + 2,
        start_idx,
        start_idx + 2,
        start_idx + 3,
    ]);
}

#[allow(clippy::too_many_arguments)]
fn push_tool_outline_span(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
    edge: ToolOutlineEdge,
    anchor: usize,
    first: usize,
    last: usize,
    transform: Mat4,
    tex_col: u32,
    tex_row: u32,
    light_val: f32,
) {
    let front_z = -TOOL_MODEL_THICKNESS * 0.5;
    let back_z = TOOL_MODEL_THICKNESS * 0.5;
    let first_edge = first as f32 / 16.0;
    let last_edge = (last + 1) as f32 / 16.0;
    let anchor_start = anchor as f32 / 16.0;
    let anchor_end = (anchor + 1) as f32 / 16.0;
    let anchor_center = (anchor as f32 + 0.5) / 16.0;

    let corners = match edge {
        // Physical +Y. Source U increases from physical right to left.
        ToolOutlineEdge::Top => [
            (
                tool_model_position(last_edge, anchor_start, back_z),
                [last_edge, anchor_center],
            ),
            (
                tool_model_position(first_edge, anchor_start, back_z),
                [first_edge, anchor_center],
            ),
            (
                tool_model_position(first_edge, anchor_start, front_z),
                [first_edge, anchor_center],
            ),
            (
                tool_model_position(last_edge, anchor_start, front_z),
                [last_edge, anchor_center],
            ),
        ],
        // Physical -Y.
        ToolOutlineEdge::Bottom => [
            (
                tool_model_position(last_edge, anchor_end, front_z),
                [last_edge, anchor_center],
            ),
            (
                tool_model_position(first_edge, anchor_end, front_z),
                [first_edge, anchor_center],
            ),
            (
                tool_model_position(first_edge, anchor_end, back_z),
                [first_edge, anchor_center],
            ),
            (
                tool_model_position(last_edge, anchor_end, back_z),
                [last_edge, anchor_center],
            ),
        ],
        // The source texture's left edge becomes physical +X after the north
        // face's U mirror.
        ToolOutlineEdge::Left => [
            (
                tool_model_position(anchor_start, last_edge, back_z),
                [anchor_center, last_edge],
            ),
            (
                tool_model_position(anchor_start, last_edge, front_z),
                [anchor_center, last_edge],
            ),
            (
                tool_model_position(anchor_start, first_edge, front_z),
                [anchor_center, first_edge],
            ),
            (
                tool_model_position(anchor_start, first_edge, back_z),
                [anchor_center, first_edge],
            ),
        ],
        // The source texture's right edge becomes physical -X.
        ToolOutlineEdge::Right => [
            (
                tool_model_position(anchor_end, last_edge, front_z),
                [anchor_center, last_edge],
            ),
            (
                tool_model_position(anchor_end, last_edge, back_z),
                [anchor_center, last_edge],
            ),
            (
                tool_model_position(anchor_end, first_edge, back_z),
                [anchor_center, first_edge],
            ),
            (
                tool_model_position(anchor_end, first_edge, front_z),
                [anchor_center, first_edge],
            ),
        ],
    };

    // The regular shader has no face normals, so a small Minecraft-like shade
    // difference is baked into outline faces to make the 1/16 depth readable.
    push_tool_quad(
        vertices, indices, corners, transform, tex_col, tex_row, light_val, 0.82,
    );
}

/// Minecraft generated-item geometry: two broad faces one pixel-thickness
/// apart, plus one quad for every alpha-outline span. Transparent fragments on
/// the broad faces and gaps inside a merged span are discarded by the shader.
#[allow(clippy::too_many_arguments)]
fn add_extruded_tool_view(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
    alpha_mask: &[u16; 16],
    transform: Mat4,
    tex_col: u32,
    tex_row: u32,
    light_val: f32,
) {
    let front_z = -TOOL_MODEL_THICKNESS * 0.5;
    let back_z = TOOL_MODEL_THICKNESS * 0.5;

    // North (-Z), facing this renderer's +Z-looking camera. Its U direction
    // is mirrored exactly like vanilla's FACING_NORTH_UV.
    push_tool_quad(
        vertices,
        indices,
        [
            (tool_model_position(0.0, 1.0, front_z), [0.0, 1.0]),
            (tool_model_position(1.0, 1.0, front_z), [1.0, 1.0]),
            (tool_model_position(1.0, 0.0, front_z), [1.0, 0.0]),
            (tool_model_position(0.0, 0.0, front_z), [0.0, 0.0]),
        ],
        transform,
        tex_col,
        tex_row,
        light_val,
        1.0,
    );
    // South (+Z), with the opposite winding. The first-person transform turns
    // this face toward the camera, so keep its UVs aligned with the physical
    // alpha outline instead of mirroring the sprite inside the correct frame.
    push_tool_quad(
        vertices,
        indices,
        [
            (tool_model_position(1.0, 1.0, back_z), [1.0, 1.0]),
            (tool_model_position(0.0, 1.0, back_z), [0.0, 1.0]),
            (tool_model_position(0.0, 0.0, back_z), [0.0, 0.0]),
            (tool_model_position(1.0, 0.0, back_z), [1.0, 0.0]),
        ],
        transform,
        tex_col,
        tex_row,
        light_val,
        1.0,
    );

    for edge in [
        ToolOutlineEdge::Top,
        ToolOutlineEdge::Bottom,
        ToolOutlineEdge::Left,
        ToolOutlineEdge::Right,
    ] {
        for anchor in 0..16 {
            let mut first = None;
            let mut last = 0;
            for along in 0..16 {
                let along_i = along as i32;
                let anchor_i = anchor as i32;
                let (x, y, neighbor_x, neighbor_y) = match edge {
                    ToolOutlineEdge::Top => (along_i, anchor_i, along_i, anchor_i - 1),
                    ToolOutlineEdge::Bottom => (along_i, anchor_i, along_i, anchor_i + 1),
                    ToolOutlineEdge::Left => (anchor_i, along_i, anchor_i - 1, along_i),
                    ToolOutlineEdge::Right => (anchor_i, along_i, anchor_i + 1, along_i),
                };
                if mask_pixel_is_opaque(alpha_mask, x, y)
                    && !mask_pixel_is_opaque(alpha_mask, neighbor_x, neighbor_y)
                {
                    first.get_or_insert(along);
                    last = along;
                }
            }

            if let Some(first) = first {
                push_tool_outline_span(
                    vertices, indices, edge, anchor, first, last, transform, tex_col, tex_row,
                    light_val,
                );
            }
        }
    }
}

/// View-space flat sprite helper for held items that render flat (flowers,
/// seeds, food, ...). Tools use `add_extruded_tool_view` instead. Emits a
/// single double-sided quad in the local XY
/// plane, rotated like `add_cuboid_view`.
#[allow(clippy::too_many_arguments)]
fn add_sprite_view(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
    size: f32,
    pivot: Vec3,
    local_anchor: Vec3,
    rot_yaw: f32,
    rot_pitch: f32,
    rot_roll: f32,
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
    let cos_roll = rot_roll.cos();
    let sin_roll = rot_roll.sin();

    let start_idx = vertices.len() as u32;

    for (local_pos, uv) in local_corners.iter() {
        let local_pos = *local_pos - local_anchor;
        let v1 = Vec3::new(
            local_pos.x * cos_roll - local_pos.y * sin_roll,
            local_pos.x * sin_roll + local_pos.y * cos_roll,
            local_pos.z,
        );
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
    face_tiles: [(u32, u32); 6],
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

        let (col, row) = face_tiles[face_idx / 4];
        let u = (uv[0] + col as f32) * 0.0625;
        let v = (uv[1] + row as f32) * 0.0625;

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
    use crate::inventory::{Inventory, ItemStack, ALL_ITEMS};

    #[test]
    fn held_end_portal_frame_preserves_world_face_tiles() {
        let tiles = held_item_face_tiles(Item::EndPortalFrame).unwrap();
        assert_eq!(tiles[0], (9, 4));
        assert_eq!(tiles[4], (15, 15));
        assert_eq!(tiles[5], (9, 4));
    }

    #[test]
    fn held_cuboid_faces_are_planar_and_use_independent_tile_rows() {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let tiles = [(0, 0), (1, 1), (2, 2), (3, 3), (4, 4), (5, 5)];
        add_cuboid_view(
            &mut vertices,
            &mut indices,
            Vec3::splat(2.0),
            Vec3::ZERO,
            Vec3::ZERO,
            0.0,
            0.0,
            tiles,
            1.0,
        );

        assert_eq!(vertices.len(), 24);
        assert_eq!(indices.len(), 36);
        for (face_idx, face) in vertices.chunks_exact(4).enumerate() {
            let planar_axes = (0..3)
                .filter(|axis| {
                    face.iter()
                        .all(|vertex| vertex.position[*axis] == face[0].position[*axis])
                })
                .count();
            assert_eq!(planar_axes, 1, "face {face_idx} must lie on one plane");

            let expected_row = tiles[face_idx].1 as f32 * 0.0625;
            let min_v = face
                .iter()
                .map(|vertex| vertex.tex_coords[1])
                .fold(f32::INFINITY, f32::min);
            assert!((min_v - expected_row).abs() < 1e-6);
        }
    }

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

    fn tool_mesh_cases() -> [(Item, usize, usize); 13] {
        // Tools are complete first-person meshes and do not include the arm.
        [
            (Item::StoneSword, 200, 300),
            (Item::StonePickaxe, 176, 264),
            (Item::StoneAxe, 172, 258),
            (Item::StoneShovel, 168, 252),
            (Item::IronSword, 200, 300),
            (Item::IronPickaxe, 176, 264),
            (Item::IronAxe, 172, 258),
            (Item::IronShovel, 168, 252),
            (Item::DiamondSword, 200, 300),
            (Item::DiamondPickaxe, 176, 264),
            (Item::DiamondAxe, 172, 258),
            (Item::DiamondShovel, 168, 252),
            (Item::Shears, 192, 288),
        ]
    }

    fn png_alpha_mask(bytes: &[u8]) -> [u16; 16] {
        let image = image::load_from_memory(bytes).unwrap().into_rgba8();
        assert_eq!(image.dimensions(), (16, 16));
        std::array::from_fn(|y| {
            let mut row = 0_u16;
            for x in 0..16 {
                if image.get_pixel(x, y as u32).0[3] != 0 {
                    row |= 1_u16 << (15 - x);
                }
            }
            row
        })
    }

    #[test]
    fn bundled_tool_alpha_masks_match_source_textures() {
        let assets: [(&[u16; 16], &[u8]); 13] = [
            (
                &SWORD_ALPHA_MASK,
                include_bytes!("../assets/vanilla/textures/item/stone_sword.png"),
            ),
            (
                &PICKAXE_ALPHA_MASK,
                include_bytes!("../assets/vanilla/textures/item/stone_pickaxe.png"),
            ),
            (
                &AXE_ALPHA_MASK,
                include_bytes!("../assets/vanilla/textures/item/stone_axe.png"),
            ),
            (
                &SHOVEL_ALPHA_MASK,
                include_bytes!("../assets/vanilla/textures/item/stone_shovel.png"),
            ),
            (
                &SWORD_ALPHA_MASK,
                include_bytes!("../assets/vanilla/textures/item/iron_sword.png"),
            ),
            (
                &PICKAXE_ALPHA_MASK,
                include_bytes!("../assets/vanilla/textures/item/iron_pickaxe.png"),
            ),
            (
                &AXE_ALPHA_MASK,
                include_bytes!("../assets/vanilla/textures/item/iron_axe.png"),
            ),
            (
                &SHOVEL_ALPHA_MASK,
                include_bytes!("../assets/vanilla/textures/item/iron_shovel.png"),
            ),
            (
                &SWORD_ALPHA_MASK,
                include_bytes!("../assets/vanilla/textures/item/diamond_sword.png"),
            ),
            (
                &PICKAXE_ALPHA_MASK,
                include_bytes!("../assets/vanilla/textures/item/diamond_pickaxe.png"),
            ),
            (
                &AXE_ALPHA_MASK,
                include_bytes!("../assets/vanilla/textures/item/diamond_axe.png"),
            ),
            (
                &SHOVEL_ALPHA_MASK,
                include_bytes!("../assets/vanilla/textures/item/diamond_shovel.png"),
            ),
            (
                &SHEARS_ALPHA_MASK,
                include_bytes!("../assets/vanilla/textures/item/shears.png"),
            ),
        ];

        for (expected, bytes) in assets {
            assert_eq!(&png_alpha_mask(bytes), expected);
        }
    }

    #[test]
    fn every_registered_tool_has_an_extrusion_mask() {
        for item in ALL_ITEMS.iter().copied() {
            assert_eq!(
                tool_alpha_mask(item).is_some(),
                item.tool_properties().is_some(),
                "{item:?} tool registration and extrusion mask disagree"
            );
        }
    }

    #[test]
    fn generated_item_faces_have_outward_winding_and_vanilla_uv_directions() {
        let mut one_pixel_mask = [0_u16; 16];
        one_pixel_mask[8] = 1_u16 << 8;
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        add_extruded_tool_view(
            &mut vertices,
            &mut indices,
            &one_pixel_mask,
            Mat4::IDENTITY,
            0,
            0,
            1.0,
        );

        assert_eq!(vertices.len(), 24);
        assert_eq!(indices.len(), 36);
        let expected_normals = [-Vec3::Z, Vec3::Z, Vec3::Y, -Vec3::Y, Vec3::X, -Vec3::X];
        for (face, expected_normal) in vertices.chunks_exact(4).zip(expected_normals) {
            let a = Vec3::from_array(face[0].position);
            let b = Vec3::from_array(face[1].position);
            let c = Vec3::from_array(face[2].position);
            let normal = (b - a).cross(c - a).normalize();
            assert!(normal.dot(expected_normal) > 0.999);
        }

        // Both broad faces keep texture U aligned with the generated alpha
        // outline. This prevents the visible south face from showing a
        // mirrored sprite after the first-person tool is turned around.
        assert!(vertices[0].position[0] > 0.0 && vertices[0].tex_coords[0] == 0.0);
        assert!(vertices[1].position[0] < 0.0 && vertices[1].tex_coords[0] == 1.0 / 16.0);
        assert!(vertices[4].position[0] < 0.0 && vertices[4].tex_coords[0] == 1.0 / 16.0);
        assert!(vertices[5].position[0] > 0.0 && vertices[5].tex_coords[0] == 0.0);
    }

    #[test]
    fn minecraft_tools_render_as_closed_extruded_models() {
        for (tool, expected_vertices, expected_indices) in tool_mesh_cases() {
            let mut inv = Inventory::new();
            inv.hotbar[0] = Some(ItemStack::new(tool, 1));
            let (vertices, indices) = build_first_person_hand_mesh(&inv, 0.0, 0.0);

            assert_eq!(vertices.len(), expected_vertices, "{tool:?} vertices");
            assert_eq!(indices.len(), expected_indices, "{tool:?} indices");
            assert!(vertices.len() <= HAND_VERTEX_CAPACITY, "{tool:?}");
            assert!(indices.len() <= HAND_INDEX_CAPACITY, "{tool:?}");
            assert!(indices.iter().all(|index| *index < vertices.len() as u32));

            for triangle in indices.chunks_exact(3) {
                let a = Vec3::from_array(vertices[triangle[0] as usize].position);
                let b = Vec3::from_array(vertices[triangle[1] as usize].position);
                let c = Vec3::from_array(vertices[triangle[2] as usize].position);
                assert!(
                    (b - a).cross(c - a).length_squared() > 1e-12,
                    "{tool:?} contains a degenerate triangle: {triangle:?}"
                );
            }

            let (tex_col, tex_row) = tool.properties().tex_coords;
            let min_u = tex_col as f32 / 16.0;
            let max_u = (tex_col + 1) as f32 / 16.0;
            let min_v = tex_row as f32 / 16.0;
            let max_v = (tex_row + 1) as f32 / 16.0;
            assert!(vertices.iter().all(|vertex| {
                (min_u - 1e-6..=max_u + 1e-6).contains(&vertex.tex_coords[0])
                    && (min_v - 1e-6..=max_v + 1e-6).contains(&vertex.tex_coords[1])
            }));
            assert!(
                vertices.iter().any(|vertex| vertex.ao < 1.0),
                "{tool:?} must contain shaded outline faces"
            );
        }
    }

    #[test]
    fn tool_models_have_minecrafts_one_pixel_depth() {
        for (tool, _, _) in tool_mesh_cases() {
            let mut inv = Inventory::new();
            inv.hotbar[0] = Some(ItemStack::new(tool, 1));
            let (vertices, _) = build_first_person_hand_mesh(&inv, 0.0, 0.0);
            let model = &vertices[..];
            let expected_depth = TOOL_MODEL_SCALE * TOOL_MODEL_THICKNESS;

            // The first eight vertices are matching corners of the north and
            // south faces. Their separation is the generated model's 1/16.
            for (front, back) in [(0, 5), (1, 4), (2, 7), (3, 6)] {
                let front = Vec3::from_array(model[front].position);
                let back = Vec3::from_array(model[back].position);
                assert!(
                    ((back - front).length() - expected_depth).abs() < 1e-5,
                    "{tool:?} depth differs from one texture pixel"
                );
            }

            let a = Vec3::from_array(model[0].position);
            let b = Vec3::from_array(model[1].position);
            let c = Vec3::from_array(model[3].position);
            let d = Vec3::from_array(model[5].position);
            assert!(
                (b - a).cross(c - a).dot(d - a).abs() > 1e-5,
                "{tool:?} must be non-coplanar"
            );
        }
    }

    #[test]
    fn tools_follow_the_hand_animation() {
        for (tool, _, _) in tool_mesh_cases() {
            let mut inv = Inventory::new();
            inv.hotbar[0] = Some(ItemStack::new(tool, 1));
            let idle = build_first_person_hand_mesh(&inv, 0.0, 0.0);
            let moving = build_first_person_hand_mesh(&inv, 0.6, 1.0);
            assert_ne!(
                bytemuck::cast_slice::<Vertex, u8>(&idle.0),
                bytemuck::cast_slice::<Vertex, u8>(&moving.0),
                "{tool:?} must move with the mining swing"
            );
            assert_eq!(idle.1, moving.1);
        }

        let empty = Inventory::new();
        let idle_hand = build_first_person_hand_mesh(&empty, 0.0, 0.0);
        let moving_hand = build_first_person_hand_mesh(&empty, 0.6, 1.0);
        assert_ne!(
            bytemuck::cast_slice::<Vertex, u8>(&idle_hand.0),
            bytemuck::cast_slice::<Vertex, u8>(&moving_hand.0),
            "the empty hand should retain its own animation"
        );
    }

    #[test]
    fn tool_attack_swings_the_head_around_a_stable_grip() {
        let key = HandMeshKey {
            held_item: Item::DiamondSword,
        };
        let idle = animation_for_hand_mesh(key, 0.0, 0.0).matrix();
        let striking = animation_for_hand_mesh(key, 0.0, 1.0).matrix();
        let model = minecraft_tool_transform();
        let grip = model.transform_point3(tool_model_position(
            TOOL_GRIP_U,
            TOOL_GRIP_V,
            -TOOL_MODEL_THICKNESS * 0.5,
        ));
        let head = model.transform_point3(tool_model_position(
            TOOL_HEAD_U,
            TOOL_HEAD_V,
            -TOOL_MODEL_THICKNESS * 0.5,
        ));

        let grip_travel = striking
            .transform_point3(grip)
            .distance(idle.transform_point3(grip));
        let head_travel = striking
            .transform_point3(head)
            .distance(idle.transform_point3(head));
        assert!(grip_travel < 0.05, "tool grip moved too far: {grip_travel}");
        assert!(
            head_travel > 0.75,
            "tool head must trace the attack arc: {head_travel}"
        );
        assert!(
            head_travel > grip_travel * 12.0,
            "the working end must lead the strike"
        );
        let idle_head = idle.transform_point3(head);
        let striking_head = striking.transform_point3(head);
        assert!(
            striking_head.y < idle_head.y - 0.45,
            "the tool head must swing downward: idle={idle_head:?}, strike={striking_head:?}"
        );
        assert!(
            (striking_head.x - idle_head.x).abs() < 0.30,
            "the tool must not sweep sideways across the HUD: idle={idle_head:?}, strike={striking_head:?}"
        );
    }

    #[test]
    fn tool_pose_is_upright_in_the_first_person_right_hand() {
        let transform = minecraft_tool_transform();
        assert!(
            transform
                .transform_point3(Vec3::ZERO)
                .distance(MINECRAFT_TOOL_CENTER)
                < 1e-6
        );

        let grip = transform.transform_point3(tool_model_position(
            TOOL_GRIP_U,
            TOOL_GRIP_V,
            -TOOL_MODEL_THICKNESS * 0.5,
        ));
        let head = transform.transform_point3(tool_model_position(
            TOOL_HEAD_U,
            TOOL_HEAD_V,
            -TOOL_MODEL_THICKNESS * 0.5,
        ));
        assert!(
            grip.distance(Vec3::new(0.658_465_15, -0.538_198_05, 0.853_255_9)) < 1e-6,
            "upright grip endpoint changed: {grip:?}"
        );
        assert!(
            head.distance(Vec3::new(0.658_465_15, 0.098_198_05, 0.853_255_9)) < 1e-6,
            "upright head endpoint changed: {head:?}"
        );
        let direction = head - grip;
        assert!(
            direction.x.abs() < 1e-6 && direction.y > 0.63 && direction.z.abs() < 1e-6,
            "tool head must point straight upward from its grip: {direction:?}"
        );

        // Lock the enlarged lower-right placement at the user's 1915x978
        // aspect. The head and grip share one screen X, so it stays upright.
        let width = 1915.0;
        let height = 978.0;
        let aspect = width / height;
        let tan_half_fov = 35.0_f32.to_radians().tan();
        let project = |point: Vec3| {
            Vec3::new(
                point.x / (point.z * tan_half_fov * aspect),
                point.y / (point.z * tan_half_fov),
                0.0,
            )
        };
        let grip_screen = project(grip);
        let head_screen = project(head);
        assert!((0.55..0.58).contains(&grip_screen.x));
        assert!((-0.92..-0.89).contains(&grip_screen.y));
        assert!((0.55..0.58).contains(&head_screen.x));
        assert!((0.15..0.18).contains(&head_screen.y));
        assert!((grip_screen.x - head_screen.x).abs() < 1e-6);

        let mut min_pixel = Vec3::splat(f32::INFINITY);
        let mut max_pixel = Vec3::splat(f32::NEG_INFINITY);
        for y in 0..16 {
            for x in 0..16 {
                if !mask_pixel_is_opaque(&SWORD_ALPHA_MASK, x, y) {
                    continue;
                }
                for (u, v) in [
                    (x as f32 / 16.0, y as f32 / 16.0),
                    ((x + 1) as f32 / 16.0, y as f32 / 16.0),
                    ((x + 1) as f32 / 16.0, (y + 1) as f32 / 16.0),
                    (x as f32 / 16.0, (y + 1) as f32 / 16.0),
                ] {
                    let point = transform.transform_point3(tool_model_position(
                        u,
                        v,
                        -TOOL_MODEL_THICKNESS * 0.5,
                    ));
                    let ndc = project(point);
                    let pixel = Vec3::new(
                        (ndc.x + 1.0) * width * 0.5,
                        (1.0 - ndc.y) * height * 0.5,
                        point.z,
                    );
                    min_pixel = min_pixel.min(pixel);
                    max_pixel = max_pixel.max(pixel);
                }
            }
        }
        assert!(
            (1355.0..1375.0).contains(&min_pixel.x),
            "tool left edge changed: {min_pixel:?}"
        );
        assert!(
            (1700.0..1720.0).contains(&max_pixel.x),
            "tool right edge changed: {max_pixel:?}"
        );
        assert!(
            (310.0..330.0).contains(&min_pixel.y),
            "tool top edge changed: {min_pixel:?}"
        );
        assert!(
            (1010.0..1030.0).contains(&max_pixel.y),
            "tool bottom edge changed: {max_pixel:?}"
        );
        assert!(min_pixel.z > 0.60, "tool must stay clear of the near plane");

        let broad_face_normal = transform.transform_vector3(-Vec3::Z).normalize();
        let expected_outward_normal =
            Vec3::new(-OUTWARD_TOOL_YAW.sin(), 0.0, -OUTWARD_TOOL_YAW.cos());
        assert!(
            broad_face_normal.dot(expected_outward_normal) > 0.999_999,
            "tool broad face must keep its 260-degree outward yaw: {broad_face_normal:?}"
        );
        assert!(broad_face_normal.x.abs() > broad_face_normal.z.abs());
    }

    #[test]
    fn base_mesh_is_animation_independent_and_rebuild_only_on_key_change() {
        let mut inv = Inventory::new();
        inv.hotbar[0] = Some(ItemStack::new(crate::inventory::Item::Stone, 1));
        let key = hand_mesh_key(&inv);
        let mut a = (Vec::new(), Vec::new());
        let mut b = (Vec::new(), Vec::new());
        build_first_person_hand_base_mesh(key, &mut a.0, &mut a.1);
        build_first_person_hand_base_mesh(key, &mut b.0, &mut b.1);
        assert_eq!(
            bytemuck::cast_slice::<Vertex, u8>(&a.0),
            bytemuck::cast_slice::<Vertex, u8>(&b.0)
        );
        assert_eq!(a.1, b.1);
        assert!(!should_rebuild_hand_mesh(Some(key), key));
        assert!(should_rebuild_hand_mesh(None, key));
        assert!(should_rebuild_hand_mesh(
            Some(key),
            HandMeshKey {
                held_item: Item::Air
            }
        ));
    }

    #[test]
    fn animation_uniform_changes_with_walk_and_attack() {
        let idle = HandAnimationUniform::from_swings(0.0, 0.0);
        let swing = HandAnimationUniform::from_swings(0.5, 0.25);
        assert_ne!(idle.transform, swing.transform);
        let bytes = bytemuck::bytes_of(&swing);
        assert_eq!(bytes.len(), std::mem::size_of::<HandAnimationUniform>());
    }

    #[test]
    fn hand_swing_repeats_smoothly_only_while_active() {
        assert_eq!(hand_swing_progress(0.0, false), 0.0);
        assert_eq!(hand_swing_progress(12.0, false), 0.0);
        assert!(hand_swing_progress(0.0, true).abs() < 1e-6);
        assert!((hand_swing_progress(0.125, true) - 1.0).abs() < 1e-6);
        assert!(hand_swing_progress(0.25, true).abs() < 1e-6);
        assert!((hand_swing_progress(0.375, true) - 1.0).abs() < 1e-6);
    }
}
