//! Pure CPU-side data and visibility helpers for chunk rendering.
//!
//! This module deliberately does not depend on `wgpu`. GPU buffer layouts and
//! uploads belong to the renderer, while the data below can also be produced by
//! background mesh workers and covered by ordinary unit tests.

use glam::{Mat4, Vec3, Vec4};
use std::collections::{BTreeMap, HashSet};

/// Vertex format used by terrain meshes.
///
/// `local_uv` is measured in block-texture repeats. `atlas_tile` is the
/// zero-based tile coordinate in the atlas, not an already-normalized UV. This
/// separation lets a greedy quad repeat a tile in the shader instead of
/// stretching one copy across the whole merged face.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct TerrainVertex {
    /// Region-relative position in 1/32th block units.
    pub pos: [u16; 3],
    /// Low byte: sky_light | (block_light << 4). Mid byte: face shading multiplier. High bits: discrete AO (0..3).
    pub light_ao: u16,
    /// Local tile UV scaled by 2048.0.
    pub local_uv: [u16; 2],
    /// Atlas tile coordinate (x, y).
    pub atlas_tile: [u16; 2],
}

impl TerrainVertex {
    pub fn new(
        position: [f32; 3],
        local_uv: [f32; 2],
        atlas_tile: [f32; 2],
        light_level: f32,
        ao: f32,
        region_coord: (i32, i32),
    ) -> Self {
        let reg_origin_x = (region_coord.0 * REGION_SIZE_CHUNKS * 16) as f32;
        let reg_origin_z = (region_coord.1 * REGION_SIZE_CHUNKS * 16) as f32;
        let rel_x = (position[0] - reg_origin_x).max(0.0);
        let rel_y = position[1].max(0.0);
        let rel_z = (position[2] - reg_origin_z).max(0.0);

        let px = (rel_x * 32.0).round() as u16;
        let py = (rel_y * 32.0).round() as u16;
        let pz = (rel_z * 32.0).round() as u16;

        let light_u32 = light_level as u32;
        let sky_light = (light_u32 & 0x0F) as u16;
        let block_light = ((light_u32 >> 4) & 0x0F) as u16;
        let multiplier_code = ((light_u32 >> 8) & 0x3F) as u16;
        let ao_idx = if ao >= 0.875 {
            3u16
        } else if ao >= 0.625 {
            2u16
        } else if ao >= 0.375 {
            1u16
        } else {
            0u16
        };

        let packed_light = sky_light | (block_light << 4);
        let light_ao = packed_light | (multiplier_code << 8) | (ao_idx << 14);

        let u = (local_uv[0] * 2048.0).round() as u16;
        let v = (local_uv[1] * 2048.0).round() as u16;

        let tx = atlas_tile[0].round() as u16;
        let ty = atlas_tile[1].round() as u16;

        Self {
            pos: [px, py, pz],
            light_ao,
            local_uv: [u, v],
            atlas_tile: [tx, ty],
        }
    }

    pub fn world_position(&self, region_coord: (i32, i32)) -> Vec3 {
        let reg_origin_x = (region_coord.0 * REGION_SIZE_CHUNKS * 16) as f32;
        let reg_origin_z = (region_coord.1 * REGION_SIZE_CHUNKS * 16) as f32;
        let x = self.pos[0] as f32 / 32.0 + reg_origin_x;
        let y = self.pos[1] as f32 / 32.0;
        let z = self.pos[2] as f32 / 32.0 + reg_origin_z;
        Vec3::new(x, y, z)
    }

    pub fn local_position(&self) -> [f32; 3] {
        [
            self.pos[0] as f32 / 32.0,
            self.pos[1] as f32 / 32.0,
            self.pos[2] as f32 / 32.0,
        ]
    }

    pub fn local_uv_f32(&self) -> [f32; 2] {
        [
            self.local_uv[0] as f32 / 2048.0,
            self.local_uv[1] as f32 / 2048.0,
        ]
    }

    pub fn atlas_tile_u32(&self) -> (u32, u32) {
        (self.atlas_tile[0] as u32, self.atlas_tile[1] as u32)
    }

    pub fn light_level(&self) -> f32 {
        let sky_block = (self.light_ao & 0xFF) as f32;
        let multiplier = ((self.light_ao >> 8) & 0x3F) as f32;
        sky_block + multiplier * 256.0
    }

    pub fn ao(&self) -> f32 {
        let ao_idx = ((self.light_ao >> 14) & 0x03) as u8;
        match ao_idx {
            3 => 1.0,
            2 => 0.75,
            1 => 0.5,
            _ => 0.25,
        }
    }
}

/// Axis-aligned bounds of the vertices actually present in a mesh.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct MeshBounds {
    pub min: Vec3,
    pub max: Vec3,
}

impl MeshBounds {
    /// Creates bounds and panics if either endpoint is non-finite or if any
    /// minimum component is greater than the corresponding maximum.
    pub fn new(min: Vec3, max: Vec3) -> Self {
        Self::try_new(min, max).expect("mesh bounds must be finite and ordered")
    }

    pub fn try_new(min: Vec3, max: Vec3) -> Option<Self> {
        (min.is_finite() && max.is_finite() && min.cmple(max).all()).then_some(Self { min, max })
    }

    pub fn from_vertices(vertices: &[TerrainVertex], region_coord: (i32, i32)) -> Option<Self> {
        Self::from_points(
            vertices
                .iter()
                .map(|vertex| vertex.world_position(region_coord)),
        )
    }

    pub fn from_points(points: impl IntoIterator<Item = Vec3>) -> Option<Self> {
        let mut points = points.into_iter();
        let first = points.next()?;
        if !first.is_finite() {
            return None;
        }

        let mut min = first;
        let mut max = first;
        for point in points {
            if !point.is_finite() {
                return None;
            }
            min = min.min(point);
            max = max.max(point);
        }
        Some(Self { min, max })
    }

    pub fn center(self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    pub fn size(self) -> Vec3 {
        self.max - self.min
    }

    pub fn union(self, other: Self) -> Self {
        Self {
            min: self.min.min(other.min),
            max: self.max.max(other.max),
        }
    }

    pub fn translated(self, offset: Vec3) -> Self {
        Self::new(self.min + offset, self.max + offset)
    }

    /// Squared distance to the closest point on this AABB.
    pub fn distance_squared_to_point(self, point: Vec3) -> f32 {
        let closest = point.clamp(self.min, self.max);
        point.distance_squared(closest)
    }

    pub fn center_distance_squared(self, point: Vec3) -> f32 {
        self.center().distance_squared(point)
    }
}

/// Owned CPU mesh data suitable for transfer from a mesh worker.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChunkMeshData {
    pub vertices: Vec<TerrainVertex>,
    pub indices: Vec<u32>,
    pub bounds: Option<MeshBounds>,
}

impl ChunkMeshData {
    pub fn new(vertices: Vec<TerrainVertex>, indices: Vec<u32>, region_coord: (i32, i32)) -> Self {
        let bounds = MeshBounds::from_vertices(&vertices, region_coord);
        Self {
            vertices,
            indices,
            bounds,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChunkLodMeshData {
    pub opaque: ChunkMeshData,
    pub transparent: ChunkMeshData,
}

impl ChunkLodMeshData {
    pub fn from_parts(
        opaque_vertices: Vec<TerrainVertex>,
        opaque_indices: Vec<u32>,
        transparent_vertices: Vec<TerrainVertex>,
        transparent_indices: Vec<u32>,
        region_coord: (i32, i32),
    ) -> Self {
        Self {
            opaque: ChunkMeshData::new(opaque_vertices, opaque_indices, region_coord),
            transparent: ChunkMeshData::new(
                transparent_vertices,
                transparent_indices,
                region_coord,
            ),
        }
    }

    pub fn bounds(&self) -> Option<MeshBounds> {
        match (self.opaque.bounds, self.transparent.bounds) {
            (Some(opaque), Some(transparent)) => Some(opaque.union(transparent)),
            (Some(bounds), None) | (None, Some(bounds)) => Some(bounds),
            (None, None) => None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChunkMeshBundle {
    pub levels: [ChunkLodMeshData; 3],
    pub section_connectivity: [crate::culling::SectionConnectivity; crate::world::SECTION_COUNT],
}

/// CPU result for exactly one 16^3 section. `identity` is checked when a
/// worker result is integrated; the three levels intentionally mirror the
/// legacy chunk bundle until renderer cutover is complete.
#[derive(Clone, Debug, PartialEq)]
pub struct SectionMeshBundle {
    pub identity: crate::world::SectionIdentity,
    pub levels: [ChunkLodMeshData; 3],
    pub bounds: Option<MeshBounds>,
    pub connectivity: crate::culling::SectionConnectivity,
}

impl SectionMeshBundle {
    pub fn level(&self, lod: LodLevel) -> &ChunkLodMeshData {
        &self.levels[lod as usize]
    }
    pub fn is_current(&self, current: crate::world::SectionIdentity) -> bool {
        self.identity.key == current.key
            && self.identity.lifetime == current.lifetime
            && self.identity.revision == current.revision
    }
}

impl ChunkMeshBundle {
    pub fn level(&self, lod: LodLevel) -> &ChunkLodMeshData {
        &self.levels[lod as usize]
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
struct Plane {
    normal: Vec3,
    distance: f32,
}

impl Plane {
    fn from_coefficients(coefficients: Vec4) -> Self {
        let normal = coefficients.truncate();
        let length = normal.length();
        if length.is_finite() && length > f32::EPSILON && coefficients.w.is_finite() {
            Self {
                normal: normal / length,
                distance: coefficients.w / length,
            }
        } else {
            // A degenerate clip plane cannot provide a meaningful rejection.
            // Treating it as always inside avoids falsely hiding the world.
            Self {
                normal: Vec3::ZERO,
                distance: f32::INFINITY,
            }
        }
    }

    fn rejects(self, bounds: MeshBounds) -> bool {
        let positive_vertex = Vec3::new(
            if self.normal.x >= 0.0 {
                bounds.max.x
            } else {
                bounds.min.x
            },
            if self.normal.y >= 0.0 {
                bounds.max.y
            } else {
                bounds.min.y
            },
            if self.normal.z >= 0.0 {
                bounds.max.z
            } else {
                bounds.min.z
            },
        );
        self.normal.dot(positive_vertex) + self.distance < 0.0
    }
}

/// Six normalized planes extracted from a left-handed wgpu view-projection
/// matrix. The accepted clip volume is `-w <= x,y <= w` and `0 <= z <= w`.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Frustum {
    planes: [Plane; 6],
}

impl Frustum {
    pub fn from_view_projection(view_projection: Mat4) -> Self {
        // glam stores matrices as columns. Transposing makes x/y/z/w_axis the
        // original matrix rows, which are the clip-space inequalities.
        let rows = view_projection.transpose();
        let row_x = rows.x_axis;
        let row_y = rows.y_axis;
        let row_z = rows.z_axis;
        let row_w = rows.w_axis;

        Self {
            planes: [
                Plane::from_coefficients(row_w + row_x), // left:   x + w >= 0
                Plane::from_coefficients(row_w - row_x), // right: -x + w >= 0
                Plane::from_coefficients(row_w + row_y), // bottom: y + w >= 0
                Plane::from_coefficients(row_w - row_y), // top:   -y + w >= 0
                Plane::from_coefficients(row_z),         // near:   z >= 0
                Plane::from_coefficients(row_w - row_z), // far:   -z + w >= 0
            ],
        }
    }

    /// Returns true when the AABB is at least partially inside the frustum.
    /// Bounds touching a plane are considered visible.
    pub fn intersects_aabb(&self, bounds: &MeshBounds) -> bool {
        self.planes.iter().all(|plane| !plane.rejects(*bounds))
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DrawLayer {
    Opaque,
    Transparent,
}

/// One independently submitted terrain mesh.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct DrawCandidate {
    pub chunk_coord: (i32, i32),
    /// Vertical section owning this allocation. Legacy/test candidates may
    /// omit it, but runtime terrain draws always carry an exact section.
    pub section_y: Option<u16>,
    pub bounds: MeshBounds,
    pub index_count: u32,
    pub layer: DrawLayer,
    pub lod: LodLevel,
    pub distance_sq: f32,
}

impl DrawCandidate {
    pub fn new(
        chunk_coord: (i32, i32),
        bounds: MeshBounds,
        index_count: u32,
        layer: DrawLayer,
        lod: LodLevel,
        distance_sq: f32,
    ) -> Self {
        Self {
            chunk_coord,
            section_y: None,
            bounds,
            index_count,
            layer,
            lod,
            distance_sq,
        }
    }

    pub fn for_section(
        key: crate::world::SectionKey,
        bounds: MeshBounds,
        index_count: u32,
        layer: DrawLayer,
        lod: LodLevel,
        distance_sq: f32,
    ) -> Self {
        Self {
            chunk_coord: (key.cx, key.cz),
            section_y: Some(key.section_y),
            bounds,
            index_count,
            layer,
            lod,
            distance_sq,
        }
    }
}

/// Visible terrain draws split into the two required submission orders.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DrawPlan {
    pub opaque: Vec<DrawCandidate>,
    pub transparent: Vec<DrawCandidate>,
    /// Reusable storage for tracking the chunks represented by this plan.
    ///
    /// The render loop rebuilds one plan every frame. Keeping this set on the
    /// plan lets it retain its capacity instead of allocating a temporary set
    /// whenever the visible-chunk statistic is queried.
    visible_chunks: HashSet<(i32, i32)>,
    visible_chunk_count_cache: usize,
}

impl DrawPlan {
    pub fn clear(&mut self) {
        self.opaque.clear();
        self.transparent.clear();
        self.visible_chunks.clear();
        self.visible_chunk_count_cache = 0;
    }

    pub fn build_into(
        &mut self,
        candidates: impl IntoIterator<Item = DrawCandidate>,
        frustum: &Frustum,
    ) {
        self.clear();

        for candidate in candidates {
            if candidate.index_count == 0 || !frustum.intersects_aabb(&candidate.bounds) {
                continue;
            }
            if self.visible_chunks.insert(candidate.chunk_coord) {
                self.visible_chunk_count_cache += 1;
            }
            match candidate.layer {
                DrawLayer::Opaque => self.opaque.push(candidate),
                DrawLayer::Transparent => self.transparent.push(candidate),
            }
        }

        self.opaque.sort_by(|left, right| {
            left.distance_sq
                .total_cmp(&right.distance_sq)
                .then_with(|| left.chunk_coord.cmp(&right.chunk_coord))
                .then_with(|| left.section_y.cmp(&right.section_y))
        });
        self.transparent.sort_by(|left, right| {
            right
                .distance_sq
                .total_cmp(&left.distance_sq)
                .then_with(|| left.chunk_coord.cmp(&right.chunk_coord))
                .then_with(|| left.section_y.cmp(&right.section_y))
        });
    }

    #[allow(dead_code)] // Convenience constructor used by tests; production uses build_into.
    pub fn build(
        candidates: impl IntoIterator<Item = DrawCandidate>,
        frustum: &Frustum,
        _camera_position: Vec3,
    ) -> Self {
        let mut plan = Self::default();
        plan.build_into(candidates, frustum);
        plan
    }

    pub fn draw_call_count(&self) -> usize {
        self.opaque.len() + self.transparent.len()
    }

    pub fn submitted_triangle_count(&self) -> u64 {
        self.opaque
            .iter()
            .chain(&self.transparent)
            .map(|candidate| u64::from(candidate.index_count / 3))
            .sum()
    }

    pub fn visible_chunk_count(&self) -> usize {
        self.visible_chunk_count_cache
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LodLevel {
    /// Full greedy terrain mesh.
    L0,
    /// Per-column surface mesh.
    L1,
    /// Coarse terrain outline.
    L2,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct LodThresholds {
    /// Distances at or beyond this value select L1.
    pub l1_distance: f32,
    /// Distances at or beyond this value select L2.
    pub l2_distance: f32,
}

impl LodThresholds {
    pub fn new(l1_distance: f32, l2_distance: f32) -> Self {
        Self::try_new(l1_distance, l2_distance)
            .expect("LOD thresholds must be finite, non-negative, and ordered")
    }

    pub fn try_new(l1_distance: f32, l2_distance: f32) -> Option<Self> {
        (l1_distance.is_finite()
            && l2_distance.is_finite()
            && l1_distance >= 0.0
            && l1_distance <= l2_distance)
            .then_some(Self {
                l1_distance,
                l2_distance,
            })
    }
}

/// Chooses an LOD from a distance measured in world blocks.
///
/// Negative distances are treated as zero. A non-finite distance selects the
/// cheapest LOD, which is the safe fallback for a malformed camera position.
pub fn select_lod(distance: f32, thresholds: LodThresholds) -> LodLevel {
    if !distance.is_finite() {
        return LodLevel::L2;
    }

    let distance = distance.max(0.0);
    if distance >= thresholds.l2_distance {
        LodLevel::L2
    } else if distance >= thresholds.l1_distance {
        LodLevel::L1
    } else {
        LodLevel::L0
    }
}

pub fn select_lod_for_bounds(
    camera_position: Vec3,
    bounds: MeshBounds,
    thresholds: LodThresholds,
) -> LodLevel {
    select_lod(
        bounds.distance_squared_to_point(camera_position).sqrt(),
        thresholds,
    )
}

/// Number of chunks along one axis in a single render region (8x8 chunks).
pub const REGION_SIZE_CHUNKS: i32 = 8;

/// Maps a chunk coordinate (cx, cz) to its 8x8 render region coordinate.
pub fn chunk_to_region_coord(cx: i32, cz: i32) -> (i32, i32) {
    (
        cx.div_euclid(REGION_SIZE_CHUNKS),
        cz.div_euclid(REGION_SIZE_CHUNKS),
    )
}

/// Handle representing a suballocation range inside a render region's GPU buffers.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct RegionAllocationHandle {
    pub region_instance_id: u64,
    pub vertex_token: AllocationToken,
    pub index_token: AllocationToken,
    pub vertex_offset: u32,
    pub index_offset: u32,
    pub num_vertices: u32,
    pub num_indices: u32,
}

/// Stable owner identity for a terrain allocation.  Packed explicitly so stale
/// uploads from an older terrain generation/chunk lifetime cannot be freed by
/// a later mesh occupying the same range.
pub fn allocation_owner(
    terrain_generation: u64,
    chunk_lifetime: u64,
    section_y: u16,
    lod: u8,
    layer: u8,
) -> u64 {
    terrain_generation
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(chunk_lifetime.rotate_left(17))
        .wrapping_add((section_y as u64) << 32)
        .wrapping_add((lod as u64) << 8)
        .wrapping_add(layer as u64)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FreeListBlock {
    pub offset: u32,
    pub count: u32,
}

/// A free-list allocator managing suballocation ranges inside a contiguous buffer capacity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FreeList {
    capacity: u32,
    free_blocks: Vec<FreeListBlock>,
    /// Live allocations keyed by offset. Keeping this side table makes
    /// deallocation transactional: malformed, stale, and double frees are
    /// rejected instead of corrupting the free list.
    allocations: BTreeMap<u32, AllocationRecord>,
    next_generation: u64,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct AllocationRecord {
    count: u32,
    generation: u64,
    owner: u64,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct AllocationToken {
    pub offset: u32,
    pub count: u32,
    pub generation: u64,
    pub owner: u64,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FreeListError {
    ZeroSizedAllocation,
    OutOfBounds,
    Overlap,
    UnknownAllocation,
    StaleGeneration,
    WrongOwner,
    ArithmeticOverflow,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct CompactionMove {
    pub from: u32,
    pub to: u32,
    pub count: u32,
    pub generation: u64,
}

impl FreeList {
    pub fn new(capacity: u32) -> Self {
        Self {
            capacity,
            free_blocks: if capacity > 0 {
                vec![FreeListBlock {
                    offset: 0,
                    count: capacity,
                }]
            } else {
                Vec::new()
            },
            allocations: BTreeMap::new(),
            next_generation: 1,
        }
    }

    pub fn capacity(&self) -> u32 {
        self.capacity
    }

    pub fn checked_used_units(&self) -> Result<u32, FreeListError> {
        self.capacity
            .checked_sub(self.checked_free_units()?)
            .ok_or(FreeListError::ArithmeticOverflow)
    }

    pub fn used_units(&self) -> u32 {
        self.checked_used_units()
            .expect("free-list used-unit accounting overflow")
    }

    pub fn checked_free_units(&self) -> Result<u32, FreeListError> {
        self.free_blocks.iter().try_fold(0u32, |total, block| {
            let end = block
                .offset
                .checked_add(block.count)
                .ok_or(FreeListError::ArithmeticOverflow)?;
            if end > self.capacity {
                return Err(FreeListError::ArithmeticOverflow);
            }
            total
                .checked_add(block.count)
                .ok_or(FreeListError::ArithmeticOverflow)
        })
    }

    pub fn free_units(&self) -> u32 {
        self.checked_free_units()
            .expect("free-list free-unit accounting overflow")
    }

    pub fn counters_consistent(&self) -> bool {
        self.checked_free_units().and_then(|free| {
            self.checked_used_units().and_then(|used| {
                free.checked_add(used)
                    .ok_or(FreeListError::ArithmeticOverflow)
            })
        }) == Ok(self.capacity)
    }

    pub fn largest_free_block(&self) -> u32 {
        self.free_blocks.iter().map(|b| b.count).max().unwrap_or(0)
    }

    pub fn free_blocks(&self) -> &[FreeListBlock] {
        &self.free_blocks
    }

    pub fn allocate(&mut self, count: u32) -> Option<u32> {
        self.allocate_owned(count, 0).ok().map(|token| token.offset)
    }

    pub fn allocate_owned(
        &mut self,
        count: u32,
        owner: u64,
    ) -> Result<AllocationToken, FreeListError> {
        if count == 0 {
            return Err(FreeListError::ZeroSizedAllocation);
        }
        let index = self
            .free_blocks
            .iter()
            .position(|b| b.count >= count)
            .ok_or(FreeListError::OutOfBounds)?;
        let offset = self.free_blocks[index].offset;
        let block_end = offset
            .checked_add(self.free_blocks[index].count)
            .ok_or(FreeListError::ArithmeticOverflow)?;
        if block_end > self.capacity {
            return Err(FreeListError::ArithmeticOverflow);
        }
        let new_end = offset
            .checked_add(count)
            .ok_or(FreeListError::ArithmeticOverflow)?;
        if new_end > self.capacity {
            return Err(FreeListError::OutOfBounds);
        }
        let next_generation = self
            .next_generation
            .checked_add(1)
            .ok_or(FreeListError::ArithmeticOverflow)?;
        let remaining = self.free_blocks[index]
            .count
            .checked_sub(count)
            .ok_or(FreeListError::ArithmeticOverflow)?;
        self.free_blocks[index].offset = new_end;
        self.free_blocks[index].count = remaining;
        if self.free_blocks[index].count == 0 {
            self.free_blocks.remove(index);
        }
        let generation = self.next_generation;
        self.next_generation = next_generation;
        self.allocations.insert(
            offset,
            AllocationRecord {
                count,
                generation,
                owner,
            },
        );
        Ok(AllocationToken {
            offset,
            count,
            generation,
            owner,
        })
    }

    pub fn deallocate(&mut self, offset: u32, count: u32) -> Result<(), FreeListError> {
        if count == 0 {
            return Err(FreeListError::ZeroSizedAllocation);
        }
        let end = offset
            .checked_add(count)
            .ok_or(FreeListError::ArithmeticOverflow)?;
        if end > self.capacity {
            return Err(FreeListError::OutOfBounds);
        }
        let record = self
            .allocations
            .get(&offset)
            .copied()
            .ok_or(FreeListError::UnknownAllocation)?;
        if record.count != count {
            return Err(FreeListError::StaleGeneration);
        }
        self.deallocate_record(offset, count, record)
    }

    pub fn deallocate_owned(&mut self, token: AllocationToken) -> Result<(), FreeListError> {
        let record = self
            .allocations
            .get(&token.offset)
            .copied()
            .ok_or(FreeListError::UnknownAllocation)?;
        if record.count != token.count || record.generation != token.generation {
            return Err(FreeListError::StaleGeneration);
        }
        if record.owner != token.owner {
            return Err(FreeListError::WrongOwner);
        }
        self.deallocate_record(token.offset, token.count, record)
    }

    pub fn validate_owned(&self, token: AllocationToken) -> Result<(), FreeListError> {
        let record = self
            .allocations
            .get(&token.offset)
            .copied()
            .ok_or(FreeListError::UnknownAllocation)?;
        if record.count != token.count || record.generation != token.generation {
            return Err(FreeListError::StaleGeneration);
        }
        if record.owner != token.owner {
            return Err(FreeListError::WrongOwner);
        }
        Ok(())
    }

    fn deallocate_record(
        &mut self,
        offset: u32,
        count: u32,
        _record: AllocationRecord,
    ) -> Result<(), FreeListError> {
        let end = offset
            .checked_add(count)
            .ok_or(FreeListError::ArithmeticOverflow)?;
        if end > self.capacity {
            return Err(FreeListError::OutOfBounds);
        }
        let insert_idx = self
            .free_blocks
            .binary_search_by_key(&offset, |b| b.offset)
            .unwrap_or_else(|idx| idx);

        if insert_idx > 0 {
            let prev = &self.free_blocks[insert_idx - 1];
            let prev_end = prev
                .offset
                .checked_add(prev.count)
                .ok_or(FreeListError::ArithmeticOverflow)?;
            if prev_end > offset {
                return Err(FreeListError::Overlap);
            }
        }
        if let Some(next) = self.free_blocks.get(insert_idx) {
            if end > next.offset {
                return Err(FreeListError::Overlap);
            }
        }
        let mut free_blocks = self.free_blocks.clone();
        free_blocks.insert(insert_idx, FreeListBlock { offset, count });
        let mut i = 0;
        while i + 1 < free_blocks.len() {
            let block_end = free_blocks[i]
                .offset
                .checked_add(free_blocks[i].count)
                .ok_or(FreeListError::ArithmeticOverflow)?;
            if block_end == free_blocks[i + 1].offset {
                free_blocks[i].count = free_blocks[i]
                    .count
                    .checked_add(free_blocks[i + 1].count)
                    .ok_or(FreeListError::ArithmeticOverflow)?;
                free_blocks.remove(i + 1);
            } else {
                i += 1;
            }
        }
        for block in &free_blocks {
            let block_end = block
                .offset
                .checked_add(block.count)
                .ok_or(FreeListError::ArithmeticOverflow)?;
            if block_end > self.capacity {
                return Err(FreeListError::ArithmeticOverflow);
            }
        }
        self.free_blocks = free_blocks;
        self.allocations.remove(&offset);
        Ok(())
    }

    pub fn resize(&mut self, new_capacity: u32) -> Result<(), FreeListError> {
        if new_capacity <= self.capacity {
            return Ok(());
        }
        let added = new_capacity
            .checked_sub(self.capacity)
            .ok_or(FreeListError::ArithmeticOverflow)?;
        let old_capacity = self.capacity;
        // The appended range is unallocated by construction.
        let mut free_blocks = self.free_blocks.clone();
        free_blocks.push(FreeListBlock {
            offset: old_capacity,
            count: added,
        });
        free_blocks.sort_by_key(|b| b.offset);
        let mut i = 0;
        while i + 1 < free_blocks.len() {
            let block_end = free_blocks[i]
                .offset
                .checked_add(free_blocks[i].count)
                .ok_or(FreeListError::ArithmeticOverflow)?;
            if block_end == free_blocks[i + 1].offset {
                free_blocks[i].count = free_blocks[i]
                    .count
                    .checked_add(free_blocks[i + 1].count)
                    .ok_or(FreeListError::ArithmeticOverflow)?;
                free_blocks.remove(i + 1);
            } else {
                i += 1;
            }
        }
        for block in &free_blocks {
            let block_end = block
                .offset
                .checked_add(block.count)
                .ok_or(FreeListError::ArithmeticOverflow)?;
            if block_end > new_capacity {
                return Err(FreeListError::ArithmeticOverflow);
            }
        }
        self.capacity = new_capacity;
        self.free_blocks = free_blocks;
        Ok(())
    }

    /// Returns a deterministic low-priority compaction plan and applies the
    /// corresponding ranges to allocator metadata. The caller must copy GPU
    /// data according to the returned moves before rendering the next frame.
    pub fn compact(&mut self) -> Result<Vec<CompactionMove>, FreeListError> {
        let mut cursor = 0u32;
        let mut moves = Vec::new();
        let mut allocations = BTreeMap::new();
        for (&from, &record) in &self.allocations {
            let source_end = from
                .checked_add(record.count)
                .ok_or(FreeListError::ArithmeticOverflow)?;
            if source_end > self.capacity {
                return Err(FreeListError::ArithmeticOverflow);
            }
            let to = cursor;
            if from != to {
                moves.push(CompactionMove {
                    from,
                    to,
                    count: record.count,
                    generation: record.generation,
                });
            }
            allocations.insert(to, record);
            cursor = cursor
                .checked_add(record.count)
                .ok_or(FreeListError::ArithmeticOverflow)?;
            if cursor > self.capacity {
                return Err(FreeListError::ArithmeticOverflow);
            }
        }
        let free_blocks = if cursor < self.capacity {
            vec![FreeListBlock {
                offset: cursor,
                count: self
                    .capacity
                    .checked_sub(cursor)
                    .ok_or(FreeListError::ArithmeticOverflow)?,
            }]
        } else {
            Vec::new()
        };
        self.allocations = allocations;
        self.free_blocks = free_blocks;
        Ok(moves)
    }

    pub fn fragmentation(&self) -> f32 {
        let total_free = self.free_units();
        if total_free == 0 {
            return 0.0;
        }
        let largest = self.largest_free_block();
        (total_free - largest) as f32 / total_free as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{offset_of, size_of};

    fn bounds(min: [f32; 3], max: [f32; 3]) -> MeshBounds {
        MeshBounds::new(Vec3::from(min), Vec3::from(max))
    }

    fn perspective_frustum() -> Frustum {
        let view = Mat4::look_at_lh(Vec3::ZERO, Vec3::Z, Vec3::Y);
        let projection = Mat4::perspective_lh(std::f32::consts::FRAC_PI_2, 1.0, 1.0, 10.0);
        Frustum::from_view_projection(projection * view)
    }

    fn wide_frustum() -> Frustum {
        Frustum::from_view_projection(Mat4::orthographic_lh(
            -100.0, 100.0, -100.0, 100.0, 0.0, 100.0,
        ))
    }

    fn candidate(
        chunk_coord: (i32, i32),
        z: f32,
        index_count: u32,
        layer: DrawLayer,
    ) -> DrawCandidate {
        let b = bounds([-0.25, -0.25, z - 0.25], [0.25, 0.25, z + 0.25]);
        let dist_sq = b.center_distance_squared(Vec3::ZERO);
        DrawCandidate::new(chunk_coord, b, index_count, layer, LodLevel::L0, dist_sq)
    }

    #[test]
    fn terrain_vertex_is_pod_with_expected_cpu_layout() {
        fn assert_pod<T: bytemuck::Pod + bytemuck::Zeroable>() {}
        assert_pod::<TerrainVertex>();

        assert_eq!(size_of::<TerrainVertex>(), 16);
        assert_eq!(offset_of!(TerrainVertex, pos), 0);
        assert_eq!(offset_of!(TerrainVertex, light_ao), 6);
        assert_eq!(offset_of!(TerrainVertex, local_uv), 8);
        assert_eq!(offset_of!(TerrainVertex, atlas_tile), 12);

        let vertex =
            TerrainVertex::new([1.0, 2.0, 3.0], [4.0, 5.0], [6.0, 7.0], 15.0, 0.75, (0, 0));
        assert_eq!(bytemuck::bytes_of(&vertex).len(), 16);
    }

    #[test]
    fn mesh_bounds_are_derived_from_actual_vertices() {
        let vertices = [
            TerrainVertex::new([4.0, 8.0, 2.0], [0.0; 2], [0.0; 2], 0.0, 1.0, (0, 0)),
            TerrainVertex::new([1.0, 3.0, 6.0], [0.0; 2], [0.0; 2], 0.0, 1.0, (0, 0)),
            TerrainVertex::new([2.0, 12.0, 1.0], [0.0; 2], [0.0; 2], 0.0, 1.0, (0, 0)),
        ];
        let mesh_bounds = MeshBounds::from_vertices(&vertices, (0, 0)).unwrap();
        assert!((mesh_bounds.min.x - 1.0).abs() < 0.05);
        assert!((mesh_bounds.min.y - 3.0).abs() < 0.05);
        assert!((mesh_bounds.min.z - 1.0).abs() < 0.05);
        assert!((mesh_bounds.max.x - 4.0).abs() < 0.05);
        assert!((mesh_bounds.max.y - 12.0).abs() < 0.05);
        assert!((mesh_bounds.max.z - 6.0).abs() < 0.05);
        assert_eq!(MeshBounds::from_vertices(&[], (0, 0)), None);
    }

    #[test]
    fn mesh_bounds_validate_union_and_distance() {
        assert!(MeshBounds::try_new(Vec3::ONE, Vec3::ZERO).is_none());
        assert!(MeshBounds::try_new(Vec3::ZERO, Vec3::splat(f32::NAN)).is_none());

        let first = bounds([0.0, 0.0, 0.0], [2.0, 2.0, 2.0]);
        let second = bounds([-2.0, 1.0, 1.0], [-1.0, 3.0, 4.0]);
        let union = first.union(second);
        assert_eq!(union.min, Vec3::new(-2.0, 0.0, 0.0));
        assert_eq!(union.max, Vec3::new(2.0, 3.0, 4.0));
        assert_eq!(first.distance_squared_to_point(Vec3::ONE), 0.0);
        assert_eq!(
            first.distance_squared_to_point(Vec3::new(5.0, 2.0, -4.0)),
            25.0
        );
    }

    #[test]
    fn chunk_mesh_data_tracks_bounds_and_triangles() {
        let vertices = vec![
            TerrainVertex::new([0.0, 0.0, 0.0], [0.0; 2], [0.0; 2], 0.0, 1.0, (0, 0)),
            TerrainVertex::new([1.0, 0.0, 0.0], [0.0; 2], [0.0; 2], 0.0, 1.0, (0, 0)),
            TerrainVertex::new([0.0, 1.0, 0.0], [0.0; 2], [0.0; 2], 0.0, 1.0, (0, 0)),
        ];
        let mesh = ChunkMeshData::new(vertices, vec![0, 1, 2], (0, 0));
        assert!(!mesh.is_empty());
        assert_eq!(mesh.triangle_count(), 1);
        assert!(mesh.bounds.is_some());
    }

    #[test]
    fn identity_matrix_uses_wgpu_zero_to_one_depth() {
        let frustum = Frustum::from_view_projection(Mat4::IDENTITY);
        assert!(frustum.intersects_aabb(&bounds([-0.5, -0.5, 0.25], [0.5, 0.5, 0.75])));
        assert!(!frustum.intersects_aabb(&bounds([-0.5, -0.5, -0.75], [0.5, 0.5, -0.25])));
        assert!(!frustum.intersects_aabb(&bounds([-0.5, -0.5, 1.25], [0.5, 0.5, 1.75])));
    }

    #[test]
    fn identity_frustum_rejects_each_lateral_side() {
        let frustum = Frustum::from_view_projection(Mat4::IDENTITY);
        for outside in [
            bounds([-2.0, -0.5, 0.2], [-1.1, 0.5, 0.8]),
            bounds([1.1, -0.5, 0.2], [2.0, 0.5, 0.8]),
            bounds([-0.5, -2.0, 0.2], [0.5, -1.1, 0.8]),
            bounds([-0.5, 1.1, 0.2], [0.5, 2.0, 0.8]),
        ] {
            assert!(!frustum.intersects_aabb(&outside));
        }
    }

    #[test]
    fn aabb_touching_or_crossing_a_plane_remains_visible() {
        let frustum = Frustum::from_view_projection(Mat4::IDENTITY);
        assert!(frustum.intersects_aabb(&bounds([-1.0, -0.2, 0.0], [-0.8, 0.2, 0.2])));
        assert!(frustum.intersects_aabb(&bounds([-2.0, -0.2, 0.2], [0.0, 0.2, 0.8])));
    }

    #[test]
    fn perspective_frustum_handles_front_back_near_and_far() {
        let frustum = perspective_frustum();
        assert!(frustum.intersects_aabb(&bounds([-0.5, -0.5, 2.0], [0.5, 0.5, 3.0])));
        assert!(!frustum.intersects_aabb(&bounds([-0.2, -0.2, -2.0], [0.2, 0.2, -1.0])));
        assert!(!frustum.intersects_aabb(&bounds([-0.2, -0.2, 0.1], [0.2, 0.2, 0.9])));
        assert!(!frustum.intersects_aabb(&bounds([-0.5, -0.5, 10.1], [0.5, 0.5, 11.0])));
        assert!(!frustum.intersects_aabb(&bounds([4.0, -0.2, 2.0], [5.0, 0.2, 3.0])));
    }

    #[test]
    fn degenerate_view_projection_does_not_false_cull() {
        let frustum = Frustum::from_view_projection(Mat4::ZERO);
        assert!(frustum.intersects_aabb(&bounds(
            [-1000.0, -1000.0, -1000.0],
            [1000.0, 1000.0, 1000.0]
        )));
    }

    #[test]
    fn draw_plan_culls_empty_and_outside_candidates() {
        let frustum = wide_frustum();
        let candidates = [
            candidate((0, 0), 10.0, 6, DrawLayer::Opaque),
            candidate((1, 0), 20.0, 0, DrawLayer::Opaque),
            candidate((2, 0), 200.0, 6, DrawLayer::Transparent),
        ];

        let plan = DrawPlan::build(candidates, &frustum, Vec3::ZERO);
        assert_eq!(plan.opaque.len(), 1);
        assert!(plan.transparent.is_empty());
        assert_eq!(plan.draw_call_count(), 1);
    }

    #[test]
    fn opaque_is_near_to_far_with_coordinate_tie_break() {
        let frustum = wide_frustum();
        let candidates = [
            candidate((7, 0), 30.0, 6, DrawLayer::Opaque),
            candidate((2, 0), 10.0, 6, DrawLayer::Opaque),
            candidate((-3, 4), 10.0, 6, DrawLayer::Opaque),
            candidate((-4, 4), 10.0, 6, DrawLayer::Opaque),
        ];

        let plan = DrawPlan::build(candidates, &frustum, Vec3::ZERO);
        let coords: Vec<_> = plan
            .opaque
            .iter()
            .map(|candidate| candidate.chunk_coord)
            .collect();
        assert_eq!(coords, vec![(-4, 4), (-3, 4), (2, 0), (7, 0)]);
    }

    #[test]
    fn transparent_is_far_to_near_with_coordinate_tie_break() {
        let frustum = wide_frustum();
        let candidates = [
            candidate((7, 0), 30.0, 6, DrawLayer::Transparent),
            candidate((2, 0), 10.0, 6, DrawLayer::Transparent),
            candidate((-3, 4), 10.0, 6, DrawLayer::Transparent),
            candidate((-4, 4), 10.0, 6, DrawLayer::Transparent),
        ];

        let plan = DrawPlan::build(candidates, &frustum, Vec3::ZERO);
        let coords: Vec<_> = plan
            .transparent
            .iter()
            .map(|candidate| candidate.chunk_coord)
            .collect();
        assert_eq!(coords, vec![(7, 0), (-4, 4), (-3, 4), (2, 0)]);
    }

    #[test]
    fn draw_plan_statistics_count_unique_chunks() {
        let frustum = wide_frustum();
        let candidates = [
            candidate((0, 0), 10.0, 12, DrawLayer::Opaque),
            candidate((0, 0), 10.0, 6, DrawLayer::Transparent),
            candidate((1, 0), 20.0, 18, DrawLayer::Opaque),
        ];
        let plan = DrawPlan::build(candidates, &frustum, Vec3::ZERO);
        assert_eq!(plan.visible_chunk_count(), 2);
        assert_eq!(plan.draw_call_count(), 3);
        assert_eq!(plan.submitted_triangle_count(), 12);
    }

    #[test]
    fn draw_plan_reuses_visible_chunk_storage_without_allocating() {
        let frustum = wide_frustum();
        let candidates = [
            candidate((0, 0), 10.0, 12, DrawLayer::Opaque),
            candidate((0, 0), 10.0, 6, DrawLayer::Transparent),
            candidate((1, 0), 20.0, 18, DrawLayer::Opaque),
        ];
        let mut plan = DrawPlan::default();

        // The first build warms every vector/set used by the plan. Repeated
        // builds with the same shape must stay within those capacities.
        plan.build_into(candidates, &frustum);
        let opaque_capacity = plan.opaque.capacity();
        let transparent_capacity = plan.transparent.capacity();
        let visible_chunk_capacity = plan.visible_chunks.capacity();

        for _ in 0..8 {
            plan.build_into(candidates, &frustum);
        }

        assert_eq!(plan.opaque.capacity(), opaque_capacity);
        assert_eq!(plan.transparent.capacity(), transparent_capacity);
        assert_eq!(plan.visible_chunks.capacity(), visible_chunk_capacity);
        assert_eq!(plan.visible_chunk_count(), 2);
    }

    #[test]
    fn lod_selection_obeys_boundaries_and_safe_fallbacks() {
        let thresholds = LodThresholds::new(96.0, 192.0);
        assert_eq!(select_lod(-1.0, thresholds), LodLevel::L0);
        assert_eq!(select_lod(95.99, thresholds), LodLevel::L0);
        assert_eq!(select_lod(96.0, thresholds), LodLevel::L1);
        assert_eq!(select_lod(191.99, thresholds), LodLevel::L1);
        assert_eq!(select_lod(192.0, thresholds), LodLevel::L2);
        assert_eq!(select_lod(f32::INFINITY, thresholds), LodLevel::L2);
        assert_eq!(select_lod(f32::NAN, thresholds), LodLevel::L2);
    }

    #[test]
    fn lod_thresholds_reject_invalid_ranges() {
        assert!(LodThresholds::try_new(-1.0, 10.0).is_none());
        assert!(LodThresholds::try_new(10.0, 9.0).is_none());
        assert!(LodThresholds::try_new(f32::NAN, 10.0).is_none());
        assert_eq!(
            LodThresholds::try_new(0.0, 0.0),
            Some(LodThresholds {
                l1_distance: 0.0,
                l2_distance: 0.0
            })
        );
    }

    #[test]
    fn lod_can_be_selected_from_mesh_bounds() {
        let thresholds = LodThresholds::new(10.0, 20.0);
        let mesh_bounds = bounds([14.0, -1.0, -1.0], [16.0, 1.0, 1.0]);
        assert_eq!(
            select_lod_for_bounds(Vec3::ZERO, mesh_bounds, thresholds),
            LodLevel::L1
        );
    }

    #[test]
    fn lod_uses_nearest_distance_for_tall_bounds() {
        let thresholds = LodThresholds::new(64.0, 96.0);
        let tall_bounds = bounds([0.0, 0.0, 0.0], [16.0, 256.0, 16.0]);

        assert_eq!(
            select_lod_for_bounds(Vec3::new(8.0, 4.0, 8.0), tall_bounds, thresholds),
            LodLevel::L0,
            "a camera inside a tall chunk must retain its full mesh"
        );
        assert_eq!(
            select_lod_for_bounds(Vec3::new(-10.0, 128.0, 8.0), tall_bounds, thresholds),
            LodLevel::L0,
            "vertical extent must not make a nearby chunk appear distant"
        );
        assert_eq!(
            select_lod_for_bounds(Vec3::new(-100.0, 128.0, 8.0), tall_bounds, thresholds),
            LodLevel::L2
        );
    }

    #[test]
    fn chunk_to_region_coord_maps_8x8_blocks() {
        assert_eq!(chunk_to_region_coord(0, 0), (0, 0));
        assert_eq!(chunk_to_region_coord(7, 7), (0, 0));
        assert_eq!(chunk_to_region_coord(8, 7), (1, 0));
        assert_eq!(chunk_to_region_coord(-1, -1), (-1, -1));
        assert_eq!(chunk_to_region_coord(-8, -8), (-1, -1));
        assert_eq!(chunk_to_region_coord(-9, -9), (-2, -2));
    }

    #[test]
    fn freelist_allocates_deallocates_and_coalesces() {
        let mut list = FreeList::new(100);
        assert_eq!(list.capacity(), 100);
        assert_eq!(list.free_units(), 100);

        let a1 = list.allocate(30).unwrap();
        assert_eq!(a1, 0);
        assert_eq!(list.free_units(), 70);

        let a2 = list.allocate(40).unwrap();
        assert_eq!(a2, 30);
        assert_eq!(list.free_units(), 30);

        list.deallocate(a1, 30).unwrap();
        assert_eq!(list.free_units(), 60);

        let a3 = list.allocate(20).unwrap();
        assert_eq!(a3, 0);
        assert_eq!(list.free_units(), 40);

        list.deallocate(a2, 40).unwrap();
        list.deallocate(a3, 20).unwrap();
        assert_eq!(list.free_units(), 100);
        assert_eq!(list.largest_free_block(), 100);
        assert_eq!(list.fragmentation(), 0.0);
    }

    #[test]
    fn freelist_rejects_stale_double_free_and_out_of_bounds() {
        let mut list = FreeList::new(16);
        let token = list.allocate_owned(4, 7).unwrap();
        assert_eq!(list.deallocate_owned(token), Ok(()));
        assert_eq!(
            list.deallocate_owned(token),
            Err(FreeListError::UnknownAllocation)
        );
        assert_eq!(list.deallocate(15, 2), Err(FreeListError::OutOfBounds));
        let token = list.allocate_owned(3, 7).unwrap();
        let mut stale = token;
        stale.generation += 1;
        assert_eq!(
            list.deallocate_owned(stale),
            Err(FreeListError::StaleGeneration)
        );
        let mut wrong_owner = token;
        wrong_owner.owner = 8;
        assert_eq!(
            list.deallocate_owned(wrong_owner),
            Err(FreeListError::WrongOwner)
        );
    }

    #[test]
    fn owned_tokens_reject_wrong_owner_and_counters_stay_bounded() {
        let mut list = FreeList::new(16);
        let token = list.allocate_owned(4, 7).unwrap();
        assert!(matches!(
            list.deallocate_owned(AllocationToken { owner: 8, ..token }),
            Err(FreeListError::WrongOwner)
        ));
        assert!(list.counters_consistent());
        assert!(list.deallocate_owned(token).is_ok());
        assert!(list.counters_consistent());
    }

    #[test]
    fn allocation_owner_changes_across_runtime_identity() {
        assert_ne!(
            allocation_owner(1, 2, 0, 0, 0),
            allocation_owner(2, 2, 0, 0, 0)
        );
        assert_ne!(
            allocation_owner(1, 2, 0, 0, 0),
            allocation_owner(1, 3, 0, 0, 0)
        );
    }

    #[test]
    fn freelist_compaction_preserves_capacity_and_non_overlap() {
        let mut list = FreeList::new(32);
        let a = list.allocate(8).unwrap();
        let b = list.allocate(4).unwrap();
        list.deallocate(a, 8).unwrap();
        let moves = list.compact().unwrap();
        assert_eq!(list.used_units() + list.free_units(), list.capacity());
        assert!(moves.iter().all(|m| m.from != m.to));
        assert_eq!(
            list.free_blocks()
                .windows(2)
                .all(|w| w[0].offset + w[0].count <= w[1].offset),
            true
        );
        let _ = b;
    }

    #[test]
    fn freelist_deterministic_randomized_invariants() {
        let mut list = FreeList::new(257);
        let mut live = Vec::new();
        let mut seed = 0x9e37_79b9u32;
        for _ in 0..400 {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            match seed % 5 {
                0 if !live.is_empty() => {
                    let index = (seed as usize) % live.len();
                    let (offset, count) = live.swap_remove(index);
                    list.deallocate(offset, count).unwrap();
                }
                1 => {
                    let moves = list.compact().unwrap();
                    for movement in moves {
                        let allocation = live
                            .iter_mut()
                            .find(|(offset, count)| {
                                *offset == movement.from && *count == movement.count
                            })
                            .expect("every compaction move must identify a live allocation");
                        allocation.0 = movement.to;
                    }
                }
                2 => {
                    let growth = seed % 17 + 1;
                    let new_capacity = list.capacity().checked_add(growth).unwrap();
                    list.resize(new_capacity).unwrap();
                }
                _ => {
                    let count = seed % 9 + 1;
                    if let Some(offset) = list.allocate(count) {
                        live.push((offset, count));
                    }
                }
            }

            let used = list.checked_used_units().unwrap();
            let free = list.checked_free_units().unwrap();
            assert_eq!(used.checked_add(free), Some(list.capacity()));

            live.sort_unstable_by_key(|&(offset, _)| offset);
            assert!(live
                .windows(2)
                .all(|window| { window[0].0.checked_add(window[0].1).unwrap() <= window[1].0 }));
            assert!(live
                .iter()
                .all(|&(offset, count)| { offset.checked_add(count).unwrap() <= list.capacity() }));
            assert_eq!(
                live.iter()
                    .try_fold(0u32, |total, &(_, count)| total.checked_add(count)),
                Some(used)
            );
            assert!(list.free_blocks().windows(2).all(|window| {
                window[0].offset.checked_add(window[0].count).unwrap() < window[1].offset
            }));
        }
    }

    #[test]
    fn freelist_reports_arithmetic_overflow_without_mutation() {
        let mut exhausted = FreeList::new(8);
        exhausted.next_generation = u64::MAX;
        let before = exhausted.clone();
        assert_eq!(
            exhausted.allocate_owned(1, 7),
            Err(FreeListError::ArithmeticOverflow)
        );
        assert_eq!(exhausted, before);

        let mut invalid_accounting = FreeList::new(0);
        invalid_accounting.free_blocks = vec![
            FreeListBlock {
                offset: 0,
                count: u32::MAX,
            },
            FreeListBlock {
                offset: u32::MAX,
                count: 1,
            },
        ];
        assert_eq!(
            invalid_accounting.checked_free_units(),
            Err(FreeListError::ArithmeticOverflow)
        );
        assert_eq!(
            invalid_accounting.checked_used_units(),
            Err(FreeListError::ArithmeticOverflow)
        );
        assert!(!invalid_accounting.counters_consistent());

        let mut invalid_resize = FreeList::new(1);
        invalid_resize.free_blocks = vec![FreeListBlock {
            offset: u32::MAX - 1,
            count: 2,
        }];
        let before = invalid_resize.clone();
        assert_eq!(
            invalid_resize.resize(u32::MAX),
            Err(FreeListError::ArithmeticOverflow)
        );
        assert_eq!(invalid_resize, before);

        let mut invalid_compaction = FreeList::new(1);
        invalid_compaction.free_blocks.clear();
        invalid_compaction.allocations.insert(
            0,
            AllocationRecord {
                count: 2,
                generation: 1,
                owner: 7,
            },
        );
        let before = invalid_compaction.clone();
        assert_eq!(
            invalid_compaction.compact(),
            Err(FreeListError::ArithmeticOverflow)
        );
        assert_eq!(invalid_compaction, before);
    }

    #[test]
    fn embedded_shader_uses_discrete_ao_mapping() {
        let shader = include_str!("shader.wgsl");
        let mapping = [
            "if (ao_raw == 3u)",
            "out.ao = 1.0",
            "else if (ao_raw == 2u)",
            "out.ao = 0.75",
            "else if (ao_raw == 1u)",
            "out.ao = 0.5",
            "out.ao = 0.25",
        ];
        let mut search_from = shader
            .find("let ao_raw =")
            .expect("terrain vertex shader must decode the packed AO bits");
        for branch in mapping {
            let branch_offset = shader[search_from..]
                .find(branch)
                .expect("terrain AO mapping must preserve each discrete level");
            search_from += branch_offset + branch.len();
        }
    }
}
