//! Pure authoritative workstation transaction domain.
//!
//! Callers own dimension, reach and revision gates.  This module owns the
//! remaining trust boundary: the actual station kind, exact rich inventory
//! identities, recipe/cost calculation and atomic publication of session plus
//! workstation state.

use crate::authority::contract::{SessionBrewState, SessionGameplayState, SessionInventorySlot};
use crate::block_entity::FurnaceBlockEntity;
use crate::brewing::brew;
use crate::enchantment::{can_enchant, generate_options, AnvilState, EnchantmentSet};
use crate::inventory::{Item, ItemStack};
use crate::network::protocol::{ItemWire, RejectReason, SessionSlotWire, SlotRefWire, MAX_ANVIL_RENAME_BYTES};
use crate::recipes::RecipeManager;
use crate::world::BlockType;

pub const BREW_TICKS: u16 = 200;

// Transaction domain errors collapse to RejectReason::InvalidState (Wave 10 Plan 10).

/// Small caller-built proof of the authoritative block at an already-gated
/// position.  Bookshelf power is clamped here so downstream offer generation
/// never consumes an attacker-sized value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkstationContext {
    position: Option<[i32; 3]>,
    block: Option<BlockType>,
    bookshelves: u8,
}

impl WorkstationContext {
    pub const fn personal_crafting() -> Self {
        Self {
            position: None,
            block: None,
            bookshelves: 0,
        }
    }

    pub const fn at(position: [i32; 3], block: BlockType) -> Self {
        Self {
            position: Some(position),
            block: Some(block),
            bookshelves: 0,
        }
    }

    pub fn enchanting(position: [i32; 3], block: BlockType, bookshelves: u8) -> Self {
        Self {
            position: Some(position),
            block: Some(block),
            bookshelves: bookshelves.min(15),
        }
    }

    pub const fn position(self) -> Option<[i32; 3]> {
        self.position
    }

    pub const fn block(self) -> Option<BlockType> {
        self.block
    }

    pub const fn bookshelves(self) -> u8 {
        self.bookshelves
    }

    fn require_block(
        self,
        expected: impl FnOnce(BlockType) -> bool,
    ) -> Result<(), RejectReason> {
        match self.block {
            Some(block) if self.position.is_some() && expected(block) => Ok(()),
            _ => Err(RejectReason::InvalidState),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CraftReceipt {
    pub result: SessionInventorySlot,
    pub consumed_slots: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FurnaceReceipt {
    pub output: SessionInventorySlot,
    pub experience: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnchantReceipt {
    pub result: SessionInventorySlot,
    pub level_cost: u8,
    pub lapis_cost: u8,
    pub enchantments: EnchantmentSet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnvilReceipt {
    pub result: SessionInventorySlot,
    pub level_cost: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrewTick {
    Brewing { remaining_ticks: u16 },
    Ready,
}

fn stack_from_source(source: SlotRefWire) -> Result<ItemStack, RejectReason> {
    source
        .validate_bounds()
        .map_err(|_| RejectReason::InvalidState)?;
    let mut stack = source
        .expected
        .item
        .to_stack()
        .ok_or(RejectReason::InvalidState)?;
    stack.count = u32::from(source.count);
    stack.can_break = source.expected.can_break;
    stack.can_place_on = source.expected.can_place_on;
    Ok(stack)
}

fn slot_from_stack(stack: ItemStack) -> SessionInventorySlot {
    SessionInventorySlot::from_wire(
        ItemWire::from_stack(&stack),
        stack.can_break,
        stack.can_place_on,
    )
}

fn wire_from_stack(stack: ItemStack) -> SessionSlotWire {
    SessionSlotWire::new(
        ItemWire::from_stack(&stack),
        stack.can_break,
        stack.can_place_on,
    )
}

fn exact_source(
    session: &SessionGameplayState,
    source: SlotRefWire,
) -> Result<(), RejectReason> {
    source
        .validate_bounds()
        .map_err(|_| RejectReason::InvalidState)?;
    if session.slot_matches(source) {
        Ok(())
    } else {
        Err(RejectReason::InvalidState)
    }
}

/// Whether an in-flight brew reserves this inventory index.  B2 uses this
/// before dispatching any other item-consuming domain so a pending ingredient
/// or bottle cannot be spent while its exact brew transaction is outstanding.
pub fn brew_locks_slot(session: &SessionGameplayState, index: u8) -> bool {
    session.brew.is_some_and(|pending| {
        pending.ingredient.index == index
            || pending
                .bottles
                .iter()
                .flatten()
                .any(|source| source.index == index)
    })
}

/// Match and settle one 2x2 personal or 3x3 table craft.  Each occupied grid
/// cell consumes exactly one unit; repeated inventory indices are deliberately
/// permitted and are aggregated by `consume_slots_exact` against the original
/// rich identity.
pub fn execute_craft(
    session: &mut SessionGameplayState,
    recipes: &RecipeManager,
    context: WorkstationContext,
    grid_size: u8,
    sources: [Option<SlotRefWire>; 9],
) -> Result<CraftReceipt, RejectReason> {
    let active_len = match grid_size {
        2 if context == WorkstationContext::personal_crafting() => 4,
        3 => {
            context.require_block(|block| block == BlockType::CraftingTable)?;
            9
        }
        _ => return Err(RejectReason::InvalidState),
    };
    if sources[active_len..].iter().any(Option::is_some) {
        return Err(RejectReason::InvalidState);
    }

    let mut grid = [None; 9];
    let mut debits = Vec::with_capacity(active_len);
    for (index, source) in sources.into_iter().take(active_len).enumerate() {
        let Some(source) = source else {
            continue;
        };
        if source.count != 1 {
            return Err(RejectReason::InvalidState);
        }
        exact_source(session, source)?;
        grid[index] = Some(stack_from_source(source)?);
        debits.push(source);
    }

    let output = recipes
        .match_crafting_recipe(&grid[..active_len], usize::from(grid_size))
        .ok_or(RejectReason::InvalidState)?;
    let output_slot = slot_from_stack(output);
    let mut candidate = *session;
    if !candidate.consume_slots_exact(&debits) {
        return Err(RejectReason::InvalidState);
    }
    if !candidate.add_slot(output_slot) {
        return Err(RejectReason::InvalidState);
    }
    *session = candidate;
    Ok(CraftReceipt {
        result: output_slot,
        consumed_slots: debits.len() as u8,
    })
}

/// Transfer furnace output and its accumulated XP as one transaction.  The
/// furnace's coarse XP accumulator follows the existing claim semantics: the
/// first successful output take claims it all, while failed inventory capacity
/// checks leave both the output and XP untouched.
pub fn execute_furnace_take_output(
    session: &mut SessionGameplayState,
    furnace: &mut FurnaceBlockEntity,
    context: WorkstationContext,
    count: u16,
) -> Result<FurnaceReceipt, RejectReason> {
    context.require_block(|block| matches!(block, BlockType::Furnace))?;
    if count == 0 {
        return Err(RejectReason::InvalidState);
    }
    let Some(current_output) = furnace.slots[2] else {
        return Err(RejectReason::InvalidState);
    };
    if u32::from(count) > current_output.count {
        return Err(RejectReason::InvalidState);
    }

    let mut taken = current_output;
    taken.count = u32::from(count);
    let output_slot = slot_from_stack(taken);
    let experience = if furnace.accumulated_xp.is_finite() && furnace.accumulated_xp > 0.0 {
        furnace.accumulated_xp.floor().min(u32::MAX as f32) as u32
    } else {
        0
    };

    let mut next_session = *session;
    let mut next_furnace = furnace.clone();
    if !next_session.add_slot(output_slot) {
        return Err(RejectReason::InvalidState);
    }
    if !next_session.grant_experience(experience) {
        return Err(RejectReason::InvalidState);
    }
    let remaining = current_output.count - u32::from(count);
    next_furnace.slots[2] = (remaining > 0).then_some(ItemStack {
        count: remaining,
        ..current_output
    });
    next_furnace.accumulated_xp = 0.0;
    next_furnace.revision = next_furnace.revision.wrapping_add(1);

    *session = next_session;
    *furnace = next_furnace;
    Ok(FurnaceReceipt {
        output: output_slot,
        experience,
    })
}

/// Recompute the selected offer from the authoritative seed/bookshelf count,
/// then atomically debit levels and lapis and replace the exact rich source.
pub fn execute_enchant(
    session: &mut SessionGameplayState,
    context: WorkstationContext,
    source: SlotRefWire,
    option_index: u8,
) -> Result<EnchantReceipt, RejectReason> {
    context.require_block(|block| block == BlockType::EnchantingTable)?;
    let option = usize::from(option_index);
    if option >= 3 || source.count != 1 {
        return Err(RejectReason::InvalidState);
    }
    exact_source(session, source)?;
    let mut input = stack_from_source(source)?;
    if !can_enchant(input.item) {
        return Err(RejectReason::InvalidState);
    }

    let offers = generate_options(input.item, context.bookshelves, session.enchant_seed as u32);
    let selected = offers[option];
    input.enchantments.merge(&selected.enchantments);
    let replacement = wire_from_stack(input);

    let mut candidate = *session;
    if !candidate.spend_levels(u32::from(selected.cost)) {
        return Err(RejectReason::InvalidState);
    }
    if !candidate.remove_item(Item::LapisLazuli.to_u32(), u32::from(selected.lapis_cost)) {
        return Err(RejectReason::InvalidState);
    }
    if !candidate.replace_slot_exact(source, Some(replacement)) {
        return Err(RejectReason::InvalidState);
    }
    candidate.enchant_seed = candidate.enchant_seed.wrapping_add(0x9E37_79B9);
    *session = candidate;

    Ok(EnchantReceipt {
        result: SessionInventorySlot::from(replacement),
        level_cost: selected.cost,
        lapis_cost: selected.lapis_cost,
        enchantments: selected.enchantments,
    })
}

/// Reserve exact ingredient/bottle identities for an authoritative 200-tick
/// brew.  Inventory is not debited until `take_brew` publishes the result.
pub fn start_brew(
    session: &mut SessionGameplayState,
    context: WorkstationContext,
    ingredient: SlotRefWire,
    bottles: [Option<SlotRefWire>; 3],
) -> Result<(), RejectReason> {
    context.require_block(|block| block == BlockType::BrewingStand)?;
    let station = context.position.ok_or(RejectReason::InvalidState)?;
    if session.brew.is_some() {
        return Err(RejectReason::InvalidState);
    }
    if ingredient.count != 1 {
        return Err(RejectReason::InvalidState);
    }
    exact_source(session, ingredient)?;
    let ingredient_item = stack_from_source(ingredient)?.item;

    let mut any_brewable = false;
    let mut seen = [false; crate::authority::contract::SESSION_INVENTORY_SLOTS];
    seen[usize::from(ingredient.index)] = true;
    for bottle in bottles.iter().flatten().copied() {
        if bottle.count != 1 || seen[usize::from(bottle.index)] {
            return Err(RejectReason::InvalidState);
        }
        seen[usize::from(bottle.index)] = true;
        exact_source(session, bottle)?;
        let stack = stack_from_source(bottle)?;
        if !matches!(stack.item, Item::Potion | Item::SplashPotion) {
            return Err(RejectReason::InvalidState);
        }
        any_brewable |= stack
            .potion
            .and_then(|potion| brew(potion, ingredient_item))
            .is_some();
    }
    if !any_brewable {
        return Err(RejectReason::InvalidState);
    }

    session.brew = Some(SessionBrewState {
        station,
        ingredient,
        bottles,
        remaining_ticks: BREW_TICKS,
    });
    Ok(())
}

/// Advance one fixed authority tick.  Ready state is reached only after exactly
/// `BREW_TICKS` calls and remains stable until take/cancel.
pub fn tick_brew(
    session: &mut SessionGameplayState,
    context: WorkstationContext,
) -> Result<BrewTick, RejectReason> {
    context.require_block(|block| block == BlockType::BrewingStand)?;
    let station = context.position.ok_or(RejectReason::InvalidState)?;
    let Some(mut pending) = session.brew else {
        return Err(RejectReason::InvalidState);
    };
    if pending.station != station {
        return Err(RejectReason::InvalidState);
    }
    if pending.remaining_ticks == 0 {
        return Ok(BrewTick::Ready);
    }
    pending.remaining_ticks -= 1;
    session.brew = Some(pending);
    if pending.remaining_ticks == 0 {
        Ok(BrewTick::Ready)
    } else {
        Ok(BrewTick::Brewing {
            remaining_ticks: pending.remaining_ticks,
        })
    }
}

/// Publish a completed brew.  Exact identities are revalidated so moving or
/// mutating a reserved item while brewing aborts without consuming anything.
pub fn take_brew(
    session: &mut SessionGameplayState,
    context: WorkstationContext,
) -> Result<[Option<SessionInventorySlot>; 3], RejectReason> {
    context.require_block(|block| block == BlockType::BrewingStand)?;
    let station = context.position.ok_or(RejectReason::InvalidState)?;
    let pending = session.brew.ok_or(RejectReason::InvalidState)?;
    if pending.station != station {
        return Err(RejectReason::InvalidState);
    }
    if pending.remaining_ticks != 0 {
        return Err(RejectReason::InvalidState);
    }

    exact_source(session, pending.ingredient)?;
    let ingredient = stack_from_source(pending.ingredient)?.item;
    let mut outputs = [None; 3];
    let mut replacements = [None; 3];
    for (index, source) in pending.bottles.into_iter().enumerate() {
        let Some(source) = source else {
            continue;
        };
        exact_source(session, source)?;
        let mut bottle = stack_from_source(source)?;
        let Some(next) = bottle.potion.and_then(|potion| brew(potion, ingredient)) else {
            continue;
        };
        bottle.potion = Some(next);
        bottle.item = if next.splash {
            Item::SplashPotion
        } else {
            Item::Potion
        };
        let replacement = wire_from_stack(bottle);
        replacements[index] = Some((source, replacement));
        outputs[index] = Some(SessionInventorySlot::from(replacement));
    }
    if outputs.iter().all(Option::is_none) {
        return Err(RejectReason::InvalidState);
    }

    let mut candidate = *session;
    if !candidate.consume_slot_exact(pending.ingredient) {
        return Err(RejectReason::InvalidState);
    }
    for replacement in replacements.into_iter().flatten() {
        if !candidate.replace_slot_exact(replacement.0, Some(replacement.1)) {
            return Err(RejectReason::InvalidState);
        }
    }
    candidate.brew = None;
    *session = candidate;
    Ok(outputs)
}

pub fn cancel_brew(
    session: &mut SessionGameplayState,
    context: WorkstationContext,
) -> Result<(), RejectReason> {
    context.require_block(|block| block == BlockType::BrewingStand)?;
    let station = context.position.ok_or(RejectReason::InvalidState)?;
    let pending = session.brew.ok_or(RejectReason::InvalidState)?;
    if pending.station != station {
        return Err(RejectReason::InvalidState);
    }
    session.brew = None;
    Ok(())
}

/// Compute an anvil output from exact one-item sources, charge the authoritative
/// cost and publish the output only after every debit succeeds.
pub fn execute_anvil(
    session: &mut SessionGameplayState,
    context: WorkstationContext,
    left: SlotRefWire,
    right: Option<SlotRefWire>,
    rename: &str,
) -> Result<AnvilReceipt, RejectReason> {
    context.require_block(|block| block == BlockType::Anvil)?;
    if rename.as_bytes().len() > MAX_ANVIL_RENAME_BYTES || left.count != 1 {
        return Err(RejectReason::InvalidState);
    }
    if right.is_some_and(|source| source.count != 1) {
        return Err(RejectReason::InvalidState);
    }
    if right.is_some_and(|source| source.index == left.index) {
        return Err(RejectReason::InvalidState);
    }
    exact_source(session, left)?;
    if let Some(source) = right {
        exact_source(session, source)?;
    }

    let mut anvil = AnvilState {
        left: Some(stack_from_source(left)?),
        right: right.map(stack_from_source).transpose()?,
        rename: rename.to_owned(),
        ..Default::default()
    };
    anvil.refresh();
    let output = anvil.output.ok_or(RejectReason::InvalidState)?;
    let output_slot = slot_from_stack(output);

    let mut candidate = *session;
    if !candidate.spend_levels(u32::from(anvil.cost)) {
        return Err(RejectReason::InvalidState);
    }
    let mut sources = Vec::with_capacity(2);
    sources.push(left);
    sources.extend(right);
    if !candidate.consume_slots_exact(&sources) {
        return Err(RejectReason::InvalidState);
    }
    if !candidate.add_slot(output_slot) {
        return Err(RejectReason::InvalidState);
    }
    *session = candidate;
    Ok(AnvilReceipt {
        result: output_slot,
        level_cost: anvil.cost,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brewing::PotionKind;

    fn rich_stack(stack: ItemStack) -> SessionInventorySlot {
        slot_from_stack(stack)
    }

    fn source(session: &SessionGameplayState, index: u8, count: u16) -> SlotRefWire {
        SlotRefWire {
            index,
            count,
            expected: SessionSlotWire::from(session.inventory[usize::from(index)].unwrap()),
        }
    }

    fn fill_with_stone(session: &mut SessionGameplayState) {
        for slot in &mut session.inventory {
            *slot = Some(rich_stack(ItemStack::new(Item::Stone, 64)));
        }
    }

    #[test]
    fn craft_aggregates_duplicate_sources_and_rolls_back_when_full() {
        let recipes = RecipeManager::new();
        let mut session = SessionGameplayState::default();
        session.inventory[0] = Some(rich_stack(ItemStack::new(Item::OakPlanks, 2)));
        let repeated = source(&session, 0, 1);
        let mut grid = [None; 9];
        grid[0] = Some(repeated);
        grid[2] = Some(repeated);
        let receipt = execute_craft(
            &mut session,
            &recipes,
            WorkstationContext::personal_crafting(),
            2,
            grid,
        )
        .unwrap();
        assert_eq!(receipt.consumed_slots, 2);
        assert_eq!(receipt.result.item.item, Item::Stick.to_u32());
        assert_eq!(session.count_item(Item::OakPlanks.to_u32()), 0);
        assert_eq!(session.count_item(Item::Stick.to_u32()), 4);

        fill_with_stone(&mut session);
        session.inventory[0] = Some(rich_stack(ItemStack::new(Item::OakLog, 2)));
        let before = session;
        let mut grid = [None; 9];
        grid[0] = Some(source(&session, 0, 1));
        assert_eq!(
            execute_craft(
                &mut session,
                &recipes,
                WorkstationContext::personal_crafting(),
                2,
                grid,
            ),
            Err(RejectReason::InvalidState)
        );
        assert_eq!(session, before);
    }

    #[test]
    fn craft_rejects_stale_rich_metadata_without_mutation() {
        let recipes = RecipeManager::new();
        let mut stack = ItemStack::new(Item::OakLog, 1);
        stack.custom_name.set("fresh");
        let mut session = SessionGameplayState::default();
        session.inventory[0] = Some(rich_stack(stack));
        let before = session;
        let mut stale = source(&session, 0, 1);
        stale.expected.item.custom_name = [0; 24];
        let mut grid = [None; 9];
        grid[0] = Some(stale);
        assert_eq!(
            execute_craft(
                &mut session,
                &recipes,
                WorkstationContext::personal_crafting(),
                2,
                grid,
            ),
            Err(RejectReason::InvalidState)
        );
        assert_eq!(session, before);
    }

    #[test]
    fn furnace_output_and_xp_commit_together_and_validate_station() {
        let mut session = SessionGameplayState::default();
        let mut furnace = FurnaceBlockEntity::new();
        furnace.slots[2] = Some(ItemStack::new(Item::IronIngot, 4));
        furnace.accumulated_xp = 7.0;
        furnace.revision = 9;
        let invalid_session = session;
        let invalid_furnace = furnace.clone();
        assert_eq!(
            execute_furnace_take_output(
                &mut session,
                &mut furnace,
                WorkstationContext::at([1, 2, 3], BlockType::CraftingTable),
                2,
            ),
            Err(RejectReason::InvalidState)
        );
        assert_eq!(session, invalid_session);
        assert_eq!(furnace, invalid_furnace);

        let receipt = execute_furnace_take_output(
            &mut session,
            &mut furnace,
            WorkstationContext::at([1, 2, 3], BlockType::Furnace),
            2,
        )
        .unwrap();
        assert_eq!(receipt.experience, 7);
        assert_eq!(session.experience_level, 1);
        assert_eq!(session.count_item(Item::IronIngot.to_u32()), 2);
        assert_eq!(furnace.slots[2].unwrap().count, 2);
        assert_eq!(furnace.accumulated_xp, 0.0);
        assert_eq!(furnace.revision, 10);
    }

    #[test]
    fn enchant_is_seeded_and_rolls_back_insufficient_costs() {
        let context = WorkstationContext::enchanting([4, 5, 6], BlockType::EnchantingTable, 15);
        let mut session = SessionGameplayState::default();
        session.inventory[0] = Some(rich_stack(ItemStack::new(Item::IronPickaxe, 1)));
        session.inventory[1] = Some(rich_stack(ItemStack::new(Item::LapisLazuli, 3)));
        session.experience_level = 30;
        session.enchant_seed = 42;
        let expected = generate_options(Item::IronPickaxe, 15, 42)[2];
        let enchant_source = source(&session, 0, 1);
        let receipt = execute_enchant(&mut session, context, enchant_source, 2).unwrap();
        assert_eq!(receipt.enchantments, expected.enchantments);
        assert_eq!(receipt.level_cost, expected.cost);
        assert_eq!(receipt.lapis_cost, expected.lapis_cost);
        assert_eq!(session.experience_level, 30 - u32::from(expected.cost));
        assert_eq!(session.count_item(Item::LapisLazuli.to_u32()), 0);
        assert_eq!(session.enchant_seed, 42u64.wrapping_add(0x9E37_79B9));

        let mut poor = SessionGameplayState::default();
        poor.inventory[0] = Some(rich_stack(ItemStack::new(Item::IronPickaxe, 1)));
        poor.inventory[1] = Some(rich_stack(ItemStack::new(Item::LapisLazuli, 64)));
        let before = poor;
        let poor_source = source(&poor, 0, 1);
        assert_eq!(
            execute_enchant(&mut poor, context, poor_source, 2),
            Err(RejectReason::InvalidState)
        );
        assert_eq!(poor, before);

        let mut no_lapis = SessionGameplayState::default();
        no_lapis.inventory[0] = Some(rich_stack(ItemStack::new(Item::IronPickaxe, 1)));
        no_lapis.experience_level = 30;
        let before = no_lapis;
        let no_lapis_source = source(&no_lapis, 0, 1);
        assert_eq!(
            execute_enchant(&mut no_lapis, context, no_lapis_source, 2),
            Err(RejectReason::InvalidState)
        );
        assert_eq!(no_lapis, before);
    }

    #[test]
    fn brew_needs_exactly_two_hundred_ticks_and_revalidates_take() {
        let context = WorkstationContext::at([7, 8, 9], BlockType::BrewingStand);
        let mut session = SessionGameplayState::default();
        session.inventory[0] = Some(rich_stack(ItemStack::new(Item::NetherWart, 1)));
        session.inventory[1] = Some(rich_stack(ItemStack::new(Item::Potion, 1)));
        let ingredient = source(&session, 0, 1);
        let bottles = [Some(source(&session, 1, 1)), None, None];
        start_brew(&mut session, context, ingredient, bottles).unwrap();
        let inventory_before = session.inventory;
        for expected in (1..BREW_TICKS).rev() {
            assert_eq!(
                tick_brew(&mut session, context).unwrap(),
                BrewTick::Brewing {
                    remaining_ticks: expected,
                }
            );
        }
        assert_eq!(session.inventory, inventory_before);
        assert_eq!(tick_brew(&mut session, context).unwrap(), BrewTick::Ready);
        assert_eq!(session.inventory, inventory_before);
        let outputs = take_brew(&mut session, context).unwrap();
        assert_eq!(
            outputs[0].unwrap().item.potion.unwrap().kind,
            PotionKind::Awkward as u8
        );
        assert_eq!(session.count_item(Item::NetherWart.to_u32()), 0);
        assert!(session.brew.is_none());

        let mut stale = SessionGameplayState::default();
        stale.inventory[0] = Some(rich_stack(ItemStack::new(Item::NetherWart, 1)));
        stale.inventory[1] = Some(rich_stack(ItemStack::new(Item::Potion, 1)));
        let stale_ingredient = source(&stale, 0, 1);
        let stale_bottle = source(&stale, 1, 1);
        start_brew(
            &mut stale,
            context,
            stale_ingredient,
            [Some(stale_bottle), None, None],
        )
        .unwrap();
        stale.brew.as_mut().unwrap().remaining_ticks = 0;
        stale.inventory[1] = Some(rich_stack(ItemStack::new(Item::GlassBottle, 1)));
        let before = stale;
        assert_eq!(
            take_brew(&mut stale, context),
            Err(RejectReason::InvalidState)
        );
        assert_eq!(stale, before);
    }

    #[test]
    fn brew_slot_locks_cover_every_reserved_source_only() {
        let context = WorkstationContext::at([7, 8, 9], BlockType::BrewingStand);
        let mut session = SessionGameplayState::default();
        session.inventory[0] = Some(rich_stack(ItemStack::new(Item::NetherWart, 1)));
        for index in 1..=3 {
            session.inventory[index] = Some(rich_stack(ItemStack::new(Item::Potion, 1)));
        }
        let ingredient = source(&session, 0, 1);
        let bottles = [
            Some(source(&session, 1, 1)),
            Some(source(&session, 2, 1)),
            Some(source(&session, 3, 1)),
        ];
        start_brew(&mut session, context, ingredient, bottles).unwrap();
        for index in 0..=3 {
            assert!(brew_locks_slot(&session, index));
        }
        assert!(!brew_locks_slot(&session, 4));
        assert!(!brew_locks_slot(&session, u8::MAX));
        cancel_brew(&mut session, context).unwrap();
        for index in 0..=3 {
            assert!(!brew_locks_slot(&session, index));
        }
    }

    #[test]
    fn anvil_rejects_aliased_inputs_without_mutation() {
        let context = WorkstationContext::at([2, 3, 4], BlockType::Anvil);
        let mut session = SessionGameplayState::default();
        session.inventory[0] = Some(rich_stack(ItemStack::new(Item::IronPickaxe, 1)));
        session.experience_level = 5;
        let aliased = source(&session, 0, 1);
        let before = session;
        assert_eq!(
            execute_anvil(&mut session, context, aliased, Some(aliased), "Miner",),
            Err(RejectReason::InvalidState)
        );
        assert_eq!(session, before);
    }

    #[test]
    fn anvil_rename_bound_and_cost_are_atomic() {
        let context = WorkstationContext::at([2, 3, 4], BlockType::Anvil);
        let mut session = SessionGameplayState::default();
        session.inventory[0] = Some(rich_stack(ItemStack::new(Item::IronPickaxe, 1)));
        session.experience_level = 5;
        let before = session;
        let invalid_source = source(&session, 0, 1);
        assert_eq!(
            execute_anvil(
                &mut session,
                context,
                invalid_source,
                None,
                "1234567890123456789012345",
            ),
            Err(RejectReason::InvalidState)
        );
        assert_eq!(session, before);

        let valid_source = source(&session, 0, 1);
        let receipt = execute_anvil(&mut session, context, valid_source, None, "Miner").unwrap();
        assert_eq!(receipt.level_cost, 1);
        assert_eq!(
            receipt.result.item.to_stack().unwrap().custom_name.as_str(),
            "Miner"
        );
        assert_eq!(session.experience_level, 4);
        assert_eq!(
            session.inventory[0]
                .unwrap()
                .item
                .to_stack()
                .unwrap()
                .custom_name
                .as_str(),
            "Miner"
        );
    }
}
