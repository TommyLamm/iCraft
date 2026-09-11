//! Off-tick worldgen: Rayon generates columns; the authority tick applies them.

use crate::dimension::{generate_chunk_with_options, Dimension, WorldGenerationOptions};
use crate::game_rules::WorldType;
use crate::world::Chunk;
use std::collections::HashSet;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;

const MAX_WORLDGEN_IN_FLIGHT: usize = 32;

#[derive(Debug, Clone)]
pub(super) struct WorldgenJob {
    pub dimension: Dimension,
    pub chunk_x: i32,
    pub chunk_z: i32,
    pub seed: u32,
    pub world_type: WorldType,
    pub generate_structures: bool,
    pub generation: u64,
    pub lifetime: u64,
}

pub(super) struct WorldgenResult {
    pub dimension: Dimension,
    pub chunk_x: i32,
    pub chunk_z: i32,
    pub chunk: Chunk,
    pub generation: u64,
    pub lifetime: u64,
}

pub(super) struct WorldgenWorker {
    result_rx: Receiver<WorldgenResult>,
    result_tx: Arc<mpsc::Sender<WorldgenResult>>,
    in_flight: HashSet<(Dimension, i32, i32)>,
    /// Bumped to invalidate in-flight results (dimension teardown / mode flip).
    pub generation: u64,
    pub lifetime: u64,
}

impl WorldgenWorker {
    pub fn new() -> Self {
        let (result_tx, result_rx) = mpsc::channel();
        Self {
            result_rx,
            result_tx: Arc::new(result_tx),
            in_flight: HashSet::new(),
            generation: 1,
            lifetime: 1,
        }
    }

    pub fn bump_generation(&mut self) {
        self.generation = self.generation.wrapping_add(1).max(1);
        self.in_flight.clear();
    }

    pub fn bump_lifetime(&mut self) {
        self.lifetime = self.lifetime.wrapping_add(1).max(1);
        self.in_flight.clear();
    }

    pub fn schedule(&mut self, job: WorldgenJob) -> bool {
        let key = (job.dimension, job.chunk_x, job.chunk_z);
        if self.in_flight.contains(&key) || self.in_flight.len() >= MAX_WORLDGEN_IN_FLIGHT {
            return false;
        }
        self.in_flight.insert(key);
        let tx = Arc::clone(&self.result_tx);
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
                generation: job.generation,
                lifetime: job.lifetime,
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

    pub fn is_current(&self, result: &WorldgenResult) -> bool {
        result.generation == self.generation && result.lifetime == self.lifetime
    }
}
