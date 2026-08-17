use crate::dimension::Dimension;
use crate::world::{BlockType, Chunk};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use super::format::{
    CHUNK_SAVE_DATA_VERSION, ChunkSaveData, LevelData, PlayerData, SaveError, SaveResult,
};
use super::index::{DirtyChunkSet, MutationRevisionIndex};
use super::region::compress_bytes;
use super::SaveManager;

pub const SAVE_QUEUE_CAPACITY: usize = 128;
pub const NETWORK_SNAPSHOT_QUEUE_CAPACITY: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NetworkSnapshotKey {
    pub player_id: crate::network::protocol::PlayerId,
    pub dimension: Dimension,
    pub cx: i32,
    pub cz: i32,
    pub revision: u64,
}

pub struct NetworkSnapshotRequest {
    pub key: NetworkSnapshotKey,
    pub chunk: Option<Arc<Chunk>>,
    /// When false, the worker must not load from this presentation
    /// `SaveManager`'s independent region cache. Listen-host catch-up uses
    /// the in-process `ServerRuntime` as the only authority.
    pub allow_disk_fallback: bool,
}

pub struct NetworkSnapshotPayload {
    pub key: NetworkSnapshotKey,
    pub result: Result<(Vec<u8>, Vec<u8>, Vec<u8>, i8, u16), String>,
}

pub enum NetworkSnapshotWorkerResult {
    Snapshot(NetworkSnapshotPayload),
    IndexPersisted {
        generation: u64,
        result: Result<(), String>,
    },
}

enum NetworkSnapshotWorkerCommand {
    Snapshot(NetworkSnapshotRequest),
    PersistIndex {
        generation: u64,
        index: MutationRevisionIndex,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkSnapshotSubmitError {
    Full,
    Closed,
}

/// Leftover Host join catch-up encoder. Not an authority save path.
///
/// Embedded Singleplayer / listen-host do not construct this worker;
/// `process_join_catchups` already returns when `has_in_process_runtime()`.
/// `ServerRuntime` is the only writer of `mutation_revisions.bin`.
pub struct NetworkSnapshotWorker {
    tx: std::sync::mpsc::SyncSender<NetworkSnapshotWorkerCommand>,
    rx: std::sync::mpsc::Receiver<NetworkSnapshotWorkerResult>,
}

impl NetworkSnapshotWorker {
    pub fn try_submit(
        &self,
        request: NetworkSnapshotRequest,
    ) -> Result<(), NetworkSnapshotSubmitError> {
        self.tx
            .try_send(NetworkSnapshotWorkerCommand::Snapshot(request))
            .map_err(|error| match error {
                std::sync::mpsc::TrySendError::Full(_) => NetworkSnapshotSubmitError::Full,
                std::sync::mpsc::TrySendError::Disconnected(_) => {
                    NetworkSnapshotSubmitError::Closed
                }
            })
    }

    pub fn try_persist_index(
        &self,
        generation: u64,
        index: MutationRevisionIndex,
    ) -> Result<(), NetworkSnapshotSubmitError> {
        self.tx
            .try_send(NetworkSnapshotWorkerCommand::PersistIndex { generation, index })
            .map_err(|error| match error {
                std::sync::mpsc::TrySendError::Full(_) => NetworkSnapshotSubmitError::Full,
                std::sync::mpsc::TrySendError::Disconnected(_) => {
                    NetworkSnapshotSubmitError::Closed
                }
            })
    }

    pub fn try_iter(&self) -> std::sync::mpsc::TryIter<'_, NetworkSnapshotWorkerResult> {
        self.rx.try_iter()
    }
}

pub fn spawn_network_snapshot_worker(
    manager: Arc<Mutex<SaveManager>>,
    capacity: usize,
) -> NetworkSnapshotWorker {
    let (tx, worker_rx) = std::sync::mpsc::sync_channel(capacity.max(1));
    let (result_tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("icraft-network-snapshot".into())
        .spawn(move || {
            while let Ok(command) = worker_rx.recv() {
                let result = match command {
                    NetworkSnapshotWorkerCommand::Snapshot(request) => {
                        let data = if let Some(ref chunk) = request.chunk {
                            ChunkSaveData::from_chunk(chunk).ok().map(|mut data| {
                                data.mutation_revision = request.key.revision;
                                data
                            })
                        } else if request.allow_disk_fallback {
                            manager
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .load_chunk_in(
                                    request.key.dimension,
                                    request.key.cx,
                                    request.key.cz,
                                )
                        } else {
                            None
                        };
                        let result = match data {
                            Some(data) if data.mutation_revision >= request.key.revision => {
                                let min_section_y = request
                                    .chunk
                                    .as_ref()
                                    .map(|c| c.min_section_y)
                                    .unwrap_or(0);
                                let section_count = request
                                    .chunk
                                    .as_ref()
                                    .map(|c| c.sections.len() as u16)
                                    .unwrap_or(0);
                                Ok((
                                    data.blocks,
                                    data.block_states,
                                    data.block_entities,
                                    min_section_y,
                                    section_count,
                                ))
                            }
                            Some(data) => Err(format!(
                                "persisted snapshot for {:?} chunk ({}, {}) is revision {}, waiting for {}",
                                request.key.dimension,
                                request.key.cx,
                                request.key.cz,
                                data.mutation_revision,
                                request.key.revision
                            )),
                            None => Err(format!(
                                "snapshot source unavailable for {:?} chunk ({}, {})",
                                request.key.dimension, request.key.cx, request.key.cz
                            )),
                        };
                        NetworkSnapshotWorkerResult::Snapshot(NetworkSnapshotPayload {
                            key: request.key,
                            result,
                        })
                    }
                    NetworkSnapshotWorkerCommand::PersistIndex { generation, index } => {
                        let result = manager
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .save_mutation_revision_index(&index)
                            .map_err(|error| error.to_string());
                        NetworkSnapshotWorkerResult::IndexPersisted { generation, result }
                    }
                };
                if result_tx.send(result).is_err() {
                    break;
                }
            }
        })
        .expect("failed to spawn network snapshot worker");
    NetworkSnapshotWorker { tx, rx }
}

#[derive(Clone)]
pub struct UncompressedChunkSnapshot {
    pub dimension: Dimension,
    pub chunk_x: i32,
    pub chunk_z: i32,
    pub blocks: Box<
        [[[BlockType; crate::world::CHUNK_DEPTH]; crate::world::CHUNK_HEIGHT];
            crate::world::CHUNK_WIDTH],
    >,
    pub block_states: Box<
        [[[u8; crate::world::CHUNK_DEPTH]; crate::world::CHUNK_HEIGHT]; crate::world::CHUNK_WIDTH],
    >,
    pub sky_light: Box<
        [[[u8; crate::world::CHUNK_DEPTH]; crate::world::CHUNK_HEIGHT]; crate::world::CHUNK_WIDTH],
    >,
    pub block_light: Box<
        [[[u8; crate::world::CHUNK_DEPTH]; crate::world::CHUNK_HEIGHT]; crate::world::CHUNK_WIDTH],
    >,
    pub fluid_levels: Box<
        [[[u8; crate::world::CHUNK_DEPTH]; crate::world::CHUNK_HEIGHT]; crate::world::CHUNK_WIDTH],
    >,
    pub redstone_metadata: Vec<crate::redstone::RedstoneComponentMetadata>,
    /// Block entities must travel with the immutable snapshot. Keeping this
    /// in the autosave/unload path prevents container inventories and pending
    /// observer pulses from disappearing when a chunk leaves memory.
    pub block_entities: Vec<((u8, i16, u8), crate::block_entity::BlockEntity)>,
    pub mutation_revision: u64,
}

impl UncompressedChunkSnapshot {
    pub fn from_chunk_with_redstone(
        dimension: Dimension,
        chunk: &Chunk,
        redstone_metadata: Vec<crate::redstone::RedstoneComponentMetadata>,
    ) -> Self {
        let mut blocks: Box<
            [[[BlockType; crate::world::CHUNK_DEPTH]; crate::world::CHUNK_HEIGHT];
                crate::world::CHUNK_WIDTH],
        > = vec![
            [[BlockType::Air; crate::world::CHUNK_DEPTH]; crate::world::CHUNK_HEIGHT];
            crate::world::CHUNK_WIDTH
        ]
        .try_into()
        .unwrap();
        let mut block_states: Box<
            [[[u8; crate::world::CHUNK_DEPTH]; crate::world::CHUNK_HEIGHT];
                crate::world::CHUNK_WIDTH],
        > = vec![
            [[0u8; crate::world::CHUNK_DEPTH]; crate::world::CHUNK_HEIGHT];
            crate::world::CHUNK_WIDTH
        ]
        .try_into()
        .unwrap();
        let mut sky_light: Box<
            [[[u8; crate::world::CHUNK_DEPTH]; crate::world::CHUNK_HEIGHT];
                crate::world::CHUNK_WIDTH],
        > = vec![
            [[0u8; crate::world::CHUNK_DEPTH]; crate::world::CHUNK_HEIGHT];
            crate::world::CHUNK_WIDTH
        ]
        .try_into()
        .unwrap();
        let mut block_light: Box<
            [[[u8; crate::world::CHUNK_DEPTH]; crate::world::CHUNK_HEIGHT];
                crate::world::CHUNK_WIDTH],
        > = vec![
            [[0u8; crate::world::CHUNK_DEPTH]; crate::world::CHUNK_HEIGHT];
            crate::world::CHUNK_WIDTH
        ]
        .try_into()
        .unwrap();
        let mut fluid_levels: Box<
            [[[u8; crate::world::CHUNK_DEPTH]; crate::world::CHUNK_HEIGHT];
                crate::world::CHUNK_WIDTH],
        > = vec![
            [[0u8; crate::world::CHUNK_DEPTH]; crate::world::CHUNK_HEIGHT];
            crate::world::CHUNK_WIDTH
        ]
        .try_into()
        .unwrap();

        for x in 0..crate::world::CHUNK_WIDTH {
            for y in 0..crate::world::CHUNK_HEIGHT {
                for z in 0..crate::world::CHUNK_DEPTH {
                    blocks[x][y][z] = chunk.get_block_local(x, y as i32, z);
                    block_states[x][y][z] = chunk.get_block_state(x as i32, y as i32, z as i32);
                    sky_light[x][y][z] = chunk.get_sky_light(x, y as i32, z);
                    block_light[x][y][z] = chunk.get_block_light(x, y as i32, z);
                    fluid_levels[x][y][z] = chunk.get_fluid_level(x, y as i32, z);
                }
            }
        }

        Self {
            dimension,
            chunk_x: chunk.chunk_x,
            chunk_z: chunk.chunk_z,
            blocks,
            block_states,
            sky_light,
            block_light,
            fluid_levels,
            redstone_metadata,
            block_entities: chunk
                .iter_block_entities()
                .map(|(pos, entity)| (pos, entity.clone()))
                .collect(),
            mutation_revision: 0,
        }
    }

    pub fn with_mutation_revision(mut self, revision: u64) -> Self {
        self.mutation_revision = revision;
        self
    }

    pub fn to_chunk_save_data(&self) -> SaveResult<ChunkSaveData> {
        self.try_to_chunk_save_data()
    }

    pub fn try_to_chunk_save_data(&self) -> SaveResult<ChunkSaveData> {
        let mut blocks = Vec::with_capacity(16 * 256 * 16);
        let mut block_states_raw = Vec::with_capacity(16 * 256 * 16);
        let mut sky_light = Vec::with_capacity(16 * 256 * 16);
        let mut block_light = Vec::with_capacity(16 * 256 * 16);
        let mut fluid_levels = Vec::with_capacity(16 * 256 * 16);

        for x in 0..16 {
            for y in 0..256 {
                for z in 0..16 {
                    blocks.push(self.blocks[x][y][z] as u8);
                    block_states_raw.push(self.block_states[x][y][z]);
                    sky_light.push(self.sky_light[x][y][z]);
                    block_light.push(self.block_light[x][y][z]);
                    fluid_levels.push(self.fluid_levels[x][y][z]);
                }
            }
        }

        let redstone_metadata_bytes = if self.redstone_metadata.is_empty() {
            Vec::new()
        } else {
            bincode::serialize(&self.redstone_metadata)
                .map_err(|error| SaveError::Serialization(error.to_string()))
                .and_then(|bytes| {
                    compress_bytes(&bytes)
                        .map_err(|error| SaveError::Serialization(error.to_string()))
                })?
        };

        let block_entities_bytes = if self.block_entities.is_empty() {
            Vec::new()
        } else {
            bincode::serialize(&self.block_entities)
                .map_err(|error| SaveError::Serialization(error.to_string()))
                .and_then(|bytes| {
                    compress_bytes(&bytes)
                        .map_err(|error| SaveError::Serialization(error.to_string()))
                })?
        };

        Ok(ChunkSaveData {
            chunk_x: self.chunk_x,
            chunk_z: self.chunk_z,
            blocks: compress_bytes(&blocks)
                .map_err(|error| SaveError::Serialization(error.to_string()))?,
            sky_light: compress_bytes(&sky_light)
                .map_err(|error| SaveError::Serialization(error.to_string()))?,
            block_light: compress_bytes(&block_light)
                .map_err(|error| SaveError::Serialization(error.to_string()))?,
            fluid_levels: compress_bytes(&fluid_levels)
                .map_err(|error| SaveError::Serialization(error.to_string()))?,
            redstone_metadata: redstone_metadata_bytes,
            block_states: compress_bytes(&block_states_raw)
                .map_err(|error| SaveError::Serialization(error.to_string()))?,
            mutation_revision: self.mutation_revision,
            block_entities: block_entities_bytes,
            data_version: CHUNK_SAVE_DATA_VERSION,
        })
    }

    pub fn estimated_bytes(&self) -> u64 {
        let voxel_count =
            crate::world::CHUNK_WIDTH * crate::world::CHUNK_HEIGHT * crate::world::CHUNK_DEPTH;
        (voxel_count * (std::mem::size_of::<BlockType>() + 4 * std::mem::size_of::<u8>())
            + self.redstone_metadata.len()
                * std::mem::size_of::<crate::redstone::RedstoneComponentMetadata>()
            + self.block_entities.len() * std::mem::size_of::<crate::block_entity::BlockEntity>())
            as u64
    }
}

pub enum SaveCommand {
    SaveChunk {
        snapshot: UncompressedChunkSnapshot,
        revision: u64,
        tracker: DirtyChunkSet,
    },
    SaveLevelAndPlayer(LevelData, PlayerData),
    Flush(std::sync::mpsc::Sender<SaveResult<()>>),
}

pub(crate) type SaveKey = (Dimension, i32, i32, u64);

#[derive(Clone)]
pub(crate) struct PendingChunkSave {
    pub(crate) snapshot: UncompressedChunkSnapshot,
    pub(crate) revision: u64,
    pub(crate) tracker: DirtyChunkSet,
    pub(crate) bytes: u64,
}

impl PendingChunkSave {
    pub(crate) fn key(&self) -> SaveKey {
        (
            self.snapshot.dimension,
            self.snapshot.chunk_x,
            self.snapshot.chunk_z,
            self.tracker.id(),
        )
    }

    pub(crate) fn acknowledge_persisted(&self) {
        self.tracker.acknowledge_persisted(
            self.snapshot.chunk_x,
            self.snapshot.chunk_z,
            self.revision,
        );
    }

    pub(crate) fn acknowledge_failed(&self) {
        self.tracker.acknowledge_failed(
            self.snapshot.chunk_x,
            self.snapshot.chunk_z,
            self.revision,
        );
    }
}

#[derive(Debug, Default)]
pub struct SaveQueueStats {
    queued_items: AtomicU64,
    queued_bytes: AtomicU64,
    in_flight: AtomicU64,
    in_flight_bytes: AtomicU64,
    dropped: AtomicU64,
    retries: AtomicU64,
    cancels: AtomicU64,
}

impl SaveQueueStats {
    pub fn depth(&self) -> u64 {
        self.queued_items.load(Ordering::Relaxed) + self.in_flight.load(Ordering::Relaxed)
    }

    pub fn queued_bytes(&self) -> u64 {
        self.queued_bytes.load(Ordering::Relaxed)
    }

    pub fn in_flight(&self) -> u64 {
        self.in_flight.load(Ordering::Relaxed)
    }

    pub fn in_flight_bytes(&self) -> u64 {
        self.in_flight_bytes.load(Ordering::Relaxed)
    }

    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
    pub fn retries(&self) -> u64 {
        self.retries.load(Ordering::Relaxed)
    }
    pub fn cancels(&self) -> u64 {
        self.cancels.load(Ordering::Relaxed)
    }
    pub(crate) fn record_retry(&self) {
        self.retries.fetch_add(1, Ordering::Relaxed);
    }
    pub(crate) fn record_cancel(&self) {
        self.cancels.fetch_add(1, Ordering::Relaxed);
    }
}

#[derive(Default)]
pub(crate) struct SaveQueueState {
    pub(crate) pending_chunks: HashMap<SaveKey, PendingChunkSave>,
    pub(crate) failed_chunks: HashMap<SaveKey, PendingChunkSave>,
    pub(crate) pending_level_player: Option<(LevelData, PlayerData)>,
    pub(crate) failed_level_player: Option<(LevelData, PlayerData)>,
    pub(crate) flush_waiters: VecDeque<std::sync::mpsc::Sender<SaveResult<()>>>,
    pub(crate) flush_error: Option<SaveError>,
    pub(crate) closed: bool,
}

impl SaveQueueState {
    pub(crate) fn work_items(&self) -> usize {
        self.pending_chunks.len()
            + self.failed_chunks.len()
            + usize::from(self.pending_level_player.is_some())
            + usize::from(self.failed_level_player.is_some())
    }
}

pub(crate) struct SaveQueueInner {
    pub(crate) state: Mutex<SaveQueueState>,
    pub(crate) work_available: Condvar,
    pub(crate) capacity_available: Condvar,
    pub(crate) capacity: usize,
    pub(crate) stats: Arc<SaveQueueStats>,
    pub(crate) last_error: Mutex<Option<SaveError>>,
    pub(crate) producers: AtomicU64,
}

/// Desktop bounded latest-wins leftover save worker.
///
/// Live Singleplayer / Host persist through `ServerRuntime::save_all`.
/// This queue remains for leftover `LegacyOwner` construction and unit tests.
pub struct SaveQueue {
    pub(crate) inner: Arc<SaveQueueInner>,
}

impl Clone for SaveQueue {
    fn clone(&self) -> Self {
        self.inner.producers.fetch_add(1, Ordering::Relaxed);
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl Drop for SaveQueue {
    fn drop(&mut self) {
        if self.inner.producers.fetch_sub(1, Ordering::AcqRel) != 1 {
            return;
        }
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        state.closed = true;
        for waiter in state.flush_waiters.drain(..) {
            let _ = waiter.send(Err(SaveError::QueueClosed));
        }
        self.inner.work_available.notify_all();
        self.inner.capacity_available.notify_all();
    }
}

impl SaveQueue {
    pub fn send(&self, command: SaveCommand) -> SaveResult<()> {
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if state.closed {
            drop(state);
            if let SaveCommand::SaveChunk {
                snapshot,
                revision,
                tracker,
            } = command
            {
                tracker.acknowledge_failed(snapshot.chunk_x, snapshot.chunk_z, revision);
            }
            return Err(SaveError::QueueClosed);
        }

        match command {
            SaveCommand::SaveChunk {
                snapshot,
                revision,
                tracker,
            } => {
                let task = PendingChunkSave {
                    bytes: snapshot.estimated_bytes(),
                    snapshot,
                    revision,
                    tracker,
                };
                let key = task.key();

                if let Some(failed) = state.failed_chunks.remove(&key) {
                    self.inner
                        .stats
                        .queued_bytes
                        .fetch_sub(failed.bytes, Ordering::Relaxed);
                    self.inner
                        .stats
                        .queued_items
                        .fetch_sub(1, Ordering::Relaxed);
                    self.inner.stats.dropped.fetch_add(1, Ordering::Relaxed);
                }

                if let Some(existing) = state.pending_chunks.get(&key) {
                    if existing.revision > revision {
                        self.inner.stats.dropped.fetch_add(1, Ordering::Relaxed);
                        return Ok(());
                    }
                } else {
                    while state.work_items()
                        + self.inner.stats.in_flight.load(Ordering::Relaxed) as usize
                        >= self.inner.capacity
                        && !state.closed
                    {
                        state = self
                            .inner
                            .capacity_available
                            .wait(state)
                            .unwrap_or_else(|error| error.into_inner());
                    }
                    if state.closed {
                        task.acknowledge_failed();
                        return Err(SaveError::QueueClosed);
                    }
                    self.inner
                        .stats
                        .queued_items
                        .fetch_add(1, Ordering::Relaxed);
                }

                if let Some(replaced) = state.pending_chunks.insert(key, task) {
                    self.inner
                        .stats
                        .queued_bytes
                        .fetch_sub(replaced.bytes, Ordering::Relaxed);
                    self.inner.stats.dropped.fetch_add(1, Ordering::Relaxed);
                }
                let bytes = state.pending_chunks.get(&key).unwrap().bytes;
                self.inner
                    .stats
                    .queued_bytes
                    .fetch_add(bytes, Ordering::Relaxed);
            }
            SaveCommand::SaveLevelAndPlayer(level, player) => {
                if state.pending_level_player.is_none() && state.failed_level_player.is_none() {
                    while state.work_items()
                        + self.inner.stats.in_flight.load(Ordering::Relaxed) as usize
                        >= self.inner.capacity
                        && !state.closed
                    {
                        state = self
                            .inner
                            .capacity_available
                            .wait(state)
                            .unwrap_or_else(|error| error.into_inner());
                    }
                    if state.closed {
                        return Err(SaveError::QueueClosed);
                    }
                    self.inner
                        .stats
                        .queued_items
                        .fetch_add(1, Ordering::Relaxed);
                } else {
                    self.inner.stats.dropped.fetch_add(1, Ordering::Relaxed);
                }
                state.failed_level_player = None;
                state.pending_level_player = Some((level, player));
            }
            SaveCommand::Flush(waiter) => {
                if state.pending_chunks.is_empty()
                    && state.failed_chunks.is_empty()
                    && state.pending_level_player.is_none()
                    && state.failed_level_player.is_none()
                {
                    state.flush_error = None;
                }
                if state.pending_chunks.is_empty() {
                    let failed = std::mem::take(&mut state.failed_chunks);
                    for (key, task) in failed {
                        if task.tracker.begin_save(
                            task.snapshot.chunk_x,
                            task.snapshot.chunk_z,
                            task.revision,
                        ) {
                            state.pending_chunks.insert(key, task);
                        } else {
                            self.inner
                                .stats
                                .queued_items
                                .fetch_sub(1, Ordering::Relaxed);
                            self.inner
                                .stats
                                .queued_bytes
                                .fetch_sub(task.bytes, Ordering::Relaxed);
                        }
                    }
                }
                if state.pending_level_player.is_none() {
                    state.pending_level_player = state.failed_level_player.take();
                }
                state.flush_waiters.push_back(waiter);
            }
        }
        self.inner.work_available.notify_one();
        Ok(())
    }

    pub fn stats(&self) -> Arc<SaveQueueStats> {
        Arc::clone(&self.inner.stats)
    }

    pub fn last_error(&self) -> Option<SaveError> {
        self.inner
            .last_error
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }
}

#[derive(Clone)]
pub(crate) struct SaveBatch {
    pub(crate) chunks: Vec<PendingChunkSave>,
    pub(crate) level_player: Option<(LevelData, PlayerData)>,
}

pub fn spawn_save_worker(manager: Arc<Mutex<SaveManager>>, capacity: usize) -> SaveQueue {
    let inner = Arc::new(SaveQueueInner {
        state: Mutex::new(SaveQueueState::default()),
        work_available: Condvar::new(),
        capacity_available: Condvar::new(),
        capacity: capacity.max(1),
        stats: Arc::new(SaveQueueStats::default()),
        last_error: Mutex::new(None),
        producers: AtomicU64::new(1),
    });
    let queue = SaveQueue {
        inner: Arc::clone(&inner),
    };

    std::thread::Builder::new()
        .name("icraft-save".to_string())
        .spawn(move || run_save_worker(inner, manager))
        .expect("failed to spawn save worker");
    queue
}

fn run_save_worker(inner: Arc<SaveQueueInner>, manager: Arc<Mutex<SaveManager>>) {
    loop {
        let batch = {
            let mut state = inner
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            while state.pending_chunks.is_empty()
                && state.pending_level_player.is_none()
                && state.flush_waiters.is_empty()
                && !state.closed
            {
                state = inner
                    .work_available
                    .wait(state)
                    .unwrap_or_else(|error| error.into_inner());
            }
            if state.closed {
                return;
            }

            let chunks: Vec<_> = state.pending_chunks.drain().map(|(_, task)| task).collect();
            let level_player = state.pending_level_player.take();
            let item_count = chunks.len() + usize::from(level_player.is_some());
            let byte_count = chunks.iter().map(|task| task.bytes).sum::<u64>();
            inner
                .stats
                .queued_items
                .fetch_sub(item_count as u64, Ordering::Relaxed);
            inner
                .stats
                .queued_bytes
                .fetch_sub(byte_count, Ordering::Relaxed);
            inner
                .stats
                .in_flight
                .fetch_add(item_count as u64, Ordering::Relaxed);
            inner
                .stats
                .in_flight_bytes
                .fetch_add(byte_count, Ordering::Relaxed);
            inner.capacity_available.notify_all();
            SaveBatch {
                chunks,
                level_player,
            }
        };

        let panic_backup = batch.clone();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            persist_save_batch(&manager, batch)
        }));
        let (failed_chunks, failed_level_player, error, completed_items, completed_bytes) =
            match result {
                Ok(outcome) => outcome,
                Err(payload) => {
                    let message = payload
                        .downcast_ref::<&str>()
                        .map(|message| (*message).to_string())
                        .or_else(|| payload.downcast_ref::<String>().cloned())
                        .unwrap_or_else(|| "unknown panic".to_string());
                    for task in &panic_backup.chunks {
                        task.acknowledge_failed();
                    }
                    let completed_items = panic_backup.chunks.len()
                        + usize::from(panic_backup.level_player.is_some());
                    let completed_bytes = panic_backup.chunks.iter().map(|task| task.bytes).sum();
                    (
                        panic_backup.chunks,
                        panic_backup.level_player,
                        Some(SaveError::WorkerPanic(message)),
                        completed_items,
                        completed_bytes,
                    )
                }
            };

        inner
            .stats
            .in_flight
            .fetch_sub(completed_items as u64, Ordering::Relaxed);
        inner
            .stats
            .in_flight_bytes
            .fetch_sub(completed_bytes as u64, Ordering::Relaxed);
        inner.capacity_available.notify_all();

        let mut state = inner
            .state
            .lock()
            .unwrap_or_else(|lock_error| lock_error.into_inner());
        for task in failed_chunks {
            inner.stats.record_retry();
            let key = task.key();
            if state.failed_chunks.insert(key, task).is_none() {
                inner.stats.queued_items.fetch_add(1, Ordering::Relaxed);
            }
            let bytes = state.failed_chunks.get(&key).unwrap().bytes;
            inner.stats.queued_bytes.fetch_add(bytes, Ordering::Relaxed);
        }
        if let Some(level_player) = failed_level_player {
            if state.failed_level_player.replace(level_player).is_none() {
                inner.stats.queued_items.fetch_add(1, Ordering::Relaxed);
            }
        }

        if let Some(error) = &error {
            *inner
                .last_error
                .lock()
                .unwrap_or_else(|lock_error| lock_error.into_inner()) = Some(error.clone());
            eprintln!("[Save] {error}");
            if !state.flush_waiters.is_empty() && state.flush_error.is_none() {
                state.flush_error = Some(error.clone());
            }
        }

        if state.pending_chunks.is_empty() && state.pending_level_player.is_none() {
            let flush_result = match &state.flush_error {
                Some(err) => Err(err.clone()),
                None => Ok(()),
            };
            if flush_result.is_ok()
                && state.failed_chunks.is_empty()
                && state.failed_level_player.is_none()
            {
                *inner
                    .last_error
                    .lock()
                    .unwrap_or_else(|lock_error| lock_error.into_inner()) = None;
            }
            if !state.flush_waiters.is_empty() {
                if flush_result.is_err() {
                    state.flush_error = None;
                }
                for waiter in state.flush_waiters.drain(..) {
                    let _ = waiter.send(flush_result.clone());
                }
            }
        }
    }
}

fn persist_save_batch(
    manager: &Arc<Mutex<SaveManager>>,
    batch: SaveBatch,
) -> (
    Vec<PendingChunkSave>,
    Option<(LevelData, PlayerData)>,
    Option<SaveError>,
    usize,
    u64,
) {
    let completed_items = batch.chunks.len() + usize::from(batch.level_player.is_some());
    let completed_bytes = batch.chunks.iter().map(|task| task.bytes).sum();
    let mut first_error = None;
    let mut failed_chunks = Vec::new();
    let mut failed_level_player = None;
    let mut manager = manager.lock().unwrap_or_else(|error| error.into_inner());

    #[cfg(test)]
    if std::mem::take(&mut manager.panic_next_worker_save) {
        panic!("injected save worker panic");
    }

    if let Some((level, player)) = batch.level_player {
        if let Err(error) = manager.save_player_and_level(&level, &player) {
            first_error.get_or_insert_with(|| {
                SaveError::io("save level and player", manager.world_dir.clone(), &error)
            });
            failed_level_player = Some((level, player));
        }
    }

    let mut groups: HashMap<(Dimension, i32, i32), Vec<PendingChunkSave>> =
        HashMap::new();
    for task in batch.chunks {
        groups
            .entry((
                task.snapshot.dimension,
                task.snapshot.chunk_x.div_euclid(32),
                task.snapshot.chunk_z.div_euclid(32),
            ))
            .or_default()
            .push(task);
    }

    for ((dimension, _, _), tasks) in groups {
        let snapshots: Vec<_> = tasks.iter().map(|task| task.snapshot.clone()).collect();
        match manager.save_chunks_batch_in(dimension, &snapshots) {
            Ok(()) => {
                for task in &tasks {
                    task.acknowledge_persisted();
                }
            }
            Err(error) => {
                first_error.get_or_insert_with(|| error.clone());
                for task in &tasks {
                    task.acknowledge_failed();
                }
                failed_chunks.extend(tasks);
            }
        }
    }

    (
        failed_chunks,
        failed_level_player,
        first_error,
        completed_items,
        completed_bytes,
    )
}
