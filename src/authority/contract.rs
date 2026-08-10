//! Contracts shared by every authority topology.
//!
//! The contract deliberately contains no transport or presentation types.  A
//! single-player in-process runtime, a listen server and the dedicated binary
//! all use these same revision/session rules and request vectors.

use crate::inventory::GameMode;
use crate::network::protocol::{
    GameplayOperation, GameplayRequest, GameplayResponse, ItemWire, PlayerId, RejectReason,
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

/// Compact, transport-independent inventory entry owned by an authenticated
/// authority session.  ItemWire keeps durability, enchantments, potion and
/// custom-name metadata; the two Adventure permission masks are carried beside
/// it so authority projection cannot silently change stack identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionInventorySlot {
    pub item: ItemWire,
    pub can_break: u128,
    pub can_place_on: u128,
}

impl SessionInventorySlot {
    pub const fn from_wire(item: ItemWire, can_break: u128, can_place_on: u128) -> Self {
        Self {
            item,
            can_break,
            can_place_on,
        }
    }

    pub fn same_identity(self, other: Self) -> bool {
        self.item.item == other.item.item
            && self.item.durability == other.item.durability
            && self.item.enchantments == other.item.enchantments
            && self.item.potion == other.item.potion
            && self.item.custom_name == other.item.custom_name
            && self.can_break == other.can_break
            && self.can_place_on == other.can_place_on
    }
}

pub const SESSION_INVENTORY_SLOTS: usize = 41;

/// Gameplay state that must not be duplicated in a renderer root.  Keep this
/// compact and integer-based so snapshots remain deterministic and cheap to
/// compare across Singleplayer, listen and dedicated topologies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionGameplayState {
    pub health_milli: u32,
    pub max_health_milli: u32,
    pub hunger_milli: u32,
    pub saturation_milli: u32,
    pub is_dead: bool,
    pub inventory: [Option<SessionInventorySlot>; SESSION_INVENTORY_SLOTS],
    pub mounted_entity: Option<u64>,
    pub revision: u64,
}

impl Default for SessionGameplayState {
    fn default() -> Self {
        Self {
            health_milli: 20_000,
            max_health_milli: 20_000,
            hunger_milli: 20_000,
            saturation_milli: 5_000,
            is_dead: false,
            inventory: [None; SESSION_INVENTORY_SLOTS],
            mounted_entity: None,
            revision: 0,
        }
    }
}

impl SessionGameplayState {
    pub fn count_item(&self, item: u32) -> u32 {
        self.inventory
            .iter()
            .flatten()
            .filter(|slot| slot.item.item == item)
            .map(|slot| u32::from(slot.item.count))
            .sum()
    }

    /// Remove exactly `count` items while compacting across the fixed slots.
    /// Returning false leaves the state untouched, which makes trade retries
    /// atomic when a second cost item is unavailable.
    pub fn remove_item(&mut self, item: u32, count: u32) -> bool {
        if count == 0 || self.count_item(item) < count {
            return false;
        }
        let mut remaining = count;
        for slot in &mut self.inventory {
            let Some(entry) = slot.as_mut() else {
                continue;
            };
            if entry.item.item != item {
                continue;
            }
            let removed = u32::from(entry.item.count).min(remaining);
            entry.item.count = (u32::from(entry.item.count) - removed) as u16;
            remaining -= removed;
            if entry.item.count == 0 {
                *slot = None;
            }
            if remaining == 0 {
                break;
            }
        }
        true
    }

    pub fn add_item(&mut self, item: u32, count: u32) -> bool {
        if count == 0 {
            return true;
        }
        if count > u32::from(u16::MAX) {
            return false;
        }
        self.add_slot(SessionInventorySlot::from_wire(
            ItemWire {
                item,
                count: count as u16,
                durability: 0,
                enchantments: [0; 6],
                potion: None,
                custom_name: [0; 24],
            },
            0,
            0,
        ))
    }

    pub fn add_slot(&mut self, slot: SessionInventorySlot) -> bool {
        let count = u32::from(slot.item.count);
        if count == 0 {
            return true;
        }
        let Some(item) = crate::inventory::Item::from_u32(slot.item.item) else {
            return false;
        };
        let max_stack = item.properties().max_stack.min(u32::from(u16::MAX));
        let backup = *self;
        let mut remaining = count;
        while remaining > 0 {
            let mut progressed = false;
            if let Some(entry) = self.inventory.iter_mut().flatten().find(|entry| {
                (**entry).same_identity(slot)
                    && u32::from(entry.item.count) < max_stack
                    && entry.item.count > 0
            }) {
                let room = max_stack - u32::from(entry.item.count);
                let moved = room.min(remaining);
                entry.item.count = entry.item.count.saturating_add(moved as u16);
                remaining -= moved;
                progressed = true;
            } else if let Some(target) = self.inventory.iter_mut().find(|slot| slot.is_none()) {
                let moved = max_stack.min(remaining) as u16;
                let mut placed = slot;
                placed.item.count = moved;
                *target = Some(placed);
                remaining -= u32::from(moved);
                progressed = true;
            }
            if !progressed {
                *self = backup;
                return false;
            }
        }
        true
    }
}

/// Projection of a session's gameplay state carried by the fixed-tick
/// authority snapshot.  Renderer roots apply it; they never settle gameplay
/// locally after an authority boundary exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionGameplayUpdate {
    pub player_id: PlayerId,
    pub dimension: u8,
    pub state: SessionGameplayState,
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
    pub gameplay: SessionGameplayState,
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
            gameplay: SessionGameplayState::default(),
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

/// A concrete mutation emitted by the authority. Consumers can persist or
/// replicate this value without inspecting renderer chunks. `revision` is
/// scoped by `dimension`; the pair is the stable identity across worlds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorldMutation {
    pub dimension: u8,
    pub position: (i32, i32, i32),
    pub block: u32,
    pub state: u8,
    pub revision: u64,
}

/// Deterministic result of exactly one fixed tick across all loaded worlds.
/// `revision` is the maximum per-dimension revision for compatibility only;
/// clients must gate deltas using each mutation's `(dimension, revision)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthoritySnapshot {
    pub tick: u64,
    pub revision: u64,
    pub checksum: u64,
    pub mutations: Vec<WorldMutation>,
    pub session_updates: Vec<SessionGameplayUpdate>,
}

impl AuthoritySnapshot {
    pub fn empty() -> Self {
        Self {
            tick: 0,
            revision: 0,
            checksum: 0,
            mutations: Vec::new(),
            session_updates: Vec::new(),
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
                action: 0,
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
            operation: GameplayOperation::ItemUse { item: 1, count: 1 },
        },
        GameplayRequest {
            request_id: 0x1005,
            client_sequence: 5,
            session_id: 7,
            dimension: 0,
            client_revision: 4,
            operation: GameplayOperation::Combat {
                target: 42,
                action: 0,
            },
        },
        GameplayRequest {
            request_id: 0x1006,
            client_sequence: 6,
            session_id: 7,
            dimension: 0,
            client_revision: 5,
            operation: GameplayOperation::Trade {
                villager_id: 42,
                offer_index: 0,
            },
        },
        GameplayRequest {
            request_id: 0x1007,
            client_sequence: 7,
            session_id: 7,
            dimension: 0,
            client_revision: 6,
            operation: GameplayOperation::Mount { entity_id: 42 },
        },
        GameplayRequest {
            request_id: 0x1008,
            client_sequence: 8,
            session_id: 7,
            dimension: 0,
            client_revision: 7,
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
        assert_eq!(vectors.len(), 8);
        assert_eq!(vectors[0].request_id, 0x1001);
        assert_eq!(vectors[7].client_sequence, 8);
    }

    #[test]
    fn gameplay_slots_preserve_metadata_and_stack_limits() {
        let mut gameplay = SessionGameplayState::default();
        let mut first = ItemWire::empty();
        first.item = crate::inventory::Item::Stone as u32;
        first.count = 63;
        first.durability = 4;
        first.enchantments[0] = 1;
        let rich = SessionInventorySlot::from_wire(first, 0x55, 0xaa);
        assert!(gameplay.add_slot(rich));

        let mut second = first;
        second.count = 2;
        assert!(gameplay.add_slot(SessionInventorySlot::from_wire(second, 0x55, 0xaa,)));
        assert_eq!(gameplay.inventory[0].unwrap().item.count, 64);
        assert_eq!(gameplay.inventory[1].unwrap().item.count, 1);
        assert_eq!(gameplay.inventory[0].unwrap().can_break, 0x55);
        assert_eq!(gameplay.inventory[0].unwrap().can_place_on, 0xaa);

        let mut different = first;
        different.count = 1;
        different.durability = 5;
        assert!(gameplay.add_slot(SessionInventorySlot::from_wire(different, 0x55, 0xaa,)));
        assert_eq!(gameplay.inventory[2].unwrap().item.durability, 5);
    }
}
