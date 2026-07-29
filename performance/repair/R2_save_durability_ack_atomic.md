# 任務 15-R2：save durability、ACK 與 atomic replacement

> 對應計畫：`15_performance_audit_repair_plan.md` 第 2.2 節
> 狀態：待修復
> 前置：R0（文件狀態回退）
> 目標：把無界 save queue 改為 bounded latest-wins、建立 `dirty/in_flight/persisted_revision` 狀態機、`Flush` 回傳 `Result`、Windows 以 `ReplaceFileW`/`MoveFileExW(REPLACE_EXISTING|WRITE_THROUGH)` 原子替換、corruption 停止覆寫並回報，並修正 queue depth counter 與 phantom LRU key。
> Commit 訊息：`fix(perf): bounded save queue with revision ACK state machine and atomic replace`

## 相關程式碼位置（已核對）

- `src/state.rs:3231-3330` - `trigger_background_save` 與 save command 派發路徑。
- `src/state.rs:5281-5295` - save enqueue 與 dirty set drain。
- `src/state.rs:5784-5799` - `Flush` 處理與 ACK。
- `src/save.rs:509-530` - `SaveManager` 結構、`SaveCommand`、`atomic_write`。
- `src/save.rs:650-690` - `save_chunk` / `save_chunks_batch_in` region 寫入。
- `src/save.rs:475` - `SaveManager` 結構與 `DirtyChunkSet` / `UncompressedChunkSnapshot`。
- `src/save.rs:477` - `region_cache: HashMap` 與 LRU eviction。
- `src/save.rs:528` - `SaveManager::new`。
- `src/save.rs:653` - `save_chunk` / `save_chunks_batch_in`。

## 已確認的基線風險

- `std::sync::mpsc::channel` 無界，F3 counter 不含 channel backlog。
- `SaveCommand`/snapshot 沒有 revision；dirty set 在 enqueue 前已 drain，send/I/O error 又被忽略。
- `Flush` 即使寫入失敗仍 ACK success。
- `atomic_write` 在 Windows 先刪舊檔再 rename，有 crash/data-loss window。
- 讀既有 region 失敗時可建立空 region 再覆寫，風險是丟失同 region 其他 Chunk。

## 子任務清單

### 2.1 bounded latest-wins queue
- [ ] 檔案：`src/save.rs`、`src/state.rs`
- 步驟：
  1. 把 `std::sync::mpsc::channel`（`src/save.rs:509-530`）改為 bounded `crossbeam`/`mpsc::sync_channel(capacity)`。
  2. 每項 payload 攜帶 `(dimension, chunk, revision)`；enqueue 前以 latest-wins 合併同 Chunk 較舊 revision。
  3. 滿載時不 silent drop，改為阻塞或丟棄最舊同 Chunk revision 並記錄 `save_queue_drop` counter。
  4. `trigger_background_save`（`src/state.rs:3231-3330`）提交前不再 drain 全部 dirty，只提交 dirty 對應 latest revision。
- 驗收：快速重複修改同一 Chunk 時，queue 內只保留最高 revision。

### 2.2 dirty/in_flight/persisted_revision 狀態機
- [ ] 檔案：`src/save.rs`、`src/state.rs`
- 步驟：
  1. 為每個 Chunk 維護 `enum SaveState { Dirty(rev), InFlight(rev), Persisted(rev) }`。
  2. enqueue 成功後由 `Dirty(rev)` 轉 `InFlight(rev)`，保留 in-flight 不清除。
  3. worker 成功持久化並 ACK 才轉 `Persisted(rev)`；新 mutation revision 不得被舊 ACK 清除。
  4. enqueue、serialize、open、read、deserialize、write 或 replace 任一失敗都要回到 `Dirty(rev)` 或保留 dirty 並 requeue。
  5. `src/state.rs:5281-5295` 的 dirty set drain 改為「標記 InFlight 但保留 revision 比較」。
- 驗收：fault injection 期間任何步驟失敗都不會遺失未持久化 mutation。

### 2.3 Flush 回傳 Result
- [ ] 檔案：`src/save.rs`、`src/state.rs`、`src/menu.rs`、`src/app.rs`
- 步驟：
  1. `Flush`（`src/state.rs:5784-5799`）改回傳 `Result<(), SaveError>`。
  2. worker 回報失敗時 `Flush` 傳播錯誤，UI/quit path 顯示失敗訊息而非假成功。
  3. 「Save and Quit」在 `Flush` 失敗時允許使用者重試或放棄，不可直接退出。
  4. `saving` overlay（`src/menu.rs`）顯示實際失敗原因。
- 驗收：注入 I/O 失敗後 UI 顯示錯誤，不偽裝成功退出。

### 2.4 Windows 原子替換
- [ ] 檔案：`src/save.rs`、平台 helper（`#[cfg(windows)]`）
- 步驟：
  1. `atomic_write`（`src/save.rs:509-530`）在 Windows 改用 `ReplaceFileW` 或 `MoveFileExW` 搭配 `MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH`。
  2. temp 檔寫入同目錄、名稱唯一（含 PID/UUID/revision），先 `flush`+`fsync` 再替換。
  3. 移除「先刪舊檔再 rename」的順序，消除 crash window。
  4. 非 Windows 平台沿用 `rename`（POSIX 原子）。
- 驗收：替換過程 crash 後重啟只可讀到完整舊版或完整新版。

### 2.5 corruption 停止覆寫並回報
- [ ] 檔案：`src/save.rs`
- 步驟：
  1. region file 存在但讀取/反序列化失敗時，停止覆寫並回報 `SaveError::RegionCorruption`。
  2. 不可 fallback 成空 region 再覆寫，避免丟失同 region 其他 Chunk。
  3. 記錄 corruption 的 region 路徑與 Chunk 座標，供 UI 顯示。
  4. 提供唯讀 salvage 路徑：把可讀 Chunk 抽離到新 region，需使用者確認。
- 驗收：注入 region 損毀後不再覆寫，UI 回報且同 region 其他 Chunk 不遺失。

### 2.6 queue depth counter 與 phantom LRU key
- [ ] 檔案：`src/save.rs`、`src/state.rs`
- 步驟：
  1. F3 save queue depth 同時含 producer backlog、worker pending 與 in-flight bytes。
  2. 修正 `region_cache`（`src/save.rs:477`）phantom LRU key 累積：eviction 時真正移除條目，殘留 key 不計入 depth。
  3. 暫停 worker 並持續 mutation/unload 時，counter 與真實 backlog 一致。
  4. 加入 `save_queue_bytes`、`save_in_flight`、`save_drop` counter。
- 驗收：暫停 worker 時記憶體有上限，counter 與真實 backlog 一致。

### 2.7 fault-injection 測試
- [ ] 檔案：`src/save.rs`（測試模組）
- 步驟：
  1. 覆蓋 enqueue failure、worker panic、serialize/I/O failure、replace 前/中/後 crash。
  2. 重啟後斷言只可讀到完整舊版或完整新版，不缺檔、不部分檔、不遺失同 region 其他 Chunk。
  3. 快速重複修改同一 Chunk 斷言只允許最高 revision 成為 persisted。
- 驗收：全部 fault-injection 場景通過。

## 驗收條件

- [ ] queue 為 bounded latest-wins，快速重複修改同一 Chunk 只保留最高 revision。
- [ ] dirty/in_flight/persisted_revision 狀態機正確，任一步驟失敗都 requeue/保留 dirty。
- [ ] `Flush` 回傳 `Result`，UI/quit 顯示失敗而非假成功。
- [ ] Windows 以 `ReplaceFileW`/`MoveFileExW(REPLACE_EXISTING|WRITE_THROUGH)` 原子替換。
- [ ] region corruption 停止覆寫並回報，不 fallback 空 region。
- [ ] queue depth counter 含 producer/worker/in-flight，phantom LRU key 修正。
- [ ] fault-injection 全場景通過。

## 風險與回退

- bounded queue 在極高 mutation 速率可能丟棄最舊同 Chunk revision；以 latest-wins 為原則，丟棄計入 counter 可觀測。
- Windows `ReplaceFileW` 需目標存在；首次建立 region 時 fallback 到 `MoveFileExW`，並以測試覆蓋兩路徑。
- 狀態機若引入死結（永遠 InFlight），以 worker timeout + requeue 機制回收。

## 驗證命令

```text
cargo fmt --all -- --check
cargo test --all-targets
cargo test --all-targets --release
cargo build --release
cargo clippy --all-targets --all-features
cargo test --release -- save fault_injection atomic_replace corruption   # R2 fault-injection 測試
```
