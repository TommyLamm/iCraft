//! Inventory SlotType, layout geometry, and slot get/set (Plan 27).
//! Child of `state` via `#[path]` so methods can see private `State` fields.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotType {
    Creative(Item),
    Hotbar(usize),
    Backpack(usize),
    Armor(usize),
    Offhand,
    CraftInput(usize),
    CraftOutput,
    EnchantInput,
    EnchantLapis,
    BrewBottle(usize),
    BrewIngredient,
    ContainerSlot(usize),
    AnvilLeft,
    AnvilRight,
    AnvilOutput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InventoryLayoutKind {
    CreativeCatalog,
    Standard,
}

pub(super) type InventoryUiRect = MenuRect;

pub(super) fn presentation_target_for_slot(slot: SlotType) -> PresentationInventoryTarget {
    match slot {
        SlotType::ContainerSlot(_) => PresentationInventoryTarget::ContainerSlot,
        SlotType::AnvilOutput | SlotType::EnchantInput | SlotType::EnchantLapis => {
            PresentationInventoryTarget::Workstation
        }
        _ => PresentationInventoryTarget::PlayerInventory,
    }
}

pub(super) fn presentation_target_for_authority_hit(
    hit: InventoryHit<SlotType>,
) -> Option<PresentationInventoryTarget> {
    match hit {
        InventoryHit::Slot(slot) => Some(presentation_target_for_slot(slot)),
        InventoryHit::Enchant { .. }
        | InventoryHit::RecipeBook
        | InventoryHit::RecipeBookToggle => Some(PresentationInventoryTarget::Workstation),
        InventoryHit::Merchant { .. } | InventoryHit::CreativeTab { .. } | InventoryHit::Empty => {
            None
        }
    }
}

pub(super) fn inventory_layout_kind(
    game_mode: GameMode,
    station_open: bool,
    crafting_table_open: bool,
) -> InventoryLayoutKind {
    if game_mode == GameMode::Creative && !station_open && !crafting_table_open {
        InventoryLayoutKind::CreativeCatalog
    } else {
        InventoryLayoutKind::Standard
    }
}

pub(super) fn creative_slot_metrics(aspect: f32) -> (f32, f32, f32, f32) {
    let safe_aspect = aspect.max(0.1);
    let slot_w = 0.08_f32.min(0.15 / safe_aspect);
    let slot_h = slot_w * safe_aspect;
    let gap = 0.01;
    let grid_w = CREATIVE_COLUMNS as f32 * slot_w + (CREATIVE_COLUMNS - 1) as f32 * gap;
    let start_x = -grid_w / 2.0;
    (slot_w, slot_h, gap, start_x)
}

pub(super) fn creative_catalog_slot_rect(index: usize, aspect: f32) -> InventoryUiRect {
    let (slot_w, slot_h, gap, start_x) = creative_slot_metrics(aspect);
    let row = index / CREATIVE_COLUMNS;
    let column = index % CREATIVE_COLUMNS;
    let x0 = start_x + column as f32 * (slot_w + gap);
    let y1 = 0.64 - row as f32 * (slot_h + gap);
    InventoryUiRect {
        x0,
        x1: x0 + slot_w,
        y0: y1 - slot_h,
        y1,
    }
}

pub(super) fn creative_hotbar_slot_rect(index: usize, aspect: f32) -> InventoryUiRect {
    let (slot_w, slot_h, gap, start_x) = creative_slot_metrics(aspect);
    let x0 = start_x + index as f32 * (slot_w + gap);
    InventoryUiRect {
        x0,
        x1: x0 + slot_w,
        y0: -0.85,
        y1: -0.85 + slot_h,
    }
}

pub(super) fn creative_tab_rect(index: usize) -> InventoryUiRect {
    let width = 0.125;
    let gap = 0.005;
    let start_x = -(CreativeTab::TABS.len() as f32 * width
        + (CreativeTab::TABS.len() - 1) as f32 * gap)
        / 2.0;
    let x0 = start_x + index as f32 * (width + gap);
    InventoryUiRect {
        x0,
        x1: x0 + width,
        y0: 0.78,
        y1: 0.88,
    }
}

pub(super) fn creative_scroll_track_rect(aspect: f32) -> InventoryUiRect {
    let first = creative_catalog_slot_rect(0, aspect);
    let last = creative_catalog_slot_rect(CREATIVE_VISIBLE_SLOTS - 1, aspect);
    InventoryUiRect {
        x0: first.x0
            + CREATIVE_COLUMNS as f32
                * (creative_slot_metrics(aspect).0 + creative_slot_metrics(aspect).2)
            - creative_slot_metrics(aspect).2
            + 0.02,
        x1: first.x0
            + CREATIVE_COLUMNS as f32
                * (creative_slot_metrics(aspect).0 + creative_slot_metrics(aspect).2)
            - creative_slot_metrics(aspect).2
            + 0.045,
        y0: last.y0,
        y1: first.y1,
    }
}

impl State {
    pub fn is_creative_catalog_open(&self) -> bool {
        self.inventory.is_open
            && inventory_layout_kind(
                self.game_mode,
                self.active_station.is_some(),
                self.inventory.is_table_open,
            ) == InventoryLayoutKind::CreativeCatalog
    }

    pub fn fill_inventory_slots(&mut self) {
        let mut slots = std::mem::take(&mut self.inventory_slots_scratch);
        self.write_inventory_slot_rects(&mut slots);
        self.inventory_slots_scratch = slots;
    }

    pub(super) fn write_inventory_slot_rects(&self, slots: &mut Vec<(SlotType, f32, f32, f32, f32)>) {
        slots.clear();
        let aspect = self.size.width as f32 / self.size.height as f32;
        if inventory_layout_kind(
            self.game_mode,
            self.active_station.is_some(),
            self.inventory.is_table_open,
        ) == InventoryLayoutKind::CreativeCatalog
        {
            for (index, item) in self
                .inventory
                .creative_visible_items()
                .into_iter()
                .enumerate()
            {
                let rect = creative_catalog_slot_rect(index, aspect);
                slots.push((SlotType::Creative(item), rect.x0, rect.x1, rect.y0, rect.y1));
            }
            for index in 0..9 {
                let rect = creative_hotbar_slot_rect(index, aspect);
                slots.push((SlotType::Hotbar(index), rect.x0, rect.x1, rect.y0, rect.y1));
            }
            return;
        }

        let slot_w = 0.08;
        let slot_h = 0.08 * aspect;
        let gap = 0.01;

        // 1. Hotbar (0..9)
        for i in 0..9 {
            let x0 = -0.40 + i as f32 * (slot_w + gap);
            let y0 = -0.85;
            slots.push((SlotType::Hotbar(i), x0, x0 + slot_w, y0, y0 + slot_h));
        }

        // 2. Backpack (0..27)
        for r in 0..3 {
            for c in 0..9 {
                let i = r * 9 + c;
                let x0 = -0.40 + c as f32 * (slot_w + gap);
                let y0 = -0.70 + r as f32 * (slot_h + gap);
                slots.push((SlotType::Backpack(i), x0, x0 + slot_w, y0, y0 + slot_h));
            }
        }

        // 3. Armor (0..4)
        for i in 0..4 {
            let x0 = -0.40;
            let y0 = -0.15 + i as f32 * (slot_h + gap);
            slots.push((SlotType::Armor(i), x0, x0 + slot_w, y0, y0 + slot_h));
        }

        // 3b. Offhand
        let offhand_x0 = -0.40 + 1.2 * (slot_w + gap);
        let offhand_y0 = -0.15;
        slots.push((
            SlotType::Offhand,
            offhand_x0,
            offhand_x0 + slot_w,
            offhand_y0,
            offhand_y0 + slot_h,
        ));

        // 4. Container slots (if chest or furnace is open)
        if let Some(pos) = self.container_target {
            let block = self.chunk_manager.get_block(pos.0, pos.1, pos.2);
            if matches!(block, BlockType::Furnace) {
                let in_x0 = -0.15;
                let in_y0 = 0.10;
                slots.push((
                    SlotType::ContainerSlot(0),
                    in_x0,
                    in_x0 + slot_w,
                    in_y0,
                    in_y0 + slot_h,
                ));

                let fuel_x0 = -0.15;
                let fuel_y0 = -0.10;
                slots.push((
                    SlotType::ContainerSlot(1),
                    fuel_x0,
                    fuel_x0 + slot_w,
                    fuel_y0,
                    fuel_y0 + slot_h,
                ));

                let out_x0 = 0.15;
                let out_y0 = 0.0;
                slots.push((
                    SlotType::ContainerSlot(2),
                    out_x0,
                    out_x0 + slot_w,
                    out_y0,
                    out_y0 + slot_h,
                ));
            } else {
                let container_slots = self.chunk_manager.container_slot_count(pos.0, pos.1, pos.2);
                let container_rows = container_slots.saturating_add(8) / 9;
                let x_start = -0.40;
                let y_start = -0.70 - (container_rows as f32) * (slot_h + gap) - gap;
                for r in 0..container_rows {
                    for c in 0..9 {
                        let i = r * 9 + c;
                        let x0 = x_start + c as f32 * (slot_w + gap);
                        let y0 = y_start + r as f32 * (slot_h + gap);
                        slots.push((SlotType::ContainerSlot(i), x0, x0 + slot_w, y0, y0 + slot_h));
                    }
                }
            }
        }

        // 5. Crafting Grid & Output
        if self.container_target.is_none()
            && self.active_station.is_none()
            && self.inventory.is_table_open
        {
            // 3x3 table
            let x_start = -0.05;
            for r in 0..3 {
                for c in 0..3 {
                    let i = r * 3 + c;
                    let x0 = x_start + c as f32 * (slot_w + gap);
                    let y0 = -0.10 + r as f32 * (slot_h + gap);
                    slots.push((SlotType::CraftInput(i), x0, x0 + slot_w, y0, y0 + slot_h));
                }
            }
            // Output
            let x0 = x_start + 3.0 * (slot_w + gap) + 0.06;
            let y0 = -0.10 + 1.0 * (slot_h + gap);
            slots.push((SlotType::CraftOutput, x0, x0 + slot_w, y0, y0 + slot_h));
        } else if self.active_station.is_none() {
            // 2x2 player craft
            let x_start = 0.05;
            for r in 0..2 {
                for c in 0..2 {
                    let i = r * 2 + c;
                    let x0 = x_start + c as f32 * (slot_w + gap);
                    let y0 = -0.05 + r as f32 * (slot_h + gap);
                    slots.push((SlotType::CraftInput(i), x0, x0 + slot_w, y0, y0 + slot_h));
                }
            }
            // Output
            let x0 = x_start + 2.0 * (slot_w + gap) + 0.06;
            let y0 = -0.05 + 0.5 * (slot_h + gap);
            slots.push((SlotType::CraftOutput, x0, x0 + slot_w, y0, y0 + slot_h));
        }

        match self.active_station {
            Some(StationKind::Enchanting) => {
                slots.push((
                    SlotType::EnchantInput,
                    -0.18,
                    -0.18 + slot_w,
                    0.12,
                    0.12 + slot_h,
                ));
                slots.push((
                    SlotType::EnchantLapis,
                    -0.18,
                    -0.18 + slot_w,
                    -0.02,
                    -0.02 + slot_h,
                ));
            }
            Some(StationKind::Brewing) => {
                for i in 0..3 {
                    let x0 = -0.18 + i as f32 * (slot_w + gap);
                    slots.push((
                        SlotType::BrewBottle(i),
                        x0,
                        x0 + slot_w,
                        -0.02,
                        -0.02 + slot_h,
                    ));
                }
                slots.push((
                    SlotType::BrewIngredient,
                    -0.09,
                    -0.09 + slot_w,
                    0.17,
                    0.17 + slot_h,
                ));
            }
            Some(StationKind::Anvil) => {
                slots.push((
                    SlotType::AnvilLeft,
                    -0.20,
                    -0.20 + slot_w,
                    0.10,
                    0.10 + slot_h,
                ));
                slots.push((
                    SlotType::AnvilRight,
                    -0.05,
                    -0.05 + slot_w,
                    0.10,
                    0.10 + slot_h,
                ));
                slots.push((
                    SlotType::AnvilOutput,
                    0.20,
                    0.20 + slot_w,
                    0.10,
                    0.10 + slot_h,
                ));
            }
            None | Some(StationKind::Merchant) => {}
        }

    }

    pub fn get_inventory_slots(&self) -> Vec<(SlotType, f32, f32, f32, f32)> {
        let mut slots = Vec::with_capacity(64);
        self.write_inventory_slot_rects(&mut slots);
        slots
    }

    pub fn get_item_at_slot(&self, slot: SlotType) -> Option<ItemStack> {
        match slot {
            SlotType::Creative(item) => Some(ItemStack::new(item, 1)),
            SlotType::Hotbar(i) => self.inventory.hotbar[i],
            SlotType::Backpack(i) => self.inventory.main[i],
            SlotType::Armor(i) => self.inventory.armor[i],
            SlotType::Offhand => self.inventory.offhand,
            SlotType::CraftInput(i) => self.inventory.craft_input.get(i).copied().flatten(),
            SlotType::CraftOutput => self.inventory.craft_output,
            SlotType::EnchantInput => self.enchanting.input,
            SlotType::EnchantLapis => self.enchanting.lapis,
            SlotType::BrewBottle(i) => self.brewing.bottles[i],
            SlotType::BrewIngredient => self.brewing.ingredient,
            SlotType::AnvilLeft => self.anvil.left,
            SlotType::AnvilRight => self.anvil.right,
            SlotType::AnvilOutput => self.anvil.output,
            SlotType::ContainerSlot(i) => self.container_target.and_then(|pos| {
                self.chunk_manager
                    .container_slots(pos.0, pos.1, pos.2)
                    .and_then(|slots| slots.get(i).copied().flatten())
            }),
        }
    }

    pub fn set_item_at_slot(&mut self, slot: SlotType, stack: Option<ItemStack>) {
        if self.presentation_topology().is_join_client() {
            return;
        }
        // Presentation never mutates authority-owned container slots locally;
        // container clicks go through GameplayOperation::ContainerClick.
        if matches!(slot, SlotType::ContainerSlot(_)) {
            return;
        }
        match slot {
            SlotType::Creative(item) => self.inventory.write_creative_slot(item, stack),
            SlotType::Hotbar(i) => self.inventory.hotbar[i] = stack,
            SlotType::Backpack(i) => self.inventory.main[i] = stack,
            SlotType::Armor(i) => self.inventory.armor[i] = stack,
            SlotType::Offhand => self.inventory.offhand = stack,
            SlotType::CraftInput(i) => {
                if i < self.inventory.craft_input.len() {
                    self.inventory.craft_input[i] = stack;
                }
            }
            SlotType::CraftOutput => self.inventory.craft_output = stack,
            SlotType::EnchantInput => self.enchanting.input = stack,
            SlotType::EnchantLapis => self.enchanting.lapis = stack,
            SlotType::BrewBottle(i) => self.brewing.bottles[i] = stack,
            SlotType::BrewIngredient => self.brewing.ingredient = stack,
            SlotType::AnvilLeft => self.anvil.left = stack,
            SlotType::AnvilRight => self.anvil.right = stack,
            SlotType::AnvilOutput => {}
            SlotType::ContainerSlot(_) => {}
        }
    }

    pub fn handle_swap_offhand_pressed(&mut self) {
        let _ = (self.is_chat_open, self.is_paused, self.player_state.is_dead);
    }

    pub fn select_hotbar_slot(&mut self, slot: usize) {
        self.inventory.selected = slot.min(8);
        if self.presentation_topology().should_sync_inventory() {
            self.sync_authority_gameplay_from_local();
        }
    }

    pub(super) fn refresh_workstations(&mut self) {
        self.enchanting.refresh();
        self.anvil.refresh();
    }

    pub(super) fn resolve_inventory_hit(
        &self,
        is_left: bool,
    ) -> Option<(PresentationInventoryTarget, InventoryHit<SlotType>)> {
        let hit = self.probe_inventory_click(is_left).authority_hit();
        presentation_target_for_authority_hit(hit).map(|target| (target, hit))
    }

    pub(super) fn presentation_inventory_click_target(&self) -> Option<PresentationInventoryTarget> {
        // Writeback has no mouse-button; overlay geometry matches the historical
        // always-on hit test (`collect_inventory_ui_hits` with `is_left = true`).
        self.resolve_inventory_hit(true).map(|(target, _)| target)
    }

    pub(crate) fn should_writeback_after_inventory_click(&self) -> bool {
        self.presentation_topology()
            .should_writeback_after_inventory_click(self.presentation_inventory_click_target())
    }

    pub(super) fn probe_inventory_click(&self, is_left: bool) -> InventoryHitProbe<SlotType> {
        let mouse_x = self.mouse_ndc[0];
        let mouse_y = self.mouse_ndc[1];
        let ui = collect_inventory_ui_hits(
            mouse_x,
            mouse_y,
            is_left,
            self.active_station == Some(StationKind::Merchant),
            self.active_merchant_offers.len(),
            self.recipe_book_open,
            self.active_station == Some(StationKind::Enchanting),
        );
        let creative_tab = if self.is_creative_catalog_open() && is_left {
            (0..CreativeTab::TABS.len())
                .find(|&index| creative_tab_rect(index).contains(mouse_x, mouse_y))
        } else {
            None
        };
        let slot = self
            .get_inventory_slots()
            .into_iter()
            .find(|&(_, x0, x1, y0, y1)| {
                mouse_x >= x0 && mouse_x <= x1 && mouse_y >= y0 && mouse_y <= y1
            })
            .map(|(slot, _, _, _, _)| slot);
        InventoryHitProbe {
            merchant: ui.merchant,
            creative_tab,
            recipe_book_toggle: ui.recipe_book_toggle,
            recipe_book: ui.recipe_book,
            enchant: ui.enchant,
            slot,
        }
    }
}

