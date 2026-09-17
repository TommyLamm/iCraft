# 11 — 刪除桌面本地 worldgen 與載入排程

狀態：待執行。基線：`83e751d`，2026-09-17。
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

尚未執行；完成時記錄實際修改、驗證結果、刪碼量及文件更新。

