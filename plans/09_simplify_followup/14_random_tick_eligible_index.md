# Plan14 — random-tick eligible 索引

## 定位

`sample_random_ticks_in_columns` 每 50 ms 掃描 simulation union 全部 column／section，收集 `random_tick_count() > 0`，`sort_unstable`，再取 128。simulation 半徑 32 時 eligible 可達數千。Wave 08 已改 simulation-union（非全 residency），但仍每 tick 全 union 掃描。

權威入口傳 `Some(columns)`。`tick_all_loaded_*`／`columns == None` 是測試／leftover wrapper，可在本計劃改測試後刪 wrapper，但 **不要** 把 `sim_harness` 整包刪掉。

## 前置

無。決定性 rng／每 section 3 samples 必須保留。

## 精確 acceptance

- [x] eligible section 列表在 column load／unload 或 `random_tick_count` 變更時增量維護；每 tick 不重掃+排序整個 union。
- [x] 每 tick 仍決定性取出至多 128 個 section 樣本（既有 rng 序列語意：若 backlog 旋轉改變分佈，必須更新並說明 determinism 測試）。
- [x] `do_fire_tick == false` 時仍過濾 Fire。
- [x] 權威不走 `columns == None` 全表掃描；該分支僅測試則刪或 `#[cfg(test)]`。
- [x] `cargo test --lib world_tick::` 與依賴 random tick 的 crop／fire 測試通過。

## 預計檔案與測試

- `src/world_tick.rs`、`src/server_world.rs`（load／unload 時維護索引）
- 驗證：既有 random tick 測試；必要時加「unload 後不再抽到該欄」

## 建議階段

1. 在 `ServerWorld` 或 tick 模組加 eligible 索引／cursor。
2. load／set_block／unload 維護。
3. 刪 `sample_all_loaded_*` 若測試已改傳 explicit set。

## 不在本計劃

- hopper 全表掃描改 index（Wave 08 已有 cooldown dirty；完整 hopper index 另題）。
- 刪 `SimHarness`。
- 改每 section sample 次數。

## 實作與證據

### 改了什麼

- 在 `Chunk` 加 `random_tick_sections: Vec<i8>`（升序 `section_y`），與 furnace／torch 索引同模式：
  - `set_block_local` 依 `random_tick_count` 0↔>0 增量插入／移除
  - generation／restore 呼叫 `rebuild_random_tick_index`
  - unload 隨 column 一起丟棄
- `sample_random_ticks_in_columns` 改為必填 `&BTreeSet<(i32,i32)>`，只讀各 column 的索引組出已排序的 `(cx,cz,sec_y)`，不再掃 section、不再 `sort_unstable`。
- 刪除無呼叫端的 `sample_all_loaded_random_ticks`（`columns == None` 全表掃描）。
- `ServerWorld::tick` 傳 `&simulation_chunks`；`do_fire_tick == false` 的 Fire retain 過濾保留不變。
- 決定性語意：simulation union 路徑本來就是 BTreeSet 欄序 + 升序 section；索引路徑與舊「掃完再 sort」在該路徑上等價，**未**引入 backlog 旋轉，rng salt 的 `i` 序不變。

### 測了什麼

- `cargo test --lib world_tick::` — 22 passed
- `cargo test --lib random_tick` — 含
  - `random_tick_index_tracks_section_eligibility`
  - `sample_random_ticks_uses_chunk_eligible_index`（unload 後不再抽到該欄）
  - `sample_random_ticks_preserves_ordered_section_budget`
  - `do_fire_tick_false_filters_fire_random_ticks`

### 留下的缺口

- 未做 hopper 全表 index（計劃明確排除）。
- 未刪 `SimHarness`；harness 仍走 `evaluate_random_tick_at` 單點路徑。
- 每 section 仍固定 3 samples（計劃要求保留）。
