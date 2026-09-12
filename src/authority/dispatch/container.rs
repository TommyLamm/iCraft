use super::*;

impl AuthorityCore {
    pub(super) fn apply_fluid_use(
        &mut self,
        session_id: PlayerId,
        position: (i32, i32, i32),
        face: [i8; 3],
        hand: u8,
        source: crate::network::protocol::SlotRefWire,
    ) -> Result<Option<WorldMutation>, RejectReason> {
        use crate::inventory::Item;
        use crate::network::protocol::SessionSlotWire;

        let Some((dimension, original)) = self
            .sessions
            .get(&session_id)
            .map(|session| (session.dimension, session.gameplay))
        else {
            return Err(RejectReason::Unauthorized);
        };
        let selected_index = held_slot_index(&original, hand)?;
        if source.index != selected_index || source.count != 1 {
            return Err(RejectReason::InvalidState);
        }
        let source_item =
            Item::from_u32(source.expected.item.item).ok_or(RejectReason::InvalidState)?;
        let mut candidate = original;
        match source_item {
            Item::WaterBucket => {
                if source.expected.item.count != 1
                    || !candidate.slot_matches(source)
                    || !candidate.replace_slot_exact(
                        source,
                        Some(SessionSlotWire::new(
                            crate::network::protocol::ItemWire {
                                item: Item::Bucket.to_u32(),
                                ..source.expected.item
                            },
                            source.expected.can_break,
                            source.expected.can_place_on,
                        )),
                    )
                {
                    return Err(RejectReason::InvalidState);
                }
            }
            Item::Bucket => {
                let mut filled_wire = source.expected.item;
                filled_wire.item = Item::WaterBucket.to_u32();
                filled_wire.count = 1;
                if !candidate.consume_slot_exact(source)
                    || !candidate.add_slot(contract::SessionInventorySlot::from_wire(
                        filled_wire,
                        source.expected.can_break,
                        source.expected.can_place_on,
                    ))
                {
                    return Err(RejectReason::InvalidState);
                }
            }
            _ => return Err(RejectReason::InvalidState),
        }
        if !preserves_brew_locks(&original, &candidate) {
            return Err(RejectReason::InvalidState);
        }

        let Some(dimension) = Dimension::from_wire(dimension) else {
            return Err(RejectReason::InvalidDimension);
        };
        let mutation = self.with_world(dimension, |world| {
            world.apply_fluid_use(position, face, source_item)
        })?;
        let Some(mutation) = mutation else {
            return Err(RejectReason::InvalidState);
        };
        let Some(session) = self.sessions.get_mut(&session_id) else {
            return Err(RejectReason::Unauthorized);
        };
        session.gameplay = candidate;
        Ok(Some(mutation))
    }

    pub(super) fn apply_fishing(
        &mut self,
        session_id: PlayerId,
        action: u8,
        hand: u8,
        look_milli: [i16; 3],
    ) -> Result<Option<WorldMutation>, RejectReason> {
        use crate::authority::fishing;
        use crate::inventory::GameMode;

        let Some(dimension) = self
            .sessions
            .get(&session_id)
            .and_then(|session| Dimension::from_wire(session.dimension))
        else {
            return Err(RejectReason::Unauthorized);
        };
        let Some((position, game_mode, original)) = self
            .sessions
            .get(&session_id)
            .map(|session| (session.position, session.game_mode, session.gameplay))
        else {
            return Err(RejectReason::Unauthorized);
        };
        let mut candidate = original;
        let previous_hook = candidate.fishing_hook;
        match action {
            0 => {
                let rod_slot = held_slot_index(&candidate, hand)?;
                if transactions::brew_locks_slot(&candidate, rod_slot) {
                    return Err(RejectReason::InvalidState);
                }
                let hook_id = self.next_unique_entity_id(dimension, Some(session_id));
                let context = fishing::FishingDomainContext {
                    world_seed: self.world(dimension).seed as u64
                        ^ (u64::from(self.world(dimension).dimension as u8) << 32),
                    hook_entity_id: hook_id,
                    player_position_milli: position_to_milli(position)?,
                    open_water: false,
                    water_surface_y_milli: None,
                    consume_durability: game_mode != GameMode::Creative,
                };
                fishing::cast(&mut candidate, session_id, hand, look_milli, context)?;
                self.claim_entity_id(hook_id);
            }
            1 => {
                let rod_slot = held_slot_index(&candidate, hand)?;
                if transactions::brew_locks_slot(&candidate, rod_slot) {
                    return Err(RejectReason::InvalidState);
                }
                let context = self
                    .world_mut_expect(dimension)
                    .fishing_context(&candidate, position, game_mode != GameMode::Creative)?;
                fishing::reel(&mut candidate, session_id, hand, context)?;
            }
            2 => {
                let context = self
                    .world_mut_expect(dimension)
                    .fishing_context(&candidate, position, game_mode != GameMode::Creative)?;
                fishing::cancel(&mut candidate, hand, context)?;
            }
            _ => return Err(RejectReason::InvalidState),
        }
        self.world_mut_expect(dimension).sync_authority_hook(
            previous_hook,
            candidate.fishing_hook,
            session_id,
        );
        let Some(session) = self.sessions.get_mut(&session_id) else {
            return Err(RejectReason::Unauthorized);
        };
        session.gameplay = candidate;
        Ok(None)
    }

    pub(super) fn apply_container_click(
        &mut self,
        session_id: PlayerId,
        position: (i32, i32, i32),
        slot: u16,
        is_left: bool,
        claimed: Option<&ItemWire>,
    ) -> Result<Option<WorldMutation>, RejectReason> {
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
        let is_viewer = self
            .world_mut_expect(dimension)
            .container_viewers
            .get(&position)
            .is_some_and(|viewers| viewers.contains(&session_id));
        if !is_viewer {
            return Err(RejectReason::PermissionDenied);
        }
        let slot_index = usize::from(slot);
        let Some(mut slots) = self.world_mut_expect(dimension).container_item_slots(position) else {
            return Err(RejectReason::InvalidState);
        };
        if slot_index >= slots.len() {
            return Err(RejectReason::InvalidState);
        }

        let mut candidate = original;
        if let Some(claimed) = claimed {
            if !slot_wire_matches(candidate.cursor, claimed) {
                if candidate.cursor.is_some() {
                    return Err(RejectReason::InvalidState);
                }
                let Some(source) = find_hotbar_source(&candidate, claimed) else {
                    return Err(RejectReason::InvalidState);
                };
                if transactions::brew_locks_slot(&candidate, source as u8) {
                    return Err(RejectReason::InvalidState);
                }
                candidate.cursor = candidate.inventory[source].take();
            }
        }

        let click_result = crate::inventory::apply_stack_click(
            slots[slot_index],
            candidate.cursor.and_then(|slot| slot.to_stack()),
            is_left,
        );
        let (next_slot, next_cursor) = (click_result.slot, click_result.dragged);
        let extract_into_inventory = original.cursor.is_none() && claimed.is_none();
        candidate.cursor = next_cursor
            .as_ref()
            .and_then(SessionInventorySlot::from_stack);
        if extract_into_inventory {
            if let Some(extracted) = candidate.cursor.take() {
                if !candidate.add_slot(extracted) {
                    return Err(RejectReason::InvalidState);
                }
            }
        }
        if !preserves_brew_locks(&original, &candidate) {
            return Err(RejectReason::InvalidState);
        }
        slots[slot_index] = next_slot;

        let Some(session) = self.sessions.get_mut(&session_id) else {
            return Err(RejectReason::Unauthorized);
        };
        session.gameplay = candidate;
        match self
            .world_mut_expect(dimension)
            .commit_container_item_slots(position, &slots)
        {
            Ok(mutation) => Ok(Some(mutation)),
            Err(reason) => {
                if let Some(session) = self.sessions.get_mut(&session_id) {
                    session.gameplay = original;
                }
                Err(reason)
            }
        }
    }
}
