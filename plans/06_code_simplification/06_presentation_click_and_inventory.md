# Plan06 — 點擊／物品欄／挖掘 cancel 去重

## 定位

`State::handle_click`（`src/state.rs` ~17225–18464）把同一套 raycast + portal／eye／床／容器／放置／破壞寫了三遍：

1. Join（`!is_authoritative()`，~17226–17339）
2. Embedded（`has_in_process_runtime()` → `handle_authority_click` ~18466–18630）
3. Legacy 本地 mutate（~17347 起約 1,100 行，含藥水／弓／食物）

`handle_inventory_click`（~19076–19652）先對 embedded／join 特判，再把商人 offer hit-test 做第二次（~19080 與 ~19167）。

`update_frame`（~14289–14415）對「目標變了／不可破壞／未擊中／滑鼠放開」各寫一次 `submit_local_authority_block_action(CancelBreak, …)` + 清 `mining_target`／`held`／`progress`。`mining_cancel_sent`（~5742）用來壓重複封包，抽 helper 時必須留下。

本計劃只搬控制流，不改拓撲真值表。真值表由 02 的 enum 擁有。

## 前置

- 02 已合併：呼叫端使用單一拓撲 enum，不再靠呼叫端手寫 `a && !b`。

若 02 未合併：停止實作。不要在三條 click 路徑上再疊一層布林 helper。

## 精確 acceptance

- [ ] 世界點擊的 **hit 解析**只有一處：回傳窄 enum（例如 `StartBreak`、`CancelBreak`、`Place`、`IgnitePortal`、`InsertEnderEye`、`OpenContainer`、`Sleep`、`UseItem`…）。Join 與 Embedded 共用這個 resolver，再分別 map 到封包／`submit_local_authority_block_action`。
- [ ] Join 與 Embedded 在下列邊緣保持現況（執行前用測試或註解鎖定，不得「對齊」）：
  - Embedded 會送 `Action::Break`／`Place`；Join 的 `open_chest` 等投影
  - 空手放置是否送出（掃描時為 Embedded-only）
- [ ] Legacy item-use 臂（藥水／弓／食物…）整段移到名為 `legacy_*` 的函式，檔案可仍在 `state.rs`。現役 Embedded／Join 不得掉進這段。
- [ ] `handle_inventory_click`：hit-test 一次，得到 `InventoryHit { Merchant, RecipeBook, Enchant, Slot, Empty }`，再 `match (topology, hit)`。Embedded 玩家物品欄 writeback 例外（policy 已測）不得丟。Join 不得消耗／掉落。
- [ ] 連續挖掘的 CancelBreak + 清 latch 收到 `cancel_authority_break(&mut self)`（或等價）。四個呼叫點共用。`mining_cancel_sent` 語意不變。
- [ ] 不得把 leftover 本地 mutate 刪掉；只隔離。
- [ ] Plan31、container、embedded presentation、join projection 測試期望值不變。

## 預計檔案與測試

- 修改：`src/state.rs`（`handle_click`、`handle_authority_click`、`handle_inventory_click`、`update_frame` 挖掘段）。必要時新增 `src/presentation_click.rs` 放純 resolver（無 GPU／無 State 欄位）。
- 測試：
  - `cargo test --lib presentation_inventory_policy::`
  - `cargo test --test plan31_authoritative_block_actions -- --test-threads=1`
  - `cargo test --test review_hardening_container_click -- --test-threads=1`
  - `cargo test --test review_hardening_embedded_presentation -- --test-threads=1`
  - `cargo test --test review_hardening_join_projection -- --test-threads=1`
  - `cargo test --test plan34_container_break_inventory_conservation -- --test-threads=1`
  - `cargo check --bin icraft`

## 建議階段

1. 把 Join 臂與 `handle_authority_click` 並排讀完，列出差異表（封包型別、空手配對、容器等待）。差異表寫進證據。
2. 抽純 `resolve_world_click(...)`，用現有行為當 oracle：先讓 Join／Embedded 呼叫 resolver，本體仍用舊分支，對一下結果（可用 debug assert 或單元測）。
3. 刪重複分支，只留 map-to-submit。
4. 隔離 legacy item-use。
5. inventory hit-test 一次。
6. `cancel_authority_break`。
7. 跑窄測試。

## 不在本計劃

- 拆 `State::render`／`State::new`（10）。
- 刪 leftover 世界擁有者。
- 改 reach、LOS、BlockAction 驗證。
- 把 Host 的 `NetworkHandle` 廣播從 `State` 搬走（證據可列）。
