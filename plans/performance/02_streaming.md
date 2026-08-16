Created At: 2026-07-28T05:34:08Z
Original Completion Claim At: 2026-07-28T13:49:00Z

# 任務 2：增量 prioritized Chunk queues

> 對應計畫：`14_performance_optimization.md` Phase 1.2
> 狀態：Partial
> 審核回退：見 [`15_performance_audit_repair_plan.md`](15_performance_audit_repair_plan.md)；直接 `mesh.mark_dirty()` 的 mutation 未必加入 scheduler，mesh correctness 待 R1 修復。
> 前置：任務 1（基線）
> 目標：消除 `State::update_chunks` 每幀重建及排序完整載入範圍的 O(render_distance²) 工作，改為增量優先級佇列與多重整合預算。
> Commit 訊息：`perf(streaming): add incremental prioritized chunk queues and budgets`

## 相關程式碼位置（已核對）

- `src/state.rs:4968` - `State::update_chunks`：目前每幀重建及排序完整載入範圍。
- `src/state.rs:794` - `ChunkMesh`：mesh revision 快取。
- `src/state.rs:861` - `MeshSnapshot`：worker request 的 halo 快照。
- `src/state.rs:952` - `ChunkMeshResult`：worker 回傳結果與 generation/lifetime/revision 驗證。
- `src/chunk_manager.rs:210` - `ChunkManager::set_block`：mutation 來源。
- `src/chunk_render.rs:303` - `DrawPlan`：draw 排序。
- `src/chunk_schedule.rs` - `ChunkStreamingScheduler` 與螺旋 offset 預計算。

## 已確認的基線風險

- `State::update_chunks` 每幀重新建立及排序完整載入範圍。視距 16 的方形範圍是 33×33，共 1,089 個 Chunk。
- 玩家停在同一 Chunk 時仍做 O(render_distance²) 掃描。

## 子任務清單

### 2.1 預計算近到遠螺旋 offset
- [ ] 檔案：`src/chunk_schedule.rs`
- 步驟：
  1. 依 render distance 預計算穩定的近到遠螺旋 offset 列表（相對玩家 Chunk）。
  2. 快取結果，只在 render distance 或維度變更時重算。
  3. offset 列表用於 load queue 的優先級排序。
- 驗收：玩家不改變位置時，不重算 offset。

### 2.2 只在跨越 Chunk／切換維度／視距變更時更新 target
- [ ] 檔案：`src/state.rs`
- 步驟：
  1. 記錄玩家上次所在的 Chunk 座標。
  2. 只有玩家跨越 Chunk 邊界、切換維度或更改視距時，才更新 load/unload target 集合。
  3. 停在同一 Chunk 時直接跳過 target 計算。
- 驗收：靜止時 `chunk_schedule` scope 接近零。

### 2.3 Load/dirty mesh queue 去重與優先級排序
- [ ] 檔案：`src/state.rs`
- 步驟：
  1. load queue 使用可去重 priority queue（`BTreeSet<(distance, cx, cz)>` 或 `BinaryHeap`）。
  2. dirty mesh queue 同樣去重並按距離排序。
  3. dirty mutation 直接 push affected Chunk/section，不再每幀掃描全部 mesh。
- 驗收：dirty queue 不含重複項；近距離 Chunk 優先處理。

### 2.4 Unload hysteresis
- [ ] 檔案：`src/state.rs`、`src/chunk_manager.rs`
- 步驟：
  1. 加入一圈保留 hysteresis（例如 render distance + 1 或 + 2），降低邊界往返造成的 unload/reload thrash。
  2. 實際圈數由記憶體基線決定（任務 1 基線）。
  3. hysteresis 圈內的 Chunk 不領域 unload。
- 驗收：快速飛行時邊界 Chunk 不反覆 unload/reload。

### 2.5 每幀多重整合預算
- [ ] 檔案：`src/state.rs`
- 步驟：
  1. 每幀按三重預算整合 worker result：CPU 時間（目標 2-4 ms）、結果數（如最多 4 個 mesh）、upload bytes。
  2. 任一預算用完即停止本幀整合，剩餘結果留到下一幀。
  3. 記錄 `terrain_result_integrate` scope 時間。
- 驗收：快速飛行時無單幀集中 upload；`terrain_result_integrate` p95 在預算內。

### 2.6 保留 stale-result 驗證
- [ ] 檔案：`src/state.rs`
- 步驟：
  1. result 保持 generation/lifetime/revision 驗證。
  2. 取消或丟棄過期工作（玩家已飛離、維度已切換）。
  3. 確認 `ChunkMeshResult` 的失效邏輯不變。
- 驗收：快速飛行時不提交過期 mesh；既有 stale-result 測試通過。

## 驗收條件

- [ ] 玩家停在同一 Chunk 時，streaming scheduler 不建立 O(render_distance²) 的臨時 Vec 或排序。
- [ ] 快速飛行時近距離 Chunk 優先完成，無單幀集中 upload。
- [ ] dirty mutation 只 push 受影響的 Chunk，不掃描全部 mesh。
- [ ] unload hysteresis 降低邊界 thrash。
- [ ] `chunk_schedule` 與 `terrain_result_integrate` p95 改善（與任務 1 基線比較）。
- [ ] `cargo fmt --all -- --check`、`cargo check --release`、`cargo test --release` 通過。

## 風險與回退

- priority queue 若引入複雜度而無明顯改善，回退到原本的排序但加入「玩家未跨 Chunk 則跳過」短路。
- hysteresis 圈數過大會增加記憶體；以基線 working set 為上限。
- 本任務不改變遊戲行為或存檔格式。

## 驗證命令

```text
cargo fmt --all -- --check
cargo check --release
cargo test --release
cargo run --release   # 固定場景 3（快速飛行）before/after 比較
```
