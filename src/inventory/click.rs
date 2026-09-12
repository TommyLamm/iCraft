use super::stack::ItemStack;

pub struct StackClickResult {
    pub slot: Option<ItemStack>,
    pub dragged: Option<ItemStack>,
    pub cursor_from_slot: bool,
}

pub fn apply_stack_click(
    slot_item: Option<ItemStack>,
    dragged_item: Option<ItemStack>,
    is_left: bool,
) -> StackClickResult {
    let mut slot = slot_item;
    let mut dragged = dragged_item;
    let mut cursor_from_slot = false;

    if is_left {
        match (dragged, slot) {
            (Some(cursor), Some(existing)) if existing.can_merge_with(&cursor) => {
                let max_stack = existing.item.properties().max_stack;
                let transfer = max_stack.saturating_sub(existing.count).min(cursor.count);
                slot = Some(ItemStack {
                    count: existing.count + transfer,
                    ..existing
                });
                dragged = (cursor.count > transfer).then_some(ItemStack {
                    count: cursor.count - transfer,
                    ..cursor
                });
            }
            (Some(cursor), Some(existing)) => {
                slot = Some(cursor);
                dragged = Some(existing);
                cursor_from_slot = true;
            }
            (Some(cursor), None) => {
                slot = Some(cursor);
                dragged = None;
            }
            (None, Some(existing)) => {
                slot = None;
                dragged = Some(existing);
                cursor_from_slot = true;
            }
            (None, None) => {}
        }
    } else {
        match (dragged, slot) {
            (Some(cursor), Some(existing))
                if existing.can_merge_with(&cursor)
                    && existing.count < existing.item.properties().max_stack =>
            {
                slot = Some(ItemStack {
                    count: existing.count + 1,
                    ..existing
                });
                dragged = (cursor.count > 1).then_some(ItemStack {
                    count: cursor.count - 1,
                    ..cursor
                });
            }
            (Some(cursor), Some(existing)) if !existing.can_merge_with(&cursor) => {
                slot = Some(cursor);
                dragged = Some(existing);
                cursor_from_slot = true;
            }
            (Some(_), Some(_)) => {}
            (Some(cursor), None) => {
                slot = Some(ItemStack { count: 1, ..cursor });
                dragged = (cursor.count > 1).then_some(ItemStack {
                    count: cursor.count - 1,
                    ..cursor
                });
            }
            (None, Some(existing)) => {
                let take = existing.count.div_ceil(2);
                let keep = existing.count - take;
                slot = (keep > 0).then_some(ItemStack {
                    count: keep,
                    ..existing
                });
                dragged = Some(ItemStack {
                    count: take,
                    ..existing
                });
                cursor_from_slot = true;
            }
            (None, None) => {}
        }
    }

    StackClickResult {
        slot,
        dragged,
        cursor_from_slot,
    }
}
