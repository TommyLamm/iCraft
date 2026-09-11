# Plan03 — 刪 `State` leftover 本地突變與 join 端本地模擬

## 定位

09 波 03 刪的是 `!in_process_authority` 啟動分支與 Pickup 死路。`State` 還有五條 renderer-owned 模擬仍在 live 路徑：

1. **Q 鍵本地丟物**：`app.rs` 631–638 → `drop_held_item`／`drop_hovered_item`（`state.rs` 7496–7584）本地 `spawn` `DroppedItem`、改寫 hotbar／dragged／slots，**從不送 `GameplayOperation`**。撿取已被 policy 永遠 `Reject`（09 波 03），所以這些是永遠撿不到的幽靈實體；join 端物品欄與 session 分叉。`spawn_dropped_item` 另一個 caller 是死的 `BlockActionResult` 臂（`network_event.rs` 223，09 波 01 刪）。
2. **每 tick 環境傷害掃描**：`tick_simulation` 6549–6620 每 tick 掃虛空 Y、岩漿腳／眼、然後 AABB × 仙人掌雙層迴圈；`take_damage`（7605–7631）embedded 送 `Combat`，**join 是 `let _ = (amount, …)`**。權威已擁有 `PlayerHealth`。
3. **`switch_dimension` 本地 worldgen**：join 提前 return（1274–1279）；embedded 仍 `generate_chunk_with_options` + `propagate_chunk_lighting`（1320–1337）+ `println!`（1348）。`reset_presented_dimension`（1355）是正確的投影 teardown，不生成。
4. **join 端本地 brew／effects／時間**：`tick_simulation` 6282–6285、6354–6362 在 `!has_in_process_runtime` 時跑 `brewing.update`、`potion_effects.update`、`do_daylight_cycle` 本地時間；`keys.f` 60× 時間加速（6348，`app.rs` 683）**編進生產**。這些都已由 `PlayerEffect`／`TimeSync`／session update 投影。
5. **F3 存檔計數硬寫零**：`frame.rs` 49–54 每 frame 把 `save_queue_depth`／`save_in_flight*`／`save_drop`／`loaded_region_cache_bytes` 設 0，F3 仍印（2908–2911）。桌面 `State` 沒有 `SaveManager`。

## 前置

09 波 03（同一批 `state.rs` 死路；避免兩個 PR 同時改 `tick_simulation`／`switch_dimension`）。

## 精確 acceptance

- [ ] `drop_held_item`／`drop_hovered_item` 的本地 spawn 與物品欄改寫刪除；Q 鍵改為送一個 authority 掉落 op（若尚無，提交 `Reject`-safe no-op 並在 HUD 提示），或整個拿掉綁鍵。
- [ ] `tick_simulation` 的虛空／岩漿／仙人掌傷害掃描刪除；`take_damage` 刪除或只剩粒子／音效；生命值只從 session 投影來。
- [ ] embedded portal 轉移只走 `reset_presented_dimension`，等 `ChunkData`；`switch_dimension` 不再呼叫 worldgen／lighting／`println!`。
- [ ] join 端不再本地更新 brew／effects／`world_time`；`keys.f` 刪除或 `cfg(debug_assertions)`。
- [ ] `PerfCounters`／`FramePerfSample` 刪存檔佇列欄位與 F3 行（或改從 `embedded_runtime.runtime` 取樣 host 存檔統計）。
- [ ] `cargo test --bin icraft` 通過；被刪路徑的測試改為驗證「不送 op／不改本地世界」。

## 預計檔案與測試

- 改：`src/state.rs`、`src/app.rs`、`src/presentation/frame.rs`、`src/presentation/network_event.rs`
- 驗證：`cargo test --bin icraft`；`tests/review_hardening_embedded_presentation.rs`；`tests/review_hardening_join_projection.rs`；`tests/plan32_progression_travel.rs`（portal 轉移後仍收到目的地 `ChunkData`）

## 建議階段

1. 刪 F3 零計數與 `keys.f`（純刪）。
2. 刪環境傷害掃描與 `take_damage`。
3. `switch_dimension` 改走 `reset_presented_dimension`。
4. 處理 Q-drop；最後刪 `spawn_dropped_item`（需 09 波 01 已刪 `BlockActionResult` 臂）。

## 不在本計劃

- 雙 block 投影（`project_authority_mutations` vs `BlockChange`）—— Plan 15。
- 桌面玩法殼（Plan 02）。
- `State` UI 佈局搬家（Plan 27）。
