#[cfg(test)]
use glam::Mat4;
use glam::Vec3;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::Arc;
use std::thread;

use crate::chunk_manager::ChunkManager;
use crate::chunk_render::{Frustum, MeshBounds};
use crate::entity::{Entity, EntityType};

/// Pairwise face connectivity bitmask for a 16x16x16 section.
/// Faces: 0 (+X), 1 (-X), 2 (+Y), 3 (-Y), 4 (+Z), 5 (-Z).
/// Bit (in_face * 6 + out_face) is 1 if there is a path through passable/transparent voxels.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SectionConnectivity {
    pub mask: u64,
}

impl Default for SectionConnectivity {
    fn default() -> Self {
        Self::FULL
    }
}

impl SectionConnectivity {
    pub const FULL: Self = Self { mask: u64::MAX };
    pub const NONE: Self = Self { mask: 0 };

    #[inline]
    pub fn is_connected(&self, in_face: u8, out_face: u8) -> bool {
        if in_face > 5 || out_face > 5 {
            return true;
        }
        let bit = (in_face as u64) * 6 + (out_face as u64);
        (self.mask & (1 << bit)) != 0
    }

    #[inline]
    pub fn set_connected(&mut self, in_face: u8, out_face: u8) {
        if in_face <= 5 && out_face <= 5 {
            let bit = (in_face as u64) * 6 + (out_face as u64);
            self.mask |= 1 << bit;
        }
    }
}

/// A dirty mesh must never expose connectivity computed for an older
/// revision. Invalid entries deliberately resolve to FULL so visibility
/// traversal fails open until the matching worker result is integrated.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SectionConnectivityState {
    Invalid,
    Valid(SectionConnectivity),
}

impl SectionConnectivityState {
    #[inline]
    pub fn fail_open(self) -> SectionConnectivity {
        match self {
            Self::Invalid => SectionConnectivity::FULL,
            Self::Valid(connectivity) => connectivity,
        }
    }
}

impl Default for SectionConnectivityState {
    fn default() -> Self {
        Self::Invalid
    }
}

/// Helper to check if a block type is a solid occluder for section visibility flood fill.
/// Conservative rules:
/// Opaque solid full-cubes act as occluders.
/// Glass, Leaves, Water, Lava, Cutout (flowers, saplings, torches, doors, trapdoors, ladders),
/// and Air are passable.
#[inline]
pub fn is_section_occluder(block: crate::world::BlockType) -> bool {
    use crate::world::BlockType;
    // Keep this allow-list deliberately conservative: only blocks whose model is
    // a complete opaque cube may seal a section. New block types fail open.
    matches!(
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
            | BlockType::Snow
            | BlockType::Clay
            | BlockType::Sandstone
            | BlockType::Obsidian
            | BlockType::BirchLog
            | BlockType::BirchPlanks
            | BlockType::SpruceLog
            | BlockType::SprucePlanks
            | BlockType::Pumpkin
            | BlockType::Melon
            | BlockType::Dispenser
            | BlockType::Dropper
            | BlockType::NoteBlock
            | BlockType::Netherrack
            | BlockType::SoulSand
            | BlockType::Glowstone
            | BlockType::EndStone
            | BlockType::Purpur
            | BlockType::NetherBrick
    )
}

/// Compute pairwise face connectivity for a 16x16x16 section inside a Chunk.
pub fn compute_section_connectivity(
    chunk: &crate::world::Chunk,
    sec_y: usize,
) -> SectionConnectivity {
    let base_y = sec_y * crate::world::SECTION_SIZE;
    compute_section_connectivity_with(|x, ly, z| chunk.get_block_local(x, base_y + ly, z))
}

/// Computes connectivity from the exact immutable halo used by a section
/// mesh worker. Core section voxels occupy halo coordinates 1..=16.
pub fn compute_section_connectivity_snapshot(
    snapshot: &crate::world::SectionHaloSnapshot,
) -> SectionConnectivity {
    compute_section_connectivity_with(|x, y, z| snapshot.get_block(x + 1, y + 1, z + 1))
}

fn compute_section_connectivity_with(
    mut block_at: impl FnMut(usize, usize, usize) -> crate::world::BlockType,
) -> SectionConnectivity {
    let mut any_passable = false;
    let mut any_occluder = false;
    let mut is_passable = [false; 4096];

    for ly in 0..crate::world::SECTION_SIZE {
        for z in 0..crate::world::SECTION_SIZE {
            for x in 0..crate::world::SECTION_SIZE {
                let block = block_at(x, ly, z);
                let occluder = is_section_occluder(block);
                let index = ly * 256 + z * 16 + x;
                is_passable[index] = !occluder;
                if occluder {
                    any_occluder = true;
                } else {
                    any_passable = true;
                }
            }
        }
    }

    if !any_occluder {
        return SectionConnectivity::FULL;
    }
    if !any_passable {
        return SectionConnectivity::NONE;
    }

    let mut visited = [false; 4096];
    let mut connectivity = SectionConnectivity { mask: 0 };
    let mut queue = Vec::with_capacity(256);

    for start_idx in 0..4096 {
        if !is_passable[start_idx] || visited[start_idx] {
            continue;
        }

        visited[start_idx] = true;
        queue.clear();
        queue.push(start_idx);

        let mut touched_faces = 0u8;
        let mut head = 0;

        while head < queue.len() {
            let idx = queue[head];
            head += 1;

            let ly = idx / 256;
            let rem = idx % 256;
            let z = rem / 16;
            let x = rem % 16;

            if x == 15 {
                touched_faces |= 1 << 0;
            } // +X
            if x == 0 {
                touched_faces |= 1 << 1;
            } // -X
            if ly == 15 {
                touched_faces |= 1 << 2;
            } // +Y
            if ly == 0 {
                touched_faces |= 1 << 3;
            } // -Y
            if z == 15 {
                touched_faces |= 1 << 4;
            } // +Z
            if z == 0 {
                touched_faces |= 1 << 5;
            } // -Z

            let neighbors = [
                if x < 15 { Some(idx + 1) } else { None },
                if x > 0 { Some(idx - 1) } else { None },
                if ly < 15 { Some(idx + 256) } else { None },
                if ly > 0 { Some(idx - 256) } else { None },
                if z < 15 { Some(idx + 16) } else { None },
                if z > 0 { Some(idx - 16) } else { None },
            ];

            for neighbor in neighbors.into_iter().flatten() {
                if is_passable[neighbor] && !visited[neighbor] {
                    visited[neighbor] = true;
                    queue.push(neighbor);
                }
            }
        }

        for f1 in 0..6u8 {
            if (touched_faces & (1 << f1)) != 0 {
                for f2 in 0..6u8 {
                    if (touched_faces & (1 << f2)) != 0 {
                        connectivity.set_connected(f1, f2);
                    }
                }
            }
        }
    }

    connectivity
}

#[derive(Debug)]
struct SectionNode {
    x: i32,
    sec_y: usize,
    z: i32,
    entry_face: Option<u8>,
}

/// Reusable temporary storage for section visibility traversal.
///
/// The visibility set itself remains caller-owned because the render pass
/// queries it after traversal. This scratch owns only the queue and per-entry
/// visitation masks that used to be allocated afresh for every frame.
#[derive(Debug, Default)]
pub struct SectionVisibilityScratch {
    visited_entry: HashMap<(i32, usize, i32), u8>,
    queue: VecDeque<SectionNode>,
}

impl SectionVisibilityScratch {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self::default()
    }

    /// Pre-reserve traversal storage when the render-distance budget is known.
    pub fn with_capacity(visited_capacity: usize, queue_capacity: usize) -> Self {
        Self {
            visited_entry: HashMap::with_capacity(visited_capacity),
            queue: VecDeque::with_capacity(queue_capacity),
        }
    }

    #[cfg(test)]
    fn capacities(&self) -> (usize, usize) {
        (self.visited_entry.capacity(), self.queue.capacity())
    }
}

/// Perform bounded Graph Traversal starting from camera section.
/// Returns a set of visible section coordinates `(x, sec_y, z)`.
pub fn traverse_section_visibility<F>(
    cam_sec_x: i32,
    cam_sec_y: usize,
    cam_sec_z: i32,
    render_distance: i32,
    frustum: &Frustum,
    get_connectivity: F,
    visible_sections: &mut HashSet<(i32, usize, i32)>,
) where
    F: Fn(i32, usize, i32) -> Option<SectionConnectivity>,
{
    let mut scratch = SectionVisibilityScratch::default();
    traverse_section_visibility_with_scratch(
        cam_sec_x,
        cam_sec_y,
        cam_sec_z,
        render_distance,
        frustum,
        get_connectivity,
        visible_sections,
        &mut scratch,
    );
}

/// Perform bounded section visibility traversal using caller-owned scratch.
///
/// Callers that invoke this once per frame should retain one
/// [`SectionVisibilityScratch`] and pass it back on every call. Its internal
/// `HashMap` and `VecDeque` then retain their peak capacities, so steady-state
/// traversal does not allocate.
pub fn traverse_section_visibility_with_scratch<F>(
    cam_sec_x: i32,
    cam_sec_y: usize,
    cam_sec_z: i32,
    render_distance: i32,
    frustum: &Frustum,
    get_connectivity: F,
    visible_sections: &mut HashSet<(i32, usize, i32)>,
    scratch: &mut SectionVisibilityScratch,
) where
    F: Fn(i32, usize, i32) -> Option<SectionConnectivity>,
{
    visible_sections.clear();
    scratch.visited_entry.clear();
    scratch.queue.clear();

    let start = (cam_sec_x, cam_sec_y, cam_sec_z);
    visible_sections.insert(start);
    scratch.visited_entry.insert(start, 0x3F);

    scratch.queue.push_back(SectionNode {
        x: cam_sec_x,
        sec_y: cam_sec_y,
        z: cam_sec_z,
        entry_face: None,
    });

    while let Some(node) = scratch.queue.pop_front() {
        let connectivity =
            get_connectivity(node.x, node.sec_y, node.z).unwrap_or(SectionConnectivity::FULL);

        for out_face in 0..6u8 {
            if node
                .entry_face
                .map_or(true, |in_f| connectivity.is_connected(in_f, out_face))
            {
                let (target_x, target_y_raw, target_z, opposite_entry) = match out_face {
                    0 => (node.x + 1, node.sec_y as i32, node.z, 1u8),
                    1 => (node.x - 1, node.sec_y as i32, node.z, 0u8),
                    2 => (node.x, node.sec_y as i32 + 1, node.z, 3u8),
                    3 => (node.x, node.sec_y as i32 - 1, node.z, 2u8),
                    4 => (node.x, node.sec_y as i32, node.z + 1, 5u8),
                    5 => (node.x, node.sec_y as i32, node.z - 1, 4u8),
                    _ => continue,
                };

                if target_y_raw < 0 || target_y_raw >= 16 {
                    continue;
                }
                let target_sec_y = target_y_raw as usize;

                if (target_x - cam_sec_x).abs() > render_distance
                    || (target_z - cam_sec_z).abs() > render_distance
                {
                    continue;
                }

                let min_pos = Vec3::new(
                    target_x as f32 * 16.0,
                    target_sec_y as f32 * 16.0,
                    target_z as f32 * 16.0,
                );
                let bounds = MeshBounds::new(min_pos, min_pos + Vec3::splat(16.0));
                if !frustum.intersects_aabb(&bounds) {
                    continue;
                }

                let target_key = (target_x, target_sec_y, target_z);
                visible_sections.insert(target_key);

                let entry_mask = scratch.visited_entry.entry(target_key).or_insert(0u8);
                if (*entry_mask & (1 << opposite_entry)) == 0 {
                    *entry_mask |= 1 << opposite_entry;
                    scratch.queue.push_back(SectionNode {
                        x: target_x,
                        sec_y: target_sec_y,
                        z: target_z,
                        entry_face: Some(opposite_entry),
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_connectivity_fails_open() {
        let connectivity = SectionConnectivityState::Invalid.fail_open();
        for in_face in 0..6 {
            for out_face in 0..6 {
                assert!(connectivity.is_connected(in_face, out_face));
            }
        }
    }

    #[test]
    fn valid_connectivity_preserves_the_computed_mask() {
        let state = SectionConnectivityState::Valid(SectionConnectivity::NONE);
        assert_eq!(state.fail_open(), SectionConnectivity::NONE);
    }

    #[test]
    fn transparent_and_partial_blocks_fail_open() {
        use crate::world::BlockType;
        for block in [
            BlockType::Air,
            BlockType::Glass,
            BlockType::Ice,
            BlockType::Water,
            BlockType::Lava,
            BlockType::OakLeaves,
            BlockType::BirchLeaves,
            BlockType::SpruceLeaves,
            BlockType::Torch,
            BlockType::OakDoor,
            BlockType::OakTrapdoor,
            BlockType::Chest,
            BlockType::Cactus,
            BlockType::TallGrass,
            BlockType::EndPortal,
            BlockType::NetherPortal,
            BlockType::Fire,
            BlockType::DragonEgg,
            BlockType::BrewingStand,
        ] {
            assert!(!is_section_occluder(block), "{block:?} must fail open");
        }
    }

    #[test]
    fn opaque_cube_blocks_los() {
        assert!(is_los_blocked(
            Vec3::new(0.2, 1.2, 0.2),
            Vec3::new(3.8, 1.2, 0.2),
            |x, y, z| {
                (x, y, z) == (1, 1, 0) && is_section_occluder(crate::world::BlockType::Stone)
            }
        ));
    }

    #[test]
    fn section_visibility_reuses_traversal_scratch_capacity() {
        let frustum = Frustum::from_view_projection(Mat4::orthographic_lh(
            -64.0, 64.0, -64.0, 64.0, 0.0, 128.0,
        ));
        let mut visible_sections = HashSet::with_capacity(512);
        let mut scratch = SectionVisibilityScratch::with_capacity(512, 512);

        traverse_section_visibility_with_scratch(
            0,
            0,
            0,
            1,
            &frustum,
            |_, _, _| Some(SectionConnectivity::FULL),
            &mut visible_sections,
            &mut scratch,
        );
        let first_len = visible_sections.len();
        let visible_capacity = visible_sections.capacity();
        let scratch_capacities = scratch.capacities();

        for _ in 0..8 {
            traverse_section_visibility_with_scratch(
                0,
                0,
                0,
                1,
                &frustum,
                |_, _, _| Some(SectionConnectivity::FULL),
                &mut visible_sections,
                &mut scratch,
            );
            assert_eq!(visible_sections.len(), first_len);
        }

        assert!(first_len > 1);
        assert_eq!(visible_sections.capacity(), visible_capacity);
        assert_eq!(scratch.capacities(), scratch_capacities);
    }
}

/// Fast 3D DDA voxel line-of-sight raycast.
pub fn is_los_blocked<F>(origin: Vec3, target: Vec3, mut is_occluder: F) -> bool
where
    F: FnMut(i32, i32, i32) -> bool,
{
    let dir = target - origin;
    let dist = dir.length();
    if dist < 0.001 {
        return false;
    }
    let norm = dir / dist;

    let mut x = origin.x.floor() as i32;
    let mut y = origin.y.floor() as i32;
    let mut z = origin.z.floor() as i32;

    let target_x = target.x.floor() as i32;
    let target_y = target.y.floor() as i32;
    let target_z = target.z.floor() as i32;

    let step_x = if norm.x > 0.0 {
        1
    } else if norm.x < 0.0 {
        -1
    } else {
        0
    };
    let step_y = if norm.y > 0.0 {
        1
    } else if norm.y < 0.0 {
        -1
    } else {
        0
    };
    let step_z = if norm.z > 0.0 {
        1
    } else if norm.z < 0.0 {
        -1
    } else {
        0
    };

    let delta_x = if step_x != 0 {
        (1.0 / norm.x.abs()).min(100.0)
    } else {
        100.0
    };
    let delta_y = if step_y != 0 {
        (1.0 / norm.y.abs()).min(100.0)
    } else {
        100.0
    };
    let delta_z = if step_z != 0 {
        (1.0 / norm.z.abs()).min(100.0)
    } else {
        100.0
    };

    let mut t_max_x = if step_x > 0 {
        (x as f32 + 1.0 - origin.x) * delta_x
    } else if step_x < 0 {
        (origin.x - x as f32) * delta_x
    } else {
        f32::INFINITY
    };
    let mut t_max_y = if step_y > 0 {
        (y as f32 + 1.0 - origin.y) * delta_y
    } else if step_y < 0 {
        (origin.y - y as f32) * delta_y
    } else {
        f32::INFINITY
    };
    let mut t_max_z = if step_z > 0 {
        (z as f32 + 1.0 - origin.z) * delta_z
    } else if step_z < 0 {
        (origin.z - z as f32) * delta_z
    } else {
        f32::INFINITY
    };

    let start_x = x;
    let start_y = y;
    let start_z = z;

    for _ in 0..48 {
        if x == target_x && y == target_y && z == target_z {
            return false;
        }

        if !(x == start_x && y == start_y && z == start_z) {
            if is_occluder(x, y, z) {
                return true;
            }
        }

        if t_max_x < t_max_y {
            if t_max_x < t_max_z {
                x += step_x;
                t_max_x += delta_x;
            } else {
                z += step_z;
                t_max_z += delta_z;
            }
        } else {
            if t_max_y < t_max_z {
                y += step_y;
                t_max_y += delta_y;
            } else {
                z += step_z;
                t_max_z += delta_z;
            }
        }
    }

    false
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LosIdentity {
    pub dimension: crate::dimension::Dimension,
    pub generation: u64,
    /// Monotonic world-mesh revision. Any terrain mutation changes this token,
    /// conservatively invalidating LOS snapshots before a stale result can
    /// enter the cache.
    pub world_revision: u64,
}

/// Terrain identity for one loaded chunk intersected by an LOS snapshot.
///
/// Revision zero is a real identity for an as-yet-unmodified loaded chunk.
/// The coordinate remains in the list so unloading that chunk invalidates the
/// snapshot even when it has never been dirty.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LosChunkRevision {
    pub coord: (i32, i32),
    pub revision: u64,
}

#[derive(Clone, Debug)]
pub struct VoxelOcclusionSnapshot {
    pub identity: LosIdentity,
    pub chunk_revisions: Arc<[LosChunkRevision]>,
    pub voxels: HashMap<(i32, i32, i32), crate::world::BlockType>,
}

impl VoxelOcclusionSnapshot {
    pub const MAX_VOXELS: usize = 16_384;
    pub fn is_occluder(&self, x: i32, y: i32, z: i32) -> bool {
        self.voxels
            .get(&(x, y, z))
            .copied()
            .map(is_section_occluder)
            .unwrap_or(false)
    }
}

#[derive(Clone)]
pub struct EntityLosRequest {
    pub entity_id: u64,
    pub nonce: u64,
    pub epoch: u64,
    pub camera_pos: Vec3,
    pub target_pos: Vec3,
    pub camera_cell: (i32, i32, i32),
    pub target_cell: (i32, i32, i32),
    pub snapshot: Arc<VoxelOcclusionSnapshot>,
    pub identity: LosIdentity,
}

#[derive(Clone)]
pub struct EntityLosResult {
    pub entity_id: u64,
    pub nonce: u64,
    pub epoch: u64,
    pub camera_cell: (i32, i32, i32),
    pub target_cell: (i32, i32, i32),
    pub is_visible: bool,
    pub identity: LosIdentity,
    pub chunk_revisions: Arc<[LosChunkRevision]>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CullingCounters {
    pub distance: u64,
    pub frustum: u64,
    pub section: u64,
    pub los: u64,
    pub fail_open: u64,
    pub stale: u64,
    pub timeouts: u64,
    pub overflow: u64,
}

pub const ENTITY_LOS_QUEUE_CAPACITY: usize = 64;
pub const ENTITY_LOS_REQUEST_TIMEOUT_FRAMES: u64 = 30;
const ENTITY_LOS_CACHE_TTL_FRAMES: u8 = 30;
const ENTITY_LOS_HYSTERESIS_RESULTS: u8 = 3;

#[derive(Clone)]
struct LosCacheEntry {
    camera_cell: (i32, i32, i32),
    target_cell: (i32, i32, i32),
    is_visible: bool,
    ttl: u8,
    hysteresis_count: u8,
    identity: LosIdentity,
    chunk_revisions: Arc<[LosChunkRevision]>,
}

#[derive(Clone)]
struct EntityLosInFlight {
    nonce: u64,
    epoch: u64,
    camera_cell: (i32, i32, i32),
    target_cell: (i32, i32, i32),
    identity: LosIdentity,
    chunk_revisions: Arc<[LosChunkRevision]>,
}

pub struct EntityLosManager {
    request_tx: SyncSender<EntityLosRequest>,
    result_rx: Receiver<EntityLosResult>,
    cache: HashMap<u64, LosCacheEntry>,
    in_flight: HashMap<u64, EntityLosInFlight>,
    current_identity: Option<LosIdentity>,
    frame_epoch: u64,
    next_nonce: u64,
    request_capacity: usize,
    pub counters: CullingCounters,
}

impl EntityLosManager {
    pub fn new() -> Self {
        let (req_tx, req_rx) = sync_channel::<EntityLosRequest>(ENTITY_LOS_QUEUE_CAPACITY);
        let (res_tx, res_rx) = sync_channel::<EntityLosResult>(ENTITY_LOS_QUEUE_CAPACITY);

        thread::Builder::new()
            .name("entity_los_worker".to_string())
            .spawn(move || {
                while let Ok(req) = req_rx.recv() {
                    if res_tx.send(evaluate_los_request(req)).is_err() {
                        break;
                    }
                }
            })
            .expect("Failed to spawn entity_los_worker thread");

        Self::with_channels(req_tx, res_rx, ENTITY_LOS_QUEUE_CAPACITY)
    }

    fn with_channels(
        request_tx: SyncSender<EntityLosRequest>,
        result_rx: Receiver<EntityLosResult>,
        request_capacity: usize,
    ) -> Self {
        Self {
            request_tx,
            result_rx,
            cache: HashMap::new(),
            in_flight: HashMap::with_capacity(request_capacity),
            current_identity: None,
            frame_epoch: 0,
            next_nonce: 1,
            request_capacity,
            counters: CullingCounters::default(),
        }
    }

    pub fn set_current_identity(&mut self, identity: LosIdentity) {
        if self.current_identity.as_ref() != Some(&identity) {
            self.cache.clear();
            // Queued work cannot be cancelled from std::mpsc, but dropping its
            // logical ownership guarantees that its result is rejected.
            self.in_flight.clear();
        }
        self.current_identity = Some(identity);
    }

    pub fn in_flight_count(&self) -> usize {
        self.in_flight.len()
    }

    pub fn request_capacity(&self) -> usize {
        self.request_capacity
    }

    fn make_snapshot(
        &mut self,
        cam_pos: Vec3,
        target_pos: Vec3,
        manager: &ChunkManager,
    ) -> Option<Arc<VoxelOcclusionSnapshot>> {
        let identity = match self.current_identity.clone() {
            Some(identity) if identity.dimension == manager.dimension => identity,
            Some(_) => {
                self.counters.fail_open = self.counters.fail_open.saturating_add(1);
                return None;
            }
            None => {
                let identity = LosIdentity {
                    dimension: manager.dimension,
                    // DirtyChunkSet ids are unique per manager lifetime and
                    // provide a safe identity for unit/standalone callers that
                    // do not install State's terrain generation token.
                    generation: manager.dirty_chunks.id(),
                    world_revision: 0,
                };
                self.current_identity = Some(identity.clone());
                identity
            }
        };

        let Some(camera_cell) = voxel_cell(cam_pos) else {
            self.counters.fail_open = self.counters.fail_open.saturating_add(1);
            return None;
        };
        let Some(target_cell) = voxel_cell(target_pos) else {
            self.counters.fail_open = self.counters.fail_open.saturating_add(1);
            return None;
        };
        let min = (
            camera_cell.0.min(target_cell.0),
            camera_cell.1.min(target_cell.1),
            camera_cell.2.min(target_cell.2),
        );
        let max = (
            camera_cell.0.max(target_cell.0),
            camera_cell.1.max(target_cell.1),
            camera_cell.2.max(target_cell.2),
        );
        if min.1 < 0 || max.1 >= crate::world::CHUNK_HEIGHT as i32 {
            self.counters.fail_open = self.counters.fail_open.saturating_add(1);
            return None;
        }

        let width = i64::from(max.0) - i64::from(min.0) + 1;
        let height = i64::from(max.1) - i64::from(min.1) + 1;
        let depth = i64::from(max.2) - i64::from(min.2) + 1;
        let volume = usize::try_from(width)
            .ok()
            .and_then(|width| {
                usize::try_from(height)
                    .ok()
                    .and_then(|height| width.checked_mul(height))
            })
            .and_then(|area| {
                usize::try_from(depth)
                    .ok()
                    .and_then(|depth| area.checked_mul(depth))
            });
        let Some(volume) = volume.filter(|&volume| volume <= VoxelOcclusionSnapshot::MAX_VOXELS)
        else {
            self.counters.fail_open = self.counters.fail_open.saturating_add(1);
            return None;
        };

        let min_cx = min.0.div_euclid(crate::world::CHUNK_WIDTH as i32);
        let max_cx = max.0.div_euclid(crate::world::CHUNK_WIDTH as i32);
        let min_cz = min.2.div_euclid(crate::world::CHUNK_DEPTH as i32);
        let max_cz = max.2.div_euclid(crate::world::CHUNK_DEPTH as i32);
        let mut chunk_revisions = Vec::new();
        for cx in min_cx..=max_cx {
            for cz in min_cz..=max_cz {
                let Some(revision) = loaded_chunk_revision(manager, (cx, cz)) else {
                    self.counters.fail_open = self.counters.fail_open.saturating_add(1);
                    return None;
                };
                chunk_revisions.push(LosChunkRevision {
                    coord: (cx, cz),
                    revision,
                });
            }
        }

        let mut voxels = HashMap::with_capacity(volume);
        for x in min.0..=max.0 {
            for y in min.1..=max.1 {
                for z in min.2..=max.2 {
                    let Some(block) = manager.get_loaded_block(x, y, z) else {
                        self.counters.fail_open = self.counters.fail_open.saturating_add(1);
                        return None;
                    };
                    voxels.insert((x, y, z), block);
                }
            }
        }
        Some(Arc::new(VoxelOcclusionSnapshot {
            identity,
            chunk_revisions: chunk_revisions.into(),
            voxels,
        }))
    }

    pub fn poll_results(&mut self) {
        self.frame_epoch = self.frame_epoch.wrapping_add(1);
        self.expire_timeouts();
        while let Ok(res) = self.result_rx.try_recv() {
            self.integrate_result(res);
        }
    }

    fn expire_timeouts(&mut self) {
        let frame_epoch = self.frame_epoch;
        let cache = &mut self.cache;
        let mut timed_out = 0u64;
        self.in_flight.retain(|entity_id, request| {
            if frame_epoch.wrapping_sub(request.epoch) >= ENTITY_LOS_REQUEST_TIMEOUT_FRAMES {
                cache.remove(entity_id);
                timed_out = timed_out.saturating_add(1);
                false
            } else {
                true
            }
        });
        self.counters.timeouts = self.counters.timeouts.saturating_add(timed_out);
        self.counters.fail_open = self.counters.fail_open.saturating_add(timed_out);
    }

    fn integrate_result(&mut self, res: EntityLosResult) {
        if self.current_identity.as_ref() != Some(&res.identity) {
            self.counters.stale = self.counters.stale.saturating_add(1);
            return;
        }
        let matches_current_request = self.in_flight.get(&res.entity_id).is_some_and(|request| {
            request.nonce == res.nonce
                && request.epoch == res.epoch
                && request.camera_cell == res.camera_cell
                && request.target_cell == res.target_cell
                && request.identity == res.identity
                && request.chunk_revisions.as_ref() == res.chunk_revisions.as_ref()
        });
        if !matches_current_request {
            self.counters.stale = self.counters.stale.saturating_add(1);
            return;
        }
        self.in_flight.remove(&res.entity_id);

        let previous_hysteresis = self
            .cache
            .get(&res.entity_id)
            .filter(|entry| {
                entry.camera_cell == res.camera_cell
                    && entry.target_cell == res.target_cell
                    && entry.identity == res.identity
                    && entry.chunk_revisions.as_ref() == res.chunk_revisions.as_ref()
            })
            .map_or(0, |entry| entry.hysteresis_count);
        let hysteresis_count = if res.is_visible {
            0
        } else {
            previous_hysteresis.saturating_add(1)
        };
        self.cache.insert(
            res.entity_id,
            LosCacheEntry {
                camera_cell: res.camera_cell,
                target_cell: res.target_cell,
                is_visible: res.is_visible,
                ttl: ENTITY_LOS_CACHE_TTL_FRAMES,
                hysteresis_count,
                identity: res.identity,
                chunk_revisions: res.chunk_revisions,
            },
        );
    }

    pub fn is_entity_visible(
        &mut self,
        entity: &Entity,
        cam_pos: Vec3,
        cam_cell: (i32, i32, i32),
        chunk_manager: &ChunkManager,
    ) -> bool {
        let dist_sq = entity.position.distance_squared(cam_pos);
        if dist_sq <= 16.0 * 16.0 {
            self.fail_open_entity(entity.id);
            return true;
        }

        if entity.entity_type.is_projectile()
            || matches!(
                entity.entity_type,
                EntityType::EnderDragon
                    | EntityType::Wither
                    | EntityType::EndCrystal
                    | EntityType::HeartParticle
                    | EntityType::DroppedItem
            )
        {
            self.fail_open_entity(entity.id);
            return true;
        }

        if entity.entity_type == EntityType::RemotePlayer && dist_sq <= 32.0 * 32.0 {
            self.fail_open_entity(entity.id);
            return true;
        }

        let target_pos = entity.position + Vec3::new(0.0, 0.8, 0.0);
        let Some(actual_camera_cell) = voxel_cell(cam_pos) else {
            self.fail_open_entity(entity.id);
            return true;
        };
        let Some(target_cell) = voxel_cell(target_pos) else {
            self.fail_open_entity(entity.id);
            return true;
        };
        if actual_camera_cell != cam_cell {
            self.fail_open_entity(entity.id);
            return true;
        }

        let mut invalid_cache = false;
        if let Some(entry) = self.cache.get_mut(&entity.id) {
            let identity_matches = self.current_identity.as_ref() == Some(&entry.identity);
            let chunks_match = chunk_revisions_match(chunk_manager, entry.chunk_revisions.as_ref());
            if entry.camera_cell != cam_cell
                || entry.target_cell != target_cell
                || !identity_matches
                || !chunks_match
            {
                invalid_cache = true;
            } else if entry.ttl > 0 {
                entry.ttl -= 1;
                if !entry.is_visible && entry.hysteresis_count >= ENTITY_LOS_HYSTERESIS_RESULTS {
                    self.counters.los = self.counters.los.saturating_add(1);
                    return false;
                }
                return true;
            }
        }
        if invalid_cache {
            self.cache.remove(&entity.id);
            self.in_flight.remove(&entity.id);
            self.counters.fail_open = self.counters.fail_open.saturating_add(1);
        }

        let matching_request = self.in_flight.get(&entity.id).is_some_and(|request| {
            request.camera_cell == cam_cell
                && request.target_cell == target_cell
                && self.current_identity.as_ref() == Some(&request.identity)
                && chunk_revisions_match(chunk_manager, request.chunk_revisions.as_ref())
        });
        if matching_request {
            self.counters.fail_open = self.counters.fail_open.saturating_add(1);
            return true;
        }
        // A teleport or terrain identity change supersedes the entity's prior
        // logical request. Its queued result will be rejected by nonce/epoch.
        self.in_flight.remove(&entity.id);

        let Some(snapshot) = self.make_snapshot(cam_pos, target_pos, chunk_manager) else {
            self.cache.remove(&entity.id);
            return true;
        };

        if self.in_flight.len() >= self.request_capacity {
            self.counters.overflow = self.counters.overflow.saturating_add(1);
            self.fail_open_entity(entity.id);
            return true;
        }

        let nonce = self.next_nonce;
        self.next_nonce = self.next_nonce.wrapping_add(1).max(1);
        let in_flight = EntityLosInFlight {
            nonce,
            epoch: self.frame_epoch,
            camera_cell: cam_cell,
            target_cell,
            identity: snapshot.identity.clone(),
            chunk_revisions: Arc::clone(&snapshot.chunk_revisions),
        };
        let request = EntityLosRequest {
            entity_id: entity.id,
            nonce,
            epoch: self.frame_epoch,
            camera_pos: cam_pos,
            target_pos,
            camera_cell: cam_cell,
            target_cell,
            identity: snapshot.identity.clone(),
            snapshot,
        };
        match self.request_tx.try_send(request) {
            Ok(()) => {
                self.in_flight.insert(entity.id, in_flight);
            }
            Err(TrySendError::Full(_)) => {
                self.counters.overflow = self.counters.overflow.saturating_add(1);
                self.cache.remove(&entity.id);
            }
            Err(TrySendError::Disconnected(_)) => {
                self.cache.remove(&entity.id);
            }
        }
        self.counters.fail_open = self.counters.fail_open.saturating_add(1);
        true
    }

    fn fail_open_entity(&mut self, entity_id: u64) {
        self.cache.remove(&entity_id);
        self.in_flight.remove(&entity_id);
        self.counters.fail_open = self.counters.fail_open.saturating_add(1);
    }
}

impl Default for EntityLosManager {
    fn default() -> Self {
        Self::new()
    }
}

fn evaluate_los_request(req: EntityLosRequest) -> EntityLosResult {
    let blocked = is_los_blocked(req.camera_pos, req.target_pos, |x, y, z| {
        req.snapshot.is_occluder(x, y, z)
    });
    EntityLosResult {
        entity_id: req.entity_id,
        nonce: req.nonce,
        epoch: req.epoch,
        camera_cell: req.camera_cell,
        target_cell: req.target_cell,
        is_visible: !blocked,
        identity: req.identity,
        chunk_revisions: Arc::clone(&req.snapshot.chunk_revisions),
    }
}

fn voxel_cell(position: Vec3) -> Option<(i32, i32, i32)> {
    fn component(value: f32) -> Option<i32> {
        if !value.is_finite() {
            return None;
        }
        let value = f64::from(value).floor();
        // Leave one cell of arithmetic headroom for the DDA's step.
        if value <= f64::from(i32::MIN) || value >= f64::from(i32::MAX) {
            None
        } else {
            Some(value as i32)
        }
    }
    Some((
        component(position.x)?,
        component(position.y)?,
        component(position.z)?,
    ))
}

fn loaded_chunk_revision(manager: &ChunkManager, coord: (i32, i32)) -> Option<u64> {
    manager.chunks.get(&coord)?;
    Some(match manager.dirty_chunks.state(coord.0, coord.1) {
        Some(crate::save::SaveState::Dirty(revision))
        | Some(crate::save::SaveState::InFlight(revision))
        | Some(crate::save::SaveState::Persisted(revision)) => revision,
        None => 0,
    })
}

fn chunk_revisions_match(manager: &ChunkManager, revisions: &[LosChunkRevision]) -> bool {
    revisions
        .iter()
        .all(|expected| loaded_chunk_revision(manager, expected.coord) == Some(expected.revision))
}

#[cfg(test)]
mod entity_los_tests {
    use super::*;
    use crate::world::{BlockType, Chunk};
    use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
    use std::time::Duration;

    const CAMERA: Vec3 = Vec3::new(0.2, 200.2, 0.2);
    const CAMERA_CELL: (i32, i32, i32) = (0, 200, 0);

    fn loaded_manager(max_cx: i32) -> ChunkManager {
        let mut manager = ChunkManager::new(4);
        for cx in 0..=max_cx {
            manager.chunks.insert((cx, 0), Chunk::new(cx, 0));
        }
        manager
    }

    fn entity(id: u64, entity_type: EntityType, x: f32) -> Entity {
        Entity::new(id, entity_type, Vec3::new(x, 200.2, 0.2))
    }

    fn harness(
        capacity: usize,
    ) -> (
        EntityLosManager,
        Receiver<EntityLosRequest>,
        SyncSender<EntityLosResult>,
    ) {
        let (request_tx, request_rx) = sync_channel(capacity);
        let (result_tx, result_rx) = sync_channel(capacity.max(1));
        (
            EntityLosManager::with_channels(request_tx, result_rx, capacity),
            request_rx,
            result_tx,
        )
    }

    fn receive_request(request_rx: &Receiver<EntityLosRequest>) -> EntityLosRequest {
        request_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("LOS request should be queued without synchronous evaluation")
    }

    fn complete_request(
        manager: &mut EntityLosManager,
        request_rx: &Receiver<EntityLosRequest>,
        result_tx: &SyncSender<EntityLosResult>,
    ) -> EntityLosResult {
        let result = evaluate_los_request(receive_request(request_rx));
        result_tx.send(result.clone()).unwrap();
        manager.poll_results();
        result
    }

    #[test]
    fn opaque_wall_eventually_culls_and_removal_fails_open_then_refreshes() {
        let mut chunks = loaded_manager(1);
        chunks.set_block(10, 200, 0, BlockType::Stone);
        let mob = entity(7, EntityType::Zombie, 20.8);
        let (mut los, request_rx, result_tx) = harness(8);

        for completed in 1..=ENTITY_LOS_HYSTERESIS_RESULTS {
            assert!(
                los.is_entity_visible(&mob, CAMERA, CAMERA_CELL, &chunks),
                "an async result must never cull on the submitting call"
            );
            let result = complete_request(&mut los, &request_rx, &result_tx);
            assert!(!result.is_visible);
            if completed == 1 {
                assert_eq!(
                    result.chunk_revisions.as_ref(),
                    &[
                        LosChunkRevision {
                            coord: (0, 0),
                            revision: 1,
                        },
                        LosChunkRevision {
                            coord: (1, 0),
                            revision: 0,
                        },
                    ]
                );
            }
            if completed < ENTITY_LOS_HYSTERESIS_RESULTS {
                los.cache.get_mut(&mob.id).unwrap().ttl = 0;
            }
        }

        assert!(!los.is_entity_visible(&mob, CAMERA, CAMERA_CELL, &chunks));
        assert_eq!(los.counters.los, 1);

        chunks.set_block(10, 200, 0, BlockType::Air);
        assert!(
            los.is_entity_visible(&mob, CAMERA, CAMERA_CELL, &chunks),
            "a changed chunk identity must immediately fail open"
        );
        let refreshed = complete_request(&mut los, &request_rx, &result_tx);
        assert!(refreshed.is_visible);
        assert!(los.is_entity_visible(&mob, CAMERA, CAMERA_CELL, &chunks));
        assert_eq!(los.cache.get(&mob.id).unwrap().hysteresis_count, 0);
    }

    #[test]
    fn nonce_cell_and_world_stale_results_are_rejected() {
        let chunks = loaded_manager(1);
        let mob = entity(9, EntityType::Zombie, 20.8);
        let (mut los, request_rx, result_tx) = harness(4);
        assert!(los.is_entity_visible(&mob, CAMERA, CAMERA_CELL, &chunks));
        let request = receive_request(&request_rx);

        let mut stale_nonce = evaluate_los_request(request.clone());
        stale_nonce.nonce = stale_nonce.nonce.wrapping_add(1);
        result_tx.send(stale_nonce).unwrap();
        los.poll_results();
        assert_eq!(los.in_flight_count(), 1);

        let mut stale_cell = evaluate_los_request(request.clone());
        stale_cell.target_cell.0 += 1;
        result_tx.send(stale_cell).unwrap();
        los.poll_results();
        assert_eq!(los.in_flight_count(), 1);

        let next_identity = LosIdentity {
            world_revision: request.identity.world_revision.wrapping_add(1),
            ..request.identity.clone()
        };
        los.set_current_identity(next_identity);
        result_tx.send(evaluate_los_request(request)).unwrap();
        los.poll_results();

        assert_eq!(los.counters.stale, 3);
        assert_eq!(los.in_flight_count(), 0);
        assert!(!los.cache.contains_key(&mob.id));
    }

    #[test]
    fn teleport_supersedes_old_request_and_stale_result_cannot_cull() {
        let chunks = loaded_manager(2);
        let mut mob = entity(11, EntityType::Zombie, 20.8);
        let (mut los, request_rx, result_tx) = harness(4);
        assert!(los.is_entity_visible(&mob, CAMERA, CAMERA_CELL, &chunks));
        let old_request = receive_request(&request_rx);

        mob.position.x = 35.8;
        assert!(los.is_entity_visible(&mob, CAMERA, CAMERA_CELL, &chunks));
        let current_request = receive_request(&request_rx);
        assert_ne!(old_request.nonce, current_request.nonce);
        assert_ne!(old_request.target_cell, current_request.target_cell);

        result_tx.send(evaluate_los_request(old_request)).unwrap();
        los.poll_results();
        assert_eq!(los.counters.stale, 1);
        assert_eq!(los.in_flight_count(), 1);
        assert!(los.is_entity_visible(&mob, CAMERA, CAMERA_CELL, &chunks));

        result_tx
            .send(evaluate_los_request(current_request))
            .unwrap();
        los.poll_results();
        assert!(los.is_entity_visible(&mob, CAMERA, CAMERA_CELL, &chunks));
    }

    #[test]
    fn timeout_and_unloaded_chunk_remain_visible() {
        let loaded = loaded_manager(1);
        let mob = entity(13, EntityType::Zombie, 20.8);
        let (mut los, _request_rx, _result_tx) = harness(2);
        assert!(los.is_entity_visible(&mob, CAMERA, CAMERA_CELL, &loaded));
        assert_eq!(los.in_flight_count(), 1);

        for _ in 0..ENTITY_LOS_REQUEST_TIMEOUT_FRAMES {
            los.poll_results();
        }
        assert_eq!(los.in_flight_count(), 0);
        assert_eq!(los.counters.timeouts, 1);
        assert!(los.counters.fail_open >= 1);

        let unloaded = loaded_manager(0);
        let (mut unloaded_los, unloaded_request_rx, _result_tx) = harness(2);
        assert!(unloaded_los.is_entity_visible(&mob, CAMERA, CAMERA_CELL, &unloaded));
        assert_eq!(unloaded_los.in_flight_count(), 0);
        assert!(unloaded_request_rx.try_recv().is_err());
    }

    #[test]
    fn bounded_in_flight_and_physical_queue_overflow_fail_open() {
        let chunks = loaded_manager(1);
        let first = entity(15, EntityType::Zombie, 20.8);
        let second = entity(16, EntityType::Zombie, 21.8);
        let (mut los, _request_rx, _result_tx) = harness(1);

        assert!(los.is_entity_visible(&first, CAMERA, CAMERA_CELL, &chunks));
        assert_eq!(los.in_flight_count(), 1);
        assert!(los.is_entity_visible(&second, CAMERA, CAMERA_CELL, &chunks));
        assert_eq!(los.in_flight_count(), 1);
        assert_eq!(los.counters.overflow, 1);

        let current = los.current_identity.clone().unwrap();
        los.set_current_identity(LosIdentity {
            world_revision: current.world_revision.wrapping_add(1),
            ..current
        });
        assert_eq!(los.in_flight_count(), 0);
        assert!(los.is_entity_visible(&second, CAMERA, CAMERA_CELL, &chunks));
        assert_eq!(los.in_flight_count(), 0);
        assert_eq!(los.counters.overflow, 2);
        assert!(!los.cache.contains_key(&second.id));
    }

    #[test]
    fn near_and_special_translucent_models_fail_open_without_distance_cull_credit() {
        let mut chunks = loaded_manager(1);
        let near = entity(17, EntityType::Zombie, 2.0);
        let special = entity(18, EntityType::HeartParticle, 20.8);
        let (mut los, request_rx, _result_tx) = harness(4);

        assert!(los.is_entity_visible(&near, CAMERA, CAMERA_CELL, &chunks));
        assert!(los.is_entity_visible(&special, CAMERA, CAMERA_CELL, &chunks));
        assert_eq!(los.counters.distance, 0);
        assert_eq!(los.counters.fail_open, 2);
        assert!(request_rx.try_recv().is_err());

        chunks.set_block(10, 200, 0, BlockType::NetherPortal);
        let normal = entity(19, EntityType::Zombie, 20.8);
        let (mut los, request_rx, result_tx) = harness(4);
        assert!(los.is_entity_visible(&normal, CAMERA, CAMERA_CELL, &chunks));
        let result = complete_request(&mut los, &request_rx, &result_tx);
        assert!(result.is_visible);
        assert!(los.is_entity_visible(&normal, CAMERA, CAMERA_CELL, &chunks));
    }
}
