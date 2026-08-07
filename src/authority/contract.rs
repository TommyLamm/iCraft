//! Contracts shared by every authority topology.
//!
//! The contract deliberately contains no transport or presentation types.  A
//! single-player in-process runtime, a listen server and the dedicated binary
//! all use these same revision/session rules and request vectors.

use crate::inventory::GameMode;
use crate::network::protocol::{
    GameplayOperation, GameplayRequest, GameplayResponse, PlayerId, RejectReason,
};
use std::collections::VecDeque;

/// Bump this when the authoritative request/session semantics change.
pub const AUTHORITY_CONTRACT_VERSION: u16 = 1;
pub const FIXED_TICK_HZ: u32 = 20;
pub const RESPONSE_CACHE_CAPACITY: usize = 128;

/// The composition root is allowed to choose a transport, never a second
/// authority implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorityTopology {
    Singleplayer,
    ListenServer,
    Dedicated,
}

impl AuthorityTopology {
    pub const fn is_headless(self) -> bool {
        matches!(self, Self::Dedicated)
    }
}

/// Monotonic server revision shared by mutations and ACKs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RevisionClock {
    current: u64,
}

impl Default for RevisionClock {
    fn default() -> Self {
        Self::new()
    }
}

impl RevisionClock {
    pub const fn new() -> Self {
        Self { current: 0 }
    }

    pub const fn current(self) -> u64 {
        self.current
    }

    /// Allocate a non-zero revision.  Wrapping is explicit and skips zero so
    /// `0` remains the uninitialized/client-baseline revision forever.
    pub fn allocate(&mut self) -> u64 {
        self.current = self.current.wrapping_add(1);
        if self.current == 0 {
            self.current = 1;
        }
        self.current
    }

    pub fn observe(&mut self, revision: u64) {
        if revision > self.current {
            self.current = revision;
        }
    }
}

/// Transport-independent authenticated session state used by AuthorityCore.
#[derive(Debug, Clone)]
pub struct SessionContract {
    pub id: PlayerId,
    pub username: String,
    pub dimension: u8,
    pub position: [f32; 3],
    pub game_mode: GameMode,
    pub operator: bool,
    pub cheats_enabled: bool,
    pub last_client_sequence: u64,
    pub last_revision: u64,
    response_cache: VecDeque<GameplayResponse>,
}

impl SessionContract {
    pub fn new(
        id: PlayerId,
        username: impl Into<String>,
        dimension: u8,
        position: [f32; 3],
        operator: bool,
        cheats_enabled: bool,
    ) -> Self {
        Self {
            id,
            username: username.into(),
            dimension,
            position,
            game_mode: GameMode::Survival,
            operator,
            cheats_enabled,
            last_client_sequence: 0,
            last_revision: 0,
            response_cache: VecDeque::with_capacity(RESPONSE_CACHE_CAPACITY),
        }
    }

    pub fn cached_response(&self, request_id: u128) -> Option<GameplayResponse> {
        self.response_cache
            .iter()
            .find(|response| response.request_id == request_id)
            .cloned()
    }

    pub fn cache_response(&mut self, response: GameplayResponse) {
        if self.response_cache.len() >= RESPONSE_CACHE_CAPACITY {
            self.response_cache.pop_front();
        }
        self.response_cache.push_back(response);
    }

    pub fn cache_len(&self) -> usize {
        self.response_cache.len()
    }

    pub fn validate_sequence(&self, request: &GameplayRequest) -> Result<(), RejectReason> {
        if request.client_sequence <= self.last_client_sequence {
            Err(RejectReason::OutOfOrder)
        } else {
            Ok(())
        }
    }
}

/// A concrete mutation emitted by the authority.  Consumers can persist or
/// replicate this value without inspecting renderer chunks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorldMutation {
    pub dimension: u8,
    pub position: (i32, i32, i32),
    pub block: u32,
    pub state: u8,
    pub revision: u64,
}

/// Deterministic result of exactly one fixed tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthoritySnapshot {
    pub tick: u64,
    pub revision: u64,
    pub checksum: u64,
    pub mutations: Vec<WorldMutation>,
}

impl AuthoritySnapshot {
    pub fn empty() -> Self {
        Self {
            tick: 0,
            revision: 0,
            checksum: 0,
            mutations: Vec::new(),
        }
    }
}

/// Shared headless vectors.  Keep IDs/sequences stable: these are the
/// contract fixture used by local/listen/dedicated harnesses.
pub fn common_gameplay_vectors() -> Vec<GameplayRequest> {
    vec![
        GameplayRequest {
            request_id: 0x1001,
            client_sequence: 1,
            session_id: 7,
            dimension: 0,
            client_revision: 0,
            operation: GameplayOperation::BlockUse {
                x: 8,
                y: 80,
                z: 8,
                block: 3,
            },
        },
        GameplayRequest {
            request_id: 0x1002,
            client_sequence: 2,
            session_id: 7,
            dimension: 0,
            client_revision: 1,
            operation: GameplayOperation::Container {
                action: 1,
                x: 8,
                y: 80,
                z: 8,
                slot: 0,
            },
        },
        GameplayRequest {
            request_id: 0x1003,
            client_sequence: 3,
            session_id: 7,
            dimension: 0,
            client_revision: 2,
            operation: GameplayOperation::Sleep { x: 8, y: 80, z: 8 },
        },
        GameplayRequest {
            request_id: 0x1004,
            client_sequence: 4,
            session_id: 7,
            dimension: 0,
            client_revision: 3,
            operation: GameplayOperation::Command {
                command: "/gamerule doDaylightCycle false".into(),
            },
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revision_clock_is_nonzero_and_monotonic() {
        let mut clock = RevisionClock::new();
        assert_eq!(clock.current(), 0);
        assert_eq!(clock.allocate(), 1);
        clock.observe(10);
        assert_eq!(clock.allocate(), 11);
        assert_eq!(clock.current(), 11);
    }

    #[test]
    fn response_cache_is_bounded_and_vectors_are_stable() {
        let mut session = SessionContract::new(1, "alex", 0, [0.0; 3], false, false);
        for index in 0..(RESPONSE_CACHE_CAPACITY + 1) {
            session.cache_response(GameplayResponse {
                request_id: index as u128,
                server_sequence: index as u64 + 1,
                outcome: crate::network::protocol::GameplayOutcome::Accepted {
                    revision: index as u64 + 1,
                },
            });
        }
        assert_eq!(session.cache_len(), RESPONSE_CACHE_CAPACITY);
        assert!(session.cached_response(0).is_none());
        assert!(session
            .cached_response(RESPONSE_CACHE_CAPACITY as u128)
            .is_some());
        let vectors = common_gameplay_vectors();
        assert_eq!(vectors.len(), 4);
        assert_eq!(vectors[0].request_id, 0x1001);
        assert_eq!(vectors[3].client_sequence, 4);
    }
}
