//! Dimension-aware interest and replication routing for the headless authority.
//!
//! The renderer and network transport are consumers of this contract.  They
//! must not infer recipients from their own chunk caches: a session receives a
//! world delta only when this module says that its authenticated dimension and
//! interest set contain the changed object.

use crate::dimension::Dimension;
use std::collections::{BTreeSet, HashSet};

pub const MAX_INTEREST_UPDATES_PER_TICK: usize = 8_192;

pub type ChunkCoord = (i32, i32);
pub type BlockPosition = (i32, i32, i32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterestKind {
    Chunk(ChunkCoord),
    Entity(u64),
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
        }
    }

    pub fn update_position(&mut self, dimension: Dimension, position: [f32; 3]) {
        self.dimension = dimension;
        self.chunks = chunks_around(position, self.view_distance);
        self.simulation_chunks = chunks_around(position, self.simulation_distance);
        self.open_containers.retain(|position| {
            let chunk = (position.0.div_euclid(16), position.2.div_euclid(16));
            self.chunks.contains(&chunk)
        });
    }

    pub fn update_entities<I>(&mut self, entity_ids: I)
    where
        I: IntoIterator<Item = u64>,
    {
        self.entities = entity_ids.into_iter().collect();
    }

    pub fn update_simulation_entities<I>(&mut self, entity_ids: I)
    where
        I: IntoIterator<Item = u64>,
    {
        self.simulation_entities = entity_ids.into_iter().collect();
    }

    pub fn wants(&self, dimension: Dimension, kind: InterestKind) -> bool {
        if self.dimension != dimension {
            return false;
        }
        match kind {
            InterestKind::Chunk(coord) => self.chunks.contains(&coord),
            InterestKind::Entity(id) => self.simulation_entities.contains(&id),
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
}
