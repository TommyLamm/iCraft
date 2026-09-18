# 10 — Embedded 本地視距同步到 runtime

狀態：已完成。基線：`83e751d`，2026-09-17。
前置：02；11 的必要前置。

## 定位與判定

state.rs:3684–3695 的 pause options 按鈕只改 PresentationChunks.view_distance；embedded_runtime.rs:53–54 僅建構時拷貝到 runtime properties。

server_runtime/projection.rs:923 update_interest、:979 update_interest_for_at 每次從全域 properties set_distances，所以單次修改 InterestSet 會被覆寫。原計劃不能直接刪桌面 worldgen 後假設視野仍能補齊。

## 明確方案

增加 embedded local session 的 view-distance override；runtime 的兩個 interest refresh 路徑共用 effective-distance 決策。Listen host 的 remote session 繼續使用 server 設定，simulation distance 保留既有建構策略，不因每次 render distance 改變而一起修改。Join 維持 server 投影上限，不在本包新增 wire 協議。

## 實作步驟

1. runtime 增加針對 local session 的 view-distance setter／override 欄位，放在已有 runtime session overlay，保留單一資料來源。
2. 兩個 refresh path 共用 effective distances；值相同則不 invalidate，值變更更新 interest anchor、初始投影及 reverse index。
3. pause options 經 EmbeddedRuntimeBridge 呼叫該入口；建構、dimension transfer／respawn 持續使用同一 override。
4. 保持 remote 玩家 coverage 與專用伺服器設定語意，更新 ARCHITECTURE 契約。
5. 不用直接改全域 properties 的捷徑，避免本地畫面設定改變所有 remote 玩家模擬範圍。

## 驗證與驗收

既有：embedded_chunk_projection_uses_arc_column_not_dense_stream、embedded_interest_fanout_is_private_dimension_safe_and_exactly_once、stationary_session_skips_chunk_interest_rebuild_across_ticks、chunk_interest_index_tracks_join_move_and_leave。

新增：靜止本地玩家 2→4 後最終投影包含 (center_x+4, center_z)；4→2 更新 coverage/reverse index；相同值不重建；remote coverage 不变；換維度後 override 有效。所有新增場景均是待實作測試，不是已驗證結果。

## 實作紀錄

- 狀態：已完成。
- 實際修改：
  1. `src/server_runtime/session_state.rs`: `PlayerSessionState` 增加 `pub view_distance_override: Option<u8>`，並實作 `effective_view_distance(&self, server_default: u8) -> u8`。
  2. `src/server_runtime/events.rs`: `LocalSessionProfile` 增加 `pub view_distance_override: Option<u8>` 與 builder 方法 `with_view_distance(mut self, view_distance: u8) -> Self`。
  3. `src/server_runtime.rs`: 建構時將 `profile.view_distance_override` 傳入 `handle_join_with_storage_and_override`。
  4. `src/server_runtime/ingress.rs`: 實作 `handle_join_with_storage_and_override`，在調用 `update_interest` 之前即初始化 `session.view_distance_override`。
  5. `src/server_runtime/projection.rs`: 實作 `effective_distances_for_session` 與 `effective_distances_for_id`，統一 `update_interest` 與 `update_interest_for_at` 的有效距離決策；remote session 維持伺服器預設值，simulation distance 保留既有策略。
  6. `src/server_runtime/session_sync.rs`: 新增 `set_session_view_distance` 與 `set_local_view_distance`，若與當前有效視距相同則跳過重建；變更時觸發 `update_interest_for`。
  7. `src/presentation/embedded_runtime.rs`: 在單人與 listen host 建立時透過 `.with_view_distance(...)` 將本地視距注入 profile，並實作 `EmbeddedRuntimeBridge::set_view_distance` 轉接給 `set_local_view_distance`。
  8. `src/state.rs`: pause options 視距調整按鈕點擊後經 `bridge.set_view_distance(self.chunk_manager.view_distance as u32)` 同步至 runtime。
  9. `src/server_runtime/tests.rs`: 新增 5 個確定性測試：
     - `embedded_local_view_distance_expansion_projects_outer_columns` (2→4 最終投影包含 (center_x+4, center_z))
     - `embedded_local_view_distance_shrink_updates_coverage_and_reverse_index` (4→2 更新 coverage 與 reverse index)
     - `local_view_distance_same_value_does_not_rebuild` (相同值不重建)
     - `local_view_distance_override_preserves_remote_coverage` (remote coverage 不變)
     - `local_view_distance_override_persists_across_dimension_transfer` (換維度後 override 持續有效)
  10. `ARCHITECTURE.md`: 更新 `PlayerSessionState` 契約與 `session_sync.rs` 視距同步說明。
- 驗證結果：
  - `cargo test --lib -- embedded_chunk_projection_uses_arc_column_not_dense_stream embedded_interest_fanout_is_private_dimension_safe_and_exactly_once stationary_session_skips_chunk_interest_rebuild_across_ticks chunk_interest_index_tracks_join_move_and_leave embedded_local_view_distance_expansion_projects_outer_columns embedded_local_view_distance_shrink_updates_coverage_and_reverse_index local_view_distance_same_value_does_not_rebuild local_view_distance_override_preserves_remote_coverage local_view_distance_override_persists_across_dimension_transfer`（9 passed，0 failed）。
  - `cargo check --all-targets` 通過，無編譯錯誤。

