//! Off-tick persistence: tick enqueues payloads; this thread owns zlib/region
//! bincode / atomic writes and returns acks before dirty bits are cleared.

use crate::dimension::Dimension;
use crate::save::{atomic_write, atomic_write_group, ChunkSaveData, SaveManager};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TryRecvError, TrySendError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

pub(super) const SAVE_QUEUE_CAPACITY: usize = 64;

#[derive(Debug)]
pub(super) enum SavePayload {
    Chunks {
        job_id: u64,
        dimension: Dimension,
        /// `(cx, cz, dirty_revision, payload)` — payload is already flattened/
        /// compressed by the tick thread (`ChunkSaveData`).
        entries: Vec<(i32, i32, u64, ChunkSaveData)>,
    },
    Entities {
        job_id: u64,
        dimension: Dimension,
        epoch: u64,
        bytes: Vec<u8>,
        path: PathBuf,
    },
    SidecarGroup {
        job_id: u64,
        entries: Vec<(PathBuf, Vec<u8>)>,
    },
    PlayerFile {
        job_id: u64,
        player_id: u64,
        path: PathBuf,
        bytes: Vec<u8>,
    },
    /// Shutdown / `save_all` barrier: ack only after prior jobs finish.
    Barrier {
        job_id: u64,
    },
}

#[derive(Debug)]
pub(super) enum SaveAck {
    Chunks {
        job_id: u64,
        dimension: Dimension,
        revisions: Vec<(i32, i32, u64)>,
        ok: bool,
    },
    Entities {
        job_id: u64,
        dimension: Dimension,
        epoch: u64,
        ok: bool,
    },
    SidecarGroup {
        job_id: u64,
        ok: bool,
    },
    PlayerFile {
        job_id: u64,
        player_id: u64,
        ok: bool,
    },
    Barrier {
        job_id: u64,
    },
}

pub(super) struct SaveWorker {
    job_tx: Option<SyncSender<SavePayload>>,
    ack_rx: Receiver<SaveAck>,
    thread: Option<JoinHandle<()>>,
    next_job_id: u64,
}

impl SaveWorker {
    pub fn spawn(mut save_manager: SaveManager) -> Self {
        let (job_tx, job_rx) = mpsc::sync_channel(SAVE_QUEUE_CAPACITY);
        let (ack_tx, ack_rx) = mpsc::sync_channel(SAVE_QUEUE_CAPACITY * 2);
        let thread = thread::Builder::new()
            .name("icraft-save".into())
            .spawn(move || save_thread_main(job_rx, ack_tx, &mut save_manager))
            .expect("failed to spawn save worker");
        Self {
            job_tx: Some(job_tx),
            ack_rx,
            thread: Some(thread),
            next_job_id: 1,
        }
    }

    pub fn next_job_id(&mut self) -> u64 {
        let id = self.next_job_id;
        self.next_job_id = self.next_job_id.wrapping_add(1).max(1);
        id
    }

    pub fn try_enqueue(&self, payload: SavePayload) -> Result<(), TrySendError<SavePayload>> {
        match self.job_tx.as_ref() {
            Some(tx) => tx.try_send(payload),
            None => Err(TrySendError::Disconnected(payload)),
        }
    }

    pub fn enqueue_blocking(&self, payload: SavePayload) -> Result<(), mpsc::SendError<SavePayload>> {
        match self.job_tx.as_ref() {
            Some(tx) => tx.send(payload),
            None => Err(mpsc::SendError(payload)),
        }
    }

    pub fn poll_acks(&self) -> Vec<SaveAck> {
        let mut out = Vec::new();
        loop {
            match self.ack_rx.try_recv() {
                Ok(ack) => out.push(ack),
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
            }
        }
        out
    }

    /// Block until a barrier ack with `job_id` arrives (after draining others).
    pub fn wait_barrier(&self, job_id: u64) -> Vec<SaveAck> {
        let mut out = Vec::new();
        loop {
            match self.ack_rx.recv_timeout(Duration::from_secs(120)) {
                Ok(ack) => {
                    let done = matches!(&ack, SaveAck::Barrier { job_id: id } if *id == job_id);
                    out.push(ack);
                    if done {
                        return out;
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    return out;
                }
                Err(RecvTimeoutError::Disconnected) => return out,
            }
        }
    }

    pub fn shutdown(mut self) {
        let job_id = self.next_job_id();
        let _ = self.enqueue_blocking(SavePayload::Barrier { job_id });
        let _ = self.wait_barrier(job_id);
        drop(self.job_tx.take());
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for SaveWorker {
    fn drop(&mut self) {
        drop(self.job_tx.take());
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

fn save_thread_main(
    job_rx: Receiver<SavePayload>,
    ack_tx: SyncSender<SaveAck>,
    save_manager: &mut SaveManager,
) {
    while let Ok(job) = job_rx.recv() {
        match job {
            SavePayload::Chunks {
                job_id,
                dimension,
                entries,
            } => {
                let revisions: Vec<(i32, i32, u64)> = entries
                    .iter()
                    .map(|(cx, cz, revision, _)| (*cx, *cz, *revision))
                    .collect();
                let chunks: Vec<(i32, i32, ChunkSaveData)> = entries
                    .into_iter()
                    .map(|(cx, cz, _, data)| (cx, cz, data))
                    .collect();
                let ok = save_manager.save_chunks_in(dimension, chunks).is_ok();
                let _ = ack_tx.send(SaveAck::Chunks {
                    job_id,
                    dimension,
                    revisions,
                    ok,
                });
            }
            SavePayload::Entities {
                job_id,
                dimension,
                epoch,
                bytes,
                path,
            } => {
                let ok = atomic_write(&path, &bytes).is_ok();
                let _ = ack_tx.send(SaveAck::Entities {
                    job_id,
                    dimension,
                    epoch,
                    ok,
                });
            }
            SavePayload::SidecarGroup { job_id, entries } => {
                let refs: Vec<_> = entries
                    .iter()
                    .map(|(path, bytes)| (path.as_path(), bytes.as_slice()))
                    .collect();
                let ok = atomic_write_group(&refs).is_ok();
                let _ = ack_tx.send(SaveAck::SidecarGroup { job_id, ok });
            }
            SavePayload::PlayerFile {
                job_id,
                player_id,
                path,
                bytes,
            } => {
                let ok = atomic_write(&path, &bytes).is_ok();
                let _ = ack_tx.send(SaveAck::PlayerFile {
                    job_id,
                    player_id,
                    ok,
                });
            }
            SavePayload::Barrier { job_id } => {
                let _ = ack_tx.send(SaveAck::Barrier { job_id });
            }
        }
    }
}
