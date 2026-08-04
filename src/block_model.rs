//! BlockModel — model descriptors and mesh generation for non-full blocks.
//!
//! Instead of hard-coding a mesh-building branch for every special-shaped block in `world.rs`,
//! each non-full `BlockType` provides a model descriptor or element list that generates
//! vertices and indices with correct UVs, AO, lighting, and outward winding.

use crate::chunk_render::TerrainVertex;
use crate::redstone::Direction;
use crate::world::{BlockState, BlockType, RenderType};

const SIXTEENTH: f32 = 1.0 / 16.0;

fn push_quad(
    vertices: &mut Vec<TerrainVertex>,
    indices: &mut Vec<u32>,
    positions: [[f32; 3]; 4],
    local_uvs: [[f32; 2]; 4],
    atlas_tile: (u32, u32),
    light_level: f32,
    region_coord: (i32, i32),
) {
    let start = vertices.len() as u32;
    for corner in 0..4 {
        vertices.push(TerrainVertex::new(
            positions[corner],
            local_uvs[corner],
            [atlas_tile.0 as f32, atlas_tile.1 as f32],
            light_level,
            1.0,
            region_coord,
        ));
    }
    indices.extend_from_slice(&[start, start + 1, start + 2, start + 2, start + 3, start]);
}

fn append_box(
    vertices: &mut Vec<TerrainVertex>,
    indices: &mut Vec<u32>,
    origin: [f32; 3],
    bounds: ([f32; 3], [f32; 3]),
    sky_light: u8,
    block_light: u8,
    atlas_tile: (u32, u32),
    region_coord: (i32, i32),
) {
    let light_level = sky_light as f32 + block_light as f32 * 16.0;
    let (min, max) = bounds;

    let faces = [
        // North face (z = min.z)
        (
            [min[0], min[1], min[2]],
            [max[0], min[1], min[2]],
            [max[0], max[1], min[2]],
            [min[0], max[1], min[2]],
        ),
        // South face (z = max.z)
        (
            [max[0], min[1], max[2]],
            [min[0], min[1], max[2]],
            [min[0], max[1], max[2]],
            [max[0], max[1], max[2]],
        ),
        // West face (x = min.x)
        (
            [min[0], min[1], max[2]],
            [min[0], min[1], min[2]],
            [min[0], max[1], min[2]],
            [min[0], max[1], max[2]],
        ),
        // East face (x = max.x)
        (
            [max[0], min[1], min[2]],
            [max[0], min[1], max[2]],
            [max[0], max[1], max[2]],
            [max[0], max[1], min[2]],
        ),
        // Top face (y = max.y)
        (
            [min[0], max[1], min[2]],
            [max[0], max[1], min[2]],
            [max[0], max[1], max[2]],
            [min[0], max[1], max[2]],
        ),
        // Bottom face (y = min.y)
        (
            [min[0], min[1], max[2]],
            [max[0], min[1], max[2]],
            [max[0], min[1], min[2]],
            [min[0], min[1], min[2]],
        ),
    ];

    let uvs = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];

    for (p0, p1, p2, p3) in faces {
        let positions = [
            [origin[0] + p0[0], origin[1] + p0[1], origin[2] + p0[2]],
            [origin[0] + p1[0], origin[1] + p1[1], origin[2] + p1[2]],
            [origin[0] + p2[0], origin[1] + p2[1], origin[2] + p2[2]],
            [origin[0] + p3[0], origin[1] + p3[1], origin[2] + p3[2]],
        ];
        push_quad(
            vertices,
            indices,
            positions,
            uvs,
            atlas_tile,
            light_level,
            region_coord,
        );
    }
}

fn is_connectable(neighbor: BlockType, self_type: BlockType) -> bool {
    neighbor == self_type || neighbor.properties().is_solid
}

/// Renders non-full blocks. Returns `true` if the block mesh was generated.
pub fn append_custom_block_mesh<F>(
    block: BlockType,
    state_raw: u8,
    origin: [f32; 3],
    sky_light: u8,
    block_light: u8,
    region_coord: (i32, i32),
    opaque_vertices: &mut Vec<TerrainVertex>,
    opaque_indices: &mut Vec<u32>,
    trans_vertices: &mut Vec<TerrainVertex>,
    trans_indices: &mut Vec<u32>,
    get_neighbor: F,
) -> bool
where
    F: Fn(i32, i32, i32) -> BlockType,
{
    let wx = origin[0] as i32;
    let wy = origin[1] as i32;
    let wz = origin[2] as i32;

    let is_cutout = block.properties().render_type == RenderType::Cutout;
    let (target_v, target_i) = if is_cutout {
        (trans_vertices, trans_indices)
    } else {
        (opaque_vertices, opaque_indices)
    };

    let tile = block.get_face_tex_index(0);

    match block {
        BlockType::OakSlab | BlockType::CobblestoneSlab => {
            let bs = BlockState::decode(state_raw);
            let (min_y, max_y) = if bs.is_top { (0.5, 1.0) } else { (0.0, 0.5) };
            append_box(
                target_v,
                target_i,
                origin,
                ([0.0, min_y, 0.0], [1.0, max_y, 1.0]),
                sky_light,
                block_light,
                tile,
                region_coord,
            );
            true
        }

        BlockType::OakStair | BlockType::CobblestoneStair => {
            let bs = BlockState::decode(state_raw);
            let (base_min_y, base_max_y) = if bs.is_top { (0.5, 1.0) } else { (0.0, 0.5) };
            append_box(
                target_v,
                target_i,
                origin,
                ([0.0, base_min_y, 0.0], [1.0, base_max_y, 1.0]),
                sky_light,
                block_light,
                tile,
                region_coord,
            );

            let step_bounds = match (bs.facing, bs.is_top) {
                (Direction::North, false) => ([0.0, 0.5, 0.0], [1.0, 1.0, 0.5]),
                (Direction::South, false) => ([0.0, 0.5, 0.5], [1.0, 1.0, 1.0]),
                (Direction::West, false) => ([0.0, 0.5, 0.0], [0.5, 1.0, 1.0]),
                (Direction::East, false) => ([0.5, 0.5, 0.0], [1.0, 1.0, 1.0]),
                (Direction::North, true) => ([0.0, 0.0, 0.0], [1.0, 0.5, 0.5]),
                (Direction::South, true) => ([0.0, 0.0, 0.5], [1.0, 0.5, 1.0]),
                (Direction::West, true) => ([0.0, 0.0, 0.0], [0.5, 0.5, 1.0]),
                (Direction::East, true) => ([0.5, 0.0, 0.0], [1.0, 0.5, 1.0]),
            };
            append_box(
                target_v,
                target_i,
                origin,
                step_bounds,
                sky_light,
                block_light,
                tile,
                region_coord,
            );
            true
        }

        BlockType::OakFence => {
            let p = 6.0 * SIXTEENTH;
            let q = 10.0 * SIXTEENTH;
            // Post
            append_box(
                target_v,
                target_i,
                origin,
                ([p, 0.0, p], [q, 1.0, q]),
                sky_light,
                block_light,
                tile,
                region_coord,
            );

            let n = is_connectable(get_neighbor(wx, wy, wz - 1), block);
            let s = is_connectable(get_neighbor(wx, wy, wz + 1), block);
            let w = is_connectable(get_neighbor(wx - 1, wy, wz), block);
            let e = is_connectable(get_neighbor(wx + 1, wy, wz), block);

            let arm_y1 = 6.0 * SIXTEENTH;
            let arm_y2 = 10.0 * SIXTEENTH;
            if n {
                append_box(
                    target_v,
                    target_i,
                    origin,
                    ([p, arm_y1, 0.0], [q, arm_y2, p]),
                    sky_light,
                    block_light,
                    tile,
                    region_coord,
                );
            }
            if s {
                append_box(
                    target_v,
                    target_i,
                    origin,
                    ([p, arm_y1, q], [q, arm_y2, 1.0]),
                    sky_light,
                    block_light,
                    tile,
                    region_coord,
                );
            }
            if w {
                append_box(
                    target_v,
                    target_i,
                    origin,
                    ([0.0, arm_y1, p], [p, arm_y2, q]),
                    sky_light,
                    block_light,
                    tile,
                    region_coord,
                );
            }
            if e {
                append_box(
                    target_v,
                    target_i,
                    origin,
                    ([q, arm_y1, p], [1.0, arm_y2, q]),
                    sky_light,
                    block_light,
                    tile,
                    region_coord,
                );
            }
            true
        }

        BlockType::OakFenceGate => {
            let bs = BlockState::decode(state_raw);
            if !bs.is_open {
                match bs.facing {
                    Direction::North | Direction::South => {
                        append_box(
                            target_v,
                            target_i,
                            origin,
                            ([0.0, 0.0, 6.0 * SIXTEENTH], [1.0, 1.0, 10.0 * SIXTEENTH]),
                            sky_light,
                            block_light,
                            tile,
                            region_coord,
                        );
                    }
                    Direction::West | Direction::East => {
                        append_box(
                            target_v,
                            target_i,
                            origin,
                            ([6.0 * SIXTEENTH, 0.0, 0.0], [10.0 * SIXTEENTH, 1.0, 1.0]),
                            sky_light,
                            block_light,
                            tile,
                            region_coord,
                        );
                    }
                }
            } else {
                // Open gate side posts
                match bs.facing {
                    Direction::North | Direction::South => {
                        append_box(
                            target_v,
                            target_i,
                            origin,
                            (
                                [0.0, 0.0, 6.0 * SIXTEENTH],
                                [2.0 * SIXTEENTH, 1.0, 10.0 * SIXTEENTH],
                            ),
                            sky_light,
                            block_light,
                            tile,
                            region_coord,
                        );
                        append_box(
                            target_v,
                            target_i,
                            origin,
                            (
                                [14.0 * SIXTEENTH, 0.0, 6.0 * SIXTEENTH],
                                [1.0, 1.0, 10.0 * SIXTEENTH],
                            ),
                            sky_light,
                            block_light,
                            tile,
                            region_coord,
                        );
                    }
                    Direction::West | Direction::East => {
                        append_box(
                            target_v,
                            target_i,
                            origin,
                            (
                                [6.0 * SIXTEENTH, 0.0, 0.0],
                                [10.0 * SIXTEENTH, 1.0, 2.0 * SIXTEENTH],
                            ),
                            sky_light,
                            block_light,
                            tile,
                            region_coord,
                        );
                        append_box(
                            target_v,
                            target_i,
                            origin,
                            (
                                [6.0 * SIXTEENTH, 0.0, 14.0 * SIXTEENTH],
                                [10.0 * SIXTEENTH, 1.0, 1.0],
                            ),
                            sky_light,
                            block_light,
                            tile,
                            region_coord,
                        );
                    }
                }
            }
            true
        }

        BlockType::CobblestoneWall => {
            let p = 4.0 * SIXTEENTH;
            let q = 12.0 * SIXTEENTH;
            append_box(
                target_v,
                target_i,
                origin,
                ([p, 0.0, p], [q, 1.0, q]),
                sky_light,
                block_light,
                tile,
                region_coord,
            );

            let n = is_connectable(get_neighbor(wx, wy, wz - 1), block);
            let s = is_connectable(get_neighbor(wx, wy, wz + 1), block);
            let w = is_connectable(get_neighbor(wx - 1, wy, wz), block);
            let e = is_connectable(get_neighbor(wx + 1, wy, wz), block);

            if n {
                append_box(
                    target_v,
                    target_i,
                    origin,
                    ([p, p, 0.0], [q, q, p]),
                    sky_light,
                    block_light,
                    tile,
                    region_coord,
                );
            }
            if s {
                append_box(
                    target_v,
                    target_i,
                    origin,
                    ([p, p, q], [q, q, 1.0]),
                    sky_light,
                    block_light,
                    tile,
                    region_coord,
                );
            }
            if w {
                append_box(
                    target_v,
                    target_i,
                    origin,
                    ([0.0, p, p], [p, q, q]),
                    sky_light,
                    block_light,
                    tile,
                    region_coord,
                );
            }
            if e {
                append_box(
                    target_v,
                    target_i,
                    origin,
                    ([q, p, p], [1.0, q, q]),
                    sky_light,
                    block_light,
                    tile,
                    region_coord,
                );
            }
            true
        }

        BlockType::GlassPane => {
            let p = 7.0 * SIXTEENTH;
            let q = 9.0 * SIXTEENTH;
            append_box(
                target_v,
                target_i,
                origin,
                ([p, 0.0, p], [q, 1.0, q]),
                sky_light,
                block_light,
                tile,
                region_coord,
            );

            let n = is_connectable(get_neighbor(wx, wy, wz - 1), block);
            let s = is_connectable(get_neighbor(wx, wy, wz + 1), block);
            let w = is_connectable(get_neighbor(wx - 1, wy, wz), block);
            let e = is_connectable(get_neighbor(wx + 1, wy, wz), block);

            if n {
                append_box(
                    target_v,
                    target_i,
                    origin,
                    ([p, 0.0, 0.0], [q, 1.0, p]),
                    sky_light,
                    block_light,
                    tile,
                    region_coord,
                );
            }
            if s {
                append_box(
                    target_v,
                    target_i,
                    origin,
                    ([p, 0.0, q], [q, 1.0, 1.0]),
                    sky_light,
                    block_light,
                    tile,
                    region_coord,
                );
            }
            if w {
                append_box(
                    target_v,
                    target_i,
                    origin,
                    ([0.0, 0.0, p], [p, 1.0, q]),
                    sky_light,
                    block_light,
                    tile,
                    region_coord,
                );
            }
            if e {
                append_box(
                    target_v,
                    target_i,
                    origin,
                    ([q, 0.0, p], [1.0, 1.0, q]),
                    sky_light,
                    block_light,
                    tile,
                    region_coord,
                );
            }
            true
        }

        BlockType::OakLadder => {
            let bs = BlockState::decode(state_raw);
            let bounds = match bs.facing {
                Direction::North => ([0.0, 0.0, 0.0], [1.0, 1.0, 2.0 * SIXTEENTH]),
                Direction::South => ([0.0, 0.0, 14.0 * SIXTEENTH], [1.0, 1.0, 1.0]),
                Direction::West => ([0.0, 0.0, 0.0], [2.0 * SIXTEENTH, 1.0, 1.0]),
                Direction::East => ([14.0 * SIXTEENTH, 0.0, 0.0], [1.0, 1.0, 1.0]),
            };
            append_box(
                target_v,
                target_i,
                origin,
                bounds,
                sky_light,
                block_light,
                tile,
                region_coord,
            );
            true
        }

        BlockType::OakSign => {
            let bs = BlockState::decode(state_raw);
            if bs.is_top {
                // Post
                append_box(
                    target_v,
                    target_i,
                    origin,
                    (
                        [7.0 * SIXTEENTH, 0.0, 7.0 * SIXTEENTH],
                        [9.0 * SIXTEENTH, 10.0 * SIXTEENTH, 9.0 * SIXTEENTH],
                    ),
                    sky_light,
                    block_light,
                    tile,
                    region_coord,
                );
                // Board
                append_box(
                    target_v,
                    target_i,
                    origin,
                    (
                        [0.0, 10.0 * SIXTEENTH, 6.0 * SIXTEENTH],
                        [1.0, 1.0, 10.0 * SIXTEENTH],
                    ),
                    sky_light,
                    block_light,
                    tile,
                    region_coord,
                );
            } else {
                let bounds = match bs.facing {
                    Direction::North => (
                        [0.0, 4.0 * SIXTEENTH, 0.0],
                        [1.0, 12.0 * SIXTEENTH, 2.0 * SIXTEENTH],
                    ),
                    Direction::South => (
                        [0.0, 4.0 * SIXTEENTH, 14.0 * SIXTEENTH],
                        [1.0, 12.0 * SIXTEENTH, 1.0],
                    ),
                    Direction::West => (
                        [0.0, 4.0 * SIXTEENTH, 0.0],
                        [2.0 * SIXTEENTH, 12.0 * SIXTEENTH, 1.0],
                    ),
                    Direction::East => (
                        [14.0 * SIXTEENTH, 4.0 * SIXTEENTH, 0.0],
                        [1.0, 12.0 * SIXTEENTH, 1.0],
                    ),
                };
                append_box(
                    target_v,
                    target_i,
                    origin,
                    bounds,
                    sky_light,
                    block_light,
                    tile,
                    region_coord,
                );
            }
            true
        }

        _ => false,
    }
}
