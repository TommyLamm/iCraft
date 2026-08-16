//! Plan02: container click is a session-conserving authority transaction.
//!
//! Tests submit the same `GameplayOperation::ContainerClick` envelope that
//! `NetworkServer` builds from `Packet::ContainerClickRequest`. They never
//! call a typed internal helper as the write path.

use icraft::authority::contract::{
    AuthorityTopology, SessionContract, SessionGameplayState, SessionInventorySlot,
};
use icraft::authority::{AuthorityConfig, AuthorityCore};
use icraft::block_entity::{BlockEntity, ChestBlockEntity};
use icraft::brewing::{PotionData, PotionKind};
use icraft::container_sessions::ContainerSessionManager;
use icraft::dimension::Dimension;
use icraft::enchantment::Enchantment;
use icraft::inventory::{Item, ItemStack};
use icraft::network::protocol::{
    ContainerAction, GameplayOperation, GameplayOutcome, GameplayRequest, GameplayResponse,
    ItemWire, RejectReason, SlotRefWire,
};
use icraft::world::BlockType;
use std::collections::BTreeMap;

const SESSION_ID: u64 = 0x02_0001;
const CHEST: (i32, i32, i32) = (8, 80, 8);
const BREW_STAND: (i32, i32, i32) = (9, 80, 8);

fn new_core() -> AuthorityCore {
    let mut core = AuthorityCore::new(AuthorityConfig::default(), AuthorityTopology::Dedicated);
    core.register_session(SessionContract::new(
        SESSION_ID,
        "plan02",
        Dimension::Overworld as u8,
        [8.0, 80.0, 8.0],
        true,
        true,
    ))
    .expect("register Plan02 session");
    core.world.ensure_chunk(0, 0);
    core
}

fn session_slot(stack: ItemStack) -> SessionInventorySlot {
    SessionInventorySlot::from_wire(
        ItemWire::from_stack(&stack),
        stack.can_break,
        stack.can_place_on,
    )
}

fn request(
    core: &AuthorityCore,
    request_id: u128,
    client_sequence: u64,
    operation: GameplayOperation,
) -> GameplayRequest {
    GameplayRequest {
        request_id,
        client_sequence,
        session_id: SESSION_ID,
        dimension: Dimension::Overworld as u8,
        client_revision: core.revision_for_dimension(Dimension::Overworld),
        operation,
    }
}

fn submit(
    core: &mut AuthorityCore,
    request_id: u128,
    client_sequence: u64,
    operation: GameplayOperation,
) -> GameplayResponse {
    core.submit_request(request(core, request_id, client_sequence, operation))
}

fn accepted(response: &GameplayResponse) -> u64 {
    match response.outcome {
        GameplayOutcome::Accepted { revision } => revision,
        GameplayOutcome::Rejected { reason } => panic!("request rejected: {reason:?}"),
    }
}

fn rejected(response: &GameplayResponse, reason: RejectReason) {
    assert_eq!(response.outcome, GameplayOutcome::Rejected { reason });
}

fn seed_chest(core: &mut AuthorityCore, slots: &[(usize, ItemStack)]) {
    core.world
        .set_block(CHEST.0, CHEST.1, CHEST.2, BlockType::Chest, 0)
        .expect("seed chest block");
    let mut chest = ChestBlockEntity::new();
    for (index, stack) in slots {
        chest.set_stack(*index, Some(*stack));
    }
    core.world
        .chunks
        .set_block_entity(CHEST.0, CHEST.1, CHEST.2, Some(BlockEntity::Chest(chest)));
}

fn open_chest(core: &mut AuthorityCore, request_id: u128, sequence: u64) {
    accepted(&submit(
        core,
        request_id,
        sequence,
        GameplayOperation::Container {
            action: ContainerAction::Open.to_wire(),
            x: CHEST.0,
            y: CHEST.1,
            z: CHEST.2,
            slot: 0,
        },
    ));
}

fn click(
    core: &mut AuthorityCore,
    request_id: u128,
    sequence: u64,
    slot: u16,
    is_left: bool,
    dragged: Option<ItemWire>,
) -> GameplayResponse {
    submit(
        core,
        request_id,
        sequence,
        GameplayOperation::ContainerClick {
            x: CHEST.0,
            y: CHEST.1,
            z: CHEST.2,
            slot,
            is_left,
            dragged,
        },
    )
}

fn identity_key(
    stack: &ItemStack,
) -> (
    u32,
    u32,
    [u8; 6],
    Option<(u8, u8, u16, bool)>,
    [u8; 24],
    u128,
    u128,
) {
    let wire = ItemWire::from_stack(stack);
    (
        wire.item,
        wire.durability as u32,
        wire.enchantments,
        wire.potion.map(|potion| {
            (
                potion.kind,
                potion.level,
                potion.duration_seconds,
                potion.splash,
            )
        }),
        wire.custom_name,
        wire.can_break,
        wire.can_place_on,
    )
}

fn add_stack(
    totals: &mut BTreeMap<
        (
            u32,
            u32,
            [u8; 6],
            Option<(u8, u8, u16, bool)>,
            [u8; 24],
            u128,
            u128,
        ),
        u32,
    >,
    stack: ItemStack,
) {
    if stack.count == 0 || stack.item == Item::Air {
        return;
    }
    *totals.entry(identity_key(&stack)).or_insert(0) += stack.count;
}

fn stack_from_slot(slot: SessionInventorySlot) -> Option<ItemStack> {
    let mut stack = slot.item.to_stack()?;
    stack.can_break = slot.can_break;
    stack.can_place_on = slot.can_place_on;
    Some(stack)
}

fn conserved_totals(
    core: &AuthorityCore,
) -> BTreeMap<
    (
        u32,
        u32,
        [u8; 6],
        Option<(u8, u8, u16, bool)>,
        [u8; 24],
        u128,
        u128,
    ),
    u32,
> {
    let mut totals = BTreeMap::new();
    let gameplay = core.session(SESSION_ID).unwrap().gameplay;
    for slot in gameplay.inventory.into_iter().flatten() {
        if let Some(stack) = stack_from_slot(slot) {
            add_stack(&mut totals, stack);
        }
    }
    if let Some(cursor) = gameplay.cursor {
        if let Some(stack) = stack_from_slot(cursor) {
            add_stack(&mut totals, stack);
        }
    }
    if let Some(slots) =
        ContainerSessionManager::get_container_slots(&core.world.chunks, CHEST.0, CHEST.1, CHEST.2)
    {
        for slot in slots.into_iter().flatten() {
            add_stack(&mut totals, slot);
        }
    }
    totals
}

fn chest_slot(core: &AuthorityCore, index: u16) -> Option<ItemStack> {
    ContainerSessionManager::get_container_slots(&core.world.chunks, CHEST.0, CHEST.1, CHEST.2)
        .and_then(|slots| slots.get(usize::from(index)).copied().flatten())
}

fn rich_stack(item: Item, count: u32, variant: u8) -> ItemStack {
    let mut stack = ItemStack::new(item, count);
    stack.durability = 11 + u32::from(variant);
    stack.enchantments.add_or_upgrade(if variant == 0 {
        Enchantment::Unbreaking(1)
    } else {
        Enchantment::Efficiency(2)
    });
    stack.potion = Some(PotionData {
        kind: PotionKind::Strength,
        level: 1 + variant,
        duration_seconds: 30 + u16::from(variant),
        splash: variant != 0,
    });
    stack
        .custom_name
        .set(if variant == 0 { "Plan02 A" } else { "Plan02 B" });
    stack.can_break = 1u128 << (BlockType::Stone as u8);
    stack.can_place_on = 1u128 << (BlockType::Dirt as u8);
    stack
}

fn set_hotbar(core: &mut AuthorityCore, index: usize, stack: Option<ItemStack>) {
    let mut gameplay = core.session(SESSION_ID).unwrap().gameplay;
    gameplay.inventory[index] = stack.map(session_slot);
    assert!(core.set_session_gameplay(SESSION_ID, gameplay));
}

/// Negative contract: a client-authored diamond must not be written into the
/// chest. This currently succeeds via `replace_container_slot` and must reject
/// after Plan02.
#[test]
fn forged_diamond_item_wire_is_rejected_without_mutation() {
    let mut core = new_core();
    seed_chest(&mut core, &[(0, ItemStack::new(Item::Dirt, 2))]);
    set_hotbar(&mut core, 0, Some(ItemStack::new(Item::Stone, 4)));
    open_chest(&mut core, 1, 1);
    let before = conserved_totals(&core);
    let before_chest = chest_slot(&core, 0);
    let before_hotbar = core.session(SESSION_ID).unwrap().gameplay.inventory[0];

    let forged = ItemWire::from_stack(&ItemStack::new(Item::Diamond, 1));
    rejected(
        &click(&mut core, 2, 2, 0, true, Some(forged)),
        RejectReason::InvalidState,
    );

    assert_eq!(conserved_totals(&core), before);
    assert_eq!(chest_slot(&core, 0), before_chest);
    assert_eq!(
        core.session(SESSION_ID).unwrap().gameplay.inventory[0],
        before_hotbar
    );
    assert!(core.session(SESSION_ID).unwrap().gameplay.cursor.is_none());
}

#[test]
fn left_click_places_held_hotbar_stack_into_empty_slot() {
    let mut core = new_core();
    seed_chest(&mut core, &[]);
    let held = rich_stack(Item::Stone, 5, 0);
    set_hotbar(&mut core, 0, Some(held));
    open_chest(&mut core, 1, 1);
    let before = conserved_totals(&core);

    accepted(&click(
        &mut core,
        2,
        2,
        0,
        true,
        Some(ItemWire::from_stack(&held)),
    ));

    assert_eq!(conserved_totals(&core), before);
    assert_eq!(chest_slot(&core, 0), Some(held));
    assert!(core.session(SESSION_ID).unwrap().gameplay.inventory[0].is_none());
    assert!(core.session(SESSION_ID).unwrap().gameplay.cursor.is_none());
}

#[test]
fn left_click_swaps_different_stacks_and_keeps_totals() {
    let mut core = new_core();
    let chest_stack = rich_stack(Item::Dirt, 3, 1);
    let held = rich_stack(Item::Stone, 2, 0);
    seed_chest(&mut core, &[(0, chest_stack)]);
    set_hotbar(&mut core, 0, Some(held));
    open_chest(&mut core, 1, 1);
    let before = conserved_totals(&core);

    accepted(&click(
        &mut core,
        2,
        2,
        0,
        true,
        Some(ItemWire::from_stack(&held)),
    ));

    assert_eq!(conserved_totals(&core), before);
    assert_eq!(chest_slot(&core, 0), Some(held));
    let gameplay = core.session(SESSION_ID).unwrap().gameplay;
    assert!(gameplay.inventory[0].is_none());
    assert_eq!(
        gameplay.cursor.map(stack_from_slot),
        Some(Some(chest_stack))
    );
}

#[test]
fn extract_with_empty_hand_moves_into_session_inventory() {
    let mut core = new_core();
    let stored = rich_stack(Item::IronIngot, 4, 0);
    seed_chest(&mut core, &[(1, stored)]);
    open_chest(&mut core, 1, 1);
    let before = conserved_totals(&core);

    accepted(&click(&mut core, 2, 2, 1, true, None));

    assert_eq!(conserved_totals(&core), before);
    assert!(chest_slot(&core, 1).is_none());
    let gameplay = core.session(SESSION_ID).unwrap().gameplay;
    assert_eq!(
        gameplay.inventory[0].and_then(stack_from_slot),
        Some(stored)
    );
    assert!(gameplay.cursor.is_none());
}

#[test]
fn full_inventory_extract_is_rejected_and_conserves() {
    let mut core = new_core();
    seed_chest(&mut core, &[(0, ItemStack::new(Item::Diamond, 1))]);
    let mut gameplay = SessionGameplayState::default();
    for slot in &mut gameplay.inventory {
        *slot = Some(session_slot(ItemStack::new(Item::Dirt, 1)));
    }
    assert!(core.set_session_gameplay(SESSION_ID, gameplay));
    open_chest(&mut core, 1, 1);
    let before = conserved_totals(&core);
    let before_inventory = core.session(SESSION_ID).unwrap().gameplay.inventory;

    rejected(
        &click(&mut core, 2, 2, 0, true, None),
        RejectReason::InvalidState,
    );

    assert_eq!(conserved_totals(&core), before);
    assert_eq!(chest_slot(&core, 0), Some(ItemStack::new(Item::Diamond, 1)));
    assert_eq!(
        core.session(SESSION_ID).unwrap().gameplay.inventory,
        before_inventory
    );
    assert!(core.session(SESSION_ID).unwrap().gameplay.cursor.is_none());
}

#[test]
fn brew_locked_hotbar_stack_cannot_be_clicked_into_chest() {
    let mut core = new_core();
    seed_chest(&mut core, &[]);
    core.world
        .set_block(
            BREW_STAND.0,
            BREW_STAND.1,
            BREW_STAND.2,
            BlockType::BrewingStand,
            0,
        )
        .expect("seed brewing stand");
    let wart = ItemStack::new(Item::NetherWart, 1);
    let potion = ItemStack::new(Item::Potion, 1);
    let mut gameplay = SessionGameplayState::default();
    gameplay.inventory[0] = Some(session_slot(wart));
    gameplay.inventory[1] = Some(session_slot(potion));
    assert!(core.set_session_gameplay(SESSION_ID, gameplay));
    let brew_state = core.session(SESSION_ID).unwrap().gameplay;
    accepted(&submit(
        &mut core,
        1,
        1,
        GameplayOperation::Brew {
            action: 0,
            x: BREW_STAND.0,
            y: BREW_STAND.1,
            z: BREW_STAND.2,
            ingredient: Some(SlotRefWire {
                index: 0,
                count: 1,
                expected: brew_state.inventory[0].unwrap().into(),
            }),
            bottles: [
                Some(SlotRefWire {
                    index: 1,
                    count: 1,
                    expected: brew_state.inventory[1].unwrap().into(),
                }),
                None,
                None,
            ],
        },
    ));
    open_chest(&mut core, 2, 2);
    let before = conserved_totals(&core);
    let before_inventory = core.session(SESSION_ID).unwrap().gameplay.inventory;

    rejected(
        &click(&mut core, 3, 3, 0, true, Some(ItemWire::from_stack(&wart))),
        RejectReason::InvalidState,
    );

    assert_eq!(conserved_totals(&core), before);
    assert!(chest_slot(&core, 0).is_none());
    assert_eq!(
        core.session(SESSION_ID).unwrap().gameplay.inventory,
        before_inventory
    );
}

#[test]
fn non_viewer_click_is_permission_denied_and_leaves_container() {
    let mut core = new_core();
    seed_chest(&mut core, &[(0, ItemStack::new(Item::Dirt, 2))]);
    set_hotbar(&mut core, 0, Some(ItemStack::new(Item::Stone, 1)));
    let before = conserved_totals(&core);

    rejected(
        &click(
            &mut core,
            1,
            1,
            0,
            true,
            Some(ItemWire::from_stack(&ItemStack::new(Item::Stone, 1))),
        ),
        RejectReason::PermissionDenied,
    );

    assert_eq!(conserved_totals(&core), before);
    assert_eq!(chest_slot(&core, 0), Some(ItemStack::new(Item::Dirt, 2)));
}

#[test]
fn metadata_mismatch_on_same_item_is_forged_and_rejected() {
    let mut core = new_core();
    seed_chest(&mut core, &[]);
    let held = rich_stack(Item::Stone, 2, 0);
    set_hotbar(&mut core, 0, Some(held));
    open_chest(&mut core, 1, 1);
    let before = conserved_totals(&core);

    let mut forged = ItemWire::from_stack(&held);
    forged.durability = held.durability as u16 + 3;
    rejected(
        &click(&mut core, 2, 2, 0, true, Some(forged)),
        RejectReason::InvalidState,
    );
    assert_eq!(conserved_totals(&core), before);
    assert!(chest_slot(&core, 0).is_none());
}

#[test]
fn legacy_container_click_envelope_extracts_into_inventory() {
    let mut core = new_core();
    seed_chest(&mut core, &[(2, ItemStack::new(Item::Coal, 3))]);
    open_chest(&mut core, 1, 1);
    let before = conserved_totals(&core);

    accepted(&submit(
        &mut core,
        2,
        2,
        GameplayOperation::Container {
            action: ContainerAction::Click.to_wire(),
            x: CHEST.0,
            y: CHEST.1,
            z: CHEST.2,
            slot: 2,
        },
    ));

    assert_eq!(conserved_totals(&core), before);
    assert!(chest_slot(&core, 2).is_none());
    assert_eq!(
        core.session(SESSION_ID).unwrap().gameplay.inventory[0].and_then(stack_from_slot),
        Some(ItemStack::new(Item::Coal, 3))
    );
}
