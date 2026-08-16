# 任務 15-R1：統一 mesh dirty enqueue 與排程正確性

> 對應計畫：`15_performance_audit_repair_plan.md` 第 2.1 節
> 狀態：已完成（2026-07-29）
> 前置：R0（文件狀態回退）
> 目標：建立唯一 mesh invalidate API `invalidate_chunk_mesh(coord, dependency_reason)`，令所有 mutation 同時遞增 revision、enqueue 去重 priority work、套用 boundary/diagonal AO 依賴並將 stale connectivity 立即標為 invalid/fail-open，消除 mesh 永久 stale 的 correctness 缺陷。
> Commit 訊息：`fix(perf): unify mesh dirty enqueue with AO dependency and stale fail-open`

## 相關程式碼位置（已核對）

- `src/state.rs:5486-5491` - mutation 路徑只呼叫 `mesh.mark_dirty()` 未排程。
- `src/state.rs:5861-5888` - 同類直接 `mark_dirty` 路徑。
- `src/state.rs:1372-1375` - 直接 `mark_dirty` 呼叫點。
- `src/state.rs:6020-6038` - 直接 `mark_dirty` 呼叫點。
- `src/state.rs:7403-7406` - 直接 `mark_dirty` 呼叫點。
- `src/state.rs:7777-7780` - 直接 `mark_dirty` 呼叫點。
- `src/state.rs:4968` - `State::update_chunks` 只消費 `scheduler.dirty_chunk_meshes`。
- `src/state.rs:794` - `ChunkMesh` mesh revision 快取。
- `src/state.rs:861` - `MeshSnapshot` worker request halo 快照。
- `src/state.rs:952` - `ChunkMeshResult` worker 回傳結果與 generation/lifetime/revision 驗證。
- `src/chunk_schedule.rs` - `ChunkStreamingScheduler` 與 dirty mesh queue。
- `src/chunk_manager.rs` - `ChunkManager::set_block` 等 mutation 來源。

## 已確認的基線風險

- `State::mark_chunk_dirty` 同時更新 mesh revision 與 scheduler，但大量路徑直接呼叫 `mesh.mark_dirty()`，繞過排程。
- `update_chunks` 只消費 `scheduler.dirty_chunk_meshes`，因此繞過的 mutation 永遠不會重新排程 mesh，造成永久 stale。
- boundary 與 diagonal AO 依賴未在 dirty 時連帶標記鄰接 Chunk/section。
- stale connectivity graph 在 mesh dirty 時未立即 invalid，culling 可能誤用過期連通性。

## 子任務清單

### 1.1 建立唯一 invalidate_chunk_mesh API
- [x] 檔案：`src/state.rs`、`src/chunk_schedule.rs`
- 步驟：
  1. 新增 `fn invalidate_chunk_mesh(&mut self, coord: ChunkCoord, reason: DependencyReason)`，取代散落的 `mesh.mark_dirty()` 呼叫。
  2. API 內部依序：遞增 `ChunkMesh` revision（`src/state.rs:794`）、以 `(distance, cx, cz, reason)` enqueue 去重 priority work、回傳是否新增工作。
  3. 定義 `DependencyReason` 列舉（`Block`/`Fluid`/`Light`/`Weather`/`Redstone`/`Network`/`BreakPlace`/`Mob`），供觀測與測試區分來源。
  4. 把 `src/state.rs:1372-1375, 6020-6038, 7403-7406, 7777-7780, 5486-5491, 5861-5888` 全部改呼叫 `invalidate_chunk_mesh`。
- 驗收：grep 確認 runtime 路徑不再直接呼叫 `ChunkMesh::mark_dirty`；每個 mutation 來源對應一個 `DependencyReason`。

### 1.2 套用 boundary/diagonal AO dependency
- [x] 檔案：`src/state.rs`、`src/chunk_schedule.rs`
- 步驟：
  1. 在 `invalidate_chunk_mesh` 內，依受影響 section 計算 18³ halo 跨越的鄰接 Chunk/section。
  2. 對每個邊界/對角鄰接 section 一併呼叫 `invalidate_chunk_mesh`（帶 `reason=AO` 衍生），確保 AO 重建。
  3. halo 範圍與 `MeshSnapshot`（`src/state.rs:861`）的 halo 取樣一致。
  4. 去重邏輯保證同 section 不會因多重 dependency 重複入隊。
- 驗收：在 Chunk 邊界 break/place 後，鄰接 Chunk 的 section mesh 亦重新排程。

### 1.3 stale connectivity 立即 invalid/fail-open
- [x] 檔案：`src/state.rs`、`src/culling.rs`
- 步驟：
  1. `invalidate_chunk_mesh` 執行時，同步把對應 section 的 connectivity graph 標為 `Invalid`。
  2. 在新 revision graph 回來前，culling 對該 section 視為全可見（fail-open）。
  3. 確保 `ChunkMeshResult`（`src/state.rs:952`）回傳後才重建 connectivity，避免半更新狀態。
  4. stale connectivity 不可寫入 cache，避免汙染後續查詢。
- 驗收：mesh dirty 到新 graph 完成期間，該 section 不被錯誤 cull。

### 1.4 禁止 runtime 直接呼叫 mark_dirty
- [x] 檔案：`src/state.rs`、`src/chunk_manager.rs`
- 步驟：
  1. 把 `ChunkMesh::mark_dirty` 改為 `pub(crate)` 或加 `#[doc(hidden)]`，僅 `invalidate_chunk_mesh` 與 worker result 路徑可用。
  2. 新增測試或 lint：掃描 `src/` 內 `mark_dirty` 呼叫點，僅允許出現在 `invalidate_chunk_mesh` 與 `ChunkMeshResult` 整合路徑。
  3. `ChunkManager::set_block`（`src/chunk_manager.rs:210`）改走 `invalidate_chunk_mesh`。
  4. fluid、lighting、weather、redstone、network、mob/boss mutation producers 全數改接 API。
- 驗收：除白名單外無直接 `mark_dirty` 呼叫；新增 mutation 來源無法繞過排程。

### 1.5 持久化 priority dirty queue
- [x] 檔案：`src/chunk_schedule.rs`
- 步驟：
  1. 把 `dirty_chunk_meshes` 改為持久 priority queue（`BTreeSet<(distance, cx, cz, reason)>` 或 `BinaryHeap`），不每幀重建。
  2. enqueue 時去重：同 section 已存在則僅更新 reason/revision，不重複入隊。
  3. `update_chunks`（`src/state.rs:4968`）消費後從 queue 移除，保留未完成項跨幀。
  4. queue 上限由記憶體基線決定，超限時丟棄最低優先（最遠）項並記錄 drop counter。
- 驗收：玩家停在同一 Chunk 時 dirty queue 不重建臨時 Vec；快速 mutation 不造成重複工作。

### 1.6 mutation -> scheduler -> worker -> visible mesh 整合測試
- [x] 檔案：`src/state.rs`（測試模組）
- 步驟：
  1. 撰寫整合測試：place/break/fluid/light/weather/redstone/remote block change 各觸發一次，斷言 dirty queue 各入隊一次。
  2. 驅動 worker 產生 `ChunkMeshResult`，斷言 visible mesh revision 等於最新 mutation revision。
  3. 邊界與 diagonal AO 場景：斷言鄰接 section 亦被排程。
  4. 舊 revision worker 結果必須被丟棄，不得覆寫新 revision。
- 驗收：「mutation -> scheduler queued -> worker result -> visible mesh revision」鏈路全部斷言通過。

## 驗收條件

- [x] place、break、fluid、light、weather、redstone、remote block change、Chunk boundary 與 diagonal AO mutation 都會排入一次工作。
- [x] 玩家停在原 Chunk 時也會整合新 revision；舊 revision 結果必須丟棄。
- [x] runtime 無直接 `ChunkMesh::mark_dirty` 呼叫（白名單除外）。
- [x] stale connectivity 在 mesh dirty 時立即 invalid 並 fail-open。
- [x] dirty queue 為持久 priority queue，不每幀重建排序完整 Vec。
- [x] mutation -> scheduler -> worker -> visible mesh 整合測試通過。

## 風險與回退

- 收斂 `mark_dirty` 為 crate-private 可能影響測試直接建立 dirty 狀態；改以 `invalidate_chunk_mesh` 測試輔助函式取代。
- AO halo 連帶 dirty 會放大工作量；去重與 priority 上限可控制，必要時只 dirty 受影響 section 而非整個 Chunk。
- 若 priority queue 引入複雜度無明顯改善，可回退為排序 Vec 但保留「跨 Chunk 不重掃」短路。

## 驗證命令

```text
cargo fmt --all -- --check
cargo test --all-targets
cargo test --all-targets --release
cargo build --release
cargo clippy --all-targets --all-features
cargo test --release -- mesh_dirty invalidate_chunk_mesh ao_dependency   # R1 整合與 fault-injection 測試
```

## 完成記錄（2026-07-29）

- `State::invalidate_chunk_mesh` 為唯一 runtime mesh 失效入口；hostile/passive mob 模組改回傳 dirty coordinates，由 `State` 統一排程。
- `ChunkManager` 的 block/state/light/fluid 寫入會保守記錄 AO-aware invalidation；已知 producer 在統一入口確認記錄，未來漏接的新 mutation 則會在下一次 chunk 排程前由安全網補入。
- `ChunkStreamingScheduler` 使用持久 `BTreeSet` priority index + `HashMap` 去重表，保存最新 reason/revision，玩家跨 Chunk 時才重算距離，並以 16,384 項上限與 drop counter 約束記憶體。
- `SectionConnectivityState::Invalid` 在 revision 遞增時立即套用；visibility traversal 在 matching worker result 整合前以 `FULL` fail-open。
- 現有 mesh 粒度為整個 Chunk，因此 18³ halo 跨界時會重建受影響鄰接 Chunk 的全部 sections；這比 section-only invalidation 保守，但與 `MeshSnapshot` halo 一致且不會漏掉 diagonal AO。
- 新增 scheduler 去重/優先序、connectivity fail-open、boundary/diagonal AO、stale revision rejection 與 runtime bypass guard 測試。

驗證結果：

- `cargo fmt --all -- --check`：通過。
- `cargo test --all-targets`：380 個 unit tests + 1 個 integration placeholder 通過。
- `cargo test --all-targets --release`：380 個 unit tests + 1 個 integration placeholder 通過。
- `cargo build --release`、`cargo check --release`：通過。
- `cargo clippy --all-targets --all-features`：被既有 5 個 lint error 阻擋（`src/network/transport.rs:58` 與 `src/texture.rs:1235,1237,1513,1739`），R1 修改範圍無新增 clippy error。
