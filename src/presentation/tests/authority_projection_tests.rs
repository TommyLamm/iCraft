// Tests extracted from state.rs::authority_projection_tests (Plan 27).

use super::*;

use super::*;

#[test]
fn session_inventory_projection_preserves_rich_stack_metadata() {
    let mut stack = ItemStack::new(Item::DiamondPickaxe, 1);
    stack.durability = 37;
    stack
        .enchantments
        .add_or_upgrade(crate::enchantment::Enchantment::Efficiency(3));
    stack.custom_name.set("authority pick");
    stack.can_break = 0x1234;
    stack.can_place_on = 0x5678;
    let slot = State::session_slot_from_stack(Some(stack)).expect("slot");
    let roundtrip = State::stack_from_session_slot(slot).expect("stack");
    assert_eq!(roundtrip.item, stack.item);
    assert_eq!(roundtrip.count, stack.count);
    assert_eq!(roundtrip.durability, stack.durability);
    assert_eq!(roundtrip.enchantments, stack.enchantments);
    assert_eq!(roundtrip.custom_name, stack.custom_name);
    assert_eq!(roundtrip.can_break, stack.can_break);
    assert_eq!(roundtrip.can_place_on, stack.can_place_on);
}
