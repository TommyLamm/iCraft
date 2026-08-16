# Plan16 — Session 單一同步 helper

## 定位

Plan 09 刪了五個 interest 影子 `HashSet` 與 swapped world slot，並明確留下兩份 session 記錄：

| 欄位 | `SessionContract`（權威） | `PlayerSessionState`（runtime） | 誰是真理 |
| --- | --- | --- | --- |
| pose | `position/yaw/pitch` | `data.position/yaw/pitch` | 寫兩次 |
| dimension | `u8` | `Dimension` | 寫兩次 |
| `last_client_sequence` | 反重放 | 也存一份 | 權威；runtime 在 accept 複製（`server_runtime.rs:2117`） |
| inventory／HP／XP | `gameplay` | `data`（`PlayerData`） | 權威；snapshot／save overlay |
| interest、pose clock、teleport allowance、pending chunks | — | runtime only | 留下 |

散落複製點（掃描當日，執行前再 grep）：

- pose 接受：`server_runtime.rs` `accept_pose` 寫 `data`，另處再寫權威
- `teleport_session` `:1395`：先寫 `data` + `teleport_allowance`，再寫 `authority_session.position`
- gameplay accept `:2117`：複製 sequence + dimension
- respawn／transfer：同樣雙寫
- snapshot 投影：`apply_gameplay_to_player_data` 一帶
- **指令 teleport**：accept 後再 `commands::parse` 一次，若是 `Teleport` 才 `teleport_session`（`:2127–2137`）。
  `apply_command` 其實已經寫好 `session.position`。第二次 parse 是為了設 allowance。

本計劃不合併兩個型別（interest／存檔 codec 不能進確定性核心）。只收斂寫入點。

## 前置

09 已完成。可與 15、17 並行。不要跟 12 搶 `activate_dimension`。

## 精確 acceptance

- [ ] `ServerRuntime` 有一組私有 helper（名稱可微調，責任不可混）：
      - `sync_pose_from_authority(id)` 或 `write_pose(id, position, yaw, pitch)`：一次寫入權威 + `PlayerSessionState.data` + 需要時 `update_interest_for`
      - `sync_dimension(id, dimension)`：一次寫兩側
      - `sync_gameplay_projection(id)`：把權威 `SessionGameplayState` overlay 到 `PlayerData`（現有 `apply_gameplay_to_player_data` 可成為它的本體）
- [ ] pose 接受、`teleport_session`、dimension transfer、respawn、request accept 改走這些 helper。禁止再出現第三份 ad-hoc 欄位賦值。
- [ ] `teleport_session` 仍設 `teleport_allowance`，仍更新 interest。速度閘與 `WORLD_BOUND` 檢查不變。
- [ ] `GameplayOperation::Command` 在 Accepted 之後：**不要**再 `commands::parse` 字串。
      改為比較權威 pose 與 runtime `data.position`（或 `apply_command` 回傳的既有副作用），
      若 pose 變了就走 `teleport_session`／sync helper。非 Teleport 指令不得突然設 allowance。
- [ ] 不得把 `InterestSet` 搬進 `AuthorityCore`。不得合併 `PlayerData` 與 `SessionGameplayState` 的存檔 layout。
- [ ] 不得合併 `open_containers` 與 `ServerWorld.container_viewers`。
- [ ] join 仍用 `gameplay_from_player_data`；save 仍用 overlay。只是呼叫改走具名 helper。
- [ ] 既有測試期望值不變。

## 預計檔案與測試

- 修改：`src/server_runtime.rs`（必要時抽 `src/server_runtime/session_sync.rs` 子模組，公開方法留在 `ServerRuntime`）。
- 測試：
  - `cargo test --lib server_runtime::`
  - `cargo test --test review_hardening_session_lifecycle -- --test-threads=1`
  - `cargo test --test plan32_progression_travel -- --test-threads=1`
  - `cargo test --test runtime_topology_parity -- --test-threads=1`
  - `cargo test --test review_hardening_invariants -- --test-threads=1`

## 建議階段

1. 列出所有寫 `PlayerSessionState.data.position`／`session.dimension`／`last_client_sequence` 的生產點。
2. 抽 helper，先讓 `teleport_session` 與 accept 改走它。跑 session lifecycle。
3. 拿掉 Command 的第二次 `parse`。跑 Plan32（傳送／維度）與 invariants。
4. 其餘複製點收斂。

## 不在本計劃

- 合併兩個 session 型別。
- 把 interest 放到 core。
- 改 pose 速度閘、`MAX_POSE_SPEED`、teleport radius。
- 統一三套指令語言。
- 拆整個 `server_runtime.rs`（投影／ingress 子模組見 README §7）。
