# Advancements / Achievement System Implementation Plan (任務 28)

This document provides a step-by-step implementation plan for introducing the **Advancements System** into iCraft.

---

## Task Breakdown & Phasing

### Phase 1: Core Advancement Data Structures & Registry (`src/advancements.rs`)
- **Step 1.1**: Create `src/advancements.rs` defining `Advancement`, `AdvancementCategory`, `AdvancementFrameType`, `AdvancementTrigger`, `AdvancementProgressData`, `ToastNotification`, and `AdvancementManager`.
- **Step 1.2**: Register `src/advancements.rs` module in `src/main.rs`.
- **Step 1.3**: Implement tree initialization with ~50 advancements across 5 categories (`Minecraft`, `Nether`, `TheEnd`, `Adventure`, `Husbandry`).
- **Step 1.4**: Add unit tests in `src/advancements.rs` for tree validation, trigger evaluation, and state updates.

### Phase 2: Persistence Integration (`src/save.rs`)
- **Step 2.1**: Extend `PlayerData` in `src/save.rs` to include `advancements: AdvancementProgressData`.
- **Step 2.2**: Update `PlayerData::from_state` and state restoration in `State::new` / `SaveManager` to serialize/deserialize advancement data seamlessly.

### Phase 3: Gameplay Event Hooking & Trigger Dispatch (`src/state.rs`, `src/inventory.rs`, `src/crafting.rs`, `src/mob.rs`, `src/boss.rs`, `src/brewing.rs`, `src/enchantment.rs`)
- **Step 3.1**: Add `advancement_manager: AdvancementManager` to `State` in `src/state.rs`.
- **Step 3.2**: Add helper method `State::trigger_advancement(&mut self, trigger: AdvancementTrigger)`.
- **Step 3.3**: Hook inventory item additions/pickups (`Inventory::add_item`, `State::spawn_dropped_item`).
- **Step 3.4**: Hook crafting recipe completions (`crafting.rs`).
- **Step 3.5**: Hook block breaking & mining (`State::break_block`).
- **Step 3.6**: Hook mob & boss kills (`mob.rs`, `boss.rs`).
- **Step 3.7**: Hook brewing, enchanting, and dimension transitions (`brewing.rs`, `enchantment.rs`, `State::switch_dimension`).

### Phase 4: UI rendering & Interaction (`src/advancements.rs`, `src/state.rs`, `src/app.rs`)
- **Step 4.1**: Implement top-right Toast notification rendering and animation updates (3-second timer, slide in/out).
- **Step 4.2**: Implement the Advancements GUI window (`AdvancementGui`): category tabs, connected node graph rendering, locked/unlocked visual framing, and hover tooltips.
- **Step 4.3**: Add key binding `L` in `App::window_event` (`src/app.rs`) to toggle Advancements GUI screen.
- **Step 4.4**: Route mouse clicks and dragging events in Advancements GUI mode.

### Phase 5: Verification & Testing
- **Step 5.1**: Run `cargo check --release` and `cargo test` to verify unit test suite.
- **Step 5.2**: Perform manual verification of toasts, key binding `L`, GUI navigation, and world save/load retention.

---

## Affected Files Summary

- **[NEW]** `src/advancements.rs`
- **[MODIFY]** `src/main.rs`
- **[MODIFY]** `src/save.rs`
- **[MODIFY]** `src/state.rs`
- **[MODIFY]** `src/app.rs`
- **[MODIFY]** `src/inventory.rs`
- **[MODIFY]** `src/crafting.rs`
- **[MODIFY]** `src/mob.rs`
- **[MODIFY]** `src/boss.rs`
- **[MODIFY]** `src/enchantment.rs`
- **[MODIFY]** `src/brewing.rs`
- **[MODIFY]** `ARCHITECTURE.md`
- **[MODIFY]** `plans/progress.md`
