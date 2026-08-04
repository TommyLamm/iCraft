//! VoxelShape — a reusable collection of AABB boxes for collision, selection, and occlusion.
//!
//! This module replaces ad-hoc per-block collision logic with a uniform shape API.
//! Every `BlockType` provides three shapes:
//!
//! - `collision_shape` – used by player physics, entity movement, and falling-block
//!   collision. Defines physical boundaries moving entities cannot cross.
//! - `selection_shape` – used by the DDA ray-caster to determine which block the
//!   player is looking at.
//! - `occlusion_shape` – used by the culling system to decide whether a face of
//!   a neighbouring block is hidden. Only full-cube blocks occlude.

use crate::chunk_manager::ChunkManager;
use crate::physics::AABB;
use crate::redstone::Direction;
use crate::world::{BlockState, BlockType};
use glam::Vec3;

/// Maximum number of AABB elements any single shape can contain.
const MAX_SHAPE_BOXES: usize = 8;

/// A compound collision/selection/occlusion shape made of up to `MAX_SHAPE_BOXES` AABBs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VoxelShape {
    pub boxes: [AABB; MAX_SHAPE_BOXES],
    pub count: u8,
}

impl VoxelShape {
    /// An empty shape (no boxes).
    pub const EMPTY: VoxelShape = VoxelShape {
        boxes: [AABB {
            min: Vec3::ZERO,
            max: Vec3::ZERO,
        }; MAX_SHAPE_BOXES],
        count: 0,
    };

    /// A full unit cube `[0,1]³`.
    pub const FULL_CUBE: VoxelShape = VoxelShape {
        boxes: [AABB {
            min: Vec3::ZERO,
            max: Vec3::splat(1.0),
        }; MAX_SHAPE_BOXES],
        count: 1,
    };

    /// Build a shape from a single box.
    pub fn from_box(b: AABB) -> Self {
        let mut boxes = [AABB {
            min: Vec3::ZERO,
            max: Vec3::ZERO,
        }; MAX_SHAPE_BOXES];
        boxes[0] = b;
        VoxelShape { boxes, count: 1 }
    }

    /// Build a shape from a slice of boxes (clamped to MAX_SHAPE_BOXES).
    pub fn from_boxes(bxs: &[AABB]) -> Self {
        let count = bxs.len().min(MAX_SHAPE_BOXES) as u8;
        let mut boxes = [AABB {
            min: Vec3::ZERO,
            max: Vec3::ZERO,
        }; MAX_SHAPE_BOXES];
        for (i, b) in bxs.iter().enumerate().take(count as usize) {
            boxes[i] = *b;
        }
        VoxelShape { boxes, count }
    }

    /// Iterate over valid boxes.
    pub fn iter(&self) -> impl Iterator<Item = &AABB> {
        self.boxes[..self.count as usize].iter()
    }

    /// Returns `true` if the shape has no boxes.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Translate all boxes by `offset` (in world units).
    pub fn translate(&self, offset: Vec3) -> VoxelShape {
        let mut out = *self;
        for b in out.boxes[..out.count as usize].iter_mut() {
            b.min += offset;
            b.max += offset;
        }
        out
    }

    /// Returns `true` if any box in the shape intersects `other`.
    pub fn intersects(&self, other: &AABB) -> bool {
        self.iter().any(|b| b.intersects(other))
    }

    /// Returns `true` if this shape contains the given point.
    pub fn contains_point(&self, point: Vec3) -> bool {
        self.iter().any(|b| {
            point.x >= b.min.x
                && point.x <= b.max.x
                && point.y >= b.min.y
                && point.y <= b.max.y
                && point.z >= b.min.z
                && point.z <= b.max.z
        })
    }

    /// Returns `true` if this shape is a full unit cube that fully occludes adjacent faces.
    pub fn is_full_occluder(&self) -> bool {
        self.count == 1
            && (self.boxes[0].min - Vec3::ZERO).length_squared() < 1e-6
            && (self.boxes[0].max - Vec3::splat(1.0)).length_squared() < 1e-6
    }

    /// Ray intersection test returning `Some((distance, normal))` for the closest hit box.
    pub fn ray_intersects(&self, origin: Vec3, dir: Vec3, max_dist: f32) -> Option<(f32, Vec3)> {
        let mut closest_t = max_dist + 1.0;
        let mut closest_normal = Vec3::ZERO;
        let mut hit = false;

        for b in self.iter() {
            if let Some((t, norm)) = ray_intersects_aabb(origin, dir, max_dist, b) {
                if t < closest_t {
                    closest_t = t;
                    closest_normal = norm;
                    hit = true;
                }
            }
        }

        if hit && closest_t <= max_dist {
            Some((closest_t, closest_normal))
        } else {
            None
        }
    }
}

/// Ray-AABB intersection helper.
pub fn ray_intersects_aabb(
    origin: Vec3,
    dir: Vec3,
    max_dist: f32,
    box_aabb: &AABB,
) -> Option<(f32, Vec3)> {
    let mut t_min = 0.0f32;
    let mut t_max = max_dist;
    let mut normal = Vec3::ZERO;

    for i in 0..3 {
        let dir_comp = dir[i];
        if dir_comp.abs() < 1e-8 {
            if origin[i] < box_aabb.min[i] || origin[i] > box_aabb.max[i] {
                return None;
            }
        } else {
            let inv_d = 1.0 / dir_comp;
            let mut t0 = (box_aabb.min[i] - origin[i]) * inv_d;
            let mut t1 = (box_aabb.max[i] - origin[i]) * inv_d;
            let mut n0 = Vec3::ZERO;
            n0[i] = -dir_comp.signum();

            if inv_d < 0.0 {
                std::mem::swap(&mut t0, &mut t1);
                n0 = -n0;
            }

            if t0 > t_min {
                t_min = t0;
                normal = n0;
            }
            t_max = t_max.min(t1);

            if t_max < t_min {
                return None;
            }
        }
    }

    if t_min <= max_dist {
        Some((t_min, normal))
    } else {
        None
    }
}

pub fn aabb(min_x: f32, min_y: f32, min_z: f32, max_x: f32, max_y: f32, max_z: f32) -> AABB {
    AABB {
        min: Vec3::new(min_x, min_y, min_z),
        max: Vec3::new(max_x, max_y, max_z),
    }
}

pub const SIXTEENTH: f32 = 1.0 / 16.0;

// Pre-built shapes
pub fn slab_bottom() -> VoxelShape {
    VoxelShape::from_box(aabb(0.0, 0.0, 0.0, 1.0, 0.5, 1.0))
}

pub fn slab_top() -> VoxelShape {
    VoxelShape::from_box(aabb(0.0, 0.5, 0.0, 1.0, 1.0, 1.0))
}

pub fn stair_north() -> VoxelShape {
    VoxelShape::from_boxes(&[
        aabb(0.0, 0.0, 0.0, 1.0, 0.5, 1.0),
        aabb(0.0, 0.5, 0.0, 1.0, 1.0, 0.5),
    ])
}

pub fn stair_south() -> VoxelShape {
    VoxelShape::from_boxes(&[
        aabb(0.0, 0.0, 0.0, 1.0, 0.5, 1.0),
        aabb(0.0, 0.5, 0.5, 1.0, 1.0, 1.0),
    ])
}

pub fn stair_west() -> VoxelShape {
    VoxelShape::from_boxes(&[
        aabb(0.0, 0.0, 0.0, 1.0, 0.5, 1.0),
        aabb(0.0, 0.5, 0.0, 0.5, 1.0, 1.0),
    ])
}

pub fn stair_east() -> VoxelShape {
    VoxelShape::from_boxes(&[
        aabb(0.0, 0.0, 0.0, 1.0, 0.5, 1.0),
        aabb(0.5, 0.5, 0.0, 1.0, 1.0, 1.0),
    ])
}

pub fn stair_inverted_north() -> VoxelShape {
    VoxelShape::from_boxes(&[
        aabb(0.0, 0.5, 0.0, 1.0, 1.0, 1.0),
        aabb(0.0, 0.0, 0.0, 1.0, 0.5, 0.5),
    ])
}

pub fn stair_inverted_south() -> VoxelShape {
    VoxelShape::from_boxes(&[
        aabb(0.0, 0.5, 0.0, 1.0, 1.0, 1.0),
        aabb(0.0, 0.0, 0.5, 1.0, 0.5, 1.0),
    ])
}

pub fn stair_inverted_west() -> VoxelShape {
    VoxelShape::from_boxes(&[
        aabb(0.0, 0.5, 0.0, 1.0, 1.0, 1.0),
        aabb(0.0, 0.0, 0.0, 0.5, 0.5, 1.0),
    ])
}

pub fn stair_inverted_east() -> VoxelShape {
    VoxelShape::from_boxes(&[
        aabb(0.0, 0.5, 0.0, 1.0, 1.0, 1.0),
        aabb(0.5, 0.0, 0.0, 1.0, 0.5, 1.0),
    ])
}

pub fn fence_shape_connected(north: bool, south: bool, west: bool, east: bool) -> VoxelShape {
    let p = 6.0 * SIXTEENTH;
    let q = 10.0 * SIXTEENTH;
    let mut bxs = Vec::with_capacity(5);
    bxs.push(aabb(p, 0.0, p, q, 1.0, q)); // centre post
    if north {
        bxs.push(aabb(p, 6.0 * SIXTEENTH, 0.0, q, 10.0 * SIXTEENTH, p));
    }
    if south {
        bxs.push(aabb(p, 6.0 * SIXTEENTH, q, q, 10.0 * SIXTEENTH, 1.0));
    }
    if west {
        bxs.push(aabb(0.0, 6.0 * SIXTEENTH, p, p, 10.0 * SIXTEENTH, q));
    }
    if east {
        bxs.push(aabb(q, 6.0 * SIXTEENTH, p, 1.0, 10.0 * SIXTEENTH, q));
    }
    VoxelShape::from_boxes(&bxs)
}

pub fn wall_shape_connected(north: bool, south: bool, west: bool, east: bool) -> VoxelShape {
    let p = 4.0 * SIXTEENTH;
    let q = 12.0 * SIXTEENTH;
    let mut bxs = Vec::with_capacity(5);
    bxs.push(aabb(p, 0.0, p, q, 1.0, q));
    if north {
        bxs.push(aabb(p, p, 0.0, q, q, p));
    }
    if south {
        bxs.push(aabb(p, p, q, q, q, 1.0));
    }
    if west {
        bxs.push(aabb(0.0, p, p, p, q, q));
    }
    if east {
        bxs.push(aabb(q, p, p, 1.0, q, q));
    }
    VoxelShape::from_boxes(&bxs)
}

pub fn pane_shape_connected(north: bool, south: bool, west: bool, east: bool) -> VoxelShape {
    let p = 7.0 * SIXTEENTH;
    let q = 9.0 * SIXTEENTH;
    let mut bxs = Vec::with_capacity(5);
    bxs.push(aabb(p, 0.0, p, q, 1.0, q));
    if north {
        bxs.push(aabb(p, 0.0, 0.0, q, 1.0, p));
    }
    if south {
        bxs.push(aabb(p, 0.0, q, q, 1.0, 1.0));
    }
    if west {
        bxs.push(aabb(0.0, 0.0, p, p, 1.0, q));
    }
    if east {
        bxs.push(aabb(q, 0.0, p, 1.0, 1.0, q));
    }
    VoxelShape::from_boxes(&bxs)
}

pub fn ladder_north() -> VoxelShape {
    VoxelShape::from_box(aabb(0.0, 0.0, 0.0, 1.0, 1.0, 2.0 * SIXTEENTH))
}

pub fn ladder_south() -> VoxelShape {
    VoxelShape::from_box(aabb(0.0, 0.0, 14.0 * SIXTEENTH, 1.0, 1.0, 1.0))
}

pub fn ladder_west() -> VoxelShape {
    VoxelShape::from_box(aabb(0.0, 0.0, 0.0, 2.0 * SIXTEENTH, 1.0, 1.0))
}

pub fn ladder_east() -> VoxelShape {
    VoxelShape::from_box(aabb(14.0 * SIXTEENTH, 0.0, 0.0, 1.0, 1.0, 1.0))
}

pub fn fence_gate_closed(facing: Direction) -> VoxelShape {
    match facing {
        Direction::North | Direction::South => {
            VoxelShape::from_box(aabb(0.0, 0.0, 6.0 * SIXTEENTH, 1.0, 1.5, 10.0 * SIXTEENTH))
        }
        Direction::West | Direction::East => {
            VoxelShape::from_box(aabb(6.0 * SIXTEENTH, 0.0, 0.0, 10.0 * SIXTEENTH, 1.5, 1.0))
        }
    }
}

pub fn fence_gate_selection(facing: Direction) -> VoxelShape {
    match facing {
        Direction::North | Direction::South => {
            VoxelShape::from_box(aabb(0.0, 0.0, 6.0 * SIXTEENTH, 1.0, 1.0, 10.0 * SIXTEENTH))
        }
        Direction::West | Direction::East => {
            VoxelShape::from_box(aabb(6.0 * SIXTEENTH, 0.0, 0.0, 10.0 * SIXTEENTH, 1.0, 1.0))
        }
    }
}

pub fn sign_post() -> VoxelShape {
    VoxelShape::from_box(aabb(
        7.0 * SIXTEENTH,
        0.0,
        7.0 * SIXTEENTH,
        9.0 * SIXTEENTH,
        1.0,
        9.0 * SIXTEENTH,
    ))
}

pub fn wall_sign() -> VoxelShape {
    VoxelShape::from_box(aabb(0.0, 0.25, 0.0, 1.0, 0.75, 2.0 * SIXTEENTH))
}

/// Helper to test neighbor connectivity.
fn is_connectable(neighbor: BlockType, self_type: BlockType) -> bool {
    if neighbor == self_type {
        return true;
    }
    if neighbor.properties().is_solid {
        return true;
    }
    false
}

/// Computes local connection flags `(north, south, west, east)` for fences/walls/panes.
pub fn get_connections(
    self_type: BlockType,
    (x, y, z): (i32, i32, i32),
    chunk_manager: Option<&ChunkManager>,
) -> (bool, bool, bool, bool) {
    if let Some(cm) = chunk_manager {
        let n = is_connectable(cm.get_block(x, y, z - 1), self_type);
        let s = is_connectable(cm.get_block(x, y, z + 1), self_type);
        let w = is_connectable(cm.get_block(x - 1, y, z), self_type);
        let e = is_connectable(cm.get_block(x + 1, y, z), self_type);
        (n, s, w, e)
    } else {
        (true, true, true, true)
    }
}

// ---------------------------------------------------------------------------
// Main VoxelShape provider functions
// ---------------------------------------------------------------------------

/// Physical collision shape for entities/players.
pub fn block_collision_shape(
    block: BlockType,
    state_raw: u8,
    pos: (i32, i32, i32),
    chunk_manager: Option<&ChunkManager>,
) -> VoxelShape {
    let (fx, fy, fz) = (pos.0 as f32, pos.1 as f32, pos.2 as f32);

    let shape = match block {
        BlockType::Air
        | BlockType::Water
        | BlockType::Lava
        | BlockType::Fire
        | BlockType::TallGrass
        | BlockType::Dandelion
        | BlockType::Poppy
        | BlockType::SugarCane
        | BlockType::WheatCrop
        | BlockType::CarrotCrop
        | BlockType::PotatoCrop
        | BlockType::Torch
        | BlockType::RedstoneTorch
        | BlockType::RedstoneTorchOff
        | BlockType::OakSign => VoxelShape::EMPTY,

        BlockType::OakDoor | BlockType::OakDoorOpen => {
            let mut state = BlockState::decode(state_raw);
            if block == BlockType::OakDoorOpen {
                state.is_open = true;
            }
            const THICKNESS: f32 = 3.0 / 16.0;
            let (min_x, max_x, min_z, max_z) = if !state.is_open {
                match state.facing {
                    Direction::North => (0.0, 1.0, 0.0, THICKNESS),
                    Direction::South => (0.0, 1.0, 1.0 - THICKNESS, 1.0),
                    Direction::West => (0.0, THICKNESS, 0.0, 1.0),
                    Direction::East => (1.0 - THICKNESS, 1.0, 0.0, 1.0),
                }
            } else if !state.is_right_hinge {
                match state.facing {
                    Direction::North => (0.0, THICKNESS, 0.0, 1.0),
                    Direction::South => (1.0 - THICKNESS, 1.0, 0.0, 1.0),
                    Direction::West => (0.0, 1.0, 1.0 - THICKNESS, 1.0),
                    Direction::East => (0.0, 1.0, 0.0, THICKNESS),
                }
            } else {
                match state.facing {
                    Direction::North => (1.0 - THICKNESS, 1.0, 0.0, 1.0),
                    Direction::South => (0.0, THICKNESS, 0.0, 1.0),
                    Direction::West => (0.0, 1.0, 0.0, THICKNESS),
                    Direction::East => (0.0, 1.0, 1.0 - THICKNESS, 1.0),
                }
            };
            VoxelShape::from_box(aabb(min_x, 0.0, min_z, max_x, 1.0, max_z))
        }

        BlockType::OakTrapdoor | BlockType::OakTrapdoorOpen => {
            let mut state = BlockState::decode(state_raw);
            if block == BlockType::OakTrapdoorOpen {
                state.is_open = true;
            }
            const THICKNESS: f32 = 3.0 / 16.0;
            if state.is_open {
                let (min_x, max_x, min_z, max_z) = match state.facing {
                    Direction::North => (0.0, 1.0, 0.0, THICKNESS),
                    Direction::South => (0.0, 1.0, 1.0 - THICKNESS, 1.0),
                    Direction::West => (0.0, THICKNESS, 0.0, 1.0),
                    Direction::East => (1.0 - THICKNESS, 1.0, 0.0, 1.0),
                };
                VoxelShape::from_box(aabb(min_x, 0.0, min_z, max_x, 1.0, max_z))
            } else {
                let min_y = if state.is_top { 1.0 - THICKNESS } else { 0.0 };
                let max_y = if state.is_top { 1.0 } else { THICKNESS };
                VoxelShape::from_box(aabb(0.0, min_y, 0.0, 1.0, max_y, 1.0))
            }
        }

        BlockType::OakSlab | BlockType::CobblestoneSlab => {
            let state = BlockState::decode(state_raw);
            if state.is_top {
                slab_top()
            } else {
                slab_bottom()
            }
        }

        BlockType::OakStair | BlockType::CobblestoneStair => {
            let state = BlockState::decode(state_raw);
            if state.is_top {
                match state.facing {
                    Direction::North => stair_inverted_north(),
                    Direction::South => stair_inverted_south(),
                    Direction::West => stair_inverted_west(),
                    Direction::East => stair_inverted_east(),
                }
            } else {
                match state.facing {
                    Direction::North => stair_north(),
                    Direction::South => stair_south(),
                    Direction::West => stair_west(),
                    Direction::East => stair_east(),
                }
            }
        }

        BlockType::OakFence => {
            let (n, s, w, e) = get_connections(block, pos, chunk_manager);
            fence_shape_connected(n, s, w, e)
        }

        BlockType::OakFenceGate => {
            let state = BlockState::decode(state_raw);
            if state.is_open {
                VoxelShape::EMPTY
            } else {
                fence_gate_closed(state.facing)
            }
        }

        BlockType::CobblestoneWall => {
            let (n, s, w, e) = get_connections(block, pos, chunk_manager);
            wall_shape_connected(n, s, w, e)
        }

        BlockType::GlassPane => {
            let (n, s, w, e) = get_connections(block, pos, chunk_manager);
            pane_shape_connected(n, s, w, e)
        }

        BlockType::OakLadder => {
            let state = BlockState::decode(state_raw);
            match state.facing {
                Direction::North => ladder_north(),
                Direction::South => ladder_south(),
                Direction::West => ladder_west(),
                Direction::East => ladder_east(),
            }
        }

        BlockType::Cactus => VoxelShape::from_box(aabb(
            SIXTEENTH,
            0.0,
            SIXTEENTH,
            1.0 - SIXTEENTH,
            1.0,
            1.0 - SIXTEENTH,
        )),

        BlockType::Farmland => {
            VoxelShape::from_box(aabb(0.0, 0.0, 0.0, 1.0, 15.0 * SIXTEENTH, 1.0))
        }

        _ => {
            if block.properties().is_solid {
                VoxelShape::FULL_CUBE
            } else {
                VoxelShape::EMPTY
            }
        }
    };

    shape.translate(Vec3::new(fx, fy, fz))
}

/// Selection shape for DDA raycasting.
pub fn block_selection_shape(
    block: BlockType,
    state_raw: u8,
    pos: (i32, i32, i32),
    chunk_manager: Option<&ChunkManager>,
) -> VoxelShape {
    let (fx, fy, fz) = (pos.0 as f32, pos.1 as f32, pos.2 as f32);

    let shape = match block {
        BlockType::Air => VoxelShape::EMPTY,

        BlockType::OakFenceGate => {
            let state = BlockState::decode(state_raw);
            fence_gate_selection(state.facing)
        }

        BlockType::OakSign => {
            let state = BlockState::decode(state_raw);
            if state.is_top {
                sign_post()
            } else {
                wall_sign()
            }
        }

        BlockType::OakDoor
        | BlockType::OakDoorOpen
        | BlockType::OakTrapdoor
        | BlockType::OakTrapdoorOpen => VoxelShape::FULL_CUBE,

        BlockType::Torch | BlockType::RedstoneTorch | BlockType::RedstoneTorchOff => {
            VoxelShape::from_box(aabb(
                6.0 * SIXTEENTH,
                0.0,
                6.0 * SIXTEENTH,
                10.0 * SIXTEENTH,
                10.0 * SIXTEENTH,
                10.0 * SIXTEENTH,
            ))
        }

        BlockType::TallGrass
        | BlockType::Dandelion
        | BlockType::Poppy
        | BlockType::SugarCane
        | BlockType::WheatCrop
        | BlockType::CarrotCrop
        | BlockType::PotatoCrop => VoxelShape::from_box(aabb(
            2.0 * SIXTEENTH,
            0.0,
            2.0 * SIXTEENTH,
            14.0 * SIXTEENTH,
            14.0 * SIXTEENTH,
            14.0 * SIXTEENTH,
        )),

        _ => {
            let col = block_collision_shape(block, state_raw, (0, 0, 0), chunk_manager);
            if col.is_empty() {
                VoxelShape::FULL_CUBE
            } else {
                col
            }
        }
    };

    shape.translate(Vec3::new(fx, fy, fz))
}

/// Occlusion shape for face culling and line-of-sight calculation.
pub fn block_occlusion_shape(block: BlockType, _state_raw: u8, pos: (i32, i32, i32)) -> VoxelShape {
    let (fx, fy, fz) = (pos.0 as f32, pos.1 as f32, pos.2 as f32);

    let shape = if block.properties().render_type == crate::world::RenderType::Opaque
        && block.properties().is_solid
        && matches!(
            block,
            BlockType::Grass
                | BlockType::Dirt
                | BlockType::Stone
                | BlockType::Sand
                | BlockType::Gravel
                | BlockType::OakLog
                | BlockType::OakPlanks
                | BlockType::Cobblestone
                | BlockType::Bedrock
                | BlockType::CoalOre
                | BlockType::IronOre
                | BlockType::GoldOre
                | BlockType::DiamondOre
                | BlockType::RedstoneOre
                | BlockType::Brick
                | BlockType::StoneBrick
                | BlockType::Clay
                | BlockType::Sandstone
                | BlockType::Obsidian
                | BlockType::CraftingTable
                | BlockType::Furnace
                | BlockType::FurnaceLit
                | BlockType::Chest
                | BlockType::TNT
                | BlockType::Bookshelf
                | BlockType::BirchLog
                | BlockType::BirchPlanks
                | BlockType::SpruceLog
                | BlockType::SprucePlanks
                | BlockType::Netherrack
                | BlockType::SoulSand
                | BlockType::EndStone
                | BlockType::Purpur
        ) {
        VoxelShape::FULL_CUBE
    } else {
        VoxelShape::EMPTY
    };

    shape.translate(Vec3::new(fx, fy, fz))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_shape_has_no_boxes() {
        assert!(VoxelShape::EMPTY.is_empty());
        assert_eq!(VoxelShape::EMPTY.count, 0);
    }

    #[test]
    fn full_cube_is_full_occluder() {
        assert!(VoxelShape::FULL_CUBE.is_full_occluder());
    }

    #[test]
    fn slab_bottom_is_not_full_occluder() {
        assert!(!slab_bottom().is_full_occluder());
    }

    #[test]
    fn stair_north_has_two_boxes() {
        let s = stair_north();
        assert_eq!(s.count, 2);
    }

    #[test]
    fn fence_has_correct_box_count() {
        let f = fence_shape_connected(true, true, true, true);
        assert_eq!(f.count, 5);
    }

    #[test]
    fn translate_shifts_all_boxes() {
        let s = slab_bottom().translate(Vec3::new(8.0, 64.0, 8.0));
        for b in s.iter() {
            assert!(b.min.x >= 8.0);
            assert!(b.max.x <= 9.0);
            assert!(b.min.y >= 64.0);
            assert!(b.max.y <= 64.5);
        }
    }

    #[test]
    fn ray_intersects_full_cube() {
        let shape = VoxelShape::FULL_CUBE.translate(Vec3::new(10.0, 10.0, 10.0));
        let origin = Vec3::new(10.5, 10.5, 5.0);
        let dir = Vec3::new(0.0, 0.0, 1.0);
        let res = shape.ray_intersects(origin, dir, 10.0);
        assert!(res.is_some());
        let (t, norm) = res.unwrap();
        assert!((t - 5.0).abs() < 1e-4);
        assert_eq!(norm, Vec3::new(0.0, 0.0, -1.0));
    }

    #[test]
    fn ray_intersects_slab_bottom() {
        let shape = slab_bottom().translate(Vec3::new(0.0, 0.0, 0.0));
        let origin = Vec3::new(0.5, 2.0, 0.5);
        let dir = Vec3::new(0.0, -1.0, 0.0);
        let res = shape.ray_intersects(origin, dir, 10.0);
        assert!(res.is_some());
        let (t, _norm) = res.unwrap();
        assert!((t - 1.5).abs() < 1e-4);
    }

    #[test]
    fn ray_misses_empty_shape() {
        let shape = VoxelShape::EMPTY;
        let res = shape.ray_intersects(Vec3::ZERO, Vec3::X, 10.0);
        assert!(res.is_none());
    }

    #[test]
    fn stair_ray_hits_step_and_passes_gap() {
        let stair = stair_north().translate(Vec3::ZERO);
        // Ray aiming down into front gap (0.5..1.0 Z)
        let gap_hit =
            stair.ray_intersects(Vec3::new(0.5, 2.0, 0.75), Vec3::new(0.0, -1.0, 0.0), 10.0);
        assert!(gap_hit.is_some());
        let (t_gap, _) = gap_hit.unwrap();
        assert!((t_gap - 1.5).abs() < 1e-4); // hits bottom slab at y = 0.5

        // Ray aiming down into back step (0.0..0.5 Z)
        let step_hit =
            stair.ray_intersects(Vec3::new(0.5, 2.0, 0.25), Vec3::new(0.0, -1.0, 0.0), 10.0);
        assert!(step_hit.is_some());
        let (t_step, _) = step_hit.unwrap();
        assert!((t_step - 1.0).abs() < 1e-4); // hits top step at y = 1.0
    }

    #[test]
    fn non_full_blocks_have_empty_occlusion() {
        assert!(block_occlusion_shape(BlockType::OakSlab, 0, (0, 0, 0)).is_empty());
        assert!(block_occlusion_shape(BlockType::OakStair, 0, (0, 0, 0)).is_empty());
        assert!(block_occlusion_shape(BlockType::OakFence, 0, (0, 0, 0)).is_empty());
        assert!(block_occlusion_shape(BlockType::CobblestoneWall, 0, (0, 0, 0)).is_empty());
        assert!(block_occlusion_shape(BlockType::GlassPane, 0, (0, 0, 0)).is_empty());
    }
}
