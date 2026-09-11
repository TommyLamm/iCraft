//! Dimension-aware interest and replication routing for the headless authority.
//!
//! The renderer and network transport are consumers of this contract.  They
//! must not infer recipients from their own chunk caches: a session receives a
//! world delta only when this module says that its authenticated dimension and
//! interest set contain the changed object.

use crate::chunk_schedule::UNLOAD_HYSTERESIS;
use crate::dimension::Dimension;
use std::collections::{BTreeSet, HashSet};

/// Same extra Chebyshev ring the client uses before unloading a column.
pub const RESIDENCY_HYSTERESIS: i32 = UNLOAD_HYSTERESIS;
/// Spawn may keep a Chebyshev ring when a dimension has no sessions.
pub const SPAWN_RESIDENCY_RADIUS: i32 = 1;
pub const SPAWN_RESIDENCY_CAP: usize = 9;

pub const MAX_INTEREST_UPDATES_PER_TICK: usize = 8_192;

pub type ChunkCoord = (i32, i32);
pub type BlockPosition = (i32, i32, i32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterestKind {
    Chunk(ChunkCoord),
    /// Entity lifecycle/visibility follows view distance.
    Entity(u64),
    /// High-frequency state follows the smaller simulation distance.
    EntityState(u64),
    Block(BlockPosition),
    BlockEntity(BlockPosition),
    Container(BlockPosition),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutedInterestUpdate {
    pub target: u64,
    pub dimension: Dimension,
    pub revision: u64,
    pub kind: InterestKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterestDelta<T> {
    pub entered: Vec<T>,
    pub departed: Vec<T>,
}

impl<T> Default for InterestDelta<T> {
    fn default() -> Self {
        Self {
            entered: Vec::new(),
            departed: Vec::new(),
        }
    }
}

/// Cached column key for chunk interest. When this matches the next pose,
/// `update_position` skips rebuilding both chunk HashSets.
pub type InterestChunkAnchor = (Dimension, i32, i32, u8, u8);

/// Per-session routing state. The sets are bounded by the configured view and
/// simulation distances; `open_containers` is bounded by the number of
/// simultaneously open UI sessions (one per container coordinate).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterestSet {
    pub dimension: Dimension,
    pub view_distance: u8,
    pub simulation_distance: u8,
    pub chunks: HashSet<ChunkCoord>,
    pub simulation_chunks: HashSet<ChunkCoord>,
    pub entities: HashSet<u64>,
    pub simulation_entities: HashSet<u64>,
    pub open_containers: BTreeSet<BlockPosition>,
    /// Last column key for which `chunks` / `simulation_chunks` were built.
    chunk_anchor: Option<InterestChunkAnchor>,
    /// Last `EntityManager::spatial_revision` applied to the entity sets.
    pub(crate) entity_spatial_revision: Option<u64>,
    #[cfg(test)]
    chunk_rebuilds: u64,
}

impl InterestSet {
    pub fn new(dimension: Dimension, view_distance: u8, simulation_distance: u8) -> Self {
        Self {
            dimension,
            view_distance,
            simulation_distance,
            chunks: HashSet::new(),
            simulation_chunks: HashSet::new(),
            entities: HashSet::new(),
            simulation_entities: HashSet::new(),
            open_containers: BTreeSet::new(),
            chunk_anchor: None,
            entity_spatial_revision: None,
            #[cfg(test)]
            chunk_rebuilds: 0,
        }
    }

    /// Column key used to skip stationary chunk rebuilds.
    pub fn chunk_anchor_for(
        dimension: Dimension,
        position: [f32; 3],
        view_distance: u8,
        simulation_distance: u8,
    ) -> InterestChunkAnchor {
        let cx = (position[0] / 16.0).floor() as i32;
        let cz = (position[2] / 16.0).floor() as i32;
        (dimension, cx, cz, view_distance, simulation_distance)
    }

    pub fn current_chunk_anchor(&self) -> Option<InterestChunkAnchor> {
        self.chunk_anchor
    }

    /// Apply runtime view / simulation distances. Returns true when either
    /// value changed (forces the next `update_position` to rebuild).
    pub fn set_distances(&mut self, view_distance: u8, simulation_distance: u8) -> bool {
        if self.view_distance == view_distance && self.simulation_distance == simulation_distance {
            return false;
        }
        self.view_distance = view_distance;
        self.simulation_distance = simulation_distance;
        self.chunk_anchor = None;
        true
    }

    /// Force the next position / entity refresh to rebuild (teleport, distance
    /// change, or callers that cannot rely on the cached anchor).
    pub fn invalidate_anchor(&mut self) {
        self.chunk_anchor = None;
        self.entity_spatial_revision = None;
    }

    #[cfg(test)]
    pub fn chunk_rebuilds(&self) -> u64 {
        self.chunk_rebuilds
    }

    pub fn update_position(
        &mut self,
        dimension: Dimension,
        position: [f32; 3],
    ) -> InterestDelta<ChunkCoord> {
        let anchor = Self::chunk_anchor_for(
            dimension,
            position,
            self.view_distance,
            self.simulation_distance,
        );
        if self.chunk_anchor == Some(anchor) {
            return InterestDelta::default();
        }

        let old_dimension = self.dimension;
        let old_chunks = std::mem::take(&mut self.chunks);
        self.dimension = dimension;
        if old_dimension != dimension {
            self.open_containers.clear();
            // Dimension change also invalidates entity membership.
            self.entity_spatial_revision = None;
        }
        self.chunks = chunks_around(position, self.view_distance);
        self.simulation_chunks = chunks_around(position, self.simulation_distance);
        self.chunk_anchor = Some(anchor);
        #[cfg(test)]
        {
            self.chunk_rebuilds = self.chunk_rebuilds.saturating_add(1);
        }
        self.open_containers.retain(|position| {
            let chunk = (position.0.div_euclid(16), position.2.div_euclid(16));
            self.chunks.contains(&chunk)
        });
        let mut entered: Vec<_> = if old_dimension == dimension {
            self.chunks.difference(&old_chunks).copied().collect()
        } else {
            self.chunks.iter().copied().collect()
        };
        let mut departed: Vec<_> = if old_dimension == dimension {
            old_chunks.difference(&self.chunks).copied().collect()
        } else {
            old_chunks.iter().copied().collect()
        };
        entered.sort_unstable();
        departed.sort_unstable();
        InterestDelta { entered, departed }
    }

    /// Diff `entity_ids` into `entities` without `mem::take` of the live set.
    pub fn update_entities<I>(&mut self, entity_ids: I) -> InterestDelta<u64>
    where
        I: IntoIterator<Item = u64>,
    {
        let incoming: HashSet<u64> = entity_ids.into_iter().collect();
        let mut entered: Vec<_> = incoming
            .difference(&self.entities)
            .copied()
            .collect();
        let mut departed: Vec<_> = self
            .entities
            .difference(&incoming)
            .copied()
            .collect();
        for id in &departed {
            self.entities.remove(id);
        }
        for id in &entered {
            self.entities.insert(*id);
        }
        entered.sort_unstable();
        departed.sort_unstable();
        InterestDelta { entered, departed }
    }

    /// Diff simulation-distance entity ids without replacing the HashSet.
    pub fn update_simulation_entities<I>(&mut self, entity_ids: I)
    where
        I: IntoIterator<Item = u64>,
    {
        let incoming: HashSet<u64> = entity_ids.into_iter().collect();
        let departed: Vec<_> = self
            .simulation_entities
            .difference(&incoming)
            .copied()
            .collect();
        for id in departed {
            self.simulation_entities.remove(&id);
        }
        for id in incoming {
            self.simulation_entities.insert(id);
        }
    }

    pub fn wants(&self, dimension: Dimension, kind: InterestKind) -> bool {
        if self.dimension != dimension {
            return false;
        }
        match kind {
            InterestKind::Chunk(coord) => self.chunks.contains(&coord),
            InterestKind::Entity(id) => self.entities.contains(&id),
            InterestKind::EntityState(id) => self.simulation_entities.contains(&id),
            InterestKind::Block(position)
            | InterestKind::BlockEntity(position)
            | InterestKind::Container(position) => self
                .chunks
                .contains(&(position.0.div_euclid(16), position.2.div_euclid(16))),
        }
    }

    pub fn wants_container(&self, dimension: Dimension, position: BlockPosition) -> bool {
        self.dimension == dimension && self.open_containers.contains(&position)
    }
}

pub fn chunks_around(position: [f32; 3], distance: u8) -> HashSet<ChunkCoord> {
    let cx = (position[0] / 16.0).floor() as i32;
    let cz = (position[2] / 16.0).floor() as i32;
    let radius = i32::from(distance);
    let mut chunks = HashSet::with_capacity(
        ((radius.saturating_mul(2).saturating_add(1)).pow(2) as usize).min(4096),
    );
    for dx in -radius..=radius {
        for dz in -radius..=radius {
            if dx.saturating_mul(dx).saturating_add(dz.saturating_mul(dz))
                <= radius.saturating_mul(radius)
            {
                chunks.insert((cx.saturating_add(dx), cz.saturating_add(dz)));
            }
        }
    }
    chunks
}

/// Client-matching unload hysteresis: Chebyshev `view + UNLOAD_HYSTERESIS`.
pub fn residency_hysteresis_chunks(position: [f32; 3], view_distance: u8) -> HashSet<ChunkCoord> {
    let cx = (position[0] / 16.0).floor() as i32;
    let cz = (position[2] / 16.0).floor() as i32;
    let radius = i32::from(view_distance).saturating_add(RESIDENCY_HYSTERESIS);
    let mut chunks = HashSet::with_capacity(
        ((radius.saturating_mul(2).saturating_add(1)).pow(2) as usize).min(4096),
    );
    for dx in -radius..=radius {
        for dz in -radius..=radius {
            chunks.insert((cx.saturating_add(dx), cz.saturating_add(dz)));
        }
    }
    chunks
}

/// Union of every session's simulation columns, used as the tick walk set.
pub fn union_simulation_chunks<'a, I>(sets: I) -> BTreeSet<ChunkCoord>
where
    I: IntoIterator<Item = &'a InterestSet>,
{
    let mut union = BTreeSet::new();
    for set in sets {
        union.extend(set.simulation_chunks.iter().copied());
    }
    union
}

/// Capped spawn ring kept only while a dimension has no sessions.
pub fn capped_spawn_residency(spawn_x: i32, spawn_z: i32) -> BTreeSet<ChunkCoord> {
    let cx = spawn_x.div_euclid(16);
    let cz = spawn_z.div_euclid(16);
    let mut chunks = BTreeSet::new();
    for dx in -SPAWN_RESIDENCY_RADIUS..=SPAWN_RESIDENCY_RADIUS {
        for dz in -SPAWN_RESIDENCY_RADIUS..=SPAWN_RESIDENCY_RADIUS {
            if chunks.len() >= SPAWN_RESIDENCY_CAP {
                return chunks;
            }
            chunks.insert((cx.saturating_add(dx), cz.saturating_add(dz)));
        }
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dimensions_and_simulation_distance_are_isolated() {
        let mut interest = InterestSet::new(Dimension::Overworld, 4, 2);
        interest.update_position(Dimension::Overworld, [0.0, 64.0, 0.0]);
        assert!(interest.wants(Dimension::Overworld, InterestKind::Block((0, 64, 0))));
        assert!(!interest.wants(Dimension::Nether, InterestKind::Block((0, 64, 0))));
        assert!(interest.simulation_chunks.len() < interest.chunks.len());
    }

    #[test]
    fn container_viewer_only_receives_open_container_updates() {
        let mut interest = InterestSet::new(Dimension::Overworld, 4, 2);
        interest.update_position(Dimension::Overworld, [0.0, 64.0, 0.0]);
        let chest = (1, 64, 1);
        assert!(!interest.wants_container(Dimension::Overworld, chest));
        interest.open_containers.insert(chest);
        assert!(interest.wants_container(Dimension::Overworld, chest));
        assert!(!interest.wants_container(Dimension::End, chest));
    }

    #[test]
    fn interest_deltas_are_sorted_and_split_view_from_simulation() {
        let mut interest = InterestSet::new(Dimension::Overworld, 2, 1);
        let initial = interest.update_position(Dimension::Overworld, [0.0, 64.0, 0.0]);
        assert!(!initial.entered.is_empty());
        assert!(initial.entered.windows(2).all(|pair| pair[0] <= pair[1]));

        let entities = interest.update_entities([9, 3]);
        assert_eq!(entities.entered, vec![3, 9]);
        interest.update_simulation_entities([3]);
        assert!(interest.wants(Dimension::Overworld, InterestKind::Entity(9)));
        assert!(!interest.wants(Dimension::Overworld, InterestKind::EntityState(9)));
        assert!(interest.wants(Dimension::Overworld, InterestKind::EntityState(3)));

        let moved = interest.update_position(Dimension::Nether, [0.0, 64.0, 0.0]);
        assert_eq!(moved.departed.len(), initial.entered.len());
        assert_eq!(moved.entered.len(), initial.entered.len());
    }

    #[test]
    fn residency_hysteresis_matches_client_chebyshev_ring() {
        let chunks = residency_hysteresis_chunks([8.0, 64.0, 8.0], 2);
        let radius = 2 + RESIDENCY_HYSTERESIS;
        assert!(chunks.contains(&(0, 0)));
        assert!(chunks.contains(&(radius, 0)));
        assert!(chunks.contains(&(radius, radius)));
        assert!(!chunks.contains(&(radius + 1, 0)));
        assert!(!chunks.contains(&(8, 0)));
        assert_eq!(chunks.len(), ((radius * 2 + 1) * (radius * 2 + 1)) as usize);
    }

    #[test]
    fn spawn_residency_is_capped_and_simulation_union_is_per_session() {
        let spawn = capped_spawn_residency(8, 8);
        assert!(spawn.contains(&(0, 0)));
        assert!(spawn.len() <= SPAWN_RESIDENCY_CAP);
        assert_eq!(
            spawn.len(),
            ((SPAWN_RESIDENCY_RADIUS * 2 + 1) as usize).pow(2)
        );

        let mut near = InterestSet::new(Dimension::Overworld, 4, 1);
        near.update_position(Dimension::Overworld, [8.0, 64.0, 8.0]);
        let mut far = InterestSet::new(Dimension::Overworld, 4, 1);
        far.update_position(Dimension::Overworld, [32.0 * 16.0 + 8.0, 64.0, 8.0]);
        let union = union_simulation_chunks([&near, &far]);
        assert!(union.contains(&(0, 0)));
        assert!(union.contains(&(32, 0)));
        assert!(!union.contains(&(8, 0)));
    }

    #[test]
    fn stationary_position_skips_chunk_rebuild() {
        let mut interest = InterestSet::new(Dimension::Overworld, 2, 1);
        let first = interest.update_position(Dimension::Overworld, [1.0, 64.0, 1.0]);
        assert!(!first.entered.is_empty());
        assert_eq!(interest.chunk_rebuilds(), 1);
        let chunks_before = interest.chunks.len();

        // Same column, different sub-chunk pose: no enter/depart, no rebuild.
        let second = interest.update_position(Dimension::Overworld, [8.0, 64.0, 8.0]);
        assert!(second.entered.is_empty());
        assert!(second.departed.is_empty());
        assert_eq!(interest.chunk_rebuilds(), 1);
        assert_eq!(interest.chunks.len(), chunks_before);

        let third = interest.update_position(Dimension::Overworld, [12.0, 70.0, 3.0]);
        assert!(third.entered.is_empty());
        assert!(third.departed.is_empty());
        assert_eq!(interest.chunk_rebuilds(), 1);
    }

    #[test]
    fn chunk_cross_dimension_and_distance_force_rebuild() {
        let mut interest = InterestSet::new(Dimension::Overworld, 2, 1);
        interest.update_position(Dimension::Overworld, [8.0, 64.0, 8.0]);
        assert_eq!(interest.chunk_rebuilds(), 1);

        let crossed = interest.update_position(Dimension::Overworld, [24.0, 64.0, 8.0]);
        assert!(!crossed.entered.is_empty() || !crossed.departed.is_empty());
        assert_eq!(interest.chunk_rebuilds(), 2);

        let teleported = interest.update_position(Dimension::Nether, [24.0, 64.0, 8.0]);
        assert!(!teleported.entered.is_empty());
        assert!(!teleported.departed.is_empty());
        assert_eq!(interest.chunk_rebuilds(), 3);

        assert!(interest.set_distances(3, 1));
        let widened = interest.update_position(Dimension::Nether, [24.0, 64.0, 8.0]);
        assert!(!widened.entered.is_empty());
        assert_eq!(interest.chunk_rebuilds(), 4);
    }

    #[test]
    fn entity_update_is_incremental_without_take() {
        let mut interest = InterestSet::new(Dimension::Overworld, 2, 1);
        let first = interest.update_entities([1, 2, 3]);
        assert_eq!(first.entered, vec![1, 2, 3]);
        assert!(first.departed.is_empty());
        assert_eq!(interest.entities.len(), 3);

        let same = interest.update_entities([3, 1, 2]);
        assert!(same.entered.is_empty());
        assert!(same.departed.is_empty());
        assert_eq!(interest.entities.len(), 3);

        let changed = interest.update_entities([2, 4]);
        assert_eq!(changed.entered, vec![4]);
        assert_eq!(changed.departed, vec![1, 3]);
        assert_eq!(interest.entities, HashSet::from([2, 4]));
    }
}
