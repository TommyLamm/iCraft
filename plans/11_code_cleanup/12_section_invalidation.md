# 12 — Section mesh invalidation 單一來源

狀態：已完成。基線：`83e751d`，2026-09-17。
前置：05、21（已具備）。

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

- 改動細節：
  - `src/chunk_manager/presentation.rs`：刪除 `pending_mesh_invalidations` 欄位、初始化與 `acknowledge_mesh_invalidation`、`drain_mesh_invalidations` accessor；移除未使用的 `mark_block_mesh_dependencies` import；`record_mesh_invalidation` 僅維護 `pending_section_mesh_invalidations`，並依據維度 height (`min_section_y..max_section_y_exclusive`) 過濾無效垂直 section。
  - `src/state.rs`：`apply_synced_block_change` 簽名由 `Option<HashSet<(i32, i32)>>` 改為 `Option<HashSet<SectionKey>>`；刪除內部 `mark_block_mesh_dependencies` 呼叫；比對光源改走 state-aware `BlockType::light_emission_for(BlockState::decode(state))`，精確支援 RedstoneLamp / RedstoneTorch 等純 state 變更之光照增減；回傳 `drain_section_mesh_invalidations()`；`apply_remote_block_change` 只迭代受影響 section 呼叫 `invalidate_section_mesh(key, DependencyReason::Network)`；`invalidate_chunk_mesh` 移除 `acknowledge_mesh_invalidation`；`update_chunks` 移除冗餘的 `drain_mesh_invalidations()`。
  - `src/presentation/authority_projection.rs`：`project_authority_mutations` 聚合受影響的 `dirty_sections: HashSet<SectionKey>`，逐一以 `invalidate_section_mesh(key, DependencyReason::Block)` 排程，不再呼叫全欄 `invalidate_chunk_meshes`。
  - `src/presentation/tests/remote_sync_tests.rs`：更新 `remote_block_change_updates_light_and_boundary_mesh_dependencies` 斷言 section 集合；新增 5 個單元測試覆蓋：內部 Stone→Dirt 唯一排程 section=1、(15,15,15) 三軸邊界 8 個 sections 排程、世界 Y (-64/-1/0/319) 邊界與高度過濾、純 state RedstoneLamp / RedstoneTorch 光照觸發、同 frame 多次 mutation 自動合併為最新 revision identity。
  - `ARCHITECTURE.md`：補充 Presentation 端 block mutation 與 lighting 僅精確記 section mesh dependencies，全欄重建僅限欄位 commit 與 unload 邊界。
- 驗證命令與結果：
  - `cargo check --all-targets`：exit 0，無任何 error。
  - `cargo test --bin icraft remote_sync_tests`：28 passed; 0 failed; finished in 0.53s。
  - `cargo test --bin icraft authority_projection_tests`：6 passed; 0 failed; finished in 0.09s。
  - `cargo test --lib chunk_manager`：26 passed; 0 failed; finished in 0.39s。
- 淨碼量與保留原因：
  - Presentation 端的 2D 欄位 mesh invalidation 集合與對應 accessor 全部淨刪除。
  - `WorldColumns` 及 `lighting` 共享的 2D `dirty_chunks: HashSet<(i32, i32)>` 依存檔髒欄追蹤用途如實保留。


