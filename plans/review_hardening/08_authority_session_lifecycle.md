# Plan08 — 權威 session 生命週期

## 定位

審查確認四條生命週期洞，typed 工作站／挖礦不受影響：

1. **End Gateway**（`authority/mod.rs` 約 926）：同維度傳送只改 `session.position` 並
   insert `pending_session_revisions`，不 bump gameplay revision、不設
   `teleport_allowance`。下一請求開頭會清掉 pending。`handle_position` 用 100 block/s
   拒掉 1035 格 hop → 權威在外島、runtime 仍在噴泉。Nether／End **傳送門**走
   `execute_portal_dimension_transfer`，這條是例外。
2. **`ClientRespawnRequest`**（`server_runtime.rs` 約 1479）：不要求 `is_dead`。活著的
   客戶端可瞬移到世界出生點、清 fishing／mining／mount、滿血。runtime pose 寫在
   `set_session_dimension` 之前，core 失敗會分叉。
3. **`IgnitePortal`／`InsertEnderEye`**（約 1699）：先 `set_block`（可遞迴放傳送門內部）
   再扣物品。`submit_request` 在 `Err` 時仍 `take_pending_mutations()`。`Place` 已是
   先 clone 扣款再 apply。
4. **存檔／關閉**：autosave `save_all()?` 在 dedicated `tick()?` 裡，失敗會跳過
   `shutdown()` flush。`handle_leave` 先 `save_player?` 再 `remove_session`，存檔失敗
   讓該名稱直到重啟都登不進去。`request_shutdown` 設 `stopped`，`shutdown()` 見
   `stopped` 就 return。

## 前置

無。

## 精確 acceptance

- [ ] End Gateway：與傳送門相同發布模型——更新權威 pose、bump `gameplay.revision`、
  設 teleport allowance、在下一筆 inbound pose 前進投影／校正。頭測：`EnterPortal`
  在閘門上，下一筆目的地 pose 被接受，runtime 與權威座標一致。
- [ ] `respawn_session` 在 `!is_dead`（且非 hardcore spectator 規則若已存在）時回 false。
  活著的 `ClientRespawnRequest` 不改 pose／dimension／物品欄。runtime 只在兩個
  authority 呼叫都成功後才寫。
- [ ] `IgnitePortal`／`InsertEnderEye`：先 clone gameplay 並扣精確持有槽（含 brew-lock），
  再 `set_block`。失敗不得留下 Fire／傳送門內部，不得 drain 成已接受的 pending mutation。
- [ ] Autosave 錯誤不得讓 dedicated `tick` 回 Err 而跳過 `shutdown`。`shutdown()` 在
  `stopped` 已為真時，若尚未成功 flush，仍必須嘗試 `save_all`。
- [ ] `handle_leave`：無論 `save_player` 成敗都 `remove_session`。同一正規化名稱可再
  `login_session`。
- [ ] 測試覆蓋以上五條（gateway、alive-respawn、ignite 扣款失敗、autosave 錯仍 flush、
  leave-save 失敗仍能重登）。可用 `#[cfg(test)]` failpoint，不要真的打滿磁碟。

## 預計檔案與測試

- 修改：`src/authority/mod.rs`、`src/server_runtime.rs`、`src/bin/icraft-server.rs`
  （若 `tick()?` 要改成記錄 save 錯）。
- 測試：`tests/plan32_progression_travel.rs` 擴 gateway，或
  `tests/review_hardening_session_lifecycle.rs`；`src/server_runtime.rs` 單元測 leave／shutdown。

## 建議階段

1. Gateway 接到既有 transfer／teleport intent。
2. Respawn 加 `is_dead` 前置，調整寫入順序。
3. Ignite／Eye 改成 Place 交易形狀。
4. Save／leave／shutdown 改成 best-effort persist + 必定釋放身份。
5. 五條窄測試。

## 不在本計劃

- 玩家檔名單射（04）。
- Region zlib 語義（05）。
- Chunk evict（11）。

## 實作與證據

### 行為

- End Gateway 同維度 hop 改走既有 `execute_portal_dimension_transfer`：更新權威 pose、送出 transfer intent（runtime 設 `teleport_allowance` 並前進投影）、再 allocate `gameplay.revision`／`last_revision`。下一筆目的地 pose 不再被 100 block/s 門檻拒掉。
- `respawn_session` 在 `!is_dead` 時回 false。`ClientRespawnRequest` 先檢查 `is_dead`，兩個 authority 呼叫都成功後才寫 runtime pose／dimension／物品欄；活著的請求不改狀態。
- `IgnitePortal`／`InsertEnderEye` 先 clone gameplay 扣持有槽（含 brew-lock），再 `set_block`。扣款失敗不放 Fire／傳送門內部，也不留下 pending mutation。
- Dedicated `tick` 的 autosave 改成 log 錯誤，不再 `?` 中止而跳過 `shutdown`。`shutdown()` 在 `stopped` 已為真但尚未成功 flush 時仍呼叫 `save_all`。`Drop` 以 `save_flushed` 為準。
- `handle_leave` 無論 `save_player` 成敗都 `remove_session`，同一正規化名稱可再 `login_session`。Leave 存檔失敗只記 log，不讓 tick 回 Err。

### 測試

- `cargo test --test review_hardening_session_lifecycle -- --nocapture`：3 passed（gateway hop + dest pose、alive-respawn 不變、ignite brew-lock 不放 Fire）。
- `cargo test --lib autosave_error_does_not_skip_shutdown_flush`：1 passed。
- `cargo test --lib leave_save_failure_still_releases_identity`：1 passed。
- `cargo test --lib respawn_session_rejects_living_player`：1 passed。
- `cargo test --lib respawn_updates_authority_dimension_and_position`：1 passed。
- `cargo test --lib respawn_command_restores_authority_health_after_death`：1 passed。
- `cargo check --all-targets`：通過。

### leftover gaps

- `submit_request` 在 `Err` 時仍 `take_pending_mutations()`。Ignite／Eye 已改成先扣款再 `set_block`，失敗路徑不再產生 mutation；其他 domain 若仍先 mutate 再失敗，仍可能 drain。不在本計劃。
- 沒有獨立的 `InsertEnderEye` 扣款失敗測試；與 Ignite 共用同一交易形狀。
- `icraft-server.rs` 的 `tick()?` 未改：autosave 不再讓 `tick` 回 Err。
