use super::AuthorityCore;
use crate::authority::combat as combat_logic;
use crate::authority::contract::{
    self, position_to_milli, MiningProgressState, SessionContract, SessionGameplayState,
    SessionInventorySlot, WorldMutation,
};
use crate::authority::fishing;
use crate::authority::transactions;
use crate::dimension::Dimension;
use crate::network::protocol::{
    BlockActionKind, ContainerAction, GameplayOperation, GameplayOutcome, GameplayRequest,
    GameplayResponse, ItemWire, PlayerId, RejectReason, SessionSlotWire,
};
use crate::server_world::ServerWorld;

#[derive(Debug, Clone, Copy)]
struct PreflightContext {
    dimension: Dimension,
    position: [f32; 3],
    operator: bool,
}

/// Session-side half of the single authority preflight (sequence / revision /
/// spectator). Bounds are checked separately so malformed packets do not
/// consume the client sequence. World reach and operator command checks follow
/// via [`ServerWorld::validate_request`].
fn preflight_session(
    session: &SessionContract,
    request: &GameplayRequest,
    current_revision: u64,
) -> Result<PreflightContext, RejectReason> {
    let Some(dimension) = Dimension::from_wire(request.dimension) else {
        return Err(RejectReason::InvalidDimension);
    };
    if session.dimension != request.dimension {
        return Err(RejectReason::InvalidDimension);
    }
    session.validate_sequence(request)?;
    if request.client_revision > current_revision {
        return Err(RejectReason::InvalidRevision);
    }
    if request.client_revision < session.last_revision {
        return Err(RejectReason::InvalidRevision);
    }
    if session.game_mode == crate::inventory::GameMode::Spectator
        && !matches!(
            &request.operation,
            GameplayOperation::Command { .. } | GameplayOperation::Sleep { .. }
        )
    {
        return Err(RejectReason::PermissionDenied);
    }
    Ok(PreflightContext {
        dimension,
        position: session.position,
        operator: session.operator || session.cheats_enabled,
    })
}

/// Single authority-side gate: bounds, session checks, then world validate_request.
pub(crate) fn preflight(
    session: &SessionContract,
    request: &GameplayRequest,
    world: &ServerWorld,
    current_revision: u64,
) -> Result<(), RejectReason> {
    request.validate_bounds()?;
    let ctx = preflight_session(session, request, current_revision)?;
    world.validate_request(request, ctx.dimension, ctx.position, ctx.operator)
}


mod block_action;
mod combat;
mod command;
mod container;
mod workstation;

impl AuthorityCore {
    pub fn submit_request(&mut self, request: GameplayRequest) -> GameplayResponse {
        let request_id = request.request_id;
        let id = request.session_id;
        if self.sessions.get(&id).is_none() {
            return self.rejected(request_id, RejectReason::Unauthorized);
        }
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
        self.ensure_dimension(session_dimension);
        let current_revision = self.current_revision(session_dimension);
        if let Err(reason) = request.validate_bounds() {
            // Bounds failures stay non-consuming so console-only / malformed
            // packets cannot burn the accepted-sequence watermark.
            return self.reject_for_session(id, request_id, reason, None);
        }
        let ctx = {
            let Some(session) = self.sessions.get(&id) else {
                return self.rejected(request_id, RejectReason::Unauthorized);
            };
            match preflight_session(session, &request, current_revision) {
                Ok(ctx) => ctx,
                Err(reason) => {
                    return self.reject_for_session(
                        id,
                        request_id,
                        reason,
                        Some(request.client_sequence),
                    );
                }
            }
        };
        if let Err(reason) = self.world(session_dimension).validate_request(
            &request,
            ctx.dimension,
            ctx.position,
            ctx.operator,
        )
        {
            return self.reject_for_session(
                id,
                request_id,
                reason,
                Some(request.client_sequence),
            );
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
            } => match action {
                ContainerAction::Open => self
                    .world_mut_expect(session_dimension)
                    .open_container(*x, *y, *z, *slot, id),
                ContainerAction::Close => self
                    .world_mut_expect(session_dimension)
                    .close_container(*x, *y, *z, *slot, id),
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
                self.world_mut_expect(session_dimension).sleep_player(*x, *y, *z, id)
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
        let pending_world_mutations = self.world_mut_expect(session_dimension).take_pending_mutations();
        self.pending_mutations.extend(pending_world_mutations);
        let response = match result {
            Ok(mutation) => {
                if let Some(mutation) = mutation {
                    self.pending_mutations.push(mutation);
                }
                let revision = mutation
                    .map(|mutation| mutation.revision)
                    .unwrap_or_else(|| self.world_mut_expect(session_dimension).revisions.allocate());
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
    pub(super) fn apply_item_use(
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

    pub(super) fn apply_trade(
        &mut self,
        session_id: PlayerId,
        villager_id: u64,
        offer_index: u16,
    ) -> Result<Option<WorldMutation>, RejectReason> {
        let Some(dimension) = self
            .sessions
            .get(&session_id)
            .and_then(|session| Dimension::from_wire(session.dimension))
        else {
            return Err(RejectReason::Unauthorized);
        };
        let Some(position) = self.sessions.get(&session_id).map(|s| s.position) else {
            return Err(RejectReason::Unauthorized);
        };
        let Some(mut gameplay) = self.sessions.get(&session_id).map(|s| s.gameplay) else {
            return Err(RejectReason::Unauthorized);
        };
        let original = gameplay;
        self.world_mut_expect(dimension)
            .apply_trade(&mut gameplay, villager_id, offer_index, position)?;
        if !preserves_brew_locks(&original, &gameplay) {
            return Err(RejectReason::InvalidState);
        }
        if let Some(session) = self.sessions.get_mut(&session_id) {
            session.gameplay = gameplay;
        }
        Ok(None)
    }

    pub(super) fn apply_mount(
        &mut self,
        session_id: PlayerId,
        entity_id: u64,
    ) -> Result<Option<WorldMutation>, RejectReason> {
        let Some(dimension) = self
            .sessions
            .get(&session_id)
            .and_then(|session| Dimension::from_wire(session.dimension))
        else {
            return Err(RejectReason::Unauthorized);
        };
        let Some(position) = self.sessions.get(&session_id).map(|s| s.position) else {
            return Err(RejectReason::Unauthorized);
        };
        let mounted = self
            .world_mut_expect(dimension)
            .apply_mount(session_id, entity_id, position)?;
        if let Some(session) = self.sessions.get_mut(&session_id) {
            session.gameplay.mounted_entity = mounted;
        }
        Ok(None)
    }
}

pub(crate) fn stack_from_slot(
    slot: Option<SessionSlotWire>,
) -> Option<crate::inventory::ItemStack> {
    slot.and_then(|slot| SessionInventorySlot::from(slot).to_stack())
}

pub(super) fn slot_wire_matches(slot: Option<SessionInventorySlot>, claimed: &ItemWire) -> bool {
    slot.is_some_and(|slot| {
        slot.item == *claimed
            && slot.can_break == claimed.can_break
            && slot.can_place_on == claimed.can_place_on
    })
}

pub(super) fn find_hotbar_source(gameplay: &SessionGameplayState, claimed: &ItemWire) -> Option<usize> {
    let selected = usize::from(gameplay.selected_hotbar_slot.min(8));
    if slot_wire_matches(gameplay.inventory[selected], claimed) {
        return Some(selected);
    }
    (0..9).find(|&index| slot_wire_matches(gameplay.inventory[index], claimed))
}

pub(super) fn held_slot_index(gameplay: &SessionGameplayState, hand: u8) -> Result<u8, RejectReason> {
    match hand {
        0 if gameplay.selected_hotbar_slot < 9 => Ok(gameplay.selected_hotbar_slot),
        1 => Ok((contract::SESSION_INVENTORY_SLOTS - 1) as u8),
        _ => Err(RejectReason::InvalidState),
    }
}

pub(super) fn preserves_brew_locks(before: &SessionGameplayState, after: &SessionGameplayState) -> bool {
    (0..contract::SESSION_INVENTORY_SLOTS).all(|index| {
        !transactions::brew_locks_slot(before, index as u8)
            || before.inventory[index] == after.inventory[index]
    })
}



#[derive(Debug, Clone, Copy)]
struct CombatProfile {
    base_damage_milli: u32,
    used_axe: bool,
    knockback_milli: u32,
    fire_ticks: u16,
    looting_level: u8,
}

pub(super) fn combat_profile(gameplay: &SessionGameplayState) -> Result<CombatProfile, RejectReason> {
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

pub(super) fn look_from_angles(yaw: f32, pitch: f32) -> Result<[i16; 3], RejectReason> {
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

pub(super) fn quantize_health(health: f32) -> u32 {
    contract::quantize_health(health)
}
