# 12 — Section mesh invalidation 單一來源

狀態：待執行。基線：`83e751d`，2026-09-17。
前置：05；完整改造接在 21 後。

## 已核對資料流

apply_synced_block_change（state.rs:235）→ apply_presentation_cell → record_mesh_invalidation（presentation.rs:119）同時寫 whole-column 與 section 集合；lighting 又產生 dirty_chunks；caller invalidate_chunk_meshes（state.rs:4630／authority_projection.rs:408）重建全高度；update_chunks 最後把 whole-column 集合 drain 丟棄。

光照 note_light_cell_change／set-light 已能記 section dirty。因此純 block mutation 無須重建整欄全部 section。

## 實作步驟

1. 刪 presentation 只寫再 drain 丟棄的 whole-column 集合及 accessor。
2. apply_synced_block_change 只記實際 cell 所影響的 section dependency，caller 不再把它擴成整欄。
3. lighting 記錄實際變動 section；共享 lighting 的 authority dirty APIs 按存檔用途保留。
4. 整欄插入／替換／unload 由 21 的 commit 邊界明確 invalidates halo；局部 block change 不重複走整欄流程。
5. 同 frame 同 section 多次變更合併成最新 revision identity。
6. 核對並修正 state.rs:255/280 用靜態 properties.light_emission 比較的路徑：同 BlockType 的 lit-bit 變化應以 light_emission_for(old/new BlockState) 判斷。這是待回歸驗證的行為修正，單獨記錄，不冒稱刪碼。

## 驗證與驗收

既有：mesh_invalidation_queues_latest_revision_and_invalidates_connectivity、mutation_scheduler_worker_chain_commits_only_the_latest_visible_revision、boundary_and_diagonal_ao_dependencies_queue_once。remote_block_change_updates_light_and_boundary_mesh_dependencies 改驗 section 集合。

新增／調整：
- 不改 opacity/emission 的內部 Stone→Dirt，唯一排程 section 為 1。
- 三軸邊界 (15,15,15) 的 AO dependencies 為 8 sections。
- 世界 Y -64/-1/0/319、斜鄰 AO、load/unload halo、state-only lamp/torch 開關。
- 多次 mutation 只保留最新 identity。

上述數字是預期「唯一排程 section 數」，不是已量測 worker job 數。驗收不得靠漏掉 halo 或光照更新減少工作量。

## 實作紀錄

尚未執行；完成時記錄實際修改、驗證結果、刪碼量及文件更新。

