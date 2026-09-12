// Tests extracted from mod.rs (Plan 27).

use super::*;
use crate::authority::contract::{
    SessionBrewState, SessionFishingHookState, SessionGameplayState, SessionInventorySlot,
};
use crate::entity::EntityType;
use crate::inventory::Item;
use crate::network::protocol::{
    BlockActionKind, GameplayOperation, GameplayOutcome, SlotRefWire,
};
use crate::world::BlockType;
use contract::SessionContract;

fn core() -> AuthorityCore {
    let mut core = AuthorityCore::new(AuthorityConfig::default());
    core.register_session(SessionContract::new(
        7,
        "alex",
        0,
        [8.0, 80.0, 8.0],
        true,
        true,
    ))
    .unwrap();
    core
}

fn block_request(
    request_id: u128,
    client_sequence: u64,
    action: BlockActionKind,
    target: (i32, i32, i32),
    held: Option<crate::network::protocol::SessionSlotWire>,
    block: BlockType,
    face: [i8; 3],
    look_milli: [i16; 3],
    client_revision: u64,
) -> GameplayRequest {
    GameplayRequest {
        request_id,
        client_sequence,
        session_id: 7,
        dimension: 0,
        client_revision,
        operation: GameplayOperation::BlockAction {
            action,
            x: target.0,
            y: target.1,
            z: target.2,
            face,
            hand: 0,
            held,
            block: if matches!(action, BlockActionKind::Place) {
                block.to_wire()
            } else {
                BlockType::Air.to_wire()
            },
            look_milli,
        },
    }
}

#[test]
fn duplicate_and_stale_revision_are_authoritative() {
    let mut core = core();
    let mut request = GameplayRequest {
        request_id: 1,
        client_sequence: 1,
        session_id: 7,
        dimension: 0,
        client_revision: 0,
        operation: GameplayOperation::ItemUse {
            item: Item::Bread as u32,
            count: 1,
        },
    };
    let first = core.submit_request(request.clone());
    let duplicate = core.submit_request(request.clone());
    assert_eq!(first, duplicate);
    request.request_id = 2;
    request.client_sequence = 2;
    request.client_revision = core.current_revision(Dimension::Overworld) + 1;
    let revision_before_reject = core.current_revision(Dimension::Overworld);
    let rejected = core.submit_request(request);
    assert!(matches!(
        rejected.outcome,
        GameplayOutcome::Rejected {
            reason: RejectReason::InvalidRevision
        }
    ));
    assert_eq!(core.current_revision(Dimension::Overworld), revision_before_reject);
    assert_eq!(rejected.server_sequence, revision_before_reject);
}

#[test]
fn authenticated_rejections_are_cached_without_consuming_sequence() {
    let mut core = core();
    let request = GameplayRequest {
        request_id: 9,
        client_sequence: 1,
        session_id: 7,
        dimension: 0,
        client_revision: 0,
        operation: GameplayOperation::Command {
            command: "/gamerule doDaylightCycle ".to_string() + &"x".repeat(3000),
        },
    };
    let first = core.submit_request(request.clone());
    let duplicate = core.submit_request(request);
    assert_eq!(first, duplicate);
    assert_eq!(core.session(7).unwrap().last_client_sequence, 0);
    assert!(core.session(7).unwrap().cached_response(9).is_some());
}

#[test]
fn rejected_block_action_does_not_drain_a_mutation() {
    let mut core = core();
    let before = core.world(Dimension::Overworld).get_block(8, 80, 8);
    let request = GameplayRequest {
        request_id: 21,
        client_sequence: 1,
        session_id: 7,
        dimension: 0,
        client_revision: 0,
        operation: GameplayOperation::BlockAction {
            action: BlockActionKind::Place,
            x: 8,
            y: 80,
            z: 8,
            face: [0, 1, 0],
            hand: 0,
            held: None,
            block: 3,
            look_milli: [0, 0, 1000],
        },
    };
    let response = core.submit_request(request);
    assert!(matches!(
        response.outcome,
        GameplayOutcome::Rejected {
            reason: RejectReason::InvalidState
        }
    ));
    assert!(core.take_pending_mutations().is_empty());
    assert_eq!(core.world(Dimension::Overworld).get_block(8, 80, 8), before);
}

#[test]
fn typed_mining_fixed_tick_breaks_once_and_cancel_is_idempotent() {
    let mut core = core();
    let target = (8, 81, 9);
    core.world_mut(Dimension::Overworld).unwrap()
        .set_block(target.0, target.1, target.2, BlockType::Stone, 0)
        .unwrap();

    let held_stack = crate::inventory::ItemStack::new(Item::StonePickaxe, 1);
    let held = crate::network::protocol::SessionSlotWire::new(
        crate::network::protocol::ItemWire::from_stack(&held_stack),
        0,
        0,
    );
    let mut gameplay = SessionGameplayState::default();
    gameplay.inventory[0] = Some(SessionInventorySlot::from(held));
    core.set_session_gameplay(7, gameplay);

    let start = GameplayRequest {
        request_id: 100,
        client_sequence: 1,
        session_id: 7,
        dimension: 0,
        client_revision: 0,
        operation: GameplayOperation::BlockAction {
            action: BlockActionKind::StartBreak,
            x: target.0,
            y: target.1,
            z: target.2,
            face: [0, 0, -1],
            hand: 0,
            held: Some(held),
            block: BlockType::Air.to_wire(),
            look_milli: [0, -100, 995],
        },
    };
    assert!(matches!(
        core.submit_request(start).outcome,
        GameplayOutcome::Accepted { .. }
    ));
    assert!(core.session(7).unwrap().gameplay.mining.is_some());

    let mut target_mutations = 0;
    for _ in 0..80 {
        let snapshot = core.tick();
        target_mutations += snapshot
            .mutations
            .iter()
            .filter(|mutation| mutation.position == target)
            .count();
    }
    assert_eq!(
        core.world(Dimension::Overworld).get_block(target.0, target.1, target.2),
        BlockType::Air
    );
    assert_eq!(target_mutations, 1);
    assert!(core.session(7).unwrap().gameplay.mining.is_none());
    assert_eq!(
        core.world_mut(Dimension::Overworld).unwrap()
            .entities
            .entities
            .iter()
            .filter(|entity| entity.entity_type == EntityType::DroppedItem)
            .count(),
        1
    );

    let cancel = GameplayRequest {
        request_id: 101,
        client_sequence: 2,
        session_id: 7,
        dimension: 0,
        client_revision: core.current_revision(Dimension::Overworld),
        operation: GameplayOperation::BlockAction {
            action: BlockActionKind::CancelBreak,
            x: target.0,
            y: target.1,
            z: target.2,
            face: [0, 0, 0],
            hand: 0,
            held: None,
            block: BlockType::Air.to_wire(),
            look_milli: [0, -100, 995],
        },
    };
    let first_cancel = core.submit_request(cancel.clone());
    assert!(matches!(
        first_cancel.outcome,
        GameplayOutcome::Accepted { .. }
    ));
    assert_eq!(core.submit_request(cancel), first_cancel);
    assert_eq!(
        core.world_mut(Dimension::Overworld).unwrap()
            .entities
            .entities
            .iter()
            .filter(|entity| entity.entity_type == EntityType::DroppedItem)
            .count(),
        1
    );
}

#[test]
fn typed_mining_rejects_unloaded_target_without_progress_or_mutation() {
    let mut core = core();
    core.session_mut(7).unwrap().position = [15.0, 80.0, 8.0];
    let held_stack = crate::inventory::ItemStack::new(Item::StonePickaxe, 1);
    let held = crate::network::protocol::SessionSlotWire::new(
        crate::network::protocol::ItemWire::from_stack(&held_stack),
        0,
        0,
    );
    let mut gameplay = SessionGameplayState::default();
    gameplay.inventory[0] = Some(SessionInventorySlot::from(held));
    core.set_session_gameplay(7, gameplay);
    let target = (16, 81, 8);
    let response = core.submit_request(GameplayRequest {
        request_id: 102,
        client_sequence: 1,
        session_id: 7,
        dimension: 0,
        client_revision: 0,
        operation: GameplayOperation::BlockAction {
            action: BlockActionKind::StartBreak,
            x: target.0,
            y: target.1,
            z: target.2,
            face: [0, 0, -1],
            hand: 0,
            held: Some(held),
            block: BlockType::Air.to_wire(),
            look_milli: [995, -100, 0],
        },
    });
    assert!(
        matches!(
            response.outcome,
            GameplayOutcome::Rejected {
                reason: RejectReason::InvalidState
            }
        ),
        "unexpected unloaded-target response: {:?}",
        response.outcome
    );
    assert!(core.session(7).unwrap().gameplay.mining.is_none());
    assert!(core.take_pending_mutations().is_empty());
}

#[test]
fn typed_mining_game_modes_and_empty_hand_are_authoritative() {
    let target = (8, 81, 9);
    let pick = crate::inventory::ItemStack::new(Item::StonePickaxe, 1);
    let pick_wire = crate::network::protocol::SessionSlotWire::new(
        crate::network::protocol::ItemWire::from_stack(&pick),
        0,
        0,
    );

    let mut creative = core();
    creative.session_mut(7).unwrap().game_mode = crate::inventory::GameMode::Creative;
    creative
        .world_mut(Dimension::Overworld).unwrap()
        .set_block(target.0, target.1, target.2, BlockType::Stone, 0)
        .unwrap();
    let mut creative_gameplay = SessionGameplayState::default();
    creative_gameplay.inventory[0] = Some(SessionInventorySlot::from(pick_wire));
    creative.set_session_gameplay(7, creative_gameplay);
    assert!(matches!(
        creative
            .submit_request(block_request(
                110,
                1,
                BlockActionKind::StartBreak,
                target,
                Some(pick_wire),
                BlockType::Air,
                [0, 0, -1],
                [0, -100, 995],
                0,
            ))
            .outcome,
        GameplayOutcome::Accepted { .. }
    ));
    assert_eq!(
        creative.world(Dimension::Overworld).get_block(target.0, target.1, target.2),
        BlockType::Air
    );
    assert_eq!(
        creative.session(7).unwrap().gameplay.inventory[0],
        Some(SessionInventorySlot::from(pick_wire))
    );
    assert!(creative
        .world(Dimension::Overworld)
        .entities
        .entities
        .iter()
        .all(|entity| entity.entity_type != EntityType::ExperienceOrb));

    let mut adventure = core();
    adventure.session_mut(7).unwrap().game_mode = crate::inventory::GameMode::Adventure;
    adventure
        .world_mut(Dimension::Overworld).unwrap()
        .set_block(target.0, target.1, target.2, BlockType::Stone, 0)
        .unwrap();
    let mut allowed = SessionGameplayState::default();
    let tagged_pick = crate::inventory::ItemStack::new(Item::StonePickaxe, 1)
        .with_can_break(BlockType::Stone);
    let tagged_wire = crate::network::protocol::SessionSlotWire::new(
        crate::network::protocol::ItemWire::from_stack(&tagged_pick),
        tagged_pick.can_break,
        tagged_pick.can_place_on,
    );
    allowed.inventory[0] = Some(SessionInventorySlot::from(tagged_wire));
    adventure.set_session_gameplay(7, allowed);
    assert!(matches!(
        adventure
            .submit_request(block_request(
                111,
                1,
                BlockActionKind::StartBreak,
                target,
                Some(tagged_wire),
                BlockType::Air,
                [0, 0, -1],
                [0, -100, 995],
                0,
            ))
            .outcome,
        GameplayOutcome::Accepted { .. }
    ));
    for _ in 0..80 {
        let _ = adventure.tick();
    }
    assert_eq!(
        adventure.world(Dimension::Overworld).get_block(target.0, target.1, target.2),
        BlockType::Air
    );

    let mut denied = core();
    denied.session_mut(7).unwrap().game_mode = crate::inventory::GameMode::Adventure;
    denied
        .world_mut(Dimension::Overworld).unwrap()
        .set_block(target.0, target.1, target.2, BlockType::Stone, 0)
        .unwrap();
    let mut untagged = SessionGameplayState::default();
    untagged.inventory[0] = Some(SessionInventorySlot::from(pick_wire));
    denied.set_session_gameplay(7, untagged);
    assert!(matches!(
        denied
            .submit_request(block_request(
                112,
                1,
                BlockActionKind::StartBreak,
                target,
                Some(pick_wire),
                BlockType::Air,
                [0, 0, -1],
                [0, -100, 995],
                0,
            ))
            .outcome,
        GameplayOutcome::Rejected {
            reason: RejectReason::PermissionDenied
        }
    ));
    assert_eq!(
        denied.world(Dimension::Overworld).get_block(target.0, target.1, target.2),
        BlockType::Stone
    );

    let mut empty_hand = core();
    empty_hand
        .world_mut(Dimension::Overworld).unwrap()
        .set_block(target.0, target.1, target.2, BlockType::Dirt, 0)
        .unwrap();
    assert!(matches!(
        empty_hand
            .submit_request(block_request(
                113,
                1,
                BlockActionKind::StartBreak,
                target,
                None,
                BlockType::Air,
                [0, 0, -1],
                [0, -100, 995],
                0,
            ))
            .outcome,
        GameplayOutcome::Accepted { .. }
    ));
}

#[test]
fn typed_place_maps_item_debits_once_and_rejects_cheat_block() {
    let support = (8, 80, 9);
    let target = (8, 81, 9);
    let stone_stack = crate::inventory::ItemStack::new(Item::Stone, 2);
    let stone_wire = crate::network::protocol::SessionSlotWire::new(
        crate::network::protocol::ItemWire::from_stack(&stone_stack),
        0,
        0,
    );
    let mut core = core();
    core.world_mut(Dimension::Overworld).unwrap()
        .set_block(support.0, support.1, support.2, BlockType::Stone, 0)
        .unwrap();
    core.world_mut(Dimension::Overworld).unwrap()
        .set_block(target.0, target.1, target.2, BlockType::Air, 0)
        .unwrap();
    let mut gameplay = SessionGameplayState::default();
    gameplay.inventory[0] = Some(SessionInventorySlot::from(stone_wire));
    core.set_session_gameplay(7, gameplay);
    let place_request = block_request(
        114,
        1,
        BlockActionKind::Place,
        target,
        Some(stone_wire),
        BlockType::Stone,
        [0, 1, 0],
        [250, -550, 750],
        0,
    );
    let accepted = core.submit_request(place_request);
    assert!(
        matches!(accepted.outcome, GameplayOutcome::Accepted { .. }),
        "unexpected place response: {:?}",
        accepted.outcome
    );
    assert_eq!(
        core.world(Dimension::Overworld).get_block(target.0, target.1, target.2),
        BlockType::Stone
    );
    assert_eq!(
        core.session(7).unwrap().gameplay.inventory[0]
            .unwrap()
            .item
            .count,
        1
    );

    let stale = core.submit_request(block_request(
        115,
        2,
        BlockActionKind::Place,
        (8, 81, 10),
        Some(stone_wire),
        BlockType::Chest,
        [0, 1, 0],
        [0, -100, 995],
        core.current_revision(Dimension::Overworld),
    ));
    assert!(matches!(
        stale.outcome,
        GameplayOutcome::Rejected {
            reason: RejectReason::InvalidState
        }
    ));
    assert_eq!(
        core.session(7).unwrap().gameplay.inventory[0]
            .unwrap()
            .item
            .count,
        1
    );
    assert_eq!(core.world(Dimension::Overworld).get_block(8, 81, 10), BlockType::Air);

    // The same typed path is valid across an explicitly loaded chunk
    // boundary; unloaded front/support chunks are never synthesized by
    // the action itself.
    core.world_mut(Dimension::Overworld).unwrap().ensure_chunk(1, 0);
    core.session_mut(7).unwrap().position = [15.0, 80.0, 8.0];
    let support_cross = (16, 80, 8);
    let target_cross = (16, 81, 8);
    core.world_mut(Dimension::Overworld).unwrap()
        .set_block(
            support_cross.0,
            support_cross.1,
            support_cross.2,
            BlockType::Stone,
            0,
        )
        .unwrap();
    core.world_mut(Dimension::Overworld).unwrap()
        .set_block(
            target_cross.0,
            target_cross.1,
            target_cross.2,
            BlockType::Air,
            0,
        )
        .unwrap();
    let stone_one = crate::inventory::ItemStack::new(Item::Stone, 1);
    let stone_one_wire = crate::network::protocol::SessionSlotWire::new(
        crate::network::protocol::ItemWire::from_stack(&stone_one),
        0,
        0,
    );
    let mut cross_gameplay = core.session(7).unwrap().gameplay;
    cross_gameplay.inventory[0] = Some(SessionInventorySlot::from(stone_one_wire));
    core.set_session_gameplay(7, cross_gameplay);
    let cross = core.submit_request(block_request(
        116,
        3,
        BlockActionKind::Place,
        target_cross,
        Some(stone_one_wire),
        BlockType::Stone,
        [0, 1, 0],
        [750, -550, 250],
        core.current_revision(Dimension::Overworld),
    ));
    assert!(matches!(cross.outcome, GameplayOutcome::Accepted { .. }));
    assert_eq!(
        core.world(Dimension::Overworld)
            .get_block(target_cross.0, target_cross.1, target_cross.2),
        BlockType::Stone
    );
    assert!(core.session(7).unwrap().gameplay.inventory[0].is_none());
}

#[test]
fn typed_mining_cancels_on_cancel_held_change_range_or_block_replacement() {
    let target = (8, 81, 9);
    let pick = crate::inventory::ItemStack::new(Item::StonePickaxe, 1);
    let pick_wire = crate::network::protocol::SessionSlotWire::new(
        crate::network::protocol::ItemWire::from_stack(&pick),
        0,
        0,
    );
    let start = |core: &mut AuthorityCore, request_id: u128| {
        core.world_mut(Dimension::Overworld).unwrap()
            .set_block(target.0, target.1, target.2, BlockType::Stone, 0)
            .unwrap();
        let mut gameplay = SessionGameplayState::default();
        gameplay.inventory[0] = Some(SessionInventorySlot::from(pick_wire));
        core.set_session_gameplay(7, gameplay);
        let response = core.submit_request(block_request(
            request_id,
            1,
            BlockActionKind::StartBreak,
            target,
            Some(pick_wire),
            BlockType::Air,
            [0, 0, -1],
            [0, -100, 995],
            0,
        ));
        assert!(matches!(response.outcome, GameplayOutcome::Accepted { .. }));
    };

    let mut cancelled = core();
    start(&mut cancelled, 120);
    let response = cancelled.submit_request(block_request(
        121,
        2,
        BlockActionKind::CancelBreak,
        target,
        None,
        BlockType::Air,
        [0, 0, 0],
        [0, -100, 995],
        cancelled.current_revision(Dimension::Overworld),
    ));
    assert!(matches!(response.outcome, GameplayOutcome::Accepted { .. }));
    let _ = cancelled.tick();
    assert_eq!(
        cancelled.world(Dimension::Overworld).get_block(target.0, target.1, target.2),
        BlockType::Stone
    );

    let mut held_changed = core();
    start(&mut held_changed, 122);
    let mut changed = held_changed.session(7).unwrap().gameplay;
    let other = crate::inventory::ItemStack::new(Item::WoodenPickaxe, 1);
    changed.inventory[0] = Some(SessionInventorySlot::from(
        crate::network::protocol::SessionSlotWire::new(
            crate::network::protocol::ItemWire::from_stack(&other),
            0,
            0,
        ),
    ));
    held_changed.set_session_gameplay(7, changed);
    let _ = held_changed.tick();
    assert!(held_changed.session(7).unwrap().gameplay.mining.is_none());
    assert_eq!(
        held_changed.world(Dimension::Overworld).get_block(target.0, target.1, target.2),
        BlockType::Stone
    );

    let mut slot_switched = core();
    start(&mut slot_switched, 125);
    let mut switched = slot_switched.session(7).unwrap().gameplay;
    switched.selected_hotbar_slot = 1;
    switched.inventory[0] = Some(SessionInventorySlot::from(pick_wire));
    switched.inventory[1] = Some(SessionInventorySlot::from(pick_wire));
    slot_switched.set_session_gameplay(7, switched);
    let _ = slot_switched.tick();
    assert!(slot_switched.session(7).unwrap().gameplay.mining.is_none());
    assert_eq!(
        slot_switched
            .world(Dimension::Overworld)
            .get_block(target.0, target.1, target.2),
        BlockType::Stone
    );

    let mut moved = core();
    start(&mut moved, 123);
    moved.session_mut(7).unwrap().position = [30.0, 80.0, 30.0];
    let _ = moved.tick();
    assert!(moved.session(7).unwrap().gameplay.mining.is_none());
    assert_eq!(
        moved.world(Dimension::Overworld).get_block(target.0, target.1, target.2),
        BlockType::Stone
    );

    let mut replaced = core();
    start(&mut replaced, 124);
    replaced
        .world_mut(Dimension::Overworld).unwrap()
        .set_block(target.0, target.1, target.2, BlockType::Dirt, 0)
        .unwrap();
    let _ = replaced.tick();
    assert!(replaced.session(7).unwrap().gameplay.mining.is_none());
    assert_eq!(
        replaced.world(Dimension::Overworld).get_block(target.0, target.1, target.2),
        BlockType::Dirt
    );
}

#[test]
fn typed_adventure_place_requires_can_place_on_and_block_entity_projection() {
    let support = (8, 80, 9);
    let target = (8, 81, 9);
    let chest =
        crate::inventory::ItemStack::new(Item::Chest, 1).with_can_place_on(BlockType::Stone);
    let chest_wire = crate::network::protocol::SessionSlotWire::new(
        crate::network::protocol::ItemWire::from_stack(&chest),
        chest.can_break,
        chest.can_place_on,
    );
    let mut core = core();
    core.session_mut(7).unwrap().game_mode = crate::inventory::GameMode::Adventure;
    core.world_mut(Dimension::Overworld).unwrap()
        .set_block(support.0, support.1, support.2, BlockType::Stone, 0)
        .unwrap();
    core.world_mut(Dimension::Overworld).unwrap()
        .set_block(target.0, target.1, target.2, BlockType::Air, 0)
        .unwrap();
    let mut gameplay = SessionGameplayState::default();
    gameplay.inventory[0] = Some(SessionInventorySlot::from(chest_wire));
    core.set_session_gameplay(7, gameplay);
    let placed = core.submit_request(block_request(
        130,
        1,
        BlockActionKind::Place,
        target,
        Some(chest_wire),
        BlockType::Chest,
        [0, 1, 0],
        [250, -550, 750],
        0,
    ));
    assert!(matches!(placed.outcome, GameplayOutcome::Accepted { .. }));
    assert!(core
        .world(Dimension::Overworld)
        .get_block_entity(target.0, target.1, target.2)
        .is_some());
    assert!(core.session(7).unwrap().gameplay.inventory[0].is_none());

    let mut break_gameplay = SessionGameplayState::default();
    let break_chest =
        crate::inventory::ItemStack::new(Item::Chest, 1).with_can_break(BlockType::Chest);
    let break_wire = crate::network::protocol::SessionSlotWire::new(
        crate::network::protocol::ItemWire::from_stack(&break_chest),
        break_chest.can_break,
        break_chest.can_place_on,
    );
    break_gameplay.inventory[0] = Some(SessionInventorySlot::from(break_wire));
    core.set_session_gameplay(7, break_gameplay);
    let broken = core.submit_request(block_request(
        131,
        2,
        BlockActionKind::StartBreak,
        target,
        Some(break_wire),
        BlockType::Air,
        [0, 0, -1],
        [0, -100, 995],
        core.current_revision(Dimension::Overworld),
    ));
    assert!(matches!(broken.outcome, GameplayOutcome::Accepted { .. }));
    for _ in 0..300 {
        let _ = core.tick();
    }
    assert_eq!(
        core.world(Dimension::Overworld).get_block(target.0, target.1, target.2),
        BlockType::Air
    );
    assert!(core
        .world(Dimension::Overworld)
        .get_block_entity(target.0, target.1, target.2)
        .is_none());
}

#[test]
fn reconnect_resets_owner_private_mining_progress() {
    let target = (8, 81, 9);
    let pick = crate::inventory::ItemStack::new(Item::StonePickaxe, 1);
    let wire = crate::network::protocol::SessionSlotWire::new(
        crate::network::protocol::ItemWire::from_stack(&pick),
        0,
        0,
    );
    let mut core = core();
    core.world_mut(Dimension::Overworld).unwrap()
        .set_block(target.0, target.1, target.2, BlockType::Stone, 0)
        .unwrap();
    let mut gameplay = SessionGameplayState::default();
    gameplay.inventory[0] = Some(SessionInventorySlot::from(wire));
    core.set_session_gameplay(7, gameplay);
    assert!(matches!(
        core.submit_request(block_request(
            140,
            1,
            BlockActionKind::StartBreak,
            target,
            Some(wire),
            BlockType::Air,
            [0, 0, -1],
            [0, -100, 995],
            0,
        ))
        .outcome,
        GameplayOutcome::Accepted { .. }
    ));
    assert!(core.session(7).unwrap().gameplay.mining.is_some());
    let _ = core.remove_session(7);
    core.register_session(SessionContract::new(
        7,
        "alex",
        0,
        [8.0, 80.0, 8.0],
        true,
        true,
    ))
    .unwrap();
    assert!(core.session(7).unwrap().gameplay.mining.is_none());
}

#[test]
fn typed_mining_grants_xp_once_and_removes_broken_tool_without_orb() {
    let target = (8, 81, 9);
    let mut pick = crate::inventory::ItemStack::new(Item::DiamondPickaxe, 1);
    pick.durability = 1;
    let wire = crate::network::protocol::SessionSlotWire::new(
        crate::network::protocol::ItemWire::from_stack(&pick),
        0x55,
        0xaa,
    );
    let mut core = core();
    core.world_mut(Dimension::Overworld).unwrap()
        .set_block(target.0, target.1, target.2, BlockType::DiamondOre, 0)
        .unwrap();
    let mut gameplay = SessionGameplayState::default();
    gameplay.experience = 6;
    gameplay.inventory[0] = Some(SessionInventorySlot::from(wire));
    core.set_session_gameplay(7, gameplay);
    assert!(matches!(
        core.submit_request(block_request(
            150,
            1,
            BlockActionKind::StartBreak,
            target,
            Some(wire),
            BlockType::Air,
            [0, 0, -1],
            [0, -100, 995],
            0,
        ))
        .outcome,
        GameplayOutcome::Accepted { .. }
    ));
    for _ in 0..80 {
        let _ = core.tick();
    }
    assert_eq!(
        core.world(Dimension::Overworld).get_block(target.0, target.1, target.2),
        BlockType::Air
    );
    assert_eq!(core.session(7).unwrap().gameplay.experience, 4);
    assert_eq!(core.session(7).unwrap().gameplay.experience_level, 1);
    assert!(core.session(7).unwrap().gameplay.inventory[0].is_none());
    assert!(core
        .world(Dimension::Overworld)
        .entities
        .entities
        .iter()
        .all(|entity| entity.entity_type != EntityType::ExperienceOrb));
    let dropped = core
        .world(Dimension::Overworld)
        .entities
        .entities
        .iter()
        .find(|entity| entity.entity_type == EntityType::DroppedItem)
        .expect("diamond ore must produce one authoritative drop");
    assert_eq!(dropped.dropped_item, Some(Item::Diamond));
    assert_eq!(dropped.dropped_count, 1);
}

#[test]
fn authoritative_dispenser_edge_executes_once_with_global_entity_id() {
    let mut core = core();
    let lever = (7, 80, 8);
    let source = (8, 80, 8);
    let mut lever_on = crate::world::BlockState::default();
    lever_on.is_open = true;
    let lever_on = lever_on.encode();
    core.world_mut(Dimension::Overworld).unwrap()
        .set_block(lever.0, lever.1, lever.2, BlockType::Lever, lever_on)
        .unwrap();
    core.world_mut(Dimension::Overworld).unwrap()
        .set_block(source.0, source.1, source.2, BlockType::Dispenser, 0)
        .unwrap();
    {
        let world = core.world_mut(Dimension::Overworld).unwrap();
        world
            .redstone
            .on_block_changed(&world.chunks, lever, crate::redstone::Direction::East);
    }
    {
        let world = core.world_mut(Dimension::Overworld).unwrap();
        world.redstone.on_block_changed(
            &world.chunks,
            source,
            crate::redstone::Direction::South,
        );
    }
    if let Some(entity) = core
        .world_mut(Dimension::Overworld).unwrap()
        .chunks
        .get_block_entity_mut(source.0, source.1, source.2)
    {
        entity.set_stack(0, Some(crate::inventory::ItemStack::new(Item::Arrow, 2)));
    }

    let first = core.tick();
    assert_eq!(
        core.world_mut(Dimension::Overworld).unwrap()
            .entities
            .entities
            .iter()
            .filter(|entity| entity.entity_type == EntityType::Arrow)
            .count(),
        1
    );
    let arrow_id = core
        .world(Dimension::Overworld)
        .entities
        .entities
        .iter()
        .find(|entity| entity.entity_type == EntityType::Arrow)
        .unwrap()
        .id;
    assert!(arrow_id >= AUTHORITY_ENTITY_ID_START);
    assert!(first
        .mutations
        .iter()
        .any(|mutation| mutation.position == source));

    let sustained = core.tick();
    assert!(sustained
        .mutations
        .iter()
        .all(|mutation| mutation.position != source));
    assert_eq!(
        core.world_mut(Dimension::Overworld).unwrap()
            .entities
            .entities
            .iter()
            .filter(|entity| entity.entity_type == EntityType::Arrow)
            .count(),
        1
    );

    core.world_mut(Dimension::Overworld).unwrap()
        .set_block(lever.0, lever.1, lever.2, BlockType::Lever, 0)
        .unwrap();
    {
        let world = core.world_mut(Dimension::Overworld).unwrap();
        world
            .redstone
            .on_block_changed(&world.chunks, lever, crate::redstone::Direction::East);
    }
    let _ = core.tick();
    core.world_mut(Dimension::Overworld).unwrap()
        .set_block(lever.0, lever.1, lever.2, BlockType::Lever, lever_on)
        .unwrap();
    {
        let world = core.world_mut(Dimension::Overworld).unwrap();
        world
            .redstone
            .on_block_changed(&world.chunks, lever, crate::redstone::Direction::East);
    }
    let _ = core.tick();
    assert_eq!(
        core.world_mut(Dimension::Overworld).unwrap()
            .entities
            .entities
            .iter()
            .filter(|entity| entity.entity_type == EntityType::Arrow)
            .count(),
        2
    );
}

#[test]
fn item_use_mutates_session_inventory_and_revision() {
    let mut core = core();
    let mut gameplay = SessionGameplayState::default();
    gameplay.hunger_milli = 10_000;
    let mut wire = crate::network::protocol::ItemWire::empty();
    wire.item = Item::Bread as u32;
    wire.count = 2;
    gameplay.inventory[0] = Some(SessionInventorySlot::from_wire(wire, 0, 0));
    core.set_session_gameplay(7, gameplay);
    let response = core.submit_request(GameplayRequest {
        request_id: 30,
        client_sequence: 1,
        session_id: 7,
        dimension: 0,
        client_revision: 0,
        operation: GameplayOperation::ItemUse {
            item: Item::Bread as u32,
            count: 1,
        },
    });
    assert!(matches!(response.outcome, GameplayOutcome::Accepted { .. }));
    let state = core.session(7).unwrap().gameplay;
    assert_eq!(state.count_item(Item::Bread as u32), 1);
    assert!(state.hunger_milli > 10_000);
    assert!(state.revision > 0);
}

#[test]
fn unsupported_tool_item_use_does_not_consume_inventory() {
    let mut core = core();
    let mut gameplay = SessionGameplayState::default();
    let mut wire = crate::network::protocol::ItemWire::empty();
    wire.item = Item::DiamondSword as u32;
    wire.count = 1;
    gameplay.inventory[0] = Some(SessionInventorySlot::from_wire(wire, 0, 0));
    core.set_session_gameplay(7, gameplay);
    let response = core.submit_request(GameplayRequest {
        request_id: 34,
        client_sequence: 1,
        session_id: 7,
        dimension: 0,
        client_revision: 0,
        operation: GameplayOperation::ItemUse {
            item: Item::DiamondSword as u32,
            count: 1,
        },
    });
    assert!(matches!(
        response.outcome,
        GameplayOutcome::Rejected {
            reason: RejectReason::InvalidState
        }
    ));
    assert_eq!(
        core.session(7)
            .unwrap()
            .gameplay
            .count_item(Item::DiamondSword as u32),
        1
    );
    assert_eq!(
        core.session(7).unwrap().last_client_sequence,
        0,
        "non-food ItemUse must fail in validate_bounds before sequencing"
    );
}

#[test]
fn console_only_command_rejects_before_sequencing() {
    let mut core = core();
    let response = core.submit_request(GameplayRequest {
        request_id: 41,
        client_sequence: 1,
        session_id: 7,
        dimension: 0,
        client_revision: 0,
        operation: GameplayOperation::Command {
            command: "/help".to_string(),
        },
    });
    assert!(matches!(
        response.outcome,
        GameplayOutcome::Rejected {
            reason: RejectReason::Unsupported
        }
    ));
    assert_eq!(core.session(7).unwrap().last_client_sequence, 0);
}

#[test]
fn client_cannot_submit_self_damage() {
    let mut core = core();
    let before = core.session(7).unwrap().gameplay;
    let response = core.submit_request(GameplayRequest {
        request_id: 35,
        client_sequence: 1,
        session_id: 7,
        dimension: 0,
        client_revision: 0,
        operation: GameplayOperation::Combat {
            target: 0,
            action: 0x80 | 127,
        },
    });
    assert!(matches!(
        response.outcome,
        GameplayOutcome::Rejected {
            reason: RejectReason::InvalidState
        }
    ));
    assert_eq!(core.session(7).unwrap().gameplay, before);
}

#[test]
fn respawn_command_restores_authority_health_after_death() {
    let mut core = core();
    let mut dead = core.session(7).unwrap().gameplay;
    dead.health_milli = 0;
    dead.is_dead = true;
    dead.death_source = Some(crate::player::DamageSource::Mob.to_wire());
    assert!(core.set_session_gameplay(7, dead));
    assert!(core.session(7).unwrap().gameplay.is_dead);
    let response = core.submit_request(GameplayRequest {
        request_id: 40,
        client_sequence: 1,
        session_id: 7,
        dimension: 0,
        client_revision: core.current_revision(Dimension::Overworld),
        operation: GameplayOperation::Command {
            command: "/respawn".to_string(),
        },
    });
    assert!(matches!(response.outcome, GameplayOutcome::Accepted { .. }));
    let state = core.session(7).unwrap().gameplay;
    assert!(!state.is_dead);
    assert_eq!(state.health_milli, state.max_health_milli);
}

#[test]
fn respawn_session_rejects_living_player() {
    let mut core = core();
    let before = core.session(7).unwrap().clone();
    assert!(!before.gameplay.is_dead);
    assert!(!core.respawn_session(7));
    let after = core.session(7).unwrap();
    assert_eq!(after.position, before.position);
    assert_eq!(after.dimension, before.dimension);
    assert_eq!(after.gameplay, before.gameplay);
}

#[test]
fn dimension_transfer_updates_session_and_world_contract() {
    let mut core = AuthorityCore::new(AuthorityConfig::default());
    let _ = core.register_session(SessionContract::new(
        7,
        "alex",
        0,
        [8.0, 80.0, 8.0],
        true,
        true,
    ));
    assert!(core.set_session_dimension(7, crate::dimension::Dimension::Nether));
    assert_eq!(core.session(7).unwrap().dimension, 1);
    let nether = core
        .world_ref(crate::dimension::Dimension::Nether)
        .expect("nether world stays in the map");
    assert_eq!(nether.dimension, crate::dimension::Dimension::Nether);
    assert_eq!(nether.chunks.dimension, crate::dimension::Dimension::Nether);
    assert_eq!(
        core.world(crate::dimension::Dimension::Nether).dimension,
        crate::dimension::Dimension::Nether
    );
}

#[test]
fn session_dimension_index_tracks_register_move_and_remove() {
    let mut core = AuthorityCore::new(AuthorityConfig::default());
    core.register_session(SessionContract::new(
        7,
        "alex",
        Dimension::Overworld as u8,
        [8.0, 80.0, 8.0],
        true,
        true,
    ))
    .unwrap();
    core.register_session(SessionContract::new(
        8,
        "sam",
        Dimension::Nether as u8,
        [8.0, 80.0, 8.0],
        true,
        true,
    ))
    .unwrap();
    assert_eq!(
        core.session_ids_in_dimension(Dimension::Overworld),
        &[7]
    );
    assert_eq!(core.session_ids_in_dimension(Dimension::Nether), &[8]);

    assert!(core.set_session_dimension(7, Dimension::Nether));
    assert!(core.session_ids_in_dimension(Dimension::Overworld).is_empty());
    assert_eq!(core.session_ids_in_dimension(Dimension::Nether), &[7, 8]);

    assert!(core.set_session_dimension(7, Dimension::Nether));
    assert_eq!(core.session_ids_in_dimension(Dimension::Nether), &[7, 8]);

    assert!(core.remove_session(8).is_some());
    assert_eq!(core.session_ids_in_dimension(Dimension::Nether), &[7]);
    assert!(core.session_ids_in_dimension(Dimension::End).is_empty());
}

#[test]
fn session_updates_publish_join_and_dimension_change_but_not_idle_ticks() {
    let mut core = AuthorityCore::new(AuthorityConfig::default());
    core.register_session(SessionContract::new(
        7,
        "alex",
        Dimension::Overworld as u8,
        [8.0, 80.0, 8.0],
        true,
        true,
    ))
    .unwrap();

    let joined = core.tick();
    assert!(joined.session_updates.iter().any(|update| {
        update.player_id == 7 && update.dimension == Dimension::Overworld as u8
    }));
    assert_eq!(joined.session_updates.len(), core.last_snapshot.session_updates.len());

    let idle = core.tick();
    assert!(idle.session_updates.is_empty());
    assert!(core.last_snapshot.session_updates.is_empty());

    assert!(core.set_session_dimension(7, Dimension::Nether));
    let transferred = core.tick();
    assert!(transferred.session_updates.iter().any(|update| {
        update.player_id == 7 && update.dimension == Dimension::Nether as u8
    }));
    let idle_after_transfer = core.tick();
    assert!(idle_after_transfer.session_updates.is_empty());
}

#[test]
fn mining_brew_and_fishing_revision_bumps_publish_dirty_session_updates() {
    let mut core = core();
    let _ = core.tick();
    assert!(core.tick().session_updates.is_empty());

    let target = (8, 81, 9);
    core.world_mut(Dimension::Overworld).unwrap()
        .set_block(target.0, target.1, target.2, BlockType::Stone, 0)
        .unwrap();
    let held_stack = crate::inventory::ItemStack::new(Item::StonePickaxe, 1);
    let held = crate::network::protocol::SessionSlotWire::new(
        crate::network::protocol::ItemWire::from_stack(&held_stack),
        0,
        0,
    );
    let mut mining_gameplay = SessionGameplayState::default();
    mining_gameplay.inventory[0] = Some(SessionInventorySlot::from(held));
    core.set_session_gameplay(7, mining_gameplay);
    assert!(matches!(
        core.submit_request(GameplayRequest {
            request_id: 200,
            client_sequence: 1,
            session_id: 7,
            dimension: 0,
            client_revision: 0,
            operation: GameplayOperation::BlockAction {
                action: BlockActionKind::StartBreak,
                x: target.0,
                y: target.1,
                z: target.2,
                face: [0, 0, -1],
                hand: 0,
                held: Some(held),
                block: BlockType::Air.to_wire(),
                look_milli: [0, -100, 995],
            },
        })
        .outcome,
        GameplayOutcome::Accepted { .. }
    ));
    let mining_tick = core.tick();
    let mining_update = mining_tick
        .session_updates
        .iter()
        .find(|update| update.player_id == 7)
        .expect("mining revision bump must publish");
    assert!(mining_update.state.mining.is_some());
    assert!(mining_update.state.revision > 0);

    core.world_mut(Dimension::Overworld).unwrap()
        .set_block(8, 80, 8, BlockType::BrewingStand, 0)
        .unwrap();
    let mut brew_gameplay = core.session(7).unwrap().gameplay;
    brew_gameplay.mining = None;
    brew_gameplay.brew = Some(SessionBrewState {
        station: [8, 80, 8],
        ingredient: SlotRefWire {
            index: 0,
            count: 1,
            expected: held,
        },
        bottles: [None; 3],
        remaining_ticks: 4,
    });
    assert!(core.set_session_gameplay(7, brew_gameplay));
    let brew_tick = core.tick();
    let brew_update = brew_tick
        .session_updates
        .iter()
        .find(|update| update.player_id == 7)
        .expect("brew revision bump must publish");
    assert_eq!(
        brew_update.state.brew.map(|brew| brew.remaining_ticks),
        Some(3)
    );

    let _ = core.tick();
    let mut fishing_gameplay = core.session(7).unwrap().gameplay;
    fishing_gameplay.brew = None;
    fishing_gameplay.fishing_hook = Some(SessionFishingHookState {
        entity_id: super::AUTHORITY_ENTITY_ID_START,
        position_milli: [8_000, 80_000, 8_000],
        velocity_milli: [0, 0, 0],
        stage: 1,
        wait_ticks_remaining: 8,
        bite_ticks_remaining: 0,
    });
    assert!(core.set_session_gameplay(7, fishing_gameplay));
    let fishing_tick = core.tick();
    assert!(
        fishing_tick
            .session_updates
            .iter()
            .any(|update| update.player_id == 7),
        "fishing tick must publish a dirty session update"
    );
}

#[test]
fn dimension_worlds_are_parked_without_chunk_aliasing() {
    let mut core = AuthorityCore::new(AuthorityConfig::default());
    let _ = core.register_session(SessionContract::new(
        7,
        "alex",
        0,
        [8.0, 80.0, 8.0],
        true,
        true,
    ));
    let marker = BlockType::Glass;
    core.world_mut(crate::dimension::Dimension::Overworld)
        .expect("overworld world")
        .set_block(1_234, 100, -2_345, marker, 0)
        .unwrap();
    assert_eq!(
        core.world_ref(crate::dimension::Dimension::Overworld)
            .expect("overworld world")
            .get_block(1_234, 100, -2_345),
        marker
    );

    assert!(core.set_session_dimension(7, crate::dimension::Dimension::Nether));
    assert_ne!(
        core.world(crate::dimension::Dimension::Nether)
            .get_block(1_234, 100, -2_345),
        marker
    );
    assert!(core
        .world(crate::dimension::Dimension::Nether)
        .valid_coordinate(1_234, 127, -2_345));
    assert!(!core
        .world(crate::dimension::Dimension::Nether)
        .valid_coordinate(1_234, 128, -2_345));
    assert_eq!(
        core.world_ref(crate::dimension::Dimension::Overworld)
            .expect("overworld remains in the map")
            .get_block(1_234, 100, -2_345),
        marker
    );
    if let Some(session) = core.session_mut(7) {
        session.position = [154.25, 67.0, -293.5];
    }
    assert_eq!(core.session(7).unwrap().position, [154.25, 67.0, -293.5]);

    assert!(core.set_session_dimension(7, crate::dimension::Dimension::Overworld));
    assert_eq!(core.session(7).unwrap().dimension, 0);
    assert_eq!(
        core.world_ref(crate::dimension::Dimension::Overworld)
            .expect("overworld world")
            .get_block(1_234, 100, -2_345),
        marker
    );
    assert_eq!(core.session(7).unwrap().dimension, 0);
}

#[test]
fn sessions_in_multiple_dimensions_tick_and_dispatch_independently() {
    let mut core = AuthorityCore::new(AuthorityConfig::default());
    core.register_session(SessionContract::new(
        7,
        "alex",
        Dimension::Overworld as u8,
        [8.0, 80.0, 8.0],
        true,
        true,
    ))
    .unwrap();
    core.register_session(SessionContract::new(
        8,
        "sam",
        Dimension::Nether as u8,
        [8.0, 80.0, 8.0],
        true,
        true,
    ))
    .unwrap();

    core.with_world(Dimension::Overworld, |world| {
        world
            .set_block(8, 80, 8, BlockType::Glass, 0)
            .expect("seed overworld glass");
    });
    core.with_world(Dimension::Nether, |world| {
        world
            .set_block(8, 80, 8, BlockType::Obsidian, 0)
            .expect("seed nether obsidian");
    });
    let snapshot = core.tick();
    assert_eq!(snapshot.tick, 1);
    assert_eq!(core.world_mut(Dimension::Overworld).unwrap().dimension, Dimension::Overworld);
    assert!(snapshot
        .session_updates
        .iter()
        .any(|update| update.player_id == 7 && update.dimension == Dimension::Overworld as u8));
    assert!(snapshot
        .session_updates
        .iter()
        .any(|update| update.player_id == 8 && update.dimension == Dimension::Nether as u8));

    assert_eq!(core.world(Dimension::Overworld).time, 1);
    assert_eq!(core.world(Dimension::Overworld).get_block(8, 80, 8), BlockType::Glass);
    let overworld_revision = core.revision_for_dimension(Dimension::Overworld);
    assert_eq!(core.world(Dimension::Nether).time, 1);
    assert_eq!(core.world(Dimension::Nether).get_block(8, 80, 8), BlockType::Obsidian);
    let nether_revision = core.revision_for_dimension(Dimension::Nether);
    assert_eq!(overworld_revision, nether_revision);

    // Empty-hand Place DiamondOre is the shared rejected_place fixture shape
    // (integration copies live in tests/common/rejected_place.rs).
    let rejected = core.submit_request(GameplayRequest {
        request_id: 101,
        client_sequence: 1,
        session_id: 7,
        dimension: Dimension::Overworld as u8,
        client_revision: overworld_revision,
        operation: GameplayOperation::BlockAction {
            action: BlockActionKind::Place,
            x: 8,
            y: 80,
            z: 8,
            face: [0, 1, 0],
            hand: 0,
            held: None,
            block: BlockType::DiamondOre.to_wire(),
            look_milli: [0, 0, 1000],
        },
    });
    assert!(matches!(
        rejected.outcome,
        GameplayOutcome::Rejected {
            reason: RejectReason::InvalidState
        }
    ));
    assert_eq!(core.world(Dimension::Overworld).get_block(8, 80, 8), BlockType::Glass);

    // Rejected Overworld BlockAction must not mutate; routing selects the
    // session world by explicit dimension, not an ambient active pointer.
    let routed_again = core.submit_request(GameplayRequest {
        request_id: 103,
        client_sequence: 2,
        session_id: 7,
        dimension: Dimension::Overworld as u8,
        client_revision: overworld_revision,
        operation: GameplayOperation::BlockAction {
            action: BlockActionKind::Place,
            x: 9,
            y: 80,
            z: 8,
            face: [0, 1, 0],
            hand: 0,
            held: None,
            block: BlockType::Glass.to_wire(),
            look_milli: [0, 0, 1000],
        },
    });
    assert!(matches!(
        routed_again.outcome,
        GameplayOutcome::Rejected {
            reason: RejectReason::InvalidState
        }
    ));
    assert_eq!(core.world(Dimension::Overworld).get_block(8, 80, 8), BlockType::Glass);
    assert_ne!(core.world(Dimension::Overworld).get_block(9, 80, 8), BlockType::Glass);
}

#[test]
fn authority_boundary_does_not_reingest_presentation_inventory() {
    let mut core = core();
    let mut gameplay = SessionGameplayState::default();
    let mut wire = crate::network::protocol::ItemWire::empty();
    wire.item = Item::Bread as u32;
    wire.count = 2;
    gameplay.hunger_milli = 10_000;
    gameplay.inventory[0] = Some(SessionInventorySlot::from_wire(wire, 0, 0));
    core.set_session_gameplay(7, gameplay);
    let response = core.submit_request(GameplayRequest {
        request_id: 37,
        client_sequence: 1,
        session_id: 7,
        dimension: 0,
        client_revision: 0,
        operation: GameplayOperation::ItemUse {
            item: Item::Bread as u32,
            count: 1,
        },
    });
    assert!(matches!(response.outcome, GameplayOutcome::Accepted { .. }));
    assert_eq!(
        core.session(7)
            .unwrap()
            .gameplay
            .count_item(Item::Bread as u32),
        1
    );
}

#[test]
fn combat_mutates_headless_entity_without_state_fallback() {
    let mut core = core();
    let mut attacker = core.session(7).unwrap().gameplay;
    attacker.attack_cooldown_ticks = ATTACK_COOLDOWN_TICKS;
    assert!(core.set_session_gameplay(7, attacker));
    let target = core
        .world_mut(Dimension::Overworld).unwrap()
        .entities
        .spawn(EntityType::Zombie, glam::Vec3::new(8.0, 80.0, 9.0));
    let before = core
        .world_mut(Dimension::Overworld).unwrap()
        .entities
        .get_by_id(target)
        .unwrap()
        .health;
    let response = core.submit_request(GameplayRequest {
        request_id: 31,
        client_sequence: 1,
        session_id: 7,
        dimension: 0,
        client_revision: 0,
        operation: GameplayOperation::Combat { target, action: 0 },
    });
    assert!(
        matches!(response.outcome, GameplayOutcome::Accepted { .. }),
        "unexpected combat response: {response:?}"
    );
    assert!(
        core.world_mut(Dimension::Overworld).unwrap()
            .entities
            .get_by_id(target)
            .unwrap()
            .health
            < before
    );
}

#[test]
fn trade_conserves_items_and_mount_projects_session_state() {
    let mut core = core();
    let villager = 900;
    let mut sell = crate::inventory::ItemStack::new(Item::Emerald, 1);
    sell.durability = 9;
    sell.enchantments
        .add_or_upgrade(crate::enchantment::Enchantment::Fortune(2));
    sell.custom_name.set("trade emerald");
    sell.can_break = 0x11;
    sell.can_place_on = 0x22;
    let offers = vec![crate::village::trade::TradeOffer::new(
        crate::inventory::ItemStack::new(Item::Wheat, 2),
        Some(crate::inventory::ItemStack::new(Item::Carrot, 1)),
        sell,
        4,
        1,
    )];
    {
        let world = core.world_mut(Dimension::Overworld).unwrap();
        let mut entity = crate::entity::Entity::new(
            villager,
            EntityType::Villager,
            glam::Vec3::new(9.0, 80.0, 8.0),
        );
        entity.profession = crate::village::poi::VillagerProfession::Farmer;
        entity.villager_level = crate::village::trade::VillagerLevel::Novice;
        entity.offers = offers;
        world.entities.entities.push(entity);
        world.entities.rebuild_indexes();
    }
    let mut gameplay = SessionGameplayState::default();
    let mut wheat = crate::network::protocol::ItemWire::empty();
    wheat.item = Item::Wheat as u32;
    wheat.count = 2;
    gameplay.inventory[0] = Some(SessionInventorySlot::from_wire(wheat, 0, 0));
    let mut carrot = crate::network::protocol::ItemWire::empty();
    carrot.item = Item::Carrot as u32;
    carrot.count = 1;
    gameplay.inventory[1] = Some(SessionInventorySlot::from_wire(carrot, 0, 0));
    core.set_session_gameplay(7, gameplay);
    let response = core.submit_request(GameplayRequest {
        request_id: 32,
        client_sequence: 1,
        session_id: 7,
        dimension: 0,
        client_revision: 0,
        operation: GameplayOperation::Trade {
            villager_id: villager,
            offer_index: 0,
        },
    });
    assert!(matches!(response.outcome, GameplayOutcome::Accepted { .. }));
    let state = core.session(7).unwrap().gameplay;
    assert_eq!(state.count_item(Item::Wheat as u32), 0);
    assert_eq!(state.count_item(Item::Carrot as u32), 0);
    assert_eq!(state.count_item(Item::Emerald as u32), 1);
    let emerald = state
        .inventory
        .iter()
        .flatten()
        .find(|slot| slot.item.item == Item::Emerald as u32)
        .unwrap();
    assert_eq!(emerald.item.durability, 9);
    let expected_name = sell.custom_name.as_str().as_bytes();
    assert_eq!(
        &emerald.item.custom_name[..expected_name.len()],
        expected_name
    );
    assert_eq!(emerald.can_break, 0x11);
    assert_eq!(emerald.can_place_on, 0x22);

    let vehicle = 901;
    {
        let world = core.world_mut(Dimension::Overworld).unwrap();
        world.entities.entities.push(crate::entity::Entity::new(
            vehicle,
            EntityType::Boat,
            glam::Vec3::new(9.0, 80.0, 8.0),
        ));
        world.entities.rebuild_indexes();
    }
    let response = core.submit_request(GameplayRequest {
        request_id: 33,
        client_sequence: 2,
        session_id: 7,
        dimension: 0,
        client_revision: core.current_revision(Dimension::Overworld),
        operation: GameplayOperation::Mount { entity_id: vehicle },
    });
    assert!(matches!(response.outcome, GameplayOutcome::Accepted { .. }));
    assert_eq!(
        core.session(7).unwrap().gameplay.mounted_entity,
        Some(vehicle)
    );
    assert!(core
        .world_mut(Dimension::Overworld).unwrap()
        .entities
        .get_by_id(vehicle)
        .unwrap()
        .passengers
        .contains(&7));
}
