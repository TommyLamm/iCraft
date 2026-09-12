//! Fixed 3-slot in-flight tracker for GPU frame submissions.
//!
//! Desktop binary-only (`mod` in `src/main.rs`). Not part of the `icraft`
//! library, so `icraft-server` does not compile this file.
//!
//! A slot is unavailable from acquire until its submission completion
//! notification arrives. Resources themselves live on `State` (triple-buffered
//! instance buffers keyed by `frame_ring_index`); this pool only tracks which
//! of the three ring indices are still owned by the GPU.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotState {
    Available,
    InFlight(u64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameLease {
    pub slot_id: usize,
    pub submission_id: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcquireError {
    /// Every configured slot is still owned by the GPU. Callers should skip
    /// the frame rather than stall with `Maintain::Wait`.
    Exhausted { oldest_submission: u64 },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PoolTelemetry {
    pub in_flight: usize,
    pub stalls: u64,
    pub high_water: usize,
}

const SLOT_COUNT: usize = 3;

/// Fixed 3-slot ring of in-flight submission flags.
pub struct FrameResourcePool {
    slots: [SlotState; SLOT_COUNT],
    telemetry: PoolTelemetry,
}

impl FrameResourcePool {
    pub fn new() -> Self {
        Self {
            slots: [SlotState::Available; SLOT_COUNT],
            telemetry: PoolTelemetry::default(),
        }
    }

    pub fn max_slots(&self) -> usize {
        SLOT_COUNT
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
            .find(|(_, slot)| **slot == SlotState::Available)
        {
            *slot = SlotState::InFlight(submission_id);
            self.telemetry.in_flight += 1;
            self.telemetry.high_water = self.telemetry.high_water.max(self.telemetry.in_flight);
            return Ok(FrameLease {
                slot_id,
                submission_id,
            });
        }
        self.telemetry.stalls += 1;
        let oldest_submission = self
            .slots
            .iter()
            .filter_map(|slot| match slot {
                SlotState::InFlight(id) => Some(*id),
                SlotState::Available => None,
            })
            .min()
            .expect("full 3-slot pool has no available slot");
        Err(AcquireError::Exhausted { oldest_submission })
    }

    /// Reclaims exactly the slots associated with `submission_id`; completion
    /// notifications may arrive out of order.
    pub fn complete(&mut self, submission_id: u64) -> usize {
        let mut reclaimed = 0;
        for slot in &mut self.slots {
            if *slot == SlotState::InFlight(submission_id) {
                *slot = SlotState::Available;
                self.telemetry.in_flight -= 1;
                reclaimed += 1;
            }
        }
        reclaimed
    }
}

impl Default for FrameResourcePool {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_pool_never_reuses_in_flight_slots() {
        let mut pool = FrameResourcePool::new();
        let a = pool.acquire(10).unwrap();
        let b = pool.acquire(11).unwrap();
        let c = pool.acquire(12).unwrap();
        assert_eq!(a.slot_id, 0);
        assert_eq!(b.slot_id, 1);
        assert_eq!(c.slot_id, 2);
        assert_eq!(
            pool.acquire(13),
            Err(AcquireError::Exhausted {
                oldest_submission: 10
            })
        );
        assert_eq!(pool.telemetry().stalls, 1);
        assert_eq!(pool.complete(11), 1);
        let d = pool.acquire(13).unwrap();
        assert_eq!(d.slot_id, 1);
        assert_eq!(pool.complete(10), 1);
    }

    #[test]
    fn completion_is_exact_and_idempotent() {
        let mut pool = FrameResourcePool::new();
        pool.acquire(7).unwrap();
        pool.acquire(8).unwrap();
        assert_eq!(pool.complete(99), 0);
        assert_eq!(pool.complete(8), 1);
        assert_eq!(pool.complete(8), 0);
        assert_eq!(pool.telemetry().in_flight, 1);
    }
}
