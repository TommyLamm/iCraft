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

- [ ] 徹底刪除 `src/save/legacy_queue.rs` 文件。
- [ ] 在 `src/save/mod.rs`（或 `src/save.rs`）中移除 `pub mod legacy_queue;` 或 `mod legacy_queue;` 宣告。
- [ ] 移除 `src/save/mod.rs` 對 `legacy_queue` 結構體的 re-export。
- [ ] 若 `src/presentation/` 或 `src/state.rs` 中殘留 `LegacySaveWorker` 調用，確認其已在 feature 隔離下或直接移除。
- [ ] `cargo check --all-targets` 通過。
- [ ] 所有持久化相關測試（`tests/review_hardening_persistence.rs`、`tests/plan16_save_load.rs` 等）全部通過。

## 預計檔案與測試

- 刪除：
  - `src/save/legacy_queue.rs`
- 修改：
  - `src/save/mod.rs`
- 驗證測試：
  - `cargo test --lib save::`
  - `cargo test --test review_hardening_persistence -- --test-threads=1`
  - `cargo check --all-targets`

## 建議階段

1. 檢查 `legacy_queue` 在全庫的引用點。
2. 刪除 `src/save/legacy_queue.rs`。
3. 清理 `src/save/mod.rs` 的模組宣告與導出。
4. 運行持久化整合測試驗證。

## 不在本計劃

- 修改 `src/save/region.rs` 的 region v3 存檔與解碼邏輯。
- 更改 `SaveManager` 的自動存檔週期（6,000 ticks）。
