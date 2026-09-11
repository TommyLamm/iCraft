use super::AuthorityCore;
use crate::authority::combat;
use crate::authority::contract::{
    self, position_to_milli, MiningProgressState, SessionGameplayState, SessionInventorySlot,
    WorldMutation,
};
use crate::authority::fishing;
use crate::authority::transactions;
use crate::dimension::Dimension;
use crate::network::protocol::{
    BlockActionKind, ContainerAction, GameplayOperation, GameplayOutcome, GameplayRequest,
    GameplayResponse, ItemWire, PlayerId, RejectReason, SessionSlotWire,
};

impl AuthorityCore {
    pub fn submit_request(&mut self, request: GameplayRequest) -> GameplayResponse {
        let request_id = request.request_id;
        let id = request.session_id;
        let Some(session_dimension_wire) = self.sessions.get(&id).map(|session| session.dimension)
        else {
            return self.rejected(request_id, RejectReason::Unauthorized);
        };
        if let Some(cached) = self
            .sessions
            .get(&id)
            .and_then(|session| session.cached_response(request_id))
        {
            return cached;
        }
        let Some(session_dimension) = Dimension::from_wire(request.dimension) else {
            return self.reject_for_session(id, request_id, RejectReason::InvalidDimension, None);
        };
        if let Err(reason) = request.validate_bounds() {
            return self.reject_for_session(id, request_id, reason, None);
        }
        if session_dimension_wire != request.dimension {
            return self.reject_for_session(id, request_id, RejectReason::InvalidDimension, None);
        }
        self.ensure_dimension(session_dimension);
        self.activate_dimension(session_dimension);
        let Some(session) = self.sessions.get(&id) else {
            return self.rejected(request_id, RejectReason::Unauthorized);
        };
        if let Err(reason) = session.validate_sequence(&request) {
            return self.reject_for_session(id, request_id, reason, None);
        }
        if request.client_revision > self.current_revision() {
            return self.reject_for_session(id, request_id, RejectReason::InvalidRevision, None);
        }
        if request.client_revision < session.last_revision {
            return self.reject_for_session(id, request_id, RejectReason::InvalidRevision, None);
        }
        if session.game_mode == crate::inventory::GameMode::Spectator
            && !matches!(
                &request.operation,
                crate::network::protocol::GameplayOperation::Command { .. }
                    | crate::network::protocol::GameplayOperation::Sleep { .. }
            )
        {
            return self.reject_for_session(id, request_id, RejectReason::PermissionDenied, None);
        }
        let session_position = session.position;
        let operator = session.operator || session.cheats_enabled;
        if let Err(reason) =
            self.world()
                .validate_request(&request, session_dimension, session_position, operator)
        {
            return self.reject_for_session(id, request_id, reason, None);
        }

        self.pending_session_revisions.clear();
        let result = match &request.operation {
            GameplayOperation::BlockAction {
                action,
                x,
                y,
                z,
                face,
                hand,
                held,
                block,
                look_milli,
            } => self.apply_block_action(
                id,
                *action,
                (*x, *y, *z),
                *face,
                *hand,
                *held,
                *block,
                *look_milli,
            ),
            GameplayOperation::Container {
                action,
                x,
                y,
                z,
                slot,
            } => match ContainerAction::from_wire(*action) {
                Some(ContainerAction::Open) => self
                    .world_mut_active()
                    .open_container(*x, *y, *z, *slot, id),
                Some(ContainerAction::Close) => self
                    .world_mut_active()
                    .close_container(*x, *y, *z, *slot, id),
                None => Err(RejectReason::InvalidState),
            },
            GameplayOperation::ContainerClick {
                x,
                y,
                z,
                slot,
                is_left,
                dragged,
            } => self.apply_container_click(id, (*x, *y, *z), *slot, *is_left, dragged.as_ref()),
            GameplayOperation::ItemUse { item, count } => self.apply_item_use(id, *item, *count),
            GameplayOperation::Combat { target, action } => {
                self.apply_authoritative_combat(&request, id, *target, *action)
            }
            GameplayOperation::Sleep { x, y, z } => {
                self.world_mut_active().sleep_player(*x, *y, *z, id)
            }
            GameplayOperation::Trade {
                villager_id,
                offer_index,
            } => self.apply_trade(id, *villager_id, *offer_index),
            GameplayOperation::Mount { entity_id } => self.apply_mount(id, *entity_id),
            GameplayOperation::Command { command } => self.apply_command(id, command),
            GameplayOperation::Fishing {
                action,
                hand,
                look_milli,
            } => self.apply_fishing(id, *action, *hand, *look_milli),
            GameplayOperation::FluidUse {
                x,
                y,
                z,
                face,
                hand,
                source,
            } => self.apply_fluid_use(id, (*x, *y, *z), *face, *hand, *source),
            GameplayOperation::FurnaceTakeOutput { .. }
            | GameplayOperation::Craft { .. }
            | GameplayOperation::Enchant { .. }
            | GameplayOperation::Brew { .. }
            | GameplayOperation::Anvil { .. }
            | GameplayOperation::UseState { .. } => {
                self.apply_transaction_operation(id, &request.operation)
            }
        };
        let pending_world_mutations = self.world_mut_active().take_pending_mutations();
        self.pending_mutations.extend(pending_world_mutations);
        let response = match result {
            Ok(mutation) => {
                if let Some(mutation) = mutation {
                    self.pending_mutations.push(mutation);
                }
                let revision = mutation
                    .map(|mutation| mutation.revision)
                    .unwrap_or_else(|| self.world_mut_active().revisions.allocate());
                GameplayResponse {
                    request_id,
                    server_sequence: revision,
                    outcome: GameplayOutcome::Accepted { revision },
                }
            }
            Err(reason) => {
                // A well-formed, authenticated request consumes its client
                // sequence even when the domain rejects it.  This prevents a
                // rejected operation from being replayed under a later ACK
                // and keeps the 128-entry cache idempotent.
                self.reject_for_session(id, request_id, reason, Some(request.client_sequence))
            }
        };
        if let Some(session) = self.sessions.get_mut(&id) {
            if matches!(response.outcome, GameplayOutcome::Accepted { .. }) {
                session.last_client_sequence = request.client_sequence;
                if let GameplayOutcome::Accepted { revision } = response.outcome {
                    session.last_revision = revision;
                    session.gameplay.revision = revision;
                }
                session.cache_response(response.clone());
            }
        }
        if matches!(response.outcome, GameplayOutcome::Accepted { .. }) {
            self.mark_session_update(id);
        }
        if let GameplayOutcome::Accepted { revision } = response.outcome {
            for changed_id in std::mem::take(&mut self.pending_session_revisions) {
                if changed_id == id {
                    continue;
                }
                if let Some(session) = self.sessions.get_mut(&changed_id) {
                    session.last_revision = revision;
                    session.gameplay.revision = revision;
                }
                self.mark_session_update(changed_id);
            }
        } else {
            self.pending_session_revisions.clear();
        }
        response
    }

    /// Dispatch player gameplay against the authenticated session and the
    /// headless world.  Renderer roots never perform these mutations after an
    /// authority boundary exists; an unsupported/invalid domain is rejected
    /// before it can fall back to local simulation.
    fn apply_item_use(
        &mut self,
        session_id: PlayerId,
        item: u32,
        count: u16,
    ) -> Result<Option<WorldMutation>, RejectReason> {
        use crate::inventory::GameMode;

        let Some(item_kind) = crate::inventory::Item::from_u32(item) else {
            return Err(RejectReason::InvalidState);
        };
        if count == 0 {
            return Err(RejectReason::InvalidState);
        }
        let Some(food) = item_kind.food_properties() else {
            return Err(RejectReason::Unsupported);
        };
        let Some(session) = self.sessions.get_mut(&session_id) else {
            return Err(RejectReason::Unauthorized);
        };
        let game_mode = session.game_mode;
        let original_gameplay = session.gameplay;
        if !session.gameplay.transact(|gameplay| {
            let hunger = gameplay.hunger_milli as f32 / 1000.0;
            if hunger >= 20.0 && !food.always_edible && game_mode != GameMode::Creative {
                return false;
            }
            gameplay.hunger_milli = ((hunger + food.hunger).min(20.0) * 1000.0).round() as u32;
            gameplay.saturation_milli = ((gameplay.saturation_milli as f32 / 1000.0
                + food.saturation)
                .min(gameplay.hunger_milli as f32 / 1000.0)
                * 1000.0)
                .round() as u32;
            if game_mode != GameMode::Creative && !gameplay.remove_item(item, u32::from(count)) {
                return false;
            }
            preserves_brew_locks(&original_gameplay, gameplay)
        }) {
            return Err(RejectReason::InvalidState);
        }
        Ok(None)
    }

    fn apply_trade(
        &mut self,
        session_id: PlayerId,
        villager_id: u64,
        offer_index: u16,
    ) -> Result<Option<WorldMutation>, RejectReason> {
        let Some(position) = self.sessions.get(&session_id).map(|s| s.position) else {
            return Err(RejectReason::Unauthorized);
        };
        let Some(mut gameplay) = self.sessions.get(&session_id).map(|s| s.gameplay) else {
            return Err(RejectReason::Unauthorized);
        };
        let original = gameplay;
        self.world_mut_active()
            .apply_trade(&mut gameplay, villager_id, offer_index, position)?;
        if !preserves_brew_locks(&original, &gameplay) {
            return Err(RejectReason::InvalidState);
        }
        if let Some(session) = self.sessions.get_mut(&session_id) {
            session.gameplay = gameplay;
        }
        Ok(None)
    }

    fn apply_mount(
        &mut self,
        session_id: PlayerId,
        entity_id: u64,
    ) -> Result<Option<WorldMutation>, RejectReason> {
        let Some(position) = self.sessions.get(&session_id).map(|s| s.position) else {
            return Err(RejectReason::Unauthorized);
        };
        let mounted = self
            .world_mut_active()
            .apply_mount(session_id, entity_id, position)?;
        if let Some(session) = self.sessions.get_mut(&session_id) {
            session.gameplay.mounted_entity = mounted;
        }
        Ok(None)
    }

    fn apply_block_action(
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
        let Some(session) = self.sessions.get(&session_id).cloned() else {
            return Err(RejectReason::Unauthorized);
        };
        let Some(dimension) = Dimension::from_wire(session.dimension) else {
            return Err(RejectReason::InvalidDimension);
        };
        if dimension != self.world().dimension {
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
                .world_mut_active()
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
            .world()
            .valid_coordinate(position.0, position.1, position.2)
        {
            return Err(RejectReason::InvalidCoordinate);
        }

        match action {
            BlockActionKind::StartBreak => {
                let Some(target_block) = self
                    .world_mut_active()
                    .chunks
                    .get_loaded_block(position.0, position.1, position.2)
                else {
                    return Err(RejectReason::InvalidState);
                };
                if target_block == crate::world::BlockType::Air
                    || !self.world_mut_active().has_block_line_of_sight(
                        session.position,
                        look_milli,
                        position,
                    )
                {
                    return Err(RejectReason::InvalidState);
                }
                let policy = crate::game_rules::GameModePolicy::for_rules(
                    session.game_mode,
                    &self.world().rules,
                );
                if !policy.can_break_stack(current_stack.as_ref(), target_block) {
                    return Err(RejectReason::PermissionDenied);
                }
                let target_state = self
                    .world()
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
                if !self.world().has_block_line_of_sight(
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
                    .world_mut_active()
                    .chunks
                    .get_loaded_block(support.0, support.1, support.2)
                else {
                    return Err(RejectReason::InvalidState);
                };
                let policy = crate::game_rules::GameModePolicy::for_rules(
                    session.game_mode,
                    &self.world().rules,
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
                    .world_mut_active()
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
                    || self.world().get_block(position.0, position.1, position.2)
                        != crate::world::BlockType::Air
                {
                    return Err(RejectReason::InvalidState);
                }
                let support = (
                    position.0.saturating_sub(i32::from(face[0])),
                    position.1.saturating_sub(i32::from(face[1])),
                    position.2.saturating_sub(i32::from(face[2])),
                );
                if self.world().get_block(support.0, support.1, support.2)
                    != crate::world::BlockType::Obsidian
                    || !self.world_mut_active().has_block_line_of_sight(
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
                let mutation = self.world_mut_active().set_block(
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
                    || self.world().get_block(position.0, position.1, position.2)
                        != crate::world::BlockType::EndPortalFrame
                    || !self.world_mut_active().has_block_line_of_sight(
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
                let mutation = self.world_mut_active().set_block(
                    position.0,
                    position.1,
                    position.2,
                    crate::world::BlockType::EndPortalFrameFilled,
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
            BlockActionKind::EnterPortal => {
                unreachable!("portal entry handled before slot validation")
            }
        }
    }

    fn apply_fluid_use(
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

    fn apply_fishing(
        &mut self,
        session_id: PlayerId,
        action: u8,
        hand: u8,
        look_milli: [i16; 3],
    ) -> Result<Option<WorldMutation>, RejectReason> {
        use crate::authority::fishing;
        use crate::inventory::GameMode;

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
                let hook_id = self.next_unique_entity_id();
                let context = fishing::FishingDomainContext {
                    world_seed: self.world().seed as u64
                        ^ (u64::from(self.world().dimension as u8) << 32),
                    hook_entity_id: hook_id,
                    player_position_milli: position_to_milli(position)?,
                    open_water: false,
                    water_surface_y_milli: None,
                    consume_durability: game_mode != GameMode::Creative,
                };
                fishing::cast(&mut candidate, session_id, hand, look_milli, context)
                    .map_err(map_fishing_error)?;
                self.claim_entity_id(hook_id);
            }
            1 => {
                let rod_slot = held_slot_index(&candidate, hand)?;
                if transactions::brew_locks_slot(&candidate, rod_slot) {
                    return Err(RejectReason::InvalidState);
                }
                let context = self
                    .world_mut_active()
                    .fishing_context(&candidate, position, game_mode != GameMode::Creative)
                    .map_err(map_fishing_error)?;
                fishing::reel(&mut candidate, session_id, hand, context)
                    .map_err(map_fishing_error)?;
            }
            2 => {
                let context = self
                    .world_mut_active()
                    .fishing_context(&candidate, position, game_mode != GameMode::Creative)
                    .map_err(map_fishing_error)?;
                fishing::cancel(&mut candidate, hand, context).map_err(map_fishing_error)?;
            }
            _ => return Err(RejectReason::InvalidState),
        }
        self.world_mut_active().sync_authority_hook(
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

    fn apply_container_click(
        &mut self,
        session_id: PlayerId,
        position: (i32, i32, i32),
        slot: u16,
        is_left: bool,
        claimed: Option<&ItemWire>,
    ) -> Result<Option<WorldMutation>, RejectReason> {
        let Some(original) = self
            .sessions
            .get(&session_id)
            .map(|session| session.gameplay)
        else {
            return Err(RejectReason::Unauthorized);
        };
        let is_viewer = self
            .world_mut_active()
            .container_viewers
            .get(&position)
            .is_some_and(|viewers| viewers.contains(&session_id));
        if !is_viewer {
            return Err(RejectReason::PermissionDenied);
        }
        let slot_index = usize::from(slot);
        let Some(mut slots) = self.world_mut_active().container_item_slots(position) else {
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
        candidate.cursor = next_cursor.as_ref().map(session_slot_from_item_stack);
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
            .world_mut_active()
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

    fn apply_transaction_operation(
        &mut self,
        session_id: PlayerId,
        operation: &GameplayOperation,
    ) -> Result<Option<WorldMutation>, RejectReason> {
        use crate::authority::transactions::{self, WorkstationContext};
        use crate::inventory::Item;

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
                mutation = Some(self.world_mut_active().take_furnace_output(
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
                        self.world()
                            .get_block(position[0], position[1], position[2]),
                    ),
                    _ => return Err(RejectReason::InvalidState),
                };
                transactions::execute_craft(
                    &mut candidate,
                    &self.world().recipe_manager,
                    context,
                    *grid,
                    *sources,
                )
                .map_err(map_transaction_error)?;
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
                    self.world().get_block(*x, *y, *z),
                    self.world().bookshelf_power(position),
                );
                transactions::execute_enchant(&mut candidate, context, *source, *option)
                    .map_err(map_transaction_error)?;
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
                let context = WorkstationContext::at(position, self.world().get_block(*x, *y, *z));
                match *action {
                    0 => {
                        let ingredient = ingredient.ok_or(RejectReason::InvalidState)?;
                        transactions::start_brew(&mut candidate, context, ingredient, *bottles)
                            .map_err(map_transaction_error)?;
                    }
                    1 if ingredient.is_none() && bottles.iter().all(Option::is_none) => {
                        transactions::cancel_brew(&mut candidate, context)
                            .map_err(map_transaction_error)?;
                    }
                    2 if ingredient.is_none() && bottles.iter().all(Option::is_none) => {
                        transactions::take_brew(&mut candidate, context)
                            .map_err(map_transaction_error)?;
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
                let context = WorkstationContext::at(position, self.world().get_block(*x, *y, *z));
                transactions::execute_anvil(&mut candidate, context, *left, *right, rename)
                    .map_err(map_transaction_error)?;
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

    fn apply_authoritative_combat(
        &mut self,
        request: &GameplayRequest,
        session_id: PlayerId,
        target: u64,
        action: u8,
    ) -> Result<Option<WorldMutation>, RejectReason> {
        use crate::authority::combat::{
            self, AuthorityDamageInput, CombatantId, DamageEvent, EntityCombatSnapshot,
            PlayerCombatSnapshot,
        };
        use crate::inventory::GameMode;
        use crate::player::DamageSource;

        if action != 0 || target == 0 || target == session_id {
            return Err(RejectReason::InvalidState);
        }
        let Some(attacker) = self.sessions.get(&session_id).cloned() else {
            return Err(RejectReason::Unauthorized);
        };
        if attacker.gameplay.is_dead {
            return Err(RejectReason::InvalidState);
        }
        let attacker_position_milli = position_to_milli(attacker.position)?;
        let attacker_look_milli = look_from_angles(attacker.yaw, attacker.pitch)?;
        let profile = combat_profile(&attacker.gameplay)?;
        let cooldown_ready =
            attacker.gameplay.attack_cooldown_ticks >= super::ATTACK_COOLDOWN_TICKS;

        if let Some(target_session) = self.sessions.get(&target).cloned() {
            if !self.world().rules.pvp
                || target_session.dimension != attacker.dimension
                || matches!(
                    target_session.game_mode,
                    GameMode::Creative | GameMode::Spectator
                )
            {
                return Err(RejectReason::PermissionDenied);
            }
            let target_position_milli = position_to_milli(target_session.position)?;
            let event = DamageEvent::from_authority(AuthorityDamageInput {
                event_id: request.request_id,
                attacker: CombatantId::Player(session_id),
                target: CombatantId::Player(target),
                source: DamageSource::Mob,
                base_damage_milli: profile.base_damage_milli,
                attacker_position_milli,
                target_position_milli,
                attacker_look_milli,
                target_look_milli: look_from_angles(target_session.yaw, target_session.pitch)?,
                cooldown_ready,
                has_line_of_sight: self
                    .world_mut_active()
                    .has_line_of_sight(attacker.position, target_session.position),
                attacker_used_axe: profile.used_axe,
                knockback_milli: profile.knockback_milli,
                fire_ticks: profile.fire_ticks,
                looting_level: profile.looting_level,
            })
            .map_err(map_combat_error)?;
            let mut target_snapshot = PlayerCombatSnapshot {
                player_id: target,
                gameplay: target_session.gameplay,
                velocity_milli: target_session.gameplay.velocity_milli,
                last_applied_event: None,
            };
            let outcome = combat::resolve_player_hit(&event, &mut target_snapshot)
                .map_err(map_combat_error)?;
            target_snapshot.gameplay.velocity_milli = target_snapshot.velocity_milli;
            let mut attacker_gameplay = attacker.gameplay;
            attacker_gameplay.attack_cooldown_ticks = 0;

            if outcome.death.is_some() && !self.world().rules.keep_inventory {
                target_snapshot.gameplay.inventory = [None; contract::SESSION_INVENTORY_SLOTS];
                target_snapshot.gameplay.experience = 0;
                target_snapshot.gameplay.experience_level = 0;
            }
            if outcome.death.is_some() {
                target_snapshot.gameplay.mounted_entity = None;
                self.world_mut_active().remove_passenger(target);
            }
            self.sessions
                .get_mut(&session_id)
                .ok_or(RejectReason::Unauthorized)?
                .gameplay = attacker_gameplay;
            self.sessions
                .get_mut(&target)
                .ok_or(RejectReason::InvalidState)?
                .gameplay = target_snapshot.gameplay;
            self.pending_session_revisions.insert(target);
            if !self.world().rules.keep_inventory {
                if let Some(death) = outcome.death {
                    self.spawn_death_outcome(target_session.position, death);
                }
            }
            return Ok(None);
        }

        let Some(entity) = self.world().entities.get_by_id(target) else {
            return Err(RejectReason::InvalidState);
        };
        let target_entity_type = entity.entity_type;
        let target_position = entity.position.to_array();
        let target_bounds = entity.get_aabb();
        let attacker_position = glam::Vec3::from_array(attacker.position);
        let target_hit_position = attacker_position.clamp(target_bounds.min, target_bounds.max);
        let mut target_snapshot = EntityCombatSnapshot {
            entity_id: entity.id,
            entity_type: entity.entity_type,
            health_milli: quantize_health(entity.health),
            max_health_milli: quantize_health(entity.max_health),
            velocity_milli: position_to_milli(entity.velocity.to_array())?,
            armor_points_milli: 0,
            toughness_milli: 0,
            enchantment_protection_factor: 0,
            knockback_resistance_milli: 0,
            invulnerability_ticks: (entity.invulnerable_time.max(0.0) * 20.0)
                .round()
                .min(u16::MAX as f32) as u16,
            fire_ticks_remaining: (entity.fire_aspect_timer.max(0.0) * 20.0)
                .round()
                .min(u16::MAX as f32) as u16,
            has_wool: entity.has_wool,
            death_source: None,
            death_settled: entity.player_kill_rewarded,
            last_applied_event: None,
        };
        let event = DamageEvent::from_authority(AuthorityDamageInput {
            event_id: request.request_id,
            attacker: CombatantId::Player(session_id),
            target: CombatantId::Entity(target),
            source: DamageSource::Mob,
            base_damage_milli: profile.base_damage_milli,
            attacker_position_milli,
            target_position_milli: position_to_milli(target_hit_position.to_array())?,
            attacker_look_milli,
            target_look_milli: look_from_angles(entity.yaw, entity.pitch)?,
            cooldown_ready,
            has_line_of_sight: self
                .world_mut_active()
                .has_line_of_sight(attacker.position, target_hit_position.to_array()),
            attacker_used_axe: profile.used_axe,
            knockback_milli: profile.knockback_milli,
            fire_ticks: profile.fire_ticks,
            looting_level: profile.looting_level,
        })
        .map_err(map_combat_error)?;
        let outcome =
            combat::resolve_entity_hit(&event, &mut target_snapshot).map_err(map_combat_error)?;

        let mut attacker_gameplay = attacker.gameplay;
        attacker_gameplay.attack_cooldown_ticks = 0;
        self.sessions
            .get_mut(&session_id)
            .ok_or(RejectReason::Unauthorized)?
            .gameplay = attacker_gameplay;
        if target_snapshot.health_milli == 0 {
            let _ = self.world_mut_active().entities.remove_by_id(target);
            if target_entity_type == crate::entity::EntityType::EnderDragon {
                self.world_mut_active().handle_dragon_completion();
            }
        } else if let Some(entity) = self.world_mut_active().entities.get_by_id_mut(target) {
            entity.health = target_snapshot.health_milli as f32 / 1_000.0;
            entity.velocity = glam::Vec3::new(
                target_snapshot.velocity_milli[0] as f32 / 1_000.0,
                target_snapshot.velocity_milli[1] as f32 / 1_000.0,
                target_snapshot.velocity_milli[2] as f32 / 1_000.0,
            );
            entity.invulnerable_time = f32::from(target_snapshot.invulnerability_ticks) / 20.0;
            entity.fire_aspect_timer = f32::from(target_snapshot.fire_ticks_remaining) / 20.0;
            entity.player_kill_rewarded = target_snapshot.death_settled;
        }
        if let Some(death) = outcome.death {
            self.spawn_death_outcome(target_position, death);
        }
        Ok(None)
    }

    fn spawn_death_outcome(&mut self, position: [f32; 3], death: combat::DeathOutcome) {
        for slot in death.drops {
            let id = self.next_unique_entity_id();
            self.claim_entity_id(id);
            let _ = self
                .world_mut_active()
                .spawn_authority_drop(id, slot, position);
        }
        if death.experience > 0 {
            let id = self.next_unique_entity_id();
            self.claim_entity_id(id);
            let _ =
                self.world_mut_active()
                    .spawn_authority_experience(id, death.experience, position);
        }
    }

    /// Commands that mutate authenticated session or world state. `State` only
    /// projects the resulting snapshot and never edits pose or game mode as
    /// authority. `/respawn` stays a string match so it is not folded into the
    /// operator-only leftover Command list; TCP `ClientRespawnRequest` is a
    /// separate non-op entry.
    fn apply_command(
        &mut self,
        session_id: PlayerId,
        command: &str,
    ) -> Result<Option<WorldMutation>, RejectReason> {
        if command.trim().eq_ignore_ascii_case("/respawn") {
            return if self.respawn_session(session_id) {
                Ok(None)
            } else {
                Err(RejectReason::Unauthorized)
            };
        }
        let parsed = crate::commands::parse(command).map_err(|_| RejectReason::InvalidState)?;
        if matches!(
            parsed.surface(),
            crate::commands::CommandSurface::ConsoleOnly
        ) {
            return Err(RejectReason::Unsupported);
        }
        match parsed {
            crate::commands::Command::GameMode { mode, target } => {
                if target.is_some_and(|target| {
                    !matches!(
                        target,
                        crate::commands::CommandTarget::SelfPlayer
                            | crate::commands::CommandTarget::NearestPlayer
                    )
                }) {
                    return Err(RejectReason::PermissionDenied);
                }
                let Some(session) = self.sessions.get_mut(&session_id) else {
                    return Err(RejectReason::Unauthorized);
                };
                session.game_mode = mode;
                Ok(None)
            }
            crate::commands::Command::Teleport { target, position } => {
                if !matches!(
                    target,
                    crate::commands::CommandTarget::SelfPlayer
                        | crate::commands::CommandTarget::NearestPlayer
                ) {
                    return Err(RejectReason::PermissionDenied);
                }
                if !self.world().dimension.height().contains_y(position[1]) {
                    return Err(RejectReason::InvalidCoordinate);
                }
                let Some(session) = self.sessions.get_mut(&session_id) else {
                    return Err(RejectReason::Unauthorized);
                };
                session.position = [
                    position[0] as f32 + 0.5,
                    position[1] as f32,
                    position[2] as f32 + 0.5,
                ];
                Ok(None)
            }
            crate::commands::Command::Give {
                target,
                item,
                count,
            } => {
                if !matches!(
                    target,
                    crate::commands::CommandTarget::SelfPlayer
                        | crate::commands::CommandTarget::NearestPlayer
                ) {
                    return Err(RejectReason::PermissionDenied);
                }
                let Some(session) = self.sessions.get_mut(&session_id) else {
                    return Err(RejectReason::Unauthorized);
                };
                let stack = crate::inventory::ItemStack::new(item, count);
                let slot = SessionInventorySlot::from_wire(
                    crate::network::protocol::ItemWire::from_stack(&stack),
                    stack.can_break,
                    stack.can_place_on,
                );
                if !session.gameplay.add_slot(slot) {
                    return Err(RejectReason::InvalidState);
                }
                Ok(None)
            }
            crate::commands::Command::GameRule { rule, value } => {
                self.world_mut_active()
                    .set_gamerule(&rule, value.as_deref())?;
                Ok(None)
            }
            crate::commands::Command::Time(crate::commands::TimeCommand::Set(time)) => {
                self.world_mut_active().set_time(time);
                Ok(None)
            }
            crate::commands::Command::Time(crate::commands::TimeCommand::Add(time)) => {
                self.world_mut_active().add_time(time);
                Ok(None)
            }
            // ConsoleOnly arms are rejected above; keep a defensive catch-all.
            _ => Err(RejectReason::Unsupported),
        }
    }

    fn rejected(&mut self, request_id: u128, reason: RejectReason) -> GameplayResponse {
        GameplayResponse {
            request_id,
            server_sequence: self.world().revisions.current(),
            outcome: GameplayOutcome::Rejected { reason },
        }
    }

    fn reject_for_session(
        &mut self,
        session_id: PlayerId,
        request_id: u128,
        reason: RejectReason,
        consumed_sequence: Option<u64>,
    ) -> GameplayResponse {
        let response = self.rejected(request_id, reason);
        if let Some(session) = self.sessions.get_mut(&session_id) {
            if let Some(sequence) = consumed_sequence {
                session.last_client_sequence = sequence;
            }
            session.cache_response(response.clone());
        }
        response
    }
}

pub(crate) fn stack_from_slot(
    slot: Option<SessionSlotWire>,
) -> Option<crate::inventory::ItemStack> {
    slot.and_then(|slot| SessionInventorySlot::from(slot).to_stack())
}

fn session_slot_from_item_stack(stack: &crate::inventory::ItemStack) -> SessionInventorySlot {
    SessionInventorySlot::from_wire(
        ItemWire::from_stack(stack),
        stack.can_break,
        stack.can_place_on,
    )
}

fn slot_wire_matches(slot: Option<SessionInventorySlot>, claimed: &ItemWire) -> bool {
    slot.is_some_and(|slot| {
        slot.item == *claimed
            && slot.can_break == claimed.can_break
            && slot.can_place_on == claimed.can_place_on
    })
}

fn find_hotbar_source(gameplay: &SessionGameplayState, claimed: &ItemWire) -> Option<usize> {
    let selected = usize::from(gameplay.selected_hotbar_slot.min(8));
    if slot_wire_matches(gameplay.inventory[selected], claimed) {
        return Some(selected);
    }
    (0..9).find(|&index| slot_wire_matches(gameplay.inventory[index], claimed))
}

fn held_slot_index(gameplay: &SessionGameplayState, hand: u8) -> Result<u8, RejectReason> {
    match hand {
        0 if gameplay.selected_hotbar_slot < 9 => Ok(gameplay.selected_hotbar_slot),
        1 => Ok((contract::SESSION_INVENTORY_SLOTS - 1) as u8),
        _ => Err(RejectReason::InvalidState),
    }
}

fn preserves_brew_locks(before: &SessionGameplayState, after: &SessionGameplayState) -> bool {
    (0..contract::SESSION_INVENTORY_SLOTS).all(|index| {
        !transactions::brew_locks_slot(before, index as u8)
            || before.inventory[index] == after.inventory[index]
    })
}

fn map_fishing_error(error: fishing::FishingDomainError) -> RejectReason {
    match error {
        fishing::FishingDomainError::HookTooFar => RejectReason::TooFar,
        fishing::FishingDomainError::InvalidContext
        | fishing::FishingDomainError::InvalidHand
        | fishing::FishingDomainError::InvalidSelectedSlot
        | fishing::FishingDomainError::MissingRod
        | fishing::FishingDomainError::InvalidRod
        | fishing::FishingDomainError::HookAlreadyActive
        | fishing::FishingDomainError::NoActiveHook
        | fishing::FishingDomainError::StaleHook
        | fishing::FishingDomainError::CorruptHook
        | fishing::FishingDomainError::InventoryFull
        | fishing::FishingDomainError::ExperienceOverflow => RejectReason::InvalidState,
    }
}

fn map_transaction_error(_error: transactions::TransactionError) -> RejectReason {
    RejectReason::InvalidState
}

#[derive(Debug, Clone, Copy)]
struct CombatProfile {
    base_damage_milli: u32,
    used_axe: bool,
    knockback_milli: u32,
    fire_ticks: u16,
    looting_level: u8,
}

fn combat_profile(gameplay: &SessionGameplayState) -> Result<CombatProfile, RejectReason> {
    use crate::enchantment::{attack_damage_bonus, Enchantment};
    use crate::inventory::ToolType;

    let selected = usize::from(gameplay.selected_hotbar_slot);
    if selected >= 9 {
        return Err(RejectReason::InvalidState);
    }
    let stack = gameplay.inventory[selected]
        .map(|slot| slot.item.to_stack().ok_or(RejectReason::InvalidState))
        .transpose()?;
    let tool = stack
        .as_ref()
        .and_then(|stack| stack.item.tool_properties());
    let enchantments = stack
        .as_ref()
        .map(|stack| stack.enchantments)
        .unwrap_or_default();
    let base = tool.map(|tool| tool.damage).unwrap_or(1.0) + attack_damage_bonus(&enchantments);
    Ok(CombatProfile {
        base_damage_milli: (base.max(0.001) * 1_000.0).round().clamp(1.0, 100_000.0) as u32,
        used_axe: tool.is_some_and(|tool| tool.tool_type == ToolType::Axe),
        knockback_milli: 400 + u32::from(enchantments.level_of(Enchantment::Knockback(1))) * 500,
        fire_ticks: u16::from(enchantments.level_of(Enchantment::FireAspect(1))) * 80,
        looting_level: enchantments.level_of(Enchantment::Looting(1)).min(3),
    })
}

fn look_from_angles(yaw: f32, pitch: f32) -> Result<[i16; 3], RejectReason> {
    if !yaw.is_finite() || !pitch.is_finite() || pitch.abs() > 90.0 {
        return Err(RejectReason::InvalidState);
    }
    let yaw = yaw.to_radians();
    let pitch = pitch.to_radians();
    let horizontal = pitch.cos();
    let look = [
        (-yaw.sin() * horizontal * 1_000.0).round() as i16,
        (-pitch.sin() * 1_000.0).round() as i16,
        (yaw.cos() * horizontal * 1_000.0).round() as i16,
    ];
    Ok(look)
}

fn quantize_health(health: f32) -> u32 {
    if health.is_finite() {
        (health.max(0.0) * 1_000.0).round().min(u32::MAX as f32) as u32
    } else {
        u32::MAX
    }
}

fn map_combat_error(error: combat::CombatReject) -> RejectReason {
    match error {
        combat::CombatReject::OutOfRange => RejectReason::TooFar,
        combat::CombatReject::ReplayedEvent => RejectReason::Duplicate,
        combat::CombatReject::InvalidEvent
        | combat::CombatReject::IdentityMismatch
        | combat::CombatReject::Cooldown
        | combat::CombatReject::NoLineOfSight
        | combat::CombatReject::NotFacingTarget
        | combat::CombatReject::TargetDead
        | combat::CombatReject::TargetInvulnerable
        | combat::CombatReject::InvalidTargetState => RejectReason::InvalidState,
    }
}
