//! Pure authoritative fishing transaction domain.
//!
//! The caller supplies bounded observations from the authoritative world. This
//! module never reads a clock, owns an RNG, or mutates world/entity storage. It
//! only commits a cloned [`SessionGameplayState`] after every validation and
//! inventory/XP operation succeeds.

use crate::authority::contract::{
    milli_within_abs_limit, SessionFishingHookState, SessionGameplayState, SessionInventorySlot,
    SESSION_INVENTORY_SLOTS,
};
use crate::fishing::{
    authoritative_launch_velocity_milli, deterministic_fishing_roll, FishingHookStage,
    FISHING_BITE_WINDOW_TICKS, FISHING_FIXED_TICK_HZ, FISHING_INITIAL_WAIT_TICKS,
    FISHING_MAX_DISTANCE_MILLI, FISHING_REPEAT_WAIT_TICKS, FISHING_ROD_MAX_DURABILITY,
};
use crate::inventory::{Item, ItemStack};
use crate::network::protocol::{ItemWire, SessionSlotWire};

const OFFHAND_SLOT: u8 = (SESSION_INVENTORY_SLOTS - 1) as u8;
const HOTBAR_SLOTS: u8 = 9;
const MAX_HOOK_VELOCITY_MILLI: i32 = 64_000;
const MAX_WATER_SURFACE_DELTA_MILLI: i32 = 2_000;

/// Trusted observations gathered by the authority before a domain step.
/// `water_surface_y_milli` is canonical: it is present exactly when
/// `open_water` is true.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FishingDomainContext {
    pub world_seed: u64,
    pub hook_entity_id: u64,
    pub player_position_milli: [i32; 3],
    pub open_water: bool,
    pub water_surface_y_milli: Option<i32>,
    /// False for creative-like sessions, where a rod is still required but is
    /// not damaged by reeling.
    pub consume_durability: bool,
}

impl FishingDomainContext {
    pub fn validate(self) -> Result<Self, FishingDomainError> {
        if self.hook_entity_id == 0
            || self
                .player_position_milli
                .into_iter()
                .any(|value| !milli_within_abs_limit(value))
            || self.open_water != self.water_surface_y_milli.is_some()
            || self
                .water_surface_y_milli
                .is_some_and(|value| !milli_within_abs_limit(value))
        {
            return Err(FishingDomainError::InvalidContext);
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FishingDomainError {
    InvalidContext,
    InvalidHand,
    InvalidSelectedSlot,
    MissingRod,
    InvalidRod,
    HookAlreadyActive,
    NoActiveHook,
    StaleHook,
    CorruptHook,
    HookTooFar,
    InventoryFull,
    ExperienceOverflow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FishingCastOutcome {
    pub hook: SessionFishingHookState,
    pub player_id: u64,
    pub hand: u8,
    pub rod_slot: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FishingTickEvent {
    Flying,
    Landed,
    Waiting,
    WaitingForOpenWater,
    BiteStarted,
    Splash,
    BiteExpired,
    DespawnedTooFar,
    DespawnedMissingRod,
    DespawnedReeled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FishingTickOutcome {
    pub hook_entity_id: u64,
    pub event: FishingTickEvent,
    pub hook: Option<SessionFishingHookState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FishingLootCategory {
    Fish,
    Junk,
    Treasure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FishingLoot {
    pub category: FishingLootCategory,
    pub slot: SessionInventorySlot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FishingMissReason {
    TooEarly,
    TooLate,
    LeftOpenWater,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FishingReelResult {
    Caught(FishingLoot),
    Missed(FishingMissReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RodDurabilityOutcome {
    Preserved { slot: u8, remaining: u16 },
    Damaged { slot: u8, remaining: u16 },
    Broken { slot: u8 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FishingReelOutcome {
    pub hook_entity_id: u64,
    pub result: FishingReelResult,
    pub rod: RodDurabilityOutcome,
    pub experience: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FishingCancelOutcome {
    pub hook_entity_id: u64,
    pub rod_slot: u8,
}

/// Position the world layer must inspect for open water before calling
/// [`tick`]. Flying hooks probe the post-physics position, matching the legacy
/// `FishingManager`; floating/nibbling hooks probe their current position.
pub fn water_probe_position(state: &SessionGameplayState) -> Result<[i32; 3], FishingDomainError> {
    let mut hook = state.fishing_hook.ok_or(FishingDomainError::NoActiveHook)?;
    validate_hook(hook)?;
    if FishingHookStage::from_wire(hook.stage) == Some(FishingHookStage::Flying) {
        advance_flying_hook(&mut hook)?;
    }
    Ok(hook.position_milli)
}

/// Cast one hook. Hook IDs are allocated by the authority/world layer and are
/// validated here, keeping this pure domain free of a second entity allocator.
pub fn cast(
    state: &mut SessionGameplayState,
    player_id: u64,
    hand: u8,
    look_milli: [i16; 3],
    context: FishingDomainContext,
) -> Result<FishingCastOutcome, FishingDomainError> {
    let context = context.validate()?;
    if state.fishing_hook.is_some() {
        return Err(FishingDomainError::HookAlreadyActive);
    }
    let rod_slot = held_rod_slot(state, hand)?;
    let velocity_milli = authoritative_launch_velocity_milli(look_milli)
        .ok_or(FishingDomainError::InvalidContext)?;
    let mut position_milli = context.player_position_milli;
    position_milli[1] = position_milli[1]
        .checked_add(1_620)
        .ok_or(FishingDomainError::InvalidContext)?;
    validate_position(position_milli)?;

    let hook = SessionFishingHookState {
        entity_id: context.hook_entity_id,
        position_milli,
        velocity_milli,
        stage: FishingHookStage::Flying.to_wire(),
        wait_ticks_remaining: FISHING_INITIAL_WAIT_TICKS,
        bite_ticks_remaining: 0,
    };
    validate_hook(hook)?;

    transact(state, |candidate| {
        // Revalidate the exact held stack on the clone immediately before the
        // publication point. This keeps the API safe if its implementation is
        // later composed inside a larger candidate transaction.
        if held_rod_slot(candidate, hand)? != rod_slot || candidate.fishing_hook.is_some() {
            return Err(FishingDomainError::HookAlreadyActive);
        }
        candidate.fishing_hook = Some(hook);
        Ok(FishingCastOutcome {
            hook,
            player_id,
            hand,
            rod_slot,
        })
    })
}

/// Advance at most one fixed 20Hz step. There is deliberately no catch-up loop:
/// the authority scheduler calls this once per simulation tick.
pub fn tick(
    state: &mut SessionGameplayState,
    context: FishingDomainContext,
) -> Result<FishingTickOutcome, FishingDomainError> {
    let context = context.validate()?;
    let hook = state.fishing_hook.ok_or(FishingDomainError::NoActiveHook)?;
    validate_context_hook(hook, context)?;

    transact(state, |candidate| {
        let mut hook = candidate
            .fishing_hook
            .ok_or(FishingDomainError::NoActiveHook)?;
        validate_context_hook(hook, context)?;

        if find_held_rod_slot(candidate)?.is_none() {
            candidate.fishing_hook = None;
            return Ok(FishingTickOutcome {
                hook_entity_id: hook.entity_id,
                event: FishingTickEvent::DespawnedMissingRod,
                hook: None,
            });
        }
        if hook_too_far(hook.position_milli, context.player_position_milli) {
            candidate.fishing_hook = None;
            return Ok(FishingTickOutcome {
                hook_entity_id: hook.entity_id,
                event: FishingTickEvent::DespawnedTooFar,
                hook: None,
            });
        }

        let stage =
            FishingHookStage::from_wire(hook.stage).ok_or(FishingDomainError::CorruptHook)?;
        let event = match stage {
            FishingHookStage::Flying => {
                advance_flying_hook(&mut hook)?;
                if context.open_water {
                    let surface_y = context
                        .water_surface_y_milli
                        .ok_or(FishingDomainError::InvalidContext)?;
                    if i64::from(surface_y).abs_diff(i64::from(hook.position_milli[1]))
                        > MAX_WATER_SURFACE_DELTA_MILLI as u64
                    {
                        return Err(FishingDomainError::InvalidContext);
                    }
                    hook.position_milli[1] = surface_y;
                    hook.velocity_milli = [0; 3];
                    hook.stage = FishingHookStage::FloatingInWater.to_wire();
                    FishingTickEvent::Landed
                } else {
                    FishingTickEvent::Flying
                }
            }
            FishingHookStage::FloatingInWater => {
                if !context.open_water {
                    FishingTickEvent::WaitingForOpenWater
                } else if hook.wait_ticks_remaining > 0 {
                    hook.wait_ticks_remaining -= 1;
                    FishingTickEvent::Waiting
                } else {
                    hook.stage = FishingHookStage::Nibbling.to_wire();
                    hook.bite_ticks_remaining = FISHING_BITE_WINDOW_TICKS;
                    FishingTickEvent::BiteStarted
                }
            }
            FishingHookStage::Nibbling => {
                if !context.open_water || hook.bite_ticks_remaining == 0 {
                    hook.stage = FishingHookStage::FloatingInWater.to_wire();
                    hook.wait_ticks_remaining = FISHING_REPEAT_WAIT_TICKS;
                    hook.bite_ticks_remaining = 0;
                    FishingTickEvent::BiteExpired
                } else {
                    hook.bite_ticks_remaining -= 1;
                    FishingTickEvent::Splash
                }
            }
            FishingHookStage::Reeled => {
                candidate.fishing_hook = None;
                return Ok(FishingTickOutcome {
                    hook_entity_id: hook.entity_id,
                    event: FishingTickEvent::DespawnedReeled,
                    hook: None,
                });
            }
        };
        validate_hook(hook)?;
        candidate.fishing_hook = Some(hook);
        Ok(FishingTickOutcome {
            hook_entity_id: hook.entity_id,
            event,
            hook: Some(hook),
        })
    })
}

pub fn reel(
    state: &mut SessionGameplayState,
    player_id: u64,
    hand: u8,
    context: FishingDomainContext,
) -> Result<FishingReelOutcome, FishingDomainError> {
    let context = context.validate()?;
    let hook = state.fishing_hook.ok_or(FishingDomainError::NoActiveHook)?;
    validate_context_hook(hook, context)?;
    if hook_too_far(hook.position_milli, context.player_position_milli) {
        return Err(FishingDomainError::HookTooFar);
    }
    let rod_slot = held_rod_slot(state, hand)?;

    transact(state, |candidate| {
        let hook = candidate
            .fishing_hook
            .ok_or(FishingDomainError::NoActiveHook)?;
        validate_context_hook(hook, context)?;
        if hook_too_far(hook.position_milli, context.player_position_milli) {
            return Err(FishingDomainError::HookTooFar);
        }
        if held_rod_slot(candidate, hand)? != rod_slot {
            return Err(FishingDomainError::InvalidRod);
        }

        let stage =
            FishingHookStage::from_wire(hook.stage).ok_or(FishingDomainError::CorruptHook)?;
        let (result, experience, loot) = if stage == FishingHookStage::Nibbling
            && hook.bite_ticks_remaining > 0
            && context.open_water
        {
            let loot = deterministic_loot(context.world_seed, player_id, hook.entity_id);
            let experience =
                1 + deterministic_fishing_roll(context.world_seed, player_id, hook.entity_id, 2)
                    % 6;
            (FishingReelResult::Caught(loot), experience, Some(loot))
        } else {
            let reason = if stage == FishingHookStage::Nibbling && !context.open_water {
                FishingMissReason::LeftOpenWater
            } else if stage == FishingHookStage::FloatingInWater
                && hook.wait_ticks_remaining > FISHING_INITIAL_WAIT_TICKS
            {
                FishingMissReason::TooLate
            } else {
                FishingMissReason::TooEarly
            };
            (FishingReelResult::Missed(reason), 0, None)
        };

        let durability = apply_rod_damage(
            candidate,
            rod_slot,
            context.consume_durability,
            deterministic_fishing_roll(context.world_seed, player_id, hook.entity_id, 3),
        )?;
        if let Some(loot) = loot {
            if !candidate.add_slot(loot.slot) {
                return Err(FishingDomainError::InventoryFull);
            }
            if !candidate.grant_experience(experience) {
                return Err(FishingDomainError::ExperienceOverflow);
            }
        }
        candidate.fishing_hook = None;
        Ok(FishingReelOutcome {
            hook_entity_id: hook.entity_id,
            result,
            rod: durability,
            experience,
        })
    })
}

pub fn cancel(
    state: &mut SessionGameplayState,
    hand: u8,
    context: FishingDomainContext,
) -> Result<FishingCancelOutcome, FishingDomainError> {
    let context = context.validate()?;
    let hook = state.fishing_hook.ok_or(FishingDomainError::NoActiveHook)?;
    validate_context_hook(hook, context)?;
    let rod_slot = held_rod_slot(state, hand)?;

    transact(state, |candidate| {
        let hook = candidate
            .fishing_hook
            .ok_or(FishingDomainError::NoActiveHook)?;
        validate_context_hook(hook, context)?;
        if held_rod_slot(candidate, hand)? != rod_slot {
            return Err(FishingDomainError::InvalidRod);
        }
        candidate.fishing_hook = None;
        Ok(FishingCancelOutcome {
            hook_entity_id: hook.entity_id,
            rod_slot,
        })
    })
}

fn transact<T>(
    state: &mut SessionGameplayState,
    mutation: impl FnOnce(&mut SessionGameplayState) -> Result<T, FishingDomainError>,
) -> Result<T, FishingDomainError> {
    let mut candidate = *state;
    let outcome = mutation(&mut candidate)?;
    *state = candidate;
    Ok(outcome)
}

fn held_slot_index(state: &SessionGameplayState, hand: u8) -> Result<u8, FishingDomainError> {
    match hand {
        0 if state.selected_hotbar_slot < HOTBAR_SLOTS => Ok(state.selected_hotbar_slot),
        0 => Err(FishingDomainError::InvalidSelectedSlot),
        1 => Ok(OFFHAND_SLOT),
        _ => Err(FishingDomainError::InvalidHand),
    }
}

fn held_rod_slot(state: &SessionGameplayState, hand: u8) -> Result<u8, FishingDomainError> {
    let index = held_slot_index(state, hand)?;
    validate_rod_at(state, index)?;
    Ok(index)
}

fn find_held_rod_slot(state: &SessionGameplayState) -> Result<Option<u8>, FishingDomainError> {
    if state.selected_hotbar_slot >= HOTBAR_SLOTS {
        return Err(FishingDomainError::InvalidSelectedSlot);
    }
    for index in [state.selected_hotbar_slot, OFFHAND_SLOT] {
        if validate_rod_at(state, index).is_ok() {
            return Ok(Some(index));
        }
    }
    Ok(None)
}

fn validate_rod_at(
    state: &SessionGameplayState,
    index: u8,
) -> Result<SessionInventorySlot, FishingDomainError> {
    let Some(Some(slot)) = state.slot(index) else {
        return Err(FishingDomainError::MissingRod);
    };
    if SessionSlotWire::from(slot).validate_bounds().is_err()
        || slot.item.item != Item::FishingRod as u32
        || slot.item.count != 1
        || slot.item.durability == 0
        || slot.item.durability > FISHING_ROD_MAX_DURABILITY
    {
        return Err(FishingDomainError::InvalidRod);
    }
    Ok(slot)
}

fn apply_rod_damage(
    state: &mut SessionGameplayState,
    index: u8,
    consume_durability: bool,
    durability_salt: u32,
) -> Result<RodDurabilityOutcome, FishingDomainError> {
    let slot = validate_rod_at(state, index)?;
    let remaining = slot.item.durability;
    let should_consume = consume_durability
        && slot.item.to_stack().is_some_and(|stack| {
            crate::enchantment::should_consume_durability(&stack.enchantments, durability_salt)
        });
    if !should_consume {
        return Ok(RodDurabilityOutcome::Preserved {
            slot: index,
            remaining,
        });
    }
    if remaining == 1 {
        state.inventory[usize::from(index)] = None;
        return Ok(RodDurabilityOutcome::Broken { slot: index });
    }
    state.inventory[usize::from(index)]
        .as_mut()
        .expect("validated rod slot")
        .item
        .durability -= 1;
    Ok(RodDurabilityOutcome::Damaged {
        slot: index,
        remaining: remaining - 1,
    })
}

fn deterministic_loot(world_seed: u64, player_id: u64, hook_entity_id: u64) -> FishingLoot {
    let category_roll = deterministic_fishing_roll(world_seed, player_id, hook_entity_id, 0) % 100;
    let (category, item) = if category_roll < 85 {
        let item = match deterministic_fishing_roll(world_seed, player_id, hook_entity_id, 1) % 4 {
            0 => Item::RawCod,
            1 => Item::RawSalmon,
            2 => Item::TropicalFish,
            _ => Item::Pufferfish,
        };
        (FishingLootCategory::Fish, item)
    } else if category_roll < 95 {
        (FishingLootCategory::Junk, Item::LilyPad)
    } else {
        (FishingLootCategory::Treasure, Item::Bow)
    };
    FishingLoot {
        category,
        slot: SessionInventorySlot::from_wire(ItemWire::from_stack(&ItemStack::new(item, 1)), 0, 0),
    }
}

fn validate_context_hook(
    hook: SessionFishingHookState,
    context: FishingDomainContext,
) -> Result<(), FishingDomainError> {
    validate_hook(hook)?;
    if hook.entity_id != context.hook_entity_id {
        return Err(FishingDomainError::StaleHook);
    }
    Ok(())
}

fn advance_flying_hook(hook: &mut SessionFishingHookState) -> Result<(), FishingDomainError> {
    hook.velocity_milli[1] = hook.velocity_milli[1]
        .checked_sub(12_000 / FISHING_FIXED_TICK_HZ)
        .ok_or(FishingDomainError::CorruptHook)?;
    for axis in 0..3 {
        hook.position_milli[axis] = hook.position_milli[axis]
            .checked_add(hook.velocity_milli[axis] / FISHING_FIXED_TICK_HZ)
            .ok_or(FishingDomainError::CorruptHook)?;
    }
    validate_position(hook.position_milli)
}

fn validate_hook(hook: SessionFishingHookState) -> Result<(), FishingDomainError> {
    if hook.entity_id == 0
        || FishingHookStage::from_wire(hook.stage).is_none()
        || hook
            .position_milli
            .into_iter()
            .any(|value| !milli_within_abs_limit(value))
        || hook
            .velocity_milli
            .into_iter()
            .any(|value| value.unsigned_abs() > MAX_HOOK_VELOCITY_MILLI as u32)
        || hook.wait_ticks_remaining > FISHING_REPEAT_WAIT_TICKS
        || hook.bite_ticks_remaining > FISHING_BITE_WINDOW_TICKS
    {
        return Err(FishingDomainError::CorruptHook);
    }
    Ok(())
}

fn validate_position(position: [i32; 3]) -> Result<(), FishingDomainError> {
    if position
        .into_iter()
        .any(|value| !milli_within_abs_limit(value))
    {
        Err(FishingDomainError::InvalidContext)
    } else {
        Ok(())
    }
}

fn hook_too_far(hook_position: [i32; 3], player_position: [i32; 3]) -> bool {
    let squared_distance = hook_position
        .into_iter()
        .zip(player_position)
        .map(|(hook, player)| i128::from(hook) - i128::from(player))
        .map(|delta| delta * delta)
        .sum::<i128>();
    squared_distance > i128::from(FISHING_MAX_DISTANCE_MILLI).pow(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(item: Item, count: u32) -> SessionInventorySlot {
        SessionInventorySlot::from_wire(ItemWire::from_stack(&ItemStack::new(item, count)), 0, 0)
    }

    fn rod(durability: u16) -> SessionInventorySlot {
        let mut rod = slot(Item::FishingRod, 1);
        rod.item.durability = durability;
        rod
    }

    fn state_with_rod(durability: u16) -> SessionGameplayState {
        let mut state = SessionGameplayState::default();
        state.inventory[0] = Some(rod(durability));
        state
    }

    fn context(hook_entity_id: u64, open_water: bool) -> FishingDomainContext {
        FishingDomainContext {
            world_seed: 0xA55A_1234,
            hook_entity_id,
            player_position_milli: [0, 64_000, 0],
            open_water,
            water_surface_y_milli: open_water.then_some(64_800),
            consume_durability: true,
        }
    }

    fn cast_and_land(state: &mut SessionGameplayState, hook_entity_id: u64) {
        cast(state, 7, 0, [0, 0, 1_000], context(hook_entity_id, false)).unwrap();
        let landed = tick(state, context(hook_entity_id, true)).unwrap();
        assert_eq!(landed.event, FishingTickEvent::Landed);
    }

    fn advance_to_bite(state: &mut SessionGameplayState, hook_entity_id: u64) {
        cast_and_land(state, hook_entity_id);
        for _ in 0..FISHING_INITIAL_WAIT_TICKS {
            assert_eq!(
                tick(state, context(hook_entity_id, true)).unwrap().event,
                FishingTickEvent::Waiting
            );
        }
        assert_eq!(
            tick(state, context(hook_entity_id, true)).unwrap().event,
            FishingTickEvent::BiteStarted
        );
    }

    #[test]
    fn cast_recast_and_cancel_are_atomic() {
        let mut state = state_with_rod(64);
        let cast_outcome = cast(&mut state, 7, 0, [0, 0, 1_000], context(11, false)).unwrap();
        assert_eq!(cast_outcome.rod_slot, 0);
        let after_cast = state;
        assert_eq!(
            cast(&mut state, 7, 0, [0, 0, 1_000], context(12, false)),
            Err(FishingDomainError::HookAlreadyActive)
        );
        assert_eq!(state, after_cast);
        assert_eq!(
            cancel(&mut state, 0, context(11, false)).unwrap().rod_slot,
            0
        );
        assert!(state.fishing_hook.is_none());
    }

    #[test]
    fn bite_and_reel_are_deterministic_and_grant_bounded_loot_xp() {
        let mut state = state_with_rod(64);
        advance_to_bite(&mut state, 21);
        let before_reel = state;
        let mut retry = before_reel;
        let outcome = reel(&mut state, 7, 0, context(21, true)).unwrap();
        let retried = reel(&mut retry, 7, 0, context(21, true)).unwrap();
        assert_eq!(outcome, retried);
        assert_eq!(state, retry);
        assert!(matches!(outcome.result, FishingReelResult::Caught(_)));
        assert!((1..=6).contains(&outcome.experience));
        assert!(state.fishing_hook.is_none());
    }

    #[test]
    fn too_early_and_too_late_reels_are_typed_misses() {
        let mut early = state_with_rod(64);
        cast_and_land(&mut early, 31);
        assert!(matches!(
            reel(&mut early, 7, 0, context(31, true)).unwrap().result,
            FishingReelResult::Missed(FishingMissReason::TooEarly)
        ));

        let mut late = state_with_rod(64);
        advance_to_bite(&mut late, 32);
        for _ in 0..=FISHING_BITE_WINDOW_TICKS {
            tick(&mut late, context(32, true)).unwrap();
        }
        assert!(matches!(
            reel(&mut late, 7, 0, context(32, true)).unwrap().result,
            FishingReelResult::Missed(FishingMissReason::TooLate)
        ));
    }

    #[test]
    fn last_durability_breaks_rod_after_successful_catch() {
        let mut state = state_with_rod(1);
        advance_to_bite(&mut state, 41);
        let outcome = reel(&mut state, 7, 0, context(41, true)).unwrap();
        assert_eq!(outcome.rod, RodDurabilityOutcome::Broken { slot: 0 });
        assert!(state
            .inventory
            .iter()
            .flatten()
            .all(|entry| entry.item.item != Item::FishingRod as u32));
    }

    #[test]
    fn inventory_full_catch_rolls_back_hook_rod_loot_and_xp() {
        let mut state = state_with_rod(2);
        for entry in &mut state.inventory[1..SESSION_INVENTORY_SLOTS] {
            *entry = Some(slot(Item::Stone, 64));
        }
        advance_to_bite(&mut state, 51);
        let before = state;
        assert_eq!(
            reel(&mut state, 7, 0, context(51, true)),
            Err(FishingDomainError::InventoryFull)
        );
        assert_eq!(state, before);
    }

    #[test]
    fn invalid_hand_rod_and_context_leave_state_unchanged() {
        let mut state = state_with_rod(0);
        let before = state;
        assert_eq!(
            cast(&mut state, 7, 0, [0, 0, 1_000], context(61, false)),
            Err(FishingDomainError::InvalidRod)
        );
        assert_eq!(state, before);

        let mut state = state_with_rod(64);
        let before = state;
        assert_eq!(
            cast(&mut state, 7, 2, [0, 0, 1_000], context(61, false)),
            Err(FishingDomainError::InvalidHand)
        );
        assert_eq!(state, before);
    }

    #[test]
    fn each_tick_is_single_step_and_corrupt_counters_are_rejected() {
        let mut state = state_with_rod(64);
        cast(&mut state, 7, 0, [0, 0, 1_000], context(71, false)).unwrap();
        assert_eq!(water_probe_position(&state).unwrap(), [0, 65_740, 700]);
        assert_eq!(
            tick(&mut state, context(71, true)).unwrap().event,
            FishingTickEvent::Landed
        );
        let before_wait = state.fishing_hook.unwrap().wait_ticks_remaining;
        tick(&mut state, context(71, true)).unwrap();
        assert_eq!(
            state.fishing_hook.unwrap().wait_ticks_remaining,
            before_wait - 1
        );

        state.fishing_hook.as_mut().unwrap().wait_ticks_remaining = u32::MAX;
        let before = state;
        assert_eq!(
            tick(&mut state, context(71, true)),
            Err(FishingDomainError::CorruptHook)
        );
        assert_eq!(state, before);
    }
}
