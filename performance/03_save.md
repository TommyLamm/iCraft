# 任務 3：真正的背景存檔

> 對應計畫：`14_performance_optimization.md` Phase 1.3
> 狀態：✅ 已完成
> Completed At: 2026-07-28T06:03:00Z
> 前置：任務 1（基線）
> 目標：將 Chunk 壓縮與 region I/O 從主線程移至背景 worker，autosave 只 snapshot dirty chunks，消除 autosave 期間的長幀。
> Commit 訊息：`perf(save): move chunk compression off-thread and batch region writes`

## 相關程式碼位置（已核對）

- `src/save.rs:475` - `SaveManager` 結構與 `DirtyChunkSet` / `UncompressedChunkSnapshot` / `atomic_write`。
- `src/save.rs:477` - `region_cache: HashMap` 與 LRU eviction (上限 16 條 / 64 MB)。
- `src/save.rs:480` - `compress_bytes`（Zlib 壓縮，在背景 worker 執行）。
- `src/save.rs:528` - `SaveManager::new`。
- `src/save.rs:653` - `save_chunk` / `save_chunks_batch_in`（按 region batch 寫入）。
- `src/state.rs:4548` - `trigger_background_save`（只 snapshot dirty chunks，不壓縮無檔案 I/O）。
- `src/menu.rs` - saving overlay。

## 已確認的基線風險

- `trigger_background_save` 在主線程將所有 Chunk 的五組 64 KiB voxel 陣列轉換並壓縮後才送入 save worker。視距 16 單次需處理約 357 MB 原始資料。
- `SaveManager::save_chunk_in` 每存一個 Chunk 都重新序列化並覆寫整個 region。全量 autosave 會對同一 region 進行大量重複寫入。

## 子任務清單

### 3.1 建立 DirtyChunkSet
- [x] 檔案：`src/save.rs`、`src/chunk_manager.rs`、`src/state.rs`
- 步驟：
  1. 建立 `DirtyChunkSet` 結構，記錄自上次 autosave 後被修改的 Chunk 座標。
  2. 所有權威 block/state/light/fluid/redstone mutation 統一標記 dirty。
  3. `ChunkManager::set_block` 及相關 mutation 路徑呼叫 `mark_dirty`。
- 驗收：dirty set 只包含實際修改過的 Chunk。

### 3.2 Autosave 只 snapshot dirty chunks
- [x] 檔案：`src/state.rs`、`src/save.rs`
- 步驟：
  1. `trigger_background_save` 改為只 snapshot dirty chunks，不再遍歷全部載入 Chunk。
  2. 完整 flush 只在明確 Save and Quit 時執行。
  3. snapshot 保持最小、無壓縮（只做 shallow copy 或 Arc 共享）。
- 驗收：autosave 的主線程工作量與 dirty chunk 數成正比，不與載入總數成正比。

### 3.3 壓縮與 region I/O 移入 save worker
- [x] 檔案：`src/save.rs`、`src/state.rs`
- 步驟：
  1. flatten、Bincode、Zlib 和 region I/O 全部移入專用 save worker thread。
  2. `SaveCommand` 改為可合併、bounded queue。
  3. 同一 Chunk 僅保留最新 revision（latest-revision-wins）。
- 驗收：主線程不做任何壓縮或檔案 I/O。

### 3.4 按 region batch 寫入
- [x] 檔案：`src/save.rs`
- 步驟：
  1. worker 按 region 分組，一個 batch 只序列化和寫入 region 一次。
  2. 同一 region 的多個 Chunk 在一次 region rewrite 中全部寫入。
- 驗收：多個同 region Chunk 的單次 autosave 只產生一次 region rewrite。

### 3.5 Atomic file replacement
- [x] 檔案：`src/save.rs`
- 步驟：
  1. 使用同目錄 temporary file + flush + atomic rename，避免中途中斷毀損存檔。
  2. 確認 Windows 上 rename 覆蓋已存在檔案的行為正確（`MoveFileEx` with `MOVEFILE_REPLACE_EXISTING`）。
- 驗收：寫入中斷不毀損既有存檔。

### 3.6 Region cache 上限與 LRU eviction
- [x] 檔案：`src/save.rs`
- 步驟：
  1. region cache（`save.rs:477`）加入 byte/entry 上限。
  2. 超過上限時 LRU eviction。
  3. 記錄 `region_cache_bytes` counter（已在任務 1 規劃）。
- 驗收：region cache 記憶體有上限，不隨遊戲時間無限增長。

### 3.7 關閉時 flush 與 saving overlay
- [x] 檔案：`src/state.rs`、`src/menu.rs`、`src/app.rs`
- 步驟：
  1. 關閉遊戲時等待 queued revisions flush 完成。
  2. 顯示現有 saving overlay 直到 flush 結束。
  3. "Save and Quit" 執行完整 flush。
- 驗收：關閉遊戲不遺失未 flush 的修改。

## 驗收條件

- [x] autosave 不在主線程壓縮 Chunk。
- [x] 多個同 region Chunk 的單次 autosave 只產生一次 region rewrite。
- [x] 快速修改同一 Chunk 時舊 revision 不覆蓋新 revision。
- [x] 現有存檔格式保持可讀；舊存檔可正常載入。
- [x] region cache 有 byte/entry 上限。
- [x] autosave 期間主線程 p95 改善（與任務 1 基線比較）。
- [x] `cargo fmt --all -- --check`、`cargo check --release`、`cargo test --release` 通過。

## 風險與回退

- atomic rename 在某些檔案系統行為不同；先在 Windows NTFS 驗證。
- LRU eviction 若過激會增加 region 重讀；上限由基線決定。
- Save 格式不變；若需版本化，必須具備向後讀取。

## 驗證命令

```text
cargo fmt --all -- --check
cargo check --release
cargo test --release
cargo run --release   # 固定場景 7（autosave）before/after 比較
```
