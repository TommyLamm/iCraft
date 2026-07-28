use crate::dimension::Dimension;
use std::collections::{HashSet, VecDeque};

pub const UNLOAD_HYSTERESIS: i32 = 2;
pub const MAX_INTEGRATE_TIME_MS: u64 = 3;
pub const MAX_INTEGRATE_MESHES: usize = 4;
pub const MAX_INTEGRATE_UPLOAD_BYTES: u64 = 2 * 1024 * 1024; // 2 MiB

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
    pub dirty_chunk_meshes: HashSet<(i32, i32)>,
}

impl ChunkStreamingScheduler {
    pub fn new() -> Self {
        Self {
            spiral_offsets: Vec::new(),
            last_player_chunk: None,
            last_render_distance: 0,
            last_dimension: None,
            pending_load_queue: VecDeque::new(),
            dirty_chunk_meshes: HashSet::new(),
        }
    }

    pub fn mark_dirty(&mut self, coord: (i32, i32)) {
        self.dirty_chunk_meshes.insert(coord);
    }

    pub fn remove_dirty(&mut self, coord: &(i32, i32)) {
        self.dirty_chunk_meshes.remove(coord);
    }

    pub fn clear(&mut self) {
        self.last_player_chunk = None;
        self.last_render_distance = 0;
        self.last_dimension = None;
        self.pending_load_queue.clear();
        self.dirty_chunk_meshes.clear();
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
        scheduler.mark_dirty((1, 2));
        assert!(scheduler.dirty_chunk_meshes.contains(&(1, 2)));
        scheduler.remove_dirty(&(1, 2));
        assert!(!scheduler.dirty_chunk_meshes.contains(&(1, 2)));
    }
}
