use super::catalog::{
    CreativeTab, Item, CREATIVE_COLUMNS, CREATIVE_ITEMS, CREATIVE_ROWS, CREATIVE_VISIBLE_SLOTS,
};
use super::click::apply_stack_click;
use super::stack::{CreativeDragOrigin, ItemStack};
use crate::world::BlockType;

/// Inventory for a container block entity (e.g. chest).
/// Single chest = 27 slots; double chest uses two halves of 27 slots each.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ContainerInventory {
    pub slots: [Option<ItemStack>; 27],
}

impl ContainerInventory {
    pub fn new() -> Self {
        Self { slots: [None; 27] }
    }

    pub fn is_empty(&self) -> bool {
        self.slots.iter().all(|s| s.is_none())
    }

    pub fn total_items(&self) -> u32 {
        self.slots.iter().flatten().map(|s| s.count).sum()
    }

    pub fn clear(&mut self) {
        self.slots.fill(None);
    }

    /// Returns the remainder if the stack couldn't be fully added.
    pub fn add_stack(&mut self, mut incoming: ItemStack) -> Option<ItemStack> {
        if incoming.count == 0 || incoming.item == Item::Air {
            return None;
        }
        let max_stack = incoming.item.properties().max_stack;
        for slot in self.slots.iter_mut() {
            if let Some(existing) = slot {
                if existing.can_merge_with(&incoming) && existing.count < max_stack {
                    let moved = (max_stack - existing.count).min(incoming.count);
                    existing.count += moved;
                    incoming.count -= moved;
                    if incoming.count == 0 {
                        return None;
                    }
                }
            }
        }
        for slot in self.slots.iter_mut() {
            if slot.is_none() {
                let moved = incoming.count.min(max_stack);
                *slot = Some(ItemStack {
                    count: moved,
                    ..incoming
                });
                incoming.count -= moved;
                if incoming.count == 0 {
                    return None;
                }
            }
        }
        Some(incoming)
    }

    /// Return the number of slots that can accept incoming.
    pub fn storage_capacity_for(&self, incoming: ItemStack) -> u32 {
        let max_stack = incoming.item.properties().max_stack;
        self.slots
            .iter()
            .map(|slot| match slot {
                Some(existing) if existing.can_merge_with(&incoming) => {
                    max_stack.saturating_sub(existing.count)
                }
                None => max_stack,
                _ => 0,
            })
            .sum()
    }

    /// Selects a random non-empty slot index deterministically given a seed value.
    pub fn select_random_non_empty_slot(&self, seed: u64) -> Option<usize> {
        let non_empty: Vec<usize> = self
            .slots
            .iter()
            .enumerate()
            .filter_map(|(i, slot)| slot.as_ref().filter(|s| s.count > 0).map(|_| i))
            .collect();
        if non_empty.is_empty() {
            None
        } else {
            let idx = (seed as usize) % non_empty.len();
            Some(non_empty[idx])
        }
    }
}

pub struct Inventory {
    pub hotbar: [Option<ItemStack>; 9],
    pub main: [Option<ItemStack>; 27],
    pub armor: [Option<ItemStack>; 4],
    pub offhand: Option<ItemStack>,
    pub craft_input: Vec<Option<ItemStack>>, // 4 slots for 2x2, 9 slots for 3x3
    pub craft_output: Option<ItemStack>,
    pub dragged: Option<ItemStack>,
    pub creative_drag_origin: Option<CreativeDragOrigin>,
    pub creative_tab: CreativeTab,
    pub creative_scroll_row: usize,
    pub selected: usize, // Selected hotbar slot: 0..8
    pub is_open: bool,
    pub is_table_open: bool,
}

impl Inventory {
    pub fn new() -> Self {
        Self {
            hotbar: [None; 9],
            main: [None; 27],
            armor: [None; 4],
            offhand: None,
            craft_input: vec![None; 4],
            craft_output: None,
            dragged: None,
            creative_drag_origin: None,
            creative_tab: CreativeTab::All,
            creative_scroll_row: 0,
            selected: 0,
            is_open: false,
            is_table_open: false,
        }
    }

    pub fn new_creative() -> Self {
        let mut inv = Self::new();
        let creative_items = [
            Item::Grass,
            Item::Dirt,
            Item::Stone,
            Item::OakLog,
            Item::OakPlanks,
            Item::Glass,
            Item::Cobblestone,
            Item::Water,
            Item::Torch,
        ];
        for (i, &item) in creative_items.iter().enumerate() {
            inv.hotbar[i] = Some(ItemStack::new(item, 64));
        }

        let extra_items = [
            Item::RedstoneWire,
            Item::RedstoneTorch,
            Item::Repeater,
            Item::Comparator,
            Item::StoneButton,
            Item::Lever,
            Item::PressurePlate,
            Item::Piston,
            Item::StickyPiston,
            Item::RedstoneLamp,
            Item::OakDoor,
            Item::OakTrapdoor,
            Item::Dispenser,
            Item::Dropper,
            Item::NoteBlock,
            Item::DiamondPickaxe,
            Item::FlintAndSteel,
            Item::Netherrack,
            Item::SoulSand,
            Item::Glowstone,
            Item::EndStone,
            Item::EndPortalFrame,
            Item::EyeOfEnder,
            Item::WitherSkeletonSkull,
            Item::EndCrystal,
            Item::Elytra,
            Item::NetherStar,
        ];
        for (i, &item) in extra_items.iter().enumerate() {
            inv.main[i] = Some(ItemStack::new(item, item.properties().max_stack));
        }
        inv
    }

    pub fn creative_items_for_tab(tab: CreativeTab) -> Vec<Item> {
        CREATIVE_ITEMS
            .iter()
            .copied()
            .filter(|item| tab == CreativeTab::All || item.creative_tab() == Some(tab))
            .collect()
    }

    pub fn creative_max_scroll_for_tab(tab: CreativeTab) -> usize {
        let item_count = Self::creative_items_for_tab(tab).len();
        let total_rows = item_count.div_ceil(CREATIVE_COLUMNS);
        total_rows.saturating_sub(CREATIVE_ROWS)
    }

    pub fn creative_max_scroll(&self) -> usize {
        Self::creative_max_scroll_for_tab(self.creative_tab)
    }

    pub fn creative_visible_items(&self) -> Vec<Item> {
        let start = self.creative_scroll_row * CREATIVE_COLUMNS;
        Self::creative_items_for_tab(self.creative_tab)
            .into_iter()
            .skip(start)
            .take(CREATIVE_VISIBLE_SLOTS)
            .collect()
    }

    pub fn select_creative_tab(&mut self, tab: CreativeTab) {
        self.creative_tab = tab;
        self.creative_scroll_row = 0;
    }

    pub fn clamp_creative_scroll(&mut self) {
        self.creative_scroll_row = self.creative_scroll_row.min(self.creative_max_scroll());
    }

    pub fn scroll_creative(&mut self, direction: i32) {
        let max_scroll = self.creative_max_scroll() as i32;
        self.creative_scroll_row =
            (self.creative_scroll_row as i32 + direction).clamp(0, max_scroll) as usize;
    }

    pub fn creative_supply(&mut self, item: Item, is_left: bool) -> bool {
        if item == Item::Air {
            return false;
        }
        if self.dragged.is_some()
            && self.creative_drag_origin != Some(CreativeDragOrigin::Catalog)
            && !self.try_return_dragged_to_storage()
        {
            return false;
        }

        let count = if is_left {
            item.properties().max_stack
        } else {
            1
        };
        self.dragged = Some(ItemStack::new(item, count));
        self.creative_drag_origin = Some(CreativeDragOrigin::Catalog);
        true
    }

    pub fn write_creative_slot(&mut self, _item: Item, _stack: Option<ItemStack>) {
        // Creative catalog slots are immutable, infinite-supply views.
    }

    pub fn click_creative_hotbar(&mut self, index: usize, is_left: bool) {
        if index >= self.hotbar.len() {
            return;
        }
        if self.dragged.is_some() && self.creative_drag_origin.is_none() {
            self.creative_drag_origin = Some(CreativeDragOrigin::Inventory);
        }

        let previous_origin = self.creative_drag_origin;
        let result = apply_stack_click(self.hotbar[index], self.dragged, is_left);
        self.hotbar[index] = result.slot;
        self.dragged = result.dragged;
        self.creative_drag_origin = if self.dragged.is_none() {
            None
        } else if result.cursor_from_slot {
            Some(CreativeDragOrigin::Inventory)
        } else {
            previous_origin
        };
    }

    pub fn finish_creative_cursor(&mut self) -> bool {
        match self.creative_drag_origin {
            Some(CreativeDragOrigin::Catalog) => {
                self.dragged = None;
                self.creative_drag_origin = None;
                true
            }
            Some(CreativeDragOrigin::Inventory) | None => self.try_return_dragged_to_storage(),
        }
    }

    fn try_return_dragged_to_storage(&mut self) -> bool {
        let Some(stack) = self.dragged else {
            self.creative_drag_origin = None;
            return true;
        };
        if self.storage_capacity_for(stack) < stack.count {
            return false;
        }

        let origin = self.creative_drag_origin;
        self.dragged = None;
        match self.add_stack(stack) {
            None => {
                self.creative_drag_origin = None;
                true
            }
            Some(remainder) => {
                self.dragged = Some(remainder);
                self.creative_drag_origin = origin;
                false
            }
        }
    }

    pub fn find_item(&self, item: Item) -> Option<(usize, ItemStack)> {
        for (i, slot) in self.hotbar.iter().enumerate() {
            if let Some(stack) = slot {
                if stack.item == item {
                    return Some((i, *stack));
                }
            }
        }
        for (i, slot) in self.main.iter().enumerate() {
            if let Some(stack) = slot {
                if stack.item == item {
                    return Some((i + 9, *stack));
                }
            }
        }
        if let Some(stack) = &self.offhand {
            if stack.item == item {
                return Some((99, *stack));
            }
        }
        None
    }

    pub fn remove_at_slot(&mut self, slot_idx: usize) {
        if slot_idx < 9 {
            self.hotbar[slot_idx] = None;
        } else if slot_idx < 36 {
            self.main[slot_idx - 9] = None;
        } else if slot_idx == 99 {
            self.offhand = None;
        }
    }

    pub fn swap_offhand(&mut self) {
        let sel = self.selected;
        let main = self.hotbar[sel];
        self.hotbar[sel] = self.offhand;
        self.offhand = main;
    }

    fn storage_capacity_for(&self, incoming: ItemStack) -> u32 {
        let max_stack = incoming.item.properties().max_stack;
        self.hotbar
            .iter()
            .chain(self.main.iter())
            .map(|slot| match slot {
                Some(existing) if existing.can_merge_with(&incoming) => {
                    max_stack.saturating_sub(existing.count)
                }
                None => max_stack,
                _ => 0,
            })
            .sum()
    }

    #[cfg(test)]
    pub(crate) fn try_store_for_close(&mut self, returning_items: &[ItemStack]) -> bool {
        let original_hotbar = self.hotbar;
        let original_main = self.main;
        let original_dragged = self.dragged;
        let original_origin = self.creative_drag_origin;

        for &stack in returning_items {
            if self.add_stack(stack).is_some() {
                self.hotbar = original_hotbar;
                self.main = original_main;
                self.dragged = original_dragged;
                self.creative_drag_origin = original_origin;
                return false;
            }
        }

        if !self.finish_creative_cursor() {
            self.hotbar = original_hotbar;
            self.main = original_main;
            self.dragged = original_dragged;
            self.creative_drag_origin = original_origin;
            return false;
        }

        true
    }

    pub fn clear(&mut self) {
        self.hotbar = [None; 9];
        self.main = [None; 27];
        self.armor = [None; 4];
        self.offhand = None;
        self.craft_input.fill(None);
        self.craft_output = None;
        self.dragged = None;
        self.creative_drag_origin = None;
    }

    pub fn get_selected_block(&self) -> Option<BlockType> {
        self.hotbar[self.selected].and_then(|stack| stack.item.properties().block_type)
    }

    pub fn add_item(&mut self, item: Item) -> bool {
        self.add_stack(ItemStack::new(item, 1)).is_none()
    }

    pub fn add_stack(&mut self, mut incoming: ItemStack) -> Option<ItemStack> {
        if incoming.count == 0 {
            return None;
        }
        if incoming.item == Item::Air {
            return Some(incoming);
        }
        let max_stack = incoming.item.properties().max_stack;
        for slot in self.hotbar.iter_mut().chain(self.main.iter_mut()) {
            if let Some(existing) = slot {
                if existing.can_merge_with(&incoming) && existing.count < max_stack {
                    let moved = (max_stack - existing.count).min(incoming.count);
                    existing.count += moved;
                    incoming.count -= moved;
                    if incoming.count == 0 {
                        return None;
                    }
                }
            }
        }
        for slot in self.hotbar.iter_mut().chain(self.main.iter_mut()) {
            if slot.is_none() {
                let moved = incoming.count.min(max_stack);
                *slot = Some(ItemStack {
                    count: moved,
                    ..incoming
                });
                incoming.count -= moved;
                if incoming.count == 0 {
                    return None;
                }
            }
        }
        Some(incoming)
    }

    pub fn count_item(&self, item: Item) -> u32 {
        self.hotbar
            .iter()
            .chain(self.main.iter())
            .flatten()
            .filter(|stack| stack.item == item)
            .map(|stack| stack.count)
            .sum()
    }

    pub fn remove_one(&mut self, item: Item) -> bool {
        for slot in self.hotbar.iter_mut().chain(self.main.iter_mut()) {
            if slot.is_some_and(|stack| stack.item == item) {
                let stack = slot.as_mut().unwrap();
                if stack.count > 1 {
                    stack.count -= 1;
                } else {
                    *slot = None;
                }
                return true;
            }
        }
        false
    }

    pub fn use_selected_item(&mut self, is_creative: bool) {
        if is_creative {
            return;
        }
        if let Some(stack) = &mut self.hotbar[self.selected] {
            if stack.count > 1 {
                stack.count -= 1;
            } else {
                self.hotbar[self.selected] = None;
            }
        }
    }

    pub fn remove_selected_item(&mut self, count: u32) {
        if let Some(stack) = &mut self.hotbar[self.selected] {
            if stack.count > count {
                stack.count -= count;
            } else {
                self.hotbar[self.selected] = None;
            }
        }
    }

    pub fn replace_selected_item(&mut self, item: Item) {
        self.hotbar[self.selected] = Some(ItemStack::new(item, 1));
    }
}
