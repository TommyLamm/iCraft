# Plan10 — 漏斗 cooldown dirty、熔爐索引、紅石 sleep

## 定位

漏斗 cooldown 每 tick `mark_block_entity_dirty`（`world_tick.rs` ~525–532），即使沒傳物品。有漏斗的欄幾乎永遠 dirty，autosave／驅逐必刷盤。撿物已改空間查詢（已落地）。

熔爐對 `simulation_chunks` 每個 chunk `iter_block_entities()` 找 `Furnace`，沒有 compact index（torch／redstone 已有）。

紅石 `tick` 在 `sleeping && dirty.is_empty()` 之前仍跑 `sync_loaded_chunks`、`refresh_container_revisions`、`update_observers`（`redstone.rs` ~675–688）。

靜止且 `on_ground` 的 DroppedItem 仍走完整 XYZ 物理。

## 前置

無。08 會放大本計劃的存檔收益。

## 精確 acceptance

- [ ] cooldown 倒數不 `mark_dirty`；slot 真的變或 cooldown 從 0→N（傳輸後）才 dirty。重載後 cooldown 語意在證據寫明（可接受「最多早／晚 8 tick」或在 unload 時 flush 一次記憶體值）。
- [ ] 熔爐用與 torch 相同的 compact index。
- [ ] 紅石把 comparator／observer 刷新移到 early-return **之後**，或只在 container revision 變時喚醒；壓力板占用變化仍能喚醒。
- [ ] 靜止落地掉落物跳過物理，直到方塊變更或被推動。
- [ ] hopper／furnace／redstone 單元測試通過。

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
