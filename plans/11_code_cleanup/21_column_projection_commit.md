# 21 — 整欄投影共用 commit 邊界

狀態：待執行。基線：`83e751d`，2026-09-17。
前置：11；12 完整改造在本包後。

## 定位與判定

現有三份安裝欄位流程：state.rs:3229 Loaded、:4708 Join、presentation/authority_projection.rs:101 Embedded；11 刪第一份，餘下兩份仍重複 insert/lifetime/mesh/invalidate。

本次 source 核對：
- state.rs:4707 在新欄 decode 成功之前已推進 client_chunk_revisions。
- replacement 的 restore_chunk_payload（:4788）丟棄 restore_network_payload 的 Result。
- 兩條活躍入口用 ChunkMesh::pending() 的 Overworld 預設；gpu_terrain.rs:515 已有 pending_for_dimension。
- Embedded 只 invalidates 自欄，Join 多靠 lighting dirty 間接處理鄰欄；AO／遮面不能依賴「剛好有光值變化」。

## 實作步驟

1. Join 將 payload 完整 decode 到候選 Chunk，Embedded 取得 Arc<Chunk> 對應資料；来源處理與 commit 分開。
2. 抽唯一 decoded-column commit 邊界，輸入 dimension/coord/revision/Chunk，統一 revision gate、替換、lifetime、mesh 配置、invalidation。
3. decode 失敗時不修改現有 column／revision／lifetime；新欄與 replacement 一律同規則。
4. 使用 pending_for_dimension，按實際 dimension 配置 section 範圍。
5. 整欄插入／替換明確通知已載入鄰欄的 face/AO halo，即使 light 值沒有改變。按實際依賴選鄰欄，不對全世界 invalidates。
6. 保留 Join 欠缺 light stream 的重建需求、Embedded 已有 authority lighting；不為了共用 commit 把來源差異抹成第二次無用 lighting。
7. 維持 pending newer block delta 的 replay 次序、相同 revision 接收規則，刪舊 restore wrapper／重複 lifecycle 代码。
8. 更新 ARCHITECTURE 的 projection commit 與錯誤語意。

## 驗證與驗收

沿用 embedded_chunk_projection_uses_arc_column_not_dense_stream 和 remote_sync／authority_projection 相關測試。新增：
- 相同欄位經 Embedded／Join 得相同 block/state/fluid，且 section 範圍符合 dimension。
- invalid 新欄／replacement 都不推 revision；replacement 保留原 column。
- 新鄰欄在光值不變時仍更新 face／AO。
- pending newer delta 在完整欄位提交後正確重播；stale column 不回退狀態。
- unload/reload、維度切換的舊 mesh identity 被拒絕。

這包含明列的資料提交正確性修正，不能只用刪行數驗收。

## 實作紀錄

尚未執行；完成時記錄實際修改、驗證結果、刪碼量及文件更新。

