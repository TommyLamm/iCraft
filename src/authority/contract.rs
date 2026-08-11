//! Contracts shared by every authority topology.
//!
//! The contract deliberately contains no transport or presentation types.  A
//! single-player in-process runtime, a listen server and the dedicated binary
//! all use these same revision/session rules and request vectors.

use crate::inventory::GameMode;
use crate::network::protocol::{
    GameplayOperation, GameplayRequest, GameplayResponse, ItemWire, MiningProgressWire, PlayerId,
    RejectReason, SessionBrewWire, SessionFishingHookWire, SessionGameplayWire, SessionSlotWire,
    SlotRefWire,
};
use std::collections::VecDeque;

/// Bump this when the authoritative request/session semantics change.
pub const AUTHORITY_CONTRACT_VERSION: u16 = 2;
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

impl From<SessionSlotWire> for SessionInventorySlot {
    fn from(slot: SessionSlotWire) -> Self {
        Self::from_wire(slot.item, slot.can_break, slot.can_place_on)
    }
}

impl From<SessionInventorySlot> for SessionSlotWire {
    fn from(slot: SessionInventorySlot) -> Self {
        Self::new(slot.item, slot.can_break, slot.can_place_on)
    }
}

pub const SESSION_INVENTORY_SLOTS: usize = 41;

/// Fixed-width fishing projection kept inside the authority session. Position
/// and velocity are milliblocks so the contract remains `Eq` and deterministic
/// across in-process and serialized topologies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionFishingHookState {
    pub entity_id: u64,
    pub position_milli: [i32; 3],
    pub velocity_milli: [i32; 3],
    pub stage: u8,
    pub wait_ticks_remaining: u32,
    pub bite_ticks_remaining: u32,
}

/// A bounded in-flight brew transaction. It stores only exact session slot
/// references and fixed-size bottle inputs, never an attacker-controlled Vec.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionBrewState {
    pub station: [i32; 3],
    pub ingredient: SlotRefWire,
    pub bottles: [Option<SlotRefWire>; 3],
    pub remaining_ticks: u16,
}

/// Authority-owned fixed-tick mining session. Progress is reset on logout or
/// reconnect; only the final block/drop/XP mutation is durable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MiningProgressState {
    pub dimension: u8,
    pub target: [i32; 3],
    pub progress_milli: u16,
    pub hand: u8,
    pub slot_index: u8,
    pub held: Option<SessionSlotWire>,
    pub block: u32,
    pub state: u8,
    pub look_milli: [i16; 3],
}

impl From<MiningProgressState> for MiningProgressWire {
    fn from(state: MiningProgressState) -> Self {
        Self {
            dimension: state.dimension,
            target: state.target,
            progress_milli: state.progress_milli,
            hand: state.hand,
            slot_index: state.slot_index,
            held: state.held,
            block: state.block,
            state: state.state,
            look_milli: state.look_milli,
        }
    }
}

impl From<MiningProgressWire> for MiningProgressState {
    fn from(state: MiningProgressWire) -> Self {
        Self {
            dimension: state.dimension,
            target: state.target,
            progress_milli: state.progress_milli,
            hand: state.hand,
            slot_index: state.slot_index,
            held: state.held,
            block: state.block,
            state: state.state,
            look_milli: state.look_milli,
        }
    }
}

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
    pub death_source: Option<u8>,
    pub invulnerability_ticks: u16,
    pub velocity_milli: [i32; 3],
    pub experience: u32,
    pub experience_level: u32,
    pub selected_hotbar_slot: u8,
    pub inventory: [Option<SessionInventorySlot>; SESSION_INVENTORY_SLOTS],
    pub mounted_entity: Option<u64>,
    pub attack_cooldown_ticks: u16,
    pub shield_active: bool,
    pub shield_cooldown_ticks: u16,
    pub enchant_seed: u64,
    pub fishing_hook: Option<SessionFishingHookState>,
    pub brew: Option<SessionBrewState>,
    pub mining: Option<MiningProgressState>,
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
            death_source: None,
            invulnerability_ticks: 0,
            velocity_milli: [0; 3],
            experience: 0,
            experience_level: 0,
            selected_hotbar_slot: 0,
            inventory: [None; SESSION_INVENTORY_SLOTS],
            mounted_entity: None,
            attack_cooldown_ticks: 5,
            shield_active: false,
            shield_cooldown_ticks: 0,
            enchant_seed: 0,
            fishing_hook: None,
            brew: None,
            mining: None,
            revision: 0,
        }
    }
}

impl From<SessionFishingHookState> for SessionFishingHookWire {
    fn from(state: SessionFishingHookState) -> Self {
        Self {
            entity_id: state.entity_id,
            position_milli: state.position_milli,
            velocity_milli: state.velocity_milli,
            stage: state.stage,
            wait_ticks_remaining: state.wait_ticks_remaining,
            bite_ticks_remaining: state.bite_ticks_remaining,
        }
    }
}

impl From<SessionFishingHookWire> for SessionFishingHookState {
    fn from(state: SessionFishingHookWire) -> Self {
        Self {
            entity_id: state.entity_id,
            position_milli: state.position_milli,
            velocity_milli: state.velocity_milli,
            stage: state.stage,
            wait_ticks_remaining: state.wait_ticks_remaining,
            bite_ticks_remaining: state.bite_ticks_remaining,
        }
    }
}

impl From<SessionBrewState> for SessionBrewWire {
    fn from(state: SessionBrewState) -> Self {
        Self {
            station: state.station,
            ingredient: state.ingredient,
            bottles: state.bottles,
            remaining_ticks: state.remaining_ticks,
        }
    }
}

impl From<SessionBrewWire> for SessionBrewState {
    fn from(state: SessionBrewWire) -> Self {
        Self {
            station: state.station,
            ingredient: state.ingredient,
            bottles: state.bottles,
            remaining_ticks: state.remaining_ticks,
        }
    }
}

impl From<SessionGameplayState> for SessionGameplayWire {
    fn from(state: SessionGameplayState) -> Self {
        Self {
            health_milli: state.health_milli,
            max_health_milli: state.max_health_milli,
            hunger_milli: state.hunger_milli,
            saturation_milli: state.saturation_milli,
            is_dead: state.is_dead,
            death_source: state.death_source,
            invulnerability_ticks: state.invulnerability_ticks,
            velocity_milli: state.velocity_milli,
            experience: state.experience,
            experience_level: state.experience_level,
            selected_hotbar_slot: state.selected_hotbar_slot,
            hotbar: std::array::from_fn(|index| state.inventory[index].map(Into::into)),
            main: std::array::from_fn(|index| state.inventory[index + 9].map(Into::into)),
            armor: std::array::from_fn(|index| state.inventory[index + 36].map(Into::into)),
            offhand: state.inventory[40].map(Into::into),
            mounted_entity: state.mounted_entity,
            attack_cooldown_ticks: state.attack_cooldown_ticks,
            shield_active: state.shield_active,
            shield_cooldown_ticks: state.shield_cooldown_ticks,
            enchant_seed: state.enchant_seed,
            fishing_hook: state.fishing_hook.map(Into::into),
            brew: state.brew.map(Into::into),
            mining: state.mining.map(Into::into),
            revision: state.revision,
        }
    }
}

impl From<SessionGameplayWire> for SessionGameplayState {
    fn from(state: SessionGameplayWire) -> Self {
        let mut inventory = [None; SESSION_INVENTORY_SLOTS];
        for (index, slot) in state.hotbar.into_iter().enumerate() {
            inventory[index] = slot.map(Into::into);
        }
        for (index, slot) in state.main.into_iter().enumerate() {
            inventory[index + 9] = slot.map(Into::into);
        }
        for (index, slot) in state.armor.into_iter().enumerate() {
            inventory[index + 36] = slot.map(Into::into);
        }
        inventory[40] = state.offhand.map(Into::into);
        Self {
            health_milli: state.health_milli,
            max_health_milli: state.max_health_milli,
            hunger_milli: state.hunger_milli,
            saturation_milli: state.saturation_milli,
            is_dead: state.is_dead,
            death_source: state.death_source,
            invulnerability_ticks: state.invulnerability_ticks,
            velocity_milli: state.velocity_milli,
            experience: state.experience,
            experience_level: state.experience_level,
            selected_hotbar_slot: state.selected_hotbar_slot,
            inventory,
            mounted_entity: state.mounted_entity,
            attack_cooldown_ticks: state.attack_cooldown_ticks,
            shield_active: state.shield_active,
            shield_cooldown_ticks: state.shield_cooldown_ticks,
            enchant_seed: state.enchant_seed,
            fishing_hook: state.fishing_hook.map(Into::into),
            brew: state.brew.map(Into::into),
            mining: state.mining.map(Into::into),
            revision: state.revision,
        }
    }
}

impl SessionGameplayState {
    /// Execute a small session transaction against a copy and publish it only
    /// on success. Domain implementations can compose several slot/XP debits
    /// without writing bespoke rollback code.
    pub fn transact(&mut self, mutation: impl FnOnce(&mut Self) -> bool) -> bool {
        let mut candidate = *self;
        if !mutation(&mut candidate) {
            return false;
        }
        *self = candidate;
        true
    }

    pub fn slot(&self, index: u8) -> Option<Option<SessionInventorySlot>> {
        self.inventory.get(usize::from(index)).copied()
    }

    pub fn slot_matches(&self, source: SlotRefWire) -> bool {
        let Some(Some(current)) = self.slot(source.index) else {
            return false;
        };
        current == SessionInventorySlot::from(source.expected)
            && source.count > 0
            && source.count <= current.item.count
    }

    /// Debit an exact rich stack reference. Failure leaves all gameplay fields
    /// untouched, so callers can safely compose this through `transact`.
    pub fn consume_slot_exact(&mut self, source: SlotRefWire) -> bool {
        self.consume_slots_exact(&[source])
    }

    /// Atomically debit a bounded set of exact rich slot references. Repeated
    /// indices are aggregated against the original expected stack, which lets
    /// a crafting grid consume several cells from one inventory stack without
    /// weakening stale-request detection.
    pub fn consume_slots_exact(&mut self, sources: &[SlotRefWire]) -> bool {
        self.transact(|candidate| {
            let mut totals = [0u32; SESSION_INVENTORY_SLOTS];
            let mut expected = [None; SESSION_INVENTORY_SLOTS];
            for source in sources {
                if !candidate.slot_matches(*source) {
                    return false;
                }
                let index = usize::from(source.index);
                if expected[index].is_some_and(|slot| slot != source.expected) {
                    return false;
                }
                expected[index] = Some(source.expected);
                let Some(total) = totals[index].checked_add(u32::from(source.count)) else {
                    return false;
                };
                if total > u32::from(source.expected.item.count) {
                    return false;
                }
                totals[index] = total;
            }

            for (index, total) in totals.into_iter().enumerate() {
                if total == 0 {
                    continue;
                }
                let slot = &mut candidate.inventory[index];
                let entry = slot.as_mut().expect("slot matches guarantee an entry");
                entry.item.count -= total as u16;
                if entry.item.count == 0 {
                    *slot = None;
                }
            }
            true
        })
    }

    /// Compare-and-replace a rich inventory slot. Both the expected source and
    /// replacement must be fully bounded by the protocol contract.
    pub fn replace_slot_exact(
        &mut self,
        source: SlotRefWire,
        replacement: Option<SessionSlotWire>,
    ) -> bool {
        self.transact(|candidate| {
            if !candidate.slot_matches(source)
                || replacement
                    .as_ref()
                    .is_some_and(|slot| slot.validate_bounds().is_err())
            {
                return false;
            }
            candidate.inventory[usize::from(source.index)] =
                replacement.map(SessionInventorySlot::from);
            true
        })
    }

    pub fn experience_to_next_level(&self) -> u32 {
        7u32.saturating_add(self.experience_level.saturating_mul(2))
    }

    /// Grant raw XP using the same level curve as PlayerState. Overflow aborts
    /// the cloned transaction instead of partially advancing a level.
    pub fn grant_experience(&mut self, amount: u32) -> bool {
        self.transact(|candidate| {
            let Some(total) = candidate.experience.checked_add(amount) else {
                return false;
            };
            candidate.experience = total;
            while candidate.experience >= candidate.experience_to_next_level() {
                candidate.experience -= candidate.experience_to_next_level();
                let Some(level) = candidate.experience_level.checked_add(1) else {
                    return false;
                };
                candidate.experience_level = level;
            }
            true
        })
    }

    pub fn spend_levels(&mut self, levels: u32) -> bool {
        self.transact(|candidate| {
            let Some(level) = candidate.experience_level.checked_sub(levels) else {
                return false;
            };
            candidate.experience_level = level;
            true
        })
    }

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
                can_break: 0,
                can_place_on: 0,
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
    pub yaw: f32,
    pub pitch: f32,
    pub spawn_point: Option<[i32; 3]>,
    pub spawn_dimension: Option<u8>,
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
            yaw: 0.0,
            pitch: 0.0,
            spawn_point: None,
            spawn_dimension: None,
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
    /// Complete raw fluid byte (including Plan27 waterlogged bit 7).  Block
    /// mutations and fluid-only level/falling changes share one revision lane.
    pub raw_fluid: u8,
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

    #[test]
    fn gameplay_exact_slot_helpers_are_atomic() {
        let mut gameplay = SessionGameplayState::default();
        let mut item = ItemWire::empty();
        item.item = crate::inventory::Item::IronIngot as u32;
        item.count = 8;
        let expected = SessionSlotWire::new(item, 0x55, 0xaa);
        gameplay.inventory[4] = Some(expected.into());
        let source = SlotRefWire {
            index: 4,
            count: 3,
            expected,
        };
        assert!(gameplay.slot_matches(source));
        assert!(gameplay.consume_slot_exact(source));
        assert_eq!(gameplay.inventory[4].unwrap().item.count, 5);

        let after_success = gameplay;
        assert!(!gameplay.consume_slot_exact(source));
        assert_eq!(gameplay, after_success);

        let exact_remaining = SlotRefWire {
            count: 2,
            expected: SessionSlotWire::new(ItemWire { count: 5, ..item }, 0x55, 0xaa),
            ..source
        };
        assert!(gameplay.consume_slots_exact(&[
            exact_remaining,
            SlotRefWire {
                count: 3,
                ..exact_remaining
            },
        ]));
        assert_eq!(gameplay.slot(4), Some(None));
        assert_eq!(gameplay.slot(SESSION_INVENTORY_SLOTS as u8), None);

        let mut replacement_gameplay = SessionGameplayState::default();
        replacement_gameplay.inventory[4] = Some(expected.into());
        assert!(replacement_gameplay.replace_slot_exact(source, None));
        assert_eq!(replacement_gameplay.slot(4), Some(None));
    }

    #[test]
    fn gameplay_xp_helpers_and_extended_defaults_are_stable() {
        let mut gameplay = SessionGameplayState::default();
        assert_eq!(gameplay.experience_to_next_level(), 7);
        assert!(gameplay.grant_experience(16));
        assert_eq!(gameplay.experience, 0);
        assert_eq!(gameplay.experience_level, 2);
        assert!(gameplay.spend_levels(1));
        assert_eq!(gameplay.experience_level, 1);

        let before_failed_spend = gameplay;
        assert!(!gameplay.spend_levels(2));
        assert_eq!(gameplay, before_failed_spend);

        gameplay.experience = u32::MAX;
        let before_overflow = gameplay;
        assert!(!gameplay.grant_experience(1));
        assert_eq!(gameplay, before_overflow);

        let session = SessionContract::new(7, "alex", 0, [1.0, 64.0, 2.0], false, false);
        assert_eq!(session.yaw, 0.0);
        assert_eq!(session.pitch, 0.0);
        assert_eq!(session.spawn_point, None);
        assert_eq!(session.spawn_dimension, None);
        assert_eq!(session.gameplay.selected_hotbar_slot, 0);
        assert_eq!(session.gameplay.death_source, None);
        assert_eq!(session.gameplay.invulnerability_ticks, 0);
        assert_eq!(session.gameplay.attack_cooldown_ticks, 5);
        assert!(!session.gameplay.shield_active);
        assert_eq!(session.gameplay.fishing_hook, None);
        assert_eq!(session.gameplay.brew, None);
    }

    #[test]
    fn session_gameplay_wire_conversion_preserves_all_fixed_slots_and_state() {
        let mut gameplay = SessionGameplayState {
            health_milli: 9_000,
            velocity_milli: [2_000, 500, -1_000],
            experience: 123,
            experience_level: 7,
            selected_hotbar_slot: 8,
            mounted_entity: Some(42),
            shield_active: true,
            revision: 11,
            ..SessionGameplayState::default()
        };
        let mut item = ItemWire::empty();
        item.item = crate::inventory::Item::Diamond as u32;
        item.count = 3;
        for index in 0..SESSION_INVENTORY_SLOTS {
            gameplay.inventory[index] = Some(SessionInventorySlot::from_wire(
                item,
                index as u128,
                (index as u128) << 1,
            ));
        }
        gameplay.fishing_hook = Some(SessionFishingHookState {
            entity_id: 99,
            position_milli: [1, 2, 3],
            velocity_milli: [4, 5, 6],
            stage: 2,
            wait_ticks_remaining: 7,
            bite_ticks_remaining: 8,
        });

        let wire = SessionGameplayWire::from(gameplay);
        wire.validate_bounds().unwrap();
        assert_eq!(SessionGameplayState::from(wire), gameplay);
    }
}
