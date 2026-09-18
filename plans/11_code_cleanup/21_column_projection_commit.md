# 21 — 整欄投影共用 commit 邊界

狀態：已完成。基線：`83e751d`，2026-09-17。
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

- 抽取統一 `commit_projected_chunk_column` 邊界於 `src/presentation/authority_projection.rs`，承接 Embedded `Arc<Chunk>` 與 Join 解碼後 candidate `Chunk`。
- Join `apply_remote_chunk_data`（`src/state.rs`）改為先以 `ChunkSaveData::restore_network_payload` 解碼至候選 `Chunk`，失敗立即 fail-closed 返回，不推進 `client_chunk_revisions` 也不覆寫現有欄位或 lifetime。
- 刪除丟棄錯誤 Result 的舊 `restore_chunk_payload` wrapper。
- 刪除硬編碼 Overworld 高度的死方法 `ChunkMesh::pending()`，全面改用維度感知之 `ChunkMesh::pending_for_dimension`（在 `Dimension` 上補足 `to_wire` 輔助函式）。
- 提交整欄時，主動以 `DependencyReason::Ao` 使所有已載入鄰欄（`surrounding_chunk_coords`）之 mesh section 失效，保證遮面與環境光遮蔽更新，不再依賴「剛好有光值變化」。
- 修復 `apply_remote_block_change` 與 `apply_remote_block_entity_delta` 在欄位未 resident 時提早推進 `client_chunk_revisions` 導致底欄被判定為過期而丟棄的潛在缺陷；pending block changes 維持暫存並於欄位 commit 後按 revision 升冪順序正確重播。
- 在 `src/presentation/tests/authority_projection_tests.rs` 中新增 5 項明列驗收測試，涵蓋 Embedded vs Join 等價性、解碼失敗 fail-closed、鄰欄 AO 失效、pending delta 重播與 stale column 拒絕、以及 section identity 代際/生命週期過期拒絕。
- 更新 `ARCHITECTURE.md` 投影提交邊界、錯誤語意與 AO 失效規範；更新 `plans/11_code_cleanup/README.md` 狀態為已完成。
- 驗證：
  - `cargo check --all-targets`（通過，code 0）
  - `cargo test --bin icraft -- authority_projection_tests`（通過，6 passed, 0 failed）
  - `cargo test --bin icraft -- remote_sync_tests`（通過，23 passed, 0 failed）
  - `cargo test --lib -- embedded_chunk_projection_uses_arc_column_not_dense_stream review_hardening_join_projection`（通過，1 passed, 0 failed）

