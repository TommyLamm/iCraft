//! Bounded frame-resource pooling for GPU work submitted asynchronously.
//!
//! The bookkeeping core deliberately has no wgpu dependency beyond the small
//! callback adapter at the bottom of this module. A slot is unavailable from
//! the moment it is acquired for a submission until that submission's
//! completion notification arrives.

use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotState {
    Available,
    InFlight(u64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameSlot {
    pub id: usize,
    pub state: SlotState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameLease {
    pub slot_id: usize,
    pub submission_id: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcquireError {
    /// Every configured slot is still owned by the GPU. Waiting for this
    /// submission (or otherwise stalling) is required before retrying.
    Exhausted { oldest_submission: u64 },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PoolTelemetry {
    pub in_flight: usize,
    pub stalls: u64,
    pub high_water: usize,
}

struct ResourceSlot<T> {
    resource: T,
    state: SlotState,
}

/// A bounded pool of per-frame resources (uniform/storage buffers, particle
/// arenas, and similar data). New slots are grown lazily up to `max_slots`.
pub struct FrameResourcePool<T> {
    slots: Vec<ResourceSlot<T>>,
    max_slots: usize,
    telemetry: PoolTelemetry,
}

impl<T> FrameResourcePool<T> {
    pub fn new(max_slots: usize) -> Self {
        assert!(
            max_slots > 0,
            "frame resource pool must have at least one slot"
        );
        Self {
            slots: Vec::new(),
            max_slots,
            telemetry: PoolTelemetry::default(),
        }
    }

    pub fn with_initial(max_slots: usize, initial: impl IntoIterator<Item = T>) -> Self {
        let mut pool = Self::new(max_slots);
        for resource in initial {
            if pool.slots.len() == max_slots {
                break;
            }
            pool.slots.push(ResourceSlot {
                resource,
                state: SlotState::Available,
            });
        }
        pool
    }

    pub fn max_slots(&self) -> usize {
        self.max_slots
    }
    pub fn len(&self) -> usize {
        self.slots.len()
    }
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }
    pub fn telemetry(&self) -> PoolTelemetry {
        self.telemetry
    }

    /// Acquires an available slot for a submission. The slot is immediately
    /// marked in-flight, preventing reuse even if completion is delayed.
    pub fn acquire(&mut self, submission_id: u64) -> Result<FrameLease, AcquireError> {
        if let Some((slot_id, slot)) = self
            .slots
            .iter_mut()
            .enumerate()
            .find(|(_, slot)| slot.state == SlotState::Available)
        {
            slot.state = SlotState::InFlight(submission_id);
            self.telemetry.in_flight += 1;
            self.telemetry.high_water = self.telemetry.high_water.max(self.telemetry.in_flight);
            return Ok(FrameLease {
                slot_id,
                submission_id,
            });
        }
        if self.slots.len() < self.max_slots {
            return Err(AcquireError::Exhausted {
                oldest_submission: 0,
            });
        }
        self.telemetry.stalls += 1;
        let oldest_submission = self
            .slots
            .iter()
            .filter_map(|slot| match slot.state {
                SlotState::InFlight(id) => Some(id),
                SlotState::Available => None,
            })
            .min()
            .expect("non-empty bounded pool has no available slot");
        Err(AcquireError::Exhausted { oldest_submission })
    }

    /// Acquires a slot, lazily creating its resource when capacity remains.
    /// This is the convenient entry point for callers whose frame resources
    /// are created on first use.
    pub fn acquire_or_create(
        &mut self,
        submission_id: u64,
        create: impl FnOnce() -> T,
    ) -> Result<FrameLease, AcquireError> {
        if self
            .slots
            .iter()
            .all(|slot| slot.state != SlotState::Available)
            && self.slots.len() < self.max_slots
        {
            self.slots.push(ResourceSlot {
                resource: create(),
                state: SlotState::Available,
            });
        }
        self.acquire(submission_id)
    }

    /// Adds a resource as a new available slot, typically after an exhausted
    /// acquisition has been handled by the caller. Returns `false` at capacity.
    pub fn push(&mut self, resource: T) -> bool {
        if self.slots.len() >= self.max_slots {
            return false;
        }
        self.slots.push(ResourceSlot {
            resource,
            state: SlotState::Available,
        });
        true
    }

    pub fn slot(&self, slot_id: usize) -> Option<FrameSlot> {
        self.slots.get(slot_id).map(|slot| FrameSlot {
            id: slot_id,
            state: slot.state,
        })
    }

    pub fn get(&self, slot_id: usize) -> Option<&T> {
        self.slots.get(slot_id).map(|s| &s.resource)
    }
    pub fn get_mut(&mut self, slot_id: usize) -> Option<&mut T> {
        self.slots.get_mut(slot_id).map(|s| &mut s.resource)
    }

    /// Reclaims exactly the slots associated with `submission_id`; completion
    /// notifications may arrive out of order.
    pub fn complete(&mut self, submission_id: u64) -> usize {
        let mut reclaimed = 0;
        for slot in &mut self.slots {
            if slot.state == SlotState::InFlight(submission_id) {
                slot.state = SlotState::Available;
                self.telemetry.in_flight -= 1;
                reclaimed += 1;
            }
        }
        reclaimed
    }
}

impl<T> Default for FrameResourcePool<T> {
    fn default() -> Self {
        Self::new(4)
    }
}

/// Registers a wgpu completion callback while keeping the core pool testable
/// without a device or queue.
pub fn register_submission_completion<T: Send + 'static>(
    queue: &wgpu::Queue,
    pool: Arc<Mutex<FrameResourcePool<T>>>,
    submission_id: u64,
) {
    queue.on_submitted_work_done(move || {
        if let Ok(mut pool) = pool.lock() {
            pool.complete(submission_id);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_pool_never_reuses_in_flight_slots() {
        let mut pool = FrameResourcePool::with_initial(2, [1, 2]);
        let a = pool.acquire(10).unwrap();
        let b = pool.acquire(11).unwrap();
        assert_eq!(a.slot_id, 0);
        assert_eq!(b.slot_id, 1);
        assert_eq!(
            pool.acquire(12),
            Err(AcquireError::Exhausted {
                oldest_submission: 10
            })
        );
        assert_eq!(pool.len(), 2);
        assert_eq!(pool.telemetry().stalls, 1);
        assert_eq!(pool.complete(11), 1);
        let c = pool.acquire(12).unwrap();
        assert_eq!(c.slot_id, 1);
        assert_eq!(pool.complete(10), 1);
    }

    #[test]
    fn completion_is_exact_and_idempotent() {
        let mut pool = FrameResourcePool::with_initial(4, [0, 1, 2, 3]);
        pool.acquire(7).unwrap();
        pool.acquire(8).unwrap();
        assert_eq!(pool.complete(99), 0);
        assert_eq!(pool.complete(8), 1);
        assert_eq!(pool.complete(8), 0);
        assert_eq!(pool.telemetry().in_flight, 1);
    }

    #[test]
    fn lazy_growth_is_bounded() {
        let mut pool = FrameResourcePool::new(2);
        let first = pool.acquire_or_create(1, || 10).unwrap();
        assert_eq!(pool.get(first.slot_id), Some(&10));
        pool.complete(1);
        assert!(pool.push(1));
        assert!(!pool.push(2));
        let mut full = FrameResourcePool::new(2);
        assert!(full.push(1));
        assert!(full.push(2));
        assert!(!full.push(3));
        assert_eq!(full.len(), 2);
    }
}
