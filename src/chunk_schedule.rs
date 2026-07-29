use crate::dimension::Dimension;
use std::collections::{BTreeSet, HashMap, VecDeque};

pub const UNLOAD_HYSTERESIS: i32 = 2;
pub const MAX_INTEGRATE_TIME_MS: u64 = 3;
pub const MAX_INTEGRATE_MESHES: usize = 4;
pub const MAX_INTEGRATE_UPLOAD_BYTES: u64 = 2 * 1024 * 1024; // 2 MiB
pub const MAX_DIRTY_MESH_QUEUE: usize = 16_384;

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DependencyReason {
    Block,
    Fluid,
    Light,
    Weather,
    Redstone,
    Network,
    BreakPlace,
    Mob,
    Ao,
    ChunkLoad,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct DirtyMeshWork {
    pub coord: (i32, i32),
    pub revision: u64,
    pub reason: DependencyReason,
    distance_sq: u64,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct DirtyMeshPriority {
    distance_sq: u64,
    cx: i32,
    cz: i32,
    reason: DependencyReason,
}

impl DirtyMeshPriority {
    fn new(coord: (i32, i32), reason: DependencyReason, player_chunk: (i32, i32)) -> Self {
        Self {
            distance_sq: distance_sq(coord, player_chunk),
            cx: coord.0,
            cz: coord.1,
            reason,
        }
    }

    fn coord(self) -> (i32, i32) {
        (self.cx, self.cz)
    }
}

fn distance_sq(coord: (i32, i32), player_chunk: (i32, i32)) -> u64 {
    let dx = i64::from(coord.0) - i64::from(player_chunk.0);
    let dz = i64::from(coord.1) - i64::from(player_chunk.1);
    (dx * dx + dz * dz) as u64
}

/// Precomputes relative chunk coordinates (dx, dz) sorted by squared distance dx^2 + dz^2 ascending.
pub fn precompute_spiral_offsets(r: i32) -> Vec<(i32, i32)> {
    let mut offsets = Vec::with_capacity(((2 * r + 1) * (2 * r + 1)) as usize);
    for dx in -r..=r {
        for dz in -r..=r {
            offsets.push((dx, dz));
        }
    }
    offsets.sort_by_key(|&(dx, dz)| dx * dx + dz * dz);
    offsets
}

/// State tracking incremental streaming schedules and queues.
pub struct ChunkStreamingScheduler {
    pub spiral_offsets: Vec<(i32, i32)>,
    pub last_player_chunk: Option<(i32, i32)>,
    pub last_render_distance: i32,
    pub last_dimension: Option<Dimension>,
    pub pending_load_queue: VecDeque<(i32, i32)>,
    dirty_chunk_meshes: HashMap<(i32, i32), DirtyMeshWork>,
    dirty_mesh_priority: BTreeSet<DirtyMeshPriority>,
    pub dirty_mesh_drop_count: u64,
}

impl ChunkStreamingScheduler {
    pub fn new() -> Self {
        Self {
            spiral_offsets: Vec::new(),
            last_player_chunk: None,
            last_render_distance: 0,
            last_dimension: None,
            pending_load_queue: VecDeque::new(),
            dirty_chunk_meshes: HashMap::new(),
            dirty_mesh_priority: BTreeSet::new(),
            dirty_mesh_drop_count: 0,
        }
    }

    /// Persistently queues the latest revision for a chunk. Repeated
    /// invalidations update the existing work item instead of duplicating it.
    pub fn enqueue_dirty(
        &mut self,
        coord: (i32, i32),
        reason: DependencyReason,
        revision: u64,
        player_chunk: (i32, i32),
    ) -> bool {
        let newly_queued = !self.dirty_chunk_meshes.contains_key(&coord);
        if let Some(previous) = self.dirty_chunk_meshes.get(&coord).copied() {
            self.dirty_mesh_priority.remove(&DirtyMeshPriority {
                distance_sq: previous.distance_sq,
                cx: coord.0,
                cz: coord.1,
                reason: previous.reason,
            });
        }

        let priority = DirtyMeshPriority::new(coord, reason, player_chunk);
        let work = DirtyMeshWork {
            coord,
            revision,
            reason,
            distance_sq: priority.distance_sq,
        };
        self.dirty_chunk_meshes.insert(coord, work);
        self.dirty_mesh_priority.insert(priority);

        let mut retained = true;
        if self.dirty_chunk_meshes.len() > MAX_DIRTY_MESH_QUEUE {
            if let Some(farthest) = self.dirty_mesh_priority.pop_last() {
                self.dirty_chunk_meshes.remove(&farthest.coord());
                self.dirty_mesh_drop_count = self.dirty_mesh_drop_count.saturating_add(1);
                retained = farthest.coord() != coord;
            }
        }
        newly_queued && retained
    }

    pub fn remove_dirty(&mut self, coord: &(i32, i32)) {
        if let Some(work) = self.dirty_chunk_meshes.remove(coord) {
            self.dirty_mesh_priority.remove(&DirtyMeshPriority {
                distance_sq: work.distance_sq,
                cx: coord.0,
                cz: coord.1,
                reason: work.reason,
            });
        }
    }

    pub fn dirty_len(&self) -> usize {
        self.dirty_chunk_meshes.len()
    }

    #[cfg(test)]
    pub fn is_dirty(&self, coord: &(i32, i32)) -> bool {
        self.dirty_chunk_meshes.contains_key(coord)
    }

    #[cfg(test)]
    pub fn dirty_work(&self, coord: &(i32, i32)) -> Option<DirtyMeshWork> {
        self.dirty_chunk_meshes.get(coord).copied()
    }

    /// Removes and returns the nearest queued item that remains within the
    /// current render square. Items outside the square stay queued.
    pub fn pop_nearest_dirty(
        &mut self,
        player_chunk: (i32, i32),
        render_distance: i32,
    ) -> Option<DirtyMeshWork> {
        let key = self.dirty_mesh_priority.iter().copied().find(|priority| {
            (priority.cx - player_chunk.0).abs() <= render_distance
                && (priority.cz - player_chunk.1).abs() <= render_distance
        })?;
        self.dirty_mesh_priority.remove(&key);
        self.dirty_chunk_meshes.remove(&key.coord())
    }

    pub fn requeue_dirty(&mut self, work: DirtyMeshWork, player_chunk: (i32, i32)) {
        self.enqueue_dirty(work.coord, work.reason, work.revision, player_chunk);
    }

    /// Player movement is infrequent relative to frames, so reprioritize only
    /// when the streaming target changes rather than sorting every frame.
    pub fn reprioritize_dirty(&mut self, player_chunk: (i32, i32)) {
        self.dirty_mesh_priority.clear();
        for work in self.dirty_chunk_meshes.values_mut() {
            work.distance_sq = distance_sq(work.coord, player_chunk);
            self.dirty_mesh_priority.insert(DirtyMeshPriority {
                distance_sq: work.distance_sq,
                cx: work.coord.0,
                cz: work.coord.1,
                reason: work.reason,
            });
        }
    }

    pub fn clear(&mut self) {
        self.last_player_chunk = None;
        self.last_render_distance = 0;
        self.last_dimension = None;
        self.pending_load_queue.clear();
        self.dirty_chunk_meshes.clear();
        self.dirty_mesh_priority.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_precompute_spiral_offsets_ordering() {
        let offsets = precompute_spiral_offsets(2);
        assert_eq!(offsets.len(), 25);
        assert_eq!(offsets[0], (0, 0));
        let mut prev_dist = 0;
        for &(dx, dz) in &offsets {
            let dist = dx * dx + dz * dz;
            assert!(
                dist >= prev_dist,
                "spiral offsets must be sorted by distance"
            );
            prev_dist = dist;
        }
    }

    #[test]
    fn test_scheduler_dirty_management() {
        let mut scheduler = ChunkStreamingScheduler::new();
        assert!(scheduler.enqueue_dirty((1, 2), DependencyReason::Block, 1, (0, 0)));
        assert!(scheduler.is_dirty(&(1, 2)));
        scheduler.remove_dirty(&(1, 2));
        assert!(!scheduler.is_dirty(&(1, 2)));
    }

    #[test]
    fn duplicate_invalidations_update_revision_and_reason_without_duplicate_work() {
        let mut scheduler = ChunkStreamingScheduler::new();
        assert!(scheduler.enqueue_dirty((1, 2), DependencyReason::Block, 1, (0, 0)));
        assert!(!scheduler.enqueue_dirty((1, 2), DependencyReason::Light, 2, (0, 0)));
        assert_eq!(scheduler.dirty_len(), 1);
        let work = scheduler.dirty_work(&(1, 2)).unwrap();
        assert_eq!(work.revision, 2);
        assert_eq!(work.reason, DependencyReason::Light);
    }

    #[test]
    fn nearest_work_is_persistent_and_reprioritized_after_player_moves() {
        let mut scheduler = ChunkStreamingScheduler::new();
        scheduler.enqueue_dirty((8, 0), DependencyReason::Block, 1, (0, 0));
        scheduler.enqueue_dirty((1, 0), DependencyReason::Block, 1, (0, 0));

        assert_eq!(
            scheduler.pop_nearest_dirty((0, 0), 16).unwrap().coord,
            (1, 0)
        );
        scheduler.reprioritize_dirty((10, 0));
        assert_eq!(
            scheduler.pop_nearest_dirty((10, 0), 16).unwrap().coord,
            (8, 0)
        );
    }
}
