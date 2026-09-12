use crate::dimension::Dimension;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use super::format::MutationRevisionIndexCapacityError;

/// Hard admission limit for distinct mutated chunk coordinates.
///
/// Existing coordinates remain updatable at the limit; new coordinates are
/// refused explicitly so an unloaded chunk's revision is never evicted.
pub const MUTATION_REVISION_INDEX_CAPACITY: usize = 65_536;

pub fn default_mutation_revision_index_capacity() -> usize {
    MUTATION_REVISION_INDEX_CAPACITY
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct MutationRevisionIndex {
    revisions: HashMap<(Dimension, i32, i32), u64>,
    #[serde(skip, default = "default_mutation_revision_index_capacity")]
    capacity: usize,
}

impl Default for MutationRevisionIndex {
    fn default() -> Self {
        Self::with_capacity_limit(MUTATION_REVISION_INDEX_CAPACITY)
    }
}

impl MutationRevisionIndex {
    pub fn with_capacity_limit(capacity: usize) -> Self {
        Self {
            revisions: HashMap::new(),
            capacity,
        }
    }

    pub fn bump(
        &mut self,
        dimension: Dimension,
        cx: i32,
        cz: i32,
    ) -> Result<u64, MutationRevisionIndexCapacityError> {
        self.require_admission_capacity(dimension, cx, cz)?;
        let revision = self
            .revisions
            .get(&(dimension, cx, cz))
            .copied()
            .unwrap_or(0)
            .saturating_add(1);
        self.revisions.insert((dimension, cx, cz), revision);
        Ok(revision)
    }

    pub fn ensure_at_least(
        &mut self,
        dimension: Dimension,
        cx: i32,
        cz: i32,
        revision: u64,
    ) -> Result<bool, MutationRevisionIndexCapacityError> {
        self.require_admission_capacity(dimension, cx, cz)?;
        let entry = self.revisions.entry((dimension, cx, cz)).or_insert(0);
        if *entry >= revision {
            return Ok(false);
        }
        *entry = revision;
        Ok(true)
    }

    pub fn latest(&self, dimension: Dimension, cx: i32, cz: i32) -> u64 {
        self.revisions
            .get(&(dimension, cx, cz))
            .copied()
            .unwrap_or(0)
    }

    pub fn entries_in(&self, dimension: Dimension) -> impl Iterator<Item = ((i32, i32), u64)> + '_ {
        self.revisions
            .iter()
            .filter(move |((entry_dimension, _, _), _)| *entry_dimension == dimension)
            .map(|((_, cx, cz), revision)| ((*cx, *cz), *revision))
    }

    pub fn len(&self) -> usize {
        self.revisions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.revisions.is_empty()
    }

    pub fn capacity(&self) -> usize {
        self.capacity.max(self.revisions.len())
    }

    pub fn remove(&mut self, dimension: Dimension, cx: i32, cz: i32) -> Option<u64> {
        self.revisions.remove(&(dimension, cx, cz))
    }

    pub fn reclaim_through(
        &mut self,
        dimension: Dimension,
        cx: i32,
        cz: i32,
        acknowledged_revision: u64,
    ) -> bool {
        // A stale acknowledgement must not reclaim a newer mutation.
        let key = (dimension, cx, cz);
        match self.revisions.get(&key).copied() {
            Some(latest) if latest <= acknowledged_revision => {
                self.revisions.remove(&key);
                true
            }
            _ => false,
        }
    }

    fn require_admission_capacity(
        &self,
        dimension: Dimension,
        cx: i32,
        cz: i32,
    ) -> Result<(), MutationRevisionIndexCapacityError> {
        if self.revisions.contains_key(&(dimension, cx, cz))
            || self.revisions.len() < self.capacity()
        {
            Ok(())
        } else {
            Err(MutationRevisionIndexCapacityError {
                capacity: self.capacity(),
            })
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveState {
    Dirty(u64),
    InFlight(u64),
    Persisted(u64),
}

#[derive(Debug)]
pub(crate) struct DirtyChunkSetInner {
    id: u64,
    states: Mutex<HashMap<(i32, i32), SaveState>>,
}

static NEXT_DIRTY_SET_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone)]
pub struct DirtyChunkSet {
    inner: Arc<DirtyChunkSetInner>,
}

impl Default for DirtyChunkSet {
    fn default() -> Self {
        Self::new()
    }
}

impl DirtyChunkSet {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(DirtyChunkSetInner {
                id: NEXT_DIRTY_SET_ID.fetch_add(1, Ordering::Relaxed),
                states: Mutex::new(HashMap::new()),
            }),
        }
    }

    pub fn id(&self) -> u64 {
        self.inner.id
    }

    pub fn mark_dirty(&self, cx: i32, cz: i32) -> u64 {
        let mut states = self.inner.states.lock().unwrap_or_else(|e| e.into_inner());
        let next_revision = match states.get(&(cx, cz)).copied() {
            Some(SaveState::Dirty(revision))
            | Some(SaveState::InFlight(revision))
            | Some(SaveState::Persisted(revision)) => revision.saturating_add(1),
            None => 1,
        };
        states.insert((cx, cz), SaveState::Dirty(next_revision));
        next_revision
    }

    pub fn is_dirty(&self, cx: i32, cz: i32) -> bool {
        matches!(
            self.inner
                .states
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(&(cx, cz)),
            Some(SaveState::Dirty(_))
        )
    }

    pub fn remove(&self, cx: i32, cz: i32) -> bool {
        self.inner
            .states
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&(cx, cz))
            .is_some()
    }

    pub fn clear(&self) {
        self.inner
            .states
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }

    pub fn dirty_revisions(&self) -> Vec<((i32, i32), u64)> {
        self.inner
            .states
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter_map(|(&coord, &state)| match state {
                SaveState::Dirty(revision) => Some((coord, revision)),
                SaveState::InFlight(_) | SaveState::Persisted(_) => None,
            })
            .collect()
    }

    pub fn dirty_revision(&self, cx: i32, cz: i32) -> Option<u64> {
        match self
            .inner
            .states
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&(cx, cz))
            .copied()
        {
            Some(SaveState::Dirty(revision)) => Some(revision),
            Some(SaveState::InFlight(_)) | Some(SaveState::Persisted(_)) | None => None,
        }
    }

    pub fn begin_save(&self, cx: i32, cz: i32, revision: u64) -> bool {
        let mut states = self.inner.states.lock().unwrap_or_else(|e| e.into_inner());
        if states.get(&(cx, cz)) == Some(&SaveState::Dirty(revision)) {
            states.insert((cx, cz), SaveState::InFlight(revision));
            true
        } else {
            false
        }
    }

    pub fn acknowledge_persisted(&self, cx: i32, cz: i32, revision: u64) {
        let mut states = self.inner.states.lock().unwrap_or_else(|e| e.into_inner());
        if states.get(&(cx, cz)) == Some(&SaveState::InFlight(revision)) {
            states.insert((cx, cz), SaveState::Persisted(revision));
        }
    }

    pub fn acknowledge_failed(&self, cx: i32, cz: i32, revision: u64) {
        let mut states = self.inner.states.lock().unwrap_or_else(|e| e.into_inner());
        if states.get(&(cx, cz)) == Some(&SaveState::InFlight(revision)) {
            states.insert((cx, cz), SaveState::Dirty(revision));
        }
    }

    pub fn state(&self, cx: i32, cz: i32) -> Option<SaveState> {
        self.inner
            .states
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&(cx, cz))
            .copied()
    }

    pub fn len(&self) -> usize {
        self.inner
            .states
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .filter(|state| matches!(state, SaveState::Dirty(_)))
            .count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
