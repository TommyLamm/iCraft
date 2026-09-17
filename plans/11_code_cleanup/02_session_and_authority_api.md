# 02 — Session 單一定義與 authority API 清理

狀態：已完成。基線：`83e751d`，2026-09-17。
前置：無。

## 定位與判定

- `src/server_runtime.rs:123` 和 `src/server_runtime/session_state.rs:8` 重複 PlayerSessionState 欄位與 impl；後者未被建構。
- `src/authority/dispatch/mod.rs:61 preflight` 無 caller；正式 submit_request 使用 preflight_session／world.validate_request。
- 原總計劃需更正：`src/server_runtime/ingress.rs:763 session_revision` 有 runtime 產品測試 caller（tests.rs:367/429/552/633/641/673/681/689）。

## 實作步驟

1. 以目前實際使用的 root 定義核對內容，保留唯一實作於 session_state.rs；constructor 改 pub(super)，root 明確 re-export。
2. 刪 root 重複定義與 glob re-export；不要留下雙型別或 type alias 兼容層。
3. 刪無用 preflight 包裝；不改 submit_request 現有 bounds／cache／sequence／拒絕回應的先後順序。
4. session_revision 搬到 tests.rs 的測試用 impl 或 fixture 函式；保留有效產品 assertions。
5. 清拆檔後未用 imports；按 caller 收窄 CombatProfile／ConnectionWriter 相關 helper 可見性。
6. 修正 session 的 player_dirty 註解：現行 player 寫入是 save_all_inner 同步 save_player 成功後清 dirty，並非 PlayerFile worker ack。
7. current_revision 與 revision_for_dimension 都活躍；本包可統一同義入口，但須改所有 caller，不將它列為死碼。若只造成命名 churn 而無減少重複則不做。

## 驗證與驗收

`cargo test --lib server_runtime::tests`、`cargo test --lib authority::`。關注既有：
- runtime_pose_validation_rejects_regression_and_speed_but_allows_server_teleport
- dimension_interest_and_session_transfer_are_isolated
- duplicate_and_stale_revision_are_authoritative
- authenticated_rejections_are_cached_without_consuming_sequence

僅一份 PlayerSessionState；test-only 查詢 helper 不留在正式 API。實作後更新 ARCHITECTURE 的 source 定位。

## 實作紀錄

- 改動細項：
  1. `PlayerSessionState` 去重：保留唯一實作於 `src/server_runtime/session_state.rs`，`new` 改為 `pub(super)`，刪除 `src/server_runtime.rs` 中 148 行重複的 struct 與 impl 定義；`server_runtime.rs` 移除 glob re-export 改為具體 `pub use session_state::PlayerSessionState;`。
  2. 刪除 `src/authority/dispatch/mod.rs` 中無 caller 的 `pub(crate) fn preflight` 包裝函式。
  3. 將 `session_revision` 自正式 `src/server_runtime/ingress.rs` API 刪除，搬遷至 `src/server_runtime/tests.rs` 作為 test-only helper (`impl ServerRuntime { fn session_revision(...) }`)，既有測試 assertions 完整保留。
  4. 清理拆檔後未用 imports 與收窄可見性：
     - `src/authority/dispatch/mod.rs`：移除未使用的 `combat_logic`、`fishing`、`ServerWorld`、`MiningProgressState`、`position_to_milli`、`BlockActionKind` 等 imports。
     - `CombatProfile`、`combat_profile`、`look_from_angles` 移至唯一呼叫者 `src/authority/dispatch/combat.rs` 作為私有項目；`combat.rs` 改為直接調用 `contract::quantize_health` 並移除 `mod.rs` 的轉接函式。
     - `block_action.rs`、`container.rs`、`combat.rs` 補上明確匯入。
     - `src/network/transport.rs`：將 `ConnectionWriter::send_payload`、`ConnectionWriter::send` 與 `ConnectionReader::recv` 收窄為 `pub(super)`。
  5. 修正 `session_state.rs` 中的 `player_dirty` 註解，明確註記為 `save_all_inner` 同步 `save_player` 成功後清除。
  6. 更新 `ARCHITECTURE.md` 的 Code map / Ownership 定位說明，註明 `PlayerSessionState` 定義於 `src/server_runtime/session_state.rs` 並由 `ServerRuntime` re-export。
- 實際驗證命令與結果：
  - `cargo test --lib server_runtime::tests`：31 passed; 0 failed (exit 0)
  - `cargo test --lib authority::`：66 passed; 0 failed (exit 0)
  - `cargo check --all-targets --all-features`：exit 0
- 淨刪碼／保留原因：
  - 統計：9 files changed, 78 insertions(+), 240 deletions(-)，淨刪除 162 行程式碼。
  - 保留：`current_revision` 與 `revision_for_dimension` 在權威與 world 間均活躍使用，不作無謂改名 churn；測試 helper `session_revision` 移至測試模組以維持既有測試驗證深度。


