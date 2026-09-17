# 02 — Session 單一定義與 authority API 清理

狀態：待執行。基線：`83e751d`，2026-09-17。
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

尚未執行；完成時記錄實際修改、驗證結果、刪碼量及文件更新。

