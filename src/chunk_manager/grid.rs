//! Sliding dense 2-D column grid indexed by `(cx - origin_cx, cz - origin_cz)`.
//!
//! Hot `get_block` / lighting / physics / meshing paths index by arithmetic
//! instead of hashing. Coordinates outside the window (and not in the rare
//! overflow map) are unloaded.

use crate::authority::interest::RESIDENCY_HYSTERESIS;
use crate::world::Chunk;
use std::collections::HashMap;

/// Dense resident-column storage sized `2 * (distance + RESIDENCY_HYSTERESIS) + 1`.
pub struct DenseColumnGrid {
    origin_cx: i32,
    origin_cz: i32,
    side: usize,
    /// Configured view / simulation distance used to size and re-center.
    distance: i32,
    slots: Vec<Option<Chunk>>,
    occupied: usize,
    /// Rare out-of-window columns (tests, debug, momentary recenter gaps).
    overflow: HashMap<(i32, i32), Chunk>,
}

impl DenseColumnGrid {
    pub fn with_distance(distance: i32) -> Self {
        let distance = distance.max(0);
        let radius = distance.saturating_add(RESIDENCY_HYSTERESIS);
        let side = (2 * radius + 1) as usize;
        let origin = -radius;
        let side = side.max(1);
        Self {
            origin_cx: origin,
            origin_cz: origin,
            side,
            distance,
            slots: (0..side * side).map(|_| None).collect(),
            occupied: 0,
            overflow: HashMap::new(),
        }
    }

    pub fn distance(&self) -> i32 {
        self.distance
    }

    pub fn origin(&self) -> (i32, i32) {
        (self.origin_cx, self.origin_cz)
    }

    pub fn side(&self) -> usize {
        self.side
    }

    pub fn radius(&self) -> i32 {
        self.distance.saturating_add(RESIDENCY_HYSTERESIS)
    }

    #[inline]
    fn slot_index(&self, cx: i32, cz: i32) -> Option<usize> {
        let dx = cx.checked_sub(self.origin_cx)?;
        let dz = cz.checked_sub(self.origin_cz)?;
        if dx < 0 || dz < 0 {
            return None;
        }
        let dx = dx as usize;
        let dz = dz as usize;
        if dx >= self.side || dz >= self.side {
            return None;
        }
        Some(dz * self.side + dx)
    }

    pub fn in_window(&self, cx: i32, cz: i32) -> bool {
        self.slot_index(cx, cz).is_some()
    }

    pub fn len(&self) -> usize {
        self.occupied + self.overflow.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn contains_key(&self, key: &(i32, i32)) -> bool {
        if let Some(index) = self.slot_index(key.0, key.1) {
            return self.slots[index].is_some();
        }
        self.overflow.contains_key(key)
    }

    pub fn get(&self, key: &(i32, i32)) -> Option<&Chunk> {
        if let Some(index) = self.slot_index(key.0, key.1) {
            return self.slots[index].as_ref();
        }
        self.overflow.get(key)
    }

    pub fn get_mut(&mut self, key: &(i32, i32)) -> Option<&mut Chunk> {
        if let Some(index) = self.slot_index(key.0, key.1) {
            return self.slots[index].as_mut();
        }
        self.overflow.get_mut(key)
    }

    pub fn insert(&mut self, key: (i32, i32), chunk: Chunk) -> Option<Chunk> {
        if let Some(index) = self.slot_index(key.0, key.1) {
            let prev = self.slots[index].replace(chunk);
            if prev.is_none() {
                self.occupied += 1;
            }
            return prev;
        }
        // Out of window: keep in overflow so tests / union edges stay loadable.
        self.overflow.insert(key, chunk)
    }

    pub fn remove(&mut self, key: &(i32, i32)) -> Option<Chunk> {
        if let Some(index) = self.slot_index(key.0, key.1) {
            let prev = self.slots[index].take();
            if prev.is_some() {
                self.occupied -= 1;
            }
            return prev;
        }
        self.overflow.remove(key)
    }

    pub fn clear(&mut self) {
        for slot in &mut self.slots {
            *slot = None;
        }
        self.occupied = 0;
        self.overflow.clear();
    }

    /// Re-center the window on `(center_cx, center_cz)`. Columns that fall
    /// outside the new window move to overflow so dirty flush / explicit
    /// eviction can still observe them.
    pub fn recenter(&mut self, center_cx: i32, center_cz: i32) {
        let radius = self.radius();
        let new_origin_cx = center_cx - radius;
        let new_origin_cz = center_cz - radius;
        if new_origin_cx == self.origin_cx && new_origin_cz == self.origin_cz {
            return;
        }
        let mut next = vec![None; self.side * self.side];
        let mut next_occupied = 0usize;
        let mut still_overflow = HashMap::new();
        for dz in 0..self.side {
            for dx in 0..self.side {
                let Some(chunk) = self.slots[dz * self.side + dx].take() else {
                    continue;
                };
                let cx = self.origin_cx + dx as i32;
                let cz = self.origin_cz + dz as i32;
                let ndx = cx - new_origin_cx;
                let ndz = cz - new_origin_cz;
                if ndx >= 0
                    && ndz >= 0
                    && (ndx as usize) < self.side
                    && (ndz as usize) < self.side
                {
                    next[ndz as usize * self.side + ndx as usize] = Some(chunk);
                    next_occupied += 1;
                } else {
                    still_overflow.insert((cx, cz), chunk);
                }
            }
        }
        for (key, chunk) in self.overflow.drain() {
            let ndx = key.0 - new_origin_cx;
            let ndz = key.1 - new_origin_cz;
            if ndx >= 0
                && ndz >= 0
                && (ndx as usize) < self.side
                && (ndz as usize) < self.side
            {
                let index = ndz as usize * self.side + ndx as usize;
                if next[index].is_none() {
                    next_occupied += 1;
                }
                next[index] = Some(chunk);
            } else {
                still_overflow.insert(key, chunk);
            }
        }
        self.origin_cx = new_origin_cx;
        self.origin_cz = new_origin_cz;
        self.slots = next;
        self.occupied = next_occupied;
        self.overflow = still_overflow;
    }

    /// Grow / re-origin the window so it covers every `centers[i] ± radius`
    /// (Chebyshev). Used by multiplayer authority to cover the session union.
    /// Existing columns outside the new AABB move to overflow so callers can
    /// still flush dirty data before explicit eviction.
    pub fn cover_centers(&mut self, centers: &[(i32, i32)]) {
        if centers.is_empty() {
            return;
        }
        let radius = self.radius();
        let mut min_cx = i32::MAX;
        let mut max_cx = i32::MIN;
        let mut min_cz = i32::MAX;
        let mut max_cz = i32::MIN;
        for &(cx, cz) in centers {
            min_cx = min_cx.min(cx - radius);
            max_cx = max_cx.max(cx + radius);
            min_cz = min_cz.min(cz - radius);
            max_cz = max_cz.max(cz + radius);
        }
        let side_x = (max_cx - min_cx + 1).max(1) as usize;
        let side_z = (max_cz - min_cz + 1).max(1) as usize;
        let side = side_x.max(side_z);
        if min_cx == self.origin_cx && min_cz == self.origin_cz && side == self.side {
            return;
        }
        let mut next = vec![None; side * side];
        let mut next_occupied = 0usize;
        let mut next_overflow = HashMap::new();
        let mut migrate = |cx: i32, cz: i32, chunk: Chunk| {
            let dx = cx - min_cx;
            let dz = cz - min_cz;
            if dx >= 0 && dz >= 0 && (dx as usize) < side && (dz as usize) < side {
                let index = dz as usize * side + dx as usize;
                if next[index].is_none() {
                    next_occupied += 1;
                }
                next[index] = Some(chunk);
            } else {
                next_overflow.insert((cx, cz), chunk);
            }
        };
        for dz in 0..self.side {
            for dx in 0..self.side {
                if let Some(chunk) = self.slots[dz * self.side + dx].take() {
                    migrate(
                        self.origin_cx + dx as i32,
                        self.origin_cz + dz as i32,
                        chunk,
                    );
                }
            }
        }
        for (key, chunk) in self.overflow.drain() {
            migrate(key.0, key.1, chunk);
        }
        self.origin_cx = min_cx;
        self.origin_cz = min_cz;
        self.side = side;
        self.slots = next;
        self.occupied = next_occupied;
        self.overflow = next_overflow;
    }

    /// Deterministic key iteration: window cells in row-major `(cz, cx)`, then
    /// overflow keys sorted. Iteration order must not feed RNG / checksum.
    pub fn keys(&self) -> impl Iterator<Item = (i32, i32)> + '_ {
        let window = (0..self.side).flat_map(move |dz| {
            (0..self.side).filter_map(move |dx| {
                if self.slots[dz * self.side + dx].is_some() {
                    Some((self.origin_cx + dx as i32, self.origin_cz + dz as i32))
                } else {
                    None
                }
            })
        });
        let mut overflow_keys: Vec<_> = self.overflow.keys().copied().collect();
        overflow_keys.sort_unstable();
        window.chain(overflow_keys)
    }

    pub fn values(&self) -> impl Iterator<Item = &Chunk> {
        self.iter().map(|(_, chunk)| chunk)
    }

    pub fn iter(&self) -> impl Iterator<Item = ((i32, i32), &Chunk)> {
        let window = (0..self.side).flat_map(move |dz| {
            (0..self.side).filter_map(move |dx| {
                self.slots[dz * self.side + dx]
                    .as_ref()
                    .map(|chunk| ((self.origin_cx + dx as i32, self.origin_cz + dz as i32), chunk))
            })
        });
        let mut overflow: Vec<_> = self.overflow.iter().map(|(&k, v)| (k, v)).collect();
        overflow.sort_unstable_by_key(|(k, _)| *k);
        window.chain(overflow)
    }

    pub fn iter_mut(&mut self) -> Vec<((i32, i32), *mut Chunk)> {
        // Collect raw pointers so callers can rebuild a safe iterator pattern;
        // prefer `get_mut` for single-column mutation.
        let mut out = Vec::with_capacity(self.len());
        for dz in 0..self.side {
            for dx in 0..self.side {
                let key = (self.origin_cx + dx as i32, self.origin_cz + dz as i32);
                if let Some(chunk) = self.slots[dz * self.side + dx].as_mut() {
                    out.push((key, chunk as *mut Chunk));
                }
            }
        }
        let mut overflow_keys: Vec<_> = self.overflow.keys().copied().collect();
        overflow_keys.sort_unstable();
        for key in overflow_keys {
            if let Some(chunk) = self.overflow.get_mut(&key) {
                out.push((key, chunk as *mut Chunk));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::BlockType;

    #[test]
    fn window_covers_distance_plus_hysteresis() {
        let grid = DenseColumnGrid::with_distance(2);
        // radius = 4, side = 9, origin = -4 → [-4, 4]
        assert!(grid.in_window(0, 0));
        assert!(grid.in_window(-4, -4));
        assert!(grid.in_window(4, 4));
        assert!(!grid.in_window(5, 0));
    }

    #[test]
    fn insert_get_remove_roundtrip() {
        let mut grid = DenseColumnGrid::with_distance(2);
        grid.insert((1, -1), Chunk::empty(1, -1));
        assert!(grid.contains_key(&(1, -1)));
        assert_eq!(grid.len(), 1);
        let chunk = grid.get(&(1, -1)).unwrap();
        assert_eq!(chunk.get_block_local(0, 0, 0), BlockType::Air);
        assert!(grid.remove(&(1, -1)).is_some());
        assert!(grid.is_empty());
    }

    #[test]
    fn keys_are_deterministic_row_major() {
        let mut grid = DenseColumnGrid::with_distance(2);
        grid.insert((1, 0), Chunk::empty(1, 0));
        grid.insert((-1, -1), Chunk::empty(-1, -1));
        grid.insert((0, 0), Chunk::empty(0, 0));
        assert_eq!(grid.keys().collect::<Vec<_>>(), vec![(-1, -1), (0, 0), (1, 0)]);
    }

    #[test]
    fn overflow_accepts_out_of_window_insert() {
        let mut grid = DenseColumnGrid::with_distance(0);
        // radius 2 → [-2,2]; (4,-3) is outside
        grid.insert((4, -3), Chunk::empty(4, -3));
        assert!(grid.contains_key(&(4, -3)));
        assert_eq!(grid.keys().collect::<Vec<_>>(), vec![(4, -3)]);
    }

    #[test]
    fn recenter_moves_outside_to_overflow() {
        let mut grid = DenseColumnGrid::with_distance(1);
        grid.insert((0, 0), Chunk::empty(0, 0));
        grid.insert((3, 0), Chunk::empty(3, 0));
        grid.recenter(10, 0);
        assert!(!grid.in_window(0, 0));
        assert!(grid.contains_key(&(0, 0)), "outside columns stay in overflow");
        grid.insert((10, 0), Chunk::empty(10, 0));
        assert!(grid.contains_key(&(10, 0)));
    }
}
