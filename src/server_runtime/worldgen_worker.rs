//! Off-tick worldgen: Rayon generates columns; the authority tick applies them.

use crate::dimension::{generate_chunk_with_options, Dimension, WorldGenerationOptions};
use crate::game_rules::WorldType;
use crate::world::Chunk;
use std::collections::HashSet;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};

const MAX_WORLDGEN_IN_FLIGHT: usize = 32;

#[derive(Debug, Clone)]
pub(super) struct WorldgenJob {
    pub dimension: Dimension,
    pub chunk_x: i32,
    pub chunk_z: i32,
    pub seed: u32,
    pub world_type: WorldType,
    pub generate_structures: bool,
}

pub(super) struct WorldgenResult {
    pub dimension: Dimension,
    pub chunk_x: i32,
    pub chunk_z: i32,
    pub chunk: Chunk,
}

pub(super) struct WorldgenWorker {
    result_rx: Receiver<WorldgenResult>,
    result_tx: Sender<WorldgenResult>,
    in_flight: HashSet<(Dimension, i32, i32)>,
}

impl WorldgenWorker {
    pub fn new() -> Self {
        let (result_tx, result_rx) = mpsc::channel();
        Self {
            result_rx,
            result_tx,
            in_flight: HashSet::new(),
        }
    }

    pub fn schedule(&mut self, job: WorldgenJob) -> bool {
        let key = (job.dimension, job.chunk_x, job.chunk_z);
        if self.in_flight.contains(&key) || self.in_flight.len() >= MAX_WORLDGEN_IN_FLIGHT {
            return false;
        }
        self.in_flight.insert(key);
        let tx = self.result_tx.clone();
        rayon::spawn(move || {
            let options = WorldGenerationOptions {
                world_type: job.world_type,
                generate_structures: job.generate_structures,
            };
            let chunk = generate_chunk_with_options(
                job.dimension,
                job.chunk_x,
                job.chunk_z,
                job.seed,
                options,
            );
            let _ = tx.send(WorldgenResult {
                dimension: job.dimension,
                chunk_x: job.chunk_x,
                chunk_z: job.chunk_z,
                chunk,
            });
        });
        true
    }

    pub fn poll_completed(&mut self) -> Vec<WorldgenResult> {
        let mut out = Vec::new();
        loop {
            match self.result_rx.try_recv() {
                Ok(result) => {
                    self.in_flight
                        .remove(&(result.dimension, result.chunk_x, result.chunk_z));
                    out.push(result);
                }
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
            }
        }
        out
    }

    #[cfg(test)]
    pub fn is_in_flight(&self, dimension: Dimension, chunk_x: i32, chunk_z: i32) -> bool {
        self.in_flight.contains(&(dimension, chunk_x, chunk_z))
    }

    #[cfg(test)]
    pub fn in_flight_count(&self) -> usize {
        self.in_flight.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_channels_are_isolated_between_instances() {
        let mut worker_a = WorldgenWorker::new();
        let mut worker_b = WorldgenWorker::new();

        let job_a = WorldgenJob {
            dimension: Dimension::Overworld,
            chunk_x: 10,
            chunk_z: 20,
            seed: 42,
            world_type: WorldType::Superflat,
            generate_structures: false,
        };
        assert!(worker_a.schedule(job_a));

        // Poll worker_b: worker_b should never receive worker_a's result.
        for _ in 0..10 {
            let completed_b = worker_b.poll_completed();
            assert!(
                completed_b.is_empty(),
                "worker_b should not receive worker_a's result"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        // Wait for worker_a to receive its completed job.
        let mut completed_a = Vec::new();
        for _ in 0..100 {
            completed_a.extend(worker_a.poll_completed());
            if !completed_a.is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(completed_a.len(), 1);
        assert_eq!(completed_a[0].chunk_x, 10);
        assert_eq!(completed_a[0].chunk_z, 20);
        assert!(worker_b.poll_completed().is_empty());
    }
}
