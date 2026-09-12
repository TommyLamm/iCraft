use crate::dimension::Dimension;
use crate::world::{SectionIdentity, SectionKey};
use std::collections::{BTreeSet, HashMap, VecDeque};

pub const UNLOAD_HYSTERESIS: i32 = 2;
pub const MAX_INTEGRATE_TIME_MS: u64 = 3;
pub const MAX_INTEGRATE_MESHES: usize = 4;
pub const MAX_INTEGRATE_UPLOAD_BYTES: u64 = 2 * 1024 * 1024; // 2 MiB
pub const MAX_INTEGRATE_LOADS: usize = 2;
pub const MAX_INTEGRATE_LOAD_BYTES: u64 = 2 * 1024 * 1024; // 2 MiB
pub const MAX_DIRTY_MESH_QUEUE: usize = 16_384;

/// Chebyshev `view + UNLOAD_HYSTERESIS` membership used by client unload and
/// authority residency.
#[inline]
pub fn within_unload_hysteresis(
    cx: i32,
    cz: i32,
    player_cx: i32,
    player_cz: i32,
    view_distance: i32,
) -> bool {
    let radius = view_distance.saturating_add(UNLOAD_HYSTERESIS);
    (cx - player_cx).abs() <= radius && (cz - player_cz).abs() <= radius
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct DirtySectionWork {
    pub identity: SectionIdentity,
    pub reason: DependencyReason,
    pub distance_sq: u64,
}

/// Persistent section queue kept separate from legacy chunk scheduling.
#[derive(Default)]
pub struct SectionMeshScheduler {
    pending: HashMap<SectionKey, DirtySectionWork>,
    priority: BTreeSet<(u64, i32, i8, i32)>,
    pub in_flight: HashMap<SectionKey, SectionIdentity>,
}

impl SectionMeshScheduler {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn clear(&mut self) {
        self.pending.clear();
        self.priority.clear();
        self.in_flight.clear();
    }
    pub fn enqueue(
        &mut self,
        identity: SectionIdentity,
        reason: DependencyReason,
        player_chunk: (i32, i32),
    ) {
        let key = identity.key;
        if let Some(old) = self.pending.insert(
            key,
            DirtySectionWork {
                identity,
                reason,
                distance_sq: section_distance(key, player_chunk),
            },
        ) {
            self.priority
                .remove(&(old.distance_sq, key.cx, key.section_y, key.cz));
        }
        let work = self.pending[&key];
        self.priority
            .insert((work.distance_sq, key.cx, key.section_y, key.cz));
        // Latest-wins per section; overflow evicts the farthest pending key so
        // view-distance 16 (~26k Overworld sections) cannot grow unbounded.
        if self.pending.len() > MAX_DIRTY_MESH_QUEUE {
            if let Some(farthest) = self.priority.pop_last() {
                self.pending
                    .remove(&SectionKey::new(farthest.1, farthest.2, farthest.3));
            }
        }
    }
    pub fn pop_nearest(
        &mut self,
        player_chunk: (i32, i32),
        distance: i32,
    ) -> Option<DirtySectionWork> {
        let item = self.priority.iter().copied().find(|(_, cx, _, cz)| {
            (cx - player_chunk.0).abs() <= distance && (cz - player_chunk.1).abs() <= distance
        })?;
        self.priority.remove(&item);
        self.pending
            .remove(&SectionKey::new(item.1, item.2, item.3))
    }
    pub fn mark_in_flight(&mut self, work: DirtySectionWork) {
        self.in_flight.insert(work.identity.key, work.identity);
    }
    pub fn complete(&mut self, identity: SectionIdentity) -> bool {
        self.in_flight
            .get(&identity.key)
            .is_some_and(|current| *current == identity)
            && {
                self.in_flight.remove(&identity.key);
                true
            }
    }
    pub fn remove(&mut self, key: SectionKey) -> Option<DirtySectionWork> {
        if let Some(old) = self.pending.remove(&key) {
            self.priority
                .remove(&(old.distance_sq, key.cx, key.section_y, key.cz));
            return Some(old);
        }
        None
    }
    pub fn remove_chunk(&mut self, cx: i32, cz: i32) {
        let keys: Vec<_> = self
            .pending
            .keys()
            .copied()
            .filter(|k| k.cx == cx && k.cz == cz)
            .collect();
        for key in keys {
            self.remove(key);
        }
        self.in_flight.retain(|key, _| key.cx != cx || key.cz != cz);
    }
    pub fn requeue(&mut self, work: DirtySectionWork, player_chunk: (i32, i32)) {
        self.enqueue(work.identity, work.reason, player_chunk);
    }
    pub fn reprioritize(&mut self, player_chunk: (i32, i32)) {
        let works: Vec<_> = self
            .pending
            .values()
            .map(|w| (w.identity, w.reason))
            .collect();
        self.pending.clear();
        self.priority.clear();
        for (identity, reason) in works {
            self.enqueue(identity, reason, player_chunk);
        }
    }
    pub fn is_in_flight(&self, key: SectionKey) -> bool {
        self.in_flight.contains_key(&key)
    }
    pub fn bounded(&self, max_items: usize) -> usize {
        self.pending.len().min(max_items)
    }
    pub fn len(&self) -> usize {
        self.pending.len()
    }
}

fn section_distance(key: SectionKey, player: (i32, i32)) -> u64 {
    let dx = i64::from(key.cx) - i64::from(player.0);
    let dz = i64::from(key.cz) - i64::from(player.1);
    (dx * dx + dz * dz) as u64
}

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
}

impl ChunkStreamingScheduler {
    pub fn new() -> Self {
        Self {
            spiral_offsets: Vec::new(),
            last_player_chunk: None,
            last_render_distance: 0,
            last_dimension: None,
            pending_load_queue: VecDeque::new(),
        }
    }


    pub fn clear(&mut self) {
        self.last_player_chunk = None;
        self.last_render_distance = 0;
        self.last_dimension = None;
        self.pending_load_queue.clear();
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
    fn section_scheduler_deduplicates_and_rejects_stale_completion() {
        let key = SectionKey::new(2, 3, 4);
        let mut scheduler = SectionMeshScheduler::new();
        scheduler.enqueue(
            SectionIdentity::new(key, 1, 7),
            DependencyReason::Block,
            (0, 0),
        );
        scheduler.enqueue(
            SectionIdentity::new(key, 2, 7),
            DependencyReason::Light,
            (0, 0),
        );
        assert_eq!(scheduler.len(), 1);
        let work = scheduler.pop_nearest((0, 0), 10).unwrap();
        scheduler.mark_in_flight(work);
        assert!(!scheduler.complete(SectionIdentity::new(key, 1, 7)));
        assert!(scheduler.complete(work.identity));
    }

    #[test]
    fn section_scheduler_overwrite_keeps_latest_identity() {
        let key = SectionKey::new(1, 0, 2);
        let mut scheduler = SectionMeshScheduler::new();
        scheduler.enqueue(
            SectionIdentity::new(key, 4, 9),
            DependencyReason::Block,
            (0, 0),
        );
        scheduler.enqueue(
            SectionIdentity::new(key, 8, 9),
            DependencyReason::ChunkLoad,
            (0, 0),
        );
        assert_eq!(scheduler.len(), 1);
        let work = scheduler.pop_nearest((0, 0), 16).unwrap();
        assert_eq!(work.identity.revision, 8);
        assert_eq!(work.reason, DependencyReason::ChunkLoad);
        assert_eq!(scheduler.len(), 0);
    }

    #[test]
    fn section_scheduler_caps_pending_at_max_dirty_mesh_queue() {
        let mut scheduler = SectionMeshScheduler::new();
        let player = (0, 0);
        for i in 0..=MAX_DIRTY_MESH_QUEUE {
            let key = SectionKey::new(i as i32, 0, 0);
            scheduler.enqueue(
                SectionIdentity::new(key, 1, 1),
                DependencyReason::Block,
                player,
            );
        }
        assert_eq!(scheduler.len(), MAX_DIRTY_MESH_QUEUE);
        let nearest = scheduler.pop_nearest(player, i32::MAX).unwrap();
        assert_eq!(nearest.identity.key, SectionKey::new(0, 0, 0));
        assert!(scheduler.pop_nearest(player, i32::MAX).is_some_and(
            |work| work.identity.key != SectionKey::new(MAX_DIRTY_MESH_QUEUE as i32, 0, 0)
        ));
    }
}
