use super::*;

/// Socket ownership for an embedded authority runtime. `Disabled` creates no
/// host-command channel or network thread; local inputs still use the same
/// bounded FIFO and fixed-tick budget as a listen server's remote inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportMode {
    Disabled,
    Listen,
}

/// Persistent identity used to bootstrap an in-process presentation client.
/// Callers should reserve an ID that cannot collide with their listen
/// transport's remotely allocated IDs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalSessionProfile {
    pub id: u64,
    pub username: String,
    pub storage: LocalSessionStorage,
}

/// Player persistence policy is explicit because an existing singleplayer
/// world stores its player in `player.dat`, while authenticated remote players
/// are isolated under `players/<name>.dat`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalSessionStorage {
    WorldPlayer,
    Named,
}

impl LocalSessionProfile {
    /// Existing singleplayer/listen-host worlds default to the legacy world
    /// player payload and `dimension.dat`.
    pub fn new(id: u64, username: impl Into<String>) -> Self {
        Self {
            id,
            username: username.into(),
            storage: LocalSessionStorage::WorldPlayer,
        }
    }

    pub fn named(id: u64, username: impl Into<String>) -> Self {
        Self {
            id,
            username: username.into(),
            storage: LocalSessionStorage::Named,
        }
    }
}

/// Composition choices for a headless runtime embedded by singleplayer or a
/// listen host. This does not imply that the desktop `State` has been cut over
/// to consume the runtime output yet.
#[derive(Debug, Clone)]
pub struct EmbeddedRuntimeOptions {
    pub transport: TransportMode,
    pub local_session: Option<LocalSessionProfile>,
}

impl EmbeddedRuntimeOptions {
    pub fn singleplayer(local_session: LocalSessionProfile) -> Self {
        Self {
            transport: TransportMode::Disabled,
            local_session: Some(local_session),
        }
    }

    pub fn listen(local_session: LocalSessionProfile) -> Self {
        Self {
            transport: TransportMode::Listen,
            local_session: Some(local_session),
        }
    }
}

/// Cloneable producer for the runtime's single bounded input FIFO. In listen
/// mode this sender and the socket transport publish into the same receiver,
/// so neither source can synchronously overtake events already in the queue.
#[derive(Clone)]
pub struct RuntimeInput {
    pub(super) sender: SyncSender<ServerToHost>,
    pub(super) metrics: NetworkMetrics,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeInputError {
    Full,
    Disconnected,
}

impl fmt::Display for RuntimeInputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Full => f.write_str("runtime input queue is full"),
            Self::Disconnected => f.write_str("runtime input queue is disconnected"),
        }
    }
}

impl std::error::Error for RuntimeInputError {}

impl RuntimeInput {
    pub fn try_send(&self, event: ServerToHost) -> Result<(), RuntimeInputError> {
        self.metrics.enqueue();
        match self.sender.try_send(event) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => {
                self.metrics.dequeue();
                self.metrics.record_queue_full();
                Err(RuntimeInputError::Full)
            }
            Err(TrySendError::Disconnected(_)) => {
                self.metrics.dequeue();
                Err(RuntimeInputError::Disconnected)
            }
        }
    }

    pub fn submit_request(
        &self,
        session_id: u64,
        mut request: GameplayRequest,
    ) -> Result<(), RuntimeInputError> {
        request.session_id = session_id;
        self.try_send(ServerToHost::GameplayRequest {
            id: session_id,
            request,
        })
    }
}

/// Target-aware events intended for an in-process presentation consumer.
/// World and per-session gameplay changes remain in `snapshot`, including its
/// bounded `session_updates`; this lane only diverts responses that would
/// otherwise be addressed to a nonexistent socket session.
///
/// Wire gameplay still uses [`ProjectionEvent`] (a [`Packet`] plus dest) for TCP.
/// Embedded columns may instead carry [`PresentationEvent::ChunkColumn`].
pub use crate::network::server::{ProjectionDest, ProjectionEvent};

use crate::network::protocol::Packet;
use crate::world::chunk_xz;

/// Embedded presentation drain: wire-shaped packets or in-process `Arc<Chunk>`.
#[derive(Clone)]
pub enum PresentationEvent {
    Packet(ProjectionEvent),
    /// Zero-copy column for the local embedded session. Never encoded for TCP.
    ChunkColumn {
        to: u64,
        dimension: u8,
        cx: i32,
        cz: i32,
        revision: u64,
        chunk: Arc<crate::world::Chunk>,
    },
}

impl std::fmt::Debug for PresentationEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Packet(event) => f.debug_tuple("Packet").field(event).finish(),
            Self::ChunkColumn {
                to,
                dimension,
                cx,
                cz,
                revision,
                chunk,
            } => f
                .debug_struct("ChunkColumn")
                .field("to", to)
                .field("dimension", dimension)
                .field("cx", cx)
                .field("cz", cz)
                .field("revision", revision)
                .field("chunk_arc", &Arc::as_ptr(chunk))
                .finish(),
        }
    }
}

impl PresentationEvent {
    pub fn session_id(&self) -> Option<u64> {
        match self {
            Self::Packet(event) => event.session_id(),
            Self::ChunkColumn { to, .. } => Some(*to),
        }
    }

    pub fn as_packet_event(&self) -> Option<&ProjectionEvent> {
        match self {
            Self::Packet(event) => Some(event),
            Self::ChunkColumn { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReplaceablePresentationKey {
    Chunk {
        target: u64,
        dimension: u8,
        cx: i32,
        cz: i32,
    },
    EntityState {
        target: u64,
        dimension: u8,
        entity_id: u64,
    },
    PlayerPosition {
        target: u64,
        id: u64,
    },
    TimeSync {
        target: u64,
    },
}

pub(super) fn presentation_replaceable_key(event: &PresentationEvent) -> Option<ReplaceablePresentationKey> {
    match event {
        PresentationEvent::ChunkColumn {
            to,
            dimension,
            cx,
            cz,
            ..
        } => Some(ReplaceablePresentationKey::Chunk {
            target: *to,
            dimension: *dimension,
            cx: *cx,
            cz: *cz,
        }),
        PresentationEvent::Packet(event) => {
            let target = event.session_id()?;
            match &event.packet {
                Packet::ChunkData {
                    dimension,
                    cx,
                    cz,
                    ..
                } => Some(ReplaceablePresentationKey::Chunk {
                    target,
                    dimension: *dimension,
                    cx: *cx,
                    cz: *cz,
                }),
                Packet::EntityState {
                    dimension, state, ..
                } => Some(ReplaceablePresentationKey::EntityState {
                    target,
                    dimension: *dimension,
                    entity_id: state.entity_id,
                }),
                Packet::PlayerPosition { id, .. } => Some(ReplaceablePresentationKey::PlayerPosition {
                    target,
                    id: *id,
                }),
                Packet::TimeSync { .. } => Some(ReplaceablePresentationKey::TimeSync { target }),
                _ => None,
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct RuntimeTickOutput {
    pub snapshot: AuthoritySnapshot,
    pub presentation_events: Vec<PresentationEvent>,
}

#[derive(Debug)]
pub enum ServerConfigError {
    Io(io::Error),
    Invalid {
        key: String,
        value: String,
        reason: String,
    },
}

impl fmt::Display for ServerConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "server.properties: {error}"),
            Self::Invalid { key, value, reason } => {
                write!(f, "invalid server.properties {key}={value:?}: {reason}")
            }
        }
    }
}

impl std::error::Error for ServerConfigError {}

impl From<io::Error> for ServerConfigError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}
