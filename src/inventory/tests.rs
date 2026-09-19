use super::*;
use crate::world::BlockType;
use std::collections::HashSet;

#[test]
fn fishing_rod_starts_with_authority_durability() {
    assert_eq!(ItemStack::new(Item::FishingRod, 1).durability, 64);
}

#[test]
fn test_inventory_creative_init() {
    let inv = Inventory::new_creative();
    assert_eq!(inv.selected, 0);
    assert_eq!(inv.get_selected_block(), Some(BlockType::Grass));
    assert_eq!(inv.hotbar[0].unwrap().count, 64);
}

#[test]
fn test_inventory_add_item() {
    let mut inv = Inventory::new();
    assert!(inv.add_item(Item::Stone));
    assert_eq!(inv.hotbar[0].unwrap().item, Item::Stone);
    assert_eq!(inv.hotbar[0].unwrap().count, 1);

    assert!(inv.add_item(Item::Stone));
    assert_eq!(inv.hotbar[0].unwrap().count, 2);
}

#[test]
fn add_stack_returns_exact_metadata_preserving_remainder() {
    let mut inv = Inventory::new();
    inv.hotbar.fill(Some(ItemStack::new(Item::DiamondSword, 1)));
    inv.main.fill(Some(ItemStack::new(Item::DiamondPickaxe, 1)));

    let mut incoming = ItemStack::new(Item::Stone, 2);
    incoming.custom_name.set("Keepsake");
    incoming
        .enchantments
        .add_or_upgrade(crate::enchantment::Enchantment::Unbreaking(2));
    incoming.potion = Some(crate::brewing::PotionData {
        kind: crate::brewing::PotionKind::Speed,
        level: 2,
        duration_seconds: 45,
        splash: true,
    });
    inv.hotbar[0] = Some(ItemStack {
        count: 63,
        ..incoming
    });

    let remainder = inv.add_stack(incoming).expect("one item should remain");
    assert_eq!(inv.hotbar[0].unwrap().count, 64);
    assert_eq!(
        remainder,
        ItemStack {
            count: 1,
            ..incoming
        }
    );
}

#[test]
fn stack_clicks_merge_only_identical_metadata() {
    let plain = ItemStack::new(Item::Dirt, 4);
    let mut named = ItemStack::new(Item::Dirt, 3);
    named.custom_name.set("Garden Soil");

    for is_left in [true, false] {
        let rejected = apply_stack_click(Some(plain), Some(named), is_left);
        assert_eq!(rejected.slot, Some(named));
        assert_eq!(rejected.dragged, Some(plain));
        assert!(rejected.cursor_from_slot);
    }

    let left = apply_stack_click(Some(plain), Some(ItemStack { count: 3, ..plain }), true);
    assert_eq!(left.slot.unwrap().count, 7);
    assert!(left.dragged.is_none());

    let right = apply_stack_click(Some(plain), Some(ItemStack { count: 3, ..plain }), false);
    assert_eq!(right.slot.unwrap().count, 5);
    assert_eq!(right.dragged.unwrap().count, 2);
}

#[test]
fn stack_identity_checks_every_metadata_field() {
    let original = ItemStack::new(Item::Dirt, 1);
    assert!(original.can_merge_with(&ItemStack {
        count: 63,
        ..original
    }));

    let mut variants = Vec::new();
    variants.push(ItemStack::new(Item::Stone, 1));
    variants.push(ItemStack {
        durability: 1,
        ..original
    });

    let mut enchanted = original;
    enchanted
        .enchantments
        .add_or_upgrade(crate::enchantment::Enchantment::Unbreaking(1));
    variants.push(enchanted);

    variants.push(ItemStack {
        potion: Some(crate::brewing::PotionData::water()),
        ..original
    });

    let mut named = original;
    named.custom_name.set("Named");
    variants.push(named);

    assert!(variants
        .into_iter()
        .all(|variant| !original.can_merge_with(&variant)));
}

#[test]
fn add_item_does_not_merge_plain_items_into_named_stacks() {
    let mut inv = Inventory::new();
    let mut named = ItemStack::new(Item::Dirt, 1);
    named.custom_name.set("Named");
    inv.hotbar[0] = Some(named);

    assert!(inv.add_item(Item::Dirt));
    assert_eq!(inv.hotbar[0], Some(named));
    assert_eq!(inv.hotbar[1], Some(ItemStack::new(Item::Dirt, 1)));
}

#[test]
fn test_item_properties() {
    let pick = ItemStack::new(Item::StonePickaxe, 1);
    assert_eq!(pick.durability, 131);
    let grass = ItemStack::new(Item::Grass, 64);
    assert_eq!(grass.durability, 0);
}

#[test]
fn test_renders_flat() {
    // Non-block items render as flat sprites.
    assert!(Item::Seeds.renders_flat());
    assert!(Item::Wheat.renders_flat());
    assert!(Item::StonePickaxe.renders_flat());
    assert!(Item::Apple.renders_flat());
    // Cross-model plant blocks render as flat sprites.
    assert!(Item::Dandelion.renders_flat());
    assert!(Item::Poppy.renders_flat());
    assert!(Item::TallGrass.renders_flat());
    assert!(Item::SugarCane.renders_flat());
    // Full-cube block items keep cube rendering.
    assert!(!Item::Stone.renders_flat());
    assert!(!Item::Grass.renders_flat());
    assert!(!Item::Torch.renders_flat());
    assert!(!Item::Air.renders_flat());
}

#[test]
fn test_new_mob_items() {
    let flesh = Item::RottenFlesh;
    let prop = flesh.properties();
    assert_eq!(prop.name, "Rotten Flesh");
    assert_eq!(prop.tex_coords, (8, 3));
}

#[test]
fn creative_catalog_contains_every_non_air_item_once() {
    assert_eq!(CREATIVE_ITEMS.len(), 154);
    assert!(!CREATIVE_ITEMS.contains(&Item::Air));
    let unique: HashSet<_> = CREATIVE_ITEMS.iter().copied().collect();
    assert_eq!(unique.len(), CREATIVE_ITEMS.len());
}

#[test]
fn creative_catalog_items_have_valid_properties() {
    for item in CREATIVE_ITEMS {
        let properties = item.properties();
        assert!(!properties.name.is_empty(), "{item:?}");
        assert!(properties.max_stack > 0, "{item:?}");
        assert!(properties.tex_coords.0 < 16, "{item:?}");
        assert!(properties.tex_coords.1 < 16, "{item:?}");
        assert!(item.creative_tab().is_some(), "{item:?}");
    }
}

#[test]
fn creative_tabs_partition_catalog_without_duplicates() {
    let mut partition = Vec::new();
    for tab in CreativeTab::TABS
        .into_iter()
        .filter(|tab| *tab != CreativeTab::All)
    {
        let tab_items = Inventory::creative_items_for_tab(tab);
        assert!(tab_items
            .iter()
            .all(|item| item.creative_tab() == Some(tab)));
        partition.extend(tab_items);
    }

    let unique: HashSet<_> = partition.iter().copied().collect();
    assert_eq!(partition.len(), CREATIVE_ITEMS.len());
    assert_eq!(unique.len(), CREATIVE_ITEMS.len());
    assert_eq!(
        unique,
        CREATIVE_ITEMS.iter().copied().collect::<HashSet<_>>()
    );
}

#[test]
fn creative_window_scrolls_by_row_and_clamps() {
    let mut inventory = Inventory::new();
    inventory.selected = 4;
    assert_eq!(inventory.creative_visible_items().len(), 45);
    let expected_max_scroll = (CREATIVE_ITEMS.len().saturating_sub(45) + 8) / 9;
    assert_eq!(inventory.creative_max_scroll(), expected_max_scroll);

    inventory.scroll_creative(1);
    assert_eq!(inventory.creative_scroll_row, 1);
    assert_eq!(inventory.selected, 4);
    assert_eq!(
        inventory.creative_visible_items()[0],
        CREATIVE_ITEMS[CREATIVE_COLUMNS]
    );

    inventory.scroll_creative(-999);
    assert_eq!(inventory.creative_scroll_row, 0);
    inventory.creative_scroll_row = usize::MAX;
    inventory.clamp_creative_scroll();
    assert_eq!(inventory.creative_scroll_row, expected_max_scroll);
    assert_eq!(
        inventory.creative_visible_items().len(),
        CREATIVE_ITEMS.len() - expected_max_scroll * 9
    );

    inventory.select_creative_tab(CreativeTab::Tools);
    assert_eq!(inventory.creative_scroll_row, 0);
    assert_eq!(inventory.creative_visible_items().len(), 14);
    assert_eq!(inventory.creative_max_scroll(), 0);
}

#[test]
fn creative_catalog_supplies_left_max_and_right_one() {
    let mut inventory = Inventory::new();
    assert!(inventory.creative_supply(Item::Stone, true));
    assert_eq!(inventory.dragged.unwrap().count, 64);
    assert_eq!(
        inventory.creative_drag_origin,
        Some(CreativeDragOrigin::Catalog)
    );

    assert!(inventory.creative_supply(Item::DiamondSword, false));
    assert_eq!(inventory.dragged.unwrap().count, 1);
    assert_eq!(inventory.dragged.unwrap().item, Item::DiamondSword);
}

#[test]
fn creative_virtual_slot_write_is_a_no_op() {
    let mut inventory = Inventory::new_creative();
    let hotbar = inventory.hotbar;
    let main = inventory.main;
    inventory.write_creative_slot(Item::Stone, None);
    inventory.write_creative_slot(Item::Dirt, Some(ItemStack::new(Item::Diamond, 64)));
    assert_eq!(inventory.hotbar, hotbar);
    assert_eq!(inventory.main, main);
    assert!(CREATIVE_ITEMS.contains(&Item::Stone));
    assert!(CREATIVE_ITEMS.contains(&Item::Dirt));
}

#[test]
fn creative_hotbar_reuses_drag_drop_and_close_semantics() {
    let mut inventory = Inventory::new();
    assert!(inventory.creative_supply(Item::Stone, true));
    inventory.click_creative_hotbar(0, true);
    assert_eq!(inventory.hotbar[0].unwrap().item, Item::Stone);
    assert_eq!(inventory.hotbar[0].unwrap().count, 64);
    assert!(inventory.dragged.is_none());

    inventory.click_creative_hotbar(0, false);
    assert_eq!(inventory.hotbar[0].unwrap().count, 32);
    assert_eq!(inventory.dragged.unwrap().count, 32);
    assert_eq!(
        inventory.creative_drag_origin,
        Some(CreativeDragOrigin::Inventory)
    );
    assert!(inventory.finish_creative_cursor());
    assert!(inventory.dragged.is_none());
    assert_eq!(inventory.count_item(Item::Stone), 64);

    assert!(inventory.creative_supply(Item::Dirt, true));
    assert!(inventory.finish_creative_cursor());
    assert!(inventory.dragged.is_none());
    assert_eq!(inventory.count_item(Item::Dirt), 0);
}

#[test]
fn creative_close_keeps_real_cursor_when_storage_is_full() {
    let mut inventory = Inventory::new();
    inventory
        .hotbar
        .fill(Some(ItemStack::new(Item::DiamondSword, 1)));
    inventory
        .main
        .fill(Some(ItemStack::new(Item::DiamondPickaxe, 1)));
    inventory.dragged = Some(ItemStack::new(Item::Stone, 64));
    inventory.creative_drag_origin = Some(CreativeDragOrigin::Inventory);

    assert!(!inventory.finish_creative_cursor());
    assert_eq!(inventory.dragged.unwrap().item, Item::Stone);
    assert_eq!(inventory.dragged.unwrap().count, 64);
    assert_eq!(
        inventory.creative_drag_origin,
        Some(CreativeDragOrigin::Inventory)
    );
}

#[test]
fn close_storage_transaction_rolls_back_partial_returns_and_real_cursor() {
    let mut inventory = Inventory::new();
    inventory
        .hotbar
        .fill(Some(ItemStack::new(Item::DiamondSword, 1)));
    inventory
        .main
        .fill(Some(ItemStack::new(Item::DiamondPickaxe, 1)));
    inventory.hotbar[0] = Some(ItemStack::new(Item::Stone, 63));
    inventory.dragged = Some(ItemStack::new(Item::Dirt, 64));
    inventory.creative_drag_origin = Some(CreativeDragOrigin::Inventory);
    let original_hotbar = inventory.hotbar;
    let original_main = inventory.main;

    assert!(!inventory.try_store_for_close(&[ItemStack::new(Item::Stone, 2)]));
    assert_eq!(inventory.hotbar, original_hotbar);
    assert_eq!(inventory.main, original_main);
    assert_eq!(inventory.dragged, Some(ItemStack::new(Item::Dirt, 64)));
    assert_eq!(
        inventory.creative_drag_origin,
        Some(CreativeDragOrigin::Inventory)
    );
}

#[test]
fn close_storage_discards_only_catalog_cursor() {
    let mut inventory = Inventory::new();
    inventory
        .hotbar
        .fill(Some(ItemStack::new(Item::DiamondSword, 1)));
    inventory
        .main
        .fill(Some(ItemStack::new(Item::DiamondPickaxe, 1)));
    inventory.dragged = Some(ItemStack::new(Item::Stone, 64));
    inventory.creative_drag_origin = Some(CreativeDragOrigin::Catalog);

    assert!(inventory.try_store_for_close(&[]));
    assert!(inventory.dragged.is_none());
    assert!(inventory.creative_drag_origin.is_none());
}

#[test]
fn creative_hotbar_rejects_same_item_with_different_metadata() {
    let mut inventory = Inventory::new();
    inventory.hotbar[0] = Some(ItemStack::new(Item::Dirt, 4));
    let mut named = ItemStack::new(Item::Dirt, 3);
    named.custom_name.set("Garden Soil");
    inventory.dragged = Some(named);
    inventory.creative_drag_origin = Some(CreativeDragOrigin::Inventory);

    inventory.click_creative_hotbar(0, true);
    assert_eq!(inventory.hotbar[0], Some(named));
    assert_eq!(inventory.dragged, Some(ItemStack::new(Item::Dirt, 4)));
    assert_eq!(
        inventory.creative_drag_origin,
        Some(CreativeDragOrigin::Inventory)
    );
}

#[test]
fn splash_potion_stack_has_water_splash_metadata() {
    let stack = ItemStack::new(Item::SplashPotion, 1);
    let potion = stack.potion.expect("splash potion metadata");
    assert_eq!(potion.kind, crate::brewing::PotionKind::Water);
    assert!(potion.splash);
}

#[test]
fn automation_item_ids_append_without_reindexing_legacy_items() {
    // The wire/save ID is the enum discriminant.  Keep the catalog and
    // enum in lock-step, and assert the automation additions are appended
    // after the legacy catalog rather than silently shifting old saves.
    assert_eq!(ALL_ITEMS.len(), Item::Observer as usize + 1);
    for (id, item) in ALL_ITEMS.iter().copied().enumerate() {
        assert_eq!(item.to_u32(), id as u32);
        assert_eq!(Item::from_u32(id as u32), Some(item));
    }
    assert_eq!(Item::WaterBucket as usize + 3, Item::Observer as usize);
    assert_eq!(Item::from_u32(Item::Hopper.to_u32()), Some(Item::Hopper));
}
