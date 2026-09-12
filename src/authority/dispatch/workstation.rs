use super::*;

impl AuthorityCore {
    pub(super) fn apply_transaction_operation(
        &mut self,
        session_id: PlayerId,
        operation: &GameplayOperation,
    ) -> Result<Option<WorldMutation>, RejectReason> {
        use crate::authority::transactions::{self, WorkstationContext};
        use crate::inventory::Item;

        let Some(dimension) = self
            .sessions
            .get(&session_id)
            .and_then(|session| Dimension::from_wire(session.dimension))
        else {
            return Err(RejectReason::Unauthorized);
        };
        let Some(original) = self
            .sessions
            .get(&session_id)
            .map(|session| session.gameplay)
        else {
            return Err(RejectReason::Unauthorized);
        };
        let mut candidate = original;
        let mut mutation = None;
        match operation {
            GameplayOperation::FurnaceTakeOutput { x, y, z, count } => {
                mutation = Some(self.world_mut_expect(dimension).take_furnace_output(
                    &mut candidate,
                    [*x, *y, *z],
                    *count,
                )?);
            }
            GameplayOperation::Craft {
                grid,
                sources,
                station,
            } => {
                if sources
                    .iter()
                    .flatten()
                    .any(|source| transactions::brew_locks_slot(&candidate, source.index))
                {
                    return Err(RejectReason::InvalidState);
                }
                let context = match (*grid, *station) {
                    (2, None) => WorkstationContext::personal_crafting(),
                    (3, Some(position)) => WorkstationContext::at(
                        position,
                        self.world(dimension)
                            .get_block(position[0], position[1], position[2]),
                    ),
                    _ => return Err(RejectReason::InvalidState),
                };
                transactions::execute_craft(
                    &mut candidate,
                    &self.world(dimension).recipe_manager,
                    context,
                    *grid,
                    *sources,
                )?;
            }
            GameplayOperation::Enchant {
                x,
                y,
                z,
                source,
                option,
            } => {
                if transactions::brew_locks_slot(&candidate, source.index) {
                    return Err(RejectReason::InvalidState);
                }
                let position = [*x, *y, *z];
                let context = WorkstationContext::enchanting(
                    position,
                    self.world(dimension).get_block(*x, *y, *z),
                    self.world(dimension).bookshelf_power(position),
                );
                transactions::execute_enchant(&mut candidate, context, *source, *option)?;
                if !preserves_brew_locks(&original, &candidate) {
                    return Err(RejectReason::InvalidState);
                }
            }
            GameplayOperation::Brew {
                action,
                x,
                y,
                z,
                ingredient,
                bottles,
            } => {
                let position = [*x, *y, *z];
                let context = WorkstationContext::at(position, self.world(dimension).get_block(*x, *y, *z));
                match *action {
                    0 => {
                        let ingredient = ingredient.ok_or(RejectReason::InvalidState)?;
                        transactions::start_brew(&mut candidate, context, ingredient, *bottles)?;
                    }
                    1 if ingredient.is_none() && bottles.iter().all(Option::is_none) => {
                        transactions::cancel_brew(&mut candidate, context)?;
                    }
                    2 if ingredient.is_none() && bottles.iter().all(Option::is_none) => {
                        transactions::take_brew(&mut candidate, context)?;
                    }
                    _ => return Err(RejectReason::InvalidState),
                }
            }
            GameplayOperation::Anvil {
                x,
                y,
                z,
                left,
                right,
                rename,
            } => {
                if transactions::brew_locks_slot(&candidate, left.index)
                    || right.is_some_and(|source| {
                        transactions::brew_locks_slot(&candidate, source.index)
                    })
                {
                    return Err(RejectReason::InvalidState);
                }
                let position = [*x, *y, *z];
                let context = WorkstationContext::at(position, self.world(dimension).get_block(*x, *y, *z));
                transactions::execute_anvil(&mut candidate, context, *left, *right, rename)?;
            }
            GameplayOperation::UseState { hand, active } => {
                if *active {
                    let slot = held_slot_index(&candidate, *hand)?;
                    let held =
                        candidate.inventory[usize::from(slot)].ok_or(RejectReason::InvalidState)?;
                    if held.item.item != Item::Shield.to_u32()
                        || held.item.count != 1
                        || held.item.durability == 0
                        || candidate.shield_cooldown_ticks > 0
                    {
                        return Err(RejectReason::InvalidState);
                    }
                }
                candidate.shield_active = *active;
            }
            _ => return Err(RejectReason::Unsupported),
        }
        if !matches!(operation, GameplayOperation::Brew { .. })
            && !preserves_brew_locks(&original, &candidate)
        {
            return Err(RejectReason::InvalidState);
        }
        let Some(session) = self.sessions.get_mut(&session_id) else {
            return Err(RejectReason::Unauthorized);
        };
        session.gameplay = candidate;
        Ok(mutation)
    }
}
