# Plan10 — 漏斗 cooldown dirty、熔爐索引、紅石 sleep

## 定位

漏斗 cooldown 每 tick `mark_block_entity_dirty`（`world_tick.rs` ~525–532），即使沒傳物品。有漏斗的欄幾乎永遠 dirty，autosave／驅逐必刷盤。撿物已改空間查詢（已落地）。

熔爐對 `simulation_chunks` 每個 chunk `iter_block_entities()` 找 `Furnace`，沒有 compact index（torch／redstone 已有）。

紅石 `tick` 在 `sleeping && dirty.is_empty()` 之前仍跑 `sync_loaded_chunks`、`refresh_container_revisions`、`update_observers`（`redstone.rs` ~675–688）。

靜止且 `on_ground` 的 DroppedItem 仍走完整 XYZ 物理。

## 前置

無。08 會放大本計劃的存檔收益。

## 精確 acceptance

- [x] cooldown 倒數不 `mark_dirty`；slot 真的變或 cooldown 從 0→N（傳輸後）才 dirty。重載後 cooldown 語意在證據寫明（可接受「最多早／晚 8 tick」或在 unload 時 flush 一次記憶體值）。
- [x] 熔爐用與 torch 相同的 compact index。
- [x] 紅石把 comparator／observer 刷新移到 early-return **之後**，或只在 container revision 變時喚醒；壓力板占用變化仍能喚醒。
- [x] 靜止落地掉落物跳過物理，直到方塊變更或被推動。
- [x] hopper／furnace／redstone 單元測試通過。

## 預計檔案與測試

- `src/world_tick.rs`、`src/server_world.rs`、`src/redstone.rs`、`src/entity.rs` 物理 tick
- 驗證：`cargo test --lib world_tick::`；`redstone::`；掉落物物理測試

## 建議階段

1. Hopper dirty 策略 + 測試「cooldown 不刷盤、傳輸會刷盤」。
2. Furnace index。
3. 紅石 sleep 順序（用比較器對著箱子的測試鎖行為）。
4. 掉落物睡眠物理。

## 不在本計劃

- Region 寫入批次（08）。
- 重做全世界 cellular automaton。

## 實作與證據

工作樹 `C:\Users\Tommy\Desktop\iCraft-wt-08-10`，分支 `plan/08-10-hopper-furnace-redstone`，起點 `plan/08-09-incremental-checksum` @ `5fcf768`。

### 改了什麼

- Hopper cooldown 倒數不再 `mark_block_entity_dirty`。slot 變更或傳輸後 cooldown `0→8` 才 dirty。
- 熔爐用與 torch 相同的 compact `u32` index（`Furnace`／`FurnaceLit`）；`tick_furnaces` 不再掃全部 block entities。
- 紅石把 comparator／observer 刷新移到 sleeping early-return 之後。壓力板占用變化、container `mark_container_changed`、loaded-chunk 集合變化仍會喚醒。
- 落地且速度近零的 DroppedItem 跳過 XYZ 物理，直到支撐方塊消失或被推動；pickup cooldown 仍倒數。

已更新 `ARCHITECTURE.md`（存檔／tick 契約）。

### 重載後 cooldown 語意

不在 unload 時 flush 記憶體 countdown。最後一次 dirty persist 通常是傳輸後的 `transfer_cooldown = 8`。重載會從 8 再倒，因此漏斗最多晚 8 tick 才再傳。若該欄因其他原因被刷盤，則會寫入當下記憶體值。

### 測了什麼

- `cargo test --lib world_tick::` — 20 passed（含 `hopper_cooldown_countdown_does_not_mark_chunk_dirty`、`hopper_transfer_marks_chunk_dirty_and_arms_cooldown`）
- `cargo test --lib redstone::` — 30 passed（含 `sleeping_skips_comparator_and_observer_refresh`、`container_revision_wakes_sleeping_comparator`、`occupant_movement_wakes_sleeping_pressure_plate_processing`）
- 掉落物物理：`dropped_item_*` 與 `settled_dropped_item_skips_physics_until_support_changes_or_pushed` passed
- `furnace_index_tracks_local_mutations_without_duplicates`、`tick_automation_walks_simulation_columns_not_residency` passed

### 留下的缺口

未做 Region 寫入批次（08）。未把 idle furnace 從每 tick `furnace.tick` 拿掉；index 只省掃描。未在 hopper unload 時 flush cooldown。
