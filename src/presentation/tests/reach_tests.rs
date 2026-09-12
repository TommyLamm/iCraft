// Tests extracted from state.rs::reach_tests (Plan 27).

use super::*;

use super::*;

#[test]
fn calculate_block_break_rewards_harvest_and_drops() {
    let pos = (10, 60, 10);

    // Stone with bare hand in Survival -> not eligible to harvest (no drops)
    let rewards =
        calculate_block_break_rewards(BlockType::Stone, 0, pos, None, GameMode::Survival);
    assert!(rewards.drops.is_empty());
    assert_eq!(rewards.xp, 0);

    // Stone with Pickaxe -> eligible, drops Stone
    let pick = ItemStack::new(Item::StonePickaxe, 1);
    let rewards = calculate_block_break_rewards(
        BlockType::Stone,
        0,
        pos,
        Some(&pick),
        GameMode::Survival,
    );
    assert_eq!(rewards.drops.len(), 1);
    assert_eq!(rewards.drops[0].item, Item::Stone);

    // DiamondOre with IronPickaxe -> drops Diamond + 5 XP
    let iron_pick = ItemStack::new(Item::IronPickaxe, 1);
    let rewards = calculate_block_break_rewards(
        BlockType::DiamondOre,
        0,
        pos,
        Some(&iron_pick),
        GameMode::Survival,
    );
    assert_eq!(rewards.drops[0].item, Item::Diamond);
    assert_eq!(rewards.xp, 5);

    // DiamondOre with SilkTouch -> drops DiamondOre block
    let mut silk_pick = ItemStack::new(Item::IronPickaxe, 1);
    silk_pick
        .enchantments
        .add_or_upgrade(crate::enchantment::Enchantment::SilkTouch);
    let rewards = calculate_block_break_rewards(
        BlockType::DiamondOre,
        0,
        pos,
        Some(&silk_pick),
        GameMode::Survival,
    );
    assert_eq!(rewards.drops[0].item, Item::DiamondOre);
    assert_eq!(rewards.xp, 0);

    // Creative mode -> zero drops
    let rewards = calculate_block_break_rewards(
        BlockType::Stone,
        0,
        pos,
        Some(&pick),
        GameMode::Creative,
    );
    assert!(rewards.drops.is_empty());
}

#[test]
fn calculate_block_break_rewards_mature_and_immature_crops() {
    let pos = (10, 60, 10);

    // Mature Wheat (age 7) -> Wheat
    let mature_wheat =
        calculate_block_break_rewards(BlockType::WheatCrop, 7, pos, None, GameMode::Survival);
    assert_eq!(mature_wheat.drops.len(), 1);
    assert_eq!(mature_wheat.drops[0].item, Item::Wheat);

    // Immature Wheat (age 3) -> Seeds
    let immature_wheat =
        calculate_block_break_rewards(BlockType::WheatCrop, 3, pos, None, GameMode::Survival);
    assert_eq!(immature_wheat.drops.len(), 1);
    assert_eq!(immature_wheat.drops[0].item, Item::Seeds);

    // Immature Carrot (age 2) -> Carrot
    let immature_carrot =
        calculate_block_break_rewards(BlockType::CarrotCrop, 2, pos, None, GameMode::Survival);
    assert_eq!(immature_carrot.drops.len(), 1);
    assert_eq!(immature_carrot.drops[0].item, Item::Carrot);
}

#[test]
fn inventory_click_outside_and_close_overflow_tests() {
    let mut inv = Inventory::new();
    inv.dragged = Some(ItemStack::new(Item::Dirt, 64));
    assert_eq!(inv.dragged.unwrap().count, 64);

    // Fill inventory completely
    for slot in inv.hotbar.iter_mut() {
        *slot = Some(ItemStack::new(Item::Stone, 64));
    }
    for slot in inv.main.iter_mut() {
        *slot = Some(ItemStack::new(Item::Stone, 64));
    }

    // add_stack with full inventory returns remainder
    let remainder = inv.add_stack(ItemStack::new(Item::Dirt, 64));
    assert_eq!(remainder, Some(ItemStack::new(Item::Dirt, 64)));
}

#[test]
fn inventory_slot_hits_map_to_inventory_decision_targets() {
    assert_eq!(
        presentation_target_for_slot(SlotType::ContainerSlot(3)),
        PresentationInventoryTarget::ContainerSlot
    );
    assert_eq!(
        presentation_target_for_slot(SlotType::Hotbar(0)),
        PresentationInventoryTarget::PlayerInventory
    );
    assert_eq!(
        presentation_target_for_slot(SlotType::Backpack(4)),
        PresentationInventoryTarget::PlayerInventory
    );
    assert_eq!(
        presentation_target_for_slot(SlotType::AnvilOutput),
        PresentationInventoryTarget::Workstation
    );
    assert_eq!(
        presentation_target_for_slot(SlotType::EnchantInput),
        PresentationInventoryTarget::Workstation
    );
    assert_eq!(
        presentation_target_for_slot(SlotType::EnchantLapis),
        PresentationInventoryTarget::Workstation
    );
    assert_eq!(
        presentation_target_for_authority_hit(InventoryHit::Merchant { offer_index: 0 }),
        None
    );
    assert_eq!(
        presentation_target_for_authority_hit(InventoryHit::RecipeBook),
        Some(PresentationInventoryTarget::Workstation)
    );
    assert_eq!(
        presentation_target_for_authority_hit(InventoryHit::Empty),
        None
    );

    let embedded = PresentationTopology::Embedded;
    let join = PresentationTopology::JoinClient;
    assert_eq!(
        embedded.inventory_decision(PresentationInventoryTarget::ContainerSlot),
        PresentationInventoryAction::SendAuthorityOp
    );
    assert_eq!(
        join.inventory_decision(PresentationInventoryTarget::ContainerSlot),
        PresentationInventoryAction::SendAuthorityOp
    );
    assert_eq!(
        embedded.inventory_decision(PresentationInventoryTarget::PlayerInventory),
        PresentationInventoryAction::LocalMutate
    );
    assert_eq!(
        join.inventory_decision(PresentationInventoryTarget::PlayerInventory),
        PresentationInventoryAction::Reject
    );
    assert_eq!(
        embedded.inventory_decision(PresentationInventoryTarget::Workstation),
        PresentationInventoryAction::Reject
    );
    assert_eq!(
        join.inventory_decision(PresentationInventoryTarget::Workstation),
        PresentationInventoryAction::Reject
    );
}
