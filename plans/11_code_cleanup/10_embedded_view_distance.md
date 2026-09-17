# 10 — Embedded 本地視距同步到 runtime

狀態：待執行。基線：`83e751d`，2026-09-17。
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

尚未執行；完成時記錄實際修改、驗證結果、刪碼量及文件更新。

