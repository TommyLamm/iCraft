# 11 — 刪除桌面本地 worldgen 與載入排程

狀態：已完成。基線：`83e751d`，2026-09-17。
前置：05、10；08/20 可獨立完成。

## 定位與判定

活躍鏈：State::update_chunks:3544–3572 → schedule_chunk_load:3367 → rayon::spawn → dimension::generate_chunk_with_options → TerrainWorkerResult::Loaded → process_terrain_worker_results:3188。這包是刪除重複的活躍來源，不是直接死码掃除。

## 實作步驟

1. Embedded 地形只取 runtime ChunkColumn；Join 地形只取 ChunkData。先確認 10 已使本地 runtime 覆蓋最新 render distance。
2. 刪 schedule_chunk_load、ChunkLoadResult、Loaded variant、chunk_load_in_flight、MAX_CHUNK_LOAD_JOBS、load dispatch／pending queue／spiral 及其 metrics。
3. ChunkStreamingScheduler 的 last chunk/distance/dimension 仍供 unload／reprioritize 使用，收窄成必要錨點，不整包盲刪。
4. TerrainWorkerResult 若只剩 mesh，直接使用 SectionMeshResult；保留有效 mesh generation/lifetime/revision。
5. 刪 State world_type／generate_structures／world_seed 欄位及 bootstrap 冗餘傳遞；constructor 建立 weather/runtime 所需 seed 保留為局部值。
6. 刪 restore_failed（唯一 producer 固定 false）、PresentationChunkLoadPolicy／GenerateLocally／schedule_presentation_chunk_load；用真正 projection 測試代替舊 policy helper 測試。
7. 刪沒有正式入口的 Pickup policy 變體，保留 authority pickup 產品案例。
8. 不刪 pending_block_changes，它在 state.rs:4614 有真實 Join producer。
9. 更新 ARCHITECTURE，桌面不再生成任何 terrain column；兩條投影 commit 去重交給 21。

## 驗證與驗收

原 remote_sync_tests::terrain_worker_tokens_reject_stale_generation_lifetime_and_revision 刪 load 部分，保留 :503–533 section mesh assertions；補 unload/reload lifetime 變化及 current section absent。

驗證單機／listen host 初始載入、視距修改、快速移動、傳送、曾修改世界、join 初始投影。presentation 不先顯示另行生成的未修改版本；rapid unload/reload 後舊 mesh 結果仍被拒絕。

桌面正式 terrain 路徑不再呼叫 generate_chunk_with_options，並只剩一套 mesh worker。

## 實作紀錄

- 刪除桌面本機 worldgen 載入排程與通道：
  - `src/state.rs`: 移除 `MAX_CHUNK_LOAD_JOBS`、`ChunkLoadResult`、`TerrainWorkerResult`，改以 `SectionMeshResult` 作為 worker channel 通訊型別；刪除 `chunk_load_in_flight`、`world_seed`、`world_type`、`generate_structures` 欄位與 `schedule_chunk_load`；在 `update_chunks` 中移除 spiral 載入排程佇列、in-flight 清理與 load dispatch 迴圈；簡化 `process_terrain_worker_results` 為純 mesh 整合。
  - `src/chunk_schedule.rs`: 移除 `MAX_INTEGRATE_LOADS`、`MAX_INTEGRATE_LOAD_BYTES`、`precompute_spiral_offsets` 及未使用的 `distance_sq`；將 `ChunkStreamingScheduler` 收窄為僅包含 `last_player_chunk`、`last_render_distance`、`last_dimension`。
  - `src/presentation/frame.rs`: 更新 `perf_counters.in_flight` 僅計量 `section_scheduler.in_flight.len()`。
  - `src/presentation/bootstrap.rs`: 移除 `LaunchWorldState` 與 `load_launch_world_state` 中冗餘的 `world_type` 與 `generate_structures`。
  - `src/presentation/network_event.rs`: 移除 `LoginSuccess` 時對 `self.world_seed` 的多餘寫入，直接以封包 seed 初始化 `WeatherPresentation`。
  - 保留有真實 Join producer 的 `pending_block_changes`。
- 策略與測試清理：
  - `src/presentation_inventory_policy.rs`: 移除 `PresentationChunkLoadPolicy`、`schedule_presentation_chunk_load`、`chunk_load_policy()` 及死分支 `PresentationInventoryTarget::Pickup`。
  - `tests/review_hardening_embedded_presentation.rs`: 移除對 `PresentationInventoryTarget::Pickup` 的引用。
  - `src/menu/tests.rs`: 將測試重構為 `presentation_topology_role_resolution`，僅驗證角色至 topology 的解析。
  - `tests/review_hardening_join_projection.rs`: 以真正的投影測試驗證未接收權威封包前 join client chunks 保持未載入、收到 `ChunkData` 時正確寫入地形以及 embedded presentation 正確接收投影。
  - `src/presentation/tests/remote_sync_tests.rs`: 在 `terrain_worker_tokens_reject_stale_generation_lifetime_and_revision` 中移除 load token 斷言，保留 section mesh 斷言並補充 unload/reload lifetime 遞增與 section absent 驗證。
- 文件更新：
  - `ARCHITECTURE.md`: 明確說明 presentation 永不執行 worldgen 或 chunk loading，已移除 client spiral load queue 與 `PresentationChunkLoadPolicy`；說明 `Pickup` 策略變體已移除。
  - `plans/11_code_cleanup/README.md`: 工作包 11 狀態標記為「已完成」。
- 驗證命令及結果：
  - `cargo check --all-targets`：通過（無錯誤）。
  - `cargo test --test review_hardening_join_projection`：3 passed, 0 failed。
  - `cargo test --test review_hardening_embedded_presentation`：3 passed, 0 failed。
  - `cargo test --lib presentation_inventory_policy`：4 passed, 0 failed。
  - `cargo test --bin icraft remote_sync_tests`：23 passed, 0 failed。
  - `cargo test --bin icraft menu::tests::presentation_topology_role_resolution`：1 passed, 0 failed。
  - `cargo test --lib chunk_schedule`：3 passed, 0 failed。
- 變更統計：11 files changed, 150 insertions(+), 544 deletions(-)（淨刪除 394 行）。

