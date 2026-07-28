# 任務 6：紅石 dirty worklist 與 sleeping

> 對應計畫：`14_performance_optimization.md` Phase 2.2
> 狀態：✅ 已完成
> 前置：任務 1（基線）、任務 5（固定 tick）
> 目標：取代每 tick 完整 component HashMap clone 與最多 64 輪 settle，改為 dirty worklist 增量傳播與無工作時 sleep。
> Commit 訊息：`perf(redstone): replace full-map settling with dirty propagation`

## 相關程式碼位置（已核對）

- `src/redstone.rs:261` - `RedstoneSystem` 結構。
- `src/redstone.rs:620` - `settle_power`：每 tick 最多 64 輪，每輪 clone 完整 component HashMap。
- `src/state.rs:6512` - `apply_redstone_update`：套用紅石變更。
- `src/chunk_manager.rs:210` - `ChunkManager::set_block`：block mutation 來源。

## 已確認的基線風險

- `RedstoneSystem::settle_power` 在每個 20 Hz tick 最多執行 64 輪，每輪 clone 完整 component `HashMap`，即使沒有實際紅石變更也會進入 settle。

## 子任務清單

### 6.1 Dirty worklist
- [x] 檔案：`src/redstone.rs`
- 步驟：
  1. block mutation、scheduled tick、pressure-plate occupant change 將受影響節點加入去重 worklist。
  2. worklist 使用 `HashSet` 或 `BTreeSet` 去重。
  3. 只重新計算 changed node 及其鄰接元件。
- 驗收：靜止紅石世界的 `settle_power` 工作量接近零。

### 6.2 移除完整 component HashMap clone
- [x] 檔案：`src/redstone.rs`
- 步驟：
  1. 不再每輪 clone 完整 component map。
  2. 只讀取/寫入 worklist 中的節點及其鄰接。
  3. 使用 in-place 更新 + dirty 標記。
- 驗收：`redstone` scope p95 下降（與基線比較）。

### 6.3 Sleeping 機制
- [x] 檔案：`src/redstone.rs`、`src/state.rs`
- 步驟：
  1. 無 dirty node、無 scheduled tick、無 active fuse/device 時進入 sleep。
  2. sleep 狀態下跳過 `settle_power` 與 component graph 遍歷。
  3. 任何 mutation/scheduled tick/pressure plate change 喚醒。
- 驗收：無紅石活動時 `redstone` scope 接近零。

### 6.4 Chunk load 直接回傳 component metadata
- [x] 檔案：`src/redstone.rs`、`src/chunk_manager.rs`、`src/dimension.rs`
- 步驟：
  1. Chunk load worker 直接回傳 component metadata/index。
  2. 避免首次 20 Hz tick 掃描完整 Chunk。
  3. 確認 `redstone_metadata` sidecar 已支援此回傳。
- 驗收：新載入 Chunk 不在第一個 tick 做完整掃描。

### 6.5 Loop/overflow protection
- [x] 檔案：`src/redstone.rs`
- 步驟：
  1. 保留 loop/overflow protection。
  2. 計數改為每次事件的 node budget，而非每輪完整 map。
  3. 超過 budget 時記錄並停止傳播（與目前行為一致）。
- 驗收：大型迴路不無限傳播。

### 6.6 跨 Chunk 與大型線路 parity tests
- [x] 檔案：`src/redstone.rs`（測試區段）
- 步驟：
  1. 建立跨 Chunk 邊界的大型紅石線路測試。
  2. 新舊實作 differential tests：相同初始狀態 + 相同 mutation 序列，最終 power 狀態一致。
  3. 加入 loop budget parity tests。
- 驗收：新舊實作結果完全一致。

## 驗收條件

- [x] 靜止紅石世界的 `settle_power` 工作量接近零。
- [x] 無紅石活動時 `redstone` scope 接近零。
- [x] 現有紅石單元測試及新舊實作 differential tests 結果一致。
- [x] 跨 Chunk 邊界 propagation 正確。
- [x] loop/overflow protection 保留。
- [x] `redstone` scope p95 改善（與基線比較）。
- [x] `cargo fmt --all -- --check`、`cargo check --release`、`cargo test --release` 通過。

## 風險與回退

- 紅石行為 parity 是最高優先；differential tests 必須全綠才合併。
- worklist 實作若無改善，回退到原 settle 但加入「無 dirty 時短路」。
- 不改變紅石語義或存檔格式。

## 驗證命令

```text
cargo fmt --all -- --check
cargo check --release
cargo test --release
cargo run --release   # 固定場景 4（高密度紅石）before/after 比較
```
