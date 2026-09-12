# Plan07 — `State` 點擊與挖礦掉落去重

## 定位

在 `State` 表現層代碼中，存在兩處顯著的重複邏輯：

1. **重複的挖礦獎勵計算 (`src/state.rs:L14833-15024`, ~192 行)**：
   `State` 中定義了獨立的 `BlockBreakRewards` 結構體與 192 行的 `calculate_block_break_rewards` 函式。而完全相同的結構體與函式已存在於權威端的 `src/authority/mining.rs:L8-156`。
   表現層應直接調用 `crate::authority::mining::calculate_block_break_rewards` 或由權威投射結果更新，而非在表現層重複維持一份 190 行的方塊掉落表。

2. **點擊意圖分派重複 (`src/state.rs:L11847-12020`, ~60 行)**：
   `handle_join_world_click` 與 `handle_authority_click` 在 90% 的 `BlockAction` 分支上（`StartBreak`, `IgnitePortal`, `InsertEnderEye`, `Place`）執行完全相同的 `submit_local_authority_block_action` 請求構建與校驗邏輯。
   可將共通分支抽取為單一 `handle_live_world_click` 輔助函式。

預期削減代碼 ~250 行。

## 前置

01（`State` 廢棄管線與緩衝清理）。建議與 08 串行處理 `src/state.rs`。

## 精確 acceptance

- [ ] 移除 `src/state.rs` 中的 `BlockBreakRewards` 結構體與 `calculate_block_break_rewards` 實現。
- [ ] 將 `src/state.rs` 中的相關調用改為使用 `crate::authority::mining::calculate_block_break_rewards`。
- [ ] 整合 `handle_join_world_click` 與 `handle_authority_click` 的重複方塊操作分派分支。
- [ ] 確保 Join Client 與 Host 在方塊點擊（放置、破壞、點火、插眼）時的意圖發送與行為完全一致。
- [ ] `cargo check --all-targets` 通過。
- [ ] 相關點擊與物品欄測試全數通過。

## 預計檔案與測試

- 修改：
  - `src/state.rs`
  - `src/authority/mining.rs`（若需要調整 pub 可見性）
- 驗證測試：
  - `cargo test --lib authority::mining`
  - `cargo test --lib presentation_inventory_policy::`
  - `cargo test --test review_hardening_block_actions -- --test-threads=1`
  - `cargo check --all-targets`

## 建議階段

1. 將 `src/authority/mining.rs` 中的 `calculate_block_break_rewards` 設為 `pub(crate)`。
2. 刪除 `src/state.rs` 中的重複實現並改為引用。
3. 整合 `handle_join_world_click` 與 `handle_authority_click`。
4. 運行方塊操作整合測試。

## 不在本計劃

- 修改方塊破壞或掉落的掉落物生成公式。
- 隔離 `state.rs` 中的 legacy 模擬代碼（此為 Plan 14）。
