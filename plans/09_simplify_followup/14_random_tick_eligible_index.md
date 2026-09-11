# Plan14 — random-tick eligible 索引

## 定位

`sample_random_ticks_in_columns` 每 50 ms 掃描 simulation union 全部 column／section，收集 `random_tick_count() > 0`，`sort_unstable`，再取 128。simulation 半徑 32 時 eligible 可達數千。Wave 08 已改 simulation-union（非全 residency），但仍每 tick 全 union 掃描。

權威入口傳 `Some(columns)`。`tick_all_loaded_*`／`columns == None` 是測試／leftover wrapper，可在本計劃改測試後刪 wrapper，但 **不要** 把 `sim_harness` 整包刪掉。

## 前置

無。決定性 rng／每 section 3 samples 必須保留。

## 精確 acceptance

- [ ] eligible section 列表在 column load／unload 或 `random_tick_count` 變更時增量維護；每 tick 不重掃+排序整個 union。
- [ ] 每 tick 仍決定性取出至多 128 個 section 樣本（既有 rng 序列語意：若 backlog 旋轉改變分佈，必須更新並說明 determinism 測試）。
- [ ] `do_fire_tick == false` 時仍過濾 Fire。
- [ ] 權威不走 `columns == None` 全表掃描；該分支僅測試則刪或 `#[cfg(test)]`。
- [ ] `cargo test --lib world_tick::` 與依賴 random tick 的 crop／fire 測試通過。

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
