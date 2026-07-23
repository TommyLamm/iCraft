//! Pure CPU-side data and visibility helpers for chunk rendering.
//!
//! This module deliberately does not depend on `wgpu`. GPU buffer layouts and
//! uploads belong to the renderer, while the data below can also be produced by
//! background mesh workers and covered by ordinary unit tests.

use glam::{Mat4, Vec3, Vec4};
use std::collections::HashSet;

/// Vertex format used by terrain meshes.
///
/// `local_uv` is measured in block-texture repeats. `atlas_tile` is the
/// zero-based tile coordinate in the atlas, not an already-normalized UV. This
/// separation lets a greedy quad repeat a tile in the shader instead of
/// stretching one copy across the whole merged face.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct TerrainVertex {
    pub position: [f32; 3],
    pub local_uv: [f32; 2],
    pub atlas_tile: [f32; 2],
    /// Existing packed light value: sky light in the low nibble and block
    /// light in the next nibble.
    pub light_level: f32,
    pub ao: f32,
}

impl TerrainVertex {
    pub const fn new(
        position: [f32; 3],
        local_uv: [f32; 2],
        atlas_tile: [f32; 2],
        light_level: f32,
        ao: f32,
    ) -> Self {
        Self {
            position,
            local_uv,
            atlas_tile,
            light_level,
            ao,
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

    pub fn from_vertices(vertices: &[TerrainVertex]) -> Option<Self> {
        Self::from_points(vertices.iter().map(|vertex| Vec3::from(vertex.position)))
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
    pub fn new(vertices: Vec<TerrainVertex>, indices: Vec<u32>) -> Self {
        let bounds = MeshBounds::from_vertices(&vertices);
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
    ) -> Self {
        Self {
            opaque: ChunkMeshData::new(opaque_vertices, opaque_indices),
            transparent: ChunkMeshData::new(transparent_vertices, transparent_indices),
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
    pub bounds: MeshBounds,
    pub index_count: u32,
    pub layer: DrawLayer,
}

impl DrawCandidate {
    pub fn new(
        chunk_coord: (i32, i32),
        bounds: MeshBounds,
        index_count: u32,
        layer: DrawLayer,
    ) -> Self {
        Self {
            chunk_coord,
            bounds,
            index_count,
            layer,
        }
    }

    pub fn distance_squared_from(self, camera_position: Vec3) -> f32 {
        self.bounds.center_distance_squared(camera_position)
    }
}

/// Visible terrain draws split into the two required submission orders.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DrawPlan {
    pub opaque: Vec<DrawCandidate>,
    pub transparent: Vec<DrawCandidate>,
}

impl DrawPlan {
    pub fn build(
        candidates: impl IntoIterator<Item = DrawCandidate>,
        frustum: &Frustum,
        camera_position: Vec3,
    ) -> Self {
        let mut plan = Self::default();

        for candidate in candidates {
            if candidate.index_count == 0 || !frustum.intersects_aabb(&candidate.bounds) {
                continue;
            }
            match candidate.layer {
                DrawLayer::Opaque => plan.opaque.push(candidate),
                DrawLayer::Transparent => plan.transparent.push(candidate),
            }
        }

        plan.opaque.sort_by(|left, right| {
            left.distance_squared_from(camera_position)
                .total_cmp(&right.distance_squared_from(camera_position))
                .then_with(|| left.chunk_coord.cmp(&right.chunk_coord))
        });
        plan.transparent.sort_by(|left, right| {
            right
                .distance_squared_from(camera_position)
                .total_cmp(&left.distance_squared_from(camera_position))
                .then_with(|| left.chunk_coord.cmp(&right.chunk_coord))
        });

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
        self.opaque
            .iter()
            .chain(&self.transparent)
            .map(|candidate| candidate.chunk_coord)
            .collect::<HashSet<_>>()
            .len()
    }
}

pub fn build_draw_plan(
    candidates: impl IntoIterator<Item = DrawCandidate>,
    frustum: &Frustum,
    camera_position: Vec3,
) -> DrawPlan {
    DrawPlan::build(candidates, frustum, camera_position)
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
        bounds.center_distance_squared(camera_position).sqrt(),
        thresholds,
    )
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
        DrawCandidate::new(
            chunk_coord,
            bounds([-0.25, -0.25, z - 0.25], [0.25, 0.25, z + 0.25]),
            index_count,
            layer,
        )
    }

    #[test]
    fn terrain_vertex_is_pod_with_expected_cpu_layout() {
        fn assert_pod<T: bytemuck::Pod + bytemuck::Zeroable>() {}
        assert_pod::<TerrainVertex>();

        assert_eq!(size_of::<TerrainVertex>(), 36);
        assert_eq!(offset_of!(TerrainVertex, position), 0);
        assert_eq!(offset_of!(TerrainVertex, local_uv), 12);
        assert_eq!(offset_of!(TerrainVertex, atlas_tile), 20);
        assert_eq!(offset_of!(TerrainVertex, light_level), 28);
        assert_eq!(offset_of!(TerrainVertex, ao), 32);

        let vertex = TerrainVertex::new([1.0, 2.0, 3.0], [4.0, 5.0], [6.0, 7.0], 15.0, 0.75);
        assert_eq!(bytemuck::bytes_of(&vertex).len(), 36);
    }

    #[test]
    fn mesh_bounds_are_derived_from_actual_vertices() {
        let vertices = [
            TerrainVertex::new([4.0, 8.0, -2.0], [0.0; 2], [0.0; 2], 0.0, 1.0),
            TerrainVertex::new([-1.0, 3.0, 6.0], [0.0; 2], [0.0; 2], 0.0, 1.0),
            TerrainVertex::new([2.0, 12.0, 1.0], [0.0; 2], [0.0; 2], 0.0, 1.0),
        ];
        let mesh_bounds = MeshBounds::from_vertices(&vertices).unwrap();
        assert_eq!(mesh_bounds.min, Vec3::new(-1.0, 3.0, -2.0));
        assert_eq!(mesh_bounds.max, Vec3::new(4.0, 12.0, 6.0));
        assert_eq!(MeshBounds::from_vertices(&[]), None);
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
            TerrainVertex::new([0.0, 0.0, 0.0], [0.0; 2], [0.0; 2], 0.0, 1.0),
            TerrainVertex::new([1.0, 0.0, 0.0], [0.0; 2], [0.0; 2], 0.0, 1.0),
            TerrainVertex::new([0.0, 1.0, 0.0], [0.0; 2], [0.0; 2], 0.0, 1.0),
        ];
        let mesh = ChunkMeshData::new(vertices, vec![0, 1, 2]);
        assert!(!mesh.is_empty());
        assert_eq!(mesh.triangle_count(), 1);
        assert_eq!(mesh.bounds, Some(bounds([0.0, 0.0, 0.0], [1.0, 1.0, 0.0])));
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

        let plan = build_draw_plan(candidates, &frustum, Vec3::ZERO);
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
}
