use super::*;

impl AuthorityCore {
    pub(super) fn apply_block_action(
        &mut self,
        session_id: PlayerId,
        action: BlockActionKind,
        position: (i32, i32, i32),
        face: [i8; 3],
        hand: u8,
        held: Option<SessionSlotWire>,
        block_wire: u32,
        look_milli: [i16; 3],
    ) -> Result<Option<WorldMutation>, RejectReason> {
        let Some(session) = self.sessions.get(&session_id).map(|s| s.action_view()) else {
            return Err(RejectReason::Unauthorized);
        };
        let Some(dimension) = Dimension::from_wire(session.dimension) else {
            return Err(RejectReason::InvalidDimension);
        };
        if dimension != self.world(dimension).dimension {
            return Err(RejectReason::InvalidDimension);
        }
        if matches!(action, BlockActionKind::CancelBreak) {
            if let Some(session) = self.sessions.get_mut(&session_id) {
                session.gameplay.mining = None;
            }
            return Ok(None);
        }
        if matches!(action, BlockActionKind::EnterPortal) {
            let expected =
                crate::world::BlockType::from_wire(block_wire).ok_or(RejectReason::InvalidState)?;
            let actual = self
                .world_mut_expect(dimension)
                .chunks
                .get_loaded_block(position.0, position.1, position.2)
                .ok_or(RejectReason::InvalidState)?;
            let feet = (
                session.position[0].floor() as i32,
                session.position[1].floor() as i32,
                session.position[2].floor() as i32,
            );
            let body = (feet.0, feet.1 + 1, feet.2);
            if actual != expected
                || !matches!(
                    actual,
                    crate::world::BlockType::NetherPortal
                        | crate::world::BlockType::EndPortal
                        | crate::world::BlockType::EndGateway
                )
                || (position != feet && position != body)
                || session.portal_cooldown > 0.0
            {
                return Err(RejectReason::InvalidState);
            }
            if let Some(session) = self.sessions.get_mut(&session_id) {
                session.portal_contact_time = 0.0;
                session.portal_requested = true;
                session.gameplay.mining = None;
            }
            return Ok(None);
        }
        let slot_index = if hand == 0 {
            session.gameplay.selected_hotbar_slot
        } else if hand == 1 {
            40
        } else {
            return Err(RejectReason::InvalidState);
        };
        let current = session.gameplay.slot(slot_index).flatten();
        let current_stack = match held {
            Some(expected) => {
                if current != Some(SessionInventorySlot::from(expected)) {
                    return Err(RejectReason::InvalidState);
                }
                stack_from_slot(Some(expected))
            }
            None => {
                if current.is_some() {
                    return Err(RejectReason::InvalidState);
                }
                None
            }
        };
        if !self
            .world(dimension)
            .valid_coordinate(position.0, position.1, position.2)
        {
            return Err(RejectReason::InvalidCoordinate);
        }

        match action {
            BlockActionKind::StartBreak => {
                let Some(target_block) = self
                    .world_mut_expect(dimension)
                    .chunks
                    .get_loaded_block(position.0, position.1, position.2)
                else {
                    return Err(RejectReason::InvalidState);
                };
                if target_block == crate::world::BlockType::Air
                    || !self.world_mut_expect(dimension).has_block_line_of_sight(
                        session.position,
                        look_milli,
                        position,
                    )
                {
                    return Err(RejectReason::InvalidState);
                }
                let policy = crate::game_rules::GameModePolicy::for_rules(
                    session.game_mode,
                    &self.world(dimension).rules,
                );
                if !policy.can_break_stack(current_stack.as_ref(), target_block) {
                    return Err(RejectReason::PermissionDenied);
                }
                let target_state = self
                    .world(dimension)
                    .get_block_state(position.0, position.1, position.2);
                let progress = MiningProgressState {
                    dimension: dimension as u8,
                    target: [position.0, position.1, position.2],
                    progress_milli: session
                        .gameplay
                        .mining
                        .filter(|active| {
                            active.dimension == dimension as u8
                                && active.target == [position.0, position.1, position.2]
                                && active.hand == hand
                                && active.slot_index == slot_index
                                && active.held == held
                                && active.block == target_block.to_wire()
                                && active.state == target_state
                        })
                        .map_or(0, |active| active.progress_milli),
                    hand,
                    slot_index,
                    held,
                    block: target_block.to_wire(),
                    state: target_state,
                    look_milli,
                };
                if let Some(session) = self.sessions.get_mut(&session_id) {
                    session.gameplay.mining = Some(progress);
                }
                if session.game_mode == crate::inventory::GameMode::Creative {
                    let _ = self.commit_mining_break(
                        session_id,
                        dimension,
                        position,
                        current_stack,
                        session.game_mode,
                    );
                }
                Ok(None)
            }
            BlockActionKind::CancelBreak => unreachable!("cancel handled before slot validation"),
            BlockActionKind::Place => {
                let block = crate::world::BlockType::from_wire(block_wire)
                    .ok_or(RejectReason::InvalidState)?;
                if !self.world(dimension).has_block_line_of_sight(
                    session.position,
                    look_milli,
                    (
                        position.0 - i32::from(face[0]),
                        position.1 - i32::from(face[1]),
                        position.2 - i32::from(face[2]),
                    ),
                ) {
                    return Err(RejectReason::InvalidState);
                }
                let support = (
                    position.0.saturating_sub(i32::from(face[0])),
                    position.1.saturating_sub(i32::from(face[1])),
                    position.2.saturating_sub(i32::from(face[2])),
                );
                let Some(support_block) = self
                    .world_mut_expect(dimension)
                    .chunks
                    .get_loaded_block(support.0, support.1, support.2)
                else {
                    return Err(RejectReason::InvalidState);
                };
                let policy = crate::game_rules::GameModePolicy::for_rules(
                    session.game_mode,
                    &self.world(dimension).rules,
                );
                let Some(held_stack) = current_stack.as_ref() else {
                    return Err(RejectReason::PermissionDenied);
                };
                if held_stack.item.properties().block_type != Some(block) {
                    return Err(RejectReason::InvalidState);
                }
                if !policy.can_place_stack(Some(held_stack), support_block) {
                    return Err(RejectReason::PermissionDenied);
                }
                // Prepare the inventory debit before mutating the world. The
                // copied gameplay state makes the place transaction atomic if
                // a late slot check ever fails, and placing always cancels an
                // in-flight mining target for this owner.
                let mut next_gameplay = session.gameplay;
                next_gameplay.mining = None;
                if session.game_mode != crate::inventory::GameMode::Creative {
                    let index = usize::from(slot_index);
                    let Some(Some(slot)) = next_gameplay.inventory.get_mut(index) else {
                        return Err(RejectReason::PermissionDenied);
                    };
                    if slot.item.count == 0 {
                        return Err(RejectReason::InvalidState);
                    }
                    slot.item.count -= 1;
                    if slot.item.count == 0 {
                        next_gameplay.inventory[index] = None;
                    }
                }
                let mutation = self
                    .world_mut_expect(dimension)
                    .apply_block_place(position, face, block)?;
                let Some(session) = self.sessions.get_mut(&session_id) else {
                    return Err(RejectReason::Unauthorized);
                };
                session.gameplay = next_gameplay;
                Ok(mutation)
            }
            BlockActionKind::IgnitePortal => {
                let Some(held_stack) = current_stack else {
                    return Err(RejectReason::PermissionDenied);
                };
                if held_stack.item != crate::inventory::Item::FlintAndSteel
                    || self.world(dimension).get_block(position.0, position.1, position.2)
                        != crate::world::BlockType::Air
                {
                    return Err(RejectReason::InvalidState);
                }
                let support = (
                    position.0.saturating_sub(i32::from(face[0])),
                    position.1.saturating_sub(i32::from(face[1])),
                    position.2.saturating_sub(i32::from(face[2])),
                );
                if self.world(dimension).get_block(support.0, support.1, support.2)
                    != crate::world::BlockType::Obsidian
                    || !self.world_mut_expect(dimension).has_block_line_of_sight(
                        session.position,
                        look_milli,
                        support,
                    )
                {
                    return Err(RejectReason::InvalidState);
                }
                // Debit the exact held slot on a clone before Fire (and any
                // recursive portal interiors) can enter the world.
                let mut next_gameplay = session.gameplay;
                next_gameplay.mining = None;
                if session.game_mode != crate::inventory::GameMode::Creative {
                    let index = usize::from(slot_index);
                    let Some(slot) = next_gameplay.inventory[index].as_mut() else {
                        return Err(RejectReason::InvalidState);
                    };
                    slot.item.durability = slot.item.durability.saturating_sub(1);
                    if slot.item.durability == 0 {
                        next_gameplay.inventory[index] = None;
                    }
                }
                if !preserves_brew_locks(&session.gameplay, &next_gameplay) {
                    return Err(RejectReason::InvalidState);
                }
                let mutation = self.world_mut_expect(dimension).set_block(
                    position.0,
                    position.1,
                    position.2,
                    crate::world::BlockType::Fire,
                    0,
                )?;
                if mutation.is_none() {
                    return Err(RejectReason::InvalidState);
                }
                let Some(target) = self.sessions.get_mut(&session_id) else {
                    return Err(RejectReason::Unauthorized);
                };
                target.gameplay = next_gameplay;
                Ok(mutation)
            }
            BlockActionKind::InsertEnderEye => {
                let Some(held_stack) = current_stack else {
                    return Err(RejectReason::PermissionDenied);
                };
                if held_stack.item != crate::inventory::Item::EyeOfEnder
                    || self.world(dimension).get_block(position.0, position.1, position.2)
                        != crate::world::BlockType::EndPortalFrame
                    || !self.world_mut_expect(dimension).has_block_line_of_sight(
                        session.position,
                        look_milli,
                        position,
                    )
                {
                    return Err(RejectReason::InvalidState);
                }
                let mut next_gameplay = session.gameplay;
                next_gameplay.mining = None;
                if session.game_mode != crate::inventory::GameMode::Creative {
                    let index = usize::from(slot_index);
                    let Some(slot) = next_gameplay.inventory[index].as_mut() else {
                        return Err(RejectReason::InvalidState);
                    };
                    if slot.item.count == 0 {
                        return Err(RejectReason::InvalidState);
                    }
                    slot.item.count -= 1;
                    if slot.item.count == 0 {
                        next_gameplay.inventory[index] = None;
                    }
                }
                if !preserves_brew_locks(&session.gameplay, &next_gameplay) {
                    return Err(RejectReason::InvalidState);
                }
                let mut filled = crate::world::BlockState::default();
                filled.is_open = true;
                let mutation = self.world_mut_expect(dimension).set_block(
                    position.0,
                    position.1,
                    position.2,
                    crate::world::BlockType::EndPortalFrame,
                    filled.encode(),
                )?;
                if mutation.is_none() {
                    return Err(RejectReason::InvalidState);
                }
                let Some(target) = self.sessions.get_mut(&session_id) else {
                    return Err(RejectReason::Unauthorized);
                };
                target.gameplay = next_gameplay;
                Ok(mutation)
            }
            BlockActionKind::EnterPortal => {
                unreachable!("portal entry handled before slot validation")
            }
        }
    }
}
