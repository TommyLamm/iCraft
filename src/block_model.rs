//! BlockModel — model descriptors and mesh generation for non-full blocks.
//!
//! Instead of hard-coding a mesh-building branch for every special-shaped block in `world.rs`,
//! each non-full `BlockType` provides a model descriptor or element list that generates
//! vertices and indices with correct UVs, AO, lighting, and outward winding.

use crate::chunk_render::TerrainVertex;
use crate::redstone::Direction;
use crate::resources::ResourcePackManager;
use crate::world::{BlockState, BlockType, RenderType};
use serde::Deserialize;
use std::collections::HashMap;

const SIXTEENTH: f32 = 1.0 / 16.0;

/// Parsed, bounded subset of an item/block model descriptor.  The existing
/// procedural meshes remain the built-in fallback; a pack can override the
/// atlas tile without asking the world/state layer to understand JSON.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModelDescriptor {
    pub atlas_tile: Option<(u32, u32)>,
}

#[derive(Debug, Deserialize)]
struct RawModelDescriptor {
    #[serde(default)]
    atlas_tile: Option<[u32; 2]>,
}

/// Resource-backed block/item model registry used by mesh consumers that have
/// a selected `ResourcePackManager`.  A registry is deliberately immutable
/// after construction so background mesh jobs can share it safely.
#[derive(Debug, Clone, Default)]
pub struct ModelRegistry {
    descriptors: HashMap<String, ModelDescriptor>,
}

impl ModelRegistry {
    pub fn from_resource_packs<I, S>(manager: &mut ResourcePackManager, model_paths: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut registry = Self::default();
        for path in model_paths {
            let path = path.as_ref();
            // A complete registry probes every known block path.  Missing
            // descriptors are normal for packs that only override a subset
            // of blocks, so use the quiet read path and diagnose only assets
            // that are present but malformed/unsupported.
            let bytes = manager.read_asset(path);
            let parsed = bytes.as_deref().and_then(parse_model_descriptor);
            if bytes.is_some() && parsed.is_none() {
                manager.record_asset_diagnostic(
                    path,
                    "model",
                    "model descriptor is unsupported; using procedural model fallback",
                );
            }
            let descriptor = parsed.unwrap_or_default();
            if let Ok(path) = normalize_model_path(path) {
                registry.descriptors.insert(path, descriptor);
            }
        }
        registry
    }

    pub fn descriptor(&self, path: &str) -> Option<&ModelDescriptor> {
        let path = normalize_model_path(path).ok()?;
        self.descriptors.get(&path)
    }

    pub fn atlas_tile_for(&self, path: &str, fallback: (u32, u32)) -> (u32, u32) {
        self.descriptor(path)
            .and_then(|descriptor| descriptor.atlas_tile)
            .unwrap_or(fallback)
    }

    pub fn atlas_tile_for_block(&self, block: BlockType, fallback: (u32, u32)) -> (u32, u32) {
        self.atlas_tile_for(&model_path_for_block(block), fallback)
    }
}

/// Convert a PascalCase / SCREAMING ident into snake_case.
fn pascal_to_snake(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 4);
    let bytes = name.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        let c = b as char;
        if c.is_ascii_uppercase() && i > 0 {
            let prev_lower = (bytes[i - 1] as char).is_ascii_lowercase();
            let next_lower = bytes
                .get(i + 1)
                .map(|n| (*n as char).is_ascii_lowercase())
                .unwrap_or(false);
            if prev_lower || next_lower {
                out.push('_');
            }
        }
        out.push(c.to_ascii_lowercase());
    }
    out
}

/// Model path slug derived from the [`BlockType`] discriminant name.
///
/// `Grass` keeps the historical `grass_block` pack path; reserved wire holes
/// use `reservedN` so packs never need the deleted powered/open variant names.
fn model_slug_for_block(block: BlockType) -> String {
    match block {
        BlockType::Grass => "grass_block".to_string(),
        other => pascal_to_snake(&format!("{other:?}")),
    }
}

/// Canonical resource path for a block model descriptor.
///
/// Paths are derived from the [`BlockType`] name (`models/block/{snake}.json`)
/// so adding a variant does not require a parallel `MODEL_PATHS` table.
pub fn model_path_for_block(block: BlockType) -> String {
    format!("models/block/{}.json", model_slug_for_block(block))
}

/// Every canonical block model path, in [`BlockType`] discriminant order.
pub fn all_model_paths() -> impl Iterator<Item = String> {
    (0..=BlockType::Observer as u8).map(|id| {
        let block: BlockType = unsafe { std::mem::transmute(id) };
        model_path_for_block(block)
    })
}

fn normalize_model_path(path: &str) -> Result<String, ()> {
    let path = path.replace('\\', "/");
    if path.is_empty() || path.starts_with('/') || path.contains("..") || path.contains(':') {
        return Err(());
    }
    Ok(path)
}

fn parse_model_descriptor(bytes: &[u8]) -> Option<ModelDescriptor> {
    let raw = serde_json::from_slice::<RawModelDescriptor>(bytes).ok()?;
    let atlas_tile = raw.atlas_tile.map(|tile| (tile[0], tile[1]));
    if atlas_tile.is_some_and(|(x, y)| x >= 16 || y >= 16) {
        return None;
    }
    Some(ModelDescriptor { atlas_tile })
}

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

/// Shared axis-aligned box emitter for `TerrainVertex` meshes.
///
/// Face order: north, south, west, east, top, bottom. `skip_face` drops one
/// face by that index (used by waterlogged slab water caps).
pub fn emit_box(
    vertices: &mut Vec<TerrainVertex>,
    indices: &mut Vec<u32>,
    origin: [f32; 3],
    bounds: ([f32; 3], [f32; 3]),
    sky_light: u8,
    block_light: u8,
    atlas_tile: (u32, u32),
    region_coord: (i32, i32),
    skip_face: Option<usize>,
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

    for (face_index, (p0, p1, p2, p3)) in faces.into_iter().enumerate() {
        if skip_face == Some(face_index) {
            continue;
        }
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
    emit_box(
        vertices,
        indices,
        origin,
        bounds,
        sky_light,
        block_light,
        atlas_tile,
        region_coord,
        None,
    );
}

fn append_box_without_face(
    vertices: &mut Vec<TerrainVertex>,
    indices: &mut Vec<u32>,
    origin: [f32; 3],
    bounds: ([f32; 3], [f32; 3]),
    sky_light: u8,
    block_light: u8,
    atlas_tile: (u32, u32),
    region_coord: (i32, i32),
    skip_face: usize,
) {
    emit_box(
        vertices,
        indices,
        origin,
        bounds,
        sky_light,
        block_light,
        atlas_tile,
        region_coord,
        Some(skip_face),
    );
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
    append_custom_block_mesh_impl(
        block,
        state_raw,
        origin,
        sky_light,
        block_light,
        region_coord,
        opaque_vertices,
        opaque_indices,
        trans_vertices,
        trans_indices,
        None,
        get_neighbor,
    )
}

/// Render a non-full block while applying the descriptor selected by a
/// resource-pack model registry.  Callers that do not have a registry should
/// use `append_custom_block_mesh`, which retains the procedural tile mapping.
pub fn append_custom_block_mesh_with_registry<F>(
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
    model_path: &str,
    registry: &ModelRegistry,
    get_neighbor: F,
) -> bool
where
    F: Fn(i32, i32, i32) -> BlockType,
{
    let fallback = block.get_face_tex_index(0);
    let tile = registry.atlas_tile_for(model_path, fallback);
    append_custom_block_mesh_impl(
        block,
        state_raw,
        origin,
        sky_light,
        block_light,
        region_coord,
        opaque_vertices,
        opaque_indices,
        trans_vertices,
        trans_indices,
        Some(tile),
        get_neighbor,
    )
}

/// Append the translucent fluid volume that occupies the complementary half
/// of a waterlogged slab.  The host slab remains an opaque/cutout solid; this
/// helper only contributes the water overlay to the translucent mesh lane.
pub fn append_waterlogged_slab_mesh(
    block: BlockType,
    state_raw: u8,
    origin: [f32; 3],
    sky_light: u8,
    block_light: u8,
    region_coord: (i32, i32),
    trans_vertices: &mut Vec<TerrainVertex>,
    trans_indices: &mut Vec<u32>,
    atlas_tile_override: Option<(u32, u32)>,
) {
    if !block.is_waterloggable() {
        return;
    }
    let state = BlockState::decode(state_raw);
    let min_y = if state.is_top { 0.0 } else { 0.5 };
    let max_y = if state.is_top { 0.5 } else { 1.0 };
    let tile = atlas_tile_override.unwrap_or_else(|| block.get_face_tex_index(0));
    append_box_without_face(
        trans_vertices,
        trans_indices,
        origin,
        ([0.0, min_y, 0.0], [1.0, max_y, 1.0]),
        sky_light,
        block_light,
        tile,
        region_coord,
        if state.is_top { 4 } else { 5 },
    );
}

fn append_custom_block_mesh_impl<F>(
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
    atlas_tile_override: Option<(u32, u32)>,
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

    let tile = atlas_tile_override.unwrap_or_else(|| block.get_face_tex_index(0));

    match block {
        BlockType::Chest | BlockType::EndCityChest => {
            let bs = BlockState::decode(state_raw);
            append_box(
                target_v,
                target_i,
                origin,
                (
                    [2.0 * SIXTEENTH, 0.0, 2.0 * SIXTEENTH],
                    [14.0 * SIXTEENTH, 14.0 * SIXTEENTH, 14.0 * SIXTEENTH],
                ),
                sky_light,
                block_light,
                tile,
                region_coord,
            );
            let lid_bounds = if !bs.is_open {
                (
                    [SIXTEENTH, 14.0 * SIXTEENTH, SIXTEENTH],
                    [15.0 * SIXTEENTH, 1.0, 15.0 * SIXTEENTH],
                )
            } else {
                match bs.facing {
                    Direction::North => (
                        [SIXTEENTH, 14.0 * SIXTEENTH, 0.0],
                        [15.0 * SIXTEENTH, 1.0, 14.0 * SIXTEENTH],
                    ),
                    Direction::South => (
                        [SIXTEENTH, 14.0 * SIXTEENTH, 2.0 * SIXTEENTH],
                        [15.0 * SIXTEENTH, 1.0, 1.0],
                    ),
                    Direction::West => (
                        [0.0, 14.0 * SIXTEENTH, SIXTEENTH],
                        [14.0 * SIXTEENTH, 1.0, 15.0 * SIXTEENTH],
                    ),
                    Direction::East => (
                        [2.0 * SIXTEENTH, 14.0 * SIXTEENTH, SIXTEENTH],
                        [1.0, 1.0, 15.0 * SIXTEENTH],
                    ),
                    // Chests are horizontal containers; malformed vertical
                    // facings use the stable closed-like North geometry.
                    Direction::Up | Direction::Down => (
                        [SIXTEENTH, 14.0 * SIXTEENTH, 0.0],
                        [15.0 * SIXTEENTH, 1.0, 14.0 * SIXTEENTH],
                    ),
                }
            };
            append_box(
                target_v,
                target_i,
                origin,
                lid_bounds,
                sky_light,
                block_light,
                tile,
                region_coord,
            );
            true
        }

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
                _ => ([0.5, 0.0, 0.0], [1.0, 0.5, 1.0]),
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
                    _ => {
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
                    _ => {
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
                _ => ([14.0 * SIXTEENTH, 0.0, 0.0], [1.0, 1.0, 1.0]),
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

        // Hopper geometry is deliberately kept as two bounded boxes here so
        // it follows the same CPU mesh/cache path as the collision shape. A
        // hopper is not a greedy cube; returning `false` would make a placed
        // automation block invisible even though its gameplay state exists.
        BlockType::Hopper => {
            append_box(
                target_v,
                target_i,
                origin,
                ([0.0, 10.0 * SIXTEENTH, 0.0], [1.0, 1.0, 1.0]),
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
                    [6.0 * SIXTEENTH, 0.0, 6.0 * SIXTEENTH],
                    [10.0 * SIXTEENTH, 10.0 * SIXTEENTH, 10.0 * SIXTEENTH],
                ),
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
                    _ => (
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("icraft_model_{label}_{stamp}_{id}"));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn selected_model_descriptor_reaches_mesh_consumer() {
        let root = temp_dir("selected");
        fs::write(
            root.join("pack.json"),
            br#"{"id":"icraft.builtin","name":"builtin","version":"1","format":1,"description":""}"#,
        )
        .unwrap();
        let user = root.join("resourcepacks");
        let pack = user.join("models");
        fs::create_dir_all(pack.join("models")).unwrap();
        fs::write(
            pack.join("pack.json"),
            br#"{"id":"models.pack","name":"models","version":"1","format":1,"description":""}"#,
        )
        .unwrap();
        fs::write(
            pack.join("models/block.json"),
            br#"{"parent":"builtin","atlas_tile":[15,14]}"#,
        )
        .unwrap();
        let mut manager = ResourcePackManager::discover(&root, &user);
        manager.apply_enabled_order(["models.pack"]).unwrap();
        let registry = ModelRegistry::from_resource_packs(&mut manager, ["models/block.json"]);
        assert_eq!(
            registry.descriptor("models/block.json").unwrap().atlas_tile,
            Some((15, 14))
        );

        let mut opaque_vertices = Vec::new();
        let mut opaque_indices = Vec::new();
        let mut trans_vertices = Vec::new();
        let mut trans_indices = Vec::new();
        assert!(append_custom_block_mesh_with_registry(
            BlockType::OakSlab,
            0,
            [0.0, 0.0, 0.0],
            15,
            0,
            (0, 0),
            &mut opaque_vertices,
            &mut opaque_indices,
            &mut trans_vertices,
            &mut trans_indices,
            "models/block.json",
            &registry,
            |_, _, _| BlockType::Air,
        ));
        assert_eq!(opaque_vertices[0].atlas_tile, [15, 14]);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn chest_mesh_has_distinct_binary_open_and_closed_geometry() {
        let mut closed_vertices = Vec::new();
        let mut closed_indices = Vec::new();
        let mut open_vertices = Vec::new();
        let mut open_indices = Vec::new();
        let closed = BlockState::default().encode();
        let mut opened = BlockState::default();
        opened.is_open = true;
        assert!(append_custom_block_mesh(
            BlockType::Chest,
            closed,
            [0.0, 0.0, 0.0],
            15,
            0,
            (0, 0),
            &mut closed_vertices,
            &mut closed_indices,
            &mut Vec::new(),
            &mut Vec::new(),
            |_, _, _| BlockType::Air,
        ));
        assert!(append_custom_block_mesh(
            BlockType::Chest,
            opened.encode(),
            [0.0, 0.0, 0.0],
            15,
            0,
            (0, 0),
            &mut open_vertices,
            &mut open_indices,
            &mut Vec::new(),
            &mut Vec::new(),
            |_, _, _| BlockType::Air,
        ));
        assert_eq!(closed_indices.len(), open_indices.len());
        assert!(!closed_vertices.is_empty());
        assert_ne!(closed_vertices, open_vertices);
    }

    #[test]
    fn waterlogged_slab_adds_only_the_translucent_complement() {
        let mut opaque_vertices = Vec::new();
        let mut opaque_indices = Vec::new();
        let mut trans_vertices = Vec::new();
        let mut trans_indices = Vec::new();
        let state = BlockState::default().encode();
        assert!(append_custom_block_mesh(
            BlockType::OakSlab,
            state,
            [0.0, 0.0, 0.0],
            15,
            0,
            (0, 0),
            &mut opaque_vertices,
            &mut opaque_indices,
            &mut trans_vertices,
            &mut trans_indices,
            |_, _, _| BlockType::Air,
        ));
        let opaque_len = opaque_vertices.len();
        append_waterlogged_slab_mesh(
            BlockType::OakSlab,
            state,
            [0.0, 0.0, 0.0],
            15,
            0,
            (0, 0),
            &mut trans_vertices,
            &mut trans_indices,
            Some((3, 4)),
        );
        assert_eq!(opaque_vertices.len(), opaque_len);
        assert_eq!(trans_vertices.len(), 20);
        assert_eq!(trans_indices.len(), 30);
        assert!(trans_vertices
            .iter()
            .all(|vertex| vertex.atlas_tile == [3, 4]));
        // Packed `pos.y` is relative to `REGION_ORIGIN_Y`; compare decoded local Y.
        assert!(trans_vertices
            .iter()
            .all(|vertex| vertex.local_position()[1] >= 0.5));
        assert!(trans_vertices
            .iter()
            .all(|vertex| vertex.local_position()[1] <= 1.0));
        let has_horizontal_quad = |vertices: &[TerrainVertex], indices: &[u32], y: f32| {
            indices.chunks_exact(6).any(|quad| {
                quad.iter().all(|index| {
                    (vertices[*index as usize].local_position()[1] - y).abs() < 1e-3
                })
            })
        };
        assert!(!has_horizontal_quad(&trans_vertices, &trans_indices, 0.5));
        assert!(has_horizontal_quad(&trans_vertices, &trans_indices, 1.0));

        let mut top_vertices = Vec::new();
        let mut top_indices = Vec::new();
        let top_state = BlockState {
            is_top: true,
            ..BlockState::default()
        }
        .encode();
        append_waterlogged_slab_mesh(
            BlockType::OakSlab,
            top_state,
            [0.0, 0.0, 0.0],
            15,
            0,
            (0, 0),
            &mut top_vertices,
            &mut top_indices,
            Some((3, 4)),
        );
        assert_eq!(top_vertices.len(), 20);
        assert_eq!(top_indices.len(), 30);
        assert!(top_vertices
            .iter()
            .all(|vertex| vertex.local_position()[1] <= 0.5));
        assert!(!has_horizontal_quad(&top_vertices, &top_indices, 0.5));
        assert!(has_horizontal_quad(&top_vertices, &top_indices, 0.0));

        let before = trans_vertices.len();
        append_waterlogged_slab_mesh(
            BlockType::Stone,
            state,
            [0.0, 0.0, 0.0],
            15,
            0,
            (0, 0),
            &mut trans_vertices,
            &mut trans_indices,
            None,
        );
        assert_eq!(trans_vertices.len(), before);
    }

    #[test]
    fn canonical_block_model_paths_cover_every_wire_variant() {
        let paths: Vec<_> = all_model_paths().collect();
        assert_eq!(paths.len(), BlockType::Observer as usize + 1);
        assert_eq!(
            model_path_for_block(BlockType::Stone),
            "models/block/stone.json"
        );
        assert_eq!(
            model_path_for_block(BlockType::Grass),
            "models/block/grass_block.json"
        );
        assert_eq!(
            model_path_for_block(BlockType::OakSlab),
            "models/block/oak_slab.json"
        );
        assert_eq!(
            model_path_for_block(BlockType::TNT),
            "models/block/tnt.json"
        );
        assert_eq!(
            model_path_for_block(BlockType::Reserved50),
            "models/block/reserved50.json"
        );
        assert_eq!(
            model_path_for_block(BlockType::Observer),
            "models/block/observer.json"
        );
        for (id, path) in paths.iter().enumerate() {
            let block: BlockType = unsafe { std::mem::transmute(id as u8) };
            assert_eq!(path, &model_path_for_block(block));
            assert!(path.starts_with("models/block/") && path.ends_with(".json"));
            let slug = path
                .trim_start_matches("models/block/")
                .trim_end_matches(".json");
            assert!(!slug.is_empty());
            assert!(slug
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'));
        }
        assert!(paths.windows(2).all(|pair| pair[0] != pair[1]));
    }

    #[test]
    fn missing_descriptors_are_quiet_when_building_complete_registry() {
        let root = temp_dir("missing");
        fs::write(
            root.join("pack.json"),
            br#"{"id":"icraft.builtin","name":"builtin","version":"1","format":1,"description":""}"#,
        )
        .unwrap();
        let user = root.join("resourcepacks");
        fs::create_dir_all(&user).unwrap();
        let mut manager = ResourcePackManager::discover(&root, &user);
        let before = manager.diagnostics().len();
        let registry = ModelRegistry::from_resource_packs(&mut manager, all_model_paths());
        assert_eq!(
            registry
                .descriptor(&model_path_for_block(BlockType::Stone))
                .and_then(|descriptor| descriptor.atlas_tile),
            None
        );
        assert_eq!(manager.diagnostics().len(), before);
        let _ = fs::remove_dir_all(root);
    }
}
