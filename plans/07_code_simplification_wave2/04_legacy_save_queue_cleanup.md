# Plan04 — 刪除 `save/legacy_queue` 遺留存檔佇列

## 定位

在存檔格式 v3 與 `SaveManager` 落地後，伺服器與單人遊戲的存檔均由 `ServerRuntime` / `SaveManager`（`src/save/region.rs`, `index.rs`, `player.rs`, `format.rs`）負責進行同步與定期自動存檔（fail-closed chunk restore）。

`src/save/legacy_queue.rs`（946 行）是早期為桌面客戶端編寫的多線程最新勝出存檔工人：
- 包含 `UncompressedChunkSnapshot`（在堆上分配 5 個 `[[[T; 16]; 256]; 16]` 陣列再線性扁平化）。
- 包含 `NetworkSnapshotWorker` 與 `SaveQueue`（獨立條件變數、後台線程）。
- 僅在 `legacy_owner` 特性或其自身單元測試中使用。

正如 `ARCHITECTURE.md` 第 307–311 行所述：
> `src/save/legacy_queue.rs` provides the leftover desktop bounded latest-wins save worker for LegacyOwner construction and unit tests. The active `ServerRuntime` authority performs its own autosave and synchronous shutdown flush through `SaveManager`. Embedded Singleplayer / listen-host presentation does not spawn that worker.

刪除此檔案可直接減少 946 行堆分配與多線程樣板代碼。

## 前置

無。可與 01–03、05–06 並行。

## 精確 acceptance

- [x] 徹底刪除 `src/save/legacy_queue.rs` 文件。
- [x] 在 `src/save/mod.rs`（或 `src/save.rs`）中移除 `pub mod legacy_queue;` 或 `mod legacy_queue;` 宣告。
- [x] 移除 `src/save/mod.rs` 對 `legacy_queue` 結構體的 re-export。
- [x] 若 `src/presentation/` 或 `src/state.rs` 中殘留 `LegacySaveWorker` 調用，確認其已在 feature 隔離下或直接移除。
- [x] `cargo check --all-targets` 通過。
- [x] 所有持久化相關測試（`tests/authority_persistence.rs`、`tests/review_hardening_chunk_restore.rs` 等）全部通過。

## 預計檔案與測試

- 刪除：
  - `src/save/legacy_queue.rs`
- 修改：
  - `src/save/mod.rs`
  - `src/save/tests.rs`
- 驗證測試：
  - `cargo test --lib save:: -- --test-threads=1`
  - `cargo test --test authority_persistence -- --test-threads=1`
  - `cargo test --test review_hardening_chunk_restore -- --test-threads=1`
  - `cargo check --all-targets`

## 建議階段

1. 檢查 `legacy_queue` 在全庫的引用點。
2. 刪除 `src/save/legacy_queue.rs`。
3. 清理 `src/save/mod.rs` 的模組宣告與導出。
4. 運行持久化整合測試驗證。

## 不在本計劃

- 修改 `src/save/region.rs` 的 region v3 存檔與解碼邏輯。
- 更改 `SaveManager` 的自動存檔週期（6,000 ticks）。

## 實作與證據

1. **檔案刪除與導出清理**：
   - 徹底刪除 `src/save/legacy_queue.rs`（移除 946 行包含 `SaveQueue`, `SaveCommand`, `NetworkSnapshotWorker`, `UncompressedChunkSnapshot` 等多線程與大內存快照樣板）。
   - 在 `src/save/mod.rs` 移除 `pub mod legacy_queue;` 及其導出的結構體與函式，並清理 `SaveManager` 中只為 legacy queue 測試服務的 `save_chunks_batch_in` 與 `inject_worker_panic_once` / `inject_serialization_failure_once`。
2. **測試清理與修復**：
   - 在 `src/save/tests.rs` 移除針對 `legacy_queue` 的佇列與快照測試，保持其餘存檔 roundtrip、LRU 快取、損壞回退、原子性覆寫與崩潰安全測試完全通過。
3. **驗證結果**：
   - `cargo test --lib save:: -- --test-threads=1`：44 passed; 0 failed; 2 ignored
   - `cargo test --test authority_persistence -- --test-threads=1`：4 passed; 0 failed
   - `cargo test --test review_hardening_chunk_restore -- --test-threads=1`：5 passed; 0 failed
   - `cargo check --all-targets`：0 errors

